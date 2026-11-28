//! Application coordinator and domain message router.

pub mod appearance;
mod archive;
pub mod backup;
pub mod blive_feeds;
mod chat;
pub mod draft;
mod editor;
pub mod integrations;
mod live;
pub mod media;
pub mod obs;
mod packages;
mod secret;
pub mod session;
pub mod shell;
mod skins;
pub mod speech;
mod startup;
mod stickers;
mod system;
pub mod tts;
pub mod twitch;
#[cfg(feature = "updates")]
pub mod update;
mod window;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, RwLock};
use std::time::Duration;

use nana_ui::{RuntimeProgramContext, RuntimeProgramUpdate};
use nana_ui_core::{FileDialogKind, FileDialogRequest, FileFilter};
use nana_ui_platform::host::WindowCommand;
use nanatype_core::{Cmd, Core};
use nanatype_frame::locale_of;
use nanatype_i18n::Locale;
use nanatype_markup::serialize;
use nanatype_model::{AssetRef, Document, Inline};
use nanatype_settings::{GeometryStore, InputKeys, InputPresence, LoadNote, Settings};
use nanatype_theme::SkinRegistry;

use crate::chat::ChatMessage;
use crate::clock;
use crate::msg::{
    AppMsg, ChatMsg, EditorMsg, FileTarget, GlobalAction, Msg, NoticeSource, PackageMsg, SkinMsg,
    StickerMsg, WindowMsg,
};
use crate::output::OutputManager;
use crate::paths::Paths;
use crate::services::hotkeys;
use crate::services::persist::Persist;
use crate::services::sounds;
use crate::services::tray::Tray;
use crate::ui::feedback::{self, ToastRequest};
use crate::ui::{I18N, Notice, Ui};
use crate::windows::{QuickBarState, WindowKind, WindowManager};

pub use appearance::AppearanceState;
pub use draft::{DraftController, DraftFence, DraftPolicy};
#[cfg(test)]
pub(crate) use draft::{apply_source_markup, sync_rich_draft};
pub(crate) use draft::{markup_for, quote};
pub use integrations::IntegrationsHub;
pub use media::MediaHub;
pub use session::SessionController;
pub use shell::{SettingsSections, ShellState};

/// Chat messages the panel keeps.
const CHAT_CAP: usize = 500;
/// How long an error stays in the tray tooltip.
const TRAY_ERROR_SECS: u64 = 30;

/// Editor operations applied by the host without replacing its editing state.
// RichEditCommand is intentionally kept inline: the command queue owns these
// values and the dependency's command size is part of its runtime contract.
#[allow(clippy::large_enum_variant)]
pub enum EditorCommand {
    Edit(Box<nana_ui::runtime::RichEditCommand>),
    Insert(nana_ui_core::RichText),
    /// Delete the selection as one undo step.
    Delete,
}

impl From<nana_ui::runtime::RichEditCommand> for EditorCommand {
    fn from(command: nana_ui::runtime::RichEditCommand) -> Self {
        Self::Edit(Box::new(command))
    }
}

pub struct App {
    pub appearance: AppearanceState,
    pub session: SessionController,
    pub integrations: IntegrationsHub,
    pub media: MediaHub,
    pub shell: ShellState,
    pub draft: DraftController,
    /// Shared input-window hotkeys read by the host input router without a global lock.
    pub hotkey_input: Arc<RwLock<InputKeys>>,
    pub ui: Ui,
    pub ctx: RuntimeProgramContext<Msg>,
    pub settings: Settings,
}

/// The settings file's state as the session log's first app event.
fn record_settings_load(note: &nanatype_settings::LoadNote) {
    use nanatype_settings::LoadNote;
    let (code, from) = match note {
        LoadNote::Fresh => (0u64, 0u64),
        LoadNote::Loaded => (1, 0),
        LoadNote::Migrated { from } => (2, u64::from(*from)),
        LoadNote::Quarantined { reason, .. } => {
            nanatype_diag::report!(crate::diag::SETTINGS_QUARANTINED; "{reason}");
            (3, 0)
        }
        LoadNote::TooNew { schema } => (4, u64::from(*schema)),
    };
    nanatype_diag::event!(
        crate::diag::SETTINGS_LOADED,
        note = code,
        from_schema = from
    );
}

