//! Settings building blocks: a section, a labelled row, and controls bound to
//! one settings field each.

use std::sync::{Arc, LazyLock};

use nana_ui::runtime::view::{AnyView, IntoView, segmented_option, widget};
use nana_ui::runtime::{
    Activate, Chip, ColorChanged, ColorField, IconButton, NumberChanged, RangeChanged, RangeField,
    SegmentedControl, SegmentedOptionChosen, Select, SelectChanged, SelectOption, Stack,
    StatusBadge, Switch, Text, TextChanged, ToggleChanged,
};
use nana_ui::{ButtonKind, ControlSize, StatusTone};
use nana_ui_core::{AlignSpec, JustifySpec, LengthSpec, SemanticColorRole};
use nanatype_i18n::Locale;
use nanatype_model::Rgba8;
use nanatype_settings::Settings;

use crate::msg::Msg;
use crate::ui::{I18N, LinkState, Status, Ui};

pub static ICON_PNG_B64: LazyLock<String> =
    LazyLock::new(|| base64(include_bytes!("../../../assets/tray-64.png")));

pub use nanatype_frame::ui::controls::{base64, confirm, hint, label};

use crate::ui::kit;

pub use crate::ui::kit::{
    Shown, Type, block, divided, divider, field, gap, icon, inset, row_text, shown_if,
};

/// A page's title.
pub fn heading(ui: Ui, key: &str) -> AnyView {
    Type::PageTitle.view(ui.now_t(key))
}

/// A heading inside a card, below the page title.
pub fn subheading(ui: Ui, key: &str) -> AnyView {
    Type::Strong.view(ui.now_t(key))
}

/// A titled group of rows. An empty `title_key` leaves the title out (the
/// page heading already names it).
pub fn section(ui: Ui, title_key: &str, rows: Vec<AnyView>) -> AnyView {
    section_with(ui, title_key, rows.into_iter().map(|r| (r, None)).collect())
}

/// [`section`] whose rows may hide; a hidden row takes its divider with it.
pub fn section_with(ui: Ui, title_key: &str, rows: Vec<(AnyView, Shown)>) -> AnyView {
    let title = (!title_key.is_empty()).then(|| ui.now_t(title_key));
    crate::ui::kit::section_titled(title, rows)
}

/// The rows that only mean something while a master toggle is on, for
/// [`section_with`]. They fold away with it, divider and all, so a
/// switched-off feature does not show a page of dead settings.
pub fn when_on(
    ui: Ui,
    enabled: impl Fn(&Settings) -> bool + Send + Sync + Copy + 'static,
    rows: Vec<AnyView>,
) -> (AnyView, Shown) {
    shown_if(
        widget(Stack::column(0.0))
            .children(divided(rows))
            .into_any(),
        move || ui.settings.with(|s| enabled(s)),
    )
}

/// A feature's card: its master row leads, and the rows that only mean
/// something while `enabled` holds fold away with it.
pub fn toggled_section(
    ui: Ui,
    title_key: &str,
    master: AnyView,
    enabled: impl Fn(&Settings) -> bool + Send + Sync + Copy + 'static,
    rows: Vec<AnyView>,
) -> AnyView {
    section_with(
        ui,
        title_key,
        vec![(master, None), when_on(ui, enabled, rows)],
    )
}

/// Period of a connecting badge's breath (ms).
const PULSE_MS: u64 = 1200;

/// Whether any link badge on the visible page is breathing, so the settings
/// window needs frames for it.
pub fn links_pulsing(ui: Ui) -> bool {
    !ui.reduced_motion.get_untracked()
        && matches!(
            ui.page.get_untracked(),
            super::Page::Integrations | super::Page::Output | super::Page::Platforms
        )
        && ui.status.with_untracked(|s| {
            [
                &s.vts,
                &s.osc,
                &s.ws,
                &s.spout_display,
                &s.spout_peer_display,
                &s.spout_history,
            ]
            .into_iter()
            .any(|l| *l == LinkState::Connecting)
                || s.bilibili_link == crate::ui::danmaku::Link::Connecting
                || s.blivechat_link == crate::ui::danmaku::Link::Connecting
        })
}

