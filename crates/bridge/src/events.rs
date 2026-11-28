pub use nanatype_model::BridgeEvent;
use nanatype_model::LinkFault;

use crate::AvatarHotkey;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Sink {
    Vts,
    NanaLive,
    Osc,
    WebSocket,
}

#[derive(Debug, Clone, PartialEq)]
pub enum BridgeStatus {
    Off(Sink),
    Connecting(Sink),
    Connected(Sink),
    /// The avatar app (VTube Studio, NanaLive) is reachable but the plugin
    /// is not authorized yet.
    NeedsToken(Sink),
    /// The avatar app granted a new token; persist it.
    Token(Sink, String),
    /// The avatar app refused the stored token; forget it.
    TokenRejected(Sink),
    /// The current model's hotkeys in the avatar app.
    Hotkeys(Sink, Vec<AvatarHotkey>),
    /// WebSocket clients currently connected.
    Clients(usize),
    /// The sink failed; it retries where it can.
    Error(Sink, LinkFault),
}
