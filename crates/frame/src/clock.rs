//! One monotonic clock for the session, in milliseconds since launch.

use std::cell::Cell;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

#[derive(Clone, Copy)]
enum FrameAnchor {
    /// Explicit RAII frame scope cleared at drop (no staleness heuristic needed).
    Scoped(u64),
    /// Legacy `begin_frame` anchor kept for callers without a scope; expires
    /// after [`FRAME_STALE_MS`] so idle windows never read a past timestamp.
    Legacy(u64),
}

thread_local! {
    static CAPTURE_TIME: Cell<Option<u64>> = const { Cell::new(None) };
    /// Session time of the frame the host is preparing. Glyph reveal offsets
    /// are measured from this, not from a later sample taken while the frame
    /// is built: the compositor clock was already fixed at the start of the
    /// frame, and a later sample shows the glyphs ahead of the effects.
    static FRAME: Cell<Option<FrameAnchor>> = const { Cell::new(None) };
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

/// RAII frame-clock anchor: sets [`frame_ms`] to `ms` for the lifetime of the
/// scope and restores the prior anchor when dropped, avoiding any staleness
/// window across idle gaps.
#[must_use = "the frame anchor is only held while FrameScope lives"]
pub struct FrameScope {
    previous: Option<FrameAnchor>,
}

impl FrameScope {
    pub fn enter(ms: u64) -> Self {
        Self {
            previous: FRAME.replace(Some(FrameAnchor::Scoped(ms))),
        }
    }
}

impl Drop for FrameScope {
    fn drop(&mut self) {
        FRAME.set(self.previous);
    }
}

/// Bind `ms` as the active frame timestamp until the returned [`FrameScope`] drops.
pub fn frame_scope(ms: u64) -> FrameScope {
    FrameScope::enter(ms)
}

/// Remember `ms` as this frame's anchor. The host calls it once, before
/// playback advances, with the same reading those ticks stamp onto a line.
pub fn begin_frame(ms: u64) {
    FRAME.set(Some(FrameAnchor::Legacy(ms)));
}

/// An anchor older than this belongs to an earlier frame: a view built
/// between frames (an event, or a window that sat idle) reads [`now_ms`].
const FRAME_STALE_MS: u64 = 250;

/// This frame's anchor, or [`now_ms`] when the host has not started a frame
/// recently (offscreen tests and tools, or text built outside a frame).
pub fn frame_ms() -> u64 {
    let now = now_ms();
    match FRAME.get() {
        Some(FrameAnchor::Scoped(at)) => at,
        Some(FrameAnchor::Legacy(at)) if at <= now && now - at <= FRAME_STALE_MS => at,
        _ => now,
    }
}

pub fn instant(ms: u64) -> Instant {
    start() + Duration::from_millis(ms)
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Restore(Option<FrameAnchor>);
    impl Drop for Restore {
        fn drop(&mut self) {
            FRAME.set(self.0);
        }
    }

    #[test]
    fn frame_anchor_holds_until_the_next_frame() {
        let _restore = Restore(FRAME.get());
        capture_at(1_000, || {
            begin_frame(950);
            assert_eq!(frame_ms(), 950);
            begin_frame(980);
            assert_eq!(frame_ms(), 980);
        });
    }

    #[test]
    fn an_anchor_from_an_earlier_frame_is_not_reused() {
        let _restore = Restore(FRAME.get());
        begin_frame(1_000);
        // The window sat idle; a line arriving now must not start in the past.
        capture_at(60_000, || assert_eq!(frame_ms(), 60_000));
        // Nor can an anchor lie ahead of the clock.
        capture_at(500, || assert_eq!(frame_ms(), 500));
    }

    #[test]
    fn frame_scope_holds_exact_time_and_restores_on_drop() {
        let _restore = Restore(FRAME.replace(None));
        capture_at(60_000, || {
            assert_eq!(frame_ms(), 60_000);
            {
                let _outer = frame_scope(1_000);
                assert_eq!(frame_ms(), 1_000);
                {
                    let _inner = frame_scope(2_500);
                    assert_eq!(frame_ms(), 2_500);
                }
                assert_eq!(frame_ms(), 1_000);
            }
            assert_eq!(frame_ms(), 60_000);
        });
    }

    #[test]
    fn frame_ms_follows_the_capture_clock_without_an_anchor() {
        let _restore = Restore(FRAME.replace(None));
        for ms in [0, 1_000, 60_000] {
            capture_at(ms, || assert_eq!(frame_ms(), ms));
        }
    }

    #[test]
    fn frame_ms_follows_the_session_clock_without_an_anchor() {
        let _restore = Restore(FRAME.replace(None));
        let before = now_ms();
        let frame = frame_ms();
        let after = now_ms();
        assert!((before..=after).contains(&frame));
    }
}