/// A link's state as a dot-and-text badge: green connected, amber connecting
/// or waiting for access, red failed, grey off. Connecting breathes unless
/// motion is reduced.
pub fn link_badge(ui: Ui, get: fn(&Status) -> &LinkState) -> AnyView {
    let describe = move || {
        let locale = ui.locale.get();
        ui.status.with(|s| match get(s) {
            LinkState::Off => (I18N.t_in(locale, "int-status-off"), StatusTone::Neutral),
            LinkState::Connecting => (
                I18N.t_in(locale, "int-status-connecting"),
                StatusTone::Warning,
            ),
            LinkState::Connected(text) => (text.clone(), StatusTone::Success),
            LinkState::NeedsAuth => (
                I18N.t_in(locale, "int-status-needs-token"),
                StatusTone::Warning,
            ),
            LinkState::Error(text) => (text.clone(), StatusTone::Danger),
        })
    };
    breathing_badge(ui, describe, move || {
        ui.status.with(|st| *get(st) == LinkState::Connecting)
    })
}

/// Bilibili's badge: its own status line and the chat panel's dot state,
/// drawn like the other links.
#[cfg(feature = "unofficial-chat")]
pub fn bilibili_badge(ui: Ui) -> AnyView {
    chat_badge(ui, |s| (&s.bilibili, s.bilibili_link))
}

/// Third-party chat's badge (blivechat or 彗星号).
pub fn blivechat_badge(ui: Ui) -> AnyView {
    chat_badge(ui, |s| (&s.blivechat, s.blivechat_link))
}

fn chat_badge(ui: Ui, get: fn(&Status) -> (&String, crate::ui::danmaku::Link)) -> AnyView {
    use crate::ui::danmaku::Link;
    let describe = move || {
        let locale = ui.locale.get();
        ui.status.with(|s| {
            let (line, link) = get(s);
            let text = if line.is_empty() {
                I18N.t_in(locale, "int-status-off")
            } else {
                line.clone()
            };
            let tone = match link {
                Link::Off => StatusTone::Neutral,
                Link::Connecting => StatusTone::Warning,
                Link::Connected => StatusTone::Success,
                Link::Error => StatusTone::Danger,
            };
            (text, tone)
        })
    };
    breathing_badge(ui, describe, move || {
        ui.status.with(|st| get(st).1 == Link::Connecting)
    })
}

fn breathing_badge(
    ui: Ui,
    describe: impl Fn() -> (String, StatusTone) + Send + Sync + Copy + 'static,
    connecting: impl Fn() -> bool + Send + Sync + Copy + 'static,
) -> AnyView {
    // The badge is bound in place rather than swapped by `dynamic`: a
    // switching block builds its branch in a full-width column, which in
    // this shrink-width row took the whole control slot, squeezing the
    // label and pushing the switch out of the card.
    let (text, tone) = describe();
    let badge =
        widget(StatusBadge::new(text, tone).compact(true)).bind(move |b: &mut StatusBadge| {
            let (text, tone) = describe();
            let text: Arc<str> = text.into();
            if b.label != text {
                b.label = text;
            }
            b.tone = tone;
        });
    // An idle link says nothing: its switch already shows it is off.
    widget(Stack::row(0.0).with_layout(|l| l.flex_shrink = Some(0.0)))
        .visible(move || describe().1 != StatusTone::Neutral)
        .bind(move |s: &mut Stack| {
            let connecting = connecting();
            let opacity = if connecting && !ui.reduced_motion.get() {
                let phase = (ui.settings_pulse_ms.get() % PULSE_MS) as f32 / PULSE_MS as f32;
                let wave = 0.5 - 0.5 * (phase * std::f32::consts::TAU).cos();
                0.55 + 0.45 * wave
            } else {
                1.0
            };
            crate::ui::restyle(s, |l| l.opacity = Some(opacity));
        })
        .children(badge)
        .into_any()
}

/// Label (and hint) on the left, control on the right; stacks when narrow.
pub fn row(ui: Ui, label_key: &str, hint_key: Option<&str>, control: AnyView) -> AnyView {
    row_text(ui.now_t(label_key), hint_key.map(|k| ui.now_t(k)), control)
}

/// An info mark whose tooltip carries the detail a label leaves out.
pub fn info_tip(ui: Ui, key: &str) -> AnyView {
    widget(
        IconButton::new(nana_ui::icons_tabler::INFO_CIRCLE, ui.now_t(key))
            .size(ControlSize::Small)
            .kind(ButtonKind::Ghost)
            .with_tooltip(ui.now_t(key)),
    )
    .into_any()
}

/// [`row`] with an info mark before its control, for detail worth a look
/// but not a line of its own.
pub fn row_tip(ui: Ui, label_key: &str, tip_key: &str, control: AnyView) -> AnyView {
    row(
        ui,
        label_key,
        None,
        widget(
            Stack::row(0.0)
                .gap_step(gap::step::HAIR)
                .align(AlignSpec::Center),
        )
        .children((info_tip(ui, tip_key), control))
        .into_any(),
    )
}

