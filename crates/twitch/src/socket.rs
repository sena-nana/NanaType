//! One anonymous IRC connection to one channel.
use crate::{
    Event, Room,
    irc::{self, Incoming},
};
use std::{
    net::{TcpStream, ToSocketAddrs},
    time::{Duration, Instant},
};
use tungstenite::{Message, WebSocket, client::IntoClientRequest, stream::MaybeTlsStream};

type Socket = WebSocket<MaybeTlsStream<TcpStream>>;

const PING_EVERY: Duration = Duration::from_secs(60);
const SILENCE: Duration = Duration::from_secs(90);
const JOIN_DEADLINE: Duration = Duration::from_secs(15);

fn connect(endpoint: &str) -> Result<Socket, String> {
    let request = endpoint
        .into_client_request()
        .map_err(|_| "invalid-endpoint")?;
    let host = request.uri().host().ok_or("invalid-endpoint")?.to_owned();
    let port = request
        .uri()
        .port_u16()
        .unwrap_or(if request.uri().scheme_str() == Some("wss") {
            443
        } else {
            80
        });
    let addresses = (host.as_str(), port)
        .to_socket_addrs()
        .map_err(|_| "transport-connect-failed")?;
    let stream = addresses
        .take(4)
        .find_map(|a| TcpStream::connect_timeout(&a, Duration::from_secs(3)).ok())
        .ok_or("transport-connect-failed")?;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(5)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
    let config = tungstenite::protocol::WebSocketConfig::default()
        .max_message_size(Some(1 << 20))
        .max_frame_size(Some(1 << 20));
    let (socket, _) = tungstenite::client_tls_with_config(request, stream, Some(config), None)
        .map_err(|_| "transport-connect-failed")?;
    let tcp = match socket.get_ref() {
        MaybeTlsStream::Plain(stream) => Some(stream),
        MaybeTlsStream::Rustls(stream) => Some(&stream.sock),
        _ => None,
    };
    if let Some(tcp) = tcp {
        let _ = tcp.set_read_timeout(Some(Duration::from_millis(100)));
        let _ = tcp.set_write_timeout(Some(Duration::from_secs(2)));
    }
    Ok(socket)
}

fn idle(error: &tungstenite::Error) -> bool {
    matches!(error, tungstenite::Error::Io(e) if matches!(e.kind(), std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut))
}

fn say(socket: &mut Socket, line: &str) -> Result<(), String> {
    socket
        .send(Message::text(line))
        .map_err(|_| "disconnected".into())
}

/// Join `channel` and deliver its chat until the connection ends. `Ok` means
/// Twitch asked us to reconnect.
pub(crate) fn run(
    endpoint: &str,
    channel: &str,
    current: &dyn Fn() -> bool,
    joined: &mut dyn FnMut(Room),
    emit: &dyn Fn(Event),
) -> Result<(), String> {
    let mut socket = connect(endpoint)?;
    say(&mut socket, "CAP REQ :twitch.tv/tags twitch.tv/commands")?;
    say(&mut socket, &format!("NICK {}", irc::anonymous_nick()))?;
    say(&mut socket, &format!("JOIN #{channel}"))?;
    let started = Instant::now();
    let mut heard = Instant::now();
    let mut pinged = Instant::now();
    let mut room = false;
    while current() {
        if pinged.elapsed() >= PING_EVERY {
            say(&mut socket, "PING :nanatype")?;
            pinged = Instant::now();
        }
        if heard.elapsed() >= SILENCE {
            return Err("heartbeat-timeout".into());
        }
        if !room && started.elapsed() >= JOIN_DEADLINE {
            return Err("channel-unavailable".into());
        }
        let text = match socket.read() {
            Ok(Message::Text(text)) => text,
            Ok(Message::Close(_)) => return Err("disconnected".into()),
            Ok(_) => continue,
            Err(error) if idle(&error) => continue,
            Err(_) => return Err("disconnected".into()),
        };
        heard = Instant::now();
        for raw in text.split("\r\n").filter(|l| !l.is_empty()) {
            let Some(line) = irc::parse(raw) else {
                continue;
            };
            match irc::interpret(&line, channel) {
                Incoming::Ping(token) => say(&mut socket, &format!("PONG :{token}"))?,
                Incoming::Reconnect => return Ok(()),
                Incoming::Joined(r) => {
                    room = true;
                    joined(r);
                }
                Incoming::Event(event) if room => emit(event),
                Incoming::Refused(code) => return Err(code),
                _ => {}
            }
        }
    }
    let _ = socket.close(None);
    Ok(())
}
