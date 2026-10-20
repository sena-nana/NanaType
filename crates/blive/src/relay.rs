//! A blivechat-compatible server on localhost for sources that can only push.
//!
//! A browser page cannot accept connections, so the 彗星号 workshop plugin
//! connects here instead: `ws://127.0.0.1:port/comet?token=…` publishes
//! blivechat frames (`{"cmd": n, "data": ...}`, already converted by the page),
//! and every client of `/api/chat` receives them, the same endpoint a real
//! blivechat serves. The token keeps other local pages from injecting chat.
//!
//! The same event can arrive twice when a canvas is open in two places (the
//! 彗星号 preview and an OBS browser source), so frames are passed on once per
//! message id.

use std::collections::{HashSet, VecDeque};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, RecvTimeoutError, Sender, unbounded};
use nanatype_model::LinkFault;
use serde_json::Value;
use tungstenite::handshake::server::{ErrorResponse, Request, Response};
use tungstenite::{Message, Utf8Bytes, WebSocket};

use crate::client::idle;
use crate::protocol::{CHAT_COMMANDS, HEARTBEAT, HEARTBEAT_MESSAGE, TEXT_ID, string};

/// The publisher's path; the token comes as `?token=`.
pub const PUBLISH_PATH: &str = "/comet";
/// blivechat's client endpoint.
pub const SUBSCRIBE_PATH: &str = "/api/chat";
/// Larger frames are not chat.
const MAX_FRAME: usize = 64 * 1024;
/// Message ids remembered for deduplication.
const REMEMBERED: usize = 1024;
/// blivechat clients drop a server silent for 30 s.
const HEARTBEAT_EVERY: Duration = Duration::from_secs(10);
/// How often the relay looks at its sockets.
const TICK: Duration = Duration::from_millis(15);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayConfig {
    pub port: u16,
    /// What publishers must send as `?token=`.
    pub token: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub enum RelayStatus {
    #[default]
    Off,
    /// The port is open, with this many pages pushing to it.
    Listening { publishers: usize },
    /// The port could not be opened.
    Error(LinkFault),
}

type StatusFn = Arc<dyn Fn(RelayStatus) + Send + Sync>;

struct Running {
    config: RelayConfig,
    stop: Sender<()>,
    thread: thread::JoinHandle<()>,
}

/// Owns the relay thread. Dropping it (or `set(None)`) closes the port.
pub struct RelayHandle {
    status: StatusFn,
    running: Option<Running>,
}

impl RelayHandle {
    pub fn new(status: impl Fn(RelayStatus) + Send + Sync + 'static) -> Self {
        Self {
            status: Arc::new(status),
            running: None,
        }
    }

    /// Listen with `config`, restart after a change, or stop on `None`. The
    /// port is open when this returns, so a client can join at once.
    pub fn set(&mut self, config: Option<RelayConfig>) {
        if self.running.as_ref().map(|r| &r.config) == config.as_ref() {
            return;
        }
        if let Some(old) = self.running.take() {
            // Wait for the port to close, so the next relay can bind it.
            drop(old.stop);
            let _ = old.thread.join();
        }
        let Some(config) = config else {
            (self.status)(RelayStatus::Off);
            return;
        };
        let listener = match bind(config.port) {
            Ok(listener) => listener,
            // Not kept: the next `set` with this configuration tries again.
            Err(fault) => return (self.status)(RelayStatus::Error(fault)),
        };
        (self.status)(RelayStatus::Listening { publishers: 0 });
        let (stop, stop_rx) = unbounded::<()>();
        let token = config.token.clone();
        let status = Arc::clone(&self.status);
        let thread = thread::Builder::new()
            .name("nanatype-comet-relay".into())
            .spawn(move || run(listener, &token, &stop_rx, &*status))
            .expect("spawn relay thread");
        self.running = Some(Running {
            config,
            stop,
            thread,
        });
    }
}

/// The address a 彗星号 page connects to.
pub fn publish_url(port: u16, token: &str) -> String {
    format!("ws://127.0.0.1:{port}{PUBLISH_PATH}?token={token}")
}

fn bind(port: u16) -> Result<TcpListener, LinkFault> {
    let listener = TcpListener::bind(SocketAddr::from(([127, 0, 0, 1], port))).map_err(|e| {
        if e.kind() == std::io::ErrorKind::AddrInUse {
            LinkFault::PortInUse(port)
        } else {
            LinkFault::from_io(&e)
        }
    })?;
    listener
        .set_nonblocking(true)
        .map_err(|e| LinkFault::from_io(&e))?;
    Ok(listener)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Role {
    Publisher,
    Subscriber,
}

struct Peer {
    ws: WebSocket<TcpStream>,
    role: Role,
}

/// Which endpoint a handshake asks for; `Err` is the refusal's status and text.
fn route(request: &Request, token: &str) -> Result<Role, (u16, &'static str)> {
    let uri = request.uri();
    match uri.path() {
        SUBSCRIBE_PATH => Ok(Role::Subscriber),
        PUBLISH_PATH => {
            let given = uri
                .query()
                .unwrap_or_default()
                .split('&')
                .find_map(|pair| pair.strip_prefix("token="));
            if !token.is_empty() && given == Some(token) {
                Ok(Role::Publisher)
            } else {
                Err((401, "wrong token"))
            }
        }
        _ => Err((404, "not a relay endpoint")),
    }
}

// tungstenite's callback answers a refusal with a whole response.
#[allow(clippy::result_large_err)]
fn accept(stream: TcpStream, token: &str) -> Option<Peer> {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(1)));
    let mut role = None;
    let ws = tungstenite::accept_hdr(stream, |request: &Request, response: Response| match route(
        request, token,
    ) {
        Ok(r) => {
            role = Some(r);
            Ok(response)
        }
        Err((status, why)) => {
            let mut refusal = ErrorResponse::new(Some(why.to_owned()));
            *refusal.status_mut() = tungstenite::http::StatusCode::from_u16(status)
                .unwrap_or(tungstenite::http::StatusCode::BAD_REQUEST);
            Err(refusal)
        }
    })
    .ok()?;
    let _ = ws
        .get_ref()
        .set_read_timeout(Some(Duration::from_millis(1)));
    Some(Peer { ws, role: role? })
}

