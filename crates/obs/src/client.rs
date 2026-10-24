//! The connection thread: identify, follow the program scene, reconnect.

use std::collections::HashMap;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, TryRecvError, unbounded};
use nanatype_model::LinkFault;
use serde_json::{Value, json};
use tungstenite::{Message, WebSocket};

use crate::protocol::{self, Incoming, ObsItem};

const RETRY_AFTER: Duration = Duration::from_secs(3);
/// Hello and Identified come at once; anything slower is not obs-websocket.
const HANDSHAKE_LIMIT: Duration = Duration::from_secs(5);
/// A drag sends a transform per frame: report once it rests this long…
const SETTLE: Duration = Duration::from_millis(300);
/// …or at least this often while it goes on.
const SETTLE_MAX: Duration = Duration::from_secs(1);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObsConfig {
    pub host: String,
    pub port: u16,
    /// The obs-websocket server password, when it has one.
    pub password: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum ObsStatus {
    #[default]
    Off,
    Connecting,
    Connected,
    /// The server wants a password and none is set. Not retried.
    NeedsPassword,
    /// The password was refused. Not retried.
    AuthFailed,
    /// The connection failed or dropped; it is retried.
    Error(LinkFault),
}

/// The program scene and what is placed in it (top level only).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ObsScene {
    pub name: String,
    pub items: Vec<ObsItem>,
}

impl ObsScene {
    pub fn item(&self, source: &str) -> Option<&ObsItem> {
        self.items.iter().find(|i| i.source == source)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ObsUpdate {
    Status(ObsStatus),
    /// The program scene as it now stands; `None` while not connected.
    Scene(Option<Arc<ObsScene>>),
    /// OBS refused a request, in its own words.
    Refused(String),
}

/// An update stamped with the configuration it belongs to.
#[derive(Debug, Clone, PartialEq)]
pub struct ObsEvent {
    pub epoch: u64,
    pub update: ObsUpdate,
}

enum Command {
    SetBounds { source: String, size: [u32; 2] },
}

type DeliverFn = Arc<dyn Fn(ObsEvent) + Send + Sync>;
type EmitFn = Box<dyn Fn(ObsUpdate) + Send>;

struct Running {
    config: ObsConfig,
    commands: Sender<Command>,
}

/// Owns the connection thread. Dropping it (or `set(None)`) stops the thread.
pub struct ObsHandle {
    deliver: DeliverFn,
    epoch: u64,
    running: Option<Running>,
}

impl ObsHandle {
    pub fn new(deliver: impl Fn(ObsEvent) + Send + Sync + 'static) -> Self {
        Self {
            deliver: Arc::new(deliver),
            epoch: 0,
            running: None,
        }
    }

    pub fn epoch(&self) -> u64 {
        self.epoch
    }

    /// Connect with `config`, reconnect after a change, or stop on `None`.
    pub fn set(&mut self, config: Option<ObsConfig>) {
        if self.running.as_ref().map(|r| &r.config) == config.as_ref() {
            return;
        }
        self.running = None;
        self.epoch += 1;
        let deliver = Arc::clone(&self.deliver);
        let epoch = self.epoch;
        let emit: EmitFn = Box::new(move |update| deliver(ObsEvent { epoch, update }));
        let Some(config) = config else {
            emit(ObsUpdate::Scene(None));
            emit(ObsUpdate::Status(ObsStatus::Off));
            return;
        };
        let (tx, rx) = unbounded();
        let cfg = config.clone();
        thread::Builder::new()
            .name("nanatype-obs".into())
            .spawn(move || run(cfg, rx, emit))
            .expect("spawn obs-websocket thread");
        self.running = Some(Running {
            config,
            commands: tx,
        });
    }

    /// Give `source` in the program scene a bounding box of `size`. Ignored
    /// while not connected.
    pub fn set_bounds(&self, source: &str, size: [u32; 2]) {
        if let Some(r) = &self.running {
            let _ = r.commands.send(Command::SetBounds {
                source: source.to_owned(),
                size,
            });
        }
    }
}

fn ws_fault(e: &tungstenite::Error) -> LinkFault {
    match e {
        tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed => {
            LinkFault::Closed
        }
        tungstenite::Error::Io(io) => LinkFault::from_io(io),
        other => LinkFault::Other(other.to_string()),
    }
}

fn idle(e: &tungstenite::Error) -> bool {
    matches!(e, tungstenite::Error::Io(io) if matches!(
        io.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    ))
}

fn connect(config: &ObsConfig) -> Result<WebSocket<TcpStream>, LinkFault> {
    let io = |e: std::io::Error| LinkFault::from_io(&e);
    let unresolved = || LinkFault::Unresolved(config.host.clone());
    let addr = (config.host.as_str(), config.port)
        .to_socket_addrs()
        .map_err(|_| unresolved())?
        .next()
        .ok_or_else(unresolved)?;
    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).map_err(io)?;
    stream
        .set_read_timeout(Some(Duration::from_secs(2)))
        .map_err(io)?;
    let url = format!("ws://{}:{}", config.host, config.port);
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

enum Ended {
    Stopped,
    Dropped(LinkFault),
    /// Waits for a new configuration.
    Fatal(ObsStatus),
}

fn run(config: ObsConfig, commands: Receiver<Command>, emit: EmitFn) {
    loop {
        emit(ObsUpdate::Status(ObsStatus::Connecting));
        let ended = session(&config, &commands, &emit);
        emit(ObsUpdate::Scene(None));
        match ended {
            Ended::Stopped => return,
            Ended::Fatal(status) => {
                emit(ObsUpdate::Status(status));
                while commands.recv().is_ok() {}
                return;
            }
            Ended::Dropped(fault) => emit(ObsUpdate::Status(ObsStatus::Error(fault))),
        }
        // Commands sent while away mean nothing to the next connection.
        let until = Instant::now() + RETRY_AFTER;
        loop {
            match commands.recv_timeout(until.saturating_duration_since(Instant::now())) {
                Ok(_) => {}
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => return,
            }
        }
    }
}

/// What a sent request was for.
enum Pending {
    Program,
    Items(String),
    Bounds,
}

struct Session<'a> {
    socket: WebSocket<TcpStream>,
    emit: &'a EmitFn,
    next_id: u64,
    pending: HashMap<String, Pending>,
    scene: Option<ObsScene>,
    /// The program scene whose items were asked for last.
    wanted: Option<String>,
    /// A transform changed: report by the first deadline, the second capping
    /// a drag that never rests.
    report: Option<(Instant, Instant)>,
}

impl Session<'_> {
    fn send(&mut self, text: String) -> Result<(), LinkFault> {
        self.socket
            .send(Message::text(text))
            .map_err(|e| ws_fault(&e))
    }

