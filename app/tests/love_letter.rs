mod common;

use nana_ui_devtools::agent::AgentSession;
use nanatype::ui::{Ui, dialogue, handwriting, sweep};
use nanatype_core::{Cmd, Core};
use nanatype_i18n::Locale;
use nanatype_settings::Settings;
use std::sync::Arc;

use common::frame;

const CAPTURES: &str = "NANATYPE_LOVE_LETTER_CAPTURES";

#[test]
fn love_letter_retains_font_geometry_writes_overlapping_strokes_and_dissolves_together() {
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
    let families = [Some(family.clone()), Some(family), None];
    let skin = Arc::new(nanatype::skin_theme::prepare(
        &loaded.skin,
        &Default::default(),
        &families,
        "zh-CN",
    ));
    for (case, width, height, reduced, enabled, remote, typing) in [
        ("wide", 932, 272, false, true, false, true),
        ("narrow", 300, 300, false, true, false, true),
        ("portrait", 300, 650, false, true, false, true),
        ("remote", 932, 272, false, true, true, true),
        ("reduced", 932, 272, true, true, false, true),
        ("disabled", 932, 272, false, false, false, true),
        ("instant", 932, 272, false, true, false, false),
    ] {
        if std::env::var("NANATYPE_LOVE_LETTER_CASE").is_ok_and(|v| v != case) {
            continue;
        }
        let mut settings = Settings::default();
        settings.skin.id = "nanatype.love_letter".into();
        settings.skin.advanced_effects = enabled;
        settings.playback.reduced_motion = reduced;
        settings.playback.punctuation_pauses = false;
        settings.playback.typing = typing;
        let root = Ui::new(Arc::new(settings.clone()), skin.clone(), Locale::ZhCn);
        root.reduced_motion.set(reduced);
        let ui = if remote { root.remote() } else { root };
        let mut core = Core::new(settings, skin.clone());
        let base = 100_000;
        core.handle(Cmd::SendMarkup("[speed=90ms]情书写给你，喜欢你的心，永远温柔。\n[b]想念[/b] Love ♡ [color=#A96077]喜欢[/color] 龘🙂[/speed]".into()),base);
        ui.display.set(Arc::new(core.display().clone()));
        let mut session =
            common::session().mount_at(base, width, height, move || dialogue::display_window(ui));
        session.set_gpu_renderers(sweep::renderers());
        session.set_clear(Some([0.; 4]));
        let times: Vec<_> =
            if case == "wide" && std::env::var_os("NANATYPE_LOVE_LETTER_CAPTURES").is_some() {
                (0..=4200).step_by(40).collect()
            } else {
                vec![0, 120, 240, 360, 600, 1000, 2200, 4200]
            };
        let mut early = Vec::new();
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
                early = pixels;
            }
        }
        let stable = frame(
            &mut session,
            ui,
            base + 4400,
            4400,
            &format!("{case}-stable"),
            CAPTURES,
        );
        if typing {
            assert_ne!(early, stable, "{case}: no reveal progression");
        }
        if enabled && !reduced && typing {
            let node = ui.text_node.get_untracked().unwrap();
            let p = session
                .document()
                .context()
                .world()
                .glyph_presentation(node)
                .unwrap();
            assert!(
                !p.transitions
                    .entrance
                    .as_ref()
                    .unwrap()
                    .material
                    .textures()
                    .is_empty(),
                "{case}: no stroke map"
            );
        }
        let builds = handwriting::map_builds();
        let generation = session.document().context().world().layout_generation();
        assert_eq!(
            stable,
            frame(
                &mut session,
                ui,
                base + 4500,
                4500,
                &format!("{case}-stable-later"),
                CAPTURES
            )
        );
        assert_eq!(
            handwriting::map_builds(),
            builds,
            "{case}: stable frame rebuilt map"
        );
        assert_eq!(
            session.document().context().world().layout_generation(),
            generation
        );
        let fx = core.handle(Cmd::ClearDisplay, base + 4600);
        let wake = fx
            .wakes
            .iter()
            .find(|w| w.reason == nanatype_playback::WakeReason::PresentationDone)
            .copied();
        ui.display.set(Arc::new(core.display().clone()));
        let mut exit_start = Vec::new();
        for t in (0..=720).step_by(40) {
            let pixels = frame(
                &mut session,
                ui,
                base + 4600 + t,
                4600 + t,
                &format!("{case}-exit-{t:03}"),
                CAPTURES,
            );
            if t == 0 {
                exit_start = pixels;
            } else if t == 280 && wake.is_some() {
                assert_ne!(exit_start, pixels, "{case}: no dissolve");
            }
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
            base + 5600,
            5600,
            &format!("{case}-cleared"),
            CAPTURES,
        );
        assert!(
            !session.document().compositor_needs_tick(),
            "{case}: animation did not settle"
        );
        if case == "wide" {
            core.handle(
                Cmd::SendMarkup(
                    "[speed=90ms]".to_owned()
                        + &"情书喜欢你的心永远温柔。\n".repeat(10)
                        + "[/speed]",
                ),
                base + 6000,
            );
            ui.display.set(Arc::new(core.display().clone()));
            frame(
                &mut session,
                ui,
                base + 6000,
                6000,
                "interrupted-start",
                CAPTURES,
            );
            let before = frame(
                &mut session,
                ui,
                base + 6240,
                6240,
                "interrupted-writing",
                CAPTURES,
            );
            core.handle(Cmd::ClearDisplay, base + 6240);
            ui.display.set(Arc::new(core.display().clone()));
            let frozen = frame(
                &mut session,
                ui,
                base + 6240,
                6240,
                "interrupted-frozen",
                CAPTURES,
            );
            assert_eq!(
                before, frozen,
                "clearing unfinished strokes revealed hidden ink"
            );
            let maps = handwriting::map_builds();
            let layout = session.document().context().world().layout_generation();
            ui.text_scroll.set(60.);
            frame(
                &mut session,
                ui,
                base + 6400,
                6400,
                "interrupted-scrolled",
                CAPTURES,
            );
            assert_eq!(
                handwriting::map_builds(),
                maps,
                "scroll rebuilt stroke maps"
            );
            assert_eq!(
                session.document().context().world().layout_generation(),
                layout
            );
            core.handle(Cmd::Skip, base + 6500);
            ui.display.set(Arc::new(core.display().clone()));
            frame(
                &mut session,
                ui,
                base + 7200,
                7200,
                "interrupted-cleared",
                CAPTURES,
            );
            assert!(!session.document().compositor_needs_tick());
        }
        println!("love_letter {case}: stroke map, stable layout and finite retirement verified");
    }
}

