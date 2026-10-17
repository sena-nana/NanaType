//! Static thumbnails for the skin library's card grid.
//!
//! NanaUI has no product-side offscreen path (its offscreen painter lives in
//! the devtools crate, for snapshots only), so a card does not show a
//! rendered picture of the box. It shows a miniature instead: the box and
//! name plate painted from the skin's own [`Surface`]s — the same fill,
//! gradient, border, radii and shadows the live box uses — with bars for the
//! text, static ornaments and the bubble tail. Portraits and text effects
//! are left out; a continue dot appears only if the skin has an indicator.
//!
//! A miniature is worked out once per loaded skin and cached ([`thumb`]);
//! its ornament glyphs come from the skin, independent of the UI locale. The nodes it builds
//! are still, so 24 cards cost nothing per frame.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use nana_ui::runtime::view::{AnyView, IntoView, widget};
use nana_ui::runtime::{NodeStyle, Stack, Text};
use nana_ui_core::{LengthSpec, OverflowSpec, PointerEventsSpec, PositionSpec, TransformOrigin};
use nanatype_theme::{
    ColorRef, ColorRole, DecorLayer, Edge, Len, Skin, TextAlign, place, tail_geometry,
};

use crate::ui::surface::{Surface, affine};
use crate::ui::{SkinRow, c};

/// The space a miniature is fitted into, in px.
pub const FIT: [f32; 2] = [172.0, 60.0];
/// The widest a miniature box gets, in box heights: a 1280×180 box reads
/// as a sliver at card size, so wide boxes are drawn shorter.
const MAX_ASPECT: f32 = 3.0;

/// A placed rectangle, `[x, y, w, h]`, in the skin's own px.
type Rect = [f32; 4];

