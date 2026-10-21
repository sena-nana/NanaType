mod common;

use nana_ui_devtools::agent::AgentSession;
use nanatype::ui::{Ui, dialogue, sweep, text_light};
use nanatype_core::{Cmd, Core};
use nanatype_i18n::Locale;
use nanatype_settings::Settings;
use std::sync::Arc;

use common::frame;

const CAPTURES: &str = "NANATYPE_SONNET_CAPTURES";

#[test]
fn sonnet_renders_finite_glyph_light_and_synchronized_retirement() {
    let mut fonts = nanatype::skin_fonts::BuiltinFontSet::default();
    let registry = common::registry();
    let loaded = registry.load("nanatype.sonnet").unwrap();
    let f = &loaded.skin.fonts;
    let wanted: Vec<_> = [&f.body, &f.name, &f.accent]
        .into_iter()
        .chain(f.latin.as_ref())
        .filter_map(|f| f.file.as_deref())
        .collect();
    assert!(
        fonts
            .sync(
                wanted,
                nanatype_theme::builtin_font,
                nanatype::skin_fonts::register_with_nana_ui
            )
            .is_empty()
    );
    let families =
        [&f.body, &f.name, &f.accent].map(|f| f.file.as_deref().and_then(|r| fonts.family(r)));
    let skin = Arc::new(nanatype::skin_theme::prepare(
        &loaded.skin,
        &Default::default(),
        &families,
        "zh-CN",
    ));
    for (case, width, height, reduced, enabled, remote) in [
        ("wide", 992, 222, false, true, false),
        ("narrow", 300, 260, false, true, false),
        ("portrait", 260, 600, false, true, false),
        ("remote", 992, 222, false, true, true),
        ("reduced", 992, 222, true, true, false),
        ("disabled", 992, 222, false, false, false),
    ] {
        if std::env::var("NANATYPE_SONNET_CASE").is_ok_and(|v| v != case) {
            continue;
        }
        let base = 100_000;
        let mut settings = Settings::default();
        settings.skin.id = "nanatype.sonnet".into();
        settings.skin.advanced_effects = enabled;
        settings.playback.reduced_motion = reduced;
        settings.playback.punctuation_pauses = false;
        let root = Ui::new(Arc::new(settings.clone()), skin.clone(), Locale::ZhCn);
        root.reduced_motion.set(reduced);
        let ui = if remote { root.remote() } else { root };
        let mut core = Core::new(settings, skin.clone());
        core.handle(
            Cmd::SendMarkup("[speed=50ms]瞬息已是夜晚 A sonnet\n星光写成了诗 Light[/speed]".into()),
            base,
        );
        ui.display.set(Arc::new(core.display().clone()));
        let mut session =
            common::session().mount_at(base, width, height, move || dialogue::display_window(ui));
        session.set_gpu_renderers(sweep::renderers());
        session.set_clear(Some([0.; 4]));
        let mut entry = Vec::new();
        let times: Vec<u64> = if std::env::var_os("NANATYPE_SONNET_CAPTURES").is_some() {
            (0..=2600).step_by(40).collect()
        } else {
            vec![0, 120, 360, 1200, 2600]
        };
        for t in times {
            let pixels = frame(
                &mut session,
                ui,
                base + t,
                t,
                &format!("{case}-enter-{t:04}"),
                CAPTURES,
            );
            if t == 120 {
                entry = pixels;
            }
        }
        let full = frame(
            &mut session,
            ui,
            base + 2800,
            2800,
            &format!("{case}-stable"),
            CAPTURES,
        );
        assert_ne!(entry, full, "{case}: reveal did not progress");
        assert!(
            !text_light::animating(*ui, base + 2800),
            "{case}: light never settled"
        );
        if enabled && !reduced {
            assert!(
                text_light::cache_counts().0 > 0,
                "{case}: light geometry was not captured"
            );
        }
        let layout = session.document().context().world().layout_generation();
        frame(
            &mut session,
            ui,
            base + 2900,
            2900,
            &format!("{case}-stable-later"),
            CAPTURES,
        );
        assert_eq!(
            session.document().context().world().layout_generation(),
            layout
        );
        let fx = core.handle(Cmd::ClearDisplay, base + 3000);
        let wake = fx
            .wakes
            .iter()
            .find(|w| w.reason == nanatype_playback::WakeReason::PresentationDone)
            .copied();
        assert_eq!(wake.is_some(), enabled && !reduced);
        ui.display.set(Arc::new(core.display().clone()));
        let mut retirement = Vec::new();
        for t in (0..=720).step_by(40) {
            retirement.push(frame(
                &mut session,
                ui,
                base + 3000 + t,
                3000 + t,
                &format!("{case}-exit-{t:03}"),
                CAPTURES,
            ));
        }
        if wake.is_some() {
            assert_ne!(
                retirement[0], retirement[5],
                "{case}: retirement did not animate"
            );
            assert!(!text_light::animating(*ui, base + 3720));
        }
        if let Some(wake) = wake {
            core.handle(
                Cmd::Wake {
                    epoch: wake.epoch,
                    reason: wake.reason,
                },
                wake.at_ms,
            );
        }
        assert!(core.display().empty);
        ui.display.set(Arc::new(core.display().clone()));
        frame(
            &mut session,
            ui,
            base + 4200,
            4200,
            &format!("{case}-cleared"),
            CAPTURES,
        );
        assert!(!text_light::animating(*ui, base + 4200));
        assert!(
            !session.document().compositor_needs_tick(),
            "{case}: finite presentation still requests frames"
        );
        println!("sonnet {case}: reveal, retirement and finite clock verified");
    }
}

