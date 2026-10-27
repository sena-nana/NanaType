//! The editor's one window: the launcher until a project is open, then the
//! editor filling the window. Toasts sit in its corner.

use nana_ui::runtime::view::{AnyView, EntityRef, IntoView, dynamic, widget};
use nana_ui::runtime::{Activate, Button, ScrollAxes, ScrollView, Stack, ToastStack};
use nana_ui::{ButtonKind, ControlSize, icons_tabler as ti};
use nana_ui_core::{AlignSpec, LengthSpec, SemanticColorRole};
use nanatype_theme::SkinOrigin;

use super::Ui;
use super::kit::{Type, gap, inset};
use crate::msg::Msg;

/// The launcher's page, at most this wide.
const PAGE_MAX_W: f32 = 1120.0;
/// The start panel beside the grids.
const START_W: f32 = 300.0;

pub fn editor_window(ui: Ui, toasts: EntityRef<ToastStack>) -> impl IntoView {
    let body = dynamic(
        move || (ui.package_editor.with(Option::is_some), ui.locale.get()),
        move |(editing, _)| {
            if *editing {
                return super::views::editor(ui);
            }
            widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|l| {
                l.flex_grow = Some(1.0);
                l.flex_shrink = Some(1.0);
                l.height = Some(LengthSpec::Px(0.0));
                l.min_height = Some(LengthSpec::Px(0.0));
            }))
            .children(
                widget(
                    Stack::column(0.0)
                        .width(LengthSpec::Fill)
                        .align(AlignSpec::Center)
                        .padding_step(inset::step::PAGE, inset::step::PAGE),
                )
                .children(
                    widget(
                        Stack::column(0.0)
                            .gap_step(gap::step::GROUP)
                            .max_width(PAGE_MAX_W)
                            .width(LengthSpec::Fill),
                    )
                    .children((
                        header(ui),
                        widget(
                            Stack::row(0.0)
                                .gap_step(gap::step::GROUP)
                                .align(AlignSpec::Start)
                                .wrap(true)
                                .width(LengthSpec::Fill),
                        )
                        .children((
                            widget(Stack::column(0.0).width(LengthSpec::Px(START_W)).shrink(0.))
                                .children(super::views::launcher(ui)),
                            widget(
                                Stack::column(0.0)
                                    .gap_step(gap::step::GROUP)
                                    .width(LengthSpec::Px(0.))
                                    .grow(1.)
                                    .shrink(1.)
                                    .min_width(LengthSpec::Px(super::views::CARD_W)),
                            )
                            .children((
                                widget(Stack::column(0.).gap_step(gap::step::CONTROL)).children((
                                    Type::Overline.view(ui.now_t("pkgedit-drafts")),
                                    super::views::drafts(ui),
                                )),
                                installed(ui),
                            )),
                        )),
                        data_dir(ui),
                    )),
                ),
            )
            .into_any()
        },
    )
    .grow();
    let root = widget(
        Stack::fill_column(0.0)
            .surface(SemanticColorRole::Background)
            .with_layout(|l| l.min_width = Some(LengthSpec::Px(0.0))),
    )
    .children((body, super::canvas_controls::overlay(ui)));
    super::feedback::with_toasts(root, toasts)
}

/// What this program is.
fn header(ui: Ui) -> AnyView {
    widget(Stack::column(0.0).gap_step(gap::step::TIGHT))
        .children((
            Type::PageTitle.view(ui.now_t("editor-app-title")),
            widget(Type::Caption.text(ui.now_t("editor-app-subtitle"))),
        ))
        .into_any()
}

/// Whose packages it reads.
fn data_dir(ui: Ui) -> AnyView {
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::CONTROL)
            .align(AlignSpec::Center)
            .wrap(true),
    )
    .children((
        widget(
            Type::Caption.text(
                ui.tr(
                    "editor-data-dir",
                    &[(
                        "path",
                        ui.data_dir
                            .with_untracked(|d| d.display().to_string())
                            .into(),
                    )],
                ),
            ),
        ),
        widget(
            Button::new(ui.now_t("editor-data-dir-open"))
                .kind(ButtonKind::Text)
                .size(ControlSize::Small),
        )
        .on_cx(move |_, _: &Activate, cx| {
            cx.dispatch_program_all(Msg::OpenFolder(ui.data_dir.get_untracked()));
        }),
    ))
    .into_any()
}

/// NanaType's installed packages, each to edit (user packages) or copy.
fn installed(ui: Ui) -> AnyView {
    let rows = dynamic(
        move || ui.packages.get(),
        move |rows| {
            if rows.is_empty() {
                return super::kit::empty_state(
                    ui.now_t("packages-none"),
                    Some((
                        ui.now_t("packages-refresh"),
                        Box::new(|| Msg::RefreshPackages),
                    )),
                );
            }
            let locale = ui.locale.get_untracked();
            super::views::grid(
                rows.iter()
                    .map(|row| {
                        let id = row.manifest.meta.id.clone();
                        let builtin = matches!(row.origin, SkinOrigin::Builtin);
                        let integrity = super::plugin_view::integrity_label(*ui, &row.integrity);
                        let name =
                            nanatype_theme::localized(&row.manifest.meta.name, locale.tag(), &id);
                        let caption = if builtin {
                            format!(
                                "{} · {}",
                                ui.now_t("skins-builtin"),
                                row.manifest.meta.version
                            )
                        } else {
                            format!("{} · {integrity}", row.manifest.meta.version)
                        };
                        super::views::card(
                            format!("editor-package:{id}"),
                            if builtin { ti::PACKAGE } else { ti::BOX },
                            name,
                            caption,
                            super::views::package_actions(ui, &id, builtin),
                        )
                    })
                    .collect(),
            )
        },
    );
    widget(
        Stack::column(0.0)
            .gap_step(gap::step::CONTROL)
            .width(LengthSpec::Fill),
    )
    .children((
        widget(
            Stack::row(0.0)
                .gap_step(gap::step::CONTROL)
                .width(LengthSpec::Fill)
                .align(AlignSpec::Center),
        )
        .children((
            widget(super::kit::grow_column(0.0))
                .children(Type::Overline.view(ui.now_t("packages-installed"))),
            super::kit::button(ui.now_t("packages-refresh"), || Msg::RefreshPackages),
        )),
        rows,
    ))
    .into_any()
}
