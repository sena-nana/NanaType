//! Data declarations (`[data.<id>]`) and processing pipelines: the declared
//! entries listed in the side panel, the selected one's form in the centre.
//! A new entry is added once; an existing one is written field by field.
use super::package::{Apply, form_editable, notice, text_field};
use crate::msg::Msg;
use crate::session::{Action, EditorView};
use crate::ui::controls::{confirm, hint};
use crate::ui::kit::{self, Tone, Type, gap};
use crate::ui::rows;
use crate::ui::shell::{panel_header, scroll, tool};
use crate::ui::views::*;
use crate::ui::{DataTarget, Ui};
use nana_ui::runtime::view::{AnyView, IntoView, Signal, dynamic, widget};
use nana_ui::runtime::{
    Activate, Button, Select, SelectChanged, SelectOption, Stack, Text, TextChanged,
};
use nana_ui::{ButtonKind, ControlSize, icons_tabler as ti};
use nana_ui_core::{AlignSpec, LengthSpec, SemanticColorRole};
use nanatype_theme::{DataFormat, DataSpec, MANIFEST, PACKAGE_MANIFEST, PipelineSpec, PortSource};
use std::collections::BTreeMap;
use std::sync::Arc;

const FORMATS: [DataFormat; 3] = [DataFormat::Text, DataFormat::Json, DataFormat::Nthz];

/// What the page is rebuilt for.
fn revision(ui: Ui) -> Option<(u64, usize, usize)> {
    ui.proj().package_editor.with(|v| {
        v.as_ref().map(|v| {
            (
                v.revision,
                Arc::as_ptr(&v.files) as usize,
                Arc::as_ptr(&v.report) as usize,
            )
        })
    })
}

/// The first diagnostic that names the data entry `id`.
fn data_problem(view: &EditorView, id: &str) -> Option<String> {
    view.report
        .diagnostics
        .iter()
        .find(|d| {
            d.message.starts_with(&format!("data.{id}")) || d.message.contains(&format!("{id}: "))
        })
        .map(|d| d.message.clone())
}

/// One declared entry in the side panel: its name over what it is.
fn entry(
    ui: Ui,
    key: String,
    target: DataTarget,
    name: String,
    detail: String,
    broken: bool,
) -> AnyView {
    let data_target = ui.proj().data_target;
    let selected = data_target.with_untracked(|t| t.as_ref() == Some(&target));
    widget({
        let mut button = Button::new(format!("{name}  ·  {detail}"))
            .accessible_name(name)
            .content_align(nana_ui::runtime::TextHorizontalAlignment::Start)
            .icon(if broken {
                ti::ALERT_CIRCLE
            } else if matches!(target, DataTarget::Data(_)) {
                ti::DATABASE
            } else {
                ti::TRANSITION_RIGHT
            })
            .kind(if selected {
                ButtonKind::Selected
            } else {
                ButtonKind::Ghost
            })
            .size(ControlSize::Small);
        let l = Arc::make_mut(&mut button.style.layout);
        l.width = Some(LengthSpec::Fill);
        l.min_width = Some(LengthSpec::Px(0.));
        button
    })
    .key(key)
    .on(move |_: &Activate| data_target.set(Some(target.clone())))
    .into_any()
}

fn add(ui: Ui, label: &str, key: &'static str, target: DataTarget) -> AnyView {
    let data_target = ui.proj().data_target;
    widget(tool(ti::PLUS, ui.now_t(label)))
        .key(key)
        .on(move |_: &Activate| data_target.set(Some(target.clone())))
        .into_any()
}

