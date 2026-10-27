//! Package information: the manifest form and the form helpers shared with
//! the data and pipeline declarations.
use crate::msg::Msg;
use crate::session::{Action, EditorView};
use crate::ui::Ui;
use crate::ui::controls::{confirm, hint};
use crate::ui::kit::{self, field, gap};
use crate::ui::views::*;
use nana_ui::runtime::view::{AnyView, IntoView, Signal, dynamic, widget};
use nana_ui::runtime::{
    Activate, Button, Checkbox, Select, SelectChanged, SelectOption, Stack, TextArea, TextChanged,
    ToggleChanged, ViewContext,
};
use nana_ui::theme::space;
use nana_ui::{ButtonKind, ControlSize};
use nana_ui_core::{AlignSpec, LengthSpec, RadiusTier, SemanticColorRole};
use nanatype_theme::editor::Metadata;
use nanatype_theme::{MANIFEST, PACKAGE_MANIFEST, PackageCapability};
use std::sync::Arc;

// ---- package metadata --------------------------------------------------------

pub fn view(ui: Ui) -> AnyView {
    dynamic(
        move || {
            ui.package_editor.with(|v| {
                v.as_ref().map(|v| {
                    (
                        v.metadata.clone(),
                        v.revision,
                        Arc::as_ptr(&v.files) as usize,
                    )
                })
            })
        },
        move |state| {
            let Some(view) = view_of(ui) else {
                return widget(Stack::column(0.)).into_any();
            };
            let Some(Some(metadata)) = state.clone().map(|s| s.0) else {
                return hint(ui.now_t("pkgedit-manifest-invalid"));
            };
            metadata_form(ui, metadata, &view)
        },
    )
    .into_any()
}

/// A control under its caption, up to `width` wide, never wider than the line.
pub(crate) fn labelled(caption: String, width: f32, control: impl IntoView) -> AnyView {
    widget(
        Stack::column(0.0)
            .gap_step(nana_ui_core::SpacingStep::Xxs)
            .width(LengthSpec::Px(width))
            .shrink(1.)
            .with_layout(|l| l.max_width = Some(LengthSpec::Percent(100.))),
    )
    .children((hint(caption), control))
    .into_any()
}

pub(crate) fn form_editable(ui: Ui, file: &str) -> bool {
    ui.package_editor.with_untracked(|v| {
        v.as_ref()
            .is_some_and(|v| v.files.iter().any(|f| f.path == file && f.editable))
    })
}

pub(crate) fn field(ui: Ui, key: &str, value: Signal<String>, placeholder: &str) -> AnyView {
    labelled(
        ui.now_t(key),
        field::MD,
        widget(
            kit::text_input(value.get_untracked())
                .placeholder(placeholder.to_owned())
                .read_only(!form_editable(
                    ui,
                    if key.contains("pipeline") {
                        MANIFEST
                    } else {
                        PACKAGE_MANIFEST
                    },
                ))
                .size(ControlSize::Small),
        )
        .key(format!("package-editor-field:{key}"))
        .on(move |e: &TextChanged| value.set(e.value.to_string())),
    )
}

