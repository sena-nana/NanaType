//! The package editor: a small IDE over a draft. Only the draft changes;
//! installed packages, the active skin and running plugins never do.
use crate::msg::{FileTarget, Msg};
use crate::session::{Action, EditorView, Tab};
use crate::ui::Ui;
use crate::ui::controls::{confirm, hint, label};
use crate::ui::kit::{self, Tone, Type, field, gap, inset};
use nana_ui::runtime::view::{AnyView, IntoView, Signal, dynamic, signal, watch_effect, widget};
use nana_ui::runtime::{
    Activate, Button, Checkbox, CodeEditing, ScrollAxes, ScrollView, Select, SelectChanged,
    SelectOption, SizeChanged, Stack, Text, TextArea, TextChanged, TextDiagnosticSeverity,
    TextDiagnosticSpan, TextMatchSpan, TextSelection, ToggleChanged, TreeSelectionChanged,
    TreeView, ViewContext,
};
use nana_ui::theme::space;
use nana_ui::{ButtonKind, ControlSize, icons_tabler as ti};
use nana_ui_core::{AlignSpec, LengthSpec, RadiusTier, SemanticColorRole, TreeNode};
use nanatype_theme::editor::{AssetKind, DraftOrigin, FileInfo, Level, Metadata, ProjectKind};
use nanatype_theme::{
    DataFormat, DataSpec, MANIFEST, PACKAGE_MANIFEST, PackageCapability, PipelineSpec, PortSource,
};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// The files pane's tree beside the source.
const FILE_TREE_W: f32 = field::MD;
/// The tree when it stacks above the source in a narrow editor.
const STACKED_TREE_H: f32 = 180.0;
/// The least room the source editor keeps.
const SOURCE_MIN_H: f32 = 160.0;
/// Lists and problems under a form, before they scroll.
const LIST_MAX_H: f32 = 180.0;
const PROBLEMS_MAX_H: f32 = 150.0;
/// A form's multi-line value, and a short note.
const FORM_TEXT_H: f32 = 72.0;
const NOTE_H: f32 = 56.0;

type Search = (Signal<String>, Signal<usize>);

/// A button handler that sends one editor action.
fn send(
    action: impl Fn() -> Action + Send + 'static,
) -> impl FnMut(&mut Button, &Activate, &mut ViewContext<'_, Button>) + Send + 'static {
    move |_, _, cx| cx.dispatch_program_all(Msg::PackageEditor(action()))
}

/// Update a field changed through a picker/template without resetting live typing.
pub(super) fn sync_text(state: &mut nana_ui::runtime::TextInputState, value: String) {
    if state.value.as_str() != value {
        *state = nana_ui::runtime::TextInputState::new(value);
    }
}

/// A form value belongs to the session, including invalid text and hidden panels.
pub(super) fn buffer<T>(ui: Ui, key: impl Into<String>, initial: T) -> Signal<T>
where
    T: Clone + PartialEq + serde::Serialize + serde::de::DeserializeOwned + Send + Sync + 'static,
{
    let key = key.into();
    let view = view_of(ui);
    let session = view.as_ref().map_or(0, |v| v.session);
    let restored = view
        .and_then(|v| v.forms.get(&key).cloned())
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_else(|| initial.clone());
    let value = signal(restored.clone());
    let mut previous = restored;
    watch_effect(move || {
        let current = value.get();
        if current != previous {
            previous = current.clone();
            if let Some(post) = ui.post.get_untracked() {
                post(Action::Buffer {
                    session,
                    key: key.clone(),
                    value: if current == initial {
                        None
                    } else {
                        serde_json::to_value(current).ok()
                    },
                });
            }
        }
    });
    value
}

/// `YYYY-MM-DD HH:MM` in local time.
fn stamp(unix: u64) -> String {
    let t = unix as i64 + nanatype_frame::localtime::offset_seconds();
    let (days, secs) = (t.div_euclid(86_400), t.rem_euclid(86_400));
    // Howard Hinnant's civil-from-days.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02}",
        secs / 3600,
        secs / 60 % 60
    )
}

/// Node keys are `/`-separated paths, so file paths inside them need escaping.
fn node_key(prefix: &str, path: &str) -> String {
    format!("{prefix}:{}", path.replace('/', "|"))
}

/// Keep a control at its natural width inside a stretching column.
fn natural(view: impl IntoView) -> AnyView {
    widget(Stack::row(0.)).children(view).into_any()
}

/// A text field up to `max` wide that shrinks with a narrow window.
fn input_box(max: f32, input: impl IntoView) -> AnyView {
    widget(
        Stack::row(0.)
            .width(LengthSpec::Px(max))
            .min_width(LengthSpec::Px(0.))
            .shrink(1.)
            // Wrapped rows do not shrink their items: cap at the line instead.
            .with_layout(|l| l.max_width = Some(LengthSpec::Percent(100.))),
    )
    .children(input)
    .into_any()
}

fn button(ui: Ui, key: &str) -> Button {
    Button::new(ui.now_t(key)).size(ControlSize::Small)
}

/// Wrapping, selectable-looking text that never widens its column.
fn readable(value: impl Into<String>) -> Text {
    let mut text = Text::new(value).style(Type::Caption.style());
    let layout = Arc::make_mut(&mut text.style.layout);
    layout.overflow_wrap = Some(nana_ui_core::OverflowWrapSpec::Anywhere);
    layout.white_space = nana_ui_core::WhiteSpaceSpec::PreWrap;
    text
}

fn kind_key(kind: ProjectKind) -> &'static str {
    match kind {
        ProjectKind::Skin => "pkgedit-kind-skin",
        ProjectKind::Resources => "pkgedit-kind-resources",
        ProjectKind::Lua => "pkgedit-kind-lua",
        ProjectKind::Sounds => "pkgedit-kind-sounds",
        ProjectKind::Mixed => "pkgedit-kind-mixed",
    }
}

