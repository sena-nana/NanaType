//! The files workspace: the package tree, tabbed sources and asset previews.
use crate::msg::{FileTarget, Msg};
use crate::session::{Action, EditorView};
use crate::ui::Ui;
use crate::ui::controls::{confirm, hint, label};
use crate::ui::kit::{self, Type, field, gap};
use crate::ui::shell::{panel_header, tool};
use crate::ui::views::*;
use nana_ui::runtime::IconButton;
use nana_ui::runtime::view::{AnyView, IntoView, dynamic, signal, widget};
use nana_ui::runtime::{
    Activate, CodeEditing, ScrollAxes, ScrollView, Stack, Text, TextArea, TextChanged,
    TextDiagnosticSeverity, TextDiagnosticSpan, TextMatchSpan, TextSelection, TreeSelectionChanged,
    TreeView,
};
use nana_ui::{ButtonKind, ControlSize, icons_tabler as ti};
use nana_ui_core::{AlignSpec, LengthSpec, RadiusTier, SemanticColorRole, TreeNode};
use nanatype_theme::PACKAGE_MANIFEST;
use nanatype_theme::editor::{AssetKind, FileInfo, Level};
use std::collections::BTreeMap;
use std::sync::Arc;

// ---- files -----------------------------------------------------------------

fn file_icon(file: &FileInfo) -> nana_ui::runtime::Icon {
    if file.protected {
        return ti::LOCK;
    }
    match file.kind {
        AssetKind::Image => ti::PHOTO,
        AssetKind::Audio => ti::MUSIC,
        AssetKind::Font => ti::TYPOGRAPHY,
        AssetKind::Data(_) => ti::DATABASE,
        AssetKind::Lua | AssetKind::Shader => ti::FILE_CODE,
        AssetKind::Manifest | AssetKind::Integrity => ti::PACKAGE,
        _ => ti::FILE,
    }
}

/// Nest `a/b/c` paths into folders; `dependencies/` starts collapsed.
fn tree(files: &[FileInfo], selected: Option<&str>) -> Vec<TreeNode<Arc<str>>> {
    #[derive(Default)]
    struct Folder<'a> {
        folders: BTreeMap<&'a str, Folder<'a>>,
        files: Vec<&'a FileInfo>,
    }
    let mut root = Folder::default();
    for file in files {
        let mut folder = &mut root;
        let parts: Vec<&str> = file.path.split('/').collect();
        for part in &parts[..parts.len() - 1] {
            folder = folder.folders.entry(part).or_default();
        }
        folder.files.push(file);
    }
    fn build(prefix: &str, folder: &Folder<'_>, selected: Option<&str>) -> Vec<TreeNode<Arc<str>>> {
        let mut nodes = Vec::new();
        for (name, sub) in &folder.folders {
            let path = format!("{prefix}{name}/");
            let expanded = path != "dependencies/";
            nodes.push(
                TreeNode::branch(
                    Arc::from(path.as_str()),
                    *name,
                    expanded,
                    build(&path, sub, selected),
                )
                .icon(ti::FOLDER),
            );
        }
        for file in &folder.files {
            let name = file.path.rsplit('/').next().unwrap_or(&file.path);
            nodes.push(
                TreeNode::leaf(Arc::from(file.path.as_str()), name)
                    .icon(file_icon(file))
                    .selected(selected == Some(file.path.as_str())),
            );
        }
        nodes
    }
    build("", &root, selected)
}

