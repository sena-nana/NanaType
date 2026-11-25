//! Visual appearance, theme compilation, font scopes, and skin watch state.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use nana_ui::CompiledTheme;
use nanatype_theme::{LoadedSkin, Skin, SkinRegistry};

pub struct AppearanceState {
    pub registry: SkinRegistry,
    pub loaded: LoadedSkin,
    pub skin: Arc<Skin>,
    pub theme: Arc<CompiledTheme>,
    pub output_theme: Arc<CompiledTheme>,
    pub system_dark: bool,
    pub system_high_contrast: bool,
    pub fonts: nana_ui::HostFontScope,
    pub builtin_fonts: crate::skin_fonts::BuiltinFontSet<nana_ui::HostFontScope>,
    pub skin_families: [Option<String>; 3],
    pub skin_latin_family: Option<String>,
    pub skin_watch: Option<crate::services::watch::Watch>,
    pub skin_reload: crate::services::watch::ReloadGate,
    /// The newest skin preview asked for; older ones are dropped when ready.
    pub skin_preview_ticket: u64,
    pub skin_reload_scan: bool,
    pub prepared_skin_urls: Option<HashMap<String, String>>,
    /// Saved skin param warnings already shown in this session.
    pub warned_skin_params: HashSet<String>,
    pub export_target: Option<String>,
}

impl AppearanceState {
    pub fn editor_look(&self, sticker_scale: f32) -> crate::ui::rich::Look {
        let mut base = crate::ui::dialogue::Base::editor(&self.skin);
        base.sticker_scale = sticker_scale;
        base.look()
    }

    pub fn editor_rich(
        &self,
        doc: &nanatype_model::Document,
        sticker_scale: f32,
        urls: &HashMap<String, String>,
        locale: nanatype_i18n::Locale,
    ) -> nana_ui_core::RichText {
        let look = self.editor_look(sticker_scale);
        crate::ui::rich::to_rich(
            doc,
            &crate::ui::rich::Env {
                look: &look,
                urls,
                editor: true,
                locale,
            },
        )
    }
}

impl super::App {
    /// Tint ordinary windows' system title bars with the application chrome.
    pub fn tint_captions(&self) {
        for kind in [
            crate::windows::WindowKind::Input,
            crate::windows::WindowKind::Settings,
        ] {
            if let Some(handle) = self.shell.windows.handle(kind) {
                crate::windows::tint_caption(handle, &self.appearance.theme);
            }
        }
    }

    pub(super) fn refresh_skin_rows(&mut self) {
        self.appearance.skin_reload.invalidate();
        let previous = self.ui.skins.with_untracked(|rows| {
            rows.iter()
                .flat_map(|r| r.image_urls.values().cloned())
                .collect::<Vec<_>>()
        });
        let locale = self.ui.locale.get_untracked();
        let off = &self.settings.disabled_packages;
        let mut rows = crate::ui::skin_rows(&self.appearance.registry, locale);
        rows.retain(|r| !off.contains(&r.id));
        self.ui.skins.set(Arc::new(rows));
        crate::ui::retire_skin_urls(self.ui, previous);
        let mut packs = crate::ui::sound_pack_rows(&self.appearance.registry, locale);
        packs.retain(|p| !off.contains(&p.id));
        self.ui.sound_packs.set(Arc::new(packs));
        if self.sync_sound_pack() {
            self.apply_skin();
            self.load_sounds();
        }
    }

    pub(super) fn sync_tray_skins(&mut self) {
        let skins: Vec<(String, String)> = self
            .ui
            .skins
            .get_untracked()
            .iter()
            .filter(|r| r.error.is_none())
            .map(|r| (r.id.clone(), r.name.clone()))
            .collect();
        if let Some(tray) = &mut self.shell.tray {
            tray.set_skins(&skins, &self.settings.skin.id);
        }
    }

    pub(super) fn sync_tray_characters(&mut self) {
        let g = &self.settings.general;
        let names: Vec<String> = g
            .characters
            .iter()
            .enumerate()
            .map(|(i, c)| crate::ui::character_name(self.ui, i, c))
            .collect();
        if let Some(tray) = &mut self.shell.tray {
            tray.set_characters(&names, g.character);
        }
    }

    pub(super) fn reload_skin(&mut self, from_scan: bool) {
        if !from_scan {
            self.request_skin_reload(false);
            return;
        }
        // A watcher/install already validated this scan. A manual switch must
        // re-read inactive authoring folders, which are not being watched.
        let result = self
            .appearance
            .registry
            .load_scanned(&self.settings.skin.id);
        self.apply_loaded_skin(result);
    }

