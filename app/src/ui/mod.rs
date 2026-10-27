//! Views. Every window reads the same [`Ui`] signals, so the input preview,
//! the typewriter and the history list always agree on how a line looks. The
//! skin renderer itself lives in `nanatype-frame` and reads the [`Stage`]
//! inside [`Ui`].

pub mod blivechat;
pub mod danmaku;
pub mod dialogue;
pub mod history;
pub mod input;
pub mod link_text;
pub mod p2p;
pub mod plugins;
pub mod settings;
pub mod sources;
pub(crate) mod speech;
pub mod twitch;

pub use nanatype_frame::ui::{
    I18N, MONO, REMOTE_LANES, Source, Stage, avatar, c, collect_effect_garbage, controls,
    editor_view, feedback, kit, materials, mono, motion, overflow, pipeline, plugin_view, preview,
    quote, remote_bands, restyle, rich, shader_effects, surface, sweep, text_light, text_scramble,
    tnum,
};

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, Weak};

use nana_ui::runtime::view::{NodeRef, Signal, computed, node_ref, signal};
use nanatype_i18n::Locale;
use nanatype_model::ChatRef;
use nanatype_settings::Settings;
use nanatype_theme::Skin;

use crate::msg::{GlobalAction, Shortcut};
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
    /// Third-party chat (blivechat or 彗星号), likewise.
    pub blivechat: String,
    pub blivechat_link: danmaku::Link,
    pub spout_display: LinkState,
    pub spout_peer_display: LinkState,
    pub spout_history: LinkState,
    /// The obs-websocket link.
    pub obs: LinkState,
    pub notices: Vec<Notice>,
    pub queued: usize,
    pub audio_devices: Vec<String>,
    pub performance: Performance,
}

/// What the scheduling preferences achieved, for the Output page's hints.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Performance {
    /// The raised GPU priority is in effect.
    pub gpu_high: bool,
    pub cpu: nanatype_scheduling::Status,
    /// The GPUs on offer and the choice startup handed the renderer.
    pub gpu: nanatype_scheduling::GpuChoice,
    /// The GPU the renderer is on, once it has a device.
    pub gpu_running: Option<String>,
}

/// Why a CPU preference is not in effect, in the UI's language.
pub fn problem_text(ui: Ui, problem: &nanatype_scheduling::Problem) -> String {
    use nanatype_scheduling::Problem;
    let detail = |key, detail: &str| ui.tr(key, &[("detail", detail.into())]);
    match problem {
        Problem::Priority(error) => detail("perf-error-priority", error),
        Problem::Affinity(error) => detail("perf-error-affinity", error),
        Problem::Topology(Some(error)) => detail("perf-error-topology-detail", error),
        Problem::Topology(None) => ui.tr("perf-error-topology", &[]),
        Problem::TopologyIncomplete => ui.tr("perf-error-topology-incomplete", &[]),
        Problem::NoHybrid => ui.tr("perf-no-hybrid", &[]),
        Problem::NoPerformanceCores => ui.tr("perf-no-performance-cores", &[]),
        Problem::Unsupported => ui.tr("perf-unsupported", &[]),
    }
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
    /// The Steam Workshop item it comes from.
    pub workshop: Option<u64>,
    pub image_urls: Arc<HashMap<String, String>>,
    pub accent: [f32; 4],
    pub box_color: [f32; 4],
    pub text_color: [f32; 4],
    /// None of the skin's body fonts is installed, bundled or embedded by
    /// NanaUI, so its text shows in a stand-in
    /// ([`crate::skin_theme::uses_fallback_font`]).
    pub fallback_font: bool,
}

