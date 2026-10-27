mod common;

use nana_ui::runtime::Stack;
use nana_ui::runtime::view::{signal, widget};
use nana_ui_core::LengthSpec;
use nana_ui_devtools::agent::AgentSession;
use nanatype::ui::{surface::Surface, sweep};
use nanatype_model::Rgba8;
use nanatype_theme::{ContourPreset, SweepSpec};
use std::{sync::Arc, time::Duration};

use common::pixel;

#[test]
fn combined_effects_keep_ruby_objects_and_partner_animation_without_layout() {
    use nanatype::ui::{Ui, dialogue};
    use nanatype_core::{Cmd, Core};
    use nanatype_i18n::Locale;
    use nanatype_settings::Settings;
    let markup = "[wave][rainbow][pulse][flicker]中文 [ruby=かんじ]漢字[/ruby] [sticker=cats/hi h=1.2] [at=1 name=奈奈][/flicker][/pulse][/rainbow][/wave]";
    let parsed = nanatype_markup::parse(markup);
    assert!(parsed.diagnostics.is_empty());
    assert_eq!(
        nanatype_markup::parse(&nanatype_markup::serialize(&parsed.doc)).doc,
        parsed.doc
    );
    for remote in [false, true] {
        let skin = common::skin("nanatype.vanilla");
        let settings = Settings::default();
        let ui = Ui::new(Arc::new(settings.clone()), skin.clone(), Locale::ZhCn);
        let mut core = Core::new(settings, skin);
        core.handle(Cmd::SendMarkup(markup.into()), 0);
        core.handle(Cmd::Skip, 1);
        ui.display.set(Arc::new(core.display().clone()));
        ui.peer_display.set(ui.display.get_untracked());
        ui.now.set(100_000);
        ui.peer_now.set(100_000);
        let mut session = common::session().mount_at(100_000, 796, 236, move || {
            dialogue::display_window(if remote { ui.remote() } else { ui })
        });
        let (_, first) = session.screenshot_rgba().unwrap();
        let layout = session.document().context().world().layout_generation();
        session
            .document_mut()
            .sync_presentation_clock(Duration::from_millis(250));
        assert!(session.flush().unwrap().is_idle());
        let (_, next) = session.screenshot_rgba().unwrap();
        assert_ne!(
            first, next,
            "all effects continue on the selected display track"
        );
        assert_eq!(
            session.document().context().world().layout_generation(),
            layout
        );
        assert!(session.document().compositor_needs_tick());
    }
}

#[test]
fn finite_reveals_and_history_presence_stop_requesting_frames() {
    use nanatype::ui::{Ui, dialogue, history};
    use nanatype_core::{Cmd, Core};
    use nanatype_i18n::Locale;
    use nanatype_settings::Settings;
    for (markup, reduced) in [
        ("[speed=20ms]节奏 Only pacing[/speed]", false),
        ("[fade]入场 Only intro[/fade]", false),
        ("[wave amp=0]零幅度 Zero amplitude[/wave]", false),
        ("[wave]减少动态 Reduced[/wave]", true),
        ("[pause]等待 Paused reveal", false),
    ] {
        for history_window in [false, true] {
            let skin = common::skin("nanatype.vanilla");
            let settings = Settings::default();
            let ui = Ui::new(Arc::new(settings.clone()), skin.clone(), Locale::ZhCn);
            ui.reduced_motion.set(reduced);
            let mut core = Core::new(settings, skin);
            core.handle(Cmd::SendMarkup(markup.into()), 0);
            ui.display.set(Arc::new(core.display().clone()));
            ui.history
                .set(Arc::new(core.history().iter().cloned().collect()));
            let mut session = common::session().mount_at(0, 796, 540, move || {
                use nana_ui::runtime::view::IntoView;
                if history_window {
                    history::history_window(ui).into_any()
                } else {
                    dialogue::display_window(ui).into_any()
                }
            });
            session.flush().unwrap();
            // Include presence completion, layer-demotion hold and reveal fade.
            for ms in (16..=4000).step_by(16) {
                session
                    .document_mut()
                    .context_mut()
                    .advance_animations(Duration::from_millis(ms));
                session
                    .document_mut()
                    .sync_presentation_clock(Duration::from_millis(ms));
                nanatype::clock::capture_at(ms, || session.flush().unwrap());
            }
            assert!(
                !session.document().compositor_needs_tick(),
                "{markup}, history={history_window}"
            );
        }
    }
}

