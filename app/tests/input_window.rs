//! Ordinary input chrome around a fixed canvas fitted beside a resizable sidebar.

mod common;

use std::{path::PathBuf, sync::Arc, time::Duration};

use nana_ui::runtime::StableNodeId;
use nana_ui_devtools::agent::{AccessibilityDumpNode, AgentSession, RuntimeAgentSession};
use nanatype::ui::{Ui, input, settings};
use nanatype_i18n::Locale;
use nanatype_model::Document;
use nanatype_settings::Settings;

fn preview_settings() -> Settings {
    let mut settings = Settings::default();
    settings.general.input_skin_preview = true;
    settings
}

#[test]
fn the_input_skin_preview_plays_the_skins_animated_programs() {
    let ui = Ui::new(
        Arc::new(preview_settings()),
        common::skin("nanatype.forest"),
        Locale::ZhCn,
    );
    let mut session = common::session().mount(1200, 640, move || input::input_window(ui));
    session.set_gpu_renderers(nanatype::ui::sweep::renderers());
    for _ in 0..3 {
        session.flush().unwrap();
    }
    assert!(nanatype::ui::shader_effects::input_animating(*ui));
    // Turning the preview off drops the box's programs and their frames.
    ui.settings
        .update(|s| Arc::make_mut(s).general.input_skin_preview = false);
    for _ in 0..3 {
        session.flush().unwrap();
    }
    assert!(!nanatype::ui::shader_effects::input_animating(*ui));
}

#[test]
fn plain_text_fills_the_editor_column_across_skin_sidebar_size_and_source_changes() {
    let registry = common::registry();
    for (skin_id, dark, scale) in [("nanatype.halo", false, 1.0), ("nanatype.night", true, 2.0)] {
        let mut skin = (*registry.load(skin_id).unwrap().skin).clone();
        skin.dialogue.box_.margin = [100.0; 4];
        skin.dialogue.padding = [150.0; 4];
        skin.dialogue.box_.width_fraction = 0.4;
        let ui = Ui::new(Arc::new(Settings::default()), Arc::new(skin), Locale::ZhCn);
        let text = "不受皮肤限制 👩‍💻\n".repeat(50);
        ui.draft.set(Arc::new(Document::plain(&text)));
        ui.source.update(|s| s.text = text.clone());
        let (mut session, (_, (editor, source, _))) = common::session()
            .themed(dark)
            .scale(scale)
            .mount_output(1200, 640, move || input::input_window(ui));
        for (w, h, sidebar) in [(720, 360, 300.0), (1200, 640, 480.0)] {
            session.set_viewport(w, h, scale).unwrap();
            settle(&mut session);
            let width = keyed(&session, "input-insertion-dock").bounds.width;
            drag_sidebar(&mut session, width - sidebar);
            for source_mode in [false, true] {
                ui.source_mode.set(source_mode);
                settle(&mut session);
                let node = if source_mode {
                    source.stable_id()
                } else {
                    editor.stable_id()
                };
                let world = session.document().context().world();
                let bounds = world.layout_box(node).unwrap();
                let port = keyed(&session, "input-preview-scroll");
                assert!((bounds.x - port.bounds.x - 16.0).abs() < 1.0);
                assert!((bounds.y - port.bounds.y - 16.0).abs() < 1.0);
                assert!((bounds.width - port.bounds.width + 32.0).abs() < 1.0);
                assert!((bounds.height - port.bounds.height + 32.0).abs() < 1.0);
                assert_eq!(
                    world
                        .layout_pointer_position(node, bounds.x, bounds.y)
                        .unwrap(),
                    (bounds.x, bounds.y)
                );
                let offset = world
                    .scroll_offset(StableNodeId::new(port.id).unwrap())
                    .unwrap();
                assert_eq!((offset.x, offset.y), (0.0, 0.0));
            }
        }
        assert_eq!(ui.draft.get_untracked().plain_text(), text);
    }
}

fn keyed(session: &RuntimeAgentSession, key: &str) -> AccessibilityDumpNode {
    session
        .accessibility_dump()
        .into_iter()
        .find(|n| {
            n.agent_path
                .as_deref()
                .is_some_and(|p| p.rsplit('/').next() == Some(key))
        })
        .unwrap_or_else(|| panic!("missing {key}"))
}

fn assert_preview_fits(session: &RuntimeAgentSession) {
    let canvas = keyed(session, "input-preview-canvas");
    let port = keyed(session, "input-preview-scroll").bounds;
    let sidebar = keyed(session, "input-insertion-dock").bounds;
    let world = session.document().context().world();
    let node = StableNodeId::new(canvas.id).unwrap();
    let layout = world.layout_box(node).unwrap();
    let (left, top) = world
        .layout_pointer_position(node, layout.x, layout.y)
        .unwrap();
    let (right, bottom) = world
        .layout_pointer_position(node, layout.x + layout.width, layout.y + layout.height)
        .unwrap();
    assert!(
        left >= port.x + 15.0 && right <= port.x + port.width - 15.0,
        "entire preview must fit beside sidebar: {left}..{right}, port={port:?}"
    );
    assert!(
        right < sidebar.x,
        "preview must never extend behind sidebar"
    );
    assert!(top >= port.y + 15.0 && bottom > top);
    assert!(
        (right - left) <= layout.width + 0.1,
        "never upscale preview"
    );
}