/// The package's files, with import and remove at the top and the path
/// operations (new, move, rename, replace) below.
pub fn side(ui: Ui) -> AnyView {
    let operations = signal(false);
    widget(Stack::fill_column(0.))
        .key("package-editor-files")
        .children((
            dynamic(
                move || removable(ui),
                move |path| {
                    panel_header(
                        ui.now_t("pkgedit-tab-files"),
                        (
                            widget(tool(ti::UPLOAD, ui.now_t("pkgedit-import")))
                                .key("package-editor-import")
                                .on_cx(|_, _: &Activate, cx| {
                                    cx.dispatch_program_all(Msg::PickFile(
                                        FileTarget::ImportProjectAsset,
                                    ))
                                }),
                            widget(tool(ti::PENCIL, ui.now_t("pkgedit-file-operations")))
                                .key("package-editor-operations-toggle")
                                .bind(move |b: &mut IconButton| b.selected = operations.get())
                                .on(move |_: &Activate| operations.update(|o| *o = !*o)),
                            path.clone().map(|path| remove_button(ui, path)),
                        ),
                    )
                },
            ),
            dynamic(
                move || operations.get(),
                move |open| {
                    if !*open {
                        return widget(Stack::column(0.)).into_any();
                    }
                    widget(
                        Stack::column(0.)
                            .gap_step(nana_ui_core::SpacingStep::Sm)
                            .padding_xy(gap::CONTROL, gap::TIGHT),
                    )
                    .children(crate::ui::assets::operations(ui))
                    .into_any()
                },
            ),
            widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|l| {
                l.height = Some(LengthSpec::Px(0.));
                l.flex_grow = Some(1.);
                l.min_height = Some(LengthSpec::Px(0.));
            }))
            .children(
                widget(Stack::column(0.).padding_xy(gap::TIGHT, 0.)).children(file_tree(
                    ui,
                    |_| true,
                    "package-editor-tree",
                )),
            ),
        ))
        .into_any()
}

/// The selected file: its path bar, find and replace, and the source or
/// the binary file's facts.
pub fn center(ui: Ui) -> AnyView {
    let search: Search = (ui.search_query, ui.search_index);
    widget(
        Stack::fill_column(0.)
            .gap_step(nana_ui_core::SpacingStep::Sm)
            .padding_xy(gap::CONTROL, gap::TIGHT),
    )
    .key("package-editor-source-pane")
    .children((toolbar(ui, search), source(ui, search)))
    .into_any()
}

/// The live stage beside the source.
pub fn preview(ui: Ui) -> AnyView {
    widget(Stack::fill_column(0.))
        .key("package-editor-files-preview")
        .children((
            panel_header(ui.now_t("pkgedit-tab-preview"), ()),
            crate::ui::shell::scroll(
                dynamic(
                    move || {
                        ui.package_editor
                            .with(|v| v.as_ref().is_some_and(|v| v.report.skin.is_some()))
                    },
                    move |skin| {
                        if *skin {
                            crate::ui::preview_panel::compact(ui)
                        } else {
                            hint(ui.now_t("pkgedit-preview-none"))
                        }
                    },
                )
                .into_any(),
            ),
        ))
        .into_any()
}

/// The files `keep` admits, as a tree; selecting one opens it.
pub(crate) fn file_tree(ui: Ui, keep: fn(&FileInfo) -> bool, key: &'static str) -> AnyView {
    dynamic(
        move || {
            ui.package_editor.with(|v| {
                v.as_ref().map(|v| {
                    (
                        v.files
                            .iter()
                            .map(|f| (f.path.clone(), f.kind, f.protected, f.dependency.is_some()))
                            .collect::<Vec<_>>(),
                        v.selected.clone(),
                    )
                })
            })
        },
        move |_| {
            let Some(view) = view_of(ui) else {
                return widget(Stack::column(0.)).into_any();
            };
            let files = view
                .files
                .iter()
                .filter(|f| keep(f))
                .cloned()
                .collect::<Vec<_>>();
            if files.is_empty() {
                return hint(ui.now_t("pkgedit-files-none"));
            }
            widget(
                TreeView::new(tree(&files, view.selected.as_deref()))
                    .label(ui.now_t("pkgedit-tab-files")),
            )
            .key(key)
            .on_cx(|_, e: &TreeSelectionChanged, cx| {
                if let Some(path) = &e.active
                    && !path.ends_with('/')
                {
                    cx.dispatch_program_all(Msg::PackageEditor(Action::Select(path.to_string())));
                }
            })
            .into_any()
        },
    )
    .into_any()
}