/// [`row`] whose hint follows the settings (it is re-read on every change).
pub fn row_live_hint(
    ui: Ui,
    label_key: &str,
    hint: impl Fn() -> Option<String> + Send + 'static,
    control: AnyView,
) -> AnyView {
    crate::ui::kit::row_hint_fn(ui.now_t(label_key), hint, control)
}

/// A setting flag shown as a chip: its label key, then how it is read and
/// written.
pub type Flag = (&'static str, fn(&Settings) -> bool, fn(&mut Settings, bool));

/// Several on/off settings as one row of chips, each selected while its flag
/// is on, rather than a switch row apiece.
pub fn chip_toggles(ui: Ui, flags: &[Flag]) -> AnyView {
    let chips: Vec<AnyView> = flags
        .iter()
        .map(|&(key, get, set)| {
            widget(
                Chip::new(ui.now_t(key))
                    .selected(get(&ui.settings.get_untracked()))
                    .size(ControlSize::Small),
            )
            .bind(move |c: &mut Chip| c.selected = ui.settings.with(|s| get(s)))
            .on_cx(move |_, _: &Activate, cx| {
                let v = !ui.settings.with_untracked(|s| get(s));
                cx.dispatch_program_all(edit(move |s| set(s, v)));
            })
            .into_any()
        })
        .collect();
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::TIGHT)
            .wrap(true)
            .align(AlignSpec::Center)
            .justify(JustifySpec::End),
    )
    .children(chips)
    .into_any()
}

/// How a setting of type `T` is read and written.
pub type Bound<T> = (fn(&Settings) -> T, fn(&mut Settings, T));

/// A host and its port on one line: `host : port`.
pub fn address(ui: Ui, host: Bound<String>, port_of: Bound<u16>) -> AnyView {
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::TIGHT)
            .align(AlignSpec::Center),
    )
    .children((
        code_text(ui, "127.0.0.1".into(), field::SM, host.0, host.1),
        widget(Type::Caption.text(":")),
        port(ui, port_of.0, port_of.1),
    ))
    .into_any()
}

/// A card's empty list as one row: what is missing on the left, the button
/// that adds it on the right.
pub fn empty_row(ui: Ui, text_key: &str, action: AnyView) -> AnyView {
    kit::line(hint(ui.now_t(text_key)), action)
}

/// A small button that sends `msg`.
pub fn button(ui: Ui, key: &str, msg: impl Fn() -> Msg + Send + 'static) -> AnyView {
    crate::ui::kit::button(ui.now_t(key), msg)
}

/// The view's call to action: [`button`] as [`crate::ui::kit::cta`].
pub fn primary(ui: Ui, key: &str, msg: impl Fn() -> Msg + Send + 'static) -> AnyView {
    crate::ui::kit::cta_button(ui.now_t(key), msg)
}

pub fn edit(f: impl FnOnce(&mut Settings) + Send + 'static) -> Msg {
    Msg::Edit(Box::new(f))
}

pub fn switch(
    ui: Ui,
    get: impl Fn(&Settings) -> bool + Send + Sync + Copy + 'static,
    set: impl Fn(&mut Settings, bool) + Send + Sync + Copy + 'static,
) -> AnyView {
    widget(Switch::new("", get(&ui.settings.get_untracked())))
        .bind(move |s: &mut Switch| s.checked = ui.settings.with(|x| get(x)))
        .on_cx(move |_, e: &ToggleChanged, cx| {
            let v = e.checked;
            cx.dispatch_program_all(edit(move |s| set(s, v)));
        })
        .into_any()
}

/// A select whose options are `(value, label)`.
pub fn select(
    ui: Ui,
    options: Vec<(String, String)>,
    get: impl Fn(&Settings) -> String + Send + Sync + Copy + 'static,
    set: impl Fn(&mut Settings, &str) + Send + Sync + Copy + 'static,
) -> AnyView {
    let opts: Vec<SelectOption> = options
        .iter()
        .map(|(v, l)| SelectOption::new(v.clone(), l.clone()))
        .collect();
    widget(Stack::row(0.0).width(LengthSpec::Px(field::MD)))
        .children(
            widget(
                Select::new(Some(get(&ui.settings.get_untracked())))
                    .options(opts)
                    .size(ControlSize::Small),
            )
            .bind(move |s: &mut Select| {
                let v: Arc<str> = ui.settings.with(|x| get(x)).into();
                if s.value.as_ref() != Some(&v) {
                    s.value = Some(v);
                }
            })
            .on_cx(move |_, e: &SelectChanged, cx| {
                let v = e.value.to_string();
                cx.dispatch_program_all(edit(move |s| set(s, &v)));
            }),
        )
        .into_any()
}