/// One installed sound pack, for the picker on the Playback page.
#[derive(Debug, Clone, PartialEq)]
pub struct SoundPackRow {
    pub id: String,
    pub name: String,
    pub author: String,
    pub error: Option<String>,
    pub folder: Option<std::path::PathBuf>,
    /// The Steam Workshop item it comes from.
    pub workshop: Option<u64>,
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

/// How the Windows / Command key is spelled on a keycap.
pub const META_LABEL: &str = if cfg!(target_os = "macos") {
    "⌘"
} else {
    "Win"
};

/// Shared reactive state. `Copy`: views capture it freely. The skin
/// renderer's part is the [`Stage`] it derefs to.
#[derive(Clone, Copy)]
pub struct Ui {
    pub speech: Signal<Arc<crate::app::speech::State>>,
    /// Set synchronously in the editor routing phase, before queued messages/timers.
    pub speech_edit_pending: Signal<bool>,
    pub stage: Stage,
    pub packages: Signal<Arc<Vec<crate::plugins::PackageRow>>>,
    pub plugin_errors: Signal<Arc<HashMap<String, String>>>,
    pub plugin_activation: Signal<Option<Arc<crate::plugins::ActivationReview>>>,
    pub plugin_source_trusted: Signal<bool>,
    pub plugin_activation_busy: Signal<bool>,
    pub plugin_activation_error: Signal<Option<String>>,
    pub twitch: Signal<Arc<crate::app::twitch::State>>,
    pub blivechat: Signal<Arc<crate::app::blivechat::State>>,
    /// The OBS link and its program scene.
    pub obs: Signal<Arc<crate::app::obs::State>>,
    #[cfg(feature = "updates")]
    pub update: Signal<Arc<crate::app::update::State>>,
    /// Kept only in memory and cleared after import; never serialized with settings.
    /// The platform the live-platforms settings page shows.
    pub platform_tab: Signal<crate::chat::Platform>,
    /// Actual input client width, published by its layout (also in headless views).
    pub input_width: Signal<f32>,
    pub(crate) input_sidebar_width: Signal<f32>,
    /// The sidebar's panel, pinned chat and fold state.
    pub input_dock: Signal<nanatype_settings::InputDockLayout>,
    pub(crate) input_split: NodeRef,
    /// The split between pinned chat and the insert panels.
    pub(crate) input_chat_split: NodeRef,
    pub peer: Signal<Arc<crate::p2p::SessionUi>>,
    pub peer_panel: Signal<bool>,
    /// The invitation field. A Steam friend invite fills it from outside the card.
    pub peer_invitation: Signal<String>,
    /// The management popover. A Steam invite opens it so the password can be typed.
    pub peer_popover: Signal<bool>,
    pub diagnostics: Signal<Arc<Vec<String>>>,
    /// Skin previews animate only on the Skins page.
    pub page: Signal<settings::Page>,
    pub status: Signal<Arc<Status>>,
    pub skins: Signal<Arc<Vec<SkinRow>>>,
    /// The skin preview's prepared stage, kept while the page is away so
    /// coming back shows it at once.
    pub skin_preview: Signal<Option<Arc<SkinPreview>>>,
    pub sound_packs: Signal<Arc<Vec<SoundPackRow>>>,
    /// Steam Workshop subscriptions; empty and hidden outside the Steam build.
    pub workshop: Signal<Arc<crate::workshop::View>>,
    pub stickers: Signal<Arc<Vec<StickerRow>>>,
    pub sticker_packs: Signal<Arc<Vec<crate::stickers::PackRow>>>,
    pub telegram: Signal<Arc<crate::stickers::telegram::State>>,
    pub recording: Signal<Option<Shortcut>>,
    pub hotkeys: Signal<Arc<HashMap<GlobalAction, HotkeyState>>>,
    /// The system's own reduce-motion preference, as last reported.
    pub system_reduced_motion: Signal<bool>,
    /// Whether the input window is shown by the application's presence policy.
    pub input_shown: Signal<bool>,
    /// The first-run guide's step, 0-based; kept while the user visits other
    /// pages (its "Connect OBS" goes to Output).
    pub onboard_step: Signal<u8>,
    /// The skin step mounts its live preview. Navigation holds it for the
    /// step's entrance; opening the step directly shows it at once.
    pub onboard_skin_shown: Signal<bool>,
    pub settings_pulse_ms: Signal<u64>,
    /// Live chat, oldest first (capped).
    pub chat: Signal<Arc<Vec<Arc<crate::chat::ChatMessage>>>>,
    /// The chat message the next sent line answers.
    pub reply: Signal<Option<ChatRef>>,
    /// The sidebar chat's width, so its rows can lay text out to it.
    pub chat_width: Signal<f32>,
    /// Last chat action, retained while editing its reply.
    pub chat_target: Signal<Option<u64>>,
    /// The `@` / `/` list open over the editor.
    pub completion: Signal<Option<Arc<crate::completion::CompletionView>>>,
    /// Where the list shows in the input window (left, top), placed by the
    /// host from the editor's layout; none until it has been.
    pub completion_at: Signal<Option<[f32; 2]>>,
    pub(crate) completion_node: NodeRef,
    /// Actual input client height, beside `input_width`.
    pub input_height: Signal<f32>,
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
        let peer = signal(Arc::new(crate::p2p::SessionUi::default()));
        let page = signal(settings::Page::General);
        let onboard_step = signal(0u8);
        let mut stage = Stage::new(settings, skin, locale);
        stage.peer_active = computed(move || peer.with(|p| p.active));
        let stage_settings = stage.settings;
        stage.live_preview = computed(move || {
            page.get() == settings::Page::Skins
                || (page.get() == settings::Page::General
                    && !stage_settings.with(|s| s.general.onboarding_done)
                    && onboard_step.get() == 1)
        });
        Self {
            stage,
            packages: signal(Arc::new(Vec::new())),
            plugin_errors: signal(Arc::new(HashMap::new())),
            plugin_activation: signal(None),
            plugin_source_trusted: signal(false),
            plugin_activation_busy: signal(false),
            plugin_activation_error: signal(None),
            speech: signal(Arc::new(crate::app::speech::State::default())),
            speech_edit_pending: signal(false),
            twitch: signal(Arc::new(crate::app::twitch::State::default())),
            blivechat: signal(Arc::new(crate::app::blivechat::State::default())),
            obs: signal(Arc::new(crate::app::obs::State::default())),
            #[cfg(feature = "updates")]
            update: signal(Arc::new(crate::app::update::State::default())),
            input_width: signal(crate::windows::INPUT_SIZE.0),
            input_sidebar_width: signal(nanatype_settings::INPUT_SIDEBAR_DEFAULT),
            input_dock: signal(nanatype_settings::InputDockLayout::default()),
            input_split: node_ref(),
            input_chat_split: node_ref(),
            peer,
            platform_tab: signal(crate::chat::Platform::ALL[0]),
            peer_panel: signal(true),
            peer_invitation: signal(String::new()),
            peer_popover: signal(false),
            diagnostics: signal(Arc::new(Vec::new())),
            page,
            status: signal(Arc::new(Status::default())),
            skins: signal(Arc::new(Vec::new())),
            skin_preview: signal(None),
            sound_packs: signal(Arc::new(Vec::new())),
            workshop: signal(Arc::new(crate::workshop::View::default())),
            stickers: signal(Arc::new(Vec::new())),
            sticker_packs: signal(Arc::new(Vec::new())),
            telegram: signal(Arc::new(Default::default())),
            recording: signal(None),
            hotkeys: signal(Arc::new(HashMap::new())),
            system_reduced_motion: signal(false),
            input_shown: signal(true),
            onboard_step,
            onboard_skin_shown: signal(true),
            settings_pulse_ms: signal(0),
            chat: signal(Arc::new(Vec::new())),
            reply: signal(None),
            chat_width: signal(400.0),
            chat_target: signal(None),
            completion: signal(None),
            completion_at: signal(None),
            completion_node: node_ref(),
            input_height: signal(crate::windows::INPUT_SIZE.1),
            skins_grid: signal(true),
        }
    }

