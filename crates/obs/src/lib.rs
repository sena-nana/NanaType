//! A small [obs-websocket] 5.x client (built into OBS 28 and later).
//!
//! It follows OBS's program scene and reports each source placed in it with
//! its bounding box, so a Spout or Syphon output can be rendered at the size
//! OBS shows it. It can also turn a bounding box on for one source.
//!
//! Updates arrive on a thread of their own through the callback given to
//! [`ObsHandle::new`]; nothing here touches UI state.
//!
//! [obs-websocket]: https://github.com/obsproject/obs-websocket/blob/master/docs/generated/protocol.md

#![forbid(unsafe_code)]

mod client;
pub mod protocol;

#[cfg(test)]
mod tests;

pub use client::{ObsConfig, ObsEvent, ObsHandle, ObsScene, ObsStatus, ObsUpdate};
pub use protocol::{ObsItem, Transform};
