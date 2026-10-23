//! NanaType's diagnostics over NanaUI's `nana-diagnostics` (docs/logging.md).
//!
//! NanaUI owns the pipeline: thread-local rings, the flight recorder, `.nlog`
//! session files, crash snapshots. This crate owns NanaType's policy:
//! - [`domains`]: one event domain per subsystem, assigned here so codes
//!   never collide;
//! - [`redact`]: the only way free text reaches a record. Typed fields carry
//!   counts, codes and durations; a fault message goes through `redact` first;
//! - [`bridge`]: `log` / `tracing` output of dependencies (wgpu, iroh, ureq,
//!   winit...) as rate-limited faults;
//! - [`config`]: severity and persistence from the "Detailed logs" setting
//!   and `NANATYPE_DEBUG`;
//! - [`package`]: the diagnostics package a user attaches to a bug report.
//!
//! Each crate declares its own events in a `diag` module and lists them in
//! an `EVENTS` slice for the catalog test (`app/tests/diag_catalog.rs`).

pub mod bridge;
pub mod domains;
pub mod package;
mod policy;
pub mod redact;

pub use nana_diagnostics::{
    Domain, EventDescriptor, FieldDescriptor, HistogramCells, Metric, Severity, Throttle, enabled,
    event, fault, global, metric, register_thread, set_session_info, span,
};
pub use policy::{config, flush_for_exit, marker, verbose};

/// Record a fault whose message is formatted, then [`redact::text`]ed:
/// `report!(EVENT, window = 2u64; "spout: {detail}")`. Nothing is formatted
/// unless the event's severity is recorded.
#[macro_export]
macro_rules! report {
    ($event:expr $(, $name:ident = $value:expr)* ; $($message:tt)+) => {
        $crate::fault!(
            $event $(, $name = $value)*;
            "{}",
            $crate::redact::text(&::std::format!($($message)+))
        )
    };
}

/// Events declared here, for the catalog test.
pub static EVENTS: &[&EventDescriptor] = &[
    &bridge::EXTERNAL_INFO,
    &bridge::EXTERNAL_WARN,
    &bridge::EXTERNAL_ERROR,
];
