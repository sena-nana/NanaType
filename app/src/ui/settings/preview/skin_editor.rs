use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, LazyLock, Mutex, OnceLock};

use nana_ui::ButtonKind;
use nana_ui::icons_tabler as ti;
use nana_ui::runtime::view::{AnyView, IntoView, dynamic, widget};
use nana_ui::runtime::{
    Activate, Button, ColorChanged, ColorField, Icon, IconButton, NumberChanged, NumberInput,
    RangeField, RangeInput, RangeMarker, Select, SelectChanged, SelectOption, SizeField,
    SizeFieldChanged, SizePreset, Stack,
};
use nana_ui_core::ControlSize;
use nana_ui_core::{AlignSpec, LengthSpec};
use nanatype_model::Rgba8;
use nanatype_theme::Skin;

use super::preview_session::{change, draft};
use super::{Draft, Pane, default_size};
use crate::msg::{Msg, SkinMsg};
use crate::ui::Ui;
use crate::ui::kit::{self, gap};
use crate::ui::settings::controls::{hint, section};

/// The body size (px) and line height (× size) the profile accepts.
const SIZE_RANGE: (f64, f64) = (8.0, 200.0);
const LINE_RANGE: (f64, f64) = (1.0, 3.0);
const LINE_STEP: f64 = 0.05;
/// The window sizes a profile accepts, both sides in one field (the width
/// also needs 160: below it the draft reads as invalid).
const WINDOW_RANGE: (f64, f64) = (96.0, 8192.0);
/// The skin's own window size scaled, offered beside it as presets (%).
const WINDOW_SCALES: [u32; 4] = [75, 125, 150, 200];

/// The line heights the slider snaps to: tight, comfortable, double and
/// the ends. The skin's own value joins them (see [`line_marks`]).
const LINE_SNAPS: [f64; 6] = [1.0, 1.2, 1.5, 2.0, 2.5, 3.0];

static FONT_FAMILIES: OnceLock<Vec<String>> = OnceLock::new();
static FONT_AVAILABILITY: LazyLock<Mutex<HashMap<String, bool>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

fn font_families() -> &'static [String] {
    FONT_FAMILIES.get_or_init(|| {
        let mut families: Vec<String> = nanatype_theme::BuiltinFonts::shipped()
            .fonts()
            .iter()
            .filter_map(|f| f.family())
            .collect();
        families.extend(
            [
                "Microsoft YaHei UI",
                "SimSun",
                "Yu Gothic UI",
                "Meiryo UI",
                "PingFang SC",
                "Noto Sans SC",
                "Noto Sans JP",
                "Arial",
                "Consolas",
            ]
            .map(str::to_owned),
        );
        families.sort();
        families.dedup();
        families
    })
}

fn surface_key(base: &str, history: bool) -> String {
    format!("{base}-{}", if history { "history" } else { "display" })
}

/// `value` as a draft field: empty when it is the skin's own `default`, so
/// the field keeps following the skin.
fn unless_default(value: f32, default: f32) -> String {
    if (value - default).abs() < 1e-4 {
        String::new()
    } else {
        value.to_string()
    }
}

/// A control at the one width every narrow row's control takes.
fn dense_width(l: &mut nana_ui_core::LayoutStyle) {
    l.width = Some(LengthSpec::Px(kit::field::DENSE));
    l.max_width = Some(LengthSpec::Percent(100.0));
    l.min_width = Some(LengthSpec::Px(0.0));
    l.flex_shrink = Some(0.0);
}

/// A small icon action labelled, and tooltipped, by `key`.
pub(super) fn action(ui: Ui, icon: Icon, key: &str) -> IconButton {
    IconButton::new(icon, ui.now_t(key))
        .size(ControlSize::Small)
        .with_tooltip(ui.now_t(key))
}

