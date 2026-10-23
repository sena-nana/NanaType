//! NanaType: typewriter dialogue for VTubers.
//!
//! Three windows share one session: the input window (toolbar, a live
//! preview identical to the typewriter, and the editor), the transparent
//! dialogue box, and the Bilibili-style history. Both overlays can be
//! captured by OBS through window capture or Spout2. With Bilibili linked, a
//! chat panel lists the live chat for the streamer to answer.

#![recursion_limit = "512"]

// A Steam build updates through SteamPipe and ships no reverse-engineered
// chat; see docs/steam.md for the feature sets.
#[cfg(all(
    feature = "steam",
    any(feature = "updates", feature = "unofficial-chat")
))]
compile_error!(
    "the `steam` feature excludes `updates` and `unofficial-chat`: build with --no-default-features --features builtin-fonts,steam"
);

pub mod app;
pub mod assets;
pub mod chat;
pub mod completion;
pub mod diag;
pub mod distribution;
pub mod host;
pub mod msg;
pub mod output;
pub mod p2p;
mod perf;
pub mod plugins;
pub mod scheduler;
pub mod services;
pub mod stickers;
pub mod ui;
pub mod windows;
pub mod workshop;

pub use nanatype_frame::{clock, localtime, paths, skin_fonts, skin_theme};

use std::process::ExitCode;

use nana_ui::NanaApplication;

use crate::services::instance::{self, Product};
use crate::windows::WindowKind;

/// Run the application; returns when the user quits.
pub fn run() -> ExitCode {
    if nanatype_frame::worker_entry() {
        return ExitCode::SUCCESS;
    }
    // Started outside Steam, a Steam release relaunches through it; the copy
    // Steam starts must find the single-instance lock free.
    if std::env::var_os(nana_ui::SELF_CHECK_ENV).is_none()
        && nanatype_steam::restart_through_steam()
    {
        return ExitCode::SUCCESS;
    }
    if !instance::claim(Product::APP) {
        return ExitCode::SUCCESS;
    }
    services::steam::connect();
    clock::now_ms();

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
    // The session log starts with the runtime, before settings load.
    let logging = nanatype_settings::peek_diagnostics(&paths::Paths::of_app().settings());
    match NanaApplication::builder(identity)
        .diagnostics(nanatype_diag::config(logging.verbose))
        .run::<host::Host>(primary)
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("NanaType: {error}");
            ExitCode::FAILURE
        }
    }
}
