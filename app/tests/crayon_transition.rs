//! Check the authored brush masks through the production material pipeline.
use nana_ui::runtime::view::widget;
use nana_ui::runtime::{DocumentId, RuntimeDocument, Stack};
use nana_ui_core::{LengthSpec, MaterialProgram, MaterialRef};
use nana_ui_devtools::agent::{AgentSession, RuntimeAgentSession};
use std::sync::Arc;

fn capture(source: &str, progress: f32, scale: f32) -> Vec<u8> {
    let source = format!(
        "{}\nfn material(i:MaterialInput)->vec4<f32>{{var sample=i;sample.progress=material_param(0u).x;return brush(sample);}}",
        source.replace("fn material(", "fn brush(")
    );
    nanatype_theme::validate_material(&source).unwrap();
    let material = MaterialRef::new(MaterialProgram::new(source))
        .with_parameters(Arc::from([[progress, 0., 0., 0.]]));
    let id = DocumentId::new(1).unwrap();
    let mut doc = RuntimeDocument::new(id);
    doc.context_mut()
        .mount_view_root(id, move || {
            widget(
                Stack::row(0.)
                    .width(LengthSpec::Px(160.))
                    .height(LengthSpec::Px(80.))
                    .with_layout(move |layout| {
                        layout.background = Some([1.; 4]);
                        layout.paint.background_material = Some(material);
                    }),
            )
        })
        .unwrap();
    let mut session = RuntimeAgentSession::new_scaled(doc, 160, 80, scale).unwrap();
    session.set_clear(Some([0.; 4]));
    session.screenshot_rgba().unwrap().1
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
            for (progress, visible) in [(0., !entering), (1., entering)] {
                let image = capture(source, progress, scale);
                assert!(
                    image
                        .chunks_exact(4)
                        .all(|p| p[3] == if visible { 255 } else { 0 })
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
