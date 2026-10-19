//! `NANATYPE_TRANSITION_AUDIT`: entry and retirement frames.

use super::*;

/// Deterministic real retained-scene entry/retirement frames, with Core's
/// authoritative timing. No CPU recreation of the author's shader effect.
pub fn audit(ui: Ui, out: &std::path::Path, w: u32, h: u32, theme: Arc<nana_ui::CompiledTheme>) {
    use nana_ui::runtime::view::{AnyView, IntoView};
    use std::time::Duration;
    let base = 100_000;
    let skin = ui.skin.get_untracked();
    let requested_width = std::env::var("NANATYPE_TRANSITION_WIDTH")
        .ok()
        .map(|v| v.parse().unwrap());
    let width = requested_width.unwrap_or(w);
    let history_width = requested_width.unwrap_or(skin.history.width as u32);
    let history_height = skin.history.height as u32;
    let mut settings = (*ui.settings.get_untracked()).clone();
    settings.playback.punctuation_pauses = false;
    settings.history.hide_latest = nanatype_settings::HideLatest::Never;
    settings.overlays.history.size = Some([history_width as f32, history_height as f32]);
    ui.settings.set(Arc::new(settings.clone()));
    let mut core = Core::new(settings, skin);
    core.handle(
        Cmd::SetReducedMotion(ui.reduced_motion.get_untracked()),
        base,
    );
    core.handle(
        Cmd::SendMarkup(
            "[ruby=いろ]彩铅[/ruby] Crayon 😀\n[color=#B94F49]斜向擦入，斜向擦除[/color]".into(),
        ),
        base,
    );
    ui.display.set(Arc::new(core.display().clone()));
    ui.history
        .set(Arc::new(core.history().iter().cloned().collect()));
    ui.now.set(base);
    let scale = std::env::var("NANATYPE_SNAPSHOT_SCALE")
        .ok()
        .map(|v| v.parse().unwrap())
        .unwrap_or(1.);
    let make_session = |width, height, view: fn(Ui) -> AnyView| {
        let id = DocumentId::new(1).unwrap();
        let mut doc = RuntimeDocument::new(id);
        doc.context_mut().set_theme_tokens(theme.clone()).unwrap();
        nanatype::clock::capture_at(base, || {
            doc.context_mut()
                .mount_view_root(id, move || view(ui))
                .unwrap()
        });
        let mut session = RuntimeAgentSession::new_scaled(doc, width, height, scale).unwrap();
        session.set_clear(Some([0.; 4]));
        session
    };
    let mut history_session = make_session(history_width, history_height, |ui| {
        history::history_window(ui).into_any()
    });
    let mut session = make_session(width, h + 140, |ui| dialogue::display_window(ui).into_any());
    let mut ink = Vec::new();
    let capture = |session: &mut RuntimeAgentSession, t: u64, name: &str| {
        ui.now.set(base + t);
        session
            .document_mut()
            .sync_presentation_clock(Duration::from_millis(t));
        nanatype::clock::capture_at(base + t, || {
            let path = out.join(format!("{name}.png"));
            session.screenshot_png(&path).unwrap();
            image::open(path).unwrap().to_rgba8()
        })
    };
    for t in [
        0, 40, 80, 120, 180, 240, 360, 600, 900, 1200, 1500, 1800, 2200,
    ] {
        capture(&mut session, t, &format!("enter-{t:04}"));
        capture(&mut history_session, t, &format!("history-enter-{t:04}"));
    }
    let history_pixels = image::open(out.join("history-enter-2200.png"))
        .unwrap()
        .to_rgba8();
    let fx = core.handle(Cmd::ClearDisplay, base + 2400);
    let wake = fx
        .wakes
        .iter()
        .find(|w| w.reason == nanatype_playback::WakeReason::PresentationDone)
        .copied();
    ui.display.set(Arc::new(core.display().clone()));
    ui.history
        .set(Arc::new(core.history().iter().cloned().collect()));
    for t in [0, 40, 80, 120, 160, 200, 240, 280, 320] {
        let image = capture(&mut session, 2400 + t, &format!("exit-{t:03}"));
        assert!(
            capture(
                &mut history_session,
                2400 + t,
                &format!("history-exit-{t:03}")
            ) == history_pixels,
            "display retirement changed history at {t}ms"
        );
        let count = image
            .enumerate_pixels()
            .filter(|(x, y, p)| {
                *y > (65. * scale) as u32
                    && *y < (210. * scale) as u32
                    && *x > (30. * scale) as u32
                    && p[0] < 130
                    && p[1] < 130
                    && p[2] < 130
                    && p[3] > 100
            })
            .count();
        ink.push(count);
    }
    if wake.is_some() {
        assert!(
            ink[0] > ink[8] + 80,
            "retirement mask did not erase the actual body: {ink:?}"
        );
    }
    if let Some(w) = wake {
        core.handle(
            Cmd::Wake {
                epoch: w.epoch,
                reason: w.reason,
            },
            w.at_ms,
        );
        assert!(core.display().empty);
    }
    ui.display.set(Arc::new(core.display().clone()));
    ui.history
        .set(Arc::new(core.history().iter().cloned().collect()));
    assert!(
        capture(&mut history_session, 3000, "history-after-clear") == history_pixels,
        "clearing the display changed completed history"
    );
    std::fs::write(out.join("transition-check.json"),serde_json::to_vec_pretty(&serde_json::json!({"entry_ms":180,"exit_ms":320,"scale":scale,"width":width,"history_width":history_width,"history_survives_clear":true,"exit_ink":ink,"finite_deadline":wake.is_some(),"advanced_effects":ui.settings.with_untracked(|s|s.skin.advanced_effects),"reduced_motion":ui.reduced_motion.get_untracked()})).unwrap()).unwrap();
}
