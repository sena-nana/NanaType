//! The isolated Lua debugger.
use crate::msg::Msg;
use crate::session::Action;
use crate::ui::Ui;
use crate::ui::controls::{hint, label};
use crate::ui::kit::{self, field, gap};
use crate::ui::views::*;
use nana_ui::runtime::view::{AnyView, IntoView, dynamic, widget};
use nana_ui::runtime::{Activate, Button, Checkbox, Stack, TextChanged, ToggleChanged};
use nana_ui::theme::space;
use nana_ui::{ButtonKind, ControlSize};
use nana_ui_core::{AlignSpec, RadiusTier, SemanticColorRole};
use nanatype_theme::PackageCapability;
use nanatype_theme::editor::{AssetKind, ProjectKind};
use std::sync::Arc;

// ---- isolated Lua debugging --------------------------------------------------

/// Trust, start and stop, test events and the plugin's returned views.
fn controls(ui: Ui) -> AnyView {
    let custom = ui.debug_custom;
    dynamic(
        move || {
            ui.package_editor.with(|v| {
                v.as_ref().map(|v| {
                    (
                        v.kind,
                        v.debug.clone(),
                        v.files.iter().any(|f| f.kind == AssetKind::Lua),
                    )
                })
            })
        },
        move |state| {
            let Some((kind, debug, _)) = state.clone() else {
                return widget(Stack::column(0.)).into_any();
            };
            let lua = matches!(kind, ProjectKind::Lua | ProjectKind::Mixed)
                && view_of(ui).is_some_and(|v| {
                    v.metadata
                        .as_ref()
                        .is_some_and(|m| m.capabilities.contains(&PackageCapability::Lua))
                });
            if !lua {
                return hint(ui.now_t("pkgedit-debug-no-lua"));
            }
            let events = crate::session::sample_events(&ui.now_t("skin-edit-sample"))
                .into_iter()
                .map(|(name, event)| {
                    widget(Button::new(name).size(ControlSize::Small))
                        .key(format!("package-editor-event:{name}"))
                        .bind(move |b: &mut Button| {
                            b.disabled = !ui
                                .package_editor
                                .with(|v| v.as_ref().is_some_and(|v| v.debug.running))
                        })
                        .on_cx(send(move || Action::DebugEvent(event.clone())))
                        .into_any()
                })
                .collect::<Vec<_>>();
            let output = match (&debug.update, debug.running) {
                (Some(update), _) => crate::ui::plugin_view::debug_views(
                    update,
                    Arc::new(|event| Msg::PackageEditor(Action::DebugEvent(event))),
                ),
                (None, true) => hint(ui.now_t("pkgedit-debug-waiting")),
                (None, false) => hint(ui.now_t("pkgedit-debug-idle")),
            };
            widget(
                Stack::column(0.0)
                    .gap_step(nana_ui_core::SpacingStep::Lg)
                    .padding_xy(0., space::SM),
            )
            .key("package-editor-debug")
            .children((
                widget(readable(ui.now_t("pkgedit-debug-risk")).color(SemanticColorRole::Warning)),
                widget(Checkbox::new(
                    ui.now_t("pkgedit-debug-trust"),
                    debug.trusted,
                ))
                .key("package-editor-debug-trust")
                .on_cx(|_, e: &ToggleChanged, cx| {
                    cx.dispatch_program_all(Msg::PackageEditor(Action::Trust(e.checked)))
                }),
                widget(
                    Stack::row(0.0)
                        .gap_step(nana_ui_core::SpacingStep::Sm)
                        .wrap(true),
                )
                .children((
                    widget(button(ui, "pkgedit-debug-start").kind(ButtonKind::Primary))
                        .key("package-editor-debug-start")
                        .bind(move |b: &mut Button| b.disabled = !debug.trusted)
                        .on_cx(send(|| Action::DebugStart)),
                    widget(button(ui, "pkgedit-debug-stop"))
                        .key("package-editor-debug-stop")
                        .bind(move |b: &mut Button| b.disabled = !debug.running)
                        .on_cx(send(|| Action::DebugStop)),
                    widget(button(ui, "pkgedit-debug-clear"))
                        .key("package-editor-debug-clear")
                        .on_cx(send(|| Action::DebugClear)),
                )),
                hint(ui.now_t("pkgedit-debug-events")),
                widget(
                    Stack::row(0.0)
                        .gap_step(nana_ui_core::SpacingStep::Sm)
                        .wrap(true),
                )
                .children(events),
                widget(
                    Stack::row(0.0)
                        .gap_step(nana_ui_core::SpacingStep::Sm)
                        .wrap(true)
                        .align(AlignSpec::Center),
                )
                .children((
                    input_box(
                        field::LG,
                        widget(kit::text_input(custom.get_untracked()).size(ControlSize::Small))
                            .key("package-editor-debug-custom")
                            .on(move |e: &TextChanged| custom.set(e.value.to_string())),
                    ),
                    widget(button(ui, "pkgedit-debug-send"))
                        .key("package-editor-debug-send")
                        .on_cx(move |_, _: &Activate, cx| {
                            let msg = match serde_json::from_str::<serde_json::Value>(
                                &custom.get_untracked(),
                            ) {
                                Ok(event) if event.get("type").is_some_and(|t| t.is_string()) => {
                                    Msg::PackageEditor(Action::DebugEvent(event))
                                }
                                _ => Msg::Notice {
                                    text: ui.now_t("pkgedit-debug-invalid-json"),
                                    error: true,
                                },
                            };
                            cx.dispatch_program_all(msg);
                        }),
                )),
                debug.error.as_ref().map(|e| {
                    let location = debug.location.clone();
                    widget(Stack::column(0.0).gap_step(nana_ui_core::SpacingStep::Xs))
                        .key("package-editor-debug-error")
                        .children((
                            widget(
                                readable(format!("{}: {e}", ui.now_t("pkgedit-debug-error")))
                                    .color(SemanticColorRole::Danger),
                            ),
                            location.map(|at| {
                                let label = format!(
                                    "{} {}:{}",
                                    ui.now_t("pkgedit-debug-locate"),
                                    at.path,
                                    at.line
                                );
                                widget(Button::new(label).size(ControlSize::Small))
                                    .key("package-editor-debug-locate")
                                    .on_cx(move |_, _: &Activate, cx| {
                                        cx.dispatch_program_all(crate::ui::shell::reveal(
                                            ui,
                                            at.clone(),
                                        ));
                                    })
                            }),
                        ))
                }),
                (
                    label(ui.now_t("pkgedit-debug-output")),
                    widget(
                        Stack::column(0.)
                            .padding_step(
                                nana_ui_core::SpacingStep::Md,
                                nana_ui_core::SpacingStep::Md,
                            )
                            .surface(SemanticColorRole::Subtle)
                            .radius(RadiusTier::Sm),
                    )
                    .key("package-editor-debug-output")
                    .children((
                        output,
                        debug
                            .update
                            .as_ref()
                            .and_then(|u| serde_json::to_string_pretty(u).ok())
                            .map(|s| widget(readable(s))),
                    )),
                ),
                (
                    hint(ui.now_t("pkgedit-event-sequence")),
                    widget(kit::text_area(ui.debug_sequence.get_untracked()).height(88.))
                        .on(move |e: &TextChanged| ui.debug_sequence.set(e.value.to_string())),
                    widget(button(ui, "pkgedit-replay"))
                        .disabled(!debug.running || debug.replaying || debug.update.is_none())
                        .on_cx(move |_, _: &Activate, cx| {
                            match serde_json::from_str::<Vec<serde_json::Value>>(
                                &ui.debug_sequence.get_untracked(),
                            ) {
                                Ok(events) => cx.dispatch_program_all(Msg::PackageEditor(
                                    Action::DebugSequence(events),
                                )),
                                Err(e) => cx.dispatch_program_all(Msg::Notice {
                                    text: e.to_string(),
                                    error: true,
                                }),
                            }
                        }),
                ),
            ))
            .into_any()
        },
    )
    .into_any()
}