fn click_label(session: &mut RuntimeAgentSession, label: &str, role: &str) {
    settle(session);
    for _ in 0..3 {
        session.flush().unwrap();
    }
    let node = session
        .accessibility_dump()
        .into_iter()
        .find(|n| n.role == role && n.label.as_deref() == Some(label))
        .unwrap_or_else(|| panic!("missing {role}: {label}"));
    click_node(session, node);
}

fn click_keyed(session: &mut RuntimeAgentSession, key: &str) {
    settle(session);
    for _ in 0..3 {
        session.flush().unwrap();
    }
    click_node(session, keyed(session, key));
}

fn shown(session: &RuntimeAgentSession, key: &str) -> bool {
    session.accessibility_dump().iter().any(|n| {
        n.agent_path
            .as_deref()
            .is_some_and(|p| p.rsplit('/').next() == Some(key))
    })
}

/// Opens a sidebar panel from the rail, unless it is already open (pressing
/// the open panel's icon folds the sidebar).
fn open_panel(session: &mut RuntimeAgentSession, tab: &str) {
    settle(session);
    if !shown(session, &format!("input-dock-{tab}-panel")) {
        click_keyed(session, &format!("input-dock-rail-{tab}"));
    }
    assert!(
        shown(session, &format!("input-dock-{tab}-panel")),
        "{tab} opens"
    );
}

fn drag_sidebar(session: &mut RuntimeAgentSession, delta: f32) {
    settle(session);
    let split = keyed(session, "input-workspace-split");
    let pane = nana_ui::runtime::Entity::<nana_ui::runtime::SplitPane>::from_stable_id(
        StableNodeId::new(split.id).unwrap(),
    );
    let handle = session
        .document()
        .context()
        .read(pane, |p| p.handle.unwrap())
        .unwrap();
    let b = session
        .document()
        .context()
        .world()
        .layout_box(handle)
        .unwrap();
    let x = b.x + b.width / 2.0;
    let y = b.y + b.height / 2.0;
    session
        .pointer_event(nana_ui::PointerPhase::Move, x, y, -1, 0)
        .unwrap();
    session
        .pointer_event(nana_ui::PointerPhase::Down, x, y, 0, 1)
        .unwrap();
    for i in 1..=5 {
        session
            .pointer_event(
                nana_ui::PointerPhase::Move,
                x + delta * i as f32 / 5.0,
                y,
                -1,
                1,
            )
            .unwrap();
    }
    session
        .pointer_event(nana_ui::PointerPhase::Up, x + delta, y, 0, 0)
        .unwrap();
    settle(session);
}

fn click_node(session: &mut RuntimeAgentSession, node: AccessibilityDumpNode) {
    let bounds = session
        .document()
        .context()
        .world()
        .layout_box(StableNodeId::new(node.id).unwrap())
        .unwrap();
    let (x, y) = (
        bounds.x + bounds.width / 2.0,
        bounds.y + bounds.height / 2.0,
    );
    // Match the native mouse adapter, including hover/tooltip updates.
    session
        .pointer_event(nana_ui::PointerPhase::Move, x, y, -1, 0)
        .unwrap();
    session.flush().unwrap();
    session.click_xy(x, y).unwrap();
    for _ in 0..3 {
        session.flush().unwrap();
    }
    settle(session);
}
fn settle(session: &mut RuntimeAgentSession) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static CLOCK: AtomicU64 = AtomicU64::new(1);
    let now = Duration::from_secs(CLOCK.fetch_add(1, Ordering::Relaxed));
    session.document_mut().context_mut().advance_animations(now);
    session.document_mut().sync_presentation_clock(now);
    session.flush().unwrap();
}

#[test]
fn the_rail_reaches_every_panel_at_the_narrowest_sidebar_and_folds_it() {
    let registry = common::registry();
    for locale in Locale::ALL {
        let ui = Ui::new(
            Arc::new(Settings::default()),
            registry.load("nanatype.halo").unwrap().skin,
            locale,
        );
        let (mut session, _) =
            common::session().mount_output(720, 480, move || input::input_window(ui));
        settle(&mut session);
        // The open panel's icon folds the sidebar to its rail; the preview
        // takes the room, and any icon brings the sidebar back.
        let sidebar = keyed(&session, "input-insertion-dock").bounds;
        let port = keyed(&session, "input-preview-scroll").bounds;
        click_keyed(&mut session, "input-dock-rail-controls");
        let folded = keyed(&session, "input-insertion-dock").bounds;
        assert!(folded.width < 48.0, "{locale:?}: folded to {folded:?}");
        assert!(!shown(&session, "input-dock-panels"));
        assert!(keyed(&session, "input-preview-scroll").bounds.width > port.width + 150.0);
        click_keyed(&mut session, "input-dock-rail-stickers");
        assert!(shown(&session, "input-dock-stickers-panel"));
        assert_eq!(keyed(&session, "input-insertion-dock").bounds, sidebar);
        drag_sidebar(&mut session, 2000.0);
        let sidebar = keyed(&session, "input-insertion-dock").bounds;
        assert!(
            (sidebar.width - nanatype_settings::INPUT_SIDEBAR_MIN).abs() < 1.0,
            "{locale:?}: {sidebar:?}"
        );
        for tab in ["controls", "stickers", "phrases", "chat"] {
            let icon = keyed(&session, &format!("input-dock-rail-{tab}")).bounds;
            assert!(
                icon.width > 0.0
                    && icon.x >= sidebar.x - 0.5
                    && icon.x + icon.width <= sidebar.x + sidebar.width + 0.5,
                "{locale:?}: {tab} {icon:?} lies outside the sidebar {sidebar:?}"
            );
            open_panel(&mut session, tab);
            for other in ["controls", "stickers", "phrases", "chat"] {
                assert_eq!(
                    shown(&session, &format!("input-dock-{other}-panel")),
                    other == tab,
                    "{locale:?}: only {tab} shows"
                );
            }
        }
    }
}

