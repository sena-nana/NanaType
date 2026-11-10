//! The editor's frame around an open project: a toolbar, the activity rail,
//! the workspace of the current [`Mode`], a drawer for problems and the Lua
//! log, and the status bar. Only the draft changes; installed packages, the
//! active skin and running plugins never do.
use super::modes;
use super::{Drawer, Ui};
use crate::msg::Msg;
use crate::session::{Action, EditorView, Mode};
use crate::shortcuts::hinted;
use crate::ui::controls::hint;
use crate::ui::kit::{self, Tone, Type, gap};
use crate::ui::views::{button, kind_key, node_key, readable, send, view_of};
use nana_ui::runtime::view::{AnyView, IntoView, dynamic, watch_effect, widget};
use nana_ui::runtime::{
    ActionMenu, ActionMenuItem, Activate, Button, IconButton, PopoverClosed, PopoverToggled,
    ScrollAxes, ScrollView, SizeChanged, SplitPane, Stack, StatusBar, Text, Toolbar,
};
use nana_ui::{ButtonKind, ControlSize, icons_tabler as ti};
use nana_ui_core::{AlignSpec, LengthSpec, SemanticColorRole, SplitAxis, SplitPaneModel};
use nanatype_theme::editor::{AssetKind, DraftOrigin, Level};
use std::collections::BTreeSet;
use std::sync::Arc;

/// Below this editor width the side panel folds away until asked for.
pub const COMPACT: f32 = 1280.0;
/// The smallest window the workspace is laid out for.
pub const MIN_SIZE: [f32; 2] = [960.0, 600.0];

/// [`MIN_SIZE`] at a content zoom, for the window's size limits.
pub fn minimum_size(zoom: f32) -> (f64, f64) {
    let [w, h] = MIN_SIZE.map(|v| f64::from((v * zoom).round()));
    (w, h)
}
/// Side panel and inspector: default, least and most width.
pub const SIDE_W: f32 = 240.0;
const SIDE_LIMITS: (f32, f32) = (180.0, 360.0);
pub const INSPECTOR_W: f32 = 300.0;
const INSPECTOR_LIMITS: (f32, f32) = (240.0, 440.0);
/// The open drawer: default, least and most height.
const DRAWER_H: f32 = 200.0;
const DRAWER_LIMITS: (f32, f32) = (96.0, 560.0);
/// The problems the drawer lists before it says how many more there are.
const PROBLEMS_SHOWN: usize = 200;

/// The workspace showing: the rail's choice, or Files when the package has
/// nothing for it.
pub fn current(ui: Ui) -> Mode {
    let mode = ui.mode.get();
    ui.package_editor
        .with(|v| v.as_ref().map_or(mode, |v| mode.effective(v)))
}

fn mode_key(mode: Mode) -> &'static str {
    match mode {
        Mode::Design => "pkgedit-mode-design",
        Mode::Files => "pkgedit-tab-files",
        Mode::Sounds => "pkgedit-tab-sounds",
        Mode::Data => "pkgedit-mode-data",
        Mode::Lua => "pkgedit-mode-lua",
        Mode::Package => "pkgedit-tab-package",
        Mode::Signing => "pkgedit-tab-signing",
    }
}

fn mode_icon(mode: Mode) -> nana_ui::runtime::Icon {
    match mode {
        Mode::Design => ti::PALETTE,
        Mode::Files => ti::FILES,
        Mode::Sounds => ti::MUSIC,
        Mode::Data => ti::DATABASE,
        Mode::Lua => ti::SCRIPT,
        Mode::Package => ti::PACKAGE,
        Mode::Signing => ti::ROCKET,
    }
}

/// A compact icon button with its label as tooltip.
pub(crate) fn tool(icon: nana_ui::runtime::Icon, label: String) -> IconButton {
    IconButton::new(icon, label.clone())
        .size(ControlSize::Small)
        .with_tooltip(label)
}

/// A control kept `width` wide in a row.
pub(crate) fn sized(width: f32, control: impl IntoView) -> AnyView {
    widget(
        Stack::row(0.)
            .width(LengthSpec::Px(width))
            .shrink(1.)
            .min_width(LengthSpec::Px(0.)),
    )
    .children(control)
    .into_any()
}

