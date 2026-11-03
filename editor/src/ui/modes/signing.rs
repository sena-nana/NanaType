//! Protection and signing.
use crate::session::Action;
use crate::ui::Ui;
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
            let signed = matches!(view.seal, Ok(Some(_)));
            let own = matches!((&view.seal, &view.key), (Ok(Some(seal)), Some(key)) if &seal.fingerprint == key);
            // The seal on one line: what it says, what a seal means on hover,
            // and Unseal when the seal is this computer's.
            let seal_tip = ui.now_t(match (signed, own) {
                (true, true) => "pkgedit-signed-own",
                (true, false) => "pkgedit-signed-other",
                _ => "pkgedit-seal-note",
            });
            let seal_row = kit::row_marked(
                status,
                seal_tip,
                own.then(|| {
                    natural(
                        widget(button(ui, "pkgedit-unseal"))
                            .key("package-editor-unseal")
                            .on_cx(send(|| Action::Unseal)),
                    )
                })
                .into_any(),
            );
            let key_control = match (&view.key, view.key_checked) {
                (Some(fingerprint), _) => widget(
                    readable(fingerprint.clone()).style(crate::ui::kit::Type::Caption.mono()),
                )
                .key("package-editor-key")
                .into_any(),
                (None, false) => natural(
                    widget(button(ui, "pkgedit-key-load"))
                        .key("package-editor-key-load")
                        .on_cx(send(|| Action::LoadKey)),
                ),
                (None, true) => natural(
                    widget(button(ui, "pkgedit-key-create"))
                        .key("package-editor-key-create")
                        .on_cx(send(|| Action::CreateKey)),
                ),
            };
            let key_label = ui.now_t(match (&view.key, view.key_checked) {
                (Some(_), _) => "pkgedit-key-fingerprint",
                (None, true) => "pkgedit-key-none",
                (None, false) => "pkgedit-key-title",
            });
            let key_row = kit::row_marked(key_label, ui.now_t("pkgedit-key-note"), key_control);
            let derive_id = signal(format!("{}-copy", view.package.chars().take(56).collect::<String>()));
            let derive = kit::row_text(
                ui.now_t("pkgedit-derive-id"),
                None,
                widget(Stack::row(0.0).gap_step(kit::gap::step::CONTROL).align(AlignSpec::Center)).children((
                    input_box(
                        field::MD,
                        widget(kit::text_input(derive_id.get_untracked()).size(ControlSize::Small))
                            .key("package-editor-derive-id")
                            .on(move |e: &TextChanged| derive_id.set(e.value.to_string())),
                    ),
                    widget(button(ui, "pkgedit-derive"))
                        .key("package-editor-derive")
                        .on_cx(send(move || Action::Derive(derive_id.get_untracked()))),
                ))
                .into_any(),
            );
            let protect: Option<AnyView> = (!signed).then(|| {
                let protected = signal(BTreeSet::<String>::new());
                let rows = view
                    .files
                    .iter()
                    .filter(|f| f.dependency.is_none() && f.path != nanatype_theme::INTEGRITY_FILE)
                    .map(|f| {
                        let path = f.path.clone();
                        let manifest = path == PACKAGE_MANIFEST;
                        let check = widget(Checkbox::new(String::new(), manifest).disabled(manifest))
                            .key(node_key("package-editor-protect", &path))
                            .on({
                                let path = path.clone();
                                move |e: &ToggleChanged| {
                                    protected.update(|p| {
                                        if e.checked {
                                            p.insert(path.clone());
                                        } else {
                                            p.remove(&path);
                                        }
                                    })
                                }
                            })
                            .into_any();
                        (kit::row_text(path, None, check), None)
                    })
                    .collect::<Vec<_>>();
                let can_sign = view.key.is_some();
                widget(Stack::column(0.0).gap_step(kit::gap::step::CONTROL))
                    .children((
                        kit::section_tools(
                            ui.now_t("pkgedit-protect-title"),
                            kit::hint_mark(ui.now_t("pkgedit-protect")),
                            rows,
                        ),
                        natural(
                            widget(button(ui, "pkgedit-sign").kind(ButtonKind::Primary))
                                .key("package-editor-sign")
                                .bind(move |b: &mut Button| b.disabled = !can_sign)
                                .on_cx(send(move || Action::Sign(protected.get_untracked()))),
                        ),
                    ))
                    .into_any()
            });
            widget(Stack::column(0.0).gap_step(kit::gap::step::SECTION).padding_xy(0., space::SM))
                .key("package-editor-signing")
                .children((
                    kit::section_titled(
                        Some(ui.now_t("pkgedit-signing-title")),
                        vec![
                            (widget(Stack::column(0.0)).key("package-editor-seal").children(seal_row).into_any(), None),
                            (key_row.into_any(), None),
                        ],
                    ),
                    protect,
                    kit::section_tools(
                        ui.now_t("pkgedit-derive-title"),
                        kit::hint_mark(ui.now_t("pkgedit-derive-hint")),
                        vec![(derive, None)],
                    ),
                ))
                .into_any()
        },
    )
    .into_any()
}
