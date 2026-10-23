//! Events of the framework NanaType and NanaType Editor share
//! (docs/logging.md). The `app` domain's codes 100 and up are the
//! framework's; the app's own start at 1.

use nanatype_diag::domains::{APP, PLUGINS};
use nanatype_diag::{EventDescriptor, Severity};

pub static LOCALE_STRINGS_FAILED: EventDescriptor =
    EventDescriptor::new(APP, 100, "app.locale_strings_failed", Severity::Warn, &[]);
pub static MEDIA_CACHE_FAILED: EventDescriptor =
    EventDescriptor::new(APP, 101, "app.media_cache_failed", Severity::Warn, &[]);
pub static MEDIA_BASE_MISMATCH: EventDescriptor =
    EventDescriptor::new(APP, 102, "app.media_base_mismatch", Severity::Warn, &[]);
pub static TOAST_FAILED: EventDescriptor =
    EventDescriptor::new(APP, 103, "app.toast_failed", Severity::Warn, &[]);
pub static WORKER_UNAVAILABLE: EventDescriptor =
    EventDescriptor::new(APP, 104, "app.worker_unavailable", Severity::Error, &[]);
pub static BACKGROUND_UNAVAILABLE: EventDescriptor =
    EventDescriptor::new(PLUGINS, 100, "plugins.background_unavailable", Severity::Error, &[]);

pub static EVENTS: &[&EventDescriptor] = &[
    &LOCALE_STRINGS_FAILED,
    &MEDIA_CACHE_FAILED,
    &MEDIA_BASE_MISMATCH,
    &TOAST_FAILED,
    &WORKER_UNAVAILABLE,
    &BACKGROUND_UNAVAILABLE,
];