/// A tab body that scrolls within the remaining height.
fn scrolled(content: AnyView) -> AnyView {
    let scroll = ScrollView::new(ScrollAxes::Vertical).with_layout(|l| {
        l.flex_grow = Some(1.0);
        l.flex_shrink = Some(1.0);
        l.height = Some(LengthSpec::Px(0.0));
        l.min_height = Some(LengthSpec::Px(0.0));
        l.width = Some(LengthSpec::Fill);
    });
    widget(scroll)
        .key("editor-panel-scroll")
        .children(widget(Stack::column(0.).padding_xy(0., space::XS)).children(content))
        .into_any()
}

fn view_of(ui: Ui) -> Option<Arc<EditorView>> {
    ui.package_editor.get_untracked()
}

// ---- launcher (Packages page) ----------------------------------------------

/// New, open and saved drafts, above the package list.
pub fn launcher(ui: Ui) -> AnyView {
    let kind = signal(ProjectKind::Mixed);
    let id = signal(String::new());
    let kinds = ProjectKind::ALL
        .iter()
        .map(|k| SelectOption::new(k.key(), ui.now_t(kind_key(*k))))
        .collect::<Vec<_>>();
    let drafts = dynamic(
        move || ui.package_drafts.get(),
        move |drafts| {
            if drafts.is_empty() {
                return hint(ui.now_t("pkgedit-drafts-none"));
            }
            let rows = drafts
                .iter()
                .map(|draft| {
                    let open = draft.id.clone();
                    let discard = draft.id.clone();
                    widget(
                        Stack::row(0.0)
                            .gap_step(nana_ui_core::SpacingStep::Md)
                            .wrap(true)
                            .align(AlignSpec::Center),
                    )
                    .key(format!("package-draft:{}", draft.id))
                    .children((
                        widget(readable(format!(
                            "{} · {}",
                            draft.package,
                            stamp(draft.saved_at)
                        ))),
                        widget(button(ui, "pkgedit-open"))
                            .on_cx(send(move || Action::OpenDraft(open.clone()))),
                        confirm(
                            *ui,
                            ui.now_t("pkgedit-discard-question"),
                            ui.now_t("pkgedit-discard"),
                            move |armed| {
                                widget(button(ui, "pkgedit-discard"))
                                    .on(move |_: &Activate| armed.set(true))
                                    .into_any()
                            },
                            move || Msg::PackageEditor(Action::DiscardDraft(discard.clone())),
                        ),
                    ))
                    .into_any()
                })
                .collect::<Vec<_>>();
            widget(Stack::column(0.0).gap_step(nana_ui_core::SpacingStep::Sm))
                .children(rows)
                .into_any()
        },
    );
    widget(crate::ui::kit::panel())
        .key("package-editor-launcher")
        .children((
            label(ui.now_t("pkgedit-title")),
            widget(readable(ui.now_t("pkgedit-intro"))),
            widget(
                Stack::row(0.0)
                    .gap_step(nana_ui_core::SpacingStep::Md)
                    .wrap(true)
                    .align(AlignSpec::Center),
            )
            .children((
                widget(Stack::row(0.).width(LengthSpec::Px(field::SM))).children(
                    widget(
                        Select::new(Some(ProjectKind::Mixed.key()))
                            .options(kinds)
                            .size(ControlSize::Small),
                    )
                    .key("package-editor-kind")
                    .on(move |e: &SelectChanged| {
                        if let Some(k) = ProjectKind::ALL.into_iter().find(|k| k.key() == &*e.value)
                        {
                            kind.set(k);
                        }
                    }),
                ),
                input_box(
                    field::MD,
                    widget(
                        kit::text_input("")
                            .placeholder(ui.now_t("pkgedit-id-placeholder"))
                            .size(ControlSize::Small),
                    )
                    .key("package-editor-id")
                    .on(move |e: &TextChanged| id.set(e.value.to_string())),
                ),
                widget(button(ui, "pkgedit-new").kind(ButtonKind::Primary))
                    .key("package-editor-new")
                    .bind(move |b: &mut Button| {
                        b.disabled = id.with(|i| i.trim().is_empty());
                    })
                    .on_cx(send(move || Action::New {
                        kind: kind.get_untracked(),
                        id: id.get_untracked(),
                    })),
                widget(button(ui, "pkgedit-open-file")).on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program_all(Msg::PickFile(FileTarget::EditPackage))
                }),
            )),
            dynamic(
                move || ui.recoveries.get(),
                move |rows| {
                    widget(Stack::column(0.).gap_step(gap::step::CONTROL))
                        .children(
                            rows.iter()
                                .map(|r| {
                                    let id = r.id.clone();
                                    let discard = id.clone();
                                    widget(Stack::row(0.).wrap(true))
                                        .children((
                                            widget(readable(format!(
                                                "{} · {}",
                                                ui.now_t("pkgedit-recovery"),
                                                r.package
                                            ))),
                                            widget(button(ui, "pkgedit-recover")).on_cx(send(
                                                move || Action::RestoreRecovery(id.clone()),
                                            )),
                                            widget(button(ui, "pkgedit-discard")).on_cx(send(
                                                move || Action::DiscardRecovery(discard.clone()),
                                            )),
                                        ))
                                        .into_any()
                                })
                                .collect::<Vec<_>>(),
                        )
                        .into_any()
                },
            ),
            widget(button(ui, "pkgedit-open-directory"))
                .key("editor-open-directory")
                .on_cx(|_, _: &Activate, cx| {
                    cx.dispatch_program_all(Msg::PickFile(FileTarget::OpenDirectory))
                }),
            label(ui.now_t("pkgedit-drafts")),
            drafts,
        ))
        .into_any()
}

