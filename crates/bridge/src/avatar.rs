//! A plugin client for an avatar app's control API: VTube Studio or
//! NanaLive. The two speak alike (a WebSocket, a plugin token the user
//! approves, a hotkey list, expressions by file, parameter writes) and differ
//! in their wire format and message names, which a [`Protocol`] supplies.
//!
//! Flow: connect → authenticate with the stored token, or request one (the
//! app asks the user to confirm) → list the current model's hotkeys and
//! follow model changes → act on markers as they are reached:
//!
//! * `[expr=name]` turns on the expression of the hotkey it maps to (or
//!   presses the hotkey when it is not an expression toggle);
//! * `[vts ...]` presses hotkeys, turns expression files on or off and holds
//!   parameters;
//! * while the speaker is talking a mouth parameter is driven (lip sync).
//!
//! Expressions turned on during a line are turned off again when it leaves
//! the box (or when the next line starts). Reconnects every few seconds while
//! the app is closed.

use std::collections::{BTreeMap, BTreeSet};
use std::marker::PhantomData;
use std::net::{Shutdown, TcpStream};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, after, select, unbounded};
use nanatype_model::{LinkFault, VtsAction, VtsReset};
use serde_json::Value;
use tungstenite::{Message, WebSocket};

use crate::{BridgeEvent, BridgeStatus, Sink, StatusFn};

pub const PLUGIN_NAME: &str = "NanaType";
pub const PLUGIN_DEVELOPER: &str = "sena-nana";

/// How often parameters are written while the mouth moves or a value is
/// held. VTube Studio hands a parameter back to tracking a second after the
/// last injection; NanaLive takes a write for one frame.
const INJECT_EVERY: Duration = Duration::from_millis(33);
pub(crate) const ACTIVE_READ: Duration = Duration::from_millis(16);
const IDLE_MAX: Duration = Duration::from_millis(500);

/// Drive a mouth parameter while the speaker is talking.
#[derive(Debug, Clone, PartialEq)]
pub struct LipSync {
    pub param: String,
    /// Peak value of the mouth parameter (`MouthOpen` runs 0..1).
    pub amp: f32,
}

/// What an avatar client does: where it connects and how it maps markers.
#[derive(Debug, Clone, PartialEq)]
pub struct AvatarConfig {
    pub port: u16,
    pub token: Option<String>,
    /// Expression name → hotkey id. Unmapped names match a hotkey by name.
    pub expressions: BTreeMap<String, String>,
    pub lip_sync: Option<LipSync>,
    pub reset: VtsReset,
}

/// A hotkey of the current model, as the app lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AvatarHotkey {
    pub id: String,
    pub name: String,
    pub kind: String,
    /// The expression or animation file the hotkey plays, if any.
    pub file: String,
}

/// An API call: its message type and data.
pub(crate) type Call = (&'static str, Value);

/// What a message from the app means to the client.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Reply {
    /// The user granted a token.
    Token(String),
    Authenticated,
    /// The stored token was refused (revoked, or from another install).
    AuthRejected,
    Hotkeys(Vec<AvatarHotkey>),
    /// Another model loaded, or the model's hotkeys changed.
    ModelChanged,
    /// The app refused a request, in its own words (empty when it gave none).
    Error(String),
    Ignore,
}

/// One app's side of the conversation.
pub(crate) trait Protocol: Send + 'static {
    const SINK: Sink;
    const THREAD: &'static str;
    fn connect(port: u16) -> Result<WebSocket<TcpStream>, LinkFault>;
    /// A request as it goes on the wire.
    fn frame(kind: &str, id: &str, data: Value) -> Message;
    /// A received frame, decoded; `None` for frames that are not messages.
    fn parse(message: Message) -> Option<Value>;
    fn authenticate(token: &str) -> Call;
    fn request_token() -> Call;
    fn list_hotkeys() -> Call;
    /// Sent once authenticated, after the hotkey list: event subscriptions.
    fn subscriptions() -> Vec<Call>;
    fn hotkey(id: &str) -> Call;
    fn expression(file: &str, active: bool, fade: Option<f32>) -> Call;
    /// Set the mouth and the held parameters, written into `buf`.
    fn inject(buf: &mut Vec<u8>, id: &str, mouth: Option<(&str, f32)>, holds: &[Hold]) -> Message;
    /// What a message means. `auth_id` is the request id of the pending
    /// authentication, if any.
    fn reply(message: &Value, auth_id: Option<&str>) -> Reply;
}

