//! The connection thread: join, keep alive, reconnect.

use std::io::{Read, Write};
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use nanatype_model::{LinkFault, tcp_connect_timeout, ws_connect, ws_idle};
use serde_json::Value;
use tungstenite::stream::MaybeTlsStream;
use tungstenite::{Message, WebSocket};

use crate::protocol::{self, ChatEvent, FatalKind, HEARTBEAT_MESSAGE, Incoming, RoomKey};
use crate::{comet, laplace};

/// blivechat sends a heartbeat every 10 s and drops clients silent for 15 s.
const HEARTBEAT_EVERY: Duration = Duration::from_secs(10);
/// No frame at all for this long means the connection is dead.
const SILENCE_LIMIT: Duration = Duration::from_secs(30);
/// The LAPLACE bridge only pings every 54 s, and answers ours.
const LAPLACE_PING_EVERY: Duration = Duration::from_secs(20);
const LAPLACE_SILENCE_LIMIT: Duration = Duration::from_secs(60);
/// 彗星号 answers each `ping` with `pong`.
const COMET_PING_EVERY: Duration = Duration::from_secs(10);
const COMET_SILENCE_LIMIT: Duration = Duration::from_secs(30);
/// Initial handshake read timeout; active sessions set read_timeout to the next deadline.
const HANDSHAKE_READ_TIMEOUT: Duration = Duration::from_secs(5);
const RETRY_AFTER: Duration = Duration::from_secs(3);
const RETRY_AFTER_FATAL: Duration = Duration::from_secs(30);

/// What to connect to on the server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Join {
    /// A room on a blivechat server.
    Blivechat(RoomKey),
    /// The rooms the 彗星号 client at `host:port` listens to, on the
    /// platforms asked for.
    Comet(comet::Rooms),
    /// A LAPLACE Event Bridge's feed at `url` (`ws://` or `wss://`, with
    /// its token), from [`crate::laplace::Link::url`].
    Laplace { url: String },
}

/// `host` and `port` are a blivechat server's or the 彗星号 client's; a
/// LAPLACE bridge is wholly named by its link.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BliveConfig {
    pub host: String,
    pub port: u16,
    pub join: Join,
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

struct StopSignal {
    stopped: AtomicBool,
    wake: Condvar,
    socket: Mutex<Option<TcpStream>>,
}

struct SocketGuard<'a>(&'a StopSignal);

impl Drop for SocketGuard<'_> {
    fn drop(&mut self) {
        *self.0.socket.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

impl StopSignal {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            stopped: AtomicBool::new(false),
            wake: Condvar::new(),
            socket: Mutex::new(None),
        })
    }

    fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }

    fn cancel(&self) {
        self.stopped.store(true, Ordering::Release);
        let stream = self
            .socket
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .and_then(|s| s.try_clone().ok());
        if let Some(stream) = stream {
            let _ = stream.shutdown(Shutdown::Both);
        }
        self.wake.notify_all();
    }

    fn attach(&self, stream: Option<TcpStream>) -> SocketGuard<'_> {
        let mut slot = self.socket.lock().unwrap_or_else(|e| e.into_inner());
        if self.is_stopped() {
            if let Some(stream) = stream {
                let _ = stream.shutdown(Shutdown::Both);
            }
        } else {
            *slot = stream;
        }
        SocketGuard(self)
    }

    /// Sleep until `until`, waking early when stopped. Returns `true` if stopped.
    fn wait(&self, until: Instant) -> bool {
        let mut guard = self.socket.lock().unwrap_or_else(|e| e.into_inner());
        while !self.is_stopped() {
            let left = until.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            let (next, _) = self
                .wake
                .wait_timeout(guard, left)
                .unwrap_or_else(|e| e.into_inner());
            guard = next;
        }
        true
    }

    fn wait_forever(&self) {
        let mut guard = self.socket.lock().unwrap_or_else(|e| e.into_inner());
        while !self.is_stopped() {
            guard = self.wake.wait(guard).unwrap_or_else(|e| e.into_inner());
        }
    }
}

struct Running {
    config: BliveConfig,
    stop: Arc<StopSignal>,
}