    pub(super) fn apply_loaded_skin(
        &mut self,
        result: Result<LoadedSkin, Vec<nanatype_theme::SkinDiagnostic>>,
    ) {
        let (loaded, fallback) = match result {
            Ok(loaded) => (loaded, None),
            // Invalid edits keep the author's last working skin.
            Err(errors) if self.settings.skin.id == self.appearance.loaded.skin.meta.id => {
                self.notice(
                    errors
                        .iter()
                        .map(ToString::to_string)
                        .collect::<Vec<_>>()
                        .join("\n"),
                    true,
                );
                return;
            }
            Err(errors) => (
                self.appearance
                    .registry
                    .load(nanatype_theme::DEFAULT_SKIN)
                    .expect("built-in default loads"),
                Some(errors),
            ),
        };
        if let Some(errors) = fallback {
            let detail = errors.first().map(|d| d.to_string()).unwrap_or_default();
            let text = self.ui.tr(
                "skins-fallback",
                &[("id", self.settings.skin.id.as_str().into())],
            );
            self.notice(format!("{text}: {detail}"), true);
        }
        let changed_files = loaded.skin.meta.id != self.appearance.loaded.skin.meta.id
            || loaded.skin.sounds != self.appearance.loaded.skin.sounds;
        self.appearance.loaded = loaded.clone();
        self.media.loaded_skin = Some(loaded);
        self.register_fonts();
        self.apply_skin();
        self.apply_saved_sizes();
        if changed_files {
            self.load_sounds();
        }
        self.refresh_assets();
        self.refresh_watches();
    }

    pub(super) fn scan_skins(&mut self, ticket: u64) {
        let mut registry = self.appearance.registry.clone();
        let scanned = self.appearance.skin_reload_scan;
        let id = self.settings.skin.id.clone();
        if let Err(error) = crate::plugins::try_background("skin-scan", &self.ctx, move || {
            let result = crate::plugins::caught("Skin scan worker failed", || {
                if scanned {
                    registry.rescan();
                }
                let loaded = if scanned {
                    registry.load_scanned(&id)
                } else {
                    registry.load(&id)
                };
                let image_urls = loaded
                    .as_ref()
                    .ok()
                    .map(|skin| crate::assets::skin_preview_urls(&skin.skin, &*skin.files));
                Box::new(crate::services::watch::PreparedSkins {
                    registry,
                    loaded,
                    scanned,
                    image_urls,
                })
            });
            crate::msg::Msg::Skin(crate::msg::SkinMsg::ScanReady { ticket, result })
        }) {
            self.appearance.skin_reload.cancel();
            self.notice(error.to_string(), true);
        }
    }

    pub(super) fn request_skin_reload(&mut self, scan: bool) {
        self.appearance.skin_reload_scan |= scan;
        if let Some(ticket) = self.appearance.skin_reload.request() {
            self.scan_skins(ticket);
        }
    }

    /// Register the skin's packaged fonts, replacing the previous skin's.
    /// Built-in fonts (`builtin:`) are kept registered across skins that
    /// share them; a file named by several roles is registered once.
    pub(super) fn register_fonts(&mut self) {
        self.appearance.fonts.clear();
        let f = self.appearance.loaded.skin.fonts.clone();
        let files = self.appearance.loaded.files.clone();
        let roles = [&f.body, &f.name, &f.accent];
        let mut wanted: Vec<String> = roles
            .iter()
            .copied()
            .chain(f.latin.as_ref())
            .filter_map(|s| s.file.clone())
            .filter(|p| nanatype_theme::is_builtin_font(p))
            .collect();
        if let Some(p) = self
            .settings
            .skin
            .personalizations
            .get(&self.settings.skin.id)
        {
            for family in [&p.display.font_family, &p.history.font_family]
                .into_iter()
                .flatten()
            {
                for font in nanatype_theme::BuiltinFonts::shipped().fonts() {
                    if font
                        .family()
                        .is_some_and(|f| f.eq_ignore_ascii_case(family))
                    {
                        wanted.push(format!("builtin:{}", font.path));
                    }
                }
            }
        }
        let mut errors = self.appearance.builtin_fonts.sync(
            wanted.iter().map(String::as_str),
            nanatype_theme::builtin_font,
            crate::skin_fonts::register_with_nana_ui,
        );
        let mut families = [None, None, None, None];
        let mut packaged: HashMap<String, Option<String>> = HashMap::new();
        for (i, spec) in roles.into_iter().chain(f.latin.as_ref()).enumerate() {
            let Some(file) = &spec.file else {
                continue;
            };
            if nanatype_theme::is_builtin_font(file) {
                // Not in this build: None, and the role keeps its families.
                families[i] = self.appearance.builtin_fonts.family(file);
                continue;
            }
            if let Some(family) = packaged.get(file) {
                families[i] = family.clone();
                continue;
            }
            let family = match files.read(file) {
                Ok(bytes) => match self.appearance.fonts.add_bytes(bytes) {
                    Ok(registration) => registration.families().first().cloned(),
                    Err(e) => {
                        errors.push(format!("{file}: {e}"));
                        None
                    }
                },
                Err(e) => {
                    errors.push(e.to_string());
                    None
                }
            };
            packaged.insert(file.clone(), family.clone());
            families[i] = family;
        }
        self.appearance.skin_latin_family = families[3].take();
        self.appearance.skin_families =
            [families[0].take(), families[1].take(), families[2].take()];
        for e in errors {
            self.notice(e, true);
        }
    }