    /// The broadcast window this track's display draws in.
    pub fn display_kind(self) -> crate::windows::WindowKind {
        if self.track_slot > 0 {
            crate::windows::WindowKind::PeerDisplay
        } else {
            crate::windows::WindowKind::Display
        }
    }

    /// Remote person `index` in seat order.
    pub fn lane(self, index: usize) -> Self {
        Self {
            stage: self.stage.lane(index),
            ..self
        }
    }

    /// The first remote person's track (see [`Stage::remote`]).
    pub fn remote(self) -> Self {
        self.lane(0)
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
                    nanatype_theme::SkinOrigin::Workshop { folder, .. } => Some(folder.clone()),
                    _ => None,
                },
                workshop: e.workshop_item(),
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

pub fn sound_pack_rows(
    registry: &nanatype_theme::SkinRegistry,
    locale: Locale,
) -> Vec<SoundPackRow> {
    registry
        .sound_packs()
        .iter()
        .map(|p| SoundPackRow {
            id: p.id.clone(),
            name: nanatype_theme::localized(&p.name, locale.tag(), &p.id),
            author: p.author.clone(),
            error: p.errors.first().map(|d| d.to_string()),
            folder: match &p.origin {
                nanatype_theme::SkinOrigin::Directory(path) => Some(path.clone()),
                nanatype_theme::SkinOrigin::Workshop { folder, .. } => Some(folder.clone()),
                _ => None,
            },
            workshop: match p.origin {
                nanatype_theme::SkinOrigin::Workshop { item, .. } => Some(item),
                _ => None,
            },
        })
        .collect()
}

/// Prepare only the playing preview, preserving the active skin and the
/// other catalogue models. A row change rebuilds this one preview once.
/// The skin preview's prepared stage, and what it was prepared from: a
/// newer catalogue model or another language asks for a new one.
pub struct SkinPreview {
    pub id: String,
    pub source: Arc<Skin>,
    pub language: String,
    pub assets: Arc<preview::StageAssets>,
}

impl SkinPreview {
    /// Whether this is the stage of `row` in `language`.
    pub fn shows(&self, row: &SkinRow, language: &str) -> bool {
        self.id == row.id
            && self.language == language
            && row
                .skin
                .as_ref()
                .is_some_and(|s| Arc::ptr_eq(s, &self.source))
    }
}

/// A skin preview to prepare: what the UI thread knows, for the loading,
/// font registration and theming [`PreviewJob::run`] does on any thread.
pub struct PreviewJob {
    id: String,
    play: bool,
    effects: bool,
    materials: bool,
    row: Option<SkinRow>,
    language: String,
}

/// The fully loaded skin, with its preview images when not built in.
type LoadedPreview = (Arc<Skin>, Option<HashMap<String, String>>);

/// A prepared skin preview, for [`apply_skin_preview`] on the UI thread.
pub struct PreparedPreview {
    id: String,
    animate: bool,
    loaded: Option<LoadedPreview>,
    errors: Vec<nanatype_theme::SkinDiagnostic>,
    preview: Option<SkinPreview>,
}

impl PreviewJob {
    pub fn new(ui: Ui, id: &str, play: bool) -> Self {
        Self {
            id: id.to_owned(),
            play,
            effects: shader_effects::enabled(*ui),
            materials: materials::enabled(*ui),
            row: ui
                .skins
                .with_untracked(|rows| rows.iter().find(|r| r.id == id).cloned()),
            language: ui.locale.get_untracked().tag().to_owned(),
        }
    }

