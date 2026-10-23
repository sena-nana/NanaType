//! Controls for the params a skin declares, generated from its schema. Edits
//! land in the skin's unsaved draft like the other preview fields; only Save
//! writes them to settings.
use std::sync::Arc;

use nana_ui::ControlSize;
use nana_ui::runtime::view::{AnyView, IntoView, dynamic, segmented_option, widget};
use nana_ui::runtime::{
    Activate, Chip, ColorChanged, ColorField, RangeChanged, RangeField, SegmentedControl,
    SegmentedOptionChosen, Select, SelectChanged, SelectOption, Stack, Switch, Text, TextChanged,
    TextInput, TextInputState, ToggleChanged,
};
use nana_ui_core::{AlignSpec, LengthSpec, SemanticColorRole};
use nanatype_model::{ParamValue, Rgba8};
use nanatype_theme::{ParamError, ParamSpec, ParamType, Resolved, Skin, localized};

use super::controls::{Type, field, gap, hint, label};
use super::preview::{change, draft};
use crate::ui::Ui;
use crate::ui::kit;

const READOUT_W: f32 = 56.0;
/// Enums with this many short options or fewer show as a segmented control.
const SEGMENTED_MAX: usize = 4;

/// One param of the skin being previewed, cheap to move into callbacks.
#[derive(Clone)]
struct Param {
    ui: Ui,
    skin_id: Arc<str>,
    skin: Arc<Skin>,
    id: Arc<str>,
}

impl Param {
    fn spec(&self) -> &ParamSpec {
        &self.skin.params[&*self.id]
    }
    fn current(&self) -> Option<Resolved> {
        draft(self.ui, &self.skin_id)
            .params
            .current(&self.skin, &self.id)
    }
    fn set(&self, value: ParamValue) {
        change(self.ui, &self.skin_id, |d| d.params.set(&self.id, value));
    }
    fn input(&self, text: &str) {
        change(self.ui, &self.skin_id, |d| {
            let _ = d.params.input(&self.skin, &self.id, text);
        });
    }
    /// Typed text that does not parse yet, if any.
    fn raw(&self) -> Option<String> {
        draft(self.ui, &self.skin_id)
            .params
            .raw
            .get(&*self.id)
            .cloned()
    }
}

fn locale(ui: Ui) -> String {
    ui.locale.get_untracked().tag().to_owned()
}

/// Why typed text is refused, worded for the page.
fn error_text(ui: Ui, e: &ParamError) -> String {
    match e {
        ParamError::NotANumber | ParamError::WrongType(_) => ui.now_t("skin-params-invalid-number"),
        ParamError::OutOfRange { min, max } => ui.tr(
            "skin-params-out-of-range",
            &[("min", (*min).into()), ("max", (*max).into())],
        ),
        ParamError::BadColor | ParamError::NeedsOpaque => ui.now_t("skin-params-invalid-color"),
        ParamError::UnknownOption => ui.now_t("skin-params-invalid-option"),
        ParamError::TooLong { max, .. } => {
            ui.tr("skin-params-text-too-long", &[("max", (*max).into())])
        }
        ParamError::Empty => ui.now_t("skin-params-text-empty"),
        ParamError::ControlCharacter => ui.now_t("skin-params-text-control"),
    }
}

fn number_text(v: f32, spec: &ParamSpec) -> String {
    let step = spec.step.unwrap_or(0.01);
    let decimals = if step.fract() == 0.0 {
        0
    } else {
        (-step.log10()).ceil().clamp(0.0, 4.0) as usize
    };
    match spec.unit.as_deref() {
        // A fraction shown as a percentage.
        Some("%") => format!("{}%", (v * 100.0).round()),
        Some(unit) => format!("{v:.decimals$} {unit}"),
        None => format!("{v:.decimals$}"),
    }
}

