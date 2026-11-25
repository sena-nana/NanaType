//! Viewport tracking: dialogue overflow scroll/page-breaks and split-pane/window
//! geometry persistence.

use std::collections::HashMap;

use nana_ui::runtime::RuntimeDocument;
use nana_ui_platform::{WindowGeometry, WindowId};
use nanatype_core::Cmd;
use nanatype_model::Overflow;

use crate::app::App;
use crate::msg::Msg;
use crate::ui::overflow::{self, Measure, PageKey};
use crate::ui::{Ui, input};
use crate::windows::WindowKind;

#[derive(Default)]
pub(super) struct ViewportSync {
    pub follow: overflow::Follow,
    pub pager: overflow::Pager,
}

impl ViewportSync {
    pub fn follow_display(
        &mut self,
        app: &mut App,
        docs: &HashMap<WindowId, RuntimeDocument>,
        now: u64,
    ) {
        apply_follow(app, docs, now, app.ui, &mut self.follow, &mut self.pager);
    }
}

fn apply_follow(
    app: &mut App,
    docs: &HashMap<WindowId, RuntimeDocument>,
    now: u64,
    ui: Ui,
    follower: &mut overflow::Follow,
    pager: &mut overflow::Pager,
) {
    let view = ui.display.get_untracked();
    if view.exit.and_then(|e| e.presentation()).is_some() {
        return;
    }
    let mut mode = app.appearance.skin.dialogue.overflow;
    let mirror = app.settings.general.input_mode == nanatype_settings::InputMode::Mirror;
    if mode == Overflow::Page && (mirror || view.line.is_none()) {
        mode = Overflow::Scroll;
    }
    let key = (!view.empty && mode != Overflow::Clip).then(|| PageKey::of(&view));
    let page = overflow::published().filter(|p| Some(p.key) == key);
    let measure = page.as_ref().and_then(|page| {
        let world = docs.get(&WindowKind::Display.id())?.context().world();
        let (_, layout) = world.text_layout(ui.text_node.get_untracked()?)?;
        let vertical = ui
            .canvas()
            .vertical_text(&app.settings, &app.appearance.skin);
        if layout.is_vertical() != vertical {
            return None;
        }
        let end = layout.lines.last()?.source.end;
        if end < page.text_trimmed || end > page.text_len {
            return None;
        }
        Some(Measure {
            vertical,
            lines: overflow::line_spans(layout),
            viewport: {
                let port = world.layout_box(ui.text_port.get_untracked()?)?;
                if vertical { port.width } else { port.height }
            },
            glyphs: &page.glyphs,
        })
    });
    let reduced = ui.reduced_motion.get_untracked();
    let (page_key, break_key) = match mode {
        Overflow::Page => (None, key),
        _ => (key, None),
    };
    let y = follower.frame(page_key, measure.as_ref(), now, reduced);
    if let Some(breaks) = pager.frame(break_key, measure.as_ref())
        && let Some(line) = view.line
    {
        app.post(Msg::Core(Cmd::PageBreaks { line, breaks }));
    }
    if ui.text_scroll.get_untracked() != y {
        ui.text_scroll.set(y);
    }
}

pub(super) fn sync_input_splits(
    app: &mut App,
    id: WindowId,
    docs: &HashMap<WindowId, RuntimeDocument>,
) {
    if WindowKind::from_id(id) != Some(WindowKind::Input) {
        return;
    }
    let Some(doc) = docs.get(&id) else {
        return;
    };
    if let Some(width) = input::changed_sidebar_width(app.ui, doc.context()) {
        app.ui.input_sidebar_width.set(width);
        app.shell.windows.geometry.input_sidebar_width = Some(width);
        app.shell.windows.geometry_dirty = true;
        app.save_geometry();
    }
    if let Some(height) = input::changed_chat_height(app.ui, doc.context()) {
        app.ui.input_dock.update(|d| d.chat_height = height);
        app.save_geometry();
    }
}

pub(super) fn record_overlay_move_or_resize(
    app: &mut App,
    kind: WindowKind,
    geometry: WindowGeometry,
) {
    app.shell.windows.record_geometry(kind, geometry);
    let (w, h) = geometry.logical_size;
    let unlocked = kind
        .overlay(&app.settings)
        .is_some_and(|o| o.lock != nanatype_settings::Lock::Locked);
    let current = app.window_size(kind);
    if unlocked
        && matches!(
            kind,
            WindowKind::Display | WindowKind::History | WindowKind::Danmaku
        )
        && ((current.0 - w).abs() > 1.0 || (current.1 - h).abs() > 1.0)
    {
        let profile =
            crate::ui::settings::preview::load_draft(&app.settings, &app.settings.skin.id, app.ui)
                .profile(&app.appearance.skin)
                .ok();
        app.remember_overlay_size(kind, [w, h], profile);
    }
}
