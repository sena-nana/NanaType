//! Views. Every window reads the same [`Ui`] signals, so the input preview,
//! the typewriter and the history list always agree on how a line looks. The
//! skin renderer itself lives in `nanatype-frame` and reads the [`Stage`]
//! inside [`Ui`].

pub mod bilibili;
pub mod blivechat;
pub mod danmaku;
pub mod dialogue;
pub mod douyin;
pub mod history;
pub mod input;
pub mod link_text;
pub mod p2p;
pub mod plugins;
pub mod settings;
pub mod twitch;
pub mod youtube;

pub use nanatype_frame::ui::{
    I18N, MONO, Source, Stage, avatar, c, collect_effect_garbage, controls, editor_view, feedback,
    kit, materials, mono, motion, overflow, pipeline, plugin_view, preview, quote, restyle, rich,
    shader_effects, surface, sweep, text_light, text_scramble, tnum,
};

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, Weak};

use nana_ui::runtime::view::{NodeRef, Signal, computed, node_ref, signal};
use nanatype_i18n::Locale;
use nanatype_model::ChatRef;
use nanatype_settings::Settings;
use nanatype_theme::Skin;

use crate::msg::GlobalAction;
use crate::services::hotkeys::HotkeyState;

/// A notice, shown as a toast (see [`feedback`]).
#[derive(Debug, Clone, PartialEq)]
pub struct Notice {
    pub id: u64,
    pub text: String,
    pub error: bool,
}

/// Where an outbound link (VTube Studio, OSC, WebSocket, Spout) stands.
/// `Connected` and `Error` carry the text to show; the other states are
/// labelled in the viewer's locale when drawn.
#[derive(Debug, Clone, Default, PartialEq)]
pub enum LinkState {
    #[default]
    Off,
    Connecting,
    Connected(String),
    NeedsAuth,
    Error(String),
}

