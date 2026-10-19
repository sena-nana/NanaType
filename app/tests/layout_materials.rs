//! Stroke-time maps build on a background worker: the UI thread only takes a
//! layout snapshot, the reveal starts with the plain fade, and the finished map
//! replaces it without restarting the reveal clock.

mod common;

use nana_ui_devtools::agent::AgentSession;
use nanatype::ui::{Ui, dialogue, pipeline, sweep};
use nanatype_core::{Cmd, Core};
use nanatype_i18n::Locale;
use nanatype_settings::Settings;
use std::{sync::Arc, time::Duration};

type Clock = (Option<Arc<[f32]>>, Option<Duration>);
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
    let loaded = registry.load("nanatype.love_letter").unwrap();
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
        Cmd::SendMarkup("情书写给你，喜欢你的心，永远温柔。想念你的每一天，写下每一个字。".into()),
        base,
    );
    ui.display.set(Arc::new(core.display().clone()));
    let mut session =
        common::session().mount_at(base, 932, 272, move || dialogue::display_window(ui));
    session.set_gpu_renderers(sweep::renderers());
    session.set_clear(Some([0.; 4]));
    let builds = pipeline::builds();
    common::frame(&mut session, ui, base + 40, 40, "first", "");
    let (size, clock) = entrance(&mut session, ui);
    assert_eq!(
        size,
        Some([1, 1]),
        "the first frame must not wait for the map: placeholder slot, plain fade"
    );
    wakes
        .recv_timeout(Duration::from_secs(30))
        .expect("background map finished and woke the UI");
    assert_eq!(
        pipeline::builds(),
        builds,
        "nothing is built on the UI thread yet"
    );
    ui.layout_epoch.update(|e| *e += 1);
    common::frame(&mut session, ui, base + 80, 80, "ready", "");
    let (size, after) = entrance(&mut session, ui);
    assert!(size.is_some_and(|s| s != [1, 1]), "map applied: {size:?}");
    assert_eq!(
        after, clock,
        "swapping the map keeps the running reveal clock"
    );
    assert_eq!(pipeline::builds(), builds + 1);
    // Stable frames reuse the immutable map.
    common::frame(&mut session, ui, base + 400, 400, "stable", "");
    assert_eq!(pipeline::builds(), builds + 1);
    pipeline::settle();
    assert!(wakes.try_recv().is_err(), "no duplicate background work");
}
