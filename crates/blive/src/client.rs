//! The connection thread: join, keep alive, reconnect.

use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, TryRecvError, unbounded};
use nanatype_model::LinkFault;
use tungstenite::{Message, WebSocket};

use crate::protocol::{self, ChatEvent, FatalKind, HEARTBEAT_MESSAGE, Incoming, RoomKey};

/// blivechat sends a heartbeat every 10 s and drops clients silent for 15 s.
const HEARTBEAT_EVERY: Duration = Duration::from_secs(10);
/// No frame at all for this long means the connection is dead.
const SILENCE_LIMIT: Duration = Duration::from_secs(30);
const RETRY_AFTER: Duration = Duration::from_secs(3);
const RETRY_AFTER_FATAL: Duration = Duration::from_secs(30);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BliveConfig {
    pub host: String,
    pub port: u16,
    pub room: RoomKey,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum BliveStatus {
    #[default]
    Off,
    Connecting,
    /// Joined the room.
    Connected,
    /// The connection failed or dropped; it is retried.
    Error(LinkFault),
    /// blivechat gave up on the room. An auth code error is not retried.
    Fatal(FatalKind, String),
}

/// What the connection reports.
#[derive(Debug, Clone, PartialEq)]
pub enum BliveUpdate {
    Status(BliveStatus),
    Chat(ChatEvent),
}

/// An update stamped with the configuration it belongs to; updates from an
/// older [`BliveHandle::epoch`] come from a connection already replaced.
#[derive(Debug, Clone, PartialEq)]
pub struct BliveEvent {
    pub epoch: u64,
    pub update: BliveUpdate,
}

type DeliverFn = Arc<dyn Fn(BliveEvent) + Send + Sync>;
/// Reports one update under the epoch of the thread that made it.
type EmitFn = Box<dyn Fn(BliveUpdate) + Send>;

struct Running {
    config: BliveConfig,
    _stop: Sender<()>,
}

/// Owns the connection thread. Dropping it (or `set(None)`) stops the thread.
pub struct BliveHandle {
    deliver: DeliverFn,
    epoch: u64,
    running: Option<Running>,
}

impl BliveHandle {
    pub fn new(deliver: impl Fn(BliveEvent) + Send + Sync + 'static) -> Self {
        Self {
            deliver: Arc::new(deliver),
            epoch: 0,
            running: None,
        }
    }

    /// The current configuration's epoch.
    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Connect with `config`, reconnect after a change, or stop on `None`.
    pub fn set(&mut self, config: Option<BliveConfig>) {
        if self.running.as_ref().map(|r| &r.config) == config.as_ref() {
            return;
        }
        self.running = None;
        self.epoch += 1;
        let deliver = Arc::clone(&self.deliver);
        let epoch = self.epoch;
        let emit: EmitFn = Box::new(move |update| deliver(BliveEvent { epoch, update }));
        let Some(config) = config else {
            emit(BliveUpdate::Status(BliveStatus::Off));
            return;
        };
        let (stop_tx, stop_rx) = unbounded::<()>();
        let cfg = config.clone();
        thread::Builder::new()
            .name("nanatype-blive".into())
            .spawn(move || run(cfg, stop_rx, emit))
            .expect("spawn blivechat thread");
        self.running = Some(Running {
            config,
            _stop: stop_tx,
        });
    }

    /// Join again with the same configuration (after an auth code error,
    /// which is not retried on its own).
    pub fn reconnect(&mut self) {
        if let Some(running) = self.running.take() {
            self.set(Some(running.config));
        }
    }
}

/// The fault behind a WebSocket error.
fn ws_fault(e: &tungstenite::Error) -> LinkFault {
    match e {
        tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed => {
            LinkFault::Closed
        }
        tungstenite::Error::Io(io) => LinkFault::from_io(io),
        other => LinkFault::Other(other.to_string()),
    }
}

fn connect(config: &BliveConfig) -> Result<WebSocket<TcpStream>, LinkFault> {
    let io = |e: std::io::Error| LinkFault::from_io(&e);
    let unresolved = || LinkFault::Unresolved(config.host.clone());
    let addr = (config.host.as_str(), config.port)
        .to_socket_addrs()
        .map_err(|_| unresolved())?
        .next()
        .ok_or_else(unresolved)?;
    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).map_err(io)?;
    let url = format!("ws://{}:{}/api/chat", config.host, config.port);
    let (socket, _) = tungstenite::client(url, stream).map_err(|e| match e {
        tungstenite::HandshakeError::Failure(e) => ws_fault(&e),
        tungstenite::HandshakeError::Interrupted(_) => LinkFault::TimedOut,
    })?;
    socket
        .get_ref()
        .set_read_timeout(Some(Duration::from_millis(100)))
        .map_err(io)?;
    Ok(socket)
}

