//! NanaType Editor: create, edit, sign, preview and debug NanaType packages.
//!
//! A separate program from NanaType that shares its framework
//! (`nanatype-frame`) and its data folder: it reads the packages NanaType
//! installed, keeps drafts and the signing key beside them, and exports
//! `.ntpkg` files for NanaType to import. Nothing it does changes the active
//! skin, installed packages, plugin consent or running plugins.

#![recursion_limit = "512"]

pub mod app;
pub mod asset;
pub mod canvas;
pub mod diag;
pub mod directory;
mod history;
pub mod host;
pub mod msg;
#[cfg(target_os = "macos")]
mod open_documents;
pub mod recovery;
pub mod session;
pub mod ui;

use std::path::PathBuf;
use std::process::ExitCode;

use nana_ui::NanaApplication;
use nanatype_frame::instance::{self, Product};
use nanatype_frame::paths::Paths;

/// What the command line asked for: `nanatype-editor [--data <dir>] [file.ntpkg]`.
#[derive(Debug, Default, PartialEq)]
pub struct Launch {
    /// NanaType's data folder, for a portable NanaType elsewhere.
    pub data: Option<PathBuf>,
    pub open: Option<PathBuf>,
}

impl Launch {
    pub fn from_args(args: impl IntoIterator<Item = String>) -> Self {
        let mut launch = Self::default();
        let mut args = args.into_iter();
        while let Some(arg) = args.next() {
            if arg == "--data" {
                launch.data = args.next().map(PathBuf::from);
            } else if !arg.starts_with("--") && launch.open.is_none() {
                launch.open = Some(PathBuf::from(arg));
            }
        }
        launch
    }

    /// Where NanaType keeps its packages, drafts and signing key.
    pub fn paths(&self) -> Paths {
        match &self.data {
            Some(data) => Paths::in_data_dir(data.clone()),
            None => Paths::of_app(),
        }
    }
}

/// Run the editor; returns when the user closes it.
pub fn run() -> ExitCode {
    // The Lua debugger re-launches this executable as its worker.
    if nanatype_frame::worker_entry() {
        return ExitCode::SUCCESS;
    }
    let launch = Launch::from_args(std::env::args().skip(1));
    if !instance::claim_with_open(Product::EDITOR, launch.open.as_deref()) {
        return ExitCode::SUCCESS;
    }
    nanatype_frame::clock::now_ms();

    // The macro embeds the identity marker the packager validates against.
    let identity = nana_ui_platform::application_identity!(
        id: "dev.sena-nana.nanatype-editor",
        name: "NanaType Editor",
        version: env!("CARGO_PKG_VERSION"),
        vendor: "sena-nana",
    );
    let mut primary = nana_ui::WindowDescriptor::new("NanaType Editor").initial_size(1100.0, 760.0);
    primary.minimum_size = (640.0, 480.0);
    primary.system_caption = true;
    primary.resizable = true;
    primary.focus_on_show = true;
    primary.role = nana_ui_platform::WindowRole::Main;
    primary.icon = icon();
    // One "Detailed logs" switch, in NanaType's settings, covers both programs.
    let launch = Launch::from_args(std::env::args().skip(1));
    let logging = nanatype_settings::peek_diagnostics(&launch.paths().settings());
    match NanaApplication::builder(identity)
        .diagnostics(nanatype_diag::config(logging.verbose))
        .run::<host::Host>(primary)
    {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("NanaType Editor: {error}");
            ExitCode::FAILURE
        }
    }
}

/// The editor's icon for its title bar and the taskbar.
fn icon() -> Option<nana_ui_platform::WindowIcon> {
    let img = image::load_from_memory(include_bytes!("../assets/editor-64.png"))
        .ok()?
        .into_rgba8();
    let (w, h) = img.dimensions();
    nana_ui_platform::WindowIcon::from_rgba(img.into_raw(), w, h).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launch_reads_a_data_folder_and_a_package() {
        let args = ["--data", "/x/data", "a.ntpkg", "b.ntpkg"].map(String::from);
        assert_eq!(
            Launch::from_args(args),
            Launch {
                data: Some("/x/data".into()),
                open: Some("a.ntpkg".into()),
            }
        );
        assert_eq!(Launch::from_args(Vec::new()), Launch::default());
    }
}