/// What a miniature draws, in the skin's px, and the scale that fits it.
#[derive(Debug, Clone, PartialEq)]
pub struct Thumb {
    /// Everything drawn: the box, the plate and a little room for shadows.
    pub size: [f32; 2],
    pub scale: f32,
    pub box_rect: Rect,
    pub box_surface: Surface,
    /// `[left, top, right, bottom]`.
    pub padding: [f32; 4],
    pub text_color: [f32; 4],
    /// The text's outline, so outlined text on no box still shows.
    pub stroke: Option<([f32; 4], f32)>,
    /// Bar height and line pitch.
    pub bar: f32,
    pub pitch: f32,
    pub lines: usize,
    pub align: TextAlign,
    pub plate: Option<Plate>,
    pub accent: [f32; 4],
    pub continue_visible: bool,
    below: Vec<Part>,
    above: Vec<Part>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Plate {
    pub rect: Rect,
    pub surface: Surface,
    pub text_color: [f32; 4],
    pub bar: [f32; 2],
}

/// A still, positioned ornament or tail triangle, sharing the live painter.
#[derive(Debug, Clone, PartialEq)]
struct Part {
    rect: Rect,
    surface: Surface,
    glyph: Option<Glyph>,
}

#[derive(Debug, Clone, PartialEq)]
struct Glyph {
    text: String,
    size: f32,
    color: [f32; 4],
    family: Option<String>,
    weight: u16,
}

fn len_px(len: Len, axis: f32) -> f32 {
    match len {
        Len::Px(v) => v,
        Len::Percent(v) => axis * v / 100.0,
    }
}

/// Bounds include the rotated rectangle and all outer shadow layers.
fn part_bounds(part: &Part) -> Rect {
    let [x, y, w, h] = part.rect;
    let [px, py, pw, ph] = part.surface.paint_bounds([w, h]);
    let (sin, cos) = part.surface.rotate_deg.to_radians().sin_cos();
    let corners = [[px, py], [px + pw, py], [px + pw, py + ph], [px, py + ph]].map(|[cx, cy]| {
        [
            x + w * 0.5 + (cx - w * 0.5) * cos - (cy - h * 0.5) * sin,
            y + h * 0.5 + (cx - w * 0.5) * sin + (cy - h * 0.5) * cos,
        ]
    });
    let lo = corners
        .iter()
        .fold([f32::INFINITY; 2], |a, p| [a[0].min(p[0]), a[1].min(p[1])]);
    let hi = corners.iter().fold([f32::NEG_INFINITY; 2], |a, p| {
        [a[0].max(p[0]), a[1].max(p[1])]
    });
    let pad = part
        .surface
        .shadows
        .iter()
        .filter(|s| !s.inset)
        .map(|s| s.blur_radius + s.spread_radius.max(0.0) + s.offset_x.abs().max(s.offset_y.abs()))
        .fold(0.0_f32, f32::max);
    [
        lo[0] - pad,
        lo[1] - pad,
        hi[0] - lo[0] + pad * 2.0,
        hi[1] - lo[1] + pad * 2.0,
    ]
}

impl Thumb {
    /// The miniature of a loaded skin. Frame images resolve against
    /// `urls` (`skin:<path>` → URL); without one the box takes its fill.
    pub fn of(skin: &Skin, urls: &HashMap<String, String>) -> Self {
        let d = &skin.dialogue;
        let url = |path: &str| urls.get(&format!("skin:{path}")).cloned();
        let [mt, mr, mb, ml] = d.box_.margin;
        let bh = (d.region[1] - mt - mb).max(24.0);
        let native_width = (d.region[0] - ml - mr) * d.box_.width_fraction;
        let artwork = d
            .shader_effects
            .iter()
            .any(|e| e.target == nanatype_theme::ShaderTarget::Artwork);
        let bw = if artwork {
            native_width.max(bh)
        } else {
            native_width.clamp(bh, bh * MAX_ASPECT)
        };
        let sx = bw / native_width.max(1.0);
        let mut box_surface =
            Surface::dialogue_box(skin, d.box_.frame.as_ref().and_then(|f| url(&f.image)));
        if box_surface.integrates_tail()
            && let Some(t) = &mut box_surface.tail
        {
            if matches!(t.side, Edge::Top | Edge::Bottom) {
                t.width *= sx;
                t.lean *= sx;
            } else {
                t.length *= sx;
            }
        }
        let size = d.text.size.max(8.0);
        let pitch = size * d.text.line_height.max(1.0);
        let [pl, pt, pr, pb] = d.padding;
        let room = (bh - pt - pb).max(pitch);
        let lines = ((room / pitch).floor() as usize).clamp(1, 2);
        let p = &d.nameplate;
        let plate = p.visible.then(|| {
            let name = p.size.max(8.0);
            let bar = [name * 2.2, name * 0.5];
            let (pw, ph) = (
                bar[0] + 2.0 * p.padding[0],
                name * 1.25 + 2.0 * p.padding[1],
            );
            let [fx, fy] = nanatype_theme::anchor_frac(p.anchor);
            let [ox, oy] = p.offset;
            Plate {
                rect: [
                    (fx * (native_width - pw) + ox) * sx,
                    fy * (bh - ph) + oy,
                    pw * sx,
                    ph,
                ],
                surface: Surface::nameplate(skin, p.frame.as_ref().and_then(|f| url(&f.image))),
                text_color: c(p.text_color),
                bar: [bar[0] * sx, bar[1]],
            }
        });
        let mut below = Vec::new();
        let mut above = Vec::new();
        if let Some(tail) = d.tail.as_ref().filter(|_| !box_surface.integrates_tail()) {
            let b = &d.box_;
            let border = b.border_widths()[tail.side.index()];
            let g = tail_geometry(tail, border);
            let [x, y, w, h] = g.rect;
            let (ax, ay) = match tail.side {
                Edge::Bottom => (native_width * tail.at, bh),
                Edge::Top => (native_width * tail.at, 0.0),
                Edge::Right => (native_width, bh * tail.at),
                Edge::Left => (0.0, bh * tail.at),
            };
            let part = |dx, dy, color, points: &[[f32; 2]]| Part {
                rect: [(ax + x + dx) * sx, ay + y + dy, w * sx, h],
                surface: Surface::tail_part(color, points),
                glyph: None,
            };
            if tail.shadow {
                for shadow in b
                    .shadow_layers()
                    .iter()
                    .filter(|s| !s.inset && s.blur == 0.0 && s.spread == 0.0)
                {
                    below.push(part(
                        shadow.dx,
                        shadow.dy,
                        c(skin.color(shadow.color)),
                        &g.outer,
                    ));
                }
            }
            if border > 0.0 {
                above.push(part(0.0, 0.0, c(b.border_color), &g.outer));
            }
            let fill = skin.color(tail.color.unwrap_or(ColorRef::Role(ColorRole::Box)));
            above.push(part(0.0, 0.0, c(fill), &g.inner));
        }
        for dec in &d.decor {
            let rect = if let Some([t, r, b, l]) = dec.inset {
                [
                    l * sx,
                    t,
                    (native_width - l - r).max(0.0) * sx,
                    (bh - t - b).max(0.0),
                ]
            } else {
                let p = place(dec.anchor, dec.offset, dec.size);
                [
                    (p.left.percent * native_width / 100.0 + p.left.px) * sx,
                    p.top.percent * bh / 100.0 + p.top.px,
                    len_px(dec.size[0], native_width) * sx,
                    len_px(dec.size[1], bh),
                ]
            };
            let font = skin.fonts.role(dec.font);
            let glyph = (!dec.text.is_empty()).then(|| Glyph {
                text: dec.text.clone(),
                size: dec.text_size * sx.min(1.0),
                color: c(skin.color(dec.text_color)),
                family: font.families.first().cloned(),
                weight: font.weight,
            });
            let part = Part {
                rect,
                surface: Surface::decor_image(
                    skin,
                    dec,
                    dec.image.as_ref().and_then(|path| url(path)),
                ),
                glyph,
            };
            match dec.layer {
                DecorLayer::Below => below.push(part),
                DecorLayer::Above => above.push(part),
            }
        }
        let mut lo = [0.0f32, 0.0];
        let mut hi = [bw, bh];
        let mut include = |[x, y, w, h]: Rect| {
            lo = [lo[0].min(x), lo[1].min(y)];
            hi = [hi[0].max(x + w), hi[1].max(y + h)];
        };
        include(part_bounds(&Part {
            rect: [0.0, 0.0, bw, bh],
            surface: box_surface.clone(),
            glyph: None,
        }));
        if let Some(p) = &plate {
            include(part_bounds(&Part {
                rect: p.rect,
                surface: p.surface.clone(),
                glyph: None,
            }));
        }
        for p in below.iter().chain(&above) {
            include(part_bounds(p));
        }
        let room = bh * 0.06;
        let (lo, hi) = (lo.map(|v| v - room), hi.map(|v| v + room));
        let total = [hi[0] - lo[0], hi[1] - lo[1]];
        let scale = (FIT[0] / total[0]).min(FIT[1] / total[1]).min(1.0);
        let shift = |r: Rect| [r[0] - lo[0], r[1] - lo[1], r[2], r[3]];
        Self {
            size: total,
            scale,
            box_rect: shift([0.0, 0.0, bw, bh]),
            box_surface,
            padding: [pl * sx, pt, pr * sx, pb],
            text_color: c(d.text.color),
            stroke: d
                .text
                .stroke
                .as_ref()
                .map(|s| (c(s.color), s.width.max(1.0))),
            bar: size * 0.5,
            pitch,
            lines,
            align: d.text.align,
            plate: plate.map(|p| Plate {
                rect: shift(p.rect),
                ..p
            }),
            accent: c(d.continue_.color),
            continue_visible: !d.continue_.glyph.is_empty() || d.continue_.image.is_some(),
            below: below
                .into_iter()
                .map(|p| Part {
                    rect: shift(p.rect),
                    ..p
                })
                .collect(),
            above: above
                .into_iter()
                .map(|p| Part {
                    rect: shift(p.rect),
                    ..p
                })
                .collect(),
        }
    }

