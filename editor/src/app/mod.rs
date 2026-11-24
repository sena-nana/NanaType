//! The editor's state and what each message does to it. Opening, validation,
//! saving, export and serialized edits run off the UI thread; scripts only run
//! in the debugger's isolated worker.

mod dialogs;
mod worker;

use std::collections::HashMap;
use std::{path::PathBuf, sync::Arc};

use nana_ui::{CompiledTheme, RuntimeProgramContext};
use nana_ui_platform::host::WindowCommand;
use nanatype_frame::paths::Paths;
use nanatype_frame::ui::feedback::ToastRequest;
use nanatype_frame::ui::kit::theme::Chrome;
use nanatype_theme::SkinRegistry;

use crate::msg::{FileTarget, Msg};
use crate::session::{self as pe, Action, Editor, SecretKeys, Session};
use crate::ui::{DataTarget, Ui};

use worker::{DirectorySync, EditTx, OpenGate, TransactionQueue};

pub struct App {
    pub ctx: RuntimeProgramContext<Msg>,
    pub paths: Paths,
    /// NanaType's packages, read only: open and copy, never write.
    pub registry: SkinRegistry,
    pub package_editor: Editor,
    pub ui: Ui,
    pub theme: Arc<CompiledTheme>,
    pub system_dark: bool,
    /// The system asks for high contrast (Windows and macOS report it).
    pub system_high_contrast: bool,
    /// The system's text size, where it has one (Windows), for NanaType's
    /// "Follow system" UI scale.
    pub system_text_scale: Option<f32>,
    /// The editor window, once it is on screen.
    pub window: Option<nana_ui::WindowHandle>,
    /// Native window commands for the host to issue.
    pub commands: Vec<WindowCommand>,
    pub toasts: Vec<ToastRequest>,
    dialogs: HashMap<u64, FileTarget>,
    next_dialog: u64,
    /// The newest package listing; older ones that finish later are dropped.
    listing: u64,
    open_gate: OpenGate,
    pending: Option<Transition>,
    key_loading: bool,
    pub exit_requested: bool,
    audio: Option<nanatype_audio::AudioHandle>,
    conflict_copy: Option<(u64, u64, String, Vec<u8>)>,
    replace_path: Option<(u64, u64, String)>,
    watch: Option<crate::directory::Watch>,
    tx_queue: TransactionQueue,
    /// The sound to play once its preview is decoded.
    audition: Option<String>,
    dir_sync: DirectorySync,
}

enum Transition {
    Close,
    Exit,
    Open(Box<Session>),
}

