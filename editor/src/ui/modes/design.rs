//! The design workspace: the skin's declared tables as layers, the live
//! canvas (optionally beside the selected source), and the inspector.
//! Selecting a layer, a part on the canvas or a table in the source is one
//! selection.
use crate::msg::Msg;
use crate::session::Action;
use crate::ui::controls::hint;
use crate::ui::kit::{self, gap};
use crate::ui::shell::{self, panel_header, tool};
use crate::ui::{Ui, canvas_controls, preview_panel, properties};
use nana_ui::icons_tabler as ti;
use nana_ui::runtime::view::{AnyView, IntoView, dynamic, node_ref, watch_effect, widget};
use nana_ui::runtime::{
    Activate, IconButton, ScrollAxes, ScrollView, Stack, Toolbar, TreeSelectionChanged, TreeView,
};
use nana_ui_core::{
    AlignSpec, JustifySpec, LengthSpec, SemanticColorRole, SplitAxis, SplitPaneModel, TreeNode,
};
use nanatype_theme::{MANIFEST, TomlTable, editor::PropertyValue as Value};
use std::sync::Arc;

/// The source under the canvas when split: default, least and most height.
const SOURCE_H: f32 = 320.0;
const SOURCE_LIMITS: (f32, f32) = (140.0, 900.0);
/// The canvas strip's selects.
const MOTION_W: f32 = 136.0;
const ZOOM_W: f32 = 112.0;
const BACKGROUND_W: f32 = 120.0;
/// The parameter test values under the layers, before they scroll.
const PARAMS_MAX_H: f32 = 240.0;

/// The canvas part a layer path draws, if any: `dialogue/nameplate` is the
/// `nameplate` part, `dialogue/decor/2` the `decor.2` part.
pub fn part_of(path: &[String]) -> Option<String> {
    match path {
        [root, part]
            if root == "dialogue"
                && matches!(part.as_str(), "nameplate" | "portrait" | "continue") =>
        {
            Some(part.clone())
        }
        [root, decor, index] if root == "dialogue" && decor == "decor" => {
            index.parse::<usize>().ok().map(|i| format!("decor.{i}"))
        }
        _ => None,
    }
}

/// The layer path that declares a canvas part.
pub fn path_of(part: &str) -> Option<Vec<String>> {
    if let Some(index) = part.strip_prefix("decor.") {
        return Some(vec!["dialogue".into(), "decor".into(), index.into()]);
    }
    matches!(part, "nameplate" | "portrait" | "continue")
        .then(|| vec!["dialogue".into(), part.into()])
}

fn layer_label(ui: Ui, key: &str) -> String {
    // Keep message ids explicit so the locale audit sees the keys we ship.
    let message = match key {
        "dialogue" => "pkgedit-layer-dialogue",
        "nameplate" => "pkgedit-layer-nameplate",
        "portrait" => "pkgedit-layer-portrait",
        "continue" => "pkgedit-layer-continue",
        "text" => "pkgedit-layer-text",
        "decor" => "pkgedit-layer-decor",
        "history" => "pkgedit-layer-history",
        "chrome" => "pkgedit-layer-chrome",
        "effects" => "pkgedit-layer-effects",
        "fonts" => "pkgedit-layer-fonts",
        "materials" => "pkgedit-layer-materials",
        "mention" => "pkgedit-layer-mention",
        "params" => "pkgedit-layer-params",
        "param_groups" => "pkgedit-layer-param_groups",
        "presets" => "pkgedit-layer-presets",
        "shaders" => "pkgedit-layer-shaders",
        "sounds" => "pkgedit-layer-sounds",
        "responsive" => "pkgedit-layer-responsive",
        "pipelines" => "pkgedit-layer-pipelines",
        "frame" => "pkgedit-layer-frame",
        _ => return key.to_owned(),
    };
    format!("{} · {key}", ui.now_t(message))
}

