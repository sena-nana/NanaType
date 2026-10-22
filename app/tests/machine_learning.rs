mod common;

use nana_ui_devtools::agent::AgentSession;
use nanatype::ui::{Ui, dialogue, sweep, text_scramble};
use nanatype_core::{Cmd, Core};
use nanatype_i18n::Locale;
use nanatype_settings::Settings;
use std::sync::Arc;

use common::frame;

const CAPTURES: &str = "NANATYPE_ML_CAPTURES";

#[test]
fn scramble_renders_locks_and_retires_without_relayout() {
    let fixtures = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../target/machine-learning-fixtures");
    std::fs::create_dir_all(&fixtures).unwrap();
    nanatype::services::media::init(&fixtures).unwrap();
    let picture = fixtures.join("red.png");
    image::save_buffer(
        &picture,
        &[255u8, 48, 24, 255].repeat(16 * 16),
        16,
        16,
        image::ColorType::Rgba8,
    )
    .unwrap();
    let picture_url = nanatype::services::media::url(&picture, Some(&fixtures)).unwrap();
    let skin = common::skin("nanatype.machine_learning");
    for (case, w, h, reduced, enabled, remote) in [
        ("wide", 892, 232, false, true, false),
        ("narrow", 300, 260, false, true, false),
        ("portrait", 280, 600, false, true, false),
        ("remote", 892, 232, false, true, true),
        ("reduced", 892, 232, true, true, false),
        ("disabled", 892, 232, false, false, false),
    ] {
        let mut settings = Settings::default();
        settings.skin.id = "nanatype.machine_learning".into();
        settings.skin.advanced_effects = enabled;
        settings.playback.reduced_motion = reduced;
        settings.general.speaker_name = "奈奈 / ML".into();
        let root = Ui::new(Arc::new(settings.clone()), skin.clone(), Locale::ZhCn);
        root.reduced_motion.set(reduced);
        let ui = if remote { root.remote() } else { root };
        ui.asset_urls
            .set(Arc::new(std::collections::HashMap::from([(
                "lib:ml/test".into(),
                picture_url.clone(),
            )])));
        let mut core = Core::new(settings, skin.clone());
        core.handle(
            Cmd::SendMarkup(
                "[speed=15ms]模型正在学习 ML 2026\n[ruby=しゅうそく]收敛[/ruby] é 👩‍💻 [sticker=ml/test h=1] [at=1 name=奈奈][/speed]"
                    .into(),
            ),
            100_000,
        );
        let original = core.display().doc.clone();
        ui.display.set(Arc::new(core.display().clone()));
        let mut session =
            common::session().mount_at(100_000, w, h, move || dialogue::display_window(ui));
        session.set_gpu_renderers(sweep::renderers());
        session.set_clear(Some([0.; 4]));
        frame(
            &mut session,
            ui,
            100_000,
            0,
            &format!("{case}-enter-000"),
            CAPTURES,
        );
        let a = frame(
            &mut session,
            ui,
            100_060,
            60,
            &format!("{case}-enter-060"),
            CAPTURES,
        );
        let generation = session.document().context().world().layout_generation();
        let b = frame(
            &mut session,
            ui,
            100_085,
            85,
            &format!("{case}-enter-085"),
            CAPTURES,
        );
        if enabled && !reduced {
            assert_ne!(a, b, "{case}: substitution/reveal did not change");
            assert!(text_scramble::animating(*ui, 100_060));
        } else {
            assert!(!text_scramble::animating(*ui, 100_060));
        }
        assert_eq!(
            session.document().context().world().layout_generation(),
            generation,
            "{case}: animation relaid out text"
        );
        frame(
            &mut session,
            ui,
            100_140,
            140,
            &format!("{case}-enter-140"),
            CAPTURES,
        );
        let full = frame(
            &mut session,
            ui,
            101_200,
            1200,
            &format!("{case}-stable"),
            CAPTURES,
        );
        let red = full
            .as_chunks::<4>()
            .0
            .iter()
            .position(|p| p[0] > 245 && p[1].abs_diff(48) < 5 && p[2].abs_diff(24) < 5)
            .expect("inline picture must render its actual pixels");
        assert!(!text_scramble::animating(*ui, 101_200));
        assert_eq!(core.display().doc, original);
        if enabled && !reduced {
            assert!(
                text_scramble::cache_counts().0 > 0,
                "{case}: no GPU slots captured"
            );
        }
        let fx = core.handle(Cmd::ClearDisplay, 102_000);
        let wake = fx
            .wakes
            .iter()
            .find(|w| w.reason == nanatype_playback::WakeReason::PresentationDone)
            .copied();
        assert_eq!(wake.is_some(), enabled && !reduced);
        ui.display.set(Arc::new(core.display().clone()));
        let mut phases = Vec::new();
        for t in [0, 25, 80, 160, 240, 320, 399, 400] {
            phases.push(frame(
                &mut session,
                ui,
                102_000 + t,
                2000 + t,
                &format!("{case}-exit-{t:03}"),
                CAPTURES,
            ));
        }
        if let Some(wake) = wake {
            assert_eq!(wake.at_ms, 102_400);
            assert_ne!(
                phases[0], phases[1],
                "{case}: random characters stayed fixed"
            );
            assert_ne!(
                phases[2], phases[5],
                "{case}: random removal did not progress"
            );
            let early = &phases[2][red * 4..red * 4 + 4];
            let late = &phases[4][red * 4..red * 4 + 4];
            assert!(
                early[0] > late[0] && late[0] > 60,
                "{case}: the image did not stay in place and fade: {early:?} → {late:?}"
            );
            assert!(!text_scramble::animating(*ui, 102_400));
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
            103_000,
            3000,
            &format!("{case}-cleared"),
            CAPTURES,
        );
        assert!(!text_scramble::animating(*ui, 103_000));
        assert!(
            !session.document().compositor_needs_tick(),
            "{case}: animation did not settle"
        );
        println!("machine_learning {case}: substitution, retirement and stable layout verified");
    }
}

