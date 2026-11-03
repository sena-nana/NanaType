//! The dialogue box windows. The box itself is `nanatype-frame`'s renderer;
//! these windows add what only NanaType has: plugin views and the peer track.

use nana_ui::runtime::Stack;
use nana_ui::runtime::view::{IntoView, dynamic, widget};

pub use nanatype_frame::ui::dialogue::*;

use super::Ui;

/// Each native window is one GPU output surface: the local display, and one
/// window per other seated person.
pub fn display_window(ui: Ui) -> impl IntoView {
    widget(Stack::column(0.)).children((
        dialogue_box(*ui, Source::Display, None),
        super::plugins::mounted(ui, nanatype_plugins::Mount::Display),
    ))
}

/// One partner seat's window: that person's dialogue box, and while nobody
/// has connected yet the first seat's invite controls.
pub fn peer_window(ui: Ui, seat: usize) -> impl IntoView {
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
                // Before a room snapshot, and in tests, the first seat shows
                // whatever `peer_display` has.
                let seated = if *seats == 0 {
                    seat == 0 && !legacy_empty
                } else {
                    seat < (*seats).min(super::REMOTE_LANES)
                };
                if !active || !seated {
                    return widget(Stack::column(0.0)).into_any();
                }
                widget(Stack::fill_column(0.0))
                    .children(dialogue_box(ui.stage.lane(seat), Source::Display, None))
                    .into_any()
            },
        ),
        (seat == 0).then(|| super::p2p::waiting_room(ui)),
    ))
}
