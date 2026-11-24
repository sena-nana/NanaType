//! Canvas gestures edit a private resolved skin while dragging and commit once.
use crate::{session::Action, ui::Ui};
use nana_ui_platform::{InputPayload, KeyState, PointerPhase};
use nanatype_theme::{
    Len, PackageProject, Skin, anchor_frac,
    editor::{self, PropertyValue},
};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct Edit {
    pub element: String,
    pub target: Vec<String>,
    pub size: [f32; 2],
    pub delta: [f32; 2],
    pub resize: bool,
}

pub struct Drag {
    start: [f32; 2],
    scale: f32,
    edit: Edit,
    skin: Arc<Skin>,
}

fn point(rect: [f32; 4], p: [f32; 2]) -> bool {
    p[0] >= rect[0] && p[1] >= rect[1] && p[0] <= rect[0] + rect[2] && p[1] <= rect[1] + rect[3]
}

fn resized(len: Len, delta: f32, axis: f32) -> Len {
    match len {
        Len::Px(v) => Len::Px((v + delta).max(1.)),
        Len::Percent(v) => Len::Percent((v + delta / axis.max(1.) * 100.).max(0.1)),
    }
}

pub fn preview(skin: &Skin, edit: &Edit) -> Result<Skin, String> {
    let mut skin = nanatype_theme::layout_dialogue(skin, edit.size);
    skin.dialogue.responsive = skin
        .dialogue
        .responsive
        .as_ref()
        .map(|_| Default::default());
    let [dx, dy] = edit.delta;
    if let Some(index) = edit
        .element
        .strip_prefix("decor.")
        .and_then(|s| s.parse::<usize>().ok())
    {
        let d = skin
            .dialogue
            .decor
            .get_mut(index)
            .ok_or("decoration no longer exists")?;
        if let Some([t, r, b, l]) = d.inset {
            d.inset = Some(if edit.resize {
                [t, r - dx, b - dy, l]
            } else {
                [t + dy, r - dx, b - dy, l + dx]
            });
        } else if edit.resize {
            let radians = -d.rotate.to_radians();
            let (sn, cs) = radians.sin_cos();
            let (dx, dy) = (dx * cs - dy * sn, dx * sn + dy * cs);
            let old = d.size.map(|v| match v {
                Len::Px(v) => v,
                Len::Percent(v) => v,
            });
            d.size = [
                resized(d.size[0], dx, edit.size[0]),
                resized(d.size[1], dy, edit.size[1]),
            ];
            let fraction = anchor_frac(d.anchor);
            for i in 0..2 {
                let (before, after) = match d.size[i] {
                    Len::Px(v) => (old[i], v),
                    Len::Percent(v) => (old[i] * edit.size[i] / 100., v * edit.size[i] / 100.),
                };
                d.offset[i] += (after - before) * fraction[i];
            }
        } else {
            d.offset[0] += dx;
            d.offset[1] += dy;
        }
    } else {
        let offset = match edit.element.as_str() {
            "nameplate" => &mut skin.dialogue.nameplate.offset,
            "portrait" => &mut skin.dialogue.portrait.offset,
            "continue" => &mut skin.dialogue.continue_.offset,
            _ => return Err("select an editable element".into()),
        };
        offset[0] += dx;
        offset[1] += if edit.element == "portrait" { -dy } else { dy };
    }
    Ok(skin)
}

