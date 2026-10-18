//! The history window: a Bilibili-style column of avatar, name and bubble.
//! Rows are drawn by the same document builder as the dialogue box, so a line
//! keeps its colours, effects and stickers; with "typing along" sync the
//! newest row reveals in step with the typewriter.

use std::sync::Arc;

use nana_ui::runtime::view::{AnyView, IntoView, Transition, computed, dynamic, each, widget};
use nana_ui::runtime::{NodeStyle, ScrollAxes, ScrollView, Stack, Text};
use nana_ui_core::{
    AlignSpec, BackgroundImage, CssGradient, Easing, GradientStop, LengthSpec, LinearGradient,
    OverflowSpec, PointerEventsSpec, PositionSpec,
};
use nanatype_core::HistoryRow;
use nanatype_model::{Document, Entrance, HistoryItem, Inline, LineId, Rgba8, SourceKind, Stroke};
use nanatype_theme::{AvatarShape, DecorLayer, HistoryEntrance, HistoryLayout, HistoryTitle, Skin};

use super::dialogue::{self, Base, Timing, document};
use super::surface::{self, Surface};
use super::{Ui, c};

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

/// How long a new row takes to settle in.
const ROW_ENTER_MS: u64 = 220;
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
        None => 16.0 + 8.0,
    }
}

fn initial(name: &str) -> String {
    name.chars()
        .next()
        .map(|c| c.to_string())
        .unwrap_or_else(|| "·".into())
}

/// Width available to a row's text column.
fn column_width(skin: &nanatype_theme::Skin, settings: &nanatype_settings::Settings) -> f32 {
    let (w, _) = crate::windows::size_of(crate::windows::WindowKind::History, skin, settings);
    let avatar = if settings.history.show_avatar && skin.history.avatar.shape != AvatarShape::None {
        skin.history.avatar.size + 10.0
    } else {
        0.0
    };
    (w - list_inset(&skin.history) - avatar).max(80.0)
}