impl App {
    pub fn new(ctx: &RuntimeProgramContext<Msg>) -> Self {
        let paths = Paths::resolve();
        paths.ensure();
        // Before any skin shader draws: blocks programs that were on trial
        // when the last run ended.
        crate::ui::shader_effects::init_guard(paths.config.join("shader-guard.json"));
        // Before any window draws a picture: NanaUI reads local pictures
        // only under this base, and the rest are mirrored into it.
        crate::services::media::init(&paths.data);
        let legacy = Paths::legacy_settings();
        let loaded_settings =
            nanatype_settings::load_settings(&paths.settings(), legacy.as_deref());
        record_settings_load(&loaded_settings.note);
        crate::services::retired::start(&loaded_settings.note, paths.config.clone(), paths.blobs());
        let mut settings = loaded_settings.value;
        settings.sanitize();
        let geometry: GeometryStore = nanatype_settings::load_json(&paths.geometry()).value;

        let post = {
            let ctx = ctx.clone();
            move |m: Msg| ctx.dispatch(m)
        };
        // Layout-dependent materials (stroke-time maps) build off the UI thread.
        crate::ui::pipeline::set_waker({
            let post = post.clone();
            move || post(Msg::Skin(SkinMsg::LayoutMaterialsReady))
        });
        let persist = Persist::start({
            let post = post.clone();
            move |e| {
                post(Msg::Notice {
                    text: e,
                    error: true,
                    source: NoticeSource::Persist,
                })
            }
        });
        let registry = SkinRegistry::new(Some(paths.packages()), env!("CARGO_PKG_VERSION"));
        let (loaded, fallback) = registry.load_scanned_or_default(&settings.skin.id);
        let skin = Arc::new(crate::skin_theme::personalized(
            &loaded.skin,
            &settings,
            &settings.skin.id,
        ));
        let locale = locale_of(&settings);
        nanatype_diag::marker("start");
        crate::diag::session_info(&settings, locale.tag());
        let system_dark = matches!(
            ctx.system_appearance(),
            Some(nana_ui_platform::SystemAppearance::Dark)
        );
        let system_high_contrast = ctx.high_contrast();
        let (theme, output_theme) = crate::ui::kit::theme::compile_both(
            crate::ui::kit::theme::Chrome::of(&settings, system_dark, system_high_contrast),
        );
        let ui = startup::ui(ctx, &settings, &skin, locale, &geometry, system_dark);

        let workers = startup::Workers::start(&paths, &settings, post.clone());
        let mut core = Core::from_settings(&settings, &skin);
        core.handle(
            Cmd::SetReducedMotion(settings.playback.reduced_motion || ctx.reduced_motion()),
            clock::now_ms(),
        );
        let obs_password = secret::LazySecret::new(
            &paths.config,
            "obs-websocket",
            &crate::diag::OBS_SECRET_FAILED,
            false,
        );
        let appearance = AppearanceState {
            registry,
            loaded: loaded.clone(),
            skin,
            theme,
            output_theme,
            system_dark,
            system_high_contrast,
            fonts: nana_ui::HostFontScope::new(),
            builtin_fonts: Default::default(),
            skin_families: [None, None, None],
            skin_latin_family: None,
            skin_watch: None,
            skin_reload: Default::default(),
            skin_preview_ticket: 0,
            skin_reload_scan: false,
            prepared_skin_urls: None,
            warned_skin_params: HashSet::new(),
            export_target: None,
        };
        let session = SessionController {
            core,
            scheduler: workers.scheduler,
            speech: speech::Speech::new(paths.data.join("speech"), post.clone()),
            tts: tts::Narrator::new(tts::Lazy::new(post.clone(), workers.audio.clone())),
            audio: workers.audio,
            bank: Vec::new(),
            sound_pack: None,
            sound_pack_missing: None,
            keys: sounds::Keys::default(),
            archive: Default::default(),
            live_chat: Default::default(),
            chat: VecDeque::new(),
        };
        let blive = workers.blive;
        let integrations = IntegrationsHub {
            bridge: workers.bridge,
            twitch: workers.twitch,
            blive,
            obs: workers.obs,
            obs_password,
            #[cfg(feature = "updates")]
            updater: workers.updater,
            #[cfg(feature = "updates")]
            pending_update: Default::default(),
            chat_assets: workers.chat_assets,
            plugins: Default::default(),
            plugin_activation: Default::default(),
        };
        let media = MediaHub::new(
            paths.blobs(),
            workers.sticker_manager,
            workers.telegram,
            loaded,
        );
        let mut windows = WindowManager::default();
        windows.geometry = geometry;
        let shell = ShellState {
            paths,
            readonly_settings: matches!(loaded_settings.note, LoadNote::TooNew { .. }),
            persist,
            hotkeys: workers.hotkeys,
            tray: None,
            windows,
            quick: QuickBarState::default(),
            summoned: false,
            output: OutputManager::default(),
            backup: Default::default(),
            next_notice: 1,
            dialogs: HashMap::new(),
            next_dialog: 1,
            commands: Vec::new(),
            toasts: Vec::new(),
            tray_error: None,
            quitting: false,
            _critical: nanatype_scheduling::latency_critical(),
        };
        let draft = DraftController::new(ui.draft_fence.get_untracked());
        let hotkey_input = Arc::new(RwLock::new(settings.hotkeys.input.clone()));
        let mut app = Self {
            appearance,
            session,
            integrations,
            media,
            shell,
            draft,
            hotkey_input,
            ui,
            ctx: ctx.clone(),
            settings,
        };
        app.publish_speech();
        app.apply_performance(None);
        app.startup_notices(loaded_settings.note, fallback.is_some(), locale);
        match Tray::build(post) {
            Ok(tray) => app.shell.tray = Some(tray),
            Err(e) => {
                let text = app.ui.tr("err-tray", &[("message", e.as_str().into())]);
                app.notice(text, true);
            }
        }
        app.finish_startup(locale);
        startup::dev_hooks(ctx);
        let ctx = ctx.clone();
        crate::services::instance::serve(crate::services::instance::Product::APP, move || {
            ctx.dispatch(Msg::Window(WindowMsg::SecondInstance))
        });
        app.integrations.plugins.sync(
            &app.appearance.registry,
            &app.settings.enabled_plugins,
            &app.settings.plugin_approvals,
            app.ctx.clone(),
            app.ui,
        );
        app
    }

    pub fn post(&self, msg: Msg) {
        self.ctx.dispatch(msg);
    }

    pub fn notice(&mut self, text: String, error: bool) {
        self.notice_from(text, error, NoticeSource::App);
    }

    /// Tells the user how settings and the saved skin loaded.
    fn startup_notices(&mut self, note: LoadNote, skin_fell_back: bool, locale: Locale) {
        match note {
            LoadNote::Quarantined { moved_to, .. } => {
                let text = self.ui.tr(
                    "err-settings-quarantined",
                    &[("path", moved_to.display().to_string().into())],
                );
                self.notice(text, true);
            }
            LoadNote::TooNew { .. } => {
                let text = self.ui.now_t("err-settings-newer");
                self.notice(text, true);
            }
            LoadNote::Migrated { .. } => self.save_settings(),
            _ => {}
        }
        if skin_fell_back {
            let text = I18N.tr_in(
                locale,
                "skins-fallback",
                &[("id", self.settings.skin.id.as_str().into())],
            );
            self.notice(text, true);
        }
    }

    /// Fonts, skin, assets, services and tray state for a newly built app.
    fn finish_startup(&mut self, locale: Locale) {
        self.sync_sound_pack();
        self.register_fonts();
        self.apply_skin();
        self.refresh_skin_rows();
        self.media
            .sticker_manager
            .sync(&self.settings.assets.sticker_dirs);
        self.refresh_assets();
        self.load_sounds();
        self.apply_services();
        self.audio_levels();
        if let Some(tray) = &mut self.shell.tray {
            tray.relabel(&I18N, locale);
            tray.sync(&self.settings);
        }
        self.sync_tray_skins();
        self.sync_tray_characters();
        self.sync_tray();
        self.share_input_keys();
        self.request_displays();
        #[cfg(feature = "updates")]
        self.check_update_on_start();
        self.notice_unseen_crash();
        self.archive_start();
        self.backup_start();
        if !self.settings.general.onboarding_done {
            self.open_settings();
        }
    }

    fn share_input_keys(&self) {
        if let Ok(mut shared) = self.hotkey_input.write()
            && *shared != self.settings.hotkeys.input
        {
            *shared = self.settings.hotkeys.input.clone();
        }
    }

