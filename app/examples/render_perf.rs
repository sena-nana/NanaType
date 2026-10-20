//! Reproducible retained rendering workloads. No network, audio or user settings.
//! cargo run --release -p nanatype --example render_perf -- target/render-perf/before.json
use std::sync::Arc;
use std::time::{Duration, Instant};

use nana_ui::runtime::{DocumentId, RuntimeDocument};
use nana_ui_devtools::agent::RuntimeAgentSession;
use nanatype::ui::{Ui, dialogue, history};
use nanatype_core::{Cmd, Core};
use nanatype_i18n::Locale;
use nanatype_settings::Settings;
use nanatype_theme::SkinRegistry;

fn main() {
    let registry = SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
    let mut cases = Vec::new();
    for (name, skin_id, text, reduced, history_rows) in [
        (
            "static",
            "nanatype.vanilla",
            "静止的文字 Static text",
            false,
            0,
        ),
        (
            "pacing_only",
            "nanatype.vanilla",
            "[speed=20ms]节奏标记不应无限刷新[/speed]",
            false,
            0,
        ),
        (
            "reveal",
            "nanatype.vanilla",
            "逐字呈现的台词 Typewriter dialogue",
            false,
            0,
        ),
        (
            "effects",
            "nanatype.vanilla",
            "[wave][rainbow]组合动效 Combined effects[/rainbow][/wave]",
            false,
            0,
        ),
        (
            "reduced",
            "nanatype.vanilla",
            "[wave]减少动态效果 Reduced motion[/wave]",
            true,
            0,
        ),
        ("decor", "nanatype.sakura", "装饰动画 Ornaments", false, 0),
        (
            "history_30",
            "nanatype.vanilla",
            "历史记录 History",
            false,
            30,
        ),
        (
            "history_1000",
            "nanatype.vanilla",
            "历史记录 History",
            false,
            1000,
        ),
    ] {
        let skin = registry.load(skin_id).unwrap().skin;
        let mut settings = Settings::default();
        settings.history.max_items = history_rows.max(200);
        settings.history.visible_items = history_rows.max(30);
        settings.playback.reduced_motion = reduced;
        let settings = Arc::new(settings);
        let ui = Ui::new(settings.clone(), skin.clone(), Locale::ZhCn);
        ui.reduced_motion.set(reduced);
        let mut core = Core::new((*settings).clone(), skin.clone());
        for _ in 0..history_rows.max(1) {
            core.handle(Cmd::SendMarkup(text.into()), 0);
            if name != "reveal" {
                core.handle(Cmd::Skip, 1);
            }
        }
        ui.display.set(Arc::new(core.display().clone()));
        ui.history
            .set(Arc::new(core.history().iter().cloned().collect()));
        let id = DocumentId::new(1).unwrap();
        let mut doc = RuntimeDocument::new(id);
        doc.context_mut()
            .set_theme_tokens(nanatype::ui::kit::theme::compile(false))
            .unwrap();
        doc.context_mut()
            .mount_view_root(id, move || {
                if history_rows > 0 {
                    use nana_ui::runtime::view::IntoView;
                    history::history_window(ui).into_any()
                } else {
                    use nana_ui::runtime::view::IntoView;
                    dialogue::display_window(ui).into_any()
                }
            })
            .unwrap();
        let mut session = RuntimeAgentSession::new(doc, 960, 540).unwrap();
        session.flush().unwrap();
        let mut trials = Vec::new();
        for trial in 0..3 {
            let mut times = Vec::new();
            let mut active = 0;
            let mut layout_changes = 0;
            let mut last_layout = session.document().context().world().layout_generation();
            for frame in 0..180 {
                let ms = 1000 + (trial * 180 + frame) * 16;
                nanatype::clock::capture_at(ms, || {
                    let start = Instant::now();
                    if dialogue::display_animating(ui, ms) {
                        ui.now.set(ms);
                    }
                    session
                        .document_mut()
                        .sync_presentation_clock(Duration::from_millis(ms));
                    let update = session.flush().unwrap();
                    if update.layout_generation != last_layout {
                        layout_changes += 1;
                    }
                    last_layout = update.layout_generation;
                    if frame >= 60 {
                        times.push(start.elapsed().as_secs_f64() * 1000.0);
                        active += usize::from(session.document().compositor_needs_tick());
                    }
                });
            }
            times.sort_by(f64::total_cmp);
            trials.push(serde_json::json!({"cpu_flush_median_ms": times[times.len()/2],
                "cpu_flush_p95_ms": times[times.len()*95/100], "compositor_active_ticks": active,
                "layout_revision_changes": layout_changes,
                "work_counters": format!("{:?}", session.document().context().last_work_counters())}));
        }
        cases.push(serde_json::json!({"scenario": name, "trials": trials}));
    }
    let report = serde_json::json!({"schema": 1, "cases": cases,
        "measurement": "CPU Runtime flush only; no GPU readback included",
        "gpu_time_ms": null, "gpu_upload_bytes": null, "gpu_draws": null,
        "notes": "60 warmup and 120 measured ticks per trial. Native multi-window/OBS measured separately."});
    let path = std::env::args().nth(1).expect("output JSON path");
    std::fs::create_dir_all(std::path::Path::new(&path).parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_string_pretty(&report).unwrap()).unwrap();
}