fn layer_icon(key: &str) -> nana_ui::runtime::Icon {
    match key {
        "dialogue" => ti::MESSAGE,
        "nameplate" => ti::TAG,
        "portrait" => ti::USER_CIRCLE,
        "continue" => ti::PLAYER_PLAY,
        "text" => ti::LETTER_T,
        "decor" => ti::SPARKLES,
        "history" => ti::HISTORY,
        "fonts" => ti::TYPOGRAPHY,
        "materials" => ti::TEXTURE,
        "shaders" | "effects" => ti::WAND,
        "params" | "param_groups" | "presets" | "param_checks" => ti::ADJUSTMENTS,
        "sounds" => ti::MUSIC,
        "responsive" => ti::DEVICE_MOBILE,
        "pipelines" => ti::TRANSITION_RIGHT,
        "mention" => ti::AT,
        _ => ti::SQUARE,
    }
}

const SEP: &str = "/";

/// Tables (and arrays of tables) become layers; plain values live in the
/// inspector. `dialogue` leads, as the part most skins are about.
fn layers(ui: Ui, table: &TomlTable, selected: &[String]) -> Vec<TreeNode<Arc<str>>> {
    fn node(
        ui: Ui,
        path: Vec<String>,
        label: String,
        icon: nana_ui::runtime::Icon,
        value: &Value,
        selected: &[String],
    ) -> Option<TreeNode<Arc<str>>> {
        let id = Arc::<str>::from(path.join(SEP));
        let on_path = selected.starts_with(&path);
        let children = match value {
            Value::Table(t) if !t.contains_key("param") => children(ui, &path, t, selected),
            Value::Array(items)
                if !items.is_empty() && items.iter().all(|i| matches!(i, Value::Table(_))) =>
            {
                items
                    .iter()
                    .enumerate()
                    .filter_map(|(i, item)| {
                        let mut child = path.clone();
                        child.push(i.to_string());
                        let name = item
                            .as_table()
                            .and_then(|t| {
                                ["id", "name", "text", "image"]
                                    .iter()
                                    .find_map(|k| t.get(*k).and_then(Value::as_str))
                            })
                            .map(|n| format!("{} · {n}", i + 1))
                            .unwrap_or_else(|| (i + 1).to_string());
                        node(ui, child, name, icon, item, selected)
                    })
                    .collect()
            }
            _ => return None,
        };
        Some(if children.is_empty() {
            TreeNode::leaf(id, label)
                .icon(icon)
                .selected(selected == path.as_slice())
        } else {
            let expanded = on_path || path == ["dialogue"];
            TreeNode::branch(id, label, expanded, children)
                .icon(icon)
                .selected(selected == path.as_slice())
        })
    }
    fn children(
        ui: Ui,
        path: &[String],
        table: &TomlTable,
        selected: &[String],
    ) -> Vec<TreeNode<Arc<str>>> {
        let mut keys = table.keys().filter(|k| *k != "meta").collect::<Vec<_>>();
        keys.sort_by_key(|k| (k.as_str() != "dialogue", k.as_str()));
        keys.into_iter()
            .filter_map(|key| {
                let mut child = path.to_vec();
                child.push(key.clone());
                node(
                    ui,
                    child,
                    layer_label(ui, key),
                    layer_icon(key),
                    &table[key.as_str()],
                    selected,
                )
            })
            .collect()
    }
    children(ui, &[], table, selected)
}

/// Select a layer: the inspector follows, the canvas selects the part it
/// draws, and a split source scrolls to its table.
pub fn select(ui: Ui, path: Vec<String>) -> Option<Msg> {
    ui.properties_file.set(MANIFEST.into());
    ui.properties_path.set(path.clone());
    ui.canvas_selection.set(part_of(&path));
    if !ui.split_source.get_untracked() {
        return None;
    }
    let text = ui
        .package_editor
        .with_untracked(|v| v.as_ref().and_then(|v| v.documents.get(MANIFEST).cloned()))?;
    let (start, _) = properties::table_range(&text, &path)?;
    let (line, column) = nanatype_theme::editor::line_column(&text, start);
    Some(Msg::PackageEditor(Action::Reveal(
        nanatype_theme::editor::SourceDiagnostic {
            path: MANIFEST.into(),
            offset: start,
            len: 0,
            line,
            column,
            level: nanatype_theme::editor::Level::Warning,
            message: String::new(),
        },
    )))
}

