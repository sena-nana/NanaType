//! Package information: `package.toml` as one dense form. A field is written
//! when Enter is pressed or focus leaves it; a choice is written as it is made.
use crate::msg::Msg;
use crate::session::{Action, EditorView};
use crate::ui::Ui;
use crate::ui::controls::hint;
use crate::ui::kit::{self, Type, gap};
use crate::ui::rows;
use crate::ui::shell::tool;
use crate::ui::views::*;
use nana_ui::runtime::view::{AnyView, IntoView, Signal, dynamic, signal, widget};
use nana_ui::runtime::{
    Activate, Checkbox, Select, SelectChanged, SelectOption, Stack, Text, TextChanged,
    TextSubmitted, ToggleChanged,
};
use nana_ui::{ControlSize, icons_tabler as ti};
use nana_ui_core::{AlignSpec, LengthSpec};
use nanatype_theme::editor::{AssetKind, Metadata};
use nanatype_theme::{PACKAGE_MANIFEST, PackageCapability};
use std::collections::BTreeMap;
use std::sync::Arc;

/// What a form writes, or nothing when it would change nothing.
pub(crate) type Apply = Arc<dyn Fn() -> Option<Msg> + Send + Sync>;

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
                return rows::form(
                    "package-editor-metadata",
                    hint(ui.now_t("pkgedit-manifest-invalid")),
                );
            };
            metadata_form(ui, metadata, &view)
        },
    )
    .into_any()
}

pub(crate) fn form_editable(ui: Ui, file: &str) -> bool {
    ui.package_editor.with_untracked(|v| {
        v.as_ref()
            .is_some_and(|v| v.files.iter().any(|f| f.path == file && f.editable))
    })
}