/// The per-package editor entries on the packages page.
pub fn package_actions(ui: Ui, id: &str, builtin: bool) -> AnyView {
    let edit = id.to_owned();
    let copy = id.to_owned();
    widget(Stack::row(0.0).gap_step(gap::step::CONTROL).wrap(true))
        .children((
            (!builtin).then(|| {
                widget(button(ui, "pkgedit-edit"))
                    .key(format!("package-edit:{id}"))
                    .on_cx(send(move || Action::OpenInstalled(edit.clone())))
            }),
            widget(button(ui, "pkgedit-copy"))
                .key(format!("package-copy:{id}"))
                .on_cx(send(move || Action::CopyInstalled(copy.clone()))),
        ))
        .into_any()
}

// ---- the editor --------------------------------------------------------------

pub fn editor(ui: Ui) -> AnyView {
    widget(Stack::fill_column(0.0).with_layout(|l| l.min_width = Some(LengthSpec::Px(0.0))))
        .key("package-editor")
        .on(move |e: &SizeChanged| {
            ui.workspace_width.set_if_changed(e.width);
        })
        .children((
            header(ui),
            super::conflicts::view(ui),
            widget(
                Stack::row(0.)
                    .gap_step(gap::step::CONTROL)
                    .wrap(true)
                    .padding_xy(inset::PAGE, 0.),
            )
            .children((
                widget(button(ui, "pkgedit-undo"))
                    .key("package-editor-undo")
                    .bind(move |b: &mut Button| {
                        b.disabled = !ui
                            .package_editor
                            .with(|v| v.as_ref().is_some_and(|v| v.undo))
                    })
                    .on_cx(send(|| Action::Undo)),
                widget(button(ui, "pkgedit-redo"))
                    .key("package-editor-redo")
                    .bind(move |b: &mut Button| {
                        b.disabled = !ui
                            .package_editor
                            .with(|v| v.as_ref().is_some_and(|v| v.redo))
                    })
                    .on_cx(send(|| Action::Redo)),
            )),
            widget(
                Stack::fill_column(0.0)
                    .gap_step(gap::step::CONTROL)
                    .padding_step(inset::step::PAGE, inset::step::ROW_Y)
                    .with_layout(|l| l.min_width = Some(LengthSpec::Px(0.0))),
            )
            .children((close_prompt(ui), tabs(ui), body(ui))),
        ))
        .into_any()
}

fn origin_text(ui: Ui, view: &EditorView) -> String {
    let origin = match &view.origin {
        DraftOrigin::Directory { path } => format!(
            "{}: {}",
            ui.now_t("pkgedit-origin-directory"),
            path.display()
        ),
        DraftOrigin::New => ui.now_t("pkgedit-origin-new"),
        DraftOrigin::Installed { id } => format!("{}: {id}", ui.now_t("pkgedit-origin-installed")),
        DraftOrigin::Archive { path } => {
            format!("{}: {}", ui.now_t("pkgedit-origin-archive"), path.display())
        }
        DraftOrigin::Copy { from } => format!("{}: {from}", ui.now_t("pkgedit-origin-copy")),
    };
    if view.derived_from.is_empty() {
        origin
    } else {
        format!(
            "{origin}\n{}: {} · {}",
            ui.now_t("pkgedit-derived-from"),
            view.derived_from,
            ui.now_t("pkgedit-derived-unsigned")
        )
    }
}

fn status_text(ui: Ui, view: &EditorView) -> String {
    let save = ui.now_t(if view.dirty {
        "pkgedit-unsaved"
    } else if view.stored {
        "pkgedit-saved-state"
    } else {
        "pkgedit-not-stored"
    });
    let check = if view.validating {
        ui.now_t("pkgedit-validating")
    } else {
        match view.report.errors() {
            0 => ui.now_t("pkgedit-valid"),
            n => ui.tr("pkgedit-errors", &[("count", (n as i64).into())]),
        }
    };
    let seal = crate::ui::plugin_view::integrity_label(*ui, &view.seal);
    format!("{save} · {check} · {seal}")
}