pub fn select_keys(ui: Ui, options: &[(&'static str, &'static str)]) -> Vec<(String, String)> {
    let locale = ui.locale.get_untracked();
    options
        .iter()
        .map(|(v, k)| ((*v).to_owned(), I18N.t_in(locale, k)))
        .collect()
}

/// `v` to the step's precision, trailing zeros dropped.
pub fn trim_num(v: f64, step: f64) -> String {
    let places = if step >= 1.0 {
        0
    } else if step >= 0.1 {
        1
    } else {
        2
    };
    let s = format!("{v:.places$}");
    if s.contains('.') {
        s.trim_end_matches('0').trim_end_matches('.').to_owned()
    } else {
        s
    }
}

/// How a slider reads its value out: number and unit, localised where the
/// unit has a message.
fn unit_text(locale: Locale, v: f64, step: f64, unit: &str) -> String {
    let n = trim_num(v, step);
    match unit {
        "s" => I18N.tr_in(
            locale,
            "playback-after-delay-value",
            &[("seconds", n.into())],
        ),
        "ms" => I18N.tr_in(locale, "common-ms", &[("ms", n.into())]),
        "%" | "×" | "" => format!("{n}{unit}"),
        _ => format!("{n} {unit}"),
    }
}

/// Words for a slider value ("Off", "Skin default"); `None` reads out the
/// number and unit.
pub type Readout = fn(Locale, f64) -> Option<String>;

/// Width the readout beside a slider keeps, so the tracks line up.
const READOUT_W: f32 = 84.0;

pub fn slider(
    ui: Ui,
    range: (f64, f64, f64),
    unit: &'static str,
    get: fn(&Settings) -> f64,
    set: fn(&mut Settings, f64),
) -> AnyView {
    slider_with(ui, range, unit, |_, _| None, get, set)
}

/// A 0–1 value shown as a percentage.
pub fn percent(ui: Ui, get: fn(&Settings) -> f64, set: fn(&mut Settings, f64)) -> AnyView {
    slider_with(
        ui,
        (0.0, 1.0, 0.01),
        "",
        |_, v| Some(format!("{}%", (v * 100.0).round())),
        get,
        set,
    )
}

/// A slider with its value read out beside it, worded by `readout`.
pub fn slider_with(
    ui: Ui,
    range: (f64, f64, f64),
    unit: &'static str,
    readout: Readout,
    get: fn(&Settings) -> f64,
    set: fn(&mut Settings, f64),
) -> AnyView {
    let (min, max, step) = range;
    let text = move || {
        let v = ui.settings.with(|x| get(x)).clamp(min, max);
        let locale = ui.locale.get();
        readout(locale, v).unwrap_or_else(|| unit_text(locale, v, step, unit))
    };
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::CONTROL)
            .width(LengthSpec::Px(field::MD))
            .align(AlignSpec::Center),
    )
    .children((
        widget(Stack::row(0.0).grow(1.0).shrink(1.0)).children(
            widget(
                RangeField::new(
                    get(&ui.settings.get_untracked()).clamp(min, max),
                    min,
                    max,
                    step,
                )
                .show_value(false)
                .show_label(false),
            )
            .bind(move |r: &mut RangeField| {
                let v = ui.settings.with(|x| get(x)).clamp(min, max);
                if (r.value - v).abs() > f64::EPSILON {
                    r.value = v;
                }
            })
            .on_cx(move |_, e: &RangeChanged, cx| {
                let v = e.value;
                cx.dispatch_program_all(edit(move |s| set(s, v)));
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
            let s = text();
            if t.value != s {
                t.value = s;
            }
        }),
    ))
    .into_any()
}

/// A choice among a few short options, all in view: a segmented control.
pub fn segmented(
    ui: Ui,
    options: Vec<(String, String)>,
    get: impl Fn(&Settings) -> String + Send + Sync + Copy + 'static,
    set: impl Fn(&mut Settings, &str) + Send + Sync + Copy + 'static,
) -> AnyView {
    let opts: Vec<AnyView> = options
        .into_iter()
        .map(|(value, text)| {
            let current = value.clone();
            segmented_option(text)
                .selected(move || ui.settings.with(|s| get(s)) == current)
                .on_cx(move |_, _: &SegmentedOptionChosen, cx| {
                    let v = value.clone();
                    cx.dispatch_program_all(edit(move |s| set(s, &v)));
                })
                .into_any()
        })
        .collect();
    widget(SegmentedControl::new().size(ControlSize::Small))
        .children(opts)
        .into_any()
}

