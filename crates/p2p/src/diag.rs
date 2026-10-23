//! Peer-to-peer events (docs/logging.md). Addresses, invitations and
//! secrets are never recorded.

use nanatype_diag::domains::P2P;
use nanatype_diag::{EventDescriptor, FieldDescriptor, Severity};

pub static ENDPOINT_BOUND: EventDescriptor = EventDescriptor::new(
    P2P,
    1,
    "p2p.endpoint_bound",
    Severity::Info,
    &[
        FieldDescriptor::bool("local_only"),
        FieldDescriptor::bool("custom_relay"),
    ],
);
/// No relay answered within the wait; direct paths may still work.
pub static ONLINE_TIMEOUT: EventDescriptor =
    EventDescriptor::new(P2P, 2, "p2p.online_timeout", Severity::Warn, &[]);
pub static ONLINE: EventDescriptor = EventDescriptor::new(
    P2P,
    3,
    "p2p.online",
    Severity::Info,
    &[FieldDescriptor::u64("ms")],
);
pub static ASSET_TRANSFER_FAILED: EventDescriptor = EventDescriptor::new(
    P2P,
    4,
    "p2p.asset_transfer_failed",
    Severity::Debug,
    &[FieldDescriptor::bool("timed_out")],
);

pub static EVENTS: &[&EventDescriptor] = &[
    &ENDPOINT_BOUND,
    &ONLINE_TIMEOUT,
    &ONLINE,
    &ASSET_TRANSFER_FAILED,
];