/// The declared data and pipelines, each group with its add tool.
pub fn side(ui: Ui) -> AnyView {
    let data_target = ui.proj().data_target;
    let list = dynamic(
        move || (revision(ui), data_target.get()),
        move |_| {
            let Some(view) = view_of(ui) else {
                return widget(Stack::column(0.)).into_any();
            };
            let data_editable = form_editable(ui, PACKAGE_MANIFEST);
            let skin = view.files.iter().find(|f| f.path == MANIFEST);
            let pipelines_editable = skin.is_some_and(|f| f.editable);
            let data = view
                .data
                .iter()
                .map(|(id, spec)| {
                    entry(
                        ui,
                        format!("package-editor-data:{id}"),
                        DataTarget::Data(Some(id.clone())),
                        id.clone(),
                        format!("{} · {}", spec.format.name(), spec.path),
                        data_problem(&view, id).is_some(),
                    )
                })
                .collect::<Vec<_>>();
            let pipelines = view
                .pipelines
                .iter()
                .map(|(name, spec)| {
                    entry(
                        ui,
                        format!("package-editor-pipeline:{name}"),
                        DataTarget::Pipeline(Some(name.clone())),
                        name.clone(),
                        match spec {
                            Ok(spec) => processor_key(&spec.processor, spec.version),
                            Err(e) => e.clone(),
                        },
                        spec.is_err(),
                    )
                })
                .collect::<Vec<_>>();
            let note = match skin {
                None => Some("pkgedit-pipelines-no-skin"),
                Some(_) if !pipelines_editable => Some("pkgedit-skin-protected"),
                Some(_) if pipelines.is_empty() => Some("pkgedit-pipelines-none"),
                Some(_) => None,
            };
            widget(Stack::column(0.).width(LengthSpec::Fill))
                .children((
                    panel_header(
                        ui.now_t("pkgedit-data"),
                        data_editable.then(|| {
                            add(
                                ui,
                                "pkgedit-data-add",
                                "package-editor-data-add",
                                DataTarget::Data(None),
                            )
                        }),
                    ),
                    widget(
                        Stack::column(0.)
                            .gap_step(gap::step::HAIR)
                            .padding_xy(gap::TIGHT, 0.),
                    )
                    .key("package-editor-data")
                    .children((
                        (!data_editable).then(|| hint(ui.now_t("pkgedit-manifest-protected"))),
                        data.is_empty().then(|| hint(ui.now_t("pkgedit-data-none"))),
                        data,
                    )),
                    kit::rule(),
                    panel_header(
                        ui.now_t("pkgedit-pipelines"),
                        pipelines_editable.then(|| {
                            add(
                                ui,
                                "pkgedit-pipeline-add",
                                "package-editor-pipeline-add",
                                DataTarget::Pipeline(None),
                            )
                        }),
                    ),
                    widget(
                        Stack::column(0.)
                            .gap_step(gap::step::HAIR)
                            .padding_xy(gap::TIGHT, 0.),
                    )
                    .key("package-editor-pipelines")
                    .children((note.map(|key| hint(ui.now_t(key))), pipelines)),
                ))
                .into_any()
        },
    );
    widget(Stack::fill_column(0.))
        .key("package-editor-data-page")
        .children(scroll(list.into_any()))
        .into_any()
}

/// The selected entry's form.
pub fn center(ui: Ui) -> AnyView {
    let data_target = ui.proj().data_target;
    scroll(
        dynamic(
            move || (revision(ui), data_target.get()),
            move |(_, target)| {
                let Some(view) = view_of(ui) else {
                    return widget(Stack::column(0.)).into_any();
                };
                match target {
                    Some(DataTarget::Data(previous))
                        if previous
                            .as_ref()
                            .is_none_or(|id| view.data.contains_key(id)) =>
                    {
                        data_form(ui, &view, previous.clone())
                    }
                    Some(DataTarget::Pipeline(previous))
                        if previous
                            .as_ref()
                            .is_none_or(|name| view.pipelines.iter().any(|(n, _)| n == name)) =>
                    {
                        pipeline_form(ui, &view, previous.clone())
                    }
                    _ => rows::form(
                        "package-editor-data-empty",
                        hint(ui.now_t("pkgedit-data-select")),
                    ),
                }
            },
        )
        .into_any(),
    )
}

