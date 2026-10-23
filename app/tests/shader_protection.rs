//! Runtime protection of skin shaders on the real GPU proxy: a program over
//! the GPU budget is frozen, then disabled; one that was on trial when the
//! last run ended stays blocked; turning effects back on retries both.

mod common;

use nana_ui::runtime::Stack;
use nana_ui::runtime::view::widget;
use nana_ui_core::{LengthSpec, PositionSpec};
use nana_ui_devtools::agent::{AgentSession, RuntimeAgentSession};
use nanatype::{
    clock,
    ui::{Ui, dialogue::Source, shader_effects, surface::Surface, sweep},
};
use nanatype_theme::{ShaderEffect, ShaderSpec, ShaderTarget, Skin, validate_shader};
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
    });
}