impl Drop for Running {
    fn drop(&mut self) {
        self.stop.cancel();
    }
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
        let stop = StopSignal::new();
        let worker_stop = Arc::clone(&stop);
        let cfg = config.clone();
        thread::Builder::new()
            .name("nanatype-blive".into())
            .spawn(move || run(cfg, worker_stop, emit))
            .expect("spawn blivechat thread");
        self.running = Some(Running { config, stop });
    }

    /// Join again with the same configuration (after an auth code error,
    /// which is not retried on its own).
    pub fn reconnect(&mut self) {
        if let Some(running) = self.running.take() {
            let config = running.config.clone();
            drop(running);
            self.set(Some(config));
        }
    }
}

fn run(config: BliveConfig, stop: Arc<StopSignal>, emit: EmitFn) {
    loop {
        if stop.is_stopped() {
            return;
        }
        emit(BliveUpdate::Status(BliveStatus::Connecting));
        let retry = match session(&config, &stop, &emit) {
            Ended::Stopped => return,
            Ended::Dropped(reason) => {
                // A wrong token stays wrong; ask again now and then.
                let retry = if reason == LinkFault::Unauthorized {
                    RETRY_AFTER_FATAL
                } else {
                    RETRY_AFTER
                };
                emit(BliveUpdate::Status(BliveStatus::Error(reason)));
                retry
            }
            Ended::Fatal(FatalKind::AuthCode, message) => {
                emit(BliveUpdate::Status(BliveStatus::Fatal(
                    FatalKind::AuthCode,
                    message,
                )));
                // Wait for a new configuration.
                stop.wait_forever();
                return;
            }
            Ended::Fatal(kind, message) => {
                emit(BliveUpdate::Status(BliveStatus::Fatal(kind, message)));
                RETRY_AFTER_FATAL
            }
        };
        if stop.wait(Instant::now() + retry) {
            return;
        }
    }
}

enum Ended {
    Stopped,
    Dropped(LinkFault),
    Fatal(FatalKind, String),
}

trait SocketTcp {
    fn tcp(&self) -> Option<&TcpStream>;
}

impl SocketTcp for TcpStream {
    fn tcp(&self) -> Option<&TcpStream> {
        Some(self)
    }
}

impl SocketTcp for MaybeTlsStream<TcpStream> {
    fn tcp(&self) -> Option<&TcpStream> {
        match self {
            MaybeTlsStream::Plain(s) => Some(s),
            MaybeTlsStream::Rustls(s) => Some(&s.sock),
            _ => None,
        }
    }
}

fn session(config: &BliveConfig, stop: &StopSignal, emit: &EmitFn) -> Ended {
    match &config.join {
        Join::Blivechat(room) => {
            match ws_connect(
                &config.host,
                config.port,
                "/api/chat",
                None,
                HANDSHAKE_READ_TIMEOUT,
            ) {
                Ok(socket) => Session {
                    feed: Feed::Blivechat,
                    join: Some(protocol::join_message(room)),
                    beat: Message::text(HEARTBEAT_MESSAGE),
                    beat_every: HEARTBEAT_EVERY,
                    silence: SILENCE_LIMIT,
                }
                .run(socket, stop, emit),
                Err(e) => Ended::Dropped(e),
            }
        }
        Join::Comet(rooms) => {
            match ws_connect(&config.host, config.port, "/", None, HANDSHAKE_READ_TIMEOUT) {
                Ok(socket) => Session {
                    feed: Feed::Comet(*rooms),
                    // A first ping at once: a quiet room still hears back.
                    join: Some(comet::PING.into()),
                    beat: Message::text(comet::PING),
                    beat_every: COMET_PING_EVERY,
                    silence: COMET_SILENCE_LIMIT,
                }
                .run(socket, stop, emit),
                Err(e) => Ended::Dropped(e),
            }
        }
        Join::Laplace { url } => match laplace_connect(url) {
            Ok(socket) => Session {
                feed: Feed::Laplace,
                join: None,
                beat: Message::Ping(Default::default()),
                beat_every: LAPLACE_PING_EVERY,
                silence: LAPLACE_SILENCE_LIMIT,
            }
            .run(socket, stop, emit),
            Err(e) => Ended::Dropped(e),
        },
    }
}

