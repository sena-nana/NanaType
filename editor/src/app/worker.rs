//! Serialized background transaction queue, directory sync state machine,
//! and background worker tasks (validation, recovery, asset preview, export).
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::Arc;

use nanatype_frame::plugin_host::background;
use nanatype_theme::{PackageProject, SkinRegistry};

use crate::msg::Msg;
use crate::session::{self as pe, Action};
use crate::ui::DataTarget;

use super::App;

/// Explicit open-request cancellation gate replacing raw `opening += 1` side-effects.
#[derive(Default)]
pub struct OpenGate {
    ticket: u64,
}

impl OpenGate {
    pub fn begin(&mut self) -> u64 {
        self.ticket += 1;
        self.ticket
    }

    pub fn cancel(&mut self) {
        self.ticket += 1;
    }

    pub fn is_current(&self, ticket: u64) -> bool {
        ticket == self.ticket
    }
}

/// Coordinates directory watch scans and saves without overlapping boolean flags.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DirectorySync {
    #[default]
    Idle,
    Scanning {
        rescan: bool,
    },
    Saving {
        rescan: bool,
    },
}

impl DirectorySync {
    pub fn reset(&mut self) {
        *self = Self::Idle;
    }

    /// Ask to start a scan. Returns `true` when the caller should spawn the scan
    /// immediately; otherwise records `rescan = true` on the active state.
    pub fn request_scan(&mut self) -> bool {
        match self {
            Self::Idle => {
                *self = Self::Scanning { rescan: false };
                true
            }
            Self::Scanning { rescan } | Self::Saving { rescan } => {
                *rescan = true;
                false
            }
        }
    }

    pub fn start_save(&mut self) {
        let rescan = matches!(self, Self::Scanning { .. } | Self::Saving { rescan: true });
        *self = Self::Saving { rescan };
    }

    /// Complete an in-flight scan. Returns `true` if another scan should be
    /// started immediately (unless a save is now active).
    pub fn finish_scan(&mut self, needs_retry: bool) -> bool {
        match self {
            Self::Saving { rescan } => {
                *rescan |= needs_retry;
                false
            }
            Self::Scanning { rescan } => {
                let retry = *rescan || needs_retry;
                *self = Self::Idle;
                retry
            }
            Self::Idle => needs_retry,
        }
    }

    pub fn finish_save(&mut self) -> bool {
        let rescan = matches!(self, Self::Saving { rescan: true });
        *self = Self::Idle;
        rescan
    }
}

/// Mutation closure executed inside a serialized editor transaction.
pub type ApplyEdit =
    Box<dyn FnOnce(&mut PackageProject, &SkinRegistry) -> Result<(), String> + Send + 'static>;

pub struct EditTx {
    pub name: &'static str,
    pub prefix: String,
    pub rename: Option<(DataTarget, DataTarget)>,
    pub refresh_asset: bool,
    pub apply: ApplyEdit,
}

pub enum TxOp {
    Edit(EditTx),
    Save,
}

/// FIFO-serialized transaction queue guaranteeing `Save` waits for all queued
/// edits and aborts if a preceding edit in the batch fails.
#[derive(Default)]
pub struct TransactionQueue {
    pending: VecDeque<(u64, TxOp)>,
    in_flight: Option<u64>,
    next_tx_id: u64,
    edit_failed: bool,
}

impl TransactionQueue {
    pub fn enqueue_edit(&mut self, session_id: u64, op: EditTx) {
        self.pending.push_back((session_id, TxOp::Edit(op)));
    }

    pub fn enqueue_save(&mut self, session_id: u64) {
        if !matches!(self.pending.back(), Some((id, TxOp::Save)) if *id == session_id) {
            self.pending.push_back((session_id, TxOp::Save));
        }
    }

    pub fn clear(&mut self) {
        self.pending.clear();
        self.in_flight = None;
        self.edit_failed = false;
    }

    pub fn is_busy(&self) -> bool {
        self.in_flight.is_some() || !self.pending.is_empty()
    }

    pub fn finish_edit(&mut self, tx_id: u64, ok: bool) -> bool {
        if self.in_flight != Some(tx_id) {
            return false;
        }
        self.in_flight = None;
        if !ok {
            self.edit_failed = true;
        }
        true
    }

    pub fn finish_save(&mut self) {
        self.in_flight = None;
    }

    fn next_id(&mut self) -> u64 {
        self.next_tx_id += 1;
        self.next_tx_id
    }
}