fn number(p: Param) -> AnyView {
    let (min, max) = p.spec().range();
    let step = p.spec().step.unwrap_or((max - min) / 100.0);
    let value = move |p: &Param| match p.current() {
        Some(Resolved::Number(v)) => f64::from(v),
        _ => min,
    };
    let (slider, readout) = (p.clone(), p.clone());
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::CONTROL)
            .width(LengthSpec::Fill)
            .align(AlignSpec::Center),
    )
    .children((
        widget(Stack::row(0.0).grow(1.0).shrink(1.0)).children(
            widget(
                RangeField::new(value(&p), min, max, step)
                    .show_value(false)
                    .show_label(false),
            )
            .bind(move |r: &mut RangeField| {
                let v = value(&slider);
                if (r.value - v).abs() > f64::EPSILON {
                    r.value = v;
                }
            })
            .on(move |e: &RangeChanged| {
                let v = ((e.value - min) / step).round() * step + min;
                p.set(ParamValue::Number(v.clamp(min, max)));
            }),
        ),
        widget(
            Text::new(String::new())
                .style({
                    let mut st = Type::Small.style();
                    let l = Arc::make_mut(&mut st.layout);
                    l.width = Some(LengthSpec::Px(READOUT_W));
                    l.white_space_nowrap = true;
                    crate::ui::tnum(l);
                    st
                })
                .color(SemanticColorRole::Muted),
        )
        .bind(move |t: &mut Text| {
            let s = number_text(value(&readout) as f32, readout.spec());
            if t.value != s {
                t.value = s;
            }
        }),
    ))
    .into_any()
}

/// A text field showing the draft's unparsed text, else `shown` of the
/// current value.
fn typed_field(p: Param, width: LengthSpec, shown: fn(Option<Resolved>) -> String) -> AnyView {
    let read = {
        let p = p.clone();
        move || p.raw().unwrap_or_else(|| shown(p.current()))
    };
    let key = format!("skin-param-{}", p.id);
    widget(Stack::row(0.0).width(width))
        .children(
            widget(kit::text_input(read()).size(ControlSize::Small))
                .key(key)
                .bind(move |input: &mut TextInput| {
                    let text = read();
                    if input.state.value.as_str() != text {
                        input.state = TextInputState::new(text);
                    }
                })
                .on(move |e: &TextChanged| p.input(&e.value)),
        )
        .into_any()
}

fn color(p: Param) -> AnyView {
    let rgba = |p: &Param| match p.current() {
        Some(Resolved::Color(c)) => c,
        _ => Rgba8::BLACK,
    };
    let alpha = p.spec().alpha == Some(true);
    let (read, write) = (p.clone(), p.clone());
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::CONTROL)
            .align(AlignSpec::Center)
            .with_layout(crate::ui::mono),
    )
    .children((
        widget(ColorField::new(crate::ui::c(rgba(&p))).size(ControlSize::Small))
            .bind(move |f: &mut ColorField| {
                let want = crate::ui::c(rgba(&read));
                if f.value != want && !f.opened {
                    *f = f.clone().value(want);
                }
            })
            .on(move |e: &ColorChanged| {
                let [r, g, b, a] = e.value;
                let to = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                let c = Rgba8::rgba(to(r), to(g), to(b), if alpha { to(a) } else { 255 }); // design-tokens: data
                write.set(ParamValue::Text(c.to_hex()));
            }),
        typed_field(p, LengthSpec::Px(field::XS), |r| match r {
            Some(Resolved::Color(c)) => c.to_hex(),
            _ => String::new(),
        }),
    ))
    .into_any()
}

fn toggle(p: Param) -> AnyView {
    let on = |p: &Param| matches!(p.current(), Some(Resolved::Bool(true)));
    let read = p.clone();
    widget(Switch::new("", on(&p)))
        .bind(move |s: &mut Switch| s.checked = on(&read))
        .on(move |e: &ToggleChanged| p.set(ParamValue::Bool(e.checked)))
        .into_any()
}

