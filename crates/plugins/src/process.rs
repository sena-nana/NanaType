//! Asynchronous transport: the UI thread never waits for Lua or IPC.
use crate::{WorkerRequest, WorkerResponse, read_message, write_message};
use std::{
    path::Path,
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

pub struct PluginProcess {
    sender: mpsc::SyncSender<WorkerRequest>,
    child: Arc<Mutex<Child>>,
}
impl PluginProcess {
    pub fn spawn(
        executable: &Path,
        source: String,
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
                    write_message(
                        &mut input,
                        &WorkerRequest::Event {
                            event: serde_json::json!({"type":"init"}),
                        },
                    )?;
                    let mut last_post = std::time::Instant::now() - Duration::from_secs(1);
                    loop {
                        let response = responses
                            .recv_timeout(Duration::from_millis(200))
                            .map_err(|_| std::io::Error::other("plugin response timed out"))?;
                        let failed = matches!(response, WorkerResponse::Error { .. });
                        let wait = Duration::from_millis(33).saturating_sub(last_post.elapsed());
                        if !wait.is_zero() {
                            std::thread::sleep(wait);
                        }
                        post(response);
                        last_post = std::time::Instant::now();
                        if failed {
                            break;
                        }
                        match events.recv() {
                            Ok(event) => write_message(&mut input, &event)?,
                            Err(_) => break,
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
        Ok(Self { sender, child })
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
                .and_then(|vm| vm.event(&event))
                .map(|update| WorkerResponse::Update { update }),
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
