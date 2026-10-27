//! The inspector: the declared table the layer tree selects, one compact row
//! per value, and that table's own source. The source remains authoritative.
use super::{Ui, controls::hint, kit, views::buffer};
use crate::session::EditorView;
use crate::{msg::Msg, session::Action};
use nana_ui::runtime::view::{AnyView, IntoView, dynamic, signal, widget};
use nana_ui::runtime::{
    Activate, Button, Checkbox, ColorChanged, ColorField, IconButton, ScrubChanged, ScrubField,
    Select, SelectChanged, SelectOption, Stack, Text, TextChanged, ToggleChanged,
};
use nana_ui::{ButtonKind, ControlSize, icons_tabler as ti};
use nana_ui_core::{AlignSpec, LengthSpec, SemanticColorRole};
use nanatype_theme::{
    MANIFEST, PACKAGE_MANIFEST, TomlTable,
    editor::{self, PropertyValue as Value},
};
use std::sync::Arc;

/// The inspector's label column.
const LABEL_W: f32 = 104.0;
/// The section source editor.
const SECTION_H: f32 = 140.0;
/// A new field's value.
const NEW_VALUE_H: f32 = 64.0;

fn button(ui: Ui, key: &str) -> Button {
    Button::new(ui.now_t(key))
        .size(ControlSize::Small)
        .kind(ButtonKind::Ghost)
}

/// `file`'s declared values over what it inherits, and its own declarations.
pub fn effective(view: &EditorView, file: &str) -> Result<(TomlTable, TomlTable), String> {
    let text = view.documents.get(file).ok_or_else(|| file.to_owned())?;
    let local = editor::parse_document(text).map_err(|e| e.to_string())?;
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
    Ok((effective, local))
}

/// Byte range of the source block that declares `path`: its `[a.b]` header
/// (or the n-th `[[a.b]]`) through its sub-tables, up to the next table.
pub fn table_range(text: &str, path: &[String]) -> Option<(usize, usize)> {
    let (name, nth) = match path.last().and_then(|s| s.parse::<usize>().ok()) {
        Some(i) if path.len() > 1 => (path[..path.len() - 1].join("."), Some(i)),
        _ => (path.join("."), None),
    };
    let header = |line: &str| -> Option<(String, bool)> {
        let line = line.split('#').next()?.trim();
        let array = line.starts_with("[[") && line.ends_with("]]");
        let inner = if array {
            line.get(2..line.len() - 2)?
        } else if line.starts_with('[') && line.ends_with(']') && line.len() > 2 {
            line.get(1..line.len() - 1)?
        } else {
            return None;
        };
        let inner = inner
            .split('.')
            .map(|p| p.trim().trim_matches('"').trim_matches('\''))
            .collect::<Vec<_>>();
        inner
            .iter()
            .all(|p| {
                !p.is_empty()
                    && p.chars()
                        .all(|c| c.is_alphanumeric() || matches!(c, '_' | '-' | ' '))
            })
            .then(|| (inner.join("."), array))
    };
    let mut offset = 0;
    let mut start = None;
    let mut seen = 0;
    for line in text.split_inclusive('\n') {
        if let Some((found, array)) = header(line) {
            match start {
                None => {
                    if found == name && array == nth.is_some() {
                        if nth.is_none_or(|n| n == seen) {
                            start = Some(offset);
                        }
                        seen += 1;
                    }
                }
                Some(begin) => {
                    let nested = found.starts_with(&format!("{name}.")) && !(found == name);
                    if !nested {
                        return Some((begin, offset));
                    }
                }
            }
        }
        offset += line.len();
    }
    start.map(|begin| (begin, text.len()))
}