/// The form's title: what is edited, and Remove for a declared entry.
fn heading(ui: Ui, title: String, remove: Option<Action>) -> AnyView {
    let data_target = ui.proj().data_target;
    widget(
        Stack::row(0.)
            .gap_step(gap::step::CONTROL)
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
        .children(widget(Type::Strong.text(title).truncating())),
        remove.map(|action| {
            let action = std::sync::Mutex::new(Some(action));
            confirm(
                *ui,
                ui.now_t("pkgedit-entry-remove-question"),
                ui.now_t("pkgedit-entry-remove"),
                move |armed| {
                    widget(tool(ti::TRASH, ui.now_t("pkgedit-entry-remove")))
                        .key("package-editor-entry-remove")
                        .on(move |_: &Activate| armed.set(true))
                        .into_any()
                },
                move || {
                    data_target.set(None);
                    match action.lock().ok().and_then(|mut a| a.take()) {
                        Some(action) => Msg::PackageEditor(action),
                        None => Msg::LayoutMaterialsReady,
                    }
                },
            )
        }),
    ))
    .into_any()
}

/// Add, for a form that declares a new entry.
fn create(ui: Ui, key: &'static str, apply: Apply) -> AnyView {
    natural(
        widget(button(ui, "pkgedit-add").kind(ButtonKind::Primary))
            .key(key)
            .on_cx(move |_, _: &Activate, cx| {
                if let Some(msg) = apply() {
                    cx.dispatch_program_all(msg);
                }
            }),
    )
}

struct AreaForm<'a> {
    key: &'static str,
    session: u64,
    form_key: String,
    initial: String,
    label: String,
    value: Signal<String>,
    placeholder: &'a str,
    editable: bool,
    apply: Apply,
}

/// A multi-line value written when focus leaves it.
fn area(form: AreaForm<'_>) -> AnyView {
    on_blur(
        widget(
            kit::text_area(form.value.get_untracked())
                .read_only(!form.editable)
                .label(form.label)
                .placeholder(form.placeholder.to_owned())
                .height(FORM_TEXT_H),
        )
        .key(form.key)
        .on_cx(move |_, e: &TextChanged, cx| {
            let s = e.value.to_string();
            form.value.set(s.clone());
            cx.dispatch_program_all(buffer_msg(form.session, &form.form_key, &s, &form.initial));
        }),
        move || (form.apply)(),
    )
}

