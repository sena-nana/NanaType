//! One monotonic clock for the session, in milliseconds since launch.

use std::cell::Cell;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

thread_local! { static CAPTURE_TIME: Cell<Option<u64>> = const { Cell::new(None) }; }

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

pub fn instant(ms: u64) -> Instant {
    start() + Duration::from_millis(ms)
}