pub fn inspector(ui: Ui) -> AnyView {
    let file = ui.properties_file;
    let path = ui.properties_path;
    dynamic(
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
            if !view.documents.contains_key(file) {
                return hint(ui.now_t("pkgedit-no-file"));
            }
            let (effective, local) = match effective(&view, file) {
                Ok(v) => v,
                Err(e) => return hint(e),
            };
            let table = if path_value.is_empty() {
                Some(&effective)
            } else {
                editor::value_at(&effective, path_value).and_then(Value::as_table)
            };
            let Some(table) = table else {
                return hint(ui.now_t("pkgedit-property-select"));
            };
            let editable = view.files.iter().any(|f| f.path == *file && f.editable);
            let mut rows = vec![title(ui, file, path_value, editable)];
            let (groups, values): (Vec<_>, Vec<_>) = table
                .iter()
                .partition(|(_, v)| matches!(v, Value::Table(t) if !t.contains_key("param")));
            for (key, value) in values {
                if file == PACKAGE_MANIFEST
                    && matches!(key.as_str(), "id" | "format" | "schema" | "derived_from")
                {
                    continue;
                }
                let mut target = path_value.clone();
                target.push(key.clone());
                rows.push(property(ui, file, &target, key, value, &local, editable));
            }
            for (key, _) in groups {
                let mut target = path_value.clone();
                target.push(key.clone());
                rows.push(
                    widget(Stack::row(0.))
                        .children(
                            widget(
                                Button::new(key.clone())
                                    .icon(ti::CHEVRON_RIGHT)
                                    .kind(ButtonKind::Ghost)
                                    .size(ControlSize::Small),
                            )
                            .key(format!("editor-properties-group:{}", target.join(".")))
                            .on(move |_: &Activate| path.set(target.clone())),
                        )
                        .into_any(),
                );
            }
            rows.push(add_field(ui, file, path_value, table, editable));
            rows.push(section_source(ui, file, path_value, editable));
            widget(
                Stack::column(0.)
                    .gap_step(kit::gap::step::TIGHT)
                    .padding_xy(kit::gap::CONTROL, kit::gap::TIGHT)
                    .width(LengthSpec::Fill),
            )
            .children(rows)
            .into_any()
        },
    )
    .into_any()
}

/// The table's name, where it sits, and the way back up.
fn title(ui: Ui, file: &str, path_value: &[String], editable: bool) -> AnyView {
    let path = ui.properties_path;
    let name = path_value
        .last()
        .cloned()
        .unwrap_or_else(|| file.to_owned());
    let parent = (!path_value.is_empty()).then(|| path_value[..path_value.len() - 1].to_vec());
    widget(
        Stack::column(0.)
            .gap_step(kit::gap::step::HAIR)
            .padding_xy(0., kit::gap::TIGHT),
    )
    .key("editor-properties")
    .children((
        widget(
            Stack::row(0.)
                .gap_step(kit::gap::step::TIGHT)
                .align(AlignSpec::Center),
        )
        .children((
            parent.map(|parent| {
                widget(super::shell::tool(ti::ARROW_UP, ui.now_t("pkgedit-up")))
                    .key("editor-properties-up")
                    .on(move |_: &Activate| path.set(parent.clone()))
            }),
            widget(
                Stack::row(0.)
                    .grow(1.)
                    .shrink(1.)
                    .min_width(LengthSpec::Px(0.)),
            )
            .children(widget(kit::Type::Strong.text(name).truncating())),
        )),
        widget(
            Text::new(
                std::iter::once(file.to_owned())
                    .chain(path_value.iter().cloned())
                    .collect::<Vec<_>>()
                    .join(" › "),
            )
            .style(kit::Type::Small.mono())
            .color(SemanticColorRole::Muted)
            .truncating(),
        ),
        (!editable).then(|| hint(ui.now_t("pkgedit-read-only"))),
    ))
    .into_any()
}