    fn request(&mut self, kind: &str, data: Value, what: Pending) -> Result<(), LinkFault> {
        self.next_id += 1;
        let id = self.next_id.to_string();
        self.pending.insert(id.clone(), what);
        self.send(protocol::request(kind, &id, data))
    }

    fn refresh(&mut self) -> Result<(), LinkFault> {
        self.request("GetCurrentProgramScene", json!({}), Pending::Program)
    }

    fn items(&mut self, scene: String) -> Result<(), LinkFault> {
        self.wanted = Some(scene.clone());
        self.request(
            "GetSceneItemList",
            json!({ "sceneName": scene }),
            Pending::Items(scene),
        )
    }

    fn report_now(&mut self) {
        self.report = None;
        let scene = self.scene.clone().map(Arc::new);
        (self.emit)(ObsUpdate::Scene(scene));
    }

    fn program(&self) -> Option<&str> {
        self.scene.as_ref().map(|s| s.name.as_str())
    }

    fn command(&mut self, command: Command) -> Result<(), LinkFault> {
        match command {
            Command::SetBounds { source, size } => {
                let Some((scene, id)) = self
                    .scene
                    .as_ref()
                    .and_then(|s| Some((s.name.clone(), s.item(&source)?.id)))
                else {
                    return Ok(());
                };
                self.next_id += 1;
                let rid = self.next_id.to_string();
                self.pending.insert(rid.clone(), Pending::Bounds);
                self.send(protocol::set_bounds(&rid, &scene, id, size))
            }
        }
    }

    fn incoming(&mut self, message: Incoming) -> Result<(), LinkFault> {
        match message {
            Incoming::Response {
                id,
                ok,
                comment,
                data,
            } => match self.pending.remove(&id) {
                Some(Pending::Program) if ok => {
                    if let Some(name) = protocol::program_scene(&data) {
                        self.items(name)?;
                    }
                }
                Some(Pending::Items(scene)) if ok => {
                    // The program scene may have changed again since.
                    if self.wanted.as_deref() != Some(scene.as_str()) {
                        return Ok(());
                    }
                    self.scene = Some(ObsScene {
                        name: scene,
                        items: protocol::scene_items(&data),
                    });
                    self.report_now();
                }
                Some(_) if !ok => (self.emit)(ObsUpdate::Refused(comment)),
                _ => {}
            },
            Incoming::Event { kind, data } => self.event(&kind, &data)?,
            _ => {}
        }
        Ok(())
    }

