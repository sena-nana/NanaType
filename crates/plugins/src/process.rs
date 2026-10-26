//! Asynchronous transport: the UI thread never waits for Lua or IPC.
use crate::{
    DataReply, DataRequest, MAX_DATA_ROUNDS, WorkerRequest, WorkerResponse, bound_replies,
    read_message, write_message,
};
use std::{
    collections::VecDeque,
    io::{Read, Write},
    path::Path,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    time::{Duration, Instant},
};

/// At most one view update per plugin reaches the UI in this interval (~30/s).
const POST_INTERVAL: Duration = Duration::from_millis(33);
/// Each this many draw commands add another interval before the next post:
/// a canvas at the protocol budget repaints about six times a second, because
/// the host paints it on the UI thread.
const PAINT_COST_STEP: usize = 512;

fn pace(posted: &WorkerResponse) -> Duration {
    match posted {
        WorkerResponse::Update { update, .. } => {
            POST_INTERVAL * (1 + update.paint_cost() / PAINT_COST_STEP) as u32
        }
        _ => POST_INTERVAL,
    }
}

/// Answers a script's data request on the supervisor thread, never the UI thread.
pub type DataAnswer = Arc<dyn Fn(&DataRequest) -> DataReply + Send + Sync>;

pub struct PluginProcess {
    sender: mpsc::SyncSender<WorkerRequest>,
    _child: ChildGuard,
    pid: u32,
    /// Windows confinement; closing it ends the worker.
    #[cfg(windows)]
    _job: crate::os_sandbox::Job,
}
/// A started worker: the process to reap, its pipes, and (Windows) its job.
struct Launched {
    child: Box<dyn Reap>,
    pid: u32,
    input: Box<dyn Write + Send>,
    output: Box<dyn Read + Send>,
    /// None for an AppContainer worker, whose stderr is not inherited.
    errors: Option<Box<dyn Read + Send>>,
    #[cfg(windows)]
    job: crate::os_sandbox::Job,
}

/// Start `executable` as a worker with an empty environment (an AppContainer
/// worker also gets its container's LOCALAPPDATA, TEMP and TMP), confined
/// before it reads its first request.
fn launch(executable: &Path) -> std::io::Result<Launched> {
    #[cfg(windows)]
    if crate::app_container::enabled() {
        let (child, input, output, job) =
            crate::app_container::spawn(executable, &[WORKER_ARG], &[])?;
        return Ok(Launched {
            pid: child.id(),
            child: Box::new(child),
            input: Box::new(input),
            output: Box::new(output),
            errors: None,
            job,
        });
    }
    let mut command = Command::new(executable);
    command
        .arg(WORKER_ARG)
        .env_clear()
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000);
    }
    let mut child = command.spawn()?;
    // The worker blocks on its first request, which is only written after
    // the job applies; a worker that cannot be confined is not used.
    #[cfg(windows)]
    let job = match crate::os_sandbox::confine_child(&child) {
        Ok(job) => job,
        Err(error) => {
            let _ = child.kill();
            let _ = child.wait();
            return Err(error);
        }
    };
    Ok(Launched {
        pid: child.id(),
        input: Box::new(child.stdin.take().unwrap()),
        output: Box::new(child.stdout.take().unwrap()),
        errors: Some(Box::new(child.stderr.take().unwrap())),
        child: Box::new(child),
        #[cfg(windows)]
        job,
    })
}

/// The last bytes a worker wrote to stderr (a panic, a sandbox failure),
/// kept for the log entry of a worker that fails.
#[derive(Clone, Default)]
struct StderrTail {
    bytes: Arc<Mutex<VecDeque<u8>>>,
    /// Signalled when the pipe closes: the tail is then complete.
    closed: Arc<Mutex<Option<mpsc::Receiver<()>>>>,
}

const STDERR_TAIL_BYTES: usize = 8 * 1024;