/// Add a field under the table, from a template where one fits.
fn add_field(
    ui: Ui,
    file: &str,
    path_value: &[String],
    table: &TomlTable,
    editable: bool,
) -> AnyView {
    let prefix = format!("properties.new.{file}:{path_value:?}.");
    let name = buffer(ui, format!("{prefix}name"), String::new());
    let value = buffer(ui, format!("{prefix}value"), String::from("\"\""));
    let open = signal(false);
    let mut templates_row = Vec::new();
    if file == MANIFEST && path_value == ["params".to_owned()] {
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
        templates_row.push(
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
                    open.set(true);
                }
            })
            .into_any(),
        );
    }
    if file == MANIFEST
        && path_value == ["dialogue".to_owned()]
        && !table.contains_key("responsive")
    {
        templates_row.push(
            widget(button(ui, "pkgedit-add-responsive"))
                .on(move |_: &Activate| {
                    name.set("responsive".into());
                    value.set("{landscape={},portrait={},rules=[]}".into());
                    open.set(true);
                })
                .into_any(),
        );
    }
    if file == MANIFEST
        && path_value
            .last()
            .is_some_and(|p| matches!(p.as_str(), "param_groups" | "presets"))
    {
        let preset = path_value.last().is_some_and(|p| p == "presets");
        templates_row.push(widget(button(ui,"pkgedit-template")).on(move |_:&Activate|{name.set(if preset{"new_preset"}else{"new_group"}.into());value.set(if preset{"{name={zh-CN='新预设',en-US='New preset',ja-JP='新しいプリセット'},values={}}"}else{"{name={zh-CN='新分组',en-US='New group',ja-JP='新しいグループ'},order=0}"}.into());open.set(true);}).into_any());
    }
    let occupied = table.keys().cloned().collect::<Vec<_>>();
    let target = path_value.to_vec();
    let depth = target.len();
    let f = file.to_owned();
    let form = dynamic(
        move || open.get() || !name.with(String::is_empty),
        move |shown| {
            if !*shown {
                return widget(Stack::column(0.)).into_any();
            }
            let occupied = occupied.clone();
            let f = f.clone();
            let mut target = target.clone();
            widget(Stack::column(0.).gap_step(kit::gap::step::TIGHT))
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
                    widget(kit::text_area(value.get_untracked()).height(NEW_VALUE_H))
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
                            cx.dispatch_program_all(Msg::PackageEditor(Action::SetDeclaration {
                                file: f.clone(),
                                path: target.clone(),
                                value: Some(value.get_untracked()),
                                unbind: false,
                            }));
                        },
                    ),
                ))
                .into_any()
        },
    );
    widget(Stack::column(0.).gap_step(kit::gap::step::TIGHT))
        .children((
            kit::rule(),
            widget(
                Stack::row(0.)
                    .gap_step(kit::gap::step::TIGHT)
                    .align(AlignSpec::Center)
                    .wrap(true),
            )
            .children((
                widget(
                    Button::new(ui.now_t("pkgedit-add-field"))
                        .icon(ti::PLUS)
                        .kind(ButtonKind::Ghost)
                        .size(ControlSize::Small),
                )
                .key("editor-properties-add")
                .on(move |_: &Activate| open.update(|o| *o = !*o)),
                templates_row,
            )),
            form,
        ))
        .into_any()
}