/// The selected file, when it can be removed: not a dependency, not
/// protected, not the manifest, and the package unsigned.
fn removable(ui: Ui) -> Option<String> {
    ui.package_editor.with(|v| {
        v.as_ref().and_then(|v| {
            let path = v.selected.clone()?;
            let file = v.files.iter().find(|f| f.path == path)?;
            (file.dependency.is_none()
                && !file.protected
                && path != PACKAGE_MANIFEST
                && v.seal == Ok(None))
            .then_some(path)
        })
    })
}

/// Remove `path` after showing what refers to it.
fn remove_button(ui: Ui, path: String) -> AnyView {
    confirm(
        *ui,
        format!(
            "{}\n{}",
            ui.now_t("pkgedit-remove-question"),
            ui.package_editor.with_untracked(|v| v
                .as_ref()
                .map(|v| v
                    .report
                    .references
                    .iter()
                    .filter(|r| r.target == path)
                    .map(|r| format!("{}: {}", r.file, r.path.join(".")))
                    .collect::<Vec<_>>()
                    .join("\n"))
                .unwrap_or_default())
        ),
        ui.now_t("pkgedit-remove"),
        move |armed| {
            widget(tool(ti::TRASH, ui.now_t("pkgedit-remove")))
                .key("package-editor-remove")
                .on(move |_: &Activate| armed.set(true))
                .into_any()
        },
        move || Msg::PackageEditor(Action::Remove(path.clone())),
    )
}

/// Byte ranges of every case-insensitive (ASCII) match of `needle`.
fn matches(text: &str, needle: &str) -> Vec<(usize, usize)> {
    nanatype_theme::editor::find_matches(text, needle)
}

fn toolbar(ui: Ui, (query, current): Search) -> AnyView {
    let path_line = dynamic(
        move || {
            ui.package_editor.with(|v| {
                v.as_ref().map(|v| {
                    let ext = v
                        .files
                        .iter()
                        .find(|f| Some(&f.path) == v.selected.as_ref())
                        .map(FileInfo::source_extension)
                        .unwrap_or_default();
                    (v.selected.clone(), v.editable, v.text.is_some(), ext)
                })
            })
        },
        move |state| {
            let Some((Some(path), editable, text, ext)) = state.clone() else {
                return widget(Stack::row(0.)).into_any();
            };
            let formattable = editable && matches!(ext.as_str(), "toml" | "json");
            widget(
                Stack::row(0.0)
                    .gap_step(nana_ui_core::SpacingStep::Sm)
                    .wrap(true)
                    .align(AlignSpec::Center),
            )
            .children((
                widget(Text::new(path.clone()).style(Type::Small.mono()))
                    .key("package-editor-path"),
                (!editable && text).then(|| hint(ui.now_t("pkgedit-read-only"))),
                formattable.then(|| {
                    widget(button(ui, "pkgedit-format"))
                        .key("package-editor-format")
                        .on_cx(send(|| Action::Format))
                }),
            ))
            .into_any()
        },
    );
    let search = widget(
        Stack::row(0.0)
            .gap_step(nana_ui_core::SpacingStep::Sm)
            .wrap(true)
            .align(AlignSpec::Center),
    )
    .children((
        input_box(
            field::SM,
            widget(
                kit::text_input("")
                    .placeholder(ui.now_t("pkgedit-search"))
                    .size(ControlSize::Small),
            )
            .key("package-editor-search")
            .on(move |e: &TextChanged| {
                query.set(e.value.to_string());
                current.set(0);
            }),
        ),
        widget(Text::new(""))
            .key("package-editor-search-count")
            .bind(move |t: &mut Text| {
                let q = query.get();
                let n = ui.package_editor.with(|v| {
                    v.as_ref()
                        .and_then(|v| v.text.as_ref().map(|t| matches(t, &q).len()))
                        .unwrap_or(0)
                });
                let value = if q.is_empty() {
                    String::new()
                } else if n == 0 {
                    ui.now_t("pkgedit-search-none")
                } else {
                    format!("{} / {n}", current.get() % n + 1)
                };
                if t.value != value {
                    t.value = value;
                }
            }),
        widget(tool(ti::ARROW_UP, ui.now_t("pkgedit-search-previous"))).on(move |_: &Activate| {
            let n = ui.package_editor.with_untracked(|v| {
                v.as_ref()
                    .and_then(|v| v.text.as_ref())
                    .map(|t| matches(t, &query.get_untracked()).len())
                    .unwrap_or(0)
            });
            current.update(|c| *c = if n == 0 { 0 } else { (*c + n - 1) % n });
        }),
        widget(tool(ti::ARROW_DOWN, ui.now_t("pkgedit-search-next")))
            .key("package-editor-search-next")
            .on(move |_: &Activate| current.update(|c| *c += 1)),
    ));
    widget(
        Stack::column(0.0)
            .gap_step(nana_ui_core::SpacingStep::Xs)
            .shrink(0.),
    )
    .children((
        path_line,
        widget(
            Stack::row(0.)
                .wrap(true)
                .gap_step(nana_ui_core::SpacingStep::Sm)
                .align(AlignSpec::Center),
        )
        .children((
            search,
            input_box(
                field::SM,
                widget(
                    kit::text_input(ui.search_replacement.get_untracked())
                        .placeholder(ui.now_t("pkgedit-replacement"))
                        .size(ControlSize::Small),
                )
                .on(move |e: &TextChanged| ui.search_replacement.set(e.value.to_string())),
            ),
            widget(button(ui, "pkgedit-replace-one").kind(ButtonKind::Ghost)).on_cx(send(
                move || Action::ReplaceText {
                    needle: query.get_untracked(),
                    replacement: ui.search_replacement.get_untracked(),
                    index: current.get_untracked(),
                    all: false,
                },
            )),
            widget(button(ui, "pkgedit-replace-all").kind(ButtonKind::Ghost)).on_cx(send(
                move || Action::ReplaceText {
                    needle: query.get_untracked(),
                    replacement: ui.search_replacement.get_untracked(),
                    index: 0,
                    all: true,
                },
            )),
        )),
    ))
    .into_any()
}