/// The body size: a number field, the skin's size while the draft has none.
fn size_field(ui: Ui, id: &str, history: bool, default: f32) -> AnyView {
    let read_id = id.to_owned();
    let write_id = id.to_owned();
    let read = move || {
        draft(ui, &read_id)
            .pane(history)
            .size
            .trim()
            .parse::<f32>()
            .unwrap_or(default)
    };
    // The value last shown, so only outside changes (reset, discard)
    // replace what is being typed.
    let shown = Arc::new(AtomicU32::new(read().to_bits()));
    let mut input = kit::number_input(f64::from(read()))
        .range(SIZE_RANGE.0, SIZE_RANGE.1)
        .step(1.0)
        .label(ui.now_t("skin-edit-size"))
        .size(ControlSize::Small);
    dense_width(Arc::make_mut(&mut input.style.layout));
    kit::dense_row(
        ui.now_t("skin-edit-size"),
        widget(input)
            .key(surface_key("skin-edit-size", history))
            .bind(move |input: &mut NumberInput| {
                let v = read();
                if shown.swap(v.to_bits(), Ordering::Relaxed) != v.to_bits() {
                    input.assign(f64::from(v));
                }
            })
            .on(move |e: &NumberChanged| {
                change(ui, &write_id, |d| {
                    d.pane_mut(history).size = unless_default(e.value as f32, default)
                });
            }),
    )
}

/// The window's width × height in one field, with the skin's own size and
/// its scales as presets; the skin's size while the draft has none.
fn resolution_field(ui: Ui, id: &str, history: bool, default: [f32; 2]) -> AnyView {
    let read_id = id.to_owned();
    let write_id = id.to_owned();
    let read = move || {
        let d = draft(ui, &read_id);
        let pane = d.pane(history);
        [&pane.width, &pane.height]
            .into_iter()
            .zip(default)
            .map(|(typed, default)| f64::from(typed.trim().parse::<f32>().unwrap_or(default)))
            .collect::<Vec<_>>()
    };
    let size = |v: Vec<f64>| [v[0], v[1]];
    let to = |v: [f32; 2]| v.map(|side| f64::from(side.round()));
    let accepts = |[w, h]: [f64; 2]| {
        (160.0..=WINDOW_RANGE.1).contains(&w) && (WINDOW_RANGE.0..=WINDOW_RANGE.1).contains(&h)
    };
    let [w, h] = to(default);
    let presets = std::iter::once(SizePreset::new(
        format!("{} · {w}×{h}", ui.now_t("common-skin-default")),
        [w, h],
    ))
    .chain(WINDOW_SCALES.iter().filter_map(|&percent| {
        let value = to(default.map(|side| side * percent as f32 / 100.0));
        accepts(value).then(|| {
            SizePreset::new(
                ui.tr(
                    "skin-edit-size-preset-scale",
                    &[
                        ("percent", percent.into()),
                        ("width", value[0].to_string().into()),
                        ("height", value[1].to_string().into()),
                    ],
                ),
                value,
            )
        })
    }))
    .collect();
    let mut field = SizeField::new(size(read()))
        .range(WINDOW_RANGE.0, WINDOW_RANGE.1)
        .step(1.0)
        .size(ControlSize::Small)
        .presets(presets)
        .label(ui.now_t("skin-edit-resolution"))
        .width_label(ui.now_t("skin-edit-width"))
        .height_label(ui.now_t("skin-edit-height"))
        .presets_label(ui.now_t("skin-edit-size-presets"));
    dense_width(Arc::make_mut(&mut field.style.layout));
    kit::dense_row(
        ui.now_t("skin-edit-resolution"),
        widget(field)
            .key(surface_key("skin-edit-resolution", history))
            .bind(move |f: &mut SizeField| {
                let v = size(read());
                if f.value != v {
                    f.value = v;
                }
            })
            .on(move |e: &SizeFieldChanged| {
                let [w, h] = e.value.map(|side| side as f32);
                change(ui, &write_id, |d| {
                    let pane = d.pane_mut(history);
                    pane.width = unless_default(w, default[0].round());
                    pane.height = unless_default(h, default[1].round());
                });
            }),
    )
}