#[test]
fn pinned_chat_sits_above_the_insert_panels_and_unpins_into_its_own_panel() {
    let ui = Ui::new(
        Arc::new(preview_settings()),
        common::skin("nanatype.halo"),
        Locale::ZhCn,
    );
    ui.input_dock.update(|d| d.chat_pinned = true);
    let (mut session, (_, (editor, _, _))) =
        common::session().mount_output(1200, 640, move || input::input_window(ui));
    settle(&mut session);
    let before = session
        .document()
        .context()
        .world()
        .layout_box(editor.stable_id())
        .unwrap();
    let chat = keyed(&session, "input-chat").bounds;
    let controls = keyed(&session, "input-dock-controls-panel").bounds;
    assert!(
        chat.y + chat.height <= controls.y + 0.5,
        "{chat:?} over {controls:?}"
    );
    assert!((chat.height - nanatype_settings::INPUT_CHAT_DEFAULT).abs() < 1.0);
    if let Some(out) = std::env::var_os("NANATYPE_INPUT_WINDOW_CAPTURE_DIR") {
        std::fs::create_dir_all(&out).unwrap();
        session
            .screenshot_png(PathBuf::from(out).join("pinned-chat-1200x640.png"))
            .unwrap();
    }
    // Other insert panels share the area under chat.
    open_panel(&mut session, "stickers");
    assert!(shown(&session, "input-chat"));
    // Unpinned, chat takes the whole sidebar as its own panel.
    click_keyed(&mut session, "input-chat-pin");
    assert!(shown(&session, "input-dock-chat-panel"));
    assert!(!shown(&session, "input-dock-stickers-panel"));
    let full = keyed(&session, "input-chat").bounds;
    assert!(full.height > chat.height + 100.0);
    // Pinned again, it goes back on top of the controls.
    click_keyed(&mut session, "input-chat-pin");
    assert!(ui.input_dock.with_untracked(|d| d.chat_pinned));
    assert!(shown(&session, "input-dock-controls-panel") && shown(&session, "input-chat"));
    // The pinned chat's rail icon puts it away.
    click_keyed(&mut session, "input-dock-rail-chat");
    assert!(!shown(&session, "input-chat"));
    assert_eq!(
        session
            .document()
            .context()
            .world()
            .layout_box(editor.stable_id())
            .unwrap(),
        before
    );
}

#[test]
fn sidebar_keeps_the_editor_and_peer_form_across_resizes() {
    for locale in Locale::ALL {
        let skin = common::skin("nanatype.halo");
        let ui = Ui::new(Arc::new(preview_settings()), skin, locale);
        let (mut session, (_, (editor, _, _))) =
            common::session()
                .themed(false)
                .mount_output(1200, 640, move || input::input_window(ui));
        session.flush().unwrap();
        let before = session
            .document()
            .context()
            .world()
            .layout_box(editor.stable_id())
            .unwrap();
        ui.locale.set(if locale == Locale::ZhCn {
            Locale::EnUs
        } else {
            Locale::ZhCn
        });
        session.flush().unwrap();
        assert!(
            session
                .accessibility_dump()
                .iter()
                .any(|n| n.label.as_deref() == Some(ui.now_t("dock-tab-controls").as_str())),
            "dock follows live locale changes"
        );
        ui.locale.set(locale);
        session.flush().unwrap();
        let preview = keyed(&session, "input-skin-preview-toggle");
        assert_eq!(
            preview.label.as_deref(),
            Some(ui.now_t("input-skin-preview").as_str())
        );
        assert!(preview.bounds.width >= 28.0);
        assert!(
            preview.bounds.y >= 0.0
                && preview.bounds.y + preview.bounds.height <= nanatype::windows::TOOLBAR_H
        );
        click_label(&mut session, &ui.now_t("input-management"), "menu");
        let invitation = keyed(&session, "peer-invitation-input");
        assert!(invitation.bounds.width > 190.0);
        assert!(
            session
                .set_value(invitation.id, "private invitation draft")
                .unwrap()
        );
        ui.peer_panel.set(false);
        session.flush().unwrap();
        assert_eq!(
            session
                .document()
                .context()
                .world()
                .layout_box(editor.stable_id())
                .unwrap(),
            before
        );
        ui.peer_panel.set(true);
        session
            .key_press("Escape", "Escape", Default::default())
            .unwrap();
        session.set_viewport(720, 360, 1.0).unwrap();
        click_label(&mut session, &ui.now_t("input-management"), "menu");
        let invitation = keyed(&session, "peer-invitation-input");
        assert_eq!(
            invitation.value.as_deref(),
            Some("private invitation draft")
        );
        assert!(
            session
                .set_value(invitation.id, "continued in compact layout")
                .unwrap()
        );
        session
            .key_press("Escape", "Escape", Default::default())
            .unwrap();
        open_panel(&mut session, "controls");
        if let Some(out) = std::env::var_os("NANATYPE_INPUT_WINDOW_CAPTURE_DIR") {
            std::fs::create_dir_all(&out).unwrap();
            session
                .screenshot_png(
                    PathBuf::from(out).join(format!("rail-commands-{locale:?}-720x360.png")),
                )
                .unwrap();
        }
        click_label(&mut session, &ui.now_t("tb-page"), "button");
        assert!(
            session
                .document_mut()
                .context_mut()
                .take_program_messages()
                .into_iter()
                .any(
                    |m| m.downcast::<nanatype::msg::Msg>().is_ok_and(|m| matches!(
                        *m,
                        nanatype::msg::Msg::Tool(nanatype::msg::Tool::Insert(
                            nanatype_model::Inline::Marker(nanatype_model::Marker::Page)
                        ))
                    ))
                )
        );
        assert!((keyed(&session, "input-insertion-dock").bounds.width - 320.0).abs() < 1.0);
        session.set_viewport(1200, 640, 1.0).unwrap();
        click_label(&mut session, &ui.now_t("input-management"), "menu");
        assert_eq!(
            keyed(&session, "peer-invitation-input").value.as_deref(),
            Some("continued in compact layout")
        );
        assert_eq!(
            session
                .document()
                .context()
                .world()
                .layout_box(editor.stable_id())
                .unwrap(),
            before
        );
        ui.peer.set(Arc::new(nanatype::p2p::SessionUi {
            active: true,
            host: true,
            invitation: "preview".into(),
            status: nanatype_p2p::Status::Waiting,
            ..Default::default()
        }));
        session.flush().unwrap();
        for key in ["peer-copy", "peer-window-toggle", "peer-leave"] {
            let label = ui.now_t(key);
            let node = session
                .accessibility_dump()
                .into_iter()
                .find(|n| n.label.as_deref() == Some(&label))
                .unwrap();
            assert!(node.bounds.x >= 0.0 && node.bounds.x + node.bounds.width <= 1201.0);
        }
    }
}

