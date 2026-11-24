//! The editor's window. Its preview draws through `nanatype-frame`'s skin
//! renderer from the [`Stage`] inside [`GlobalUi`] and [`ProjectUi`].

mod assets;
pub mod canvas_controls;
mod conflicts;
pub mod modes;
mod preview_panel;
mod properties;
mod rows;
pub mod shell;
pub mod views;
pub mod window;

pub use nanatype_frame::ui::{
    I18N, Stage, controls, feedback, kit, mono, plugin_view, preview, shader_effects, text_light,
    text_scramble,
};

use std::collections::{BTreeSet, HashMap};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU64, Ordering},
};

use nana_ui::runtime::view::{NodeRef, Signal, computed, node_ref, signal};
use nana_ui::runtime::{ScrollOffset, TextSelection};
use nanatype_i18n::Locale;
use nanatype_settings::Settings;
use nanatype_theme::Skin;

use crate::session::{EditorView, Mode};

static NEXT_PROJECT_UI: AtomicU64 = AtomicU64::new(1);

/// The panel under the workspace, above the status bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Drawer {
    #[default]
    Closed,
    Problems,
    Debug,
}

/// The entry the data workspace's form edits; `None` inside is a new one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataTarget {
    Data(Option<String>),
    Pipeline(Option<String>),
}

/// Window-lifetime reactive state shared across sessions.
#[derive(Clone, Copy)]
pub struct GlobalUi {
    pub stage: Stage,
    pub audio_gain: Signal<f32>,
    /// The editor's whole width: below [`shell::COMPACT`] the side panel folds.
    pub workspace_width: Signal<f32>,
    /// Side and inspector widths, kept across mode switches and sessions.
    pub side_width: Signal<f32>,
    pub inspector_width: Signal<f32>,
    pub package_drafts: Signal<Arc<Vec<nanatype_theme::editor::DraftInfo>>>,
    /// The packages installed in NanaType, to edit or copy.
    pub packages: Signal<Arc<Vec<nanatype_frame::packages::PackageRow>>>,
    /// Where the installed packages, drafts and signing key live.
    pub data_dir: Signal<std::path::PathBuf>,
    pub recoveries: Signal<Arc<Vec<nanatype_theme::editor::DraftInfo>>>,
}

/// Cached `(selection, scroll_offset)` state keyed by file path.
pub type SourceCursors = Arc<Mutex<HashMap<String, (TextSelection, ScrollOffset)>>>;

/// Per-session reactive state. Dropped and recreated atomically on every
/// `Open` / `Close` transition so closed projects never leak state into new ones.
#[derive(Clone, Copy)]
pub struct ProjectUi {
    pub id: u64,
    /// The open project, if any.
    pub package_editor: Signal<Option<Arc<EditorView>>>,
    /// The activity rail's choice; see [`Mode::effective`].
    pub mode: Signal<Mode>,
    /// The preview's window.
    pub package_editor_surface: Signal<preview::Surface>,
    pub properties_file: Signal<String>,
    pub properties_path: Signal<Vec<String>>,
    pub file_target: Signal<String>,
    pub debug_custom: Signal<String>,
    pub debug_sequence: Signal<String>,
    pub debug_filter: Signal<String>,
    /// Plain non-reactive `(selection, scroll_offset)` cache per file path.
    pub source_cursors: Signal<SourceCursors>,
    pub source_node: NodeRef,
    pub search_query: Signal<String>,
    pub search_index: Signal<usize>,
    pub search_replacement: Signal<String>,
    /// Find and replace shows over the source.
    pub find_open: Signal<bool>,
    pub search_field: NodeRef,
    pub canvas_enabled: Signal<bool>,
    pub canvas_focus: Signal<bool>,
    pub canvas_geometry: Signal<Vec<nanatype_frame::ui::editor_geometry::Part>>,
    pub canvas_selection: Signal<Option<String>>,
    pub canvas_target: Signal<Vec<String>>,
    pub canvas_preview: Signal<Option<Arc<Skin>>>,
    pub asset: Signal<Option<Arc<crate::asset::Preview>>>,
    pub preview_host: Stage,
    pub preview_simulate: Signal<bool>,
    pub preview_size: Signal<[String; 2]>,
    pub preview_text: Signal<String>,
    pub preview_speaker: Signal<String>,
    pub preview_mode: Signal<u8>,
    pub preview_viewport: Signal<preview::Viewport>,
    pub drawer: Signal<Drawer>,
    /// The design canvas shares the centre with the selected source.
    pub split_source: Signal<bool>,
    /// The side panel, shown beside the centre (wide) or over it (compact).
    pub side_open: Signal<bool>,
    /// The files workspace keeps a live preview on its right.
    pub files_preview: Signal<bool>,
    pub side_split: NodeRef,
    pub inspector_split: NodeRef,
    /// The files with a tab in the source pane, in the order they were opened.
    pub open_files: Signal<Vec<String>>,
    /// The file each workspace last showed, to come back to.
    pub mode_files: Signal<Vec<(Mode, String)>>,
    /// The files the next seal protects, beside `package.toml`.
    pub protect: Signal<BTreeSet<String>>,
    /// The project menu's "copy as a new package" asks for the new ID.
    pub derive_open: Signal<bool>,
    /// What the data workspace's centre shows.
    pub data_target: Signal<Option<DataTarget>>,
}

