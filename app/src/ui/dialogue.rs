//! The dialogue box window. The box itself is `nanatype-frame`'s renderer;
//! this window adds what only NanaType has: plugin views.

use nana_ui::runtime::Stack;
use nana_ui::runtime::view::{IntoView, widget};

pub use nanatype_frame::ui::dialogue::*;

use super::Ui;

/// The local display window's dialogue box and plugin views.
pub fn display_window(ui: Ui) -> impl IntoView {
    widget(Stack::column(0.)).children((
        dialogue_box(*ui, Source::Display, None),
        super::plugins::mounted(ui, nanatype_plugins::Mount::Display),
    ))
}
