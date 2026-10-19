//! `NANATYPE_SNAPSHOT_MOTION_FRAMES`: one artwork cycle in a retained scene.

use super::*;

/// Repeatable animation poses and layout stress cases for the theme pass.
pub fn capture(
    ui: Ui,
    out: &std::path::Path,
    w: u32,
    h: u32,
    theme: Arc<nana_ui::CompiledTheme>,
    frames: u32,
) {
    use nanatype::{clock, ui::shader_effects};
    let frames = frames.clamp(4, 512);
    let skin = ui.skin.get_untracked();
    let effect = skin
        .dialogue
        .shader_effects
        .first()
        .expect("artwork effect");
    let period = (skin.shaders[&effect.shader].period_s * 1000.).round() as u64;
    // Object motion follows dialogue status. Freeze the relevant real status
    // for these captures rather than requiring the completed state to animate.
    let status = match skin.meta.id.as_str() {
        "nanatype.livechat" => {
            ui.display.update(|v| {
                let v = Arc::make_mut(v);
                v.reveal.shown /= 2;
                v.reveal.segment = None;
                v.continue_indicator = false;
            });
            "typing"
        }
        "nanatype.rpg" => {
            ui.display
                .update(|v| Arc::make_mut(v).continue_indicator = true);
            "waiting"
        }
        _ => "default",
    };
    let start = period * 8;
    let id = DocumentId::new(1).unwrap();
    ui.now.set(start);
    let mut session = clock::capture_at(start - period, || {
        let mut doc = RuntimeDocument::new(id);
        doc.context_mut().set_theme_tokens(theme).unwrap();
        doc.context_mut()
            .mount_view_root(id, move || {
                dialogue::dialogue_box(ui, dialogue::Source::Display, None)
            })
            .unwrap();
        let mut session = RuntimeAgentSession::new(doc, w, h).unwrap();
        session.set_gpu_renderers(nanatype::ui::sweep::renderers());
        session.set_clear(Some([0.; 4]));
        let _ = session.screenshot_rgba().unwrap();
        let deadline = std::time::Instant::now();
        while shader_effects::cache_stats().pending > 0 && deadline.elapsed().as_secs() < 20 {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(shader_effects::cache_stats().pending, 0);
        // The first successful GPU draw establishes dialogue event timestamps.
        // Do it before the loop starts, after asynchronous compilation finishes.
        let _ = session.screenshot_rgba().unwrap();
        session
    });
    clock::capture_at(start, || {
        for _ in 0..3 {
            let _ = session.screenshot_rgba().unwrap();
        }
    });
    let cache = shader_effects::cache_stats();
    // Capture the real static fallback in the same retained scene. This also
    // supplies the gallery without a second GPU context or a fake redraw.
    let original_settings = ui.settings.get_untracked();
    ui.settings
        .update(|s| Arc::make_mut(s).skin.advanced_effects = false);
    clock::capture_at(start, || {
        session.screenshot_png(out.join("display.png")).unwrap()
    });
    ui.settings.set(original_settings);
    let [left, top, right, bottom] = skin.dialogue.padding;
    let inset = skin.dialogue.overscan;
    let [mt, mr, mb, ml] = skin.dialogue.box_.margin;
    let available_width = w as f32 - inset * 2. - ml - mr;
    let side_space = available_width * (1. - skin.dialogue.box_.width_fraction) * 0.5;
    let reading = [
        (inset + ml + side_space + left).ceil() as u32,
        (inset + mt + top).ceil() as u32,
        (w as f32 - inset - mr - side_space - right).floor() as u32,
        (h as f32 - inset - mb - bottom).floor() as u32,
    ];
    let mut first = Vec::new();
    let mut moving = 0;
    for frame in 0..=frames {
        let (_, rgba) = clock::capture_at(
            start + period * u64::from(frame) / u64::from(frames),
            || session.screenshot_rgba().unwrap(),
        );
        if frame == 0 {
            first = rgba.clone();
        }
        let mut changed = 0;
        for (index, (a, b)) in first
            .as_chunks::<4>()
            .0
            .iter()
            .zip(rgba.as_chunks::<4>().0.iter())
            .enumerate()
        {
            let (x, y) = (index as u32 % w, index as u32 / w);
            assert_eq!(a[3], b[3], "moving outer alpha at frame {frame}, {x},{y}");
            if x >= reading[0] && x < reading[2] && y >= reading[1] && y < reading[3] {
                assert_eq!(a, b, "reading area moved at frame {frame}, {x},{y}");
            }
            changed += usize::from(a != b);
        }
        moving = moving.max(changed);
        let file = if frame == frames {
            "motion-loop.png".into()
        } else {
            format!("motion-{frame:03}.png")
        };
        image::RgbaImage::from_raw(w, h, rgba.clone())
            .unwrap()
            .save(out.join(file))
            .unwrap();
        if frame == frames {
            assert!(
                first == rgba,
                "artwork does not close its loop ({changed} changed pixels)"
            );
        }
    }
    // A quiet status light may occupy only a few dozen pixels; avoid making
    // restrained designs animate a larger area just to satisfy the harness.
    assert!(moving > 16, "no visible ornamental motion");
    // Exercise opening and speaker-change accents in the same scene. Metadata
    // changes trigger the shader events without replacing the reading fixture.
    ui.display.update(|v| {
        let v = Arc::make_mut(v);
        v.line = Some(nanatype_model::LineId::next());
        v.speaker.name = "宁宁".into();
    });
    let event_samples = [0u64, 125, 250, 500, 1000, 2000];
    for offset in event_samples {
        let (_, rgba) = clock::capture_at(start + period * 2 + offset, || {
            session.screenshot_rgba().unwrap()
        });
        for y in reading[1]..reading[3] {
            let a = ((y * w + reading[0]) * 4) as usize;
            let b = ((y * w + reading[2]) * 4) as usize;
            assert!(
                first[a..b] == rgba[a..b],
                "event changed reading pixels at {offset}ms, row {y}"
            );
        }
        image::RgbaImage::from_raw(w, h, rgba)
            .unwrap()
            .save(out.join(format!("event-{offset:04}.png")))
            .unwrap();
    }
    // Status decorations settle when their job is done. Compare complete
    // retained scenes across clock values after changing only that status.
    let resting_status_stable = if matches!(
        skin.meta.id.as_str(),
        "nanatype.livechat" | "nanatype.rpg" | "nanatype.terminal"
    ) {
        ui.display.update(|v| {
            let v = Arc::make_mut(v);
            v.reveal.shown = v.reveal.hidden_from;
            v.reveal.segment = None;
            v.continue_indicator = skin.meta.id == "nanatype.terminal";
        });
        let (_, a) = clock::capture_at(start + period * 4, || session.screenshot_rgba().unwrap());
        let (_, b) = clock::capture_at(start + period * 4 + period / 3, || {
            session.screenshot_rgba().unwrap()
        });
        assert!(a == b, "completed status still moves: {}", skin.meta.id);
        image::RgbaImage::from_raw(w, h, a)
            .unwrap()
            .save(out.join("status-resting.png"))
            .unwrap();
        Some(true)
    } else {
        None
    };
    let after = shader_effects::cache_stats();
    assert_eq!(
        (cache.programs, cache.masks, cache.bytes),
        (after.programs, after.masks, after.bytes)
    );
    assert!(shader_effects::take_notices().is_empty());
    std::fs::write(
        out.join("motion-check.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "frames": frames, "period_ms": period, "status": status, "reading": reading, "moving_pixels_max": moving,
            "resting_status_stable": resting_status_stable,
            "reading_stable": true, "alpha_stable": true, "loop_exact": true, "cache_stable": true,
        "event_reading_stable": true, "event_samples_ms": event_samples,
        }))
        .unwrap(),
    )
    .unwrap();
    println!(
        "motion: {frames} frames, {period} ms, {moving} animated pixels; reading, alpha, loop and caches stable"
    );
}