pub fn commit(
    project: &mut PackageProject,
    skin: &Skin,
    authored: &nanatype_theme::TomlTable,
    edit: &Edit,
) -> Result<(), String> {
    let mut next = project.clone();
    let before = nanatype_theme::layout_dialogue(skin, edit.size);
    let after = preview(skin, edit)?;
    let mut inherited = editor::authored_layout(authored, edit.size);
    // Seed the selected layer once, preserving the full authored array and its bindings.
    if edit.target != ["dialogue".to_owned()]
        && let Some(list) =
            editor::value_at(&inherited, &["dialogue".into(), "decor".into()]).cloned()
    {
        let mut layer = nanatype_theme::TomlTable::from_iter([("decor".into(), list)]);
        for key in edit.target.iter().rev() {
            layer = [(key.clone(), PropertyValue::Table(layer))]
                .into_iter()
                .collect();
        }
        nanatype_theme::deep_merge(&mut inherited, &layer);
    }
    let patch_field = |next: &mut PackageProject,
                       field: &str,
                       value: Option<PropertyValue>|
     -> Result<(), String> {
        let mut target = edit.target.clone();
        target.extend(edit.element.split('.').map(str::to_owned));
        target.push(field.into());
        editor::set_declaration(
            next,
            "skin.toml",
            &target,
            value.as_ref().map(editor::literal).as_deref(),
            Some(&inherited),
            false,
        )
        .map_err(|e| e.to_string())
    };
    if let Some(index) = edit
        .element
        .strip_prefix("decor.")
        .and_then(|s| s.parse::<usize>().ok())
    {
        let prev = before
            .dialogue
            .decor
            .get(index)
            .ok_or("decoration no longer exists")?;
        let curr = after
            .dialogue
            .decor
            .get(index)
            .ok_or("decoration no longer exists")?;
        if prev.offset != curr.offset {
            let val = PropertyValue::try_from(curr.offset).map_err(|e| e.to_string())?;
            patch_field(&mut next, "offset", Some(val))?;
        }
        if prev.size != curr.size {
            let val = PropertyValue::try_from(curr.size).map_err(|e| e.to_string())?;
            patch_field(&mut next, "size", Some(val))?;
        }
        if prev.inset != curr.inset {
            let val = curr
                .inset
                .map(PropertyValue::try_from)
                .transpose()
                .map_err(|e| e.to_string())?;
            patch_field(&mut next, "inset", val)?;
        }
    } else {
        let (prev_offset, curr_offset) = match edit.element.as_str() {
            "nameplate" => (
                before.dialogue.nameplate.offset,
                after.dialogue.nameplate.offset,
            ),
            "portrait" => (
                before.dialogue.portrait.offset,
                after.dialogue.portrait.offset,
            ),
            "continue" => (
                before.dialogue.continue_.offset,
                after.dialogue.continue_.offset,
            ),
            _ => return Err("select an editable element".into()),
        };
        if prev_offset != curr_offset {
            let val = PropertyValue::try_from(curr_offset).map_err(|e| e.to_string())?;
            patch_field(&mut next, "offset", Some(val))?;
        }
    }
    *project = next;
    Ok(())
}

/// Refuse a gesture hidden by another responsive override. The committed
/// renderer must reproduce the feedback shown during the gesture.
pub fn check_result(before: &Skin, after: &Skin, edit: &Edit) -> Result<(), String> {
    fn decor_matches(a: &Skin, b: &Skin, index: usize) -> bool {
        let idx = |s: &Skin| {
            if s.dialogue.responsive.is_some() {
                s.decor_indices.iter().position(|v| *v == index)
            } else {
                Some(index)
            }
        };
        match (
            idx(a).and_then(|i| a.dialogue.decor.get(i)),
            idx(b).and_then(|i| b.dialogue.decor.get(i)),
        ) {
            (Some(x), Some(y)) => x == y,
            (None, None) => true,
            _ => false,
        }
    }
    let expected = nanatype_theme::responsive_dialogue(&preview(before, edit)?, edit.size);
    let actual = nanatype_theme::responsive_dialogue(after, edit.size);
    let matched = if let Some(index) = edit
        .element
        .strip_prefix("decor.")
        .and_then(|s| s.parse::<usize>().ok())
    {
        decor_matches(&expected, &actual, index)
    } else {
        match edit.element.as_str() {
            "nameplate" => expected.dialogue.nameplate == actual.dialogue.nameplate,
            "portrait" => expected.dialogue.portrait == actual.dialogue.portrait,
            "continue" => expected.dialogue.continue_ == actual.dialogue.continue_,
            _ => false,
        }
    };
    if !matched {
        return Err("the selected layer is overridden at this preview size; select the matching orientation or breakpoint layer".into());
    }
    Ok(())
}