    pub(super) fn apply_skin(&mut self) {
        let (mut personalized, warnings) = crate::skin_theme::personalized_with_diagnostics(
            &self.appearance.loaded.skin,
            &self.settings,
            &self.settings.skin.id,
        );
        // Saved param values the skin now refuses: say so once per skin and
        // problem, and keep the stored values (a hot reload may fix them).
        for w in warnings {
            let key = format!("{}\n{w}", self.settings.skin.id);
            if self.appearance.warned_skin_params.insert(key) {
                self.notice(
                    format!(
                        "{}: {w}",
                        personalized.display_name(self.ui.locale.get_untracked().tag())
                    ),
                    false,
                );
            }
        }
        if let Some(pack) = &self.session.sound_pack {
            personalized.sounds = nanatype_theme::layer_sounds(&personalized.sounds, &pack.sounds);
        }
        if let (Some(role), Some(family)) = (
            &mut personalized.fonts.latin,
            &self.appearance.skin_latin_family,
        ) {
            role.families.insert(0, family.clone());
        }
        let skin = Arc::new(crate::skin_theme::prepare(
            &personalized,
            &Default::default(),
            &self.appearance.skin_families,
            self.ui.locale.get_untracked().tag(),
        ));
        // Recording the first per-skin window size may establish an otherwise
        // identical profile. Geometry alone must not replace the playing text.
        if skin.as_ref() == self.appearance.skin.as_ref() {
            return;
        }
        self.appearance.skin = skin.clone();

        self.ui.skin.set(skin.clone());
        let fx = self.session.core.handle(
            nanatype_core::Cmd::SetSkin(Box::new(nanatype_core::SkinCapabilities::from(&*skin))),
            crate::clock::now_ms(),
        );
        self.apply_effects(fx);
    }

    /// How much `kind` zooms its content now ([`crate::windows::zoom`]).
    pub fn zoom(&self, kind: crate::windows::WindowKind) -> f32 {
        crate::windows::zoom(
            kind,
            &self.settings,
            self.ui.system_text_scale.get_untracked(),
        )
    }

    /// Zoom `kind`'s content to the UI scale, and give it the minimum size
    /// that zoom needs (a smaller window grows to it at once). Broadcast
    /// windows are left alone.
    pub fn apply_zoom(&mut self, kind: crate::windows::WindowKind) {
        let Some(minimum) = crate::windows::minimum_size(kind, self.zoom(kind)) else {
            return;
        };
        let id = kind.id();
        self.shell
            .commands
            .push(nana_ui_platform::host::WindowCommand::SetContentZoom {
                id,
                zoom: self.zoom(kind),
            });
        self.shell
            .commands
            .push(nana_ui_platform::host::WindowCommand::SetSizeLimits {
                id,
                minimum: Some(minimum),
                maximum: None,
            });
    }

    /// [`Self::apply_zoom`] for every window that is open.
    pub fn apply_zoom_all(&mut self) {
        for kind in crate::windows::WindowKind::ALL {
            if self.shell.windows.is_open(kind) {
                self.apply_zoom(kind);
            }
        }
    }

    pub(super) fn apply_saved_sizes(&mut self) {
        for kind in [
            crate::windows::WindowKind::Display,
            crate::windows::WindowKind::History,
            crate::windows::WindowKind::Danmaku,
        ] {
            if let Some(frame) = self.shell.windows.frame(kind) {
                let size = self.window_size(kind);
                self.shell
                    .commands
                    .push(nana_ui_platform::host::WindowCommand::SetBounds {
                        id: kind.id(),
                        position: (frame.x as f32, frame.y as f32),
                        size,
                    });
            }
        }
    }

    /// Refresh operator chrome without reloading assets or changing content boxes.
    pub fn refresh_theme(&mut self) {
        let chrome = crate::ui::kit::theme::Chrome::of(
            &self.settings,
            self.appearance.system_dark,
            self.appearance.system_high_contrast,
        );
        let (theme, output) = crate::ui::kit::theme::compile_both(chrome);
        self.appearance.theme = theme;
        self.appearance.output_theme = output;
        self.ui.chrome_dark.set(chrome.dark);
        self.ui.chrome_contrast.set(chrome.high_contrast);
        self.tint_captions();
    }
}
