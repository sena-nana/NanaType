//! The session: what the editor holds, what the typewriter shows, what the
//! history lists, and how sending, continuing and recalling move between them.
//!
//! [`Core`] is a reducer. The host feeds it [`Cmd`]s with a monotonic clock
//! (`now_ms`) and carries out the returned [`Effects`]: scheduling wakes and
//! cues, replacing the draft, hiding the input window, repainting. Nothing
//! here touches windows, audio devices or sockets, so every rule is tested
//! without a UI.

#![forbid(unsafe_code)]

mod core;
mod view;

#[cfg(test)]
mod tests;

pub use crate::core::{
    Cmd, Core, CueBatch, DEFAULT_TYPING_VARIANTS, Effects, Event, UiSound, Wake,
};
pub use view::{DisplayView, HistoryRow, Reveal, SegmentReveal, ShakeView, TextExit};

/// Milliseconds on the host's monotonic clock.
pub type Millis = u64;