/// Decoration operations on the selected part, or adding one.
fn decor_tools(ui: Ui) -> AnyView {
    let op = move |icon, label: &str, operation: &'static str, needs: bool| {
        widget(tool(icon, ui.now_t(label)))
            .key(format!("editor-layer-{operation}"))
            .bind(move |b: &mut IconButton| {
                b.disabled = needs
                    && !ui
                        .canvas_selection
                        .with(|s| s.as_ref().is_some_and(|s| s.starts_with("decor.")))
            })
            .on_cx(move |_, _: &Activate, cx| {
                let element = if needs {
                    ui.canvas_selection.get_untracked().unwrap_or_default()
                } else {
                    String::new()
                };
                cx.dispatch_program_all(Msg::PackageEditor(Action::CanvasList {
                    element,
                    operation: operation.into(),
                    target: ui.canvas_target.get_untracked(),
                }))
            })
            .into_any()
    };
    widget(
        Stack::row(0.)
            .gap_step(gap::step::HAIR)
            .align(AlignSpec::Center),
    )
    .children((
        op(ti::PLUS, "pkgedit-add-decor", "add", false),
        op(ti::COPY, "pkgedit-duplicate", "duplicate", true),
        op(ti::ARROW_UP, "pkgedit-layer-up", "up", true),
        op(ti::ARROW_DOWN, "pkgedit-layer-down", "down", true),
        op(ti::TRASH, "pkgedit-remove", "remove", true),
    ))
    .into_any()
}

pub fn side(ui: Ui) -> AnyView {
    let tree = dynamic(
        move || {
            (
                ui.properties_path.get(),
                ui.package_editor.with(|v| {
                    v.as_ref().map(|v| {
                        (
                            v.documents.get(MANIFEST).cloned(),
                            Arc::as_ptr(&v.properties) as usize,
                        )
                    })
                }),
            )
        },
        move |(selected, _)| {
            let Some(view) = ui.package_editor.get_untracked() else {
                return widget(Stack::column(0.)).into_any();
            };
            let table = match properties::effective(&view, MANIFEST) {
                Ok((table, _)) => table,
                Err(e) => return hint(e),
            };
            widget(TreeView::new(layers(ui, &table, selected)).label(ui.now_t("pkgedit-layers")))
                .key("editor-layers")
                .on_cx(move |_, e: &TreeSelectionChanged, cx| {
                    if let Some(id) = &e.active {
                        let path = id.split(SEP).map(str::to_owned).collect::<Vec<_>>();
                        if let Some(msg) = select(ui, path) {
                            cx.dispatch_program_all(msg);
                        }
                    }
                })
                .into_any()
        },
    );
    widget(Stack::fill_column(0.))
        .key("editor-design-side")
        .children((
            panel_header(ui.now_t("pkgedit-layers"), decor_tools(ui)),
            widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|l| {
                l.height = Some(LengthSpec::Px(0.));
                l.flex_grow = Some(1.);
                l.min_height = Some(LengthSpec::Px(0.));
            }))
            .children(widget(Stack::column(0.).padding_xy(gap::TIGHT, 0.)).children(tree)),
            kit::rule(),
            panel_header(ui.now_t("pkgedit-preview-params"), ()),
            widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|l| {
                l.max_height = Some(LengthSpec::Px(PARAMS_MAX_H));
                l.flex_shrink = Some(0.);
            }))
            .children(
                widget(Stack::column(0.).padding_xy(gap::CONTROL, gap::TIGHT))
                    .children(preview_panel::params(ui)),
            ),
        ))
        .into_any()
}

