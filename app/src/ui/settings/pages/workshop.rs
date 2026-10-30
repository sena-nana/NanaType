//! Steam Workshop on the Skins and Playback pages: a way to browse, and the
//! subscriptions of each kind with how they stand. Shown only in the Steam
//! build ([`crate::workshop::View::shown`]).

use super::*;
use crate::ui::kit::Row as CardRow;
use crate::workshop::{Availability, Kind, Row, RowState, Tag};

/// Browse the Workshop for `tag` (in the Steam overlay when it is on).
pub(super) fn browse_button(ui: Ui, tag: Tag) -> AnyView {
    button(ui, "workshop-browse", move || Msg::WorkshopBrowse(tag))
}

/// The page section listing subscriptions of the given kinds; empty while
/// there are none. `None` outside the Steam build.
pub(super) fn subscriptions(ui: Ui, kinds: &'static [Kind], tag: Tag) -> Option<AnyView> {
    if !crate::distribution::STEAM {
        return None;
    }
    let list = dynamic(
        move || ui.workshop.get(),
        move |view| {
            let mut rows: Vec<CardRow> = Vec::new();
            if let Availability::Unavailable(why) = &view.availability {
                rows.push(row_text(
                    ui.now_t("workshop-unavailable"),
                    Some(why.clone()),
                    widget(Stack::row(0.0)).into_any(),
                ));
            }
            rows.extend(
                view.rows
                    .iter()
                    .filter(|r| kinds.contains(&r.kind))
                    .map(|r| subscription(ui, r)),
            );
            if rows.is_empty() {
                rows.push(row_text(
                    ui.now_t("workshop-none"),
                    None,
                    widget(Stack::row(0.0)).into_any(),
                ));
            }
            CardRow::inset(widget(Stack::column(0.0)).children(rows))
        },
    );
    Some(section_with(
        ui,
        "workshop-subscriptions",
        vec![
            (
                row(
                    ui,
                    "workshop-find",
                    Some("workshop-find-hint"),
                    browse_button(ui, tag),
                ),
                None,
            ),
            (CardRow::inset(list), None),
        ],
    ))
}

fn state_text(ui: Ui, state: &RowState) -> String {
    match state {
        RowState::Ready => ui.now_t("workshop-ready"),
        RowState::Downloading(Some(percent)) => {
            ui.tr("workshop-downloading", &[("percent", (*percent).into())])
        }
        RowState::Downloading(None) | RowState::Pending => ui.now_t("workshop-pending"),
        RowState::NotInstalled => ui.now_t("workshop-not-installed"),
        RowState::Invalid(why) => ui.tr("workshop-invalid", &[("message", why.as_str().into())]),
        RowState::Shadowed(id) => ui.tr("workshop-shadowed", &[("id", id.as_str().into())]),
    }
}

/// One subscription: its title and state, its page, and unsubscribing
/// (asked before it acts).
fn subscription(ui: Ui, row: &Row) -> CardRow {
    let item = row.item;
    let mut buttons = vec![icon_action(
        ui,
        "workshop-view",
        ti::EXTERNAL_LINK,
        move || Msg::WorkshopOpen(item),
    )];
    if row.state == RowState::NotInstalled {
        buttons.push(icon_action(
            ui,
            "workshop-download",
            ti::DOWNLOAD,
            move || Msg::WorkshopDownload(item),
        ));
    }
    buttons.push(unsubscribe(ui, &row.title, item));
    row_text(
        row.title.clone(),
        Some(state_text(ui, &row.state)),
        controls(buttons),
    )
}

/// Unsubscribe from `item`, asked first.
pub(super) fn unsubscribe(ui: Ui, title: &str, item: u64) -> AnyView {
    confirm(
        *ui,
        ui.tr("workshop-unsubscribe-confirm", &[("name", title.into())]),
        ui.now_t("workshop-unsubscribe"),
        move |armed| {
            widget(
                IconButton::new(ti::TRASH, ui.now_t("workshop-unsubscribe"))
                    .size(ControlSize::Small)
                    .with_tooltip(ui.now_t("workshop-unsubscribe")),
            )
            .on(move |_: &Activate| armed.set(true))
            .into_any()
        },
        move || Msg::WorkshopUnsubscribe(item),
    )
}
