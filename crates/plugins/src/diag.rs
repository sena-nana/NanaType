//! Plugin worker events (docs/logging.md). What a plugin writes with
//! `nana.log` is recorded by its host, which knows the plugin's ID.

use nanatype_diag::domains::PLUGINS;
use nanatype_diag::{EventDescriptor, FieldDescriptor, Severity};

pub static WORKER_READY: EventDescriptor = EventDescriptor::new(
    PLUGINS,
    1,
    "plugins.worker_ready",
    Severity::Info,
    &[FieldDescriptor::u64("startup_ms")],
);
/// The supervisor gave up on a worker: it timed out, broke the protocol or
/// its pipe closed. The message ends with the worker's last stderr lines.
pub static WORKER_FAILED: EventDescriptor = EventDescriptor::new(
    PLUGINS,
    2,
    "plugins.worker_failed",
    Severity::Warn,
    // 0 starting, 1 running.
    &[FieldDescriptor::u64("stage")],
);
/// The plugin's own script failed (a Lua error or a broken budget).
pub static SCRIPT_ERROR: EventDescriptor =
    EventDescriptor::new(PLUGINS, 3, "plugins.script_error", Severity::Warn, &[]);

pub static EVENTS: &[&EventDescriptor] = &[&WORKER_READY, &WORKER_FAILED, &SCRIPT_ERROR];
