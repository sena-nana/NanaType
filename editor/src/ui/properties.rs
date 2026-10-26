//! Schema-shaped property navigation. The source remains authoritative.
use super::{Ui, controls::hint, kit, views::buffer};
use crate::{msg::Msg, session::Action};
use nana_ui::runtime::view::{AnyView, IntoView, dynamic, signal, widget};
use nana_ui::runtime::{
    Activate, Button, Checkbox, ColorChanged, ColorField, Select, SelectChanged, SelectOption,
    Stack, TextChanged, ToggleChanged,
};
use nana_ui::{ButtonKind, ControlSize};
use nana_ui_core::{LengthSpec, SemanticColorRole};
use nanatype_theme::{
    MANIFEST, PACKAGE_MANIFEST,
    editor::{self, PropertyValue as Value},
};
use std::sync::Arc;

fn button(ui: Ui, key: &str) -> Button {
    Button::new(ui.now_t(key))
        .size(ControlSize::Small)
        .kind(ButtonKind::Ghost)
}
pub fn view(ui: Ui) -> AnyView {
    let file = ui.properties_file;
    let path = ui.properties_path;
    let file_control = dynamic(
        move || {
            ui.package_editor.with(|v| {
                v.as_ref()
                    .map(|v| v.documents.keys().cloned().collect::<Vec<_>>())
                    .unwrap_or_default()
            })
        },
        move |files| {
            let chosen = file.get_untracked();
            let chosen = if files.contains(&chosen) {
                chosen
            } else {
                files.first().cloned().unwrap_or_default()
            };
            if chosen != file.get_untracked() {
                file.set(chosen.clone());
            }
            widget(
                Select::new(Some(chosen)).size(ControlSize::Small).options(
                    files
                        .iter()
                        .map(|f| SelectOption::new(f.clone(), f.clone()))
                        .collect::<Vec<_>>(),
                ),
            )
            .key("editor-properties-file")
            .on(move |e: &SelectChanged| {
                file.set(e.value.to_string());
                path.set(Vec::new());
            })
            .into_any()
        },
    );
    let content = dynamic(
        move || {
            (
                file.get(),
                path.get(),
                ui.package_editor.with(|v| {
                    v.as_ref().map(|v| {
                        (
                            v.documents.clone(),
                            v.revision,
                            Arc::as_ptr(&v.properties) as usize,
                        )
                    })
                }),
            )
        },
        move |(file, path_value, _)| {
            let Some(view) = ui.package_editor.get_untracked() else {
                return hint(String::new());
            };
            let Some(text) = view.documents.get(file) else {
                return hint(ui.now_t("pkgedit-no-file"));
            };
            let local = match editor::parse_document(text) {
                Ok(v) => v,
                Err(e) => return hint(e.to_string()),
            };
            let mut effective = if file == MANIFEST {
                (*view.properties).clone()
            } else if file == "sounds.toml" {
                editor::skin_properties(&Default::default())
                    .unwrap_or_default()
                    .into_iter()
                    .filter(|(k, _)| k == "sounds")
                    .collect()
            } else {
                Default::default()
            };
            effective.remove("meta");
            nanatype_theme::deep_merge(&mut effective, &local);
            if file == MANIFEST {
                for root in ["params", "param_groups", "presets"] {
                    effective
                        .entry(root)
                        .or_insert_with(|| Value::Table(Default::default()));
                }
                effective
                    .entry("param_checks")
                    .or_insert_with(|| Value::Array(Vec::new()));
            }
            let table = if path_value.is_empty() {
                Some(&effective)
            } else {
                editor::value_at(&effective, path_value).and_then(Value::as_table)
            };
            let Some(table) = table else {
                return hint(ui.now_t("pkgedit-property-select"));
            };
            let editable = view.files.iter().any(|f| f.path == *file && f.editable);
            let mut rows = Vec::new();
            rows.push(
                widget(kit::Type::Strong.text(format!("{file} / {}", path_value.join(" / "))))
                    .into_any(),
            );
            if !editable {
                rows.push(hint(ui.now_t("pkgedit-read-only")));
            }
            if !path_value.is_empty() {
                rows.push(
                    widget(button(ui, "pkgedit-up"))
                        .on({
                            let parent = path_value[..path_value.len() - 1].to_vec();
                            move |_: &Activate| path.set(parent.clone())
                        })
                        .into_any(),
                );
            }
            for (key, value) in table {
                if file == PACKAGE_MANIFEST
                    && matches!(key.as_str(), "id" | "format" | "schema" | "derived_from")
                {
                    continue;
                }
                let mut target = path_value.clone();
                target.push(key.clone());
                if matches!(value,Value::Table(t) if !t.contains_key("param")) {
                    rows.push(
                        widget(
                            Button::new(format!("▸ {key}"))
                                .kind(ButtonKind::Ghost)
                                .size(ControlSize::Small),
                        )
                        .key(format!("editor-properties-group:{}", target.join(".")))
                        .on(move |_: &Activate| path.set(target.clone()))
                        .into_any(),
                    );
                } else {
                    rows.push(property(ui, file, &target, key, value, &local, editable));
                }
            }
            let prefix = format!("properties.new.{file}:{path_value:?}.");
            let name = buffer(ui, format!("{prefix}name"), String::new());
            let value = buffer(ui, format!("{prefix}value"), String::from("\"\""));
            if file == MANIFEST && path_value == &["params".to_owned()] {
                let templates = [
                    ("number", "{type='number',default=1,min=0,max=10,step=1}"),
                    ("color", "{type='color',default='#ffffff'}"),
                    ("bool", "{type='bool',default=true}"),
                    (
                        "enum",
                        "{type='enum',default='first',options=[{id='first',name={en-US='First',zh-CN='第一个',ja-JP='最初'}}]}",
                    ),
                    ("text", "{type='text',default='Text',max_len=8}"),
                ];
                rows.push(
                    widget(
                        Select::new(None::<String>)
                            .size(ControlSize::Small)
                            .options(
                                templates
                                    .iter()
                                    .map(|(k, _)| SelectOption::new(*k, *k))
                                    .collect::<Vec<_>>(),
                            ),
                    )
                    .on(move |e: &SelectChanged| {
                        if let Some((kind, template)) =
                            templates.iter().find(|(k, _)| *k == e.value.as_ref())
                        {
                            name.set(format!("new_{kind}"));
                            value.set(template.to_string());
                        }
                    })
                    .into_any(),
                );
            }
            if file == MANIFEST
                && path_value == &["dialogue".to_owned()]
                && !table.contains_key("responsive")
            {
                rows.push(
                    widget(button(ui, "pkgedit-add-responsive"))
                        .on(move |_: &Activate| {
                            name.set("responsive".into());
                            value.set("{landscape={},portrait={},rules=[]}".into());
                        })
                        .into_any(),
                );
            }
            if file == MANIFEST
                && path_value
                    .last()
                    .is_some_and(|p| matches!(p.as_str(), "param_groups" | "presets"))
            {
                let preset = path_value.last().unwrap() == "presets";
                rows.push(widget(button(ui,"pkgedit-template")).on(move |_:&Activate|{name.set(if preset{"new_preset"}else{"new_group"}.into());value.set(if preset{"{name={zh-CN='新预设',en-US='New preset',ja-JP='新しいプリセット'},values={}}"}else{"{name={zh-CN='新分组',en-US='New group',ja-JP='新しいグループ'},order=0}"}.into());}).into_any());
            }
            let occupied = table.keys().cloned().collect::<Vec<_>>();
            let mut target = path_value.clone();
            let depth = target.len();
            let f = file.clone();
            rows.push(
                widget(Stack::column(0.).gap_step(kit::gap::step::CONTROL))
                    .children((
                        hint(ui.now_t("pkgedit-add-property-hint")),
                        widget(
                            kit::text_input(name.get_untracked())
                                .placeholder(ui.now_t("pkgedit-property-name"))
                                .size(ControlSize::Small),
                        )
                        .bind(move |a: &mut nana_ui::runtime::TextInput| {
                            super::views::sync_text(&mut a.state, name.get())
                        })
                        .on(move |e: &TextChanged| name.set(e.value.to_string())),
                        widget(kit::text_area(value.get_untracked()).height(64.))
                            .bind(move |a: &mut nana_ui::runtime::TextArea| {
                                super::views::sync_text(&mut a.state, value.get())
                            })
                            .on(move |e: &TextChanged| value.set(e.value.to_string())),
                        widget(button(ui, "pkgedit-add").disabled(!editable)).on_cx(
                            move |_, _: &Activate, cx| {
                                let key = name.get_untracked();
                                if key.trim().is_empty() || occupied.contains(&key) {
                                    cx.dispatch_program_all(Msg::Notice {
                                        text: ui.now_t("pkgedit-property-exists"),
                                        error: true,
                                    });
                                    return;
                                }
                                target.truncate(depth);
                                target.push(key);
                                cx.dispatch_program_all(Msg::PackageEditor(
                                    Action::SetDeclaration {
                                        file: f.clone(),
                                        path: target.clone(),
                                        value: Some(value.get_untracked()),
                                        unbind: false,
                                    },
                                ));
                            },
                        ),
                    ))
                    .into_any(),
            );
            widget(Stack::column(0.).gap_step(kit::gap::step::CONTROL))
                .children(rows)
                .into_any()
        },
    );
    widget(Stack::column(0.).gap_step(kit::gap::step::CONTROL))
        .key("editor-properties")
        .children((file_control, content))
        .into_any()
}