impl StderrTail {
    fn follow(&self, mut errors: Box<dyn Read + Send>) -> std::io::Result<()> {
        let bytes = self.bytes.clone();
        let (done, closed) = mpsc::channel();
        *self.closed.lock().unwrap_or_else(|e| e.into_inner()) = Some(closed);
        std::thread::Builder::new()
            .name("plugin-stderr".into())
            .spawn(move || {
                let mut chunk = [0u8; 1024];
                while let Ok(n @ 1..) = errors.read(&mut chunk) {
                    let mut tail = bytes.lock().unwrap_or_else(|e| e.into_inner());
                    tail.extend(&chunk[..n]);
                    let excess = tail.len().saturating_sub(STDERR_TAIL_BYTES);
                    tail.drain(..excess);
                }
                let _ = done.send(());
            })?;
        Ok(())
    }

    /// The tail once the worker closed stderr, or after a short wait.
    fn text(&self) -> String {
        if let Some(closed) = self.closed.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = closed.recv_timeout(Duration::from_millis(200));
        }
        let tail = self.bytes.lock().unwrap_or_else(|e| e.into_inner());
        let (a, b) = tail.as_slices();
        String::from_utf8_lossy(&[a, b].concat()).trim().to_owned()
    }
}

impl PluginProcess {
    /// `init` is the first event; `answer` serves the plugin's declared reads.
    pub fn spawn(
        executable: &Path,
        source: String,
        init: serde_json::Value,
        answer: DataAnswer,
        post: impl Fn(WorkerResponse) + Send + 'static,
    ) -> std::io::Result<Self> {
        let Launched {
            child,
            pid,
            mut input,
            mut output,
            errors,
            #[cfg(windows)]
            job,
        } = launch(executable)?;
        let stderr = StderrTail::default();
        if let Some(errors) = errors {
            stderr.follow(errors)?;
        }
        let launched = Instant::now();
        let child = Arc::new(Mutex::new(child));
        let worker = child.clone();
        // Also reap the process if either supervisor thread fails to start.
        let child = ChildGuard(child);
        let (sender, events) = mpsc::sync_channel(crate::MAX_QUEUED_EVENTS);
        let (response_send, responses) = mpsc::sync_channel(1);
        std::thread::Builder::new()
            .name("plugin-output".into())
            .spawn(move || {
                while let Ok(response) = read_message::<WorkerResponse>(&mut output) {
                    if response_send.send(response).is_err() {
                        break;
                    }
                }
            })?;
        std::thread::Builder::new()
            .name("plugin-supervisor".into())
            .spawn(move || {
                let _child = ChildGuard(worker);
                let mut started = false;
                let run = || -> std::io::Result<()> {
                    write_message(&mut input, &WorkerRequest::Init { source })?;
                    match responses.recv_timeout(Duration::from_secs(15)) {
                        Ok(WorkerResponse::Ready) => {
                            started = true;
                            nanatype_diag::event!(
                                crate::diag::WORKER_READY,
                                startup_ms = launched.elapsed().as_millis() as u64
                            );
                        }
                        Ok(response) => {
                            if let WorkerResponse::Error { message } = &response {
                                nanatype_diag::report!(crate::diag::SCRIPT_ERROR; "{message}");
                            }
                            post(response);
                            return Ok(());
                        }
                        Err(_) => return Err(std::io::Error::other("plugin startup timed out")),
                    }
                    write_message(&mut input, &WorkerRequest::Event { event: init })?;
                    // Views are complete snapshots: while events are queued the
                    // supervisor keeps the worker busy and posts only the newest
                    // view, at most once per throttle window.
                    let mut last_post = std::time::Instant::now() - Duration::from_secs(1);
                    let mut interval = POST_INTERVAL;
                    let mut rounds = 0;
                    loop {
                        let mut response = responses
                            .recv_timeout(Duration::from_millis(200))
                            .map_err(|_| std::io::Error::other("plugin response timed out"))?;
                        let (requests, lines, dropped) = match &mut response {
                            WorkerResponse::Update {
                                requests,
                                logs,
                                dropped_logs,
                                ..
                            } => (
                                std::mem::take(requests),
                                std::mem::take(logs),
                                std::mem::take(dropped_logs),
                            ),
                            _ => (Vec::new(), Vec::new(), 0),
                        };
                        if !lines.is_empty() || dropped > 0 {
                            crate::validate_logs(&lines).map_err(std::io::Error::other)?;
                            post(WorkerResponse::Log { lines, dropped });
                        }
                        if let WorkerResponse::Error { message } = &response {
                            nanatype_diag::report!(crate::diag::SCRIPT_ERROR; "{message}");
                            post(response);
                            break;
                        }
                        // The worker checked them too; a mismatch means it is not ours.
                        crate::validate_requests(&requests).map_err(std::io::Error::other)?;
                        // Superseded by the next response when more work follows.
                        let mut pending = Some(response);
                        // Chained requests are answered without waiting for the UI;
                        // beyond the round budget they are dropped until the next event.
                        if !requests.is_empty() && rounds < MAX_DATA_ROUNDS {
                            rounds += 1;
                            let results =
                                bound_replies(requests.iter().map(|r| answer(r)).collect());
                            write_message(&mut input, &WorkerRequest::Data { results })?;
                            continue;
                        }
                        if last_post.elapsed() >= interval
                            && let Some(view) = pending.take()
                        {
                            interval = pace(&view);
                            post(view);
                            last_post = std::time::Instant::now();
                        }
                        let next = match events.try_recv() {
                            Ok(event) => Some(event),
                            Err(mpsc::TryRecvError::Empty) => {
                                if let Some(view) = pending.take() {
                                    std::thread::sleep(
                                        interval.saturating_sub(last_post.elapsed()),
                                    );
                                    interval = pace(&view);
                                    post(view);
                                    last_post = std::time::Instant::now();
                                }
                                events.recv().ok()
                            }
                            Err(mpsc::TryRecvError::Disconnected) => None,
                        };
                        match next {
                            Some(event) => {
                                rounds = 0;
                                write_message(&mut input, &event)?
                            }
                            None => break,
                        }
                    }
                    Ok(())
                };
                if let Err(error) = run() {
                    let stage = u64::from(started);
                    match stderr.text() {
                        tail if tail.is_empty() => nanatype_diag::report!(
                            crate::diag::WORKER_FAILED,
                            stage = stage;
                            "{error}"
                        ),
                        tail => nanatype_diag::report!(
                            crate::diag::WORKER_FAILED,
                            stage = stage;
                            "{error}; stderr: {}",
                            last_lines(&tail)
                        ),
                    }
                    post(WorkerResponse::Error {
                        message: error.to_string(),
                    });
                }
            })?;
        Ok(Self {
            sender,
            _child: child,
            pid,
            #[cfg(windows)]
            _job: job,
        })
    }
    /// The worker's process ID, for diagnostics; dropping the handle reaps it.
    pub fn id(&self) -> u32 {
        self.pid
    }
    pub fn event(&self, event: serde_json::Value) -> Result<(), EventError> {
        if serde_json::to_vec(&event)
            .map_err(|_| EventError::TooLarge)?
            .len()
            > crate::MAX_MESSAGE_BYTES / 2
        {
            return Err(EventError::TooLarge);
        }
        self.sender
            .try_send(WorkerRequest::Event { event })
            .map_err(|e| match e {
                mpsc::TrySendError::Full(_) => EventError::Busy,
                mpsc::TrySendError::Disconnected(_) => EventError::Stopped,
            })
    }
}

