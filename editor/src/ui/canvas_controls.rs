use super::{Ui, controls::hint, kit};
use crate::{msg::Msg, session::Action};
use nana_ui::runtime::view::{AnyView, IntoView, dynamic, widget};
use nana_ui::runtime::{
    Activate, Button, Checkbox, Select, SelectChanged, SelectOption, Stack, ToggleChanged,
};
use nana_ui::{ButtonKind, ControlSize};
use nana_ui_core::{LengthSpec, PointerEventsSpec, PositionSpec, SemanticColorRole};
pub fn view(ui: Ui) -> AnyView {
    let targets = dynamic(
        move || {
            ui.package_editor.with(|v| {
                v.as_ref()
                    .map(|v| std::sync::Arc::as_ptr(&v.report) as usize)
            })
        },
        move |_| {
            let mut options = vec![
                SelectOption::new("base", ui.now_t("pkgedit-layout-base")),
                SelectOption::new("landscape", ui.now_t("pkgedit-landscape")),
                SelectOption::new("portrait", ui.now_t("pkgedit-portrait")),
            ];
            if let Some(view) = ui.package_editor.get_untracked()
                && let Some(skin) = &view.report.skin
                && let Some(spec) = &skin.dialogue.responsive
            {
                for (i, rule) in spec.rules.iter().enumerate() {
                    options.push(SelectOption::new(
                        i.to_string(),
                        format!(
                            "{} {}: ≤{}×{}",
                            ui.now_t("pkgedit-layout-rule"),
                            i + 1,
                            rule.max_width,
                            rule.max_height
                        ),
                    ));
                }
            }
            widget(
                Select::new(Some("base"))
                    .size(ControlSize::Small)
                    .options(options),
            )
            .key("editor-layout-target")
            .bind(move |s: &mut Select| {
                let target = ui.canvas_target.get();
                let key = if target.len() == 1 {
                    "base"
                } else if target.len() == 3 {
                    target[2].as_str()
                } else {
                    target.get(3).map(String::as_str).unwrap_or("base")
                };
                s.value = Some(std::sync::Arc::from(key));
            })
            .on(move |e: &SelectChanged| {
                let target = match e.value.as_ref() {
                    "base" => vec!["dialogue".into()],
                    "landscape" | "portrait" => {
                        vec!["dialogue".into(), "responsive".into(), e.value.to_string()]
                    }
                    _ => vec![
                        "dialogue".into(),
                        "responsive".into(),
                        "rules".into(),
                        e.value.to_string(),
                        "layout".into(),
                    ],
                };
                ui.canvas_target.set(target);
                if e.value.as_ref() == "portrait" {
                    ui.preview_size.set(["300".into(), "600".into()]);
                } else if e.value.as_ref() == "landscape" {
                    ui.preview_size.set(["800".into(), "240".into()]);
                }
            })
            .into_any()
        },
    );
    widget(Stack::column(0.).gap_step(kit::gap::step::CONTROL))
        .children((
            widget(Checkbox::new(
                ui.now_t("pkgedit-canvas-edit"),
                ui.canvas_enabled.get_untracked(),
            ))
            .key("editor-canvas-enable")
            .bind(move |c: &mut Checkbox| c.checked = ui.canvas_enabled.get())
            .on(move |e: &ToggleChanged| {
                ui.canvas_enabled.set(e.checked);
                ui.preview_mode.set(0);
            }),
            targets,
            dynamic(
                move || {
                    (
                        ui.package_editor.with(|v| {
                            v.as_ref()
                                .map(|v| std::sync::Arc::as_ptr(&v.report) as usize)
                        }),
                        ui.preview_size.get(),
                    )
                },
                move |(_, size)| {
                    let Some(skin) = ui
                        .package_editor
                        .with_untracked(|v| v.as_ref().and_then(|v| v.report.skin.clone()))
                    else {
                        return widget(Stack::column(0.)).into_any();
                    };
                    let default = super::preview::default_size(&skin, false);
                    let [w, h] = [
                        size[0]
                            .parse::<f32>()
                            .ok()
                            .filter(|v| v.is_finite() && *v > 0.)
                            .unwrap_or(default[0]),
                        size[1]
                            .parse::<f32>()
                            .ok()
                            .filter(|v| v.is_finite() && *v > 0.)
                            .unwrap_or(default[1]),
                    ];
                    let mut layers = vec![ui.now_t("pkgedit-layout-base")];
                    if let Some(spec) = &skin.dialogue.responsive {
                        let portrait = nanatype_theme::dialogue_is_portrait(&skin, [w, h]);
                        layers.push(ui.now_t(if portrait {
                            "pkgedit-portrait"
                        } else {
                            "pkgedit-landscape"
                        }));
                        for (i, rule) in spec.rules.iter().enumerate() {
                            if w <= rule.max_width
                                && h <= rule.max_height
                                && rule.portrait.is_none_or(|p| p == portrait)
                            {
                                layers.push(format!(
                                    "{} {}",
                                    ui.now_t("pkgedit-layout-rule"),
                                    i + 1
                                ));
                            }
                        }
                    }
                    hint(format!(
                        "{}: {}",
                        ui.now_t("pkgedit-layout-matched"),
                        layers.join(" → ")
                    ))
                },
            ),
            dynamic(
                move || {
                    (
                        ui.canvas_enabled.get(),
                        ui.canvas_geometry.get(),
                        ui.canvas_selection.get(),
                    )
                },
                move |(enabled, parts, selected)| {
                    if !enabled {
                        return widget(Stack::column(0.)).into_any();
                    }
                    let rows = parts
                        .iter()
                        .filter(|p| p.key != "canvas")
                        .map(|p| {
                            let key = p.key.clone();
                            widget(
                                Button::new(key.clone())
                                    .kind(if selected.as_ref() == Some(&key) {
                                        ButtonKind::Selected
                                    } else {
                                        ButtonKind::Ghost
                                    })
                                    .size(ControlSize::Small),
                            )
                            .on(move |_: &Activate| {
                                ui.canvas_selection.set(Some(key.clone()));
                                ui.canvas_focus.set(true);
                            })
                            .into_any()
                        })
                        .collect::<Vec<_>>();
                    widget(Stack::column(0.).gap_step(kit::gap::step::CONTROL))
                        .children((
                            hint(ui.now_t("pkgedit-canvas-help")),
                            widget(Stack::row(0.).wrap(true)).children(rows),
                            dynamic(
                                move || ui.canvas_selection.get(),
                                move |selected| {
                                    let Some(key) = selected else {
                                        return widget(Stack::column(0.)).into_any();
                                    };
                                    widget(Stack::row(0.).wrap(true))
                                        .children(
                                            [
                                                ("pkgedit-duplicate", "duplicate"),
                                                ("pkgedit-remove", "remove"),
                                                ("pkgedit-layer-up", "up"),
                                                ("pkgedit-layer-down", "down"),
                                            ]
                                            .into_iter()
                                            .map(|(label, op)| {
                                                let key = key.clone();
                                                widget(
                                                    Button::new(ui.now_t(label))
                                                        .kind(ButtonKind::Ghost)
                                                        .size(ControlSize::Small),
                                                )
                                                .on_cx(move |_, _: &Activate, cx| {
                                                    cx.dispatch_program_all(Msg::PackageEditor(
                                                        Action::CanvasList {
                                                            element: key.clone(),
                                                            operation: op.into(),
                                                            target: ui
                                                                .canvas_target
                                                                .get_untracked(),
                                                        },
                                                    ))
                                                })
                                                .into_any()
                                            })
                                            .collect::<Vec<_>>(),
                                        )
                                        .into_any()
                                },
                            ),
                            widget(
                                Button::new(ui.now_t("pkgedit-add-decor"))
                                    .kind(ButtonKind::Ghost)
                                    .size(ControlSize::Small),
                            )
                            .on_cx(move |_, _: &Activate, cx| {
                                cx.dispatch_program_all(Msg::PackageEditor(Action::CanvasList {
                                    element: String::new(),
                                    operation: "add".into(),
                                    target: ui.canvas_target.get_untracked(),
                                }))
                            }),
                        ))
                        .into_any()
                },
            ),
        ))
        .into_any()
}
pub fn overlay(ui: Ui) -> AnyView {
    dynamic(
        move || {
            (
                ui.canvas_enabled.get(),
                ui.canvas_selection.get(),
                ui.canvas_geometry.get(),
            )
        },
        move |(enabled, key, parts)| {
            let rect = if *enabled {
                key.as_ref()
                    .and_then(|key| parts.iter().find(|p| &p.key == key))
                    .map(|p| p.rect)
            } else {
                None
            };
            let Some([x, y, w, h]) = rect else {
                return widget(Stack::column(0.)).into_any();
            };
            widget(
                Stack::column(0.)
                    .width(LengthSpec::Px(w))
                    .height(LengthSpec::Px(h))
                    .outline(SemanticColorRole::Accent, 1.)
                    .with_layout(move |l| {
                        l.position = PositionSpec::Absolute;
                        l.offset_left = Some(LengthSpec::Px(x));
                        l.offset_top = Some(LengthSpec::Px(y));
                        l.pointer_events = Some(PointerEventsSpec::None);
                    }),
            )
            .children(widget(
                Stack::column(0.)
                    .width(LengthSpec::Px(8.))
                    .height(LengthSpec::Px(8.))
                    .surface(SemanticColorRole::Accent)
                    .with_layout(|l| {
                        l.position = PositionSpec::Absolute;
                        l.offset_right = Some(LengthSpec::Px(-4.));
                        l.offset_bottom = Some(LengthSpec::Px(-4.));
                    }),
            ))
            .into_any()
        },
    )
    .into_any()
}
