use super::App;
use crate::msg::{FileTarget, Msg};
use crate::package_editor::{self as pe, Action, Session};
use std::{path::PathBuf, sync::Arc};

impl App {
    /// The package editor. Opening, validation, saving and export run off the
    /// UI thread; scripts only run in the debugger's isolated worker.
    pub(super) fn package_editor_msg(&mut self, action: Action) {
        match action {
            Action::New { kind, id } => {
                let result = pe::create(kind, id.trim()).map(Box::new);
                self.editor_opened(result);
            }
            Action::OpenInstalled(id) => {
                if self.editor_busy() {
                    return;
                }
                let registry = self.registry.clone();
                crate::plugins::background("package-editor-open", &self.ctx, move || {
                    Msg::PackageEditor(Action::Opened(
                        pe::open_installed(&registry, &id).map(Box::new),
                    ))
                });
            }
            Action::CopyInstalled(id) => {
                if self.editor_busy() {
                    return;
                }
                let registry = self.registry.clone();
                crate::plugins::background("package-editor-open", &self.ctx, move || {
                    let new_id = pe::copy_id(&registry, &id);
                    Msg::PackageEditor(Action::Opened(
                        pe::copy_installed(&registry, &id, &new_id).map(Box::new),
                    ))
                });
            }
            Action::OpenDraft(id) => {
                if self.editor_busy() {
                    return;
                }
                let dir = self.package_editor.drafts.clone();
                crate::plugins::background("package-editor-open", &self.ctx, move || {
                    Msg::PackageEditor(Action::Opened(pe::open_draft(&dir, &id).map(Box::new)))
                });
            }
            Action::Opened(result) => self.editor_opened(result),
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
                if let Some(session) = &mut self.package_editor.session
                    && session.request_close()
                {
                    self.package_editor.session = None;
                }
                self.editor_publish();
            }
            Action::ConfirmClose { save } => {
                if save {
                    self.editor_save();
                    if self
                        .package_editor
                        .session
                        .as_ref()
                        .is_some_and(Session::dirty)
                    {
                        return; // Saving failed; keep the work open.
                    }
                }
                self.package_editor.session = None;
                self.editor_publish();
            }
            Action::CancelClose => self.editor_with(Session::cancel_close),
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
            Action::Select(path) => self.editor_with(|s| s.select(&path)),
            Action::Edit { path, text } => {
                let result = self
                    .package_editor
                    .session
                    .as_mut()
                    .map(|s| s.edit(&path, &text));
                if let Some(Err(e)) = result {
                    self.notice(e, true);
                }
                self.editor_publish();
            }
            Action::Format => self.editor_try(Session::format),
            Action::Reveal(diagnostic) => self.editor_with(|s| s.reveal(&diagnostic)),
            Action::Remove(path) => self.editor_try(|s| s.remove(&path)),
            Action::Validated { ticket, report } => {
                self.editor_with(|s| s.validated(ticket, report));
            }
            Action::ApplyMetadata(metadata) => {
                let registry = self.registry.clone();
                self.editor_try(|s| s.apply_metadata(&metadata, &registry));
            }
            Action::ApplyData { previous, entry } => self.editor_try(|s| {
                s.apply_data(
                    previous.as_deref(),
                    entry.as_ref().map(|(id, d)| (id.as_str(), d)),
                )
            }),
            Action::ApplyPipeline { previous, entry } => self.editor_try(|s| {
                s.apply_pipeline(
                    previous.as_deref(),
                    entry.as_ref().map(|(name, p)| (name.as_str(), p)),
                )
            }),
            Action::LoadKey => {
                match self.package_editor.keys.load() {
                    Ok(key) => self.package_editor.key = key,
                    Err(e) => self.notice(
                        format!("{}: {e}", self.ui.now_t("pkgedit-key-unavailable")),
                        true,
                    ),
                }
                self.package_editor.key_checked = true;
                self.editor_publish();
            }
            Action::CreateKey => {
                if self.package_editor.key.is_none() {
                    let created = nanatype_theme::SealIdentity::generate()
                        .map_err(|e| e.to_string())
                        .and_then(|key| self.package_editor.keys.save(&key).map(|()| key));
                    match created {
                        Ok(key) => self.package_editor.key = Some(key),
                        Err(e) => self.notice(
                            format!("{}: {e}", self.ui.now_t("pkgedit-key-unavailable")),
                            true,
                        ),
                    }
                    self.package_editor.key_checked = true;
                }
                self.editor_publish();
            }
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
            Action::Derive(id) => self.editor_try(|s| s.derive(id.trim())),
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

    /// Another project is open with unsaved changes: say so instead of
    /// replacing it.
    fn editor_busy(&mut self) -> bool {
        let dirty = self
            .package_editor
            .session
            .as_ref()
            .is_some_and(Session::dirty);
        if dirty {
            self.notice(self.ui.now_t("pkgedit-unsaved-open"), true);
        }
        dirty
    }

    fn editor_opened(&mut self, result: Result<Box<Session>, String>) {
        match result {
            Ok(session) => {
                if self.editor_busy() {
                    return;
                }
                self.package_editor.session = Some(*session);
                self.ui.package_editor_tab.set(pe::Tab::Files);
                self.ui.page.set(crate::ui::settings::Page::Packages);
            }
            Err(e) => self.notice(
                format!("{}: {e}", self.ui.now_t("pkgedit-open-failed")),
                true,
            ),
        }
        self.editor_publish();
    }

    pub(super) fn editor_open_archive(&mut self, path: PathBuf) {
        if self.editor_busy() {
            return;
        }
        crate::plugins::background("package-editor-open", &self.ctx, move || {
            Msg::PackageEditor(Action::Opened(pe::open_archive(&path).map(Box::new)))
        });
    }

    /// Validate the whole package and its dependency closure, then write it.
    pub(super) fn editor_export(&mut self, path: PathBuf) {
        let Some(session) = &self.package_editor.session else {
            return;
        };
        let project = session.project.clone();
        let registry = self.registry.clone();
        crate::plugins::background("package-editor-export", &self.ctx, move || {
            Msg::PackageEditor(Action::Exported(pe::export(&registry, &project, &path)))
        });
    }

    pub(super) fn editor_import(&mut self, path: PathBuf) {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        // Read no more than the per-file budget plus one byte.
        let bytes = std::fs::File::open(&path).and_then(|file| {
            use std::io::Read;
            let mut bytes = Vec::new();
            file.take(nanatype_theme::MAX_FILE_BYTES + 1)
                .read_to_end(&mut bytes)
                .map(|_| bytes)
        });
        match bytes {
            Ok(bytes) => self.editor_try(|s| s.import(&name, bytes).map(|_| ())),
            Err(e) => self.notice(e.to_string(), true),
        }
    }

    fn editor_save(&mut self) {
        let dir = self.package_editor.drafts.clone();
        let result = self.package_editor.session.as_mut().map(|s| s.save(&dir));
        match result {
            Some(Ok(())) => {
                self.notice(self.ui.now_t("pkgedit-saved"), false);
                self.editor_list_drafts();
            }
            Some(Err(e)) => self.notice(e, true),
            None => {}
        }
    }

    pub(super) fn editor_list_drafts(&mut self) {
        let dir = self.package_editor.drafts.clone();
        crate::plugins::background("package-drafts", &self.ctx, move || {
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
        if let Some((ticket, project)) = session.validation_due() {
            let registry = self.registry.clone();
            crate::plugins::background("package-editor-check", &self.ctx, move || {
                Msg::PackageEditor(Action::Validated {
                    ticket,
                    report: Arc::new(pe::validate(&registry, &project)),
                })
            });
        }
        let view = session.view(
            self.package_editor.key.as_ref(),
            self.package_editor.key_checked,
        );
        self.ui.package_editor.set(Some(Arc::new(view)));
    }
}
