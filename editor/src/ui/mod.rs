//! The editor's window. Its preview draws through `nanatype-frame`'s skin
//! renderer from the [`Stage`] inside [`Ui`].

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

/// Shared reactive state. `Copy`: views capture it freely.
#[derive(Clone, Copy)]
pub struct Ui {
    pub stage: Stage,
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
        stage.live_preview = computed(|| true);
        Self {
            stage,
            package_editor: signal(None),
            package_editor_tab: signal(Default::default()),
            package_editor_surface: signal(Default::default()),
            package_drafts: signal(Arc::new(Vec::new())),
            packages: signal(Arc::new(Vec::new())),
            data_dir: signal(Default::default()),
        }
    }
}