/// The stage on the neutral canvas, with its strip above and the sample
/// text below.
fn canvas(ui: Ui) -> AnyView {
    let strip = widget(
        Toolbar::new()
            .chrome(false)
            .label(ui.now_t("pkgedit-canvas")),
    )
    .key("editor-canvas-strip")
    .children(
        widget(
            Stack::row(0.)
                .gap_step(gap::step::TIGHT)
                .align(AlignSpec::Center)
                .width(LengthSpec::Fill)
                .padding_xy(gap::CONTROL, gap::TIGHT),
        )
        .children((
            shell::sized(MOTION_W, preview_panel::motion(ui)),
            preview_panel::replay(ui),
            kit::vrule(),
            shell::sized(ZOOM_W, preview_panel::zoom(ui)),
            shell::sized(BACKGROUND_W, preview_panel::background(ui)),
            preview_panel::settings(ui),
            widget(
                Stack::row(0.)
                    .grow(1.)
                    .shrink(1.)
                    .min_width(LengthSpec::Px(0.))
                    .justify(JustifySpec::End)
                    .gap_step(gap::step::TIGHT)
                    .align(AlignSpec::Center),
            )
            .children((preview_panel::stale(ui), canvas_controls::matched(ui))),
        )),
    );
    let stage = widget(ScrollView::new(ScrollAxes::Vertical).with_layout(|l| {
        l.height = Some(LengthSpec::Px(0.));
        l.flex_grow = Some(1.);
        l.min_height = Some(LengthSpec::Px(0.));
        l.width = Some(LengthSpec::Fill);
    }))
    .key("editor-canvas")
    .children(
        widget(
            Stack::column(0.)
                .width(LengthSpec::Fill)
                .align(AlignSpec::Center)
                .padding_step(kit::inset::step::CARD, kit::inset::step::CARD),
        )
        .children(dynamic(
            move || {
                ui.package_editor
                    .with(|v| v.as_ref().is_some_and(|v| v.report.skin.is_some()))
            },
            move |skin| {
                if *skin {
                    preview_panel::stage(ui)
                } else {
                    hint(ui.now_t("pkgedit-preview-none"))
                }
            },
        )),
    );
    widget(Stack::fill_column(0.).surface(SemanticColorRole::Background))
        .children((
            strip,
            kit::rule(),
            stage,
            kit::rule(),
            widget(
                Stack::row(0.)
                    .width(LengthSpec::Fill)
                    .padding_xy(gap::CONTROL, gap::TIGHT)
                    .surface(SemanticColorRole::Surface),
            )
            .children(preview_panel::sample(ui)),
        ))
        .into_any()
}

pub fn center(ui: Ui) -> AnyView {
    // A part picked on the canvas selects its layer.
    watch_effect(move || {
        if let Some(path) = ui.canvas_selection.get().as_deref().and_then(path_of)
            && !ui.properties_path.with_untracked(|p| p.starts_with(&path))
        {
            ui.properties_file.set(MANIFEST.into());
            ui.properties_path.set(path);
        }
    });
    let split = node_ref();
    dynamic(
        move || ui.split_source.get(),
        move |split_source| {
            if !*split_source {
                return canvas(ui);
            }
            shell::split(
                split,
                SplitPaneModel::new(
                    SplitAxis::Vertical,
                    SOURCE_H,
                    SOURCE_LIMITS.0,
                    SOURCE_LIMITS.1,
                )
                .with_from_end(true),
                "editor-design-split",
                shell::pane(canvas(ui)),
                shell::pane(super::files::center(ui)),
            )
        },
    )
    .into_any()
}

pub fn inspector(ui: Ui) -> AnyView {
    widget(Stack::fill_column(0.))
        .key("editor-design-inspector")
        .children((
            panel_header(ui.now_t("pkgedit-inspector"), ()),
            shell::scroll(properties::inspector(ui)),
        ))
        .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parts_and_layer_paths_round_trip() {
        for part in ["nameplate", "portrait", "continue", "decor.3"] {
            assert_eq!(part_of(&path_of(part).unwrap()).as_deref(), Some(part));
        }
        assert_eq!(part_of(&["dialogue".into(), "text".into()]), None);
    }
}
