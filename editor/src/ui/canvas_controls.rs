//! The canvas's layout target, the layers a preview size hits, and the
//! selection drawn over the stage.
use super::{Ui, kit};
use nana_ui::ControlSize;
use nana_ui::runtime::view::{AnyView, IntoView, dynamic, widget};
use nana_ui::runtime::{Select, SelectChanged, SelectOption, Stack, Text};
use nana_ui_core::{LengthSpec, PointerEventsSpec, PositionSpec, SemanticColorRole};

/// The base, orientation and breakpoint layers the open skin declares.
fn target_options(ui: Ui) -> Vec<SelectOption> {
    let mut options = vec![
        SelectOption::new("base", ui.now_t("pkgedit-layout-base")),
        SelectOption::new("landscape", ui.now_t("pkgedit-landscape")),
        SelectOption::new("portrait", ui.now_t("pkgedit-portrait")),
    ];
    ui.package_editor.with(|v| {
        if let Some(view) = v
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
    });
    options
}

/// Which declaration layer canvas gestures write: base, an orientation or a
/// breakpoint rule.
pub fn target(ui: Ui) -> AnyView {
    widget(Select::new(Some("base")).size(ControlSize::Small))
        .key("editor-layout-target")
        .bind(move |s: &mut Select| {
            let options = target_options(ui);
            if s.options != options {
                s.options = options;
            }
            let target = ui.canvas_target.get();
            let key = if target.len() == 1 {
                "base"
            } else if target.len() == 3 {
                target[2].as_str()
            } else {
                target.get(3).map(String::as_str).unwrap_or("base")
            };
            let value = Some(std::sync::Arc::from(key));
            if s.value != value {
                s.value = value;
            }
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
}

/// The layers the current preview size applies, later ones on top.
fn matched_text(ui: Ui) -> String {
    let size = ui.preview_size.get();
    let Some(skin) = ui
        .package_editor
        .with(|v| v.as_ref().and_then(|v| v.report.skin.clone()))
    else {
        return String::new();
    };
    let default = super::preview::default_size(&skin, false);
    let [w, h] = [0, 1].map(|i| {
        size[i]
            .parse::<f32>()
            .ok()
            .filter(|v| v.is_finite() && *v > 0.)
            .unwrap_or(default[i])
    });
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
                layers.push(format!("{} {}", ui.now_t("pkgedit-layout-rule"), i + 1));
            }
        }
    }
    format!(
        "{}: {}",
        ui.now_t("pkgedit-layout-matched"),
        layers.join(" → ")
    )
}

pub fn matched(ui: Ui) -> AnyView {
    widget(
        kit::Type::Caption
            .text(String::new())
            .color(SemanticColorRole::Muted)
            .truncating(),
    )
    .key("editor-layout-matched")
    .bind(move |t: &mut Text| {
        let value = matched_text(ui);
        if t.value != value {
            t.value = value;
        }
    })
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