/// A flexible gap that pushes what follows to the row's end.
pub(crate) fn spacer() -> AnyView {
    widget(
        Stack::row(0.)
            .grow(1.)
            .shrink(1.)
            .min_width(LengthSpec::Px(0.)),
    )
    .into_any()
}

/// Open `mode`; choosing the open mode again shows or hides the side panel.
/// Each workspace comes back to the file it last showed; the Lua and sounds
/// workspaces otherwise open one of their own.
pub fn select_mode(ui: Ui, mode: Mode) -> Option<Msg> {
    let previous = ui.mode.get_untracked();
    if previous == mode {
        ui.side_open.update(|open| *open = !*open);
        return None;
    }
    let view = ui.package_editor.get_untracked()?;
    ui.mode_files.update(|m| {
        m.retain(|(k, _)| *k != previous);
        if let Some(path) = &view.selected {
            m.push((previous, path.clone()));
        }
    });
    ui.mode.set(mode);
    ui.side_open.set(true);
    match mode {
        Mode::Sounds => {
            ui.properties_file
                .set(modes::sounds::declared_in(&view).into());
            ui.properties_path.set(vec!["sounds".into()]);
        }
        Mode::Design => ui.properties_file.set(nanatype_theme::MANIFEST.into()),
        _ => {}
    }
    let wanted = |f: &nanatype_theme::editor::FileInfo| match mode {
        Mode::Lua => f.kind == AssetKind::Lua,
        Mode::Sounds => f.kind == AssetKind::Audio,
        _ => true,
    };
    let fits = |path: &str| view.files.iter().any(|f| f.path == path && wanted(f));
    let remembered = ui.mode_files.with_untracked(|m| {
        m.iter()
            .find(|(k, _)| *k == mode)
            .map(|(_, path)| path.clone())
    });
    let next = remembered.filter(|p| fits(p)).or_else(|| {
        // Only the workspaces of one kind of file need one of that kind.
        if !matches!(mode, Mode::Lua | Mode::Sounds) || view.selected.as_deref().is_some_and(fits) {
            return None;
        }
        view.files
            .iter()
            .find(|f| wanted(f) && f.dependency.is_none())
            .map(|f| f.path.clone())
    })?;
    (view.selected.as_deref() != Some(next.as_str()))
        .then(|| Msg::PackageEditor(Action::Select(next)))
}

/// Read the panes' widths back after input routing, so a drag survives
/// the workspace being rebuilt for another mode.
pub fn sync_widths(ui: Ui, cx: &nana_ui::runtime::AppContext) {
    for (node, width) in [
        (ui.side_split, ui.side_width),
        (ui.inspector_split, ui.inspector_width),
    ] {
        let Some(id) = node.get_untracked() else {
            continue;
        };
        let pane = nana_ui::runtime::Entity::<SplitPane>::from_stable_id(id);
        if let Ok(size) = cx.read(pane, |p| p.model.size()) {
            width.set_if_changed(size);
        }
    }
}

pub fn editor(ui: Ui) -> AnyView {
    // Gestures and geometry belong to the design canvas.
    watch_effect(move || {
        ui.canvas_enabled
            .set_if_changed(current(ui) == Mode::Design);
    });
    widget(Stack::fill_column(0.0).with_layout(|l| l.min_width = Some(LengthSpec::Px(0.0))))
        .key("package-editor")
        .on(move |e: &SizeChanged| {
            let was = ui.workspace_width.get_untracked();
            if (was < COMPACT) != (e.width < COMPACT) {
                ui.side_open.set(e.width >= COMPACT);
            }
            ui.workspace_width.set_if_changed(e.width);
        })
        .children((
            toolbar(ui),
            super::conflicts::view(ui),
            close_prompt(ui),
            derive_prompt(ui),
            widget(
                Stack::row(0.)
                    .align(AlignSpec::Stretch)
                    .grow(1.)
                    .shrink(1.)
                    .width(LengthSpec::Fill)
                    .with_layout(|l| l.min_height = Some(LengthSpec::Px(0.))),
            )
            .key("editor-workspace-body")
            .children((rail(ui), edge(), body(ui))),
            status_bar(ui),
        ))
        .into_any()
}

// ---- toolbar ---------------------------------------------------------------