/// The end of a stderr tail: what fits in one log message, from a line start.
fn last_lines(tail: &str) -> &str {
    const KEEP: usize = 400;
    if tail.len() <= KEEP {
        return tail;
    }
    let mut start = tail.len() - KEEP;
    while !tail.is_char_boundary(start) {
        start += 1;
    }
    let cut = &tail[start..];
    cut.find('\n').map_or(cut, |i| &cut[i + 1..])
}

/// Why an event was not queued. Only `Stopped` means the worker is gone;
/// a busy or oversized event is dropped and the plugin keeps running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventError {
    TooLarge,
    Busy,
    Stopped,
}
impl std::fmt::Display for EventError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::TooLarge => "plugin event exceeds budget",
            Self::Busy => "plugin event queue full",
            Self::Stopped => "plugin stopped",
        })
    }
}
/// A worker process, killed and waited for by whichever owner lets go last.
trait Reap: Send {
    fn kill(&mut self);
    fn wait(&mut self);
}
impl Reap for Child {
    fn kill(&mut self) {
        let _ = Child::kill(self);
    }
    fn wait(&mut self) {
        let _ = Child::wait(self);
    }
}
#[cfg(windows)]
impl Reap for crate::app_container::Contained {
    fn kill(&mut self) {
        self.terminate();
    }
    fn wait(&mut self) {
        self.wait_exit();
    }
}

