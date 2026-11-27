use std::sync::Arc;

use nana_ui::runtime::view::{AnyView, Computed, IntoView, Signal, Transition, dynamic, widget};
use nana_ui::runtime::{NodeStyle, PaintTransform, Stack, Text};
use nana_ui_core::{
    AlignSpec, BackgroundImage, CssGradient, Easing, FlexWrap, GradientStop, LengthSpec,
    LinearGradient, OverflowSpec, PointerEventsSpec, PositionSpec, TransformOrigin,
};
use nanatype_core::HistoryRow;
use nanatype_model::{Document, Entrance, Inline, Rgba8, SourceKind, Stroke};
use nanatype_theme::{
    AvatarShape, HistoryAlign, HistoryEase, HistoryEntrance, HistoryEventLayout, HistoryLayout,
    Skin,
};

use super::super::dialogue::{Base, Timing, document};
use super::super::surface::{self, Surface};
use super::super::{Stage, c};
use super::event_look::EventLook;
use super::selection::row_opacity;

pub fn local_hm(unix_ms: u64) -> String {
    local_time(unix_ms, false)
}

pub fn local_time(unix_ms: u64, seconds: bool) -> String {
    let offset = crate::localtime::offset_seconds();
    let secs = (unix_ms / 1000) as i64 + offset;
    let day = secs.rem_euclid(86_400);
    let hm = format!("{:02}:{:02}", day / 3600, (day % 3600) / 60);
    if seconds {
        format!("{hm}:{:02}", day % 60)
    } else {
        hm
    }
}

/// The curve a row settles on unless the skin picks another.
pub(super) const ROW_EASE: Easing = Easing::CubicBezier([0.2, 0.8, 0.2, 1.0]);
/// Slow at both ends: paper, ink, water.
pub(super) const ROW_GENTLE: Easing = Easing::CubicBezier([0.45, 0.05, 0.25, 1.0]);
/// Past its place by about a tenth, then back.
const ROW_OVERSHOOT: Easing = Easing::CubicBezier([0.3, 1.45, 0.5, 1.0]);
/// How long an older row takes to step back when a new one arrives.
pub(super) const RECEDE_MS: u64 = 320;
pub(super) const ROW_FADE_MS: u64 = 600;
pub(super) const FADE_FRAME_SLACK_MS: u64 = 100;
/// Space between the rows of a group.
const GROUP_GAP: f32 = 8.0;

/// Meta text (time, role) is a step below the name: 11px beside a 14px name.
pub(super) fn time_size(h: &nanatype_theme::History) -> f32 {
    h.time_size.unwrap_or((h.name_size - 3.0).max(9.0))
}

/// Keep enough room for a short phrase and an @ label in narrow inline logs.
/// At the skin's normal width this leaves its requested name column unchanged.
pub(super) fn inline_name_width(h: &nanatype_theme::History, col_w: f32, show_time: bool) -> f32 {
    let time = if show_time {
        time_size(h) * if h.time_seconds { 4.6 } else { 2.9 } + 10.0
    } else {
        0.0
    };
    let body = (h.text.size * 6.0).max(80.0);
    h.name_width.min((col_w - time - body - 10.0).max(32.0))
}

/// Whether a narrow inline log still has room for the time: beside it the
/// name must keep about three characters. Below that the time goes first,
/// the least of the three; with it kept, the name was cut to "…".
pub(super) fn inline_time_fits(h: &nanatype_theme::History, col_w: f32) -> bool {
    inline_name_width(h, col_w, true) >= (h.name_size * 3.0).min(h.name_width)
}

/// Outline only text runs, so @ labels retain their own fill and lettering.
fn transparent_text(doc: &Document, color: Rgba8) -> Document {
    let mut drawn = doc.clone();
    for paragraph in &mut drawn.paragraphs {
        for inline in &mut paragraph.inlines {
            let run = match inline {
                Inline::Text(run) => run,
                Inline::Ruby(ruby) => &mut ruby.base,
                _ => continue,
            };
            if run.style.stroke.is_none() {
                run.style.stroke = Some(Stroke {
                    color: crate::skin_theme::on(run.style.color.unwrap_or(color)),
                    width_px: 1.5,
                });
            }
        }
    }
    drawn
}

/// `col` mixed `t` of the way to white.
fn lighten(col: Rgba8, t: f32) -> Rgba8 {
    let m = |x: u8| (f32::from(x) + (255.0 - f32::from(x)) * t).round() as u8;
    Rgba8::rgba(m(col.r), m(col.g), m(col.b), col.a)
}

/// The streamer's picture-less avatar: the accent, lighter towards the
/// bottom right.
fn avatar_gradient(from: Rgba8, to: Rgba8) -> BackgroundImage {
    let stop = |position: f32, col: Rgba8| GradientStop {
        position,
        color: nana_ui_core::PaintColor::srgb(c(col)),
    };
    BackgroundImage::Gradient(CssGradient::Linear(LinearGradient {
        angle_deg: 135.0,
        stops: vec![stop(0.0, from), stop(1.0, to)],
    }))
}