fn toolbar(ui: Ui) -> AnyView {
    dynamic(
        move || {
            (
                current(ui),
                ui.package_editor.with(|v| {
                    v.as_ref()
                        .map(|v| (v.package.clone(), v.kind, v.dirty && v.can_revert))
                }),
            )
        },
        move |(mode, state)| {
            let Some((package, kind, revertible)) = state.clone() else {
                return widget(Stack::row(0.)).into_any();
            };
            toolbar_row(ui, *mode, package, kind, revertible)
        },
    )
    .into_any()
}

/// The project's own commands on the left, the workspace's layout and the
/// way out on the right. How the stage is looked at belongs to the canvas.
fn toolbar_row(
    ui: Ui,
    mode: Mode,
    package: String,
    kind: nanatype_theme::editor::ProjectKind,
    revertible: bool,
) -> AnyView {
    let open = nana_ui::runtime::view::signal(false);
    let item = move |key: &'static str, label: String, disabled: bool, action: fn() -> Action| {
        widget({
            let mut item = ActionMenuItem::new(label);
            item.disabled = disabled;
            item
        })
        .key(key)
        .on_cx(move |_, _: &Activate, cx| {
            open.set(false);
            cx.dispatch_program_all(Msg::PackageEditor(action()));
        })
    };
    let project = widget(
        ActionMenu::new()
            .trigger(package)
            .accessible_name(ui.now_t("pkgedit-project-menu")),
    )
    .key("package-editor-project")
    .bind(move |m: &mut ActionMenu| m.popover.open = open.get())
    .on(move |e: &PopoverToggled| open.set(e.open))
    .on(move |_: &PopoverClosed| open.set(false))
    .children((
        item(
            "package-editor-menu-save",
            hinted(ui.now_t("pkgedit-save"), "Mod+S"),
            false,
            || Action::Save,
        ),
        item(
            "package-editor-menu-export",
            hinted(ui.now_t("pkgedit-export"), "Mod+E"),
            false,
            || Action::Export,
        ),
        item(
            "package-editor-revert",
            ui.now_t("pkgedit-revert"),
            !revertible,
            || Action::Revert,
        ),
        // A copy under a new ID is a matter of the whole project.
        widget(ActionMenuItem::new(ui.now_t("pkgedit-derive-title")))
            .key("package-editor-menu-derive")
            .on(move |_: &Activate| {
                open.set(false);
                ui.derive_open.set(true);
            }),
        item(
            "package-editor-close",
            hinted(ui.now_t("pkgedit-close"), "Mod+W"),
            false,
            || Action::Close,
        ),
    ));
    let history = (
        widget(tool(
            ti::ARROW_BACK_UP,
            hinted(ui.now_t("pkgedit-undo"), "Mod+Z"),
        ))
        .key("package-editor-undo")
        .bind(move |b: &mut IconButton| {
            b.disabled = !ui
                .package_editor
                .with(|v| v.as_ref().is_some_and(|v| v.undo))
        })
        .on_cx(send(|| Action::Undo)),
        widget(tool(
            ti::ARROW_FORWARD_UP,
            hinted(ui.now_t("pkgedit-redo"), "Mod+Shift+Z"),
        ))
        .key("package-editor-redo")
        .bind(move |b: &mut IconButton| {
            b.disabled = !ui
                .package_editor
                .with(|v| v.as_ref().is_some_and(|v| v.redo))
        })
        .on_cx(send(|| Action::Redo)),
    );
    let layout = match mode {
        Mode::Design => Some(
            widget(tool(ti::LAYOUT_ROWS, ui.now_t("pkgedit-split-source")))
                .key("editor-split-source")
                .bind(move |b: &mut IconButton| b.selected = ui.split_source.get())
                .on_cx(move |_, _: &Activate, cx| {
                    let open = !ui.split_source.get_untracked();
                    ui.split_source.set(open);
                    if open {
                        // Open the skin's source at the selected layer.
                        let path = ui.properties_path.get_untracked();
                        let msg = modes::design::select(ui, path).unwrap_or_else(|| {
                            Msg::PackageEditor(Action::Select(nanatype_theme::MANIFEST.into()))
                        });
                        cx.dispatch_program_all(msg);
                    }
                }),
        ),
        Mode::Files if files_previewable(ui) => Some(
            widget(tool(
                ti::LAYOUT_SIDEBAR_RIGHT,
                ui.now_t("pkgedit-files-preview"),
            ))
            .key("editor-files-preview")
            .bind(move |b: &mut IconButton| b.selected = ui.files_preview.get())
            .on(move |_: &Activate| ui.files_preview.update(|s| *s = !*s)),
        ),
        _ => None,
    };
    widget(
        Toolbar::new()
            .label(ui.now_t("pkgedit-toolbar"))
            .style(toolbar_style()),
    )
    .key("package-editor-header")
    .children((
        project,
        kit::tag(ui.now_t(kind_key(kind))),
        kit::vrule(),
        history,
        spacer(),
        layout,
        widget(tool(
            ti::LAYOUT_SIDEBAR,
            hinted(ui.now_t("pkgedit-side-toggle"), "Mod+B"),
        ))
        .key("editor-side-toggle")
        .bind(move |b: &mut IconButton| b.selected = ui.side_open.get())
        .on(move |_: &Activate| ui.side_open.update(|s| *s = !*s)),
        kit::vrule(),
        widget(tool(
            ti::UPLOAD,
            hinted(ui.now_t("pkgedit-export"), "Mod+E"),
        ))
        .key("package-editor-export")
        .on_cx(send(|| Action::Export)),
        // Filled while there is something to save.
        widget(tool(
            ti::DEVICE_FLOPPY,
            hinted(ui.now_t("pkgedit-save"), "Mod+S"),
        ))
        .key("package-editor-save")
        .bind(move |b: &mut IconButton| {
            let kind = if ui
                .package_editor
                .with(|v| v.as_ref().is_some_and(|v| v.dirty))
            {
                ButtonKind::Primary
            } else {
                ButtonKind::Ghost
            };
            if b.kind != kind {
                b.kind = kind;
            }
        })
        .on_cx(send(|| Action::Save)),
    ))
    .into_any()
}