    fn notice_from(&mut self, text: String, error: bool, source: NoticeSource) {
        if error {
            nanatype_diag::report!(
                crate::diag::ERROR_NOTICE,
                source = source.code(),
                text_bytes = text.len() as u64;
                "{text}"
            );
        }
        let id = self.shell.next_notice;
        self.shell.next_notice += 1;
        let mut notices = (*self.ui.notices.get_untracked()).clone();
        notices.push(Notice {
            id,
            text: text.clone(),
            error,
        });
        if notices.len() > 4 {
            notices.remove(0);
        }
        self.ui.notices.set(Arc::new(notices));
        self.shell
            .toasts
            .push(ToastRequest::notice(text.clone(), error));
        // The tray says the latest error, so it is seen with every window
        // hidden (summon mode) too.
        self.shell.tray_error = error.then_some((id, text));
        self.session.scheduler.post_after(
            Duration::from_millis(feedback::NOTICE_MS),
            Msg::DismissNotice(id),
        );
        if error {
            self.session.scheduler.post_after(
                Duration::from_secs(TRAY_ERROR_SECS),
                Msg::TrayErrorExpired(id),
            );
        }
    }

    /// The input window is on screen (or about to be).
    pub fn input_visible(&self) -> bool {
        self.shell.windows.is_open(WindowKind::Input)
            && crate::windows::desired(
                WindowKind::Input,
                &self.appearance.skin,
                &self.settings,
                self.shell.summoned,
            )
            .visible
    }

    /// Tray labels and tooltip for the current state.
    fn sync_tray(&self) {
        let Some(tray) = &self.shell.tray else {
            return;
        };
        let s = &self.settings;
        let locale = self.ui.locale.get_untracked();
        let skin = self
            .ui
            .skins
            .with_untracked(|rows| {
                rows.iter()
                    .find(|r| r.id == s.skin.id)
                    .map(|r| r.name.clone())
            })
            .unwrap_or_else(|| s.skin.id.clone());
        let locked = s.overlays.display.lock == nanatype_settings::Lock::Locked;
        let mut tooltip = crate::services::tray::tooltip(
            &I18N,
            locale,
            s.general.input_mode,
            &skin,
            locked,
            self.shell.tray_error.as_ref().map(|(_, e)| e.as_str()),
        );
        if self.session.speech.controller.active() {
            tooltip.push_str(" · ");
            tooltip.push_str(&self.session.speech.state.status(self.ui));
        }
        let meta = if cfg!(target_os = "macos") {
            "⌘"
        } else {
            "Win"
        };
        let danmaku_separate = s.history.chat.separate;
        tray.show(
            &I18N,
            crate::services::tray::TrayState {
                locale,
                speech_active: self.session.speech.controller.active(),
                input: crate::windows::desired(
                    WindowKind::Input,
                    &self.appearance.skin,
                    s,
                    self.shell.summoned,
                )
                .visible,
                display: s.overlays.display.visible,
                history: s.overlays.history.visible,
                danmaku: danmaku_separate && s.overlays.danmaku.visible,
                danmaku_separate,
                chat: self.ui.input_dock.with_untracked(|d| d.chat_visible()),
                locked,
                continue_key: GlobalAction::Continue.get(s).map(|h| h.label(meta)),
                tooltip,
                badge: crate::services::tray::Badge {
                    mirroring: s.general.input_mode == nanatype_settings::InputMode::Mirror,
                    hidden: !s.overlays.display.visible
                        && !s.overlays.history.visible
                        && !(danmaku_separate && s.overlays.danmaku.visible),
                    locked,
                },
            },
        );
    }

    fn request_displays(&self) {
        let service = self.ctx.windows().clone();
        crate::plugins::background("nanatype-displays", &self.ctx, move || {
            let displays = match service.displays().wait() {
                Ok(displays) => displays,
                Err(e) => {
                    nanatype_diag::report!(crate::diag::DISPLAYS_UNAVAILABLE; "{e:?}");
                    return Msg::Window(WindowMsg::Displays(Vec::new()));
                }
            };
            let scale = displays
                .iter()
                .find(|d| d.primary)
                .or(displays.first())
                .map(|d| d.scale_factor)
                .unwrap_or(1.0);
            let monitors = displays
                .iter()
                .filter_map(|d| {
                    let b = d.logical_bounds(scale)?;
                    let (w, h) = d.physical_size.unwrap_or((0, 0));
                    Some(nanatype_settings::MonitorInfo {
                        key: nanatype_settings::MonitorKey(format!(
                            "{}|{w}x{h}",
                            d.name.clone().unwrap_or_else(|| format!("{:x}", d.id.0))
                        )),
                        work_area: [
                            b.position.0,
                            b.position.1,
                            b.size.0,
                            // NanaUI reports no work area; leave room for a
                            // bottom taskbar or Dock.
                            b.size.1 - 48.0 / scale,
                        ],
                        primary: d.primary,
                    })
                })
                .collect();
            Msg::Window(WindowMsg::Displays(monitors))
        });
    }

    // ---- services ---------------------------------------------------------

    /// What is registered: every binding but a profile's phrase or
    /// character whose chord a global hotkey (or an earlier one) has.
    fn hotkey_bindings(&self) -> hotkeys::Bindings {
        let clashes = GlobalAction::clashes(&self.settings);
        GlobalAction::all_in(&self.settings)
            .filter_map(|a| a.get(&self.settings).cloned().map(|h| (a, h)))
            .filter(|binding| !clashes.contains(binding))
            .collect()
    }

    fn apply_services(&mut self) {
        self.shell.hotkeys.apply(
            self.hotkey_bindings(),
            self.ui.recording.get_untracked().is_some(),
        );
        self.integrations
            .apply_bridge_settings(&self.settings, self.ui);
        self.twitch_configure();
        self.blive_configure_all();
        self.obs_configure();
        self.refresh_output_status();
        self.refresh_watches();
    }