/// The table's own block of source, edited in place: everything outside
/// the block stays byte for byte, comments included.
fn section_source(ui: Ui, file: &str, path_value: &[String], editable: bool) -> AnyView {
    let Some(text) = ui
        .package_editor
        .with_untracked(|v| v.as_ref().and_then(|v| v.documents.get(file).cloned()))
    else {
        return widget(Stack::column(0.)).into_any();
    };
    let range = (!path_value.is_empty())
        .then(|| table_range(&text, path_value))
        .flatten();
    let heading = widget(
        Stack::row(0.)
            .gap_step(kit::gap::step::TIGHT)
            .align(AlignSpec::Center),
    )
    .children((
        widget(
            Stack::row(0.)
                .grow(1.)
                .shrink(1.)
                .min_width(LengthSpec::Px(0.)),
        )
        .children(widget(
            kit::Type::Overline.text(ui.now_t("pkgedit-section-source")),
        )),
        range.map(|(start, end)| {
            let file = file.to_owned();
            let text = text.clone();
            widget(super::shell::tool(
                ti::CODE,
                ui.now_t("pkgedit-section-reveal"),
            ))
            .key("editor-section-reveal")
            .on_cx(move |_, _: &Activate, cx| {
                let (line, column) = editor::line_column(&text, start);
                ui.split_source.set(true);
                cx.dispatch_program_all(super::shell::reveal(
                    ui,
                    editor::SourceDiagnostic {
                        path: file.clone(),
                        offset: start,
                        len: end.saturating_sub(start).min(1),
                        line,
                        column,
                        level: editor::Level::Warning,
                        message: String::new(),
                    },
                ));
            })
        }),
    ));
    let Some((start, end)) = range else {
        return widget(Stack::column(0.).gap_step(kit::gap::step::TIGHT))
            .children((
                kit::rule(),
                heading,
                hint(ui.now_t("pkgedit-section-inline")),
            ))
            .into_any();
    };
    let block = text[start..end].to_owned();
    let draft = signal(block.clone());
    let file = file.to_owned();
    widget(Stack::column(0.).gap_step(kit::gap::step::TIGHT))
        .key("editor-section-source")
        .children((
            kit::rule(),
            heading,
            widget({
                let mut area = kit::text_area(block.clone())
                    .height(SECTION_H)
                    .read_only(!editable);
                area.code_editing = Some(nana_ui::runtime::CodeEditing::new("#", "    "));
                super::mono(Arc::make_mut(&mut area.style.layout));
                area
            })
            .key("editor-section-text")
            .on(move |e: &TextChanged| draft.set(e.value.to_string())),
            widget(
                Stack::row(0.)
                    .width(LengthSpec::Fill)
                    .justify(nana_ui_core::JustifySpec::End),
            )
            .children(
                widget(button(ui, "pkgedit-apply"))
                    .key("editor-section-apply")
                    .bind(move |b: &mut Button| {
                        b.disabled = !editable || draft.with(|d| *d == block)
                    })
                    .on_cx(move |_, _: &Activate, cx| {
                        let mut next = text[..start].to_owned();
                        next.push_str(&draft.get_untracked());
                        next.push_str(&text[end..]);
                        cx.dispatch_program_all(Msg::PackageEditor(Action::Edit {
                            path: file.clone(),
                            text: next,
                        }));
                    }),
            ),
        ))
        .into_any()
}

/// A numeric value that scrubs: whole numbers step by one, others by a tenth.
fn number(value: &Value) -> Option<(f64, bool)> {
    match value {
        Value::Integer(i) => Some((*i as f64, true)),
        Value::Float(f) => Some((*f, false)),
        _ => None,
    }
}

