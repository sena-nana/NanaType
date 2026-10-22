use super::App;
use crate::msg::{FileTarget, Msg};
use crate::services::sounds;
use nanatype_theme::TomlTable;

impl App {
    /// Skin files, previews, personalization and the skin catalogue.
    pub(super) fn skin_msg(&mut self, msg: Msg) {
        match msg {
            Msg::DeleteSkin(id) => {
                if let Err(e) = self.registry.remove(&id) {
                    let text = self
                        .ui
                        .tr("skins-delete-failed", &[("message", e.to_string().into())]);
                    self.notice(text, true);
                }
                self.refresh_skin_rows();
                self.sync_tray_skins();
                if id == self.settings.skin.id {
                    self.edit(|s| s.skin.id = nanatype_theme::DEFAULT_SKIN.into());
                }
            }
            Msg::ExportSkin(id) => {
                self.export_target = Some(id);
                self.pick(FileTarget::ExportSkin);
            }
            Msg::DuplicateSkin(base) => self.duplicate_skin(&base),
            Msg::SkinFilesChanged => {
                self.request_skin_reload(true);
            }
            Msg::LayoutMaterialsReady => self.ui.layout_epoch.update(|e| *e += 1),
            Msg::SkinScanReady { ticket, result } => match self.skin_reload.finish(ticket) {
                Err(latest) => {
                    if let Ok(prepared) = result {
                        crate::ui::retire_skin_urls(
                            self.ui,
                            prepared
                                .image_urls
                                .into_iter()
                                .flat_map(|urls| urls.into_values()),
                        );
                    }
                    self.scan_skins(latest);
                }
                Ok(()) => match result {
                    Ok(prepared) => {
                        self.skin_reload_scan = false;
                        self.registry = prepared.registry;
                        self.reapply_workshop();
                        self.prepared_skin_urls = prepared.image_urls;
                        self.apply_loaded_skin(prepared.loaded);
                        if prepared.scanned {
                            self.refresh_skin_rows();
                        }
                    }
                    Err(error) => self.notice(error, true),
                },
            },
            Msg::PrepareSkinPreview { id, play } => {
                if let Err(errors) =
                    crate::ui::prepare_skin_preview(self.ui, &self.registry, &id, play)
                {
                    self.notice(
                        errors
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join("\n"),
                        true,
                    );
                }
            }
            Msg::SaveSkinPersonalization { id, mut profile } => {
                if profile.valid() && self.registry.entries().iter().any(|r| r.id == id) {
                    // Keep only values the skin's params accept and that
                    // differ from their defaults.
                    match self.registry.load(&id) {
                        Ok(loaded) => {
                            nanatype_theme::retain_valid_params(&loaded.skin, &mut profile.params)
                        }
                        Err(_) => profile.params.clear(),
                    }
                    let active = id == self.settings.skin.id;
                    self.edit(move |s| {
                        s.skin.personalizations.insert(id, profile);
                    });
                    if active {
                        self.apply_saved_sizes();
                    }
                }
            }
            Msg::SoundsLoaded {
                skin,
                pack,
                bank,
                errors,
            } => {
                if skin == self.loaded.skin.meta.id
                    && pack == self.sound_pack.as_ref().map(|p| p.id.clone())
                {
                    sounds::install(&self.audio, &self.bank, &bank);
                    self.bank = bank;
                    if let Some(e) = errors.first() {
                        self.notice(e.clone(), true);
                    }
                }
            }
            _ => unreachable!("not a skin message"),
        }
    }

    fn duplicate_skin(&mut self, base: &str) {
        let mut n = 1;
        let id = loop {
            let candidate = format!(
                "{base}-copy{}",
                if n == 1 { String::new() } else { n.to_string() }
            );
            let candidate = candidate.replace("nanatype.", "my.");
            if self.registry.entry(&candidate).is_none() {
                break candidate;
            }
            n += 1;
        };
        let locale = self.ui.locale.get_untracked();
        let base_name = self
            .ui
            .skins
            .get_untracked()
            .iter()
            .find(|r| r.id == base)
            .map(|r| r.name.clone())
            .unwrap_or_else(|| base.to_owned());
        let mut name = nanatype_theme::Localized::new();
        name.insert(locale.tag().to_owned(), format!("{base_name} *"));
        // Bake the current personalisation into the copy.
        let mut over = TomlTable::new();
        let profile = self.settings.skin.personalizations.get(base).cloned();
        let mut legacy = self.settings.skin.overrides.clone();
        if profile.is_some() {
            legacy.text_size = None;
            legacy.text_color = None;
        }
        let o = &legacy;
        let mut dialogue = TomlTable::new();
        let mut text = TomlTable::new();
        if let Some(c) = o.text_color {
            text.insert("color".into(), c.to_hex().into());
        }
        if let Some(sz) = o.text_size {
            text.insert("size".into(), f64::from(sz).into());
        }
        if !text.is_empty() {
            dialogue.insert("text".into(), text.into());
        }
        if let Some(c) = o.box_color {
            let mut b = TomlTable::new();
            b.insert("color".into(), c.to_hex().into());
            dialogue.insert("box".into(), b.into());
        }
        if let Some(c) = o.name_color {
            let mut p = TomlTable::new();
            p.insert("color".into(), c.to_hex().into());
            dialogue.insert("nameplate".into(), p.into());
        }
        if !dialogue.is_empty() {
            over.insert("dialogue".into(), dialogue.into());
        }
        if let Some(c) = o.accent {
            let mut ch = TomlTable::new();
            ch.insert("accent".into(), c.to_hex().into());
            over.insert("chrome".into(), ch.into());
        }
        // Param values become the copy's defaults, so the copy carries the
        // look on its own and its profile starts without them.
        let mut profile = profile;
        if let Some(p) = profile.as_mut().filter(|p| !p.params.is_empty()) {
            let mut params = TomlTable::new();
            for (key, value) in std::mem::take(&mut p.params) {
                let value = match value {
                    nanatype_model::ParamValue::Bool(b) => b.into(),
                    nanatype_model::ParamValue::Number(n) => n.into(),
                    nanatype_model::ParamValue::Text(t) => t.into(),
                };
                let mut spec = TomlTable::new();
                spec.insert("default".into(), value);
                params.insert(key, spec.into());
            }
            over.insert("params".into(), params.into());
        }
        match self.registry.duplicate(base, &id, &name, &over) {
            Ok(dir) => {
                self.refresh_skin_rows();
                self.sync_tray_skins();
                self.edit(|s| {
                    if let Some(profile) = profile {
                        s.skin.personalizations.insert(id.clone(), profile);
                    }
                    s.skin.id = id;
                    s.skin.overrides = Default::default();
                });
                crate::services::reveal(&dir);
            }
            Err(errors) => {
                let detail = errors.first().map(|d| d.to_string()).unwrap_or_default();
                self.notice(detail, true);
            }
        }
    }
}