/// Open a LAPLACE bridge's `ws://` or `wss://` link.
fn laplace_connect(url: &str) -> Result<WebSocket<MaybeTlsStream<TcpStream>>, LinkFault> {
    let io = |e: std::io::Error| LinkFault::from_io(&e);
    let parsed = url::Url::parse(url).map_err(|e| LinkFault::Other(e.to_string()))?;
    let host = parsed
        .host_str()
        .unwrap_or_default()
        .trim_start_matches('[')
        .trim_end_matches(']');
    let port = parsed
        .port_or_known_default()
        .unwrap_or(laplace::DEFAULT_PORT);
    let stream = tcp_connect_timeout(host, port, Duration::from_secs(5))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(io)?;
    let (socket, _) = tungstenite::client_tls_with_config(url, stream, None, None)
        .map_err(LinkFault::from_handshake)?;
    Ok(socket)
}

/// Which protocol a connection's frames are in.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Feed {
    Blivechat,
    Laplace,
    Comet(comet::Rooms),
}

/// How one protocol keeps a joined connection alive.
struct Session {
    feed: Feed,
    /// Sent once on connecting.
    join: Option<String>,
    beat: Message,
    beat_every: Duration,
    silence: Duration,
}

impl Session {
    fn run<S: Read + Write + SocketTcp>(
        self,
        mut socket: WebSocket<S>,
        stop: &StopSignal,
        emit: &EmitFn,
    ) -> Ended {
        let _guard = stop.attach(socket.get_ref().tcp().and_then(|s| s.try_clone().ok()));
        if stop.is_stopped() {
            let _ = socket.close(None);
            return Ended::Stopped;
        }
        if let Some(join) = self.join
            && let Err(e) = socket.send(Message::text(join))
        {
            return if stop.is_stopped() {
                Ended::Stopped
            } else {
                Ended::Dropped(LinkFault::from_ws(&e))
            };
        }
        emit(BliveUpdate::Status(BliveStatus::Connected));
        let mut last_sent = Instant::now();
        let mut last_heard = Instant::now();
        loop {
            if stop.is_stopped() {
                let _ = socket.close(None);
                let _ = socket.flush();
                return Ended::Stopped;
            }
            if last_sent.elapsed() >= self.beat_every {
                if let Err(e) = socket.send(self.beat.clone()) {
                    return if stop.is_stopped() {
                        Ended::Stopped
                    } else {
                        Ended::Dropped(LinkFault::from_ws(&e))
                    };
                }
                last_sent = Instant::now();
            }
            if last_heard.elapsed() >= self.silence {
                return Ended::Dropped(LinkFault::Silent);
            }
            let next_deadline = (last_sent + self.beat_every).min(last_heard + self.silence);
            let wait = next_deadline
                .saturating_duration_since(Instant::now())
                .max(Duration::from_millis(1));
            if let Some(tcp) = socket.get_ref().tcp() {
                let _ = tcp.set_read_timeout(Some(wait));
            }
            let text = match socket.read() {
                Ok(Message::Text(text)) => text.as_str().to_owned(),
                // 彗星号 encrypts its messages.
                Ok(Message::Binary(frame)) if matches!(self.feed, Feed::Comet(_)) => {
                    match comet::decrypt(&frame) {
                        Some(text) => text,
                        None => continue,
                    }
                }
                Ok(Message::Close(_)) => {
                    return if stop.is_stopped() {
                        Ended::Stopped
                    } else {
                        Ended::Dropped(LinkFault::Closed)
                    };
                }
                Ok(_) => {
                    last_heard = Instant::now();
                    continue;
                }
                Err(e) if ws_idle(&e) => continue,
                Err(e) => {
                    return if stop.is_stopped() {
                        Ended::Stopped
                    } else {
                        Ended::Dropped(LinkFault::from_ws(&e))
                    };
                }
            };
            last_heard = Instant::now();
            let incoming = match self.feed {
                Feed::Blivechat => protocol::parse(&text),
                Feed::Laplace => laplace::parse(&text),
                Feed::Comet(rooms) => comet::parse(&text, now_ms(), rooms),
            };
            match incoming {
                // 彗星号's `pong` is the answer to ours.
                Some(Incoming::Heartbeat) if matches!(self.feed, Feed::Comet(_)) => {}
                Some(Incoming::Heartbeat) => {
                    let answer = if self.feed == Feed::Laplace {
                        let ping: Value = serde_json::from_str(&text).unwrap_or_default();
                        Message::text(laplace::pong(&ping["timestamp"], now_ms()))
                    } else {
                        Message::text(HEARTBEAT_MESSAGE)
                    };
                    if let Err(e) = socket.send(answer) {
                        return Ended::Dropped(LinkFault::from_ws(&e));
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
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}
