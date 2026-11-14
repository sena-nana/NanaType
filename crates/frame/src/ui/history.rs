//! The history window: a live-chat-style column of avatar, name and bubble.
//! Rows are drawn by the same document builder as the dialogue box, so a line
//! keeps its colours, effects and stickers; with "typing along" sync the
//! newest row reveals in step with the typewriter.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use nana_ui::runtime::view::{
    AnyView, Computed, Implicit, IntoView, Signal, Transition, computed, dynamic, each, signal,
    widget,
};
use nana_ui::runtime::{
    Activate, AnimatableProperty, IconButton, NodeStyle, PaintTransform, ScrollAxes, ScrollView,
    SizeChanged, Stack, Text, TextChanged, TextInput, TextInputState,
};
use nana_ui::{ButtonKind, ControlSize};
use nana_ui_core::{
    AlignSpec, BackgroundImage, CssGradient, Easing, GradientStop, LengthSpec, LinearGradient,
    OverflowSpec, PointerEventsSpec, PositionSpec, TransformOrigin,
};
use nanatype_core::HistoryRow;
use nanatype_model::{
    Document, Entrance, EventKind, HistoryItem, Inline, LineId, Rgba8, SourceKind, Stroke,
};
use nanatype_theme::{
    AvatarShape, DecorLayer, EventPriceColor, HistoryAlign, HistoryEase, HistoryEntrance,
    HistoryEventLayout, HistoryLayout, HistoryTitle, Skin,
};

use super::dialogue::{self, Base, Timing, document};
use super::surface::{self, Surface};
use super::{Stage, c};

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
const ROW_EASE: Easing = Easing::CubicBezier([0.2, 0.8, 0.2, 1.0]);
/// Slow at both ends: paper, ink, water.
const ROW_GENTLE: Easing = Easing::CubicBezier([0.45, 0.05, 0.25, 1.0]);
/// Past its place by about a tenth, then back.
const ROW_OVERSHOOT: Easing = Easing::CubicBezier([0.3, 1.45, 0.5, 1.0]);
/// How long an older row takes to step back when a new one arrives.
const RECEDE_MS: u64 = 320;
const ROW_FADE_MS: u64 = 600;
const FADE_FRAME_SLACK_MS: u64 = 100;

/// Meta text (time, role) is a step below the name: 11px beside a 14px name.
fn time_size(h: &nanatype_theme::History) -> f32 {
    h.time_size.unwrap_or((h.name_size - 3.0).max(9.0))
}

/// Keep enough room for a short phrase and an @ label in narrow inline logs.
/// At the skin's normal width this leaves its requested name column unchanged.
fn inline_name_width(h: &nanatype_theme::History, col_w: f32, show_time: bool) -> f32 {
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
fn inline_time_fits(h: &nanatype_theme::History, col_w: f32) -> bool {
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
        paint_color: None,
        position,
        color: c(col),
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
fn column_width(
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

fn viewport_width(ui: Stage, skin: &Skin, settings: &nanatype_settings::Settings) -> f32 {
    let measured = ui.history_width.get();
    if measured > 0.0 {
        measured
    } else {
        crate::canvas::size_of(ui.log_canvas, skin, settings).0
    }
}

fn viewport_height(ui: Stage, skin: &Skin, settings: &nanatype_settings::Settings) -> f32 {
    let measured = ui.history_height.get();
    if measured > 0.0 {
        measured
    } else {
        crate::canvas::size_of(ui.log_canvas, skin, settings).1
    }
}

/// Fade the entire bubble artwork as one layer, leaving its text at full
/// strength. Reserving the artwork's borders in the content padding keeps
/// image frames, retained contours and ordinary CSS surfaces on the same path.
fn bubble_view(
    surface: Surface,
    opacity: f32,
    pad: [f32; 2],
    max: f32,
    stretch: bool,
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
            .width(if stretch {
                LengthSpec::Px(max)
            } else {
                LengthSpec::Shrink
            })
            .with_layout(move |l| {
                l.position = PositionSpec::Relative;
                l.max_width = Some(LengthSpec::Px(max));
                l.padding_top = Some(LengthSpec::Px(pad[1] + top));
                l.padding_right = Some(LengthSpec::Px(pad[0] + right));
                l.padding_bottom = Some(LengthSpec::Px(pad[1] + bottom));
                l.padding_left = Some(LengthSpec::Px(pad[0] + left));
            }),
    )
    .children((backdrop, body))
    .into_any()
}

/// A gift, paid message or membership as the skin dresses it, every
/// fallback to the message row already applied.
#[derive(Clone)]
struct EventLook {
    layout: HistoryEventLayout,
    style: nanatype_theme::HistoryEventStyle,
    /// The colour under the text; the bubble's when the row has no own fill.
    fill: Rgba8,
    text: Rgba8,
    name: Rgba8,
    icon: Rgba8,
    opacity: f32,
    scale: f32,
    pad: [f32; 2],
    /// The platform's colour for the amount, where the skin wants it.
    price: Option<(EventPriceColor, Rgba8)>,
    /// The amount in its pill: text, fill, lettering.
    badge: Option<(String, Rgba8, Rgba8)>,
}

impl EventLook {
    fn of(skin: &Skin, item: &HistoryItem) -> Option<Self> {
        let h = &skin.history;
        if item.kind != SourceKind::Event {
            return None;
        }
        let info = item.event.as_ref();
        let style = h
            .event
            .as_ref()?
            .resolve(info.map_or(EventKind::Notice, |e| e.kind));
        let placement = style.price_color.unwrap_or_default();
        let price = info
            .and_then(|e| e.color)
            .filter(|_| placement != EventPriceColor::None)
            .map(|rgb| Rgba8::rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8));
        let filled = price.filter(|_| placement == EventPriceColor::Fill);
        let fill = filled.unwrap_or(style.color.map_or(h.bubble_color, |col| skin.color(col)));
        // Only a fill that hides what is behind it can be measured against.
        let opaque = fill.a >= 128;
        let readable = |wanted: Rgba8, least: f32, otherwise: Rgba8| {
            if opaque && crate::skin_theme::contrast(wanted, fill) < least {
                otherwise
            } else {
                wanted
            }
        };
        let pick = |col: Option<nanatype_theme::ColorRef>, default: Rgba8| {
            col.map_or(default, |col| skin.color(col))
        };
        let text = readable(
            pick(style.text_color, h.text.color),
            4.5,
            crate::skin_theme::on(fill),
        );
        // The skin chose its name and icon colours for its own fill, not
        // for whichever colour the platform hands over.
        let on_fill = |wanted: Rgba8| match filled {
            Some(_) => text,
            None => readable(wanted, 3.0, text),
        };
        let name = on_fill(pick(style.name_color, h.name_color));
        let icon = on_fill(pick(style.icon_color, name));
        let badge = info
            .and_then(|e| e.amount.clone())
            .filter(|_| style.badge.is_some() || placement == EventPriceColor::Badge)
            .map(|amount| {
                let spec = style.badge.clone().unwrap_or_default();
                let (plate, lettering) = match price {
                    Some(price) if placement == EventPriceColor::Badge => {
                        (price, crate::skin_theme::on(price))
                    }
                    // On the platform's colour the pill takes the text's.
                    Some(_) if filled.is_some() => (text, fill),
                    _ => {
                        let plate = skin.color(spec.color);
                        let lettering = skin.color(spec.text_color);
                        if crate::skin_theme::contrast(lettering, plate) < 4.5 {
                            (plate, crate::skin_theme::on(plate))
                        } else {
                            (plate, lettering)
                        }
                    }
                };
                (amount, plate, lettering)
            });
        Some(Self {
            layout: style.layout.unwrap_or_default(),
            fill,
            text,
            name,
            icon,
            opacity: style
                .opacity
                .unwrap_or(if style.has_surface() || filled.is_some() {
                    1.0
                } else {
                    h.chat.bubble_opacity
                })
                .clamp(0.0, 1.0),
            scale: style.text_scale.unwrap_or(h.chat.text_scale),
            pad: style.padding.unwrap_or(h.bubble_padding),
            price: price.map(|price| (placement, price)),
            badge,
            style,
        })
    }

    /// The row's surface, the platform's colour laid where the skin put it.
    fn surface(&self, skin: &Skin, frame_url: Option<String>) -> Surface {
        let mut surface = Surface::event(skin, &self.style, frame_url);
        match self.price {
            Some((EventPriceColor::Fill, price)) => {
                if surface.frame.take().is_some() {
                    surface.radii = [skin.history.bubble_radius; 4];
                }
                surface.fill = Some(c(price));
                surface.gradient = None;
                surface.material = None;
            }
            Some((EventPriceColor::Border, price)) => {
                surface.frame = None;
                surface.border_material = None;
                match &mut surface.border {
                    Some(border) => border.color = c(price),
                    None => {
                        surface.border = Some(surface::Border {
                            widths: [2.0; 4],
                            color: c(price),
                            style: Default::default(),
                        });
                    }
                }
            }
            _ => {}
        }
        surface
    }

    /// The row's text as this look shows it: without the amount the badge
    /// already carries.
    fn dress(&self, doc: &Document) -> Document {
        let mut inlines: Vec<Inline> = doc
            .paragraphs
            .iter()
            .flat_map(|p| p.inlines.iter().cloned())
            .collect();
        if let Some((amount, ..)) = &self.badge
            && matches!(inlines.first(), Some(Inline::Text(run)) if run.text.trim_end() == amount)
        {
            inlines.remove(0);
        }
        if inlines.is_empty() {
            Document::new()
        } else {
            Document::from_inlines(inlines)
        }
    }

    fn icon_view(&self, skin: &Skin, size: f32) -> Option<AnyView> {
        let glyph = self.style.icon.clone().filter(|g| !g.is_empty())?;
        let mut style = NodeStyle::default();
        let l = Arc::make_mut(&mut style.layout);
        l.font_size = Some(size);
        l.font_family = skin.fonts.accent.families.first().cloned();
        l.color = Some(c(self.icon));
        l.white_space_nowrap = true;
        l.flex_shrink = Some(0.0);
        Some(widget(Text::new(glyph).decorative().style(style)).into_any())
    }

    fn badge_view(&self, skin: &Skin, size: f32) -> Option<AnyView> {
        let (amount, plate, lettering) = self.badge.clone()?;
        let spec = self.style.badge.clone().unwrap_or_default();
        let mut style = NodeStyle::default();
        let l = Arc::make_mut(&mut style.layout);
        l.font_size = Some(size);
        l.font_weight = Some(700);
        l.font_family = skin.fonts.name.families.first().cloned();
        l.color = Some(c(lettering));
        l.white_space_nowrap = true;
        super::tnum(l);
        let border = spec
            .border_color
            .filter(|_| spec.border_width > 0.0 && self.price.is_none())
            .map(|col| (spec.border_width, c(skin.color(col))));
        Some(
            widget(
                Stack::row(0.0)
                    .padding_xy((size * 0.6).round(), 1.0)
                    .with_layout(move |l| {
                        l.background = Some(c(plate));
                        l.border_radius = Some(spec.radius);
                        l.flex_shrink = Some(0.0);
                        if let Some((width, color)) = border {
                            l.border_width = Some(width);
                            l.border_color = Some(color);
                            l.border_style = Some(nana_ui_core::BorderStyle::Solid);
                        }
                    }),
            )
            .children(widget(Text::new(amount).style(style)))
            .into_any(),
        )
    }
}

/// Rows of one speaker this close together share a header.
const GROUP_MS: u64 = 120_000;
/// Space between the rows of a group.
const GROUP_GAP: f32 = 8.0;

/// Positive spacing between entries: grouping never pulls one bubble into another.
fn row_gap(skin: &Skin, follows: bool) -> f32 {
    let h = &skin.history;
    if h.layout == HistoryLayout::Inline {
        h.gap.clamp(8.0, 14.0)
    } else if follows {
        (h.text.size * 0.45).round().clamp(GROUP_GAP, 12.0)
    } else {
        h.gap.max((h.text.size * 0.65).round()).clamp(12.0, 20.0)
    }
}

/// Whether `row` continues `prev`'s group: same speaker, soon after.
fn continues(prev: &HistoryItem, row: &HistoryItem) -> bool {
    !row.author.is_empty()
        && prev.kind == row.kind
        && prev.author == row.author
        && prev.uid == row.uid
        && row.at >= prev.at
        && row.at - prev.at < GROUP_MS
}

/// The shown rows that continue the row above them.
fn follow_ups(rows: &[HistoryRow], shown: &[u64]) -> Vec<u64> {
    let by_key: std::collections::HashMap<_, _> = rows.iter().map(|r| (r.key, &r.item)).collect();
    let item = |k: u64| by_key.get(&k).copied();
    shown
        .windows(2)
        .filter(|w| matches!((item(w[0]), item(w[1])), (Some(a), Some(b)) if continues(a, b)))
        .map(|w| w[1])
        .collect()
}

/// How a new row arrives, as the skin asked: the transition, and the point
/// its scale or turn is taken around.
fn entrance(h: &nanatype_theme::History, reduced: bool) -> Option<(Transition, TransformOrigin)> {
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
fn glide_ease(h: &nanatype_theme::History) -> Easing {
    match h.entrance_ease {
        HistoryEase::Gentle => ROW_GENTLE,
        HistoryEase::Linear => Easing::Linear,
        HistoryEase::Smooth | HistoryEase::Overshoot => ROW_EASE,
    }
}

/// The rule between two speakers, in the middle of the gap above a row.
fn separator_view(skin: &Skin, gap: f32) -> Option<AnyView> {
    let rule = skin.history.separator.as_ref().filter(|r| r.width > 0.0)?;
    let color = skin.color(rule.color);
    let (width, inset, fade) = (rule.width, rule.inset, rule.fade);
    let clear = Rgba8::rgba(color.r, color.g, color.b, 0);
    let stop = |position: f32, col: Rgba8| GradientStop {
        paint_color: None,
        position,
        color: c(col),
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
                l.paint.background_image = Some(BackgroundImage::Gradient(CssGradient::Linear(
                    LinearGradient {
                        angle_deg: 90.0,
                        stops: vec![
                            stop(0.0, clear),
                            stop(0.18, color),
                            stop(0.82, color),
                            stop(1.0, clear),
                        ],
                    },
                )));
            } else {
                l.background = Some(c(color));
            }
        }))
        .into_any(),
    )
}