/// Space the list loses to the window padding, or to the panel's margin,
/// border and padding.
fn list_inset(h: &nanatype_theme::History) -> f32 {
    match &h.panel {
        Some(p) => {
            let [_, right, _, left] = p.shape.border_widths(p.border_width);
            p.margin[1] + p.margin[3] + left + right + p.padding[0] + p.padding[2]
        }
        None => h.margin[1] + h.margin[3] + h.padding[0] + h.padding[2],
    }
}

fn initial(name: &str) -> String {
    name.chars()
        .next()
        .map(|c| c.to_string())
        .unwrap_or_else(|| "·".into())
}

/// Width available to a row's text column.
pub(super) fn column_width(
    ui: Stage,
    skin: &nanatype_theme::Skin,
    settings: &nanatype_settings::Settings,
) -> f32 {
    let w = viewport_width(ui, skin, settings);
    let avatar = if settings.history.show_avatar && skin.history.avatar.shape != AvatarShape::None {
        skin.history.avatar.size + 10.0
    } else {
        0.0
    };
    (w - list_inset(&skin.history)
        - 2.0 * skin.history.row_padding
        - skin.history.column_inset
        - avatar)
        .max(1.0)
}

pub(super) fn viewport_width(
    ui: Stage,
    skin: &Skin,
    settings: &nanatype_settings::Settings,
) -> f32 {
    let measured = ui.history_width.get();
    if measured > 0.0 {
        measured
    } else {
        crate::canvas::size_of(ui.log_canvas, skin, settings).0
    }
}

pub(super) fn viewport_height(
    ui: Stage,
    skin: &Skin,
    settings: &nanatype_settings::Settings,
) -> f32 {
    let measured = ui.history_height.get();
    if measured > 0.0 {
        measured
    } else {
        crate::canvas::size_of(ui.log_canvas, skin, settings).1
    }
}

/// Positive spacing between entries: grouping never pulls one bubble into another.
pub(super) fn row_gap(skin: &Skin, follows: bool) -> f32 {
    let h = &skin.history;
    if h.layout == HistoryLayout::Inline {
        h.gap.clamp(8.0, 14.0)
    } else if follows {
        (h.text.size * 0.45).round().clamp(GROUP_GAP, 12.0)
    } else {
        h.gap.max((h.text.size * 0.65).round()).clamp(12.0, 20.0)
    }
}

pub(super) fn rise(dy: f32) -> PaintTransform {
    PaintTransform {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: dy,
    }
}

/// How a new row arrives, as the skin asked: the transition, and the point
/// its scale or turn is taken around.
pub(super) fn entrance(
    h: &nanatype_theme::History,
    reduced: bool,
) -> Option<(Transition, TransformOrigin)> {
    let ms = std::time::Duration::from_millis(u64::from(h.entrance_ms.min(2000)));
    let (ease, settle) = match h.entrance_ease {
        HistoryEase::Smooth => (ROW_EASE, None),
        HistoryEase::Gentle => (ROW_GENTLE, None),
        // The opacity must not pass 1: only the movement overshoots.
        HistoryEase::Overshoot => (ROW_EASE, Some(ROW_OVERSHOOT)),
        HistoryEase::Linear => (Easing::Linear, None),
    };
    let matrix =
        |a: f32, b: f32, c: f32, d: f32, e: f32, f: f32| PaintTransform { a, b, c, d, e, f };
    let side = match h.align {
        HistoryAlign::Start => 0.0,
        HistoryAlign::Center => 50.0,
    };
    let (from, origin) = match h.entrance {
        HistoryEntrance::None => return None,
        // Reduced motion keeps the fade and drops the travel.
        HistoryEntrance::Fade => (None, [50.0, 50.0]),
        _ if reduced => (None, [50.0, 50.0]),
        HistoryEntrance::SlideUp => (Some(rise(16.0)), [50.0, 50.0]),
        HistoryEntrance::Drop => (Some(rise(-16.0)), [50.0, 50.0]),
        HistoryEntrance::SlideLeft => (Some(matrix(1.0, 0.0, 0.0, 1.0, -24.0, 0.0)), [50.0, 50.0]),
        HistoryEntrance::SlideRight => (Some(matrix(1.0, 0.0, 0.0, 1.0, 24.0, 0.0)), [50.0, 50.0]),
        HistoryEntrance::Pop => (Some(matrix(0.86, 0.0, 0.0, 0.86, 0.0, 0.0)), [side, 100.0]),
        HistoryEntrance::Stamp => (Some(matrix(1.12, 0.0, 0.0, 1.12, 0.0, 0.0)), [50.0, 50.0]),
        HistoryEntrance::Unfold => (Some(matrix(1.0, 0.0, 0.0, 0.55, 0.0, 0.0)), [50.0, 0.0]),
        HistoryEntrance::Tilt => {
            let (sin, cos) = (-3.0_f32).to_radians().sin_cos();
            (Some(matrix(cos, sin, -sin, cos, 0.0, 10.0)), [side, 100.0])
        }
    };
    let mut enter = nana_ui::runtime::view::Presence::new(ms)
        .opacity(0.0)
        .ease(ease);
    if let Some(from) = from {
        enter = enter.transform(from);
        if let Some(settle) = settle {
            enter = enter.ease_transform(settle);
        }
    }
    let origin = TransformOrigin {
        x: LengthSpec::Percent(origin[0]),
        y: LengthSpec::Percent(origin[1]),
    };
    // A dropped row goes at once: the rows below glide into its place.
    Some((Transition::new().enter(enter), origin))
}