pub fn inspect_strict(registry: &SkinRegistry, project: &PackageProject) -> Result<(), String> {
    registry.inspect_project(project).map(|_| ()).map_err(|e| {
        e.iter()
            .map(|e| format!("{}: {}", e.field, e.message))
            .collect::<Vec<_>>()
            .join("\n")
    })
}

pub fn pump(app: &mut App) {
    while app.tx_queue.in_flight.is_none() {
        let Some((session_id, op)) = app.tx_queue.pending.pop_front() else {
            app.tx_queue.edit_failed = false;
            break;
        };
        let Some(s) = app.package_editor.session.as_mut() else {
            app.tx_queue.clear();
            break;
        };
        if s.id != session_id {
            continue;
        }
        match op {
            TxOp::Edit(edit) => {
                let tx_id = app.tx_queue.next_id();
                app.tx_queue.in_flight = Some(tx_id);
                let mut project = s.project.clone();
                let registry = app.registry.clone();
                let (name, prefix, rename, refresh_asset, apply) = (
                    edit.name,
                    edit.prefix,
                    edit.rename,
                    edit.refresh_asset,
                    edit.apply,
                );
                background(name, &app.ctx, move || {
                    let result = apply(&mut project, &registry).map(|()| project);
                    Msg::PackageEditor(Action::TxFinished {
                        session: session_id,
                        tx_id,
                        prefix,
                        rename,
                        refresh_asset,
                        result,
                    })
                });
                break;
            }
            TxOp::Save => {
                if app.tx_queue.edit_failed {
                    app.pending = None;
                    s.cancel_close();
                    continue;
                }
                if s.saving {
                    continue;
                }
                if let Err(e) = s.ensure_forms_applied() {
                    app.pending = None;
                    s.cancel_close();
                    app.notice(e, true);
                    continue;
                }
                s.saving = true;
                app.dir_sync.start_save();
                let tx_id = app.tx_queue.next_id();
                app.tx_queue.in_flight = Some(tx_id);
                let (project, mut info) = (s.project.clone(), s.info.clone());
                info.saved_at = pe::now_s();
                if let Ok(m) = project.manifest() {
                    info.package = m.meta.id;
                }
                let dir = app.package_editor.drafts.clone();
                let baseline = s.disk_baseline().clone();
                let target = s.directory().map(PathBuf::from);
                let (installed, data) = (app.paths.packages(), app.paths.data.clone());
                background("editor-save", &app.ctx, move || {
                    let result = if let Some(root) = target {
                        crate::directory::save(&root, &installed, &data, &baseline, &project)
                    } else {
                        nanatype_theme::editor::save_draft(&dir, &info, &project)
                            .map_err(|e| e.to_string())
                    };
                    Msg::PackageEditor(Action::SaveFinished {
                        session: session_id,
                        info,
                        project,
                        result,
                    })
                });
                app.editor_publish();
                break;
            }
        }
    }
}

pub fn list_packages(app: &mut App) {
    app.listing += 1;
    let (ticket, registry) = (app.listing, app.registry.clone());
    background("package-list", &app.ctx, move || Msg::PackagesListed {
        ticket,
        rows: Arc::new(nanatype_frame::packages::package_rows(&registry)),
    });
}

pub fn load_key(app: &mut App, create: bool) {
    if app.key_loading {
        return;
    }
    app.key_loading = true;
    let keys = app.package_editor.keys.clone();
    background("editor-key", &app.ctx, move || {
        Msg::PackageEditor(Action::KeyLoaded((|| {
            let key = keys.load()?;
            if key.is_some() || !create {
                return Ok(key);
            }
            let key = nanatype_theme::SealIdentity::generate().map_err(|e| e.to_string())?;
            keys.save(&key)?;
            Ok(Some(key))
        })()))
    });
}

pub fn open_session(
    app: &mut App,
    open: impl FnOnce() -> Result<pe::Session, String> + Send + 'static,
) {
    let ticket = app.open_gate.begin();
    background("package-editor-open", &app.ctx, move || {
        Msg::PackageEditor(Action::OpenFinished {
            ticket,
            result: open().map(Box::new),
        })
    });
}

