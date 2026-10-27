//! Data declarations (`[data.<id>]`) and processing pipelines.
use super::package::*;
use crate::msg::Msg;
use crate::session::{Action, EditorView};
use crate::ui::Ui;
use crate::ui::controls::{hint, label};
use crate::ui::kit::{self, field, gap};
use crate::ui::views::*;
use nana_ui::ControlSize;
use nana_ui::runtime::view::{AnyView, IntoView, dynamic, widget};
use nana_ui::runtime::{
    Activate, Button, Select, SelectChanged, SelectOption, Stack, TextChanged, ViewContext,
};
use nanatype_theme::{DataFormat, DataSpec, MANIFEST, PACKAGE_MANIFEST, PipelineSpec, PortSource};
use std::collections::BTreeMap;
use std::sync::Arc;

// ---- data declarations -------------------------------------------------------

const FORMATS: [DataFormat; 3] = [DataFormat::Text, DataFormat::Json, DataFormat::Nthz];

/// `[data.<id>]` rows and their form; read-only while package.toml is.
fn data_section(ui: Ui, view: &EditorView, editable: bool) -> AnyView {
    let editing: Editing = buffer(ui, "data.editing", None);
    let rows = view
        .data
        .iter()
        .map(|(id, spec)| {
            let bytes = view.files.iter().find(|f| f.path == spec.path).map_or_else(
                || ui.now_t("pkgedit-data-missing"),
                |f| format!("{} B", f.bytes),
            );
            let problem = view
                .report
                .diagnostics
                .iter()
                .find(|d| {
                    d.message.starts_with(&format!("data.{id}"))
                        || d.message.contains(&format!("{id}: "))
                })
                .map(|d| format!("\n✕ {}", d.message))
                .unwrap_or_default();
            let constraints = spec
                .constraints
                .as_ref()
                .and_then(|c| serde_json::to_string(c).ok())
                .map(|c| format!("\n{}: {c}", ui.now_t("pkgedit-data-constraints")))
                .unwrap_or_default();
            let summary = format!(
                "{id} · {} · {} v{} · {bytes}\n{}: {}\n{}: {}{constraints}{problem}",
                spec.path,
                spec.format.name(),
                spec.format_version,
                ui.now_t("package-data-source"),
                spec.source,
                ui.now_t("package-data-license"),
                spec.license,
            );
            let key = format!("package-editor-data:{id}");
            let remove = id.clone();
            let buttons = editable.then(|| {
                entry_buttons(ui, &key, editing, id.clone(), move || Action::ApplyData {
                    previous: Some(remove.clone()),
                    entry: None,
                })
            });
            entry_row(&key, summary, buttons)
        })
        .collect::<Vec<_>>();
    let declared = view.data.clone();
    let form = dynamic(
        move || editing.get(),
        move |open| match open {
            Some(previous) => data_form(ui, previous.clone(), &declared, editing),
            None => widget(Stack::column(0.)).into_any(),
        },
    );
    widget(Stack::column(0.0).gap_step(nana_ui_core::SpacingStep::Sm))
        .key("package-editor-data")
        .children((
            rows.is_empty().then(|| hint(ui.now_t("pkgedit-data-none"))),
            widget(Stack::column(0.0).gap_step(nana_ui_core::SpacingStep::Sm)).children(rows),
            form,
            editable.then(|| {
                natural(
                    widget(button(ui, "pkgedit-data-add"))
                        .key("package-editor-data-add")
                        .bind(move |b: &mut Button| b.disabled = editing.get().is_some())
                        .on(move |_: &Activate| editing.set(Some(None))),
                )
            }),
        ))
        .into_any()
}