#[test]
fn preview_is_finite_and_static_surfaces_do_not_scramble() {
    let skin = common::skin("nanatype.machine_learning");
    let ui = Ui::new(Arc::new(Settings::default()), skin.clone(), Locale::ZhCn);
    ui.page.set(nanatype::ui::settings::Page::Skins);
    ui.shader_preview.set(true);
    let text = nanatype_markup::parse("机器学习 ML").doc;
    let mut session = common::session().mount_at(100_000, 892, 232, move || {
        dialogue::preview_box(
            *ui,
            dialogue::BoxCtx {
                skin: skin.clone(),
                urls: Arc::new(Default::default()),
                source: dialogue::Source::Preview,
                preview_name: "奈奈".into(),
            },
            &text,
        )
    });
    session.set_gpu_renderers(sweep::renderers());
    frame(&mut session, ui, 100_060, 60, "preview-enter", CAPTURES);
    assert!(text_scramble::preview_animating(*ui, 100_060));
    frame(
        &mut session,
        ui,
        106_000,
        6000,
        "preview-finished",
        CAPTURES,
    );
    assert!(!text_scramble::preview_animating(*ui, 106_000));
    assert!(!session.document().compositor_needs_tick());
    let skin = ui.skin.get_untracked();
    assert!(dialogue::Base::editor(&skin).scramble.is_none());
    let entrance = nanatype_model::Entrance {
        kind: nanatype_model::EntranceKind::None,
        ms: 0,
    };
    assert!(
        dialogue::Base::history(&skin, entrance, false)
            .scramble
            .is_none()
    );
}