fn choice(p: Param) -> AnyView {
    let lang = locale(p.ui);
    let options: Vec<(String, String)> = p
        .spec()
        .options
        .iter()
        .map(|o| (o.id.clone(), localized(&o.name, &lang, &o.id)))
        .collect();
    let chosen = |p: &Param| match p.current() {
        Some(Resolved::Enum { id, .. }) => id,
        _ => String::new(),
    };
    if options.len() <= SEGMENTED_MAX && options.iter().all(|(_, l)| l.chars().count() <= 6) {
        let segments: Vec<AnyView> = options
            .into_iter()
            .map(|(value, text)| {
                let (read, write, id) = (p.clone(), p.clone(), value.clone());
                segmented_option(text)
                    .selected(move || chosen(&read) == id)
                    .on(move |_: &SegmentedOptionChosen| write.set(ParamValue::Text(value.clone())))
                    .into_any()
            })
            .collect();
        return widget(SegmentedControl::new().size(ControlSize::Small))
            .children(segments)
            .into_any();
    }
    let read = p.clone();
    widget(
        Select::new(Some(chosen(&p)))
            .options(
                options
                    .into_iter()
                    .map(|(v, l)| SelectOption::new(v, l))
                    .collect::<Vec<_>>(),
            )
            .size(ControlSize::Small),
    )
    .bind(move |s: &mut Select| {
        let v: Arc<str> = chosen(&read).into();
        if s.value.as_ref() != Some(&v) {
            s.value = Some(v);
        }
    })
    .on(move |e: &SelectChanged| p.set(ParamValue::Text(e.value.to_string())))
    .into_any()
}

/// One labelled param, with the reason its typed text is refused.
fn row(p: Param) -> AnyView {
    let name = p.spec().label(&p.id, &locale(p.ui));
    let error = {
        let (read, p) = (p.clone(), p.clone());
        dynamic(
            move || read.raw(),
            move |raw| {
                let e = p.spec().parse_input(raw.as_ref()?).err()?;
                Some(widget(
                    Text::new(error_text(p.ui, &e))
                        .style(Type::Caption.style())
                        .color(SemanticColorRole::Danger),
                ))
            },
        )
    };
    let control = match p.spec().kind {
        ParamType::Number => number(p),
        ParamType::Color => color(p),
        ParamType::Bool => toggle(p),
        ParamType::Enum => choice(p),
        ParamType::Text => typed_field(p, LengthSpec::Fill, |r| match r {
            Some(Resolved::Text(t)) => t,
            _ => String::new(),
        }),
    };
    widget(
        Stack::column(0.0)
            .gap_step(gap::step::TIGHT)
            .width(LengthSpec::Fill),
    )
    .children((
        widget(
            Stack::row(0.0)
                .gap_step(gap::step::CONTROL)
                .align(AlignSpec::Center)
                .width(LengthSpec::Fill),
        )
        .children((
            widget(Stack::row(0.0).width(LengthSpec::Px(field::XS))).children(hint(name)),
            widget(Stack::row(0.0).grow(1.0).shrink(1.0)).children(control),
        )),
        error,
    ))
    .into_any()
}