fn severity(level: Level) -> TextDiagnosticSeverity {
    match level {
        Level::Error => TextDiagnosticSeverity::Error,
        Level::Warning => TextDiagnosticSeverity::Warning,
    }
}

fn source(ui: Ui, (query, current): Search) -> AnyView {
    dynamic(
        move || {
            ui.package_editor.with(|v| {
                v.as_ref()
                    .map(|v| (v.selected.clone(), v.editable, v.revision, v.text.is_some()))
            })
        },
        move |state| {
            let Some(view) = view_of(ui) else {
                return widget(Stack::column(0.)).into_any();
            };
            let Some((Some(path), editable, _, true)) = state.clone() else {
                return file_info(ui, &view);
            };
            let text = view.text.clone().unwrap_or_default();
            let mut area = kit::text_area(text.to_string())
                .line_numbers(true)
                .read_only(!editable)
                .label(path.clone());
            if let Some(saved) = ui.source_views.with_untracked(|m| m.get(&path).cloned()) {
                if saved.state.value.as_str() == text.as_ref() {
                    area.state = saved.state;
                }
                area.scroll_offset = saved.scroll_offset;
            }
            let ext = view
                .files
                .iter()
                .find(|f| f.path == path)
                .map(FileInfo::source_extension)
                .unwrap_or_default();
            area.code_editing = if ext == "lua" {
                Some(CodeEditing::new("--", "    "))
            } else if ext == "wgsl" {
                Some(CodeEditing::wgsl())
            } else if ext == "toml" {
                Some(CodeEditing::new("#", "    "))
            } else {
                None
            };
            {
                let l = Arc::make_mut(&mut area.style.layout);
                l.width = Some(LengthSpec::Fill);
                l.height = Some(LengthSpec::Px(0.));
                l.flex_grow = Some(1.);
                l.min_height = Some(LengthSpec::Px(SOURCE_MIN_H));
                crate::ui::mono(l);
            }
            if let Some((_, start, end)) = view.jump {
                area.state.selection = TextSelection::new(start, end);
            }
            let edit_path = path.clone();
            let span_path = path.clone();
            let applied_jump = std::cell::Cell::new(view.jump.map(|j| j.0));
            // The search step last moved to; typing must not move the caret.
            let applied_search = std::cell::RefCell::new((String::new(), 0usize));
            widget(area)
                .node_ref(ui.source_node)
                .key(node_key("package-editor-source", &path))
                .bind(move |a: &mut TextArea| {
                    let (diagnostics, jump, text) = ui.package_editor.with(|v| {
                        let Some(v) = v.as_ref() else {
                            return (Vec::new(), None, None);
                        };
                        let spans = v
                            .syntax
                            .iter()
                            .chain(
                                v.report
                                    .diagnostics
                                    .iter()
                                    .filter(|d| !v.validating && d.path == span_path),
                            )
                            .map(|d| {
                                TextDiagnosticSpan::new(d.offset, d.len, severity(d.level))
                                    .with_message(d.message.clone())
                            })
                            .collect::<Vec<_>>();
                        (spans, v.jump, v.text.clone())
                    });
                    if *a.diagnostics != *diagnostics {
                        a.diagnostics = diagnostics.into();
                    }
                    if let Some((id, start, end)) = jump
                        && applied_jump.get() != Some(id)
                    {
                        applied_jump.set(Some(id));
                        a.state.selection = TextSelection::new(start, end);
                    }
                    let query = query.get();
                    let step = current.get();
                    let found = text
                        .as_deref()
                        .map(|t| matches(t, &query))
                        .unwrap_or_default();
                    let current = step % found.len().max(1);
                    let spans = found
                        .into_iter()
                        .enumerate()
                        .map(|(i, (offset, length))| TextMatchSpan {
                            offset,
                            length,
                            current: i == current,
                        })
                        .collect::<Vec<_>>();
                    if *applied_search.borrow() != (query.clone(), step) {
                        *applied_search.borrow_mut() = (query, step);
                        if let Some(hit) = spans.iter().find(|s| s.current) {
                            a.state.selection =
                                TextSelection::new(hit.offset, hit.offset + hit.length);
                        }
                    }
                    if *a.match_spans != *spans {
                        a.match_spans = spans.into();
                    }
                })
                .on_cx(move |_, e: &TextChanged, cx| {
                    cx.dispatch_program_all(Msg::PackageEditor(Action::Edit {
                        path: edit_path.clone(),
                        text: e.value.to_string(),
                    }));
                })
                .into_any()
        },
    )
    .grow()
    .into_any()
}

