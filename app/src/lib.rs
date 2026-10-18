//! NanaType: typewriter dialogue for VTubers.
//!
//! Three windows share one session: the input window (toolbar, a live
//! preview identical to the typewriter, and the editor), the transparent
//! dialogue box, and the Bilibili-style history. Both overlays can be
//! captured by OBS through window capture or Spout2. With Bilibili linked, a
//! chat panel lists the live chat for the streamer to answer.

#![recursion_limit = "512"]

pub mod app;
pub mod assets;
pub mod chat;
pub mod clock;
pub mod host;
pub mod localtime;
pub mod msg;
pub mod output;
pub mod p2p;
pub mod paths;
mod perf;
pub mod plugins;
pub mod scheduler;
pub mod services;
pub mod skin_fonts;
pub mod skin_theme;
pub mod stickers;
pub mod ui;
pub mod windows;

use std::process::ExitCode;

use nana_ui::{DiagnosticsConfig, NanaApplication};

use crate::services::instance::{Instance, acquire, listen};
use crate::windows::WindowKind;

static INSTANCE: std::sync::OnceLock<std::net::TcpListener> = std::sync::OnceLock::new();

/// Run the application; returns when the user quits.
pub fn run() -> ExitCode {
    if std::env::args().nth(1).as_deref() == Some("--plugin-worker") {
        nanatype_plugins::run_worker();
        return ExitCode::SUCCESS;
    }
    debug_log("main start");
    let listener = match acquire() {
        Instance::Primary(listener) => listener,
        Instance::Secondary => {
            debug_log("secondary instance; exiting");
            return ExitCode::SUCCESS;
        }
    };
    clock::now_ms();
    INSTANCE.set(listener).ok();

    // The macro embeds the identity marker the packager validates against.
    let identity = nana_ui_platform::application_identity!(
        id: "dev.sena-nana.nanatype",
        name: "NanaType",
        version: env!("CARGO_PKG_VERSION"),
        vendor: "sena-nana",
    );
    debug_assert_eq!(identity.id, paths::APP_ID);
    // The primary window is the input window; it opens hidden and is placed
    // and shown once the monitor layout is known.
    let defaults = nanatype_settings::Settings::default();
    let skin = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"))
        .load(nanatype_theme::DEFAULT_SKIN)
        .expect("built-in skin")
        .skin;
    let primary = windows::descriptor(WindowKind::Input, "NanaType", &skin, &defaults);
    match NanaApplication::builder(identity)
        .diagnostics(DiagnosticsConfig::default())
        .run::<host::Host>(primary)
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            debug_log(&format!("run failed: {error}"));
            eprintln!("NanaType: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Start answering second launches; called once the program can dispatch.
pub fn listen_for_instances(show: impl Fn() + Send + 'static) {
    if let Some(listener) = INSTANCE.get().and_then(|l| l.try_clone().ok()) {
        listen(listener, show);
    }
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
