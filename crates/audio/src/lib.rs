//! Sound for the typewriter.
//!
//! [`AudioEngine::start`] opens the output device on its own thread and
//! returns an [`AudioHandle`] that any thread can use. Sounds are decoded
//! once ([`decode`]) and played from memory by a small mixer: a fixed voice
//! pool, per-bus gain, pitch through playback rate, and a soft limiter.
//! Scheduled sounds carry an `Instant` and start at the matching sample, so
//! typing clicks line up with the reveal even when the UI thread is busy.

mod decode;
mod engine;
mod mixer;
mod supervise;

pub use decode::{DecodeError, Pcm, decode, decode_up_to};
pub use engine::{AudioEngine, AudioError, AudioHandle, AudioStatus, list_devices};
pub use mixer::{Bus, Scheduled, SoundId, Voice};

#[cfg(test)]
mod tests;
