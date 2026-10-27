//! The editor's state and what each message does to it. Opening, validation,
//! saving and export run off the UI thread; scripts only run in the
//! debugger's isolated worker.

use std::collections::HashMap;
use std::{path::PathBuf, sync::Arc};

use nana_ui::{CompiledTheme, RuntimeProgramContext};
use nana_ui_core::{FileDialogKind, FileDialogRequest, FileFilter};
use nana_ui_platform::WindowId;
use nana_ui_platform::host::WindowCommand;
use nanatype_frame::paths::Paths;
use nanatype_frame::plugin_host::background;
use nanatype_frame::ui::feedback::ToastRequest;
use nanatype_theme::SkinRegistry;

use crate::msg::{FileTarget, Msg};
use crate::session::{self as pe, Action, Editor, SecretKeys, Session};
use crate::ui::Ui;

pub struct App {
    pub ctx: RuntimeProgramContext<Msg>,
    pub paths: Paths,
    /// NanaType's packages, read only: open and copy, never write.
    pub registry: SkinRegistry,
    pub package_editor: Editor,
    pub ui: Ui,
    pub theme: Arc<CompiledTheme>,
    pub system_dark: bool,
    /// The editor window, once it is on screen.
    pub window: Option<nana_ui::WindowHandle>,
    /// Native window commands for the host to issue.
    pub commands: Vec<WindowCommand>,
    pub toasts: Vec<ToastRequest>,
    dialogs: HashMap<u64, FileTarget>,
    next_dialog: u64,
    /// The newest package listing; older ones that finish later are dropped.
    listing: u64,
    opening: u64,
    pending: Option<Transition>,
    key_loading: bool,
    pub exit_requested: bool,
    audio: Option<nanatype_audio::AudioHandle>,
    conflict_copy: Option<(u64, u64, String, Vec<u8>)>,
    replace_path: Option<(u64, u64, String)>,
    watch: Option<crate::directory::Watch>,
    scanning_directory: bool,
    scan_again: bool,
}

enum Transition {
    Close,
    Exit,
    Open(Box<Session>),
}