/// Connection and device states for the settings pages.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Status {
    pub audio: String,
    pub vts: LinkState,
    pub vts_hotkeys: Vec<(String, String)>,
    pub osc: LinkState,
    pub ws: LinkState,
    /// Bilibili: what the panel and the settings say, and the dot's state.
    pub bilibili: String,
    pub bilibili_link: danmaku::Link,
    /// Third-party chat (blivechat or 彗星号), likewise.
    pub blivechat: String,
    pub blivechat_link: danmaku::Link,
    pub spout_display: LinkState,
    pub spout_peer_display: LinkState,
    pub spout_history: LinkState,
    pub notices: Vec<Notice>,
    pub queued: usize,
    pub audio_devices: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SkinRow {
    pub id: String,
    pub name: String,
    pub description: String,
    /// The skin's chrome is dark.
    pub dark: bool,
    pub tags: Vec<String>,
    /// The loaded skin, for the picker's preview.
    pub skin: Option<Arc<Skin>>,
    pub author: String,
    pub version: String,
    pub builtin: bool,
    pub error: Option<String>,
    pub folder: Option<std::path::PathBuf>,
    pub image_urls: Arc<HashMap<String, String>>,
    pub accent: [f32; 4],
    pub box_color: [f32; 4],
    pub text_color: [f32; 4],
    /// None of the skin's body fonts is installed, bundled or embedded by
    /// NanaUI, so its text shows in a stand-in
    /// ([`crate::skin_theme::uses_fallback_font`]).
    pub fallback_font: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StickerRow {
    pub asset: String,
    pub name: String,
    pub lib: String,
    pub url: String,
    pub height_em: f32,
    pub pack_title: String,
}

/// Shared reactive state. `Copy`: views capture it freely. The skin
/// renderer's part is the [`Stage`] it derefs to.
#[derive(Clone, Copy)]
pub struct Ui {
    pub stage: Stage,
    pub packages: Signal<Arc<Vec<crate::plugins::PackageRow>>>,
    pub plugin_errors: Signal<Arc<HashMap<String, String>>>,
    pub plugin_activation: Signal<Option<Arc<crate::plugins::ActivationReview>>>,
    pub plugin_source_trusted: Signal<bool>,
    pub plugin_activation_busy: Signal<bool>,
    pub plugin_activation_error: Signal<Option<String>>,
    pub bilibili: Signal<Arc<crate::app::bilibili::State>>,
    pub douyin: Signal<Arc<crate::app::douyin::State>>,
    pub youtube: Signal<Arc<crate::app::youtube::State>>,
    pub twitch: Signal<Arc<crate::app::twitch::State>>,
    pub blivechat: Signal<Arc<crate::app::blivechat::State>>,
    pub update: Signal<Arc<crate::app::update::State>>,
    /// Kept only in memory and cleared after import; never serialized with settings.
    pub douyin_cookie: Signal<String>,
    pub skin_preview_history: Signal<bool>,
    /// The platform the live-platforms settings page shows.
    pub platform_tab: Signal<crate::chat::Platform>,
    /// Actual input client width, published by its layout (also in headless views).
    pub input_width: Signal<f32>,
    pub(crate) input_sidebar_width: Signal<f32>,
    pub(crate) input_dock_regions: Signal<Vec<nanatype_settings::InputDockRegion>>,
    pub(crate) input_dock_selected: Signal<u64>,
    pub(crate) input_split: NodeRef,
    pub peer: Signal<Arc<crate::p2p::SessionUi>>,
    pub peer_panel: Signal<bool>,
    pub diagnostics: Signal<Arc<Vec<String>>>,
    /// Skin previews animate only on the Skins page.
    pub page: Signal<settings::Page>,
    pub status: Signal<Arc<Status>>,
    pub skins: Signal<Arc<Vec<SkinRow>>>,
    pub stickers: Signal<Arc<Vec<StickerRow>>>,
    pub sticker_packs: Signal<Arc<Vec<crate::stickers::PackRow>>>,
    pub telegram: Signal<Arc<crate::stickers::telegram::State>>,
    pub recording: Signal<Option<GlobalAction>>,
    pub hotkeys: Signal<Arc<HashMap<GlobalAction, HotkeyState>>>,
    /// The system's own reduce-motion preference, as last reported.
    pub system_reduced_motion: Signal<bool>,
    /// Whether the input window is shown by the application's presence policy.
    pub input_shown: Signal<bool>,
    /// The first-run guide's step, 0-based; kept while the user visits other
    /// pages (its "Connect OBS" goes to Output).
    pub onboard_step: Signal<u8>,
    pub settings_pulse_ms: Signal<u64>,
    /// Live chat, oldest first (capped).
    pub chat: Signal<Arc<Vec<Arc<crate::chat::ChatMessage>>>>,
    /// The chat message the next sent line answers.
    pub reply: Signal<Option<ChatRef>>,
    /// The chat panel's width, so its rows can lay text out to it.
    pub chat_width: Signal<f32>,
    /// Last chat action, retained while editing its reply.
    pub chat_target: Signal<Option<u64>>,
    /// The skin library shows cards (`true`) or the list; kept while the
    /// user visits other pages.
    pub skins_grid: Signal<bool>,
}

impl std::ops::Deref for Ui {
    type Target = Stage;
    fn deref(&self) -> &Stage {
        &self.stage
    }
}

impl Ui {
    pub fn new(settings: Arc<Settings>, skin: Arc<Skin>, locale: Locale) -> Self {
        let input_dock = nanatype_settings::InputDockLayout::default();
        let peer = signal(Arc::new(crate::p2p::SessionUi::default()));
        let page = signal(settings::Page::General);
        let mut stage = Stage::new(settings, skin, locale);
        stage.peer_active = computed(move || peer.with(|p| p.active));
        stage.live_preview = computed(move || page.get() == settings::Page::Skins);
        Self {
            stage,
            packages: signal(Arc::new(Vec::new())),
            plugin_errors: signal(Arc::new(HashMap::new())),
            plugin_activation: signal(None),
            plugin_source_trusted: signal(false),
            plugin_activation_busy: signal(false),
            plugin_activation_error: signal(None),
            bilibili: signal(Arc::new(crate::app::bilibili::State::default())),
            douyin: signal(Arc::new(crate::app::douyin::State::default())),
            youtube: signal(Arc::new(crate::app::youtube::State::default())),
            twitch: signal(Arc::new(crate::app::twitch::State::default())),
            blivechat: signal(Arc::new(crate::app::blivechat::State::default())),
            update: signal(Arc::new(crate::app::update::State::default())),
            douyin_cookie: signal(String::new()),
            skin_preview_history: signal(false),
            input_width: signal(crate::windows::INPUT_SIZE.0),
            input_sidebar_width: signal(nanatype_settings::INPUT_SIDEBAR_DEFAULT),
            input_dock_regions: signal(input_dock.regions),
            input_dock_selected: signal(input_dock.selected),
            input_split: node_ref(),
            peer,
            platform_tab: signal(crate::chat::Platform::Bilibili),
            peer_panel: signal(true),
            diagnostics: signal(Arc::new(Vec::new())),
            page,
            status: signal(Arc::new(Status::default())),
            skins: signal(Arc::new(Vec::new())),
            stickers: signal(Arc::new(Vec::new())),
            sticker_packs: signal(Arc::new(Vec::new())),
            telegram: signal(Arc::new(Default::default())),
            recording: signal(None),
            hotkeys: signal(Arc::new(HashMap::new())),
            system_reduced_motion: signal(false),
            input_shown: signal(true),
            onboard_step: signal(0),
            settings_pulse_ms: signal(0),
            chat: signal(Arc::new(Vec::new())),
            reply: signal(None),
            chat_width: signal(400.0),
            chat_target: signal(None),
            skins_grid: signal(true),
        }
    }

    /// The broadcast window this track's display draws in.
    pub fn display_kind(self) -> crate::windows::WindowKind {
        if self.track_slot == 1 {
            crate::windows::WindowKind::PeerDisplay
        } else {
            crate::windows::WindowKind::Display
        }
    }

    /// The remote participant's track (see [`Stage::remote`]).
    pub fn remote(self) -> Self {
        Self {
            stage: self.stage.remote(),
            ..self
        }
    }
}

fn picker_image_urls(entry: &nanatype_theme::SkinEntry) -> Arc<HashMap<String, String>> {
    type Cached = (Weak<Skin>, Arc<HashMap<String, String>>);
    static BUILTIN_IMAGES: LazyLock<Mutex<HashMap<String, Cached>>> =
        LazyLock::new(|| Mutex::new(HashMap::new()));
    let (Some(skin), Some(files)) = (&entry.skin, entry.files()) else {
        return Arc::default();
    };
    // Cache only immutable embedded resources. Directory edits must always
    // produce fresh content URLs, even when file length/mtime do not change.
    if entry.is_builtin() && crate::services::media::global().is_some() {
        let mut cache = BUILTIN_IMAGES.lock().unwrap_or_else(|e| e.into_inner());
        if let Some((previous, urls)) = cache.get(&entry.id)
            && previous.upgrade().is_some_and(|s| Arc::ptr_eq(&s, skin))
        {
            return urls.clone();
        }
        let urls = Arc::new(crate::assets::skin_preview_urls(skin, files));
        cache.insert(entry.id.clone(), (Arc::downgrade(skin), urls.clone()));
        urls
    } else {
        Arc::new(crate::assets::skin_preview_urls(skin, files))
    }
}

/// The skin picker's static rows, built-ins first.
pub fn skin_rows(registry: &nanatype_theme::SkinRegistry, locale: Locale) -> Vec<SkinRow> {
    registry
        .entries()
        .iter()
        .map(|e| {
            let mut row = SkinRow {
                id: e.id.clone(),
                name: e.id.clone(),
                description: String::new(),
                dark: false,
                tags: Vec::new(),
                author: e.author.clone(),
                version: e.version.clone(),
                builtin: e.is_builtin(),
                error: e.errors.first().map(|d| d.to_string()),
                folder: match &e.origin {
                    nanatype_theme::SkinOrigin::Directory(p) => Some(p.clone()),
                    _ => None,
                },
                image_urls: picker_image_urls(e),
                accent: [0.5; 4],
                box_color: [0.5; 4],
                text_color: [0.5; 4],
                fallback_font: false,
                skin: e.prepared_skin().cloned().or_else(|| e.skin.clone()),
            };
            if let Some(s) = &e.skin {
                let b = &s.dialogue.box_;
                row.name = s.display_name(locale.tag());
                row.description = s.description(locale.tag());
                row.dark = s.chrome.appearance == nanatype_theme::Appearance::Dark;
                row.tags = (s.meta.tags.iter())
                    .filter(|t| nanatype_theme::SKIN_TAGS.contains(&t.as_str()))
                    .cloned()
                    .collect();
                row.accent = c(s.chrome.accent);
                // A gradient box shows its first stop.
                let first = b.shape.gradient.as_ref().and_then(|g| g.stops.first());
                row.box_color = c(first.map_or(b.color, |(_, col)| s.color(*col)));
                row.text_color = c(s.dialogue.text.color);
                row.fallback_font = crate::skin_theme::uses_fallback_font(s);
            }
            row
        })
        .collect()
}

/// Prepare only the playing preview, preserving the active skin and the
/// other catalogue models. A row change rebuilds this one preview once.
pub fn prepare_skin_preview(
    ui: Ui,
    registry: &nanatype_theme::SkinRegistry,
    id: &str,
    play: bool,
) -> Result<(), Vec<nanatype_theme::SkinDiagnostic>> {
    if !shader_effects::enabled(*ui)
        && !registry
            .entry(id)
            .and_then(|e| e.skin.as_ref())
            .is_some_and(|s| s.shaders.values().any(|s| s.static_fallback))
        && !(materials::enabled(*ui)
            && registry
                .entry(id)
                .and_then(|e| e.skin.as_ref())
                .is_some_and(|s| !s.materials.is_empty()))
    {
        ui.shader_preview.set(false);
        return Ok(());
    }
    let loaded = registry
        .load(id)
        .inspect_err(|_| ui.shader_preview.set(false))?;
    let animate = play
        && shader_effects::enabled(*ui)
        && (loaded.skin.dialogue.shader_effects.iter().any(|e| {
            loaded
                .skin
                .shaders
                .get(&e.shader)
                .is_some_and(|s| s.animated)
        }) || loaded.skin.dialogue.reveal.light.is_some()
            || loaded.skin.entrance_needs_text_layout()
            || loaded.skin.dialogue.reveal.scramble.is_some()
            || loaded
                .skin
                .dialogue
                .decor
                .iter()
                .any(|d| d.shader.is_some()));
    let previous = ui.skins.with_untracked(|rows| {
        rows.iter()
            .find(|r| r.id == id)
            .map(|r| r.image_urls.values().cloned().collect::<Vec<_>>())
            .unwrap_or_default()
    });
    ui.skins.update(|rows| {
        if let Some(row) = Arc::make_mut(rows).iter_mut().find(|r| r.id == id) {
            if !row.builtin {
                row.image_urls = Arc::new(crate::assets::skin_preview_urls(
                    &loaded.skin,
                    &*loaded.files,
                ));
            }
            row.skin = Some(loaded.skin);
        }
    });
    retire_skin_urls(ui, previous);
    ui.shader_preview.set(animate);
    Ok(())
}

pub fn retire_skin_urls(ui: Ui, previous: impl IntoIterator<Item = String>) {
    let Some(media) = crate::services::media::global() else {
        return;
    };
    let mut keep: std::collections::HashSet<String> = ui
        .asset_urls
        .with_untracked(|u| u.values().cloned().collect());
    ui.skins.with_untracked(|rows| {
        keep.extend(rows.iter().flat_map(|r| r.image_urls.values().cloned()))
    });
    ui.portraits
        .with_untracked(|urls| keep.extend(urls.values().cloned()));
    if let Some(path) = ui.settings.with_untracked(|s| s.general.avatar.clone())
        && let Some(url) = crate::services::media::url(std::path::Path::new(&path), None)
    {
        keep.insert(url);
    }
    media.retire_urls(previous, &keep);
}