pub fn number(
    ui: Ui,
    range: (f64, f64),
    get: fn(&Settings) -> f64,
    set: fn(&mut Settings, f64),
) -> AnyView {
    widget(
        Stack::row(0.0)
            .width(LengthSpec::Px(field::SM))
            .with_layout(crate::ui::tnum),
    )
    .children(
        widget({
            let mut n = kit::number_input(get(&ui.settings.get_untracked()))
                .range(range.0, range.1)
                .size(ControlSize::Small);
            crate::ui::tnum(Arc::make_mut(&mut n.style.layout));
            n
        })
        .on_cx(move |_, e: &NumberChanged, cx| {
            let v = e.value;
            cx.dispatch_program_all(edit(move |s| set(s, v)));
        }),
    )
    .into_any()
}

/// A port: a short field without steppers, in the mono role. Text that is
/// not a port number (yet) is left unsaved.
pub fn port(ui: Ui, get: fn(&Settings) -> u16, set: fn(&mut Settings, u16)) -> AnyView {
    widget(
        Stack::row(0.0)
            .width(LengthSpec::Px(field::XS))
            .with_layout(crate::ui::mono),
    )
    .children(
        widget({
            let mut input = kit::text_input(get(&ui.settings.get_untracked()).to_string())
                .size(ControlSize::Small);
            crate::ui::mono(Arc::make_mut(&mut input.style.layout));
            input
        })
        .on_cx(move |_, e: &TextChanged, cx| {
            if let Ok(v) = e.value.trim().parse::<u16>()
                && v > 0
            {
                cx.dispatch_program_all(edit(move |s| set(s, v)));
            }
        }),
    )
    .into_any()
}

pub fn text(
    ui: Ui,
    placeholder: String,
    width: f32,
    get: impl Fn(&Settings) -> String + Send + Sync + Copy + 'static,
    set: impl Fn(&mut Settings, String) + Send + Sync + Copy + 'static,
) -> AnyView {
    text_field(ui, placeholder, width, false, get, set)
}

/// [`text`] for a value typed literally (a host, an OSC address), in the
/// mono role.
pub fn code_text(
    ui: Ui,
    placeholder: String,
    width: f32,
    get: impl Fn(&Settings) -> String + Send + Sync + Copy + 'static,
    set: impl Fn(&mut Settings, String) + Send + Sync + Copy + 'static,
) -> AnyView {
    text_field(ui, placeholder, width, true, get, set)
}

fn text_field(
    ui: Ui,
    placeholder: String,
    width: f32,
    code: bool,
    get: impl Fn(&Settings) -> String + Send + Sync + Copy + 'static,
    set: impl Fn(&mut Settings, String) + Send + Sync + Copy + 'static,
) -> AnyView {
    widget(
        Stack::row(0.0)
            .width(LengthSpec::Px(width))
            .with_layout(|l| {
                if code {
                    crate::ui::mono(l);
                }
            }),
    )
    .children(
        widget({
            let mut input = kit::text_input(get(&ui.settings.get_untracked()))
                .placeholder(placeholder)
                .size(ControlSize::Small);
            if code {
                crate::ui::mono(Arc::make_mut(&mut input.style.layout));
            }
            input
        })
        .on_cx(move |_, e: &TextChanged, cx| {
            let v = e.value.to_string();
            cx.dispatch_program_all(edit(move |s| set(s, v)));
        }),
    )
    .into_any()
}

/// A colour well and its hex value; the hex is in the mono role.
pub fn color(
    ui: Ui,
    fallback: Rgba8,
    get: fn(&Settings) -> Option<Rgba8>,
    set: fn(&mut Settings, Option<Rgba8>),
) -> AnyView {
    let start = get(&ui.settings.get_untracked()).unwrap_or(fallback);
    widget(
        Stack::row(0.0)
            .width(LengthSpec::Px(field::MD))
            .with_layout(crate::ui::mono),
    )
    .children(
        widget(ColorField::new(crate::ui::c(start)).size(ControlSize::Small)).on_cx(
            move |_, e: &ColorChanged, cx| {
                let [r, g, b, a] = e.value;
                let to = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                let c = Rgba8::rgba(to(r), to(g), to(b), to(a)); // design-tokens: data
                cx.dispatch_program_all(edit(move |s| set(s, Some(c))));
            },
        ),
    )
    .into_any()
}
