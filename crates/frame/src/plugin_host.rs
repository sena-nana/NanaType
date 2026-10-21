//! Plugin worker processes, shared by live plugins and the package editor's
//! debugger, and short background jobs that post one message back.
use nana_ui::RuntimeProgramContext;
use nanatype_plugins::{PluginProcess, WorkerResponse};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};

/// Answer a worker's data requests from one reviewed snapshot, by value.
pub fn data_answer(data: Arc<nanatype_theme::DataAccess>) -> nanatype_plugins::DataAnswer {
    Arc::new(
        move |request| match data.answer(&request.id, &request.query) {
            Ok(value) => nanatype_plugins::DataReply {
                token: request.token,
                value: Some(value),
                error: None,
            },
            Err(error) => nanatype_plugins::DataReply::error(request.token, error),
        },
    )
}

enum WorkerState {
    Pending(Vec<serde_json::Value>),
    Running(PluginProcess),
    Stopped,
}

/// Creates and reaps the OS process off the UI thread, shared by live plugins
/// and the editor debugger. Pending events share the production queue budget.
pub struct Worker {
    state: Arc<Mutex<WorkerState>>,
    cancelled: Arc<AtomicBool>,
    wake: mpsc::SyncSender<Option<WorkerResponse>>,
}
impl Worker {
    pub fn spawn(
        executable: &std::path::Path,
        source: String,
        init: serde_json::Value,
        answer: nanatype_plugins::DataAnswer,
        post: impl Fn(WorkerResponse) + Send + 'static,
    ) -> Result<Self, String> {
        let executable = executable.to_owned();
        Self::launch(
            move |output| {
                PluginProcess::spawn(&executable, source, init, answer, move |response| {
                    let _ = output.send(Some(response));
                })
                .map_err(|e| e.to_string())
            },
            post,
        )
    }
    fn launch(
        create: impl FnOnce(mpsc::SyncSender<Option<WorkerResponse>>) -> Result<PluginProcess, String>
        + Send
        + 'static,
        post: impl Fn(WorkerResponse) + Send + 'static,
    ) -> Result<Self, String> {
        let state = Arc::new(Mutex::new(WorkerState::Pending(Vec::new())));
        let cancelled = Arc::new(AtomicBool::new(false));
        let (wake, responses) = mpsc::sync_channel(1);
        let (owned, stopped, output) = (state.clone(), cancelled.clone(), wake.clone());
        std::thread::Builder::new()
            .name("plugin-owner".into())
            .spawn(move || {
                let run = || -> Result<(), String> {
                    if stopped.load(Ordering::Relaxed) {
                        return Ok(());
                    }
                    let process = create(output)?;
                    {
                        let mut state = owned.lock().unwrap_or_else(|e| e.into_inner());
                        if stopped.load(Ordering::Relaxed) {
                            return Ok(());
                        }
                        if let WorkerState::Pending(events) = &mut *state {
                            for event in events.drain(..) {
                                process.event(event).map_err(|e| e.to_string())?;
                            }
                        }
                        *state = WorkerState::Running(process);
                    }
                    while let Ok(Some(response)) = responses.recv() {
                        if stopped.load(Ordering::Relaxed) {
                            break;
                        }
                        let failed = matches!(response, WorkerResponse::Error { .. });
                        post(response);
                        if failed {
                            break;
                        }
                    }
                    Ok(())
                };
                let result = run();
                let process = std::mem::replace(
                    &mut *owned.lock().unwrap_or_else(|e| e.into_inner()),
                    WorkerState::Stopped,
                );
                drop(process);
                if let Err(message) = result
                    && !stopped.load(Ordering::Relaxed)
                {
                    post(WorkerResponse::Error { message });
                }
            })
            .map_err(|e| e.to_string())?;
        Ok(Self {
            state,
            cancelled,
            wake,
        })
    }
    pub fn id(&self) -> Option<u32> {
        match &*self.state.lock().unwrap_or_else(|e| e.into_inner()) {
            WorkerState::Running(process) => Some(process.id()),
            _ => None,
        }
    }
    pub fn event(&self, event: serde_json::Value) -> Result<(), nanatype_plugins::EventError> {
        use nanatype_plugins::EventError;
        match &mut *self.state.lock().unwrap_or_else(|e| e.into_inner()) {
            WorkerState::Pending(events) => {
                if serde_json::to_vec(&event)
                    .map_err(|_| EventError::TooLarge)?
                    .len()
                    > nanatype_plugins::MAX_MESSAGE_BYTES / 2
                {
                    return Err(EventError::TooLarge);
                }
                if events.len() == nanatype_plugins::MAX_QUEUED_EVENTS {
                    return Err(EventError::Busy);
                }
                events.push(event);
                Ok(())
            }
            WorkerState::Running(process) => process.event(event),
            WorkerState::Stopped => Err(EventError::Stopped),
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        let _ = self.wake.try_send(None);
    }
}

/// Run `work` on a short-lived background thread and post its message. Package
/// capture, hashing and signature checks never run on the UI thread.
pub fn background<M: Send + 'static>(
    name: &str,
    ctx: &RuntimeProgramContext<M>,
    work: impl FnOnce() -> M + Send + 'static,
) {
    if let Err(error) = try_background(name, ctx, work) {
        crate::debug_log(&format!("{name} unavailable: {error}"));
    }
}

pub fn try_background<M: Send + 'static>(
    name: &str,
    ctx: &RuntimeProgramContext<M>,
    work: impl FnOnce() -> M + Send + 'static,
) -> std::io::Result<()> {
    let ctx = ctx.clone();
    std::thread::Builder::new()
        .name(name.into())
        .spawn(move || ctx.dispatch(work()))
        .map(|_| ())
}

#[cfg(test)]
mod worker_tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn cancel_during_start_bounds_pending_events_and_discards_late_failure() {
        let (entered, started) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        let (posted, results) = mpsc::channel();
        let worker = Worker::launch(
            move |_| {
                entered.send(()).unwrap();
                gate.recv_timeout(Duration::from_secs(5)).unwrap();
                Err("late process creation failure".into())
            },
            move |response| {
                posted.send(response).unwrap();
            },
        )
        .unwrap();
        started.recv_timeout(Duration::from_secs(5)).unwrap();
        for _ in 0..nanatype_plugins::MAX_QUEUED_EVENTS {
            worker.event(serde_json::json!({"type":"tick"})).unwrap();
        }
        assert_eq!(
            worker.event(serde_json::json!({})),
            Err(nanatype_plugins::EventError::Busy)
        );
        assert_eq!(
            worker.event(serde_json::json!(
                "x".repeat(nanatype_plugins::MAX_MESSAGE_BYTES)
            )),
            Err(nanatype_plugins::EventError::TooLarge)
        );
        drop(worker);
        release.send(()).unwrap();
        assert!(
            matches!(
                results.recv_timeout(Duration::from_secs(5)),
                Err(mpsc::RecvTimeoutError::Disconnected)
            ),
            "cancelled work must exit without publishing an error"
        );
    }
}
