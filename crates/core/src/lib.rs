//! The session: what the editor holds, what the typewriter shows, what the
//! history lists, and how sending, continuing and recalling move between them.
//!
//! [`Core`] is a reducer. The host feeds it [`Cmd`]s with a monotonic clock
//! (`now_ms`) and carries out the returned [`Effects`]: scheduling wakes and
//! cues, replacing the draft, hiding the input window, repainting. Nothing
//! here touches windows, audio devices or sockets, so every rule is tested
//! without a UI.

#![forbid(unsafe_code)]

mod config;
mod core;
mod draft;
mod feed;
mod view;

#[cfg(test)]
mod scramble_tests;
#[cfg(test)]
mod tests;

pub use crate::core::{Cmd, Core, CueBatch, Effects, Event, UiSound, Wake};
pub use config::{CoreConfig, DEFAULT_TYPING_VARIANTS, SkinCapabilities};
pub use draft::Draft;
pub use view::{DisplayExit, DisplayView, HistoryRow, Reveal, SegmentReveal, ShakeView};

/// Milliseconds on the host's monotonic clock.
pub type Millis = u64;
