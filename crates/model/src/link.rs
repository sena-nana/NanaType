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
}