pub fn input(ui: Ui, event: &InputPayload, drag: &mut Option<Drag>) -> Option<Action> {
    let proj = ui.proj();
    if !proj.canvas_enabled.get_untracked() {
        return None;
    }
    let view = proj.package_editor.get_untracked()?;
    if view.validating || view.preview_stale {
        *drag = None;
        proj.canvas_preview.set(None);
        return None;
    }
    let skin = view.report.skin.clone()?;
    let geometry = proj.canvas_geometry.get_untracked();
    let rect = geometry.iter().find(|p| p.key == "canvas")?.rect;
    let size = super::ui::preview::draft(
        proj.preview_host,
        &skin.meta.id,
        Some(skin.dialogue.overscan),
    )
    .display;
    let size = [
        size.width
            .parse()
            .unwrap_or_else(|_| super::ui::preview::default_size(&skin, false)[0]),
        size.height
            .parse()
            .unwrap_or_else(|_| super::ui::preview::default_size(&skin, false)[1]),
    ];
    let make = |element: String, delta: [f32; 2], resize: bool| Edit {
        element,
        target: proj.canvas_target.get_untracked(),
        size,
        delta,
        resize,
    };
    match event {
        InputPayload::Pointer(p) => match p.phase {
            PointerPhase::Down if p.button == 0 => {
                proj.canvas_focus.set(point(rect, [p.x, p.y]));
                if !proj.canvas_focus.get_untracked() {
                    return None;
                }
                proj.preview_mode.set(0);
                let selected = proj
                    .canvas_selection
                    .get_untracked()
                    .and_then(|key| geometry.iter().find(|part| part.key == key));
                let resize = selected.is_some_and(|part| {
                    part.key.starts_with("decor.")
                        && point(
                            [
                                part.rect[0] + part.rect[2] - 8.,
                                part.rect[1] + part.rect[3] - 8.,
                                16.,
                                16.,
                            ],
                            [p.x, p.y],
                        )
                });
                let part = if resize {
                    selected
                } else {
                    geometry
                        .iter()
                        .filter(|part| part.key != "canvas" && point(part.rect, [p.x, p.y]))
                        .min_by(|a, b| (a.rect[2] * a.rect[3]).total_cmp(&(b.rect[2] * b.rect[3])))
                };
                proj.canvas_selection.set(part.map(|p| p.key.clone()));
                if let Some(part) = part {
                    *drag = Some(Drag {
                        start: [p.x, p.y],
                        scale: (rect[2] / size[0]).max(0.001),
                        edit: make(part.key.clone(), [0., 0.], resize),
                        skin,
                    });
                }
            }
            PointerPhase::Move => {
                if let Some(d) = drag.as_mut() {
                    d.edit.delta = [(p.x - d.start[0]) / d.scale, (p.y - d.start[1]) / d.scale];
                    if let Ok(skin) = preview(&d.skin, &d.edit) {
                        proj.canvas_preview.set(Some(Arc::new(skin)));
                    }
                }
            }
            PointerPhase::Up => {
                if let Some(d) = drag.take() {
                    if d.edit.delta.iter().any(|v| v.abs() > 0.01) {
                        return Some(Action::CanvasEdit(d.edit));
                    }
                    proj.canvas_preview.set(None);
                }
            }
            PointerPhase::Cancel => {
                *drag = None;
                proj.canvas_preview.set(None);
            }
            _ => {}
        },
        InputPayload::Key(k)
            if k.state == KeyState::Pressed && proj.canvas_focus.get_untracked() =>
        {
            if k.logical.0 == "Escape" {
                *drag = None;
                proj.canvas_preview.set(None);
                return None;
            }
            let step = if k.modifiers.shift { 10. } else { 1. };
            let delta = match k.logical.0.as_ref() {
                "ArrowLeft" => [-step, 0.],
                "ArrowRight" => [step, 0.],
                "ArrowUp" => [0., -step],
                "ArrowDown" => [0., step],
                _ => return None,
            };
            return proj
                .canvas_selection
                .get_untracked()
                .map(|key| Action::CanvasEdit(make(key, delta, false)));
        }
        _ => {}
    }
    None
}
