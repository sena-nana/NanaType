//! Protection and signing.
use crate::session::Action;
use crate::ui::Ui;
use crate::ui::controls::hint;
use crate::ui::kit::{self, field};
use crate::ui::views::*;
use nana_ui::runtime::view::{AnyView, IntoView, dynamic, signal, widget};
use nana_ui::runtime::{Button, Checkbox, Stack, TextChanged, ToggleChanged};
use nana_ui::theme::space;
use nana_ui::{ButtonKind, ControlSize};
use nana_ui_core::AlignSpec;
use nanatype_theme::PACKAGE_MANIFEST;
use std::collections::BTreeSet;
use std::sync::Arc;

// ---- signing -------------------------------------------------------------------

pub fn view(ui: Ui) -> AnyView {
    dynamic(
        move || {
            ui.package_editor.with(|v| {
                v.as_ref().map(|v| {
                    (
                        format!("{:?}", v.seal),
                        v.key.clone(),
                        v.key_checked,
                        Arc::as_ptr(&v.files) as usize,
                    )
                })
            })
        },
        move |_| {
            let Some(view) = view_of(ui) else {
                return widget(Stack::column(0.)).into_any();
            };
            let status = crate::ui::plugin_view::integrity_label(*ui, &view.seal);
            let key_row = match (&view.key, view.key_checked) {
                (Some(fingerprint), _) => widget(readable(format!(
                    "{}: {}",
                    ui.now_t("pkgedit-key-fingerprint"),
                    fingerprint
                )))
                .key("package-editor-key")
                .into_any(),
                (None, false) => natural(
                    widget(button(ui, "pkgedit-key-load"))
                        .key("package-editor-key-load")
                        .on_cx(send(|| Action::LoadKey)),
                ),
                (None, true) => widget(Stack::row(0.0).gap_step(nana_ui_core::SpacingStep::Md).wrap(true).align(AlignSpec::Center))
                    .children((
                        hint(ui.now_t("pkgedit-key-none")),
                        widget(button(ui, "pkgedit-key-create"))
                            .key("package-editor-key-create")
                            .on_cx(send(|| Action::CreateKey)),
                    ))
                    .into_any(),
            };
            let signed = matches!(view.seal, Ok(Some(_)));
            let own = matches!((&view.seal, &view.key), (Ok(Some(seal)), Some(key)) if &seal.fingerprint == key);
            let derive_id = signal(format!("{}-copy", view.package.chars().take(56).collect::<String>()));
            let derive = widget(Stack::row(0.0).gap_step(nana_ui_core::SpacingStep::Md).wrap(true).align(AlignSpec::Center)).children((
                input_box(
                    field::MD,
                    widget(kit::text_input(derive_id.get_untracked()).size(ControlSize::Small))
                        .key("package-editor-derive-id")
                        .on(move |e: &TextChanged| derive_id.set(e.value.to_string())),
                ),
                widget(button(ui, "pkgedit-derive"))
                    .key("package-editor-derive")
                    .on_cx(send(move || Action::Derive(derive_id.get_untracked()))),
            ));
            let body: AnyView = if signed {
                widget(Stack::column(0.0).gap_step(nana_ui_core::SpacingStep::Md))
                    .children((
                        hint(ui.now_t(if own { "pkgedit-signed-own" } else { "pkgedit-signed-other" })),
                        own.then(|| {
                            natural(
                                widget(button(ui, "pkgedit-unseal"))
                                    .key("package-editor-unseal")
                                    .on_cx(send(|| Action::Unseal)),
                            )
                        }),
                        derive,
                    ))
                    .into_any()
            } else {
                let protected = signal(BTreeSet::<String>::new());
                let rows = view
                    .files
                    .iter()
                    .filter(|f| f.dependency.is_none() && f.path != nanatype_theme::INTEGRITY_FILE)
                    .map(|f| {
                        let path = f.path.clone();
                        let manifest = path == PACKAGE_MANIFEST;
                        widget(Checkbox::new(path.clone(), manifest).disabled(manifest))
                            .key(node_key("package-editor-protect", &path))
                            .on(move |e: &ToggleChanged| {
                                protected.update(|p| {
                                    if e.checked {
                                        p.insert(path.clone());
                                    } else {
                                        p.remove(&path);
                                    }
                                })
                            })
                            .into_any()
                    })
                    .collect::<Vec<_>>();
                let can_sign = view.key.is_some();
                widget(Stack::column(0.0).gap_step(nana_ui_core::SpacingStep::Sm))
                    .children((
                        hint(ui.now_t("pkgedit-protect")),
                        widget(Stack::column(0.0).gap_step(nana_ui_core::SpacingStep::Xs)).children(rows),
                        natural(
                            widget(button(ui, "pkgedit-sign").kind(ButtonKind::Primary))
                                .key("package-editor-sign")
                                .bind(move |b: &mut Button| b.disabled = !can_sign)
                                .on_cx(send(move || Action::Sign(protected.get_untracked()))),
                        ),
                        hint(ui.now_t("pkgedit-derive-hint")),
                        derive,
                    ))
                    .into_any()
            };
            widget(Stack::column(0.0).gap_step(nana_ui_core::SpacingStep::Lg).padding_xy(0., space::SM))
                .key("package-editor-signing")
                .children((
                    widget(readable(status)).key("package-editor-seal"),
                    hint(ui.now_t("pkgedit-seal-note")),
                    key_row,
                    hint(ui.now_t("pkgedit-key-note")),
                    body,
                ))
                .into_any()
        },
    )
    .into_any()
}
