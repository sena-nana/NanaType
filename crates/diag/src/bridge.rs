//! Dependencies' `log` output, and `tracing` output through tracing's `log`
//! feature (winit, grammers), recorded as faults in the `external`
//! domain: `<crate>: <message>`, redacted and rate-limited per crate.
//!
//! Warn and up normally, Info and up with detailed logs. HTTP and TLS stacks
//! log request details below Error, so they are held to Error.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, RwLock};
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

struct CrateBudget {
    throttle: Throttle,
    suppressed: AtomicU64,
}

impl CrateBudget {
    fn new() -> Self {
        Self {
            throttle: Throttle::new(),
            suppressed: AtomicU64::new(0),
        }
    }
}

fn budget_for(crate_name: &str) -> Arc<CrateBudget> {
    static BUDGETS: OnceLock<RwLock<HashMap<Box<str>, Arc<CrateBudget>>>> = OnceLock::new();
    let map = BUDGETS.get_or_init(|| RwLock::new(HashMap::new()));
    {
        let guard = map.read().unwrap_or_else(|e| e.into_inner());
        if let Some(budget) = guard.get(crate_name) {
            return Arc::clone(budget);
        }
    }
    let mut guard = map.write().unwrap_or_else(|e| e.into_inner());
    Arc::clone(
        guard
            .entry(crate_name.into())
            .or_insert_with(|| Arc::new(CrateBudget::new())),
    )
}

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
        let budget = budget_for(name);
        if !budget.throttle.allow(INTERVAL) {
            budget.suppressed.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let suppressed = budget.suppressed.swap(0, Ordering::Relaxed);
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
    fn budgets_are_stable_and_collision_free() {
        let a = budget_for("wgpu_core");
        let a_again = budget_for("wgpu_core");
        let b = budget_for("iroh");
        assert!(Arc::ptr_eq(&a, &a_again));
        assert!(!Arc::ptr_eq(&a, &b));

        // Even crates with the same FNV-1a mod 32 residue get independent budgets.
        let c1 = budget_for("crate_a");
        let c2 = budget_for("crate_a_collision_check");
        assert!(!Arc::ptr_eq(&c1, &c2));
        assert!(c1.throttle.allow(INTERVAL));
        assert!(c2.throttle.allow(INTERVAL));
        assert!(!c1.throttle.allow(INTERVAL));
    }
}