    /// A stand-in for a skin that did not load: a plain box in the row's
    /// colours.
    pub fn fallback(row: &SkinRow) -> Self {
        let (bw, bh) = (180.0, 60.0);
        let room = 4.0;
        let size = [bw + 2.0 * room, bh + 2.0 * room];
        Self {
            size,
            scale: (FIT[0] / size[0]).min(FIT[1] / size[1]),
            box_rect: [room, room, bw, bh],
            box_surface: Surface {
                fill: Some(row.box_color),
                radii: [8.0; 4],
                ..Surface::default()
            },
            padding: [14.0, 12.0, 14.0, 12.0],
            text_color: row.text_color,
            stroke: None,
            bar: 8.0,
            pitch: 16.0,
            lines: 2,
            align: TextAlign::Start,
            plate: None,
            accent: row.accent,
            continue_visible: false,
            below: Vec::new(),
            above: Vec::new(),
        }
    }
}

/// Cached miniatures by skin id, with the skin they were made from: a
/// reloaded skin is a new `Arc` and is worked out again.
type ThumbCache = Mutex<
    Option<
        HashMap<
            String,
            (
                std::sync::Weak<Skin>,
                Arc<Thumb>,
                Arc<HashMap<String, String>>,
            ),
        >,
    >,
>;
static CACHE: ThumbCache = Mutex::new(None);

/// The miniature of `row`, from the cache when its skin has not changed.
pub fn thumb(row: &SkinRow) -> Arc<Thumb> {
    let Some(skin) = row.skin.as_ref().filter(|_| row.error.is_none()) else {
        return Arc::new(Thumb::fallback(row));
    };
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cache = cache.get_or_insert_with(HashMap::new);
    cache.retain(|_, (s, _, _)| s.strong_count() > 0);
    if let Some((cached, t, urls)) = cache.get(&row.id)
        && cached.upgrade().is_some_and(|s| Arc::ptr_eq(&s, skin))
        && **urls == *row.image_urls
    {
        return t.clone();
    }
    let t = Arc::new(Thumb::of(skin, &row.image_urls));
    cache.insert(
        row.id.clone(),
        (Arc::downgrade(skin), t.clone(), row.image_urls.clone()),
    );
    t
}

/// An absolutely placed node at `r`.
fn placed(r: Rect) -> Stack {
    Stack::column(0.0)
        .width(LengthSpec::Px(r[2]))
        .height(LengthSpec::Px(r[3]))
        .with_layout(move |l| {
            l.position = PositionSpec::Absolute;
            l.offset_left = Some(LengthSpec::Px(r[0]));
            l.offset_top = Some(LengthSpec::Px(r[1]));
        })
}

/// A rounded bar standing in for a run of text.
fn bar(w: LengthSpec, h: f32, color: [f32; 4], stroke: Option<([f32; 4], f32)>) -> AnyView {
    widget(
        Stack::row(0.0)
            .width(w)
            .height(LengthSpec::Px(h))
            .with_layout(move |l| {
                l.background = Some(color);
                l.border_radius = Some(h / 2.0);
                l.flex_shrink = Some(0.0);
                if let Some((color, width)) = stroke {
                    l.border_width = Some(width);
                    l.border_color = Some(color);
                    l.border_style = Some(nana_ui_core::BorderStyle::Solid);
                }
            }),
    )
    .into_any()
}

fn part_view(part: &Part) -> AnyView {
    let surface = part.surface.clone();
    let glyph = part.glyph.as_ref().map(|g| {
        let mut style = NodeStyle::default();
        let l = Arc::make_mut(&mut style.layout);
        l.font_size = Some(g.size);
        l.font_family = g.family.clone();
        l.font_weight = Some(g.weight);
        l.color = Some(g.color);
        l.white_space_nowrap = true;
        widget(Text::new(g.text.clone()).decorative().style(style))
    });
    widget(
        surface.stack(
            placed(part.rect)
                .align(nana_ui_core::AlignSpec::Center)
                .justify(nana_ui_core::JustifySpec::Center),
        ),
    )
    .children(glyph)
    .into_any()
}

/// The miniature, drawn at its scale.
pub fn miniature(t: &Thumb) -> AnyView {
    let [w, h] = t.size;
    let scale = t.scale;
    let [pl, pt, pr, pb] = t.padding;
    let align = match t.align {
        TextAlign::Start => nana_ui_core::AlignSpec::Start,
        TextAlign::Center => nana_ui_core::AlignSpec::Center,
        TextAlign::End => nana_ui_core::AlignSpec::End,
    };
    let widths = [78.0, 52.0];
    let lines: Vec<AnyView> = (0..t.lines)
        .map(|i| {
            bar(
                LengthSpec::Percent(widths[i]),
                t.bar,
                t.text_color,
                t.stroke,
            )
        })
        .collect();
    let surface = t.box_surface.clone();
    let gap = (t.pitch - t.bar).max(2.0);
    let top = pt + (t.pitch - t.bar) / 2.0;
    let dot = (t.bar * 0.9).max(6.0);
    let the_box = widget(surface.stack(placed(t.box_rect))).children((
        widget(
            Stack::column(gap)
                .width(LengthSpec::Fill)
                .align(align)
                .with_layout(move |l| {
                    l.padding_left = Some(LengthSpec::Px(pl));
                    l.padding_top = Some(LengthSpec::Px(top));
                    l.padding_right = Some(LengthSpec::Px(pr));
                }),
        )
        .children(lines),
        // Only skins with a real continue indicator get a stand-in dot.
        t.continue_visible.then(|| {
            widget(
                Stack::row(0.0)
                    .width(LengthSpec::Px(dot))
                    .height(LengthSpec::Px(dot))
                    .with_layout({
                        let accent = t.accent;
                        move |l| {
                            l.position = PositionSpec::Absolute;
                            l.offset_right = Some(LengthSpec::Px(pr.max(dot)));
                            l.offset_bottom = Some(LengthSpec::Px(pb.max(dot) * 0.8));
                            l.background = Some(accent);
                            l.border_radius = Some(dot / 2.0);
                        }
                    }),
            )
        }),
    ));
    let plate = t.plate.as_ref().map(|p| {
        let surface = p.surface.clone();
        let rect = p.rect;
        widget(
            surface
                .stack(
                    placed(rect)
                        .align(nana_ui_core::AlignSpec::Center)
                        .justify(nana_ui_core::JustifySpec::Center),
                )
                .with_layout(move |l| {
                    // `apply` may have rotated it in place; keep the offsets.
                    l.position = PositionSpec::Absolute;
                }),
        )
        .children(bar(LengthSpec::Px(p.bar[0]), p.bar[1], p.text_color, None))
    });
    widget(
        Stack::row(0.0)
            .width(LengthSpec::Px(w * scale))
            .height(LengthSpec::Px(h * scale))
            .with_layout(|l| {
                l.position = PositionSpec::Relative;
                l.flex_shrink = Some(0.0);
                l.pointer_events = Some(PointerEventsSpec::None);
            }),
    )
    .children(
        widget(
            Stack::column(0.0)
                .width(LengthSpec::Px(w))
                .height(LengthSpec::Px(h))
                .with_layout(move |l| {
                    l.position = PositionSpec::Absolute;
                    l.offset_left = Some(LengthSpec::Px(0.0));
                    l.offset_top = Some(LengthSpec::Px(0.0));
                    l.overflow_x = OverflowSpec::Visible;
                    l.overflow_y = OverflowSpec::Visible;
                    l.transform = Some(affine(0.0, 0.0, 0.0, scale));
                    l.transform_origin = Some(TransformOrigin {
                        x: LengthSpec::Px(0.0),
                        y: LengthSpec::Px(0.0),
                    });
                }),
        )
        .children((
            t.below.iter().map(part_view).collect::<Vec<_>>(),
            the_box,
            t.above.iter().map(part_view).collect::<Vec<_>>(),
            plate,
        )),
    )
    .into_any()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn miniature_cache_does_not_keep_a_removed_skin_or_its_programs_alive() {
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        let mut row = crate::ui::skin_rows(&registry, nanatype_i18n::Locale::EnUs).remove(0);
        row.id = "removed-test-skin".into();
        row.skin = Some(Arc::new((**row.skin.as_ref().unwrap()).clone()));
        let weak = Arc::downgrade(row.skin.as_ref().unwrap());
        let _thumb = thumb(&row);
        row.skin = None;
        assert_eq!(
            weak.strong_count(),
            0,
            "thumbnail retained decoded shader textures"
        );
    }

    #[test]
    fn every_builtin_miniature_fits_its_slot() {
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        let rows = crate::ui::skin_rows(&registry, nanatype_i18n::Locale::EnUs);
        assert!(rows.len() >= 18);
        for row in &rows {
            let t = thumb(row);
            assert!(row.skin.is_some(), "{} did not load", row.id);
            let [w, h] = t.size.map(|v| v * t.scale);
            assert!(
                w <= FIT[0] + 0.5 && h <= FIT[1] + 0.5,
                "{}: {w}×{h}",
                row.id
            );
            assert!(w >= 60.0, "{} is a sliver: {w}×{h}", row.id);
            assert!(Arc::ptr_eq(&t, &thumb(row)), "{} was not cached", row.id);
        }
    }

    #[test]
    fn a_skin_that_did_not_load_gets_a_plain_box() {
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        let mut row = crate::ui::skin_rows(&registry, nanatype_i18n::Locale::EnUs).remove(0);
        row.skin = None;
        row.error = Some("broken".into());
        let t = thumb(&row);
        assert!(t.plate.is_none());
        assert_eq!(t.box_surface.fill, Some(row.box_color));
    }

    #[test]
    fn ornaments_and_tails_fit_inside_the_thumbnail_and_no_fake_indicator_is_added() {
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        for entry in registry.entries() {
            let skin = entry.skin.as_ref().unwrap();
            let t = Thumb::of(skin, &HashMap::new());
            assert_eq!(
                t.continue_visible,
                !skin.dialogue.continue_.glyph.is_empty()
                    || skin.dialogue.continue_.image.is_some(),
                "{}",
                entry.id
            );
            for part in t.below.iter().chain(&t.above) {
                let [x, y, w, h] = part_bounds(part);
                assert!(
                    x >= 0.0 && y >= 0.0 && x + w <= t.size[0] + 0.01 && y + h <= t.size[1] + 0.01,
                    "{}: {:?}",
                    entry.id,
                    part.rect
                );
            }
            assert!(t.above.len() >= skin.dialogue.decor.len(), "{}", entry.id);
            if skin.dialogue.tail.is_some() && !t.box_surface.integrates_tail() {
                assert!(
                    !t.below.is_empty() || t.above.len() > skin.dialogue.decor.len(),
                    "{}",
                    entry.id
                );
            }
        }
    }
}