/// Remembers the last [`REMEMBERED`] message ids.
#[derive(Default)]
struct Seen {
    order: VecDeque<String>,
    ids: HashSet<String>,
}

impl Seen {
    /// `true` the first time `key` is offered.
    fn first(&mut self, key: String) -> bool {
        if self.ids.contains(&key) {
            return false;
        }
        if self.order.len() == REMEMBERED
            && let Some(old) = self.order.pop_front()
        {
            self.ids.remove(&old);
        }
        self.ids.insert(key.clone());
        self.order.push_back(key);
        true
    }
}

/// What to do with a publisher's frame.
#[derive(Debug, PartialEq, Eq)]
enum Published {
    Forward,
    Heartbeat,
    Drop,
}

fn classify(text: &str, seen: &mut Seen) -> Published {
    if text.len() > MAX_FRAME {
        return Published::Drop;
    }
    let Ok(frame) = serde_json::from_str::<Value>(text) else {
        return Published::Drop;
    };
    match frame["cmd"].as_i64() {
        Some(HEARTBEAT) => Published::Heartbeat,
        Some(cmd) if CHAT_COMMANDS.contains(&cmd) => {
            let data = &frame["data"];
            let id = string(if cmd == crate::protocol::ADD_TEXT {
                &data[TEXT_ID]
            } else {
                &data["id"]
            });
            if id.is_empty() || seen.first(format!("{cmd}:{id}")) {
                Published::Forward
            } else {
                Published::Drop
            }
        }
        _ => Published::Drop,
    }
}

