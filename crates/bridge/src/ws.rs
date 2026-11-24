//! Localhost WebSocket broadcast. Every event goes to every client as JSON:
//! `{"type":"expression","name":"smile","intensity":null}`.

use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender, after, never, select, unbounded};
use nanatype_model::LinkFault;
use tungstenite::{Message, WebSocket};

use crate::{BridgeEvent, BridgeStatus, Sink, StatusFn};

const CLIENT_POLL_MIN: Duration = Duration::from_millis(50);
const CLIENT_POLL_MAX: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WsConfig {
    pub port: u16,
}

pub(crate) struct WsServer {
    config: WsConfig,
    tx: Sender<String>,
    stopped: Arc<AtomicBool>,
    _stop: Sender<()>,
}

impl Drop for WsServer {
    fn drop(&mut self) {
        self.stopped.store(true, Ordering::Release);
        let addr = SocketAddr::from(([127, 0, 0, 1], self.config.port));
        let _ = TcpStream::connect_timeout(&addr, Duration::from_millis(50));
    }
}

impl WsServer {
    pub fn start(config: WsConfig, status: StatusFn) -> Self {
        let (tx, rx) = unbounded::<String>();
        let (stop_tx, stop_rx) = unbounded::<()>();
        let stopped = Arc::new(AtomicBool::new(false));
        let worker_stopped = Arc::clone(&stopped);
        let port = config.port;
        thread::Builder::new()
            .name("nanatype-ws".into())
            .spawn(move || run(port, rx, stop_rx, worker_stopped, status))
            .expect("spawn ws thread");
        Self {
            config,
            tx,
            stopped,
            _stop: stop_tx,
        }
    }

    pub fn config(&self) -> &WsConfig {
        &self.config
    }

    pub fn emit(&self, event: &BridgeEvent) {
        if let Ok(json) = serde_json::to_string(event) {
            let _ = self.tx.send(json);
        }
    }
}

fn run(
    port: u16,
    rx: Receiver<String>,
    stop: Receiver<()>,
    stopped: Arc<AtomicBool>,
    status: StatusFn,
) {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = match TcpListener::bind(addr) {
        Ok(l) => l,
        Err(e) => {
            let fault = if e.kind() == std::io::ErrorKind::AddrInUse {
                LinkFault::PortInUse(port)
            } else {
                LinkFault::from_io(&e)
            };
            status(BridgeStatus::Error(Sink::WebSocket, fault));
            return;
        }
    };
    let (client_tx, client_rx) = unbounded::<WebSocket<TcpStream>>();
    let accept_stopped = Arc::clone(&stopped);
    let _ = thread::Builder::new()
        .name("nanatype-ws-accept".into())
        .spawn(move || {
            while let Ok((stream, _)) = listener.accept() {
                if accept_stopped.load(Ordering::Acquire) {
                    break;
                }
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                if let Ok(ws) = tungstenite::accept(stream) {
                    let _ = ws.get_ref().set_nonblocking(true);
                    if client_tx.send(ws).is_err() {
                        break;
                    }
                }
            }
        });
    status(BridgeStatus::Connected(Sink::WebSocket));
    let mut clients: Vec<WebSocket<TcpStream>> = Vec::new();
    let mut reported = usize::MAX;
    let mut poll_wait = CLIENT_POLL_MIN;
    loop {
        if clients.len() != reported {
            reported = clients.len();
            status(BridgeStatus::Clients(reported));
        }
        let timer = if clients.is_empty() {
            never()
        } else {
            after(poll_wait)
        };
        select! {
            recv(stop) -> _ => {
                for mut c in clients {
                    let _ = c.close(None);
                }
                return;
            }
            recv(client_rx) -> res => match res {
                Ok(ws) => {
                    clients.push(ws);
                    while let Ok(ws) = client_rx.try_recv() {
                        clients.push(ws);
                    }
                    poll_wait = CLIENT_POLL_MIN;
                }
                Err(_) => return,
            },
            recv(rx) -> res => match res {
                Ok(first) => {
                    if clients.is_empty() {
                        while rx.try_recv().is_ok() {}
                        continue;
                    }
                    let mut batch = vec![first];
                    batch.extend(rx.try_iter());
                    clients.retain_mut(|c| {
                        batch
                            .iter()
                            .all(|m| c.send(Message::text(m.clone())).is_ok())
                    });
                    poll_wait = CLIENT_POLL_MIN;
                }
                Err(_) => return,
            },
            recv(timer) -> _ => {}
        }
        // Read (and discard) client frames non-blockingly so pings are answered and closes noticed.
        let mut saw_frame = false;
        clients.retain_mut(|c| match c.read() {
            Ok(Message::Close(_)) => false,
            Ok(_) => {
                saw_frame = true;
                true
            }
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) =>
            {
                true
            }
            Err(_) => false,
        });
        if saw_frame {
            poll_wait = CLIENT_POLL_MIN;
        } else {
            poll_wait = (poll_wait * 2).min(CLIENT_POLL_MAX);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clients_receive_broadcast_json() {
        let probe = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = probe.local_addr().unwrap().port();
        drop(probe);
        let server = WsServer::start(WsConfig { port }, Arc::new(|_| {}));
        let mut client = None;
        for _ in 0..50 {
            if let Ok(stream) = TcpStream::connect(("127.0.0.1", port))
                && let Ok((ws, _)) = tungstenite::client(format!("ws://127.0.0.1:{port}"), stream)
            {
                client = Some(ws);
                break;
            }
            thread::sleep(Duration::from_millis(20));
        }
        let mut client = client.expect("server accepts");
        client
            .get_ref()
            .set_read_timeout(Some(Duration::from_secs(3)))
            .unwrap();
        // The server may not have registered the client yet; send until it arrives.
        let mut got = None;
        for _ in 0..30 {
            server.emit(&BridgeEvent::Speaking { speaking: true });
            match client.read() {
                Ok(Message::Text(t)) => {
                    got = Some(t.to_string());
                    break;
                }
                _ => thread::sleep(Duration::from_millis(20)),
            }
        }
        let text = got.expect("event delivered");
        assert_eq!(text, r#"{"type":"speaking","speaking":true}"#);
    }
}