fn read_asset(path: PathBuf) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|e| e.to_string())?
        .take(nanatype_theme::MAX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    Ok(bytes)
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
        let dark = nanatype_frame::skin_theme::dark_mode(settings.skin.chrome, system_dark);
        let ui = Ui::new(Arc::new(settings), loaded.skin, locale);
        let post = ctx.clone();
        ui.post.set(Some(Arc::new(move |a| {
            post.dispatch(Msg::PackageEditor(a))
        })));
        ui.chrome_dark.set(dark);
        ui.reduced_motion.set(ctx.reduced_motion());
        ui.data_dir.set(paths.data.clone());
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
            theme: nanatype_frame::ui::kit::theme::compile(dark),
            system_dark,
            window: None,
            commands: Vec::new(),
            toasts: Vec::new(),
            dialogs: HashMap::new(),
            next_dialog: 1,
            listing: 0,
            opening: 0,
            pending: None,
            key_loading: false,
            exit_requested: false,
            audio: None,
            conflict_copy: None,
            replace_path: None,
            watch: None,
            scanning_directory: false,
            scan_again: false,
        };
        app.editor_list_drafts();
        app.list_recoveries();
        // The registry was just scanned: only list it.
        app.list_packages();
        app
    }

    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::PackageEditor(action) => self.package_editor_msg(action),
            Msg::PickFile(target) => self.pick(target),
            Msg::Notice { text, error } => self.notice(text, error),
            Msg::RefreshPackages => {
                self.registry.rescan();
                self.list_packages();
            }
            Msg::PackagesListed { ticket, rows } => {
                if ticket == self.listing {
                    self.ui.packages.set_if_changed(rows);
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

    /// Follow NanaType's chrome setting, or the system's appearance.
    pub fn refresh_theme(&mut self) {
        let chrome = self.ui.settings.with_untracked(|s| s.skin.chrome);
        let dark = nanatype_frame::skin_theme::dark_mode(chrome, self.system_dark);
        self.theme = nanatype_frame::ui::kit::theme::compile(dark);
        self.ui.chrome_dark.set(dark);
        if let Some(window) = &self.window {
            nanatype_frame::shell::tint_caption(window, &self.theme);
        }
    }

    /// List the packages with their signature status; hashing runs off the
    /// UI thread.
    fn list_packages(&mut self) {
        self.listing += 1;
        let (ticket, registry) = (self.listing, self.registry.clone());
        background("package-list", &self.ctx, move || Msg::PackagesListed {
            ticket,
            rows: Arc::new(nanatype_frame::packages::package_rows(&registry)),
        });
    }

    pub fn notice(&mut self, text: String, error: bool) {
        if error {
            nanatype_diag::report!(crate::diag::ERROR_NOTICE; "{text}");
        }
        self.toasts.push(ToastRequest::notice(text, error));
    }

    pub fn pick(&mut self, target: FileTarget) {
        let id = self.next_dialog;
        self.next_dialog += 1;
        self.dialogs.insert(id, target);
        let t = |k: &str| self.ui.now_t(k);
        let package = FileFilter::new("NanaType package", ["ntpkg", "zip"]);
        let request = match target {
            FileTarget::SaveConflict => {
                FileDialogRequest::new(id, FileDialogKind::SaveFile).title(t("pkgedit-save-copy"))
            }
            FileTarget::OpenDirectory => FileDialogRequest::new(id, FileDialogKind::PickFolder)
                .title(t("pkgedit-open-directory")),
            FileTarget::EditPackage => FileDialogRequest::new(id, FileDialogKind::OpenFile)
                .title(t("pkgedit-open-file"))
                .filters([package]),
            FileTarget::ExportProject => FileDialogRequest::new(id, FileDialogKind::SaveFile)
                .title(t("pkgedit-export"))
                .filters([package])
                .file_name(format!(
                    "{}.ntpkg",
                    self.ui
                        .package_editor
                        .with_untracked(|v| v.as_ref().map(|v| v.package.clone()))
                        .unwrap_or_default()
                )),
            FileTarget::ImportProjectAsset | FileTarget::ReplaceProjectAsset => {
                FileDialogRequest::new(
                    id,
                    if target == FileTarget::ImportProjectAsset {
                        FileDialogKind::OpenFiles
                    } else {
                        FileDialogKind::OpenFile
                    },
                )
                .title(t("pkgedit-import"))
                .filters([FileFilter::new(
                    t("pkgedit-import"),
                    nanatype_theme::editor::IMPORT_EXTENSIONS.iter().copied(),
                )])
            }
        };
        self.commands.push(WindowCommand::OpenFileDialog {
            id: WindowId::PRIMARY,
            request,
        });
    }

    pub fn file_chosen(&mut self, request: u64, paths: Vec<PathBuf>) {
        let Some(target) = self.dialogs.remove(&request) else {
            return;
        };
        if target == FileTarget::ImportProjectAsset {
            for path in paths {
                self.editor_import(path);
            }
            return;
        }
        let Some(path) = paths.into_iter().next() else {
            if target == FileTarget::SaveConflict {
                self.conflict_copy = None;
            }
            if target == FileTarget::ReplaceProjectAsset {
                self.replace_path = None;
            }
            return;
        };
        match target {
            FileTarget::SaveConflict => {
                if let Some((session, version, name, bytes)) = self.conflict_copy.take() {
                    let source = self
                        .package_editor
                        .session
                        .as_ref()
                        .filter(|s| s.id == session && s.version == version)
                        .and_then(|s| s.directory())
                        .map(|root| root.join(&name));
                    let installed = self.paths.packages();
                    if source.is_none() {
                        return;
                    }
                    background("editor-conflict-copy", &self.ctx, move || {
                        let result = (|| {
                            use std::io::Write;
                            crate::directory::check_copy_destination(
                                &path,
                                source.as_deref(),
                                &installed,
                            )?;
                            let parent = path.parent().ok_or("missing destination directory")?;
                            let mut file = tempfile::NamedTempFile::new_in(parent)
                                .map_err(|e| e.to_string())?;
                            file.write_all(&bytes).map_err(|e| e.to_string())?;
                            file.as_file().sync_all().map_err(|e| e.to_string())?;
                            file.persist(path).map_err(|e| e.to_string())?;
                            Ok(())
                        })();
                        Msg::PackageEditor(Action::ConflictCopied {
                            session,
                            version,
                            path: name,
                            result,
                        })
                    });
                }
            }
            FileTarget::OpenDirectory | FileTarget::EditPackage => self.editor_open_archive(path),
            FileTarget::ExportProject => self.editor_export(path),
            FileTarget::ImportProjectAsset => self.editor_import(path),
            FileTarget::ReplaceProjectAsset => {
                if let Some((session, version, target)) = self.replace_path.take() {
                    let Some(s) = &self.package_editor.session else {
                        return;
                    };
                    if s.id != session || s.version != version {
                        return;
                    }
                    self.prepare_project(
                        "editor-replace-asset",
                        String::new(),
                        move |project, _| {
                            nanatype_theme::editor::replace_asset(
                                project,
                                &target,
                                read_asset(path)?,
                            )
                            .map_err(|e| e.to_string())
                        },
                    );
                }
            }
        }
    }

    fn package_editor_msg(&mut self, action: Action) {
        match action {
            Action::ReloadDirectory => {
                if let Some(s) = &self.package_editor.session {
                    self.poll_directory(s.id);
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
                let mut path = target;
                path.push("decor".into());
                let raw = s
                    .project
                    .files
                    .get("skin.toml")
                    .and_then(|b| std::str::from_utf8(b).ok())
                    .and_then(|t| nanatype_theme::editor::parse_document(t).ok());
                let properties = view.properties;
                let mut values = raw
                    .as_ref()
                    .and_then(|t| nanatype_theme::editor::value_at(t, &path))
                    .or_else(|| nanatype_theme::editor::value_at(&properties, &path))
                    .and_then(nanatype_theme::editor::PropertyValue::as_array)
                    .cloned()
                    .unwrap_or_else(|| {
                        let size = self.ui.preview_size.get_untracked();
                        let size = [
                            size[0].parse().unwrap_or(800.),
                            size[1].parse().unwrap_or(240.),
                        ];
                        let props = nanatype_theme::editor::authored_layout(&properties, size);
                        nanatype_theme::editor::value_at(
                            &props,
                            &["dialogue".into(), "decor".into()],
                        )
                        .and_then(nanatype_theme::editor::PropertyValue::as_array)
                        .cloned()
                        .unwrap_or_default()
                    });
                let index = element
                    .strip_prefix("decor.")
                    .and_then(|s| s.parse::<usize>().ok());
                match (operation.as_str(), index) {
                    ("add", _) => {
                        let spec = nanatype_theme::editor::parse_document(
                            "value={size=[48,48],color='#85a8ff',radius=8}",
                        )
                        .unwrap();
                        values.push(spec["value"].clone());
                    }
                    ("duplicate", Some(i)) if i < values.len() => {
                        values.insert(i + 1, values[i].clone())
                    }
                    ("remove", Some(i)) if i < values.len() => {
                        values.remove(i);
                    }
                    ("up", Some(i)) if i + 1 < values.len() => values.swap(i, i + 1),
                    ("down", Some(i)) if i > 0 && i < values.len() => values.swap(i, i - 1),
                    _ => return,
                }
                self.package_editor_msg(Action::SetDeclaration {
                    file: "skin.toml".into(),
                    path,
                    value: Some(nanatype_theme::editor::literal(
                        &nanatype_theme::editor::PropertyValue::Array(values),
                    )),
                    // Structural operations preserve the remaining raw bindings;
                    // duplicating/removing an element deliberately changes their count.
                    unbind: true,
                });
            }
            Action::CanvasEdit(edit) => {
                let Some(s) = &self.package_editor.session else {
                    return;
                };
                let view = s.view(None, false);
                if view.validating || view.preview_stale {
                    self.ui.canvas_preview.set(None);
                    return;
                }
                let Some(skin) = view.report.skin.clone() else {
                    return;
                };
                let authored = view.properties;
                self.prepare_project(
                    "editor-canvas",
                    "canvas.".into(),
                    move |project, registry| {
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
                    },
                );
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
                self.prepare_project("editor-rename", String::new(), move |project, _| {
                    nanatype_theme::editor::rename_asset(project, &from, &to)
                        .map_err(|e| e.to_string())
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
                    self.prepare_asset(&path);
                    return;
                }
                match result {
                    Ok(asset) => self.ui.asset.set(Some(asset)),
                    Err(e) => self.notice(e, true),
                }
            }
            Action::PlayAsset => {
                if let Some(asset) = self.ui.asset.get_untracked()
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
                    audio.set_master(self.ui.audio_gain.get_untracked().clamp(0., 1.));
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
            Action::DirectoryPoll(session) => self.poll_directory(session),
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
                self.scanning_directory = false;
                if self
                    .package_editor
                    .session
                    .as_ref()
                    .is_some_and(|s| s.version != version || s.saving)
                {
                    self.scan_again = true;
                } else {
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
                if self.scan_again
                    && self
                        .package_editor
                        .session
                        .as_ref()
                        .is_some_and(|s| !s.saving)
                {
                    self.scan_again = false;
                    self.poll_directory(session);
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
                self.prepare_project("editor-property", prefix, move |project, registry| {
                    nanatype_theme::editor::set_declaration(
                        project,
                        &file,
                        &path,
                        value.as_deref(),
                        Some(&inherited),
                        unbind,
                    )
                    .map_err(|e| e.to_string())?;
                    registry.inspect_project(project).map_err(|e| {
                        e.iter()
                            .map(|e| format!("{}: {}", e.field, e.message))
                            .collect::<Vec<_>>()
                            .join("\n")
                    })?;
                    Ok(())
                });
            }
            Action::ProjectPrepared {
                session,
                version,
                prefix,
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
                if prefix == "canvas." {
                    self.ui.canvas_preview.set(None);
                }
                if self
                    .package_editor
                    .session
                    .as_ref()
                    .is_some_and(|s| s.version != version)
                {
                    self.notice(self.ui.now_t("pkgedit-operation-stale"), true);
                    return;
                }
                match result {
                    Ok(project) => {
                        let refresh_asset = prefix.is_empty();
                        self.editor_try(|s| {
                            s.replace_project(project)?;
                            if !prefix.is_empty() {
                                s.applied_forms(&prefix);
                            }
                            Ok(())
                        });
                        if refresh_asset {
                            self.prepare_current_asset();
                        }
                    }
                    Err(e) => self.notice(e, true),
                }
            }
            Action::Recoveries(rows) => self.ui.recoveries.set(Arc::new(rows)),
            Action::RestoreRecovery(id) => {
                let root = self.package_editor.drafts.clone();
                self.open_session(move || crate::recovery::load(&root, &id).map(Session::restore));
            }
            Action::DiscardRecovery(id) => self.remove_recovery(id),
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
                self.opening += 1;
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
                if let Some(s) = &mut self.package_editor.session {
                    s.saving = false;
                }
                match result {
                    Ok(()) => {
                        let s = self.package_editor.session.as_mut().unwrap();
                        s.saved_snapshot(info, project);
                        let clean = !s.dirty();
                        if clean {
                            s.recovery_epoch
                                .store(0, std::sync::atomic::Ordering::Release);
                            let id = s.info.id.clone();
                            self.remove_recovery(id);
                        }
                        self.notice(self.ui.now_t("pkgedit-saved"), false);
                        self.editor_list_drafts();
                        if clean && let Some(next) = self.pending.take() {
                            self.finish_transition(next);
                        }
                    }
                    Err(e) => self.notice(e, true),
                }
                self.editor_publish();
                if self.scan_again {
                    self.scan_again = false;
                    self.poll_directory(session);
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
                self.opening += 1;
                let result = pe::create(kind, id.trim()).map(Box::new);
                self.editor_opened(result);
            }
            Action::OpenInstalled(id) => {
                let registry = self.registry.clone();
                self.open_session(move || pe::open_installed(&registry, &id));
            }
            Action::CopyInstalled(id) => {
                let registry = self.registry.clone();
                self.open_session(move || {
                    let new_id = pe::copy_id(&registry, &id);
                    pe::copy_installed(&registry, &id, &new_id)
                });
            }
            Action::OpenDraft(id) => {
                let dir = self.package_editor.drafts.clone();
                self.open_session(move || pe::open_draft(&dir, &id));
            }
            Action::Opened(result) => self.editor_opened(result),
            Action::OpenFinished { ticket, result } => {
                if ticket == self.opening {
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
                        self.package_editor.session = None;
                        self.editor_publish();
                    }
                    Ok(()) => {}
                    Err(e) => self.notice(e.to_string(), true),
                }
                self.editor_list_drafts();
            }
            Action::Drafts(drafts) => self.ui.package_drafts.set(Arc::new(drafts)),
            Action::Close => {
                self.opening += 1;
                self.transition(Transition::Close);
            }
            Action::ConfirmClose { save } => {
                if save {
                    self.editor_save();
                } else if let Some(next) = self.pending.take() {
                    self.finish_transition(next);
                }
            }
            Action::CancelClose => {
                self.opening += 1;
                self.pending = None;
                self.editor_with(Session::cancel_close);
            }
            Action::Save => {
                self.editor_save();
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
                self.prepare_asset(&path);
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
                    self.prepare_current_asset();
                }
            }
            Action::Validated { ticket, report } => {
                self.editor_with(|s| s.validated(ticket, report));
            }
            Action::ApplyMetadata(metadata) => {
                self.prepare_project(
                    "editor-metadata",
                    "metadata.".into(),
                    move |project, registry| {
                        nanatype_theme::editor::apply_metadata(project, &metadata)
                            .map_err(|e| e.to_string())?;
                        let _ = nanatype_theme::editor::bundle_dependencies(project, registry);
                        Ok(())
                    },
                );
            }
            Action::ApplyData { previous, entry } => self.editor_try(|s| {
                s.apply_data(
                    previous.as_deref(),
                    entry.as_ref().map(|(id, d)| (id.as_str(), d)),
                )?;
                s.applied_forms("data.");
                Ok(())
            }),
            Action::ApplyPipeline { previous, entry } => self.editor_try(|s| {
                s.apply_pipeline(
                    previous.as_deref(),
                    entry.as_ref().map(|(name, p)| (name.as_str(), p)),
                )?;
                s.applied_forms("pipeline.");
                Ok(())
            }),
            Action::LoadKey => self.load_key(false),
            Action::CreateKey => self.load_key(true),
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
                if s.forms.keys().any(|k| !k.ends_with(".editing")) {
                    self.notice(self.ui.now_t("pkgedit-pending-forms"), true);
                    return;
                }
                let project = s.project.clone();
                self.open_session(move || pe::derived(&project, id.trim()));
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

    fn load_key(&mut self, create: bool) {
        if self.key_loading {
            return;
        }
        self.key_loading = true;
        let keys = self.package_editor.keys.clone();
        background("editor-key", &self.ctx, move || {
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
            s.recovery_epoch
                .store(0, std::sync::atomic::Ordering::Release);
            self.remove_recovery(s.info.id.clone());
        }
        self.dialogs.clear();
        self.conflict_copy = None;
        self.replace_path = None;
        self.ui.properties_file.set("skin.toml".into());
        self.ui.properties_path.set(Vec::new());
        self.ui.file_target.set(String::new());
        self.ui.preview_host.skin_drafts.set(Default::default());
        self.ui.preview_simulate.set(false);
        self.ui.preview_size.set(Default::default());
        self.ui.preview_mode.set(0);
        self.ui.preview_viewport.set(Default::default());
        self.ui.canvas_target.set(vec!["dialogue".into()]);
        self.ui.canvas_enabled.set(false);
        self.ui.source_views.set(Default::default());
        self.ui.canvas_preview.set(None);
        self.ui.canvas_selection.set(None);
        self.ui.canvas_geometry.set(Vec::new());
        self.ui.asset.set(None);
        if let Some(audio) = self.audio.take() {
            audio.stop();
        }
        self.watch = None;
        self.scanning_directory = false;
        self.scan_again = false;
        self.package_editor.session = None;
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
                self.ui.mode.set(pe::Mode::Design);
                self.ui
                    .side_open
                    .set(self.ui.workspace_width.get_untracked() >= crate::ui::shell::COMPACT);
                self.poll_directory(id);
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
        self.open_session(move || {
            if path.is_dir() {
                pe::open_directory(&path, &installed, &data)
            } else {
                pe::open_archive(&path)
            }
        });
    }

    /// All asynchronous opens share cancellation and the unsaved-change flow.
    fn open_session(&mut self, open: impl FnOnce() -> Result<Session, String> + Send + 'static) {
        self.opening += 1;
        let ticket = self.opening;
        background("package-editor-open", &self.ctx, move || {
            Msg::PackageEditor(Action::OpenFinished {
                ticket,
                result: open().map(Box::new),
            })
        });
    }

    /// Prepare one transaction off-thread; ProjectPrepared rejects stale results.
    fn prepare_project(
        &self,
        name: &'static str,
        prefix: String,
        apply: impl FnOnce(&mut nanatype_theme::PackageProject, &SkinRegistry) -> Result<(), String>
        + Send
        + 'static,
    ) {
        let Some(s) = &self.package_editor.session else {
            return;
        };
        let (session, version, mut project, registry) =
            (s.id, s.version, s.project.clone(), self.registry.clone());
        background(name, &self.ctx, move || {
            let result = apply(&mut project, &registry).map(|()| project);
            Msg::PackageEditor(Action::ProjectPrepared {
                session,
                version,
                prefix,
                result,
            })
        });
    }

    /// Validate the whole package and its dependency closure, then write it.
    fn editor_export(&mut self, path: PathBuf) {
        let Some(session) = &self.package_editor.session else {
            return;
        };
        if let Err(e) = session.ensure_forms_applied() {
            self.notice(e, true);
            return;
        }
        let project = session.project.clone();
        let registry = self.registry.clone();
        background("package-editor-export", &self.ctx, move || {
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

    fn editor_import(&mut self, path: PathBuf) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let Some(s) = self.package_editor.session.as_ref() else {
            return;
        };
        let session = s.id;
        let folder = s.import_folder();
        background("editor-import", &self.ctx, move || {
            let result = read_asset(path);
            Msg::PackageEditor(Action::Imported {
                session,
                folder,
                name,
                result,
            })
        });
    }

    fn editor_save(&mut self) {
        let Some(s) = &mut self.package_editor.session else {
            return;
        };
        if s.saving {
            return;
        }
        if let Err(e) = s.ensure_forms_applied() {
            self.notice(e, true);
            return;
        }
        s.saving = true;
        let (session, project, mut info) = (s.id, s.project.clone(), s.info.clone());
        info.saved_at = pe::now_s();
        if let Ok(m) = project.manifest() {
            info.package = m.meta.id;
        }
        let dir = self.package_editor.drafts.clone();
        let baseline = s.disk_baseline().clone();
        let target = s.directory().map(PathBuf::from);
        let (installed, data) = (self.paths.packages(), self.paths.data.clone());
        background("editor-save", &self.ctx, move || {
            let result = if let Some(root) = target {
                crate::directory::save(&root, &installed, &data, &baseline, &project)
            } else {
                nanatype_theme::editor::save_draft(&dir, &info, &project).map_err(|e| e.to_string())
            };
            Msg::PackageEditor(Action::SaveFinished {
                session,
                info,
                project,
                result,
            })
        });
        self.editor_publish();
    }

    fn prepare_current_asset(&mut self) {
        if let Some(path) = self
            .package_editor
            .session
            .as_ref()
            .and_then(|s| s.selected_path().map(str::to_owned))
        {
            self.prepare_asset(&path);
        }
    }

    fn prepare_asset(&mut self, path: &str) {
        self.ui.asset.set(None);
        if let Some(audio) = self.audio.take() {
            audio.stop();
        }
        let Some(s) = &self.package_editor.session else {
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
        background("editor-asset-preview", &self.ctx, move || {
            let result = crate::asset::prepare(&path, bytes).map(Arc::new);
            Msg::PackageEditor(Action::AssetPrepared {
                session,
                version,
                path,
                result,
            })
        });
    }

    fn poll_directory(&mut self, id: u64) {
        let Some(s) = &self.package_editor.session else {
            return;
        };
        if s.id != id {
            return;
        }
        let Some(root) = s.directory().map(PathBuf::from) else {
            return;
        };
        if s.saving || self.scanning_directory {
            self.scan_again = true;
            return;
        }
        self.scanning_directory = true;
        let (session, version, installed) = (s.id, s.version, self.paths.packages());
        background("editor-directory-scan", &self.ctx, move || {
            Msg::PackageEditor(Action::DirectoryScanned {
                session,
                version,
                result: crate::directory::read(&root, &installed),
            })
        });
    }

    fn list_recoveries(&self) {
        let root = self.package_editor.drafts.clone();
        background("editor-recoveries", &self.ctx, move || {
            Msg::PackageEditor(Action::Recoveries(crate::recovery::list(&root)))
        });
    }

    fn remove_recovery(&self, id: String) {
        let root = self.package_editor.drafts.clone();
        background("editor-discard-recovery", &self.ctx, move || {
            let _ = crate::recovery::remove(&root, &id);
            Msg::PackageEditor(Action::Recoveries(crate::recovery::list(&root)))
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
        if let Some(s) = &mut self.package_editor.session
            && s.recovery_at().is_some_and(|at| at <= now)
        {
            s.recovering = true;
            let (session, version, epoch, snapshot, root) = (
                s.id,
                s.version,
                s.recovery_epoch.clone(),
                s.recovery(),
                self.package_editor.drafts.clone(),
            );
            background("editor-recovery", &self.ctx, move || {
                Msg::PackageEditor(Action::RecoveryWritten {
                    session,
                    version,
                    result: crate::recovery::write(&root, snapshot, &epoch, version),
                })
            });
        }
        if self.maintenance_at().is_some_and(|at| at <= now) {
            self.editor_publish();
        }
    }

    pub fn editor_list_drafts(&mut self) {
        let dir = self.package_editor.drafts.clone();
        background("package-drafts", &self.ctx, move || {
            Msg::PackageEditor(Action::Drafts(nanatype_theme::editor::list_drafts(&dir)))
        });
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
        let Some(session) = &mut self.package_editor.session else {
            if self.ui.package_editor.with_untracked(Option::is_some) {
                self.ui.package_editor.set(None);
            }
            return;
        };
        let clean_recovery =
            if !session.dirty() && (session.recovered_version != 0 || session.recovering) {
                session.recovered_version = 0;
                session
                    .recovery_epoch
                    .store(0, std::sync::atomic::Ordering::Release);
                Some(session.info.id.clone())
            } else {
                None
            };
        if session
            .validation_at()
            .is_some_and(|at| at <= nanatype_frame::clock::now_ms())
            && let Some((ticket, project)) = session.validation_due()
        {
            let registry = self.registry.clone();
            background("package-editor-check", &self.ctx, move || {
                Msg::PackageEditor(Action::Validated {
                    ticket,
                    report: Arc::new(match std::env::current_exe() {
                        Ok(exe) => pe::validate_with_worker(&registry, &project, &exe),
                        Err(_) => pe::validate(&registry, &project),
                    }),
                })
            });
        }
        let view = session.view(
            self.package_editor.key.as_ref(),
            self.package_editor.key_checked,
        );
        self.ui.package_editor.set(Some(Arc::new(view)));
        if let Some(id) = clean_recovery {
            self.remove_recovery(id);
        }
    }
}