fn property(
    ui: Ui,
    file: &str,
    path: &[String],
    caption: &str,
    value: &Value,
    local: &TomlTable,
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
    let key = format!("editor-property:{file}:{}", path.join("."));
    let commit = {
        let f = file.to_owned();
        let p = path.to_vec();
        move |value: String| {
            Msg::PackageEditor(Action::SetDeclaration {
                file: f.clone(),
                path: p.clone(),
                value: Some(value),
                unbind: unbind.get_untracked(),
            })
        }
    };
    let scrub = number(value).filter(|_| !bound);
    let control = if let Value::Boolean(b) = value {
        widget(
            Checkbox::new(caption, raw.get_untracked().parse().unwrap_or(*b)).disabled(!editable),
        )
        .key(key)
        .bind(move |c: &mut Checkbox| c.checked = raw.get().parse().unwrap_or(false))
        .on(move |e: &ToggleChanged| raw.set(e.checked.to_string()))
        .into_any()
    } else if let Some((initial, whole)) = scrub {
        let commit = commit.clone();
        let mut field = ScrubField::new(initial)
            .range(-1.0e6, 1.0e6)
            .step(if whole { 1.0 } else { 0.1 })
            .precision(if whole { 0 } else { 2 })
            .size(ControlSize::Small)
            .disabled(!editable);
        Arc::make_mut(&mut field.style.layout).width = Some(LengthSpec::Fill);
        widget(field)
            .key(key)
            .on_cx(move |_, e: &ScrubChanged, cx| {
                let value = if whole {
                    (e.value.round() as i64).to_string()
                } else {
                    format!("{}", (e.value * 100.).round() / 100.)
                };
                raw.set(value.clone());
                cx.dispatch_program_all(commit(value));
            })
            .into_any()
    } else {
        let mut input = kit::text_input(raw.get_untracked())
            .size(ControlSize::Small)
            .read_only(!editable);
        {
            let l = Arc::make_mut(&mut input.style.layout);
            l.width = Some(LengthSpec::Fill);
            l.min_width = Some(LengthSpec::Px(0.));
        }
        widget(input)
            .key(key)
            .bind(move |a: &mut nana_ui::runtime::TextInput| {
                super::views::sync_text(&mut a.state, raw.get())
            })
            .on(move |e: &TextChanged| raw.set(e.value.to_string()))
            .into_any()
    };
    let color = value
        .as_str()
        .and_then(|s| s.parse::<nanatype_model::Rgba8>().ok())
        .map(|c| {
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
        })
        .map(|swatch| {
            widget(Stack::row(0.).shrink(0.))
                .children(swatch)
                .into_any()
        });
    let reset_f = file.to_owned();
    let reset_p = path.to_vec();
    let present = editor::value_at(local, path).is_some();
    let parameters = ui.package_editor.with_untracked(|v| {
        v.as_ref()
            .and_then(|v| v.report.skin.as_ref())
            .map(|s| s.params.keys().cloned().collect::<Vec<_>>())
            .unwrap_or_default()
    });
    let bindable = file == MANIFEST
        && !parameters.is_empty()
        && path.first().is_some_and(|p| {
            !matches!(
                p.as_str(),
                "params" | "param_groups" | "presets" | "param_checks" | "meta"
            )
        });
    let binding_id = bindable.then(|| {
        buffer(
            ui,
            format!("properties.{file}:{path:?}.binding"),
            String::new(),
        )
    });
    let linking = signal(false);
    let apply = (scrub.is_none() && !matches!(value, Value::Boolean(_)) || bound).then(|| {
        let commit = commit.clone();
        widget(super::shell::tool(ti::CHECK, ui.now_t("pkgedit-apply")))
            .key(format!("editor-property-apply:{file}:{}", path.join(".")))
            .bind(move |b: &mut IconButton| b.disabled = !editable)
            .on_cx(move |_, _: &Activate, cx| {
                let value = raw.get_untracked();
                let value = if string {
                    serde_json::to_string(&value).unwrap_or_default()
                } else {
                    value
                };
                cx.dispatch_program_all(commit(value));
            })
    });
    // Booleans apply on toggle like numbers do.
    let toggle_apply = matches!(value, Value::Boolean(_)).then(|| {
        let commit = commit.clone();
        widget(super::shell::tool(ti::CHECK, ui.now_t("pkgedit-apply")))
            .key(format!("editor-property-apply:{file}:{}", path.join(".")))
            .bind(move |b: &mut IconButton| b.disabled = !editable)
            .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(commit(raw.get_untracked())))
    });
    let label = widget(
        Stack::row(0.)
            .width(LengthSpec::Px(LABEL_W))
            .shrink(0.)
            .align(AlignSpec::Center),
    )
    .children((!matches!(value, Value::Boolean(_))).then(|| {
        widget(
            kit::Type::Caption
                .text(caption)
                .color(if present {
                    SemanticColorRole::Accent
                } else {
                    SemanticColorRole::Muted
                })
                .truncating(),
        )
    }));
    let row = widget(
        Stack::row(0.)
            .gap_step(kit::gap::step::TIGHT)
            .align(AlignSpec::Center)
            .width(LengthSpec::Fill),
    )
    .key(format!("editor-property-row:{file}:{}", path.join(".")))
    .children((
        label,
        widget(
            Stack::row(0.)
                .grow(1.)
                .shrink(1.)
                .min_width(LengthSpec::Px(0.))
                .gap_step(kit::gap::step::TIGHT)
                .align(AlignSpec::Center),
        )
        .children((control, color)),
        apply,
        toggle_apply,
        bindable.then(|| {
            widget(super::shell::tool(ti::LINK, ui.now_t("pkgedit-bind")))
                .key(format!("editor-property-link:{file}:{}", path.join(".")))
                .bind(move |b: &mut IconButton| b.selected = linking.get() || bound)
                .on(move |_: &Activate| linking.update(|l| *l = !*l))
        }),
        widget(super::shell::tool(ti::RESTORE, ui.now_t("pkgedit-inherit")))
            .key(format!("editor-property-inherit:{file}:{}", path.join(".")))
            .bind(move |b: &mut IconButton| b.disabled = !editable || !present)
            .on_cx(move |_, _: &Activate, cx| {
                cx.dispatch_program_all(Msg::PackageEditor(Action::SetDeclaration {
                    file: reset_f.clone(),
                    path: reset_p.clone(),
                    value: None,
                    unbind: false,
                }))
            }),
    ));
    let binding = binding_id.map(|id| {
        let bind_file = file.to_owned();
        let bind_path = path.to_vec();
        let file = file.to_owned();
        let path = path.to_vec();
        dynamic(
            move || linking.get() || id.with(|v| !v.is_empty()),
            move |shown| {
                if !*shown {
                    return widget(Stack::row(0.)).into_any();
                }
                let bind_file = bind_file.clone();
                let bind_path = bind_path.clone();
                widget(
                    Stack::row(0.)
                        .wrap(true)
                        .gap_step(kit::gap::step::TIGHT)
                        .padding_xy(LABEL_W, 0.),
                )
                .children((
                    widget(
                        Select::new(None::<String>)
                            .size(ControlSize::Small)
                            .options(
                                parameters
                                    .iter()
                                    .map(|p| SelectOption::new(p.clone(), p.clone()))
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
                .into_any()
            },
        )
    });
    widget(
        Stack::column(0.)
            .gap_step(kit::gap::step::HAIR)
            .width(LengthSpec::Fill),
    )
    .children((
        row,
        binding,
        bound.then(|| {
            widget(
                Stack::column(0.)
                    .gap_step(kit::gap::step::HAIR)
                    .padding_xy(LABEL_W, 0.),
            )
            .children((
                widget(Checkbox::new(ui.now_t("pkgedit-unbind"), false))
                    .on(move |e: &ToggleChanged| unbind.set(e.checked)),
                widget(
                    kit::Type::Caption
                        .text(ui.now_t("pkgedit-bound-warning"))
                        .color(SemanticColorRole::Warning),
                ),
            ))
        }),
    ))
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::table_range;

    fn path(p: &str) -> Vec<String> {
        p.split('.').map(str::to_owned).collect()
    }

    #[test]
    fn a_table_block_runs_through_its_sub_tables_to_the_next_table() {
        let text = "id = 1\n[dialogue]\nx = 1\n[dialogue.nameplate] # tag\nsize = 2\n[dialogue.nameplate.shadow]\nblur = 1\n[dialogue.portrait]\nsize = 3\n";
        let (start, end) = table_range(text, &path("dialogue.nameplate")).unwrap();
        assert_eq!(
            &text[start..end],
            "[dialogue.nameplate] # tag\nsize = 2\n[dialogue.nameplate.shadow]\nblur = 1\n"
        );
        let (start, end) = table_range(text, &path("dialogue.portrait")).unwrap();
        assert_eq!(&text[start..end], "[dialogue.portrait]\nsize = 3\n");
        assert!(table_range(text, &path("dialogue.text")).is_none());
    }

    #[test]
    fn array_tables_are_found_by_index() {
        let text = "[[dialogue.decor]]\ntext = 'a'\n[[dialogue.decor]]\ntext = 'b'\n[chrome]\n";
        let (start, end) = table_range(text, &path("dialogue.decor.1")).unwrap();
        assert_eq!(&text[start..end], "[[dialogue.decor]]\ntext = 'b'\n");
    }
}
