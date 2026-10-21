//! The framework NanaType and NanaType Editor share: the skin renderer
//! (dialogue box, history, input box), the UI kit, the isolated skin preview,
//! the plugin worker host, the media server and where files live.
//!
//! Neither product's features live here: live chat, P2P, stickers, outputs
//! and hotkeys stay in `nanatype`; drafts, signing and the debugger stay in
//! `nanatype-editor`.

#![recursion_limit = "512"]

pub mod assets;
pub mod canvas;
pub mod clock;
pub mod instance;
pub mod localtime;
pub mod media;
pub mod packages;
pub mod paths;
pub mod plugin_host;
pub mod shell;
pub mod skin_fonts;
pub mod skin_theme;
pub mod ui;

/// Run as a plugin worker when this process was started as one. Both products
/// call this first in `main`: the host re-launches its own executable with
/// [`nanatype_plugins::WORKER_ARG`], so live plugins and the debugger need no
/// extra binary.
pub fn worker_entry() -> bool {
    if std::env::args().nth(1).as_deref() == Some(nanatype_plugins::WORKER_ARG) {
        nanatype_plugins::run_worker();
        return true;
    }
    false
}

/// The interface language the settings ask for, else the system's.
pub fn locale_of(s: &nanatype_settings::Settings) -> nanatype_i18n::Locale {
    s.general
        .locale
        .as_deref()
        .and_then(nanatype_i18n::Locale::from_tag)
        .unwrap_or_else(nanatype_i18n::Locale::system)
}

/// Append a line to the file named by `NANATYPE_DEBUG` (development aid).
pub fn debug_log(line: &str) {
    use std::io::Write;
    if let Some(path) = std::env::var_os("NANATYPE_DEBUG")
        && let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
    {
        let _ = writeln!(f, "{:>7} {line}", clock::now_ms());
    }
}