/// The curve rows already on screen glide on: the skin's, without the
/// overshoot a whole list would wobble with.
pub(super) fn glide_ease(h: &nanatype_theme::History) -> Easing {
    match h.entrance_ease {
        HistoryEase::Gentle => ROW_GENTLE,
        HistoryEase::Linear => Easing::Linear,
        HistoryEase::Smooth | HistoryEase::Overshoot => ROW_EASE,
    }
}

/// The rule between two speakers, in the middle of the gap above a row.
pub(super) fn separator_view(skin: &Skin, gap: f32) -> Option<AnyView> {
    let rule = skin.history.separator.as_ref().filter(|r| r.width > 0.0)?;
    let color = skin.color(rule.color);
    let (width, inset, fade) = (rule.width, rule.inset, rule.fade);
    let clear = Rgba8::rgba(color.r, color.g, color.b, 0);
    let stop = |position: f32, col: Rgba8| GradientStop {
        position,
        color: nana_ui_core::PaintColor::srgb(c(col)),
    };
    Some(
        widget(Stack::row(0.0).with_layout(move |l| {
            l.position = PositionSpec::Absolute;
            l.offset_top = Some(LengthSpec::Px(((gap - width) / 2.0).max(0.0)));
            l.offset_left = Some(LengthSpec::Px(inset));
            l.offset_right = Some(LengthSpec::Px(inset));
            l.height = Some(LengthSpec::Px(width));
            l.pointer_events = Some(PointerEventsSpec::None);
            if fade {
                l.paint.background_layers = vec![BackgroundImage::Gradient(CssGradient::Linear(
                    LinearGradient {
                        angle_deg: 90.0,
                        stops: vec![
                            stop(0.0, clear),
                            stop(0.18, color),
                            stop(0.82, color),
                            stop(1.0, clear),
                        ],
                    },
                ))];
            } else {
                l.paint_colors.background = Some(nana_ui_core::PaintColor::srgb(c(color)));
            }
        }))
        .into_any(),
    )
}

/// Fade the entire bubble artwork as one layer, leaving its text at full
/// strength. Reserving the artwork's borders in the content padding keeps
/// image frames, retained contours and ordinary CSS surfaces on the same path.
fn bubble_view_with(
    surface: Surface,
    opacity: f32,
    pad: [f32; 2],
    stretch: bool,
    max: impl Fn() -> f32 + Send + Sync + 'static,
    body: AnyView,
) -> AnyView {
    let widths = surface.frame.as_ref().map_or_else(
        || surface.border.as_ref().map_or([0.0; 4], |b| b.widths),
        |(_, widths)| *widths,
    );
    let [top, right, bottom, left] = widths;
    let backdrop = widget(Stack::column(0.0).with_layout(move |l| {
        l.position = PositionSpec::Absolute;
        // An absolute box hangs from its parent's padding box, as in CSS:
        // zero insets paint the full outer box, reserved padding included.
        l.offset_top = Some(LengthSpec::Px(0.0));
        l.offset_right = Some(LengthSpec::Px(0.0));
        l.offset_bottom = Some(LengthSpec::Px(0.0));
        l.offset_left = Some(LengthSpec::Px(0.0));
        l.pointer_events = Some(PointerEventsSpec::None);
        l.opacity = Some(opacity);
    }))
    // Opacity belongs to a parent group. A leaf's CSS gradient does not
    // participate in NanaUI's primitive alpha path, unlike its solid border.
    .children(widget(surface.stack(Stack::fill_column(0.0))));
    widget(
        Stack::column(0.0)
            .width(LengthSpec::Shrink)
            .with_layout(move |l| {
                l.position = PositionSpec::Relative;
                l.padding_top = Some(LengthSpec::Px(pad[1] + top));
                l.padding_right = Some(LengthSpec::Px(pad[0] + right));
                l.padding_bottom = Some(LengthSpec::Px(pad[1] + bottom));
                l.padding_left = Some(LengthSpec::Px(pad[0] + left));
            }),
    )
    .bind(move |s: &mut Stack| {
        let m = max();
        let l = s.layout_mut();
        l.max_width = Some(LengthSpec::Px(m));
        if stretch {
            l.width = Some(LengthSpec::Px(m));
        }
    })
    .children((backdrop, body))
    .into_any()
}