/// The slider's marks: [`LINE_SNAPS`] and the skin's `default`, which
/// stands in for a snap too close to tell apart on the track.
pub(super) fn line_marks(default: f64) -> Vec<RangeMarker> {
    let mut values: Vec<f64> = LINE_SNAPS
        .into_iter()
        .filter(|v| (v - default).abs() >= 0.1)
        .chain([default])
        .collect();
    values.sort_by(f64::total_cmp);
    values
        .into_iter()
        .map(|v| RangeMarker::new(v, ""))
        .collect()
}

/// The line height: a slider across the row with its reading in the
/// thumb, marks it snaps to as it is dragged, and the skin's own line
/// height to go back to.
fn line_slider(ui: Ui, id: &str, history: bool, default: f32) -> AnyView {
    let read_id = id.to_owned();
    let write_id = id.to_owned();
    let (min, max) = LINE_RANGE;
    let skin_value = f64::from(default).clamp(min, max);
    let value = move || {
        draft(ui, &read_id)
            .pane(history)
            .line_height
            .trim()
            .parse::<f64>()
            .unwrap_or(f64::from(default))
            .clamp(min, max)
    };
    let mut range = crate::ui::settings::controls::compact_thumb(
        RangeField::new(value(), min, max, LINE_STEP)
            .value_thumb(true)
            .markers(line_marks(skin_value))
            .unit("×")
            .default_value(Some(skin_value))
            .label(ui.now_t("skin-edit-line"))
            .show_label(false),
    );
    // The track takes the row past the label, so dragging has room; the
    // marks stand for the ends the bounds used to spell out.
    let l = Arc::make_mut(&mut range.style.layout);
    l.flex_grow = Some(1.0);
    l.min_width = Some(LengthSpec::Px(0.0));
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::GROUP)
            .align(AlignSpec::Center)
            .width(LengthSpec::Fill)
            .padding_xy(0.0, gap::HAIR),
    )
    .children((
        kit::Type::Caption.view(ui.now_t("skin-edit-line")),
        widget(range)
            .key(surface_key("skin-edit-line", history))
            .bind(move |r: &mut RangeField| {
                let v = value();
                if (r.value - v).abs() > f64::EPSILON {
                    r.value = v;
                }
            })
            .on(move |e: &RangeInput| {
                // The skin's own mark may sit off the step; it stays exact.
                let v = if (e.value - skin_value).abs() < 1e-6 {
                    skin_value
                } else {
                    ((e.value / LINE_STEP).round() * LINE_STEP).clamp(min, max)
                };
                change(ui, &write_id, |d| {
                    d.pane_mut(history).line_height = unless_default(v as f32, default)
                });
            }),
    ))
    .into_any()
}

/// The text colour: a picker, the skin's colour while the draft has none.
fn color_field(ui: Ui, id: &str, history: bool, default: Rgba8) -> AnyView {
    let read_id = id.to_owned();
    let write_id = id.to_owned();
    let rgba = move || {
        draft(ui, &read_id)
            .pane(history)
            .color
            .trim()
            .parse::<Rgba8>()
            .unwrap_or(default)
    };
    let field = ColorField::new(crate::ui::c(rgba()))
        .label(ui.now_t("skin-edit-color"))
        .size(ControlSize::Small);
    // The colour field fills what holds it, so its width is the box's.
    kit::dense_row(
        ui.now_t("skin-edit-color"),
        widget(Stack::row(0.0).with_layout(dense_width)).children(
            widget(field)
                .key(surface_key("skin-edit-color", history))
                .bind(move |f: &mut ColorField| {
                    let want = crate::ui::c(rgba());
                    if f.value != want && !f.opened {
                        *f = f.clone().value(want);
                    }
                })
                .on(move |e: &ColorChanged| {
                    let [r, g, b, _] = e.value;
                    let to = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                    let c = Rgba8::rgba(to(r), to(g), to(b), 255); // design-tokens: data
                    change(ui, &write_id, |d| {
                        d.pane_mut(history).color = if c == default {
                            String::new()
                        } else {
                            c.to_hex()
                        }
                    });
                }),
        ),
    )
}