enum Command {
    Event(BridgeEvent),
    /// The voice began a word of this many characters.
    Word(usize),
    Config(AvatarConfig),
    RequestToken,
    RefreshHotkeys,
}

pub(crate) struct StopSignal {
    stopped: AtomicBool,
    socket: Mutex<Option<TcpStream>>,
}

pub(crate) struct SocketGuard(Arc<StopSignal>);

impl Drop for SocketGuard {
    fn drop(&mut self) {
        *self.0.socket.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }
}

impl StopSignal {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            stopped: AtomicBool::new(false),
            socket: Mutex::new(None),
        })
    }

    pub(crate) fn is_stopped(&self) -> bool {
        self.stopped.load(Ordering::Acquire)
    }

    pub(crate) fn cancel(&self) {
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
    }

    pub(crate) fn attach(self: &Arc<Self>, stream: Option<TcpStream>) -> SocketGuard {
        let mut slot = self.socket.lock().unwrap_or_else(|e| e.into_inner());
        if self.is_stopped() {
            if let Some(stream) = stream {
                let _ = stream.shutdown(Shutdown::Both);
            }
        } else {
            *slot = stream;
        }
        SocketGuard(Arc::clone(self))
    }
}

/// The form mapping keys are kept in: lower case, `_` for spaces.
pub fn mapping_key(name: &str) -> String {
    name.trim().to_lowercase().replace(char::is_whitespace, "_")
}

/// Pick the hotkey for an expression: explicit mapping first, then a hotkey
/// whose name equals the expression (ignoring case). A mapped id that is not
/// in the list (yet) is still returned, as a bare hotkey.
pub(crate) fn resolve_hotkey(
    name: &str,
    map: &BTreeMap<String, String>,
    hotkeys: &[AvatarHotkey],
) -> Option<AvatarHotkey> {
    let key = mapping_key(name);
    if let Some(id) = map
        .iter()
        .find(|(k, _)| mapping_key(k) == key)
        .map(|(_, id)| id)
    {
        return Some(
            hotkeys
                .iter()
                .find(|h| &h.id == id)
                .cloned()
                .unwrap_or_else(|| AvatarHotkey {
                    id: id.clone(),
                    name: name.to_owned(),
                    kind: String::new(),
                    file: String::new(),
                }),
        );
    }
    find_hotkey(hotkeys, name).cloned()
}

/// A hotkey by id, or by name compared as mapping keys are.
fn find_hotkey<'a>(hotkeys: &'a [AvatarHotkey], name_or_id: &str) -> Option<&'a AvatarHotkey> {
    let key = mapping_key(name_or_id);
    hotkeys
        .iter()
        .find(|h| h.id == name_or_id || mapping_key(&h.name) == key)
}

/// What firing a hotkey should send: an expression toggle becomes a plain
/// activation, so firing it twice does not turn it off and it can be reset.
pub(crate) fn hotkey_request(hotkey: &AvatarHotkey) -> Request {
    if hotkey.kind == "ToggleExpression" && !hotkey.file.is_empty() {
        Request::Expression {
            file: hotkey.file.clone(),
            active: true,
            fade: None,
        }
    } else {
        Request::Hotkey(hotkey.id.clone())
    }
}

/// One API call the client makes for an event.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Request {
    Hotkey(String),
    Expression {
        file: String,
        active: bool,
        fade: Option<f32>,
    },
}

impl Request {
    fn call<P: Protocol>(&self) -> Call {
        match self {
            Self::Hotkey(id) => P::hotkey(id),
            Self::Expression { file, active, fade } => P::expression(file, *active, *fade),
        }
    }
}