fn header(ui: Ui) -> AnyView {
    dynamic(
        move || {
            ui.package_editor.with(|v| {
                v.as_ref().map(|v| {
                    (
                        v.package.clone(),
                        v.kind,
                        origin_text(ui, v),
                        status_text(ui, v),
                        v.dirty,
                        v.can_revert,
                    )
                })
            })
        },
        move |state| {
            let Some((package, kind, origin, status, dirty, can_revert)) = state.clone() else {
                return widget(Stack::column(0.)).into_any();
            };
            let state = if dirty { Tone::Warning } else { Tone::Success };
            widget(Stack::column(0.0).with_layout(|l| l.flex_shrink = Some(0.0)))
                .key("package-editor-header")
                .children((
                    widget(
                        Stack::row(0.0)
                            .gap_step(gap::step::GROUP)
                            .wrap(true)
                            .padding_step(inset::step::PAGE, inset::step::CARD)
                            .align(AlignSpec::Center)
                            .width(LengthSpec::Fill),
                    )
                    .children((
                        widget(crate::ui::kit::grow_column(gap::HAIR)).children((
                            widget(
                                Stack::row(0.0)
                                    .gap_step(gap::step::CONTROL)
                                    .wrap(true)
                                    .align(AlignSpec::Center),
                            )
                            .children((
                                widget(readable(package).style(Type::PageTitle.style())),
                                crate::ui::kit::tag(ui.now_t(kind_key(kind))),
                            )),
                            widget(readable(origin).color(SemanticColorRole::Muted)),
                        )),
                        widget(
                            Stack::row(0.0)
                                .gap_step(gap::step::CONTROL)
                                .wrap(true)
                                .align(AlignSpec::Center),
                        )
                        .children((
                            dirty.then(|| {
                                widget(button(ui, "pkgedit-revert").disabled(!can_revert))
                                    .key("package-editor-revert")
                                    .on_cx(send(|| Action::Revert))
                            }),
                            widget(button(ui, "pkgedit-export"))
                                .key("package-editor-export")
                                .on_cx(send(|| Action::Export)),
                            widget(button(ui, "pkgedit-close"))
                                .key("package-editor-close")
                                .on_cx(send(|| Action::Close)),
                            widget(if dirty {
                                button(ui, "pkgedit-save").kind(ButtonKind::Primary)
                            } else {
                                button(ui, "pkgedit-save")
                            })
                            .key("package-editor-save")
                            .on_cx(send(|| Action::Save)),
                        )),
                    )),
                    widget(
                        Stack::row(0.0)
                            .gap_step(gap::step::CONTROL)
                            .wrap(true)
                            .padding_xy(inset::PAGE, 0.0)
                            .align(AlignSpec::Center)
                            .with_layout(|l| l.padding_bottom = Some(LengthSpec::Px(inset::BAR_Y))),
                    )
                    .children((
                        crate::ui::kit::status_dot(state),
                        widget(readable(status)).key("package-editor-status"),
                    )),
                    crate::ui::kit::rule(),
                ))
                .into_any()
        },
    )
    .into_any()
}

fn close_prompt(ui: Ui) -> AnyView {
    dynamic(
        move || {
            ui.package_editor
                .with(|v| v.as_ref().is_some_and(|v| v.confirm_close))
        },
        move |open| {
            if !*open {
                return widget(Stack::column(0.)).into_any();
            }
            widget(
                Stack::row(0.0)
                    .gap_step(gap::step::CONTROL)
                    .wrap(true)
                    .align(AlignSpec::Center)
                    .padding_step(inset::step::PANEL, inset::step::BAR_Y)
                    .radius(RadiusTier::Sm)
                    .surface_mix(nana_ui_core::SemanticColorMix::alpha(
                        SemanticColorRole::Warning,
                        crate::ui::kit::wash::TINT,
                    )),
            )
            .key("package-editor-close-prompt")
            .children((
                widget(
                    readable(ui.now_t("pkgedit-close-unsaved")).color(SemanticColorRole::Warning),
                ),
                widget(button(ui, "pkgedit-close-save").kind(ButtonKind::Primary))
                    .key("package-editor-close-save")
                    .on_cx(send(|| Action::ConfirmClose { save: true })),
                widget(button(ui, "pkgedit-close-discard").kind(ButtonKind::Danger))
                    .key("package-editor-close-discard")
                    .on_cx(send(|| Action::ConfirmClose { save: false })),
                widget(button(ui, "common-cancel")).on_cx(send(|| Action::CancelClose)),
            ))
            .into_any()
        },
    )
    .into_any()
}

const TABS: [(Tab, &str); 7] = [
    (Tab::Files, "pkgedit-tab-files"),
    (Tab::Properties, "pkgedit-tab-properties"),
    (Tab::Sounds, "pkgedit-tab-sounds"),
    (Tab::Package, "pkgedit-tab-package"),
    (Tab::Signing, "pkgedit-tab-signing"),
    (Tab::Preview, "pkgedit-tab-preview"),
    (Tab::Debug, "pkgedit-tab-debug"),
];

/// A wrapping row of tab buttons, so every tab stays reachable at 300px.
fn tabs(ui: Ui) -> AnyView {
    let buttons =
        TABS.into_iter()
            .map(|(tab, key)| {
                widget(button(ui, key))
                    .key(format!("package-editor-tab:{key}"))
                    .visible(move || {
                        ui.package_editor.with(|v| {
                            v.as_ref().is_some_and(|v| match tab {
                                Tab::Preview => v.metadata.as_ref().is_some_and(|m| {
                                    m.capabilities.contains(&PackageCapability::Skin)
                                }),
                                Tab::Sounds => v.metadata.as_ref().is_some_and(|m| {
                                    m.capabilities.contains(&PackageCapability::Skin)
                                        || m.capabilities.contains(&PackageCapability::Sounds)
                                }),
                                Tab::Debug => v.metadata.as_ref().is_some_and(|m| {
                                    m.capabilities.contains(&PackageCapability::Lua)
                                }),
                                _ => true,
                            })
                        })
                    })
                    .bind(move |b: &mut Button| {
                        let kind = if ui.package_editor_tab.get() == tab {
                            ButtonKind::Selected
                        } else {
                            ButtonKind::Ghost
                        };
                        if b.kind != kind {
                            b.kind = kind;
                        }
                    })
                    .on(move |_: &Activate| {
                        if tab == Tab::Sounds {
                            let standalone = ui.package_editor.with_untracked(|v| {
                                v.as_ref()
                                    .is_some_and(|v| v.documents.contains_key("sounds.toml"))
                            });
                            ui.properties_file.set(
                                if standalone {
                                    "sounds.toml"
                                } else {
                                    "skin.toml"
                                }
                                .into(),
                            );
                            ui.properties_path.set(vec!["sounds".into()]);
                        }
                        ui.package_editor_tab.set(tab);
                    })
                    .into_any()
            })
            .collect::<Vec<_>>();
    widget(
        Stack::row(0.0)
            .gap_step(nana_ui_core::SpacingStep::Xs)
            .wrap(true),
    )
    .key("package-editor-tabs")
    .children(buttons)
    .into_any()
}