#[test]
fn portrait_long_names_keep_their_own_rail_and_history_keeps_the_same_frame() {
    let skin = common::skin("nanatype.machine_learning");
    assert_eq!(
        skin.history.bubble_contour,
        skin.dialogue.box_.shape.contour
    );
    assert_eq!(skin.history.bubble_color, skin.dialogue.box_.color);
    assert_eq!(
        skin.history.bubble_border,
        Some(skin.dialogue.box_.border_color)
    );
    assert_eq!(skin.history.entrance, nanatype_theme::HistoryEntrance::None);
    for size in [[300., 240.], [280., 600.], [160., 600.], [400., 401.]] {
        let resolved = nanatype_theme::responsive_dialogue(&skin, size);
        assert_eq!(
            resolved.dialogue.nameplate.color.a, 0,
            "{size:?}: nameplate lost its transparent fill"
        );
        assert_eq!(
            resolved.dialogue.nameplate.text_color, skin.dialogue.nameplate.text_color,
            "{size:?}: nameplate lost its cyan text"
        );
    }
    for (w, h, vertical) in [(280, 600), (300, 1200), (160, 600), (400, 401)]
        .into_iter()
        .flat_map(|(w, h)| [(w, h, true), (w, h, false)])
    {
        let mut settings = Settings::default();
        settings.overlays.display.vertical_text = vertical;
        settings.skin.id = "nanatype.machine_learning".into();
        settings.skin.advanced_effects = true;
        let name = "奈奈 / Machine Learning · 一个很长的姓名";
        settings.general.speaker_name = name.into();
        let ui = Ui::new(Arc::new(settings.clone()), skin.clone(), Locale::ZhCn);
        let mut core = Core::new(settings, skin.clone());
        core.handle(
            Cmd::SendMarkup("模型正在学习，文字保持清晰。".into()),
            100_000,
        );
        core.handle(Cmd::Skip, 101_000);
        ui.display.set(Arc::new(core.display().clone()));
        let mut session =
            common::session().mount_at(100_000, w, h, move || dialogue::display_window(ui));
        session.set_gpu_renderers(sweep::renderers());
        session.set_clear(Some([0.; 4]));
        common::settle_shaders(|| {
            frame(
                &mut session,
                ui,
                101_200,
                1200,
                &format!(
                    "portrait-rail-{w}{}",
                    if vertical { "" } else { "-horizontal" }
                ),
                CAPTURES,
            );
        });
        let rgba = frame(
            &mut session,
            ui,
            101_200,
            1200,
            &format!(
                "portrait-rail-{w}{}",
                if vertical { "" } else { "-horizontal" }
            ),
            CAPTURES,
        );
        let label = session
            .accessibility_dump()
            .into_iter()
            .find(|node| node.label.as_deref() == Some(name))
            .expect("long name stays accessible");
        let port = session
            .document()
            .context()
            .world()
            .layout_box(ui.text_port.get_untracked().unwrap())
            .unwrap();
        assert!(label.bounds.width > 0. && label.bounds.height > 0.);
        if !vertical {
            // The portrait layout still applies; the name just reads across.
            assert!(
                label.bounds.x >= 0. && label.bounds.x + label.bounds.width <= w as f32 + 0.5,
                "{w}x{h}: horizontal name leaves the window: {:?}",
                label.bounds
            );
            continue;
        }
        assert!(
            port.x + port.width + 8. <= label.bounds.x,
            "{w}x{h}: long name overlaps the body: port={port:?}, name={:?}",
            label.bounds
        );
        let purple = |x: u32, y: u32| {
            let p = &rgba[((y * w + x) * 4) as usize..][..4];
            p[2] > 70 && u16::from(p[2]) > u16::from(p[1]) + 20
        };
        if w >= 280 {
            assert!(
                (16..60).any(|y| (16..w - 16).any(|x| purple(x, y))),
                "{w}x{h}: the network must actually render"
            );
        }
        // The name itself is lavender like the network; only its rail around it
        // must stay clear.
        let b = &label.bounds;
        let in_name = |x: u32, y: u32| {
            let (x, y) = (x as f32, y as f32);
            x >= b.x - 2. && x <= b.x + b.width + 2. && y >= b.y - 2. && y <= b.y + b.height + 2.
        };
        for y in 16..60 {
            for x in label.bounds.x.floor().max(0.) as u32..w - 16 {
                if in_name(x, y) {
                    continue;
                }
                assert!(
                    !purple(x, y),
                    "{w}x{h}: a network node overlaps the name rail"
                );
            }
        }
    }
}

#[test]
fn clipped_mirror_middle_edits_leave_the_unchanged_suffix_visible() {
    let loaded = common::registry()
        .load("nanatype.machine_learning")
        .unwrap();
    let mut skin = (*loaded.skin).clone();
    skin.dialogue.overflow = nanatype_model::Overflow::Clip;
    let skin = Arc::new(skin);
    let mut settings = Settings::default();
    settings.general.input_mode = nanatype_settings::InputMode::Mirror;
    let ui = Ui::new(Arc::new(settings.clone()), skin.clone(), Locale::ZhCn);
    let mut core = Core::new(settings, skin);
    core.handle(
        Cmd::DraftChanged(nanatype_model::Document::plain("11111")),
        100_000,
    );
    ui.display.set(Arc::new(core.display().clone()));
    let mut session =
        common::session().mount_at(100_000, 892, 232, move || dialogue::display_window(ui));
    session.set_gpu_renderers(sweep::renderers());
    frame(&mut session, ui, 100_000, 0, "clip-initial", CAPTURES);
    let before = frame(&mut session, ui, 100_400, 400, "clip-before", CAPTURES);
    core.handle(
        Cmd::DraftChanged(nanatype_model::Document::plain("11211")),
        100_500,
    );
    ui.display.set(Arc::new(core.display().clone()));
    let after = frame(&mut session, ui, 100_501, 501, "clip-middle-edit", CAPTURES);
    let region = |data: &[u8]| {
        (66..110)
            .flat_map(|y| {
                (106..132).map(move |x| {
                    let i = (y * 892 + x) * 4;
                    <[u8; 4]>::try_from(&data[i..i + 4]).unwrap()
                })
            })
            .collect::<Vec<_>>()
    };
    let before = region(&before);
    assert!(
        before.iter().any(|p| p[1] > 150),
        "fixture must include the unchanged last digit"
    );
    assert_eq!(
        before,
        region(&after),
        "a middle edit restarted the unchanged suffix in clip mode"
    );
}
