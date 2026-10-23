//! Twitch's own events (docs/logging.md). Connection and account states are
//! recorded by the app; these are the retries this crate absorbs.

use nanatype_diag::domains::TWITCH;
use nanatype_diag::{EventDescriptor, Severity};

/// A device-code poll failed and waits for the next interval.
pub static TOKEN_POLL_FAILED: EventDescriptor = EventDescriptor::new(
    TWITCH,
    100,
    "twitch.token_poll_failed",
    Severity::Debug,
    &[],
);
/// Token validation failed for another reason than a revoked token.
pub static REVALIDATE_DEFERRED: EventDescriptor = EventDescriptor::new(
    TWITCH,
    101,
    "twitch.revalidate_deferred",
    Severity::Warn,
    &[],
);
pub static CREDENTIALS_CLEAR_FAILED: EventDescriptor = EventDescriptor::new(
    TWITCH,
    102,
    "twitch.credentials_clear_failed",
    Severity::Warn,
    &[],
);

pub static EVENTS: &[&EventDescriptor] = &[
    &TOKEN_POLL_FAILED,
    &REVALIDATE_DEFERRED,
    &CREDENTIALS_CLEAR_FAILED,
];