/// "Default" and each preset; the one the draft matches is selected.
fn presets(ui: Ui, id: Arc<str>, skin: Arc<Skin>) -> AnyView {
    let (read_id, read_skin) = (id.clone(), skin.clone());
    dynamic(
        move || {
            let d = draft(ui, &read_id).params;
            let defaults = d.resolved(&read_skin) == read_skin.param_defaults();
            let preset = (!defaults)
                .then(|| d.matching_preset(&read_skin).map(str::to_owned))
                .flatten();
            (defaults, preset)
        },
        move |(defaults, preset)| {
            let lang = locale(ui);
            let reset = id.clone();
            let mut chips = vec![
                widget(
                    Chip::new(ui.now_t("skin-params-default"))
                        .selected(*defaults)
                        .size(ControlSize::Small),
                )
                .on(move |_: &Activate| change(ui, &reset, |d| d.params = Default::default()))
                .into_any(),
            ];
            for (name, spec) in skin.ordered_presets() {
                let (id, skin, name_) = (id.clone(), skin.clone(), name.to_owned());
                chips.push(
                    widget(
                        Chip::new(localized(&spec.name, &lang, name))
                            .selected(preset.as_deref() == Some(name))
                            .size(ControlSize::Small),
                    )
                    .on(move |_: &Activate| {
                        change(ui, &id, |d| d.params.apply_preset(&skin, &name_))
                    })
                    .into_any(),
                );
            }
            widget(
                Stack::row(0.0)
                    .gap_step(gap::step::TIGHT)
                    .wrap(true)
                    .width(LengthSpec::Fill),
            )
            .children(chips)
        },
    )
    .into_any()
}

/// Contrast checks the draft currently fails, one caption each.
fn warnings(ui: Ui, id: Arc<str>, skin: Arc<Skin>) -> AnyView {
    let read_skin = skin.clone();
    dynamic(
        move || {
            let values = draft(ui, &id).params.resolved(&read_skin);
            read_skin
                .failed_checks(&values)
                .into_iter()
                .map(|(c, ratio)| (c.foreground.clone(), c.background.clone(), ratio, c.min))
                .collect::<Vec<_>>()
        },
        move |failed| {
            let lang = locale(ui);
            let name = |p: &str| {
                skin.params
                    .get(p)
                    .map_or(p.to_owned(), |s| s.label(p, &lang))
            };
            let lines: Vec<AnyView> = failed
                .iter()
                .map(|(fg, bg, ratio, min)| {
                    let text = ui.tr(
                        "skin-params-contrast",
                        &[
                            ("foreground", name(fg).into()),
                            ("background", name(bg).into()),
                            ("ratio", format!("{ratio:.1}").into()),
                            ("min", format!("{min:.1}").into()),
                        ],
                    );
                    widget(
                        Text::new(text)
                            .style(Type::Caption.style())
                            .color(SemanticColorRole::Warning),
                    )
                    .into_any()
                })
                .collect();
            widget(Stack::column(0.0).gap_step(gap::step::TIGHT)).children(lines)
        },
    )
    .into_any()
}

/// The params section of skin `id`'s preview controls, or nothing when the
/// skin declares none.
pub fn view(ui: Ui, id: &str, skin: Arc<Skin>) -> Option<AnyView> {
    if skin.params.is_empty() {
        return None;
    }
    let skin_id: Arc<str> = id.into();
    let lang = locale(ui);
    let mut parts: Vec<AnyView> = vec![
        // What the params are for stays a tooltip on their title.
        widget(
            Stack::row(0.0)
                .gap_step(gap::step::HAIR)
                .align(AlignSpec::Center),
        )
        .children((
            label(ui.now_t("skin-params-title")),
            super::controls::info_tip(ui, "skin-params-hint"),
        ))
        .into_any(),
    ];
    if !skin.presets.is_empty() {
        parts.push(presets(ui, skin_id.clone(), skin.clone()));
    }
    for (group, params) in skin.ordered_params() {
        if let Some((g, spec)) = group.and_then(|g| Some((g, skin.param_groups.get(g)?))) {
            parts.push(Type::Strong.view(localized(&spec.name, &lang, g)));
        }
        for (param, _) in params {
            parts.push(row(Param {
                ui,
                skin_id: skin_id.clone(),
                skin: skin.clone(),
                id: param.into(),
            }));
        }
    }
    parts.push(warnings(ui, skin_id, skin));
    Some(
        widget(
            Stack::column(0.0)
                .gap_step(gap::step::CONTROL)
                .width(LengthSpec::Fill),
        )
        .key("skin-params")
        .children(parts)
        .into_any(),
    )
}
