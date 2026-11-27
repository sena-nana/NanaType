//! The input window: a full-area editor or an editable skin preview.
//! Both layouts retain the same rich/source editors and draft. Toolbar
//! buttons send semantic [`Tool`] actions; the source toggle edits markup.

use std::sync::Arc;

use nana_ui::ButtonKind;
use nana_ui::icons_tabler as ti;
use nana_ui::runtime::view::{
    AnyView, EntityRef, IntoView, Signal, WithRefs, dynamic, entity_ref, segmented_option, signal,
    widget, with_refs,
};
use nana_ui::runtime::{
    ActionMenu, ActionMenuItem, Activate, Button, ColorChanged, ColorField, Divider, IconButton,
    IconGlyph, NodeStyle, NumberChanged, NumberInput, Popover, PopoverClosed, PopoverToggled,
    RichAttrMask, RichSelectionAttrs, RichTextEditor, ScrollAxes, ScrollView,
    ScrollViewportChanged, SegmentedControl, SizeChanged, Stack, Text, TextArea, TextChanged,
    TextSubmitted,
};
use nana_ui::theme::space;
use nana_ui_core::{
    AlignSpec, JustifySpec, LengthSpec, OverflowSpec, PointerEventsSpec, PositionSpec,
    SemanticColorRole, SquareSize,
};
use nana_ui_core::{ControlSize, PopoverPlacement};
use nanatype_core::Cmd;
use nanatype_model::{
    AssetRef, Atom, AtomKind, Continuous, ContinuousKind, Emphasis, EntranceKind, Inline, Marker,
    Pace, Rgba8, Shadow, Size, Stroke,
};
use nanatype_settings::{InputKey, InputMode, Settings};

use super::dialogue::{Source, dialogue_box};
use super::rich::{ChipKind, MarkerLook};
use super::{I18N, Ui, c};
use crate::msg::{ChatMsg, EditorMsg, FileTarget, MarkupEdit, Msg, Tool};
use crate::ui::kit::{self, Tone, Type, gap, icon, inset};

mod cheat_sheet;
pub mod completion;
mod controls;
mod panels;
mod preview_stage;
mod stickers;
mod toolbar;
mod workspace;

use nanatype_frame::ui::editor_view::{self, EditorEvent};
pub use nanatype_frame::ui::editor_view::{marker_label, preview_editor};

pub use self::cheat_sheet::cheat_sheet;
pub use self::preview_stage::input_window;

use self::panels::{PICTURE_SWATCH, caption, image_button};
use self::preview_stage::status_content;
pub(super) use self::toolbar::{Tip, popover, pressed_tool};

fn act(tool: Tool) -> Msg {
    Msg::Editor(EditorMsg::Tool(tool))
}

/// The sidebar's sticker panel.
pub fn sticker_panel(ui: Ui) -> AnyView {
    stickers::panel(ui)
}

/// The sidebar's control palette.
pub fn controls_panel(ui: Ui) -> AnyView {
    controls::panel(ui)
}

/// Localized editor commands, in `TextEditAction::ALL` order.
pub fn editor_context_menu(
    ui: Ui,
    x: f32,
    y: f32,
    enabled: [bool; 6],
) -> nana_ui::runtime::ContextMenu {
    use nana_ui::runtime::{ContextMenu, ContextMenuItem, TextEditAction};
    let labels = [
        "edit-undo",
        "edit-redo",
        "edit-cut",
        "edit-copy",
        "edit-paste",
        "edit-select-all",
    ];
    let hints = if cfg!(target_os = "macos") {
        ["⌘Z", "⇧⌘Z", "⌘X", "⌘C", "⌘V", "⌘A"]
    } else {
        ["Ctrl+Z", "Ctrl+Y", "Ctrl+X", "Ctrl+C", "Ctrl+V", "Ctrl+A"]
    };
    ContextMenu::new(x, y).items(
        TextEditAction::ALL
            .into_iter()
            .enumerate()
            .map(|(i, action)| {
                ContextMenuItem::new(action.key(), ui.now_t(labels[i]))
                    .hint(hints[i])
                    .disabled(!enabled[i])
            }),
    )
}

/// Read the split model after native pointer/keyboard routing, before layout.
pub(crate) fn changed_sidebar_width(ui: Ui, cx: &nana_ui::runtime::AppContext) -> Option<f32> {
    workspace::changed_width(ui, cx)
}

/// The folded sidebar's width: its rail alone.
#[cfg(test)]
pub(crate) fn workspace_rail_width() -> f32 {
    workspace::RAIL_W
}

/// Read pinned chat's split the same way, after routing.
pub(crate) fn changed_chat_height(ui: Ui, cx: &nana_ui::runtime::AppContext) -> Option<f32> {
    workspace::changed_chat_height(ui, cx)
}

/// Mirror the editor's IME composition after routing: preedit never reaches
/// the draft, so the empty-draft hint needs this to step aside for it.
pub(crate) fn sync_composing(
    ui: Ui,
    cx: &nana_ui::runtime::AppContext,
    doc: nana_ui::runtime::DocumentId,
) {
    let composing = cx.editor_composing(doc);
    if composing {
        ui.draft_fence.with_untracked(|f| f.set_composing(true));
    }
    if ui.composing.get_untracked() != composing {
        ui.composing.set(composing);
    }
}

/// Selection-only navigation in NanaUI's plain TextArea emits no TextChanged.
/// Read its retained caret before a toolbar or another window inserts markup.
pub fn capture_source_selection(
    ui: Ui,
    cx: &nana_ui::runtime::AppContext,
    source: nana_ui::runtime::Entity<TextArea>,
) {
    if !ui.source_mode.get_untracked() {
        return;
    }
    let Some(state) = cx.world().text_input(source.stable_id()) else {
        return;
    };
    let mut current = ui.source.get_untracked();
    let selection = (state.selection.anchor, state.selection.focus);
    if current.text == state.value && current.selection != selection {
        current.selection = selection;
        ui.source.set(current);
    }
}

pub type EditorRef = EntityRef<RichTextEditor>;
pub type SourceRef = EntityRef<TextArea>;
pub type ToastsRef = EntityRef<nana_ui::runtime::ToastStack>;
