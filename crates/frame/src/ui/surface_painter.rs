//! Retained path painter for all skin surfaces. Geometry lives in theme;
//! this adapter records it once per resolved surface/size/theme, not per frame.
use super::{Surface, SurfaceRoute};
use nana_ui::runtime::{
    FillRule, Gradient, LineCap, LineJoin, PaintContext, PaintPath, PaintShadow, Painter,
    StrokeStyle,
};
use nana_ui_core::{BackgroundImage, CssGradient, LengthSpec};
use nanatype_theme::{BorderStyleSpec, ShapeCommand, ShapePath, motif_paths};
use std::hash::{Hash, Hasher};

pub struct SurfacePainter {
    surface: Surface,
    key: u64,
}
impl SurfacePainter {
    pub fn new(surface: Surface) -> Self {
        // Debug's float representation round-trips all resolved values. Keep
        // this fingerprint at construction so cache lookups allocate nothing.
        // It also includes newly added Surface fields automatically.
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        format!("{surface:?}").hash(&mut hash);
        Self {
            surface,
            key: hash.finish(),
        }
    }
}
pub(super) fn paint_path(shape: &ShapePath) -> PaintPath {
    let mut path = PaintPath::new();
    if shape.even_odd {
        path.set_fill_rule(FillRule::EvenOdd);
    }
    for command in &shape.commands {
        match *command {
            ShapeCommand::Move([x, y]) => {
                path.move_to(x, y);
            }
            ShapeCommand::Line([x, y]) => {
                path.line_to(x, y);
            }
            ShapeCommand::Quad([a, b], [x, y]) => {
                path.quad_to(a, b, x, y);
            }
            ShapeCommand::Cubic([a, b], [c, d], [x, y]) => {
                path.cubic_to(a, b, c, d, x, y);
            }
            ShapeCommand::Close => {
                path.close();
            }
        }
    }
    path
}
fn shadow(cx: &mut PaintContext<'_>, path: &PaintPath, s: &nana_ui_core::BoxShadowSpec) {
    cx.shadow(
        path,
        PaintShadow::Custom {
            color: s.color.into(),
            offset: [s.offset_x, s.offset_y],
            blur: s.blur_radius,
            spread: s.spread_radius,
            inset: s.inset,
        },
    );
}
fn coordinate(length: LengthSpec, axis: f32) -> f32 {
    match length {
        LengthSpec::Px(p) => p,
        LengthSpec::Percent(p) => axis * p / 100.0,
        _ => axis * 0.5,
    }
}
fn gradient_fill(
    cx: &mut PaintContext<'_>,
    path: &PaintPath,
    background: &BackgroundImage,
    size: [f32; 2],
) {
    let [w, h] = size;
    match background {
        BackgroundImage::Gradient(CssGradient::Linear(g)) => {
            let angle = g.angle_deg.to_radians();
            let dir = [angle.sin(), -angle.cos()];
            let half = (dir[0].abs() * w + dir[1].abs() * h) * 0.5;
            let mut grad = Gradient::linear(
                [w * 0.5 - dir[0] * half, h * 0.5 - dir[1] * half],
                [w * 0.5 + dir[0] * half, h * 0.5 + dir[1] * half],
            );
            for s in &g.stops {
                grad = grad.stop(s.position, s.color);
            }
            cx.fill_path(path, grad);
        }
        BackgroundImage::Gradient(CssGradient::Radial(g)) => {
            let center = [coordinate(g.center[0], w), coordinate(g.center[1], h)];
            let dx = center[0].max(w - center[0]).max(0.1);
            let dy = center[1].max(h - center[1]).max(0.1);
            let (rx, ry) = if g.circle {
                let r = dx.hypot(dy);
                (r, r)
            } else {
                (dx * 2.0_f32.sqrt(), dy * 2.0_f32.sqrt())
            };
            // Inverse-transform the outline while transforming the gradient's
            // unit circle. The resulting fill is elliptical, the outline isn't.
            cx.save();
            cx.translate(center[0], center[1]);
            cx.scale(rx, ry);
            let local = path.transformed([
                1.0 / rx,
                0.0,
                0.0,
                1.0 / ry,
                -center[0] / rx,
                -center[1] / ry,
            ]);
            let mut grad = Gradient::radial([0.0, 0.0], 1.0);
            for s in &g.stops {
                grad = grad.stop(s.position, s.color);
            }
            cx.fill_path(&local, grad);
            cx.restore();
        }
        _ => {}
    }
}
fn stroke(
    cx: &mut PaintContext<'_>,
    path: &PaintPath,
    width: f32,
    color: [f32; 4],
    style: BorderStyleSpec,
    join: LineJoin,
) {
    let mut line = StrokeStyle::new(width).join(join).cap(LineCap::Round);
    match style {
        BorderStyleSpec::Dashed => line = line.dash(vec![width * 3.0, width * 2.0], 0.0),
        BorderStyleSpec::Dotted => line = line.dash(vec![0.01, width * 2.0], 0.0),
        _ => {}
    }
    cx.stroke_path(path, line, color);
}
impl SurfacePainter {
    fn surface_path(&self, cx: &mut PaintContext<'_>, path: &PaintPath, size: [f32; 2]) {
        let s = &self.surface;
        for layer in s.shadows.iter().filter(|s| !s.inset) {
            shadow(cx, path, layer);
        }
        if let Some(fill) = s.fill {
            cx.fill_path(path, fill);
        }
        if let Some(gradient) = &s.gradient {
            gradient_fill(cx, path, gradient, size);
        }
        for layer in s.shadows.iter().filter(|s| s.inset) {
            shadow(cx, path, layer);
        }
        if let Some(b) = &s.border {
            let width = b.widths.into_iter().fold(0.0_f32, f32::max);
            let join = if matches!(
                s.contour,
                Some(nanatype_theme::ContourPreset::Rpg | nanatype_theme::ContourPreset::Pixel)
            ) {
                LineJoin::Miter
            } else {
                LineJoin::Round
            };
            if b.widths.iter().all(|v| *v == width) {
                stroke(cx, path, width, b.color, b.style, join);
            } else {
                // Preserve per-side accents and their layout widths; never
                // turn a left-only rule into a border around the whole frame.
                let [w, h] = size;
                for (i, width) in b.widths.into_iter().enumerate().filter(|(_, v)| *v > 0.0) {
                    let band = match i {
                        0 => [-32.0, -32.0, w + 64.0, width + 32.0],
                        1 => [w - width, -32.0, width + 32.0, h + 64.0],
                        2 => [-32.0, h - width, w + 64.0, width + 32.0],
                        _ => [-32.0, -32.0, width + 32.0, h + 64.0],
                    };
                    let [x, y, bw, bh] = band;
                    let mut clip = PaintPath::new();
                    clip.move_to(x, y)
                        .line_to(x + bw, y)
                        .line_to(x + bw, y + bh)
                        .line_to(x, y + bh)
                        .close();
                    cx.save();
                    cx.push_clip(&clip);
                    stroke(cx, path, width, b.color, b.style, join);
                    cx.restore();
                }
            }
        }
    }
}
impl Painter for SurfacePainter {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        let bounds = cx.bounds();
        let size = [bounds.width, bounds.height];
        if size.iter().any(|v| !v.is_finite() || *v <= 0.0) {
            return;
        }
        cx.save();
        cx.translate(bounds.x, bounds.y);
        let s = &self.surface;
        match s.route() {
            SurfaceRoute::Motif(motif) => {
                for part in motif_paths(motif, size) {
                    let path = paint_path(&part.path);
                    cx.save();
                    cx.set_opacity(cx.opacity() * part.opacity);
                    if part.fill {
                        self.surface_path(cx, &path, size);
                    } else {
                        let color = s
                            .border
                            .as_ref()
                            .map(|b| b.color)
                            .or(s.fill)
                            .unwrap_or([0.0; 4]);
                        stroke(
                            cx,
                            &path,
                            part.stroke_width,
                            color,
                            BorderStyleSpec::Solid,
                            LineJoin::Round,
                        );
                    }
                    cx.restore();
                }
            }
            SurfaceRoute::Contour(_) => {
                let path = paint_path(&s.contour_path(size).expect("contour route"));
                self.surface_path(cx, &path, size);
            }
            SurfaceRoute::Frame | SurfaceRoute::Style => {}
        }
        cx.restore();
        // No draw_default(): the path replaces the node's rectangular paint.
        // Children (including names and dialogue text) still paint normally.
    }
    fn paint_key(&self) -> u64 {
        self.key
    }
}