impl ProjectUi {
    pub fn new(global: &GlobalUi) -> Self {
        let stage = global.stage;
        let editor_mode = signal(Mode::default());
        let mode = signal(0_u8);
        let mut host = Stage::new(
            Arc::new(Settings::default()),
            stage.skin.get_untracked(),
            stage.locale.get_untracked(),
        );
        host.preview_options = nanatype_frame::ui::PreviewOptions {
            custom_speaker_name: true,
            show_portrait: true,
            geometry_capture: Some(signal(Default::default())),
        };
        host.locale = stage.locale;
        host.now = stage.now;
        host.chrome_dark = stage.chrome_dark;
        host.layout_epoch = stage.layout_epoch;
        host.live_preview = computed(move || {
            // Only the workspaces that draw the stage animate it.
            mode.get() == 1
                && matches!(editor_mode.get(), Mode::Design | Mode::Files | Mode::Sounds)
        });
        Self {
            id: NEXT_PROJECT_UI.fetch_add(1, Ordering::Relaxed),
            package_editor: signal(None),
            mode: editor_mode,
            package_editor_surface: signal(Default::default()),
            properties_file: signal("skin.toml".into()),
            properties_path: signal(Vec::new()),
            file_target: signal(String::new()),
            debug_custom: signal("{\"type\":\"command\",\"id\":\"reset\"}".into()),
            debug_sequence: signal(
                "[{\"type\":\"line_start\",\"id\":1,\"text\":\"Hello\"},{\"type\":\"line_end\",\"id\":1}]"
                    .into(),
            ),
            debug_filter: signal(String::new()),
            source_cursors: signal(Arc::new(Mutex::new(HashMap::new()))),
            source_node: node_ref(),
            search_query: signal(String::new()),
            search_index: signal(0),
            search_replacement: signal(String::new()),
            find_open: signal(false),
            search_field: node_ref(),
            canvas_enabled: signal(false),
            canvas_focus: signal(false),
            canvas_geometry: signal(Vec::new()),
            canvas_selection: signal(None),
            canvas_target: signal(vec!["dialogue".into()]),
            canvas_preview: signal(None),
            asset: signal(None),
            preview_host: host,
            preview_simulate: signal(false),
            preview_size: signal([String::new(), String::new()]),
            preview_text: signal(stage.now_t("skin-edit-sample")),
            preview_speaker: signal(stage.now_t("skins-preview-name")),
            preview_mode: mode,
            preview_viewport: signal(Default::default()),
            drawer: signal(Drawer::Closed),
            split_source: signal(false),
            side_open: signal(true),
            files_preview: signal(true),
            side_split: node_ref(),
            inspector_split: node_ref(),
            open_files: signal(Vec::new()),
            mode_files: signal(Vec::new()),
            protect: signal(BTreeSet::new()),
            derive_open: signal(false),
            data_target: signal(None),
        }
    }
}

/// Shared reactive state: window-lifetime [`GlobalUi`] plus an atomically
/// replaced per-session [`ProjectUi`]. `Copy`: views capture it freely.
#[derive(Clone, Copy)]
pub struct Ui {
    pub global: GlobalUi,
    pub project: Signal<ProjectUi>,
}

impl std::ops::Deref for Ui {
    type Target = Stage;
    fn deref(&self) -> &Stage {
        &self.global.stage
    }
}

impl Ui {
    pub fn new(settings: Arc<Settings>, skin: Arc<Skin>, locale: Locale) -> Self {
        let mut stage = Stage::new(settings, skin, locale);
        stage.live_preview = computed(|| false);
        let global = GlobalUi {
            stage,
            audio_gain: signal(0.5),
            workspace_width: signal(1440.),
            side_width: signal(shell::SIDE_W),
            inspector_width: signal(shell::INSPECTOR_W),
            package_drafts: signal(Arc::new(Vec::new())),
            packages: signal(Arc::new(Vec::new())),
            data_dir: signal(Default::default()),
            recoveries: signal(Arc::new(Vec::new())),
        };
        let project = signal(ProjectUi::new(&global));
        Self { global, project }
    }

    pub fn proj(&self) -> ProjectUi {
        self.project.get_untracked()
    }

    /// Replace the per-session UI state atomically so no signal leaks across
    /// closed or newly opened projects.
    pub fn reset_project(&self) -> ProjectUi {
        let next = ProjectUi::new(&self.global);
        self.project.set(next);
        next
    }
}
