//! Application state and the single update function.
pub mod bilibili;
mod chat;
pub mod douyin;
mod editor;
mod package_editor;
mod packages;
mod skins;
mod startup;
mod stickers;
mod system;
mod window;

use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::Duration;

use nana_ui::{CompiledTheme, RuntimeProgramContext, RuntimeProgramUpdate};
use nana_ui_core::{FileDialogKind, FileDialogRequest, FileFilter};
use nana_ui_platform::host::WindowCommand;
use nanatype_audio::{AudioHandle, Bus, Scheduled};
use nanatype_bilibili::{BilibiliConfig, BilibiliHandle};
use nanatype_bridge::{
    BridgeEvent, BridgeHandle, BridgeStatus, LipSync, OscConfig, Sink, VtsConfig, WsConfig,
};
use nanatype_core::{Cmd, Core, Effects, Event, UiSound};
use nanatype_i18n::Locale;
use nanatype_markup::{parse, serialize, serialize_line};
use nanatype_model::{AssetRef, Document, Inline};
use nanatype_playback::CueKind;
use nanatype_settings::{GeometryStore, InputPresence, LoadNote, SendKey, Settings};
use nanatype_theme::{LoadedSkin, SkinRegistry};

use crate::assets::AssetLibrary;
use crate::chat::ChatMessage;
use crate::clock;
use crate::msg::{FileTarget, GlobalAction, MarkupEdit, Msg};
use crate::paths::Paths;
use crate::scheduler::Scheduler;
use crate::services::chat_assets::ChatAssets;
use crate::services::hotkeys::{self, Hotkeys};
use crate::services::persist::Persist;
use crate::services::sounds::{self, Bank};
use crate::services::tray::Tray;
use crate::skin_theme;
use crate::ui::feedback::{self, ToastRequest};
use crate::ui::{I18N, LinkState, Notice, StickerRow, Ui};
use crate::windows::{self, WindowKind, WindowManager};

/// Chat messages the panel keeps.
const CHAT_CAP: usize = 500;

/// Send-key mode shared with the editor's key handler.
pub static SEND_KEY: AtomicU8 = AtomicU8::new(0);

/// Editor operations applied by the host without replacing its editing state.
pub enum EditorCommand {
    Edit(nana_ui::runtime::RichEditCommand),
    Insert(nana_ui_core::RichText),
}

impl From<nana_ui::runtime::RichEditCommand> for EditorCommand {
    fn from(command: nana_ui::runtime::RichEditCommand) -> Self {
        Self::Edit(command)
    }
}

pub struct App {
    pub plugins: crate::plugins::Manager,
    plugin_activation: crate::plugins::Activation,
    pub package_editor: crate::package_editor::Editor,
    pub paths: Paths,
    pub settings: Settings,
    pub readonly_settings: bool,
    pub registry: SkinRegistry,
    pub loaded: LoadedSkin,
    pub skin: Arc<nanatype_theme::Skin>,
    pub core: Core,
    pub peer: crate::p2p::Session,
    pub ui: Ui,
    pub assets: AssetLibrary,
    pub sticker_manager: crate::stickers::Manager,
    telegram: crate::stickers::telegram::Service,
    pub windows: WindowManager,
    pub scheduler: Scheduler,
    pub audio: AudioHandle,
    pub bank: Bank,
    pub bridge: BridgeHandle,
    /// The Bilibili connection and the thread saving its pictures.
    pub bilibili: BilibiliHandle,
    pub douyin: nanatype_douyin::DouyinHandle,
    _chat_assets: ChatAssets,
    /// Live chat, oldest first, capped at the panel's size.
    chat: VecDeque<Arc<ChatMessage>>,
    bili_echoes: bilibili::Echoes,
    pub hotkeys: Hotkeys,
    pub tray: Option<Tray>,
    pub persist: Persist,
    pub theme: Arc<CompiledTheme>,
    pub ctx: RuntimeProgramContext<Msg>,
    /// The input window was summoned (summon presence).
    pub summoned: bool,
    pub system_dark: bool,
    next_notice: u64,
    dialogs: HashMap<u64, FileTarget>,
    next_dialog: u64,
    /// Pending window commands for the next update result.
    pub commands: Vec<WindowCommand>,
    pub theme_changed: bool,
    pub quitting: bool,
    export_target: Option<String>,
    pub spout: crate::output::Spout,
    /// Editor commands for the host to apply to the input window's editor.
    pub rich_commands: Vec<EditorCommand>,
    pub focus_editor: bool,
    pub source_edit_undoable: bool,
    /// Fonts packaged with the active skin, registered while it is active.
    fonts: nana_ui::HostFontScope,
    /// Built-in (embedded) fonts the active skin names, each registered once.
    builtin_fonts: crate::skin_fonts::BuiltinFontSet<nana_ui::HostFontScope>,
    /// Family registered for each skin font role (body, name, accent).
    skin_families: [Option<String>; 3],
    skin_latin_family: Option<String>,
    skin_watch: Option<crate::services::watch::Watch>,
    skin_reload: crate::services::watch::ReloadGate,
    skin_reload_scan: bool,
    prepared_skin_urls: Option<HashMap<String, String>>,
    sticker_watch: Option<crate::services::watch::Watch>,
    /// Toasts for the host to show in the settings, input and chat windows.
    pub toasts: Vec<ToastRequest>,
    /// The latest notice, while it is an error: the tray tooltip shows it.
    tray_error: Option<(u64, String)>,
}

/// How long an error stays in the tray tooltip.
const TRAY_ERROR_SECS: u64 = 30;

fn locale_of(s: &Settings) -> Locale {
    s.general
        .locale
        .as_deref()
        .and_then(Locale::from_tag)
        .unwrap_or_else(Locale::system)
}