impl App {
    pub fn new(ctx: &RuntimeProgramContext<Msg>, paths: Paths) -> Self {
        let _ = std::fs::create_dir_all(paths.package_drafts());
        // NanaUI reads local pictures only under this base.
        nanatype_frame::media::init(&paths.data);
        // Read-only application preferences; preview personalization is opt-in.
        let mut settings = nanatype_settings::load_settings(&paths.settings(), None).value;
        settings.sanitize();
        let locale = nanatype_frame::locale_of(&settings);
        nanatype_diag::marker("start");
        nanatype_diag::set_session_info(
            "os",
            format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        );
        nanatype_diag::set_session_info("locale", locale.tag());
        let registry = SkinRegistry::new(Some(paths.packages()), env!("CARGO_PKG_VERSION"));
        let (loaded, _) = registry.load_scanned_or_default(&settings.skin.id);
        let system_dark = matches!(
            ctx.system_appearance(),
            Some(nana_ui_platform::SystemAppearance::Dark)
        );
        let system_high_contrast = ctx.high_contrast();
        let chrome = Chrome::of(&settings, system_dark, system_high_contrast);
        let ui = Ui::new(Arc::new(settings), loaded.skin, locale);
        ui.chrome_dark.set(chrome.dark);
        ui.chrome_contrast.set(chrome.high_contrast);
        ui.reduced_motion.set(ctx.reduced_motion());
        ui.global.data_dir.set(paths.data.clone());
        let post = ctx.clone();
        nanatype_frame::ui::pipeline::set_waker(move || post.dispatch(Msg::LayoutMaterialsReady));
        let mut app = Self {
            ctx: ctx.clone(),
            package_editor: Editor::new(
                paths.package_drafts(),
                Arc::new(SecretKeys::new(&paths.data)),
            ),
            paths,
            registry,
            ui,
            theme: nanatype_frame::ui::kit::theme::compile(chrome),
            system_dark,
            system_high_contrast,
            system_text_scale: ctx.text_scale(),
            window: None,
            commands: Vec::new(),
            toasts: Vec::new(),
            dialogs: HashMap::new(),
            next_dialog: 1,
            listing: 0,
            open_gate: OpenGate::default(),
            pending: None,
            key_loading: false,
            exit_requested: false,
            audio: None,
            conflict_copy: None,
            replace_path: None,
            watch: None,
            tx_queue: TransactionQueue::default(),
            audition: None,
            dir_sync: DirectorySync::Idle,
        };
        app.editor_list_drafts();
        worker::list_recoveries(&app);
        // The registry was just scanned: only list it.
        worker::list_packages(&mut app);
        app
    }

    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::PackageEditor(action) => self.package_editor_msg(action),
            Msg::PickFile(target) => dialogs::pick(self, target),
            Msg::Notice { text, error } => self.notice(text, error),
            Msg::RefreshPackages => {
                self.registry.rescan();
                worker::list_packages(self);
            }
            Msg::PackagesListed { ticket, rows } => {
                if ticket == self.listing {
                    self.ui.global.packages.set_if_changed(rows);
                }
            }
            Msg::OpenFolder(path) => nanatype_frame::shell::reveal(&path),
            Msg::LayoutMaterialsReady => self
                .ui
                .layout_epoch
                .update(|epoch| *epoch = epoch.wrapping_add(1)),
            Msg::Open(path) => self.editor_open_archive(path),
            // The host handles these.
            Msg::ToastDismissed(_) | Msg::SecondInstance => {}
        }
    }

    /// Zoom the window's content to NanaType's UI scale, and let its
    /// minimum size grow with it.
    pub fn apply_zoom(&self) {
        let Some(window) = &self.window else {
            return;
        };
        let zoom = self
            .ui
            .settings
            .with_untracked(|s| s.accessibility.zoom(self.system_text_scale));
        drop(window.set_content_zoom(zoom));
        drop(window.set_size_limits(Some(crate::ui::shell::minimum_size(zoom)), None));
    }

    /// Follow NanaType's chrome and contrast settings, or the system's.
    pub fn refresh_theme(&mut self) {
        let chrome = self
            .ui
            .settings
            .with_untracked(|s| Chrome::of(s, self.system_dark, self.system_high_contrast));
        self.theme = nanatype_frame::ui::kit::theme::compile(chrome);
        self.ui.chrome_dark.set(chrome.dark);
        self.ui.chrome_contrast.set(chrome.high_contrast);
        if let Some(window) = &self.window {
            nanatype_frame::shell::tint_caption(window, &self.theme);
        }
    }

    pub fn notice(&mut self, text: String, error: bool) {
        if error {
            nanatype_diag::report!(crate::diag::ERROR_NOTICE; "{text}");
        }
        self.toasts.push(ToastRequest::notice(text, error));
    }

    pub fn pick(&mut self, target: FileTarget) {
        dialogs::pick(self, target);
    }

    pub fn file_chosen(&mut self, request: u64, paths: Vec<PathBuf>) {
        dialogs::file_chosen(self, request, paths);
    }

    fn enqueue_edit(&mut self, op: EditTx) {
        let Some(s) = &self.package_editor.session else {
            return;
        };
        self.tx_queue.enqueue_edit(s.id, op);
        worker::pump(self);
    }

    fn enqueue_save(&mut self) {
        let Some(s) = &self.package_editor.session else {
            return;
        };
        self.tx_queue.enqueue_save(s.id);
        worker::pump(self);
    }

    fn package_editor_msg(&mut self, action: Action) {
        match action {
            Action::ReloadDirectory => {
                if let Some(s) = &self.package_editor.session {
                    worker::poll_directory(self, s.id);
                }
            }
            Action::DebugSequence(events) => self.editor_try(|s| s.debug_sequence(events)),
            Action::ReplaceText {
                needle,
                replacement,
                index,
                all,
            } => self.editor_try(|s| s.replace_text(&needle, &replacement, index, all)),
            Action::CanvasList {
                element,
                operation,
                target,
            } => {
                let Some(s) = &self.package_editor.session else {
                    return;
                };
                let view = s.view(None, false);
                if view.validating || view.preview_stale || view.report.skin.is_none() {
                    return;
                }
                let index = element
                    .strip_prefix("decor.")
                    .and_then(|s| s.parse::<usize>().ok());
                let op = match (operation.as_str(), index) {
                    ("add", _) => nanatype_theme::editor::DecorListOp::Add,
                    ("duplicate", Some(i)) => nanatype_theme::editor::DecorListOp::Duplicate(i),
                    ("remove", Some(i)) => nanatype_theme::editor::DecorListOp::Remove(i),
                    ("up", Some(i)) => nanatype_theme::editor::DecorListOp::MoveUp(i),
                    ("down", Some(i)) => nanatype_theme::editor::DecorListOp::MoveDown(i),
                    _ => return,
                };
                let inherited = view.properties;
                let size = self.ui.proj().preview_size.get_untracked();
                let fallback_size = [
                    size[0].parse().unwrap_or(800.),
                    size[1].parse().unwrap_or(240.),
                ];
                self.enqueue_edit(EditTx {
                    name: "editor-canvas-list",
                    prefix: "canvas.".into(),
                    rename: None,
                    refresh_asset: false,
                    apply: Box::new(move |project, registry| {
                        let changed = nanatype_theme::editor::apply_decor_list(
                            project,
                            &target,
                            Some(&inherited),
                            fallback_size,
                            op,
                        )
                        .map_err(|e| e.to_string())?;
                        if changed {
                            worker::inspect_strict(registry, project)?;
                        }
                        Ok(())
                    }),
                });
            }
            Action::CanvasEdit(edit) => {
                let Some(s) = &self.package_editor.session else {
                    return;
                };
                let view = s.view(None, false);
                if view.validating || view.preview_stale {
                    self.ui.proj().canvas_preview.set(None);
                    return;
                }
                let Some(skin) = view.report.skin.clone() else {
                    return;
                };
                let authored = view.properties;
                self.enqueue_edit(EditTx {
                    name: "editor-canvas",
                    prefix: "canvas.".into(),
                    rename: None,
                    refresh_asset: false,
                    apply: Box::new(move |project, registry| {
                        crate::canvas::commit(project, &skin, &authored, &edit)?;
                        let (_, loaded) = registry.inspect_project(project).map_err(|e| {
                            e.iter()
                                .map(|e| e.message.clone())
                                .collect::<Vec<_>>()
                                .join("\n")
                        })?;
                        if let Some(loaded) = loaded {
                            crate::canvas::check_result(&skin, &loaded.skin, &edit)?;
                        }
                        Ok(())
                    }),
                });
            }
            Action::CopyConflict(path) => {
                if let Some(s) = &self.package_editor.session
                    && let Some(bytes) = s.project.files.get(&path)
                {
                    self.conflict_copy = Some((s.id, s.version, path, bytes.clone()));
                    self.pick(FileTarget::SaveConflict);
                }
            }
            Action::ConflictCopied {
                session,
                version,
                path,
                result,
            } => {
                if self
                    .package_editor
                    .session
                    .as_ref()
                    .is_none_or(|s| s.id != session || s.version != version)
                {
                    return;
                }
                match result {
                    Ok(()) => self.editor_try(|s| s.resolve_conflict(&path, false)),
                    Err(e) => self.notice(e, true),
                }
            }
            Action::CreateFile(path) => self.editor_try(|s| {
                let mut project = s.project.clone();
                nanatype_theme::editor::create_file(&mut project, &path)
                    .map_err(|e| e.to_string())?;
                s.replace_project(project)?;
                s.select(&path);
                Ok(())
            }),
            Action::RenameFile { from, to } => {
                self.enqueue_edit(EditTx {
                    name: "editor-rename",
                    prefix: String::new(),
                    rename: None,
                    refresh_asset: true,
                    apply: Box::new(move |project, _| {
                        nanatype_theme::editor::rename_asset(project, &from, &to)
                            .map_err(|e| e.to_string())
                    }),
                });
            }
            Action::ReplaceFile => {
                if let Some(s) = &self.package_editor.session
                    && let Some(path) = s.selected_path().map(str::to_owned)
                {
                    self.replace_path = Some((s.id, s.version, path));
                    self.pick(FileTarget::ReplaceProjectAsset);
                }
            }
            Action::AssetPrepared {
                session,
                version,
                path,
                result,
            } => {
                if self
                    .package_editor
                    .session
                    .as_ref()
                    .is_none_or(|s| s.id != session || s.selected_path() != Some(path.as_str()))
                {
                    return;
                }
                if self
                    .package_editor
                    .session
                    .as_ref()
                    .is_some_and(|s| s.version != version)
                {
                    worker::prepare_asset(self, &path);
                    return;
                }
                let audition = self.audition.take().is_some_and(|a| a == path);
                match result {
                    Ok(asset) => {
                        self.ui.proj().asset.set(Some(asset));
                        if audition {
                            self.package_editor_msg(Action::PlayAsset);
                        }
                    }
                    Err(e) => self.notice(e, true),
                }
            }
            Action::PlayAsset => {
                if let Some(asset) = self.ui.proj().asset.get_untracked()
                    && let Some(pcm) = asset.pcm.clone()
                {
                    if let Some(old) = self.audio.take() {
                        old.stop();
                    }
                    let audio = self.audio.get_or_insert_with(|| {
                        let ctx = self.ctx.clone();
                        nanatype_audio::AudioEngine::start(None, move |s| {
                            if let nanatype_audio::AudioStatus::Failed(e) = s {
                                ctx.dispatch(Msg::PackageEditor(Action::AudioStatus(e)));
                            }
                        })
                    });
                    audio.cancel(1);
                    audio.set_master(self.ui.global.audio_gain.get_untracked().clamp(0., 1.));
                    audio.load("editor.preview".into(), pcm);
                    audio.play(nanatype_audio::Voice {
                        sound: "editor.preview".into(),
                        bus: nanatype_audio::Bus::Ui,
                        gain: 1.,
                        pitch: 1.,
                    });
                }
            }
            Action::StopAudio => {
                if let Some(audio) = self.audio.take() {
                    audio.stop();
                }
            }
            Action::AudioStatus(e) => self.notice(e, true),
            Action::ResolveConflict { path, keep } => {
                self.editor_try(|s| s.resolve_conflict(&path, keep))
            }
            Action::DirectoryPoll(session) => worker::poll_directory(self, session),
            Action::DirectoryScanned {
                session,
                version,
                result,
            } => {
                if self
                    .package_editor
                    .session
                    .as_ref()
                    .is_none_or(|s| s.id != session)
                {
                    return;
                }
                let stale = self
                    .package_editor
                    .session
                    .as_ref()
                    .is_some_and(|s| s.version != version || s.saving)
                    || self.tx_queue.is_busy();
                if !stale {
                    match result {
                        Ok(project) => {
                            if self
                                .package_editor
                                .session
                                .as_ref()
                                .is_some_and(|s| s.disk_baseline().files != project.files)
                            {
                                self.editor_with(|s| s.merge_disk(project));
                            }
                        }
                        Err(e) => self.notice(e, true),
                    }
                }
                if self.dir_sync.finish_scan(stale)
                    && self
                        .package_editor
                        .session
                        .as_ref()
                        .is_some_and(|s| !s.saving)
                {
                    worker::poll_directory(self, session);
                }
            }
            Action::SetDeclaration {
                file,
                path,
                value,
                unbind,
            } => {
                let Some(s) = &self.package_editor.session else {
                    return;
                };
                let inherited = s.view(None, false).properties;
                let prefix = format!("properties.{file}:{path:?}");
                self.enqueue_edit(EditTx {
                    name: "editor-property",
                    prefix,
                    rename: None,
                    refresh_asset: false,
                    apply: Box::new(move |project, registry| {
                        nanatype_theme::editor::set_declaration(
                            project,
                            &file,
                            &path,
                            value.as_deref(),
                            Some(&inherited),
                            unbind,
                        )
                        .map_err(|e| e.to_string())?;
                        worker::inspect_strict(registry, project)
                    }),
                });
            }
            Action::TxFinished {
                session,
                tx_id,
                prefix,
                rename,
                refresh_asset,
                result,
            } => {
                if prefix == "canvas." {
                    self.ui.proj().canvas_preview.set(None);
                }
                if self
                    .package_editor
                    .session
                    .as_ref()
                    .is_none_or(|s| s.id != session)
                {
                    return;
                }
                let ok = result.is_ok();
                if !self.tx_queue.finish_edit(tx_id, ok) {
                    return;
                }
                match result {
                    Ok(project) => {
                        let mut applied = false;
                        self.editor_try(|s| {
                            s.replace_project(project)?;
                            if !prefix.is_empty() {
                                s.applied_forms(&prefix);
                            }
                            applied = true;
                            Ok(())
                        });
                        if applied {
                            if let Some((from, to)) = rename {
                                let target = self.ui.proj().data_target;
                                if target.with_untracked(|t| t.as_ref() == Some(&from)) {
                                    target.set(Some(to));
                                }
                            }
                            if refresh_asset {
                                worker::prepare_current_asset(self);
                            }
                        }
                    }
                    Err(e) => self.notice(e, true),
                }
                worker::pump(self);
                self.editor_publish();
            }
            Action::Audition(path) => {
                self.audition = Some(path.clone());
                self.editor_with(|s| s.select(&path));
                worker::prepare_asset(self, &path);
            }
            Action::Recoveries(rows) => self.ui.global.recoveries.set(Arc::new(rows)),
            Action::RestoreRecovery(id) => {
                let root = self.package_editor.drafts.clone();
                worker::open_session(self, move || {
                    crate::recovery::load(&root, &id).map(Session::restore)
                });
            }
            Action::DiscardRecovery(id) => worker::remove_recovery(self, id),
            Action::RecoveryWritten {
                session,
                version,
                result,
            } => {
                if let Some(s) = &mut self.package_editor.session
                    && s.id == session
                {
                    s.recovering = false;
                    s.recovered_version = version;
                }
                if let Err(e) = result {
                    self.notice(e, true);
                }
            }
            Action::Undo => self.editor_with(Session::undo),
            Action::Redo => self.editor_with(Session::redo),
            Action::Buffer {
                session,
                key,
                value,
            } => {
                if let Some(s) = &mut self.package_editor.session
                    && s.id == session
                {
                    s.buffer(key, value);
                }
                self.editor_publish();
            }
            Action::DiscardForms(prefix) => self.editor_with(|s| s.discard_forms(&prefix)),
            Action::Exit => {
                self.open_gate.cancel();
                self.transition(Transition::Exit);
            }
            Action::SaveFinished {
                session,
                info,
                project,
                result,
            } => {
                if self
                    .package_editor
                    .session
                    .as_ref()
                    .is_none_or(|s| s.id != session)
                {
                    return;
                }
                self.tx_queue.finish_save();
                if let Some(s) = &mut self.package_editor.session {
                    s.saving = false;
                }
                let pending = self.pending.take();
                match result {
                    Ok(()) => {
                        let s = self.package_editor.session.as_mut().unwrap();
                        s.saved_snapshot(info, project);
                        let clean = !s.dirty();
                        if clean {
                            s.cancel_recovery();
                            let id = s.info.id.clone();
                            worker::remove_recovery(self, id);
                        } else {
                            s.cancel_close();
                        }
                        self.notice(self.ui.now_t("pkgedit-saved"), false);
                        self.editor_list_drafts();
                        if clean && let Some(next) = pending {
                            self.finish_transition(next);
                        }
                    }
                    Err(e) => {
                        if let Some(s) = &mut self.package_editor.session {
                            s.cancel_close();
                        }
                        self.notice(e, true);
                    }
                }
                let rescan = self.dir_sync.finish_save();
                worker::pump(self);
                self.editor_publish();
                if rescan {
                    worker::poll_directory(self, session);
                }
            }
            Action::KeyLoaded(result) => {
                self.key_loading = false;
                match result {
                    Ok(key) => self.package_editor.key = key,
                    Err(e) => self.notice(e, true),
                }
                self.package_editor.key_checked = true;
                self.editor_publish();
            }
            Action::Imported {
                session,
                folder,
                name,
                result,
            } => {
                if self
                    .package_editor
                    .session
                    .as_ref()
                    .is_none_or(|s| s.id != session)
                {
                    return;
                }
                match result {
                    Ok(bytes) => {
                        self.editor_try(|s| s.import_into(&folder, &name, bytes).map(|_| ()))
                    }
                    Err(e) => self.notice(e, true),
                }
            }
            Action::New { kind, id } => {
                self.open_gate.cancel();
                let result = pe::create(kind, id.trim()).map(Box::new);
                self.editor_opened(result);
            }
            Action::OpenInstalled(id) => {
                let registry = self.registry.clone();
                worker::open_session(self, move || pe::open_installed(&registry, &id));
            }
            Action::CopyInstalled(id) => {
                let registry = self.registry.clone();
                worker::open_session(self, move || {
                    let new_id = pe::copy_id(&registry, &id);
                    pe::copy_installed(&registry, &id, &new_id)
                });
            }
            Action::OpenDraft(id) => {
                let dir = self.package_editor.drafts.clone();
                worker::open_session(self, move || pe::open_draft(&dir, &id));
            }
            Action::Opened(result) => self.editor_opened(result),
            Action::OpenFinished { ticket, result } => {
                if self.open_gate.is_current(ticket) {
                    self.editor_opened(result);
                }
            }
            Action::DiscardDraft(id) => {
                let open = self
                    .package_editor
                    .session
                    .as_ref()
                    .is_some_and(|s| s.info.id == id);
                match nanatype_theme::editor::discard_draft(&self.package_editor.drafts, &id) {
                    Ok(()) if open => {
                        self.tx_queue.clear();
                        self.dir_sync.reset();
                        self.package_editor.session = None;
                        self.ui.reset_project();
                        self.editor_publish();
                    }
                    Ok(()) => {}
                    Err(e) => self.notice(e.to_string(), true),
                }
                self.editor_list_drafts();
            }
            Action::Drafts(drafts) => self.ui.global.package_drafts.set(Arc::new(drafts)),
            Action::Close => {
                self.open_gate.cancel();
                self.transition(Transition::Close);
            }
            Action::ConfirmClose { save } => {
                if save {
                    self.enqueue_save();
                } else if let Some(next) = self.pending.take() {
                    self.finish_transition(next);
                }
            }
            Action::CancelClose => {
                self.open_gate.cancel();
                self.pending = None;
                self.editor_with(Session::cancel_close);
            }
            Action::Save => {
                self.enqueue_save();
                self.editor_publish();
            }
            Action::Revert => self.editor_with(Session::revert),
            Action::Export => {
                if self.package_editor.session.is_some() {
                    self.pick(FileTarget::ExportProject);
                }
            }
            Action::Exported(result) => match result {
                Ok(path) => self.notice(
                    format!("{}: {}", self.ui.now_t("pkgedit-exported"), path.display()),
                    false,
                ),
                Err(e) => self.notice(
                    format!("{}: {e}", self.ui.now_t("pkgedit-export-failed")),
                    true,
                ),
            },
            Action::Select(path) => {
                self.editor_with(|s| s.select(&path));
                worker::prepare_asset(self, &path);
            }
            Action::Edit { path, text } => {
                if text.len() > nanatype_theme::editor::MAX_EDIT_BYTES {
                    self.notice(self.ui.now_t("pkgedit-source-too-large"), true);
                    self.editor_with(Session::reject_edit);
                    return;
                }
                let result = self
                    .package_editor
                    .session
                    .as_mut()
                    .map(|s| s.edit(&path, &text));
                if let Some(Err(e)) = result {
                    self.notice(e, true);
                    self.editor_with(Session::reject_edit);
                }
                self.editor_publish();
            }
            Action::Format => self.editor_try(Session::format),
            Action::Reveal(diagnostic) => self.editor_with(|s| s.reveal(&diagnostic)),
            Action::Remove(path) => {
                let mut removed = false;
                self.editor_try(|s| {
                    s.remove(&path)?;
                    removed = true;
                    Ok(())
                });
                if removed {
                    worker::prepare_current_asset(self);
                }
            }
            Action::Validated { ticket, report } => {
                self.editor_with(|s| s.validated(ticket, report));
            }
            Action::ApplyMetadata(metadata) => {
                self.enqueue_edit(EditTx {
                    name: "editor-metadata",
                    prefix: "metadata.".into(),
                    rename: None,
                    refresh_asset: false,
                    apply: Box::new(move |project, registry| {
                        nanatype_theme::editor::apply_metadata(project, &metadata)
                            .map_err(|e| e.to_string())?;
                        let _ = nanatype_theme::editor::bundle_dependencies(project, registry);
                        Ok(())
                    }),
                });
            }
            Action::ApplyData { previous, entry } => {
                let rename = previous
                    .as_ref()
                    .zip(entry.as_ref())
                    .and_then(|(from, (to, _))| {
                        (from != to).then(|| {
                            (
                                DataTarget::Data(Some(from.clone())),
                                DataTarget::Data(Some(to.clone())),
                            )
                        })
                    });
                self.enqueue_edit(EditTx {
                    name: "editor-data",
                    prefix: "data.".into(),
                    rename,
                    refresh_asset: false,
                    apply: Box::new(move |project, _| {
                        nanatype_theme::editor::apply_data(
                            project,
                            previous.as_deref(),
                            entry.as_ref().map(|(id, d)| (id.as_str(), d)),
                        )
                        .map_err(|e| e.to_string())
                    }),
                });
            }
            Action::ApplyPipeline { previous, entry } => {
                let rename = previous
                    .as_ref()
                    .zip(entry.as_ref())
                    .and_then(|(from, (to, _))| {
                        (from != to).then(|| {
                            (
                                DataTarget::Pipeline(Some(from.clone())),
                                DataTarget::Pipeline(Some(to.clone())),
                            )
                        })
                    });
                self.enqueue_edit(EditTx {
                    name: "editor-pipeline",
                    prefix: "pipeline.".into(),
                    rename,
                    refresh_asset: false,
                    apply: Box::new(move |project, _| {
                        nanatype_theme::editor::apply_pipeline(
                            project,
                            previous.as_deref(),
                            entry.as_ref().map(|(name, p)| (name.as_str(), p)),
                        )
                        .map_err(|e| e.to_string())
                    }),
                });
            }
            Action::LoadKey => worker::load_key(self, false),
            Action::CreateKey => worker::load_key(self, true),
            Action::Sign(protected) => {
                let Some(key) = self.package_editor.key.clone() else {
                    return;
                };
                self.editor_try(|s| s.sign(&key, &protected));
            }
            Action::Unseal => {
                let key = self.package_editor.key.clone();
                self.editor_try(|s| s.unseal(key.as_ref()));
            }
            Action::Derive(id) => {
                let Some(s) = &self.package_editor.session else {
                    return;
                };
                if !s.forms.is_empty() {
                    self.notice(self.ui.now_t("pkgedit-pending-forms"), true);
                    return;
                }
                let project = s.project.clone();
                worker::open_session(self, move || pe::derived(&project, id.trim()));
            }
            Action::Trust(trusted) => self.editor_with(|s| s.trust(trusted)),
            Action::DebugStart => {
                let ctx = self.ctx.clone();
                let executable = std::env::current_exe().map_err(|e| e.to_string());
                self.editor_try(|s| {
                    s.start_debug(&executable?, move |generation, response| {
                        ctx.dispatch(Msg::PackageEditor(Action::DebugResult {
                            generation,
                            response,
                        }));
                    })
                });
            }
            Action::DebugStop => self.editor_with(Session::stop_debug),
            Action::DebugEvent(event) => self.editor_try(|s| s.debug_event(event)),
            Action::DebugClear => self.editor_with(Session::clear_debug),
            Action::DebugResult {
                generation,
                response,
            } => self.editor_with(|s| s.debug_result(generation, &response)),
        }
    }

    fn transition(&mut self, next: Transition) {
        if let Some(s) = &mut self.package_editor.session
            && !s.request_close()
        {
            self.pending = Some(next);
            self.editor_publish();
            return;
        }
        self.finish_transition(next);
    }

    fn finish_transition(&mut self, next: Transition) {
        if let Some(s) = &self.package_editor.session {
            s.cancel_recovery();
            worker::remove_recovery(self, s.info.id.clone());
        }
        self.dialogs.clear();
        self.conflict_copy = None;
        self.replace_path = None;
        self.tx_queue.clear();
        if let Some(audio) = self.audio.take() {
            audio.stop();
        }
        self.watch = None;
        self.dir_sync.reset();
        self.package_editor.session = None;
        let project_ui = self.ui.reset_project();
        match next {
            Transition::Close => {}
            Transition::Exit => self.exit_requested = true,
            Transition::Open(session) => {
                let id = session.id;
                if let Some(root) = session.directory() {
                    let ctx = self.ctx.clone();
                    match crate::directory::Watch::start(root, move || {
                        ctx.dispatch(Msg::PackageEditor(Action::DirectoryPoll(id)))
                    }) {
                        Ok(watch) => self.watch = Some(watch),
                        Err(e) => self.notice(e, true),
                    }
                }
                self.package_editor.session = Some(*session);
                project_ui.mode.set(pe::Mode::Design);
                project_ui.side_open.set(
                    self.ui.global.workspace_width.get_untracked() >= crate::ui::shell::COMPACT,
                );
                worker::poll_directory(self, id);
            }
        }
        self.editor_publish();
    }

    fn editor_opened(&mut self, result: Result<Box<Session>, String>) {
        match result {
            Ok(session) => self.transition(Transition::Open(session)),
            Err(e) => self.notice(
                format!("{}: {e}", self.ui.now_t("pkgedit-open-failed")),
                true,
            ),
        }
        self.editor_publish();
    }

    pub fn editor_open_archive(&mut self, path: PathBuf) {
        let (installed, data) = (self.paths.packages(), self.paths.data.clone());
        worker::open_session(self, move || {
            if path.is_dir() {
                pe::open_directory(&path, &installed, &data)
            } else {
                pe::open_archive(&path)
            }
        });
    }

    pub fn maintenance_at(&self) -> Option<u64> {
        self.package_editor
            .session
            .as_ref()
            .and_then(|s| s.validation_at().into_iter().chain(s.recovery_at()).min())
    }

    pub fn maintenance(&mut self) {
        let now = nanatype_frame::clock::now_ms();
        if let Some(s) = &self.package_editor.session
            && s.recovery_at().is_some_and(|at| at <= now)
        {
            worker::write_recovery(self);
        }
        if self.maintenance_at().is_some_and(|at| at <= now) {
            self.editor_publish();
        }
    }

    pub fn editor_list_drafts(&mut self) {
        worker::list_drafts(self);
    }

    fn editor_with(&mut self, f: impl FnOnce(&mut Session)) {
        if let Some(session) = &mut self.package_editor.session {
            f(session);
        }
        self.editor_publish();
    }

    fn editor_try(&mut self, f: impl FnOnce(&mut Session) -> Result<(), String>) {
        if let Some(session) = &mut self.package_editor.session
            && let Err(e) = f(session)
        {
            self.notice(e, true);
        }
        self.editor_publish();
    }

    /// Push the view and start a background validation when one is due.
    fn editor_publish(&mut self) {
        let proj = self.ui.proj();
        let mut validation = None;
        let clean_recovery;
        let view;
        {
            let Some(session) = &mut self.package_editor.session else {
                if proj.package_editor.with_untracked(Option::is_some) {
                    proj.package_editor.set(None);
                }
                return;
            };
            clean_recovery = session.mark_recovery_clean();
            if session
                .validation_at()
                .is_some_and(|at| at <= nanatype_frame::clock::now_ms())
            {
                validation = session.validation_due();
            }
            view = session.view(
                self.package_editor.key.as_ref(),
                self.package_editor.key_checked,
            );
        }
        if let Some((ticket, project)) = validation {
            worker::run_validation(self, ticket, project);
        }
        proj.package_editor.set(Some(Arc::new(view)));
        if let Some(id) = clean_recovery {
            worker::remove_recovery(self, id);
        }
    }
}
