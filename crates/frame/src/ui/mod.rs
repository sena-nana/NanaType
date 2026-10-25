//! Views both products draw. Every skin surface reads the same [`Stage`]
//! signals, so the input preview, the typewriter and the history list always
//! agree on how a line looks.

pub mod avatar;
pub mod controls;
pub mod dialogue;
pub mod editor_view;
mod effect_uniforms;
mod effects;
pub mod feedback;
mod handwriting;
pub mod history;
pub mod kit;
pub mod materials;
pub mod motion;
pub mod overflow;
pub mod pipeline;
pub mod plugin_view;
pub mod preview;
pub mod quote;
pub mod rich;
pub mod shader_effects;
mod shader_guard;
pub mod surface;
pub mod sweep;
pub mod text_light;
pub mod text_scramble;

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use nana_ui::runtime::view::{Computed, NodeRef, Signal, computed, node_ref, signal};
use nanatype_core::{DisplayView, HistoryRow};
use nanatype_i18n::{Arg, I18n, Locale};
use nanatype_model::{Document, Entrance, EntranceKind, LineId};
use nanatype_settings::Settings;
use nanatype_theme::Skin;

use crate::canvas::Canvas;

/// Every string of the product this build is (see `nanatype-i18n`'s features).
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

/// The editor's markup source and selection.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Source {
    pub text: String,
    /// Byte offsets.
    pub selection: (usize, usize),
    /// Bumps when the program (not the user) rewrites the text.
    pub revision: u64,
}

/// What the skin renderer reads. `Copy`: views capture it freely.
///
/// A product keeps one for its windows; the skin preview builds a private one
/// per sample, so a preview never writes the live draft, history or display.
#[derive(Clone, Copy)]
pub struct Stage {
    pub speaker: Signal<nanatype_model::Speaker>,
    pub locale: Signal<Locale>,
    pub settings: Signal<Arc<Settings>>,
    /// Unsaved personalization fields of the skin previews, by skin id.
    pub skin_drafts: Signal<HashMap<String, preview::Draft>>,
    pub history_width: Signal<f32>,
    pub history_height: Signal<f32>,
    pub skin: Signal<Arc<Skin>>,
    /// Resolved application appearance, independent of the selected skin.
    pub chrome_dark: Signal<bool>,
    pub display: Signal<Arc<DisplayView>>,
    /// A private display snapshot used by isolated skin previews. It keeps
    /// the preview on the same dialogue renderer without touching the live
    /// typewriter state.
    pub preview_display: Signal<Arc<DisplayView>>,
    /// Speaker label used by the isolated display preview.
    pub preview_name: Signal<String>,
    pub peer_display: Signal<Arc<DisplayView>>,
    /// A remote participant's track is shown (it hides their latest line
    /// from the history like the local one).
    pub peer_active: Computed<bool>,
    pub peer_text_node: NodeRef,
    pub peer_text_port: NodeRef,
    pub peer_text_scroll: Signal<f32>,
    pub peer_display_size: Signal<[f32; 2]>,
    pub peer_text_area_height: Signal<f32>,
    pub peer_portraits: Signal<Arc<HashMap<String, String>>>,
    pub peer_local_line: Signal<Option<LineId>>,
    /// Stable slot for per-track animation and layout caches.
    pub track_slot: usize,
    /// Animation clock (ms); advanced every frame while something moves.
    pub now: Signal<u64>,
    /// History-sample playback: messages revealed, and the wall time of the
    /// latest step. A zero time means playback has not started.
    pub preview_playback: Signal<(u64, u64)>,
    pub peer_now: Signal<u64>,
    pub history_now: Signal<u64>,
    pub history: Signal<Arc<Vec<HistoryRow>>>,
    pub history_index: Computed<Arc<HashMap<u64, usize>>>,
    /// The draft being typed, and the editor around it.
    pub draft: Signal<Arc<Document>>,
    pub source: Signal<Source>,
    /// The rich editor's document, as the program last set or received it.
    pub editor_value: Signal<nana_ui_core::RichText>,
    /// Suppress the native text-only intermediate of a rich-fragment insertion.
    pub editor_transaction: Signal<bool>,
    /// An IME composition is open in the draft; its preedit is not in `draft`.
    pub composing: Signal<bool>,
    /// Attributes at the editor's selection (toolbar pressed states).
    pub sel_attrs: Signal<nana_ui::runtime::RichSelectionAttrs>,
    /// The editor shows markup source instead of the rich text.
    pub source_mode: Signal<bool>,
    /// What the markup source offers to complete at its caret.
    pub source_completions: Signal<Arc<[nana_ui::runtime::TextCompletion]>>,
    /// Asset reference → `file://` URL, for documents' atoms.
    pub asset_urls: Signal<Arc<HashMap<String, String>>>,
    /// Portrait variant → `file://` URL for the active skin.
    pub portraits: Signal<Arc<HashMap<String, String>>>,
    /// Motion is reduced: the setting, or the system preference.
    pub reduced_motion: Signal<bool>,
    pub shader_preview: Signal<bool>,
    /// Skin previews are on screen, so their effects may animate. A product
    /// derives it from what its windows show.
    pub live_preview: Computed<bool>,
    /// Advances when background layout materials finish (stroke-time maps).
    pub layout_epoch: Signal<u64>,
    /// The typewriter's text node and the box it is clipped to, read back by
    /// the host to follow long text.
    pub text_node: NodeRef,
    pub text_port: NodeRef,
    /// How far the typewriter's text is scrolled up (logical px).
    pub text_scroll: Signal<f32>,
    pub display_size: Signal<[f32; 2]>,
    pub text_area_height: Signal<f32>,
    /// Views the running plugins mount on the display and history.
    pub plugin_views: Signal<Arc<HashMap<String, nanatype_plugins::PluginUpdate>>>,
}