#[test]
fn ornaments_animate_without_runtime_work_and_stop_when_reduced_or_cleared() {
    use nanatype::ui::{Ui, dialogue, shader_effects};
    use nanatype_core::{Cmd, Core, DisplayView};
    use nanatype_i18n::Locale;
    use nanatype_settings::Settings;
    let registry = common::registry();
    let skin = registry.load("nanatype.sakura").unwrap().skin;
    let settings = Settings::default();
    let ui = Ui::new(Arc::new(settings.clone()), skin.clone(), Locale::ZhCn);
    let mut core = Core::new(settings, skin.clone());
    core.handle(Cmd::SendMarkup("静止台词，装饰仍在动".into()), 0);
    core.handle(Cmd::Skip, 1);
    ui.display.set(Arc::new(core.display().clone()));
    let mut session = common::session().mount_at(0, 796, 236, move || dialogue::display_window(ui));
    session.flush().unwrap();
    ui.now.set(1000);
    session
        .document_mut()
        .sync_presentation_clock(Duration::from_millis(1000));
    session.flush().unwrap();
    assert!(
        shader_effects::animating(*ui),
        "visible WGSL ornaments request render-only frames"
    );
    assert!(
        !session.document().compositor_needs_tick(),
        "WGSL loops must not drive the Runtime compositor clock"
    );
    let layout = session.document().context().world().layout_generation();
    session
        .document_mut()
        .sync_presentation_clock(Duration::from_millis(1100));
    assert!(
        session.flush().unwrap().is_idle(),
        "compositor ticks do not write Runtime styles"
    );
    assert_eq!(
        layout,
        session.document().context().world().layout_generation()
    );
    ui.reduced_motion.set(true);
    session.flush().unwrap();
    assert!(
        !shader_effects::animating(*ui),
        "reduced motion removes ornament loops"
    );
    ui.reduced_motion.set(false);
    session.flush().unwrap();
    assert!(shader_effects::animating(*ui));
    ui.display
        .set(Arc::new(DisplayView::empty(core.display().entrance)));
    ui.now.set(10_000);
    nanatype::clock::capture_at(10_000, || session.flush().unwrap());
    assert!(
        !shader_effects::animating(*ui),
        "an empty display never retains ornament loops"
    );
}

#[test]
fn chat_materializes_the_viewport_and_keeps_scrolling_available() {
    use nanatype::ui::{Ui, danmaku};
    use nanatype_core::{Cmd, Core};
    use nanatype_i18n::Locale;
    use nanatype_settings::Settings;
    let skin = common::skin("nanatype.halo");
    let mut settings = Settings::default();
    settings.history.max_items = 1000;
    let ui = Ui::new(Arc::new(settings.clone()), skin.clone(), Locale::ZhCn);
    let mut core = Core::new(settings, skin);
    for i in 0..1000 {
        core.handle(
            Cmd::SendMarkup(format!("消息 {i} {}", "长文本 ".repeat(i % 6 + 1))),
            0,
        );
        core.handle(Cmd::Skip, 1);
    }
    ui.history
        .set(Arc::new(core.history().iter().cloned().collect()));
    let mut session = common::session().mount(400, 600, move || danmaku::chat_panel(ui));
    session.flush().unwrap();
    let mounted = session.document().context().world().len();
    assert!(
        mounted < 1000,
        "a viewport does not mount 1000 message trees: {mounted}"
    );
    session.set_viewport(400, 1200, 1.0).unwrap();
    session.flush().unwrap();
    let tall = session.document().context().world().len();
    assert!(
        tall > mounted && tall < 1000,
        "mounted nodes follow viewport height: {mounted} -> {tall}"
    );
    session.set_viewport(400, 600, 1.0).unwrap();
    session.flush().unwrap();
    assert!(
        session.document().scene().primitive_count() < 500,
        "1000 messages keep only viewport nodes"
    );
    session.scroll_by(200.0, 300.0, 0.0, -450.0).unwrap();
    assert!(session.document().scene().primitive_count() < 500);
    ui.chat_width.set(320.0);
    session.flush().unwrap();
    assert!(
        session.document().scene().primitive_count() < 500,
        "resize remeasures the virtual viewport"
    );
}