/// The mouth opening `t` seconds into talking: a syllable-paced flap with a
/// slower swell, so it does not look mechanical. In `0..=1`.
pub(crate) fn mouth(t: f32) -> f32 {
    use std::f32::consts::TAU;
    let flap = 0.5 + 0.5 * (TAU * 5.5 * t).sin();
    let swell = 0.75 + 0.25 * (TAU * 1.3 * t + 1.0).sin();
    (flap * swell).clamp(0.0, 1.0)
}

/// How long a word of `chars` characters is likely said.
pub(crate) fn word_secs(chars: usize) -> f32 {
    (0.11 * chars as f32).clamp(0.14, 0.9)
}

/// The mouth `t` seconds into a word said over `secs` with `chars`
/// characters: it opens on each syllable (about one a character) and closes
/// as the word ends, so it follows the voice instead of a fixed flap. In
/// `0..=1`; zero once the word is over.
pub(crate) fn word_mouth(t: f32, secs: f32, chars: usize) -> f32 {
    if !(0.0..secs).contains(&t) {
        return 0.0;
    }
    let syllables = chars.clamp(1, 6) as f32;
    let phase = t / secs * syllables;
    // Each syllable rises and falls; the word as a whole swells and closes.
    let syllable = (std::f32::consts::PI * phase.fract()).sin();
    let word = (std::f32::consts::PI * t / secs).sin().sqrt();
    (0.35 + 0.65 * syllable).clamp(0.0, 1.0) * word
}

/// The word being said, from the voice's word boundaries.
#[derive(Debug, Clone, Copy)]
struct Word {
    at: Instant,
    secs: f32,
    chars: usize,
}

/// A parameter held by `[vts param=...]` until `until`.
#[derive(Debug, Clone)]
pub(crate) struct Hold {
    pub name: String,
    pub value: f32,
    pub until: Instant,
}

/// The client's state across connections: what the markers asked for.
#[derive(Debug, Default)]
struct Avatar {
    /// Expression files this client turned on and has not turned off.
    active: BTreeSet<String>,
    /// Talking since.
    speaking: Option<Instant>,
    /// The last word the voice began, while it talks. A voice without word
    /// boundaries leaves it empty and the mouth flaps.
    word: Option<Word>,
    /// The mouth needs one last write to close.
    close_mouth: bool,
    holds: Vec<Hold>,
    next_inject: Option<Instant>,
}

struct Session {
    socket: WebSocket<TcpStream>,
    authenticated: bool,
    hotkeys: Vec<AvatarHotkey>,
}

/// The handle the bridge keeps; dropping it stops the client.
pub(crate) struct AvatarClient {
    config: AvatarConfig,
    tx: Sender<Command>,
    stop: Arc<StopSignal>,
    _stop_tx: Sender<()>,
}

impl Drop for AvatarClient {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

impl AvatarClient {
    pub fn start<P: Protocol>(config: AvatarConfig, status: StatusFn) -> Self {
        let (tx, rx) = unbounded();
        let (stop_tx, stop_rx) = unbounded::<()>();
        let stop = StopSignal::new();
        let worker_stop = Arc::clone(&stop);
        let cfg = config.clone();
        thread::Builder::new()
            .name(P::THREAD.into())
            .spawn(move || Client::<P>::new(cfg, status, worker_stop).run(rx, stop_rx))
            .expect("spawn avatar client thread");
        Self {
            config,
            tx,
            stop,
            _stop_tx: stop_tx,
        }
    }

    pub fn port(&self) -> u16 {
        self.config.port
    }

    /// Apply a new mapping, token or lip sync setting without reconnecting.
    pub fn update(&mut self, config: AvatarConfig) {
        if config != self.config {
            self.config = config.clone();
            let _ = self.tx.send(Command::Config(config));
        }
    }

    pub fn emit(&self, event: &BridgeEvent) {
        if !matches!(
            event,
            BridgeEvent::Portrait { .. } | BridgeEvent::Shake { .. }
        ) {
            let _ = self.tx.send(Command::Event(event.clone()));
        }
    }