fn property(
    ui: Ui,
    file: &str,
    path: &[String],
    caption: &str,
    value: &Value,
    local: &nanatype_theme::TomlTable,
    editable: bool,
) -> AnyView {
    let bound =
        editor::bound_at(local, path) || matches!(value,Value::Table(t) if t.contains_key("param"));
    let text = match value {
        Value::String(s) => s.clone(),
        _ => editor::literal(value),
    };
    let raw = buffer(ui, format!("properties.{file}:{path:?}"), text);
    let unbind = signal(false);
    let string = matches!(value, Value::String(_));
    let control = if let Value::Boolean(b) = value {
        widget(
            Checkbox::new(caption, raw.get_untracked().parse().unwrap_or(*b)).disabled(!editable),
        )
        .key(format!("editor-property:{file}:{}", path.join(".")))
        .bind(move |c: &mut Checkbox| c.checked = raw.get().parse().unwrap_or(false))
        .on(move |e: &ToggleChanged| raw.set(e.checked.to_string()))
        .into_any()
    } else {
        widget(
            kit::text_input(raw.get_untracked())
                .size(ControlSize::Small)
                .read_only(!editable),
        )
        .key(format!("editor-property:{file}:{}", path.join(".")))
        .bind(move |a: &mut nana_ui::runtime::TextInput| {
            super::views::sync_text(&mut a.state, raw.get())
        })
        .on(move |e: &TextChanged| raw.set(e.value.to_string()))
        .into_any()
    };
    let color = value
        .as_str()
        .and_then(|s| s.parse::<nanatype_model::Rgba8>().ok());
    let color = color.map(|c| {
        widget(
            ColorField::new([
                c.r as f32 / 255.,
                c.g as f32 / 255.,
                c.b as f32 / 255.,
                c.a as f32 / 255.,
            ])
            .size(ControlSize::Small),
        )
        .bind(move |f: &mut ColorField| {
            f.disabled = !editable;
            if let Ok(c) = raw.get().parse::<nanatype_model::Rgba8>() {
                let value = [c.r, c.g, c.b, c.a].map(|v| v as f32 / 255.);
                if f.value != value {
                    *f = f.clone().value(value);
                }
            }
        })
        .on(move |e: &ColorChanged| {
            let c = e.value.map(|x| (x.clamp(0., 1.) * 255.).round() as u8);
            raw.set(nanatype_model::Rgba8::rgba(c[0], c[1], c[2], c[3]).to_hex());
        })
        .into_any()
    });
    let f = file.to_owned();
    let p = path.to_vec();
    let reset_f = f.clone();
    let reset_p = p.clone();
    let present = editor::value_at(local, path).is_some();
    let parameters = ui.package_editor.with_untracked(|v| {
        v.as_ref()
            .and_then(|v| v.report.skin.as_ref())
            .map(|s| s.params.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default()
    });
    let binding = if file == MANIFEST
        && !parameters.is_empty()
        && path.first().is_some_and(|p| {
            !matches!(
                p.as_str(),
                "params" | "param_groups" | "presets" | "param_checks" | "meta"
            )
        }) {
        let id = buffer(
            ui,
            format!("properties.{file}:{path:?}.binding"),
            String::new(),
        );
        let bind_file = file.to_owned();
        let bind_path = path.to_vec();
        Some(
            widget(Stack::row(0.).wrap(true).gap_step(kit::gap::step::CONTROL))
                .children((
                    widget(
                        Select::new(None::<String>)
                            .size(ControlSize::Small)
                            .options(
                                parameters
                                    .into_iter()
                                    .map(|p| SelectOption::new(p.clone(), p))
                                    .collect::<Vec<_>>(),
                            ),
                    )
                    .key(format!(
                        "editor-property-parameter:{file}:{}",
                        path.join(".")
                    ))
                    .bind(move |s: &mut Select| {
                        s.disabled = !editable;
                        s.value =
                            id.with(|v| (!v.is_empty()).then(|| Arc::<str>::from(v.as_str())));
                    })
                    .on(move |e: &SelectChanged| id.set(e.value.to_string())),
                    widget(button(ui, "pkgedit-bind"))
                        .key(format!("editor-property-bind:{file}:{}", path.join(".")))
                        .bind(move |b: &mut Button| {
                            b.disabled = !editable || id.with(|v| v.is_empty())
                        })
                        .on_cx(move |_, _: &Activate, cx| {
                            cx.dispatch_program_all(Msg::PackageEditor(Action::SetDeclaration {
                                file: bind_file.clone(),
                                path: bind_path.clone(),
                                value: Some(format!(
                                    "{{param={}}}",
                                    editor::literal(&Value::String(id.get_untracked()))
                                )),
                                unbind: unbind.get_untracked(),
                            }))
                        }),
                ))
                .into_any(),
        )
    } else {
        None
    };
    widget(
        Stack::column(0.)
            .gap_step(kit::gap::step::TIGHT)
            .width(LengthSpec::Fill),
    )
    .children((
        widget(kit::Type::Caption.text(format!(
            "{caption} · {}",
            ui.now_t(if present {
                "pkgedit-local"
            } else {
                "pkgedit-inherited"
            })
        ))),
        widget(Stack::row(0.).wrap(true).gap_step(kit::gap::step::CONTROL)).children((
            control,
            color,
            widget(button(ui, "pkgedit-apply").disabled(!editable)).on_cx(
                move |_, _: &Activate, cx| {
                    let value = raw.get_untracked();
                    let value = if string {
                        serde_json::to_string(&value).unwrap_or_default()
                    } else {
                        value
                    };
                    cx.dispatch_program_all(Msg::PackageEditor(Action::SetDeclaration {
                        file: f.clone(),
                        path: p.clone(),
                        value: Some(value),
                        unbind: unbind.get_untracked(),
                    }));
                },
            ),
            widget(button(ui, "pkgedit-inherit").disabled(!editable || !present)).on_cx(
                move |_, _: &Activate, cx| {
                    cx.dispatch_program_all(Msg::PackageEditor(Action::SetDeclaration {
                        file: reset_f.clone(),
                        path: reset_p.clone(),
                        value: None,
                        unbind: false,
                    }))
                },
            ),
        )),
        binding,
        bound.then(|| {
            widget(Checkbox::new(ui.now_t("pkgedit-unbind"), false))
                .on(move |e: &ToggleChanged| unbind.set(e.checked))
        }),
        bound.then(|| {
            widget(
                kit::Type::Caption
                    .text(ui.now_t("pkgedit-bound-warning"))
                    .color(SemanticColorRole::Warning),
            )
        }),
    ))
    .into_any()
}
