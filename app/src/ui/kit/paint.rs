//! The call to action's lit fill: NanaType's one look a style cannot express
//! (cards and keycaps are NanaUI's own painters). It reads the installed
//! theme while painting, so a light/dark switch restyles it in place.

use nana_ui::runtime::{Gradient, PaintContext, PaintPath, PaintShadow, Painter};
use nana_ui_core::{RadiusTier, SemanticColorRole, ThemeAppearance};
use nanatype_model::Rgba8;

use crate::skin_theme::{contrast, mix, on, rgba};

/// `rgba` with its alpha replaced.
fn alpha(rgba: [f32; 4], a: f32) -> [f32; 4] {
    [rgba[0], rgba[1], rgba[2], a]
}

/// The right end of a call to action's fill (accent → accent.light): up to
/// 22% toward white, as far as the label keeps 4.5:1 on it.
fn accent_light(accent: [f32; 4]) -> [f32; 4] {
    let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
    let base = Rgba8::rgb(q(accent[0]), q(accent[1]), q(accent[2]));
    let label = on(base);
    let end = (0..=11)
        .rev()
        .map(|i| mix(base, Rgba8::WHITE, i as f32 * 0.02))
        .find(|c| contrast(label, *c) >= 4.5)
        .unwrap_or(base);
    alpha(rgba(end), accent[3])
}

/// The view's one primary action: a solid accent fill lightening to the
/// right (accent → accent.light at 90°) on an accent-tinted
/// `0 3px 8px accent@33%` shadow, its label in the on-accent colour.
pub struct CtaPainter;

impl Painter for CtaPainter {
    fn paint(&self, cx: &mut PaintContext<'_>) {
        let state = cx.state();
        if state.disabled {
            cx.draw_default();
            return;
        }
        let rect = cx.bounds();
        let r = cx.radius(RadiusTier::Sm);
        let mut path = PaintPath::new();
        path.rounded_rect(rect, [r; 4]);
        let role = if state.hovered || state.pressed {
            SemanticColorRole::AccentStrong
        } else {
            SemanticColorRole::Accent
        };
        let accent = cx.color(role);
        let dark = cx.theme_appearance() == ThemeAppearance::Dark;
        cx.shadow(
            &path,
            PaintShadow::Custom {
                color: alpha(accent, if dark { 0.25 } else { 0.33 }).into(),
                offset: [0.0, 3.0],
                blur: if dark { 10.0 } else { 8.0 },
                spread: 0.0,
                inset: false,
            },
        );
        let fill = Gradient::linear([rect.x, rect.y], [rect.x + rect.width, rect.y])
            .stop(0.0, accent)
            .stop(1.0, accent_light(accent));
        cx.fill_path(&path, fill);
        // The framework's own look over it: the label and the focus ring
        // (the node itself has no fill of its own).
        cx.draw_default();
    }

    fn paint_key(&self) -> u64 {
        1
    }
}