/// The files workspace has a stage to show: the package draws a skin.
fn files_previewable(ui: Ui) -> bool {
    ui.package_editor
        .with(|v| v.as_ref().is_some_and(|v| Mode::Design.available(v)))
}

fn toolbar_style() -> nana_ui::runtime::NodeStyle {
    let mut style = Toolbar::new().style;
    let l = Arc::make_mut(&mut style.layout);
    l.width = Some(LengthSpec::Fill);
    l.flex_shrink = Some(0.);
    l.align_items = AlignSpec::Center;
    l.gap = Some(LengthSpec::Px(gap::TIGHT));
    style
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
                    .padding_step(kit::inset::step::PANEL, kit::inset::step::BAR_Y)
                    .surface_mix(nana_ui_core::SemanticColorMix::alpha(
                        SemanticColorRole::Warning,
                        kit::wash::TINT,
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

/// The new package's ID, asked for under the toolbar: the copy drops the
/// signature and records where it came from.
fn derive_prompt(ui: Ui) -> AnyView {
    dynamic(
        move || ui.derive_open.get(),
        move |open| {
            let package = ui
                .package_editor
                .with_untracked(|v| v.as_ref().map(|v| v.package.clone()));
            let (true, Some(package)) = (*open, package) else {
                return widget(Stack::column(0.)).into_any();
            };
            let id = nana_ui::runtime::view::signal(format!(
                "{}-copy",
                package.chars().take(56).collect::<String>()
            ));
            widget(
                Stack::row(0.0)
                    .gap_step(gap::step::CONTROL)
                    .wrap(true)
                    .align(AlignSpec::Center)
                    .padding_step(kit::inset::step::PANEL, kit::inset::step::BAR_Y)
                    .surface(SemanticColorRole::Surface),
            )
            .key("package-editor-derive-prompt")
            .children((
                widget(readable(ui.now_t("pkgedit-derive-id"))),
                super::views::input_box(
                    kit::field::MD,
                    widget(kit::text_input(id.get_untracked()).size(ControlSize::Small))
                        .key("package-editor-derive-id")
                        .on(move |e: &nana_ui::runtime::TextChanged| id.set(e.value.to_string()))
                        .on_cx(move |_, e: &nana_ui::runtime::TextSubmitted, cx| {
                            ui.derive_open.set(false);
                            cx.dispatch_program_all(Msg::PackageEditor(Action::Derive(
                                e.value.clone(),
                            )));
                        }),
                ),
                kit::hint_mark(ui.now_t("pkgedit-derive-hint")),
                widget(button(ui, "pkgedit-derive").kind(ButtonKind::Primary))
                    .key("package-editor-derive")
                    .on_cx(move |_, _: &Activate, cx| {
                        ui.derive_open.set(false);
                        cx.dispatch_program_all(Msg::PackageEditor(Action::Derive(
                            id.get_untracked(),
                        )));
                    }),
                widget(button(ui, "common-cancel"))
                    .on(move |_: &Activate| ui.derive_open.set(false)),
            ))
            .into_any()
        },
    )
    .into_any()
}

// ---- activity rail -----------------------------------------------------------

/// A full-height hairline between the rail and the workspace.
fn edge() -> AnyView {
    widget(
        Stack::column(0.)
            .width(LengthSpec::Px(kit::HAIRLINE))
            .height(LengthSpec::Fill)
            .shrink(0.)
            .surface(SemanticColorRole::Border),
    )
    .into_any()
}

fn rail(ui: Ui) -> AnyView {
    let entry = move |mode: Mode| {
        let number = crate::shortcuts::WORKSPACES
            .iter()
            .position(|m| *m == mode)
            .map_or(0, |i| i + 1);
        widget(
            tool(
                mode_icon(mode),
                hinted(ui.now_t(mode_key(mode)), &format!("Mod+{number}")),
            )
            .size(ControlSize::Medium),
        )
        .key(format!("package-editor-mode:{}", mode_key(mode)))
        .visible(move || {
            ui.package_editor
                .with(|v| v.as_ref().is_some_and(|v| mode.available(v)))
        })
        .bind(move |b: &mut IconButton| {
            let on = current(ui) == mode;
            if b.selected != on {
                b.selected = on;
            }
        })
        .on_cx(move |_, _: &Activate, cx| {
            if let Some(msg) = select_mode(ui, mode) {
                cx.dispatch_program_all(msg);
            }
        })
        .into_any()
    };
    widget(
        Stack::column(0.)
            .width(LengthSpec::Shrink)
            .height(LengthSpec::Fill)
            .gap_step(gap::step::TIGHT)
            .align(AlignSpec::Center)
            .padding_xy(gap::TIGHT, gap::CONTROL)
            .shrink(0.)
            .surface(SemanticColorRole::Surface),
    )
    .key("package-editor-rail")
    .children((
        [
            Mode::Design,
            Mode::Files,
            Mode::Sounds,
            Mode::Data,
            Mode::Lua,
        ]
        .into_iter()
        .map(entry)
        .collect::<Vec<_>>(),
        spacer_column(),
        [Mode::Package, Mode::Signing]
            .into_iter()
            .map(entry)
            .collect::<Vec<_>>(),
    ))
    .into_any()
}

fn spacer_column() -> AnyView {
    widget(
        Stack::column(0.)
            .width(LengthSpec::Px(0.))
            .grow(1.)
            .min_height(LengthSpec::Px(0.)),
    )
    .into_any()
}

// ---- workspace -----------------------------------------------------------------

/// The workspace over the drawer: one split while the drawer is open, so
/// the problems and the log take as much height as they are given.
fn body(ui: Ui) -> AnyView {
    let node = nana_ui::runtime::view::node_ref();
    widget(Stack::fill_column(0.).with_layout(|l| {
        l.width = Some(LengthSpec::Px(0.));
        l.flex_grow = Some(1.);
        l.min_width = Some(LengthSpec::Px(0.));
    }))
    .children(
        dynamic(
            move || ui.drawer.get() != Drawer::Closed,
            move |open| {
                if !*open {
                    return workspace(ui);
                }
                split(
                    node,
                    SplitPaneModel::new(
                        SplitAxis::Vertical,
                        DRAWER_H,
                        DRAWER_LIMITS.0,
                        DRAWER_LIMITS.1,
                    )
                    .with_from_end(true),
                    "editor-drawer-split",
                    pane(workspace(ui)),
                    pane(drawer(ui)),
                )
            },
        )
        .grow(),
    )
    .into_any()
}

fn workspace(ui: Ui) -> AnyView {
    widget(Stack::fill_column(0.).with_layout(|l| {
        l.min_width = Some(LengthSpec::Px(0.));
        l.min_height = Some(LengthSpec::Px(0.));
    }))
    .children(
        dynamic(
            move || {
                (
                    current(ui),
                    ui.side_open.get(),
                    ui.files_preview.get(),
                    ui.package_editor.with(Option::is_some),
                )
            },
            move |(mode, side, preview, open)| {
                if !*open {
                    return widget(Stack::column(0.)).into_any();
                }
                let side = side.then_some(());
                match mode {
                    Mode::Design => panes(
                        ui,
                        side.map(|_| modes::design::side(ui)),
                        modes::design::center(ui),
                        Some(modes::design::inspector(ui)),
                    ),
                    Mode::Files => panes(
                        ui,
                        side.map(|_| modes::files::side(ui)),
                        modes::files::center(ui),
                        (*preview && files_previewable(ui)).then(|| modes::files::preview(ui)),
                    ),
                    Mode::Sounds => panes(
                        ui,
                        side.map(|_| modes::sounds::side(ui)),
                        modes::sounds::center(ui),
                        Some(modes::sounds::inspector(ui)),
                    ),
                    Mode::Lua => panes(
                        ui,
                        side.map(|_| modes::lua::side(ui)),
                        modes::lua::center(ui),
                        Some(modes::lua::inspector(ui)),
                    ),
                    // The list is this workspace's way in: it does not fold.
                    Mode::Data => panes(
                        ui,
                        Some(modes::data::side(ui)),
                        modes::data::center(ui),
                        None,
                    ),
                    Mode::Package => scroll(modes::package::view(ui)),
                    Mode::Signing => scroll(modes::signing::view(ui)),
                }
            },
        )
        .grow(),
    )
    .into_any()
}

/// A pane that fills its slot and scrolls nothing itself.
pub(crate) fn pane(content: impl IntoView) -> AnyView {
    widget(Stack::fill_column(0.).with_layout(|l| {
        l.min_width = Some(LengthSpec::Px(0.));
        l.min_height = Some(LengthSpec::Px(0.));
    }))
    .children(content)
    .into_any()
}

/// Side panel | centre | inspector, the sides resizable.
fn panes(ui: Ui, side: Option<AnyView>, center: AnyView, inspector: Option<AnyView>) -> AnyView {
    let rest = match inspector {
        Some(inspector) => split(
            ui.inspector_split,
            SplitPaneModel::new(
                SplitAxis::Horizontal,
                ui.inspector_width.get_untracked(),
                INSPECTOR_LIMITS.0,
                INSPECTOR_LIMITS.1,
            )
            .with_from_end(true),
            "editor-inspector-split",
            pane(center),
            widget(Stack::fill_column(0.).surface(SemanticColorRole::Surface))
                .key("editor-inspector")
                .children(inspector)
                .into_any(),
        ),
        None => pane(center),
    };
    match side {
        Some(side) => split(
            ui.side_split,
            SplitPaneModel::new(
                SplitAxis::Horizontal,
                ui.side_width.get_untracked(),
                SIDE_LIMITS.0,
                SIDE_LIMITS.1,
            ),
            "editor-side-split",
            widget(Stack::fill_column(0.).surface(SemanticColorRole::Surface))
                .key("editor-side")
                .children(side)
                .into_any(),
            pane(rest),
        ),
        None => rest,
    }
}

pub(crate) fn split(
    node: nana_ui::runtime::view::NodeRef,
    model: SplitPaneModel,
    key: &'static str,
    first: AnyView,
    second: AnyView,
) -> AnyView {
    let mut pane = SplitPane::new(&model);
    {
        let l = Arc::make_mut(&mut pane.style.layout);
        l.flex_grow = Some(1.0);
        l.flex_shrink = Some(1.0);
        l.width = Some(LengthSpec::Fill);
        l.min_width = Some(LengthSpec::Px(0.0));
        l.min_height = Some(LengthSpec::Px(0.0));
    }
    widget(pane)
        .key(key)
        .node_ref(node)
        .first(first)
        .second(second)
        .into_any()
}

/// Content that scrolls within the remaining height.
pub(crate) fn scroll(content: AnyView) -> AnyView {
    widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|l| {
        l.flex_grow = Some(1.0);
        l.flex_shrink = Some(1.0);
        l.height = Some(LengthSpec::Px(0.0));
        l.min_height = Some(LengthSpec::Px(0.0));
        l.width = Some(LengthSpec::Fill);
    }))
    .key("editor-panel-scroll")
    .children(content)
    .into_any()
}

/// A panel's small caption with optional tools at its end.
pub(crate) fn panel_header(title: String, tools: impl IntoView) -> AnyView {
    widget(
        Stack::row(0.)
            .gap_step(gap::step::TIGHT)
            .align(AlignSpec::Center)
            .width(LengthSpec::Fill)
            .padding_xy(gap::CONTROL, gap::TIGHT)
            .shrink(0.),
    )
    .children((
        widget(
            Stack::row(0.)
                .grow(1.)
                .shrink(1.)
                .min_width(LengthSpec::Px(0.)),
        )
        .children(widget(Type::Overline.text(title))),
        tools,
    ))
    .into_any()
}

// ---- drawer and status -------------------------------------------------------------

fn diagnostics(view: &EditorView) -> Vec<nanatype_theme::editor::SourceDiagnostic> {
    let mut seen = BTreeSet::new();
    view.syntax
        .iter()
        .chain(view.debug.location.iter())
        .chain(view.report.diagnostics.iter())
        .filter(|d| seen.insert((d.path.clone(), d.offset, d.message.clone())))
        .cloned()
        .collect()
}

fn drawer(ui: Ui) -> AnyView {
    dynamic(
        move || {
            (
                ui.drawer.get(),
                ui.package_editor
                    .with(|v| v.as_ref().is_some_and(|v| Mode::Lua.available(v))),
            )
        },
        move |(drawer, lua)| {
            if *drawer == Drawer::Closed {
                return widget(Stack::column(0.)).into_any();
            }
            let tab = move |which: Drawer, key: &str| {
                widget(button(ui, key).kind(if *drawer == which {
                    ButtonKind::Selected
                } else {
                    ButtonKind::Ghost
                }))
                .key(format!("package-editor-drawer:{key}"))
                .on(move |_: &Activate| ui.drawer.set(which))
                .into_any()
            };
            let body = match drawer {
                Drawer::Debug => modes::lua::log(ui),
                _ => problems(ui),
            };
            widget(Stack::fill_column(0.).surface(SemanticColorRole::Surface))
                .key("package-editor-drawer")
                .children((
                    widget(
                        Stack::row(0.)
                            .gap_step(gap::step::TIGHT)
                            .align(AlignSpec::Center)
                            .padding_xy(gap::CONTROL, gap::HAIR),
                    )
                    .children((
                        tab(Drawer::Problems, "pkgedit-problems"),
                        lua.then(|| tab(Drawer::Debug, "pkgedit-debug-log")),
                        spacer(),
                        widget(tool(ti::X, ui.now_t("pkgedit-drawer-close")))
                            .key("package-editor-drawer-close")
                            .on(move |_: &Activate| ui.drawer.set(Drawer::Closed)),
                    )),
                    scroll(
                        widget(Stack::column(0.).padding_xy(gap::CONTROL, 0.))
                            .children(body)
                            .into_any(),
                    ),
                ))
                .into_any()
        },
    )
    .into_any()
}

/// Located diagnostics; each jumps to its file and line.
pub(crate) fn problems(ui: Ui) -> AnyView {
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
            let all = diagnostics(&view);
            let more = all.len().saturating_sub(PROBLEMS_SHOWN);
            let rows =
                all.into_iter()
                    .take(PROBLEMS_SHOWN)
                    .enumerate()
                    .map(|(i, d)| {
                        let error = d.level == Level::Error;
                        let location = format!("{}:{}:{}", d.path, d.line, d.column);
                        let key = node_key(
                            "package-editor-problem",
                            &format!("{}:{}:{i}", d.path, d.offset),
                        );
                        let message = d.message.clone();
                        // The mark says error or warning by shape and by name.
                        let mark = if error {
                            ti::ALERT_CIRCLE
                        } else {
                            ti::ALERT_TRIANGLE
                        };
                        widget(
                            Stack::row(0.0)
                                .gap_step(gap::step::TIGHT)
                                .align(AlignSpec::Start)
                                .width(LengthSpec::Fill),
                        )
                        .children((
                            widget(
                                Button::new(location)
                                    .icon(mark)
                                    .kind(ButtonKind::Ghost)
                                    .accessible_name(format!(
                                        "{} {}:{}",
                                        ui.now_t(if error {
                                            "pkgedit-problem-error"
                                        } else {
                                            "pkgedit-problem-warning"
                                        }),
                                        d.path,
                                        d.line
                                    ))
                                    .size(ControlSize::Small),
                            )
                            .key(key)
                            .on_cx(move |_, _: &Activate, cx| {
                                cx.dispatch_program_all(reveal(ui, d.clone()));
                            }),
                            widget(Stack::column(0.).shrink(1.).padding_xy(0., gap::TIGHT))
                                .children(widget(readable(message).color(if error {
                                    SemanticColorRole::Danger
                                } else {
                                    SemanticColorRole::Text
                                }))),
                        ))
                        .into_any()
                    })
                    .collect::<Vec<_>>();
            if rows.is_empty() {
                return hint(ui.now_t(if view.validating {
                    "pkgedit-validating"
                } else {
                    "pkgedit-no-problems"
                }));
            }
            widget(Stack::column(0.))
                .key("package-editor-problems")
                .children((
                    rows,
                    (more > 0).then(|| {
                        hint(ui.tr("pkgedit-problems-more", &[("count", (more as i64).into())]))
                    }),
                ))
                .into_any()
        },
    )
    .into_any()
}

