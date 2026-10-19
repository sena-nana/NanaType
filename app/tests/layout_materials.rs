//! Stroke-time maps build on a background worker: the UI thread only takes a
//! layout snapshot, the reveal starts with the plain fade, and the finished map
//! replaces it without restarting the reveal clock.

mod common;

use nana_ui_devtools::agent::AgentSession;
use nanatype::ui::{Ui, dialogue, pipeline, sweep};
use nanatype_core::{Cmd, Core};
use nanatype_i18n::Locale;
use nanatype_settings::Settings;
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

type Clock = (Option<Arc<[f32]>>, Option<Duration>);
const CAPTURES: &str = "NANATYPE_LAYOUT_MATERIAL_CAPTURES";
fn entrance(
    session: &mut nana_ui_devtools::agent::RuntimeAgentSession,
    ui: Ui,
) -> (Option<[u32; 2]>, Clock) {
    let node = ui.text_node.get_untracked().unwrap();
    let p = session
        .document()
        .context()
        .world()
        .glyph_presentation(node)
        .unwrap();
    let size = p
        .transitions
        .entrance
        .as_ref()
        .and_then(|m| m.material.textures().first().map(|t| t.size));
    let start = p.transitions.entrance.as_ref().map(|m| m.start);
    (size, (p.reveal.as_ref().map(|r| r.at_s.clone()), start))
}