    fn event(&mut self, kind: &str, data: &Value) -> Result<(), LinkFault> {
        let in_program = data.get("sceneName").and_then(Value::as_str) == self.program();
        match kind {
            "CurrentProgramSceneChanged" => {
                if let Some(name) = protocol::program_scene(data) {
                    self.items(name)?;
                }
            }
            "SceneItemTransformChanged" if in_program => {
                let id = data.get("sceneItemId").and_then(Value::as_i64);
                let transform = data.get("sceneItemTransform").map(protocol::transform);
                if let (Some(id), Some(transform), Some(scene)) = (id, transform, &mut self.scene)
                    && let Some(item) = scene.items.iter_mut().find(|i| i.id == id)
                    && item.transform != transform
                {
                    item.transform = transform;
                    let now = Instant::now();
                    let cap = self.report.map_or(now + SETTLE_MAX, |(_, cap)| cap);
                    self.report = Some(((now + SETTLE).min(cap), cap));
                }
            }
            "SceneItemCreated" | "SceneItemRemoved" | "SceneItemListReindexed" if in_program => {
                if let Some(name) = self.program().map(str::to_owned) {
                    self.items(name)?;
                }
            }
            "SceneNameChanged" | "InputNameChanged" | "SceneRemoved" => self.refresh()?,
            _ => {}
        }
        Ok(())
    }
}

/// Read until Hello and Identified; a close in between is a refusal.
fn handshake(
    socket: &mut WebSocket<TcpStream>,
    config: &ObsConfig,
    commands: &Receiver<Command>,
) -> Result<(), Ended> {
    let until = Instant::now() + HANDSHAKE_LIMIT;
    let mut identified = false;
    while !identified {
        if commands_closed(commands) {
            return Err(Ended::Stopped);
        }
        if Instant::now() >= until {
            return Err(Ended::Dropped(LinkFault::TimedOut));
        }
        match socket.read() {
            Ok(Message::Text(text)) => match protocol::parse(text.as_str()) {
                Some(Incoming::Hello(hello)) => {
                    let identify = protocol::identify(&hello, config.password.as_deref())
                        .map_err(|_| Ended::Fatal(ObsStatus::NeedsPassword))?;
                    socket
                        .send(Message::text(identify))
                        .map_err(|e| Ended::Dropped(ws_fault(&e)))?;
                }
                Some(Incoming::Identified) => identified = true,
                _ => {}
            },
            Ok(Message::Close(frame)) => {
                let code = frame.map(|f| u16::from(f.code));
                return Err(if code == Some(protocol::CLOSE_AUTH_FAILED) {
                    Ended::Fatal(ObsStatus::AuthFailed)
                } else {
                    Ended::Dropped(LinkFault::Closed)
                });
            }
            Ok(_) => {}
            Err(e) if idle(&e) => {}
            Err(e) => return Err(Ended::Dropped(ws_fault(&e))),
        }
    }
    Ok(())
}

/// The handle is gone (or replaced). Commands that came in are dropped.
fn commands_closed(commands: &Receiver<Command>) -> bool {
    loop {
        match commands.try_recv() {
            Ok(_) => {}
            Err(TryRecvError::Empty) => return false,
            Err(TryRecvError::Disconnected) => return true,
        }
    }
}

fn session(config: &ObsConfig, commands: &Receiver<Command>, emit: &EmitFn) -> Ended {
    let mut socket = match connect(config) {
        Ok(socket) => socket,
        Err(e) => return Ended::Dropped(e),
    };
    if let Err(ended) = handshake(&mut socket, config, commands) {
        let _ = socket.close(None);
        let _ = socket.flush();
        return ended;
    }
    emit(ObsUpdate::Status(ObsStatus::Connected));
    let mut s = Session {
        socket,
        emit,
        next_id: 0,
        pending: HashMap::new(),
        scene: None,
        wanted: None,
        report: None,
    };
    let result = (|| -> Result<Ended, LinkFault> {
        s.refresh()?;
        loop {
            loop {
                match commands.try_recv() {
                    Ok(command) => s.command(command)?,
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => return Ok(Ended::Stopped),
                }
            }
            if s.report.is_some_and(|(at, _)| Instant::now() >= at) {
                s.report_now();
            }
            match s.socket.read() {
                Ok(Message::Text(text)) => {
                    if let Some(message) = protocol::parse(text.as_str()) {
                        s.incoming(message)?;
                    }
                }
                Ok(Message::Close(_)) => return Err(LinkFault::Closed),
                Ok(_) => {}
                Err(e) if idle(&e) => {}
                Err(e) => return Err(ws_fault(&e)),
            }
        }
    })();
    match result {
        Ok(ended) => {
            let _ = s.socket.close(None);
            let _ = s.socket.flush();
            ended
        }
        Err(fault) => Ended::Dropped(fault),
    }
}
