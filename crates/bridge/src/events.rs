pub use nanatype_model::BridgeEvent;
use nanatype_model::LinkFault;

use crate::VtsHotkey;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sink {
    Vts,
    Osc,
    WebSocket,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BridgeStatus {
    Off(Sink),
    Connecting(Sink),
    Connected(Sink),
    /// VTube Studio is reachable but the plugin is not authorized yet.
    VtsNeedsToken,
    /// A new token was granted; persist it.
    VtsToken(String),
    /// VTube Studio refused the stored token; forget it.
    VtsTokenRejected,
    VtsHotkeys(Vec<VtsHotkey>),
    /// WebSocket clients currently connected.
    Clients(usize),
    /// The sink failed; it retries where it can.
    Error(Sink, LinkFault),
}