pub fn export_project(app: &mut App, path: PathBuf) {
    let Some(session) = &app.package_editor.session else {
        return;
    };
    if let Err(e) = session.ensure_forms_applied() {
        app.notice(e, true);
        return;
    }
    let project = session.project.clone();
    let registry = app.registry.clone();
    background("package-editor-export", &app.ctx, move || {
        Msg::PackageEditor(Action::Exported((|| {
            let exe = std::env::current_exe().map_err(|e| e.to_string())?;
            let report = pe::validate_with_worker(&registry, &project, &exe);
            if report.errors() > 0 {
                return Err(report
                    .diagnostics
                    .iter()
                    .filter(|d| d.level == nanatype_theme::editor::Level::Error)
                    .map(|d| format!("{}:{}: {}", d.path, d.line, d.message))
                    .collect::<Vec<_>>()
                    .join("\n"));
            }
            pe::export(&registry, &project, &path)
        })()))
    });
}

pub fn prepare_current_asset(app: &mut App) {
    if let Some(path) = app
        .package_editor
        .session
        .as_ref()
        .and_then(|s| s.selected_path().map(str::to_owned))
    {
        prepare_asset(app, &path);
    }
}

pub fn prepare_asset(app: &mut App, path: &str) {
    app.ui.proj().asset.set(None);
    if let Some(audio) = app.audio.take() {
        audio.stop();
    }
    let Some(s) = &app.package_editor.session else {
        return;
    };
    let Some(bytes) = s.project.files.get(path) else {
        return;
    };
    let (kind, _) = nanatype_theme::editor::describe(path, bytes);
    if !matches!(
        kind,
        nanatype_theme::editor::AssetKind::Image
            | nanatype_theme::editor::AssetKind::Font
            | nanatype_theme::editor::AssetKind::Audio
    ) {
        return;
    }
    let (session, version, path, bytes) = (s.id, s.version, path.to_owned(), bytes.clone());
    background("editor-asset-preview", &app.ctx, move || {
        let result = crate::asset::prepare(&path, bytes).map(Arc::new);
        Msg::PackageEditor(Action::AssetPrepared {
            session,
            version,
            path,
            result,
        })
    });
}

pub fn poll_directory(app: &mut App, id: u64) {
    let Some(s) = &app.package_editor.session else {
        return;
    };
    if s.id != id {
        return;
    }
    let Some(root) = s.directory().map(PathBuf::from) else {
        return;
    };
    if s.saving || !app.dir_sync.request_scan() {
        return;
    }
    let (session, version, installed) = (s.id, s.version, app.paths.packages());
    background("editor-directory-scan", &app.ctx, move || {
        Msg::PackageEditor(Action::DirectoryScanned {
            session,
            version,
            result: crate::directory::read(&root, &installed),
        })
    });
}

pub fn list_recoveries(app: &App) {
    let root = app.package_editor.drafts.clone();
    background("editor-recoveries", &app.ctx, move || {
        Msg::PackageEditor(Action::Recoveries(crate::recovery::list(&root)))
    });
}

pub fn remove_recovery(app: &App, id: String) {
    let root = app.package_editor.drafts.clone();
    background("editor-discard-recovery", &app.ctx, move || {
        let _ = crate::recovery::remove(&root, &id);
        Msg::PackageEditor(Action::Recoveries(crate::recovery::list(&root)))
    });
}

pub fn list_drafts(app: &App) {
    let dir = app.package_editor.drafts.clone();
    background("package-drafts", &app.ctx, move || {
        Msg::PackageEditor(Action::Drafts(nanatype_theme::editor::list_drafts(&dir)))
    });
}

pub fn write_recovery(app: &mut App) {
    let Some(s) = &mut app.package_editor.session else {
        return;
    };
    s.recovering = true;
    let (session, version, epoch, snapshot, root) = (
        s.id,
        s.version,
        s.recovery_token(),
        s.recovery(),
        app.package_editor.drafts.clone(),
    );
    background("editor-recovery", &app.ctx, move || {
        Msg::PackageEditor(Action::RecoveryWritten {
            session,
            version,
            result: crate::recovery::write(&root, snapshot, &epoch, version),
        })
    });
}

pub fn run_validation(app: &App, ticket: u64, project: PackageProject) {
    let registry = app.registry.clone();
    background("package-editor-check", &app.ctx, move || {
        Msg::PackageEditor(Action::Validated {
            ticket,
            report: Arc::new(match std::env::current_exe() {
                Ok(exe) => pe::validate_with_worker(&registry, &project, &exe),
                Err(_) => pe::validate(&registry, &project),
            }),
        })
    });
}