fn body(ui: Ui) -> AnyView {
    let left = widget(Stack::fill_column(0.).with_layout(|l| {
        l.width = Some(LengthSpec::Px(0.));
        l.flex_grow = Some(1.);
        l.min_width = Some(LengthSpec::Px(0.));
    }))
    .visible(move || ui.package_editor_tab.get() != Tab::Preview)
    .children(
        dynamic(
            move || ui.package_editor_tab.get(),
            move |tab| match tab {
                Tab::Files => files(ui),
                Tab::Sounds => (scrolled(super::properties::view(ui)), problems(ui)).into_any(),
                Tab::Properties => (scrolled(super::properties::view(ui)), problems(ui)).into_any(),
                Tab::Package => (scrolled(package(ui)), problems(ui)).into_any(),
                Tab::Signing => (scrolled(signing(ui)), problems(ui)).into_any(),
                Tab::Debug => (scrolled(debug(ui)), problems(ui)).into_any(),
                Tab::Preview => widget(Stack::column(0.)).into_any(),
            },
        )
        .grow(),
    );
    let right = widget(Stack::fill_column(0.))
        .bind(move |s: &mut Stack| {
            let split =
                ui.workspace_width.get() >= 1100. && ui.package_editor_tab.get() != Tab::Preview;
            *s = Stack::fill_column(0.).with_layout(|l| {
                l.width = Some(if split {
                    LengthSpec::Px(440.)
                } else {
                    LengthSpec::Fill
                });
                l.flex_grow = Some(if split { 0. } else { 1. });
                l.flex_shrink = Some(1.);
                l.min_width = Some(LengthSpec::Px(0.));
            });
        })
        .visible(move || {
            ui.package_editor_tab.get() == Tab::Preview
                || (ui.workspace_width.get() >= 1100.
                    && ui
                        .package_editor
                        .with(|v| v.as_ref().is_some_and(|v| v.report.skin.is_some())))
        })
        .children(
            dynamic(
                move || {
                    ui.package_editor_tab.get() == Tab::Preview || ui.workspace_width.get() >= 1100.
                },
                move |visible| {
                    if *visible {
                        scrolled(preview(ui))
                    } else {
                        widget(Stack::column(0.)).into_any()
                    }
                },
            )
            .grow(),
        );
    widget(
        Stack::row(0.)
            .align(AlignSpec::Stretch)
            .gap_step(gap::step::GROUP)
            .grow(1.)
            .shrink(1.)
            .width(LengthSpec::Fill)
            .height(LengthSpec::Fill)
            .with_layout(|l| {
                l.min_height = Some(LengthSpec::Px(0.));
            }),
    )
    .key("editor-workspace-body")
    .children((left, right))
    .into_any()
}

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

/// Below this width the file tree sits above the source instead of beside it.
const NARROW: f32 = 560.;

fn files(ui: Ui) -> AnyView {
    let narrow = signal(false);
    let search: Search = (ui.search_query, ui.search_index);
    widget(Stack::fill_column(0.))
        .key("package-editor-files")
        .on(move |e: &SizeChanged| {
            let n = e.width < NARROW && ui.workspace_width.get_untracked() < 1100.;
            if narrow.get_untracked() != n {
                narrow.set(n);
            }
        })
        .children(
            dynamic(
                move || narrow.get(),
                move |narrow| {
                    let narrow = *narrow;
                    let side = widget(
                        Stack::column(0.0)
                            .gap_step(nana_ui_core::SpacingStep::Sm)
                            .shrink(0.)
                            .with_layout(move |l| {
                                if narrow {
                                    l.width = Some(LengthSpec::Fill);
                                } else {
                                    l.width = Some(LengthSpec::Px(FILE_TREE_W));
                                    l.height = Some(LengthSpec::Fill);
                                }
                            }),
                    )
                    .children((
                        widget(
                            Stack::row(0.0)
                                .gap_step(nana_ui_core::SpacingStep::Sm)
                                .wrap(true),
                        )
                        .children((
                            widget(button(ui, "pkgedit-import"))
                                .key("package-editor-import")
                                .on_cx(|_, _: &Activate, cx| {
                                    cx.dispatch_program_all(Msg::PickFile(
                                        FileTarget::ImportProjectAsset,
                                    ))
                                }),
                            remove_button(ui),
                        )),
                        super::assets::operations(ui),
                        widget(ScrollView::new(ScrollAxes::Vertical).with_layout(move |l| {
                            if narrow {
                                l.max_height = Some(LengthSpec::Px(STACKED_TREE_H));
                            } else {
                                l.height = Some(LengthSpec::Px(0.));
                                l.flex_grow = Some(1.);
                            }
                        }))
                        .children(file_tree(ui)),
                    ));
                    let main = widget(
                        Stack::fill_column(0.0)
                            .gap_step(nana_ui_core::SpacingStep::Sm)
                            .with_layout(move |l| {
                                if !narrow {
                                    // Share the row with the tree instead of taking all of it.
                                    l.width = Some(LengthSpec::Px(0.));
                                }
                            }),
                    )
                    .children((
                        toolbar(ui, search),
                        source(ui, search),
                        problems(ui),
                    ));
                    let layout = if narrow {
                        Stack::fill_column(0.0).gap_step(nana_ui_core::SpacingStep::Md)
                    } else {
                        Stack::row(0.0)
                            .gap_step(nana_ui_core::SpacingStep::Lg)
                            .align(AlignSpec::Stretch)
                            .grow(1.)
                            .shrink(1.)
                            .width(LengthSpec::Fill)
                            .height(LengthSpec::Fill)
                    };
                    widget(layout).children((side, main)).into_any()
                },
            )
            .grow(),
        )
        .into_any()
}

