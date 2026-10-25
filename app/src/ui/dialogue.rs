//! The dialogue box windows. The box itself is `nanatype-frame`'s renderer;
//! these windows add what only NanaType has: plugin views and the peer track.

use nana_ui::runtime::Stack;
use nana_ui::runtime::view::{AnyView, IntoView, dynamic, widget};

pub use nanatype_frame::ui::dialogue::*;

use super::Ui;

/// Each native window is one GPU output surface. The partner window stacks
/// one dialogue band per other seated person.
pub fn display_window(ui: Ui) -> impl IntoView {
    widget(Stack::column(0.)).children((
        dialogue_box(*ui, Source::Display, None),
        super::plugins::mounted(ui, nanatype_plugins::Mount::Display),
    ))
}

pub fn peer_window(ui: Ui) -> impl IntoView {
    widget(Stack::fill_column(0.0).with_layout(|layout| {
        layout.position = nana_ui_core::PositionSpec::Relative;
    }))
    .children((
        dynamic(
            move || {
                (
                    ui.peer.with(|state| state.active),
                    ui.peer_seats.get(),
                    ui.peer_display.with(|view| view.empty),
                )
            },
            move |(active, seats, legacy_empty)| {
                if !active {
                    return widget(Stack::column(0.0)).into_any();
                }
                // Before a room snapshot, and in tests, one box fills the window.
                let count = if *seats == 0 {
                    usize::from(!legacy_empty)
                } else {
                    (*seats).min(super::REMOTE_LANES)
                };
                let boxes: Vec<AnyView> = (0..count)
                    .map(|index| {
                        widget(Stack::column(0.0).with_layout(|layout| {
                            layout.flex_grow = Some(1.0);
                            layout.height = Some(nana_ui_core::LengthSpec::Fill);
                            layout.width = Some(nana_ui_core::LengthSpec::Fill);
                        }))
                        .children(dialogue_box(ui.stage.lane(index), Source::Display, None))
                        .into_any()
                    })
                    .collect();
                widget(Stack::fill_column(0.0)).children(boxes).into_any()
            },
        ),
        super::p2p::waiting_room(ui),
    ))
}
