//! When to tell the host about output-device trouble, and how long to wait
//! before trying again.
//!
//! A missing or broken device is reported once; after that the engine stays
//! quiet, plays nothing and retries with a growing delay, so an unplugged
//! headset does not flood the log and the notices. The policy is kept apart
//! from cpal streams so it can be tested without hardware.

use std::time::{Duration, Instant};

use crate::AudioStatus;

/// A stream that played this long counts as healthy: losing it is news again.
pub(crate) const STABLE: Duration = Duration::from_secs(10);
/// The first reopen after a loss; the system usually has a replacement ready.
pub(crate) const FIRST_RETRY: Duration = Duration::from_millis(500);
const BACKOFF_BASE: Duration = Duration::from_secs(1);
pub(crate) const BACKOFF_MAX: Duration = Duration::from_secs(15);
/// How often to look for the chosen device while the default stands in.
pub(crate) const PREFERRED_POLL: Duration = Duration::from_secs(5);
pub(crate) const PREFERRED_POLL_MAX: Duration = Duration::from_secs(120);

/// What a stream error means for the stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Fault {
    /// A glitch; the stream keeps playing.
    Glitch,
    /// The stream has to be rebuilt, but nothing is wrong: the system
    /// default moved to another device.
    Rerouted,
    /// The device went away or broke.
    Lost(String),
}

pub(crate) fn classify(e: &cpal::Error) -> Fault {
    use cpal::ErrorKind as K;
    match e.kind() {
        K::Xrun | K::RealtimeDenied | K::DeviceChanged => Fault::Glitch,
        K::StreamInvalidated => Fault::Rerouted,
        _ => Fault::Lost(e.to_string()),
    }
}

/// Delay before the next open after `failures` failures in a row.
pub(crate) fn backoff(failures: u32) -> Duration {
    match failures {
        0 | 1 => FIRST_RETRY,
        n => BACKOFF_BASE
            .saturating_mul(1 << (n - 2).min(16))
            .min(BACKOFF_MAX),
    }
}

#[derive(Debug)]
pub(crate) struct Supervisor {
    /// A failure has been reported and no stream has proved itself since:
    /// further failures are not news.
    reported: bool,
    /// Failures since the last stable stream; drives the backoff.
    failures: u32,
    /// When the current stream opened.
    since: Option<Instant>,
    /// Wait before looking for the chosen device again.
    poll: Duration,
}

impl Default for Supervisor {
    fn default() -> Self {
        Self {
            reported: false,
            failures: 0,
            since: None,
            poll: PREFERRED_POLL,
        }
    }
}

impl Supervisor {
    /// The user picked a device: start afresh, so trouble with it is reported.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// A stream is playing. `fell_back` means the chosen device was missing
    /// or failed and the default stands in; the result is then how long to
    /// wait before looking for the chosen one again.
    pub fn opened(&mut self, now: Instant, fell_back: bool) -> Option<Duration> {
        self.since = Some(now);
        if fell_back {
            Some(self.poll)
        } else {
            self.poll = PREFERRED_POLL;
            None
        }
    }

    /// The chosen device is back; the engine reopens it. Should it still not
    /// open, the next look waits longer.
    pub fn preferred_back(&mut self) {
        self.poll = self.poll.saturating_mul(2).min(PREFERRED_POLL_MAX);
    }

    /// No device could be opened: what to report, if anything, and how long
    /// to wait before trying again.
    pub fn open_failed(&mut self, error: String) -> (Option<AudioStatus>, Duration) {
        self.since = None;
        self.failures = self.failures.saturating_add(1);
        let status = self.report(|| AudioStatus::Failed(error));
        (status, backoff(self.failures))
    }

    /// The playing stream reported an error. `None` means keep it; otherwise
    /// drop it, report the status if any, and reopen after the delay.
    pub fn stream_failed(
        &mut self,
        now: Instant,
        fault: Fault,
    ) -> Option<(Option<AudioStatus>, Duration)> {
        let lost = match fault {
            Fault::Glitch => return None,
            Fault::Rerouted => None,
            Fault::Lost(e) => Some(e),
        };
        if self
            .since
            .take()
            .is_some_and(|t| now.saturating_duration_since(t) >= STABLE)
        {
            self.reported = false;
            self.failures = 0;
        }
        self.failures = self.failures.saturating_add(1);
        let status = lost.and_then(|e| self.report(|| AudioStatus::Lost(e)));
        Some((status, backoff(self.failures)))
    }

    fn report(&mut self, status: impl FnOnce() -> AudioStatus) -> Option<AudioStatus> {
        if self.reported {
            None
        } else {
            self.reported = true;
            Some(status())
        }
    }
}
