//! The dialogue box windows. The box itself is `nanatype-frame`'s renderer;
//! these windows add what only NanaType has: plugin views and the peer track.

use nana_ui::runtime::Stack;
use nana_ui::runtime::view::{IntoView, dynamic, widget};

pub use nanatype_frame::ui::dialogue::*;

use super::Ui;

/// Each native window owns one participant and one GPU output surface.
pub fn display_window(ui: Ui) -> impl IntoView {
    widget(Stack::column(0.)).children((
        dialogue_box(*ui, Source::Display, None),
        super::plugins::mounted(ui, nanatype_plugins::Mount::Display),
    ))
}

pub fn peer_window(ui: Ui) -> impl IntoView {
    dynamic(
        move || ui.peer.with(|state| state.active),
        move |active| {
            if *active {
                dialogue_box(ui.stage.remote(), Source::Display, None)
            } else {
                widget(Stack::column(0.0)).into_any()
            }
        },
    )
}
