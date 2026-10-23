//! Dependencies' `log` output, and `tracing` output through tracing's `log`
//! feature (iroh, winit, grammers), recorded as faults in the `external`
//! domain: `<crate>: <message>`, redacted and rate-limited per crate.
//!
//! Warn and up normally, Info and up with detailed logs. HTTP and TLS stacks
//! log request details below Error, so they are held to Error.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use log::{Level, LevelFilter, Log, Metadata, Record};
use nana_diagnostics::{EventDescriptor, FieldDescriptor, Severity, Throttle, fault};

use crate::{domains::EXTERNAL, redact};

pub static EXTERNAL_INFO: EventDescriptor =
    EventDescriptor::new(EXTERNAL, 1, "external.info", Severity::Info, SUPPRESSED);
pub static EXTERNAL_WARN: EventDescriptor =
    EventDescriptor::new(EXTERNAL, 2, "external.warn", Severity::Warn, SUPPRESSED);
pub static EXTERNAL_ERROR: EventDescriptor =
    EventDescriptor::new(EXTERNAL, 3, "external.error", Severity::Error, SUPPRESSED);

/// Lines of the same crate dropped by the rate limit since its last record.
const SUPPRESSED: &[FieldDescriptor] = &[FieldDescriptor::u64("suppressed")];

/// Crates held to Error.
const ERROR_ONLY: &[&str] = &[
    "hyper",
    "hyper_util",
    "reqwest",
    "ureq",
    "rustls",
    "h2",
    "tungstenite",
];

/// One record per crate per interval.
const INTERVAL: Duration = Duration::from_secs(1);
/// Crates hash into this many budgets; a collision only shares one.
const SLOTS: usize = 32;

static BUDGETS: [Throttle; SLOTS] = [const { Throttle::new() }; SLOTS];
static SUPPRESSED_COUNT: [AtomicU64; SLOTS] = [const { AtomicU64::new(0) }; SLOTS];

struct Bridge {
    level: LevelFilter,
}

/// Become the process's `log` logger. Later calls (a second builder, tests)
/// keep the first logger.
pub(crate) fn install(verbose: bool) {
    let level = if verbose {
        LevelFilter::Info
    } else {
        LevelFilter::Warn
    };
    if log::set_boxed_logger(Box::new(Bridge { level })).is_ok() {
        log::set_max_level(level);
    }
}

/// The crate a `log` target belongs to: `wgpu_core::device` → `wgpu_core`.
fn crate_of(target: &str) -> &str {
    target.split("::").next().unwrap_or(target)
}

fn max_level(crate_name: &str, level: LevelFilter) -> LevelFilter {
    if ERROR_ONLY.contains(&crate_name) {
        level.min(LevelFilter::Error)
    } else {
        level
    }
}

fn slot(crate_name: &str) -> usize {
    // FNV-1a: stable and cheap; the budget only needs a spread.
    let hash = crate_name.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    (hash % SLOTS as u64) as usize
}

fn descriptor(level: Level) -> &'static EventDescriptor {
    match level {
        Level::Error => &EXTERNAL_ERROR,
        Level::Warn => &EXTERNAL_WARN,
        _ => &EXTERNAL_INFO,
    }
}

impl Log for Bridge {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= max_level(crate_of(metadata.target()), self.level)
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let event = descriptor(record.level());
        if !nana_diagnostics::enabled(event.severity) {
            return;
        }
        let name = crate_of(record.target());
        let slot = slot(name);
        if !BUDGETS[slot].allow(INTERVAL) {
            SUPPRESSED_COUNT[slot].fetch_add(1, Ordering::Relaxed);
            return;
        }
        let suppressed = SUPPRESSED_COUNT[slot].swap(0, Ordering::Relaxed);
        let message = redact::text(&record.args().to_string());
        fault!(*event, suppressed = suppressed; "{name}: {message}");
    }

    fn flush(&self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_and_tls_crates_are_held_to_error() {
        assert_eq!(max_level("reqwest", LevelFilter::Info), LevelFilter::Error);
        assert_eq!(max_level("rustls", LevelFilter::Warn), LevelFilter::Error);
        assert_eq!(max_level("wgpu_core", LevelFilter::Info), LevelFilter::Info);
        assert_eq!(max_level("wgpu_core", LevelFilter::Warn), LevelFilter::Warn);
    }

    #[test]
    fn targets_resolve_to_their_crate() {
        assert_eq!(crate_of("wgpu_core::device::resource"), "wgpu_core");
        assert_eq!(crate_of("naga"), "naga");
    }

    #[test]
    fn levels_map_to_their_events() {
        assert_eq!(descriptor(Level::Error).name, "external.error");
        assert_eq!(descriptor(Level::Warn).name, "external.warn");
        assert_eq!(descriptor(Level::Info).name, "external.info");
        assert_eq!(descriptor(Level::Debug).name, "external.info");
    }

    #[test]
    fn bridge_filters_by_level_and_crate() {
        let bridge = Bridge {
            level: LevelFilter::Warn,
        };
        let meta = |level, target| Metadata::builder().level(level).target(target).build();
        assert!(bridge.enabled(&meta(Level::Warn, "wgpu_hal::vulkan")));
        assert!(!bridge.enabled(&meta(Level::Info, "wgpu_hal::vulkan")));
        assert!(!bridge.enabled(&meta(Level::Warn, "hyper::proto")));
        assert!(bridge.enabled(&meta(Level::Error, "hyper::proto")));
    }

    #[test]
    fn slots_are_stable() {
        assert_eq!(slot("wgpu_core"), slot("wgpu_core"));
        assert!(slot("iroh") < SLOTS);
    }
}