/// The debugger's log, filtered: the drawer's second page.
pub fn log(ui: Ui) -> AnyView {
    widget(Stack::column(0.).gap_step(nana_ui_core::SpacingStep::Xs))
        .children((
            input_box(
                field::MD,
                widget(
                    kit::text_input(ui.debug_filter.get_untracked())
                        .placeholder(ui.now_t("pkgedit-log-filter"))
                        .size(ControlSize::Small),
                )
                .on(move |e: &TextChanged| ui.debug_filter.set(e.value.to_string())),
            ),
            dynamic(
                move || {
                    (
                        ui.debug_filter.get(),
                        ui.package_editor
                            .with(|v| v.as_ref().map(|v| v.debug.log.clone())),
                    )
                },
                move |(filter, log)| {
                    let filter = filter.to_lowercase();
                    widget(readable(
                        log.iter()
                            .flatten()
                            .filter(|line| line.to_lowercase().contains(&filter))
                            .cloned()
                            .collect::<Vec<_>>()
                            .join("\n"),
                    ))
                    .key("package-editor-debug-log")
                    .into_any()
                },
            ),
        ))
        .into_any()
}

fn is_lua(file: &nanatype_theme::editor::FileInfo) -> bool {
    file.kind == AssetKind::Lua
}

/// The package's Lua sources.
pub fn side(ui: Ui) -> AnyView {
    widget(Stack::fill_column(0.))
        .children((
            crate::ui::shell::panel_header(ui.now_t("pkgedit-lua-sources"), ()),
            crate::ui::shell::scroll(
                widget(Stack::column(0.).padding_xy(gap::TIGHT, 0.))
                    .children(super::files::file_tree(
                        ui,
                        is_lua,
                        "package-editor-lua-tree",
                    ))
                    .into_any(),
            ),
        ))
        .into_any()
}

/// The selected source above the debugger.
pub fn center(ui: Ui) -> AnyView {
    crate::ui::shell::split(
        nana_ui::runtime::view::node_ref(),
        nana_ui_core::SplitPaneModel::new(nana_ui_core::SplitAxis::Vertical, 300., 160., 900.)
            .with_from_end(true),
        "editor-lua-split",
        crate::ui::shell::pane(super::files::center(ui)),
        crate::ui::shell::pane(crate::ui::shell::scroll(
            widget(Stack::column(0.).padding_xy(gap::CONTROL, gap::TIGHT))
                .children(controls(ui))
                .into_any(),
        )),
    )
}