    pub fn word(&self, chars: usize) {
        let _ = self.tx.send(Command::Word(chars));
    }

    pub fn request_token(&self) {
        let _ = self.tx.send(Command::RequestToken);
    }

    pub fn refresh_hotkeys(&self) {
        let _ = self.tx.send(Command::RefreshHotkeys);
    }
}

struct Client<P> {
    config: AvatarConfig,
    status: StatusFn,
    stop: Arc<StopSignal>,
    session: Option<Session>,
    avatar: Avatar,
    next_attempt: Instant,
    /// Asked for while disconnected; sent once connected.
    want_token: bool,
    /// The request id of the authentication waiting for its answer.
    auth_id: Option<String>,
    counter: u64,
    inject_buf: Vec<u8>,
    _protocol: PhantomData<P>,
}

impl<P: Protocol> Client<P> {
    fn new(config: AvatarConfig, status: StatusFn, stop: Arc<StopSignal>) -> Self {
        Self {
            config,
            status,
            stop,
            session: None,
            avatar: Avatar::default(),
            next_attempt: Instant::now(),
            want_token: false,
            auth_id: None,
            counter: 0,
            inject_buf: Vec::with_capacity(512),
            _protocol: PhantomData,
        }
    }

    fn next_id(&mut self) -> String {
        self.counter += 1;
        format!("nt-{}", self.counter)
    }

    /// Send on the open session; a failed send drops it. Returns the
    /// request id.
    fn send(&mut self, (kind, data): Call) -> String {
        let id = self.next_id();
        self.send_message(P::frame(kind, &id, data));
        id
    }

    fn send_message(&mut self, message: Message) {
        let Some(s) = self.session.as_mut() else {
            return;
        };
        if let Err(e) = s.socket.send(message) {
            self.drop_session(LinkFault::from_ws(&e));
        }
    }

    /// The connection is gone: say why and try again shortly.
    fn drop_session(&mut self, why: LinkFault) {
        self.session = None;
        self.auth_id = None;
        (self.status)(BridgeStatus::Error(P::SINK, why));
        self.next_attempt = Instant::now() + Duration::from_secs(3);
    }

    fn authenticated(&self) -> bool {
        self.session.as_ref().is_some_and(|s| s.authenticated)
    }

    fn hotkeys(&self) -> &[AvatarHotkey] {
        self.session.as_ref().map_or(&[], |s| &s.hotkeys)
    }

    fn authenticate(&mut self) {
        if let Some(t) = self.config.token.clone() {
            self.auth_id = Some(self.send(P::authenticate(&t)));
        } else {
            (self.status)(BridgeStatus::NeedsToken(P::SINK));
        }
    }

    fn request_token(&mut self) {
        self.send(P::request_token());
    }