fn entry_view(
    ui: Stage,
    key: u64,
    first: bool,
    follows: bool,
    slide: Option<Slide>,
    items: Computed<Vec<u64>>,
) -> AnyView {
    let skin = ui.skin.get_untracked();
    let gap = if first { 0.0 } else { row_gap(&skin, follows) };
    let origin = entrance(&skin.history, false).map(|(_, origin)| origin);
    let rule = (!first && !follows)
        .then(|| separator_view(&skin, gap))
        .flatten();
    let message = message_view(ui, key, follows);
    // Older rows step back as new ones arrive; a search shows what it found
    // at full strength.
    let message = match skin.history.recede {
        Some(recede) => widget(Stack::column(0.0).width(LengthSpec::Fill))
            .bind(move |s: &mut Stack| {
                let rank = items.with(|shown| {
                    shown
                        .iter()
                        .rev()
                        .position(|k| *k == key)
                        .unwrap_or_default()
                });
                let opacity = if query(ui).is_some() {
                    1.0
                } else {
                    recede.opacity(rank)
                };
                s.layout_mut().opacity = Some(opacity);
            })
            .animate([Implicit::new(
                AnimatableProperty::Opacity,
                std::time::Duration::from_millis(RECEDE_MS),
            )
            .ease(ROW_GENTLE)])
            .children(message)
            .into_any(),
        None => message,
    };
    // The gap is padding, so the row's measured height includes it.
    let row = widget(
        Stack::column(0.0)
            .width(LengthSpec::Fill)
            .with_layout(move |l| {
                l.position = PositionSpec::Relative;
                l.padding_top = Some(LengthSpec::Px(gap));
                l.transform_origin = origin;
            }),
    )
    .children((rule, message));
    match slide {
        Some(slide) => {
            slide.built(key, gap);
            row.on(move |e: &SizeChanged| slide.row_laid_out(key, e.height))
                .into_any()
        }
        None => row.into_any(),
    }
}

/// What the log showed after its last layout, to tell how far the rows
/// already on screen moved.
#[derive(Default)]
struct SlideState {
    /// Each row's laid-out height, its gap included.
    heights: HashMap<u64, f32>,
    /// The gap each row was last built with.
    gaps: HashMap<u64, f32>,
    /// The rows, and their gaps, as last laid out.
    order: Vec<u64>,
    laid_gaps: HashMap<u64, f32>,
    /// The list's height, the viewport's and the scroll offset they made.
    content: f32,
    viewport: f32,
    offset: Option<f32>,
}

/// Rows already on screen glide to their new place instead of jumping
/// there. `follow_end` scrolls to a new row at once, and the list's own
/// FLIP sees no scroll; so the list's outer layer jumps back by each move
/// and its inner layer eases the same distance back out. Each layout event
/// adds only its own part of the move, so they may arrive in any order.
#[derive(Clone)]
struct Slide {
    state: Arc<Mutex<SlideState>>,
    /// The sum of all moves so far: the outer layer sits this far down, the
    /// inner layer eases to as far up.
    travel: Signal<f32>,
    items: Computed<Vec<u64>>,
}

fn rise(dy: f32) -> PaintTransform {
    PaintTransform {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        e: 0.0,
        f: dy,
    }
}

impl Slide {
    fn new(items: Computed<Vec<u64>>) -> Self {
        Self {
            state: Arc::default(),
            travel: signal(0.0),
            items,
        }
    }

    fn built(&self, key: u64, gap: f32) {
        self.state.lock().unwrap().gaps.insert(key, gap);
    }

    fn row_laid_out(&self, key: u64, height: f32) {
        self.state.lock().unwrap().heights.insert(key, height);
        self.settle();
    }

    fn content_laid_out(&self, height: f32) {
        self.state.lock().unwrap().content = height;
        self.settle();
    }

    fn viewport_laid_out(&self, height: f32) {
        self.state.lock().unwrap().viewport = height;
        self.settle();
    }

    fn settle(&self) {
        let order = nana_ui::runtime::view::untrack(|| self.items.with(Vec::clone));
        let moved = self.state.lock().unwrap().moved(order);
        if moved.abs() > 0.01 {
            self.travel.update(|t| *t += moved);
        }
    }
}

impl SlideState {
    /// How far the rows still shown moved down on screen since the last
    /// call, which now lays out `order`.
    fn moved(&mut self, order: Vec<u64>) -> f32 {
        if self.content <= 0.0 || self.viewport <= 0.0 {
            return 0.0;
        }
        // `follow_end` keeps the list scrolled to its end.
        let offset = (self.content - self.viewport).max(0.0);
        let Some(was) = self.offset.replace(offset) else {
            self.remember(order);
            return 0.0;
        };
        // The first row still shown rose by the rows dropped above it and by
        // the gap it no longer has.
        let first = order
            .iter()
            .find_map(|key| Some((*key, self.order.iter().position(|k| k == key)?)));
        let moved = first.map(|(key, at)| {
            let dropped: f32 = self.order[..at]
                .iter()
                .map(|k| self.heights.get(k).copied().unwrap_or(0.0))
                .sum();
            let gap = |gaps: &HashMap<u64, f32>| gaps.get(&key).copied().unwrap_or(0.0);
            dropped + gap(&self.laid_gaps) - gap(&self.gaps) + offset - was
        });
        self.remember(order);
        moved.unwrap_or(0.0)
    }

    fn remember(&mut self, order: Vec<u64>) {
        self.heights.retain(|k, _| order.contains(k));
        self.gaps.retain(|k, _| order.contains(k));
        self.laid_gaps.clone_from(&self.gaps);
        self.order = order;
    }
}

/// The log's row `key`, read without tracking.
fn history_row(ui: Stage, key: u64) -> Option<HistoryRow> {
    let at = ui.history_index.with(|index| index.get(&key).copied())?;
    ui.history.with_untracked(|rows| rows.get(at).cloned())
}

