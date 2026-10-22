//! The `@` / `/` completion list of the rich editor, hung under the token
//! being completed as an IDE does: each row names only the word it fills
//! in, with what it does on the right. The host places it from the editor's
//! layout (`Host::anchor_completion`); the markup source shows the same rows
//! in its own completion popup.
//!
//! The editor keeps focus: its keys choose a row (see
//! `host::completion_key`), and a click fills one in.
use super::*;
use crate::completion::Trigger;
use crate::msg::Complete;
use crate::ui::kit::{card_frame, gap, icon, popover};
use nana_ui::runtime::{IconGlyph, ListItem};
use nana_ui_core::RadiusTier;

/// The list's width, and a row's height before its first layout, for
/// placing it.
pub const WIDTH: f32 = popover::XL;
pub const ROW_ESTIMATE: f32 = 28.0;
/// From the list's edge to a row's label: the frame's padding, the row's,
/// its icon and the gap after it. The list shifts left by it so labels
/// line up under the word they complete.
const LABEL_INSET: f32 = gap::TIGHT + space::MD + icon::MD + space::MD;

/// A placement of the list in the input window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchor {
    /// Left and top, in the window.
    pub at: [f32; 2],
    /// Read from a layout of the current text, with the list's own size:
    /// otherwise it moves once the next frame is laid out.
    pub settled: bool,
}

/// Where the list goes in the input window: its labels under the word it
/// completes, read from the rich editor's last layout; above the line when
/// there is no room below, and kept inside the window. None in source mode
/// (its own popup shows the rows) or before the editor is laid out.
pub fn anchor(
    ui: Ui,
    world: &nana_ui::runtime::UiWorld,
    editor: nana_ui::runtime::StableNodeId,
) -> Option<Anchor> {
    if ui.source_mode.get_untracked() {
        return None;
    }
    let (start, rows) = ui
        .completion
        .with_untracked(|c| c.as_ref().map(|v| (v.start, v.items.len())))?;
    let (_, layout) = world.text_layout(editor)?;
    // A layout from before the last keystroke ends short of the word.
    let laid_out = layout.lines.last().map_or(0, |l| l.source.end);
    let current = ui.editor_value.with_untracked(|v| v.text().len()) == laid_out;
    let (x, top, height) = nana_ui::runtime::rich_editor_caret_box(layout, start.min(laid_out))?;
    // The preview may scale the editor: map its own space onto the window.
    let local = world.layout_box(editor)?;
    let shown = world.presentation_input_bounds(editor)?;
    let scale = if local.width > 0.0 {
        shown.width / local.width
    } else {
        1.0
    };
    let scroll = world
        .scroll_offset(editor)
        .map_or((0.0, 0.0), |s| (s.x, s.y));
    let token_x = shown.x + (x - scroll.0) * scale;
    let line_top = shown.y + (top - scroll.1) * scale;
    let line_bottom = line_top + height * scale;
    // The list's own size once laid out; a row estimate before.
    let measured = ui
        .completion_node
        .get_untracked()
        .and_then(|n| world.layout_box(n))
        .filter(|b| b.height > 0.0);
    let (w, h) = measured.map_or((WIDTH, rows as f32 * ROW_ESTIMATE), |b| (b.width, b.height));
    let (win_w, win_h) = (
        ui.input_width.get_untracked(),
        ui.input_height.get_untracked(),
    );
    let left = (token_x - LABEL_INSET)
        .min(win_w - w - gap::TIGHT)
        .max(gap::TIGHT);
    let below = line_bottom + gap::TIGHT;
    let above = line_top - gap::TIGHT - h;
    let top = if below + h <= win_h - gap::TIGHT || above < gap::TIGHT {
        below
    } else {
        above
    };
    Some(Anchor {
        at: [left, top],
        settled: current && measured.is_some(),
    })
}

/// The markup source's own popup was closed (Esc reaches it before the
/// input window's keys) while the app's list is still open.
pub fn source_dismissed(
    ui: Ui,
    world: &nana_ui::runtime::UiWorld,
    source: nana_ui::runtime::StableNodeId,
) -> bool {
    ui.source_mode.get_untracked()
        && ui.completion.with_untracked(Option::is_some)
        && world
            .text_completion_snapshot(source)
            .is_some_and(|s| s.dismissed)
}

pub(super) fn popup(ui: Ui) -> AnyView {
    let rows = dynamic(
        move || {
            ui.completion.with(|c| {
                c.as_ref()
                    .map(|v| (v.trigger, Arc::as_ptr(&v.items) as *const () as usize))
            })
        },
        move |key| {
            let Some(view) = ui.completion.get_untracked() else {
                return Vec::new();
            };
            let glyph = match key.map(|(t, _)| t) {
                Some(Trigger::Mention) => ti::AT,
                _ => ti::SLASH,
            };
            view.items
                .iter()
                .enumerate()
                .map(|(i, item)| row(ui, i, item, glyph))
                .collect()
        },
    );
    widget(
        card_frame(RadiusTier::Md)
            .gap_step(gap::step::HAIR)
            .padding_step(gap::step::TIGHT, gap::step::TIGHT)
            .with_layout(|l| {
                l.position = PositionSpec::Absolute;
                l.width = Some(LengthSpec::Px(WIDTH));
            }),
    )
    .key("input-completion")
    .node_ref(ui.completion_node)
    .bind(move |s: &mut Stack| {
        let [left, top] = ui.completion_at.get().unwrap_or_default();
        let l = s.layout_mut();
        l.offset_left = Some(LengthSpec::Px(left));
        l.offset_top = Some(LengthSpec::Px(top));
    })
    .visible(move || {
        !ui.source_mode.get()
            && ui.completion_at.get().is_some()
            && ui
                .completion
                .with(|c| c.as_ref().is_some_and(|v| !v.items.is_empty()))
    })
    .children(rows)
    .into_any()
}

fn row(ui: Ui, i: usize, item: &crate::completion::Item, glyph: nana_ui::Icon) -> AnyView {
    let mut row = ListItem::new(item.label.clone()).size(ControlSize::Small);
    row.note = item.note.clone();
    row.detail = item.detail.clone();
    widget(row)
        .bind(move |item: &mut ListItem| {
            let selected = ui
                .completion
                .with(|c| c.as_ref().is_some_and(|v| v.selected == i));
            if item.selected != selected {
                item.selected = selected;
            }
        })
        .leading(
            widget(IconGlyph::new(glyph).size(icon::MD)).bind(|g: &mut IconGlyph| {
                g.style.foreground = Some(SemanticColorRole::Muted);
            }),
        )
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(Msg::Complete(Complete::Accept(Some(i))))
        })
        .into_any()
}