fn metadata_form(ui: Ui, metadata: Metadata, view: &EditorView) -> AnyView {
    let editable = view
        .files
        .iter()
        .any(|f| f.path == PACKAGE_MANIFEST && f.editable);
    let name = buffer(ui, "metadata.name", metadata.name.clone());
    let description = buffer(ui, "metadata.description", metadata.description.clone());
    let min_app = buffer(ui, "metadata.min_app", metadata.min_app.clone());
    let base = buffer(ui, "metadata.base", metadata.base.clone());
    let preview = buffer(ui, "metadata.preview", metadata.preview.clone());
    let tags = buffer(ui, "metadata.tags", metadata.tags.join(", "));
    let localized = [("pkgedit-name", name), ("pkgedit-description", description)]
        .into_iter()
        .flat_map(|(key, values)| {
            ["zh-CN", "en-US", "ja-JP"].into_iter().map(move |locale| {
                labelled(
                    format!("{} · {locale}", ui.now_t(key)),
                    field::MD,
                    widget(
                        kit::text_input(
                            values.with_untracked(|v| v.get(locale).cloned().unwrap_or_default()),
                        )
                        .read_only(!editable)
                        .size(ControlSize::Small),
                    )
                    .key(format!("editor-meta-{key}-{locale}"))
                    .on(move |e: &TextChanged| {
                        values.update(|v| {
                            v.insert(locale.into(), e.value.to_string());
                        })
                    }),
                )
            })
        })
        .collect::<Vec<_>>();
    let version = buffer(ui, "metadata.version", metadata.version.clone());
    let author = buffer(ui, "metadata.author", metadata.author.clone());
    let created = buffer(ui, "metadata.created", metadata.created_at.clone());
    let licensed_to = buffer(ui, "metadata.licensed_to", metadata.licensed_to.clone());
    let license = buffer(ui, "metadata.license", metadata.license.clone());
    let capabilities = buffer(ui, "metadata.capabilities", metadata.capabilities.clone());
    let dependencies = buffer(
        ui,
        "metadata.dependencies",
        metadata
            .dependencies
            .iter()
            .map(|(id, v)| format!("{id}@{v}"))
            .collect::<Vec<_>>()
            .join("\n"),
    );
    let capability_box = |cap: PackageCapability, key: &'static str| {
        widget(
            Checkbox::new(
                ui.now_t(key),
                capabilities.with_untracked(|c| c.contains(&cap)),
            )
            .disabled(!editable),
        )
        .key(format!("package-editor-cap:{key}"))
        .bind(move |c: &mut Checkbox| c.checked = capabilities.with(|c| c.contains(&cap)))
        .on(move |e: &ToggleChanged| {
            capabilities.update(|c| {
                c.retain(|x| *x != cap);
                if e.checked {
                    c.push(cap);
                }
            })
        })
    };
    let apply = widget(button(ui, "pkgedit-apply").kind(ButtonKind::Primary))
        .key("package-editor-apply")
        .bind(move |b: &mut Button| b.disabled = !editable)
        .on_cx(move |_, _: &Activate, cx| {
            let deps =
                match nanatype_theme::editor::parse_dependencies(&dependencies.get_untracked()) {
                    Ok(deps) => deps,
                    Err(e) => {
                        cx.dispatch_program_all(notice(ui, "pkgedit-dependencies-hint", e));
                        return;
                    }
                };
            cx.dispatch_program_all(Msg::PackageEditor(Action::ApplyMetadata(Metadata {
                name: name.get_untracked(),
                description: description.get_untracked(),
                min_app: min_app.get_untracked(),
                base: base.get_untracked(),
                preview: preview.get_untracked(),
                tags: tags
                    .get_untracked()
                    .split(',')
                    .map(str::trim)
                    .filter(|v| !v.is_empty())
                    .map(str::to_owned)
                    .collect(),
                version: version.get_untracked().trim().to_owned(),
                author: author.get_untracked(),
                created_at: created.get_untracked().trim().to_owned(),
                licensed_to: licensed_to.get_untracked(),
                license: license.get_untracked(),
                capabilities: capabilities.get_untracked(),
                dependencies: deps,
            })));
        });
    widget(
        Stack::column(0.0)
            .gap_step(nana_ui_core::SpacingStep::Lg)
            .padding_xy(0., space::SM),
    )
    .key("package-editor-metadata")
    .children((
        (!editable).then(|| hint(ui.now_t("pkgedit-manifest-protected"))),
        widget(
            Stack::row(0.0)
                .gap_step(nana_ui_core::SpacingStep::Md)
                .wrap(true),
        )
        .children((
            field(ui, "pkgedit-version", version, "1.0.0"),
            field(ui, "package-author", author, ""),
            field(ui, "package-created-at", created, "YYYY-MM-DD"),
            field(ui, "package-licensed-to", licensed_to, ""),
            field(ui, "package-license", license, "MIT"),
        )),
        (
            widget(Stack::row(0.).gap_step(gap::step::CONTROL).wrap(true)).children(localized),
            widget(Stack::row(0.).gap_step(gap::step::CONTROL).wrap(true)).children((
                field(ui, "pkgedit-min-app", min_app, "0.1.0"),
                field(ui, "pkgedit-base", base, "nanatype.halo"),
                field(ui, "pkgedit-cover", preview, "images/cover.png"),
                field(ui, "pkgedit-tags", tags, "minimal, cute"),
            )),
        ),
        hint(ui.now_t("pkgedit-capabilities")),
        widget(
            Stack::row(0.0)
                .gap_step(nana_ui_core::SpacingStep::Lg)
                .wrap(true),
        )
        .children((
            capability_box(PackageCapability::Skin, "packages-kind-skin"),
            capability_box(PackageCapability::Resources, "packages-kind-resources"),
            capability_box(PackageCapability::Lua, "packages-kind-function"),
            capability_box(PackageCapability::Sounds, "packages-kind-sounds"),
        )),
        hint(ui.now_t("pkgedit-dependencies-hint")),
        widget(
            kit::text_area(dependencies.get_untracked())
                .read_only(!editable)
                .placeholder("nanatype.halo@1.0.0")
                .height(FORM_TEXT_H),
        )
        .key("package-editor-dependencies")
        .bind(move |a: &mut TextArea| sync_text(&mut a.state, dependencies.get()))
        .on(move |e: &TextChanged| dependencies.set(e.value.to_string())),
        dynamic(
            move || ui.packages.get(),
            move |packages| {
                widget(
                    Select::new(None::<String>)
                        .size(ControlSize::Small)
                        .options(
                            packages
                                .iter()
                                .map(|p| {
                                    let value = format!(
                                        "{}@{}",
                                        p.manifest.meta.id, p.manifest.meta.version
                                    );
                                    SelectOption::new(value.clone(), value)
                                })
                                .collect::<Vec<_>>(),
                        ),
                )
                .key("editor-dependency-picker")
                .bind(move |s: &mut Select| s.disabled = !editable)
                .on(move |e: &SelectChanged| {
                    let value = e.value.to_string();
                    let id = value.split('@').next().unwrap_or("");
                    dependencies.update(|text| {
                        let mut rows = text.lines().map(str::to_owned).collect::<Vec<_>>();
                        if let Some(row) = rows
                            .iter_mut()
                            .find(|r| r.split('@').next().map(str::trim) == Some(id))
                        {
                            *row = value.clone();
                        } else {
                            rows.push(value.clone());
                        }
                        *text = rows.join("\n");
                    });
                })
                .into_any()
            },
        ),
        dynamic(
            move || dependencies.get(),
            move |text| match nanatype_theme::editor::parse_dependencies(text) {
                Ok(_) => widget(Stack::column(0.)).into_any(),
                Err(e) => widget(readable(e).color(SemanticColorRole::Danger))
                    .key("package-editor-dependencies-error")
                    .into_any(),
            },
        ),
        (
            natural(apply),
            natural(
                widget(button(ui, "pkgedit-discard-form"))
                    .on_cx(send(|| Action::DiscardForms("metadata.".into()))),
            ),
        ),
    ))
    .into_any()
}

