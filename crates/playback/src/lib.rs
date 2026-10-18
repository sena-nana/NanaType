//! Reveal timing for dialogue lines.
//!
//! A line is compiled once into a [`Timeline`]: per-unit reveal offsets and
//! timed cues (typing sounds, sound effects, expressions, shakes), split into
//! segments at pause and page markers. The [`Player`] then walks segments with
//! an injected millisecond clock and emits [`PlayerEffect`]s; it never sleeps
//! and never polls. The renderer animates a whole segment from one schedule,
//! so nothing ticks per character on the UI thread.

#![forbid(unsafe_code)]

mod mirror;
mod pacing;
mod player;
mod queue;
mod timeline;

#[cfg(test)]
mod tests;

pub use mirror::{MirrorDelta, mirror_delta, mirror_matches, mirror_offsets};
pub use pacing::{PacingParams, PunctPauses, TypingSoundRule};
pub use player::{Phase, Player, PlayerEffect, RevealSnapshot, WakeReason};
pub use queue::{LineQueue, QueueDecision, QueuePolicy};
pub use timeline::{Cue, CueKind, Segment, SegmentEnd, Timeline, compile};