/// Fade the entire bubble artwork as one layer, leaving its text at full
/// strength. Reserving the artwork's borders in the content padding keeps
/// image frames, retained contours and ordinary CSS surfaces on the same path.
fn bubble_view(surface: Surface, opacity: f32, pad: [f32; 2], max: f32, body: AnyView) -> AnyView {
    let widths = surface.frame.as_ref().map_or_else(
        || surface.border.as_ref().map_or([0.0; 4], |b| b.widths),
        |(_, widths)| *widths,
    );
    let [top, right, bottom, left] = widths;
    let backdrop = widget(Stack::column(0.0).with_layout(move |l| {
        l.position = PositionSpec::Absolute;
        // NanaUI anchors absolute children to the parent's content box.
        // Extend through the reserved padding to paint its full outer box.
        l.offset_top = Some(LengthSpec::Px(-pad[1] - top));
        l.offset_right = Some(LengthSpec::Px(-pad[0] - right));
        l.offset_bottom = Some(LengthSpec::Px(-pad[1] - bottom));
        l.offset_left = Some(LengthSpec::Px(-pad[0] - left));
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

/// Halo's presentation follows the approved interaction preview. Other skins
/// retain their own log layout and speaker grouping. All histories start at the top.
fn interaction_presentation(skin: &Skin) -> bool {
    skin.meta.id == "nanatype.halo"
}

fn entry_view(ui: Ui, key: u64, first: bool, follows: bool) -> AnyView {
    let gap = if first {
        0.0
    } else {
        ui.skin.with_untracked(|s| row_gap(s, follows))
    };
    widget(
        Stack::column(0.0)
            .width(LengthSpec::Fill)
            .with_layout(move |l| {
                l.margin_top = Some(LengthSpec::Px(gap));
            }),
    )
    .children(message_view(ui, key, follows))
    .into_any()
}

fn message_view(ui: Ui, key: u64, follows: bool) -> AnyView {
    let source = super::danmaku::RowSource::History(key);
    let mut skin = super::materials::render_skin(ui, ui.skin.get_untracked());
    let settings = ui.settings.get_untracked();
    let fitted = interaction_presentation(&skin);
    if fitted {
        let k = Arc::make_mut(&mut skin);
        k.history.avatar.size = k.history.avatar.size.min(36.0);
        k.history.text.size = k.history.text.size.clamp(14.0, 17.0);
    }
    let h = skin.history.clone();
    let backdrop = h
        .panel
        .as_ref()
        .and_then(|p| super::quote::backdrop(p.color, false));
    let exposed = backdrop.is_none();
    let show_avatar = settings.history.show_avatar && h.avatar.shape != AvatarShape::None;
    let col_w = if fitted {
        let width =
            crate::windows::size_of(crate::windows::WindowKind::History, &skin, &settings).0 - 38.0;
        (width
            - if show_avatar {
                h.avatar.size + 10.0
            } else {
                0.0
            })
        .max(60.0)
    } else {
        column_width(&skin, &settings)
    };
    let Some(row) = source.get(ui) else {
        return widget(Stack::row(0.0)).into_any();
    };
    let inline = h.layout == HistoryLayout::Inline && !fitted;
    // Echo-Live's log names every line; bubble lists group.
    let follows = follows && !inline;
    let size = h.avatar.size;
    let radius = match h.avatar.shape {
        AvatarShape::Circle => size / 2.0,
        _ => h.avatar.radius.unwrap_or(size * 0.18),
    };
    // Chat rows name a saved picture (`blob:…`); the local speaker's avatar
    // is a path or a URL.
    let chat = row.item.kind != SourceKind::Typewriter;
    let bubble_opacity = if chat {
        h.chat.bubble_opacity.clamp(0.0, 1.0)
    } else {
        1.0
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
            Some(super::danmaku::reactive_picture(ui, asset, radius)),
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
    let show_time = settings.history.show_time && !follows;
    let inline_name_w = inline_name_width(&h, col_w, show_time);
    // Who is talking, once live chat shares the list.
    let role = (show_name && !inline && (chat || settings.history.chat.enabled)).then(|| {
        let key = if chat {
            "history-role-viewer"
        } else {
            "history-role-host"
        };
        widget(Text::new(ui.now_t(key)).style(time_style.clone()))
    });
    let name = show_name.then(|| {
        let plate = name_plate.clone();
        let max_name = if inline { inline_name_w } else { col_w - 64.0 }.max(32.0);
        Arc::make_mut(&mut name_style.layout).max_width = Some(LengthSpec::Px(
            (max_name - plate_pad[0] * 2.0 - plate_border).max(12.0),
        ));
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
            Text::new(row.item.author.clone())
                .style(name_style)
                .truncating(),
        ))
    });
    let time = show_time
        .then(|| widget(Text::new(local_time(row.item.at, h.time_seconds)).style(time_style)));

    let pad = h.bubble_padding;
    let frame_url = h.bubble_frame.as_ref().and_then(|f| {
        ui.asset_urls
            .with_untracked(|m| m.get(&format!("skin:{}", f.image)).cloned())
    });
    let mut bubble_surface =
        Surface::bubble(&skin, frame_url).materials(super::materials::enabled(ui));
    if follows {
        // Only a group's first bubble points at the avatar.
        bubble_surface.radii[0] = bubble_surface.radii[0].max(h.bubble_radius);
    }
    let body_skin = skin.clone();
    let body = dynamic(
        move || {
            source.track(ui);
            source.get(ui).map(|r| {
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
            let Some(row) = source.get(ui) else {
                return widget(Stack::row(0.0)).into_any();
            };
            let skin = body_skin.clone();
            let entrance = Entrance {
                kind: skin.dialogue.reveal.entrance.kind,
                ms: skin.dialogue.reveal.entrance.ms,
            };
            let mut base = Base::history(&skin, entrance, ui.reduced_motion.get_untracked());
            if row.item.kind != SourceKind::Typewriter {
                let scale = skin.history.chat.text_scale;
                if scale.is_finite() && scale > 0.0 {
                    base.size *= scale.clamp(0.3, 3.0);
                }
            }
            let timing = match &row.reveal {
                Some(r) => Timing::Reveal(r.clone()),
                None => Timing::Static,
            };
            if exposed
                && f32::from(skin.history.bubble_color.a) * bubble_opacity < 128.0
                && base.stroke.is_none()
            {
                document(ui, &transparent_text(&row.doc, base.color), &base, timing)
            } else {
                document(ui, &row.doc, &base, timing)
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
        (col_w - name_w - time_w).max(40.0)
    } else {
        col_w
    };
    let bubble = bubble_view(
        bubble_surface,
        bubble_opacity,
        pad,
        bubble_max,
        body.into_any(),
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
                    l.max_width = Some(LengthSpec::Px(bubble_max));
                    l.align_items = AlignSpec::Start;
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
                    .align(AlignSpec::Center)
                    .width(LengthSpec::Fill),
            )
            .children((name, role, time))
            .into_any()
        });
        widget(
            Stack::column(6.0)
                .width(LengthSpec::Px(col_w))
                .with_layout(|l| {
                    l.min_width = Some(LengthSpec::Px(0.0));
                    l.align_items = AlignSpec::Start;
                }),
        )
        .children((header, bubble))
        .into_any()
    };

    let fade_after = settings.history.fade_after_s;
    let at = row.item.at;
    let view = widget(
        Stack::row(10.0)
            .align(AlignSpec::Start)
            .width(LengthSpec::Fill),
    )
    .bind(move |s: &mut Stack| {
        let Some(after) = fade_after else {
            return;
        };
        let age = crate::clock::unix_of(ui.history_now.get()).saturating_sub(at);
        let start = u64::from(after) * 1000;
        let opacity = 1.0 - (age.saturating_sub(start) as f32 / ROW_FADE_MS as f32).clamp(0.0, 1.0);
        let mut taken = std::mem::replace(s, Stack::row(0.0));
        taken = taken.with_layout(|l| l.opacity = Some(opacity));
        *s = taken;
    })
    // Inline rows drop the empty avatar slot, so the name sits at the edge.
    .children(((show_avatar || !inline).then_some(avatar), content))
    .into_any();
    if fitted {
        widget(
            Stack::column(0.0)
                .width(LengthSpec::Fill)
                .padding_xy(8.0, 0.0),
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
    hidden: [Option<LineId>; 2],
}

fn selection(ui: Ui) -> Selection {
    let (visible, hide) = ui
        .settings
        .with(|s| (s.history.visible_items as usize, s.history.hide_latest));
    let hide = hide.resolve(ui.skin.with(|s| s.history.hide_latest));
    let hidden = if !hide {
        [None; 2]
    } else if ui.peer.with(|p| p.active) {
        [ui.peer_local_line.get(), ui.peer_display.with(|d| d.line)]
    } else {
        [ui.display.with(|d| d.line), None]
    };
    Selection { visible, hidden }
}

/// Newest first: stop after the visible suffix instead of scanning the log.
fn selected_rows(rows: &[HistoryRow], selection: Selection) -> impl Iterator<Item = &HistoryRow> {
    rows.iter()
        .rev()
        .filter(move |row| row.item.line.is_none() || !selection.hidden.contains(&row.item.line))
        .take(selection.visible)
}

fn visible_keys(ui: Ui) -> Vec<u64> {
    let selection = selection(ui);
    ui.history.with(|rows| {
        let mut keys: Vec<_> = selected_rows(rows, selection).map(|r| r.key).collect();
        keys.reverse();
        keys
    })
}

#[derive(Default, Debug, PartialEq)]
pub(crate) struct FadeSchedule {
    pub animating: bool,
    pub next_at: Option<u64>,
}

/// Only mounted history rows can need the history clock. Read untracked from
/// host callbacks, but share exactly the list's selection and fade interval.
pub(crate) fn fade_schedule(ui: Ui, now: u64) -> FadeSchedule {
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

pub fn history_window(ui: Ui) -> impl IntoView {
    let items = computed(move || visible_keys(ui));
    let follows = computed(move || {
        if ui.skin.with(|s| interaction_presentation(s)) {
            return Vec::new();
        }
        let shown = items.get();
        ui.history.with(|rows| follow_ups(rows, &shown))
    });
    let key = move || {
        let skin = ui.skin.get();
        let settings = ui.settings.get();
        (Arc::as_ptr(&skin) as usize, Arc::as_ptr(&settings) as usize)
    };
    dynamic(key, move |_| {
        let skin = super::materials::render_skin(ui, ui.skin.get_untracked());
        let presentation = interaction_presentation(&skin);
        let ms = std::time::Duration::from_millis(ROW_ENTER_MS);
        // Reduced motion keeps the fade and drops the travel.
        let reduced = ui.reduced_motion.get_untracked();
        let transition = match skin.history.entrance {
            HistoryEntrance::None => None,
            _ if reduced => Some(Transition::fade(ms)),
            HistoryEntrance::SlideUp => Some(Transition::slide(0.0, 16.0, ms)),
            HistoryEntrance::SlideLeft => Some(Transition::slide(-24.0, 0.0, ms)),
            HistoryEntrance::Fade => Some(Transition::fade(ms)),
        }
        .map(|t| t.ease(Easing::CubicBezier([0.2, 0.8, 0.2, 1.0])));
        let list = each(
            items,
            |k: &u64| *k,
            move |k: u64| {
                dynamic(
                    move || {
                        (
                            items.with(|i| i.first() == Some(&k)),
                            follows.with(|f| f.contains(&k)),
                        )
                    },
                    move |(first, f)| entry_view(ui, k, *first, *f),
                )
            },
        );
        let list = match transition {
            Some(t) => list.transition(t),
            None => list,
        };
        let h = &skin.history;
        let [mt, mr, mb, ml] = h
            .panel
            .as_ref()
            .map_or(if presentation { [0.0; 4] } else { [8.0; 4] }, |p| p.margin);
        let [pl, pt, pr, pb] = h.panel.as_ref().map_or(
            if presentation {
                [6.0, 16.0, 6.0, 16.0]
            } else {
                [4.0, 8.0, 4.0, 8.0]
            },
            |p| p.padding,
        );
        let panel_surface = Surface::history_panel(&skin);
        let decor = |layer: DecorLayer| -> Vec<AnyView> {
            h.decor
                .iter()
                .filter(|d| d.layer == layer)
                .map(|d| {
                    let image = d.image.as_ref().and_then(|path| {
                        ui.asset_urls
                            .with_untracked(|urls| urls.get(&format!("skin:{path}")).cloned())
                    });
                    dialogue::decor(&skin, d, None::<fn() -> bool>, image)
                })
                .collect()
        };
        let (below, above) = (decor(DecorLayer::Below), decor(DecorLayer::Above));
        let title = h.title.as_ref().map(|t| title_view(&skin, t));
        widget(Stack::fill_column(0.0).with_layout(move |l| {
            l.padding_top = Some(LengthSpec::Px(mt));
            l.padding_right = Some(LengthSpec::Px(mr));
            l.padding_bottom = Some(LengthSpec::Px(mb));
            l.padding_left = Some(LengthSpec::Px(ml));
        }))
        .children(
            widget(
                Surface::optional_stack(panel_surface.as_ref(), Stack::fill_column(0.0))
                    .with_layout(move |l| {
                        l.position = PositionSpec::Relative;
                        // Window move/scale gestures take priority over scrolling
                        // the list, including in the default interactive mode.
                        if ui.settings.with_untracked(|x| {
                            x.overlays.history.lock != nanatype_settings::Lock::Locked
                        }) {
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
                widget(
                    ScrollView::new(ScrollAxes::Vertical)
                        .follow_end(true)
                        .with_layout(|l| {
                            l.flex_grow = Some(1.0);
                            l.flex_shrink = Some(1.0);
                            l.min_height = Some(LengthSpec::Px(0.0));
                        }),
                )
                .children(
                    widget(Stack::column(0.0).with_layout(move |l| {
                        l.padding_left = Some(LengthSpec::Px(pl));
                        l.padding_top = Some(LengthSpec::Px(pt));
                        l.padding_right = Some(LengthSpec::Px(pr));
                        l.padding_bottom = Some(LengthSpec::Px(pb));
                        // A short log always starts at its content area's top;
                        // follow_end still scrolls overflowing logs to the newest row.
                        l.min_height = Some(LengthSpec::Percent(100.0));
                        l.justify_content = nana_ui_core::JustifySpec::Start;
                    }))
                    .children(list),
                ),
                above,
            )),
        )
    })
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
        assert_eq!(keys(30, [None; 2]), [1, 2, 3]);
        // Only the line still on the box is left out; external rows stay.
        assert_eq!(keys(30, [Some(LineId(11)), None]), [1, 2]);
        // The cap counts the rows that show.
        assert_eq!(keys(1, [Some(LineId(11)), None]), [2]);
        assert_eq!(keys(30, [Some(LineId(10)), Some(LineId(11))]), [2]);
        assert!(keys(0, [None; 2]).is_empty());
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
        let ui = Ui::new(
            Arc::new(settings),
            Arc::new(Skin::default()),
            nanatype_i18n::Locale::ZhCn,
        );
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
        ui.peer.update(|p| Arc::make_mut(p).active = true);
        ui.peer_local_line.set(Some(LineId(10)));
        ui.peer_display
            .update(|d| Arc::make_mut(d).line = Some(LineId(11)));
        assert!(visible_keys(ui).is_empty());
        assert_eq!(fade_schedule(ui, 2000), FadeSchedule::default());
        ui.peer.update(|p| Arc::make_mut(p).active = false);
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
        }
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
}