    /// Keep file watchers on the folders that matter right now.
    fn refresh_watches(&mut self) {
        let post = {
            let ctx = self.ctx.clone();
            move |m: Msg| ctx.dispatch(m)
        };
        let skin_dir = self
            .settings
            .skin
            .hot_reload
            .then(|| self.appearance.loaded.files.watch_roots())
            .filter(|paths| !paths.is_empty());
        let current = self
            .appearance
            .skin_watch
            .as_ref()
            .map(|w| w.paths().to_vec());
        if skin_dir != current {
            self.appearance.skin_watch = skin_dir.map(|paths| {
                crate::services::watch::Watch::start(paths, post.clone(), || {
                    Msg::Skin(SkinMsg::FilesChanged)
                })
            });
        }
        let stickers: Vec<PathBuf> = self
            .settings
            .assets
            .sticker_dirs
            .iter()
            .map(PathBuf::from)
            .filter(|p| p.is_dir())
            .collect();
        let current = self
            .media
            .sticker_watch
            .as_ref()
            .map(|w| w.paths().to_vec())
            .unwrap_or_default();
        if stickers != current {
            self.media.sticker_watch = (!stickers.is_empty()).then(|| {
                crate::services::watch::Watch::start_excluding(
                    stickers,
                    vec![self.shell.paths.data.join("media")],
                    post,
                    || Msg::Sticker(StickerMsg::AssetsChanged),
                )
            });
        }
    }

    pub fn refresh_output_status(&mut self) {
        let locale = self.ui.locale.get_untracked();
        let sizes = self.ui.runtime_output_sizes.get_untracked();
        self.shell
            .output
            .sync(&self.settings, &sizes, locale, Some(self.ui.output_status));
        if let Some(at) = self.shell.output.take_cleanup_schedule() {
            self.session.scheduler.output_cleanup(at);
        }
    }

    pub fn refresh_spout_status(&mut self) {
        self.refresh_output_status();
    }

    fn save_settings(&self) {
        if !self.shell.readonly_settings {
            self.shell
                .persist
                .json(self.shell.paths.settings(), &self.settings);
        }
    }

    pub fn save_geometry(&mut self) {
        let dock = self.ui.input_dock.get_untracked();
        if self.shell.windows.geometry.input_dock != Some(dock) {
            self.shell.windows.geometry.input_dock = Some(dock);
            self.shell.windows.geometry_dirty = true;
        }
        if self.shell.windows.geometry_dirty {
            self.shell.windows.geometry_dirty = false;
            self.shell
                .persist
                .json(self.shell.paths.geometry(), &self.shell.windows.geometry);
        }
    }

    fn apply_settings(&mut self, next: Settings) {
        if next == self.settings {
            return;
        }
        let prev = std::mem::replace(&mut self.settings, next);
        let sections = SettingsSections::diff(&prev, &self.settings);

        if sections.contains(SettingsSections::INPUT_PREVIEW) {
            self.draft.focus_editor = true;
        }
        if sections.contains(SettingsSections::HOTKEYS) {
            self.share_input_keys();
            self.shell.hotkeys.apply(
                self.hotkey_bindings(),
                self.ui.recording.get_untracked().is_some(),
            );
        }
        if sections.contains(SettingsSections::EFFECTS_RETRY) {
            crate::ui::shader_effects::retry_disabled();
        }
        if sections.contains(SettingsSections::REDUCED_MOTION) {
            self.sync_reduced_motion();
        }
        if sections.contains(SettingsSections::CHROME) {
            self.refresh_theme();
        }
        if sections.contains(SettingsSections::ZOOM) {
            self.apply_zoom_all();
        }
        if sections.contains(SettingsSections::LOCALE) {
            let locale = locale_of(&self.settings);
            self.ui.locale.set(locale);
            if let Some(tray) = &self.shell.tray {
                tray.relabel(&I18N, locale);
            }
            self.refresh_skin_rows();
            // The font stack follows the language.
            self.apply_skin();
            self.sync_tray_skins();
            self.sync_tray_characters();
            for kind in [
                WindowKind::Display,
                WindowKind::History,
                WindowKind::Danmaku,
                WindowKind::Settings,
            ] {
                self.shell.commands.push(WindowCommand::SetTitle {
                    id: kind.id(),
                    title: kind.title(locale),
                });
            }
        }
        if sections.contains(SettingsSections::SKIN) {
            self.reload_skin(false);
            self.sync_tray_skins();
        } else if sections.contains(SettingsSections::SKIN_LOOK) {
            if sections.contains(SettingsSections::SKIN_PROFILE) {
                self.register_fonts();
            }
            self.apply_skin();
        }
        if sections.contains(SettingsSections::ASSETS) {
            self.media
                .sticker_manager
                .sync(&self.settings.assets.sticker_dirs);
            self.refresh_assets();
            self.refresh_watches();
        } else if sections.contains(SettingsSections::SKIN_WATCH) {
            self.refresh_watches();
        }
        if sections.contains(SettingsSections::SOUND_DEVICE) {
            self.session
                .audio
                .set_device(self.settings.sound.device.clone());
        }
        if sections.contains(SettingsSections::SOUND_PACK) && self.sync_sound_pack() {
            self.apply_skin();
            self.load_sounds();
        }
        if sections.contains(SettingsSections::SOUND_LEVELS) {
            self.audio_levels();
        }
        if sections.contains(SettingsSections::PERFORMANCE) {
            self.apply_performance(Some(&prev.performance));
        }
        if sections.contains(SettingsSections::INTEGRATIONS) {
            self.integrations
                .apply_bridge_settings(&self.settings, self.ui);
            self.twitch_configure();
            self.blive_configure_all();
        }
        if sections.any(SettingsSections::INTEGRATIONS | SettingsSections::OBS) {
            self.obs_configure();
        }
        if sections.any(SettingsSections::OUTPUT | SettingsSections::OBS | SettingsSections::LOCALE)
        {
            self.refresh_output_status();
        }
        if sections.contains(SettingsSections::CORE_CONFIG) {
            let fx = self.session.core.handle(
                Cmd::SetConfig(Box::new(nanatype_core::CoreConfig::from(&self.settings))),
                clock::now_ms(),
            );
            self.apply_effects(fx);
        }

        self.ui.settings.set(Arc::new(self.settings.clone()));

        if sections.contains(SettingsSections::PLUGINS) {
            self.integrations.plugins.sync(
                &self.appearance.registry,
                &self.settings.enabled_plugins,
                &self.settings.plugin_approvals,
                self.ctx.clone(),
                self.ui,
            );
        }
        if sections.contains(SettingsSections::SPEAKER) {
            self.ui
                .speaker
                .set(self.settings.general.active_character().speaker());
        }
        if sections.contains(SettingsSections::CHARACTERS)
            && !sections.contains(SettingsSections::LOCALE)
        {
            self.sync_tray_characters();
        }
        if let Some(tray) = &self.shell.tray {
            tray.sync(&self.settings);
        }
        if sections.contains(SettingsSections::TTS_OFF) {
            self.session.tts.silence();
        }
        if sections.contains(SettingsSections::SPEECH)
            || (sections.contains(SettingsSections::LOCALE)
                && self.settings.speech.language.is_none())
        {
            self.speech_user_edit();
        }
        self.save_settings();
    }

