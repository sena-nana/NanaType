//! Settings management (Settings › General): export to a file, import one
//! after a confirmation, reset to defaults, and profiles. An import, reset
//! or restore first keeps the settings as they were in
//! `settings.previous.json`, so "Restore previous settings" undoes it.
//! Secrets stay in the system's secret store throughout.
use std::path::PathBuf;
use std::sync::Arc;

use nanatype_settings::{
    ExportOptions, GeometryStore, ImportError, Incoming, LoadNote, PREVIOUS_FILE, PROFILE_DIR,
    Profile, Settings,
};

use super::App;
use crate::msg::{BackupMsg, FileTarget, GlobalAction, Msg, ProfileMsg, Shortcut};
use crate::plugins::background;

/// Where imported avatars are written, under the data folder.
const AVATAR_DIR: &str = "avatars";

#[derive(Default)]
pub(super) struct Backup {
    /// The checked import the confirmation is open for.
    pending: Option<Box<Incoming>>,
    /// The settings before the last import, reset or restore, while their
    /// file may still be on its way to disk.
    previous: Option<Box<Settings>>,
    profiles: Vec<Profile>,
}

/// What the page shows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct View {
    pub options: ExportOptions,
    /// The import waiting for confirmation.
    pub pending: Option<Pending>,
    /// Whether there are earlier settings to go back to.
    pub has_previous: bool,
    /// `(id, name)`, in list order.
    pub profiles: Vec<(String, String)>,
}

/// An import's summary, for its confirmation.
#[derive(Debug, Clone, PartialEq)]
pub struct Pending {
    /// The settings sections it changes, by their key in settings.json.
    pub sections: Vec<&'static str>,
    pub from_schema: u32,
    pub version: String,
    pub avatars: usize,
    pub positions: bool,
    pub integrations: bool,
}

/// The label of a settings section, by its key in settings.json.
fn section_key(section: &str) -> &'static str {
    match section {
        "general" => "backup-section-general",
        "playback" => "backup-section-playback",
        "sound" => "backup-section-sound",
        "speech" => "backup-section-speech",
        "tts" => "backup-section-tts",
        "translate" => "backup-section-translate",
        "skin" => "backup-section-skin",
        "overlays" => "backup-section-overlays",
        "history" => "backup-section-history",
        "hotkeys" => "backup-section-hotkeys",
        "output" => "backup-section-output",
        "integrations" => "backup-section-integrations",
        "assets" => "backup-section-assets",
        "update" => "backup-section-update",
        _ => "backup-section-enabled-plugins",
    }
}

/// The confirmation's text for `p`.
pub fn summary(ui: crate::ui::Ui, p: &Pending) -> String {
    let mut lines = Vec::new();
    let names: Vec<String> = p
        .sections
        .iter()
        .map(|k| ui.now_t(section_key(k)))
        .collect();
    lines.push(ui.tr(
        "backup-import-changes",
        &[("sections", names.join(" · ").into())],
    ));
    if p.from_schema < nanatype_settings::SETTINGS_SCHEMA {
        let version = if p.version.is_empty() {
            "?".to_owned()
        } else {
            p.version.clone()
        };
        lines.push(ui.tr("backup-import-older", &[("version", version.into())]));
    }
    if !p.integrations {
        lines.push(ui.now_t("backup-import-no-integrations"));
    }
    if p.avatars > 0 {
        lines.push(ui.tr(
            "backup-import-avatars",
            &[("count", (p.avatars as u64).into())],
        ));
    }
    if p.positions {
        lines.push(ui.now_t("backup-import-positions"));
    }
    lines.push(ui.now_t("backup-import-undo"));
    lines.join("\n")
}

impl App {
    fn previous_path(&self) -> PathBuf {
        self.paths.config.join(PREVIOUS_FILE)
    }

    fn profile_dir(&self) -> PathBuf {
        self.paths.config.join(PROFILE_DIR)
    }

    /// Read the profiles and whether a backup is there.
    pub(super) fn backup_start(&mut self) {
        let (profiles, skipped) = nanatype_settings::load_profiles(&self.profile_dir());
        for problem in skipped {
            nanatype_diag::report!(crate::diag::PERSIST_FAILED; "profile skipped: {problem}");
        }
        self.backup.profiles = profiles;
        let has_previous = self.previous_path().exists();
        self.edit_backup_view(|v| v.has_previous = has_previous);
        self.sync_profiles();
    }

    fn edit_backup_view(&self, f: impl FnOnce(&mut View)) {
        let mut view = (*self.ui.backup.get_untracked()).clone();
        f(&mut view);
        self.ui.backup.set(Arc::new(view));
    }