impl App {
    pub fn new(ctx: &RuntimeProgramContext<Msg>) -> Self {
        let paths = Paths::resolve();
        paths.ensure();
        // Before any window draws a picture: NanaUI reads local pictures
        // only under this base, and the rest are mirrored into it.
        crate::services::media::init(&paths.data);
        let legacy = Paths::legacy_settings();
        let loaded_settings =
            nanatype_settings::load_settings(&paths.settings(), legacy.as_deref());
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
            move || post(Msg::LayoutMaterialsReady)
        });
        let persist = Persist::start({
            let post = post.clone();
            move |e| {
                post(Msg::Notice {
                    text: e,
                    error: true,
                })
            }
        });
        let registry = SkinRegistry::new(Some(paths.packages()), env!("CARGO_PKG_VERSION"));
        let (loaded, fallback) = registry.load_scanned_or_default(&settings.skin.id);
        let skin = Arc::new(skin_theme::personalized(
            &loaded.skin,
            &settings,
            &settings.skin.id,
        ));
        let locale = locale_of(&settings);
        let system_dark = matches!(
            ctx.system_appearance(),
            Some(nana_ui_platform::SystemAppearance::Dark)
        );
        let theme = skin_theme::compile(skin_theme::dark_mode(settings.skin.chrome, system_dark));
        let ui = startup::ui(ctx, &settings, &skin, locale, &geometry, system_dark);

        let workers = startup::Workers::start(&paths, &settings, post.clone());
        let mut core = Core::new(settings.clone(), skin.clone());
        core.handle(
            Cmd::SetReducedMotion(settings.playback.reduced_motion || ctx.reduced_motion()),
            clock::now_ms(),
        );
        let mut app = Self {
            plugins: Default::default(),
            plugin_activation: Default::default(),
            package_editor: crate::package_editor::Editor::new(
                paths.package_drafts(),
                Arc::new(crate::package_editor::SecretKeys::new(&paths.data)),
            ),
            assets: AssetLibrary::new(paths.blobs()),
            sticker_manager: workers.sticker_manager,
            telegram: workers.telegram,
            paths,
            readonly_settings: matches!(loaded_settings.note, LoadNote::TooNew { .. }),
            registry,
            loaded,
            skin,
            core,
            peer: crate::p2p::Session::default(),
            ui,
            windows: {
                let mut w = WindowManager::default();
                w.geometry = geometry;
                w
            },
            scheduler: workers.scheduler,
            audio: workers.audio,
            bank: Vec::new(),
            bridge: workers.bridge,
            bilibili: workers.bilibili,
            douyin: workers.douyin,
            _chat_assets: workers.chat_assets,
            chat: VecDeque::new(),
            bili_echoes: Default::default(),
            hotkeys: workers.hotkeys,
            tray: None,
            persist,
            theme,
            ctx: ctx.clone(),
            summoned: false,
            system_dark,
            next_notice: 1,
            dialogs: HashMap::new(),
            next_dialog: 1,
            commands: Vec::new(),
            theme_changed: false,
            quitting: false,
            export_target: None,
            spout: crate::output::Spout::default(),
            rich_commands: Vec::new(),
            focus_editor: false,
            source_edit_undoable: false,
            fonts: nana_ui::HostFontScope::new(),
            builtin_fonts: Default::default(),
            skin_families: [None, None, None],
            skin_latin_family: None,
            skin_watch: None,
            skin_reload: Default::default(),
            skin_reload_scan: false,
            prepared_skin_urls: None,
            sticker_watch: None,
            toasts: Vec::new(),
            tray_error: None,
            settings,
        };
        app.startup_notices(loaded_settings.note, fallback.is_some(), locale);
        match Tray::build(post) {
            Ok(tray) => app.tray = Some(tray),
            Err(e) => {
                let text = app.ui.tr("err-tray", &[("message", e.as_str().into())]);
                app.notice(text, true);
            }
        }
        app.finish_startup(locale);
        startup::dev_hooks(ctx);
        let ctx = ctx.clone();
        crate::listen_for_instances(move || ctx.dispatch(Msg::SecondInstance));
        app.plugins.sync(
            &app.registry,
            &app.settings.enabled_plugins,
            &app.settings.plugin_approvals,
            app.ctx.clone(),
            app.ui,
        );
        app
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
        self.register_fonts();
        self.apply_skin();
        self.refresh_skin_rows();
        self.sticker_manager
            .sync(&self.settings.assets.sticker_dirs);
        self.refresh_assets();
        self.load_sounds();
        self.apply_services();
        self.audio_levels();
        if let Some(tray) = &mut self.tray {
            tray.relabel(&I18N, locale);
            tray.sync(&self.settings);
        }
        self.sync_tray_skins();
        self.sync_tray();
        SEND_KEY.store(
            matches!(self.settings.general.send_key, SendKey::CtrlEnter) as u8,
            Ordering::Relaxed,
        );
        self.request_displays();
        if !self.settings.general.onboarding_done {
            self.open_settings();
        }
    }

    pub fn post(&self, msg: Msg) {
        self.ctx.dispatch(msg);
    }

    pub fn notice(&mut self, text: String, error: bool) {
        let id = self.next_notice;
        self.next_notice += 1;
        let mut status = (*self.ui.status.get_untracked()).clone();
        status.notices.push(Notice {
            id,
            text: text.clone(),
            error,
        });
        if status.notices.len() > 4 {
            status.notices.remove(0);
        }
        self.ui.status.set(Arc::new(status));
        let tone = if error {
            nana_ui::ToastTone::Danger
        } else {
            nana_ui::ToastTone::Info
        };
        self.toasts.push(ToastRequest::new(
            text.clone(),
            tone,
            feedback::NOTICE_MS,
            true,
        ));
        // The tray says the latest error, so it is seen with every window
        // hidden (summon mode) too.
        self.tray_error = error.then_some((id, text));
        self.scheduler.post_after(
            Duration::from_millis(feedback::NOTICE_MS),
            Msg::DismissNotice(id),
        );
        if error {
            self.scheduler.post_after(
                Duration::from_secs(TRAY_ERROR_SECS),
                Msg::TrayErrorExpired(id),
            );
        }
    }

    /// The input window is on screen (or about to be).
    pub fn input_visible(&self) -> bool {
        self.windows.is_open(WindowKind::Input)
            && windows::desired(WindowKind::Input, &self.skin, &self.settings, self.summoned)
                .visible
    }

    /// Tray labels and tooltip for the current state.
    fn sync_tray(&self) {
        let Some(tray) = &self.tray else {
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
        let tooltip = crate::services::tray::tooltip(
            &I18N,
            locale,
            s.general.input_mode,
            &skin,
            locked,
            self.tray_error.as_ref().map(|(_, e)| e.as_str()),
        );
        let meta = if cfg!(target_os = "macos") {
            "⌘"
        } else {
            "Win"
        };
        tray.show(
            &I18N,
            crate::services::tray::TrayState {
                locale,
                input: windows::desired(WindowKind::Input, &self.skin, s, self.summoned).visible,
                display: s.overlays.display.visible,
                history: s.overlays.history.visible,
                chat: s.integrations.bilibili.panel_open,
                locked,
                continue_key: GlobalAction::Continue.get(s).map(|h| h.label(meta)),
                tooltip,
                badge: crate::services::tray::Badge {
                    mirroring: s.general.input_mode == nanatype_settings::InputMode::Mirror,
                    hidden: !s.overlays.display.visible
                        && !s.overlays.history.visible
                        && !(self.peer.handle.is_some() && s.overlays.peer_display.visible),
                    locked,
                },
            },
        );
    }

    /// Tint ordinary windows' system title bars with the application chrome.
    pub fn tint_captions(&self) {
        for kind in [WindowKind::Input, WindowKind::Settings, WindowKind::Chat] {
            if let Some(handle) = self.windows.handle(kind) {
                windows::tint_caption(handle, &self.theme);
            }
        }
    }

    fn edit_status(&self, f: impl FnOnce(&mut crate::ui::Status)) {
        let previous = self.ui.status.get_untracked();
        let mut status = (*previous).clone();
        f(&mut status);
        if status != *previous {
            self.ui.status.set(Arc::new(status));
        }
    }

    fn request_displays(&self) {
        let service = self.ctx.windows().clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let displays = match service.displays().wait() {
                Ok(displays) => displays,
                Err(e) => {
                    crate::debug_log(&format!("displays unavailable: {e:?}"));
                    return;
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
            ctx.dispatch(Msg::Displays(monitors));
        });
    }

    // ---- skins and assets ------------------------------------------------

    fn refresh_skin_rows(&mut self) {
        self.skin_reload.invalidate();
        let previous = self.ui.skins.with_untracked(|rows| {
            rows.iter()
                .flat_map(|r| r.image_urls.values().cloned())
                .collect::<Vec<_>>()
        });
        let locale = self.ui.locale.get_untracked();
        let rows = crate::ui::skin_rows(&self.registry, locale);
        self.ui.skins.set(Arc::new(rows));
        crate::ui::retire_skin_urls(self.ui, previous);
    }

    fn sync_tray_skins(&mut self) {
        let skins: Vec<(String, String)> = self
            .ui
            .skins
            .get_untracked()
            .iter()
            .filter(|r| r.error.is_none())
            .map(|r| (r.id.clone(), r.name.clone()))
            .collect();
        if let Some(tray) = &mut self.tray {
            tray.set_skins(&skins, &self.settings.skin.id);
        }
    }

    fn publish_sticker_packs(&self) {
        self.ui
            .sticker_packs
            .set(Arc::new(self.sticker_manager.rows.clone()));
    }

    /// Asset URLs for skin files and stickers, for the views.
    fn refresh_assets(&mut self) {
        let previous = self.ui.asset_urls.with_untracked(|urls| {
            urls.iter()
                .filter(|(key, _)| key.starts_with("skin:"))
                .map(|(_, url)| url.clone())
                .collect::<Vec<_>>()
        });
        let locale = self.ui.locale.get_untracked();
        let mut seen = HashSet::new();
        let mut stickers = Vec::new();
        let mut rows = Vec::new();
        let mut images = Vec::new();
        let mut urls = self.prepared_skin_urls.take().unwrap_or_else(|| {
            crate::assets::skin_preview_urls(&self.loaded.skin, &*self.loaded.files)
        });
        for dir in &self.settings.assets.sticker_dirs {
            let root = crate::stickers::folder(std::path::Path::new(dir));
            if let Some(pack) = self.sticker_manager.loaded.get(&root) {
                for (entry, image) in pack.definition.entries.iter().zip(&pack.images) {
                    let sticker = crate::assets::Sticker {
                        lib: entry.lib.clone(),
                        name: entry.name.clone(),
                        path: image.frame_path(0),
                        root: image.dir.clone(),
                    };
                    let asset = sticker.asset().to_string();
                    if !seen.insert(asset.clone()) {
                        continue;
                    }
                    if let Some(url) = sticker.url() {
                        urls.insert(asset.clone(), url.clone());
                        rows.push(StickerRow {
                            asset,
                            name: entry
                                .translations
                                .get(locale.tag())
                                .unwrap_or(&entry.title)
                                .clone(),
                            lib: entry.lib.clone(),
                            url,
                            height_em: entry.height_em,
                            pack_title: entry.pack_title.clone(),
                        });
                    }
                    stickers.push(sticker);
                    images.push(image.clone());
                }
            }
        }
        crate::stickers::cache::publish(images.into_iter());
        stickers.sort_by(|a, b| (&a.lib, &a.name).cmp(&(&b.lib, &b.name)));
        rows.sort_by(|a, b| (&a.lib, &a.asset).cmp(&(&b.lib, &b.asset)));
        self.assets.stickers = stickers;
        self.publish_sticker_packs();
        // Pictures of files, pasted images and chat in the current views.
        let mut wanted = doc_assets(self.core.draft());
        wanted.extend(display_assets(self.core.display()));
        for row in self.core.history() {
            wanted.extend(row_assets(&row.item, &row.doc));
        }
        for m in &self.chat {
            wanted.extend(message_assets(m));
        }
        for asset in wanted {
            let key = asset.to_string();
            if !urls.contains_key(&key)
                && let Some(u) = self.assets.url(&asset, Some(&self.loaded))
            {
                urls.insert(key, u);
            }
        }
        let mut portraits = HashMap::new();
        let p = &self.skin.dialogue.portrait;
        if let Some(root) = self.loaded.files.root() {
            if let Some(url) = p
                .image
                .as_deref()
                .and_then(|img| crate::assets::skin_file_url(root, img))
            {
                portraits.insert("default".to_owned(), url);
            }
            for (name, img) in &p.variants {
                if let Some(url) = crate::assets::skin_file_url(root, img) {
                    portraits.insert(name.clone(), url);
                }
            }
        }
        self.ui.asset_urls.set(Arc::new(urls));
        self.ui.stickers.set(Arc::new(rows));
        self.ui.portraits.set(Arc::new(portraits));
        crate::ui::retire_skin_urls(self.ui, previous);
    }

    fn load_sounds(&self) {
        let skin = self.loaded.clone();
        let ctx = self.ctx.clone();
        std::thread::spawn(move || {
            let (bank, errors) = sounds::load(&skin);
            ctx.dispatch(Msg::SoundsLoaded {
                skin: skin.skin.meta.id.clone(),
                bank,
                errors,
            });
        });
    }

    fn reload_skin(&mut self, from_scan: bool) {
        if !from_scan {
            self.request_skin_reload(false);
            return;
        }
        // A watcher/install already validated this scan. A manual switch must
        // re-read inactive authoring folders, which are not being watched.
        let result = self.registry.load_scanned(&self.settings.skin.id);
        self.apply_loaded_skin(result);
    }

    fn apply_loaded_skin(
        &mut self,
        result: Result<LoadedSkin, Vec<nanatype_theme::SkinDiagnostic>>,
    ) {
        let (loaded, fallback) = match result {
            Ok(loaded) => (loaded, None),
            // Invalid edits keep the author's last working skin.
            Err(errors) if self.settings.skin.id == self.loaded.skin.meta.id => {
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
                self.registry
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
        let changed_files = loaded.skin.meta.id != self.loaded.skin.meta.id
            || loaded.skin.sounds != self.loaded.skin.sounds;
        self.loaded = loaded;
        self.register_fonts();
        self.apply_skin();
        self.apply_saved_sizes();
        if changed_files {
            self.load_sounds();
        }
        self.refresh_assets();
        self.refresh_watches();
    }

    fn scan_skins(&mut self, ticket: u64) {
        let mut registry = self.registry.clone();
        let scanned = self.skin_reload_scan;
        let id = self.settings.skin.id.clone();
        let ctx = self.ctx.clone();
        if let Err(error) = std::thread::Builder::new()
            .name("skin-scan".into())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
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
                }))
                .map_err(|_| "Skin scan worker failed".to_owned());
                ctx.dispatch(Msg::SkinScanReady { ticket, result });
            })
        {
            self.skin_reload.cancel();
            self.notice(error.to_string(), true);
        }
    }

    fn request_skin_reload(&mut self, scan: bool) {
        self.skin_reload_scan |= scan;
        if let Some(ticket) = self.skin_reload.request() {
            self.scan_skins(ticket);
        }
    }

    /// Register the skin's packaged fonts, replacing the previous skin's.
    /// Built-in fonts (`builtin:`) are kept registered across skins that
    /// share them; a file named by several roles is registered once.
    fn register_fonts(&mut self) {
        self.fonts.clear();
        let f = self.loaded.skin.fonts.clone();
        let files = self.loaded.files.clone();
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
                for font in nanatype_theme::BuiltinFonts::embedded().fonts() {
                    if font
                        .family()
                        .is_some_and(|f| f.eq_ignore_ascii_case(family))
                    {
                        wanted.push(format!("builtin:{}", font.path));
                    }
                }
            }
        }
        let mut errors = self.builtin_fonts.sync(
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
                families[i] = self.builtin_fonts.family(file);
                continue;
            }
            if let Some(family) = packaged.get(file) {
                families[i] = family.clone();
                continue;
            }
            let family = match files.read(file) {
                Ok(bytes) => match self.fonts.add_bytes(bytes) {
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
        self.skin_latin_family = families[3].take();
        self.skin_families = [families[0].take(), families[1].take(), families[2].take()];
        for e in errors {
            self.notice(e, true);
        }
    }

    fn apply_skin(&mut self) {
        let mut personalized =
            skin_theme::personalized(&self.loaded.skin, &self.settings, &self.settings.skin.id);
        if let (Some(role), Some(family)) = (&mut personalized.fonts.latin, &self.skin_latin_family)
        {
            role.families.insert(0, family.clone());
        }
        let skin = Arc::new(skin_theme::prepare(
            &personalized,
            &Default::default(),
            &self.skin_families,
            self.ui.locale.get_untracked().tag(),
        ));
        // Recording the first per-skin window size may establish an otherwise
        // identical profile. Geometry alone must not replace the playing text.
        if skin.as_ref() == self.skin.as_ref() {
            return;
        }
        self.skin = skin.clone();

        self.ui.skin.set(skin.clone());
        let fx = self.core.handle(Cmd::SetSkin(skin), clock::now_ms());
        self.apply_effects(fx);
    }

    fn apply_saved_sizes(&mut self) {
        for kind in [WindowKind::Display, WindowKind::History] {
            if let Some(frame) = self.windows.frame(kind) {
                let size = self.window_size(kind);
                self.commands.push(WindowCommand::SetBounds {
                    id: kind.id(),
                    position: (frame.x as f32, frame.y as f32),
                    size,
                });
            }
        }
    }

    /// Refresh operator chrome without reloading assets or changing content boxes.
    pub fn refresh_theme(&mut self) {
        let dark = skin_theme::dark_mode(self.settings.skin.chrome, self.system_dark);
        self.theme = skin_theme::compile(dark);
        self.ui.chrome_dark.set(dark);
        self.theme_changed = true;
        self.tint_captions();
    }

    // ---- services ---------------------------------------------------------

    fn hotkey_bindings(&self) -> hotkeys::Bindings {
        GlobalAction::all_in(&self.settings)
            .filter_map(|a| a.get(&self.settings).cloned().map(|h| (a, h)))
            .collect()
    }

    fn apply_services(&mut self) {
        self.hotkeys.apply(
            self.hotkey_bindings(),
            self.ui.recording.get_untracked().is_some(),
        );
        let i = self.settings.integrations.clone();
        self.bridge.set_vts(i.vts.enabled.then(|| VtsConfig {
            port: i.vts.port,
            token: i.vts.token.clone(),
            expressions: i.vts.expressions.clone(),
            lip_sync: (i.vts.lip_sync && !i.vts.mouth_param.trim().is_empty()).then(|| LipSync {
                param: i.vts.mouth_param.trim().to_owned(),
                amp: i.vts.mouth_amp,
            }),
            reset: i.vts.reset_expressions,
        }));
        self.bridge.set_osc(i.osc.enabled.then(|| OscConfig {
            host: i.osc.host.clone(),
            port: i.osc.port,
            prefix: i.osc.prefix.clone(),
        }));
        self.bridge.set_ws(i.websocket.enabled.then_some(WsConfig {
            port: i.websocket.port,
        }));
        let b = &i.bilibili;
        let config = BilibiliConfig {
            enabled: b.enabled
                && (b.room.is_empty() || b.room.parse::<u64>().is_ok_and(|id| id > 0)),
            room: b.room.trim().parse().ok().filter(|id| *id > 0),
        };
        let previous_epoch = self.bilibili.epoch();
        self.bilibili.set(config);
        if self.bilibili.epoch() != previous_epoch {
            self.bilibili_reconnecting();
        }
        self.sync_bilibili_speaker();
        self.douyin_configure();
        self.edit_status(|s| {
            if !i.vts.enabled {
                s.vts = LinkState::Off;
            }
            if !i.osc.enabled {
                s.osc = LinkState::Off;
            }
            if !i.websocket.enabled {
                s.ws = LinkState::Off;
            }
        });
        self.refresh_spout_status();
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
            .then(|| self.loaded.files.watch_roots())
            .filter(|paths| !paths.is_empty());
        let current = self.skin_watch.as_ref().map(|w| w.paths().to_vec());
        if skin_dir != current {
            self.skin_watch = skin_dir.map(|paths| {
                crate::services::watch::Watch::start(paths, post.clone(), || Msg::SkinFilesChanged)
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
            .sticker_watch
            .as_ref()
            .map(|w| w.paths().to_vec())
            .unwrap_or_default();
        if stickers != current {
            self.sticker_watch = (!stickers.is_empty()).then(|| {
                crate::services::watch::Watch::start_excluding(
                    stickers,
                    vec![self.paths.data.join("media")],
                    post,
                    || Msg::AssetsChanged,
                )
            });
        }
    }

    pub fn refresh_spout_status(&mut self) {
        let locale = self.ui.locale.get_untracked();
        self.spout
            .sync(&self.settings, locale, self.peer.handle.is_some());
        if let Some(at) = self.spout.take_cleanup_schedule() {
            self.scheduler.output_cleanup(at);
        }
        let display = self
            .spout
            .status
            .get(&WindowKind::Display)
            .cloned()
            .unwrap_or_default();
        let history = self
            .spout
            .status
            .get(&WindowKind::History)
            .cloned()
            .unwrap_or_default();
        let peer = self
            .spout
            .status
            .get(&WindowKind::PeerDisplay)
            .cloned()
            .unwrap_or_default();
        self.edit_status(|s| {
            s.spout_display = display;
            s.spout_peer_display = peer;
            s.spout_history = history;
        });
    }

    fn audio_levels(&self) {
        let s = &self.settings.sound;
        self.audio.set_master(s.master);
        self.audio.set_bus(Bus::Typing, s.typing_volume);
        self.audio.set_bus(Bus::Effects, s.effects_volume);
        self.audio.set_bus(Bus::Ui, s.ui_volume);
    }

    fn save_settings(&self) {
        if !self.readonly_settings {
            self.persist.json(self.paths.settings(), &self.settings);
        }
    }

    pub fn save_geometry(&mut self) {
        let selected = self.ui.input_dock_selected.get_untracked();
        let dock_changed = self.ui.input_dock_regions.with_untracked(|regions| {
            self.windows
                .geometry
                .input_dock
                .as_ref()
                .is_none_or(|saved| saved.selected != selected || saved.regions != *regions)
        });
        if dock_changed {
            self.windows.geometry.input_dock = Some(nanatype_settings::InputDockLayout {
                regions: self.ui.input_dock_regions.get_untracked(),
                selected,
            });
            self.windows.geometry_dirty = true;
        }
        if self.windows.geometry_dirty {
            self.windows.geometry_dirty = false;
            self.persist
                .json(self.paths.geometry(), &self.windows.geometry);
        }
    }

    fn apply_settings(&mut self, next: Settings) {
        if next == self.settings {
            return;
        }
        let prev = std::mem::replace(&mut self.settings, next);
        let s = &self.settings;
        let locale_changed = locale_of(&prev) != locale_of(s);
        let skin_changed = prev.skin.id != s.skin.id;
        let body_profile = |settings: &Settings| {
            settings
                .skin
                .personalizations
                .get(&s.skin.id)
                .cloned()
                .map(|mut p| {
                    p.display.size = None;
                    p.history.size = None;
                    p
                })
        };
        let active_profile_changed = body_profile(&prev) != body_profile(s);
        let look_changed = prev.skin.overrides != s.skin.overrides || active_profile_changed;
        let chrome_changed = prev.skin.chrome != s.skin.chrome;
        let assets_changed = prev.assets.sticker_dirs != s.assets.sticker_dirs;
        let device_changed = prev.sound.device != s.sound.device;
        let autostart_changed = prev.general.launch_at_login != s.general.launch_at_login;
        if prev.general.input_skin_preview != s.general.input_skin_preview {
            self.focus_editor = true;
        }
        SEND_KEY.store(
            matches!(s.general.send_key, SendKey::CtrlEnter) as u8,
            Ordering::Relaxed,
        );
        self.sync_reduced_motion();
        if chrome_changed {
            self.refresh_theme();
        }
        if locale_changed {
            let locale = locale_of(&self.settings);
            self.ui.locale.set(locale);
            if let Some(tray) = &self.tray {
                tray.relabel(&I18N, locale);
            }
            self.refresh_skin_rows();
            // The font stack follows the language.
            self.apply_skin();
            self.sync_tray_skins();
            for kind in [
                WindowKind::Display,
                WindowKind::PeerDisplay,
                WindowKind::History,
                WindowKind::Settings,
                WindowKind::Chat,
            ] {
                self.commands.push(WindowCommand::SetTitle {
                    id: kind.id(),
                    title: I18N.t_in(locale, kind.title_key()),
                });
            }
        }
        if skin_changed {
            self.reload_skin(false);
            self.sync_tray_skins();
        } else if look_changed {
            if active_profile_changed {
                self.register_fonts();
            }
            self.apply_skin();
        }
        if assets_changed {
            self.sticker_manager
                .sync(&self.settings.assets.sticker_dirs);
            self.refresh_assets();
        }
        if device_changed {
            self.audio.set_device(self.settings.sound.device.clone());
        }
        if autostart_changed
            && let Err(e) = crate::services::set_autostart(self.settings.general.launch_at_login)
        {
            self.notice(e, true);
        }
        self.audio_levels();
        self.apply_services();
        let fx = self.core.handle(
            Cmd::SetSettings(Box::new(self.settings.clone())),
            clock::now_ms(),
        );
        self.apply_effects(fx);
        self.ui.settings.set(Arc::new(self.settings.clone()));
        if prev.enabled_plugins != self.settings.enabled_plugins
            || prev.plugin_approvals != self.settings.plugin_approvals
        {
            self.plugins.sync(
                &self.registry,
                &self.settings.enabled_plugins,
                &self.settings.plugin_approvals,
                self.ctx.clone(),
                self.ui,
            );
        }
        if let Some(tray) = &self.tray {
            tray.sync(&self.settings);
        }
        self.save_settings();
    }

    pub fn edit(&mut self, f: impl FnOnce(&mut Settings)) {
        let mut next = self.settings.clone();
        f(&mut next);
        next.sanitize();
        self.apply_settings(next);
    }

    /// Remember an overlay's size. Drags and wheel zoom arrive at event rate,
    /// and a size is geometry, not configuration: no service, playback core,
    /// tray or skin depends on it, so only the settings signal and the
    /// (debounced) file are updated.
    pub fn edit_geometry(&mut self, f: impl FnOnce(&mut Settings)) {
        let mut next = self.settings.clone();
        f(&mut next);
        next.sanitize();
        if next == self.settings {
            return;
        }
        // Remembering a size can first materialize a skin personalization;
        // anything beyond sizes takes the full path.
        let without_sizes = |s: &Settings| {
            let mut s = s.clone();
            for p in s.skin.personalizations.values_mut() {
                p.display.size = None;
                p.history.size = None;
            }
            for kind in [
                WindowKind::Display,
                WindowKind::PeerDisplay,
                WindowKind::History,
            ] {
                if let Some(o) = kind.overlay_mut(&mut s) {
                    o.size = None;
                }
            }
            s
        };
        if without_sizes(&next) != without_sizes(&self.settings) {
            self.apply_settings(next);
            return;
        }
        self.settings = next;
        self.ui.settings.set(Arc::new(self.settings.clone()));
        self.save_settings();
    }

    // ---- the session ------------------------------------------------------

    pub fn run(&mut self, cmd: Cmd) {
        if self.peer_blocked(&cmd) {
            return;
        }
        let is_send = matches!(cmd, Cmd::Send);
        let fx = match bilibili::handle_command(
            &mut self.core,
            cmd,
            self.ui.bilibili.get_untracked().sending,
            clock::now_ms(),
        ) {
            Ok(fx) => fx,
            Err(()) => {
                self.notice(self.ui.now_t("bili-images-unavailable"), true);
                return;
            }
        };
        let sent = is_send && fx.replace_draft.as_ref().is_some_and(Document::is_blank);
        self.apply_effects(fx);
        if sent {
            self.ui.chat_target.set(None);
            let count = self.core.queued();
            let feedback = if count > 0 {
                self.ui.tr("chat-queued", &[("count", count.into())])
            } else {
                format!("{} ✓", self.ui.now_t("status-sent"))
            };
            self.toasts.push(ToastRequest::new(
                feedback,
                nana_ui::ToastTone::Success,
                feedback::SENT_MS,
                false,
            ));
        }
    }

    fn play_cue_now(&mut self, cue: &CueKind) {
        if let Some(v) = sounds::voice(cue, &self.loaded) {
            self.audio.play(v);
        } else {
            self.run(Cmd::CueFired(cue.clone()));
        }
    }

    fn apply_effects(&mut self, fx: Effects) {
        for epoch in &fx.cancel_epochs {
            self.audio.cancel(*epoch);
            self.scheduler.cancel(*epoch);
        }
        for wake in &fx.wakes {
            self.scheduler.wake(*wake);
        }
        for batch in &fx.cues {
            let mut audio = Vec::new();
            for cue in batch.cues.iter() {
                let at = batch.origin_ms + u64::from(cue.at_ms);
                match sounds::voice(&cue.kind, &self.loaded) {
                    Some(voice) => audio.push(Scheduled {
                        at: clock::instant(at),
                        voice,
                    }),
                    None => self.scheduler.cue(batch.epoch, at, cue.kind.clone()),
                }
            }
            self.audio.schedule(batch.epoch, audio);
        }
        for cue in &fx.fire_now {
            self.play_cue_now(&cue.kind);
        }
        for sound in &fx.ui_sounds {
            let id = match sound {
                UiSound::Open => sounds::OPEN,
                UiSound::Advance => sounds::ADVANCE,
            };
            self.audio.play(nanatype_audio::Voice {
                sound: id.into(),
                bus: Bus::Ui,
                gain: 1.0,
                pitch: 1.0,
            });
        }
        for event in &fx.events {
            self.bilibili_submitted(event);
            self.peer_submitted(event);
            self.bridge_event(event);
        }
        if let Some(doc) = &fx.replace_draft {
            self.set_source(serialize_line(doc, &self.core.draft_meta()), true);
        }
        if fx.hide_input {
            self.summoned = false;
        }
        if fx.display_changed {
            let mut display = self.core.display().clone();
            if self.peer.handle.is_some()
                && display
                    .line
                    .is_some_and(|id| !self.peer.lines.contains(&id))
            {
                display.empty = true;
            }
            self.ui.display.set(Arc::new(display));
            self.ui.now.set(clock::now_ms());
        }
        if fx.history_changed && self.peer.handle.is_none() {
            self.ui
                .history
                .set(Arc::new(self.core.history().iter().cloned().collect()));
        }
        let queued = self.core.queued();
        if self.ui.status.with_untracked(|s| s.queued != queued) {
            self.edit_status(|s| s.queued = queued);
        }
        if fx.reply_changed {
            self.ui.reply.set(self.core.pending_reply().cloned());
        }
        if fx.display_changed || fx.history_changed {
            self.refresh_atom_urls();
            self.peer_publish();
            if self.peer.handle.is_some() {
                self.peer_render();
            }
        }
    }

    /// Make sure every atom on screen has a URL.
    /// Resolve the pictures of the display, the draft and the newest
    /// history row.
    fn refresh_atom_urls(&mut self) {
        let mut assets = display_assets(self.core.display());
        assets.extend(doc_assets(self.core.draft()));
        if let Some(row) = self.core.history().back() {
            assets.extend(row_assets(&row.item, &row.doc));
        }
        self.ensure_urls(assets);
    }

    /// Give every asset of `assets` a URL the views can load.
    pub(crate) fn ensure_urls(&mut self, assets: impl IntoIterator<Item = AssetRef>) {
        let urls = self.ui.asset_urls.get_untracked();
        let found: Vec<(String, String)> = assets
            .into_iter()
            .filter(|asset| !urls.contains_key(&asset.to_string()))
            .filter_map(|asset| {
                Some((
                    asset.to_string(),
                    self.assets.url(&asset, Some(&self.loaded))?,
                ))
            })
            .collect();
        if found.is_empty() {
            return;
        }
        let mut next = (*urls).clone();
        next.extend(found);
        self.ui.asset_urls.set(Arc::new(next));
    }

    fn bridge_event(&mut self, event: &Event) {
        let out = match event {
            Event::LineStarted(line) => BridgeEvent::LineStart {
                id: line.id.0,
                text: line.doc.plain_text(),
                markup: serialize(&line.doc),
            },
            Event::LineFinished(id) => BridgeEvent::LineEnd { id: id.0 },
            Event::Expression { name, intensity } => BridgeEvent::Expression {
                name: name.clone(),
                intensity: *intensity,
            },
            Event::Portrait(v) => BridgeEvent::Portrait { variant: v.clone() },
            Event::Speaking(s) => BridgeEvent::Speaking { speaking: *s },
            Event::Shake { amp, ms } => BridgeEvent::Shake { amp: *amp, ms: *ms },
            Event::Vts(action) => BridgeEvent::Vts {
                action: action.clone(),
            },
            Event::LineSubmitted(_) | Event::LineRevealed(_) | Event::Queued { .. } => return,
        };
        if self.plugins.has_running() {
            // Plugins can draw into the streamed display slot: never hand
            // them the user's absolute file paths.
            let plugin_out = match (&out, event) {
                (BridgeEvent::LineStart { id, text, .. }, Event::LineStarted(line)) => {
                    BridgeEvent::LineStart {
                        id: *id,
                        text: text.clone(),
                        markup: serialize(&without_local_paths(&line.doc)),
                    }
                }
                _ => out.clone(),
            };
            if let Ok(event) = serde_json::to_value(&plugin_out) {
                self.plugins.broadcast(event, self.ui);
            }
        }
        self.bridge.emit(out);
    }

    // ---- the editor ---------------------------------------------------------

    /// Replace the editor text. `program` marks it as not typed by the user.
    fn set_source(&mut self, text: String, program: bool) {
        if !program
            && self.ui.bilibili.get_untracked().sending
            && nanatype_bilibili::sending::image_count(&parse(&text).doc)
                > nanatype_bilibili::sending::image_count(self.core.draft())
        {
            self.notice(self.ui.now_t("bili-images-unavailable"), true);
            return;
        }

        let mut src = self.ui.source.get_untracked();
        if src.text == text && !program {
            return;
        }
        let len = text.len();
        src.text = text;
        if program {
            src.revision += 1;
            src.selection = (len, len);
        }
        self.ui.source.set(src.clone());
        self.reparse(&src.text);
    }

    fn reparse(&mut self, text: &str) {
        let parsed = parse(text);
        let locale = self.ui.locale.get_untracked();
        let diags: Vec<String> = parsed
            .diagnostics
            .iter()
            .filter(|d| !matches!(d.kind, nanatype_markup::DiagnosticKind::Unclosed(_)))
            .map(|d| {
                let pos = text[..d.range.start.min(text.len())].chars().count() + 1;
                let message = match &d.kind {
                    nanatype_markup::DiagnosticKind::UnknownTag(t) => format!("[{t}] ?"),
                    nanatype_markup::DiagnosticKind::BadValue { tag, value } => {
                        format!("[{tag}={value}] ?")
                    }
                    nanatype_markup::DiagnosticKind::UnmatchedClose(t) => format!("[/{t}] ?"),
                    other => format!("{other:?}"),
                };
                I18N.tr_in(
                    locale,
                    "editor-diagnostic",
                    &[("pos", pos.into()), ("message", message.into())],
                )
            })
            .collect();
        self.ui.diagnostics.set(Arc::new(diags));
        let doc = parsed.doc;
        self.ui.editor_value.set(self.editor_rich(&doc));
        self.ui.draft.set(Arc::new(doc.clone()));
        self.run(Cmd::DraftMeta(parsed.meta));
        self.run(Cmd::DraftChanged(doc));
    }

    /// The editor's look: the dialogue text of the active skin.
    fn editor_look(&self) -> crate::ui::rich::Look {
        crate::ui::dialogue::Base::editor(&self.skin).look()
    }

    /// `doc` as the editor's rich text.
    fn editor_rich(&self, doc: &Document) -> nana_ui_core::RichText {
        let look = self.editor_look();
        let urls = self.ui.asset_urls.get_untracked();
        crate::ui::rich::to_rich(
            doc,
            &crate::ui::rich::Env {
                look: &look,
                urls: &urls,
                editor: true,
                locale: self.ui.locale.get_untracked(),
            },
        )
    }

    /// The user edited the rich text.
    fn editor_changed(&mut self, rich: nana_ui_core::RichText) {
        let previous = self.core.draft().clone();
        let doc = sync_rich_draft(self.ui, rich);
        if self.ui.bilibili.get_untracked().sending
            && nanatype_bilibili::sending::image_count(&doc)
                > nanatype_bilibili::sending::image_count(&previous)
        {
            self.set_source(serialize_line(&previous, &self.core.draft_meta()), true);
            self.notice(self.ui.now_t("bili-images-unavailable"), true);
            return;
        }
        self.run(Cmd::DraftChanged(doc));
        self.refresh_atom_urls();
    }

    /// A toolbar action: an editor command, or a markup edit in source mode.
    fn tool(&mut self, tool: crate::msg::Tool) {
        if self.ui.bilibili.get_untracked().sending
            && matches!(tool, crate::msg::Tool::Insert(Inline::Atom(_)))
        {
            self.notice(self.ui.now_t("bili-images-unavailable"), true);
            return;
        }
        self.focus_editor = true;
        use crate::msg::Tool;
        use nana_ui::runtime::{RichAttrMask, RichEditCommand};
        use nana_ui_core::{PaintColor, RichSpanStyle, RichTextShadow, RichTextStroke};

        if self.ui.source_mode.get_untracked() {
            if let Some(edit) = markup_for(&tool) {
                self.markup(edit);
            }
            return;
        }
        let attrs = self.ui.sel_attrs.get_untracked();
        let look = self.editor_look();
        let uniform = |mask: RichAttrMask| !attrs.mixed.contains(mask);
        let paint = |c: nanatype_model::Rgba8| PaintColor::srgb(skin_theme::rgba(c));
        // Effects and pacing share one interned attribute per span.
        let (mut effects, mut pace) = match attrs.style.effect {
            Some(index) if uniform(RichAttrMask::EFFECT) => crate::ui::rich::attr_of(index),
            _ => Default::default(),
        };
        let command = match tool {
            Tool::Bold => {
                let on = uniform(RichAttrMask::WEIGHT)
                    && attrs.style.shape.weight.is_some_and(|w| w >= 600);
                RichEditCommand::SetAttrs(RichSpanStyle::new().weight(if on {
                    400.min(look.weight)
                } else {
                    700.max(look.weight + 200).min(900)
                }))
            }
            Tool::Italic => {
                let on = uniform(RichAttrMask::ITALIC) && attrs.style.shape.italic == Some(true);
                RichEditCommand::SetAttrs(RichSpanStyle::new().italic(!on))
            }
            Tool::Underline | Tool::Strike => {
                let mut d = attrs.style.paint.decoration.unwrap_or_default();
                if matches!(tool, Tool::Underline) {
                    d.underline = !(uniform(RichAttrMask::DECORATION) && d.underline);
                } else {
                    d.line_through = !(uniform(RichAttrMask::DECORATION) && d.line_through);
                }
                let mut s = RichSpanStyle::new();
                s.paint.decoration = Some(d);
                RichEditCommand::SetAttrs(s)
            }
            Tool::Color(c) => RichEditCommand::SetAttrs(RichSpanStyle::new().color(paint(c))),
            Tool::Size(Some(nanatype_model::Size::Em(em))) => {
                RichEditCommand::SetAttrs(RichSpanStyle::new().size_em(em))
            }
            Tool::Size(Some(nanatype_model::Size::Px(px))) => {
                RichEditCommand::SetAttrs(RichSpanStyle::new().size(px))
            }
            Tool::Size(None) => RichEditCommand::ClearAttrs(RichAttrMask::SIZE),
            Tool::Font(None) => RichEditCommand::ClearAttrs(RichAttrMask::FAMILY),
            Tool::Font(Some(font)) => RichEditCommand::SetAttrs(look.font_style(&font)),
            Tool::Align(align) => {
                RichEditCommand::SetParagraphAlignment(align.map(crate::ui::rich::align_to_rich))
            }
            Tool::Stroke(st) => {
                let (w, c) = st
                    .map(|s| (s.width_px, s.color))
                    .unwrap_or((0.0, look.color));
                RichEditCommand::SetAttrs(
                    RichSpanStyle::new().stroke(
                        RichTextStroke::new(w, paint(c))
                            .placement(nana_ui_core::TextStrokePlacement::Under),
                    ),
                )
            }
            Tool::Shadow(sh) => {
                let mut s = RichSpanStyle::new();
                s.paint.shadows = Some(match sh {
                    Some(sh) => Arc::from([RichTextShadow::new(
                        [sh.dx, sh.dy],
                        sh.blur,
                        paint(sh.color),
                    )]),
                    None => Arc::from([]),
                });
                RichEditCommand::SetAttrs(s)
            }
            Tool::Ruby(text) => {
                RichEditCommand::SetRuby((!text.trim().is_empty()).then(|| text.trim().to_owned()))
            }
            Tool::Effect(_) | Tool::Entrance(_) | Tool::Pace(_) => {
                match tool {
                    Tool::Effect(kind) => effects.toggle(kind),
                    Tool::Entrance(kind) => {
                        effects.entrance = Some(nanatype_model::Entrance {
                            kind,
                            ms: self.skin.dialogue.reveal.entrance.ms.max(120),
                        })
                    }
                    Tool::Pace(p) => pace = p,
                    _ => unreachable!(),
                }
                RichEditCommand::SetAttrs(
                    RichSpanStyle::new().effect(crate::ui::rich::attr_index_any(&effects, pace)),
                )
            }
            // Built the way the editor's text builds it: a tag for a marker,
            // a picture at its own aspect.
            Tool::Insert(inline) => {
                let doc = Document::from_inlines(vec![inline]);
                self.ensure_urls(doc_assets(&doc));
                match self.editor_rich(&doc).objects().first() {
                    Some((_, object)) => RichEditCommand::InsertObject(object.clone()),
                    None => return,
                }
            }
            Tool::ClearFormat => RichEditCommand::ClearFormatting,
        };
        self.rich_commands.push(command.into());
    }

    fn markup(&mut self, edit: MarkupEdit) {
        self.source_edit_undoable |= !matches!(&edit, MarkupEdit::Typed { .. });
        let mut src = self.ui.source.get_untracked();
        let before = src.text.clone();
        if !apply_source_markup_guarded(&mut src, edit, self.ui.bilibili.get_untracked().sending) {
            self.ui.source.set(src);
            self.notice(self.ui.now_t("bili-images-unavailable"), true);
            return;
        }
        self.ui.source.set(src.clone());
        if src.text != before {
            self.reparse(&src.text);
        }
    }

    fn insert_asset(&mut self, asset: AssetRef) {
        if self.ui.bilibili.get_untracked().sending {
            self.notice(self.ui.now_t("bili-images-unavailable"), true);
            return;
        }
        self.tool(crate::msg::Tool::Insert(nanatype_model::Inline::Atom(
            nanatype_model::Atom {
                kind: nanatype_model::AtomKind::Image,
                asset,
                height_em: 2.0,
                alt: String::new(),
                effects: Default::default(),
            },
        )));
    }

    // ---- files ----------------------------------------------------------------

    fn pick(&mut self, target: FileTarget) {
        if target == FileTarget::InsertImage && self.ui.bilibili.get_untracked().sending {
            self.notice(self.ui.now_t("bili-images-unavailable"), true);
            return;
        }
        let id = self.next_dialog;
        self.next_dialog += 1;
        self.dialogs.insert(id, target);
        let t = |k: &str| self.ui.now_t(k);
        let images = FileFilter::new(
            t("tb-image").trim_end_matches('…').to_owned(),
            ["png", "jpg", "jpeg", "gif", "webp", "bmp"],
        );
        let skin_filter = FileFilter::new("NanaType package", ["ntpkg", "zip"]);
        let request = match target {
            FileTarget::Avatar => FileDialogRequest::new(id, FileDialogKind::OpenFile)
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
                    self.export_target.clone().unwrap_or_default()
                )),
            FileTarget::StickerFolder => {
                FileDialogRequest::new(id, FileDialogKind::PickFolder).title(t("assets-add-folder"))
            }
            FileTarget::EditPackage => FileDialogRequest::new(id, FileDialogKind::OpenFile)
                .title(t("pkgedit-open-file"))
                .filters([skin_filter]),
            FileTarget::ExportProject => FileDialogRequest::new(id, FileDialogKind::SaveFile)
                .title(t("pkgedit-export"))
                .filters([skin_filter])
                .file_name(format!(
                    "{}.ntpkg",
                    self.ui
                        .package_editor
                        .with_untracked(|v| v.as_ref().map(|v| v.package.clone()))
                        .unwrap_or_default()
                )),
            FileTarget::ImportProjectAsset => FileDialogRequest::new(id, FileDialogKind::OpenFile)
                .title(t("pkgedit-import"))
                .filters([FileFilter::new(
                    t("pkgedit-import"),
                    nanatype_theme::editor::IMPORT_EXTENSIONS.iter().copied(),
                )]),
        };
        let window = [WindowKind::Settings, WindowKind::Input]
            .into_iter()
            .find(|k| self.windows.is_open(*k))
            .unwrap_or(WindowKind::Input);
        self.commands.push(WindowCommand::OpenFileDialog {
            id: window.id(),
            request,
        });
    }

    pub fn file_chosen(&mut self, request: u64, paths: Vec<PathBuf>) {
        let Some(target) = self.dialogs.remove(&request) else {
            return;
        };
        let Some(path) = paths.into_iter().next() else {
            return;
        };
        match target {
            FileTarget::Avatar => {
                let p = path.display().to_string();
                self.edit(|s| s.general.avatar = Some(p));
            }
            FileTarget::InsertImage => {
                self.insert_asset(AssetRef::File(path.display().to_string()))
            }
            FileTarget::ImportSkin => {
                // Inspection validates every resource and signature: keep it off the UI thread.
                let mut registry = self.registry.clone();
                crate::plugins::background("package-install", &self.ctx, move || {
                    Msg::PackageInstalled(
                        registry
                            .install(&path)
                            .map(|outcome| (outcome, Box::new(registry))),
                    )
                });
            }
            FileTarget::ExportSkin => {
                if let Some(id) = self.export_target.take() {
                    let registry = self.registry.clone();
                    crate::plugins::background("package-export", &self.ctx, move || {
                        Msg::PackageExported(registry.export(&id, &path).map_err(|e| e.to_string()))
                    });
                }
            }
            FileTarget::StickerFolder => {
                self.import_sticker_folders(vec![path]);
            }
            FileTarget::EditPackage => self.editor_open_archive(path),
            FileTarget::ExportProject => self.editor_export(path),
            FileTarget::ImportProjectAsset => self.editor_import(path),
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
                self.plugin_activation.cancel(self.ui);
                self.registry = *registry;
                self.skin_reload.invalidate();
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
                self.plugins = Default::default();
                self.plugins.sync(
                    &self.registry,
                    &self.settings.enabled_plugins,
                    &self.settings.plugin_approvals,
                    self.ctx.clone(),
                    self.ui,
                );
                if id == self.settings.skin.id {
                    self.reload_skin(true);
                } else if self.registry.entry(&id).is_some() {
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

    // ---- update ---------------------------------------------------------------

    fn import_sticker_folders(&mut self, paths: Vec<PathBuf>) {
        self.edit(|s| crate::stickers::add_folders(&mut s.assets.sticker_dirs, &paths));
    }

    pub fn update(&mut self, msg: Msg) {
        match msg {
            Msg::PackageEditor(action) => self.package_editor_msg(action),
            m @ (Msg::RequestPluginActivation(_)
            | Msg::CancelPluginActivation
            | Msg::ConfirmPluginActivation
            | Msg::DisablePlugin(_)
            | Msg::RefreshPackages
            | Msg::RemovePackage(_)
            | Msg::PluginPackagesChanged
            | Msg::PluginPackagesReviewed(_)
            | Msg::PluginsReviewed { .. }
            | Msg::PackagesListed { .. }
            | Msg::PluginReviewReady { .. }
            | Msg::PackageInstalled(_)
            | Msg::PackageExported(_)
            | Msg::PluginResult { .. }
            | Msg::PluginCommand { .. }
            | Msg::PluginInput { .. }) => self.package_msg(m),
            m @ (Msg::StickerImportFolders(_)
            | Msg::Telegram(_)
            | Msg::TelegramEvent(_)
            | Msg::AssetsChanged
            | Msg::Stickers(_)
            | Msg::StickerDownload(_)
            | Msg::StickerRetry(_)
            | Msg::StickerRepository(_)
            | Msg::StickerRemove(_)) => self.sticker_msg(m),
            m @ (Msg::Markup(_)
            | Msg::Tool(_)
            | Msg::EditorCommand(_)
            | Msg::EditorChanged(_)
            | Msg::EditorPaste
            | Msg::ToggleSource
            | Msg::EditorContext { .. }
            | Msg::EditorAction { .. }
            | Msg::InsertPhrase(_)
            | Msg::PasteImage { .. }) => self.editor_msg(m),
            m @ (Msg::OverlayAction(..)
            | Msg::ToggleWindow(_)
            | Msg::ShowWindow(_)
            | Msg::HideInput
            | Msg::ResetWindow(_)
            | Msg::OpenSettings
            | Msg::SettingsPage(_)
            | Msg::SecondInstance
            | Msg::Displays(_)) => self.window_msg(m),
            m @ (Msg::DeleteSkin(_)
            | Msg::ExportSkin(_)
            | Msg::DuplicateSkin(_)
            | Msg::SkinFilesChanged
            | Msg::LayoutMaterialsReady
            | Msg::SkinScanReady { .. }
            | Msg::PrepareSkinPreview { .. }
            | Msg::SaveSkinPersonalization { .. }
            | Msg::SoundsLoaded { .. }) => self.skin_msg(m),
            m @ (Msg::Chat(_)
            | Msg::ChatAssetReady(_)
            | Msg::ChatMention { .. }
            | Msg::ClearReply
            | Msg::ClearChat) => self.chat_msg(m),
            m @ (Msg::Bilibili(_)
            | Msg::BilibiliLogin
            | Msg::BilibiliCancelLogin
            | Msg::BilibiliRefresh
            | Msg::BilibiliLogout
            | Msg::BilibiliOwnRoom
            | Msg::BilibiliSendToggle
            | Msg::BilibiliSendConfirm(_)
            | Msg::BilibiliRetry(_)
            | Msg::BilibiliSkipFailed(_)) => self.bilibili_msg(m),
            m
            @ (Msg::Douyin(_) | Msg::DouyinRefresh | Msg::DouyinLogout | Msg::DouyinImport(_)) => {
                self.douyin_msg(m)
            }
            Msg::PeerStart { invitation, relay } => self.peer_start(invitation, relay),
            Msg::PeerLeave => self.peer_leave(),
            Msg::PeerPanel(expanded) => self.ui.peer_panel.set(expanded),
            Msg::PeerEvent(generation, event) => self.peer_event(generation, event),
            Msg::Core(cmd) => self.run(cmd),
            Msg::Edit(f) => self.edit(f),
            Msg::Hotkey(action) => self.hotkey(action),
            Msg::RecordHotkey(action) => self.record_hotkey(action),
            Msg::HotkeyCaptured(hotkey) => self.hotkey_captured(hotkey),
            Msg::HotkeyStatus(status) => self.ui.hotkeys.set(Arc::new(status)),
            Msg::InputHwnd(hwnd) => self.hotkeys.set_input_window(hwnd),
            Msg::PickFile(target) => self.pick(target),
            Msg::OpenFolder(path) => crate::services::reveal(&path),
            Msg::OpenUrl(url) => crate::services::open_url(url),
            Msg::CopyText(text) => self.copy_text(text),
            Msg::PlaySample => self.play_sample(),
            Msg::VtsAuthorize => self.vts_authorize(),
            Msg::VtsRefresh => self.bridge.vts_refresh_hotkeys(),
            Msg::Notice { text, error } => self.notice(text, error),
            Msg::DismissNotice(id) => self.edit_status(|s| s.notices.retain(|n| n.id != id)),
            Msg::TrayErrorExpired(id) => self.tray_error_expired(id),
            // The host removes toasts from its documents.
            Msg::ToastDismissed(..) => {}
            Msg::UiChanged => {}
            Msg::OutputCleanup => self.refresh_spout_status(),
            Msg::Wake { epoch, reason } => self.run(Cmd::Wake { epoch, reason }),
            Msg::Cue(kind) => self.run(Cmd::CueFired(kind)),
            Msg::Audio(status) => self.audio_status(status),
            Msg::Bridge(status) => self.bridge_status(status),
            Msg::Quit => self.quit(),
        }
    }

    fn bridge_status(&mut self, status: BridgeStatus) {
        let set = |app: &mut Self, sink: Sink, state: LinkState| {
            app.edit_status(|s| match sink {
                Sink::Vts => s.vts = state,
                Sink::Osc => s.osc = state,
                Sink::WebSocket => s.ws = state,
            });
        };
        if let BridgeStatus::Error(sink, fault) = &status {
            crate::debug_log(&format!("bridge {sink:?}: {fault}"));
        }
        let locale = self.ui.locale.get_untracked();
        if let Some((sink, state)) = crate::ui::link_text::bridge(locale, &status) {
            set(self, sink, state);
        }
        match status {
            BridgeStatus::VtsToken(token) => self.edit(|s| s.integrations.vts.token = Some(token)),
            BridgeStatus::VtsTokenRejected => self.edit(|s| s.integrations.vts.token = None),
            BridgeStatus::VtsHotkeys(keys) => {
                let list = keys.into_iter().map(|k| (k.id, k.name)).collect();
                self.edit_status(|s| s.vts_hotkeys = list);
            }
            _ => {}
        }
    }

    // ---- live chat ----------------------------------------------------------

    fn chat_message(&mut self, message: Arc<ChatMessage>) {
        let bilibili = message.platform == crate::chat::Platform::Bilibili;
        let epoch = if bilibili {
            self.bilibili.epoch()
        } else {
            self.douyin.epoch()
        };
        if message.epoch != 0 && message.epoch != epoch {
            return;
        }
        if bilibili && self.bilibili_message_deleted(&message) {
            return;
        }
        let include = !bilibili || self.bilibili_history(&message);
        self.ensure_urls(message_assets(&message));
        let history = &self.settings.history.chat;
        if include && history.enabled && message.passes(&history.filter) {
            let locale = self.ui.locale.get_untracked();
            self.run(Cmd::External(message.history_item(locale)));
            if bilibili {
                self.bilibili_record_row(&message);
            }
        }
        self.chat.push_back(message);
        while self.chat.len() > CHAT_CAP {
            self.chat.pop_front();
        }
        self.publish_chat();
    }

    fn publish_chat(&self) {
        self.ui
            .chat
            .set(Arc::new(self.chat.iter().cloned().collect()));
    }

    fn mention(&mut self, seq: u64, reply: bool) {
        let Some(message) = self.chat.iter().rev().find(|m| m.seq == seq).cloned() else {
            return;
        };
        if reply {
            let locale = self.ui.locale.get_untracked();
            self.run(Cmd::SetReply(Some(message.quote(locale))));
        } else {
            let mention = Inline::Mention(nanatype_model::Mention::new(message.user.clone()));
            self.tool(crate::msg::Tool::Insert(mention));
            // A space after the label, so typing goes on as text.
            if self.ui.source_mode.get_untracked() {
                self.markup(MarkupEdit::Insert(" ".into()));
            } else {
                self.rich_commands
                    .push(nana_ui::runtime::RichEditCommand::InsertText(" ".into()).into());
            }
        }
        self.ui.chat_target.set(Some(seq));
        self.focus_editor = true;
        let text = self.ui.tr(
            if reply {
                "chat-replying"
            } else {
                "chat-inserted"
            },
            &[("name", message.user.name.as_str().into())],
        );
        self.toasts.push(ToastRequest::new(
            text,
            nana_ui::ToastTone::Success,
            1600,
            false,
        ));
        self.update(Msg::ShowWindow(WindowKind::Input));
    }

    pub fn open_chat(&mut self) {
        if !self.settings.integrations.bilibili.panel_open {
            self.edit(|s| s.integrations.bilibili.panel_open = true);
        }
        if self.windows.is_open(WindowKind::Chat) {
            if let Some(h) = self.windows.handle(WindowKind::Chat) {
                drop(h.set_visible(true));
                drop(h.focus());
            }
            return;
        }
        let title = self.ui.now_t(WindowKind::Chat.title_key());
        let desc = windows::descriptor(WindowKind::Chat, &title, &self.skin, &self.settings);
        self.commands.push(WindowCommand::Open {
            id: WindowKind::Chat.id(),
            settings: desc,
        });
    }

    /// The chat panel's width changed; rows re-wrap past a few pixels.
    pub fn chat_resized(&mut self, width: f32) {
        if (self.ui.chat_width.get_untracked() - width).abs() >= 4.0 {
            self.ui.chat_width.set(width);
        }
    }

    pub fn close_chat(&mut self) {
        if self.settings.integrations.bilibili.panel_open {
            self.edit(|s| s.integrations.bilibili.panel_open = false);
        }
        self.commands
            .push(WindowCommand::Close(WindowKind::Chat.id()));
    }

    fn set_input_visibility(&mut self, visible: bool) {
        let mut summoned = self.summoned;
        self.edit(|s| windows::set_input_visibility(s, &mut summoned, visible));
        self.summoned = summoned;
    }

    fn hotkey(&mut self, action: GlobalAction) {
        match action {
            GlobalAction::Summon => {
                let restoring = self.hotkeys.take_summon_restore();
                if self.settings.general.input_presence == InputPresence::Summon
                    && self.summoned
                    && !restoring
                {
                    // The summon key toggles the input window in summon mode.
                    self.summoned = false;
                } else {
                    self.update(Msg::ShowWindow(WindowKind::Input));
                    // Carbon does not grant foreground the way WM_HOTKEY does,
                    // and AppKit has to be touched on this thread.
                    #[cfg(target_os = "macos")]
                    {
                        hotkeys::activate(self.hotkeys.input_window());
                    }
                }
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
                let peer_active = self.peer.handle.is_some();
                self.edit(|s| {
                    let show = !(s.overlays.display.visible
                        || s.overlays.history.visible
                        || (peer_active && s.overlays.peer_display.visible));
                    s.overlays.display.visible = show;
                    s.overlays.peer_display.visible = show;
                    s.overlays.history.visible = show;
                });
            }
            GlobalAction::Phrase(i) => self.update(Msg::InsertPhrase(i)),
        }
    }

    pub fn open_settings(&mut self) {
        if self.windows.is_open(WindowKind::Settings) {
            self.commands
                .push(WindowCommand::Focus(WindowKind::Settings.id()));
            if let Some(h) = self.windows.handle(WindowKind::Settings) {
                drop(h.set_visible(true));
                drop(h.focus());
            }
            return;
        }
        let title = self.ui.now_t("window-settings");
        let desc = windows::descriptor(WindowKind::Settings, &title, &self.skin, &self.settings);
        self.commands.push(WindowCommand::Open {
            id: WindowKind::Settings.id(),
            settings: desc,
        });
    }

    pub fn window_size(&self, kind: WindowKind) -> (f32, f32) {
        windows::size_of(kind, &self.skin, &self.settings)
    }

    /// Native window state for this frame.
    pub fn reconcile_windows(&mut self) -> Vec<WindowCommand> {
        let want: Vec<_> = WindowKind::ALL
            .into_iter()
            .map(|k| {
                (k, {
                    let mut desired =
                        windows::desired(k, &self.skin, &self.settings, self.summoned);
                    desired.size = self.window_size(k);
                    if k == WindowKind::PeerDisplay && self.peer.handle.is_none() {
                        desired.visible = false;
                    }
                    desired
                })
            })
            .collect();
        let mut out = std::mem::take(&mut self.commands);
        out.extend(self.windows.reconcile(&want));
        self.save_geometry();
        out
    }

    /// Motion is reduced when the setting or the system asks for it.
    pub fn sync_reduced_motion(&mut self) {
        let reduced =
            self.settings.playback.reduced_motion || self.ui.system_reduced_motion.get_untracked();
        if self.ui.reduced_motion.get_untracked() != reduced {
            self.ui.reduced_motion.set(reduced);
        }
        let fx = self
            .core
            .handle(Cmd::SetReducedMotion(reduced), clock::now_ms());
        self.apply_effects(fx);
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
            exit: self.quitting,
            ..RuntimeProgramUpdate::default()
        }
    }
}

/// Pictures `doc` shows: atoms and the avatars of mentioned viewers.
fn doc_assets(doc: &Document) -> Vec<AssetRef> {
    doc.paragraphs
        .iter()
        .flat_map(|p| &p.inlines)
        .filter_map(|i| match i {
            Inline::Atom(a) => Some(a.asset.clone()),
            Inline::Mention(m) => m.user.avatar.clone(),
            _ => None,
        })
        .collect()
}

fn display_assets(view: &nanatype_core::DisplayView) -> Vec<AssetRef> {
    let mut out = doc_assets(&view.doc);
    out.extend(view.speaker.avatar.as_ref().and_then(|a| a.parse().ok()));
    out.extend(view.reply.as_ref().and_then(|r| r.user.avatar.clone()));
    out
}

/// A history row's pictures: its text's, its avatar (chat rows name a
/// saved one) and the avatar of the message it answers.
fn row_assets(item: &nanatype_model::HistoryItem, doc: &Document) -> Vec<AssetRef> {
    let mut out = doc_assets(doc);
    out.extend(
        item.avatar
            .as_deref()
            .and_then(|a| a.parse::<AssetRef>().ok()),
    );
    out.extend(item.reply.as_ref().and_then(|r| r.user.avatar.clone()));
    out
}

fn message_assets(m: &ChatMessage) -> Vec<AssetRef> {
    let mut out = doc_assets(&m.doc);
    out.extend(m.user.avatar.clone());
    out
}

/// Synchronize an actual editor event before reactive views read the draft.
pub(crate) fn sync_rich_draft(ui: Ui, rich: nana_ui_core::RichText) -> Document {
    let look = crate::ui::dialogue::Base::editor(&ui.skin.get_untracked()).look();
    let doc = crate::ui::rich::from_rich(&rich, &look);
    ui.editor_value.set(rich);
    let meta = parse(&ui.source.get_untracked().text).meta;
    let text = serialize_line(&doc, &meta);
    let mut src = ui.source.get_untracked();
    if src.text != text {
        let len = text.len();
        src.text = text;
        src.selection = (len, len);
        src.revision += 1;
        ui.source.set(src);
    }
    ui.diagnostics.set(Arc::new(Vec::new()));
    ui.draft.set(Arc::new(doc.clone()));
    doc
}

pub(crate) fn apply_source_markup_guarded(
    src: &mut crate::ui::Source,
    edit: MarkupEdit,
    synchronizing: bool,
) -> bool {
    let mut candidate = src.clone();
    apply_source_markup(&mut candidate, edit);
    if synchronizing
        && nanatype_bilibili::sending::image_count(&parse(&candidate.text).doc)
            > nanatype_bilibili::sending::image_count(&parse(&src.text).doc)
    {
        // Trigger the retained editor binding to restore its pre-edit value.
        src.revision += 1;
        return false;
    }
    *src = candidate;
    true
}

pub(crate) fn apply_source_markup(src: &mut crate::ui::Source, edit: MarkupEdit) {
    match edit {
        MarkupEdit::Typed { text, selection } => {
            src.text = text;
            src.selection = selection;
        }
        MarkupEdit::Wrap { open, close } => {
            let (a, b) = clamp_sel(&src.text, src.selection);
            src.text.insert_str(b, &close);
            src.text.insert_str(a, &open);
            let caret = if a == b {
                a + open.len()
            } else {
                b + open.len() + close.len()
            };
            src.selection = (caret, caret);
            src.revision += 1;
        }
        MarkupEdit::Insert(tag) => {
            let (a, b) = clamp_sel(&src.text, src.selection);
            src.text.replace_range(a..b, &tag);
            src.selection = (a + tag.len(), a + tag.len());
            src.revision += 1;
        }
        MarkupEdit::Align(align) => {
            let (a, b) = clamp_sel(&src.text, src.selection);
            let first = src.text[..a].bytes().filter(|v| *v == b'\n').count();
            let last_byte = if b > a { b - 1 } else { b };
            let last = src.text.as_bytes()[..last_byte]
                .iter()
                .filter(|v| **v == b'\n')
                .count();
            let mut parsed = parse(&src.text);
            for paragraph in parsed.doc.paragraphs.iter_mut().take(last + 1).skip(first) {
                paragraph.align = align;
            }
            src.text = nanatype_markup::serialize_line(&parsed.doc, &parsed.meta);
            let start = src
                .text
                .split_inclusive('\n')
                .take(first)
                .map(str::len)
                .sum::<usize>();
            let end = src
                .text
                .split_inclusive('\n')
                .take(last + 1)
                .map(str::len)
                .sum::<usize>()
                .min(src.text.len());
            src.selection = (start, end);
            src.revision += 1;
        }
        MarkupEdit::ClearFormat => {
            let (a, b) = clamp_sel(&src.text, src.selection);
            let (a, b) = if a == b { (0, src.text.len()) } else { (a, b) };
            let plain = strip_tags(&src.text[a..b]);
            src.text.replace_range(a..b, &plain);
            src.selection = (a, a + plain.len());
            if parse(&src.text)
                .doc
                .paragraphs
                .iter()
                .any(|p| p.align.is_some())
            {
                apply_source_markup(src, MarkupEdit::Align(None));
            } else {
                src.revision += 1;
            }
        }
    }
}

fn clamp_sel(text: &str, (a, b): (usize, usize)) -> (usize, usize) {
    let fix = |mut i: usize| {
        i = i.min(text.len());
        while i > 0 && !text.is_char_boundary(i) {
            i -= 1;
        }
        i
    };
    let (a, b) = (fix(a), fix(b));
    (a.min(b), a.max(b))
}

/// Remove `[tag]` and `[/tag]` but keep `[[` escapes and the text.
fn strip_tags(text: &str) -> String {
    let parsed = parse(text);
    let mut doc = parsed.doc;
    for paragraph in &mut doc.paragraphs {
        paragraph.align = None;
    }
    for run in doc.runs_mut() {
        run.style = Default::default();
        run.effects = Default::default();
        run.pace = Default::default();
    }
    serialize(&doc.normalized())
}

fn quote(v: &str) -> String {
    if v.chars()
        .any(|c| c.is_whitespace() || matches!(c, ']' | '"' | '[' | '\\'))
    {
        format!("\"{}\"", v.replace('\\', "\\\\").replace('"', "\\\""))
    } else {
        v.to_owned()
    }
}

/// Debug demo: type a line through the editor's own command path (what the
/// toolbar and keyboard drive), then send it.
fn demo_editor(ctx: &RuntimeProgramContext<Msg>) {
    use crate::msg::Tool;
    use nana_ui::runtime::RichEditCommand as C;
    use nanatype_model::{ContinuousKind, Inline, Marker};
    let text = |t: &str| Msg::EditorCommand(C::InsertText(t.into()));
    let steps = [
        text("你好，这是"),
        Msg::Tool(Tool::Bold),
        text("粗体"),
        Msg::Tool(Tool::Bold),
        text("和"),
        Msg::Tool(Tool::Effect(ContinuousKind::Wave)),
        text("波浪"),
        Msg::Tool(Tool::Effect(ContinuousKind::Wave)),
        Msg::Tool(Tool::Insert(Inline::Marker(Marker::Wait { ms: 400 }))),
        Msg::Tool(Tool::Color("#e8505b".parse().expect("colour"))),
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

/// The markup equivalent of a toolbar action (source mode).
pub(crate) fn markup_for(tool: &crate::msg::Tool) -> Option<MarkupEdit> {
    use crate::msg::Tool;
    let wrap = |open: String, close: &str| MarkupEdit::Wrap {
        open,
        close: close.to_owned(),
    };
    Some(match tool {
        Tool::Bold => wrap("[b]".into(), "[/b]"),
        Tool::Italic => wrap("[i]".into(), "[/i]"),
        Tool::Underline => wrap("[u]".into(), "[/u]"),
        Tool::Strike => wrap("[s]".into(), "[/s]"),
        Tool::Color(c) => wrap(format!("[color={c}]"), "[/color]"),
        Tool::Size(Some(nanatype_model::Size::Em(em))) => wrap(format!("[size={em}]"), "[/size]"),
        Tool::Size(Some(nanatype_model::Size::Px(px))) => wrap(format!("[size={px}px]"), "[/size]"),
        Tool::Size(None) => wrap("[size=inherit]".into(), "[/size]"),
        Tool::Font(font) => {
            let value = match font {
                None => "inherit".into(),
                Some(nanatype_model::FontSlot::Body) => "body".into(),
                Some(nanatype_model::FontSlot::Name) => "name".into(),
                Some(nanatype_model::FontSlot::Accent) => "accent".into(),
                Some(nanatype_model::FontSlot::Family(f)) => {
                    format!("\"{}\"", f.replace('\\', "\\\\").replace('\"', "\\\""))
                }
            };
            wrap(format!("[font={value}]"), "[/font]")
        }
        Tool::Align(align) => MarkupEdit::Align(*align),
        Tool::Stroke(Some(s)) => wrap(format!("[stroke={},{}]", s.color, s.width_px), "[/stroke]"),
        Tool::Shadow(Some(s)) => wrap(
            format!("[shadow={},{},{},{}]", s.color, s.dx, s.dy, s.blur),
            "[/shadow]",
        ),
        Tool::Ruby(r) => wrap(format!("[ruby={}]", quote(r)), "[/ruby]"),
        Tool::Effect(k) => wrap(format!("[{}]", k.key()), &format!("[/{}]", k.key())),
        Tool::Entrance(k) => {
            let tag = match k {
                nanatype_model::EntranceKind::Fade => "fade",
                nanatype_model::EntranceKind::Pop => "pop",
                nanatype_model::EntranceKind::SlideUp => "slide",
                nanatype_model::EntranceKind::None => "noentrance",
            };
            wrap(format!("[{tag}]"), &format!("[/{tag}]"))
        }
        Tool::Pace(nanatype_model::Pace::Instant) => wrap("[instant]".into(), "[/instant]"),
        Tool::Pace(nanatype_model::Pace::Scale(s)) => wrap(format!("[speed={s}]"), "[/speed]"),
        Tool::Pace(nanatype_model::Pace::FixedMs(ms)) => {
            wrap(format!("[speed={ms}ms]"), "[/speed]")
        }
        Tool::Insert(inline) => {
            let doc = Document::from_inlines(vec![inline.clone()]);
            MarkupEdit::Insert(serialize(&doc))
        }
        Tool::ClearFormat => MarkupEdit::ClearFormat,
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selections_clamp_to_char_boundaries() {
        assert_eq!(clamp_sel("你好", (1, 99)), (0, 6));
        assert_eq!(clamp_sel("abc", (2, 1)), (1, 2));
    }

    #[test]
    fn clearing_format_keeps_text_and_markers() {
        assert_eq!(
            strip_tags("[b]你[color=#ff0000]好[/color][/b][wait=300]"),
            "你好[wait=300]"
        );
    }

    #[test]
    fn quoting_only_when_needed() {
        assert_eq!(quote("cats/hi"), "cats/hi");
        assert_eq!(quote("file:C:/My Pics/a.png"), "\"file:C:/My Pics/a.png\"");
    }

    #[test]
    fn synchronized_source_edits_reject_images_without_changing_the_draft() {
        for tag in ["[img=blob:fixture]", "[sticker=cats/hello]", "[emote=wave]"] {
            for edit in [
                MarkupEdit::Insert(tag.into()),
                MarkupEdit::Typed {
                    text: tag.into(),
                    selection: (tag.len(), tag.len()),
                },
            ] {
                let mut source = crate::ui::Source {
                    text: "original".into(),
                    selection: (8, 8),
                    revision: 0,
                };
                assert!(
                    !apply_source_markup_guarded(&mut source, edit, true),
                    "{tag}"
                );
                assert_eq!(source.text, "original");
                assert_eq!(source.selection, (8, 8));
                assert_eq!(source.revision, 1);
            }
        }
        let mut source = crate::ui::Source {
            text: "[emote=wave]".into(),
            ..Default::default()
        };
        assert!(apply_source_markup_guarded(
            &mut source,
            MarkupEdit::Typed {
                text: "text only".into(),
                selection: (9, 9)
            },
            true
        ));
        assert_eq!(source.text, "text only");
    }
}

#[cfg(test)]
mod typography_tests {
    use super::*;
    #[test]
    fn source_alignment_targets_whole_paragraphs_and_preserves_meta_and_styles() {
        let mut src = crate::ui::Source {
            text: "[@auto=50][b]one[/b]\n[size=32px]two[/size]\n".into(),
            selection: (0, 25),
            revision: 0,
        };
        let boundary = src.text.find('\n').unwrap() + 1;
        src.selection = (1, boundary);
        let before = parse(&src.text);
        apply_source_markup(
            &mut src,
            MarkupEdit::Align(Some(nanatype_model::ParagraphAlign::Center)),
        );
        let after = parse(&src.text);
        assert_eq!(after.meta, before.meta);
        assert_eq!(
            after.doc.paragraphs[0].align,
            Some(nanatype_model::ParagraphAlign::Center)
        );
        assert_eq!(after.doc.paragraphs[1], before.doc.paragraphs[1]);
        apply_source_markup(&mut src, MarkupEdit::Align(None));
        assert_eq!(parse(&src.text).doc, before.doc);
    }
}

/// `doc` with picked files reduced to their names.
fn without_local_paths(doc: &Document) -> Document {
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
        if !self.quitting {
            self.persist.flush(Duration::from_secs(2));
        }
    }
}