fn data_form(ui: Ui, view: &EditorView, previous: Option<String>) -> AnyView {
    let editable = form_editable(ui, PACKAGE_MANIFEST);
    let session = view.session;
    let scope = format!("data.{}.", previous.as_deref().unwrap_or("@new"));
    let current = previous.as_ref().and_then(|id| view.data.get(id)).cloned();
    let spec = current.clone();
    let field = |f: fn(&DataSpec) -> String| spec.as_ref().map(f).unwrap_or_default();
    let id_init = previous.clone().unwrap_or_default();
    let path_init = field(|c| c.path.clone());
    let format_init = current.as_ref().map_or(DataFormat::Text, |c| c.format);
    let version_init = current.as_ref().map_or(1, |c| c.format_version).to_string();
    let source_init = field(|c| c.source.clone());
    let license_init = field(|c| c.license.clone());
    let constraints_init = current
        .as_ref()
        .and_then(|c| c.constraints.as_ref())
        .and_then(|c| serde_json::to_string(c).ok())
        .unwrap_or_default();

    let id = buffer(ui, format!("{scope}id"), id_init.clone());
    let path = buffer(ui, format!("{scope}path"), path_init.clone());
    let format = buffer(ui, format!("{scope}format"), format_init);
    let version = buffer(ui, format!("{scope}version"), version_init.clone());
    let source = buffer(ui, format!("{scope}source"), source_init.clone());
    let license = buffer(ui, format!("{scope}license"), license_init.clone());
    let constraints = buffer(ui, format!("{scope}constraints"), constraints_init.clone());

    let declared = previous.clone().zip(current.clone());
    let new = previous.is_none();
    let write: Apply = {
        let previous = previous.clone();
        Arc::new(move || {
            let text = constraints.get_untracked();
            let parsed = if text.trim().is_empty() {
                Ok(None)
            } else {
                serde_json::from_str::<nanatype_theme::JsonConstraints>(&text)
                    .map(Some)
                    .map_err(|e| e.to_string())
            };
            let parsed = match parsed {
                Ok(v) => v,
                Err(e) => return Some(notice(ui, "pkgedit-data-constraints", e)),
            };
            let format_version = match version.get_untracked().trim().parse::<u32>() {
                Ok(v) => v,
                Err(e) => return Some(notice(ui, "pkgedit-data-version", e)),
            };
            let next = (
                id.get_untracked().trim().to_owned(),
                DataSpec {
                    path: path.get_untracked().trim().to_owned(),
                    format: format.get_untracked(),
                    format_version,
                    source: source.get_untracked().trim().to_owned(),
                    license: license.get_untracked().trim().to_owned(),
                    constraints: parsed,
                },
            );
            if declared.as_ref() == Some(&next) {
                return None;
            }
            Some(Msg::PackageEditor(Action::ApplyData {
                previous: previous.clone(),
                entry: Some(next),
            }))
        })
    };
    // A new entry is written by Add alone: half a declaration is not one.
    let apply: Apply = if new {
        Arc::new(|| None)
    } else {
        write.clone()
    };
    let row = |key: &str, name: &str, initial: String, value: Signal<String>, placeholder: &str| {
        rows::row(
            ui.now_t(key),
            text_field(
                format!("package-editor-field:{key}"),
                session,
                format!("{scope}{name}"),
                initial,
                ui.now_t(key),
                value,
                placeholder,
                editable,
                apply.clone(),
            ),
        )
    };
    let bytes = current.as_ref().map(|spec| {
        view.files.iter().find(|f| f.path == spec.path).map_or_else(
            || ui.now_t("pkgedit-data-missing"),
            |f| format!("{} B", f.bytes),
        )
    });
    let problem = previous.as_ref().and_then(|id| data_problem(view, id));
    let format_key = format!("{scope}format");
    rows::form(
        "package-editor-data-form",
        (
            heading(
                ui,
                previous
                    .clone()
                    .unwrap_or_else(|| ui.now_t("pkgedit-data-add")),
                previous
                    .clone()
                    .filter(|_| editable)
                    .map(|id| Action::ApplyData {
                        previous: Some(id),
                        entry: None,
                    }),
            ),
            (!editable).then(|| hint(ui.now_t("pkgedit-manifest-protected"))),
            problem.map(|message| {
                rows::item(
                    Some(Tone::Danger),
                    widget(readable(message).color(SemanticColorRole::Danger)),
                    (),
                )
            }),
            rows::group(
                ui.now_t("pkgedit-data"),
                (),
                (
                    row("pkgedit-data-id", "id", id_init, id, "notes"),
                    row(
                        "pkgedit-data-path",
                        "path",
                        path_init,
                        path,
                        "data/notes.txt",
                    ),
                    bytes.map(|b| rows::fact(ui.now_t("pkgedit-file-size"), b)),
                    rows::row(
                        ui.now_t("pkgedit-data-format"),
                        widget(
                            Select::new(Some(format.get_untracked().name()))
                                .options(
                                    FORMATS
                                        .iter()
                                        .map(|f| SelectOption::new(f.name(), f.name()))
                                        .collect::<Vec<_>>(),
                                )
                                .size(ControlSize::Small)
                                .disabled(!editable),
                        )
                        .key("package-editor-data-format")
                        .on_cx({
                            let apply = apply.clone();
                            move |_, e: &SelectChanged, cx| {
                                if let Some(f) = FORMATS.into_iter().find(|f| f.name() == &*e.value)
                                {
                                    format.set(f);
                                    cx.dispatch_program_all(buffer_msg(
                                        session,
                                        &format_key,
                                        &f,
                                        &format_init,
                                    ));
                                    if let Some(msg) = apply() {
                                        cx.dispatch_program_all(msg);
                                    }
                                }
                            }
                        }),
                    ),
                    row(
                        "pkgedit-data-version",
                        "version",
                        version_init,
                        version,
                        "1",
                    ),
                    row("package-data-source", "source", source_init, source, ""),
                    row(
                        "package-data-license",
                        "license",
                        license_init,
                        license,
                        "CC0-1.0",
                    ),
                ),
            ),
            rows::group(
                ui.now_t("pkgedit-data-constraints"),
                kit::hint_mark(ui.now_t("pkgedit-data-constraints-hint")),
                area(AreaForm {
                    key: "package-editor-data-constraints",
                    session,
                    form_key: format!("{scope}constraints"),
                    initial: constraints_init,
                    label: ui.now_t("pkgedit-data-constraints"),
                    value: constraints,
                    placeholder: "{\"type\": \"object\", \"required\": [\"items\"]}",
                    editable,
                    apply: apply.clone(),
                }),
            ),
            (new && editable).then(|| create(ui, "package-editor-data-apply", write)),
        ),
    )
}