    pub fn edit(&mut self, f: impl FnOnce(&mut Settings)) {
        let mut next = self.settings.clone();
        f(&mut next);
        next.sanitize();
        self.apply_settings(next);
    }

    /// Persist a window/split resize directly into settings without cloning
    /// `Settings` for diffing or running unrelated service/skin dispatch.
    pub fn remember_overlay_size(
        &mut self,
        kind: WindowKind,
        size: [f32; 2],
        profile: Option<nanatype_settings::SkinPersonalization>,
    ) {
        let before = kind.saved_size(&self.settings);
        let materialized = matches!(kind, WindowKind::Display | WindowKind::History)
            && profile.is_some()
            && !self
                .settings
                .skin
                .personalizations
                .contains_key(&self.settings.skin.id);
        kind.remember_size(&mut self.settings, size, profile);
        self.settings.sanitize();
        if !materialized && kind.saved_size(&self.settings) == before {
            return;
        }
        self.ui.settings.set(Arc::new(self.settings.clone()));
        self.save_settings();
    }

    pub fn set_overlay_size(&mut self, kind: WindowKind, size: Option<[f32; 2]>) {
        let Some(overlay) = kind.overlay_mut(&mut self.settings) else {
            return;
        };
        if overlay.size == size {
            return;
        }
        overlay.size = size;
        self.settings.sanitize();
        self.ui.settings.set(Arc::new(self.settings.clone()));
        self.save_settings();
    }

    // ---- files ----------------------------------------------------------------

    fn pick(&mut self, target: FileTarget) {
        if target == FileTarget::InsertImage && !self.draft.allow_new_image() {
            self.notice(self.ui.now_t("chat-images-unavailable"), true);
            return;
        }
        let id = self.shell.next_dialog;
        self.shell.next_dialog += 1;
        self.shell.dialogs.insert(id, target);
        let t = |k: &str| self.ui.now_t(k);
        let images = FileFilter::new(
            t("tb-image").trim_end_matches('…').to_owned(),
            ["png", "jpg", "jpeg", "gif", "webp", "bmp"],
        );
        let skin_filter = FileFilter::new("NanaType package", ["ntpkg", "zip"]);
        let request = match target {
            FileTarget::SpeechModel => FileDialogRequest::new(id, FileDialogKind::OpenFile)
                .title(t("speech-model-import"))
                .filters([FileFilter::new("Whisper base Q5_1", ["bin"])]),
            FileTarget::Avatar(_) => FileDialogRequest::new(id, FileDialogKind::OpenFile)
                .title(t("general-avatar-pick"))
                .filters([images]),
            FileTarget::InsertImage => FileDialogRequest::new(id, FileDialogKind::OpenFile)
                .title(t("tb-image"))
                .filters([images]),
            FileTarget::ImportSkin => FileDialogRequest::new(id, FileDialogKind::OpenFile)
                .title(t("skins-import"))
                .filters([skin_filter]),
            FileTarget::ExportSkin => FileDialogRequest::new(id, FileDialogKind::SaveFile)
                .title(t("skins-export"))
                .filters([skin_filter])
                .file_name(format!(
                    "{}.ntpkg",
                    self.appearance.export_target.clone().unwrap_or_default()
                )),
            FileTarget::StickerFolder => {
                FileDialogRequest::new(id, FileDialogKind::PickFolder).title(t("assets-add-folder"))
            }
            FileTarget::ExportHistory(format) => {
                let (title, label) = match format {
                    crate::archive::Format::Txt => ("history-saved-export-txt", "Text"),
                    crate::archive::Format::Srt => ("history-saved-export-srt", "SubRip"),
                };
                FileDialogRequest::new(id, FileDialogKind::SaveFile)
                    .title(t(title))
                    .filters([FileFilter::new(label, [format.ext()])])
                    .file_name(self.archive_export_name(format))
            }
            FileTarget::ExportSettings => FileDialogRequest::new(id, FileDialogKind::SaveFile)
                .title(t("backup-export"))
                .filters([FileFilter::new("JSON", ["json"])])
                .file_name(self.settings_export_name()),
            FileTarget::ImportSettings => FileDialogRequest::new(id, FileDialogKind::OpenFile)
                .title(t("backup-import"))
                .filters([FileFilter::new("JSON", ["json"])]),
        };
        let window = [WindowKind::Settings, WindowKind::Input]
            .into_iter()
            .find(|k| self.shell.windows.is_open(*k))
            .unwrap_or(WindowKind::Input);
        self.shell.commands.push(WindowCommand::OpenFileDialog {
            id: window.id(),
            request,
        });
    }

    pub fn file_chosen(&mut self, request: u64, paths: Vec<PathBuf>) {
        let Some(target) = self.shell.dialogs.remove(&request) else {
            return;
        };
        let Some(path) = paths.into_iter().next() else {
            return;
        };
        match target {
            FileTarget::SpeechModel => self.install_speech_model(Some(path)),
            FileTarget::Avatar(i) => {
                let p = path.display().to_string();
                self.edit(|s| {
                    if let Some(c) = s.general.characters.get_mut(i) {
                        c.avatar = Some(p);
                    }
                });
            }
            FileTarget::InsertImage => {
                self.insert_asset(AssetRef::File(path.display().to_string()))
            }
            FileTarget::ImportSkin => {
                // Inspection validates every resource and signature: keep it off the UI thread.
                let mut registry = self.appearance.registry.clone();
                crate::plugins::background("package-install", &self.ctx, move || {
                    Msg::Package(PackageMsg::Installed(
                        registry
                            .install(&path)
                            .map(|outcome| (outcome, Box::new(registry))),
                    ))
                });
            }
            FileTarget::ExportSkin => {
                if let Some(id) = self.appearance.export_target.take() {
                    let registry = self.appearance.registry.clone();
                    crate::plugins::background("package-export", &self.ctx, move || {
                        Msg::Package(PackageMsg::Exported(
                            registry.export(&id, &path).map_err(|e| e.to_string()),
                        ))
                    });
                }
            }
            FileTarget::StickerFolder => {
                self.import_sticker_folders(vec![path]);
            }
            FileTarget::ExportHistory(format) => self.archive_export(format, path),
            FileTarget::ExportSettings => self.settings_export(path),
            FileTarget::ImportSettings => self.settings_import(path),
        }
    }

