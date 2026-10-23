//! Update check and download events (docs/logging.md). A source is its
//! index in the list the app passed; its URL is redacted if it appears.

use nanatype_diag::domains::UPDATE;
use nanatype_diag::{EventDescriptor, FieldDescriptor, Severity};

pub static SOURCE_CHECKED: EventDescriptor = EventDescriptor::new(
    UPDATE,
    1,
    "update.source_checked",
    Severity::Info,
    &[
        FieldDescriptor::u64("source"),
        FieldDescriptor::bool("ok"),
        FieldDescriptor::u64("ms"),
    ],
);
pub static SOURCE_FAILED: EventDescriptor = EventDescriptor::new(
    UPDATE,
    2,
    "update.source_failed",
    Severity::Warn,
    &[FieldDescriptor::u64("source")],
);
pub static DOWNLOAD_FAILED: EventDescriptor = EventDescriptor::new(
    UPDATE,
    3,
    "update.download_failed",
    Severity::Warn,
    &[FieldDescriptor::u64("source")],
);
pub static DOWNLOADED: EventDescriptor = EventDescriptor::new(
    UPDATE,
    4,
    "update.downloaded",
    Severity::Info,
    &[
        FieldDescriptor::u64("source"),
        FieldDescriptor::u64("bytes"),
    ],
);
/// A published version string that does not parse; it is never offered.
pub static VERSION_UNPARSABLE: EventDescriptor =
    EventDescriptor::new(UPDATE, 5, "update.version_unparsable", Severity::Warn, &[]);

pub static EVENTS: &[&EventDescriptor] = &[
    &SOURCE_CHECKED,
    &SOURCE_FAILED,
    &DOWNLOAD_FAILED,
    &DOWNLOADED,
    &VERSION_UNPARSABLE,
];
