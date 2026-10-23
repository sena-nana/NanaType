//! Runtime protection of skin shaders on the real GPU proxy: a program over
//! the GPU budget is frozen, then disabled; one that was on trial when the
//! last run ended stays blocked; turning effects back on retries both. Skin
//! materials, which NanaUI draws, fall back to native paint the same way.

mod common;

use nana_ui::runtime::Stack;
use nana_ui::runtime::view::widget;
use nana_ui_core::{LengthSpec, PositionSpec};
use nana_ui_devtools::agent::{AgentSession, RuntimeAgentSession};
use nanatype::{
    clock,
    ui::{Ui, dialogue::Source, materials, shader_effects, surface::Surface, sweep},
};
use nanatype_theme::{
    MaterialSpec, ShaderEffect, ShaderSpec, ShaderTarget, Skin, validate_material, validate_shader,
};
use std::{sync::Arc, time::Duration};

/// A skin with one animated program that paints `rgb` opaquely, with a
/// frozen fallback so a throttled program still draws.
fn painting(rgb: &str) -> Arc<Skin> {
    let mut skin = Skin::default();
    let program = validate_shader(&format!(
        "fn effect(i: EffectInput) -> vec4<f32> {{ return vec4({rgb}, 1.); }}"
    ))
    .unwrap();
    skin.shader_programs.insert("p".into(), Arc::new(program));
    skin.shaders.insert(
        "p".into(),
        ShaderSpec {
            intensity: 1.,
            static_fallback: true,
            ..Default::default()
        },
    );
    skin.dialogue.shader_effects.push(ShaderEffect {
        shader: "p".into(),
        target: ShaderTarget::Background,
    });
    Arc::new(skin)
}

fn mount(skin: &Arc<Skin>) -> (Ui, RuntimeAgentSession) {
    let ui = Ui::new(
        Arc::new(nanatype_settings::Settings::default()),
        skin.clone(),
        nanatype_i18n::Locale::ZhCn,
    );
    ui.display.update(|v| Arc::make_mut(v).empty = false);
    let effect = shader_effects::view(
        *ui,
        Source::Display,
        skin,
        "p",
        Surface {
            fill: Some([1.; 4]),
            ..Default::default()
        },
        ShaderTarget::Background,
    )
    .unwrap();
    let mut session = common::session().mount(120, 80, move || {
        widget(
            Stack::column(0.)
                .width(LengthSpec::Px(120.))
                .height(LengthSpec::Px(80.))
                .with_layout(|l| l.position = PositionSpec::Relative),
        )
        .children(effect)
    });
    session.set_gpu_renderers(sweep::renderers());
    session.set_clear(Some([0.; 4]));
    common::settle_shaders(|| {
        let _ = session.screenshot_rgba().unwrap();
    });
    (ui, session)
}

/// A skin whose material `m` paints `rgb` opaquely.
fn material_skin(rgb: &str) -> Arc<Skin> {
    let mut skin = Skin::default();
    let source =
        format!("fn material(i: MaterialInput) -> vec4<f32> {{ return vec4({rgb}, 1.); }}");
    let program = validate_material(&source).unwrap();
    skin.material_programs.insert("m".into(), Arc::new(program));
    skin.materials.insert(
        "m".into(),
        MaterialSpec {
            source,
            ..Default::default()
        },
    );
    Arc::new(skin)
}

/// A blue box under the skin's material `m`.
fn mount_material(skin: &Skin) -> RuntimeAgentSession {
    let material = materials::resolve(skin, Some("m"));
    let mut session = common::session().mount(120, 80, move || {
        widget(
            Stack::column(0.)
                .width(LengthSpec::Px(120.))
                .height(LengthSpec::Px(80.))
                .with_layout(|l| {
                    l.background = Some([0., 0., 1., 1.]);
                    l.paint.background_material = material.clone();
                }),
        )
    });
    // Installs the material policy too.
    session.set_gpu_renderers(sweep::renderers());
    session.set_clear(Some([0.; 4]));
    session
}

fn native(pixel: [u8; 4]) -> bool {
    pixel[2] > 240 && pixel[0] < 10 && pixel[1] < 10
}

/// The centre pixel of a fresh frame.
fn centre(session: &mut RuntimeAgentSession) -> [u8; 4] {
    let (_, rgba) = session.screenshot_rgba().unwrap();
    common::pixel(&rgba, 120, 60, 40)
}

/// Past the end of the current budget window.
fn next_window() {
    std::thread::sleep(Duration::from_millis(1100));
}

fn notices_mention(text: &str) -> bool {
    shader_effects::take_notices()
        .iter()
        .any(|n| n.contains(text))
}

