//! The history window: `nanatype-frame`'s history renderer with the live
//! plugins' views above the log.

use nana_ui::runtime::view::IntoView;

pub use nanatype_frame::ui::history::*;

use super::Ui;

pub fn history_window(ui: Ui) -> impl IntoView {
    history_window_with(*ui, move || {
        super::plugins::mounted(ui, nanatype_plugins::Mount::History)
    })
}
