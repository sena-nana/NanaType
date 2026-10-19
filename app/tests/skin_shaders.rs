//! Exercise the real GPU proxy, including asynchronous compilation, masking,
//! texture decoding, dynamic settings and unchanged static fallback.

mod common;

use nana_ui::runtime::Stack;
use nana_ui::runtime::view::widget;
use nana_ui_core::{LengthSpec, PositionSpec};
use nana_ui_devtools::agent::AgentSession;
use nanatype::{
    clock,
    ui::{Ui, dialogue::Source, shader_effects, surface::Surface, sweep},
};
use nanatype_theme::{ShaderSpec, ShaderTarget, ShaderTexture, Skin, validate_shader};
use std::sync::Arc;

#[test]
fn gpu_proxy_samples_masks_and_disables_without_mutating_the_scene() {
    clock::capture_at(3000, || {
        let mut skin = Skin::default();
        let mut program =
            validate_shader("fn effect(i:EffectInput)->vec4<f32>{return sample0(i.uv);}").unwrap();
        program.textures.push(ShaderTexture {
            size: [1, 1],
            bytes: Arc::from([255u8, 0, 0, 255]),
        });
        skin.shader_programs
            .insert("test".into(), Arc::new(program));
        skin.shaders.insert(
            "test".into(),
            ShaderSpec {
                intensity: 1.,
                ..Default::default()
            },
        );
        let skin = Arc::new(skin);
        let ui = Ui::new(
            Arc::new(nanatype_settings::Settings::default()),
            skin.clone(),
            nanatype_i18n::Locale::ZhCn,
        );
        ui.display.update(|v| Arc::make_mut(v).empty = false);
        let surface = Surface {
            fill: Some([1.; 4]),
            radii: [22.; 4],
            ..Default::default()
        };
        let effect = shader_effects::view(
            ui,
            Source::Display,
            &skin,
            "test",
            surface,
            ShaderTarget::Background,
        )
        .unwrap();
        let mut session = common::session().mount(180, 100, move || {
            widget(Stack::column(0.).padding(10.)).children(
                widget(
                    Stack::row(0.)
                        .width(LengthSpec::Px(160.))
                        .height(LengthSpec::Px(80.))
                        .with_layout(|l| l.position = PositionSpec::Relative),
                )
                .children(effect),
            )
        });
        session.set_gpu_renderers(sweep::renderers());
        session.set_clear(Some([0.; 4]));
        common::settle_shaders(|| {
            let _ = session.screenshot_rgba().unwrap();
        });
        assert_eq!(
            shader_effects::cache_stats().pending,
            0,
            "shader compilation stalled"
        );
        let (_, rgba) = session.screenshot_rgba().unwrap();
        let pixel = |x: usize, y: usize| &rgba[(y * 180 + x) * 4..(y * 180 + x) * 4 + 4];
        assert!(
            pixel(80, 40)[0] > 240 && pixel(80, 40)[1] < 10 && pixel(80, 40)[3] > 240,
            "red texture missing: {:?}; notices: {:?}",
            pixel(80, 40),
            shader_effects::take_notices()
        );
        assert_eq!(pixel(10, 10)[3], 0, "rounded mask leaked");
        let before = shader_effects::cache_stats();
        let _ = session.screenshot_rgba().unwrap();
        let after = shader_effects::cache_stats();
        assert_eq!(
            (before.programs, before.masks, before.bytes),
            (after.programs, after.masks, after.bytes),
            "stable frames allocate GPU resources"
        );
        ui.reduced_motion.set(true);
        let (_, rgba) = session.screenshot_rgba().unwrap();
        assert!(rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 0));
        ui.reduced_motion.set(false);
        ui.settings
            .update(|s| Arc::make_mut(s).skin.advanced_effects = false);
        let (_, rgba) = session.screenshot_rgba().unwrap();
        assert!(rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 0));
        assert!(shader_effects::take_notices().is_empty());
        drop(session);

        // Complete artwork owns its silhouette. A rounded native surface
        // must not silently clip it, and no fallback image is a shader input.
        ui.settings
            .update(|s| Arc::make_mut(s).skin.advanced_effects = true);
        let artwork = shader_effects::view(
            ui,
            Source::Display,
            &skin,
            "test",
            Surface {
                radii: [35.; 4],
                ..Default::default()
            },
            ShaderTarget::Artwork,
        )
        .unwrap();
        let mut session = common::session().mount(180, 100, move || {
            widget(
                Stack::column(0.)
                    .width(LengthSpec::Px(180.))
                    .height(LengthSpec::Px(100.))
                    .with_layout(|l| l.position = PositionSpec::Relative),
            )
            .children(artwork)
        });
        session.set_gpu_renderers(sweep::renderers());
        session.set_clear(Some([0.; 4]));
        common::settle_shaders(|| {
            let _ = session.screenshot_rgba().unwrap();
        });
        let (_, rgba) = session.screenshot_rgba().unwrap();
        assert!(rgba[3] > 240, "artwork inherited native corner clipping");
        let prepared = shader_effects::cache_stats();
        for (width, height, dpi) in [(240, 160, 1.5), (300, 180, 2.), (180, 100, 1.)] {
            session.set_viewport(width, height, dpi).unwrap();
            let (_, rgba) = session.screenshot_rgba().unwrap();
            assert!(rgba[3] > 240, "artwork coverage changed at DPI {dpi}");
            let resized = shader_effects::cache_stats();
            assert_eq!(
                (prepared.programs, prepared.masks),
                (resized.programs, resized.masks),
                "artwork resized its constant coverage texture"
            );
            assert!(
                resized.bytes <= prepared.bytes + 3 * 176,
                "resize grew more than the per-output parameter bindings"
            );
        }
        ui.reduced_motion.set(true);
        let (_, rgba) = session.screenshot_rgba().unwrap();
        assert!(rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 0));
        drop(session);

        // An invalid module simulates a backend compiler rejection after the
        // import validator. It must neither crash nor retry on every frame.
        let mut bad = (*skin).clone();
        let mut program = (*bad.shader_programs["test"]).clone();
        program.hash = "backend-failure-fixture".into();
        program.source = "intentionally invalid WGSL".into();
        bad.shader_programs.insert("test".into(), Arc::new(program));
        bad.dialogue
            .shader_effects
            .push(nanatype_theme::ShaderEffect {
                shader: "test".into(),
                target: ShaderTarget::Background,
            });
        let bad = Arc::new(bad);
        let bad_ui = Ui::new(
            Arc::new(nanatype_settings::Settings::default()),
            bad.clone(),
            nanatype_i18n::Locale::ZhCn,
        );
        bad_ui.display.update(|v| Arc::make_mut(v).empty = false);
        let effect = shader_effects::view(
            bad_ui,
            Source::Display,
            &bad,
            "test",
            Surface {
                fill: Some([1.; 4]),
                ..Default::default()
            },
            ShaderTarget::Background,
        )
        .unwrap();
        let mut session = common::session().scale(1.5).mount(180, 100, move || {
            widget(Stack::fill_column(0.)).children(effect)
        });
        session.set_gpu_renderers(sweep::renderers());
        session.set_clear(Some([0.; 4]));
        common::settle_shaders(|| {
            let _ = session.screenshot_rgba().unwrap();
        });
        let (_, rgba) = session.screenshot_rgba().unwrap();
        assert!(rgba.as_chunks::<4>().0.iter().all(|p| p[3] == 0));
        assert_eq!(shader_effects::take_notices().len(), 1);
        assert!(!shader_effects::animating(bad_ui));
        let _ = session.screenshot_rgba().unwrap();
        assert!(shader_effects::take_notices().is_empty());
    });
}
