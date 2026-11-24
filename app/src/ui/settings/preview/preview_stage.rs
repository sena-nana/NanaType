use std::sync::Arc;

use nana_ui::runtime::Stack;
use nana_ui::runtime::view::{AnyView, IntoView, Signal, dynamic, widget};
use nana_ui_core::LengthSpec;

use super::preview_session::draft;
use super::{stage_pair, stage_pair_placeholder};
use crate::ui::kit::gap;
use crate::ui::{SkinRow, Ui};

/// Reactive preview stage: swaps placeholder for prepared surfaces without
/// remounting the surrounding editor controls.
pub(super) fn preview_stage(ui: Ui, row: SkinRow, width: Signal<f32>) -> AnyView {
    let skin_id = row.id.clone();
    dynamic(
        move || {
            (
                ui.skin_preview
                    .with(|p| p.as_ref().map(|p| Arc::as_ptr(p) as usize)),
                ui.locale.get(),
            )
        },
        move |(_, locale)| {
            let current_row = ui
                .skins
                .with_untracked(|rows| rows.iter().find(|r| r.id == skin_id).cloned())
                .unwrap_or_else(|| row.clone());
            let prepared = ui
                .skin_preview
                .with_untracked(|p| p.clone().filter(|p| p.shows(&current_row, locale.tag())));
            let body = match prepared.as_ref() {
                Some(p) => {
                    let d = draft(ui, &skin_id);
                    p.assets.pick_family(&d.display.font);
                    p.assets.pick_family(&d.history.font);
                    stage_pair(*ui, p.assets.clone(), current_row.image_urls.clone(), width)
                }
                None => {
                    let skin = current_row.skin.as_ref().expect("checked in view_in");
                    stage_pair_placeholder(*ui, skin, width)
                }
            };
            widget(
                Stack::column(0.0)
                    .gap_step(gap::step::GROUP)
                    .width(LengthSpec::Fill),
            )
            .children(body)
            .into_any()
        },
    )
    .into_any()
}
