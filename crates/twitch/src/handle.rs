//! Public `TwitchHandle` coordinating the dedicated IRC reader, OAuth worker,
//! and Helix sender without blocking the caller or polling idle sockets.
use crate::{
    Config, ConnectionState, Event, SessionEvent,
    api::{self, Http},
    auth::{AuthCmd, AuthHandle},
    credentials::Store,
    irc,
    reader::{self, Deliver, ReaderHandle},
    sender::{AuthSync, Permit, SenderCmd, SenderHandle},
};
use crossbeam_channel::unbounded;
use nanatype_model::LineId;
use std::{
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, AtomicUsize, Ordering},
    },
};

pub struct TwitchHandle {
    epoch: Arc<AtomicU64>,
    /// Serializes `advance()` so `SetEpoch` ordering always matches `epoch`.
    epoch_lock: Mutex<()>,
    capacity: Arc<AtomicUsize>,
    auth: AuthHandle,
    sender: SenderHandle,
    reader: Option<ReaderHandle>,
    deliver: Deliver,
    endpoint: String,
    config: Option<Config>,
}

impl TwitchHandle {
    pub fn new(directory: PathBuf, deliver: impl Fn(SessionEvent) + Send + Sync + 'static) -> Self {
        Self::with_parts(
            Store::new(&directory),
            Arc::new(api::Ureq::new()),
            irc::ENDPOINT.into(),
            Arc::new(deliver),
        )
    }

    fn with_parts(store: Store, http: Arc<dyn Http>, endpoint: String, deliver: Deliver) -> Self {
        let epoch = Arc::new(AtomicU64::new(0));
        let (auth_tx, auth_rx) = unbounded();
        let (sender_tx, sender_rx) = unbounded();
        let auth_tx_for_sender = auth_tx.clone();
        let sender = SenderHandle::spawn(
            sender_tx.clone(),
            sender_rx,
            Arc::clone(&epoch),
            Arc::clone(&deliver),
            Arc::clone(&http),
            Box::new(move |sync| match sync {
                AuthSync::Refreshed(tokens) => {
                    let _ = auth_tx_for_sender.send(AuthCmd::SaveRefreshed(tokens));
                }
                AuthSync::Expired(e) => {
                    let _ = auth_tx_for_sender.send(AuthCmd::Expire(e));
                }
            }),
        );
        let auth = AuthHandle::spawn(
            auth_tx,
            auth_rx,
            Arc::clone(&epoch),
            Arc::clone(&deliver),
            store,
            http,
            Box::new(move |auth| {
                let _ = sender_tx.send(SenderCmd::SetAuth(auth));
            }),
        );
        Self {
            epoch,
            epoch_lock: Mutex::new(()),
            capacity: Arc::new(AtomicUsize::new(0)),
            auth,
            sender,
            reader: None,
            deliver,
            endpoint,
            config: None,
        }
    }

    pub fn epoch(&self) -> u64 {
        self.epoch.load(Ordering::Acquire)
    }

    fn advance(&self) -> u64 {
        let _guard = self.epoch_lock.lock().unwrap_or_else(|e| e.into_inner());
        let e = self.epoch.fetch_add(1, Ordering::AcqRel) + 1;
        self.sender.send(SenderCmd::SetEpoch);
        e
    }

    pub fn set(&mut self, config: Config) {
        if self.config.as_ref() == Some(&config) {
            return;
        }
        self.config = Some(config.clone());
        // Stop the previous IRC reader before advancing the epoch so old frames stop immediately.
        self.reader = None;
        let e = {
            let _guard = self.epoch_lock.lock().unwrap_or_else(|e| e.into_inner());
            let e = self.epoch.fetch_add(1, Ordering::AcqRel) + 1;
            self.sender.send(SenderCmd::Configure);
            e
        };
        if !config.enabled {
            (self.deliver)(SessionEvent {
                epoch: e,
                event: Event::Connection(ConnectionState::Off),
            });
        } else {
            match crate::channel(&config.channel) {
                Ok(channel) => {
                    let sender_tx = self.sender.sender();
                    match reader::spawn(
                        self.endpoint.clone(),
                        channel,
                        Arc::clone(&self.epoch),
                        Arc::clone(&self.deliver),
                        move |room| {
                            let _ = sender_tx.send(SenderCmd::SetRoom(Some(room)));
                        },
                    ) {
                        Ok(reader) => self.reader = Some(reader),
                        Err(()) => (self.deliver)(SessionEvent {
                            epoch: e,
                            event: Event::Connection(ConnectionState::Error(
                                "worker-start-failed".into(),
                            )),
                        }),
                    }
                }
                Err(error) => (self.deliver)(SessionEvent {
                    epoch: e,
                    event: Event::Connection(ConnectionState::Error(error)),
                }),
            }
        }
        self.auth.send(AuthCmd::EnsureLoaded(e));
    }