#[test]
fn sweep_is_clipped_premultiplied_and_retained_across_dpi_and_opacity() {
    for scale in [1.0, 1.5, 2.0] {
        let opacity = signal(1.0);
        let surface = Surface {
            contour: Some(ContourPreset::Cloud),
            fill: Some([0.0, 0.0, 0.0, 0.5]),
            radii: [12.0; 4],
            ..Surface::default()
        };
        let mut session = common::session().scale(scale).mount(320, 120, move || {
            widget(
                Stack::column(0.0)
                    .width(LengthSpec::Px(320.0))
                    .height(LengthSpec::Px(120.0)),
            )
            .bind(move |node: &mut Stack| {
                nanatype::ui::restyle(node, |l| l.opacity = Some(opacity.get()))
            })
            .children(sweep::view(
                surface,
                SweepSpec {
                    enabled: true,
                    color: Rgba8::WHITE,
                    intensity: 0.6,
                    period_s: 4.0,
                },
                true,
            ))
        });
        session.set_clear(Some([0.0; 4]));
        session.set_gpu_renderers(sweep::renderers());
        let (size, first) =
            nanatype::clock::capture_at(2000, || session.screenshot_rgba().unwrap());
        let center = pixel(
            &first,
            size.width,
            (160.0 * scale) as u32,
            (60.0 * scale) as u32,
        );
        assert!(
            (65..=85).contains(&center[3]),
            "mask alpha * intensity at {scale}: {center:?}"
        );
        assert_eq!(
            pixel(&first, size.width, 0, 0)[3],
            0,
            "cloud corner stays transparent"
        );
        let layout = session.document().context().world().layout_generation();
        opacity.set(0.5);
        let (_, faded) = nanatype::clock::capture_at(2000, || session.screenshot_rgba().unwrap());
        let center_faded = pixel(
            &faded,
            size.width,
            (160.0 * scale) as u32,
            (60.0 * scale) as u32,
        );
        assert!(
            (i16::from(center_faded[3]) * 2 - i16::from(center[3])).abs() <= 3,
            "inherited alpha at {scale}: {center_faded:?}"
        );
        assert_eq!(
            layout,
            session.document().context().world().layout_generation(),
            "opacity is presentation only"
        );
        let (_, next) = nanatype::clock::capture_at(500, || session.screenshot_rgba().unwrap());
        assert_ne!(faded, next, "only the time uniform moves the sweep");
    }
}

#[test]
fn sweep_nine_slice_alpha_and_failed_reload_keep_the_surface_route() {
    use nana_ui::runtime::view::{IntoView, dynamic};
    use nanatype::ui::surface::nine_slice;
    let folder = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../target/render-pipeline/gpu-fixtures");
    std::fs::create_dir_all(&folder).unwrap();
    let path = folder.join("sweep-frame.png");
    image::RgbaImage::from_pixel(6, 6, image::Rgba([255, 255, 255, 128]))
        .save(&path)
        .unwrap();
    let frame = nanatype_theme::NineSlice {
        image: "sweep-frame.png".into(),
        slice: [2; 4],
        scale: 1.0,
        repeat: false,
        layout_inset: true,
        fill: true,
    };
    for scale in [1.0, 1.5, 2.0] {
        let surface = Surface {
            fill: Some([1.0; 4]),
            contour: Some(ContourPreset::Cloud),
            frame: Some(nine_slice(&frame, nanatype::assets::file_url(&path))),
            ..Default::default()
        };
        let current = signal(Arc::new(surface.clone()));
        let mut session = common::session().scale(scale).mount(80, 40, move || {
            widget(
                Stack::column(0.0)
                    .width(LengthSpec::Px(80.0))
                    .height(LengthSpec::Px(40.0)),
            )
            .children(
                dynamic(
                    move || current.get(),
                    move |surface| {
                        sweep::view(
                            (**surface).clone(),
                            SweepSpec {
                                enabled: true,
                                intensity: 0.6,
                                ..Default::default()
                            },
                            false,
                        )
                        .into_any()
                    },
                )
                .css(nana_ui::runtime::view::css! { position: absolute; inset: 0; }),
            )
        });
        session.set_clear(Some([0.0; 4]));
        session.set_gpu_renderers(sweep::renderers());
        let (size, first) = session.screenshot_rgba().unwrap();
        let center = pixel(
            &first,
            size.width,
            (40.0 * scale) as u32,
            (20.0 * scale) as u32,
        );
        assert!(
            (65..=85).contains(&center[3]),
            "nine-slice alpha at {scale}: {center:?}"
        );
        let mut missing = surface.clone();
        missing.frame = Some(nine_slice(
            &frame,
            "file:///nanatype-no-such-frame.png".into(),
        ));
        current.set(Arc::new(missing));
        let (_, missing) = session.screenshot_rgba().unwrap();
        assert!(
            missing.as_chunks::<4>().0.iter().all(|p| p[3] == 0),
            "missing frame emitted light at {scale}"
        );
        let mut contour = surface.clone();
        contour.frame = None;
        current.set(Arc::new(contour));
        let (_, contour) = session.screenshot_rgba().unwrap();
        assert!(
            pixel(
                &contour,
                size.width,
                (40.0 * scale) as u32,
                (20.0 * scale) as u32
            )[3] > 130
        );
    }
}
