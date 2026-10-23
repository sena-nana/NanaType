//! Two-person, in-memory sessions. Only the owner runs a track's player;
//! the host orders durable submissions, and mirrors are replaceable state.
#![forbid(unsafe_code)]
pub mod diag;
mod protocol;
mod transport;

pub use protocol::MAX_ENTRIES as HISTORY_LIMIT;
pub use protocol::{
    Entry, Invite, MessageId, PortraitAsset, Room, TrackSnapshot, asset_references, rebase_display,
    rebase_row,
};
pub use transport::{Config, Event, PeerHandle, Resources, Status};