/// Binary or oversized files: format, size, hash and any data declaration.
fn file_info(ui: Ui, view: &EditorView) -> AnyView {
    let Some(file) = view
        .selected
        .as_ref()
        .and_then(|p| view.files.iter().find(|f| &f.path == p))
    else {
        return hint(ui.now_t("pkgedit-no-file"));
    };
    let mut lines = vec![
        format!("{}: {}", ui.now_t("pkgedit-file-format"), file.format),
        format!("{}: {} B", ui.now_t("pkgedit-file-size"), file.bytes),
        format!(
            "BLAKE3: {}",
            if file.hash.is_empty() {
                "…"
            } else {
                &file.hash
            }
        ),
        ui.now_t(if file.protected {
            "package-data-protected"
        } else {
            "package-data-unprotected"
        }),
    ];
    if let Some(dependency) = &file.dependency {
        lines.push(format!(
            "{}: {dependency}",
            ui.now_t("pkgedit-file-dependency")
        ));
    }
    if let Some((id, spec)) = &file.data {
        lines.push(format!("{}: {id}", ui.now_t("pkgedit-data-declared")));
        lines.push(format!(
            "{}: {}",
            ui.now_t("package-data-source"),
            spec.source
        ));
        lines.push(format!(
            "{}: {}",
            ui.now_t("package-data-license"),
            spec.license
        ));
    }
    widget(
        Stack::column(0.0)
            .gap_step(nana_ui_core::SpacingStep::Sm)
            .padding_step(nana_ui_core::SpacingStep::Md, nana_ui_core::SpacingStep::Md)
            .surface(SemanticColorRole::Subtle)
            .radius(RadiusTier::Sm),
    )
    .key("package-editor-file-info")
    .children((
        label(ui.now_t("pkgedit-binary")),
        widget(readable(lines.join("\n"))),
        crate::ui::assets::preview(ui),
    ))
    .into_any()
}
