//! Views. Every window reads the same [`Ui`] signals, so the input preview,
//! the typewriter and the history list always agree on how a line looks.

pub mod bilibili;
pub mod danmaku;
pub mod dialogue;
pub mod douyin;
mod effect_uniforms;
mod effects;
pub mod feedback;
mod handwriting;
pub mod history;
pub mod input;
pub mod keycap;
pub mod link_text;
pub mod materials;
pub mod motion;
pub mod overflow;
pub mod p2p;
pub mod package_editor;
pub mod pipeline;
pub mod plugins;
pub mod quote;
pub mod rich;
pub mod settings;
pub mod shader_effects;
pub mod surface;
pub mod sweep;
pub mod text_light;
pub mod text_scramble;

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, Weak};

use nana_ui::runtime::view::{Computed, NodeRef, Signal, computed, node_ref, signal};
use nanatype_core::{DisplayView, HistoryRow};
use nanatype_i18n::{Arg, I18n, Locale};
use nanatype_model::{ChatRef, Document, Entrance, EntranceKind};
use nanatype_settings::Settings;
use nanatype_theme::Skin;

use crate::msg::GlobalAction;
use crate::services::hotkeys::HotkeyState;

pub static I18N: LazyLock<I18n> = LazyLock::new(|| I18n::new(Locale::ZhCn));

/// The mono type role: text typed literally (markup source, ports, OSC
/// addresses, hex colours), so `[b]`, `0` and `O` read as written.
pub const MONO: &str = "Cascadia Mono, Consolas, JetBrains Mono, monospace";

/// Tabular figures (`tnum`), for numbers that change in place (fields,
/// slider readouts, counts and times) so the digits do not jitter.
pub fn tnum(l: &mut nana_ui_core::LayoutStyle) {
    l.font_features = Some(vec![nana_ui_core::FontFeatureSetting::new(*b"tnum", 1)]);
}

/// [`MONO`] with tabular figures, for a field holding a literal value.
pub fn mono(l: &mut nana_ui_core::LayoutStyle) {
    l.font_family = Some(MONO.into());
    tnum(l);
}

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

/// The editor's markup source and selection.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Source {
    pub text: String,
    /// Byte offsets.
    pub selection: (usize, usize),
    /// Bumps when the program (not the user) rewrites the text.
    pub revision: u64,
}

