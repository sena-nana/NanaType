//! Publishing: what a package goes through on its way out, top to bottom.
//! Check, choose what the seal protects, sign, export.
use crate::session::Action;
use crate::shortcuts::hinted;
use crate::ui::controls::hint;
use crate::ui::kit::{self, Tone, gap};
use crate::ui::shell::tool;
use crate::ui::views::*;
use crate::ui::{Drawer, Ui, rows};
use nana_ui::runtime::view::{AnyView, IntoView, dynamic, widget};
use nana_ui::runtime::{Activate, Button, Checkbox, Stack, ToggleChanged};
use nana_ui::{ButtonKind, icons_tabler as ti};
use nanatype_theme::PACKAGE_MANIFEST;
use std::collections::BTreeSet;
use std::sync::Arc;

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
                        (v.validating, v.report.errors(), v.dirty),
                    )
                })
            })
        },
        move |_| {
            let Some(view) = view_of(ui) else {
                return widget(Stack::column(0.)).into_any();
            };
            let signed = matches!(view.seal, Ok(Some(_)));
            let own = matches!((&view.seal, &view.key), (Ok(Some(seal)), Some(key)) if &seal.fingerprint == key);
            // 1. The check every export runs anyway.
            let errors = view.report.errors();
            let check = rows::item(
                Some(if view.validating {
                    Tone::Neutral
                } else if errors > 0 {
                    Tone::Danger
                } else {
                    Tone::Success
                }),
                hint(if view.validating {
                    ui.now_t("pkgedit-validating")
                } else if errors > 0 {
                    ui.tr("pkgedit-errors", &[("count", (errors as i64).into())])
                } else {
                    ui.now_t("pkgedit-valid")
                }),
                (errors > 0).then(|| {
                    widget(button(ui, "pkgedit-problems").kind(ButtonKind::Ghost))
                        .key("package-editor-publish-problems")
                        .on(move |_: &Activate| ui.drawer.set(Drawer::Problems))
                }),
            );
            // 2. The seal as it stands, and this computer's key.
            let seal_tip = ui.now_t(match (signed, own) {
                (true, true) => "pkgedit-signed-own",
                (true, false) => "pkgedit-signed-other",
                _ => "pkgedit-seal-note",
            });
            let seal = rows::row(
                ui.now_t("pkgedit-seal-status"),
                (
                    widget(Stack::column(0.).grow(1.).shrink(1.))
                        .key("package-editor-seal")
                        .children(hint(crate::ui::plugin_view::integrity_label(
                            *ui, &view.seal,
                        ))),
                    kit::hint_mark(seal_tip),
                    own.then(|| {
                        widget(button(ui, "pkgedit-unseal"))
                            .key("package-editor-unseal")
                            .on_cx(send(|| Action::Unseal))
                    }),
                ),
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
            let key = rows::row(
                ui.now_t(match (&view.key, view.key_checked) {
                    (Some(_), _) => "pkgedit-key-fingerprint",
                    (None, true) => "pkgedit-key-none",
                    (None, false) => "pkgedit-key-title",
                }),
                (
                    widget(Stack::column(0.).grow(1.).shrink(1.)).children(key_control),
                    kit::hint_mark(ui.now_t("pkgedit-key-note")),
                ),
            );
            // 3. What the seal covers. The choice outlives this page: it is
            // made over several visits, and the file list changes under it.
            let protect: Option<AnyView> = (!signed).then(|| {
                let files = view
                    .files
                    .iter()
                    .filter(|f| f.dependency.is_none() && f.path != nanatype_theme::INTEGRITY_FILE)
                    .map(|f| f.path.clone())
                    .collect::<Vec<_>>();
                let all = files.iter().cloned().collect::<BTreeSet<_>>();
                let every = all.clone();
                let boxes = files
                    .iter()
                    .map(|path| {
                        let manifest = path == PACKAGE_MANIFEST;
                        let (bound, toggled) = (path.clone(), path.clone());
                        widget(Checkbox::new(path.clone(), manifest).disabled(manifest))
                            .key(node_key("package-editor-protect", path))
                            .bind(move |c: &mut Checkbox| {
                                let on = manifest || ui.protect.with(|p| p.contains(&bound));
                                if c.checked != on {
                                    c.checked = on;
                                }
                            })
                            .on(move |e: &ToggleChanged| {
                                ui.protect.update(|p| {
                                    if e.checked {
                                        p.insert(toggled.clone());
                                    } else {
                                        p.remove(&toggled);
                                    }
                                })
                            })
                            .into_any()
                    })
                    .collect::<Vec<_>>();
                let can_sign = view.key.is_some();
                rows::group(
                    ui.now_t("pkgedit-protect-title"),
                    (
                        kit::hint_mark(ui.now_t("pkgedit-protect")),
                        widget(tool(ti::CHECKS, ui.now_t("pkgedit-protect-all")))
                            .key("package-editor-protect-all")
                            .on(move |_: &Activate| ui.protect.set(every.clone())),
                        widget(tool(ti::SQUARE, ui.now_t("pkgedit-protect-none")))
                            .key("package-editor-protect-none")
                            .on(move |_: &Activate| ui.protect.set(BTreeSet::new())),
                    ),
                    (
                        widget(Stack::column(0.).gap_step(gap::step::HAIR)).children(boxes),
                        natural(
                            widget(button(ui, "pkgedit-sign").kind(ButtonKind::Primary))
                                .key("package-editor-sign")
                                .bind(move |b: &mut Button| b.disabled = !can_sign)
                                .on_cx(send(move || {
                                    // Only files that are still there.
                                    Action::Sign(ui.protect.with_untracked(|p| {
                                        p.intersection(&all).cloned().collect()
                                    }))
                                })),
                        ),
                    ),
                )
            });
            // 4. Out.
            let export = rows::group(
                ui.now_t("pkgedit-export"),
                kit::hint_mark(ui.now_t("pkgedit-intro")),
                natural(
                    widget(
                        Button::new(hinted(ui.now_t("pkgedit-export"), "Mod+E"))
                            .icon(ti::UPLOAD)
                            .kind(if signed || view.key.is_none() {
                                ButtonKind::Primary
                            } else {
                                ButtonKind::Subtle
                            })
                            .size(nana_ui::ControlSize::Small),
                    )
                    .key("package-editor-publish-export")
                    .on_cx(send(|| Action::Export)),
                ),
            );
            rows::form(
                "package-editor-signing",
                (
                    rows::group(ui.now_t("pkgedit-publish-check"), (), check),
                    rows::group(ui.now_t("pkgedit-signing-title"), (), (seal, key)),
                    protect,
                    export,
                ),
            )
        },
    )
    .into_any()
}