/// `None`: no form; `Some(None)`: a new entry; `Some(Some(name))`: editing it.
pub(crate) type Editing = Signal<Option<Option<String>>>;

/// One declaration, with `buttons` when it can be edited here.
pub(crate) fn entry_row(key: &str, summary: String, buttons: Option<AnyView>) -> AnyView {
    widget(
        Stack::column(0.0)
            .gap_step(nana_ui_core::SpacingStep::Xs)
            .padding_step(nana_ui_core::SpacingStep::Md, nana_ui_core::SpacingStep::Sm)
            .surface(SemanticColorRole::Subtle)
            .radius(RadiusTier::Sm),
    )
    .key(key.to_owned())
    .children((widget(readable(summary)), buttons))
    .into_any()
}

/// Edit opens the form for `name`; remove asks first.
pub(crate) fn entry_buttons(
    ui: Ui,
    key: &str,
    editing: Editing,
    name: String,
    remove: impl Fn() -> Action + Send + 'static,
) -> AnyView {
    let remove_key = format!("{key}:remove");
    widget(
        Stack::row(0.0)
            .gap_step(nana_ui_core::SpacingStep::Sm)
            .wrap(true)
            .align(AlignSpec::Center),
    )
    .children((
        widget(button(ui, "pkgedit-entry-edit"))
            .key(format!("{key}:edit"))
            .on(move |_: &Activate| editing.set(Some(Some(name.clone())))),
        confirm(
            *ui,
            ui.now_t("pkgedit-entry-remove-question"),
            ui.now_t("pkgedit-entry-remove"),
            move |armed| {
                widget(button(ui, "pkgedit-entry-remove"))
                    .key(remove_key)
                    .on(move |_: &Activate| armed.set(true))
                    .into_any()
            },
            move || Msg::PackageEditor(remove()),
        ),
    ))
    .into_any()
}

pub(crate) fn form_buttons(
    ui: Ui,
    key: &str,
    editing: Editing,
    apply: impl FnMut(&mut Button, &Activate, &mut ViewContext<'_, Button>) + Send + 'static,
) -> AnyView {
    widget(
        Stack::row(0.0)
            .gap_step(nana_ui_core::SpacingStep::Sm)
            .wrap(true),
    )
    .children((
        widget(button(ui, "pkgedit-apply").kind(ButtonKind::Primary))
            .key(format!("{key}-apply"))
            .on_cx(apply),
        widget(button(ui, "common-cancel"))
            .key(format!("{key}-cancel"))
            .on_cx({
                let prefix = if key.contains("pipeline") {
                    "pipeline."
                } else {
                    "data."
                };
                move |_, _: &Activate, cx| {
                    editing.set(None);
                    cx.dispatch_program_all(Msg::PackageEditor(Action::DiscardForms(
                        prefix.into(),
                    )));
                }
            }),
    ))
    .into_any()
}

pub(crate) fn form(key: &str, children: impl IntoView) -> AnyView {
    widget(
        Stack::column(0.0)
            .gap_step(nana_ui_core::SpacingStep::Md)
            .padding_step(nana_ui_core::SpacingStep::Md, nana_ui_core::SpacingStep::Md)
            .outline(SemanticColorRole::Border, 1.)
            .radius(RadiusTier::Sm),
    )
    .key(key.to_owned())
    .children(children)
    .into_any()
}

pub(crate) fn notice(ui: Ui, key: &str, detail: impl std::fmt::Display) -> Msg {
    Msg::Notice {
        text: format!("{}: {detail}", ui.now_t(key)),
        error: true,
    }
}