struct ChildGuard(Arc<Mutex<Box<dyn Reap>>>);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Ok(mut child) = self.0.lock() {
            child.kill();
            child.wait();
        }
    }
}

/// Blocking, bounded compile-only transport. Hosts invoke this on a background thread.
pub fn check_source_in_worker(executable: &Path, source: String) -> Result<(), String> {
    if source.len() > crate::MAX_SCRIPT_BYTES {
        return Err("Lua source exceeds budget".into());
    }
    let Launched {
        child,
        mut input,
        mut output,
        errors,
        #[cfg(windows)]
            job: _job,
        ..
    } = launch(executable).map_err(|e| e.to_string())?;
    let _guard = ChildGuard(Arc::new(Mutex::new(child)));
    let stderr = StderrTail::default();
    if let Some(errors) = errors {
        stderr.follow(errors).map_err(|e| e.to_string())?;
    }
    write_message(&mut input, &WorkerRequest::Check { source }).map_err(|e| e.to_string())?;
    let (tx, rx) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("lua-compile-response".into())
        .spawn(move || {
            let _ = tx.send(read_message::<WorkerResponse>(&mut output));
        })
        .map_err(|e| e.to_string())?;
    match rx
        .recv_timeout(Duration::from_secs(15))
        .map_err(|e| e.to_string())?
        .map_err(|e| e.to_string())?
    {
        WorkerResponse::Ready => Ok(()),
        WorkerResponse::Error { message } => Err(message),
        _ => Err("unexpected compile response".into()),
    }
}

/// The argument a host starts its worker executable with.
pub const WORKER_ARG: &str = "--plugin-worker";

pub fn run_worker() {
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    // Fail closed: a worker that cannot be confined runs no plugin code.
    if let Err(message) = crate::os_sandbox::confine_worker() {
        let _ = write_message(&mut output, &WorkerResponse::Error { message });
        return;
    }
    let mut vm = None;
    while let Ok(request) = read_message::<WorkerRequest>(&mut input) {
        let result = match request {
            WorkerRequest::Check { source } if vm.is_none() => {
                crate::LuaSandbox::check_source(&source).map(|_| WorkerResponse::Ready)
            }
            WorkerRequest::Init { source } if vm.is_none() => {
                crate::LuaSandbox::new(&source).map(|new| {
                    vm = Some(new);
                    WorkerResponse::Ready
                })
            }
            WorkerRequest::Event { event } => vm
                .as_mut()
                .ok_or_else(|| wasmtime::format_err!("plugin not initialized"))
                .and_then(|vm| vm.dispatch(&event))
                .map(|d| WorkerResponse::Update {
                    update: d.update,
                    requests: d.requests,
                    logs: d.logs,
                    dropped_logs: d.dropped_logs,
                }),
            WorkerRequest::Data { results } => vm
                .as_mut()
                .ok_or_else(|| wasmtime::format_err!("plugin not initialized"))
                .and_then(|vm| vm.deliver(&results))
                .map(|d| WorkerResponse::Update {
                    update: d.update,
                    requests: d.requests,
                    logs: d.logs,
                    dropped_logs: d.dropped_logs,
                }),
            _ => Err(wasmtime::format_err!("invalid worker state")),
        };
        let failed = result.is_err();
        let response = result.unwrap_or_else(|e| WorkerResponse::Error {
            message: format!("{e:#}").chars().take(1024).collect(),
        });
        if write_message(&mut output, &response).is_err() || failed {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::last_lines;

    #[test]
    fn a_long_stderr_tail_keeps_whole_last_lines() {
        assert_eq!(last_lines("short"), "short");
        let tail = format!(
            "{}\nthread 'main' panicked at x.rs:1\nboom",
            "a".repeat(500)
        );
        assert_eq!(last_lines(&tail), "thread 'main' panicked at x.rs:1\nboom");
    }
}
