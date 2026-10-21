//! Everything that can happen in the editor, as one message type.

use std::path::PathBuf;
use std::sync::Arc;

use nana_ui::runtime::{Entity, Toast};

use crate::session::Action;

/// Which file dialog a pick answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileTarget {
    /// A `.ntpkg` to open as a project.
    EditPackage,
    /// Where to export the open project.
    ExportProject,
    /// A file to import into the open project.
    ImportProjectAsset,
}

pub enum Msg {
    /// The package editor itself (`crate::session`).
    PackageEditor(Action),
    PickFile(FileTarget),
    /// Show a notice as a toast.
    Notice {
        text: String,
        error: bool,
    },
    ToastDismissed(Entity<Toast>),
    /// The installed packages, listed off the UI thread for request `ticket`.
    PackagesListed {
        ticket: u64,
        rows: Arc<Vec<nanatype_frame::packages::PackageRow>>,
    },
    /// Reload the installed packages (the app may have installed one).
    RefreshPackages,
    /// Open the folder holding `path` in the system file manager.
    OpenFolder(PathBuf),
    /// Another launch asked this one to come forward.
    SecondInstance,
    /// Open this `.ntpkg` (from the command line).
    Open(PathBuf),
    /// Background layout materials finished; redraw what used them.
    LayoutMaterialsReady,
}