impl Stage {
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
        let preview_name = settings.general.speaker_name.clone();
        Self {
            speaker: signal(speaker),
            chrome_dark: signal(crate::skin_theme::dark_mode(settings.skin.chrome, false)),
            locale: signal(locale),
            settings: signal(settings),
            skin_drafts: signal(HashMap::new()),
            history_width: signal(0.0),
            history_height: signal(0.0),
            skin: signal(skin),
            display: signal(Arc::new(DisplayView::empty(entrance))),
            preview_display: signal(Arc::new(DisplayView::empty(entrance))),
            preview_name: signal(preview_name),
            peer_display: signal(Arc::new(DisplayView::empty(entrance))),
            peer_active: computed(|| false),
            peer_text_node: node_ref(),
            peer_text_port: node_ref(),
            peer_text_scroll: signal(0.0),
            peer_display_size: signal([0.0; 2]),
            peer_text_area_height: signal(0.),
            peer_portraits: signal(Arc::new(HashMap::new())),
            peer_local_line: signal(None),
            track_slot: 0,
            now: signal(crate::clock::now_ms()),
            preview_playback: signal((0, 0)),
            peer_now: signal(crate::clock::now_ms()),
            history_now: signal(crate::clock::now_ms()),
            history,
            history_index,
            draft: signal(Arc::new(Document::new())),
            source: signal(Source::default()),
            editor_value: signal(nana_ui_core::RichText::new("")),
            editor_transaction: signal(false),
            composing: signal(false),
            sel_attrs: signal(nana_ui::runtime::RichSelectionAttrs::default()),
            source_mode: signal(false),
            source_completions: signal(Arc::from([])),
            asset_urls: signal(Arc::new(HashMap::new())),
            portraits: signal(Arc::new(HashMap::new())),
            reduced_motion: signal(false),
            shader_preview: signal(false),
            live_preview: computed(|| false),
            layout_epoch: signal(0),
            text_node: node_ref(),
            text_port: node_ref(),
            text_scroll: signal(0.0),
            display_size: signal([0.0; 2]),
            text_area_height: signal(0.),
            plugin_views: signal(Arc::new(HashMap::new())),
        }
    }

    /// The dialogue canvas this track draws.
    pub fn canvas(self) -> Canvas {
        if self.track_slot == 1 {
            Canvas::PeerDisplay
        } else {
            Canvas::Display
        }
    }

    /// The remote participant's track: their display, clock and text nodes.
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

/// Rewrite a bound stack's layout in place.
pub fn restyle(s: &mut nana_ui::runtime::Stack, f: impl FnOnce(&mut nana_ui_core::LayoutStyle)) {
    f(s.layout_mut());
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
