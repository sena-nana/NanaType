//! Real GPU coverage for transparent vector frames, tails and retained caches.

mod common;

use nana_ui::runtime::Stack;
use nana_ui::runtime::view::{signal, widget};
use nana_ui_core::LengthSpec;
use nana_ui_devtools::agent::AgentSession;
use nanatype::ui::surface::{Surface, gradient};
use nanatype_model::Rgba8;
use nanatype_theme::{ContourPreset, GradientSpec};
use std::sync::Arc;

/// The pixel at logical `(x, y)` of a capture taken at `scale`.
fn pixel(pixels: &[u8], width: u32, x: f32, y: f32, scale: f32) -> [u8; 4] {
    common::pixel(
        pixels,
        width,
        (x * scale).floor() as u32,
        (y * scale).floor() as u32,
    )
}

#[test]
fn vector_alpha_tail_and_same_node_palette_updates_survive_dpi_changes() {
    let registry = common::registry();
    let skin = registry.load("nanatype.bubble").unwrap().skin;
    for scale in [1.0, 1.5, 2.0] {
        let mut frame = Surface::dialogue_box(&skin, None);
        // Exercise a concave outline independently of the built-in art direction.
        frame.contour = Some(ContourPreset::Cloud);
        assert!(frame.integrates_tail());
        let current = signal(Arc::new(frame.clone()));
        let mut session = common::session()
            .themed(false)
            .scale(scale)
            .mount(368, 200, move || {
                widget(Stack::column(0.0).padding(24.0)).children(
                    widget(
                        Stack::row(0.0)
                            .width(LengthSpec::Px(320.0))
                            .height(LengthSpec::Px(120.0)),
                    )
                    .bind(move |node: &mut Stack| {
                        let surface = current.get();
                        let previous = std::mem::replace(node, Stack::row(0.0));
                        *node = surface.stack(previous);
                    }),
                )
            });
        session.set_clear(Some([0.0; 4]));
        let (size, first) = session.screenshot_rgba().unwrap();
        assert_eq!(size.width, (368.0 * scale) as u32);
        assert_eq!(size.height, (200.0 * scale) as u32);
        // A deep top-edge valley is inside the old rectangular background.
        // It must stay transparent even with the frame's original shadow.
        assert!(
            pixel(&first, size.width, 114.0, 30.0, scale)[3] < 32,
            "rectangular residue at {scale}x"
        );
        assert!(pixel(&first, size.width, 184.0, 84.0, scale)[3] > 240);
        // The integrated path must render past its own node's layout bounds.
        assert!(
            (24..344).any(|x| pixel(&first, size.width, x as f32, 152.0, scale)[3] > 220),
            "tail clipped at {scale}x"
        );
        // Same retained node and painter type, new resolved colour.
        let mut blue = frame.clone();
        blue.fill = Some([0.0, 0.25, 1.0, 1.0]);
        blue.gradient = None;
        current.set(Arc::new(blue.clone()));
        let (_, second) = session.screenshot_rgba().unwrap();
        let center = pixel(&second, size.width, 184.0, 84.0, scale);
        assert!(
            center[2] > 220 && center[0] < 20,
            "stale colour cache at {scale}x: {center:?}"
        );
        // Changing only the preset fills the former cloud valley.
        blue.contour = Some(ContourPreset::Comic);
        current.set(Arc::new(blue.clone()));
        let (_, third) = session.screenshot_rgba().unwrap();
        assert!(
            pixel(&third, size.width, 114.0, 30.0, scale)[3] > 220,
            "stale outline cache at {scale}x"
        );
        blue.gradient = Some(gradient(
            &skin,
            &GradientSpec {
                stops: vec![
                    (0.0, Rgba8::rgb(255, 0, 0).into()),
                    (1.0, Rgba8::rgb(255, 0, 0).into()),
                ],
                ..Default::default()
            },
        ));
        current.set(Arc::new(blue.clone()));
        let (_, fourth) = session.screenshot_rgba().unwrap();
        let center = pixel(&fourth, size.width, 184.0, 84.0, scale);
        assert!(
            center[0] > 220 && center[2] < 20,
            "stale gradient cache at {scale}x: {center:?}"
        );
        // Returning to an old manifest must remove the retained custom painter.
        blue.contour = None;
        blue.tail = None;
        blue.radii = [0.0; 4];
        blue.gradient = None;
        blue.shadows.clear();
        current.set(Arc::new(blue));
        let (_, legacy) = session.screenshot_rgba().unwrap();
        assert!(
            pixel(&legacy, size.width, 28.0, 28.0, scale)[3] > 240,
            "retained contour after returning to legacy at {scale}x"
        );
    }
}