    /// Settings from a newer NanaType are read-only: nothing replaces them.
    fn refuse_readonly(&mut self) -> bool {
        if self.readonly_settings {
            let text = self.ui.now_t("err-settings-newer");
            self.notice(text, true);
        }
        self.readonly_settings
    }

    /// Keep the settings as they are now, then put `next` in their place.
    fn replace_settings(&mut self, next: Settings) {
        self.persist.json(self.previous_path(), &self.settings);
        self.backup.previous = Some(Box::new(self.settings.clone()));
        self.edit_backup_view(|v| v.has_previous = true);
        self.apply_settings(next);
        self.sync_profiles();
    }

    pub(super) fn backup_msg(&mut self, msg: BackupMsg) {
        match msg {
            BackupMsg::Export => self.pick(FileTarget::ExportSettings),
            BackupMsg::Exported(result) => {
                let text = match &result {
                    Ok((path, 0)) => self.ui.tr(
                        "backup-exported",
                        &[("path", path.display().to_string().into())],
                    ),
                    Ok((path, missing)) => self.ui.tr(
                        "backup-exported-missing",
                        &[
                            ("path", path.display().to_string().into()),
                            ("count", (*missing as u64).into()),
                        ],
                    ),
                    Err(message) => self
                        .ui
                        .tr("backup-failed", &[("message", message.clone().into())]),
                };
                self.notice(text, result.is_err());
            }
            BackupMsg::Import => self.pick(FileTarget::ImportSettings),
            BackupMsg::Checked(Err(error)) => {
                let text = match error {
                    ImportError::TooNew(_) | ImportError::FormatTooNew(_) => {
                        self.ui.now_t("backup-import-newer")
                    }
                    ImportError::NotSettings => self.ui.now_t("backup-import-not-settings"),
                    ImportError::Unreadable(message) => {
                        self.ui.tr("backup-failed", &[("message", message.into())])
                    }
                };
                self.notice(text, true);
            }
            BackupMsg::Checked(Ok(incoming)) => {
                if self.refuse_readonly() {
                    return;
                }
                let merged = incoming.merge(&self.settings);
                let pending = Pending {
                    sections: nanatype_settings::changed_sections(&self.settings, &merged),
                    from_schema: incoming.from_schema,
                    version: incoming.version.clone(),
                    avatars: incoming.avatars.len(),
                    positions: incoming.geometry.is_some(),
                    integrations: incoming.includes.integrations,
                };
                if pending.sections.is_empty() && pending.avatars == 0 && !pending.positions {
                    let text = self.ui.now_t("backup-import-same");
                    self.notice(text, false);
                    return;
                }
                self.backup.pending = Some(incoming);
                self.edit_backup_view(|v| v.pending = Some(pending));
            }
            BackupMsg::Confirm(accept) => {
                let incoming = self.backup.pending.take();
                self.edit_backup_view(|v| v.pending = None);
                let Some(incoming) = incoming.filter(|_| accept) else {
                    return;
                };
                let merged = incoming.merge(&self.settings);
                let geometry = incoming.geometry.clone().map(Box::new);
                if incoming.avatars.is_empty() {
                    self.update(Msg::Backup(BackupMsg::Unpacked(Box::new(merged), geometry)));
                    return;
                }
                let dir = self.paths.data.join(AVATAR_DIR);
                background("settings-avatars", &self.ctx, move || {
                    let mut merged = merged;
                    let now = nanatype_model::unix_millis();
                    if let Err(e) = incoming.unpack_avatars(&dir, &mut merged, now) {
                        nanatype_diag::report!(crate::diag::PERSIST_FAILED; "{}: {e}", dir.display());
                    }
                    Msg::Backup(BackupMsg::Unpacked(Box::new(merged), geometry))
                });
            }
            BackupMsg::Unpacked(settings, geometry) => {
                self.replace_settings(*settings);
                if let Some(geometry) = geometry {
                    self.windows.replace_geometry(*geometry);
                    self.save_geometry();
                }
                let text = self.ui.now_t("backup-imported");
                self.notice(text, false);
            }
            BackupMsg::Reset => {
                if self.refuse_readonly() {
                    return;
                }
                let next = self.settings.reset_from();
                self.replace_settings(next);
                let text = self.ui.now_t("backup-reset-done");
                self.notice(text, false);
            }
            BackupMsg::Restore => {
                if self.refuse_readonly() {
                    return;
                }
                if let Some(previous) = self.backup.previous.take() {
                    self.update(Msg::Backup(BackupMsg::Restored(Ok(previous))));
                    return;
                }
                let path = self.previous_path();
                background("settings-restore", &self.ctx, move || {
                    let loaded = nanatype_settings::load_settings(&path, None);
                    let result = match loaded.note {
                        LoadNote::Loaded | LoadNote::Migrated { .. } => Ok(Box::new(loaded.value)),
                        LoadNote::Fresh => Err(path.display().to_string()),
                        LoadNote::TooNew { .. } => Err("newer schema".into()),
                        LoadNote::Quarantined { reason, .. } => Err(reason),
                    };
                    Msg::Backup(BackupMsg::Restored(result))
                });
            }
            BackupMsg::Restored(Ok(previous)) => {
                // The settings restored over are kept in turn: restoring
                // again goes back.
                self.replace_settings(*previous);
                let text = self.ui.now_t("backup-restored");
                self.notice(text, false);
            }
            BackupMsg::Restored(Err(message)) => {
                let text = self.ui.tr("backup-failed", &[("message", message.into())]);
                self.notice(text, true);
            }
        }
    }

