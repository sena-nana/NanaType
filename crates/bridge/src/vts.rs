//! VTube Studio public API client (`ws://127.0.0.1:<port>`).
//!
//! Flow: connect → authenticate with the stored token, or request one (VTube
//! Studio shows a confirmation popup) → list the current model's hotkeys and
//! subscribe to model changes → act on markers as they are reached:
//!
//! * `[expr=name]` turns on the expression of the hotkey it maps to (or
//!   presses the hotkey when it is not an expression toggle);
//! * `[vts ...]` presses hotkeys, turns expression files on or off and holds
//!   parameters;
//! * while the speaker is talking a mouth parameter is driven (lip sync).
//!
//! Expressions turned on during a line are turned off again when it leaves
//! the box (or when the next line starts). Reconnects every few seconds while
//! VTube Studio is closed.

use std::collections::{BTreeMap, BTreeSet};
use std::net::{SocketAddr, TcpStream};
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};
use nanatype_model::{LinkFault, VtsAction, VtsReset};
use serde_json::{Value, json};
use tungstenite::{Message, WebSocket};

use crate::{BridgeEvent, BridgeStatus, Sink, StatusFn};

pub const PLUGIN_NAME: &str = "NanaType";
pub const PLUGIN_DEVELOPER: &str = "sena-nana";

/// How often parameters are injected while the mouth moves or a value is
/// held. VTube Studio hands a parameter back to tracking a second after the
/// last injection.
const INJECT_EVERY: Duration = Duration::from_millis(33);