#[test]
fn sonnet_preview_plays_once_and_stops_its_clock() {
    let skin = common::skin("nanatype.sonnet");
    let ui = Ui::new(Arc::new(Settings::default()), skin.clone(), Locale::ZhCn);
    ui.page.set(nanatype::ui::settings::Page::Skins);
    ui.shader_preview.set(true);
    let doc = nanatype_markup::parse("十四行诗 Sonnet").doc;
    let mut session = common::session().mount_at(100_000, 992, 222, move || {
        dialogue::preview_box(
            *ui,
            dialogue::BoxCtx {
                skin: skin.clone(),
                urls: Arc::new(Default::default()),
                source: dialogue::Source::Preview,
                preview_name: "奈奈".into(),
            },
            &doc,
        )
    });
    session.set_gpu_renderers(sweep::renderers());
    frame(&mut session, ui, 100_120, 120, "preview-enter", CAPTURES);
    assert!(text_light::preview_animating(*ui, 100_120));
    frame(
        &mut session,
        ui,
        106_000,
        6000,
        "preview-finished",
        CAPTURES,
    );
    assert!(!text_light::preview_animating(*ui, 106_000));
    assert!(!session.document().compositor_needs_tick());
    ui.shader_preview.set(false);
    assert!(!text_light::preview_animating(*ui, 100_120));
}

#[test]
fn sonnet_scrolling_reuses_glyph_masks_and_layout_during_retirement() {
    let skin = common::skin("nanatype.sonnet");
    let settings = Settings::default();
    let ui = Ui::new(Arc::new(settings.clone()), skin.clone(), Locale::ZhCn);
    let mut core = Core::new(settings, skin);
    core.handle(
        Cmd::SendMarkup("窗外星光写成诗句。 A sonnet.\n".repeat(14)),
        100_000,
    );
    core.handle(Cmd::Skip, 100_001);
    ui.display.set(Arc::new(core.display().clone()));
    let mut session =
        common::session().mount_at(100_001, 992, 222, move || dialogue::display_window(ui));
    session.set_gpu_renderers(sweep::renderers());
    frame(&mut session, ui, 101_000, 999, "scroll-skipped", CAPTURES);
    assert!(
        !text_light::animating(*ui, 101_000),
        "skip must not replay every glyph's flash"
    );
    core.handle(Cmd::ClearDisplay, 101_100);
    ui.display.set(Arc::new(core.display().clone()));
    frame(&mut session, ui, 101_200, 1199, "scroll-exit", CAPTURES);
    let masks = text_light::cache_counts();
    let generation = session.document().context().world().layout_generation();
    ui.text_scroll.set(140.);
    frame(&mut session, ui, 101_260, 1259, "scroll-shifted", CAPTURES);
    assert_eq!(
        session.document().context().world().layout_generation(),
        generation
    );
    assert_eq!(
        text_light::cache_counts(),
        masks,
        "scroll only changes the sampled viewport"
    );
}