/// Show a located diagnostic's source: beside the canvas when it is split,
/// otherwise in the files workspace.
pub(crate) fn reveal(ui: Ui, at: nanatype_theme::editor::SourceDiagnostic) -> Msg {
    if !(current(ui) == Mode::Design && ui.split_source.get_untracked()) {
        ui.mode.set(Mode::Files);
    }
    Msg::PackageEditor(Action::Reveal(at))
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
            "{origin} · {}: {} · {}",
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

fn status_bar(ui: Ui) -> AnyView {
    dynamic(
        move || {
            ui.package_editor.with(|v| {
                v.as_ref().map(|v| {
                    (
                        origin_text(ui, v),
                        status_text(ui, v),
                        v.dirty,
                        v.report.errors(),
                        diagnostics(v).len(),
                        Mode::Lua.available(v),
                    )
                })
            })
        },
        move |state| {
            let Some((origin, status, dirty, errors, problems, lua)) = state.clone() else {
                return widget(Stack::row(0.)).into_any();
            };
            let toggle = move |which: Drawer| {
                move |_: &Activate| {
                    ui.drawer.update(|d| {
                        *d = if *d == which { Drawer::Closed } else { which };
                    })
                }
            };
            let tone = if errors > 0 {
                Tone::Danger
            } else if dirty {
                Tone::Warning
            } else {
                Tone::Success
            };
            widget(
                StatusBar::new()
                    .label(ui.now_t("pkgedit-status"))
                    .style(status_style()),
            )
            .key("package-editor-status-bar")
            .children((
                widget(
                    Button::new(format!("{} {problems}", ui.now_t("pkgedit-problems")))
                        .icon(if errors > 0 {
                            ti::ALERT_CIRCLE
                        } else {
                            ti::ALERT_TRIANGLE
                        })
                        .kind(ButtonKind::Ghost)
                        .size(ControlSize::Small),
                )
                .key("package-editor-problems-toggle")
                .on(toggle(Drawer::Problems)),
                lua.then(|| {
                    widget(
                        Button::new(ui.now_t("pkgedit-debug-log"))
                            .icon(ti::TERMINAL_2)
                            .kind(ButtonKind::Ghost)
                            .size(ControlSize::Small),
                    )
                    .key("package-editor-debug-toggle")
                    .on(toggle(Drawer::Debug))
                }),
                widget(
                    Stack::row(0.)
                        .grow(1.)
                        .shrink(1.)
                        .min_width(LengthSpec::Px(0.))
                        .padding_xy(gap::CONTROL, 0.),
                )
                .children(widget(
                    // One line that gives way: at its natural width it ran
                    // over the status beside it and out of the window.
                    Text::new(origin)
                        .style(Type::Caption.one_line())
                        .color(SemanticColorRole::Muted)
                        .truncating(),
                )),
                kit::status_dot(tone),
                widget(Text::new(status).style(Type::Caption.style())).key("package-editor-status"),
            ))
            .into_any()
        },
    )
    .into_any()
}

fn status_style() -> nana_ui::runtime::NodeStyle {
    let mut style = StatusBar::new().style;
    let l = Arc::make_mut(&mut style.layout);
    l.width = Some(LengthSpec::Fill);
    l.flex_shrink = Some(0.);
    l.align_items = AlignSpec::Center;
    l.gap = Some(LengthSpec::Px(gap::TIGHT));
    style
}