fn file_tree(ui: Ui) -> AnyView {
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
            widget(
                TreeView::new(tree(&view.files, view.selected.as_deref()))
                    .label(ui.now_t("pkgedit-tab-files")),
            )
            .key("package-editor-tree")
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

fn remove_button(ui: Ui) -> AnyView {
    dynamic(
        move || {
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
        },
        move |path| {
            let Some(path) = path.clone() else {
                return widget(Stack::row(0.)).into_any();
            };
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
                    widget(button(ui, "pkgedit-remove"))
                        .key("package-editor-remove")
                        .on(move |_: &Activate| armed.set(true))
                        .into_any()
                },
                move || Msg::PackageEditor(Action::Remove(path.clone())),
            )
        },
    )
    .into_any()
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
            field::MD,
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
        widget(button(ui, "pkgedit-search-previous")).on(move |_: &Activate| {
            let n = ui.package_editor.with_untracked(|v| {
                v.as_ref()
                    .and_then(|v| v.text.as_ref())
                    .map(|t| matches(t, &query.get_untracked()).len())
                    .unwrap_or(0)
            });
            current.update(|c| *c = if n == 0 { 0 } else { (*c + n - 1) % n });
        }),
        widget(button(ui, "pkgedit-search-next"))
            .key("package-editor-search-next")
            .on(move |_: &Activate| current.update(|c| *c += 1)),
    ));
    widget(Stack::column(0.0).gap_step(nana_ui_core::SpacingStep::Xs))
        .children((
            path_line,
            search,
            widget(Stack::row(0.).wrap(true).gap_step(gap::step::CONTROL)).children((
                input_box(
                    field::MD,
                    widget(
                        kit::text_input(ui.search_replacement.get_untracked())
                            .placeholder(ui.now_t("pkgedit-replacement"))
                            .size(ControlSize::Small),
                    )
                    .on(move |e: &TextChanged| ui.search_replacement.set(e.value.to_string())),
                ),
                widget(button(ui, "pkgedit-replace-one")).on_cx(send(move || {
                    Action::ReplaceText {
                        needle: query.get_untracked(),
                        replacement: ui.search_replacement.get_untracked(),
                        index: current.get_untracked(),
                        all: false,
                    }
                })),
                widget(button(ui, "pkgedit-replace-all")).on_cx(send(move || {
                    Action::ReplaceText {
                        needle: query.get_untracked(),
                        replacement: ui.search_replacement.get_untracked(),
                        index: 0,
                        all: true,
                    }
                })),
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
        super::assets::preview(ui),
    ))
    .into_any()
}

fn problems(ui: Ui) -> AnyView {
    dynamic(
        move || {
            ui.package_editor.with(|v| {
                v.as_ref().map(|v| {
                    (
                        Arc::as_ptr(&v.report) as usize,
                        Arc::as_ptr(&v.syntax) as usize,
                        v.validating,
                    )
                })
            })
        },
        move |_| {
            let Some(view) = view_of(ui) else {
                return widget(Stack::column(0.)).into_any();
            };
            let mut seen = BTreeSet::new();
            let rows = view
                .syntax
                .iter()
                .chain(view.debug.location.iter())
                .chain(view.report.diagnostics.iter())
                .filter(|d| seen.insert((d.path.clone(), d.offset, d.message.clone())))
                .take(50)
                .enumerate()
                .map(|(i, d)| {
                    let target = d.clone();
                    let mark = if d.level == Level::Error {
                        "✕"
                    } else {
                        "⚠"
                    };
                    // A short location button; the message wraps beside it.
                    widget(
                        Stack::row(0.0)
                            .gap_step(nana_ui_core::SpacingStep::Sm)
                            .align(AlignSpec::Start)
                            .width(LengthSpec::Fill),
                    )
                    .children((
                        widget(
                            Button::new(format!("{mark} {}:{}:{}", d.path, d.line, d.column))
                                .kind(ButtonKind::Ghost)
                                .size(ControlSize::Small),
                        )
                        .key(node_key(
                            "package-editor-problem",
                            &format!("{}:{}:{i}", d.path, d.offset),
                        ))
                        .on_cx(send(move || Action::Reveal(target.clone()))),
                        widget(Stack::column(0.).shrink(1.).padding_xy(0., gap::TIGHT))
                            .children(widget(readable(d.message.clone()))),
                    ))
                    .into_any()
                })
                .collect::<Vec<_>>();
            let title = if rows.is_empty() {
                ui.now_t(if view.validating {
                    "pkgedit-validating"
                } else {
                    "pkgedit-no-problems"
                })
            } else {
                ui.now_t("pkgedit-problems")
            };
            widget(
                Stack::column(0.0)
                    .gap_step(nana_ui_core::SpacingStep::Xs)
                    .with_layout(|l| {
                        l.max_height = Some(LengthSpec::Px(LIST_MAX_H));
                        l.flex_shrink = Some(0.);
                    }),
            )
            .key("package-editor-problems")
            .children((
                hint(title),
                widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|l| {
                    l.max_height = Some(LengthSpec::Px(PROBLEMS_MAX_H));
                }))
                .children(widget(Stack::column(0.)).children(rows)),
            ))
            .into_any()
        },
    )
    .into_any()
}

// ---- package metadata --------------------------------------------------------

fn package(ui: Ui) -> AnyView {
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
fn labelled(caption: String, width: f32, control: impl IntoView) -> AnyView {
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

fn form_editable(ui: Ui, file: &str) -> bool {
    ui.package_editor.with_untracked(|v| {
        v.as_ref()
            .is_some_and(|v| v.files.iter().any(|f| f.path == file && f.editable))
    })
}

fn field(ui: Ui, key: &str, value: Signal<String>, placeholder: &str) -> AnyView {
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
        (
            (
                label(ui.now_t("pkgedit-data")),
                data_section(ui, view, editable),
            ),
            (
                label(ui.now_t("pkgedit-pipelines")),
                pipeline_section(ui, view),
            ),
        ),
    ))
    .into_any()
}