    /// Finish an import whose inspection and staging ran in the background.
    pub(crate) fn package_installed(
        &mut self,
        result: Result<
            (
                nanatype_theme::InstallOutcome,
                Box<nanatype_theme::SkinRegistry>,
            ),
            Vec<nanatype_theme::SkinDiagnostic>,
        >,
    ) {
        match result {
            Ok((outcome, registry)) => {
                self.integrations.plugin_activation.cancel(self.ui);
                self.appearance.registry = *registry;
                self.appearance.skin_reload.invalidate();
                self.refresh_skin_rows();
                self.sync_tray_skins();
                let (id, text) = match outcome {
                    nanatype_theme::InstallOutcome::Installed { id } => {
                        let t = self
                            .ui
                            .tr("skins-installed", &[("name", id.as_str().into())]);
                        (id, t)
                    }
                    nanatype_theme::InstallOutcome::Replaced { id, previous } => {
                        let t = self.ui.tr(
                            "skins-replaced",
                            &[("name", id.as_str().into()), ("previous", previous.into())],
                        );
                        (id, t)
                    }
                };
                self.notice(text, false);
                // An updated dependency may change another plugin's reviewed closure.
                self.integrations.plugins = Default::default();
                self.integrations.plugins.sync(
                    &self.appearance.registry,
                    &self.settings.enabled_plugins,
                    &self.settings.plugin_approvals,
                    self.ctx.clone(),
                    self.ui,
                );
                if id == self.settings.skin.id {
                    self.reload_skin(true);
                } else if self.appearance.registry.entry(&id).is_some() {
                    self.edit(|s| s.skin.id = id);
                }
            }
            Err(errors) => {
                let detail = errors.first().map(|d| d.to_string()).unwrap_or_default();
                let text = format!("{}: {detail}", self.ui.now_t("skins-import-failed"));
                self.notice(text, true);
            }
        }
    }

    fn import_sticker_folders(&mut self, paths: Vec<PathBuf>) {
        self.edit(|s| crate::stickers::add_folders(&mut s.assets.sticker_dirs, &paths));
    }

    // ---- update ---------------------------------------------------------------

    pub fn update(&mut self, msg: AppMsg) {
        if matches!(
            msg,
            AppMsg::Window(WindowMsg::SettingsPage(crate::ui::settings::Page::Speech))
        ) {
            self.speech_inventory();
            self.session.tts.engine.request_voices();
        }
        match msg {
            AppMsg::Speech(m) => self.speech_msg(m),
            AppMsg::Tts(m) => self.tts_msg(m),
            AppMsg::Package(m) => self.package_msg(m),
            AppMsg::Sticker(m) => self.sticker_msg(m),
            AppMsg::Editor(m) => self.editor_msg(m),
            AppMsg::Window(m) => self.window_msg(m),
            AppMsg::Skin(m) => self.skin_msg(m),
            AppMsg::Chat(m) => self.chat_msg(m),
            AppMsg::Twitch(m) => self.twitch_msg(m),
            AppMsg::BliveFeed(m) => self.blive_feed_msg(m),
            AppMsg::Obs(m) => self.obs_msg(m),
            AppMsg::History(m) => self.history_msg(m),
            AppMsg::Backup(m) => self.backup_msg(m),
            AppMsg::Profile(m) => self.profile_msg(m),
            #[cfg(feature = "updates")]
            AppMsg::Update(m) => self.update_msg(m),
            AppMsg::Core(cmd) => {
                if matches!(cmd, Cmd::DraftChanged(_)) {
                    self.draft.bump_version();
                    self.speech_user_edit();
                }
                self.run(cmd);
            }
            AppMsg::Edit(f) => self.edit(f),
            AppMsg::Hotkey(action) => self.hotkey(action),
            AppMsg::RecordHotkey(action) => self.record_hotkey(action),
            AppMsg::HotkeyCaptured(hotkey) => self.hotkey_captured(hotkey),
            AppMsg::HotkeyStatus(status) => self.ui.hotkeys.set(Arc::new(status)),
            AppMsg::InputHwnd(hwnd) => self.shell.hotkeys.set_input_window(hwnd),
            AppMsg::QuickHwnd(hwnd) => {
                self.shell.quick.set_hwnd(hwnd);
                self.sync_quick();
            }
            AppMsg::QuickSend(text) => self.quick_send(&text),
            AppMsg::QuickHide => self.hide_quick(),
            AppMsg::PickFile(target) => self.pick(target),
            AppMsg::OpenFolder(path) => crate::services::reveal(&path),
            AppMsg::ExportDiagnostics => self.export_diagnostics(),
            AppMsg::DiagnosticsExported(result) => self.diagnostics_exported(result),
            AppMsg::OpenUrl(url) => crate::services::open_url(url),
            AppMsg::CopyText(text) => self.copy_text(text),
            AppMsg::PlaySample => self.play_sample(),
            AppMsg::AvatarAuthorize(sink) => self.avatar_authorize(sink),
            AppMsg::AvatarRefresh(sink) => self.integrations.bridge.refresh_hotkeys(sink),
            AppMsg::Notice {
                text,
                error,
                source,
            } => self.notice_from(text, error, source),
            AppMsg::DismissNotice(id) => {
                let mut notices = (*self.ui.notices.get_untracked()).clone();
                let before = notices.len();
                notices.retain(|n| n.id != id);
                if notices.len() != before {
                    self.ui.notices.set(Arc::new(notices));
                }
            }
            AppMsg::TrayErrorExpired(id) => self.tray_error_expired(id),
            AppMsg::OutputCleanup => self.refresh_output_status(),
            AppMsg::Wake { epoch, reason } => self.run(Cmd::Wake { epoch, reason }),
            AppMsg::Cue(kind) => self.run(Cmd::CueFired(kind)),
            AppMsg::Audio(status) => self.audio_status(status),
            AppMsg::Bridge(status) => self.bridge_status(status),
            AppMsg::Quit => self.quit(),
        }
    }

