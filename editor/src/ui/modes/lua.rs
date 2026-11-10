//! The isolated Lua debugger.
use crate::msg::Msg;
use crate::session::Action;
use crate::shortcuts::hinted;
use crate::ui::controls::hint;
use crate::ui::kit::{self, Tone, field, gap};
use crate::ui::shell::{panel_header, scroll, tool};
use crate::ui::views::*;
use crate::ui::{Drawer, Ui, rows};
use nana_ui::runtime::view::{AnyView, IntoView, dynamic, widget};
use nana_ui::runtime::{
    Activate, Button, Checkbox, IconButton, Stack, TextChanged, TextSubmitted, ToggleChanged,
};
use nana_ui::{ButtonKind, ControlSize, icons_tabler as ti};
use nana_ui_core::{AlignSpec, LengthSpec, SemanticColorRole};
use nanatype_theme::PackageCapability;
use nanatype_theme::editor::{AssetKind, ProjectKind};
use std::sync::Arc;

// ---- isolated Lua debugging --------------------------------------------------

/// The debugger beside the source: trust, the run tools, test events and
/// what the plugin returned. Its log is the drawer's second page.
pub fn inspector(ui: Ui) -> AnyView {
    let running = move || {
        ui.package_editor
            .with(|v| v.as_ref().is_some_and(|v| v.debug.running))
    };
    let trusted = move || {
        ui.package_editor
            .with(|v| v.as_ref().is_some_and(|v| v.debug.trusted))
    };
    let tools = (
        widget(tool(
            ti::PLAYER_PLAY,
            hinted(ui.now_t("pkgedit-debug-start"), "F5"),
        ))
        .key("package-editor-debug-start")
        .bind(move |b: &mut IconButton| b.disabled = !trusted())
        .on_cx(move |_, _: &Activate, cx| {
            // A run is watched in its log.
            ui.drawer.set(Drawer::Debug);
            cx.dispatch_program_all(Msg::PackageEditor(Action::DebugStart));
        }),
        widget(tool(
            ti::PLAYER_STOP,
            hinted(ui.now_t("pkgedit-debug-stop"), "Shift+F5"),
        ))
        .key("package-editor-debug-stop")
        .bind(move |b: &mut IconButton| b.disabled = !running())
        .on_cx(send(|| Action::DebugStop)),
        widget(tool(ti::ERASER, ui.now_t("pkgedit-debug-clear")))
            .key("package-editor-debug-clear")
            .on_cx(send(|| Action::DebugClear)),
    );
    widget(Stack::fill_column(0.))
        .key("package-editor-debug-inspector")
        .children((
            panel_header(ui.now_t("pkgedit-debug-title"), tools),
            scroll(
                widget(Stack::column(0.).padding_xy(gap::CONTROL, gap::TIGHT))
                    .children(controls(ui))
                    .into_any(),
            ),
        ))
        .into_any()
}

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
            let send_custom =
                move || match serde_json::from_str::<serde_json::Value>(&custom.get_untracked()) {
                    Ok(event) if event.get("type").is_some_and(|t| t.is_string()) => {
                        Msg::PackageEditor(Action::DebugEvent(event))
                    }
                    _ => Msg::Notice {
                        text: ui.now_t("pkgedit-debug-invalid-json"),
                        error: true,
                    },
                };
            widget(Stack::column(0.0).gap_step(gap::step::CONTROL))
                .key("package-editor-debug")
                .children((
                    rows::item(
                        Some(if debug.error.is_some() {
                            Tone::Danger
                        } else if debug.running {
                            Tone::Success
                        } else {
                            Tone::Neutral
                        }),
                        hint(ui.now_t(if debug.running {
                            "pkgedit-debug-running"
                        } else {
                            "pkgedit-debug-stopped"
                        })),
                        (),
                    ),
                    // The risk is spelled out until it is accepted, and stays
                    // one hover away afterwards.
                    (!debug.trusted).then(|| {
                        widget(
                            readable(ui.now_t("pkgedit-debug-risk"))
                                .color(SemanticColorRole::Warning),
                        )
                    }),
                    widget(
                        Stack::row(0.)
                            .gap_step(gap::step::HAIR)
                            .align(AlignSpec::Center),
                    )
                    .children((
                        widget(Checkbox::new(
                            ui.now_t("pkgedit-debug-trust"),
                            debug.trusted,
                        ))
                        .key("package-editor-debug-trust")
                        .on_cx(|_, e: &ToggleChanged, cx| {
                            cx.dispatch_program_all(Msg::PackageEditor(Action::Trust(e.checked)))
                        }),
                        debug
                            .trusted
                            .then(|| kit::hint_mark(ui.now_t("pkgedit-debug-risk"))),
                    )),
                    debug.error.as_ref().map(|e| {
                        let location = debug.location.clone();
                        widget(Stack::column(0.0).gap_step(gap::step::HAIR))
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
                                    natural(
                                        widget(
                                            Button::new(label)
                                                .kind(ButtonKind::Ghost)
                                                .size(ControlSize::Small),
                                        )
                                        .key("package-editor-debug-locate")
                                        .on_cx(
                                            move |_, _: &Activate, cx| {
                                                cx.dispatch_program_all(crate::ui::shell::reveal(
                                                    ui,
                                                    at.clone(),
                                                ));
                                            },
                                        ),
                                    )
                                }),
                            ))
                    }),
                    rows::group(
                        ui.now_t("pkgedit-debug-events"),
                        (),
                        (
                            widget(Stack::row(0.0).gap_step(gap::step::TIGHT).wrap(true))
                                .children(events),
                            widget(
                                Stack::row(0.0)
                                    .gap_step(gap::step::HAIR)
                                    .align(AlignSpec::Center)
                                    .width(LengthSpec::Fill),
                            )
                            .children((
                                widget(
                                    Stack::row(0.)
                                        .grow(1.)
                                        .shrink(1.)
                                        .min_width(LengthSpec::Px(0.)),
                                )
                                .children(
                                    widget({
                                        let mut input = kit::text_input(custom.get_untracked())
                                            .label(ui.now_t("pkgedit-debug-send"))
                                            .size(ControlSize::Small);
                                        let l = Arc::make_mut(&mut input.style.layout);
                                        l.width = Some(LengthSpec::Fill);
                                        crate::ui::mono(l);
                                        input
                                    })
                                    .key("package-editor-debug-custom")
                                    .on(move |e: &TextChanged| custom.set(e.value.to_string()))
                                    .on_cx(
                                        move |_, _: &TextSubmitted, cx| {
                                            cx.dispatch_program_all(send_custom())
                                        },
                                    ),
                                ),
                                widget(tool(ti::SEND, ui.now_t("pkgedit-debug-send")))
                                    .key("package-editor-debug-send")
                                    .on_cx(move |_, _: &Activate, cx| {
                                        cx.dispatch_program_all(send_custom())
                                    }),
                            )),
                        ),
                    ),
                    rows::group(
                        ui.now_t("pkgedit-event-sequence"),
                        widget(tool(ti::PLAYER_TRACK_NEXT, ui.now_t("pkgedit-replay")))
                            .key("package-editor-debug-replay")
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
                        widget({
                            let mut area =
                                kit::text_area(ui.debug_sequence.get_untracked()).height(88.);
                            crate::ui::mono(Arc::make_mut(&mut area.style.layout));
                            area
                        })
                        .on(move |e: &TextChanged| ui.debug_sequence.set(e.value.to_string())),
                    ),
                    rows::group(
                        ui.now_t("pkgedit-debug-output"),
                        (),
                        widget(Stack::column(0.).gap_step(gap::step::TIGHT))
                            .key("package-editor-debug-output")
                            .children((
                                output,
                                debug
                                    .update
                                    .as_ref()
                                    .and_then(|u| serde_json::to_string_pretty(u).ok())
                                    .map(|s| {
                                        let mut style = kit::Type::Small.mono();
                                        kit::wrap_anywhere(&mut style);
                                        widget(nana_ui::runtime::Text::new(s).style(style))
                                    }),
                            )),
                    ),
                ))
                .into_any()
        },
    )
    .into_any()
}

/// The debugger's log, filtered: the drawer's second page.
pub fn log(ui: Ui) -> AnyView {
    widget(Stack::column(0.).gap_step(gap::step::TIGHT))
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
                    let mut style = kit::Type::Small.mono();
                    kit::wrap_anywhere(&mut style);
                    widget(
                        nana_ui::runtime::Text::new(
                            log.iter()
                                .flatten()
                                .filter(|line| line.to_lowercase().contains(&filter))
                                .cloned()
                                .collect::<Vec<_>>()
                                .join("\n"),
                        )
                        .style(style),
                    )
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

/// The selected source; the debugger is the inspector beside it.
pub fn center(ui: Ui) -> AnyView {
    super::files::center(ui)
}