fn data_form(
    ui: Ui,
    previous: Option<String>,
    declared: &BTreeMap<String, DataSpec>,
    editing: Editing,
) -> AnyView {
    let scope = format!("data.{}.", previous.as_deref().unwrap_or("@new"));
    let current = previous.as_ref().and_then(|id| declared.get(id)).cloned();
    let id = buffer(
        ui,
        format!("{scope}id"),
        previous.clone().unwrap_or_default(),
    );
    let path = buffer(
        ui,
        format!("{scope}path"),
        current.as_ref().map(|c| c.path.clone()).unwrap_or_default(),
    );
    let format = buffer(
        ui,
        format!("{scope}format"),
        current.as_ref().map_or(DataFormat::Text, |c| c.format),
    );
    let version = buffer(
        ui,
        format!("{scope}version"),
        current.as_ref().map_or(1, |c| c.format_version).to_string(),
    );
    let source = buffer(
        ui,
        format!("{scope}source"),
        current
            .as_ref()
            .map(|c| c.source.clone())
            .unwrap_or_default(),
    );
    let license = buffer(
        ui,
        format!("{scope}license"),
        current
            .as_ref()
            .map(|c| c.license.clone())
            .unwrap_or_default(),
    );
    let constraints = buffer(
        ui,
        format!("{scope}constraints"),
        current
            .as_ref()
            .and_then(|c| c.constraints.as_ref())
            .and_then(|c| serde_json::to_string(c).ok())
            .unwrap_or_default(),
    );
    let formats = FORMATS
        .iter()
        .map(|f| SelectOption::new(f.name(), f.name()))
        .collect::<Vec<_>>();
    let apply = move |_: &mut Button, _: &Activate, cx: &mut ViewContext<'_, Button>| {
        let text = constraints.get_untracked();
        let constraints = if text.trim().is_empty() {
            Ok(None)
        } else {
            serde_json::from_str::<nanatype_theme::JsonConstraints>(&text)
                .map(Some)
                .map_err(|e| e.to_string())
        };
        let version = version.get_untracked().trim().parse::<u32>();
        let msg = match (constraints, version) {
            (Ok(constraints), Ok(format_version)) => Msg::PackageEditor(Action::ApplyData {
                previous: previous.clone(),
                entry: Some((
                    id.get_untracked().trim().to_owned(),
                    DataSpec {
                        path: path.get_untracked().trim().to_owned(),
                        format: format.get_untracked(),
                        format_version,
                        source: source.get_untracked().trim().to_owned(),
                        license: license.get_untracked().trim().to_owned(),
                        constraints,
                    },
                )),
            }),
            (Err(e), _) => notice(ui, "pkgedit-data-constraints", e),
            (_, Err(e)) => notice(ui, "pkgedit-data-version", e),
        };
        cx.dispatch_program_all(msg);
    };
    form(
        "package-editor-data-form",
        (
            widget(
                Stack::row(0.0)
                    .gap_step(nana_ui_core::SpacingStep::Md)
                    .wrap(true),
            )
            .children((
                field(ui, "pkgedit-data-id", id, "notes"),
                field(ui, "pkgedit-data-path", path, "data/notes.txt"),
                labelled(
                    ui.now_t("pkgedit-data-format"),
                    field::SM,
                    widget(
                        Select::new(Some(format.get_untracked().name()))
                            .options(formats)
                            .size(ControlSize::Small),
                    )
                    .key("package-editor-data-format")
                    .on(move |e: &SelectChanged| {
                        if let Some(f) = FORMATS.into_iter().find(|f| f.name() == &*e.value) {
                            format.set(f);
                        }
                    }),
                ),
                field(ui, "pkgedit-data-version", version, "1"),
                field(ui, "package-data-source", source, ""),
                field(ui, "package-data-license", license, "CC0-1.0"),
            )),
            hint(ui.now_t("pkgedit-data-constraints-hint")),
            widget(
                kit::text_area(constraints.get_untracked())
                    .read_only(!form_editable(ui, PACKAGE_MANIFEST))
                    .placeholder("{\"type\": \"object\", \"required\": [\"items\"]}")
                    .label(ui.now_t("pkgedit-data-constraints"))
                    .height(NOTE_H),
            )
            .key("package-editor-data-constraints")
            .on(move |e: &TextChanged| constraints.set(e.value.to_string())),
            form_buttons(ui, "package-editor-data", editing, apply),
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

/// `[pipelines.<name>]` rows and their form; read-only while skin.toml is.
fn pipeline_section(ui: Ui, view: &EditorView) -> AnyView {
    let skin = view.files.iter().find(|f| f.path == MANIFEST);
    let editable = skin.is_some_and(|f| f.editable);
    let editing: Editing = buffer(ui, "pipeline.editing", None);
    let rows = view
        .pipelines
        .iter()
        .map(|(name, spec)| {
            let key = format!("package-editor-pipeline:{name}");
            match spec {
                Ok(spec) => {
                    let inputs = spec
                        .inputs
                        .iter()
                        .map(|(input, source)| format!("{input} = {}", port_text(source)))
                        .chain(spec.params.iter().map(|(p, v)| format!("{p} = {v}")))
                        .collect::<Vec<_>>()
                        .join(", ");
                    let remove = name.clone();
                    let buttons = editable.then(|| {
                        entry_buttons(ui, &key, editing, name.clone(), move || {
                            Action::ApplyPipeline {
                                previous: Some(remove.clone()),
                                entry: None,
                            }
                        })
                    });
                    let mut summary = format!(
                        "{name} · {}\n{inputs}",
                        processor_key(&spec.processor, spec.version)
                    );
                    if let Some(signature) =
                        nanatype_theme::processor(&spec.processor, spec.version)
                    {
                        let outputs = signature
                            .outputs
                            .iter()
                            .map(|(name, kind)| format!("{name}: {kind:?}"))
                            .collect::<Vec<_>>()
                            .join(", ");
                        summary.push_str(&format!(
                            "\n{}: {outputs}",
                            ui.now_t("pkgedit-pipeline-outputs")
                        ));
                    }
                    entry_row(&key, summary, buttons)
                }
                Err(e) => entry_row(
                    &key,
                    format!("{name} · ✕ {e}\n{}", ui.now_t("pkgedit-pipeline-broken")),
                    None,
                ),
            }
        })
        .collect::<Vec<_>>();
    let specs: BTreeMap<String, PipelineSpec> = view
        .pipelines
        .iter()
        .filter_map(|(n, p)| Some((n.clone(), p.clone().ok()?)))
        .collect();
    let form = dynamic(
        move || editing.get(),
        move |open| match open {
            Some(previous) => pipeline_form(ui, previous.clone(), &specs, editing),
            None => widget(Stack::column(0.)).into_any(),
        },
    );
    let note = match skin {
        None => Some("pkgedit-pipelines-no-skin"),
        Some(_) if !editable => Some("pkgedit-skin-protected"),
        Some(_) => None,
    };
    widget(Stack::column(0.0).gap_step(nana_ui_core::SpacingStep::Sm))
        .key("package-editor-pipelines")
        .children((
            note.map(|key| hint(ui.now_t(key))),
            (rows.is_empty() && skin.is_some()).then(|| hint(ui.now_t("pkgedit-pipelines-none"))),
            widget(Stack::column(0.0).gap_step(nana_ui_core::SpacingStep::Sm)).children(rows),
            form,
            editable.then(|| {
                natural(
                    widget(button(ui, "pkgedit-pipeline-add"))
                        .key("package-editor-pipeline-add")
                        .bind(move |b: &mut Button| b.disabled = editing.get().is_some())
                        .on(move |_: &Activate| editing.set(Some(None))),
                )
            }),
        ))
        .into_any()
}

fn pipeline_form(
    ui: Ui,
    previous: Option<String>,
    specs: &BTreeMap<String, PipelineSpec>,
    editing: Editing,
) -> AnyView {
    let scope = format!("pipeline.{}.", previous.as_deref().unwrap_or("@new"));
    let current = previous.as_ref().and_then(|n| specs.get(n)).cloned();
    let first = &nanatype_theme::PROCESSORS[0];
    let name = buffer(
        ui,
        format!("{scope}name"),
        previous.clone().unwrap_or_default(),
    );
    let processor = buffer(
        ui,
        format!("{scope}processor"),
        current.as_ref().map_or_else(
            || (first.name.to_owned(), first.version),
            |c| (c.processor.clone(), c.version),
        ),
    );
    let inputs = buffer(
        ui,
        format!("{scope}inputs"),
        current
            .as_ref()
            .map(|c| {
                c.inputs
                    .iter()
                    .map(|(k, v)| (k.clone(), port_text(v)))
                    .collect::<BTreeMap<_, _>>()
            })
            .unwrap_or_default(),
    );
    let params = buffer(
        ui,
        format!("{scope}params"),
        current
            .as_ref()
            .map(|c| {
                c.params
                    .iter()
                    .map(|(k, v)| format!("{k} = {v}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default(),
    );
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
    let ports = dynamic(
        move || processor.get(),
        move |(name, version)| {
            let Some(signature) = nanatype_theme::processor(name, *version) else {
                return hint(ui.now_t("pkgedit-pipeline-unknown"));
            };
            let fields = signature
                .inputs
                .iter()
                .map(|(input, _)| {
                    let input = (*input).to_owned();
                    let value =
                        inputs.with_untracked(|m| m.get(&input).cloned().unwrap_or_default());
                    labelled(
                        input.clone(),
                        field::MD,
                        widget(
                            kit::text_input(value)
                                .placeholder("strokes")
                                .size(ControlSize::Small),
                        )
                        .key(format!("package-editor-pipeline-input:{input}"))
                        .on(move |e: &TextChanged| {
                            inputs.update(|m| {
                                m.insert(input.clone(), e.value.to_string());
                            })
                        }),
                    )
                })
                .collect::<Vec<_>>();
            widget(
                Stack::row(0.0)
                    .gap_step(nana_ui_core::SpacingStep::Md)
                    .wrap(true),
            )
            .children(fields)
            .into_any()
        },
    );
    let apply = move |_: &mut Button, _: &Activate, cx: &mut ViewContext<'_, Button>| {
        let (processor, version) = processor.get_untracked();
        let declared = nanatype_theme::processor(&processor, version);
        let params = params
            .get_untracked()
            .lines()
            .filter(|l| !l.trim().is_empty())
            .map(|l| {
                let (k, v) = l.split_once('=').ok_or_else(|| l.trim().to_owned())?;
                let v = v.trim().parse::<f32>().map_err(|e| e.to_string())?;
                Ok((k.trim().to_owned(), v))
            })
            .collect::<Result<BTreeMap<_, _>, String>>();
        let msg = match params {
            Ok(params) => Msg::PackageEditor(Action::ApplyPipeline {
                previous: previous.clone(),
                entry: Some((
                    name.get_untracked().trim().to_owned(),
                    PipelineSpec {
                        inputs: inputs.with_untracked(|m| {
                            m.iter()
                                // Only the selected processor's ports; empty ones stay unbound.
                                .filter(|(k, v)| {
                                    !v.trim().is_empty()
                                        && declared
                                            .is_none_or(|s| s.inputs.iter().any(|(n, _)| n == k))
                                })
                                .map(|(k, v)| (k.clone(), parse_port(v)))
                                .collect()
                        }),
                        processor,
                        version,
                        params,
                    },
                )),
            }),
            Err(e) => notice(ui, "pkgedit-pipeline-params", e),
        };
        cx.dispatch_program_all(msg);
    };
    form(
        "package-editor-pipeline-form",
        (
            widget(
                Stack::row(0.0)
                    .gap_step(nana_ui_core::SpacingStep::Md)
                    .wrap(true),
            )
            .children((
                field(ui, "pkgedit-pipeline-name", name, "stroke_map"),
                labelled(
                    ui.now_t("pkgedit-pipeline-processor"),
                    field::MD,
                    widget(
                        Select::new(Some(selected))
                            .options(processors)
                            .size(ControlSize::Small),
                    )
                    .key("package-editor-pipeline-processor")
                    .on(move |e: &SelectChanged| {
                        if let Some((n, v)) = e.value.rsplit_once('@')
                            && let Ok(v) = v.parse()
                        {
                            processor.set((n.to_owned(), v));
                        }
                    }),
                ),
            )),
            hint(ui.now_t("pkgedit-pipeline-inputs")),
            ports,
            hint(ui.now_t("pkgedit-pipeline-params")),
            widget(
                kit::text_area(params.get_untracked())
                    .read_only(!form_editable(ui, MANIFEST))
                    .placeholder("name = 1.0")
                    .label(ui.now_t("pkgedit-pipeline-params"))
                    .height(NOTE_H),
            )
            .key("package-editor-pipeline-params")
            .on(move |e: &TextChanged| params.set(e.value.to_string())),
            form_buttons(ui, "package-editor-pipeline", editing, apply),
        ),
    )
}

/// Data declarations, then pipelines: what the package reads and how it
/// processes it.
pub fn view(ui: Ui) -> AnyView {
    dynamic(
        move || {
            ui.package_editor.with(|v| {
                v.as_ref().map(|v| {
                    (
                        v.metadata.clone(),
                        v.revision,
                        Arc::as_ptr(&v.files) as usize,
                        Arc::as_ptr(&v.report) as usize,
                    )
                })
            })
        },
        move |_| {
            let Some(view) = view_of(ui) else {
                return widget(Stack::column(0.)).into_any();
            };
            let editable = view
                .files
                .iter()
                .any(|f| f.path == PACKAGE_MANIFEST && f.editable);
            widget(Stack::column(0.).gap_step(gap::step::CONTROL))
                .key("package-editor-data-page")
                .children((
                    (!editable).then(|| hint(ui.now_t("pkgedit-manifest-protected"))),
                    label(ui.now_t("pkgedit-data")),
                    data_section(ui, &view, editable),
                    label(ui.now_t("pkgedit-pipelines")),
                    pipeline_section(ui, &view),
                ))
                .into_any()
        },
    )
    .into_any()
}