/// Shared reactive state. `Copy`: views capture it freely.
#[derive(Clone, Copy)]
pub struct Ui {
    pub packages: Signal<Arc<Vec<crate::plugins::PackageRow>>>,
    pub plugin_views: Signal<Arc<HashMap<String, nanatype_plugins::PluginUpdate>>>,
    pub plugin_errors: Signal<Arc<HashMap<String, String>>>,
    pub plugin_activation: Signal<Option<Arc<crate::plugins::ActivationReview>>>,
    pub plugin_source_trusted: Signal<bool>,
    /// The open package editor project, if any.
    pub package_editor: Signal<Option<Arc<crate::package_editor::EditorView>>>,
    pub package_editor_tab: Signal<crate::package_editor::Tab>,
    pub package_drafts: Signal<Arc<Vec<nanatype_theme::editor::DraftInfo>>>,
    pub bilibili: Signal<Arc<crate::app::bilibili::State>>,
    pub douyin: Signal<Arc<crate::app::douyin::State>>,
    /// Kept only in memory and cleared after import; never serialized with settings.
    pub douyin_cookie: Signal<String>,
    pub speaker: Signal<nanatype_model::Speaker>,
    pub locale: Signal<Locale>,
    pub settings: Signal<Arc<Settings>>,
    pub skin_drafts: Signal<HashMap<String, settings::preview::Draft>>,
    pub skin_preview_history: Signal<bool>,
    pub history_width: Signal<f32>,
    pub history_height: Signal<f32>,
    pub skin: Signal<Arc<Skin>>,
    /// Resolved application appearance, independent of the selected skin.
    pub chrome_dark: Signal<bool>,
    /// Actual input client width, published by its layout (also in headless views).
    pub input_width: Signal<f32>,
    pub(crate) input_sidebar_width: Signal<f32>,
    pub(crate) input_dock_regions: Signal<Vec<nanatype_settings::InputDockRegion>>,
    pub(crate) input_dock_selected: Signal<u64>,
    pub(crate) input_split: NodeRef,
    pub display: Signal<Arc<DisplayView>>,
    pub peer_display: Signal<Arc<DisplayView>>,
    pub peer: Signal<Arc<crate::p2p::SessionUi>>,
    pub peer_panel: Signal<bool>,
    pub peer_text_node: NodeRef,
    pub peer_text_port: NodeRef,
    pub peer_text_scroll: Signal<f32>,
    pub peer_display_size: Signal<[f32; 2]>,
    pub peer_text_area_height: Signal<f32>,
    pub peer_portraits: Signal<Arc<HashMap<String, String>>>,
    pub peer_local_line: Signal<Option<nanatype_model::LineId>>,
    /// Stable slot for per-track animation and layout caches.
    pub track_slot: usize,
    /// Animation clock (ms); advanced every frame while something moves.
    pub now: Signal<u64>,
    pub peer_now: Signal<u64>,
    pub history_now: Signal<u64>,
    pub history: Signal<Arc<Vec<HistoryRow>>>,
    pub history_index: Computed<Arc<HashMap<u64, usize>>>,
    pub draft: Signal<Arc<Document>>,
    pub source: Signal<Source>,
    pub diagnostics: Signal<Arc<Vec<String>>>,
    pub page: Signal<settings::Page>,
    pub status: Signal<Arc<Status>>,
    pub skins: Signal<Arc<Vec<SkinRow>>>,
    pub stickers: Signal<Arc<Vec<StickerRow>>>,
    pub sticker_packs: Signal<Arc<Vec<crate::stickers::PackRow>>>,
    pub telegram: Signal<Arc<crate::stickers::telegram::State>>,
    pub recording: Signal<Option<GlobalAction>>,
    pub hotkeys: Signal<Arc<HashMap<GlobalAction, HotkeyState>>>,
    /// Asset reference → `file://` URL, for documents' atoms.
    pub asset_urls: Signal<Arc<HashMap<String, String>>>,
    /// Portrait variant → `file://` URL for the active skin.
    pub portraits: Signal<Arc<HashMap<String, String>>>,
    /// Motion is reduced: the setting, or the system preference.
    pub reduced_motion: Signal<bool>,
    pub shader_preview: Signal<bool>,
    /// Advances when background layout materials finish (stroke-time maps).
    pub layout_epoch: Signal<u64>,
    /// The system's own reduce-motion preference, as last reported.
    pub system_reduced_motion: Signal<bool>,
    /// Whether the input window is shown by the application's presence policy.
    pub input_shown: Signal<bool>,
    /// The first-run guide's step, 0-based; kept while the user visits other
    /// pages (its "Connect OBS" goes to Output).
    pub onboard_step: Signal<u8>,
    /// Clock (ms) of the input window's mirroring chip pulse; only advanced
    /// while that chip is showing.
    pub pulse_ms: Signal<u64>,
    pub settings_pulse_ms: Signal<u64>,
    /// The rich editor's document, as the program last set or received it.
    pub editor_value: Signal<nana_ui_core::RichText>,
    /// Suppress the native text-only intermediate of a rich-fragment insertion.
    pub editor_transaction: Signal<bool>,
    /// Attributes at the editor's selection (toolbar pressed states).
    pub sel_attrs: Signal<nana_ui::runtime::RichSelectionAttrs>,
    /// The editor shows markup source instead of the rich text.
    pub source_mode: Signal<bool>,
    /// The typewriter's text node and the box it is clipped to, read back by
    /// the host to follow long text.
    pub text_node: NodeRef,
    pub text_port: NodeRef,
    /// How far the typewriter's text is scrolled up (logical px).
    pub text_scroll: Signal<f32>,
    pub display_size: Signal<[f32; 2]>,
    pub text_area_height: Signal<f32>,
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

impl Ui {
    pub fn new(settings: Arc<Settings>, skin: Arc<Skin>, locale: Locale) -> Self {
        let entrance = Entrance {
            kind: EntranceKind::None,
            ms: 0,
        };
        let history = signal(Arc::new(Vec::<HistoryRow>::new()));
        let history_index = computed(move || {
            Arc::new(
                history.with(|rows| rows.iter().enumerate().map(|(i, r)| (r.key, i)).collect()),
            )
        });
        let speaker = nanatype_model::Speaker {
            name: settings.general.speaker_name.clone(),
            avatar: settings.general.avatar.clone(),
            portrait: None,
        };
        let input_dock = nanatype_settings::InputDockLayout::default();
        Self {
            packages: signal(Arc::new(Vec::new())),
            plugin_views: signal(Arc::new(HashMap::new())),
            plugin_errors: signal(Arc::new(HashMap::new())),
            plugin_activation: signal(None),
            plugin_source_trusted: signal(false),
            package_editor: signal(None),
            package_editor_tab: signal(Default::default()),
            package_drafts: signal(Arc::new(Vec::new())),
            bilibili: signal(Arc::new(crate::app::bilibili::State::default())),
            douyin: signal(Arc::new(crate::app::douyin::State::default())),
            douyin_cookie: signal(String::new()),
            speaker: signal(speaker),
            chrome_dark: signal(crate::skin_theme::dark_mode(settings.skin.chrome, false)),
            input_width: signal(crate::windows::INPUT_SIZE.0),
            input_sidebar_width: signal(nanatype_settings::INPUT_SIDEBAR_DEFAULT),
            input_dock_regions: signal(input_dock.regions),
            input_dock_selected: signal(input_dock.selected),
            input_split: node_ref(),
            locale: signal(locale),
            settings: signal(settings),
            skin_drafts: signal(HashMap::new()),
            skin_preview_history: signal(false),
            history_width: signal(0.0),
            history_height: signal(0.0),
            skin: signal(skin),
            display: signal(Arc::new(DisplayView::empty(entrance))),
            peer_display: signal(Arc::new(DisplayView::empty(entrance))),
            peer: signal(Arc::new(crate::p2p::SessionUi::default())),
            peer_panel: signal(true),
            peer_text_node: node_ref(),
            peer_text_port: node_ref(),
            peer_text_scroll: signal(0.0),
            peer_display_size: signal([0.0; 2]),
            peer_text_area_height: signal(0.),
            peer_portraits: signal(Arc::new(HashMap::new())),
            peer_local_line: signal(None),
            track_slot: 0,
            now: signal(crate::clock::now_ms()),
            peer_now: signal(crate::clock::now_ms()),
            history_now: signal(crate::clock::now_ms()),
            history,
            history_index,
            draft: signal(Arc::new(Document::new())),
            source: signal(Source::default()),
            diagnostics: signal(Arc::new(Vec::new())),
            page: signal(settings::Page::General),
            status: signal(Arc::new(Status::default())),
            skins: signal(Arc::new(Vec::new())),
            stickers: signal(Arc::new(Vec::new())),
            sticker_packs: signal(Arc::new(Vec::new())),
            telegram: signal(Arc::new(Default::default())),
            recording: signal(None),
            hotkeys: signal(Arc::new(HashMap::new())),
            asset_urls: signal(Arc::new(HashMap::new())),
            portraits: signal(Arc::new(HashMap::new())),
            reduced_motion: signal(false),
            shader_preview: signal(false),
            layout_epoch: signal(0),
            system_reduced_motion: signal(false),
            input_shown: signal(true),
            onboard_step: signal(0),
            pulse_ms: signal(0),
            settings_pulse_ms: signal(0),
            editor_value: signal(nana_ui_core::RichText::new("")),
            editor_transaction: signal(false),
            sel_attrs: signal(nana_ui::runtime::RichSelectionAttrs::default()),
            source_mode: signal(false),
            text_node: node_ref(),
            text_port: node_ref(),
            text_scroll: signal(0.0),
            display_size: signal([0.0; 2]),
            text_area_height: signal(0.),
            chat: signal(Arc::new(Vec::new())),
            reply: signal(None),
            chat_width: signal(400.0),
            chat_target: signal(None),
            skins_grid: signal(true),
        }
    }