#[cfg(test)]
pub(super) fn bubble_view(
    surface: Surface,
    opacity: f32,
    pad: [f32; 2],
    max: f32,
    stretch: bool,
    body: AnyView,
) -> AnyView {
    bubble_view_with(surface, opacity, pad, stretch, move || max, body)
}

#[cfg(test)]
thread_local! {
    /// Rows built on this thread, to tell a rebuild from a kept row.
    pub(super) static ROWS_BUILT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

pub(super) fn message_view(
    ui: Stage,
    row_sig: Signal<HistoryRow>,
    follows: bool,
    skin: Arc<Skin>,
    settings: Arc<nanatype_settings::Settings>,
    col_w: Computed<f32>,
) -> AnyView {
    #[cfg(test)]
    ROWS_BUILT.set(ROWS_BUILT.get() + 1);
    let materials_on = super::super::materials::enabled(ui);
    let h = skin.history.clone();
    let backdrop = h
        .panel
        .as_ref()
        .and_then(|p| super::super::quote::backdrop(p.color, false));
    let exposed = backdrop.is_none();
    let show_avatar = settings.history.show_avatar && h.avatar.shape != AvatarShape::None;
    let cw = nana_ui::runtime::view::untrack(|| col_w.get());
    let row = row_sig.get_untracked();
    let inline = h.layout == HistoryLayout::Inline && cw >= 160.0;
    let centered = h.align == HistoryAlign::Center && !inline;
    let look = EventLook::of(&skin, &row.item);
    // A card or a notice is the whole row: it neither groups nor sits in
    // the name and time columns.
    let boxed = look
        .as_ref()
        .map(|l| l.layout)
        .filter(|l| *l != HistoryEventLayout::Bubble);
    // Echo-Live's log names every line; bubble lists group.
    let follows = follows && !inline && boxed.is_none();
    let size = h.avatar.size;
    let radius = match h.avatar.shape {
        AvatarShape::Circle => size / 2.0,
        _ => h.avatar.radius.unwrap_or(size * 0.18),
    };
    // Chat rows name a saved picture (`blob:…`); the local speaker's avatar
    // is a path or a URL.
    let chat = row.item.kind != SourceKind::Typewriter;
    let bubble_opacity = match &look {
        Some(look) => look.opacity,
        None if chat => h.chat.bubble_opacity.clamp(0.0, 1.0),
        None => 1.0,
    };
    let quote = row
        .item
        .reply
        .clone()
        .filter(|_| settings.history.show_quote && h.reply.visible)
        .map(|reply| {
            // The quote sits above the bubble, on the panel (if any).
            super::super::quote::quote_view(ui, &skin, &h.reply, &reply, &h.text, backdrop)
        });
    // All picture-less speakers use the skin's palette. Real profile pictures
    // remain intact; speaker identity is carried by the name and role label.
    let accent = skin.chrome.accent;
    let (fill, fill_gradient) = match h.avatar.color {
        Some(col) => (skin.color(col), None),
        None => (accent, Some(avatar_gradient(accent, lighten(accent, 0.35)))),
    };
    let ring = h
        .avatar
        .ring_color
        .filter(|_| h.avatar.ring_width > 0.0)
        .map(|col| (h.avatar.ring_width, c(skin.color(col))));
    let mut initial_style = NodeStyle::default();
    {
        let l = Arc::make_mut(&mut initial_style.layout);
        l.font_size = Some(h.name_size.min(size * 0.7));
        l.font_weight = Some(h.name_weight);
        l.text_ext.font_family = skin.fonts.name.families.first().cloned();
        l.paint_colors.color = Some(nana_ui_core::PaintColor::srgb(c(
            if crate::skin_theme::contrast(h.text.color, fill) >= 4.5 {
                h.text.color
            } else {
                crate::skin_theme::on(fill)
            },
        )));
    }
    let avatar: AnyView = if show_avatar && follows {
        // The group's later rows keep the slot, so bubbles line up.
        widget(
            Stack::row(0.0)
                .width(LengthSpec::Px(size))
                .with_layout(|l| l.flex_shrink = Some(0.0)),
        )
        .into_any()
    } else if show_avatar {
        // The initial on its colour, with the picture laid over it: a
        // picture still loading (or unreadable) leaves the initial showing.
        let asset = row.item.avatar.clone();
        widget(
            Stack::row(0.0)
                .width(LengthSpec::Px(size))
                .height(LengthSpec::Px(size))
                .align(AlignSpec::Center)
                .justify(nana_ui_core::JustifySpec::Center)
                .with_layout(move |l| {
                    l.position = PositionSpec::Relative;
                    l.border_radius = Some(radius);
                    l.flex_shrink = Some(0.0);
                    if let Some((w, col)) = ring {
                        l.border_width = Some(w);
                        l.paint_colors.border = Some(nana_ui_core::PaintColor::srgb(col));
                        l.border_style = Some(nana_ui_core::BorderStyle::Solid);
                    }
                    l.overflow_x = OverflowSpec::Hidden;
                    l.overflow_y = OverflowSpec::Hidden;
                    l.paint_colors.background = Some(nana_ui_core::PaintColor::srgb(c(fill)));
                    l.paint.background_layers = fill_gradient.into_iter().collect();
                }),
        )
        .children((
            widget(
                Text::new(initial(&row.item.author))
                    .decorative()
                    .style(initial_style),
            ),
            Some(super::super::avatar::reactive_picture(ui, asset, radius)),
        ))
        .into_any()
    } else {
        widget(Stack::row(0.0)).into_any()
    };

    let mut name_style = NodeStyle::default();
    {
        let l = Arc::make_mut(&mut name_style.layout);
        l.paint.text_material = materials_on
            .then(|| super::super::materials::resolve(&skin, h.text.material.as_deref()))
            .flatten();
        l.font_size = Some(h.name_size);
        l.font_weight = Some(h.name_weight);
        l.text_ext.font_family = skin.fonts.name.families.first().cloned();
        l.paint_colors.color = Some(nana_ui_core::PaintColor::srgb(c(h.name_color)));
        if let Some(p) = &h.name_plate {
            l.paint_colors.color =
                Some(nana_ui_core::PaintColor::srgb(c(skin.color(p.text_color))));
        } else if let Some(col) = look.as_ref().and_then(|l| l.style.name_color) {
            l.paint_colors.color = Some(nana_ui_core::PaintColor::srgb(c(skin.color(col))));
        }
        if let Some(stroke) = &h.name_stroke {
            surface::text_stroke(l, stroke);
        } else if exposed && h.name_plate.is_none() {
            surface::text_stroke(
                l,
                &nanatype_theme::TextStrokeSpec {
                    color: crate::skin_theme::on(h.name_color),
                    width: 1.0,
                },
            );
        }
    }
    let name_plate = Surface::history_name_plate(&skin).map(|s| s.materials(materials_on));
    let plate_pad = h.name_plate.as_ref().map_or([0.0; 2], |p| p.padding);
    let plate_border = name_plate
        .as_ref()
        .and_then(|p| p.border.as_ref())
        .map_or(0.0, |b| b.widths[1] + b.widths[3]);
    let mut time_style = NodeStyle::default();
    {
        let l = Arc::make_mut(&mut time_style.layout);
        l.font_size = Some(time_size(&h));
        l.font_weight = Some(skin.fonts.body.weight);
        l.text_ext.font_family = skin.fonts.body.families.first().cloned();
        l.paint_colors.color = Some(nana_ui_core::PaintColor::srgb(c(h.time_color)));
        if exposed {
            surface::text_stroke(
                l,
                &nanatype_theme::TextStrokeSpec {
                    color: crate::skin_theme::on(h.time_color),
                    width: 1.0,
                },
            );
        }
        l.white_space = nana_ui_core::WhiteSpaceSpec::Nowrap;
        super::super::tnum(l);
    }
    let show_name = settings.history.show_name && !row.item.author.is_empty() && !follows;
    let show_time = settings.history.show_time && !follows && (!inline || inline_time_fits(&h, cw));
    // Who is talking, once live chat shares the history list.
    let role = (show_name
        && !inline
        && ui.log_canvas == crate::canvas::Canvas::History
        && (chat || !settings.history.chat.separate))
        .then(|| {
            let key = if chat {
                "history-role-viewer"
            } else {
                "history-role-host"
            };
            widget(Text::new(ui.now_t(key)).style(time_style.clone()))
        });
    // On a card the name and the time sit on the event's own colour.
    let card_styles = look.as_ref().map(|look| {
        let (mut name, mut time) = (name_style.clone(), time_style.clone());
        let opaque = look.fill.a >= 128;
        for (style, color) in [(&mut name, look.name), (&mut time, look.text)] {
            let l = Arc::make_mut(&mut style.layout);
            l.paint_colors.color = Some(nana_ui_core::PaintColor::srgb(c(color)));
            if opaque {
                l.paint.text_stroke = None;
            }
            l.min_width = Some(LengthSpec::Px(0.0));
        }
        // A long name gives way to the amount beside it.
        Arc::make_mut(&mut name.layout).flex_shrink = Some(1.0);
        Arc::make_mut(&mut time.layout).opacity = Some(0.7);
        (name, time)
    });
    let h_name_calc = h.clone();
    let max_name_for = move |cw: f32| {
        if inline {
            inline_name_width(&h_name_calc, cw, show_time)
        } else {
            cw - 64.0
        }
        .max(32.0)
    };
    let name = show_name.then(|| {
        let plate = name_plate.clone();
        let l = Arc::make_mut(&mut name_style.layout);
        // A longer role or time shrinks the plate further: the name
        // shrinks with it, so its ellipsis stays inside rather than clipped.
        l.min_width = Some(LengthSpec::Px(0.0));
        l.flex_shrink = Some(1.0);
        let max_name_text = max_name_for.clone();
        let max_name_plate = max_name_for.clone();
        widget(
            Surface::optional_stack(
                plate.as_ref(),
                Stack::row(0.0).padding_xy(plate_pad[0], plate_pad[1]),
            )
            .with_layout(|l| {
                l.min_width = Some(LengthSpec::Px(0.0));
                l.overflow_x = OverflowSpec::Hidden;
                l.flex_shrink = Some(1.0);
            }),
        )
        .bind(move |s: &mut Stack| {
            s.layout_mut().max_width = Some(LengthSpec::Px(max_name_plate(col_w.get())));
        })
        .children(
            widget(
                Text::new(h.name(&row.item.author))
                    .style(name_style)
                    .truncating(),
            )
            .bind(move |t: &mut Text| {
                let mn = max_name_text(col_w.get());
                Arc::make_mut(&mut t.style.layout).max_width = Some(LengthSpec::Px(
                    (mn - plate_pad[0] * 2.0 - plate_border).max(12.0),
                ));
            }),
        )
    });
    let time = show_time
        .then(|| widget(Text::new(local_time(row.item.at, h.time_seconds)).style(time_style)));

    let pad = look.as_ref().map_or(h.bubble_padding, |l| l.pad);
    let frame_url = h.bubble_frame.as_ref().and_then(|f| {
        ui.asset_urls
            .with_untracked(|m| m.get(&format!("skin:{}", f.image)).cloned())
    });
    let mut bubble_surface = match &look {
        Some(look) => look.surface(&skin, frame_url),
        None => Surface::bubble(&skin, frame_url),
    }
    .materials(materials_on);
    // How much of what is behind the row its artwork hides.
    let surface_alpha =
        f32::from(look.as_ref().map_or(h.bubble_color, |l| l.fill).a) * bubble_opacity;
    let event_doc = look.as_ref().map(|look| look.dress(row.doc()));
    let body_look = look.clone();
    if follows {
        // Only a group's first bubble points at the avatar.
        bubble_surface.radii[0] = bubble_surface.radii[0].max(h.bubble_radius);
    }
    let body_skin = skin.clone();
    let reduced_motion = ui.reduced_motion.get_untracked();
    let sticker_scale = settings.assets.sticker_scale;
    let body = dynamic(
        move || {
            row_sig.with(|r| {
                (
                    r.reveal.is_some(),
                    r.reveal.as_ref().map(|x| {
                        (
                            x.shown,
                            x.hidden_from,
                            x.segment.as_ref().map(|s| s.origin_ms),
                        )
                    }),
                )
            })
        },
        move |_| {
            let row = row_sig.get_untracked();
            let skin = body_skin.clone();
            let entrance = Entrance {
                kind: skin.dialogue.reveal.entrance.kind,
                ms: skin.dialogue.reveal.entrance.ms,
            };
            let mut base = Base::history(&skin, entrance, reduced_motion);
            base.sticker_scale = sticker_scale;
            if row.item.kind != SourceKind::Typewriter {
                let scale = body_look
                    .as_ref()
                    .map_or(skin.history.chat.text_scale, |l| l.scale);
                if scale.is_finite() && scale > 0.0 {
                    base.size *= scale.clamp(0.3, 3.0);
                }
            }
            if let Some(look) = body_look.as_ref().filter(|l| l.text != base.color) {
                base.color = look.text;
                // An outline or glow drawn for the skin's own ink would
                // smear a colour chosen for this fill.
                if look.fill.a >= 128 {
                    base.stroke = None;
                    base.shadow = None;
                }
            }
            let dressed = body_look.as_ref().map(|look| look.dress(row.doc()));
            let doc = dressed.as_ref().unwrap_or_else(|| row.doc());
            let timing = match &row.reveal {
                Some(r) => Timing::Reveal(r.clone()),
                None => Timing::Static,
            };
            let see_through = exposed && surface_alpha < 128.0;
            if see_through && base.stroke.is_none() {
                document(ui, &transparent_text(doc, base.color), &base, timing)
            } else {
                document(ui, doc, &base, timing)
            }
        },
    );
    // Inline rows share the column with the name and the time; the time's
    // width is estimated from its digits.
    let h_bubble = h.clone();
    let bubble_max = move || {
        let cw = col_w.get();
        if inline {
            let name_w = if show_name {
                inline_name_width(&h_bubble, cw, show_time) + 10.0
            } else {
                0.0
            };
            let digits = if h_bubble.time_seconds { 4.6 } else { 2.9 };
            let time_w = if show_time {
                time_size(&h_bubble) * digits + 10.0
            } else {
                0.0
            };
            (cw - name_w - time_w).max(1.0)
        } else {
            cw
        }
    };
    let mark_size = (h.text.size * look.as_ref().map_or(1.0, |l| l.scale)).max(9.0);
    let badge_size = (mark_size * 0.72).max(10.0);
    let icon = look.as_ref().and_then(|l| l.icon_view(&skin, mark_size));
    let badge = look.as_ref().and_then(|l| l.badge_view(&skin, badge_size));
    let has_text = event_doc.as_ref().is_none_or(|doc| !doc.is_blank());
    let text_slot = |body: AnyView| {
        widget(Stack::row(0.0).width(LengthSpec::Shrink).with_layout(|l| {
            l.flex_shrink = Some(1.0);
            l.min_width = Some(LengthSpec::Px(0.0));
        }))
        .children(body)
        .into_any()
    };
    let avatar_extra = if show_avatar { size + 10.0 } else { 0.0 };
    if let Some(layout) = boxed {
        let (name_style, time_style) = card_styles.clone().unwrap_or_default();
        let row_w = move || col_w.get() + avatar_extra;
        let content = match layout {
            HistoryEventLayout::Notice => {
                let line = widget(Stack::row(6.0).width(LengthSpec::Fill).with_layout(|l| {
                    l.align_items = AlignSpec::Baseline;
                    l.min_width = Some(LengthSpec::Px(0.0));
                }))
                .children((
                    icon,
                    show_name.then(|| {
                        let mut style = name_style;
                        Arc::make_mut(&mut style.layout).max_width =
                            Some(LengthSpec::Px(((cw + avatar_extra) * 0.4).max(32.0)));
                        widget(Text::new(row.item.author.clone()).style(style).truncating()).bind(
                            move |t: &mut Text| {
                                let rw = col_w.get() + avatar_extra;
                                Arc::make_mut(&mut t.style.layout).max_width =
                                    Some(LengthSpec::Px((rw * 0.4).max(32.0)));
                            },
                        )
                    }),
                    // Takes the spare width, so the amount sits at the right
                    // edge.
                    widget(Stack::row(0.0).with_layout(|l| {
                        l.flex_grow = Some(1.0);
                        l.flex_shrink = Some(1.0);
                        l.min_width = Some(LengthSpec::Px(0.0));
                    }))
                    .children(has_text.then(|| body.into_any())),
                    badge,
                ));
                bubble_view_with(
                    bubble_surface,
                    bubble_opacity,
                    pad,
                    true,
                    row_w,
                    line.into_any(),
                )
            }
            _ => {
                let head = widget(
                    Stack::row(6.0)
                        .align(AlignSpec::Center)
                        .width(LengthSpec::Fill)
                        .with_layout(|l| l.min_width = Some(LengthSpec::Px(0.0))),
                )
                .children((
                    icon,
                    show_name.then(|| {
                        widget(
                            Text::new(row.item.author.clone())
                                .style(name_style)
                                .truncating(),
                        )
                    }),
                    // Takes the spare width: the amount and the time sit at
                    // the right edge.
                    widget(Stack::row(0.0).with_layout(|l| {
                        l.flex_grow = Some(1.0);
                    })),
                    badge,
                    settings.history.show_time.then(|| {
                        widget(Text::new(local_time(row.item.at, h.time_seconds)).style(time_style))
                            .visible(move || (col_w.get() + avatar_extra) - 2.0 * pad[0] >= 200.0)
                    }),
                ));
                let text = widget(Stack::column(4.0).with_layout(|l| {
                    l.flex_grow = Some(1.0);
                    l.flex_shrink = Some(1.0);
                    l.min_width = Some(LengthSpec::Px(0.0));
                }))
                .children((head, has_text.then(|| body.into_any())));
                let inner = widget(
                    Stack::row(10.0)
                        .align(if has_text {
                            AlignSpec::Start
                        } else {
                            AlignSpec::Center
                        })
                        .width(LengthSpec::Fill),
                )
                .children((show_avatar.then_some(avatar), text));
                bubble_view_with(
                    bubble_surface,
                    bubble_opacity,
                    pad,
                    true,
                    row_w,
                    inner.into_any(),
                )
            }
        };
        return row_frame(ui, &settings, &h, row.item.at, None, content);
    }
    let body = if icon.is_some() || badge.is_some() {
        widget(Stack::row(6.0).width(LengthSpec::Shrink).with_layout(|l| {
            l.align_items = AlignSpec::Baseline;
            l.min_width = Some(LengthSpec::Px(0.0));
        }))
        .children((icon, badge, has_text.then(|| text_slot(body.into_any()))))
        .into_any()
    } else {
        body.into_any()
    };
    let bubble = bubble_view_with(
        bubble_surface,
        bubble_opacity,
        pad,
        false,
        bubble_max.clone(),
        body,
    );
    // Inline rows put the quote on its own line above the whole row (over
    // the text column), so the name, the first line and the time keep
    // sharing a baseline; stacked rows put it straight above the bubble.
    let (quote, inline_quote) = if inline { (None, quote) } else { (quote, None) };
    let bubble = match quote {
        Some(q) => widget(
            Stack::column(2.0)
                .width(LengthSpec::Shrink)
                .with_layout(move |l| {
                    l.align_items = if centered {
                        AlignSpec::Center
                    } else {
                        AlignSpec::Start
                    };
                }),
        )
        .bind(move |s: &mut Stack| {
            s.layout_mut().max_width = Some(LengthSpec::Px(bubble_max()));
        })
        .children((q, bubble))
        .into_any(),
        None => bubble,
    };

    // Inline skins keep their name/body/time baseline.
    let content = if inline {
        let h_inline = h.clone();
        let name_w = move |cw: f32| inline_name_width(&h_inline, cw, show_time);
        let quote_line = inline_quote.map(|q| {
            let name_w_q = name_w.clone();
            widget(Stack::column(0.0).with_layout(|l| {
                l.min_width = Some(LengthSpec::Px(0.0));
                l.flex_shrink = Some(0.0);
            }))
            .bind(move |s: &mut Stack| {
                let cw = col_w.get();
                let indent = if show_name { name_w_q(cw) + 10.0 } else { 0.0 };
                let l = s.layout_mut();
                l.margin_left = Some(LengthSpec::Px(indent));
                l.width = Some(LengthSpec::Px((cw - indent).max(40.0)));
            })
            .children(q)
        });
        let line = widget(Stack::row(10.0).with_layout(|l| {
            l.min_width = Some(LengthSpec::Px(0.0));
            // Name, first line and time share a baseline.
            l.align_items = AlignSpec::Baseline;
        }))
        .bind(move |s: &mut Stack| {
            s.layout_mut().width = Some(LengthSpec::Px(col_w.get()));
        })
        .children((
            name.map(|n| {
                widget(Stack::row(0.0).with_layout(|l| l.flex_shrink = Some(0.0)))
                    .bind(move |s: &mut Stack| {
                        s.layout_mut().width = Some(LengthSpec::Px(name_w(col_w.get())));
                    })
                    .children(n)
            }),
            // Takes the spare width, so the time sits at the right edge.
            widget(Stack::row(0.0).with_layout(|l| {
                l.flex_grow = Some(1.0);
                l.flex_shrink = Some(1.0);
                l.min_width = Some(LengthSpec::Px(0.0));
            }))
            .children(bubble),
            time.map(|t| {
                widget(Stack::row(0.0).with_layout(|l| {
                    l.flex_shrink = Some(0.0);
                }))
                .children(t)
            }),
        ));
        widget(Stack::column(2.0).with_layout(|l| {
            l.min_width = Some(LengthSpec::Px(0.0));
            l.align_items = AlignSpec::Start;
        }))
        .bind(move |s: &mut Stack| {
            s.layout_mut().width = Some(LengthSpec::Px(col_w.get()));
        })
        .children((quote_line, line))
        .into_any()
    } else {
        let header = (name.is_some() || time.is_some()).then(|| {
            widget(
                Stack::row(6.0)
                    .align(AlignSpec::Center)
                    .justify(if centered {
                        nana_ui_core::JustifySpec::Center
                    } else {
                        nana_ui_core::JustifySpec::Start
                    })
                    .width(LengthSpec::Fill),
            )
            .bind(move |s: &mut Stack| {
                s.layout_mut().flex_wrap = if col_w.get() < 180.0 {
                    FlexWrap::Wrap
                } else {
                    FlexWrap::NoWrap
                };
            })
            .children((name, role, time))
            .into_any()
        });
        widget(Stack::column(6.0).with_layout(move |l| {
            l.min_width = Some(LengthSpec::Px(0.0));
            l.align_items = if centered {
                AlignSpec::Center
            } else {
                AlignSpec::Start
            };
        }))
        .bind(move |s: &mut Stack| {
            s.layout_mut().width = Some(LengthSpec::Px(col_w.get()));
        })
        .children((header, bubble))
        .into_any()
    };

    // Inline rows drop the empty avatar slot, so the name sits at the edge;
    // centred rows drop it so the column is the whole row.
    let avatar = (show_avatar || !(inline || centered)).then_some(avatar);
    row_frame(ui, &settings, &h, row.item.at, avatar, content)
}

/// A finished row: its avatar slot and content, fading with age when the
/// user set that.
fn row_frame(
    ui: Stage,
    settings: &nanatype_settings::Settings,
    h: &nanatype_theme::History,
    at: u64,
    avatar: Option<AnyView>,
    content: AnyView,
) -> AnyView {
    let fade_after = settings.history.fade_after_s;
    let view = widget(
        Stack::row(10.0)
            .align(AlignSpec::Start)
            .width(LengthSpec::Fill),
    )
    .bind(move |s: &mut Stack| {
        if fade_after.is_none() {
            return;
        }
        s.layout_mut().opacity = Some(row_opacity(ui, at, fade_after));
    })
    .children((avatar, content))
    .into_any();
    if h.row_padding > 0.0 {
        widget(
            Stack::column(0.0)
                .width(LengthSpec::Fill)
                .padding_xy(h.row_padding, 0.0),
        )
        .children(view)
        .into_any()
    } else {
        view
    }
}