    fn run(mut self, rx: Receiver<Command>, stop: Receiver<()>) {
        let mut _socket_guard = None;
        let mut idle_wait = ACTIVE_READ;
        loop {
            if self.stop.is_stopped()
                || stop.try_recv() != Err(crossbeam_channel::TryRecvError::Empty)
            {
                _socket_guard = None;
                if let Some(mut s) = self.session.take() {
                    let _ = s.socket.close(None);
                    let _ = s.socket.flush();
                }
                return;
            }
            if self.session.is_none() {
                _socket_guard = None;
                let now = Instant::now();
                if now < self.next_attempt {
                    // Events are dropped while disconnected; requests wait with 0 Hz idle wakeup.
                    let wait = self.next_attempt.saturating_duration_since(now);
                    select! {
                        recv(stop) -> _ => return,
                        recv(rx) -> res => match res {
                            Ok(Command::RequestToken) => {
                                self.want_token = true;
                                self.next_attempt = Instant::now();
                            }
                            Ok(Command::RefreshHotkeys) => self.next_attempt = Instant::now(),
                            Ok(Command::Config(c)) => self.configure(c),
                            Ok(Command::Event(e)) => self.track_offline(&e),
                            Ok(Command::Word(_)) => {}
                            Err(_) => return,
                        },
                        recv(after(wait)) -> _ => {}
                    }
                    continue;
                }
                (self.status)(BridgeStatus::Connecting(P::SINK));
                match P::connect(self.config.port) {
                    Ok(socket) => {
                        let clone = socket.get_ref().try_clone().ok();
                        self.session = Some(Session {
                            socket,
                            authenticated: false,
                            hotkeys: Vec::new(),
                        });
                        idle_wait = ACTIVE_READ;
                        _socket_guard = Some(self.stop.attach(clone));
                        if self.want_token {
                            self.want_token = false;
                            self.request_token();
                        } else {
                            self.authenticate();
                        }
                    }
                    Err(e) => {
                        (self.status)(BridgeStatus::Error(P::SINK, e));
                        self.next_attempt = Instant::now() + Duration::from_secs(5);
                        continue;
                    }
                }
            }
            while let Ok(cmd) = rx.try_recv() {
                idle_wait = ACTIVE_READ;
                match cmd {
                    Command::RequestToken => self.request_token(),
                    Command::RefreshHotkeys if self.authenticated() => {
                        self.send(P::list_hotkeys());
                    }
                    Command::RefreshHotkeys => {}
                    Command::Config(c) => self.configure(c),
                    Command::Event(e) => self.event(e),
                    Command::Word(chars) => {
                        if self.avatar.speaking.is_some() {
                            self.avatar.word = Some(Word {
                                at: Instant::now(),
                                secs: word_secs(chars),
                                chars,
                            });
                            self.avatar.next_inject = None;
                        }
                    }
                }
                if self.session.is_none() {
                    break;
                }
            }
            if self.session.is_none() {
                continue;
            }
            self.inject();
            let timeout = self.read_timeout(idle_wait);
            if let Some(s) = self.session.as_mut() {
                let _ = s.socket.get_ref().set_read_timeout(Some(timeout));
            }
            if self.read() {
                idle_wait = ACTIVE_READ;
            } else if !self.is_active() {
                idle_wait = (idle_wait * 2).min(IDLE_MAX);
            }
        }
    }

    fn is_active(&self) -> bool {
        !self.authenticated()
            || self.avatar.speaking.is_some()
            || self.avatar.close_mouth
            || !self.avatar.holds.is_empty()
    }

    fn read_timeout(&self, idle_wait: Duration) -> Duration {
        if !self.is_active() {
            return idle_wait;
        }
        let now = Instant::now();
        let next_inject = self
            .avatar
            .next_inject
            .map_or(Duration::ZERO, |t| t.saturating_duration_since(now));
        next_inject.clamp(Duration::from_millis(1), ACTIVE_READ)
    }

    fn configure(&mut self, config: AvatarConfig) {
        let token_changed = config.token.is_some() && config.token != self.config.token;
        if config.lip_sync.is_none() && self.avatar.speaking.take().is_some() {
            self.avatar.close_mouth = true;
        }
        self.config = config;
        if token_changed && self.session.is_some() && !self.authenticated() {
            self.authenticate();
        }
    }

    /// Keep the talking state while disconnected, so the mouth is right
    /// once the app is back.
    fn track_offline(&mut self, event: &BridgeEvent) {
        if let BridgeEvent::Speaking { speaking } = event {
            self.avatar.speaking = speaking.then(Instant::now);
        }
    }

    fn reset_expressions(&mut self) {
        for file in std::mem::take(&mut self.avatar.active) {
            self.perform(Request::Expression {
                file,
                active: false,
                fade: None,
            });
        }
    }

    fn perform(&mut self, req: Request) {
        if let Request::Expression { file, active, .. } = &req {
            if *active {
                self.avatar.active.insert(file.clone());
            } else {
                self.avatar.active.remove(file);
            }
        }
        self.send(req.call::<P>());
    }