    pub fn login(&self) {
        let e = self.advance();
        self.auth.send(AuthCmd::Login(e));
    }

    pub fn cancel_login(&self) {
        let e = self.advance();
        self.auth.send(AuthCmd::CancelLogin(e));
    }

    pub fn refresh(&self) {
        let e = self.advance();
        self.auth.send(AuthCmd::Refresh(e));
    }

    pub fn logout(&self) {
        let e = self.advance();
        self.auth.send(AuthCmd::Logout(e));
    }

    pub fn set_sending(&self, value: bool) {
        self.sender.send(SenderCmd::SetSending(value));
    }

    pub fn send(&self, line: LineId, text: String) -> bool {
        let Some(permit) = Permit::acquire(self.capacity.clone()) else {
            return false;
        };
        self.sender.send(SenderCmd::Send {
            permit,
            epoch: self.epoch(),
            line,
            text,
        });
        true
    }

    pub fn retry(&self, line: LineId) {
        self.sender.send(SenderCmd::Retry(line));
    }

    pub fn skip_failed(&self, line: LineId) {
        self.sender.send(SenderCmd::SkipFailed(line));
    }
}

impl Drop for TwitchHandle {
    fn drop(&mut self) {
        self.reader = None;
        self.advance();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        AccountState,
        api::{ApiError, Reply},
    };
    use serde_json::{Value, json};
    use std::{
        sync::Condvar,
        time::{Duration, Instant},
    };

    struct BlockingHttp {
        state: Mutex<(bool, bool)>,
        wake: Condvar,
    }

    impl BlockingHttp {
        fn new() -> Arc<Self> {
            Arc::new(Self {
                state: Mutex::new((false, false)),
                wake: Condvar::new(),
            })
        }

        fn wait_entered(&self) {
            let mut g = self.state.lock().unwrap();
            while !g.0 {
                g = self.wake.wait(g).unwrap();
            }
        }

        fn release_all(&self) {
            self.state.lock().unwrap().1 = true;
            self.wake.notify_all();
        }
    }

    impl Http for BlockingHttp {
        fn form(&self, _url: &str, _form: &[(&str, &str)]) -> Result<Reply, ApiError> {
            let mut g = self.state.lock().unwrap();
            g.0 = true;
            self.wake.notify_all();
            while !g.1 {
                g = self.wake.wait(g).unwrap();
            }
            Ok((
                200,
                json!({
                    "device_code": "dev",
                    "user_code": "ABCD",
                    "verification_uri": "https://www.twitch.tv/activate",
                    "expires_in": 1800,
                    "interval": 5
                }),
            ))
        }

        fn get(&self, _url: &str, _auth: &str) -> Result<Reply, ApiError> {
            Err(ApiError::Network)
        }

        fn post(&self, _url: &str, _auth: &str, _body: &Value) -> Result<Reply, ApiError> {
            Err(ApiError::Network)
        }
    }

    #[test]
    fn blocking_oauth_never_stalls_commands_or_configure() {
        let http = BlockingHttp::new();
        let (tx, rx) = unbounded();
        let mut handle = TwitchHandle::with_parts(
            Store::Memory(Arc::new(Mutex::new(None))),
            http.clone(),
            "ws://127.0.0.1:9".into(),
            Arc::new(move |e| {
                let _ = tx.send(e);
            }),
        );
        handle.login();
        http.wait_entered();
        let started = Instant::now();
        handle.set(Config {
            enabled: false,
            channel: String::new(),
        });
        assert!(started.elapsed() < Duration::from_millis(50));
        let off = rx.recv_timeout(Duration::from_secs(1)).unwrap();
        assert_eq!(off.event, Event::Connection(ConnectionState::Off));
        http.release_all();
    }

    /// `NANATYPE_TWITCH_CHANNEL=<login> cargo test -p nanatype-twitch -- --ignored`
    #[test]
    #[ignore = "joins a real Twitch channel"]
    fn live_chat_arrives_from_a_real_channel() {
        let channel = std::env::var("NANATYPE_TWITCH_CHANNEL").expect("NANATYPE_TWITCH_CHANNEL");
        let directory = std::env::temp_dir();
        let (tx, rx) = unbounded();
        let mut handle = TwitchHandle::new(directory, move |e| {
            let _ = tx.send(e);
        });
        handle.set(Config {
            enabled: true,
            channel,
        });
        let deadline = Instant::now() + Duration::from_secs(120);
        while let Ok(event) = rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
            match event.event {
                Event::Chat { event, .. } => {
                    eprintln!("{}: {:?}", event.author.name, event.content);
                    return;
                }
                Event::Connection(ConnectionState::Error(e)) => panic!("{e}"),
                Event::Account(AccountState::Failed(e)) => eprintln!("account failed: {e}"),
                other => eprintln!("{other:?}"),
            }
        }
        panic!("no chat within two minutes");
    }
}
