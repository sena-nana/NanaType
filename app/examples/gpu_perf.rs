//! Real retained GPU workload, without image readback in measured frames.
//! cargo run --release -p nanatype --example gpu_perf -- target/render-perf/gpu-after.json
mod common;

use common::distribution;
use nana_ui::runtime::view::IntoView;
use nana_ui::runtime::{DocumentId, RuntimeDocument};
use nana_ui::{
    GpuContext, GpuTextureDescriptor, GpuTextureFormat, GpuTextureUsages, RenderTargetId,
    ScenePaintViewport, SceneWgpuPainter, wgpu,
};
use nana_ui_devtools::agent::RuntimeAgentSession;
use nanatype::{
    clock,
    ui::{Ui, danmaku, dialogue, history},
};
use nanatype_core::{Cmd, Core};
use nanatype_i18n::Locale;
use nanatype_settings::Settings;
use nanatype_theme::SkinRegistry;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

fn session(registry: &SkinRegistry, name: &'static str, track: usize) -> (Ui, RuntimeAgentSession) {
    clock::capture_at(0, || {
        let shader_id = format!(
            "nanatype.{}",
            name.trim_end_matches("_static")
                .trim_end_matches("_multi_output")
                .trim_start_matches("shader_")
        );
        let loaded = registry
            .load(if name.starts_with("shader_") {
                &shader_id
            } else if name == "decor" {
                "nanatype.sakura"
            } else {
                "nanatype.vanilla"
            })
            .unwrap();
        let urls = nanatype::assets::skin_preview_urls(&loaded.skin, &*loaded.files);
        let mut skin = loaded.skin;
        if name == "sweep" {
            Arc::make_mut(&mut skin).dialogue.sweep.enabled = true;
        }
        let mut settings = Settings::default();
        settings.skin.advanced_effects = !name.ends_with("_static");
        settings.history.max_items = 1000;
        let settings = Arc::new(settings);
        let ui = Ui::new(settings.clone(), skin.clone(), Locale::ZhCn);
        ui.asset_urls.set(Arc::new(urls));
        ui.reduced_motion.set(name == "reduced");
        ui.chat_width.set(960.0);
        let text = match name {
            "wave" | "multi_output" => "[wave]波浪动效 Retained wave[/wave]".into(),
            "combo" => "[wave][rainbow]波浪彩虹 Combined[/rainbow][/wave]".into(),
            "pulse_flicker" => "[pulse][flicker]脉动闪烁 Pulse flicker[/flicker][/pulse]".into(),
            "pacing_only" => "[speed=20ms]节奏标记 Static pacing[/speed]".into(),
            "reduced" => "[wave]减少动态效果 Reduced motion[/wave]".into(),
            "reveal" => "逐字呈现 Typewriter dialogue ".repeat(12),
            _ => "渲染效率 Rendering workload".into(),
        };
        let mut core = Core::new((*settings).clone(), skin.clone());
        for _ in 0..if name == "chat_1000" { 1000 } else { 1 } {
            core.handle(Cmd::SendMarkup(text.clone()), 0);
            if name != "reveal" {
                core.handle(Cmd::Skip, 1);
            }
        }
        ui.display.set(Arc::new(core.display().clone()));
        // Compare the same frozen content/status in both static and GPU
        // variants; status-specific decorations must be measured while active.
        if name.starts_with("shader_") {
            ui.display.update(|v| {
                let v = Arc::make_mut(v);
                match skin.meta.id.as_str() {
                    "nanatype.livechat" => {
                        v.reveal.shown /= 2;
                        v.reveal.segment = None;
                        v.continue_indicator = false;
                    }
                    "nanatype.rpg" => v.continue_indicator = true,
                    "nanatype.terminal" => v.continue_indicator = false,
                    _ => {}
                }
            });
        }
        ui.peer_display.set(ui.display.get_untracked());
        ui.history
            .set(Arc::new(core.history().iter().cloned().collect()));
        let id = DocumentId::new((track + 1) as u64).unwrap();
        let mut doc = RuntimeDocument::new(id);
        doc.context_mut()
            .set_theme_tokens(nanatype::ui::kit::theme::compile(false))
            .unwrap();
        doc.context_mut()
            .mount_view_root(id, move || {
                if name == "plugin_heavy" {
                    nanatype::ui::plugins::mounted(ui, nanatype_plugins::Mount::Panel).into_any()
                } else if name == "chat_1000" {
                    danmaku::chat_window(ui).into_any()
                } else if track == 2 {
                    history::history_window(ui).into_any()
                } else if track == 1 {
                    dialogue::display_window(ui.remote()).into_any()
                } else {
                    dialogue::display_window(ui).into_any()
                }
            })
            .unwrap();
        let mut session = RuntimeAgentSession::new(doc, 960, 540).unwrap();
        session.flush().unwrap();
        (ui, session)
    })
}

