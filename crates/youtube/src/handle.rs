use crate::{Config, ConnectionState, Event, SessionEvent, innertube};
use crossbeam_channel::{Sender, unbounded};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

type Deliver = Arc<dyn Fn(SessionEvent) + Send + Sync>;
#[derive(Clone)]
pub(crate) struct Session {
    epoch: u64,
    revision: Arc<AtomicU64>,
    deliver: Deliver,
}
impl Session {
    #[cfg(test)]
    pub fn fixture(deliver: impl Fn(SessionEvent) + Send + Sync + 'static) -> Self {
        Self {
            epoch: 1,
            revision: Arc::new(AtomicU64::new(1)),
            deliver: Arc::new(deliver),
        }
    }
    #[cfg(test)]
    pub fn stop(&self) {
        self.revision.fetch_add(1, Ordering::AcqRel);
    }
    /// Every change of target, and dropping the handle, starts a new epoch.
    pub fn current(&self) -> bool {
        self.revision.load(Ordering::Acquire) == self.epoch
    }
    pub fn emit(&self, event: Event) {
        if self.current() {
            (self.deliver)(SessionEvent {
                epoch: self.epoch,
                event,
            });
        }
    }
    pub fn pause(&self, duration: Duration) -> bool {
        let end = Instant::now() + duration;
        while self.current() && Instant::now() < end {
            thread::sleep(
                Duration::from_millis(25).min(end.saturating_duration_since(Instant::now())),
            );
        }
        self.current()
    }
}
enum Command {
    Run(Config),
    Stop,
}
pub struct YoutubeHandle {
    tx: Sender<(u64, Command)>,
    revision: Arc<AtomicU64>,
    config: Config,
    controller: Option<JoinHandle<()>>,
}
impl YoutubeHandle {
    pub fn new(deliver: impl Fn(SessionEvent) + Send + Sync + 'static) -> Self {
        let (tx, rx) = unbounded();
        let revision = Arc::new(AtomicU64::new(0));
        let callback: Deliver = Arc::new(deliver);
        let controller = thread::Builder::new()
            .name("nanatype-youtube".into())
            .spawn({
                let revision = revision.clone();
                move || {
                    let mut worker: Option<JoinHandle<()>> = None;
                    for (epoch, command) in rx {
                        if let Some(worker) = worker.take() {
                            let _ = worker.join();
                        }
                        let session = Session {
                            epoch,
                            revision: revision.clone(),
                            deliver: callback.clone(),
                        };
                        let config = match command {
                            Command::Run(config) => config,
                            Command::Stop => break,
                        };
                        if !session.current() {
                            continue;
                        }
                        if !config.enabled {
                            session.emit(Event::Connection(ConnectionState::Off));
                            continue;
                        }
                        let target = match crate::target(&config.target) {
                            Ok(target) => target,
                            Err(error) => {
                                session.emit(Event::Connection(ConnectionState::Error(error)));
                                continue;
                            }
                        };
                        let active = session.clone();
                        worker = thread::Builder::new()
                            .name("nanatype-youtube-session".into())
                            .spawn(move || {
                                if let Err(error) = innertube::run(&active, &target) {
                                    active.emit(Event::Connection(ConnectionState::Error(error)));
                                }
                            })
                            .ok();
                        if worker.is_none() {
                            session.emit(Event::Connection(ConnectionState::Error(
                                "worker-start-failed".into(),
                            )));
                        }
                    }
                }
            })
            .expect("start YouTube service");
        Self {
            tx,
            revision,
            config: Config::default(),
            controller: Some(controller),
        }
    }
    pub fn epoch(&self) -> u64 {
        self.revision.load(Ordering::Acquire)
    }
    fn advance(&self) -> u64 {
        self.revision.fetch_add(1, Ordering::AcqRel) + 1
    }
    pub fn set(&mut self, config: Config) {
        if self.config == config {
            return;
        }
        self.config = config;
        self.refresh();
    }
    pub fn refresh(&self) {
        let _ = self
            .tx
            .send((self.advance(), Command::Run(self.config.clone())));
    }
}
impl Drop for YoutubeHandle {
    fn drop(&mut self) {
        let _ = self.tx.send((self.advance(), Command::Stop));
        if let Some(controller) = self.controller.take() {
            let _ = controller.join();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[test]
    fn an_invalid_target_never_starts_a_session() {
        let events = Arc::new(Mutex::new(Vec::new()));
        let sink = events.clone();
        let mut handle = YoutubeHandle::new(move |e| sink.lock().unwrap().push(e));
        handle.set(Config {
            enabled: true,
            target: "https://evil.test/watch?v=dQw4w9WgXcQ".into(),
        });
        let deadline = Instant::now() + Duration::from_secs(2);
        while events.lock().unwrap().is_empty() && Instant::now() < deadline {
            thread::sleep(Duration::from_millis(5));
        }
        let events = events.lock().unwrap();
        assert!(matches!(
            &events[0],
            SessionEvent { epoch: 1, event: Event::Connection(ConnectionState::Error(e)) } if e == "invalid-target"
        ));
    }

    /// `NANATYPE_YOUTUBE_TARGET=@handle cargo test -p nanatype-youtube -- --ignored`
    #[test]
    #[ignore = "reads a real YouTube stream"]
    fn live_chat_arrives_from_a_real_stream() {
        let target = std::env::var("NANATYPE_YOUTUBE_TARGET").expect("NANATYPE_YOUTUBE_TARGET");
        let (tx, rx) = crossbeam_channel::unbounded();
        let mut handle = YoutubeHandle::new(move |e| {
            let _ = tx.send(e);
        });
        handle.set(Config {
            enabled: true,
            target,
        });
        let deadline = Instant::now() + Duration::from_secs(120);
        while let Ok(event) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            match event.event {
                Event::Chat { event, .. } => {
                    eprintln!("{}: {:?}", event.author.name, event.content);
                    return;
                }
                Event::Connection(ConnectionState::Error(e)) => panic!("{e}"),
                other => eprintln!("{other:?}"),
            }
        }
        panic!("no chat within two minutes");
    }
}