/// The body font: the skin's own, the built-in fonts and common system ones.
fn font_select(ui: Ui, id: &str, history: bool) -> AnyView {
    let mut fonts = vec![SelectOption::new("", ui.now_t("common-skin-default"))];
    fonts.extend(
        font_families()
            .iter()
            .map(|f| SelectOption::new(f.clone(), f.clone())),
    );
    let read_id = id.to_owned();
    let write_id = id.to_owned();
    let mut select = Select::new(Some(String::new()))
        .options(fonts)
        .label(ui.now_t("skin-edit-font"))
        .size(ControlSize::Small);
    dense_width(Arc::make_mut(&mut select.style.layout));
    kit::dense_row(
        ui.now_t("skin-edit-font"),
        widget(select)
            .key(surface_key("skin-edit-font", history))
            .bind(move |s: &mut Select| {
                let font: Arc<str> = draft(ui, &read_id).pane(history).font.trim().into();
                // A family saved before the list offered it still shows.
                if !s.options.iter().any(|o| o.value == font) {
                    s.options
                        .push(SelectOption::new(font.to_string(), font.to_string()));
                }
                if s.value.as_ref() != Some(&font) {
                    s.value = Some(font);
                }
            })
            .on(move |e: &SelectChanged| {
                change(ui, &write_id, |d| {
                    d.pane_mut(history).font = e.value.to_string()
                });
            }),
    )
}

/// Font, size, colour, line height and window size for one surface.
fn pane_fields(ui: Ui, id: &str, base: &Skin, history: bool) -> AnyView {
    let text = if history {
        base.history.text.clone()
    } else {
        base.dialogue.text.clone()
    };
    widget(
        Stack::column(0.0)
            .gap_step(gap::step::TIGHT)
            .width(LengthSpec::Fill),
    )
    .children((
        font_select(ui, id, history),
        size_field(ui, id, history, text.size),
        color_field(ui, id, history, text.color),
        line_slider(ui, id, history, text.line_height),
        resolution_field(ui, id, history, default_size(base, history)),
    ))
    .into_any()
}

fn font_missing(font: &str, fallback_font: bool) -> bool {
    let trimmed = font.trim();
    if trimmed.is_empty() {
        return fallback_font;
    }
    let key = trimmed.to_ascii_lowercase();
    if let Some(&cached) = FONT_AVAILABILITY.lock().unwrap().get(&key) {
        return cached;
    }
    let missing = !nana_ui::shaped_face_families(trimmed, "Aa字")
        .iter()
        .any(|f| f.eq_ignore_ascii_case(trimmed));
    FONT_AVAILABILITY.lock().unwrap().insert(key, missing);
    missing
}

/// Where the editor shows. The skins page saves with its own button; the
/// first-run guide saves on Next, and keeps its undo beside the skin picker.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Host {
    Page,
    Guide,
}

/// A group of the guide's fields: titled inside a sunken panel, so each
/// group reads apart from the guide's card around it.
fn guide_group(title: Option<(Icon, String)>, body: impl IntoView) -> AnyView {
    use nana_ui::runtime::IconGlyph;
    use nana_ui_core::{RadiusTier, SemanticColorRole, SpacingStep};
    widget(
        Stack::column(0.0)
            .gap_step(gap::step::TIGHT)
            .width(LengthSpec::Fill)
            .padding_step(SpacingStep::Lg, SpacingStep::Md)
            .surface(SemanticColorRole::Subtle)
            .border(SemanticColorRole::BorderSoft, 1.0)
            .radius(RadiusTier::Md),
    )
    .children((
        title.map(|(icon, title)| {
            widget(
                Stack::row(0.0)
                    .gap_step(SpacingStep::Sm)
                    .align(AlignSpec::Center)
                    .with_layout(|l| l.padding_bottom = Some(LengthSpec::Px(gap::HAIR))),
            )
            .children((
                widget(
                    IconGlyph::new(icon)
                        .size(kit::icon::SM)
                        .role(SemanticColorRole::Muted)
                        .decorative(),
                ),
                kit::Type::Strong.view(title),
            ))
        }),
        body,
    ))
    .into_any()
}

