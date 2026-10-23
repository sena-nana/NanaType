//! What gets recorded and kept.
//!
//! The runtime always records Debug and up, so the flight recorder and a
//! crash snapshot carry the context of the last minute. "Detailed logs"
//! decides what reaches the session file: Warn and up (with faults and
//! markers) normally, everything when on. `NANATYPE_DEBUG` adds Trace.

use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use nana_diagnostics::{DiagnosticsConfig, PersistMode, Severity};

static VERBOSE: AtomicBool = AtomicBool::new(false);

/// The environment variable that turns on Trace and full persistence.
pub const DEBUG_ENV: &str = "NANATYPE_DEBUG";

/// The runtime configuration for a session whose "Detailed logs" setting is
/// `verbose`. Also installs the [`bridge`](crate::bridge) for dependencies'
/// logs, at the matching level.
pub fn config(verbose: bool) -> DiagnosticsConfig {
    let trace = std::env::var_os(DEBUG_ENV).is_some();
    let verbose = verbose || trace;
    VERBOSE.store(verbose, Ordering::Relaxed);
    crate::bridge::install(verbose);
    DiagnosticsConfig {
        min_severity: if trace {
            Severity::Trace
        } else {
            Severity::Debug
        },
        persist: if verbose || cfg!(debug_assertions) {
            PersistMode::All
        } else {
            PersistMode::Essential
        },
        ..DiagnosticsConfig::default()
    }
}

/// Did this session start with detailed logs (or `NANATYPE_DEBUG`)? Chatty
/// sources (plugins' `nana.log` debug and info lines) check this.
pub fn verbose() -> bool {
    VERBOSE.load(Ordering::Relaxed)
}

/// A free-text marker in the session log: lifecycle milestones only, never
/// user content.
pub fn marker(text: impl Into<String>) {
    if let Some(diagnostics) = nana_diagnostics::global() {
        diagnostics.marker(text);
    }
}

/// Write everything pending durably before the process exits.
pub fn flush_for_exit() {
    if let Some(diagnostics) = nana_diagnostics::global() {
        diagnostics.flush(true, Duration::from_secs(1));
    }
}