    /// The export dialog's file name: `NanaType settings 2026-11-02.json`.
    pub(super) fn settings_export_name(&self) -> String {
        let name = crate::archive::session_name(
            nanatype_model::unix_millis(),
            crate::localtime::offset_seconds(),
        );
        format!("NanaType settings {}.json", name.get(..10).unwrap_or(&name))
    }

    /// Write an export to `path`, reading avatars off the UI thread.
    pub(super) fn settings_export(&mut self, path: PathBuf) {
        let settings = self.settings.clone();
        let geometry: GeometryStore = self.windows.geometry.clone();
        let options = self.ui.backup.get_untracked().options;
        background("settings-export", &self.ctx, move || {
            let exported = nanatype_settings::export(
                &settings,
                &geometry,
                options,
                env!("CARGO_PKG_VERSION"),
                nanatype_model::unix_millis(),
            );
            let result = serde_json::to_vec_pretty(&exported.envelope)
                .map_err(|e| e.to_string())
                .and_then(|bytes| std::fs::write(&path, bytes).map_err(|e| e.to_string()))
                .map(|()| (path, exported.missing_avatars));
            Msg::Backup(BackupMsg::Exported(result))
        });
    }

    /// Read and check the file at `path` off the UI thread.
    pub(super) fn settings_import(&mut self, path: PathBuf) {
        background("settings-import", &self.ctx, move || {
            let result = std::fs::read_to_string(&path)
                .map_err(|e| ImportError::Unreadable(e.to_string()))
                .and_then(|text| nanatype_settings::parse_import(&text))
                .map(Box::new);
            Msg::Backup(BackupMsg::Checked(result))
        });
    }

    // ---- profiles -------------------------------------------------------------

    fn save_profile(&self, profile: &Profile) {
        self.persist.json(
            Profile::path(&self.profile_dir(), &profile.id),
            &profile.file(nanatype_model::unix_millis()),
        );
    }

    fn new_profile_id(&self) -> String {
        nanatype_settings::new_profile_id(nanatype_model::unix_millis(), |id| {
            self.backup.profiles.iter().any(|p| p.id == id)
        })
    }

    /// Save the current settings into profile `id`; returns its name.
    fn store_profile(&mut self, id: &str) -> Option<String> {
        let settings = nanatype_settings::profile_sections(&self.settings);
        let p = self.backup.profiles.iter_mut().find(|p| p.id == id)?;
        p.settings = settings;
        let p = p.clone();
        self.save_profile(&p);
        Some(p.name)
    }

    /// The page's list, the tray's submenu and the profile hotkeys follow
    /// the profiles.
    fn sync_profiles(&mut self) {
        let list: Vec<(String, String)> = self
            .backup
            .profiles
            .iter()
            .map(|p| (p.id.clone(), p.name.clone()))
            .collect();
        let keys = profile_hotkeys(&self.settings.hotkeys.profiles, &list);
        if keys != self.settings.hotkeys.profiles {
            self.edit(move |s| s.hotkeys.profiles = keys);
        }
        if let Some(tray) = &mut self.tray {
            tray.set_profiles(&list, self.settings.general.profile.as_deref());
        }
        self.edit_backup_view(|v| v.profiles = list);
    }

    fn profile_name(&self, name: &str) -> String {
        let name: String = name.trim().chars().take(64).collect();
        if !name.is_empty() {
            return name;
        }
        let n = self.backup.profiles.len() as u64 + 1;
        self.ui.tr("profile-unnamed", &[("n", n.into())])
    }