pub(crate) fn stopped(stop: &Receiver<()>) -> bool {
    stop.try_recv() != Err(TryRecvError::Empty)
}

/// A read that timed out: nothing arrived yet.
pub(crate) fn idle(e: &tungstenite::Error) -> bool {
    matches!(e, tungstenite::Error::Io(io) if matches!(
        io.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    ))
}

/// Sleep until `until`, waking early when stopped.
fn wait(stop: &Receiver<()>, until: Instant) -> bool {
    let left = until.saturating_duration_since(Instant::now());
    !matches!(
        stop.recv_timeout(left),
        Err(crossbeam_channel::RecvTimeoutError::Timeout)
    )
}

fn run(config: BliveConfig, stop: Receiver<()>, emit: EmitFn) {
    loop {
        if stopped(&stop) {
            return;
        }
        emit(BliveUpdate::Status(BliveStatus::Connecting));
        let retry = match session(&config, &stop, &emit) {
            Ended::Stopped => return,
            Ended::Dropped(reason) => {
                emit(BliveUpdate::Status(BliveStatus::Error(reason)));
                RETRY_AFTER
            }
            Ended::Fatal(FatalKind::AuthCode, message) => {
                emit(BliveUpdate::Status(BliveStatus::Fatal(
                    FatalKind::AuthCode,
                    message,
                )));
                // Wait for a new configuration.
                let _ = stop.recv();
                return;
            }
            Ended::Fatal(kind, message) => {
                emit(BliveUpdate::Status(BliveStatus::Fatal(kind, message)));
                RETRY_AFTER_FATAL
            }
        };
        if wait(&stop, Instant::now() + retry) {
            return;
        }
    }
}

enum Ended {
    Stopped,
    Dropped(LinkFault),
    Fatal(FatalKind, String),
}

fn session(config: &BliveConfig, stop: &Receiver<()>, emit: &EmitFn) -> Ended {
    let mut socket = match connect(config) {
        Ok(socket) => socket,
        Err(e) => return Ended::Dropped(e),
    };
    if let Err(e) = socket.send(Message::text(protocol::join_message(&config.room))) {
        return Ended::Dropped(ws_fault(&e));
    }
    emit(BliveUpdate::Status(BliveStatus::Connected));
    let mut last_sent = Instant::now();
    let mut last_heard = Instant::now();
    loop {
        if stopped(stop) {
            let _ = socket.close(None);
            let _ = socket.flush();
            return Ended::Stopped;
        }
        if last_sent.elapsed() >= HEARTBEAT_EVERY {
            if let Err(e) = socket.send(Message::text(HEARTBEAT_MESSAGE)) {
                return Ended::Dropped(ws_fault(&e));
            }
            last_sent = Instant::now();
        }
        if last_heard.elapsed() >= SILENCE_LIMIT {
            return Ended::Dropped(LinkFault::Silent);
        }
        match socket.read() {
            Ok(Message::Text(text)) => {
                last_heard = Instant::now();
                match protocol::parse(text.as_str()) {
                    Some(Incoming::Heartbeat) => {
                        if let Err(e) = socket.send(Message::text(HEARTBEAT_MESSAGE)) {
                            return Ended::Dropped(ws_fault(&e));
                        }
                        last_sent = Instant::now();
                    }
                    Some(Incoming::Fatal { kind, message }) => {
                        let _ = socket.close(None);
                        return Ended::Fatal(kind, message);
                    }
                    Some(Incoming::Event(event)) => emit(BliveUpdate::Chat(event)),
                    Some(Incoming::Other) | None => {}
                }
            }
            Ok(Message::Close(_)) => {
                return Ended::Dropped(LinkFault::Closed);
            }
            Ok(_) => last_heard = Instant::now(),
            Err(e) if idle(&e) => {}
            Err(e) => return Ended::Dropped(ws_fault(&e)),
        }
    }
}