#[cfg(test)]
thread_local! {
    /// Rows built on this thread, to tell a rebuild from a kept row.
    static ROWS_BUILT: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn message_view(ui: Stage, key: u64, follows: bool) -> AnyView {
    #[cfg(test)]
    ROWS_BUILT.set(ROWS_BUILT.get() + 1);
    let skin = super::materials::render_skin(ui, ui.skin.get_untracked());
    let settings = ui.settings.get_untracked();
    let h = skin.history.clone();
    let backdrop = h
        .panel
        .as_ref()
        .and_then(|p| super::quote::backdrop(p.color, false));
    let exposed = backdrop.is_none();
    let show_avatar = settings.history.show_avatar && h.avatar.shape != AvatarShape::None;
    let col_w = column_width(ui, &skin, &settings);
    let Some(row) = history_row(ui, key) else {
        return widget(Stack::row(0.0)).into_any();
    };
    let inline = h.layout == HistoryLayout::Inline && col_w >= 160.0;
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
            super::quote::quote_view(ui, &skin, &h.reply, &reply, &h.text, backdrop)
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
        l.font_family = skin.fonts.name.families.first().cloned();
        l.color = Some(c(
            if crate::skin_theme::contrast(h.text.color, fill) >= 4.5 {
                h.text.color
            } else {
                crate::skin_theme::on(fill)
            },
        ));
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
                        l.border_color = Some(col);
                        l.border_style = Some(nana_ui_core::BorderStyle::Solid);
                    }
                    l.overflow_x = OverflowSpec::Hidden;
                    l.overflow_y = OverflowSpec::Hidden;
                    l.background = Some(c(fill));
                    l.paint.background_image = fill_gradient;
                }),
        )
        .children((
            widget(
                Text::new(initial(&row.item.author))
                    .decorative()
                    .style(initial_style),
            ),
            Some(super::avatar::reactive_picture(ui, asset, radius)),
        ))
        .into_any()
    } else {
        widget(Stack::row(0.0)).into_any()
    };

    let mut name_style = NodeStyle::default();
    {
        let l = Arc::make_mut(&mut name_style.layout);
        l.paint.text_material = super::materials::enabled(ui)
            .then(|| super::materials::resolve(&skin, h.text.material.as_deref()))
            .flatten();
        l.font_size = Some(h.name_size);
        l.font_weight = Some(h.name_weight);
        l.font_family = skin.fonts.name.families.first().cloned();
        l.color = Some(c(h.name_color));
        if let Some(p) = &h.name_plate {
            l.color = Some(c(skin.color(p.text_color)));
        } else if let Some(col) = look.as_ref().and_then(|l| l.style.name_color) {
            l.color = Some(c(skin.color(col)));
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
    let name_plate = Surface::history_name_plate(&skin);
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
        l.font_family = skin.fonts.body.families.first().cloned();
        l.color = Some(c(h.time_color));
        if exposed {
            surface::text_stroke(
                l,
                &nanatype_theme::TextStrokeSpec {
                    color: crate::skin_theme::on(h.time_color),
                    width: 1.0,
                },
            );
        }
        l.white_space_nowrap = true;
        super::tnum(l);
    }
    let show_name = settings.history.show_name && !row.item.author.is_empty() && !follows;
    let show_time =
        settings.history.show_time && !follows && (!inline || inline_time_fits(&h, col_w));
    let inline_name_w = inline_name_width(&h, col_w, show_time);
    // Who is talking, once live chat shares the history list.
    let role = (show_name
        && !inline
        && ui.log_canvas == crate::canvas::Canvas::History
        && (chat || (settings.history.chat.enabled && !settings.history.chat.separate)))
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
            l.color = Some(c(color));
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
    let name = show_name.then(|| {
        let plate = name_plate.clone();
        let max_name = if inline { inline_name_w } else { col_w - 64.0 }.max(32.0);
        let l = Arc::make_mut(&mut name_style.layout);
        l.max_width = Some(LengthSpec::Px(
            (max_name - plate_pad[0] * 2.0 - plate_border).max(12.0),
        ));
        // A longer role or time shrinks the plate further: the name
        // shrinks with it, so its ellipsis stays inside rather than clipped.
        l.min_width = Some(LengthSpec::Px(0.0));
        l.flex_shrink = Some(1.0);
        widget(
            Surface::optional_stack(
                plate.as_ref(),
                Stack::row(0.0).padding_xy(plate_pad[0], plate_pad[1]),
            )
            .with_layout(move |l| {
                l.min_width = Some(LengthSpec::Px(0.0));
                l.max_width = Some(LengthSpec::Px(max_name));
                l.overflow_x = OverflowSpec::Hidden;
                l.flex_shrink = Some(1.0);
            }),
        )
        .children(widget(
            Text::new(h.name(&row.item.author))
                .style(name_style)
                .truncating(),
        ))
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
    .materials(super::materials::enabled(ui));
    // How much of what is behind the row its artwork hides.
    let surface_alpha =
        f32::from(look.as_ref().map_or(h.bubble_color, |l| l.fill).a) * bubble_opacity;
    let event_doc = look.as_ref().map(|look| look.dress(&row.doc));
    let body_look = look.clone();
    if follows {
        // Only a group's first bubble points at the avatar.
        bubble_surface.radii[0] = bubble_surface.radii[0].max(h.bubble_radius);
    }
    let body_skin = skin.clone();
    let body = dynamic(
        move || {
            ui.history.with(|_| ());
            history_row(ui, key).map(|r| {
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
            let Some(row) = history_row(ui, key) else {
                return widget(Stack::row(0.0)).into_any();
            };
            let skin = body_skin.clone();
            let entrance = Entrance {
                kind: skin.dialogue.reveal.entrance.kind,
                ms: skin.dialogue.reveal.entrance.ms,
            };
            let mut base = Base::history(&skin, entrance, ui.reduced_motion.get_untracked());
            base.sticker_scale = ui.settings.with_untracked(|s| s.assets.sticker_scale);
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
            let dressed = body_look
                .as_ref()
                .map(|look| Arc::new(look.dress(&row.doc)));
            let doc = dressed.as_ref().unwrap_or(&row.doc);
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
    let bubble_max = if inline {
        let name_w = if show_name { inline_name_w + 10.0 } else { 0.0 };
        let digits = if h.time_seconds { 4.6 } else { 2.9 };
        let time_w = if show_time {
            time_size(&h) * digits + 10.0
        } else {
            0.0
        };
        (col_w - name_w - time_w).max(1.0)
    } else {
        col_w
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
    let row_w = col_w + if show_avatar { size + 10.0 } else { 0.0 };
    if let Some(layout) = boxed {
        let (name_style, time_style) = card_styles.clone().unwrap_or_default();
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
                            Some(LengthSpec::Px((row_w * 0.4).max(32.0)));
                        widget(Text::new(row.item.author.clone()).style(style).truncating())
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
                bubble_view(
                    bubble_surface,
                    bubble_opacity,
                    pad,
                    row_w,
                    true,
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
                    (settings.history.show_time && row_w - 2.0 * pad[0] >= 200.0).then(|| {
                        widget(Text::new(local_time(row.item.at, h.time_seconds)).style(time_style))
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
                bubble_view(
                    bubble_surface,
                    bubble_opacity,
                    pad,
                    row_w,
                    true,
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
    let bubble = bubble_view(bubble_surface, bubble_opacity, pad, bubble_max, false, body);
    // Inline rows put the quote on its own line above the whole row (over
    // the text column), so the name, the first line and the time keep
    // sharing a baseline; stacked rows put it straight above the bubble.
    let (quote, inline_quote) = if inline { (None, quote) } else { (quote, None) };
    let bubble = match quote {
        Some(q) => widget(
            Stack::column(2.0)
                .width(LengthSpec::Shrink)
                .with_layout(move |l| {
                    l.max_width = Some(LengthSpec::Px(bubble_max));
                    l.align_items = if centered {
                        AlignSpec::Center
                    } else {
                        AlignSpec::Start
                    };
                }),
        )
        .children((q, bubble))
        .into_any(),
        None => bubble,
    };

    // Inline skins keep their name/body/time baseline.
    let content = if inline {
        let name_w = inline_name_w;
        let quote_indent = if show_name { name_w + 10.0 } else { 0.0 };
        let quote_w = (col_w - quote_indent).max(40.0);
        let quote_line = inline_quote.map(|q| {
            widget(
                Stack::column(0.0)
                    .width(LengthSpec::Px(quote_w))
                    .with_layout(move |l| {
                        l.margin_left = Some(LengthSpec::Px(quote_indent));
                        l.min_width = Some(LengthSpec::Px(0.0));
                        l.flex_shrink = Some(0.0);
                    }),
            )
            .children(q)
        });
        let line = widget(
            Stack::row(10.0)
                .width(LengthSpec::Px(col_w))
                .with_layout(|l| {
                    l.min_width = Some(LengthSpec::Px(0.0));
                    // Name, first line and time share a baseline.
                    l.align_items = AlignSpec::Baseline;
                }),
        )
        .children((
            name.map(|n| {
                widget(
                    Stack::row(0.0)
                        .width(LengthSpec::Px(name_w))
                        .with_layout(|l| l.flex_shrink = Some(0.0)),
                )
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
        widget(
            Stack::column(2.0)
                .width(LengthSpec::Px(col_w))
                .with_layout(|l| {
                    l.min_width = Some(LengthSpec::Px(0.0));
                    l.align_items = AlignSpec::Start;
                }),
        )
        .children((quote_line, line))
        .into_any()
    } else {
        let header = (name.is_some() || time.is_some()).then(|| {
            widget(
                Stack::row(6.0)
                    .wrap(col_w < 180.0)
                    .align(AlignSpec::Center)
                    .justify(if centered {
                        nana_ui_core::JustifySpec::Center
                    } else {
                        nana_ui_core::JustifySpec::Start
                    })
                    .width(LengthSpec::Fill),
            )
            .children((name, role, time))
            .into_any()
        });
        widget(
            Stack::column(6.0)
                .width(LengthSpec::Px(col_w))
                .with_layout(move |l| {
                    l.min_width = Some(LengthSpec::Px(0.0));
                    l.align_items = if centered {
                        AlignSpec::Center
                    } else {
                        AlignSpec::Start
                    };
                }),
        )
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
        let opacity = row_opacity(ui, at, fade_after);
        let mut taken = std::mem::replace(s, Stack::row(0.0));
        taken = taken.with_layout(|l| l.opacity = Some(opacity));
        *s = taken;
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

/// The visible suffix, excluding the lines still on either dialogue box.
#[derive(Clone, Copy)]
struct Selection {
    visible: usize,
    hidden: [Option<LineId>; super::TRACK_SLOTS],
}

fn selection(ui: Stage) -> Selection {
    let (visible, hide) = ui
        .settings
        .with(|s| (s.history.visible_items as usize, s.history.hide_latest));
    let hide = hide.resolve(ui.skin.with(|s| s.history.hide_latest));
    let mut hidden = [None; super::TRACK_SLOTS];
    if hide && ui.log_canvas == crate::canvas::Canvas::History {
        if ui.peer_active.get() {
            hidden[0] = ui.peer_local_line.get();
            let count = super::remote_bands(ui.peer_seats.get());
            for (slot, lane) in ui.peer_lanes.iter().take(count).enumerate() {
                hidden[slot + 1] = lane.display.with(|d| d.line);
            }
        } else {
            hidden[0] = ui.display.with(|d| d.line);
        }
    }
    Selection { visible, hidden }
}

/// Newest first: stop after the visible suffix instead of scanning the log.
fn selected_rows(rows: &[HistoryRow], selection: Selection) -> impl Iterator<Item = &HistoryRow> {
    rows.iter()
        .rev()
        .filter(move |row| row.item.line.is_none() || !selection.hidden.contains(&row.item.line))
        .take(selection.visible)
}

/// Whether `canvas` offers its search: while it takes clicks, and not while it
/// is sent to OBS over Spout/Syphon, whose picture is that same window's. So
/// the broadcast never shows a filtered log or the field.
pub fn search_allowed_for(
    canvas: crate::canvas::Canvas,
    s: &nanatype_settings::Settings,
) -> bool {
    match canvas {
        crate::canvas::Canvas::Danmaku => {
            s.overlays.danmaku.lock != nanatype_settings::Lock::Locked && !s.output.danmaku.enabled
        }
        _ => {
            s.overlays.history.lock != nanatype_settings::Lock::Locked && !s.output.history.enabled
        }
    }
}

/// Whether the history window offers its search: while it takes clicks, and
/// not while it is sent to OBS over Spout/Syphon, whose picture is this same
/// window's. So the broadcast never shows a filtered log or the field.
pub fn search_allowed(s: &nanatype_settings::Settings) -> bool {
    search_allowed_for(crate::canvas::Canvas::History, s)
}

/// The search that filters the window now, lowercased; `None` shows all.
fn query(ui: Stage) -> Option<String> {
    let q = ui
        .history_search
        .with(|q| q.as_deref().map(|q| q.trim().to_lowercase()))?;
    (!q.is_empty()
        && ui
            .settings
            .with(|s| search_allowed_for(ui.log_canvas, s)))
    .then_some(q)
}

/// A row whose text or speaker contains `q` (already lowercased).
pub fn row_matches(row: &HistoryRow, q: &str) -> bool {
    row.item.author.to_lowercase().contains(q) || row.doc.plain_text().to_lowercase().contains(q)
}

fn visible_keys(ui: Stage) -> Vec<u64> {
    let selection = selection(ui);
    // A search looks through the whole kept log, not just the shown rows.
    if let Some(q) = query(ui) {
        return ui.history.with(|rows| {
            rows.iter()
                .filter(|row| row.item.line.is_none() || !selection.hidden.contains(&row.item.line))
                .filter(|row| row_matches(row, &q))
                .map(|row| row.key)
                .collect()
        });
    }
    ui.history.with(|rows| {
        let mut keys: Vec<_> = selected_rows(rows, selection).map(|r| r.key).collect();
        keys.reverse();
        keys
    })
}

#[derive(Default, Debug, PartialEq)]
pub struct FadeSchedule {
    pub animating: bool,
    pub next_at: Option<u64>,
}

/// Only mounted history rows can need the history clock. Read untracked from
/// host callbacks, but share exactly the list's selection and fade interval.
pub fn fade_schedule(ui: Stage, now: u64) -> FadeSchedule {
    nana_ui::runtime::view::untrack(|| {
        let Some(after) = ui.settings.with(|s| s.history.fade_after_s) else {
            return FadeSchedule::default();
        };
        let selection = selection(ui);
        let unix_now = crate::clock::unix_of(now);
        let delay = u64::from(after) * 1000;
        ui.history.with(|rows| {
            let mut schedule = FadeSchedule::default();
            for row in selected_rows(rows, selection) {
                let start = row.item.at.saturating_add(delay);
                schedule.animating |= unix_now >= start.saturating_sub(FADE_FRAME_SLACK_MS)
                    && unix_now <= start.saturating_add(ROW_FADE_MS + FADE_FRAME_SLACK_MS);
                if start > unix_now {
                    let at = now.saturating_add(start - unix_now);
                    schedule.next_at = Some(schedule.next_at.map_or(at, |old| old.min(at)));
                }
            }
            schedule
        })
    })
}

/// The settings the history window reads while it builds its rows; the
/// rest (sound, hotkeys, integrations, the dialogue's own window...) never
/// changes what the log shows.
#[derive(Clone, PartialEq)]
struct LogSettings {
    history: nanatype_settings::HistorySettings,
    lock: nanatype_settings::Lock,
    size: Option<[f32; 2]>,
    entrance: Option<nanatype_model::EntranceKind>,
    typing: bool,
    advanced_effects: bool,
    motion: bool,
    skin: String,
    overrides: nanatype_settings::SkinOverrides,
    personalization: Option<nanatype_settings::SkinPersonalization>,
}

impl LogSettings {
    fn of(canvas: crate::canvas::Canvas, s: &Arc<nanatype_settings::Settings>) -> Self {
        let overlay = canvas.overlay(s);
        Self {
            history: s.history.clone(),
            lock: overlay.lock,
            size: overlay.size,
            entrance: s.playback.entrance,
            typing: s.playback.typing,
            advanced_effects: s.skin.advanced_effects,
            motion: s.skin.motion,
            skin: s.skin.id.clone(),
            overrides: s.skin.overrides.clone(),
            personalization: s.skin.personalizations.get(&s.skin.id).cloned(),
        }
    }
}

pub fn history_window(ui: Stage) -> impl IntoView {
    history_window_with(ui, false, || widget(Stack::column(0.)).into_any())
}

/// [`history_window`] with `overlay` (the product's plugin views) above the
/// log, and with `search` a search button in its corner.
pub fn history_window_with(
    ui: Stage,
    search: bool,
    overlay: impl Fn() -> AnyView + Send + Sync + 'static,
) -> impl IntoView {
    let overlay = std::sync::Arc::new(overlay);
    let items = computed(move || visible_keys(ui));
    let follows = computed(move || {
        if ui.skin.with(|s| !s.history.group_speakers) {
            return Vec::new();
        }
        let shown = items.get();
        ui.history.with(|rows| follow_ups(rows, &shown))
    });
    // Rebuilding mounts every row anew, so only what the log is drawn from
    // rebuilds it: not every edit, which replaces the whole settings value.
    let key = move || {
        let skin = ui.skin.get();
        (
            Arc::as_ptr(&skin) as usize,
            ui.settings.with(|s| LogSettings::of(ui.log_canvas, s)),
            ui.locale.get(),
            ui.reduced_motion.get(),
            ui.history_width.get(),
            ui.history_height.get(),
        )
    };
    dynamic(key, move |_| {
        let skin = super::materials::render_skin(ui, ui.skin.get_untracked());
        let reduced = ui.reduced_motion.get_untracked();
        let ms = std::time::Duration::from_millis(u64::from(skin.history.entrance_ms.min(2000)));
        let glide = glide_ease(&skin.history);
        let transition = entrance(&skin.history, reduced).map(|(transition, _)| transition);
        let slide = transition.is_some().then(|| Slide::new(items));
        let row_slide = slide.clone();
        let list = each(
            items,
            |k: &u64| *k,
            move |k: u64| {
                let slide = row_slide.clone();
                dynamic(
                    move || {
                        (
                            items.with(|i| i.first() == Some(&k)),
                            follows.with(|f| f.contains(&k)),
                        )
                    },
                    move |(first, f)| entry_view(ui, k, *first, *f, slide.clone(), items),
                )
            },
        );
        let list = match transition {
            Some(t) => list.transition(t),
            None => list,
        };
        let list = match &slide {
            Some(slide) => {
                let travel = slide.travel;
                widget(Stack::column(0.0).width(LengthSpec::Fill))
                    .bind(move |s: &mut Stack| {
                        s.layout_mut().transform = Some(rise(-travel.get()));
                    })
                    .animate([Implicit::new(AnimatableProperty::Transform, ms).ease(glide)])
                    .children(list)
                    .into_any()
            }
            None => list.into_any(),
        };
        let h = &skin.history;
        let height = viewport_height(ui, &skin, &ui.settings.get_untracked());
        let compact = height < 180.0;
        let [mut mt, mr, mut mb, ml] = h.panel.as_ref().map_or(h.margin, |p| p.margin);
        let [pl, mut pt, pr, mut pb] = h.panel.as_ref().map_or(h.padding, |p| p.padding);
        if compact {
            mt = mt.min(4.0);
            mb = mb.min(4.0);
            pt = pt.min(4.0);
            pb = pb.min(4.0);
        }
        let panel_surface = Surface::history_panel(&skin);
        let unlocked = ui
            .settings
            .with_untracked(|x| ui.log_canvas.overlay(x).lock != nanatype_settings::Lock::Locked);
        let decor = |layer: DecorLayer| -> Vec<AnyView> {
            h.decor
                .iter()
                .filter(|d| d.layer == layer)
                .map(|d| {
                    let image = d.image.as_ref().and_then(|path| {
                        ui.asset_urls
                            .with_untracked(|urls| urls.get(&format!("skin:{path}")).cloned())
                    });
                    let mut decor = d.clone();
                    if compact {
                        let extent = decor
                            .size
                            .iter()
                            .filter_map(|v| match v {
                                nanatype_theme::Len::Px(v) => Some(*v),
                                _ => None,
                            })
                            .fold(1.0_f32, f32::max);
                        let scale = (height * 0.25 / extent).min(1.0);
                        for v in &mut decor.size {
                            if let nanatype_theme::Len::Px(v) = v {
                                *v *= scale;
                            }
                        }
                        decor.offset = decor.offset.map(|v| v * scale);
                        decor.text_size *= scale;
                    }
                    dialogue::decor(&skin, &decor, None::<fn() -> bool>, image)
                })
                .collect()
        };
        let (below, above) = (decor(DecorLayer::Below), decor(DecorLayer::Above));
        let title = h
            .title
            .as_ref()
            .filter(|_| !compact)
            .map(|t| title_view(&skin, t));
        widget(Stack::fill_column(0.0).with_layout(move |l| {
            l.padding_top = Some(LengthSpec::Px(mt));
            l.padding_right = Some(LengthSpec::Px(mr));
            l.padding_bottom = Some(LengthSpec::Px(mb));
            l.padding_left = Some(LengthSpec::Px(ml));
        }))
        .on(move |e: &SizeChanged| {
            ui.history_width.set_if_changed(e.width);
            ui.history_height.set_if_changed(e.height);
        })
        .children((
            // An empty log is invisible without a panel: outline the window
            // while it can be dragged, as the display window does.
            unlocked.then(|| {
                let (id, key) = if ui.log_canvas == crate::canvas::Canvas::Danmaku {
                    ("danmaku-empty-placeholder", "danmaku-placeholder")
                } else {
                    ("history-empty-placeholder", "history-placeholder")
                };
                dialogue::empty_placeholder(ui, id, key, move || log_presence(ui))
            }),
            widget(
                Surface::optional_stack(panel_surface.as_ref(), Stack::fill_column(0.0))
                    .with_layout(move |l| {
                        l.position = PositionSpec::Relative;
                        // Window move/scale gestures take priority over scrolling
                        // the list, including in the default interactive mode.
                        if unlocked {
                            l.pointer_events = Some(PointerEventsSpec::None);
                        }
                    }),
            )
            .children((
                below,
                title,
                // Takes the panel's height left under the title (and no
                // more), so the list scrolls inside it instead of running
                // past the window's foot.
                {
                    let scroll = widget(
                        ScrollView::new(ScrollAxes::Vertical)
                            .follow_end(true)
                            .with_layout(|l| {
                                l.flex_grow = Some(1.0);
                                l.flex_shrink = Some(1.0);
                                l.min_height = Some(LengthSpec::Px(0.0));
                            }),
                    );
                    let content = widget(Stack::column(0.0).with_layout(move |l| {
                        l.padding_left = Some(LengthSpec::Px(pl));
                        l.padding_top = Some(LengthSpec::Px(pt));
                        l.padding_right = Some(LengthSpec::Px(pr));
                        l.padding_bottom = Some(LengthSpec::Px(pb));
                        // A short log always starts at its content area's top;
                        // follow_end still scrolls overflowing logs to the newest row.
                        l.min_height = Some(LengthSpec::Percent(100.0));
                        l.justify_content = nana_ui_core::JustifySpec::Start;
                    }));
                    match slide {
                        Some(slide) => {
                            let (viewport, travel) = (slide.clone(), slide.travel);
                            scroll
                                .on(move |e: &SizeChanged| viewport.viewport_laid_out(e.height))
                                .children(
                                    content
                                        .bind(move |s: &mut Stack| {
                                            s.layout_mut().transform = Some(rise(travel.get()));
                                        })
                                        .on(move |e: &SizeChanged| slide.content_laid_out(e.height))
                                        .children(list),
                                )
                                .into_any()
                        }
                        None => scroll.children(content.children(list)).into_any(),
                    }
                },
                above,
                overlay(),
            )),
            search.then(|| search_bar(ui)),
        ))
    })
}

/// A quiet search button in the window's top corner that unfolds into a
/// field. The button again (or Esc, through the host) folds it and clears it.
fn search_bar(ui: Stage) -> AnyView {
    let open = move || ui.history_search.with(Option::is_some);
    let label = ui.now_t("history-search");
    let close = ui.now_t("history-search-close");
    let cluster = widget(Stack::row(0.0).gap(4.0).align(AlignSpec::Center));
    // Across the window's top, at its end; only the field and the button
    // take clicks, so the rest still drags the window.
    let layer = widget(
        Stack::column(0.0)
            .padding(6.0)
            .align(AlignSpec::End)
            .with_layout(|l| {
                l.position = PositionSpec::Absolute;
                l.offset_left = Some(LengthSpec::Px(0.0));
                l.offset_top = Some(LengthSpec::Px(0.0));
                l.offset_right = Some(LengthSpec::Px(0.0));
                l.pointer_events = Some(PointerEventsSpec::None);
            }),
    );
    layer
        .visible(move || {
            ui.settings
                .with(|s| search_allowed_for(ui.log_canvas, s))
        })
        .children(
            cluster.children((
                widget(Stack::row(0.0).width(LengthSpec::Px(160.0)))
                    .visible(open)
                    .children(
                        widget(
                            TextInput::new("")
                                .placeholder(ui.now_t("history-search-placeholder"))
                                .label(label.clone())
                                .size(ControlSize::Small),
                        )
                        .node_ref(ui.history_search_field)
                        // Folding clears it, so it opens empty.
                        .bind(move |t: &mut TextInput| {
                            if !open() && !t.state.value.is_empty() {
                                t.state = TextInputState::new(String::new());
                            }
                        })
                        .on(move |e: &TextChanged| {
                            ui.history_search.set(Some(e.value.to_string()));
                        }),
                    ),
                widget(
                    IconButton::new(nana_ui::icons_tabler::SEARCH, label.clone())
                        .size(ControlSize::Small)
                        .kind(ButtonKind::Ghost)
                        .with_tooltip(label.clone()),
                )
                .bind(move |b: &mut IconButton| {
                    let shown = open();
                    b.icon = if shown {
                        nana_ui::icons_tabler::X
                    } else {
                        nana_ui::icons_tabler::SEARCH
                    };
                    b.label = if shown { close.clone() } else { label.clone() }.into();
                })
                .on(move |_: &Activate| {
                    ui.history_search
                        .set(if open() { None } else { Some(String::new()) });
                }),
            )),
        )
        .into_any()
}

/// Put the caret in the search field as it unfolds; `open` is whether the
/// last look saw it open. Folding needs nothing: the field hides and clears.
pub fn focus_search(ui: Stage, doc: &mut nana_ui::runtime::RuntimeDocument, open: &mut bool) {
    if !ui.history_search.with_untracked(Option::is_some) {
        *open = false;
        return;
    }
    if *open {
        return;
    }
    let document = doc.document();
    let cx = doc.context_mut();
    let _ = cx.flush_reactive();
    // Not mounted yet: try again on the next look.
    let Some(field) = ui.history_search_field.get_untracked() else {
        return;
    };
    let _ = cx.focus_node(document, field);
    *open = cx.world().focused(document) == Some(field);
}

/// How visible the log is: the newest shown row's opacity, which `fade_after_s`
/// fades like every row; 0 when no row is shown.
fn log_presence(ui: Stage) -> f32 {
    let selection = selection(ui);
    let fade_after = ui.settings.with(|s| s.history.fade_after_s);
    ui.history
        .with(|rows| selected_rows(rows, selection).next().map(|r| r.item.at))
        .map_or(0.0, |at| row_opacity(ui, at, fade_after))
}

/// A row said at `at` (unix ms), faded out `fade_after_s` seconds later.
fn row_opacity(ui: Stage, at: u64, fade_after_s: Option<u32>) -> f32 {
    // Found rows show however old they are.
    let Some(after) = fade_after_s.filter(|_| query(ui).is_none()) else {
        return 1.0;
    };
    let age = crate::clock::unix_of(ui.history_now.get()).saturating_sub(at);
    let start = u64::from(after) * 1000;
    1.0 - (age.saturating_sub(start) as f32 / ROW_FADE_MS as f32).clamp(0.0, 1.0)
}

/// The header bar across the top of the panel.
fn title_view(skin: &Skin, t: &HistoryTitle) -> AnyView {
    let mut st = NodeStyle::default();
    {
        let l = Arc::make_mut(&mut st.layout);
        l.font_size = Some(t.size);
        l.font_weight = Some(t.weight);
        l.font_family = skin.fonts.name.families.first().cloned();
        l.color = Some(c(t.color));
        l.white_space_nowrap = true;
    }
    let background = c(t.background);
    widget(
        Stack::row(0.0)
            .width(LengthSpec::Fill)
            .height(LengthSpec::Px(t.height))
            .padding_xy(t.padding, 0.0)
            .align(AlignSpec::Center)
            .with_layout(move |l| {
                l.background = Some(background);
                l.flex_shrink = Some(0.0);
            }),
    )
    .children(widget(Text::new(t.text.clone()).style(st)))
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An inline log narrower than its three columns drops the time before
    /// the name is cut to "…"; with room it keeps both.
    #[test]
    fn a_narrow_inline_log_gives_the_time_up_for_the_name() {
        let mut h = nanatype_theme::History {
            name_size: 16.0,
            name_width: 120.0,
            time_seconds: true,
            ..nanatype_theme::History::default()
        };
        h.text.size = 20.0;
        let three = h.name_size * 3.0;
        assert!(!inline_time_fits(&h, 230.0));
        assert!(inline_name_width(&h, 230.0, false) >= three);
        assert!(inline_time_fits(&h, 400.0));
        assert!(inline_name_width(&h, 400.0, true) >= three);
    }

    #[test]
    fn search_filters_the_window_but_never_the_broadcast_output() {
        let settings = nanatype_settings::Settings::default();
        let ui = Stage::new(
            Arc::new(settings.clone()),
            Arc::new(Skin::default()),
            nanatype_i18n::Locale::EnUs,
        );
        let row = |key, author: &str, text: &str| HistoryRow {
            key,
            item: HistoryItem {
                source: nanatype_model::SourceId::local(),
                kind: SourceKind::Typewriter,
                line: None,
                author: author.into(),
                avatar: None,
                doc: Document::plain(text),
                at: 0,
                uid: None,
                reply: None,
                event: None,
            },
            doc: Arc::new(Document::plain(text)),
            reveal: None,
        };
        let rows: Vec<HistoryRow> = (0..40)
            .map(|i| row(10 + i, "Viewer", "filler"))
            .chain([
                row(1, "Nana", "Hello there"),
                row(2, "Bob", "good night"),
                row(3, "nana", "bye"),
            ])
            .collect();
        ui.history.set(Arc::new(rows));
        let shown = settings.history.visible_items as usize;
        assert_eq!(visible_keys(ui).len(), shown);
        // Found rows come from the whole kept log, by text or name, in any case.
        ui.history_search.set(Some(" NANA ".into()));
        assert_eq!(visible_keys(ui), [1, 3]);
        ui.history_search.set(Some("NIGHT".into()));
        assert_eq!(visible_keys(ui), [2]);
        ui.history_search.set(Some(String::new()));
        assert_eq!(visible_keys(ui).len(), shown);
        // The window is what Spout/Syphon sends: while it does, nothing is
        // filtered, and a locked window cannot be searched either.
        ui.history_search.set(Some("night".into()));
        for edit in [
            (|s: &mut nanatype_settings::Settings| s.output.history.enabled = true)
                as fn(&mut nanatype_settings::Settings),
            |s| s.overlays.history.lock = nanatype_settings::Lock::Locked,
        ] {
            let mut s = settings.clone();
            edit(&mut s);
            assert!(!search_allowed(&s));
            ui.settings.set(Arc::new(s));
            assert_eq!(visible_keys(ui).len(), shown);
        }
        ui.settings.set(Arc::new(settings));
        assert_eq!(visible_keys(ui), [2]);
    }

    /// The search button unfolds a field that takes the caret at once, so
    /// typing filters straight away; folding and unfolding again does too.
    #[test]
    fn unfolding_the_search_focuses_its_field() {
        use nana_ui::runtime::{DocumentId, RuntimeDocument};
        use nana_ui_devtools::agent::{AgentSession, RuntimeAgentSession};
        let ui = Stage::new(
            Arc::new(nanatype_settings::Settings::default()),
            Arc::new(Skin::default()),
            nanatype_i18n::Locale::EnUs,
        );
        let id = DocumentId::new(1).unwrap();
        let mut doc = RuntimeDocument::new(id);
        doc.context_mut()
            .mount_view_root(id, move || history_window_with(ui, true, || ().into_any()))
            .unwrap();
        let mut session = RuntimeAgentSession::new(doc, 420, 560).unwrap();
        session.flush().unwrap();
        let mut open = false;
        let label = ui.now_t("history-search");
        for round in 0..2 {
            let button = session
                .accessibility_dump()
                .into_iter()
                .find(|n| n.role == "button" && n.label.as_deref() == Some(label.as_str()))
                .expect("search button");
            session.activate(button.id).unwrap();
            session.flush().unwrap();
            focus_search(ui, session.document_mut(), &mut open);
            assert!(open, "round {round}");
            let document = session.document().document();
            let focused = session.document().context().world().focused(document);
            assert_eq!(
                focused,
                ui.history_search_field.get_untracked(),
                "round {round}"
            );
            session.type_text("nana").unwrap();
            session.flush().unwrap();
            assert_eq!(ui.history_search.get_untracked().as_deref(), Some("nana"));
            // Folded (the button again, or Esc through the host): the next
            // unfolding focuses afresh.
            ui.history_search.set(None);
            session.flush().unwrap();
            focus_search(ui, session.document_mut(), &mut open);
            assert!(!open);
        }
    }

    #[test]
    fn translucent_bubble_artwork_keeps_its_content_opaque() {
        use nana_ui::runtime::{DocumentId, RuntimeDocument};
        use nana_ui_devtools::agent::{AgentSession, RuntimeAgentSession};
        use nanatype_theme::{ColorRef, ContourPreset, GradientSpec};

        for contour in [None, Some(ContourPreset::Rounded)] {
            for opacity in [0.0, 0.35, 0.7, 1.0] {
                let mut skin = Skin::default();
                skin.history.bubble_color = Rgba8::rgb(160, 40, 60);
                skin.history.bubble_border = Some(Rgba8::WHITE);
                skin.history.bubble_border_width = 2.0;
                skin.history.bubble_contour = contour;
                skin.history.bubble_gradient = Some(GradientSpec {
                    stops: vec![
                        (0.0, ColorRef::from(Rgba8::rgb(200, 80, 100))),
                        (1.0, ColorRef::from(Rgba8::rgb(100, 20, 40))),
                    ],
                    ..Default::default()
                });
                let surface = Surface::bubble(&skin, None);
                let id = DocumentId::new(1).unwrap();
                let mut doc = RuntimeDocument::new(id);
                doc.context_mut()
                    .mount_view_root(id, move || {
                        let content = widget(
                            Stack::row(0.0)
                                .width(LengthSpec::Px(80.0))
                                .height(LengthSpec::Px(12.0))
                                .with_layout(|l| l.background = Some(c(Rgba8::WHITE))),
                        )
                        .into_any();
                        widget(Stack::column(0.0).padding(12.0)).children(bubble_view(
                            surface,
                            opacity,
                            [12.0, 8.0],
                            160.0,
                            false,
                            content,
                        ))
                    })
                    .unwrap();
                let mut session = RuntimeAgentSession::new(doc, 200, 80).unwrap();
                session.set_clear(Some([0.0; 4]));
                let (size, pixels) = session.screenshot_rgba().unwrap();
                let alpha = |x, y| pixels[(y * size.width as usize + x) * 4 + 3];
                let expected = (opacity * 255.0).round() as i16;
                assert!(
                    (i16::from(alpha(20, 28)) - expected).abs() <= 2,
                    "{contour:?} {opacity}: gradient alpha {}, expected {expected}",
                    alpha(20, 28)
                );
                assert!(
                    (i16::from(alpha(13, 28)) - expected).abs() <= 2,
                    "{contour:?} {opacity}: border alpha {}, expected {expected}",
                    alpha(13, 28)
                );
                assert_eq!(
                    alpha(50, 28),
                    255,
                    "{contour:?} {opacity}: content must stay opaque"
                );
            }
        }
    }

    #[test]
    fn the_line_on_screen_waits_until_it_leaves() {
        let rows: Vec<_> = [(1, Some(LineId(10))), (2, None), (3, Some(LineId(11)))]
            .into_iter()
            .map(|(key, line)| {
                let mut row = row(key, 0);
                row.item.line = line;
                row
            })
            .collect();
        let keys = |visible, hidden| {
            let mut keys: Vec<_> = selected_rows(&rows, Selection { visible, hidden })
                .map(|r| r.key)
                .collect();
            keys.reverse();
            keys
        };
        let hide = |first, second| {
            let mut hidden = [None; crate::ui::TRACK_SLOTS];
            hidden[0] = first;
            hidden[1] = second;
            hidden
        };
        assert_eq!(keys(30, hide(None, None)), [1, 2, 3]);
        // Only the line still on the box is left out; external rows stay.
        assert_eq!(keys(30, hide(Some(LineId(11)), None)), [1, 2]);
        // The cap counts the rows that show.
        assert_eq!(keys(1, hide(Some(LineId(11)), None)), [2]);
        assert_eq!(keys(30, hide(Some(LineId(10)), Some(LineId(11)))), [2]);
        assert!(keys(0, hide(None, None)).is_empty());
    }

    fn row(key: u64, at: u64) -> HistoryRow {
        let mut item = item("Nana", SourceKind::Typewriter, 0);
        item.at = at;
        HistoryRow {
            key,
            item,
            doc: Arc::new(Document::new()),
            reveal: None,
        }
    }

    #[test]
    fn fade_schedule_uses_the_same_visible_rows_as_the_list() {
        use nanatype_settings::{HideLatest, Settings};
        let mut settings = Settings::default();
        settings.history.visible_items = 1;
        settings.history.fade_after_s = Some(2);
        settings.history.hide_latest = HideLatest::Never;
        let mut ui = Stage::new(
            Arc::new(settings),
            Arc::new(Skin::default()),
            nanatype_i18n::Locale::ZhCn,
        );
        let peer = nana_ui::runtime::view::signal(false);
        ui.peer_active = nana_ui::runtime::view::computed(move || peer.get());
        let base = crate::clock::unix_of(0);
        ui.history
            .set(Arc::new(vec![row(1, base), row(2, base + 3000)]));
        assert_eq!(visible_keys(ui), [2]);
        // Row 1 is fading, but it isn't mounted. Only row 2 schedules work.
        assert_eq!(
            fade_schedule(ui, 2000),
            FadeSchedule {
                animating: false,
                next_at: Some(5000)
            }
        );
        assert!(fade_schedule(ui, 5000).animating);
        assert_eq!(fade_schedule(ui, 5701), FadeSchedule::default());
        ui.settings
            .update(|s| Arc::make_mut(s).history.visible_items = 2);
        assert_eq!(visible_keys(ui), [1, 2]);
        assert!(fade_schedule(ui, 2000).animating);
        ui.settings
            .update(|s| Arc::make_mut(s).history.visible_items = 0);
        assert_eq!(fade_schedule(ui, 2000), FadeSchedule::default());
        ui.settings.update(|s| {
            let s = Arc::make_mut(s);
            s.history.visible_items = 30;
            s.history.hide_latest = HideLatest::Always;
        });
        ui.history.update(|rows| {
            let rows = Arc::make_mut(rows);
            rows[0].item.line = Some(LineId(10));
            rows[1].item.line = Some(LineId(11));
        });
        ui.display
            .update(|d| Arc::make_mut(d).line = Some(LineId(10)));
        assert_eq!(visible_keys(ui), [2]);
        assert!(!fade_schedule(ui, 2000).animating);
        peer.set(true);
        ui.peer_local_line.set(Some(LineId(10)));
        ui.peer_display
            .update(|d| Arc::make_mut(d).line = Some(LineId(11)));
        assert!(visible_keys(ui).is_empty());
        assert_eq!(fade_schedule(ui, 2000), FadeSchedule::default());
        peer.set(false);
        assert_eq!(visible_keys(ui), [2]);
    }

    fn item(author: &str, kind: SourceKind, at_s: u64) -> HistoryItem {
        HistoryItem {
            source: nanatype_model::SourceId::local(),
            kind,
            line: None,
            author: author.into(),
            avatar: None,
            doc: nanatype_model::Document::new(),
            at: at_s * 1000,
            uid: None,
            reply: None,
            event: None,
        }
    }

    #[test]
    fn a_long_name_ends_before_its_role_in_every_locale() {
        use nana_ui::runtime::{DocumentId, RuntimeDocument};
        use nana_ui_devtools::agent::RuntimeAgentSession;
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        let name = "这是一位名字特别长的观众 A very long viewer name";
        for locale in nanatype_i18n::Locale::ALL {
            for skin_id in ["nanatype.pixel", "nanatype.vanilla"] {
                let skin = registry.load(skin_id).unwrap().skin;
                // As the skin writes it: pixel adds a colon.
                let written = skin.history.name(name);
                let (w, h) = (skin.history.width as u32, skin.history.height as u32);
                let mut settings = nanatype_settings::Settings::default();
                settings.history.show_time = true;
                let ui = super::super::Stage::new(Arc::new(settings), skin, locale);
                ui.history.set(Arc::new(vec![HistoryRow {
                    key: 1,
                    item: item(name, SourceKind::Chat, 100),
                    doc: Arc::new(Document::plain("求个自我介绍～")),
                    reveal: None,
                }]));
                let id = DocumentId::new(1).unwrap();
                let mut doc = RuntimeDocument::new(id);
                doc.context_mut()
                    .mount_view_root(id, move || history_window(ui))
                    .unwrap();
                let mut session = RuntimeAgentSession::new(doc, w, h).unwrap();
                session.flush().unwrap();
                let nodes = session.accessibility_dump();
                let find = |label: &str| {
                    nodes
                        .iter()
                        .find(|n| n.label.as_deref() == Some(label))
                        .unwrap_or_else(|| panic!("{locale:?} {skin_id}: no {label}"))
                        .bounds
                        .clone()
                };
                let shown = find(&written);
                let role = find(&ui.now_t("history-role-viewer"));
                assert!(
                    shown.x + shown.width <= role.x,
                    "{locale:?} {skin_id}: the name {shown:?} runs under its role {role:?}"
                );
            }
        }
    }

    /// A log of opaque red bubbles, `w`×`h`, with rows `1..=rows`.
    fn motion_log(
        w: u32,
        h: u32,
        rows: u64,
        visible: u32,
    ) -> (Stage, nana_ui_devtools::agent::RuntimeAgentSession) {
        motion_log_with(w, h, rows, visible, |_| {})
    }

    /// [`motion_log`] with the skin's history changed by `dress`.
    fn motion_log_with(
        w: u32,
        h: u32,
        rows: u64,
        visible: u32,
        dress: impl FnOnce(&mut nanatype_theme::History),
    ) -> (Stage, nana_ui_devtools::agent::RuntimeAgentSession) {
        use nana_ui::runtime::{DocumentId, RuntimeDocument};
        use nana_ui_devtools::agent::{AgentSession, RuntimeAgentSession};
        let mut skin = Skin::default();
        skin.history.entrance = HistoryEntrance::SlideUp;
        skin.history.bubble_color = Rgba8::rgb(220, 30, 30);
        skin.history.group_speakers = false;
        dress(&mut skin.history);
        let mut settings = nanatype_settings::Settings::default();
        settings.history.show_avatar = false;
        settings.history.show_name = false;
        settings.history.show_time = false;
        settings.history.fade_after_s = None;
        settings.history.visible_items = visible;
        settings.history.hide_latest = nanatype_settings::HideLatest::Never;
        let ui = Stage::new(
            Arc::new(settings),
            Arc::new(skin),
            nanatype_i18n::Locale::ZhCn,
        );
        ui.history.set(Arc::new((1..=rows).map(text_row).collect()));
        let id = DocumentId::new(1).unwrap();
        let mut doc = RuntimeDocument::new(id);
        crate::clock::capture_at(0, || {
            doc.context_mut()
                .mount_view_root(id, move || history_window(ui))
                .unwrap()
        });
        let mut session = RuntimeAgentSession::new(doc, w, h).unwrap();
        session.set_clear(Some([0.0; 4]));
        crate::clock::capture_at(0, || session.flush().unwrap());
        (ui, session)
    }

    fn text_row(key: u64) -> HistoryRow {
        let mut item = item("Nana", SourceKind::Typewriter, key);
        item.doc = Document::plain("第一行文字\n第二行文字");
        HistoryRow {
            key,
            doc: Arc::new(item.doc.clone()),
            item,
            reveal: None,
        }
    }

    /// The bubbles crossing the column at x = 30, top to bottom: their
    /// vertical extent and whether they are fully opaque.
    fn bubbles(
        session: &mut nana_ui_devtools::agent::RuntimeAgentSession,
        at: u64,
    ) -> Vec<(usize, usize, bool)> {
        let t = std::time::Duration::from_millis(at);
        session.document_mut().context_mut().advance_animations(t);
        session.document_mut().sync_presentation_clock(t);
        let (size, pixels) = crate::clock::capture_at(at, || session.screenshot_rgba().unwrap());
        let alpha = |y: usize| pixels[(y * size.width as usize + 30) * 4 + 3];
        let mut runs = Vec::new();
        let mut start = None;
        for y in 0..=size.height as usize {
            let inked = y < size.height as usize && alpha(y) > 8;
            match (inked, start) {
                (true, None) => start = Some(y),
                (false, Some(s)) => {
                    runs.push((s, y, alpha((s + y) / 2) == 255));
                    start = None;
                }
                _ => {}
            }
        }
        runs
    }

    /// Append the next row at `at` ms, as a program message would.
    fn push_row(
        ui: Stage,
        session: &mut nana_ui_devtools::agent::RuntimeAgentSession,
        key: u64,
        at: u64,
    ) {
        let t = std::time::Duration::from_millis(at);
        session.document_mut().context_mut().advance_animations(t);
        ui.history
            .update(|rows| Arc::make_mut(rows).push(text_row(key)));
        crate::clock::capture_at(at, || session.flush().unwrap());
    }

    #[test]
    fn a_new_row_rises_and_fades_in() {
        let (ui, mut session) = motion_log(300, 480, 1, 30);
        let before = bubbles(&mut session, 5_000);
        assert_eq!(before.len(), 1, "{before:?}");
        push_row(ui, &mut session, 2, 5_000);
        let mid = bubbles(&mut session, 5_060);
        let done = bubbles(&mut session, 5_600);
        assert_eq!(done.len(), 2, "{done:?}");
        assert!(done[1].2, "the new row settles opaque: {done:?}");
        let entering = mid.last().unwrap();
        assert!(!entering.2, "the new row is still fading in: {mid:?}");
        assert!(
            entering.0 > done[1].0,
            "the new row rises into place: {mid:?} -> {done:?}"
        );
    }

    #[test]
    fn every_entrance_settles_where_a_plain_row_sits() {
        use HistoryEntrance::*;
        let rest = |entrance, ease| {
            let (ui, mut session) = motion_log_with(300, 480, 1, 30, |h| {
                h.entrance = entrance;
                h.entrance_ease = ease;
                h.entrance_ms = 400;
            });
            bubbles(&mut session, 5_000);
            push_row(ui, &mut session, 2, 5_000);
            let mid = bubbles(&mut session, 5_100);
            (mid, bubbles(&mut session, 6_000))
        };
        let (_, plain) = rest(None, HistoryEase::Smooth);
        for entrance in [
            SlideUp, Fade, SlideLeft, SlideRight, Drop, Pop, Stamp, Unfold, Tilt,
        ] {
            for ease in [
                HistoryEase::Smooth,
                HistoryEase::Gentle,
                HistoryEase::Overshoot,
                HistoryEase::Linear,
            ] {
                let (mid, done) = rest(entrance, ease);
                assert_eq!(done, plain, "{entrance:?} {ease:?} rests out of place");
                assert_ne!(mid, done, "{entrance:?} {ease:?} does not play");
            }
        }
        // A dropped row comes from above, an unfolding one grows downwards.
        let (mid, done) = rest(Drop, HistoryEase::Smooth);
        assert!(mid[1].0 < done[1].0, "{mid:?} -> {done:?}");
        let (mid, done) = rest(Unfold, HistoryEase::Smooth);
        assert!(mid[1].1 < done[1].1, "{mid:?} -> {done:?}");
    }

    #[test]
    fn the_entrance_takes_the_time_the_skin_gives_it() {
        let playing = |ms, at: u64| {
            let (ui, mut session) = motion_log_with(300, 480, 1, 30, |h| h.entrance_ms = ms);
            bubbles(&mut session, 5_000);
            push_row(ui, &mut session, 2, 5_000);
            !bubbles(&mut session, 5_000 + at).last().unwrap().2
        };
        assert!(!playing(80, 150), "an 80ms entrance is over by 150ms");
        assert!(
            playing(600, 150),
            "a 600ms entrance is still fading at 150ms"
        );
    }

    #[test]
    fn older_rows_step_back_behind_the_newest() {
        let (ui, mut session) = motion_log_with(300, 480, 2, 30, |h| {
            h.recede = Some(nanatype_theme::HistoryRecede {
                step: 0.3,
                floor: 0.2,
            });
        });
        let before = bubbles(&mut session, 5_000);
        assert_eq!(
            before.iter().map(|b| b.2).collect::<Vec<_>>(),
            [false, true],
            "only the newest row is at full strength: {before:?}"
        );
        push_row(ui, &mut session, 3, 5_000);
        let done = bubbles(&mut session, 6_000);
        assert_eq!(
            done.iter().map(|b| b.2).collect::<Vec<_>>(),
            [false, false, true],
            "the row that was newest stepped back: {done:?}"
        );
    }

    #[test]
    fn a_separator_is_ruled_between_rows_only() {
        let (_, mut session) = motion_log_with(300, 480, 3, 30, |h| {
            h.separator = Some(nanatype_theme::HistorySeparator {
                color: nanatype_theme::ColorRef::Rgba(Rgba8::rgb(0, 0, 200)),
                width: 2.0,
                inset: 0.0,
                fade: false,
            });
        });
        let runs = bubbles(&mut session, 5_000);
        // Three bubbles and the two rules between them.
        assert_eq!(runs.len(), 5, "{runs:?}");
        assert_eq!(runs[1].1 - runs[1].0, 2, "{runs:?}");
        let (above, below) = (runs[1].0 - runs[0].1, runs[2].0 - runs[1].1);
        assert!(
            above.abs_diff(below) <= 1,
            "the rule sits mid-gap: {runs:?}"
        );
    }

    #[test]
    fn rows_slide_up_when_a_new_row_overflows_the_log() {
        let (ui, mut session) = motion_log(300, 200, 3, 30);
        let before = bubbles(&mut session, 5_000);
        push_row(ui, &mut session, 4, 5_000);
        let mid = bubbles(&mut session, 5_060);
        let done = bubbles(&mut session, 5_600);
        // The row that was newest: last opaque bubble before, and the one
        // above the new row once settled.
        let was = before.iter().rev().find(|b| b.2).unwrap().1;
        let settled = done[done.len() - 2].1;
        let moving = mid.iter().rev().find(|b| b.2).unwrap().1;
        assert!(settled < was, "the log scrolled: {before:?} -> {done:?}");
        assert!(
            settled < moving && moving < was,
            "the old rows slide up rather than jump: {was} -> {moving} -> {settled}"
        );
        // Rows scrolled off the top are still drawn while the list slides.
        // The log overflows, so its top row is cut at the viewport's edge.
        assert_eq!(
            mid[0].0, done[0].0,
            "the top of the log stays inked: {mid:?} / {done:?}"
        );
    }

    #[test]
    fn rows_close_the_gap_when_the_oldest_row_drops_out() {
        let (ui, mut session) = motion_log(300, 480, 2, 2);
        let before = bubbles(&mut session, 5_000);
        assert_eq!(before.len(), 2, "{before:?}");
        push_row(ui, &mut session, 3, 5_000);
        let mid = bubbles(&mut session, 5_060);
        let done = bubbles(&mut session, 5_600);
        assert_eq!(done.len(), 2, "{done:?}");
        // Row 2 moves from the second place to the first.
        let (from, to) = (before[1].0, done[0].0);
        let moving = mid.iter().find(|b| b.2).unwrap().0;
        // Part of the way: not a jump to the end, nor just the lost gap.
        let progress = (from - moving) as f32 / (from - to) as f32;
        assert!(
            (0.3..0.97).contains(&progress),
            "row 2 slides up into the gap: {from} -> {moving} -> {to}"
        );
    }

    #[test]
    fn resizing_the_window_does_not_slide_the_log() {
        use nana_ui_devtools::agent::AgentSession;
        let (_, mut session) = motion_log(300, 200, 4, 30);
        bubbles(&mut session, 5_000);
        session.set_viewport(300, 260, 1.0).unwrap();
        let t = std::time::Duration::from_millis(5_000);
        session.document_mut().context_mut().advance_animations(t);
        crate::clock::capture_at(5_000, || session.flush().unwrap());
        let mid = bubbles(&mut session, 5_060);
        let done = bubbles(&mut session, 5_600);
        assert_eq!(mid, done, "a resize lands at once");
    }

    #[test]
    fn changing_a_setting_keeps_the_rows_on_screen() {
        // The log overflows and has slid up for new rows a few times.
        let (ui, mut session) = motion_log(300, 200, 3, 30);
        bubbles(&mut session, 1_000);
        for (key, at) in [(4, 1_000), (5, 2_000), (6, 3_000)] {
            push_row(ui, &mut session, key, at);
            bubbles(&mut session, at + 600);
        }
        let before = bubbles(&mut session, 5_000);
        // The app replaces the whole settings value on every edit.
        let edit = |session: &mut nana_ui_devtools::agent::RuntimeAgentSession,
                    at: u64,
                    change: fn(&mut nanatype_settings::Settings)| {
            ui.settings.update(|s| {
                let mut next = (**s).clone();
                change(&mut next);
                *s = Arc::new(next);
            });
            let t = std::time::Duration::from_millis(at);
            session.document_mut().context_mut().advance_animations(t);
            crate::clock::capture_at(at, || session.flush().unwrap());
        };
        // A setting the log does not draw from neither rebuilds its rows
        // (every row would mount anew) nor moves them.
        let built = ROWS_BUILT.get();
        edit(&mut session, 5_000, |s| s.sound.master = 0.5);
        edit(&mut session, 5_000, |s| {
            s.overlays.display.always_on_top ^= true
        });
        assert_eq!(
            ROWS_BUILT.get(),
            built,
            "unrelated settings rebuilt the rows"
        );
        let mid = bubbles(&mut session, 5_060);
        let done = bubbles(&mut session, 5_600);
        assert_eq!(
            mid, before,
            "rows on screen stay put: {before:?} -> {mid:?}"
        );
        assert_eq!(done, before);
        // One it does draw from still reaches the rows.
        edit(&mut session, 6_000, |s| s.history.show_time = true);
        assert!(ROWS_BUILT.get() > built, "the log ignored its own setting");
    }

    #[test]
    fn rows_of_one_speaker_close_together_group() {
        use SourceKind::{Chat, Typewriter};
        let rows: Vec<HistoryRow> = [
            item("Nana", Typewriter, 0),
            item("Nana", Typewriter, 30),
            item("Nana", Chat, 40),
            item("Nana", Typewriter, 50),
            item("Nana", Typewriter, 200),
            item("Mio", Typewriter, 210),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, item)| HistoryRow {
            key: i as u64,
            item,
            doc: Arc::new(nanatype_model::Document::new()),
            reveal: None,
        })
        .collect();
        let shown: Vec<u64> = (0..6).collect();
        // A chat row with the streamer's name, a two-minute pause and a new
        // speaker each start a group.
        assert_eq!(follow_ups(&rows, &shown), [1]);
        // The first shown row always has its header.
        assert_eq!(follow_ups(&rows, &shown[1..]), Vec::<u64>::new());
    }

    #[test]
    fn danmaku_log_renders_separately_and_hides_role_badges() {
        use nana_ui::runtime::{DocumentId, RuntimeDocument};
        use nana_ui_devtools::agent::RuntimeAgentSession;

        let mut settings = nanatype_settings::Settings::default();
        settings.history.show_time = true;
        settings.history.chat.separate = true;
        settings.output.danmaku.enabled = true;
        assert!(search_allowed_for(
            crate::canvas::Canvas::History,
            &settings
        ));
        assert!(!search_allowed_for(
            crate::canvas::Canvas::Danmaku,
            &settings
        ));
        settings.output.danmaku.enabled = false;
        assert!(search_allowed_for(
            crate::canvas::Canvas::Danmaku,
            &settings
        ));

        let skin = Arc::new(Skin::default());
        let ui = Stage::new(
            Arc::new(settings.clone()),
            skin.clone(),
            nanatype_i18n::Locale::ZhCn,
        );

        // Empty danmaku stage uses the danmaku placeholder.
        let id = DocumentId::new(1).unwrap();
        let mut doc = RuntimeDocument::new(id);
        let danmaku_ui = ui.danmaku_log();
        doc.context_mut()
            .mount_view_root(id, move || history_window_with(danmaku_ui, true, || ().into_any()))
            .unwrap();
        let mut session = RuntimeAgentSession::new(doc, 400, 600).unwrap();
        session.flush().unwrap();
        let placeholder = ui.now_t("danmaku-placeholder");
        assert!(
            session
                .accessibility_dump()
                .iter()
                .any(|n| n.label.as_deref() == Some(placeholder.as_str())),
            "expected danmaku placeholder when empty"
        );

        // Populated danmaku log shows viewer messages without role badges, even when host history has its own rows.
        ui.settings.update(|s| {
            let mut next = (**s).clone();
            next.history.fade_after_s = None;
            *s = Arc::new(next);
        });
        ui.history.set(Arc::new(vec![HistoryRow {
            key: 1,
            item: item("HostPerson", SourceKind::Typewriter, 10),
            doc: Arc::new(Document::plain("host line")),
            reveal: None,
        }]));
        ui.danmaku.set(Arc::new(vec![HistoryRow {
            key: 2,
            item: item("ViewerPerson", SourceKind::Chat, 20),
            doc: Arc::new(Document::plain("viewer danmaku")),
            reveal: None,
        }]));
        ui.danmaku_search.set(Some("viewer".into()));
        assert_eq!(visible_keys(danmaku_ui), [2]);
        assert_eq!(visible_keys(ui), [1]);
        session.flush().unwrap();
        let nodes = session.accessibility_dump();
        let viewer_name = skin.history.name("ViewerPerson");
        let host_name = skin.history.name("HostPerson");
        assert!(
            nodes
                .iter()
                .any(|n| n.label.as_deref() == Some(viewer_name.as_str())),
            "expected danmaku author in accessibility tree: {nodes:?}"
        );
        assert!(
            !nodes
                .iter()
                .any(|n| n.label.as_deref() == Some(host_name.as_str())),
            "host history row leaked into danmaku log"
        );
        let viewer_role = ui.now_t("history-role-viewer");
        assert!(
            !nodes
                .iter()
                .any(|n| n.label.as_deref() == Some(viewer_role.as_str())),
            "role badge should be hidden in separate danmaku log"
        );

        // When `separate == true`, Canvas::History shows the host row without a role badge.
        let mut hist_doc = RuntimeDocument::new(id);
        hist_doc
            .context_mut()
            .mount_view_root(id, move || history_window(ui))
            .unwrap();
        let mut hist_session = RuntimeAgentSession::new(hist_doc, 400, 600).unwrap();
        hist_session.flush().unwrap();
        let hist_nodes = hist_session.accessibility_dump();
        assert!(
            hist_nodes
                .iter()
                .any(|n| n.label.as_deref() == Some(host_name.as_str())),
            "expected host row in history window"
        );
        let host_role = ui.now_t("history-role-host");
        assert!(
            !hist_nodes
                .iter()
                .any(|n| n.label.as_deref() == Some(host_role.as_str())),
            "host role badge should be hidden when separate mode is active"
        );
    }
}