    pub(super) fn profile_msg(&mut self, msg: ProfileMsg) {
        match msg {
            ProfileMsg::SaveAs(name) => {
                let id = self.new_profile_id();
                let name = self.profile_name(&name);
                let profile = Profile::capture(id.clone(), name, &self.settings);
                self.save_profile(&profile);
                self.backup.profiles.push(profile);
                // The settings in use are now this profile's.
                self.edit(move |s| s.general.profile = Some(id));
                self.sync_profiles();
            }
            ProfileMsg::Update(id) => {
                if let Some(name) = self.store_profile(&id) {
                    let text = self.ui.tr("profile-updated", &[("name", name.into())]);
                    self.notice(text, false);
                }
            }
            ProfileMsg::Switch(id) => {
                if self.refuse_readonly() {
                    return;
                }
                let Some(target) = self.backup.profiles.iter().find(|p| p.id == id).cloned() else {
                    return;
                };
                // What changed under the active profile goes back into it.
                if let Some(active) = self.settings.general.profile.clone()
                    && active != id
                {
                    self.store_profile(&active);
                }
                let mut next = target.apply(&self.settings);
                next.general.profile = Some(id);
                // Hotkeys the profile brings that clash stay unregistered
                // (`hotkey_bindings`); say which, once.
                let brought: Vec<String> = GlobalAction::new_clashes(&self.settings, &next)
                    .into_iter()
                    .map(|action| {
                        crate::ui::settings::action_name(
                            self.ui.locale.get_untracked(),
                            &next,
                            Shortcut::Global(action),
                        )
                    })
                    .collect();
                self.apply_settings(next);
                self.sync_profiles();
                let text = self
                    .ui
                    .tr("profile-switched", &[("name", target.name.into())]);
                self.notice(text, false);
                if !brought.is_empty() {
                    let names = brought.join(&self.ui.now_t("peer-name-separator"));
                    let text = self
                        .ui
                        .tr("profile-hotkeys-clash", &[("names", names.into())]);
                    self.notice(text, true);
                }
            }
            ProfileMsg::Next => {
                let profiles = &self.backup.profiles;
                if profiles.is_empty() {
                    return;
                }
                let active = self.settings.general.profile.as_deref();
                let next = profiles
                    .iter()
                    .position(|p| Some(p.id.as_str()) == active)
                    .map_or(0, |i| (i + 1) % profiles.len());
                let id = profiles[next].id.clone();
                self.update(Msg::Profile(ProfileMsg::Switch(id)));
            }
            ProfileMsg::Rename { id, name } => {
                let name = self.profile_name(&name);
                if let Some(p) = self.backup.profiles.iter_mut().find(|p| p.id == id) {
                    p.name = name;
                    let p = p.clone();
                    self.save_profile(&p);
                    self.sync_profiles();
                }
            }
            ProfileMsg::Duplicate(id) => {
                let Some(source) = self.backup.profiles.iter().find(|p| p.id == id) else {
                    return;
                };
                let name = self.ui.tr(
                    "profile-copy-name",
                    &[("name", source.name.as_str().into())],
                );
                let copy = Profile {
                    id: self.new_profile_id(),
                    name,
                    settings: source.settings.clone(),
                };
                self.save_profile(&copy);
                let at = self
                    .backup
                    .profiles
                    .iter()
                    .position(|p| p.id == id)
                    .map_or(self.backup.profiles.len(), |i| i + 1);
                self.backup.profiles.insert(at, copy);
                self.sync_profiles();
            }
            ProfileMsg::Delete(id) => {
                self.backup.profiles.retain(|p| p.id != id);
                self.persist.remove(Profile::path(&self.profile_dir(), &id));
                if self.settings.general.profile.as_deref() == Some(id.as_str()) {
                    self.edit(|s| s.general.profile = None);
                }
                self.sync_profiles();
            }
        }
    }
}

/// One hotkey entry per profile in `list` (id, name), in its order: kept
/// bindings, current names, none for a new profile and none for one gone.
fn profile_hotkeys(
    keys: &[nanatype_settings::ProfileHotkey],
    list: &[(String, String)],
) -> Vec<nanatype_settings::ProfileHotkey> {
    list.iter()
        .map(|(id, name)| nanatype_settings::ProfileHotkey {
            id: id.clone(),
            name: name.clone(),
            hotkey: keys
                .iter()
                .find(|k| &k.id == id)
                .and_then(|k| k.hotkey.clone()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nanatype_settings::{Hotkey, ProfileHotkey};

    #[test]
    fn profile_hotkeys_follow_the_profiles() {
        let key = |id: &str, name: &str, hotkey: Option<&str>| ProfileHotkey {
            id: id.into(),
            name: name.into(),
            hotkey: hotkey.map(Hotkey::bare),
        };
        let keys = [key("a", "Old", Some("F2")), key("gone", "Gone", Some("F3"))];
        let list = [
            ("b".to_owned(), "New".to_owned()),
            ("a".to_owned(), "Renamed".to_owned()),
        ];
        assert_eq!(
            profile_hotkeys(&keys, &list),
            [key("b", "New", None), key("a", "Renamed", Some("F2"))]
        );
    }
}