fn main() {
    let output = std::path::PathBuf::from(std::env::args().nth(1).expect("output JSON path"));
    let media = output
        .parent()
        .unwrap_or(std::path::Path::new("."))
        .join("perf-media");
    nanatype::services::media::init(&media).expect("benchmark media mirror");
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::from_env().unwrap_or_default(),
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    });
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        ..Default::default()
    }))
    .expect("GPU adapter");
    let info = adapter.get_info();
    let timestamps =
        wgpu::Features::TIMESTAMP_QUERY | wgpu::Features::TIMESTAMP_QUERY_INSIDE_ENCODERS;
    let supported = adapter.features().contains(timestamps);
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("NanaType rendering benchmark"),
        required_features: if supported {
            timestamps
        } else {
            wgpu::Features::empty()
        },
        ..Default::default()
    }))
    .unwrap();
    let queries = supported.then(|| {
        device.create_query_set(&wgpu::QuerySetDescriptor {
            label: Some("frame timing"),
            ty: wgpu::QueryType::Timestamp,
            count: 2,
        })
    });
    let resolve = supported.then(|| {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("timestamp resolve"),
            size: 256,
            usage: wgpu::BufferUsages::QUERY_RESOLVE | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        })
    });
    let read = supported.then(|| {
        device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("timestamp read"),
            size: 16,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        })
    });
    let timestamp_period = queue.get_timestamp_period() as f64;
    let gpu = GpuContext::from_wgpu(adapter, device, queue);
    let registry = SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
    let mut cases = Vec::new();
    for name in [
        "static",
        "plugin_heavy",
        "pacing_only",
        "reduced",
        "reveal",
        "wave",
        "combo",
        "decor",
        "chat_1000",
        "multi_output",
        "pulse_flicker",
        "sweep",
        "shader_gothic_static",
        "shader_gothic",
        "shader_celestial_static",
        "shader_celestial",
        "shader_forest_static",
        "shader_forest",
        "shader_tidal_static",
        "shader_tidal",
        "shader_lantern_static",
        "shader_lantern",
        "shader_citrus_static",
        "shader_citrus",
        "shader_tidal_multi_output",
        "shader_halo_static",
        "shader_night_static",
        "shader_night",
        "shader_parchment_static",
        "shader_adv_static",
        "shader_rpg_static",
        "shader_rpg",
        "shader_vanilla_static",
        "shader_bubble_static",
        "shader_bubble",
        "shader_subtitle_static",
        "shader_livechat_static",
        "shader_livechat",
        "shader_comic_static",
        "shader_terminal_static",
        "shader_terminal",
        "shader_pixel_static",
        "shader_pixel",
        "shader_sakura_static",
        "shader_sakura",
        "shader_ink_static",
        "shader_echo_vanilla_static",
        "shader_echo_glass_static",
        "shader_echo_bubble_static",
        "shader_echo_void_static",
    ] {
        if std::env::var("NANATYPE_GPU_CASES")
            .is_ok_and(|v| !v.split(',').any(|s| s.trim() == name))
        {
            continue;
        }
        let multi = name == "multi_output" || name.ends_with("_multi_output");
        let mut plugin = (name == "plugin_heavy").then(|| {
            nanatype_plugins::LuaSandbox::new(include_str!("../tests/fixtures/heavy-plugin.lua"))
                .unwrap()
        });
        let mut sessions: Vec<_> = (0..if multi { 3 } else { 1 })
            .map(|i| session(&registry, name, i))
            .collect();
        let outputs = if multi { 2 } else { 1 };
        let format = |i: usize| {
            if name.starts_with("shader_") && multi && i % outputs == 1 {
                GpuTextureFormat::RGBA16_FLOAT
            } else {
                GpuTextureFormat::BGRA8_UNORM_SRGB
            }
        };
        let mut painters: Vec<_> = (0..outputs)
            .map(|i| SceneWgpuPainter::new(&gpu, format(i)))
            .collect();
        let textures: Vec<_> = (0..sessions.len() * outputs)
            .map(|i| {
                gpu.create_texture(&GpuTextureDescriptor {
                    label: Some("benchmark target"),
                    width: 960,
                    height: 540,
                    format: format(i),
                    usage: GpuTextureUsages::RENDER_TARGET,
                })
                .unwrap()
            })
            .collect();
        let renderers = nanatype::ui::sweep::renderers();
        let mut trials = Vec::new();
        for trial in 0..3 {
            let mut cpu = Vec::new();
            let mut gpu_times = Vec::new();
            let (mut rendered, mut shaped, mut layouts, mut draws, mut uploaded, mut primitives) =
                (0, 0, 0, 0, 0, 0);
            for frame in 0..180 {
                let ms = ((trial * 180 + frame) * 16) as u64;
                let mut count = 0;
                // Worker execution is measured separately; time host projection and GPU work here.
                let update = plugin
                    .as_mut()
                    .map(|vm| vm.event(&serde_json::json!({"type":"tick"})).unwrap());
                let start = Instant::now();
                let mut commands = gpu.begin_frame("benchmark frame");
                if let Some(q) = &queries {
                    commands.wgpu_encoder().write_timestamp(q, 0);
                }
                clock::capture_at(ms, || {
                    for (i, (ui, session)) in sessions.iter_mut().enumerate() {
                        if let Some(update) = &update {
                            ui.plugin_views
                                .set(Arc::new(std::collections::HashMap::from([(
                                    "example.heavy".into(),
                                    update.clone(),
                                )])));
                        }
                        let track_ui = if i == 1 { ui.remote() } else { *ui };
                        let runtime_animation = dialogue::display_animating(track_ui, ms);
                        let animate = runtime_animation
                            || nanatype::ui::sweep::animating(track_ui)
                            || nanatype::ui::shader_effects::animating(track_ui);
                        if runtime_animation {
                            track_ui.now.set(ms);
                        }
                        session
                            .document_mut()
                            .sync_presentation_clock(Duration::from_millis(ms));
                        let update = session.flush().unwrap();
                        if frame >= 60 && !update.is_idle() {
                            let w = session.document().context().last_work_counters();
                            shaped += w.text_shaped;
                            layouts += w.layout_nodes;
                        }
                        if frame == 0
                            || name.starts_with("shader_")
                            || animate
                            || !update.is_idle()
                            || session.document().compositor_needs_tick()
                        {
                            for (output, painter) in painters.iter_mut().enumerate().take(outputs) {
                                let target = i * outputs + output;
                                painter
                                    .paint_target(
                                        RenderTargetId(target as u64 + 1),
                                        session.document().scene(),
                                        &mut commands,
                                        &textures[target].render_target().unwrap(),
                                        ScenePaintViewport {
                                            logical_size: [960.0, 540.0],
                                            physical_size: [960, 540],
                                            scale_factor: 1.0,
                                            scene_origin: [0.0; 2],
                                            target_origin: [0.0; 2],
                                            clear_color: [0.0; 4],
                                            clear: true,
                                        },
                                        None,
                                        Some(&renderers),
                                    )
                                    .unwrap();
                                if frame >= 60
                                    && let Some(work) = painter.last_gpu_work()
                                {
                                    draws += work.draw_calls;
                                    uploaded += work.gpu_upload_bytes;
                                }
                                count += 1;
                            }
                        }
                        primitives = primitives.max(session.document().scene().primitive_count());
                    }
                });
                if let (Some(q), Some(resolve), Some(read)) = (&queries, &resolve, &read) {
                    commands.wgpu_encoder().write_timestamp(q, 1);
                    commands
                        .wgpu_encoder()
                        .resolve_query_set(q, 0..2, resolve, 0);
                    commands
                        .wgpu_encoder()
                        .copy_buffer_to_buffer(resolve, 0, read, 0, 16);
                }
                let submission = commands.submit();
                if frame == 0 {
                    let started = Instant::now();
                    while nanatype::ui::shader_effects::cache_stats().pending > 0
                        && started.elapsed() < Duration::from_secs(10)
                    {
                        std::thread::sleep(Duration::from_millis(10));
                    }
                }
                if frame >= 60 {
                    cpu.push(start.elapsed().as_secs_f64() * 1000.0);
                    rendered += count;
                }
                // Completion and timestamp mapping are outside CPU frame timing.
                gpu.wgpu()
                    .device()
                    .poll(wgpu::PollType::Wait {
                        submission_index: Some(submission.wgpu_index().clone()),
                        timeout: Some(Duration::from_secs(10)),
                    })
                    .unwrap();
                if let Some(read) = &read {
                    let (tx, rx) = std::sync::mpsc::channel();
                    read.slice(..)
                        .map_async(wgpu::MapMode::Read, move |result| {
                            let _ = tx.send(result);
                        });
                    gpu.wgpu()
                        .device()
                        .poll(wgpu::PollType::Wait {
                            submission_index: None,
                            timeout: Some(Duration::from_secs(10)),
                        })
                        .unwrap();
                    rx.recv_timeout(Duration::from_secs(10)).unwrap().unwrap();
                    let data = read.slice(..).get_mapped_range().unwrap();
                    let begin = u64::from_ne_bytes(data[..8].try_into().unwrap());
                    let end = u64::from_ne_bytes(data[8..16].try_into().unwrap());
                    if frame >= 60 && count > 0 {
                        gpu_times
                            .push(end.wrapping_sub(begin) as f64 * timestamp_period / 1_000_000.0);
                    }
                    drop(data);
                    read.unmap();
                }
            }
            trials.push(serde_json::json!({"cpu_record_submit":distribution(cpu),"gpu_timestamp":distribution(gpu_times),"rendered_targets":rendered,"text_shaped":shaped,"layout_nodes":layouts,"draw_calls":draws,"upload_bytes":uploaded,"max_scene_primitives":primitives}));
        }
        cases.push(serde_json::json!({"scenario":name,"trials":trials,"shader_cache":nanatype::ui::shader_effects::cache_stats(),"shader_errors":nanatype::ui::shader_effects::take_notices()}));
    }
    let report = serde_json::json!({"schema":1,"adapter":info.name,"backend":format!("{:?}",info.backend),"timestamps_supported":supported,"measurement":"60 warmup + 120 measured ticks, 3 trials; GPU timestamps, no pixel readback; multi_output is 3 scenes x 2 retained targets on one device (does not include Spout receiver/fences)","cases":cases});
    let path = std::env::args().nth(1).expect("output JSON path");
    std::fs::create_dir_all(std::path::Path::new(&path).parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_string_pretty(&report).unwrap()).unwrap();
}
