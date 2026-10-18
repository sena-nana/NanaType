//! Record the actual machine-learning skin at 40 fps without opening a window.
//!
//! `cargo run -p nanatype --example machine_learning_motion -- <output-directory>`

use nana_ui::runtime::{DocumentId, RuntimeDocument};
use nana_ui_devtools::agent::{AgentSession, RuntimeAgentSession};
use nanatype::{
    clock,
    ui::{Ui, dialogue, sweep, text_scramble},
};
use nanatype_core::{Cmd, Core};
use nanatype_i18n::Locale;
use nanatype_playback::WakeReason;
use nanatype_settings::{InputMode, Settings};
use std::{path::PathBuf, sync::Arc, time::Duration};
use unicode_segmentation::UnicodeSegmentation;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output = PathBuf::from(
        std::env::args_os()
            .nth(1)
            .ok_or("missing output directory")?,
    );
    std::fs::create_dir_all(&output)?;
    let skin = nanatype_theme::SkinRegistry::new(None, "1.0.0")
        .load("nanatype.machine_learning")
        .map_err(|diagnostics| std::io::Error::other(format!("{diagnostics:?}")))?
        .skin;
    let mut settings = Settings::default();
    settings.skin.id = "nanatype.machine_learning".into();
    settings.skin.advanced_effects = true;
    settings.general.input_mode = InputMode::Mirror;
    settings.general.speaker_name = "奈奈 / ML".into();
    let ui = Ui::new(Arc::new(settings.clone()), skin.clone(), Locale::ZhCn);
    let mut core = Core::new(settings, skin);
    ui.display.set(Arc::new(core.display().clone()));
    let id = DocumentId::new(1).unwrap();
    let mut document = RuntimeDocument::new(id);
    const START: u64 = 100_000;
    clock::capture_at(START, || {
        document
            .context_mut()
            .mount_view_root(id, move || dialogue::display_window(ui))
            .unwrap();
    });
    let mut session = RuntimeAgentSession::new(document, 892, 232)?;
    session.set_gpu_renderers(sweep::renderers());
    session.set_clear(Some([0.; 4]));

    let text = "模型正在学习，文字逐渐清晰。\nLearning, one character at a time.";
    let graphemes: Vec<_> = text.graphemes(true).collect();
    let last_input = 300 + (graphemes.len() as u64 - 1) * 75;
    let exit_start = (last_input + 140 + 900).div_ceil(25) * 25;
    let duration = exit_start + 400 + 600;
    let mut count = 0;
    let mut wake = None;
    let mut exit_started = false;
    let mut cleared = false;
    let mut previous_frame = None;
    let mut changed_frames = 0;
    let mut captured = 0;

    for elapsed in (0..=duration).step_by(25) {
        let now = START + elapsed;
        if elapsed >= 300 && !exit_started {
            let next = (((elapsed - 300) / 75 + 1) as usize).min(graphemes.len());
            if next != count {
                count = next;
                core.handle(
                    Cmd::DraftChanged(nanatype_model::Document::plain(
                        &graphemes[..count].concat(),
                    )),
                    now,
                );
                ui.display.set(Arc::new(core.display().clone()));
            }
        }
        if elapsed >= exit_start && !exit_started {
            let effects = core.handle(Cmd::ClearDisplay, now);
            wake = effects
                .wakes
                .into_iter()
                .find(|w| w.reason == WakeReason::PresentationDone);
            assert!(wake.is_some(), "the native 400 ms exit must be scheduled");
            exit_started = true;
            ui.display.set(Arc::new(core.display().clone()));
        }
        if elapsed >= exit_start + 400 && !cleared {
            let completed = wake.take().unwrap();
            core.handle(
                Cmd::Wake {
                    epoch: completed.epoch,
                    reason: completed.reason,
                },
                now,
            );
            assert!(core.display().empty);
            ui.display.set(Arc::new(core.display().clone()));
            cleared = true;
        }
        ui.now.set(now);
        session
            .document_mut()
            .sync_presentation_clock(Duration::from_millis(elapsed));
        let (size, rgba) = clock::capture_at(now, || session.screenshot_rgba())?;
        if previous_frame.as_ref().is_some_and(|last| last != &rgba) {
            changed_frames += 1;
        }
        image::save_buffer(
            output.join(format!("frame-{captured:04}.png")),
            &rgba,
            size.width,
            size.height,
            image::ColorType::Rgba8,
        )?;
        previous_frame = Some(rgba);
        captured += 1;
    }
    assert!(changed_frames > graphemes.len() * 2);
    assert!(!text_scramble::animating(ui, START + duration));
    assert!(!session.document().compositor_needs_tick());
    std::fs::write(
        output.join("timing.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "frame_ms": 25,
            "frames": captured,
            "last_input_ms": last_input,
            "exit_ms": exit_start,
            "exit_duration_ms": 400,
            "text": text,
        }))?,
    )?;
    println!("Captured {captured} native frames; {changed_frames} changed frames.");
    Ok(())
}
