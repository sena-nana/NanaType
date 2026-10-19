//! UI-thread cost of host-rendered plugin views at the protocol's limits.
//! `cargo test -p nanatype --release --test plugin_render_perf -- --ignored --nocapture`
mod common;

use nana_ui_devtools::agent::AgentSession;
use nanatype::ui::{self, Ui};
use nanatype_i18n::Locale;
use nanatype_plugins::{LuaSandbox, Mount};
use std::{collections::HashMap, sync::Arc, time::Instant};

/// A canvas at the draw budget plus a text tree near the node budget.
const HEAVY: &str = r#"
local tick = 0
return {
  on_event = function(e) tick = tick + 1 end,
  view = function()
    local draw = {}
    for i = 1, 2047 do
      local x = (i * 7 + tick) % 300
      draw[i] = {kind = 'rect', rect = {x, (i * 3) % 160, 6, 6}, color = {i % 255, 80, 200, 255}, radius = 2}
    end
    local rows = {}
    for i = 1, 60 do
      rows[i] = {kind = 'text', id = 'row' .. i, text = 'row ' .. i .. ' · ' .. tick}
    end
    return {views = {{slot = 'panel', root = {kind = 'column', id = 'root', children = {
      {kind = 'canvas', id = 'c', width = 320, height = 180, draw = draw},
      {kind = 'column', id = 'rows', children = rows},
    }}}}}
  end,
}
"#;

fn ms(samples: &mut [f64], q: f64) -> f64 {
    samples.sort_by(f64::total_cmp);
    samples[((samples.len() - 1) as f64 * q).round() as usize]
}

#[test]
#[ignore = "measurement; run in release with --ignored --nocapture"]
fn heavy_plugin_views_render_within_frame_budget() {
    let mut vm = LuaSandbox::new(HEAVY).unwrap();
    let root = Ui::new(
        Arc::new(Default::default()),
        Arc::new(Default::default()),
        Locale::ZhCn,
    );
    let first = vm.event(&serde_json::json!({"type": "init"})).unwrap();
    first.validate().unwrap();
    root.plugin_views
        .set(Arc::new(HashMap::from([("example.heavy".into(), first)])));
    let mut session = common::session()
        .themed(false)
        .mount(340, 1400, move || ui::plugins::mounted(root, Mount::Panel));
    session.set_clear(Some([0.; 4]));
    let _ = session.screenshot_rgba().unwrap();
    let (mut changed, mut idle, mut validate) = (Vec::new(), Vec::new(), Vec::new());
    for _ in 0..60 {
        let update = vm
            .event(&serde_json::json!({"type": "line_start"}))
            .unwrap();
        let started = Instant::now();
        update.validate().unwrap();
        validate.push(started.elapsed().as_secs_f64() * 1000.);
        let started = Instant::now();
        root.plugin_views
            .set(Arc::new(HashMap::from([("example.heavy".into(), update)])));
        let _ = session.screenshot_rgba().unwrap();
        changed.push(started.elapsed().as_secs_f64() * 1000.);
        let started = Instant::now();
        let _ = session.screenshot_rgba().unwrap();
        idle.push(started.elapsed().as_secs_f64() * 1000.);
    }
    println!(
        "{}",
        serde_json::json!({
            "measure": "heavy_plugin_view",
            "draw_commands": 2047,
            "text_nodes": 60,
            "host_validate_p50_ms": ms(&mut validate, 0.5),
            "host_validate_max_ms": ms(&mut validate, 1.0),
            "changed_frame_p50_ms": ms(&mut changed, 0.5),
            "changed_frame_p95_ms": ms(&mut changed, 0.95),
            "idle_frame_p50_ms": ms(&mut idle, 0.5),
            "note": "offscreen frames include GPU readback; compare changed vs idle",
        })
    );
}