fn run(listener: TcpListener, token: &str, stop: &Receiver<()>, status: &dyn Fn(RelayStatus)) {
    let mut peers: Vec<Peer> = Vec::new();
    let mut seen = Seen::default();
    let mut reported = 0;
    let mut last_beat = Instant::now();
    let heartbeat = Utf8Bytes::from_static(HEARTBEAT_MESSAGE);
    loop {
        match stop.recv_timeout(TICK) {
            Err(RecvTimeoutError::Timeout) => {}
            _ => {
                for mut p in peers {
                    let _ = p.ws.close(None);
                    let _ = p.ws.flush();
                }
                return;
            }
        }
        while let Ok((stream, _)) = listener.accept() {
            peers.extend(accept(stream, token));
        }
        let mut outgoing: Vec<Utf8Bytes> = Vec::new();
        peers.retain_mut(|p| {
            loop {
                match p.ws.read() {
                    Ok(Message::Text(text)) if p.role == Role::Publisher => {
                        match classify(text.as_str(), &mut seen) {
                            Published::Forward => outgoing.push(text),
                            Published::Heartbeat => {
                                if p.ws.send(Message::Text(heartbeat.clone())).is_err() {
                                    return false;
                                }
                            }
                            Published::Drop => {}
                        }
                    }
                    // Subscribers send a join and heartbeats; the relay serves
                    // every room the same feed.
                    Ok(Message::Close(_)) => return false,
                    Ok(_) => {}
                    Err(e) if idle(&e) => return true,
                    Err(_) => return false,
                }
            }
        });
        if last_beat.elapsed() >= HEARTBEAT_EVERY {
            last_beat = Instant::now();
            outgoing.push(heartbeat.clone());
        }
        if !outgoing.is_empty() {
            peers.retain_mut(|p| {
                p.role != Role::Subscriber
                    || outgoing
                        .iter()
                        .all(|m| p.ws.send(Message::Text(m.clone())).is_ok())
            });
        }
        let publishers = peers.iter().filter(|p| p.role == Role::Publisher).count();
        if publishers != reported {
            reported = publishers;
            status(RelayStatus::Listening { publishers });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::sync::mpsc;

    #[test]
    fn frames_are_forwarded_once_per_message_id() {
        let mut seen = Seen::default();
        let text =
            json!({"cmd": 2, "data": ["", 1, "a", 0, "hi", 0, 0, 0, 0, 0, 0, "m1"]}).to_string();
        assert_eq!(classify(&text, &mut seen), Published::Forward);
        assert_eq!(classify(&text, &mut seen), Published::Drop);
        let gift = json!({"cmd": 3, "data": {"id": "m1"}}).to_string();
        assert_eq!(
            classify(&gift, &mut seen),
            Published::Forward,
            "ids are per command"
        );
        assert_eq!(classify(HEARTBEAT_MESSAGE, &mut seen), Published::Heartbeat);
        assert_eq!(
            classify(r#"{"cmd":8,"data":{}}"#, &mut seen),
            Published::Drop
        );
        assert_eq!(
            classify(r#"{"cmd":1,"data":{}}"#, &mut seen),
            Published::Drop
        );
        assert_eq!(classify("nope", &mut seen), Published::Drop);
        let big = format!(r#"{{"cmd":2,"data":["{}"]}}"#, "x".repeat(MAX_FRAME));
        assert_eq!(classify(&big, &mut seen), Published::Drop);
    }

    #[test]
    fn old_ids_are_forgotten() {
        let mut seen = Seen::default();
        for i in 0..=REMEMBERED {
            assert!(seen.first(i.to_string()));
        }
        assert!(seen.first("0".into()), "the oldest id fell out");
        assert!(!seen.first(REMEMBERED.to_string()));
    }

    fn free_port() -> u16 {
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        probe.local_addr().unwrap().port()
    }

    fn relay() -> (RelayHandle, mpsc::Receiver<RelayStatus>) {
        let (tx, rx) = mpsc::channel();
        (
            RelayHandle::new(move |s| {
                let _ = tx.send(s);
            }),
            rx,
        )
    }

    fn connect(port: u16, path: &str) -> Result<WebSocket<TcpStream>, tungstenite::Error> {
        let stream = TcpStream::connect(("127.0.0.1", port))?;
        match tungstenite::client(format!("ws://127.0.0.1:{port}{path}"), stream) {
            Ok((ws, _)) => {
                ws.get_ref()
                    .set_read_timeout(Some(Duration::from_secs(3)))
                    .unwrap();
                Ok(ws)
            }
            Err(tungstenite::HandshakeError::Failure(e)) => Err(e),
            Err(tungstenite::HandshakeError::Interrupted(_)) => {
                panic!("blocking handshake interrupted")
            }
        }
    }

    fn read_chat(ws: &mut WebSocket<TcpStream>) -> String {
        loop {
            let text = ws.read().unwrap().into_text().unwrap().to_string();
            if text != HEARTBEAT_MESSAGE {
                return text;
            }
        }
    }

    #[test]
    fn publishers_need_the_token_and_reach_every_subscriber() {
        let port = free_port();
        let (mut relay, rx) = relay();
        relay.set(Some(RelayConfig {
            port,
            token: "secret".into(),
        }));
        assert_eq!(rx.recv().unwrap(), RelayStatus::Listening { publishers: 0 });
        assert!(publish_url(port, "secret").ends_with("/comet?token=secret"));

        // The port is open as soon as `set` returns.
        let mut reader = connect(port, SUBSCRIBE_PATH).expect("subscribe");
        match connect(port, "/comet?token=wrong") {
            Err(tungstenite::Error::Http(response)) => assert_eq!(response.status(), 401),
            other => panic!("wrong token accepted: {:?}", other.map(|_| ())),
        }
        assert!(matches!(
            connect(port, "/other"),
            Err(tungstenite::Error::Http(r)) if r.status() == 404
        ));
        let mut page = connect(port, "/comet?token=secret").expect("publish");
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            RelayStatus::Listening { publishers: 1 }
        );
        page.send(Message::text(HEARTBEAT_MESSAGE)).unwrap();
        assert_eq!(page.read().unwrap().into_text().unwrap(), HEARTBEAT_MESSAGE);

        let frame =
            json!({"cmd": 5, "data": {"id": "sc-1", "price": 30, "content": "加油"}}).to_string();
        let later = json!({"cmd": 5, "data": {"id": "sc-2"}}).to_string();
        for f in [&frame, &frame, &later] {
            page.send(Message::text(f.as_str())).unwrap();
        }
        assert_eq!(read_chat(&mut reader), frame);
        assert_eq!(
            read_chat(&mut reader),
            later,
            "the repeated frame was dropped"
        );

        relay.set(None);
        assert!(rx.iter().any(|s| s == RelayStatus::Off));
    }

    #[test]
    fn a_new_token_reopens_the_same_port() {
        let port = free_port();
        let (mut relay, _rx) = relay();
        for token in ["first", "second"] {
            relay.set(Some(RelayConfig {
                port,
                token: token.into(),
            }));
        }
        assert!(connect(port, "/comet?token=second").is_ok());
        assert!(connect(port, "/comet?token=first").is_err());
    }

    #[test]
    fn a_taken_port_is_reported() {
        let taken = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = taken.local_addr().unwrap().port();
        let (mut relay, rx) = relay();
        relay.set(Some(RelayConfig {
            port,
            token: "t".into(),
        }));
        assert_eq!(
            rx.recv().unwrap(),
            RelayStatus::Error(LinkFault::PortInUse(port))
        );
    }
}
