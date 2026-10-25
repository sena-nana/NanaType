//! One monotonic clock for the session, in milliseconds since launch.

use std::cell::Cell;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

thread_local! {
    static CAPTURE_TIME: Cell<Option<u64>> = const { Cell::new(None) };
    /// Session time of the frame the host is preparing. Glyph reveal offsets
    /// are measured from this, not from a later sample taken while the frame
    /// is built: the compositor clock was already fixed at the start of the
    /// frame, and a later sample shows the glyphs ahead of the effects.
    static FRAME: Cell<Option<u64>> = const { Cell::new(None) };
}

/// Run an offscreen capture step on a deterministic clock. Other threads
/// and normal application runs continue using the monotonic session clock.
#[doc(hidden)]
pub fn capture_at<R>(ms: u64, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<u64>);
    impl Drop for Restore {
        fn drop(&mut self) {
            CAPTURE_TIME.set(self.0);
        }
    }
    let _restore = Restore(CAPTURE_TIME.replace(Some(ms)));
    f()
}

static START: OnceLock<(Instant, u64)> = OnceLock::new();

fn origin() -> (Instant, u64) {
    *START.get_or_init(|| (Instant::now(), nanatype_model::unix_millis()))
}

fn start() -> Instant {
    origin().0
}

/// Unix milliseconds of the monotonic time `ms` (for history timestamps).
pub fn unix_of(ms: u64) -> u64 {
    origin().1 + ms
}

pub fn now_ms() -> u64 {
    CAPTURE_TIME
        .get()
        .unwrap_or_else(|| start().elapsed().as_millis() as u64)
}

/// Remember `ms` as this frame's anchor. The host calls it once, before
/// playback advances, with the same reading those ticks stamp onto a line.
pub fn begin_frame(ms: u64) {
    FRAME.set(Some(ms));
}

/// This frame's anchor, or [`now_ms`] when the host has not started a frame
/// (offscreen tests and tools).
pub fn frame_ms() -> u64 {
    FRAME.get().unwrap_or_else(now_ms)
}

pub fn instant(ms: u64) -> Instant {
    start() + Duration::from_millis(ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Restore(Option<u64>);
    impl Drop for Restore {
        fn drop(&mut self) {
            FRAME.set(self.0);
        }
    }

    #[test]
    fn frame_anchor_holds_until_the_next_frame() {
        let _restore = Restore(FRAME.get());
        begin_frame(50);
        assert_eq!(frame_ms(), 50);
        let _later = now_ms();
        assert_eq!(frame_ms(), 50);
        begin_frame(80);
        assert_eq!(frame_ms(), 80);
    }

    #[test]
    fn frame_ms_follows_the_session_clock_without_an_anchor() {
        let _restore = Restore(FRAME.replace(None));
        assert_eq!(frame_ms(), now_ms());
    }
}
