//! Retained scenes: repeated skin switches, simultaneous targets, device
//! alternation, resize churn and closing all consumers. No desktop windows.
use nana_ui::runtime::view::IntoView;
use nana_ui::runtime::{DocumentId, RuntimeDocument};
use nana_ui::{
    GpuContext, GpuTexture, GpuTextureDescriptor, GpuTextureFormat, GpuTextureUsages,
    RenderTargetId, ScenePaintViewport, SceneWgpuPainter, wgpu,
};
use nana_ui_devtools::agent::RuntimeAgentSession;
use nanatype::{
    clock,
    ui::{Ui, dialogue, history, shader_effects, sweep},
};
use nanatype_core::{Cmd, Core};
use nanatype_i18n::Locale;
use nanatype_theme::SkinRegistry;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};
static ADAPTERS: std::sync::Mutex<Vec<String>> = std::sync::Mutex::new(Vec::new());

fn gpu() -> GpuContext {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter = pollster::block_on(instance.request_adapter(&Default::default())).unwrap();
    let info = adapter.get_info();
    ADAPTERS
        .lock()
        .unwrap()
        .push(format!("{} ({:?})", info.name, info.backend));
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    GpuContext::from_wgpu(adapter, device, queue)
}
fn texture(g: &GpuContext, size: [u32; 2], format: GpuTextureFormat) -> GpuTexture {
    g.create_texture(&GpuTextureDescriptor {
        label: Some("skin lifecycle target"),
        width: size[0],
        height: size[1],
        format,
        usage: GpuTextureUsages::RENDER_TARGET | GpuTextureUsages::COPY_SRC,
    })
    .unwrap()
}
fn doc(ui: Ui, track: usize) -> RuntimeAgentSession {
    let id = DocumentId::new(track as u64 + 1).unwrap();
    let mut doc = RuntimeDocument::new(id);
    doc.context_mut()
        .set_theme_tokens(nanatype::skin_theme::compile(false))
        .unwrap();
    doc.context_mut()
        .mount_view_root(id, move || match track {
            0 => dialogue::display_window(ui).into_any(),
            1 => dialogue::display_window(ui.remote()).into_any(),
            _ => history::history_window(ui).into_any(),
        })
        .unwrap();
    RuntimeAgentSession::new(doc, 960, 540).unwrap()
}
fn frame(
    g: &GpuContext,
    docs: &mut [RuntimeAgentSession],
    painters: &mut [SceneWgpuPainter],
    targets: &[GpuTexture],
    ms: u64,
) -> (f64, usize, usize, [f64; 41]) {
    clock::capture_at(ms, || {
        let start = Instant::now();
        let mut commands = g.begin_frame("skin lifecycle");
        let mut shaped = 0;
        let mut layouts = 0;
        let mut stages = [0.; 41];
        for (i, doc) in docs.iter_mut().enumerate() {
            doc.document_mut()
                .sync_presentation_clock(Duration::from_millis(ms));
            let before = Instant::now();
            let update = doc.flush().unwrap();
            stages[i * 4] = before.elapsed().as_secs_f64() * 1000.;
            if !update.is_idle() {
                let work = doc.document().context().last_work_counters();
                shaped += work.text_shaped;
                layouts += work.layout_nodes;
            }
            for (o, painter) in painters.iter_mut().enumerate() {
                let (w, h) = targets[i * 3 + o].size();
                let size = [w, h];
                let before = Instant::now();
                painter
                    .paint_target(
                        RenderTargetId((i * 3 + o + 1) as u64),
                        doc.document().scene(),
                        &mut commands,
                        &targets[i * 3 + o].render_target().unwrap(),
                        ScenePaintViewport {
                            logical_size: [960., 540.],
                            physical_size: size,
                            scale_factor: 1.,
                            scene_origin: [0.; 2],
                            target_origin: if o == 1 { [24., 18.] } else { [0.; 2] },
                            clear_color: [0.; 4],
                            clear: true,
                        },
                        None,
                        Some(&sweep::renderers()),
                    )
                    .unwrap();
                stages[i * 4 + o + 1] = before.elapsed().as_secs_f64() * 1000.;
                if let Some(t) = painter.last_gpu_timings() {
                    let j = 14 + (i * 3 + o) * 3;
                    stages[j] = t.batch.as_secs_f64() * 1000.;
                    stages[j + 1] = t.gpu_upload.as_secs_f64() * 1000.;
                    stages[j + 2] = t.encode.as_secs_f64() * 1000.;
                }
            }
        }
        let before = Instant::now();
        let submission = commands.submit();
        stages[12] = before.elapsed().as_secs_f64() * 1000.;
        let cpu = start.elapsed().as_secs_f64() * 1000.;
        let before = Instant::now();
        g.wgpu()
            .device()
            .poll(wgpu::PollType::Wait {
                submission_index: Some(submission.wgpu_index().clone()),
                timeout: Some(Duration::from_secs(10)),
            })
            .unwrap();
        stages[13] = before.elapsed().as_secs_f64() * 1000.;
        shader_effects::collect_garbage();
        sweep::collect_garbage();
        (cpu, shaped, layouts, stages)
    })
}
fn outputs(g: &GpuContext, count: usize) -> (Vec<SceneWgpuPainter>, Vec<GpuTexture>) {
    let formats = [
        GpuTextureFormat::BGRA8_UNORM_SRGB,
        GpuTextureFormat::BGRA8_UNORM_SRGB,
        GpuTextureFormat::RGBA16_FLOAT,
    ];
    let painters = formats
        .iter()
        .map(|f| SceneWgpuPainter::new(g, *f))
        .collect();
    let textures = (0..count * 3)
        .map(|i| {
            texture(
                g,
                if i % 3 == 1 { [1280, 720] } else { [960, 540] },
                formats[i % 3],
            )
        })
        .collect();
    (painters, textures)
}

