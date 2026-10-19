mod common;

use nana_ui::runtime::DocumentId;
use nana_ui_platform::InputModifiers;
use nanatype::ui::{Ui, input};
use nanatype_i18n::Locale;
use nanatype_model::Document;
use nanatype_settings::Settings;
use std::sync::Arc;

#[test]
fn both_editors_keep_a_bounded_scrollable_viewport_across_skin_scale_and_mode_changes() {
    let registry = common::registry();
    let mut cases = 0;
    for entry in registry.entries() {
        let skin_id = &entry.id;
        for (width, dark, scale) in [360, 796].into_iter().flat_map(|width| {
            [false, true].into_iter().flat_map(move |dark| {
                [1.0, 1.5, 2.0]
                    .into_iter()
                    .map(move |scale| (width, dark, scale))
            })
        }) {
            let skin = registry.load(skin_id).unwrap().skin;
            let mut settings = Settings::default();
            settings.general.input_skin_preview = true;
            settings.skin.overrides.region = Some([width as f32, 200.0]);
            let client_width = (width + 32).max(720);
            let height = nanatype::windows::INPUT_SIZE.1 as u32;
            let ui = Ui::new(Arc::new(settings), skin, Locale::ZhCn);
            ui.chrome_dark.set(dark);
            let value = "输入框滚动测试 hello 👨‍👩‍👧‍👦\n".repeat(60);
            ui.draft.set(Arc::new(Document::plain(&value)));
            ui.source.set(nanatype::ui::Source {
                text: value.clone(),
                selection: (0, 0),
                revision: 0,
            });
            let id = DocumentId::new(1).unwrap();
            let (mut session, (_, (editor, source, _))) = common::session()
                .themed(dark)
                .scale(scale)
                .mount_output(client_width, height, move || input::input_window(ui));
            for source_mode in [false, true, false] {
                ui.source_mode.set(source_mode);
                session.flush().unwrap();
                let node = if source_mode {
                    source.stable_id()
                } else {
                    editor.stable_id()
                };
                let bounds = session
                    .document()
                    .context()
                    .world()
                    .layout_box(node)
                    .unwrap();
                assert!(
                    bounds.height > 0.0 && bounds.height <= 200.0,
                    "{skin_id} {scale} source={source_mode}: {bounds:?}"
                );
                let world = session.document().context().world();
                let (left, top) = world
                    .layout_pointer_position(node, bounds.x, bounds.y)
                    .unwrap();
                let (right, bottom) = world
                    .layout_pointer_position(
                        node,
                        bounds.x + bounds.width,
                        bounds.y + bounds.height,
                    )
                    .unwrap();
                assert!(
                    left >= 16.0 - 0.5
                        && top >= 0.0
                        && right <= client_width as f32 - 300.0 - 8.0 - 16.0 + 0.5
                        && bottom <= height as f32 + 0.5,
                    "editor outside window: {skin_id} width={width} dark={dark} scale={scale}: {bounds:?}"
                );
                let (x, y) = ((left + right) / 2.0, (top + bottom) / 2.0);
                session.click_xy(x, y).unwrap();
                assert_eq!(session.document().context().world().focused(id), Some(node));
                session
                    .key_press(
                        "End",
                        "End",
                        InputModifiers {
                            control: true,
                            ..Default::default()
                        },
                    )
                    .unwrap();
                let bottom = session
                    .document()
                    .context()
                    .world()
                    .scroll_offset(node)
                    .unwrap()
                    .y;
                assert!(
                    bottom > 80.0,
                    "long draft must scroll internally: {skin_id} {scale} source={source_mode}"
                );
                session.scroll_by(x, y, 0.0, 60.0).unwrap();
                let read = session
                    .document()
                    .context()
                    .world()
                    .scroll_offset(node)
                    .unwrap()
                    .y;
                assert!(
                    read < bottom,
                    "wheel must move the viewport: {read} >= {bottom}"
                );
                session.flush().unwrap();
                assert_eq!(
                    session
                        .document()
                        .context()
                        .world()
                        .scroll_offset(node)
                        .unwrap()
                        .y,
                    read,
                    "idle flush must not reveal a stationary caret"
                );
                if let Some(out) = std::env::var_os("NANATYPE_EDITOR_CAPTURE_DIR")
                    .filter(|_| (scale == 1.5 && width == 360) || skin_id == "nanatype.halo")
                {
                    let out = std::path::PathBuf::from(out);
                    std::fs::create_dir_all(&out).unwrap();
                    session
                        .screenshot_png(out.join(format!(
                            "{skin_id}-{width}-dark{dark}-{scale}x-source{source_mode}.png"
                        )))
                        .unwrap();
                }
            }
            assert_eq!(ui.draft.get_untracked().plain_text(), value);
            cases += 1;
        }
    }
    eprintln!(
        "accepted {cases} skin/width/appearance/scale combinations, {} editor-mode passes",
        cases * 3
    );
}