#[test]
fn stroke_maps_build_in_the_background_and_keep_the_reveal_clock() {
    let (woke, wakes) = std::sync::mpsc::channel();
    pipeline::set_waker(move || {
        let _ = woke.send(());
    });
    let registry = common::registry();
    let load = Instant::now();
    let loaded = registry.load("nanatype.love_letter").unwrap();
    let cold_ms = load.elapsed().as_secs_f64() * 1000.;
    let mut warm_ms: Vec<_> = (0..5)
        .map(|_| {
            let start = Instant::now();
            std::hint::black_box(registry.load("nanatype.love_letter").unwrap());
            start.elapsed().as_secs_f64() * 1000.
        })
        .collect();
    warm_ms.sort_by(f64::total_cmp);
    // One immutable output may feed more than one material slot.
    let mut project = nanatype_theme::PackageProject::capture(&*loaded.files).unwrap();
    let mut manifest: nanatype_theme::TomlTable = std::str::from_utf8(&project.files["skin.toml"])
        .unwrap()
        .parse()
        .unwrap();
    for field in ["textures", "uniforms"] {
        let slots = manifest["materials"]["write"][field]
            .as_array_mut()
            .unwrap();
        slots.push(slots[0].clone());
    }
    project
        .files
        .insert("skin.toml".into(), manifest.to_string().into_bytes());
    let loaded = registry.inspect_project(&project).unwrap().1.unwrap();
    let mut fonts = nana_ui::HostFontScope::new();
    let family = fonts
        .add_bytes(
            nanatype_theme::builtin_font(loaded.skin.fonts.body.file.as_ref().unwrap()).unwrap(),
        )
        .unwrap()
        .families()
        .first()
        .unwrap()
        .clone();
    let skin = Arc::new(nanatype::skin_theme::prepare(
        &loaded.skin,
        &Default::default(),
        &[Some(family.clone()), Some(family), None],
        "zh-CN",
    ));
    let mut settings = Settings::default();
    settings.skin.id = "nanatype.love_letter".into();
    settings.playback.punctuation_pauses = false;
    let ui = Ui::new(Arc::new(settings.clone()), skin.clone(), Locale::ZhCn);
    let mut core = Core::new(settings, skin);
    let base = 100_000;
    core.handle(
        Cmd::SendMarkup(
            "情书写给你，喜欢你的心，永远温柔。想念你的每一天，写下每一个字。".repeat(4),
        ),
        base,
    );
    ui.display.set(Arc::new(core.display().clone()));
    let mut session =
        common::session().mount_at(base, 932, 272, move || dialogue::display_window(ui));
    session.set_gpu_renderers(sweep::renderers());
    session.set_clear(Some([0.; 4]));
    let builds = pipeline::builds();
    let started = Instant::now();
    common::frame(&mut session, ui, base + 40, 40, "first", CAPTURES);
    let first_ms = started.elapsed().as_secs_f64() * 1000.;
    let (size, clock) = entrance(&mut session, ui);
    assert_eq!(
        size,
        Some([1, 1]),
        "the first frame must not wait for the map: placeholder slot, plain fade"
    );
    wakes
        .recv_timeout(Duration::from_secs(30))
        .expect("background map finished and woke the UI");
    let ready_ms = started.elapsed().as_secs_f64() * 1000.;
    assert_eq!(
        pipeline::builds(),
        builds,
        "nothing is built on the UI thread yet"
    );
    ui.layout_epoch.update(|e| *e += 1);
    common::frame(&mut session, ui, base + 80, 80, "ready", CAPTURES);
    let (size, after) = entrance(&mut session, ui);
    assert!(size.is_some_and(|s| s != [1, 1]), "map applied: {size:?}");
    let node = ui.text_node.get_untracked().unwrap();
    let presentation = session
        .document()
        .context()
        .world()
        .glyph_presentation(node)
        .unwrap();
    let material = &presentation.transitions.entrance.as_ref().unwrap().material;
    assert_eq!(material.textures()[0].size, material.textures()[1].size);
    assert!(Arc::ptr_eq(
        &material.textures()[0].rgba,
        &material.textures()[1].rgba
    ));
    assert_eq!(material.parameters()[0], material.parameters()[1]);
    assert_eq!(
        after, clock,
        "swapping the map keeps the running reveal clock"
    );
    // Stable frames reuse the immutable map.
    let mut frames: Vec<_> = (0..5)
        .map(|i| {
            let start = Instant::now();
            common::frame(
                &mut session,
                ui,
                base + 440 + i * 40,
                440 + i * 40,
                "stable",
                CAPTURES,
            );
            start.elapsed().as_secs_f64() * 1000.
        })
        .collect();
    frames.sort_by(f64::total_cmp);
    pipeline::settle();
    assert_eq!(pipeline::builds(), builds + 1);
    assert!(wakes.try_recv().is_err(), "no duplicate background work");
    // Font and width changes replace the map, preserving playback progress.
    let original = ui.skin.get_untracked();
    let mut changed = (*ui.settings.get_untracked()).clone();
    changed.skin.personalizations.insert(
        "nanatype.love_letter".into(),
        nanatype_settings::SkinPersonalization {
            display: nanatype_settings::SurfacePersonalization {
                size: Some([300., 300.]),
                text_size: Some(48.),
                ..Default::default()
            },
            ..Default::default()
        },
    );
    ui.skin.set(Arc::new(nanatype::skin_theme::personalized(
        &original,
        &changed,
        "nanatype.love_letter",
    )));
    ui.settings.set(Arc::new(changed));
    nanatype::clock::capture_at(base + 800, || session.set_viewport(300, 300, 1.5)).unwrap();
    let resized = Instant::now();
    common::frame(&mut session, ui, base + 800, 800, "resized", CAPTURES);
    let resize_ms = resized.elapsed().as_secs_f64() * 1000.;
    let (_, resized_clock) = entrance(&mut session, ui);
    let deadline = Instant::now() + Duration::from_secs(30);
    while pipeline::builds() == builds + 1 {
        wakes
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .unwrap();
        ui.layout_epoch.update(|e| *e += 1);
        common::frame(&mut session, ui, base + 840, 840, "resized-ready", CAPTURES);
    }
    let (size, after) = entrance(&mut session, ui);
    assert!(
        size.is_some_and(|s| {
            s != [1, 1]
                && s.into_iter()
                    .all(|v| v <= nanatype_theme::MAX_PIPELINE_TEXTURE_SIDE)
        }),
        "resized map: {size:?}"
    );
    assert_eq!(
        resized_clock, after,
        "the resized map keeps the current reveal clock"
    );
    assert_eq!(pipeline::builds(), builds + 2);
    println!(
        "{}",
        serde_json::json!({"profile":if cfg!(debug_assertions) { "debug/test" } else { "release/test" },
        "png_capture":std::env::var_os(CAPTURES).is_some(), "cold_load_ms":cold_ms, "warm_load_median_ms":warm_ms[2],
        "initial_frame_ms":first_ms, "map_ready_elapsed_ms":ready_ms, "stable_frame_readback_median_ms":frames[2], "layout_frame_ms":resize_ms,
        "measurement":"5 warm loads and 5 stable frames; frame timings include GPU readback, not pure GPU timestamps"})
    );
}