// ---- pipelines -----------------------------------------------------------------

/// `data-id`, `pkg:<package>/<id>` or `<pipeline>.<output>`.
fn port_text(source: &PortSource) -> String {
    match (&source.data, &source.pipeline, &source.output) {
        (Some(data), _, _) => data.clone(),
        (None, Some(pipeline), Some(output)) => format!("{pipeline}.{output}"),
        _ => String::new(),
    }
}

fn parse_port(text: &str) -> PortSource {
    let text = text.trim();
    // Data IDs and pipeline names never contain `.`; package IDs may.
    match text.split_once('.') {
        Some((pipeline, output)) if !text.starts_with("pkg:") => PortSource {
            pipeline: Some(pipeline.to_owned()),
            output: Some(output.to_owned()),
            data: None,
        },
        _ => PortSource {
            data: Some(text.to_owned()),
            ..Default::default()
        },
    }
}

fn processor_key(name: &str, version: u32) -> String {
    format!("{name}@{version}")
}

fn pipeline_form(ui: Ui, view: &EditorView, previous: Option<String>) -> AnyView {
    let editable = form_editable(ui, MANIFEST);
    let session = view.session;
    let scope = format!("pipeline.{}.", previous.as_deref().unwrap_or("@new"));
    let declared = previous
        .as_ref()
        .and_then(|n| view.pipelines.iter().find(|(name, _)| name == n))
        .map(|(_, spec)| spec.clone());
    if let Some(Err(e)) = &declared {
        // A declaration that does not parse is mended in its source.
        return rows::form(
            "package-editor-pipeline-form",
            (
                heading(ui, previous.clone().unwrap_or_default(), None),
                rows::item(
                    Some(Tone::Danger),
                    widget(readable(e.clone()).color(SemanticColorRole::Danger)),
                    (),
                ),
                hint(ui.now_t("pkgedit-pipeline-broken")),
            ),
        );
    }
    let current = declared.and_then(Result::ok);
    let first = &nanatype_theme::PROCESSORS[0];
    let name_init = previous.clone().unwrap_or_default();
    let processor_init = current.as_ref().map_or_else(
        || (first.name.to_owned(), first.version),
        |c| (c.processor.clone(), c.version),
    );
    let inputs_init = current
        .as_ref()
        .map(|c| {
            c.inputs
                .iter()
                .map(|(k, v)| (k.clone(), port_text(v)))
                .collect::<BTreeMap<_, _>>()
        })
        .unwrap_or_default();
    let params_init = current
        .as_ref()
        .map(|c| {
            c.params
                .iter()
                .map(|(k, v)| format!("{k} = {v}"))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .unwrap_or_default();

    let name = buffer(ui, format!("{scope}name"), name_init.clone());
    let processor = buffer(ui, format!("{scope}processor"), processor_init.clone());
    let inputs = buffer(ui, format!("{scope}inputs"), inputs_init.clone());
    let params = buffer(ui, format!("{scope}params"), params_init.clone());

    let mut processors = nanatype_theme::PROCESSORS
        .iter()
        .map(|p| {
            SelectOption::new(
                processor_key(p.name, p.version),
                processor_key(p.name, p.version),
            )
        })
        .collect::<Vec<_>>();
    let (selected, selected_version) = processor.get_untracked();
    let selected = processor_key(&selected, selected_version);
    if !processors.iter().any(|o| *o.value == *selected) {
        // An unregistered processor stays visible so the form shows the truth.
        processors.push(SelectOption::new(selected.clone(), selected.clone()));
    }
    let before = previous.clone().zip(current.clone());
    let new = previous.is_none();
    let write: Apply = {
        let previous = previous.clone();
        Arc::new(move || {
            let (processor, version) = processor.get_untracked();
            let signature = nanatype_theme::processor(&processor, version);
            let parsed = params
                .get_untracked()
                .lines()
                .filter(|l| !l.trim().is_empty())
                .map(|l| {
                    let (k, v) = l.split_once('=').ok_or_else(|| l.trim().to_owned())?;
                    let v = v.trim().parse::<f32>().map_err(|e| e.to_string())?;
                    Ok((k.trim().to_owned(), v))
                })
                .collect::<Result<BTreeMap<_, _>, String>>();
            let parsed = match parsed {
                Ok(p) => p,
                Err(e) => return Some(notice(ui, "pkgedit-pipeline-params", e)),
            };
            let next = (
                name.get_untracked().trim().to_owned(),
                PipelineSpec {
                    inputs: inputs.with_untracked(|m| {
                        m.iter()
                            // Only the selected processor's ports; empty ones stay unbound.
                            .filter(|(k, v)| {
                                !v.trim().is_empty()
                                    && signature
                                        .is_none_or(|s| s.inputs.iter().any(|(n, _)| n == k))
                            })
                            .map(|(k, v)| (k.clone(), parse_port(v)))
                            .collect()
                    }),
                    processor,
                    version,
                    params: parsed,
                },
            );
            if before.as_ref() == Some(&next) {
                return None;
            }
            Some(Msg::PackageEditor(Action::ApplyPipeline {
                previous: previous.clone(),
                entry: Some(next),
            }))
        })
    };
    let apply: Apply = if new {
        Arc::new(|| None)
    } else {
        write.clone()
    };
    let port_apply = apply.clone();
    let inputs_key = format!("{scope}inputs");
    let processor_key_str = format!("{scope}processor");
    let ports = dynamic(move || processor.get(), {
        let apply = port_apply;
        move |(proc_name, proc_version): &(String, u32)| {
            let Some(signature) = nanatype_theme::processor(proc_name, *proc_version) else {
                return hint(ui.now_t("pkgedit-pipeline-unknown"));
            };
            let fields = signature
                .inputs
                .iter()
                .map(|(input, _)| {
                    let input = (*input).to_owned();
                    let value =
                        inputs.with_untracked(|m| m.get(&input).cloned().unwrap_or_default());
                    let (enter, leave) = (apply.clone(), apply.clone());
                    let port = input.clone();
                    let inputs_key = inputs_key.clone();
                    let inputs_init = inputs_init.clone();
                    rows::row(
                        input.clone(),
                        on_blur(
                            widget({
                                let mut field = kit::text_input(value)
                                    .label(input.clone())
                                    .placeholder("strokes")
                                    .read_only(!editable)
                                    .size(ControlSize::Small);
                                Arc::make_mut(&mut field.style.layout).width =
                                    Some(LengthSpec::Fill);
                                field
                            })
                            .key(format!("package-editor-pipeline-input:{input}"))
                            .on_cx(move |_, e: &TextChanged, cx| {
                                inputs.update(|m| {
                                    m.insert(port.clone(), e.value.to_string());
                                });
                                let next_map = inputs.get_untracked();
                                cx.dispatch_program_all(buffer_msg(
                                    session,
                                    &inputs_key,
                                    &next_map,
                                    &inputs_init,
                                ));
                            })
                            .on_cx(
                                move |_, _: &nana_ui::runtime::TextSubmitted, cx| {
                                    if let Some(msg) = enter() {
                                        cx.dispatch_program_all(msg);
                                    }
                                },
                            ),
                            move || leave(),
                        ),
                    )
                })
                .collect::<Vec<_>>();
            let outputs = signature
                .outputs
                .iter()
                .map(|(name, kind)| format!("{name}: {kind:?}"))
                .collect::<Vec<_>>()
                .join(", ");
            widget(Stack::column(0.).gap_step(gap::step::TIGHT))
                .children((
                    fields,
                    rows::row(
                        ui.now_t("pkgedit-pipeline-outputs"),
                        widget(Text::new(outputs).style(Type::Small.mono())),
                    ),
                ))
                .into_any()
        }
    });
    rows::form(
        "package-editor-pipeline-form",
        (
            heading(
                ui,
                previous
                    .clone()
                    .unwrap_or_else(|| ui.now_t("pkgedit-pipeline-add")),
                previous
                    .clone()
                    .filter(|_| editable)
                    .map(|name| Action::ApplyPipeline {
                        previous: Some(name),
                        entry: None,
                    }),
            ),
            (!editable).then(|| hint(ui.now_t("pkgedit-skin-protected"))),
            rows::group(
                ui.now_t("pkgedit-pipelines"),
                (),
                (
                    rows::row(
                        ui.now_t("pkgedit-pipeline-name"),
                        text_field(
                            "package-editor-field:pkgedit-pipeline-name".into(),
                            session,
                            format!("{scope}name"),
                            name_init,
                            ui.now_t("pkgedit-pipeline-name"),
                            name,
                            "stroke_map",
                            editable,
                            apply.clone(),
                        ),
                    ),
                    rows::row(
                        ui.now_t("pkgedit-pipeline-processor"),
                        widget(
                            Select::new(Some(selected))
                                .options(processors)
                                .size(ControlSize::Small)
                                .disabled(!editable),
                        )
                        .key("package-editor-pipeline-processor")
                        .on_cx({
                            let apply = apply.clone();
                            move |_, e: &SelectChanged, cx| {
                                if let Some((n, v)) = e.value.rsplit_once('@')
                                    && let Ok(v) = v.parse()
                                {
                                    let next_proc = (n.to_owned(), v);
                                    processor.set(next_proc.clone());
                                    cx.dispatch_program_all(buffer_msg(
                                        session,
                                        &processor_key_str,
                                        &next_proc,
                                        &processor_init,
                                    ));
                                    if let Some(msg) = apply() {
                                        cx.dispatch_program_all(msg);
                                    }
                                }
                            }
                        }),
                    ),
                ),
            ),
            rows::group(
                ui.now_t("pkgedit-pipeline-inputs-title"),
                kit::hint_mark(ui.now_t("pkgedit-pipeline-inputs")),
                ports,
            ),
            rows::group(
                ui.now_t("pkgedit-pipeline-params-title"),
                kit::hint_mark(ui.now_t("pkgedit-pipeline-params")),
                area(AreaForm {
                    key: "package-editor-pipeline-params",
                    session,
                    form_key: format!("{scope}params"),
                    initial: params_init,
                    label: ui.now_t("pkgedit-pipeline-params-title"),
                    value: params,
                    placeholder: "name = 1.0",
                    editable,
                    apply: apply.clone(),
                }),
            ),
            (new && editable).then(|| create(ui, "package-editor-pipeline-apply", write)),
        ),
    )
}