    fn event(&mut self, event: BridgeEvent) {
        match event {
            BridgeEvent::Speaking { speaking } => {
                if speaking && self.config.lip_sync.is_some() {
                    self.avatar.speaking.get_or_insert_with(Instant::now);
                } else if !speaking && self.avatar.speaking.take().is_some() {
                    self.avatar.word = None;
                    self.avatar.close_mouth = true;
                }
            }
            BridgeEvent::LineStart { .. } if self.config.reset == VtsReset::NextLine => {
                self.reset_expressions()
            }
            BridgeEvent::LineEnd { .. } if self.config.reset == VtsReset::LineEnd => {
                self.reset_expressions()
            }
            _ if !self.authenticated() => {}
            BridgeEvent::Expression { name, .. } => {
                if let Some(hotkey) =
                    resolve_hotkey(&name, &self.config.expressions, self.hotkeys())
                {
                    self.perform(hotkey_request(&hotkey));
                }
            }
            BridgeEvent::Vts { action } => match action {
                VtsAction::Hotkey { hotkey } => {
                    let id = find_hotkey(self.hotkeys(), &hotkey).map_or(hotkey, |h| h.id.clone());
                    self.perform(Request::Hotkey(id));
                }
                VtsAction::Expression { file, active, fade } => {
                    self.perform(Request::Expression { file, active, fade })
                }
                VtsAction::Param { name, value, ms } => {
                    self.avatar.holds.retain(|h| h.name != name);
                    self.avatar.holds.push(Hold {
                        name,
                        value,
                        until: Instant::now() + Duration::from_millis(u64::from(ms)),
                    });
                    self.avatar.next_inject = None;
                }
            },
            _ => {}
        }
    }

    /// Write the mouth and held parameters when due.
    fn inject(&mut self) {
        if !self.authenticated() {
            return;
        }
        let now = Instant::now();
        let a = &mut self.avatar;
        let busy = a.speaking.is_some() || a.close_mouth || !a.holds.is_empty();
        if !busy || a.next_inject.is_some_and(|t| now < t) {
            return;
        }
        a.next_inject = Some(now + INJECT_EVERY);
        let mouth_param = self.config.lip_sync.as_ref().and_then(|lip| {
            if let Some(since) = a.speaking {
                // Closed between words; flapping again if the words stop
                // coming for longer than a pause while it still talks.
                let word = a.word.map(|w| (now.duration_since(w.at).as_secs_f32(), w));
                let value = match word {
                    Some((t, w)) if t < w.secs + 0.8 => word_mouth(t, w.secs, w.chars),
                    _ => mouth(now.duration_since(since).as_secs_f32()),
                };
                Some((lip.param.as_str(), value * lip.amp))
            } else if a.close_mouth {
                Some((lip.param.as_str(), 0.0))
            } else {
                None
            }
        });
        a.close_mouth = false;
        if mouth_param.is_none() && a.holds.is_empty() {
            return;
        }
        self.counter += 1;
        let id = format!("nt-{}", self.counter);
        let message = P::inject(&mut self.inject_buf, &id, mouth_param, &a.holds);
        a.holds.retain(|h| h.until > now);
        self.send_message(message);
    }

    fn read(&mut self) -> bool {
        let Some(s) = self.session.as_mut() else {
            return false;
        };
        match s.socket.read() {
            Ok(Message::Close(_)) => {
                self.drop_session(LinkFault::Closed);
                false
            }
            Ok(message) => {
                if let Some(v) = P::parse(message) {
                    self.response(&v);
                }
                true
            }
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                false
            }
            Err(e) => {
                self.drop_session(LinkFault::from_ws(&e));
                false
            }
        }
    }