/// Drive a mouth parameter while the speaker is talking.
#[derive(Debug, Clone, PartialEq)]
pub struct LipSync {
    pub param: String,
    /// Peak value of the mouth parameter (`MouthOpen` runs 0..1).
    pub amp: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VtsConfig {
    pub port: u16,
    pub token: Option<String>,
    /// Expression name → hotkey id. Unmapped names match a hotkey by name.
    pub expressions: BTreeMap<String, String>,
    pub lip_sync: Option<LipSync>,
    pub reset: VtsReset,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VtsHotkey {
    pub id: String,
    pub name: String,
    pub kind: String,
    /// The expression or animation file the hotkey plays, if any.
    pub file: String,
}

enum Command {
    Event(BridgeEvent),
    Config(VtsConfig),
    RequestToken,
    RefreshHotkeys,
}

pub(crate) struct VtsClient {
    config: VtsConfig,
    tx: Sender<Command>,
    _stop: Sender<()>,
}

pub(crate) fn request(kind: &str, id: &str, data: Value) -> String {
    json!({
        "apiName": "VTubeStudioPublicAPI",
        "apiVersion": "1.0",
        "requestID": id,
        "messageType": kind,
        "data": data,
    })
    .to_string()
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
    hotkeys: &[VtsHotkey],
) -> Option<VtsHotkey> {
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
                .unwrap_or_else(|| VtsHotkey {
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
fn find_hotkey<'a>(hotkeys: &'a [VtsHotkey], name_or_id: &str) -> Option<&'a VtsHotkey> {
    let key = mapping_key(name_or_id);
    hotkeys
        .iter()
        .find(|h| h.id == name_or_id || mapping_key(&h.name) == key)
}

/// What firing a hotkey should send: an expression toggle becomes a plain
/// activation, so firing it twice does not turn it off and it can be reset.
fn hotkey_request(hotkey: &VtsHotkey) -> Request {
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
enum Request {
    Hotkey(String),
    Expression {
        file: String,
        active: bool,
        fade: Option<f32>,
    },
}

impl Request {
    /// The API message type and its data.
    fn call(&self) -> (&'static str, Value) {
        match self {
            Self::Hotkey(hotkey) => ("HotkeyTriggerRequest", json!({ "hotkeyID": hotkey })),
            Self::Expression { file, active, fade } => {
                let mut data = json!({ "expressionFile": file, "active": active });
                if let Some(f) = fade {
                    data["fadeTime"] = json!(f);
                }
                ("ExpressionActivationRequest", data)
            }
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

/// A parameter held by `[vts param=...]` until `until`.
#[derive(Debug, Clone)]
struct Hold {
    name: String,
    value: f32,
    until: Instant,
}

/// The client's state across connections: what the markers asked for.
#[derive(Debug, Default)]
struct Avatar {
    /// Expression files this client turned on and has not turned off.
    active: BTreeSet<String>,
    /// Talking since.
    speaking: Option<Instant>,
    /// The mouth needs one last injection to close.
    close_mouth: bool,
    holds: Vec<Hold>,
    next_inject: Option<Instant>,
}

struct Session {
    socket: WebSocket<TcpStream>,
    authenticated: bool,
    hotkeys: Vec<VtsHotkey>,
}

impl VtsClient {
    pub fn start(config: VtsConfig, status: StatusFn) -> Self {
        let (tx, rx) = unbounded();
        let (stop_tx, stop_rx) = unbounded::<()>();
        let cfg = config.clone();
        thread::Builder::new()
            .name("nanatype-vts".into())
            .spawn(move || Client::new(cfg, status).run(rx, stop_rx))
            .expect("spawn vts thread");
        Self {
            config,
            tx,
            _stop: stop_tx,
        }
    }

    pub fn port(&self) -> u16 {
        self.config.port
    }

    /// Apply a new mapping, token or lip sync setting without reconnecting.
    pub fn update(&mut self, config: VtsConfig) {
        if config != self.config {
            self.config = config.clone();
            let _ = self.tx.send(Command::Config(config));
        }
    }

    pub fn emit(&self, event: &BridgeEvent) {
        if !matches!(
            event,
            BridgeEvent::Portrait { .. }
                | BridgeEvent::Shake { .. }
                | BridgeEvent::Translation { .. }
        ) {
            let _ = self.tx.send(Command::Event(event.clone()));
        }
    }

    pub fn request_token(&self) {
        let _ = self.tx.send(Command::RequestToken);
    }

    pub fn refresh_hotkeys(&self) {
        let _ = self.tx.send(Command::RefreshHotkeys);
    }
}

fn connect(port: u16) -> Result<WebSocket<TcpStream>, LinkFault> {
    let io = |e: std::io::Error| LinkFault::from_io(&e);
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).map_err(io)?;
    // Bound the handshake too: a port held by something else must not hang
    // the client.
    let timeout = Some(Duration::from_secs(2));
    stream.set_read_timeout(timeout).map_err(io)?;
    stream.set_write_timeout(timeout).map_err(io)?;
    let (socket, _) =
        tungstenite::client(format!("ws://{addr}"), stream).map_err(LinkFault::from_handshake)?;
    // Short reads keep events (and the mouth) on time.
    socket
        .get_ref()
        .set_read_timeout(Some(Duration::from_millis(10)))
        .map_err(io)?;
    Ok(socket)
}

struct Client {
    config: VtsConfig,
    status: StatusFn,
    session: Option<Session>,
    avatar: Avatar,
    next_attempt: Instant,
    /// Asked for while disconnected; sent once connected.
    want_token: bool,
    counter: u64,
}

impl Client {
    fn new(config: VtsConfig, status: StatusFn) -> Self {
        Self {
            config,
            status,
            session: None,
            avatar: Avatar::default(),
            next_attempt: Instant::now(),
            want_token: false,
            counter: 0,
        }
    }

    /// Send on the open session; a failed send drops it.
    fn send(&mut self, kind: &str, data: Value) {
        self.counter += 1;
        let text = request(kind, &format!("nt-{}", self.counter), data);
        let Some(s) = self.session.as_mut() else {
            return;
        };
        if let Err(e) = s.socket.send(Message::text(text)) {
            self.drop_session(LinkFault::from_ws(&e));
        }
    }

    /// The connection is gone: say why and try again shortly.
    fn drop_session(&mut self, why: LinkFault) {
        self.session = None;
        (self.status)(BridgeStatus::Error(Sink::Vts, why));
        self.next_attempt = Instant::now() + Duration::from_secs(3);
    }

    fn authenticated(&self) -> bool {
        self.session.as_ref().is_some_and(|s| s.authenticated)
    }

    fn hotkeys(&self) -> &[VtsHotkey] {
        self.session.as_ref().map_or(&[], |s| &s.hotkeys)
    }

    fn authenticate(&mut self) {
        if let Some(t) = self.config.token.clone() {
            self.send(
                "AuthenticationRequest",
                json!({
                    "pluginName": PLUGIN_NAME,
                    "pluginDeveloper": PLUGIN_DEVELOPER,
                    "authenticationToken": t,
                }),
            );
        } else {
            (self.status)(BridgeStatus::VtsNeedsToken);
        }
    }

    fn request_token(&mut self) {
        self.send(
            "AuthenticationTokenRequest",
            json!({ "pluginName": PLUGIN_NAME, "pluginDeveloper": PLUGIN_DEVELOPER }),
        );
    }

    fn run(mut self, rx: Receiver<Command>, stop: Receiver<()>) {
        loop {
            if stop.try_recv() != Err(crossbeam_channel::TryRecvError::Empty) {
                if let Some(mut s) = self.session.take() {
                    let _ = s.socket.close(None);
                    let _ = s.socket.flush();
                }
                return;
            }
            if self.session.is_none() {
                if Instant::now() < self.next_attempt {
                    // Events are dropped while disconnected; requests wait.
                    if let Ok(cmd) = rx.recv_timeout(Duration::from_millis(200)) {
                        match cmd {
                            Command::RequestToken => {
                                self.want_token = true;
                                self.next_attempt = Instant::now();
                            }
                            Command::RefreshHotkeys => self.next_attempt = Instant::now(),
                            Command::Config(c) => self.configure(c),
                            Command::Event(e) => self.track_offline(&e),
                        }
                    }
                    continue;
                }
                (self.status)(BridgeStatus::Connecting(Sink::Vts));
                match connect(self.config.port) {
                    Ok(socket) => {
                        self.session = Some(Session {
                            socket,
                            authenticated: false,
                            hotkeys: Vec::new(),
                        });
                        if self.want_token {
                            self.want_token = false;
                            self.request_token();
                        } else {
                            self.authenticate();
                        }
                    }
                    Err(e) => {
                        (self.status)(BridgeStatus::Error(Sink::Vts, e));
                        self.next_attempt = Instant::now() + Duration::from_secs(5);
                        continue;
                    }
                }
            }
            while let Ok(cmd) = rx.try_recv() {
                match cmd {
                    Command::RequestToken => self.request_token(),
                    Command::RefreshHotkeys if self.authenticated() => {
                        self.send("HotkeysInCurrentModelRequest", json!({}))
                    }
                    Command::RefreshHotkeys => {}
                    Command::Config(c) => self.configure(c),
                    Command::Event(e) => self.event(e),
                }
                if self.session.is_none() {
                    break;
                }
            }
            self.inject();
            self.read();
        }
    }

    fn configure(&mut self, config: VtsConfig) {
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
    /// once VTube Studio is back.
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
        let (kind, data) = req.call();
        self.send(kind, data);
    }

    fn event(&mut self, event: BridgeEvent) {
        match event {
            BridgeEvent::Speaking { speaking } => {
                if speaking && self.config.lip_sync.is_some() {
                    self.avatar.speaking.get_or_insert_with(Instant::now);
                } else if !speaking && self.avatar.speaking.take().is_some() {
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

    /// Inject the mouth and held parameters when due.
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
        let mut values = Vec::new();
        if let Some(lip) = &self.config.lip_sync {
            if let Some(since) = a.speaking {
                let t = now.duration_since(since).as_secs_f32();
                values.push(json!({ "id": lip.param, "value": mouth(t) * lip.amp }));
            } else if a.close_mouth {
                values.push(json!({ "id": lip.param, "value": 0.0 }));
            }
        }
        a.close_mouth = false;
        // Every hold is sent at least once, even a `ms=0` one.
        for h in &a.holds {
            values.push(json!({ "id": h.name, "value": h.value }));
        }
        a.holds.retain(|h| h.until > now);
        if !values.is_empty() {
            self.send(
                "InjectParameterDataRequest",
                json!({ "faceFound": false, "mode": "set", "parameterValues": values }),
            );
        }
    }

    fn read(&mut self) {
        let Some(s) = self.session.as_mut() else {
            return;
        };
        match s.socket.read() {
            Ok(Message::Text(text)) => {
                if let Ok(v) = serde_json::from_str::<Value>(&text) {
                    self.response(&v);
                }
            }
            Ok(Message::Close(_)) => self.drop_session(LinkFault::Closed),
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(e) => self.drop_session(LinkFault::from_ws(&e)),
        }
    }

    fn response(&mut self, v: &Value) {
        let data = &v["data"];
        match v["messageType"].as_str().unwrap_or_default() {
            "AuthenticationTokenResponse" => {
                if let Some(t) = data["authenticationToken"].as_str() {
                    self.config.token = Some(t.to_owned());
                    (self.status)(BridgeStatus::VtsToken(t.to_owned()));
                    self.authenticate();
                }
            }
            "AuthenticationResponse" => {
                let ok = data["authenticated"].as_bool().unwrap_or(false);
                if let Some(s) = self.session.as_mut() {
                    s.authenticated = ok;
                }
                if ok {
                    (self.status)(BridgeStatus::Connected(Sink::Vts));
                    self.send("HotkeysInCurrentModelRequest", json!({}));
                    self.send(
                        "EventSubscriptionRequest",
                        json!({ "eventName": "ModelLoadedEvent", "subscribe": true }),
                    );
                } else {
                    // Revoked in VTube Studio, or from another install.
                    self.config.token = None;
                    (self.status)(BridgeStatus::VtsTokenRejected);
                }
            }
            "HotkeysInCurrentModelResponse" => {
                let hotkeys = parse_hotkeys(data);
                if let Some(s) = self.session.as_mut() {
                    s.hotkeys = hotkeys.clone();
                }
                (self.status)(BridgeStatus::VtsHotkeys(hotkeys));
            }
            "ModelLoadedEvent" => {
                // A new model starts with its own expressions off.
                self.avatar.active.clear();
                self.send("HotkeysInCurrentModelRequest", json!({}));
            }
            "APIError" => {
                // VTube Studio's own words; empty when it gave none.
                let msg = data["message"].as_str().unwrap_or_default().to_owned();
                (self.status)(BridgeStatus::Error(Sink::Vts, LinkFault::Remote(msg)));
            }
            _ => {}
        }
    }
}

fn parse_hotkeys(data: &Value) -> Vec<VtsHotkey> {
    let text = |h: &Value, k: &str| h[k].as_str().unwrap_or_default().to_owned();
    data["availableHotkeys"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(|h| VtsHotkey {
                    id: text(h, "hotkeyID"),
                    name: text(h, "name"),
                    kind: text(h, "type"),
                    file: text(h, "file"),
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn smile() -> VtsHotkey {
        VtsHotkey {
            id: "abc".into(),
            name: "Smile".into(),
            kind: "ToggleExpression".into(),
            file: "Smile.exp3.json".into(),
        }
    }

    #[test]
    fn requests_carry_the_api_envelope() {
        let r: Value = serde_json::from_str(&request(
            "HotkeyTriggerRequest",
            "1",
            json!({"hotkeyID": "h"}),
        ))
        .unwrap();
        assert_eq!(r["apiName"], "VTubeStudioPublicAPI");
        assert_eq!(r["messageType"], "HotkeyTriggerRequest");
        assert_eq!(r["data"]["hotkeyID"], "h");
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
        let wave = VtsHotkey {
            kind: "TriggerAnimation".into(),
            ..smile()
        };
        assert_eq!(hotkey_request(&wave), Request::Hotkey("abc".into()));
        let (kind, data) = Request::Expression {
            file: "a.exp3.json".into(),
            active: false,
            fade: Some(0.5),
        }
        .call();
        assert_eq!(kind, "ExpressionActivationRequest");
        assert_eq!(data["active"], false);
        assert_eq!(data["fadeTime"], 0.5);
    }

    #[test]
    fn hotkey_lists_keep_their_files() {
        let data = json!({ "availableHotkeys": [
            { "hotkeyID": "abc", "name": "Smile", "type": "ToggleExpression", "file": "Smile.exp3.json" },
            { "hotkeyID": "def", "name": "Wave", "type": "TriggerAnimation" },
        ]});
        let keys = parse_hotkeys(&data);
        assert_eq!(keys[0], smile());
        assert_eq!(keys[1].file, "");
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
    fn mapping_keys_fold_case_and_spaces() {
        assert_eq!(mapping_key(" Big  Grin "), "big__grin");
        assert_eq!(mapping_key("smile"), "smile");
    }
}
