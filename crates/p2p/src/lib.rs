//! Two-person, in-memory sessions. Each participant publishes the very view
//! its own display and history windows draw (one track, latest wins); the
//! host only orders history rows as they first appear in their owner's track.
#![forbid(unsafe_code)]
pub mod diag;
mod protocol;
mod transport;

pub use protocol::MAX_ENTRIES as HISTORY_LIMIT;
pub use protocol::{
    Entry, Invite, PortraitAsset, Room, TrackSnapshot, asset_references, rebase_display, rebase_row,
};
pub use transport::{Config, Event, PeerHandle, Resources, Status};