    /// A string that follows the language setting.
    pub fn display_kind(self) -> crate::windows::WindowKind {
        if self.track_slot == 1 {
            crate::windows::WindowKind::PeerDisplay
        } else {
            crate::windows::WindowKind::Display
        }
    }

    pub fn remote(self) -> Self {
        Self {
            display: self.peer_display,
            now: self.peer_now,
            text_node: self.peer_text_node,
            text_port: self.peer_text_port,
            text_scroll: self.peer_text_scroll,
            display_size: self.peer_display_size,
            text_area_height: self.peer_text_area_height,
            portraits: self.peer_portraits,
            track_slot: 1,
            ..self
        }
    }

    /// A string that follows the language setting.
    pub fn t(self, key: &'static str) -> impl Fn() -> String + Send + 'static {
        let locale = self.locale;
        move || I18N.t_in(locale.get(), key)
    }

    pub fn now_t(self, key: &str) -> String {
        I18N.t_in(self.locale.get_untracked(), key)
    }

    pub fn tr(self, key: &str, args: &[(&str, Arg<'_>)]) -> String {
        I18N.tr_in(self.locale.get(), key, args)
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
    if !shader_effects::enabled(ui)
        && !registry
            .entry(id)
            .and_then(|e| e.skin.as_ref())
            .is_some_and(|s| s.shaders.values().any(|s| s.static_fallback))
        && !(materials::enabled(ui)
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
        && shader_effects::enabled(ui)
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

/// Rewrite a bound stack's layout in place.
pub fn restyle(s: &mut nana_ui::runtime::Stack, f: impl FnOnce(&mut nana_ui_core::LayoutStyle)) {
    let taken = std::mem::replace(s, nana_ui::runtime::Stack::row(0.0));
    *s = taken.with_layout(f);
}

/// `[r, g, b, a]` in 0..=1 from a skin colour.
pub fn c(color: nanatype_model::Rgba8) -> [f32; 4] {
    crate::skin_theme::rgba(color)
}

/// Release GPU resources of every custom effect whose material is gone.
pub fn collect_effect_garbage() {
    shader_effects::collect_garbage();
    sweep::collect_garbage();
    text_light::collect_garbage();
    text_scramble::collect_garbage();
}
