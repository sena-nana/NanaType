//! The sounds workspace: the package's audio files to audition, and the
//! sound declarations in the inspector beside them.
use crate::ui::Ui;
use crate::ui::kit::gap;
use crate::ui::shell::{panel_header, scroll};
use nana_ui::runtime::Stack;
use nana_ui::runtime::view::{AnyView, IntoView, widget};
use nanatype_theme::editor::{AssetKind, FileInfo};

fn is_audio(file: &FileInfo) -> bool {
    file.kind == AssetKind::Audio
}

pub fn side(ui: Ui) -> AnyView {
    widget(Stack::fill_column(0.))
        .children((
            panel_header(ui.now_t("pkgedit-sound-files"), ()),
            scroll(
                widget(Stack::column(0.).padding_xy(gap::TIGHT, 0.))
                    .children(super::files::file_tree(
                        ui,
                        is_audio,
                        "package-editor-sound-tree",
                    ))
                    .into_any(),
            ),
        ))
        .into_any()
}

/// The selected sound's player above the declarations.
pub fn center(ui: Ui) -> AnyView {
    widget(Stack::fill_column(0.))
        .key("package-editor-sounds")
        .children((
            widget(Stack::column(0.).padding_xy(gap::CONTROL, gap::TIGHT))
                .children(crate::ui::assets::preview(ui)),
            crate::ui::kit::rule(),
            panel_header(ui.now_t("pkgedit-sound-declarations"), ()),
            scroll(crate::ui::properties::inspector(ui)),
        ))
        .into_any()
}