    pub fn run(self, registry: &nanatype_theme::SkinRegistry) -> PreparedPreview {
        let id = self.id.as_str();
        let catalogued = registry.entry(id).and_then(|e| e.skin.as_ref());
        // Only effects, static shader fallbacks and materials need the
        // fully loaded skin; everything else draws from the catalogue.
        let load = self.effects
            || catalogued.is_some_and(|s| s.shaders.values().any(|s| s.static_fallback))
            || (self.materials && catalogued.is_some_and(|s| !s.materials.is_empty()));
        let row_skin = self.row.as_ref().and_then(|r| r.skin.clone());
        let builtin = self.row.as_ref().is_none_or(|r| r.builtin);
        let (loaded, errors, animate) = match load.then(|| registry.load(id)) {
            None => (None, Vec::new(), false),
            Some(Err(errors)) => (None, errors, false),
            Some(Ok(loaded)) => {
                let skin = &loaded.skin;
                let animate = self.play
                    && self.effects
                    && (skin
                        .dialogue
                        .shader_effects
                        .iter()
                        .any(|e| skin.shaders.get(&e.shader).is_some_and(|s| s.animated))
                        || skin.dialogue.reveal.light.is_some()
                        || skin.entrance_needs_text_layout()
                        || skin.dialogue.reveal.scramble.is_some()
                        || skin.dialogue.decor.iter().any(|d| d.shader.is_some()));
                let urls =
                    (!builtin).then(|| crate::assets::skin_preview_urls(skin, &*loaded.files));
                (Some((loaded.skin.clone(), urls)), Vec::new(), animate)
            }
        };
        let source = loaded.as_ref().map(|(s, _)| s.clone()).or(row_skin);
        let folder = self.row.and_then(|r| r.folder);
        let preview = source.map(|source| SkinPreview {
            assets: Arc::new(preview::StageAssets::prepare(
                (*source).clone(),
                |file| {
                    folder
                        .as_ref()
                        .and_then(|dir| std::fs::read(dir.join(file)).ok())
                },
                &self.language,
            )),
            id: self.id.clone(),
            source,
            language: self.language,
        });
        PreparedPreview {
            id: self.id,
            animate,
            loaded,
            errors,
            preview,
        }
    }
}

impl PreparedPreview {
    /// Let go of a preview that was asked for again before it was ready.
    pub fn discard(self, ui: Ui) {
        if let Some((_, Some(urls))) = self.loaded {
            retire_skin_urls(ui, urls.into_values());
        }
    }
}

/// Show `prepared`: the loaded skin in its catalogue row, its stage, and
/// whether its effects play. Returns the errors loading it reported.
pub fn apply_skin_preview(
    ui: Ui,
    prepared: PreparedPreview,
) -> Result<(), Vec<nanatype_theme::SkinDiagnostic>> {
    let id = prepared.id.as_str();
    if let Some((skin, urls)) = prepared.loaded {
        // A built-in skin loads to the same model each time: leave the
        // catalogue, and the library drawn from it, alone.
        let changed = urls.is_some()
            || ui.skins.with_untracked(|rows| {
                rows.iter()
                    .find(|r| r.id == id)
                    .is_some_and(|r| !r.skin.as_ref().is_some_and(|s| Arc::ptr_eq(s, &skin)))
            });
        if changed {
            let previous = ui.skins.with_untracked(|rows| {
                rows.iter()
                    .find(|r| r.id == id)
                    .map(|r| r.image_urls.values().cloned().collect::<Vec<_>>())
                    .unwrap_or_default()
            });
            ui.skins.update(|rows| {
                if let Some(row) = Arc::make_mut(rows).iter_mut().find(|r| r.id == id) {
                    if let Some(urls) = urls {
                        row.image_urls = Arc::new(urls);
                    }
                    row.skin = Some(skin);
                }
            });
            retire_skin_urls(ui, previous);
        }
    }
    ui.shader_preview.set(prepared.animate);
    if let Some(preview) = prepared.preview {
        // Keep the stage on screen when it already shows this skin.
        let current = ui.skin_preview.with_untracked(|p| {
            p.as_ref().is_some_and(|p| {
                p.id == preview.id
                    && p.language == preview.language
                    && Arc::ptr_eq(&p.source, &preview.source)
            })
        });
        if !current {
            ui.skin_preview.set(Some(Arc::new(preview)));
        }
    }
    if prepared.errors.is_empty() {
        Ok(())
    } else {
        Err(prepared.errors)
    }
}

/// Prepare skin `id`'s preview on this thread; see [`PreviewJob`].
pub fn prepare_skin_preview(
    ui: Ui,
    registry: &nanatype_theme::SkinRegistry,
    id: &str,
    play: bool,
) -> Result<(), Vec<nanatype_theme::SkinDiagnostic>> {
    apply_skin_preview(ui, PreviewJob::new(ui, id, play).run(registry))
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