    // ---- live chat ----------------------------------------------------------

    fn chat_message(&mut self, message: Arc<ChatMessage>) {
        let epoch = match message.platform {
            crate::chat::Platform::Blivechat => self
                .integrations
                .blive
                .epoch(blive_feeds::BliveFeedKind::Blivechat),
            crate::chat::Platform::Laplace => self
                .integrations
                .blive
                .epoch(blive_feeds::BliveFeedKind::Laplace),
            crate::chat::Platform::Comet => self
                .integrations
                .blive
                .epoch(blive_feeds::BliveFeedKind::Comet),
            crate::chat::Platform::Twitch => self.integrations.twitch.epoch(),
        };
        if message.epoch != 0 && message.epoch != epoch {
            return;
        }
        let include = !self.session.live_chat.echo(&message);
        self.ensure_urls(media::message_assets(&message));
        let history = &self.settings.history.chat;
        if include && message.passes(&history.filter) {
            let locale = self.ui.locale.get_untracked();
            let item = message.history_item(locale);
            self.tts_chat(&item.doc);
            self.run(Cmd::External(item.clone()));
            self.live_record_row(&message);
        }
        self.session.chat.push_back(message);
        while self.session.chat.len() > CHAT_CAP {
            self.session.chat.pop_front();
        }
        self.publish_chat();
    }

    fn publish_chat(&mut self) {
        self.ui
            .chat
            .set(Arc::new(self.session.chat.iter().cloned().collect()));
        if self.draft.completer.open.is_some() {
            self.refresh_completion();
        }
    }

    fn mention(&mut self, seq: u64, reply: bool) {
        let Some(message) = self
            .session
            .chat
            .iter()
            .rev()
            .find(|m| m.seq == seq)
            .cloned()
        else {
            return;
        };
        if reply {
            let locale = self.ui.locale.get_untracked();
            self.run(Cmd::SetReply(Some(message.quote(locale))));
        } else {
            self.insert_inline(Inline::Mention(nanatype_model::Mention::new(
                message.user.clone(),
            )));
        }
        self.ui.chat_target.set(Some(seq));
        // Keep reply inside the resident input window, even when its chat
        // rail is folded or another panel is open.
        let show_chat = reply && !self.ui.input_dock.get_untracked().chat_visible();
        self.draft.focus_editor = true;
        let text = self.ui.tr(
            if reply {
                "chat-replying"
            } else {
                "chat-inserted"
            },
            &[("name", message.user.name.as_str().into())],
        );
        self.shell.toasts.push(ToastRequest::new(
            text,
            nana_ui_core::ToastTone::Success,
            1600,
            false,
        ));
        if show_chat {
            let layout = self.ui.input_dock.get_untracked();
            if !layout.chat_visible() {
                self.chat_msg(ChatMsg::Show);
                return;
            }
        }
        self.show_input_window();
    }

    /// Show the input window for a hotkey and bring it forward.
    fn raise_input(&mut self) {
        self.show_input_window();
        // Carbon does not grant foreground the way WM_HOTKEY does, and
        // AppKit has to be touched on this thread.
        #[cfg(target_os = "macos")]
        {
            hotkeys::activate(self.shell.hotkeys.input_window());
        }
    }

    /// Bring up the quick input bar, centred and focused.
    fn show_quick(&mut self) {
        self.shell.quick.show();
        self.shell
            .commands
            .push(WindowCommand::Focus(WindowKind::Quick.id()));
        #[cfg(target_os = "macos")]
        {
            hotkeys::activate(self.shell.hotkeys.quick_window());
        }
        #[cfg(windows)]
        {
            // WM_HOTKEY owns the foreground right; request activation there even
            // when the Quick HWND is published after this callback.
            self.shell.hotkeys.request_quick_activation();
        }
    }

    fn hide_quick(&mut self) {
        self.shell.quick.hide();
        #[cfg(windows)]
        self.shell.hotkeys.cancel_quick_activation();
        self.ui.quick_text.set_if_changed(String::new());
    }

    /// Send the bar's line as typed: escaped, so `[` or `/` stay text.
    fn quick_send(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        let doc = Document::plain(text);
        self.run(Cmd::SendMarkup(serialize(&doc)));
        self.hide_quick();
    }

    /// Publish the quick bar's HWND to the hotkey worker while summon presence or
    /// the bar itself wants it.
    fn sync_quick(&mut self) {
        let summon = self.settings.general.input_presence == InputPresence::Summon;
        let quick_open = self.quick_bar_open();
        if !(summon || quick_open) && quick_open {
            self.hide_quick();
        }
        let hwnd = if summon || self.quick_bar_open() {
            self.shell.quick.hwnd()
        } else {
            0
        };
        self.shell.hotkeys.set_quick_window(hwnd);
    }

    /// The quick bar's window closed: reset its visible state so it opens again.
    pub(crate) fn quick_closed(&mut self) {
        self.shell.quick.closed();
        #[cfg(windows)]
        self.shell.hotkeys.cancel_quick_activation();
        self.shell.hotkeys.set_quick_window(0);
    }

    /// The chat messages the panel lists, oldest first.
    pub(super) fn chat_shown(&self) -> Vec<u64> {
        self.ui.chat.with_untracked(|rows| {
            crate::ui::danmaku::shown(rows, &self.settings.integrations.chat.panel)
        })
    }

    fn set_input_visibility(&mut self, visible: bool) {
        let mut summoned = self.shell.summoned;
        self.edit(|s| crate::windows::set_input_visibility(s, &mut summoned, visible));
        self.shell.summoned = summoned;
    }

