//! Check the authored brush masks through the production material pipeline.

mod common;

use nana_ui::runtime::Stack;
use nana_ui::runtime::view::widget;
use nana_ui_core::{LengthSpec, MaterialProgram, MaterialRef};
use nana_ui_devtools::agent::AgentSession;
use std::sync::Arc;

fn capture(source: &str, progress: f32, scale: f32) -> Vec<u8> {
    let source = format!(
        "{}\nfn material(i:MaterialInput)->vec4<f32>{{var sample=i;sample.progress=material_param(0u).x;return brush(sample);}}",
        source.replace("fn material(", "fn brush(")
    );
    nanatype_theme::validate_material(&source).unwrap();
    let material = MaterialRef::new(MaterialProgram::new(source))
        .with_parameters(Arc::from([[progress, 0., 0., 0.]]));
    render(Some(material), scale)
}

/// The 160×80 opaque white quad, shaded by `material` when there is one.
fn render(material: Option<MaterialRef>, scale: f32) -> Vec<u8> {
    let mut session = common::session().scale(scale).mount(160, 80, move || {
        widget(
            Stack::row(0.)
                .width(LengthSpec::Px(160.))
                .height(LengthSpec::Px(80.))
                .with_layout(move |layout| {
                    layout.background = Some([1.; 4]);
                    layout.paint.background_material = material;
                }),
        )
    });
    session.set_clear(Some([0.; 4]));
    session.screenshot_rgba().unwrap().1
}

/// Every pixel of a 160×80 logical capture has the alpha `expected` has there;
/// otherwise report how many do not and where they are.
fn assert_alpha(image: &[u8], expected: &[u8], scale: f32, case: &str) {
    let width = (160. * scale) as usize;
    let wrong: Vec<_> = image
        .as_chunks::<4>()
        .0
        .iter()
        .zip(expected.as_chunks::<4>().0)
        .enumerate()
        .filter(|(_, (p, e))| p[3] != e[3])
        .map(|(i, (p, e))| (i % width, i / width, p[3], e[3]))
        .collect();
    if wrong.is_empty() {
        return;
    }
    let span = |pick: fn(&(usize, usize, u8, u8)) -> usize| {
        let values = wrong.iter().map(pick);
        (values.clone().min().unwrap(), values.max().unwrap())
    };
    panic!(
        "{case}: {} of {} pixels differ in alpha (alpha {:?}..={:?} where {:?}..={:?} was expected, x {:?}, y {:?})",
        wrong.len(),
        image.len() / 4,
        wrong.iter().map(|p| p.2).min().unwrap(),
        wrong.iter().map(|p| p.2).max().unwrap(),
        wrong.iter().map(|p| p.3).min().unwrap(),
        wrong.iter().map(|p| p.3).max().unwrap(),
        span(|p| p.0),
        span(|p| p.1),
    );
}

#[test]
fn materials_replace_the_native_fill_on_this_adapter() {
    // A rejected material is drawn as the plain opaque background. Telling
    // that apart from a brush mask that is merely wrong keeps the test below
    // from reporting an adapter problem as a shader one.
    const CLEAR: &str =
        "fn material(i:MaterialInput)->vec4<f32>{return vec4<f32>(i.color.rgb,0.);}";
    for scale in [1., 2.] {
        let image = capture(CLEAR, 0., scale);
        assert_alpha(
            &image,
            &vec![0; image.len()],
            scale,
            &format!("transparent material at scale {scale}"),
        );
    }
}

#[test]
fn brush_edge_slants_bottom_left_to_top_right_and_moves_down_right() {
    for (source, entering) in [
        (
            include_str!("../../crates/theme/builtin/crayon/write.wgsl"),
            true,
        ),
        (
            include_str!("../../crates/theme/builtin/crayon/erase.wgsl"),
            false,
        ),
    ] {
        for scale in [1., 2.] {
            // The host, not the brush, owns the quad's edge coverage, and it
            // is not exactly 1 on every adapter: Windows CI's WARP left two
            // corners of the scale-2 quad at alpha 254. A finished brush
            // matches the unshaded quad exactly; an untouched one is
            // transparent everywhere.
            let quad = render(None, scale);
            for (progress, visible) in [(0., !entering), (1., entering)] {
                let image = capture(source, progress, scale);
                let name = if entering { "write" } else { "erase" };
                assert_alpha(
                    &image,
                    &if visible {
                        quad.clone()
                    } else {
                        vec![0; quad.len()]
                    },
                    scale,
                    &format!("{name} at progress {progress}, scale {scale}"),
                );
            }
            let image = capture(source, 0.5, scale);
            let width = (160. * scale) as usize;
            let alpha = |x: usize, y: usize| {
                image[(((y as f32 * scale) as usize * width + (x as f32 * scale) as usize) * 4) + 3]
            };
            // The completed side is upper left; lower right is still untouched.
            assert_eq!(alpha(10, 10) > 127, entering);
            assert_eq!(alpha(150, 70) > 127, !entering);
            let crossing = |y| (1..159).find(|&x| (alpha(x, y) < 128) == entering).unwrap() as i32;
            let upper = crossing(20);
            let lower = crossing(60);
            // A physical 45-degree edge shifts left by 40px when moving down 40px,
            // including on this non-square target and at either display scale.
            assert!((upper - 100).abs() <= 4, "upper crossing: {upper}");
            assert!((lower - 60).abs() <= 4, "lower crossing: {lower}");
            assert!((upper - lower - 40).abs() <= 5);
        }
    }
}
