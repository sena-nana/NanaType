//! With skin motion off, a frozen program is shaded once into a rest frame
//! and copied from then on, pixel for pixel what shading it again would draw.

mod common;

use nana_ui::runtime::Stack;
use nana_ui::runtime::view::{AnyView, IntoView, widget};
use nana_ui_core::{LengthSpec, PositionSpec};
use nana_ui_devtools::agent::AgentSession;
use nanatype::ui::{Ui, dialogue::Source, shader_effects, surface::Surface, sweep};
use nanatype_theme::{ShaderSpec, ShaderTarget, Skin, validate_shader};
use std::sync::Arc;

const W: u32 = 260;
const H: u32 = 120;

fn skin() -> Arc<Skin> {
    let mut skin = Skin::default();
    let program = validate_shader(
        "fn effect(i:EffectInput)->vec4<f32>{\
           let w = 0.5 + 0.5 * sin(i.phase * 6.2831853 + i.uv.x * 9.);\
           return vec4(i.uv.x, w, i.uv.y, 0.55 + 0.45 * i.uv.y);}",
    )
    .unwrap();
    skin.shader_programs
        .insert("wave".into(), Arc::new(program));
    skin.shaders.insert(
        "wave".into(),
        ShaderSpec {
            intensity: 0.9,
            static_fallback: true,
            ..Default::default()
        },
    );
    Arc::new(skin)
}

fn boxed(effect: AnyView, left: f32) -> AnyView {
    widget(
        Stack::row(0.)
            .width(LengthSpec::Px(100.))
            .height(LengthSpec::Px(70.))
            .with_layout(move |l| {
                l.position = PositionSpec::Absolute;
                l.offset_left = Some(LengthSpec::Px(left));
                l.offset_top = Some(LengthSpec::Px(25.));
            }),
    )
    .children(effect)
    .into_any()
}

fn max_diff(a: &[u8], b: &[u8]) -> u8 {
    a.iter()
        .zip(b)
        .map(|(x, y)| x.abs_diff(*y))
        .max()
        .unwrap_or(0)
}

#[test]
fn frozen_skins_reuse_one_rest_frame_that_matches_direct_shading() {
    let skin = skin();
    let ui = Ui::new(
        Arc::new(nanatype_settings::Settings::default()),
        skin.clone(),
        nanatype_i18n::Locale::ZhCn,
    );
    ui.display.update(|v| Arc::make_mut(v).empty = false);
    // A rotated, rounded background lands on sub-pixel origins; a complete
    // artwork covers its whole canvas.
    let background = shader_effects::view(
        *ui,
        Source::Display,
        &skin,
        "wave",
        Surface {
            fill: Some([1.; 4]),
            radii: [18.; 4],
            rotate_deg: 7.,
            ..Default::default()
        },
        ShaderTarget::Background,
    )
    .unwrap();
    let artwork = shader_effects::view(
        *ui,
        Source::Display,
        &skin,
        "wave",
        Surface::default(),
        ShaderTarget::Artwork,
    )
    .unwrap();
    let mut session = common::session().scale(1.25).mount(W, H, move || {
        widget(
            Stack::column(0.)
                .width(LengthSpec::Px(W as f32))
                .height(LengthSpec::Px(H as f32))
                .with_layout(|l| l.position = PositionSpec::Relative),
        )
        .children((boxed(background, 15.), boxed(artwork, 145.)))
    });
    session.set_gpu_renderers(sweep::renderers());
    session.set_clear(Some([0.; 4]));
    let mut frame =
        |now: u64| common::frame(&mut session, ui, now, now, "rest", "NANATYPE_REST_FRAMES");
    common::settle_shaders(|| {
        frame(1000);
    });

    // Playing: time moves the picture and nothing is held at rest.
    assert!(shader_effects::enabled(*ui));
    assert_ne!(frame(1000), frame(3000), "the program should play");
    assert_eq!(shader_effects::cache_stats().frozen, 0);

    // Motion off: one rest frame per material, reused at any time.
    ui.settings.update(|s| Arc::make_mut(s).skin.motion = false);
    assert!(!shader_effects::enabled(*ui));
    let rest = frame(1000);
    assert_eq!(shader_effects::cache_stats().frozen, 2);
    let compiled = shader_effects::cache_stats().compilations;
    assert_eq!(frame(5000), rest, "a rest frame must not change");
    assert_eq!(frame(9000), rest);
    let stats = shader_effects::cache_stats();
    assert_eq!((stats.frozen, stats.compilations), (2, compiled));
    assert!(
        rest.as_chunks::<4>().0.iter().any(|p| p[3] > 200),
        "nothing drawn"
    );

    // The copy is what shading the frozen program on every draw gives.
    shader_effects::set_rest_frames(false);
    let direct = frame(1000);
    assert_eq!(shader_effects::cache_stats().frozen, 0);
    shader_effects::set_rest_frames(true);
    assert!(
        max_diff(&rest, &direct) <= 1,
        "rest frame differs from direct shading by {}",
        max_diff(&rest, &direct)
    );

    // Motion back on: it plays again and the rest frames are released.
    ui.settings.update(|s| Arc::make_mut(s).skin.motion = true);
    assert!(shader_effects::enabled(*ui));
    assert_ne!(frame(1000), frame(3000), "the program should play again");
    assert_eq!(shader_effects::cache_stats().frozen, 0);
    assert!(shader_effects::take_notices().is_empty());
}
