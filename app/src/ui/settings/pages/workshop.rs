//! The Steam Workshop page: a way to browse, and the subscriptions of each
//! kind with how they stand. Listed only in the Steam build
//! ([`super::Page::listed`]).

use super::*;
use crate::msg::WorkshopMsg;
use crate::ui::kit::Row as CardRow;
use crate::workshop::{Availability, Kind, Row, RowState, Tag};

/// Browse the Workshop for `tag` (in the Steam overlay when it is on).
pub(super) fn browse_button(ui: Ui, tag: Tag) -> AnyView {
    tool(ui, ti::WORLD, "workshop-browse", move || {
        Msg::Workshop(WorkshopMsg::Browse(tag))
    })
}

/// One kind's subscriptions, with a way to find more of it; the list says
/// so while it is empty.
fn subscriptions(
    ui: Ui,
    title_key: &'static str,
    kinds: &'static [Kind],
    tag: Option<Tag>,
) -> AnyView {
    let list = dynamic(
        move || ui.workshop.get(),
        move |view| {
            let mut rows: Vec<CardRow> = view
                .rows
                .iter()
                .filter(|r| kinds.contains(&r.kind))
                .map(|r| subscription(ui, r))
                .collect();
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
    let mut rows = Vec::new();
    if let Some(tag) = tag {
        rows.push((
            row(
                ui,
                "workshop-find",
                Some("workshop-find-hint"),
                browse_button(ui, tag),
            ),
            None,
        ));
    }
    rows.push((CardRow::inset(list), None));
    section_with(ui, title_key, rows)
}

/// The Workshop page: every subscription by what it is, and how it stands.
/// Outside the Steam build it only says where the Workshop is.
pub(super) fn workshop(ui: Ui) -> AnyView {
    if !crate::distribution::STEAM {
        return section(ui, "", vec![block(hint(ui.now_t("workshop-steam-only")))]);
    }
    let unavailable = dynamic(
        move || ui.workshop.with(|v| v.availability.clone()),
        move |availability| match availability {
            Availability::Unavailable(why) => crate::ui::kit::callout(
                crate::ui::kit::Tone::Warning,
                format!("{} · {why}", ui.now_t("workshop-unavailable")),
            )
            .into_any(),
            _ => widget(Stack::column(0.0)).into_any(),
        },
    );
    let has_other = move || {
        ui.workshop
            .with(|v| v.rows.iter().any(|r| r.kind == Kind::Other))
    };
    widget(Stack::column(0.0).gap_step(gap::step::SECTION))
        .children((
            widget(Stack::column(0.0))
                .visible(move || {
                    ui.workshop
                        .with(|v| matches!(v.availability, Availability::Unavailable(_)))
                })
                .children(unavailable),
            subscriptions(ui, "workshop-kind-skins", &[Kind::Skin], Some(Tag::Skins)),
            subscriptions(
                ui,
                "workshop-kind-sounds",
                &[Kind::SoundPack],
                Some(Tag::SoundPacks),
            ),
            widget(Stack::column(0.0))
                .visible(has_other)
                .children(subscriptions(
                    ui,
                    "workshop-kind-other",
                    &[Kind::Other],
                    None,
                )),
        ))
        .into_any()
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
        move || Msg::Workshop(WorkshopMsg::Open(item)),
    )];
    if row.state == RowState::NotInstalled {
        buttons.push(icon_action(
            ui,
            "workshop-download",
            ti::DOWNLOAD,
            move || Msg::Workshop(WorkshopMsg::Download(item)),
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
        move || Msg::Workshop(WorkshopMsg::Unsubscribe(item)),
    )
}