#[test]
fn dock_tabs_allocate_sidebar_space_retain_content_and_keep_the_canvas_size() {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf();
    nana_ui::set_background_image_url_base(base.clone());
    let fixture =
        base.join("target/input-window-acceptance/resizable-sidebar/fixtures/sticker.png");
    std::fs::create_dir_all(fixture.parent().unwrap()).unwrap();
    image::RgbaImage::from_fn(64, 64, |x, y| {
        if (x as f32 - 31.5).powi(2) + (y as f32 - 31.5).powi(2) < 28.0_f32.powi(2) {
            image::Rgba([0, 180, 235, 255])
        } else {
            image::Rgba([0, 0, 0, 0])
        }
    })
    .save(&fixture)
    .unwrap();
    let url = format!(
        "file:///{}",
        fixture.display().to_string().replace('\\', "/")
    );
    let registry = common::registry();
    for skin_id in ["nanatype.halo", "nanatype.night", "nanatype.subtitle"] {
        for locale in Locale::ALL {
            for dark in [false, true] {
                for scale in [1.0, 1.5, 2.0] {
                    let ui = Ui::new(
                        Arc::new(preview_settings()),
                        registry.load(skin_id).unwrap().skin,
                        locale,
                    );
                    ui.chrome_dark.set(dark);
                    ui.stickers.set(Arc::new(
                        (0..60)
                            .map(|i| nanatype::ui::StickerRow {
                                asset: format!("lib:test/{i}"),
                                name: format!("Sticker {i:02}"),
                                lib: "test".into(),
                                url: url.clone(),
                                height_em: 1.6,
                                pack_title: "test".into(),
                            })
                            .collect(),
                    ));
                    let (mut session, (_, (editor, _, _))) = common::session()
                        .themed(dark)
                        .scale(scale)
                        .mount_output(1200, 640, move || input::input_window(ui));
                    for (w, h) in [(720, 360), (920, 480), (1200, 640)] {
                        session.set_viewport(w, h, scale).unwrap();
                        for _ in 0..3 {
                            session.flush().unwrap();
                        }
                        let before = session
                            .document()
                            .context()
                            .world()
                            .layout_box(editor.stable_id())
                            .unwrap();
                        let footer = keyed(&session, "input-actions").bounds;
                        let capture = |s: &mut RuntimeAgentSession, state: &str| {
                            if locale == Locale::ZhCn
                                && let Some(out) =
                                    std::env::var_os("NANATYPE_INPUT_WINDOW_CAPTURE_DIR")
                            {
                                std::fs::create_dir_all(&out).unwrap();
                                settle(s);
                                s.screenshot_png(PathBuf::from(out).join(format!(
                                    "sidebar-{skin_id}-{state}-dark{dark}-{w}x{h}-{scale}x.png"
                                )))
                                .unwrap();
                            }
                        };
                        open_panel(&mut session, "controls");
                        let commands = keyed(&session, "input-dock-controls-panel").bounds;
                        let sidebar = keyed(&session, "input-insertion-dock").bounds;
                        let rail = keyed(&session, "input-dock-rail").bounds;
                        let preview = keyed(&session, "input-preview-scroll").bounds;
                        assert!((sidebar.width - 320.0).abs() <= 1.0 / scale);
                        assert!((sidebar.x + sidebar.width - w as f32).abs() <= 1.0 / scale);
                        assert!(preview.x + preview.width <= sidebar.x - 7.0 / scale);
                        assert!((rail.x + rail.width - w as f32).abs() <= 1.0 / scale);
                        assert!(commands.x + commands.width <= rail.x + 1.0 / scale);
                        assert!(commands.y + commands.height <= footer.y + 1.0 / scale);
                        assert_eq!(keyed(&session, "input-toolbar").bounds.width, preview.width);
                        assert_preview_fits(&session);
                        capture(&mut session, "commands");
                        // The open panel's icon folds the sidebar and keeps
                        // the canvas; the next press restores its width.
                        click_keyed(&mut session, "input-dock-rail-controls");
                        assert!(keyed(&session, "input-insertion-dock").bounds.width < 48.0);
                        assert_eq!(
                            session
                                .document()
                                .context()
                                .world()
                                .layout_box(editor.stable_id())
                                .unwrap()
                                .width,
                            before.width
                        );
                        assert_preview_fits(&session);
                        capture(&mut session, "folded");
                        click_keyed(&mut session, "input-dock-rail-controls");
                        assert_eq!(keyed(&session, "input-insertion-dock").bounds, sidebar);
                        // Drag through routed pointer events, never direct component writes.
                        drag_sidebar(&mut session, -60.0);
                        let wider = keyed(&session, "input-insertion-dock").bounds;
                        assert!(
                            (wider.width - 380.0).abs() <= 1.0 / scale,
                            "{locale:?} {skin_id} {w} {scale} {wider:?}"
                        );
                        assert!(
                            (preview.width
                                - keyed(&session, "input-preview-scroll").bounds.width
                                - 60.0)
                                .abs()
                                <= 1.0 / scale
                        );
                        let after = session
                            .document()
                            .context()
                            .world()
                            .layout_box(editor.stable_id())
                            .unwrap();
                        assert_eq!(after.width, before.width);
                        assert_eq!(after.height, before.height);
                        assert_preview_fits(&session);
                        capture(&mut session, "resized");
                        open_panel(&mut session, "stickers");
                        assert!(!shown(&session, "input-dock-controls-panel"));
                        assert_eq!(keyed(&session, "input-insertion-dock").bounds, wider);
                        let port = keyed(&session, "input-dock-stickers-scroll");
                        session
                            .scroll_by(port.bounds.x + 2.0, port.bounds.y + 4.0, 0.0, -2000.0)
                            .unwrap();
                        assert!(
                            session
                                .document()
                                .context()
                                .world()
                                .scroll_offset(StableNodeId::new(port.id).unwrap())
                                .unwrap()
                                .y
                                > 0.0
                        );
                        session
                            .scroll_by(port.bounds.x + 2.0, port.bounds.y + 4.0, 0.0, 2000.0)
                            .unwrap();
                        capture(&mut session, "stickers");
                        let query = keyed(&session, "input-dock-sticker-search");
                        session.set_value(query.id, "Sticker 00").unwrap();
                        open_panel(&mut session, "controls");
                        open_panel(&mut session, "stickers");
                        assert_eq!(
                            keyed(&session, "input-dock-sticker-search")
                                .value
                                .as_deref(),
                            Some("Sticker 00")
                        );
                        session.set_value(query.id, "").unwrap();
                        // Return to the default width before the next viewport case.
                        drag_sidebar(&mut session, 60.0);
                        assert!(
                            (keyed(&session, "input-insertion-dock").bounds.width - 320.0).abs()
                                <= 1.0 / scale
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn fitted_preview_keeps_tall_canvases_scrollable_and_restores_full_size_when_space_returns() {
    let skin = common::skin("nanatype.halo");
    let mut settings = preview_settings();
    settings.skin.overrides.region = Some([1200.0, 1800.0]);
    let ui = Ui::new(Arc::new(settings), skin, Locale::ZhCn);
    let mut session = common::session().mount(720, 360, move || input::input_window(ui));
    for _ in 0..4 {
        session.flush().unwrap();
    }
    assert_preview_fits(&session);
    let port = keyed(&session, "input-preview-scroll");
    let canvas = StableNodeId::new(keyed(&session, "input-preview-canvas").id).unwrap();
    let original = session
        .document()
        .context()
        .world()
        .layout_box(canvas)
        .unwrap();
    session
        .scroll_by(2.0, port.bounds.y + 4.0, -10000.0, -10000.0)
        .unwrap();
    let world = session.document().context().world();
    let offset = world
        .scroll_offset(StableNodeId::new(port.id).unwrap())
        .unwrap();
    assert!(
        offset.x.abs() < 0.001,
        "no horizontal overflow beyond layout rounding"
    );
    assert!(
        offset.y > 0.0,
        "tall canvas must remain vertically reachable"
    );
    let (_, bottom) = world
        .layout_pointer_position(
            canvas,
            original.x + original.width,
            original.y + original.height,
        )
        .unwrap();
    assert!(bottom <= port.bounds.y + port.bounds.height - 15.0);
    ui.settings
        .update(|s| Arc::make_mut(s).general.input_skin_preview = false);
    for _ in 0..4 {
        session.flush().unwrap();
    }
    let offset = session
        .document()
        .context()
        .world()
        .scroll_offset(StableNodeId::new(port.id).unwrap())
        .unwrap();
    assert_eq!(
        (offset.x, offset.y),
        (0.0, 0.0),
        "plain mode clears the old canvas scroll range"
    );
    ui.settings
        .update(|s| Arc::make_mut(s).general.input_skin_preview = true);
    for _ in 0..4 {
        session.flush().unwrap();
    }
    assert_preview_fits(&session);
    session
        .scroll_by(2.0, port.bounds.y + 4.0, 10000.0, 10000.0)
        .unwrap();
    session.set_viewport(1800, 1200, 1.0).unwrap();
    for _ in 0..4 {
        session.flush().unwrap();
    }
    assert_preview_fits(&session);
    let fitted = keyed(&session, "input-preview-fit").bounds;
    assert!(
        (fitted.width - original.width).abs() < 0.1,
        "return to 100% when space permits"
    );
    let after = session
        .document()
        .context()
        .world()
        .layout_box(canvas)
        .unwrap();
    assert_eq!(
        (original.width, original.height),
        (after.width, after.height)
    );
}

#[test]
fn toolbar_popovers_remain_clickable_over_the_scrolling_preview() {
    let skin = common::skin("nanatype.halo");
    let ui = Ui::new(Arc::new(preview_settings()), skin, Locale::ZhCn);
    let mut session = common::session()
        .themed(false)
        .mount(720, 360, move || input::input_window(ui));
    session.flush().unwrap();
    click_label(&mut session, &ui.now_t("tb-more"), "menu");
    click_label(&mut session, &ui.now_t("tb-color"), "menu-item");
    let apply_label = ui.now_t("common-apply");
    let popup = session
        .accessibility_dump()
        .into_iter()
        .find(|n| n.role == "button" && n.label.as_deref() == Some(&apply_label))
        .expect("toolbar color panel opens through real pointer routing");
    {
        assert!(popup.bounds.x >= 0.0 && popup.bounds.y >= 0.0);
        assert!(
            popup.bounds.x + popup.bounds.width <= 721.0
                && popup.bounds.y + popup.bounds.height <= 361.0
        );
        assert!(
            popup.bounds.y + popup.bounds.height > keyed(&session, "input-preview-scroll").bounds.y
        );
    }
    if let Some(out) = std::env::var_os("NANATYPE_INPUT_WINDOW_CAPTURE_DIR") {
        std::fs::create_dir_all(&out).unwrap();
        session
            .screenshot_png(PathBuf::from(out).join("input-color-popover-minimum.png"))
            .unwrap();
    }
    session
        .click_xy(
            popup.bounds.x + popup.bounds.width / 2.0,
            popup.bounds.y + popup.bounds.height / 2.0,
        )
        .unwrap();
    assert!(
        session
            .document_mut()
            .context_mut()
            .take_program_messages()
            .into_iter()
            .any(
                |m| m.downcast::<nanatype::msg::Msg>().is_ok_and(|m| matches!(
                    *m,
                    nanatype::msg::Msg::Tool(nanatype::msg::Tool::Color(_))
                ))
            ),
        "color action must remain reachable over the preview"
    );
}

#[test]
fn fixed_canvas_and_controls_survive_resize_theme_scale_and_peer_panel_changes() {
    let registry = common::registry();
    let captures = std::env::var_os("NANATYPE_INPUT_WINDOW_CAPTURE_DIR").map(PathBuf::from);
    if let Some(out) = &captures {
        std::fs::create_dir_all(out).unwrap();
    }
    let mut cases = 0;
    for skin_id in ["nanatype.halo", "nanatype.night", "nanatype.subtitle"] {
        for dark in [false, true] {
            for scale in [1.0, 1.5, 2.0] {
                let skin = registry.load(skin_id).unwrap().skin;
                let canvas_size = (
                    skin.dialogue.region[0] + 2.0 * skin.dialogue.overscan,
                    skin.dialogue.region[1] + 2.0 * skin.dialogue.overscan,
                );
                let ui = Ui::new(Arc::new(preview_settings()), skin, Locale::ZhCn);
                ui.chrome_dark.set(dark);
                let (mut session, (_, (editor, _, _))) = common::session()
                    .themed(dark)
                    .scale(scale)
                    .mount_output(920, 480, move || input::input_window(ui));
                session.set_gpu_renderers(nanatype::ui::sweep::renderers());
                for state in ["empty", "long", "peer"] {
                    let text = if state == "empty" {
                        String::new()
                    } else {
                        "普通窗口中的皮肤编辑预览：保持直播画布尺寸与换行。\nHello NanaType! 👋\n"
                            .repeat(15)
                    };
                    ui.draft.set(Arc::new(Document::plain(&text)));
                    ui.peer_panel.set(state != "long");
                    let mut editor_width: Option<f32> = None;
                    for (w, h) in [(720, 360), (920, 480), (1200, 640), (1280, 720)] {
                        session.set_viewport(w, h, scale).unwrap();
                        for _ in 0..4 {
                            session.flush().unwrap();
                        }
                        let canvas_node = keyed(&session, "input-preview-canvas");
                        let canvas = session
                            .document()
                            .context()
                            .world()
                            .layout_box(StableNodeId::new(canvas_node.id).unwrap())
                            .unwrap();
                        let toolbar = keyed(&session, "input-toolbar").bounds;
                        let footer = keyed(&session, "input-actions").bounds;
                        let port = keyed(&session, "input-preview-scroll");
                        let rail = keyed(&session, "input-insertion-dock").bounds;
                        assert!((rail.width - 320.0).abs() <= 1.0 / scale);
                        assert!((rail.x + rail.width - w as f32).abs() <= 1.0 / scale);
                        assert!((toolbar.width + rail.width + 8.0 - w as f32).abs() <= 1.0 / scale);
                        assert!(
                            session.accessibility_dump().iter().any(|n| n
                                .agent_path
                                .as_deref()
                                .is_some_and(|p| p.ends_with("input-dock-controls-panel"))),
                            "sidebar is always visible"
                        );
                        assert!(
                            toolbar.width >= 320.0,
                            "editor column must remain usable: {toolbar:?}"
                        );
                        assert!((toolbar.width - port.bounds.width).abs() < 1.0);
                        assert!((canvas.width - canvas_size.0).abs() <= 1.0 / scale);
                        assert!((canvas.height - canvas_size.1).abs() <= 1.0 / scale);
                        assert!((toolbar.height - 48.0).abs() <= 1.0 / scale);
                        assert!((footer.height - 56.0).abs() <= 1.0 / scale);
                        assert!((footer.y + footer.height - h as f32).abs() <= 1.0 / scale);
                        assert!(toolbar.y.abs() <= 1.0 / scale);
                        assert!(
                            (port.bounds.y + port.bounds.height - footer.y).abs() <= 1.0 / scale,
                            "one footer row"
                        );
                        assert!(
                            port.bounds.height > 0.0
                                && port.bounds.y >= toolbar.y + toolbar.height - 1.0
                        );
                        assert_preview_fits(&session);
                        let bounds = session
                            .document()
                            .context()
                            .world()
                            .layout_box(editor.stable_id())
                            .unwrap();
                        if let Some(width) = editor_width {
                            assert!((bounds.width - width).abs() < 0.1);
                        }
                        editor_width = Some(bounds.width);
                        assert_eq!(ui.input_width.get_untracked(), w as f32);
                        let nodes = session.accessibility_dump();
                        for key in ["window-settings", "tb-send"] {
                            let label = ui.now_t(key);
                            let node = nodes
                                .iter()
                                .find(|n| n.label.as_deref() == Some(&label))
                                .unwrap();
                            assert!(
                                node.bounds.x >= 0.0
                                    && node.bounds.x + node.bounds.width <= w as f32 + 1.0,
                                "{skin_id} dark={dark} {state} {w} {scale} {node:?}"
                            );
                        }
                        if state == "peer" {
                            click_label(&mut session, &ui.now_t("input-management"), "menu");
                        }
                        if let Some(out) = &captures {
                            session
                                .document_mut()
                                .context_mut()
                                .advance_animations(Duration::from_secs(1));
                            session
                                .document_mut()
                                .sync_presentation_clock(Duration::from_secs(1));
                            session
                                .screenshot_png(out.join(format!(
                                    "{skin_id}-dark{dark}-{state}-{w}x{h}-{scale}x.png"
                                )))
                                .unwrap();
                        }
                        if state == "peer" {
                            session
                                .key_press("Escape", "Escape", Default::default())
                                .unwrap();
                        }
                        // Scroll from the preview's padding, away from the editor's own scroller.
                        let fitted = keyed(&session, "input-preview-fit").bounds;
                        let dy = if fitted.height + 32.0 > port.bounds.height {
                            -2000.0
                        } else {
                            0.0
                        };
                        session
                            .scroll_by(2.0, port.bounds.y + 4.0, -2000.0, dy)
                            .unwrap();
                        let offset = session
                            .document()
                            .context()
                            .world()
                            .scroll_offset(StableNodeId::new(port.id).unwrap())
                            .unwrap();
                        assert!(
                            offset.x.abs() < 0.001,
                            "full preview fits without horizontal scrolling"
                        );
                        if dy < 0.0 {
                            assert!(offset.y > 0.0, "vertical preview scroll {skin_id}");
                        }
                        session
                            .scroll_by(2.0, port.bounds.y + 4.0, 2000.0, 2000.0)
                            .unwrap();
                        assert_eq!(ui.draft.get_untracked().plain_text(), text);
                        cases += 1;
                    }
                }
            }
        }
    }
    eprintln!("accepted {cases} input-window layout combinations");
    if let Some(out) = captures {
        for dark in [false, true] {
            let skin = registry.load("nanatype.halo").unwrap().skin;
            let mut config = preview_settings();
            config.general.onboarding_done = true;
            let ui = Ui::new(Arc::new(config), skin, Locale::ZhCn);
            ui.chrome_dark.set(dark);
            let mut session = common::session()
                .themed(dark)
                .mount(920, 660, move || settings::settings_window(ui));
            session
                .screenshot_png(out.join(format!("settings-dark{dark}.png")))
                .unwrap();
        }
    }
}

#[test]
fn sticker_packs_sit_above_the_grid_filter_search_and_accept_folder_drops() {
    use nana_ui::runtime::{Entity, Tabs};
    use nanatype::msg::Msg;
    let out =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../target/sticker-sidebar-acceptance");
    std::fs::create_dir_all(out.join("import-me")).unwrap();
    let folder = out.join("import-me").canonicalize().unwrap();
    for (width, dark) in [(720, false), (1200, true)] {
        let skin = common::skin("nanatype.halo");
        let ui = Ui::new(Arc::new(preview_settings()), skin, Locale::ZhCn);
        let mut rows: Vec<_> = (0..70)
            .map(|i| nanatype::ui::StickerRow {
                asset: format!("lib:cats/cat-{i}"),
                name: format!("Cat {i:02}"),
                lib: "cats".into(),
                url: String::new(),
                height_em: 1.6,
                pack_title: "猫猫".into(),
            })
            .collect();
        rows.push(nanatype::ui::StickerRow {
            asset: "lib:dogs/dog".into(),
            name: "Dog".into(),
            lib: "dogs".into(),
            url: String::new(),
            height_em: 4.0,
            pack_title: "狗狗".into(),
        });
        ui.stickers.set(Arc::new(rows));
        let mut session = common::session()
            .themed(dark)
            .mount(width, 640, move || input::input_window(ui));
        open_panel(&mut session, "stickers");
        let select = |session: &mut RuntimeAgentSession, value: &str| {
            let tabs = Entity::<Tabs>::from_stable_id(
                StableNodeId::new(keyed(session, "input-dock-sticker-series").id).unwrap(),
            );
            let child = session
                .document()
                .context()
                .read(tabs, |t| t.option_node(value).unwrap())
                .unwrap();
            let node = session
                .accessibility_dump()
                .into_iter()
                .find(|n| n.id == child.get())
                .unwrap();
            click_node(session, node);
        };
        let packs = keyed(&session, "input-dock-sticker-series").bounds;
        let port = keyed(&session, "input-dock-stickers-scroll").bounds;
        assert!(
            packs.y + packs.height <= port.y + 0.5,
            "the pack tabs sit above the scroller"
        );
        select(&mut session, "cats");
        assert!(
            !session
                .accessibility_dump()
                .iter()
                .any(|n| n.label.as_deref() == Some("Dog"))
        );
        session
            .scroll_by(port.x + 4.0, port.y + 4.0, 0.0, -2000.0)
            .unwrap();
        assert_eq!(keyed(&session, "input-dock-sticker-series").bounds, packs);
        session
            .scroll_by(port.x + 4.0, port.y + 4.0, 0.0, 2000.0)
            .unwrap();
        let search = keyed(&session, "input-dock-sticker-search");
        session.set_value(search.id, "Dog").unwrap();
        select(&mut session, "dogs");
        assert!(
            session
                .accessibility_dump()
                .iter()
                .any(|n| n.label.as_deref() == Some("Dog"))
        );
        click_label(&mut session, "Dog", "button");
        assert!(
            session
                .document_mut()
                .context_mut()
                .take_program_messages()
                .into_iter()
                .any(|m| m.downcast::<Msg>().is_ok_and(|m| matches!(
                    *m,
                    Msg::Tool(nanatype::msg::Tool::Insert(nanatype_model::Inline::Atom(
                        nanatype_model::Atom { height_em: 4.0, .. }
                    )))
                )))
        );
        open_panel(&mut session, "controls");
        open_panel(&mut session, "stickers");
        let tabs = Entity::<Tabs>::from_stable_id(
            StableNodeId::new(keyed(&session, "input-dock-sticker-series").id).unwrap(),
        );
        assert_eq!(
            session
                .document()
                .context()
                .read(tabs, |t| t.selected.clone())
                .unwrap()
                .as_deref(),
            Some("dogs")
        );
        session
            .set_value(keyed(&session, "input-dock-sticker-search").id, "")
            .unwrap();
        session
            .screenshot_png(out.join(format!("series-{width}.png")))
            .unwrap();
        ui.stickers
            .update(|s| Arc::make_mut(s).retain(|r| r.lib != "dogs"));
        settle(&mut session);
        assert_eq!(
            session
                .document()
                .context()
                .read(tabs, |t| t.selected.clone())
                .unwrap()
                .as_deref(),
            Some("")
        );
        ui.stickers.set(Arc::new(Vec::new()));
        settle(&mut session);
        let port = keyed(&session, "input-dock-stickers-scroll").bounds;
        session
            .file_drag_xy(
                nana_ui_core::FileDragKind::Hover,
                std::slice::from_ref(&folder),
                port.x + 8.0,
                port.y + 8.0,
            )
            .unwrap();
        assert!(
            session
                .accessibility_dump()
                .iter()
                .any(|n| n.label.as_deref() == Some("松开以导入表情包文件夹"))
        );
        session
            .file_drag_xy(
                nana_ui_core::FileDragKind::Cancel,
                std::slice::from_ref(&folder),
                0.0,
                0.0,
            )
            .unwrap();
        assert!(
            !session
                .accessibility_dump()
                .iter()
                .any(|n| n.label.as_deref() == Some("松开以导入表情包文件夹"))
        );
        session
            .drop_files_xy(std::slice::from_ref(&folder), port.x + 8.0, port.y + 8.0)
            .unwrap();
        assert!(session.document_mut().context_mut().take_program_messages().into_iter().any(|m|m.downcast::<Msg>().is_ok_and(|m|matches!(*m,Msg::StickerImportFolders(ref dirs) if dirs==&vec![folder.clone()]))));
        open_panel(&mut session, "controls");
        let port = keyed(&session, "input-controls-scroll").bounds;
        session
            .drop_files_xy(std::slice::from_ref(&folder), port.x + 8.0, port.y + 8.0)
            .unwrap();
        assert!(
            !session
                .document_mut()
                .context_mut()
                .take_program_messages()
                .into_iter()
                .any(|m| m
                    .downcast::<Msg>()
                    .is_ok_and(|m| matches!(*m, Msg::StickerImportFolders(_))))
        );
        let header = keyed(&session, "input-dock-rail-stickers").bounds;
        session
            .file_drag_xy(
                nana_ui_core::FileDragKind::Hover,
                std::slice::from_ref(&folder),
                header.x + header.width / 2.0,
                header.y + header.height / 2.0,
            )
            .unwrap();
        assert!(
            session.accessibility_dump().iter().any(|n| n
                .agent_path
                .as_deref()
                .is_some_and(|p| p.ends_with("input-dock-stickers-panel"))),
            "hovering the stickers icon opens its panel"
        );
        session
            .drop_files_xy(
                std::slice::from_ref(&folder),
                header.x + header.width / 2.0,
                header.y + header.height / 2.0,
            )
            .unwrap();
        assert!(
            session
                .document_mut()
                .context_mut()
                .take_program_messages()
                .into_iter()
                .any(|m| m
                    .downcast::<Msg>()
                    .is_ok_and(|m| matches!(*m, Msg::StickerImportFolders(_)))),
            "the stickers icon accepts folder drops"
        );
    }
}