/// The guide's undo for the active skin, beside its picker: back to the
/// skin's own values, and away with what has not been saved. Each is
/// disabled while it would change nothing.
pub(in crate::ui::settings) fn guide_tools(ui: Ui) -> AnyView {
    let active = move || ui.settings.with(|s| s.skin.id.clone());
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::HAIR)
            .align(AlignSpec::Center),
    )
    .children((
        widget(action(ui, ti::RESTORE, "skin-edit-reset"))
            .key("guide-skin-reset")
            .bind(move |b: &mut IconButton| {
                let plain = follows_skin(&draft(ui, &active()));
                if b.disabled != plain {
                    b.disabled = plain;
                }
            })
            .on(move |_: &Activate| {
                change(ui, &active(), |d| {
                    // The sample lines stay; only the values go.
                    for history in [false, true] {
                        let pane = d.pane_mut(history);
                        *pane = Pane {
                            text: std::mem::take(&mut pane.text),
                            ..Pane::default()
                        };
                    }
                    d.params = Default::default();
                })
            }),
        widget(action(ui, ti::ARROW_BACK_UP, "skin-edit-discard"))
            .key("guide-skin-discard")
            .bind(move |b: &mut IconButton| {
                let id = active();
                let off = !ui.skin_drafts.with(|m| m.contains_key(&id));
                if b.disabled != off {
                    b.disabled = off;
                }
            })
            .on(move |_: &Activate| {
                let id = active();
                ui.skin_drafts.update(|m| {
                    m.remove(&id);
                });
            }),
    ))
    .into_any()
}

/// Whether `d` sets nothing of its own, so every field follows the skin.
pub(super) fn follows_skin(d: &Draft) -> bool {
    let blank = |p: &Pane| {
        Pane {
            text: String::new(),
            ..p.clone()
        } == Pane::default()
    };
    blank(&d.display) && blank(&d.history) && d.params == Default::default()
}

/// What the guide's Next saves: the active skin's unsaved fields, `None`
/// when there are none or they match what is saved, `Err` while they do
/// not make a valid profile.
pub(in crate::ui::settings) fn guide_save(ui: Ui) -> Result<Option<Msg>, super::InvalidProfile> {
    let id = ui.settings.with(|s| s.skin.id.clone());
    let Some(d) = ui.skin_drafts.with(|m| m.get(&id).cloned()) else {
        return Ok(None);
    };
    let base = ui.skins.with(|rows| {
        rows.iter()
            .find(|r| r.id == id)
            .and_then(|r| r.skin.clone())
    });
    let Some(base) = base else {
        return Ok(None);
    };
    let profile = d.profile(&base)?;
    let saved = ui
        .settings
        .with(|s| s.skin.personalizations.get(&id).cloned());
    Ok((saved.as_ref() != Some(&profile))
        .then(|| Msg::Skin(SkinMsg::SavePersonalization { id, profile })))
}