    fn response(&mut self, v: &Value) {
        match P::reply(v, self.auth_id.as_deref()) {
            Reply::Token(t) => {
                self.config.token = Some(t.clone());
                (self.status)(BridgeStatus::Token(P::SINK, t));
                self.authenticate();
            }
            Reply::Authenticated => {
                self.auth_id = None;
                if let Some(s) = self.session.as_mut() {
                    s.authenticated = true;
                }
                (self.status)(BridgeStatus::Connected(P::SINK));
                self.send(P::list_hotkeys());
                for call in P::subscriptions() {
                    self.send(call);
                }
            }
            Reply::AuthRejected => {
                self.auth_id = None;
                if let Some(s) = self.session.as_mut() {
                    s.authenticated = false;
                }
                self.config.token = None;
                (self.status)(BridgeStatus::TokenRejected(P::SINK));
            }
            Reply::Hotkeys(hotkeys) => {
                if let Some(s) = self.session.as_mut() {
                    s.hotkeys = hotkeys.clone();
                }
                (self.status)(BridgeStatus::Hotkeys(P::SINK, hotkeys));
            }
            Reply::ModelChanged => {
                // A new model starts with its own expressions off.
                self.avatar.active.clear();
                self.send(P::list_hotkeys());
            }
            Reply::Error(msg) => {
                (self.status)(BridgeStatus::Error(P::SINK, LinkFault::Remote(msg)));
            }
            Reply::Ignore => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn smile() -> AvatarHotkey {
        AvatarHotkey {
            id: "abc".into(),
            name: "Smile".into(),
            kind: "ToggleExpression".into(),
            file: "Smile.exp3.json".into(),
        }
    }

    #[test]
    fn hotkeys_resolve_by_map_then_by_name_ignoring_case() {
        let keys = vec![smile()];
        let mut map = BTreeMap::new();
        assert_eq!(resolve_hotkey("smile", &map, &keys), Some(smile()));
        map.insert("big_grin".into(), "abc".into());
        assert_eq!(resolve_hotkey("Big Grin", &map, &keys), Some(smile()));
        map.insert("smile".into(), "override".into());
        assert_eq!(
            resolve_hotkey("SMILE", &map, &keys).map(|h| h.id),
            Some("override".into())
        );
        assert_eq!(resolve_hotkey("cry", &map, &keys), None);
    }

    #[test]
    fn expression_toggles_turn_on_instead_of_toggling() {
        assert_eq!(
            hotkey_request(&smile()),
            Request::Expression {
                file: "Smile.exp3.json".into(),
                active: true,
                fade: None
            }
        );
        let wave = AvatarHotkey {
            kind: "TriggerAnimation".into(),
            ..smile()
        };
        assert_eq!(hotkey_request(&wave), Request::Hotkey("abc".into()));
    }

    #[test]
    fn the_mouth_stays_in_range_and_moves() {
        let samples: Vec<f32> = (0..60).map(|i| mouth(i as f32 / 30.0)).collect();
        assert!(samples.iter().all(|v| (0.0..=1.0).contains(v)));
        let min = samples.iter().cloned().fold(1.0, f32::min);
        let max = samples.iter().cloned().fold(0.0, f32::max);
        assert!(max - min > 0.5, "{min}..{max}");
    }

    #[test]
    fn the_mouth_follows_a_spoken_word_and_closes_after_it() {
        let secs = word_secs(3);
        assert!(secs > 0.2 && secs < 0.5, "{secs}");
        let samples: Vec<f32> = (0..=30)
            .map(|i| word_mouth(secs * i as f32 / 30.0, secs, 3))
            .collect();
        assert!(samples.iter().all(|v| (0.0..=1.0).contains(v)));
        assert_eq!(samples[0], 0.0, "opens from closed");
        assert_eq!(*samples.last().unwrap(), 0.0, "closed when the word ends");
        let max = samples.iter().cloned().fold(0.0, f32::max);
        assert!(max > 0.7, "{max}");
        // Three syllables: it dips between them.
        let dips = samples
            .windows(3)
            .filter(|w| w[1] < w[0] && w[1] < w[2])
            .count();
        assert!(dips >= 2, "{samples:?}");
        assert_eq!(word_mouth(-0.1, secs, 3), 0.0);
        // Longer words are said longer, within reason.
        assert!(word_secs(8) > word_secs(2) && word_secs(40) <= 0.9);
    }

    #[test]
    fn mapping_keys_fold_case_and_spaces() {
        assert_eq!(mapping_key(" Big  Grin "), "big__grin");
        assert_eq!(mapping_key("smile"), "smile");
    }
}