/// A one-line field of a form: typing stays in `value`; Enter and leaving
/// the field send what `apply` makes of the form.
pub(crate) fn text_field(
    key: String,
    label: String,
    value: Signal<String>,
    placeholder: &str,
    editable: bool,
    apply: Apply,
) -> AnyView {
    let leave = apply.clone();
    on_blur(
        widget({
            let mut input = kit::text_input(value.get_untracked())
                .placeholder(placeholder.to_owned())
                .label(label)
                .read_only(!editable)
                .size(ControlSize::Small);
            let l = Arc::make_mut(&mut input.style.layout);
            l.width = Some(LengthSpec::Fill);
            l.min_width = Some(LengthSpec::Px(0.));
            input
        })
        .key(key)
        .bind(move |a: &mut nana_ui::runtime::TextInput| sync_text(&mut a.state, value.get()))
        .on(move |e: &TextChanged| value.set(e.value.to_string()))
        .on_cx(move |_, _: &TextSubmitted, cx| {
            if let Some(msg) = apply() {
                cx.dispatch_program_all(msg);
            }
        }),
        move || leave(),
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
    let tags = buffer(ui, "metadata.tags", metadata.tags.join(", "));
    let version = buffer(ui, "metadata.version", metadata.version.clone());
    let author = buffer(ui, "metadata.author", metadata.author.clone());
    let created = buffer(ui, "metadata.created", metadata.created_at.clone());
    let licensed_to = buffer(ui, "metadata.licensed_to", metadata.licensed_to.clone());
    let license = buffer(ui, "metadata.license", metadata.license.clone());
    // Choices are written as they are made; one that is refused stays
    // pending with the typed fields.
    let preview = buffer(ui, "metadata.preview", metadata.preview.clone());
    let capabilities = buffer(ui, "metadata.capabilities", metadata.capabilities.clone());
    let dependencies = buffer(ui, "metadata.dependency_map", metadata.dependencies.clone());
    // The last form sent, so Enter followed by leaving the field writes once.
    let sent = signal(None::<Metadata>);
    let current = metadata.clone();
    let apply: Apply = Arc::new(move || {
        let next = Metadata {
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
            dependencies: dependencies.get_untracked(),
        };
        if next == current || sent.with_untracked(|s| s.as_ref() == Some(&next)) {
            return None;
        }
        sent.set(Some(next.clone()));
        Some(Msg::PackageEditor(Action::ApplyMetadata(next)))
    });
    let text_row = |key: &str, value: Signal<String>, placeholder: &str| {
        rows::row(
            ui.now_t(key),
            text_field(
                format!("package-editor-field:{key}"),
                ui.now_t(key),
                value,
                placeholder,
                editable,
                apply.clone(),
            ),
        )
    };
    // One row per language, under the name's and the description's captions.
    let localized = |key: &'static str, values: Signal<BTreeMap<String, String>>| {
        ["zh-CN", "en-US", "ja-JP"]
            .into_iter()
            .map(|locale| {
                let (enter, leave) = (apply.clone(), apply.clone());
                rows::row(
                    locale.to_owned(),
                    on_blur(
                        widget({
                            let mut input =
                                kit::text_input(values.with_untracked(|v| {
                                    v.get(locale).cloned().unwrap_or_default()
                                }))
                                .label(format!("{} {locale}", ui.now_t(key)))
                                .read_only(!editable)
                                .size(ControlSize::Small);
                            Arc::make_mut(&mut input.style.layout).width = Some(LengthSpec::Fill);
                            input
                        })
                        .key(format!("editor-meta-{key}-{locale}"))
                        .on(move |e: &TextChanged| {
                            values.update(|v| {
                                v.insert(locale.into(), e.value.to_string());
                            })
                        })
                        .on_cx(move |_, _: &TextSubmitted, cx| {
                            if let Some(msg) = enter() {
                                cx.dispatch_program_all(msg);
                            }
                        }),
                        move || leave(),
                    ),
                )
            })
            .collect::<Vec<_>>()
    };
    let capability_box = |cap: PackageCapability, key: &'static str| {
        let apply = apply.clone();
        widget(
            Checkbox::new(
                ui.now_t(key),
                capabilities.with_untracked(|c| c.contains(&cap)),
            )
            .disabled(!editable),
        )
        .key(format!("package-editor-cap:{key}"))
        .bind(move |c: &mut Checkbox| c.checked = capabilities.with(|c| c.contains(&cap)))
        .on_cx(move |_, e: &ToggleChanged, cx| {
            capabilities.update(|c| {
                c.retain(|x| *x != cap);
                if e.checked {
                    c.push(cap);
                }
            });
            if let Some(msg) = apply() {
                cx.dispatch_program_all(msg);
            }
        })
    };
    // The cover is one of the package's own images.
    let mut covers = vec![SelectOption::new("", ui.now_t("pkgedit-cover-none"))];
    covers.extend(
        view.files
            .iter()
            .filter(|f| f.kind == AssetKind::Image && f.dependency.is_none())
            .map(|f| SelectOption::new(f.path.clone(), f.path.clone())),
    );
    if !metadata.preview.is_empty() && !covers.iter().any(|o| *o.value == *metadata.preview) {
        // A path that names no image stays visible: the form shows the truth.
        covers.push(SelectOption::new(
            metadata.preview.clone(),
            metadata.preview.clone(),
        ));
    }
    let cover = widget(
        Select::new(Some(metadata.preview.clone()))
            .options(covers)
            .size(ControlSize::Small),
    )
    .key("package-editor-cover")
    .bind(move |s: &mut Select| s.disabled = !editable)
    .on_cx({
        let apply = apply.clone();
        move |_, e: &SelectChanged, cx| {
            preview.set(e.value.to_string());
            if let Some(msg) = apply() {
                cx.dispatch_program_all(msg);
            }
        }
    });
    let dependency_rows = metadata
        .dependencies
        .iter()
        .map(|(id, version)| {
            let remove = id.clone();
            let apply = apply.clone();
            rows::item(
                None,
                widget(Text::new(format!("{id}@{version}")).style(Type::Small.mono())),
                editable.then(|| {
                    widget(tool(ti::X, ui.now_t("pkgedit-entry-remove")))
                        .key(node_key("package-editor-dependency-remove", id))
                        .on_cx(move |_, _: &Activate, cx| {
                            dependencies.update(|d| {
                                d.remove(&remove);
                            });
                            if let Some(msg) = apply() {
                                cx.dispatch_program_all(msg);
                            }
                        })
                }),
            )
        })
        .collect::<Vec<_>>();
    let picker = dynamic(move || ui.packages.get(), {
        let apply = apply.clone();
        move |packages: &Arc<Vec<nanatype_frame::packages::PackageRow>>| {
            let apply = apply.clone();
            widget(
                Select::new(None::<String>)
                    .size(ControlSize::Small)
                    .placeholder(ui.now_t("pkgedit-dependency-add"))
                    .options(
                        packages
                            .iter()
                            .map(|p| {
                                let value =
                                    format!("{}@{}", p.manifest.meta.id, p.manifest.meta.version);
                                SelectOption::new(value.clone(), value)
                            })
                            .collect::<Vec<_>>(),
                    ),
            )
            .key("editor-dependency-picker")
            .bind(move |s: &mut Select| s.disabled = !editable)
            .on_cx(move |_, e: &SelectChanged, cx| {
                if let Some((id, version)) = e.value.split_once('@') {
                    dependencies.update(|d| {
                        d.insert(id.to_owned(), version.to_owned());
                    });
                    if let Some(msg) = apply() {
                        cx.dispatch_program_all(msg);
                    }
                }
            })
            .into_any()
        }
    });
    rows::form(
        "package-editor-metadata",
        (
            (!editable).then(|| hint(ui.now_t("pkgedit-manifest-protected"))),
            rows::group(
                ui.now_t("pkgedit-group-basic"),
                (),
                (
                    rows::fact_mono(ui.now_t("pkgedit-id"), view.package.clone()),
                    text_row("pkgedit-version", version, "1.0.0"),
                    text_row("package-author", author, ""),
                    text_row("package-created-at", created, "YYYY-MM-DD"),
                    text_row("package-licensed-to", licensed_to, ""),
                    text_row("package-license", license, "MIT"),
                ),
            ),
            rows::group(
                ui.now_t("pkgedit-name"),
                (),
                localized("pkgedit-name", name),
            ),
            rows::group(
                ui.now_t("pkgedit-description"),
                (),
                localized("pkgedit-description", description),
            ),
            rows::group(
                ui.now_t("pkgedit-group-listing"),
                (),
                (
                    text_row("pkgedit-min-app", min_app, "0.1.0"),
                    text_row("pkgedit-base", base, "nanatype.halo"),
                    rows::row(ui.now_t("pkgedit-cover"), cover),
                    text_row("pkgedit-tags", tags, "minimal, cute"),
                ),
            ),
            rows::group(
                ui.now_t("pkgedit-capabilities-title"),
                kit::hint_mark(ui.now_t("pkgedit-capabilities")),
                widget(
                    Stack::row(0.)
                        .gap_step(gap::step::GROUP)
                        .wrap(true)
                        .align(AlignSpec::Center),
                )
                .children((
                    capability_box(PackageCapability::Skin, "packages-kind-skin"),
                    capability_box(PackageCapability::Resources, "packages-kind-resources"),
                    capability_box(PackageCapability::Lua, "packages-kind-function"),
                    capability_box(PackageCapability::Sounds, "packages-kind-sounds"),
                )),
            ),
            rows::group(
                ui.now_t("pkgedit-dependencies-title"),
                kit::hint_mark(ui.now_t("pkgedit-dependencies-hint")),
                (
                    widget(Stack::column(0.).gap_step(gap::step::HAIR))
                        .key("package-editor-dependencies")
                        .children(dependency_rows),
                    rows::row(ui.now_t("pkgedit-dependency-add"), picker),
                ),
            ),
        ),
    )
}

pub(crate) fn notice(ui: Ui, key: &str, detail: impl std::fmt::Display) -> Msg {
    Msg::Notice {
        text: format!("{}: {detail}", ui.now_t(key)),
        error: true,
    }
}