fn distribution(mut samples: Vec<f64>) -> serde_json::Value {
    samples.sort_by(f64::total_cmp);
    serde_json::json!({"samples":samples.len(),"median_ms":samples[samples.len()/2],"p95_ms":samples[samples.len()*95/100],"max_ms":samples.last()})
}

fn pixel(g: &GpuContext, texture: &GpuTexture, x: u32, y: u32) -> [u8; 4] {
    let mut frame = g.begin_frame("output isolation readback");
    let buffer = g.wgpu().device().create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 256,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut source = texture.wgpu().as_image_copy();
    source.origin = wgpu::Origin3d { x, y, z: 0 };
    frame.wgpu_encoder().copy_texture_to_buffer(
        source,
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(256),
                rows_per_image: Some(1),
            },
        },
        wgpu::Extent3d {
            width: 1,
            height: 1,
            depth_or_array_layers: 1,
        },
    );
    frame.submit();
    buffer.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    g.wgpu()
        .device()
        .poll(wgpu::PollType::Wait {
            submission_index: None,
            timeout: Some(Duration::from_secs(10)),
        })
        .unwrap();
    let data = buffer.slice(..).get_mapped_range().unwrap();
    data[..4].try_into().unwrap()
}

fn output_isolation(g: &GpuContext) {
    use nana_ui::runtime::{Stack, view::widget};
    use nana_ui_core::{LengthSpec, PositionSpec};
    use nanatype_theme::{ShaderSpec, ShaderTarget, Skin, SweepSpec, validate_shader};
    for is_sweep in [false, true] {
        let mut skin = Skin::default();
        skin.shader_programs.insert(
            "probe".into(),
            Arc::new(
                validate_shader(
                    "fn effect(i:EffectInput)->vec4<f32>{return vec4(i.size.x/160.,i.size.y/120.,i.uv.x,1.);}",
                )
                .unwrap(),
            ),
        );
        skin.shaders.insert(
            "probe".into(),
            ShaderSpec {
                intensity: 1.,
                ..Default::default()
            },
        );
        let skin = Arc::new(skin);
        let ui = Ui::new(Arc::new(Default::default()), skin.clone(), Locale::ZhCn);
        ui.display.update(|d| Arc::make_mut(d).empty = false);
        let surface = nanatype::ui::surface::Surface {
            fill: Some([1.; 4]),
            ..Default::default()
        };
        let effect = if is_sweep {
            sweep::view(
                surface,
                SweepSpec {
                    enabled: true,
                    intensity: 1.,
                    ..Default::default()
                },
                false,
            )
        } else {
            shader_effects::view(
                ui,
                dialogue::Source::Display,
                &skin,
                "probe",
                surface,
                ShaderTarget::Artwork,
            )
            .unwrap()
        };
        let id = DocumentId::new(77).unwrap();
        let mut document = RuntimeDocument::new(id);
        document
            .context_mut()
            .mount_view_root(id, move || {
                widget(
                    Stack::column(0.)
                        .width(LengthSpec::Px(80.))
                        .height(LengthSpec::Px(60.))
                        .with_layout(|l| l.position = PositionSpec::Relative),
                )
                .children(effect)
            })
            .unwrap();
        let mut session = RuntimeAgentSession::new(document, 128, 128).unwrap();
        session.flush().unwrap();
        let textures = [
            texture(g, [128, 128], GpuTextureFormat::BGRA8_UNORM_SRGB),
            texture(g, [256, 256], GpuTextureFormat::BGRA8_UNORM_SRGB),
        ];
        let mut painter = SceneWgpuPainter::new(g, GpuTextureFormat::BGRA8_UNORM_SRGB);
        for _ in 0..3 {
            clock::capture_at(4000, || {
                let mut frame = g.begin_frame("same-batch output isolation");
                for (i, target) in textures.iter().enumerate() {
                    painter
                        .paint_target(
                            RenderTargetId(i as u64 + 1),
                            session.document().scene(),
                            &mut frame,
                            &target.render_target().unwrap(),
                            ScenePaintViewport {
                                logical_size: [128.; 2],
                                physical_size: if i == 0 { [128; 2] } else { [256; 2] },
                                scale_factor: if i == 0 { 1. } else { 1.5 },
                                scene_origin: [0.; 2],
                                target_origin: if i == 0 { [0.; 2] } else { [32., 16.] },
                                clear_color: [0.; 4],
                                clear: true,
                            },
                            None,
                            Some(&sweep::renderers()),
                        )
                        .unwrap();
                }
                frame.submit();
            });
            let start = Instant::now();
            while shader_effects::cache_stats().pending > 0
                && start.elapsed() < Duration::from_secs(10)
            {
                std::thread::sleep(Duration::from_millis(5));
            }
        }
        let a = pixel(g, &textures[0], 40, 30);
        let b = pixel(g, &textures[1], 108, 69);
        assert!(a[3] > 0, "probe was blank: sweep={is_sweep}");
        assert!(
            a.iter().zip(b).all(|(a, b)| a.abs_diff(b) <= 2),
            "shared parameter overwrite: {a:?} != {b:?}, sweep={is_sweep}"
        );
    }
    shader_effects::collect_garbage();
    sweep::collect_garbage();
}
fn main() {
    let out = std::path::PathBuf::from(std::env::args().nth(1).expect("output JSON"));
    std::fs::create_dir_all(out.parent().unwrap()).unwrap();
    nanatype::services::media::init(&out.parent().unwrap().join("media")).unwrap();
    let registry = SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
    let ids: Vec<_> = registry.entries().iter().map(|e| e.id.clone()).collect();
    let base = registry.load("nanatype.halo").unwrap();
    let settings = Arc::new(nanatype_settings::Settings::default());
    let ui = Ui::new(settings.clone(), base.skin.clone(), Locale::ZhCn);
    let mut core = Core::new((*settings).clone(), base.skin);
    core.handle(
        Cmd::SendMarkup("切换与多输出压力测试 Rendering stress".into()),
        0,
    );
    core.handle(Cmd::Skip, 1);
    ui.display.set(Arc::new(core.display().clone()));
    ui.peer_display.set(ui.display.get_untracked());
    ui.history
        .set(Arc::new(core.history().iter().cloned().collect()));
    let g = gpu();
    output_isolation(&g);
    let (mut painters, targets) = outputs(&g, 3);
    let mut docs: Vec<_> = (0..3).map(|i| doc(ui, i)).collect();
    let mut samples = Vec::new();
    let mut switches = Vec::new();
    let mut ms = 100_000;
    let mut warm_compilations = 0;
    for round in 0..4 {
        let mut times = Vec::new();
        let mut steady = Vec::new();
        let mut steady_complete = Vec::new();
        let mut details = Vec::new();
        for id in &ids {
            let start = Instant::now();
            let skin = registry.load(id).unwrap();
            ui.asset_urls
                .set(Arc::new(nanatype::assets::skin_preview_urls(
                    &skin.skin,
                    &*skin.files,
                )));
            ui.skin.set(skin.skin);
            let (cpu, _, _, stages) = frame(&g, &mut docs, &mut painters, &targets, ms);
            ms += 33;
            times.push(start.elapsed().as_secs_f64() * 1000.);
            details
                .push(serde_json::json!({"id":id,"record_ms":cpu,"stages_ms":stages.as_slice()}));
            samples.push(cpu);
            let deadline = Instant::now();
            while shader_effects::cache_stats().pending > 0
                && deadline.elapsed() < Duration::from_secs(15)
            {
                std::thread::sleep(Duration::from_millis(5));
            }
            for _ in 0..5 {
                let (cpu, shaped, layouts, stages) =
                    frame(&g, &mut docs, &mut painters, &targets, ms);
                ms += 33;
                assert_eq!((shaped, layouts), (0, 0), "stable frame reshaped {id}");
                samples.push(cpu);
                steady.push(cpu);
                steady_complete.push(cpu + stages[13]);
            }
            assert!(shader_effects::take_notices().is_empty());
            let stats = shader_effects::cache_stats();
            assert!(stats.bytes < 64 * 1024 * 1024);
            assert!(stats.masks <= 8);
        }
        let stats = shader_effects::cache_stats();
        if round == 0 {
            warm_compilations = stats.compilations;
        } else {
            assert_eq!(
                warm_compilations, stats.compilations,
                "warm switches recompiled"
            );
        }
        switches.push(serde_json::json!({"round":round,"switch_cpu_and_gpu_wait":distribution(times),"steady":distribution(steady),"steady_cpu_and_gpu_wait":distribution(steady_complete),"details":details,"cache":stats}));
        println!("switch round {round} complete");
    }
    // Separate devices must coexist. Alternate every frame after both warm.
    let other = gpu();
    let (mut other_painters, other_targets) = outputs(&other, 3);
    ui.skin.set(registry.load("nanatype.tidal").unwrap().skin);
    for _ in 0..8 {
        frame(&g, &mut docs, &mut painters, &targets, ms);
        ms += 33;
        frame(&other, &mut docs, &mut other_painters, &other_targets, ms);
        ms += 33;
        std::thread::sleep(Duration::from_millis(10));
    }
    let prepared = shader_effects::cache_stats();
    for _ in 0..120 {
        frame(&g, &mut docs, &mut painters, &targets, ms);
        ms += 33;
        frame(&other, &mut docs, &mut other_painters, &other_targets, ms);
        ms += 33;
    }
    let alternated = shader_effects::cache_stats();
    assert_eq!(
        prepared.compilations, alternated.compilations,
        "device alternation recompiled"
    );
    assert_eq!(
        prepared.uniform_allocations, alternated.uniform_allocations,
        "stable targets allocated new parameter buffers"
    );
    assert_eq!(
        (prepared.masks, prepared.bytes, prepared.uniforms),
        (alternated.masks, alternated.bytes, alternated.uniforms)
    );
    // Resize only target 0 repeatedly; coverage remains 1x1, binding LRU bounded.
    let mut targets = targets;
    for i in 0..80 {
        let size = [960 + i % 17, 540 + i % 11];
        targets[0] = texture(&g, size, GpuTextureFormat::BGRA8_UNORM_SRGB);
        // Replace a stable target id with changing physical extents.
        frame(&g, &mut docs, &mut painters, &targets, ms);
        ms += 33;
    }
    let resized = shader_effects::cache_stats();
    assert_eq!(alternated.compilations, resized.compilations);
    // Real folder saves on a worker while all three scenes keep rendering.
    let skin_dir = out.parent().unwrap().join("hot-skins/user.lifecycle");
    std::fs::create_dir_all(&skin_dir).unwrap();
    std::fs::write(skin_dir.join("skin.toml"), "[meta]\nid='user.lifecycle'\nbase='nanatype.tidal'\n[shaders.artwork]\nsource='effect.wgsl'\n").unwrap();
    let package = nanatype_theme::PackageManifest {
        format: 1,
        meta: nanatype_theme::Meta {
            id: "user.lifecycle".into(),
            base: Some("nanatype.tidal".into()),
            ..Default::default()
        },
        capabilities: vec![nanatype_theme::PackageCapability::Skin],
        dependencies: std::collections::BTreeMap::from([(
            "nanatype.tidal".into(),
            registry
                .load("nanatype.tidal")
                .unwrap()
                .skin
                .meta
                .version
                .clone(),
        )]),
        catalogue_order: 0,
        entry: None,
        created_at: String::new(),
        licensed_to: String::new(),
    };
    std::fs::write(skin_dir.join("package.toml"), package.to_toml().unwrap()).unwrap();
    std::fs::write(
        skin_dir.join("effect.wgsl"),
        "fn effect(i:EffectInput)->vec4<f32>{return vec4(0.);}",
    )
    .unwrap();
    let mut user_registry = SkinRegistry::new(
        Some(skin_dir.parent().unwrap().to_owned()),
        env!("CARGO_PKG_VERSION"),
    );
    let (tx, rx) = std::sync::mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        let mut previous = None;
        let mut previous_source = String::new();
        let mut reused = 0;
        for i in 0..32 {
            let source = if i % 7 == 0 {
                "fn effect(i:EffectInput)->vec4<f32>{loop{} return vec4(0.);}".to_owned()
            } else {
                format!(
                    "fn effect(i:EffectInput)->vec4<f32>{{return vec4({}.,0.,0.,.1);}}",
                    (i / 4) % 4
                )
            };
            std::fs::write(skin_dir.join("effect.wgsl"), &source).unwrap();
            let start = Instant::now();
            user_registry.rescan();
            let loaded = user_registry.load_scanned("user.lifecycle");
            let ms = start.elapsed().as_secs_f64() * 1000.;
            if let Ok(loaded) = &loaded {
                let program = loaded.skin.shader_programs["artwork"].clone();
                if previous_source == source {
                    assert!(Arc::ptr_eq(previous.as_ref().unwrap(), &program));
                    reused += 1;
                }
                previous = Some(program);
                previous_source = source;
            }
            tx.send((loaded, ms)).unwrap();
        }
        reused
    });
    let mut hot_times = Vec::new();
    let mut hot_frames = Vec::new();
    let mut rejected = 0;
    let deadline = Instant::now();
    while hot_times.len() < 32 {
        assert!(
            deadline.elapsed() < Duration::from_secs(60),
            "hot reload worker stalled"
        );
        if let Ok((loaded, ms)) = rx.try_recv() {
            hot_times.push(ms);
            let old = ui.skin.get_untracked();
            match loaded {
                Ok(loaded) => {
                    ui.asset_urls
                        .set(Arc::new(nanatype::assets::skin_preview_urls(
                            &loaded.skin,
                            &*loaded.files,
                        )));
                    ui.skin.set(loaded.skin);
                }
                Err(_) => {
                    rejected += 1;
                    assert!(Arc::ptr_eq(&old, &ui.skin.get_untracked()));
                }
            }
        }
        hot_frames.push(frame(&g, &mut docs, &mut painters, &targets, ms).0);
        ms += 33;
    }
    let reused = worker.join().unwrap();
    assert!(reused > 0);
    assert_eq!(rejected, 5);
    let hot_cache = shader_effects::cache_stats();
    drop(docs);
    drop(painters);
    drop(other_painters);
    drop(targets);
    drop(other_targets);
    shader_effects::collect_garbage();
    sweep::collect_garbage();
    let closed = shader_effects::cache_stats();
    assert_eq!((closed.masks, closed.scales, closed.uniforms), (0, 0, 0));
    assert_eq!(sweep::cache_counts().1, 0);
    assert_eq!(sweep::cache_counts().2, 0);
    let report = serde_json::json!({"adapters":*ADAPTERS.lock().unwrap(),"switches":switches,"record_submit":distribution(samples),
        "two_devices_warm":prepared,"two_devices_after_240_frames":alternated,"after_resize":resized,
        "after_close":closed,"hot_reload":{"saves":32,"rejected":rejected,"reused_identical_programs":reused,
            "worker":distribution(hot_times),"render_during_reload":distribution(hot_frames),"cache":hot_cache},
        "output_isolation_pixels":true,
        "note":"3 retained scenes x 3 targets (two same-format destinations with different origins plus float output). No OBS receiver or Spout fence timing."});
    std::fs::write(out, serde_json::to_vec_pretty(&report).unwrap()).unwrap();
}
