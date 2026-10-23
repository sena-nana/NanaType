//! Events of the framework NanaType and NanaType Editor share
//! (docs/logging.md). The `app` domain's codes 100 and up are the
//! framework's; the app's own start at 1.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use nanatype_diag::domains::{APP, PLUGIN_LOG, PLUGINS};
use nanatype_diag::{EventDescriptor, FieldDescriptor, Severity};
use nanatype_plugins::PluginLog;

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
pub static BACKGROUND_UNAVAILABLE: EventDescriptor = EventDescriptor::new(
    PLUGINS,
    100,
    "plugins.background_unavailable",
    Severity::Error,
    &[],
);

/// Lines dropped (by the plugin's per-dispatch budget or the host's rate
/// limit) since this plugin's previous recorded line.
const DROPPED: &[FieldDescriptor] = &[FieldDescriptor::u64("dropped")];

pub static PLUGIN_DEBUG: EventDescriptor =
    EventDescriptor::new(PLUGIN_LOG, 1, "plugin_log.debug", Severity::Debug, DROPPED);
pub static PLUGIN_INFO: EventDescriptor =
    EventDescriptor::new(PLUGIN_LOG, 2, "plugin_log.info", Severity::Info, DROPPED);
pub static PLUGIN_WARN: EventDescriptor =
    EventDescriptor::new(PLUGIN_LOG, 3, "plugin_log.warn", Severity::Warn, DROPPED);
pub static PLUGIN_ERROR: EventDescriptor =
    EventDescriptor::new(PLUGIN_LOG, 4, "plugin_log.error", Severity::Error, DROPPED);

/// Lines one plugin may record per second.
const LINES_PER_SECOND: u32 = 10;

struct Budget {
    window: Instant,
    used: u32,
    dropped: u64,
}

static BUDGETS: LazyLock<Mutex<HashMap<String, Budget>>> = LazyLock::new(Default::default);

/// Record what plugin `id` wrote with `nana.log`: `[id] message`, redacted,
/// at most [`LINES_PER_SECOND`] lines a second. Debug and info lines only
/// with detailed logs.
pub fn plugin_log(id: &str, lines: &[PluginLog], dropped: u32) {
    let mut budgets = BUDGETS.lock().unwrap_or_else(|e| e.into_inner());
    let budget = budgets.entry(id.to_owned()).or_insert(Budget {
        window: Instant::now(),
        used: 0,
        dropped: 0,
    });
    budget.dropped += u64::from(dropped);
    for line in lines {
        let event = match line.level {
            0 => &PLUGIN_DEBUG,
            1 => &PLUGIN_INFO,
            2 => &PLUGIN_WARN,
            _ => &PLUGIN_ERROR,
        };
        if line.level < 2 && !nanatype_diag::verbose() {
            continue;
        }
        if budget.window.elapsed() >= Duration::from_secs(1) {
            budget.window = Instant::now();
            budget.used = 0;
        }
        if budget.used >= LINES_PER_SECOND {
            budget.dropped += 1;
            continue;
        }
        budget.used += 1;
        let dropped = std::mem::take(&mut budget.dropped);
        nanatype_diag::report!(*event, dropped = dropped; "[{id}] {}", line.message);
    }
}

pub static EVENTS: &[&EventDescriptor] = &[
    &PLUGIN_DEBUG,
    &PLUGIN_INFO,
    &PLUGIN_WARN,
    &PLUGIN_ERROR,
    &LOCALE_STRINGS_FAILED,
    &MEDIA_CACHE_FAILED,
    &MEDIA_BASE_MISMATCH,
    &TOAST_FAILED,
    &WORKER_UNAVAILABLE,
    &BACKGROUND_UNAVAILABLE,
];