pub(super) fn controls(
    ui: Ui,
    id: &str,
    base: Arc<Skin>,
    fallback_font: bool,
    host: Host,
) -> AnyView {
    let guide = host == Host::Guide;
    let reset_id = id.to_owned();
    let discard_id = id.to_owned();
    let save_id = id.to_owned();
    let valid_id = id.to_owned();
    let valid_base = base.clone();
    let save_base = base.clone();
    let params = crate::ui::settings::skin_params::view(ui, id, base.clone());
    let (params, display, history) = if guide {
        (
            params.map(|p| guide_group(None, p)),
            guide_group(
                Some((ti::APP_WINDOW, ui.now_t("windows-display"))),
                pane_fields(ui, id, &base, false),
            ),
            guide_group(
                Some((ti::MESSAGES, ui.now_t("windows-history"))),
                pane_fields(ui, id, &base, true),
            ),
        )
    } else {
        (
            params,
            section(
                ui,
                "windows-display",
                vec![kit::block(pane_fields(ui, id, &base, false))],
            ),
            section(
                ui,
                "windows-history",
                vec![kit::block(pane_fields(ui, id, &base, true))],
            ),
        )
    };
    let status_id = id.to_owned();
    let pick_id = id.to_owned();
    let status = dynamic(
        move || {
            (
                draft(ui, &status_id),
                ui.settings
                    .with(|s| s.skin.personalizations.get(&status_id).cloned()),
            )
        },
        move |(d, saved)| {
            let Ok(profile) = d.profile(&base) else {
                return hint(ui.now_t("skin-edit-invalid"));
            };
            let mut notes = Vec::new();
            // A built-in font the draft names is registered by the sample;
            // register it here too, whichever of them runs first.
            for font in [&d.display.font, &d.history.font] {
                ui.skin_preview.with_untracked(|p| {
                    if let Some(p) = p.as_ref().filter(|p| p.id == pick_id) {
                        p.assets.pick_family(font);
                    }
                });
            }
            if font_missing(&d.display.font, fallback_font)
                || font_missing(&d.history.font, fallback_font)
            {
                notes.push(ui.now_t("skin-edit-font-missing"));
            }
            // The guide saves on Next, so it tells only what stands in the way.
            if !guide {
                notes.push(ui.now_t(if saved.as_ref() == Some(&profile) {
                    "skin-edit-saved"
                } else {
                    "skin-edit-draft-hint"
                }));
            }
            if notes.is_empty() {
                return widget(Stack::column(0.0)).into_any();
            }
            hint(notes.join("\n"))
        },
    );
    if guide {
        return widget(
            Stack::column(0.0)
                .gap_step(gap::step::CONTROL)
                .width(LengthSpec::Fill),
        )
        .children((params, display, history, status))
        .into_any();
    }
    // Keep controls mounted while the sample rerenders, preserving IME/selection.
    widget(
        Stack::column(0.0)
            .gap_step(gap::step::CONTROL)
            .width(LengthSpec::Fill),
    )
    .children((
        params,
        display,
        history,
        // Where the draft stands on the left, what can be done with it on
        // the right. Reset restores both surfaces.
        widget(
            Stack::row(0.0)
                .gap_step(gap::step::CONTROL)
                .align(AlignSpec::Center)
                .width(LengthSpec::Fill)
                .wrap(true),
        )
        .children((
            widget(kit::grow_column(gap::HAIR)).children(status),
            widget(
                Stack::row(0.0)
                    .gap_step(gap::step::CONTROL)
                    .align(AlignSpec::Center)
                    .with_layout(|l| l.flex_shrink = Some(0.0)),
            )
            .children((
                widget(action(ui, ti::RESTORE, "skin-edit-reset")).on(move |_: &Activate| {
                    change(ui, &reset_id, |d| {
                        d.display = Pane::default();
                        d.history = Pane::default();
                        d.params = Default::default();
                    })
                }),
                widget(action(ui, ti::ARROW_BACK_UP, "skin-edit-discard")).on(
                    move |_: &Activate| {
                        ui.skin_drafts.update(|m| {
                            m.remove(&discard_id);
                        });
                    },
                ),
                widget(
                    Button::new(ui.now_t("skin-edit-save"))
                        .icon(ti::DEVICE_FLOPPY)
                        .size(ControlSize::Small)
                        .kind(ButtonKind::Primary),
                )
                .key("skin-edit-save")
                .bind(move |b: &mut Button| {
                    b.disabled = draft(ui, &valid_id).profile(&valid_base).is_err()
                })
                .on_cx(move |_, _: &Activate, cx| {
                    if let Ok(profile) = draft(ui, &save_id).profile(&save_base) {
                        cx.dispatch_program_all(Msg::Skin(SkinMsg::SavePersonalization {
                            id: save_id.clone(),
                            profile,
                        }));
                    }
                }),
            )),
        )),
    ))
    .into_any()
}
