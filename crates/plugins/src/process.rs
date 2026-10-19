//! Asynchronous transport: the UI thread never waits for Lua or IPC.
use crate::{
    DataReply, DataRequest, MAX_DATA_ROUNDS, WorkerRequest, WorkerResponse, bound_replies,
    read_message, write_message,
};
use std::{
    path::Path,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

/// At most one view update per plugin reaches the UI in this interval (~30/s).
const POST_INTERVAL: Duration = Duration::from_millis(33);

/// Answers a script's data request on the supervisor thread, never the UI thread.
pub type DataAnswer = Arc<dyn Fn(&DataRequest) -> DataReply + Send + Sync>;

pub struct PluginProcess {
    sender: mpsc::SyncSender<WorkerRequest>,
    child: Arc<Mutex<Child>>,
    pid: u32,
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
        let mut command = Command::new(executable);
        command
            .arg("--plugin-worker")
            .env_clear()
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            command.creation_flags(0x0800_0000);
        }
        let mut child = command.spawn()?;
        let pid = child.id();
        let mut input = child.stdin.take().unwrap();
        let mut output = child.stdout.take().unwrap();
        let child = Arc::new(Mutex::new(child));
        let worker = child.clone();
        let (sender, events) = mpsc::sync_channel(32);
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
                let run = || -> std::io::Result<()> {
                    write_message(&mut input, &WorkerRequest::Init { source })?;
                    match responses.recv_timeout(Duration::from_secs(15)) {
                        Ok(WorkerResponse::Ready) => {}
                        Ok(response) => {
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
                    let mut rounds = 0;
                    loop {
                        let mut response = responses
                            .recv_timeout(Duration::from_millis(200))
                            .map_err(|_| std::io::Error::other("plugin response timed out"))?;
                        let requests = match &mut response {
                            WorkerResponse::Update { requests, .. } => std::mem::take(requests),
                            _ => Vec::new(),
                        };
                        if matches!(response, WorkerResponse::Error { .. }) {
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
                        if last_post.elapsed() >= POST_INTERVAL
                            && let Some(view) = pending.take()
                        {
                            post(view);
                            last_post = std::time::Instant::now();
                        }
                        let next = match events.try_recv() {
                            Ok(event) => Some(event),
                            Err(mpsc::TryRecvError::Empty) => {
                                if let Some(view) = pending.take() {
                                    std::thread::sleep(
                                        POST_INTERVAL.saturating_sub(last_post.elapsed()),
                                    );
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
                    post(WorkerResponse::Error {
                        message: error.to_string(),
                    });
                }
                if let Ok(mut child) = worker.lock() {
                    let _ = child.kill();
                    let _ = child.wait();
                }
            })?;
        Ok(Self { sender, child, pid })
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
impl Drop for PluginProcess {
    fn drop(&mut self) {
        if let Ok(mut child) = self.child.lock() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

pub fn run_worker() {
    let mut input = std::io::stdin().lock();
    let mut output = std::io::stdout().lock();
    let mut vm = None;
    while let Ok(request) = read_message::<WorkerRequest>(&mut input) {
        let result = match request {
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
                }),
            WorkerRequest::Data { results } => vm
                .as_mut()
                .ok_or_else(|| wasmtime::format_err!("plugin not initialized"))
                .and_then(|vm| vm.deliver(&results))
                .map(|d| WorkerResponse::Update {
                    update: d.update,
                    requests: d.requests,
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