/// `None`: no form; `Some(None)`: a new entry; `Some(Some(name))`: editing it.
type Editing = Signal<Option<Option<String>>>;

/// One declaration, with `buttons` when it can be edited here.
fn entry_row(key: &str, summary: String, buttons: Option<AnyView>) -> AnyView {
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
fn entry_buttons(
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

fn form_buttons(
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

fn form(key: &str, children: impl IntoView) -> AnyView {
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

fn notice(ui: Ui, key: &str, detail: impl std::fmt::Display) -> Msg {
    Msg::Notice {
        text: format!("{}: {detail}", ui.now_t(key)),
        error: true,
    }
}

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

// ---- signing -------------------------------------------------------------------

fn signing(ui: Ui) -> AnyView {
    dynamic(
        move || {
            ui.package_editor.with(|v| {
                v.as_ref().map(|v| {
                    (
                        format!("{:?}", v.seal),
                        v.key.clone(),
                        v.key_checked,
                        Arc::as_ptr(&v.files) as usize,
                    )
                })
            })
        },
        move |_| {
            let Some(view) = view_of(ui) else {
                return widget(Stack::column(0.)).into_any();
            };
            let status = crate::ui::plugin_view::integrity_label(*ui, &view.seal);
            let key_row = match (&view.key, view.key_checked) {
                (Some(fingerprint), _) => widget(readable(format!(
                    "{}: {}",
                    ui.now_t("pkgedit-key-fingerprint"),
                    fingerprint
                )))
                .key("package-editor-key")
                .into_any(),
                (None, false) => natural(
                    widget(button(ui, "pkgedit-key-load"))
                        .key("package-editor-key-load")
                        .on_cx(send(|| Action::LoadKey)),
                ),
                (None, true) => widget(Stack::row(0.0).gap_step(nana_ui_core::SpacingStep::Md).wrap(true).align(AlignSpec::Center))
                    .children((
                        hint(ui.now_t("pkgedit-key-none")),
                        widget(button(ui, "pkgedit-key-create"))
                            .key("package-editor-key-create")
                            .on_cx(send(|| Action::CreateKey)),
                    ))
                    .into_any(),
            };
            let signed = matches!(view.seal, Ok(Some(_)));
            let own = matches!((&view.seal, &view.key), (Ok(Some(seal)), Some(key)) if &seal.fingerprint == key);
            let derive_id = signal(format!("{}-copy", view.package.chars().take(56).collect::<String>()));
            let derive = widget(Stack::row(0.0).gap_step(nana_ui_core::SpacingStep::Md).wrap(true).align(AlignSpec::Center)).children((
                input_box(
                    field::MD,
                    widget(kit::text_input(derive_id.get_untracked()).size(ControlSize::Small))
                        .key("package-editor-derive-id")
                        .on(move |e: &TextChanged| derive_id.set(e.value.to_string())),
                ),
                widget(button(ui, "pkgedit-derive"))
                    .key("package-editor-derive")
                    .on_cx(send(move || Action::Derive(derive_id.get_untracked()))),
            ));
            let body: AnyView = if signed {
                widget(Stack::column(0.0).gap_step(nana_ui_core::SpacingStep::Md))
                    .children((
                        hint(ui.now_t(if own { "pkgedit-signed-own" } else { "pkgedit-signed-other" })),
                        own.then(|| {
                            natural(
                                widget(button(ui, "pkgedit-unseal"))
                                    .key("package-editor-unseal")
                                    .on_cx(send(|| Action::Unseal)),
                            )
                        }),
                        derive,
                    ))
                    .into_any()
            } else {
                let protected = signal(BTreeSet::<String>::new());
                let rows = view
                    .files
                    .iter()
                    .filter(|f| f.dependency.is_none() && f.path != nanatype_theme::INTEGRITY_FILE)
                    .map(|f| {
                        let path = f.path.clone();
                        let manifest = path == PACKAGE_MANIFEST;
                        widget(Checkbox::new(path.clone(), manifest).disabled(manifest))
                            .key(node_key("package-editor-protect", &path))
                            .on(move |e: &ToggleChanged| {
                                protected.update(|p| {
                                    if e.checked {
                                        p.insert(path.clone());
                                    } else {
                                        p.remove(&path);
                                    }
                                })
                            })
                            .into_any()
                    })
                    .collect::<Vec<_>>();
                let can_sign = view.key.is_some();
                widget(Stack::column(0.0).gap_step(nana_ui_core::SpacingStep::Sm))
                    .children((
                        hint(ui.now_t("pkgedit-protect")),
                        widget(Stack::column(0.0).gap_step(nana_ui_core::SpacingStep::Xs)).children(rows),
                        natural(
                            widget(button(ui, "pkgedit-sign").kind(ButtonKind::Primary))
                                .key("package-editor-sign")
                                .bind(move |b: &mut Button| b.disabled = !can_sign)
                                .on_cx(send(move || Action::Sign(protected.get_untracked()))),
                        ),
                        hint(ui.now_t("pkgedit-derive-hint")),
                        derive,
                    ))
                    .into_any()
            };
            widget(Stack::column(0.0).gap_step(nana_ui_core::SpacingStep::Lg).padding_xy(0., space::SM))
                .key("package-editor-signing")
                .children((
                    widget(readable(status)).key("package-editor-seal"),
                    hint(ui.now_t("pkgedit-seal-note")),
                    key_row,
                    hint(ui.now_t("pkgedit-key-note")),
                    body,
                ))
                .into_any()
        },
    )
    .into_any()
}

// ---- preview -------------------------------------------------------------------

fn preview(ui: Ui) -> AnyView {
    super::preview_panel::view(ui)
}

// ---- isolated Lua debugging --------------------------------------------------

fn debug(ui: Ui) -> AnyView {
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
            widget(
                Stack::column(0.0)
                    .gap_step(nana_ui_core::SpacingStep::Lg)
                    .padding_xy(0., space::SM),
            )
            .key("package-editor-debug")
            .children((
                widget(readable(ui.now_t("pkgedit-debug-risk")).color(SemanticColorRole::Warning)),
                widget(Checkbox::new(
                    ui.now_t("pkgedit-debug-trust"),
                    debug.trusted,
                ))
                .key("package-editor-debug-trust")
                .on_cx(|_, e: &ToggleChanged, cx| {
                    cx.dispatch_program_all(Msg::PackageEditor(Action::Trust(e.checked)))
                }),
                widget(
                    Stack::row(0.0)
                        .gap_step(nana_ui_core::SpacingStep::Sm)
                        .wrap(true),
                )
                .children((
                    widget(button(ui, "pkgedit-debug-start").kind(ButtonKind::Primary))
                        .key("package-editor-debug-start")
                        .bind(move |b: &mut Button| b.disabled = !debug.trusted)
                        .on_cx(send(|| Action::DebugStart)),
                    widget(button(ui, "pkgedit-debug-stop"))
                        .key("package-editor-debug-stop")
                        .bind(move |b: &mut Button| b.disabled = !debug.running)
                        .on_cx(send(|| Action::DebugStop)),
                    widget(button(ui, "pkgedit-debug-clear"))
                        .key("package-editor-debug-clear")
                        .on_cx(send(|| Action::DebugClear)),
                )),
                hint(ui.now_t("pkgedit-debug-events")),
                widget(
                    Stack::row(0.0)
                        .gap_step(nana_ui_core::SpacingStep::Sm)
                        .wrap(true),
                )
                .children(events),
                widget(
                    Stack::row(0.0)
                        .gap_step(nana_ui_core::SpacingStep::Sm)
                        .wrap(true)
                        .align(AlignSpec::Center),
                )
                .children((
                    input_box(
                        field::LG,
                        widget(kit::text_input(custom.get_untracked()).size(ControlSize::Small))
                            .key("package-editor-debug-custom")
                            .on(move |e: &TextChanged| custom.set(e.value.to_string())),
                    ),
                    widget(button(ui, "pkgedit-debug-send"))
                        .key("package-editor-debug-send")
                        .on_cx(move |_, _: &Activate, cx| {
                            let msg = match serde_json::from_str::<serde_json::Value>(
                                &custom.get_untracked(),
                            ) {
                                Ok(event) if event.get("type").is_some_and(|t| t.is_string()) => {
                                    Msg::PackageEditor(Action::DebugEvent(event))
                                }
                                _ => Msg::Notice {
                                    text: ui.now_t("pkgedit-debug-invalid-json"),
                                    error: true,
                                },
                            };
                            cx.dispatch_program_all(msg);
                        }),
                )),
                debug.error.as_ref().map(|e| {
                    let location = debug.location.clone();
                    widget(Stack::column(0.0).gap_step(nana_ui_core::SpacingStep::Xs))
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
                                widget(Button::new(label).size(ControlSize::Small))
                                    .key("package-editor-debug-locate")
                                    .on_cx(move |_, _: &Activate, cx| {
                                        cx.dispatch_program_all(Msg::PackageEditor(
                                            Action::Reveal(at.clone()),
                                        ));
                                        ui.package_editor_tab.set(Tab::Files);
                                    })
                            }),
                        ))
                }),
                (
                    label(ui.now_t("pkgedit-debug-output")),
                    widget(
                        Stack::column(0.)
                            .padding_step(
                                nana_ui_core::SpacingStep::Md,
                                nana_ui_core::SpacingStep::Md,
                            )
                            .surface(SemanticColorRole::Subtle)
                            .radius(RadiusTier::Sm),
                    )
                    .key("package-editor-debug-output")
                    .children((
                        output,
                        debug
                            .update
                            .as_ref()
                            .and_then(|u| serde_json::to_string_pretty(u).ok())
                            .map(|s| widget(readable(s))),
                    )),
                ),
                (
                    label(ui.now_t("pkgedit-debug-log")),
                    widget(
                        kit::text_input(ui.debug_filter.get_untracked())
                            .placeholder(ui.now_t("pkgedit-log-filter"))
                            .size(ControlSize::Small),
                    )
                    .on(move |e: &TextChanged| ui.debug_filter.set(e.value.to_string())),
                    dynamic(
                        move || ui.debug_filter.get(),
                        move |filter| {
                            widget(readable(
                                debug
                                    .log
                                    .iter()
                                    .filter(|line| {
                                        line.to_lowercase().contains(&filter.to_lowercase())
                                    })
                                    .cloned()
                                    .collect::<Vec<_>>()
                                    .join("\n"),
                            ))
                            .key("package-editor-debug-log")
                            .into_any()
                        },
                    ),
                ),
                (
                    hint(ui.now_t("pkgedit-event-sequence")),
                    widget(kit::text_area(ui.debug_sequence.get_untracked()).height(88.))
                        .on(move |e: &TextChanged| ui.debug_sequence.set(e.value.to_string())),
                    widget(button(ui, "pkgedit-replay"))
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
                ),
            ))
            .into_any()
        },
    )
    .into_any()
}