#[test]
fn love_letter_preview_and_interrupted_writing_have_finite_clocks() {
    let loaded = common::registry().load("nanatype.love_letter").unwrap();
    let skin = loaded.skin;
    let mut settings = Settings::default();
    settings.skin.id = skin.meta.id.clone();
    let ui = Ui::new(Arc::new(settings.clone()), skin.clone(), Locale::ZhCn);
    ui.shader_preview.set(true);
    ui.page.set(nanatype::ui::settings::Page::Skins);
    let doc = nanatype_markup::parse("情书 喜欢 心 永远 Love").doc;
    let mut session = common::session().mount_at(100_000, 932, 272, move || {
        dialogue::preview_box(
            ui,
            dialogue::BoxCtx {
                skin: skin.clone(),
                urls: Arc::new(Default::default()),
                source: dialogue::Source::Preview,
                preview_name: "致你".into(),
            },
            &doc,
        )
    });
    session.set_gpu_renderers(sweep::renderers());
    frame(&mut session, ui, 100_000, 0, "preview-start", CAPTURES);
    frame(&mut session, ui, 100_200, 200, "preview-writing", CAPTURES);
    frame(
        &mut session,
        ui,
        106_000,
        6000,
        "preview-finished",
        CAPTURES,
    );
    assert!(!session.document().compositor_needs_tick());
}