// One test: the renderer's budget, guard and notices are process-wide.
#[test]
fn over_budget_and_crashed_programs_stop_drawing_until_effects_are_retried() {
    clock::capture_at(3000, || {
        // A budget every draw exceeds.
        let skin = painting("1., 0., 0.");
        let previous = shader_effects::set_cost_budget(1);
        let (ui, mut session) = mount(&skin);
        shader_effects::take_notices();
        assert!(centre(&mut session)[0] > 240, "the program never drew");
        assert!(shader_effects::animating(*ui));

        next_window();
        let _ = session.screenshot_rgba().unwrap();
        assert!(notices_mention("frozen"), "over budget but not frozen");
        assert!(
            !shader_effects::animating(*ui),
            "a frozen program still asks for frames"
        );
        assert!(
            centre(&mut session)[0] > 240,
            "the frozen fallback stopped drawing"
        );

        next_window();
        let _ = session.screenshot_rgba().unwrap();
        assert!(
            notices_mention("even when frozen"),
            "frozen and over budget but not disabled"
        );
        assert_eq!(centre(&mut session)[3], 0, "a disabled program still draws");

        shader_effects::set_cost_budget(previous);
        shader_effects::retry_disabled();
        assert!(
            centre(&mut session)[0] > 240,
            "retrying did not restore the program"
        );
        assert!(shader_effects::animating(*ui));
        assert!(shader_effects::take_notices().is_empty());
        drop(session);

        // The crash guard: a program on trial when the process died.
        let dir = tempfile::tempdir().unwrap();
        let record = dir.path().join("shader-guard.json");
        let read = || std::fs::read_to_string(&record).unwrap_or_default();
        shader_effects::init_guard(record.clone());
        let skin = painting("0., 1., 0.");
        let hash = skin.shader_programs["p"].hash.clone();
        let (_, mut session) = mount(&skin);
        assert!(centre(&mut session)[1] > 240, "the program never drew");
        assert!(read().contains(&hash), "first draws were not written down");

        // Relaunch without a clean exit.
        shader_effects::init_guard(record.clone());
        assert!(notices_mention("was running when"));
        assert_eq!(
            centre(&mut session)[3],
            0,
            "a crash-blocked program still draws"
        );

        shader_effects::retry_disabled();
        assert!(
            centre(&mut session)[1] > 240,
            "retrying did not unblock the program"
        );
        shader_effects::close_guard();
        let after: serde_json::Value = serde_json::from_str(&read()).unwrap();
        assert_eq!(
            after,
            serde_json::json!({ "trial": [], "blocked": [] }),
            "a clean exit leaves nothing on trial"
        );
        // A clean exit is not a crash: the next launch blocks nothing.
        shader_effects::init_guard(record);
        assert!(centre(&mut session)[1] > 240);
        assert!(shader_effects::take_notices().is_empty());
        drop(session);

        // A material over the budget has nothing to freeze: it is disabled.
        // A fresh window, without the effects' spending.
        next_window();
        let skin = material_skin("1., 0., 0.");
        let previous = shader_effects::set_cost_budget(1);
        let mut session = mount_material(&skin);
        assert!(centre(&mut session)[0] > 240, "the material never drew");
        shader_effects::take_notices();
        next_window();
        let _ = session.screenshot_rgba().unwrap();
        assert!(
            notices_mention("disabled: over the GPU budget"),
            "over budget but not disabled"
        );
        let pixel = centre(&mut session);
        assert!(native(pixel), "a disabled material still draws: {pixel:?}");
        shader_effects::set_cost_budget(previous);
        shader_effects::retry_disabled();
        assert!(
            centre(&mut session)[0] > 240,
            "retrying did not restore the material"
        );
        drop(session);

        // A material on trial when the process died stays blocked.
        let dir = tempfile::tempdir().unwrap();
        let record = dir.path().join("shader-guard.json");
        let read = || std::fs::read_to_string(&record).unwrap_or_default();
        shader_effects::init_guard(record.clone());
        let skin = material_skin("0., 1., 0.");
        let hash = skin.material_programs["m"].hash.clone();
        let mut session = mount_material(&skin);
        assert!(centre(&mut session)[1] > 240, "the material never drew");
        assert!(read().contains(&hash), "first draws were not written down");
        shader_effects::init_guard(record.clone());
        assert!(notices_mention("was running when"));
        let pixel = centre(&mut session);
        assert!(
            native(pixel),
            "a crash-blocked material still draws: {pixel:?}"
        );
        shader_effects::retry_disabled();
        assert!(
            centre(&mut session)[1] > 240,
            "retrying did not unblock the material"
        );
        shader_effects::close_guard();
    });
}