#[test]
fn editor_menu_localizes_commands_clamps_to_the_window_and_accepts_keyboard_selection() {
    use nana_ui::runtime::{ContextMenuEvent, LayoutBox};
    for locale in [Locale::ZhCn, Locale::EnUs, Locale::JaJp] {
        let skin = common::skin("nanatype.halo");
        let ui = Ui::new(Arc::new(Settings::default()), skin, locale);
        let id = DocumentId::new(1).unwrap();
        let mut session = common::session()
            .themed(false)
            .mount(796, 380, move || input::input_window(ui));
        let mut menu =
            input::editor_context_menu(ui, 780.0, 370.0, [false, false, true, true, true, true]);
        menu.highlighted = Some(2);
        menu.place_in(LayoutBox {
            x: 0.0,
            y: 0.0,
            width: 796.0,
            height: 380.0,
        });
        let menu = session
            .document_mut()
            .context_mut()
            .create_component(id, menu)
            .unwrap();
        let overlay = session
            .document_mut()
            .context_mut()
            .create_component(id, nana_ui::runtime::OverlayHost::new())
            .unwrap();
        session
            .document_mut()
            .context_mut()
            .append_child(overlay, menu)
            .unwrap();
        session
            .document_mut()
            .context_mut()
            .activate_overlay(overlay, menu)
            .unwrap();
        let selected = Arc::new(std::sync::Mutex::new(None));
        let observed = selected.clone();
        session
            .document_mut()
            .context_mut()
            .on(menu, move |_, event: &ContextMenuEvent, _| {
                if let ContextMenuEvent::Select(value) = event {
                    *observed.lock().unwrap() = Some(value.to_string());
                }
            })
            .unwrap();
        session.flush().unwrap();
        let nodes = session.accessibility_dump();
        for key in [
            "edit-undo",
            "edit-redo",
            "edit-cut",
            "edit-copy",
            "edit-paste",
            "edit-select-all",
        ] {
            let label = ui.now_t(key);
            let node = nodes
                .iter()
                .find(|n| n.label.as_deref() == Some(&label))
                .unwrap_or_else(|| panic!("missing {label}"));
            assert!(
                node.bounds.x >= 0.0
                    && node.bounds.y >= 0.0
                    && node.bounds.x + node.bounds.width <= 797.0
                    && node.bounds.y + node.bounds.height <= 381.0,
                "{node:?}"
            );
        }
        if locale == Locale::ZhCn
            && let Some(out) = std::env::var_os("NANATYPE_EDITOR_CAPTURE_DIR")
        {
            session
                .screenshot_png(std::path::PathBuf::from(out).join("editor-menu.png"))
                .unwrap();
        }
        session
            .key_press("Enter", "Enter", Default::default())
            .unwrap();
        assert_eq!(selected.lock().unwrap().as_deref(), Some("x"));
    }
}
