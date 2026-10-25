//! In-memory sessions of up to eight people. Each participant publishes the
//! very view its own display and history windows draw (one track, latest
//! wins); the host only orders history rows as they first appear in their
//! owner's track.
#![forbid(unsafe_code)]
pub mod diag;
pub mod link;
mod picture;
mod protocol;
mod transport;
pub mod wire;

pub use link::{Carrier, Inbound, Outbound, Pipe, RouteSnap};
pub use protocol::MAX_ENTRIES as HISTORY_LIMIT;
pub use protocol::{
    Entry, Invite, MAX_MEMBERS, PASSWORD_MAX, PortraitAsset, Room, TrackSnapshot, asset_references,
    rebase_display, rebase_row, room_password, session_picture,
};
pub use transport::{Config, Event, PeerHandle, Resources, Status};
pub use wire::{Assembler, Packet, chunks};
