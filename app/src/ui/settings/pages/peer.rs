//! The Co-op page: the session the input window's panel also shows —
//! joining or opening a room, inviting, the partner window, and each
//! seat's output.

use super::*;

pub(super) fn peer(ui: Ui) -> AnyView {
    section(
        ui,
        "",
        vec![
            block(crate::ui::p2p::page(ui)),
            block(hint(ui.now_t("peer-page-hint"))),
        ],
    )
}
