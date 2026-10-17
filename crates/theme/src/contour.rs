//! Vector geometry shared by live surfaces and miniatures. No UI/GPU types.
use crate::{ContourPreset, Edge, MotifPreset, TailSpec};
use std::f32::consts::{PI, TAU};

#[derive(Debug, Clone, PartialEq)]
pub enum ShapeCommand {
    Move([f32; 2]),
    Line([f32; 2]),
    Quad([f32; 2], [f32; 2]),
    Cubic([f32; 2], [f32; 2], [f32; 2]),
    Close,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct ShapePath {
    pub commands: Vec<ShapeCommand>,
    pub even_odd: bool,
}
impl ShapePath {
    fn m(&mut self, p: [f32; 2]) {
        self.commands.push(ShapeCommand::Move(p));
    }
    fn l(&mut self, p: [f32; 2]) {
        self.commands.push(ShapeCommand::Line(p));
    }
    fn q(&mut self, a: [f32; 2], b: [f32; 2]) {
        self.commands.push(ShapeCommand::Quad(a, b));
    }
    fn c(&mut self, a: [f32; 2], b: [f32; 2], p: [f32; 2]) {
        self.commands.push(ShapeCommand::Cubic(a, b, p));
    }
    fn close(&mut self) {
        self.commands.push(ShapeCommand::Close);
    }
    /// Conservative bounds, including every Bezier control point.
    pub fn bounds(&self) -> Option<[f32; 4]> {
        let mut lo = [f32::INFINITY; 2];
        let mut hi = [f32::NEG_INFINITY; 2];
        for c in &self.commands {
            let points = match *c {
                ShapeCommand::Move(p) | ShapeCommand::Line(p) => [Some(p), None, None],
                ShapeCommand::Quad(a, b) => [Some(a), Some(b), None],
                ShapeCommand::Cubic(a, b, c) => [Some(a), Some(b), Some(c)],
                ShapeCommand::Close => [None; 3],
            };
            for p in points.into_iter().flatten() {
                for i in 0..2 {
                    lo[i] = lo[i].min(p[i]);
                    hi[i] = hi[i].max(p[i]);
                }
            }
        }
        lo[0]
            .is_finite()
            .then_some([lo[0], lo[1], hi[0] - lo[0], hi[1] - lo[1]])
    }
    fn ellipse_arc(
        &mut self,
        center: [f32; 2],
        radii: [f32; 2],
        start: f32,
        sweep: f32,
        move_to: bool,
    ) {
        let point = |a: f32| {
            [
                center[0] + radii[0] * a.cos(),
                center[1] + radii[1] * a.sin(),
            ]
        };
        if move_to {
            self.m(point(start));
        }
        let n = (sweep.abs() / (PI * 0.5)).ceil().max(1.0) as usize;
        for i in 0..n {
            let a = start + sweep * i as f32 / n as f32;
            let b = start + sweep * (i + 1) as f32 / n as f32;
            let k = 4.0 / 3.0 * ((b - a) / 4.0).tan();
            let p = point(a);
            let q = point(b);
            self.c(
                [p[0] - k * radii[0] * a.sin(), p[1] + k * radii[1] * a.cos()],
                [q[0] + k * radii[0] * b.sin(), q[1] - k * radii[1] * b.cos()],
                q,
            );
        }
    }
}

impl ContourPreset {
    pub const ALL: [Self; 16] = [
        Self::Rounded,
        Self::Orbit,
        Self::Hud,
        Self::Parchment,
        Self::Cinema,
        Self::Rpg,
        Self::Slant,
        Self::Cloud,
        Self::Chat,
        Self::Comic,
        Self::Terminal,
        Self::Pixel,
        Self::Sakura,
        Self::Ink,
        Self::Ribbon,
        Self::Seal,
    ];
}
impl MotifPreset {
    pub const ALL: [Self; 15] = [
        Self::Rose,
        Self::Moon,
        Self::Leaf,
        Self::Jellyfish,
        Self::Lantern,
        Self::CitrusSlice,
        Self::Orbit,
        Self::Sparkle,
        Self::Blossom,
        Self::Petal,
        Self::Flourish,
        Self::Rays,
        Self::Halftone,
        Self::Fold,
        Self::Circuit,
    ];
}

/// Clockwise edges in a local frame; positive v reaches outside the box.
fn edge_point(edge: Edge, u: f32, v: f32, origin: [f32; 2], size: [f32; 2]) -> [f32; 2] {
    let [x, y] = origin;
    let [w, h] = size;
    match edge {
        Edge::Top => [x + u, y - v],
        Edge::Right => [x + w + v, y + u],
        Edge::Bottom => [x + w - u, y + h + v],
        Edge::Left => [x - v, y + h - u],
    }
}
fn edge_profile(
    p: &mut ShapePath,
    preset: ContourPreset,
    edge: Edge,
    a: f32,
    b: f32,
    origin: [f32; 2],
    size: [f32; 2],
) {
    if b <= a {
        return;
    }
    let map = |u, v| edge_point(edge, u, v, origin, size);
    let [w, h] = size;
    let min = w.min(h);
    let wave = match preset {
        ContourPreset::Cloud => 14.0_f32.min(min * 0.1),
        ContourPreset::Sakura => 10.0_f32.min(min * 0.07),
        ContourPreset::Parchment => 4.0_f32.min(min * 0.035),
        ContourPreset::Chat if edge == Edge::Right => 5.0_f32.min(min * 0.05),
        ContourPreset::Ribbon if edge == Edge::Left => 3.0_f32.min(min * 0.1),
        _ => 0.0,
    };
    if matches!(
        preset,
        ContourPreset::Ink | ContourPreset::Seal | ContourPreset::Comic
    ) {
        let n = (((b - a) / 55.0).ceil() as usize).clamp(3, 18);
        let amp: f32 = match preset {
            ContourPreset::Ink => 8.0,
            ContourPreset::Comic => 2.5,
            _ => 1.3,
        };
        let amp = amp.min(
            min * if preset == ContourPreset::Ink {
                0.08
            } else {
                0.04
            },
        );
        for i in 1..n {
            let depth = amp * ([0.3, 0.9, 0.45, 1.0, 0.1][i % 5]);
            p.l(map(a + (b - a) * i as f32 / n as f32, -depth));
        }
        p.l(map(b, 0.0));
    } else if wave > 0.0 {
        let n = if matches!(preset, ContourPreset::Cloud | ContourPreset::Sakura) {
            (((b - a) / 135.0).ceil() as usize).clamp(1, 8)
        } else {
            (((b - a) / 220.0).ceil() as usize).clamp(1, 5)
        };
        let step = (b - a) / n as f32;
        for i in 0..n {
            let s = a + step * i as f32;
            let d = wave * if i % 2 == 0 { 1.0 } else { 0.7 };
            p.c(
                map(s + step * 0.25, 0.0),
                map(s + step * 0.25, -d),
                map(s + step * 0.5, -d),
            );
            p.c(
                map(s + step * 0.75, -d),
                map(s + step * 0.75, 0.0),
                map(s + step, 0.0),
            );
        }
    } else {
        p.l(map(b, 0.0));
    }
}
fn draw_edge(
    p: &mut ShapePath,
    preset: ContourPreset,
    edge: Edge,
    range: [f32; 2],
    origin: [f32; 2],
    size: [f32; 2],
    tail: Option<&TailSpec>,
) {
    let [a, b] = range;
    let Some(t) = tail.filter(|t| t.side == edge && t.width > 0.0 && t.length > 0.0 && b - a > 1.0)
    else {
        edge_profile(p, preset, edge, a, b, origin, size);
        return;
    };
    let len = if matches!(edge, Edge::Top | Edge::Bottom) {
        size[0]
    } else {
        size[1]
    };
    let reversed = matches!(edge, Edge::Bottom | Edge::Left);
    let width = t.width.min((b - a) * 0.6);
    let raw = t.at * len;
    let s = (if reversed { len - raw - width } else { raw }).clamp(a, b - width);
    let e = s + width;
    let lean = if reversed { -t.lean } else { t.lean };
    let tip = (s + width * 0.5 + lean).clamp(a, b);
    let map = |u, v| edge_point(edge, u, v, origin, size);
    edge_profile(p, preset, edge, a, s, origin, size);
    if matches!(
        preset,
        ContourPreset::Rounded | ContourPreset::Cloud | ContourPreset::Sakura | ContourPreset::Chat
    ) {
        p.c(
            map(s + width * 0.25, 0.0),
            map(tip - width * 0.12, t.length * 0.68),
            map(tip, t.length),
        );
        p.c(
            map(tip + width * 0.12, t.length * 0.8),
            map(e - width * 0.2, 0.0),
            map(e, 0.0),
        );
    } else {
        p.l(map(tip, t.length));
        p.l(map(e, 0.0));
    }
    edge_profile(p, preset, edge, e, b, origin, size);
}

/// Closed frame path. All inward cuts fit in a 16px edge strip on large
/// surfaces and shrink on small labels/history bubbles. Tails are one path.
pub fn contour_path(
    preset: ContourPreset,
    size: [f32; 2],
    radius: f32,
    inset: f32,
    tail: Option<&TailSpec>,
) -> ShapePath {
    let mut p = ShapePath::default();
    let [ww, hh] = size;
    if !ww.is_finite() || !hh.is_finite() || ww <= 0.0 || hh <= 0.0 {
        return p;
    }
    let inset = inset.max(0.0).min(ww.min(hh) * 0.2);
    let origin = [inset, inset];
    let size = [ww - 2.0 * inset, hh - 2.0 * inset];
    let [w, h] = size;
    let base = radius.max(4.0).min(h * 0.3).min(w * 0.15);
    use ContourPreset::*;
    let mut rs = match preset {
        Rounded => [base; 4],
        Orbit => [base, base * 1.7, base * 0.7, base * 1.3],
        Cloud => [base * 0.9, base * 1.5, base, base * 1.2],
        Sakura => [base, base * 1.4, base * 1.2, base * 0.8],
        Chat => [4.0, base * 1.4, base, base],
        Hud => [8.0, 22.0, 8.0, 16.0],
        Cinema => [2.0, 12.0, 2.0, 12.0],
        Rpg => [10.0; 4],
        Pixel => [8.0; 4],
        Terminal => [6.0; 4],
        Slant => [0.0, 24.0, 4.0, 0.0],
        Ribbon => [3.0, h * 0.5, h * 0.5, 3.0],
        Ink => [3.0; 4],
        Seal => [2.0; 4],
        Comic => [8.0, 15.0, 7.0, 12.0],
        Parchment => [base.min(7.0); 4],
    };
    for r in &mut rs {
        *r = r.min(w * 0.25).min(h * 0.5);
        if matches!(preset, Rpg | Pixel) {
            *r = r.floor();
        }
    }
    if let Some(t) = tail.filter(|t| t.width > 0.0 && t.length > 0.0) {
        let (a, b, len) = match t.side {
            Edge::Top => (0, 1, w),
            Edge::Right => (1, 2, h),
            Edge::Bottom => (2, 3, w),
            Edge::Left => (3, 0, h),
        };
        let available = (len - t.width.min(len * 0.6)).max(0.0);
        let sum = rs[a] + rs[b];
        if sum > available && sum > 0.0 {
            let factor = available / sum;
            rs[a] *= factor;
            rs[b] *= factor;
        }
    }
    let [x, y] = origin;
    let [ul, ur, br, bl] = rs;
    let corner = |p: &mut ShapePath, outer: [f32; 2], start: [f32; 2], end: [f32; 2]| {
        if matches!(preset, Rpg | Pixel) {
            let mid = [(start[0] + end[0]) * 0.5, (start[1] + end[1]) * 0.5];
            p.l([mid[0], start[1]]);
            p.l(mid);
            p.l([end[0], mid[1]]);
            p.l(end);
        } else if matches!(
            preset,
            Hud | Cinema | Slant | Comic | Terminal | Ribbon | Seal
        ) {
            p.l(end);
        } else {
            p.q(outer, end);
        }
    };
    p.m([x + ul, y]);
    draw_edge(&mut p, preset, Edge::Top, [ul, w - ur], origin, size, tail);
    corner(&mut p, [x + w, y], [x + w - ur, y], [x + w, y + ur]);
    draw_edge(
        &mut p,
        preset,
        Edge::Right,
        [ur, h - br],
        origin,
        size,
        tail,
    );
    corner(
        &mut p,
        [x + w, y + h],
        [x + w, y + h - br],
        [x + w - br, y + h],
    );
    draw_edge(
        &mut p,
        preset,
        Edge::Bottom,
        [br, w - bl],
        origin,
        size,
        tail,
    );
    corner(&mut p, [x, y + h], [x + bl, y + h], [x, y + h - bl]);
    draw_edge(&mut p, preset, Edge::Left, [bl, h - ul], origin, size, tail);
    corner(&mut p, [x, y], [x, y + ul], [x + ul, y]);
    p.close();
    p
}

#[derive(Debug, Clone, PartialEq)]
pub struct MotifPath {
    pub path: ShapePath,
    pub fill: bool,
    pub opacity: f32,
    pub stroke_width: f32,
}
/// Fixed vector motifs, never arbitrary skin-provided executable drawing.
pub fn motif_paths(motif: MotifPreset, size: [f32; 2]) -> Vec<MotifPath> {
    let [w, h] = size;
    if !w.is_finite() || !h.is_finite() || w <= 0.0 || h <= 0.0 {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut add = |path, fill, opacity, stroke_width| {
        out.push(MotifPath {
            path,
            fill,
            opacity,
            stroke_width,
        })
    };
    let pt = |x: f32, y: f32| [x * w, y * h];
    let mut p = ShapePath::default();
    use MotifPreset::*;
    match motif {
        Rose => {
            for i in 0..7 {
                let a = i as f32 * TAU / 7.;
                let center = pt(0.5 + 0.19 * a.cos(), 0.5 + 0.19 * a.sin());
                let mut petal = ShapePath::default();
                petal.ellipse_arc(center, [w * 0.23, h * 0.23], 0., TAU, true);
                petal.close();
                add(petal, false, 0.9, 1.4);
            }
            p.ellipse_arc(pt(0.5, 0.5), [w * 0.12, h * 0.12], 0., TAU, true);
            p.close();
            add(p, false, 1., 1.5);
        }
        Moon => {
            p.m(pt(0.72, 0.08));
            p.c(pt(0.12, 0.02), pt(0.02, 0.86), pt(0.66, 0.94));
            p.c(pt(0.95, 0.96), pt(0.97, 0.7), pt(0.96, 0.66));
            p.c(pt(0.39, 0.94), pt(0.25, 0.25), pt(0.72, 0.08));
            p.close();
            add(p, true, 1., 0.);
        }
        Leaf => {
            p.m(pt(0.08, 0.92));
            p.c(pt(0.02, 0.35), pt(0.43, 0.05), pt(0.93, 0.07));
            p.c(pt(0.98, 0.58), pt(0.58, 0.98), pt(0.08, 0.92));
            p.close();
            add(p, true, 0.7, 0.);
            let mut vein = ShapePath::default();
            vein.m(pt(0.08, 0.92));
            vein.q(pt(0.6, 0.5), pt(0.88, 0.12));
            vein.m(pt(0.39, 0.65));
            vein.l(pt(0.25, 0.35));
            vein.m(pt(0.57, 0.47));
            vein.l(pt(0.8, 0.6));
            add(vein, false, 1., 1.2);
        }
        Jellyfish => {
            p.m(pt(0.12, 0.48));
            p.c(pt(0.05, 0.01), pt(0.95, 0.01), pt(0.88, 0.48));
            p.q(pt(0.69, 0.62), pt(0.5, 0.5));
            p.q(pt(0.31, 0.62), pt(0.12, 0.48));
            p.close();
            add(p, true, 0.7, 0.);
            for i in 0..4 {
                let x = 0.25 + i as f32 * 0.16;
                let mut t = ShapePath::default();
                t.m(pt(x, 0.48));
                t.c(pt(x - 0.13, 0.69), pt(x + 0.13, 0.76), pt(x - 0.02, 0.96));
                add(t, false, 0.8, 1.2);
            }
        }
        Lantern => {
            p.m(pt(0.3, 0.16));
            p.l(pt(0.7, 0.16));
            p.c(pt(1., 0.35), pt(1., 0.65), pt(0.7, 0.8));
            p.l(pt(0.3, 0.8));
            p.c(pt(0., 0.65), pt(0., 0.35), pt(0.3, 0.16));
            p.close();
            add(p, true, 0.75, 0.);
            let mut ribs = ShapePath::default();
            for x in [0.35, 0.5, 0.65] {
                ribs.m(pt(x, 0.16));
                ribs.q(pt(0.5 + (x - 0.5) * 1.7, 0.48), pt(x, 0.8));
            }
            ribs.m(pt(0.25, 0.12));
            ribs.l(pt(0.75, 0.12));
            ribs.m(pt(0.25, 0.84));
            ribs.l(pt(0.75, 0.84));
            ribs.m(pt(0.5, 0.84));
            ribs.l(pt(0.5, 0.98));
            add(ribs, false, 1., 1.3);
        }
        CitrusSlice => {
            p.ellipse_arc(pt(0.5, 0.5), [w * 0.46, h * 0.46], 0., TAU, true);
            p.close();
            add(p, false, 1., 2.);
            let mut pulp = ShapePath::default();
            for i in 0..8 {
                let a = i as f32 * TAU / 8.;
                pulp.m(pt(0.5 + 0.12 * a.cos(), 0.5 + 0.12 * a.sin()));
                pulp.l(pt(0.5 + 0.36 * a.cos(), 0.5 + 0.36 * a.sin()));
            }
            add(pulp, false, 0.9, 2.5);
        }
        Orbit => {
            p.ellipse_arc(
                pt(0.5, 0.5),
                [w * 0.45, h * 0.32],
                -PI * 0.9,
                PI * 1.48,
                true,
            );
            add(p, false, 1.0, 1.1);
            let mut p = ShapePath::default();
            p.ellipse_arc(
                pt(0.5, 0.5),
                [w * 0.32, h * 0.45],
                -PI * 0.7,
                PI * 1.4,
                true,
            );
            add(p, false, 0.55, 0.9);
            let mut p = ShapePath::default();
            p.ellipse_arc(pt(0.86, 0.42), [w * 0.04, h * 0.04], 0.0, TAU, true);
            p.close();
            add(p, true, 0.85, 0.0);
        }
        Sparkle => {
            p.m(pt(0.5, 0.02));
            p.c(pt(0.53, 0.32), pt(0.62, 0.46), pt(0.98, 0.5));
            p.c(pt(0.66, 0.55), pt(0.55, 0.65), pt(0.5, 0.98));
            p.c(pt(0.45, 0.65), pt(0.34, 0.55), pt(0.02, 0.5));
            p.c(pt(0.34, 0.46), pt(0.47, 0.32), pt(0.5, 0.02));
            p.close();
            add(p, true, 1.0, 0.0);
        }
        Blossom => {
            let polar = |a: f32, r: f32| pt(0.5 + 0.48 * r * a.cos(), 0.5 + 0.48 * r * a.sin());
            p.m(polar(-PI * 0.5 - PI / 5.0, 0.26));
            for i in 0..5 {
                let a = -PI * 0.5 + i as f32 * TAU / 5.0;
                p.c(
                    polar(a - PI / 6.0, 0.55),
                    polar(a - PI / 8.0, 1.0),
                    polar(a - PI / 18.0, 0.98),
                );
                p.q(polar(a - PI / 36.0, 1.0), polar(a, 0.73));
                p.q(polar(a + PI / 36.0, 1.0), polar(a + PI / 18.0, 0.98));
                p.c(
                    polar(a + PI / 8.0, 1.0),
                    polar(a + PI / 6.0, 0.55),
                    polar(a + PI / 5.0, 0.26),
                );
            }
            p.close();
            add(p, true, 1.0, 0.0);
            let mut p = ShapePath::default();
            for i in 0..5 {
                let a = -PI * 0.5 + i as f32 * TAU / 5.0;
                p.m(polar(a, 0.12));
                p.l(polar(a, 0.43));
            }
            add(p, false, 0.3, 0.8);
        }
        Petal => {
            p.m(pt(0.05, 0.92));
            p.c(pt(0.0, 0.37), pt(0.36, -0.02), pt(0.9, 0.08));
            p.q(pt(1.0, 0.35), pt(0.85, 0.46));
            p.c(pt(0.74, 0.85), pt(0.3, 1.0), pt(0.05, 0.92));
            p.close();
            add(p, true, 1.0, 0.0);
        }
        Flourish => {
            p.m(pt(0.05, 0.75));
            p.c(pt(0.2, 0.05), pt(0.46, 0.1), pt(0.5, 0.52));
            p.c(pt(0.54, 0.1), pt(0.8, 0.05), pt(0.95, 0.75));
            p.m(pt(0.23, 0.76));
            p.c(pt(0.36, 0.95), pt(0.48, 0.77), pt(0.5, 0.52));
            p.c(pt(0.52, 0.77), pt(0.64, 0.95), pt(0.77, 0.76));
            add(p, false, 1.0, 1.2);
            for side in [-1.0, 1.0] {
                let mut p = ShapePath::default();
                let t = |x: f32, y: f32| pt(0.5 + side * x, y);
                p.m(t(0.05, 0.48));
                p.c(t(0.08, 0.12), t(0.31, 0.15), t(0.33, 0.31));
                p.c(t(0.22, 0.43), t(0.13, 0.42), t(0.05, 0.48));
                p.close();
                add(p, true, 0.8, 0.0);
            }
        }
        Rays => {
            for i in 0..7 {
                let a = (-95.0 + i as f32 * 17.0).to_radians();
                let start = 0.35 + (i % 3) as f32 * 0.05;
                p.m(pt(0.5 + start * a.cos(), 0.92 + start * a.sin()));
                p.l(pt(0.5 + 0.48 * a.cos(), 0.92 + 0.78 * a.sin()));
            }
            add(p, false, 1.0, 1.3);
        }
        Halftone => {
            for row in 0..5 {
                for col in 0..(5 - row) {
                    let x = 0.12 + col as f32 * 0.18;
                    let y = 0.12 + row as f32 * 0.18;
                    p.ellipse_arc(pt(x, y), [w * 0.022, h * 0.022], 0.0, TAU, true);
                    p.close();
                }
            }
            add(p, true, 1.0, 0.0);
        }
        Fold => {
            p.m(pt(0.05, 0.05));
            p.l(pt(0.95, 0.05));
            p.l(pt(0.95, 0.95));
            p.q(pt(0.63, 0.58), pt(0.05, 0.05));
            p.close();
            add(p, true, 0.55, 0.0);
            let mut p = ShapePath::default();
            p.m(pt(0.05, 0.05));
            p.q(pt(0.63, 0.58), pt(0.95, 0.95));
            add(p, false, 0.9, 0.8);
        }
        Circuit => {
            p.m(pt(0.06, 0.85));
            p.l(pt(0.06, 0.3));
            p.l(pt(0.25, 0.3));
            p.l(pt(0.4, 0.15));
            p.l(pt(0.82, 0.15));
            p.m(pt(0.4, 0.55));
            p.l(pt(0.7, 0.55));
            p.l(pt(0.88, 0.35));
            add(p, false, 1.0, 1.2);
            let mut p = ShapePath::default();
            p.ellipse_arc(pt(0.86, 0.15), [w * 0.04, h * 0.07], 0.0, TAU, true);
            p.close();
            add(p, true, 1.0, 0.0);
        }
    }
    out
}
