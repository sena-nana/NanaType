//! Why a connection to another program (VTube Studio, an OSC receiver, a
//! WebSocket client) dropped or could not open.
//!
//! The clients report a [`LinkFault`] rather than a sentence, so the
//! interface can say it in the viewer's language. [`LinkFault::Other`] and
//! [`LinkFault::Remote`] keep words the program did not choose: the
//! system's message, or what the other program said.

use std::fmt;
use std::io;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LinkFault {
    /// Nothing listens at the address (the other program is not running).
    Refused,
    /// No answer in time.
    TimedOut,
    /// The other side closed or reset the connection.
    Closed,
    /// The port to listen on is taken by another program.
    PortInUse(u16),
    /// The host name does not resolve.
    Unresolved(String),
    /// The connection stayed open but nothing arrived for too long.
    Silent,
    /// The other program turned the connection down for a wrong or missing
    /// token (HTTP 401 on the handshake).
    Unauthorized,
    /// The other program refused a request, in its own words.
    Remote(String),
    /// Anything else, as the system put it.
    Other(String),
}

impl LinkFault {
    /// The fault behind an I/O error.
    pub fn from_io(e: &io::Error) -> Self {
        use io::ErrorKind as K;
        match e.kind() {
            K::ConnectionRefused => Self::Refused,
            K::TimedOut | K::WouldBlock => Self::TimedOut,
            K::ConnectionReset
            | K::ConnectionAborted
            | K::BrokenPipe
            | K::UnexpectedEof
            | K::NotConnected => Self::Closed,
            _ => Self::Other(e.to_string()),
        }
    }
}

#[cfg(feature = "ws")]
impl LinkFault {
    /// The fault behind a WebSocket error.
    pub fn from_ws(e: &tungstenite::Error) -> Self {
        match e {
            tungstenite::Error::ConnectionClosed | tungstenite::Error::AlreadyClosed => {
                Self::Closed
            }
            tungstenite::Error::Io(io) => Self::from_io(io),
            tungstenite::Error::Http(response) if response.status().as_u16() == 401 => {
                Self::Unauthorized
            }
            other => Self::Other(other.to_string()),
        }
    }

    /// The fault behind a failed WebSocket handshake on a blocking stream,
    /// where an interrupted handshake means its read timed out.
    pub fn from_handshake<R: tungstenite::handshake::HandshakeRole>(
        e: tungstenite::HandshakeError<R>,
    ) -> Self {
        match e {
            tungstenite::HandshakeError::Failure(e) => Self::from_ws(&e),
            tungstenite::HandshakeError::Interrupted(_) => Self::TimedOut,
        }
    }
}

/// A WebSocket read that timed out: nothing arrived yet.
#[cfg(feature = "ws")]
pub fn ws_idle(e: &tungstenite::Error) -> bool {
    matches!(e, tungstenite::Error::Io(io) if matches!(
        io.kind(),
        io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
    ))
}

/// Connect to the first reachable address resolved from `(host, port)`,
/// waiting up to `timeout` per address.
pub fn tcp_connect_timeout(
    host: &str,
    port: u16,
    timeout: std::time::Duration,
) -> Result<std::net::TcpStream, LinkFault> {
    use std::net::{TcpStream, ToSocketAddrs};
    let unresolved = || LinkFault::Unresolved(host.to_owned());
    let addrs: Vec<_> = (host, port)
        .to_socket_addrs()
        .map_err(|_| unresolved())?
        .collect();
    if addrs.is_empty() {
        return Err(unresolved());
    }
    let mut last_err = None;
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, timeout) {
            Ok(stream) => return Ok(stream),
            Err(e) => last_err = Some(e),
        }
    }
    Err(LinkFault::from_io(
        last_err.as_ref().expect("addrs is non-empty"),
    ))
}

/// Open a plain `ws://host:port{path}` client. The handshake reads with
/// `handshake_timeout` (none blocks); the socket then reads with
/// `read_timeout`, so a session loop can poll between frames.
#[cfg(feature = "ws")]
pub fn ws_connect(
    host: &str,
    port: u16,
    path: &str,
    handshake_timeout: Option<std::time::Duration>,
    read_timeout: std::time::Duration,
) -> Result<tungstenite::WebSocket<std::net::TcpStream>, LinkFault> {
    let io = |e: io::Error| LinkFault::from_io(&e);
    let stream = tcp_connect_timeout(host, port, std::time::Duration::from_secs(2))?;
    stream.set_read_timeout(handshake_timeout).map_err(io)?;
    let url = if host.contains(':') && !host.starts_with('[') {
        format!("ws://[{host}]:{port}{path}")
    } else {
        format!("ws://{host}:{port}{path}")
    };
    let (socket, _) = tungstenite::client(url, stream).map_err(LinkFault::from_handshake)?;
    socket
        .get_ref()
        .set_read_timeout(Some(read_timeout))
        .map_err(io)?;
    Ok(socket)
}

/// English, for logs and tests; the interface translates the fault.
impl fmt::Display for LinkFault {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Refused => f.write_str("connection refused"),
            Self::TimedOut => f.write_str("timed out"),
            Self::Closed => f.write_str("connection closed"),
            Self::PortInUse(port) => write!(f, "port {port} is in use"),
            Self::Unresolved(host) => write!(f, "cannot resolve {host}"),
            Self::Silent => f.write_str("no data for too long"),
            Self::Unauthorized => f.write_str("token rejected"),
            Self::Remote(message) | Self::Other(message) => f.write_str(message),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn io_errors_map_to_faults_the_interface_can_name() {
        let e = |k| io::Error::new(k, "x");
        assert_eq!(
            LinkFault::from_io(&e(io::ErrorKind::ConnectionRefused)),
            LinkFault::Refused
        );
        assert_eq!(
            LinkFault::from_io(&e(io::ErrorKind::TimedOut)),
            LinkFault::TimedOut
        );
        assert_eq!(
            LinkFault::from_io(&e(io::ErrorKind::ConnectionReset)),
            LinkFault::Closed
        );
        assert_eq!(
            LinkFault::from_io(&e(io::ErrorKind::PermissionDenied)),
            LinkFault::Other("x".into())
        );
    }

    #[cfg(feature = "ws")]
    #[test]
    fn ws_connect_falls_back_to_ipv4_localhost() {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind ipv4 loopback");
        let port = listener.local_addr().expect("local addr").port();
        let server = std::thread::spawn(move || {
            let (stream, _) = listener.accept().expect("accept");
            let mut ws = tungstenite::accept(stream).expect("ws accept");
            let _ = ws.close(None);
        });
        let socket = ws_connect(
            "localhost",
            port,
            "/api/chat",
            Some(std::time::Duration::from_secs(2)),
            std::time::Duration::from_millis(100),
        );
        assert!(socket.is_ok(), "ws_connect failed: {socket:?}");
        server.join().expect("server thread");
    }
}
