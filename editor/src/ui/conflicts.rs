use super::{Ui, controls::hint, kit};
use crate::{msg::Msg, session::Action};
use nana_ui::runtime::view::{AnyView, IntoView, dynamic, widget};
use nana_ui::runtime::{Activate, Button, Stack};
pub fn view(ui: Ui) -> AnyView {
    dynamic(
        move || {
            ui.package_editor
                .with(|v| v.as_ref().map(|v| v.conflicts.clone()).unwrap_or_default())
        },
        move |conflicts| {
            widget(
                Stack::column(0.)
                    .gap_step(kit::gap::step::CONTROL)
                    .padding_xy(kit::inset::PAGE, 0.),
            )
            .children(
                conflicts
                    .iter()
                    .map(|path| {
                        let keep = path.clone();
                        let accept = path.clone();
                        let copy = path.clone();
                        widget(Stack::row(0.).wrap(true).gap_step(kit::gap::step::CONTROL))
                            .children((
                                hint(format!("{}: {path}", ui.now_t("pkgedit-disk-conflict"))),
                                widget(Button::new(ui.now_t("pkgedit-keep-local"))).on_cx(
                                    move |_, _: &Activate, cx| {
                                        cx.dispatch_program_all(Msg::PackageEditor(
                                            Action::ResolveConflict {
                                                path: keep.clone(),
                                                keep: true,
                                            },
                                        ))
                                    },
                                ),
                                widget(Button::new(ui.now_t("pkgedit-save-copy"))).on_cx(
                                    move |_, _: &Activate, cx| {
                                        cx.dispatch_program_all(Msg::PackageEditor(
                                            Action::CopyConflict(copy.clone()),
                                        ))
                                    },
                                ),
                                widget(Button::new(ui.now_t("pkgedit-accept-disk"))).on_cx(
                                    move |_, _: &Activate, cx| {
                                        cx.dispatch_program_all(Msg::PackageEditor(
                                            Action::ResolveConflict {
                                                path: accept.clone(),
                                                keep: false,
                                            },
                                        ))
                                    },
                                ),
                            ))
                            .into_any()
                    })
                    .collect::<Vec<_>>(),
            )
            .into_any()
        },
    )
    .into_any()
}