    fn hotkey(&mut self, action: GlobalAction) {
        match action {
            GlobalAction::Summon => {
                // Only the input window can come back minimized.
                let _ = self.shell.hotkeys.take_summon_restore();
                self.raise_input();
            }
            GlobalAction::QuickInput => {
                if self.quick_bar_open() {
                    self.hide_quick();
                } else {
                    self.show_quick();
                }
            }
            GlobalAction::ShowChat => {
                self.chat_msg(ChatMsg::Show);
                self.raise_input();
            }
            GlobalAction::ReplyLatest => {
                if let Some(&seq) = self.chat_shown().last() {
                    self.mention(seq, true);
                }
                self.raise_input();
            }
            GlobalAction::QuickPhrases => {
                let open = nanatype_settings::InputDockLayout {
                    tab: nanatype_settings::InputDockTab::Phrases,
                    collapsed: false,
                    ..self.ui.input_dock.get_untracked()
                };
                self.chat_msg(ChatMsg::Dock(open));
                self.raise_input();
            }
            GlobalAction::Continue => self.run(Cmd::Continue),
            GlobalAction::Skip => self.run(Cmd::Skip),
            GlobalAction::Clear => self.run(Cmd::ClearDisplay),
            GlobalAction::ToggleMode => self.edit(|s| {
                s.general.input_mode = match s.general.input_mode {
                    nanatype_settings::InputMode::Compose => nanatype_settings::InputMode::Mirror,
                    nanatype_settings::InputMode::Mirror => nanatype_settings::InputMode::Compose,
                }
            }),
            GlobalAction::ToggleOverlays => {
                self.edit(|s| {
                    let show = !(s.overlays.display.visible
                        || s.overlays.history.visible
                        || (s.history.chat.separate && s.overlays.danmaku.visible));
                    s.overlays.display.visible = show;
                    s.overlays.history.visible = show;
                    if s.history.chat.separate {
                        s.overlays.danmaku.visible = show;
                    }
                });
            }
            GlobalAction::Phrase(i) => self.editor_msg(EditorMsg::InsertPhrase(i)),
            GlobalAction::ToggleLock
            | GlobalAction::Replay
            | GlobalAction::ToggleHistory
            | GlobalAction::NextCharacter
            | GlobalAction::PrevCharacter
            | GlobalAction::NextProfile
            | GlobalAction::Character(_) => {
                if let Some(msg) = action.message()
                    && let Ok(app_msg) = msg.into_domain()
                {
                    self.update(app_msg);
                }
            }
            GlobalAction::Profile(i) => {
                if let Some(p) = self.settings.hotkeys.profiles.get(i) {
                    let id = p.id.clone();
                    self.profile_msg(crate::msg::ProfileMsg::Switch(id));
                }
            }
        }
    }

    pub fn open_settings(&mut self) {
        self.shell.windows.request_settings();
        if self.shell.windows.is_open(WindowKind::Settings) {
            self.shell
                .commands
                .push(WindowCommand::Focus(WindowKind::Settings.id()));
            if let Some(h) = self.shell.windows.handle(WindowKind::Settings) {
                drop(h.set_visible(true));
                drop(h.focus());
            }
        }
    }

    pub fn window_size(&self, kind: WindowKind) -> (f32, f32) {
        crate::windows::own_size(kind, self.zoom(kind))
            .unwrap_or_else(|| crate::windows::size_of(kind, &self.appearance.skin, &self.settings))
    }

    /// Native window state for this frame.
    pub fn reconcile_windows(&mut self) -> Vec<WindowCommand> {
        self.sync_quick();
        let mut out = std::mem::take(&mut self.shell.commands);
        let locale = self.ui.locale.get_untracked();
        let system_text = self.ui.system_text_scale.get_untracked();
        let quick_visible = self.quick_bar_open();
        out.extend(self.shell.windows.reconcile(
            &self.appearance.skin,
            &self.settings,
            locale,
            system_text,
            self.shell.summoned,
            quick_visible,
        ));
        self.save_geometry();
        out
    }

    pub fn update_result(&mut self) -> RuntimeProgramUpdate {
        // Publish visibility independently of the native window's frame.
        let shown = self.input_visible();
        if self.ui.input_shown.get_untracked() != shown {
            self.ui.input_shown.set(shown);
        }
        let window_commands = self.reconcile_windows();
        self.sync_tray();
        RuntimeProgramUpdate {
            window_commands,
            exit: self.shell.quitting,
            ..RuntimeProgramUpdate::default()
        }
    }
}

/// Debug demo: type a line through the editor's own command path (what the
/// toolbar and keyboard drive), then send it.
fn demo_editor(ctx: &RuntimeProgramContext<Msg>) {
    use crate::msg::Tool;
    use nana_ui::runtime::RichEditCommand as C;
    use nanatype_model::{Continuous, ContinuousKind, Inline, Marker};
    let text = |t: &str| Msg::Editor(EditorMsg::Command(C::InsertText(t.into())));
    let steps = [
        text("你好，这是"),
        Msg::Editor(EditorMsg::Tool(Tool::Bold)),
        text("粗体"),
        Msg::Editor(EditorMsg::Tool(Tool::Bold)),
        text("和"),
        Msg::Editor(EditorMsg::Tool(Tool::Effect(Continuous::new(
            ContinuousKind::Wave,
        )))),
        text("波浪"),
        Msg::Editor(EditorMsg::Tool(Tool::Effect(Continuous::new(
            ContinuousKind::Wave,
        )))),
        Msg::Editor(EditorMsg::Tool(Tool::Insert(Inline::Marker(
            Marker::Wait { ms: 400 },
        )))),
        Msg::Editor(EditorMsg::Tool(Tool::Color(
            "#e8505b".parse().expect("colour"),
        ))),
        text("红色！"),
    ];
    std::thread::sleep(Duration::from_millis(1500));
    for step in steps {
        ctx.dispatch(step);
        std::thread::sleep(Duration::from_millis(120));
    }
    std::thread::sleep(Duration::from_millis(2500));
    ctx.dispatch(Msg::Core(Cmd::Send));
}

/// `doc` with picked files reduced to their names.
pub(crate) fn without_local_paths(doc: &Document) -> Document {
    let mut doc = doc.clone();
    for inline in doc.paragraphs.iter_mut().flat_map(|p| p.inlines.iter_mut()) {
        if let Inline::Atom(atom) = inline
            && let AssetRef::File(path) = &atom.asset
        {
            let name = std::path::Path::new(path)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            atom.asset = AssetRef::File(name);
        }
    }
    doc
}

/// The runtime can end without `Msg::Quit` (logout, a platform close): the
/// debounced writer still gets to put queued settings down. (Signals may be
/// gone by now, so geometry is not re-read here.)
impl Drop for App {
    fn drop(&mut self) {
        crate::ui::shader_effects::close_guard();
        if !self.shell.quitting {
            self.shell.persist.flush(Duration::from_secs(2));
            nanatype_diag::marker("exit without quit");
            nanatype_diag::flush_for_exit();
        }
    }
}
