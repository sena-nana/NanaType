//! The editor's window. Its preview draws through `nanatype-frame`'s skin
//! renderer from the [`Stage`] inside [`Ui`].

mod assets;
pub mod canvas_controls;
mod conflicts;
mod preview_panel;
mod properties;
pub mod views;
pub mod window;

pub use nanatype_frame::ui::{
    I18N, Stage, controls, feedback, kit, mono, plugin_view, preview, shader_effects, text_light,
    text_scramble,
};

use std::sync::Arc;

use nana_ui::runtime::view::{Signal, computed, signal};
use nanatype_i18n::Locale;
use nanatype_settings::Settings;
use nanatype_theme::Skin;

use crate::session::{EditorView, Tab};

type PostAction = Arc<dyn Fn(crate::session::Action) + Send + Sync>;

/// Shared reactive state. `Copy`: views capture it freely.
#[derive(Clone, Copy)]
pub struct Ui {
    pub properties_file: Signal<String>,
    pub properties_path: Signal<Vec<String>>,
    pub file_target: Signal<String>,
    pub debug_custom: Signal<String>,
    pub debug_sequence: Signal<String>,
    pub debug_filter: Signal<String>,
    pub source_views: Signal<std::collections::BTreeMap<String, nana_ui::runtime::TextArea>>,
    pub source_node: nana_ui::runtime::view::NodeRef,
    pub search_query: Signal<String>,
    pub search_index: Signal<usize>,
    pub search_replacement: Signal<String>,
    pub canvas_enabled: Signal<bool>,
    pub canvas_focus: Signal<bool>,
    pub canvas_geometry: Signal<Vec<nanatype_frame::ui::editor_geometry::Part>>,
    pub canvas_selection: Signal<Option<String>>,
    pub canvas_target: Signal<Vec<String>>,
    pub canvas_preview: Signal<Option<Arc<Skin>>>,
    pub asset: Signal<Option<Arc<crate::asset::Preview>>>,
    pub audio_gain: Signal<f32>,
    pub stage: Stage,
    pub preview_host: Stage,
    pub preview_simulate: Signal<bool>,
    pub preview_size: Signal<[String; 2]>,
    pub preview_text: Signal<String>,
    pub preview_speaker: Signal<String>,
    pub preview_mode: Signal<u8>,
    pub preview_viewport: Signal<preview::Viewport>,
    pub workspace_width: Signal<f32>,
    pub post: Signal<Option<PostAction>>,
    /// The open project, if any.
    pub package_editor: Signal<Option<Arc<EditorView>>>,
    pub package_editor_tab: Signal<Tab>,
    /// The preview's window.
    pub package_editor_surface: Signal<preview::Surface>,
    pub package_drafts: Signal<Arc<Vec<nanatype_theme::editor::DraftInfo>>>,
    /// The packages installed in NanaType, to edit or copy.
    pub packages: Signal<Arc<Vec<nanatype_frame::packages::PackageRow>>>,
    /// Where the installed packages, drafts and signing key live.
    pub data_dir: Signal<std::path::PathBuf>,
    pub recoveries: Signal<Arc<Vec<nanatype_theme::editor::DraftInfo>>>,
}

impl std::ops::Deref for Ui {
    type Target = Stage;
    fn deref(&self) -> &Stage {
        &self.stage
    }
}

impl Ui {
    pub fn new(settings: Arc<Settings>, skin: Arc<Skin>, locale: Locale) -> Self {
        let mut stage = Stage::new(settings, skin, locale);
        // The preview is the whole point of this window: let it animate.
        stage.live_preview = computed(|| false);
        let tab = signal(Tab::default());
        let workspace_width = signal(900.);
        let mode = signal(0_u8);
        let mut host = Stage::new(
            Arc::new(Settings::default()),
            stage.skin.get_untracked(),
            locale,
        );
        host.editor_nodes = Some(signal(Default::default()));
        host.locale = stage.locale;
        host.now = stage.now;
        host.chrome_dark = stage.chrome_dark;
        host.layout_epoch = stage.layout_epoch;
        host.live_preview = computed(move || {
            mode.get() == 1 && (tab.get() == Tab::Preview || workspace_width.get() >= 1100.)
        });
        Self {
            properties_file:signal("skin.toml".into()),properties_path:signal(Vec::new()),file_target:signal(String::new()),
            debug_custom:signal("{\"type\":\"command\",\"id\":\"reset\"}".into()),debug_sequence:signal("[{\"type\":\"line_start\",\"id\":1,\"text\":\"Hello\"},{\"type\":\"line_end\",\"id\":1}]".into()),debug_filter:signal(String::new()),
            source_views:signal(Default::default()),source_node:nana_ui::runtime::view::node_ref(),search_query:signal(String::new()),search_index:signal(0),search_replacement:signal(String::new()),
            canvas_enabled:signal(false),canvas_focus:signal(false),canvas_geometry:signal(Vec::new()),canvas_selection:signal(None),canvas_target:signal(vec!["dialogue".into()]),canvas_preview:signal(None),
            asset: signal(None), audio_gain: signal(0.5),
            preview_host: host, preview_simulate: signal(false), preview_size: signal([String::new(), String::new()]),
            preview_text: signal(stage.now_t("skin-edit-sample")), preview_speaker: signal(stage.now_t("skins-preview-name")), preview_mode: mode,
            preview_viewport: signal(Default::default()), workspace_width,
            stage,
            post: signal(None),
            package_editor: signal(None),
            package_editor_tab: tab,
            package_editor_surface: signal(Default::default()),
            package_drafts: signal(Arc::new(Vec::new())),
            packages: signal(Arc::new(Vec::new())),
            data_dir: signal(Default::default()),
            recoveries: signal(Arc::new(Vec::new())),
        }
    }
}
