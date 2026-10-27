mod common;
use nana_ui::runtime::Stack;
use nana_ui::runtime::view::{IntoView, widget};
use nana_ui_core::LengthSpec;
use nanatype_editor::{
    canvas,
    session::{self, Action, Mode},
    ui::{self, Ui},
};
use nanatype_i18n::Locale;
use nanatype_settings::Settings;
use nanatype_theme::{SkinRegistry, editor};
use std::sync::Arc;

#[test]
fn canvas_geometry_comes_from_the_real_renderer_and_drag_commits_once() {
    let dir = tempfile::tempdir().unwrap();
    let registry = SkinRegistry::new(Some(dir.path().join("packages")), "1.0.0");
    let mut session =
        session::copy_installed(&registry, "nanatype.parchment", "editor.canvas").unwrap();
    let (ticket, project) = session.validation_due().unwrap();
    session.validated(ticket, Arc::new(session::validate(&registry, &project)));
    let view = Arc::new(session.view(None, false));
    let ui = Ui::new(
        Arc::new(Settings::default()),
        Arc::new(Default::default()),
        Locale::ZhCn,
    );
    ui.package_editor.set(Some(view.clone()));
    ui.mode.set(Mode::Design);
    let mut agent = common::session().themed(false).mount(1440, 1100, move || {
        widget(Stack::column(0.).height(LengthSpec::Px(1100.)))
            .children((ui::views::editor(ui), ui::canvas_controls::overlay(ui)))
            .into_any()
    });
    agent.flush().unwrap();
    let geometry =
        nanatype_frame::ui::editor_geometry::collect(ui.preview_host, agent.document().context());
    let canvas = geometry
        .iter()
        .find(|p| p.key == "canvas")
        .expect("rendered canvas");
    if let Some(mark) = geometry.iter().find(|p| p.key == "continue") {
        assert!(
            mark.rect[2] < canvas.rect[2] / 2. && mark.rect[3] < canvas.rect[3] / 2.,
            "continue selection must follow its glyph, not its anchor layer: {mark:?}"
        );
    }
    let selected = geometry
        .iter()
        .find(|p| p.key.starts_with("decor."))
        .or_else(|| geometry.iter().find(|p| p.key == "nameplate"))
        .expect("editable rendered parts")
        .clone();
    ui.canvas_geometry.set(geometry);
    ui.canvas_selection.set(Some(selected.key.clone()));
    agent.flush().unwrap();
    let x = selected.rect[0] + selected.rect[2] * 0.5;
    let y = selected.rect[1] + selected.rect[3] * 0.5;
    let mut drag = None;
    let pointer = |phase, x, y| {
        nana_ui_platform::InputPayload::Pointer(nana_ui_platform::PointerInput::mouse(phase, x, y))
    };
    canvas::input(
        ui,
        &pointer(nana_ui_platform::PointerPhase::Down, x, y),
        &mut drag,
    );
    canvas::input(
        ui,
        &pointer(nana_ui_platform::PointerPhase::Move, x + 12., y + 8.),
        &mut drag,
    );
    assert!(ui.canvas_preview.get_untracked().is_some());
    assert_eq!(
        session.project.files, project.files,
        "dragging only changes the private preview"
    );
    let Some(Action::CanvasEdit(edit)) = canvas::input(
        ui,
        &pointer(nana_ui_platform::PointerPhase::Up, x + 12., y + 8.),
        &mut drag,
    ) else {
        panic!("a completed gesture produces one transaction");
    };
    let original = session.project.files.clone();
    let mut changed = session.project.clone();
    canvas::commit(
        &mut changed,
        view.report.skin.as_ref().unwrap(),
        &view.properties,
        &edit,
    )
    .unwrap();
    session.replace_project(changed).unwrap();
    session.undo();
    assert_eq!(session.project.files, original);
    let captures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("target/package-editor");
    std::fs::create_dir_all(&captures).unwrap();
    agent
        .screenshot_png(captures.join("canvas-1440.png"))
        .unwrap();
}

#[test]
fn resizing_keeps_percentages_and_rejects_parameter_overwrites() {
    let dir = tempfile::tempdir().unwrap();
    let registry = SkinRegistry::new(Some(dir.path().join("packages")), "1.0.0");
    let mut s = session::create(editor::ProjectKind::Skin, "editor.percent").unwrap();
    s.edit(
        "skin.toml",
        "[dialogue]\ndecor=[{size=['20%',40],offset=[5,6],text='A'},{size=[30,30],text='B'}]\n",
    )
    .unwrap();
    let report = session::validate(&registry, &s.project);
    assert_eq!(report.errors(), 0);
    let skin = report.skin.unwrap();
    let authored = report.properties;
    let edit = canvas::Edit {
        element: "decor.0".into(),
        target: vec!["dialogue".into()],
        size: [800., 240.],
        delta: [80., 0.],
        resize: true,
    };
    canvas::commit(&mut s.project, &skin, &authored, &edit).unwrap();
    let t = editor::parse_document(std::str::from_utf8(&s.project.files["skin.toml"]).unwrap())
        .unwrap();
    assert_eq!(
        editor::value_at(
            &t,
            &[
                "dialogue".into(),
                "decor".into(),
                "0".into(),
                "size".into(),
                "0".into()
            ]
        )
        .unwrap()
        .as_str(),
        Some("30%")
    );
    assert_eq!(
        editor::value_at(
            &t,
            &["dialogue".into(), "decor".into(), "1".into(), "text".into()]
        )
        .unwrap()
        .as_str(),
        Some("B")
    );
    s.edit("skin.toml","[params.x]\ntype='number'\ndefault=5\nmin=0\nmax=100\nstep=1\n[dialogue]\ndecor=[{size=[40,40],offset=[{param='x'},6],text='A'}]\n").unwrap();
    let report = session::validate(&registry, &s.project);
    assert_eq!(report.errors(), 0, "{:?}", report.diagnostics);
    let before = s.project.files.clone();
    let edit = canvas::Edit {
        resize: false,
        delta: [10., 10.],
        ..edit
    };
    assert!(
        canvas::commit(
            &mut s.project,
            &report.skin.unwrap(),
            &report.properties,
            &edit
        )
        .is_err()
    );
    assert_eq!(
        before, s.project.files,
        "a rejected gesture must not partially write another field"
    );
}

#[test]
fn responsive_canvas_preserves_the_inherited_array_and_unedited_bindings() {
    let dir = tempfile::tempdir().unwrap();
    let registry = SkinRegistry::new(Some(dir.path().join("packages")), "1.0.0");
    let mut s = session::create(editor::ProjectKind::Skin, "editor.inherited").unwrap();
    let source = r##"# keep source comment
[params.ink]
type = 'color'
default = '#123456'
[dialogue]
decor = [{size=['20%',40],offset=[5,6],text='A',future={keep='raw'}}, {size=[30,30],color=INK,text='B'}]
[dialogue.responsive]
rules = [{max_width=500,layout={text={size=23}}}]
[dialogue.responsive.portrait.text]
size = 22
"##;
    // The runtime permits bindings in orientations, but rule layouts remain literal.
    for (target, ink) in [
        (vec!["dialogue", "responsive", "portrait"], "{param='ink'}"),
        (
            vec!["dialogue", "responsive", "rules", "0", "layout"],
            "'#123456'",
        ),
    ] {
        let source = source.replace("INK", ink);
        s.edit("skin.toml", &source).unwrap();
        let original = editor::parse_document(&source).unwrap();
        let report = session::validate(&registry, &s.project);
        assert_eq!(report.errors(), 0, "{:?}", report.diagnostics);
        let skin = report.skin.unwrap();
        let mut project = s.project.clone();
        let edit = canvas::Edit {
            element: "decor.0".into(),
            target: target.into_iter().map(str::to_owned).collect(),
            size: [300., 600.],
            delta: [30., 0.],
            resize: true,
        };
        canvas::commit(&mut project, &skin, &report.properties, &edit).unwrap();
        let source = std::str::from_utf8(&project.files["skin.toml"]).unwrap();
        assert!(source.starts_with("# keep source comment"));
        let patched = editor::parse_document(source).unwrap();
        assert_eq!(patched["dialogue"]["decor"], original["dialogue"]["decor"]);
        let mut path = edit.target.clone();
        path.push("decor".into());
        let copied = editor::value_at(&patched, &path)
            .unwrap()
            .as_array()
            .unwrap();
        let mut expected = original["dialogue"]["decor"].as_array().unwrap().clone();
        expected[0]["size"][0] = editor::PropertyValue::String("30%".into());
        expected[0]["size"][1] = editor::PropertyValue::Float(40.);
        assert_eq!(
            copied, &expected,
            "bindings and unknown fields must survive"
        );
        let updated = session::validate(&registry, &project);
        assert_eq!(updated.errors(), 0, "{:?}", updated.diagnostics);
        canvas::check_result(&skin, &updated.skin.unwrap(), &edit).unwrap();
    }
}

#[test]
fn authoring_panels_fit_each_language_and_keep_preview_overrides_private() {
    let dir = tempfile::tempdir().unwrap();
    let registry = SkinRegistry::new(Some(dir.path().join("packages")), "1.0.0");
    let mut session = session::copy_installed(&registry, "nanatype.halo", "editor.panels").unwrap();
    let (ticket, project) = session.validation_due().unwrap();
    session.validated(ticket, Arc::new(session::validate(&registry, &project)));
    let view = Arc::new(session.view(None, false));
    let captures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("target/package-editor");
    std::fs::create_dir_all(&captures).unwrap();
    for locale in Locale::ALL {
        for width in [960, 1280, 1440, 1920] {
            for tab in [Mode::Files, Mode::Design, Mode::Sounds, Mode::Lua] {
                let ui = Ui::new(
                    Arc::new(Settings::default()),
                    Arc::new(Default::default()),
                    locale,
                );
                let settings = ui.settings.get_untracked();
                ui.package_editor.set(Some(view.clone()));
                ui.mode.set(tab);
                ui.preview_size.set(["300".into(), "600".into()]);
                if tab == Mode::Sounds {
                    ui.properties_file.set("skin.toml".into());
                    ui.properties_path.set(vec!["sounds".into()]);
                }
                let mut agent = common::session().themed(false).mount(width, 1100, move || {
                    widget(Stack::column(0.).height(LengthSpec::Px(1100.)))
                        .children(ui::views::editor(ui))
                });
                agent.flush().unwrap();
                agent
                    .screenshot_png(captures.join(format!(
                        "authoring-{}-{tab:?}-{width}.png",
                        locale.tag().to_lowercase()
                    )))
                    .unwrap();
                assert_eq!(ui.workspace_width.get_untracked(), width as f32);
                if matches!(tab, Mode::Design | Mode::Files) {
                    assert!(
                        agent.accessibility_dump().iter().any(|n| n
                            .agent_path
                            .as_deref()
                            .is_some_and(|p| p.ends_with("package-editor-preview"))),
                        "{tab:?} keeps the stage in view at {width}"
                    );
                }
                let body = agent
                    .accessibility_dump()
                    .into_iter()
                    .find(|n| {
                        n.agent_path
                            .as_deref()
                            .is_some_and(|p| p.ends_with("editor-workspace-body"))
                    })
                    .expect("workspace body");
                assert!(
                    body.bounds.height > 300. && body.bounds.y + body.bounds.height <= 1101.,
                    "{locale} {width} {tab:?}: {body:?}"
                );
                for node in agent.accessibility_dump().iter().filter(|n| {
                    n.agent_path
                        .as_deref()
                        .is_some_and(|p| p.ends_with("editor-panel-scroll"))
                }) {
                    assert!(
                        node.bounds.height > 300.,
                        "{locale} {width} {tab:?}: {node:?}"
                    );
                }
                assert!(
                    Arc::ptr_eq(&settings, &ui.settings.get_untracked()),
                    "preview options must not rewrite app settings"
                );
                for node in agent
                    .accessibility_dump()
                    .iter()
                    .filter(|n| n.role == "button" && n.bounds.width > 0.)
                {
                    assert!(
                        node.bounds.x >= -1.
                            && node.bounds.x + node.bounds.width <= width as f32 + 1.,
                        "{locale} {width} {tab:?}: {node:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn responsive_gestures_reject_masked_layers_and_match_active_layer_output() {
    let dir = tempfile::tempdir().unwrap();
    let registry = SkinRegistry::new(Some(dir.path().join("packages")), "1.0.0");
    let mut s = session::create(editor::ProjectKind::Skin, "editor.layers").unwrap();
    s.edit("skin.toml","[dialogue]\ndecor=[{size=[40,30],offset=[5,6],text='base'}]\n[dialogue.responsive.landscape]\ndecor=[{size=[40,30],offset=[50,60],text='wide'}]\n").unwrap();
    let report = session::validate(&registry, &s.project);
    let skin = report.skin.unwrap();
    let mut edit = canvas::Edit {
        element: "decor.0".into(),
        target: vec!["dialogue".into()],
        size: [800., 240.],
        delta: [10., 5.],
        resize: false,
    };
    let mut next = s.project.clone();
    canvas::commit(&mut next, &skin, &report.properties, &edit).unwrap();
    let after = session::validate(&registry, &next).skin.unwrap();
    assert!(canvas::check_result(&skin, &after, &edit).is_err());
    edit.target = vec!["dialogue".into(), "responsive".into(), "landscape".into()];
    let mut next = s.project.clone();
    canvas::commit(&mut next, &skin, &report.properties, &edit).unwrap();
    let after = session::validate(&registry, &next).skin.unwrap();
    canvas::check_result(&skin, &after, &edit).unwrap();
    let source =
        editor::parse_document(std::str::from_utf8(&next.files["skin.toml"]).unwrap()).unwrap();
    assert_eq!(
        editor::value_at(
            &source,
            &[
                "dialogue".into(),
                "decor".into(),
                "0".into(),
                "offset".into()
            ]
        )
        .unwrap()
        .as_array()
        .unwrap()[0]
            .as_integer(),
        Some(5)
    );
}

#[test]
fn buffered_boolean_controls_match_pending_values_after_tab_switches() {
    let dir = tempfile::tempdir().unwrap();
    let registry = SkinRegistry::new(Some(dir.path().join("packages")), "1.0.0");
    let mut s = session::create(editor::ProjectKind::Skin, "editor.buffers").unwrap();
    let (ticket, p) = s.validation_due().unwrap();
    s.validated(ticket, Arc::new(session::validate(&registry, &p)));
    let key = format!(
        "properties.skin.toml:{:?}",
        ["dialogue", "nameplate", "visible"]
    );
    s.buffer(key, Some(serde_json::json!("false")));
    s.buffer(
        "metadata.capabilities".into(),
        Some(serde_json::json!(["skin", "sounds"])),
    );
    let ui = Ui::new(
        Arc::new(Settings::default()),
        Arc::new(Default::default()),
        Locale::ZhCn,
    );
    ui.package_editor.set(Some(Arc::new(s.view(None, false))));
    ui.mode.set(Mode::Design);
    ui.properties_path
        .set(vec!["dialogue".into(), "nameplate".into()]);
    let mut agent = common::session().themed(false).mount(1440, 1100, move || {
        widget(Stack::column(0.).height(LengthSpec::Px(1100.))).children(ui::views::editor(ui))
    });
    agent.flush().unwrap();
    let captures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("target/package-editor");
    std::fs::create_dir_all(&captures).unwrap();
    agent
        .screenshot_png(captures.join("inspector-1440.png"))
        .unwrap();
    let checked = |agent: &nana_ui_devtools::agent::RuntimeAgentSession, suffix: &str| {
        agent
            .accessibility_dump()
            .iter()
            .find(|n| n.agent_path.as_deref().is_some_and(|p| p.ends_with(suffix)))
            .unwrap()
            .checked
    };
    assert_eq!(
        checked(
            &agent,
            "editor-property:skin.toml:dialogue.nameplate.visible"
        ),
        Some(false)
    );
    ui.mode.set(Mode::Package);
    agent.flush().unwrap();
    assert_eq!(
        checked(&agent, "package-editor-cap:packages-kind-sounds"),
        Some(true)
    );
    ui.mode.set(Mode::Design);
    agent.flush().unwrap();
    assert_eq!(
        checked(
            &agent,
            "editor-property:skin.toml:dialogue.nameplate.visible"
        ),
        Some(false)
    );
    assert_eq!(s.project.files, p.files);
}

#[test]
fn a_parameter_binding_can_be_created_from_a_constant_in_the_property_panel() {
    let dir = tempfile::tempdir().unwrap();
    let registry = SkinRegistry::new(Some(dir.path().join("packages")), "1.0.0");
    let mut s = session::create(editor::ProjectKind::Skin, "editor.binding").unwrap();
    s.edit(
        "skin.toml",
        "[params.paint]\ntype='color'\ndefault='#123456'\n[dialogue.text]\ncolor='#abcdef'\n",
    )
    .unwrap();
    let (ticket, project) = s.validation_due().unwrap();
    s.validated(ticket, Arc::new(session::validate(&registry, &project)));
    s.buffer(
        format!(
            "properties.skin.toml:{:?}.binding",
            ["dialogue", "text", "color"]
        ),
        Some(serde_json::json!("paint")),
    );
    let ui = Ui::new(
        Arc::new(Settings::default()),
        Arc::new(Default::default()),
        Locale::ZhCn,
    );
    ui.package_editor.set(Some(Arc::new(s.view(None, false))));
    ui.mode.set(Mode::Design);
    ui.properties_path
        .set(vec!["dialogue".into(), "text".into()]);
    let mut agent = common::session().themed(false).mount(1280, 1100, move || {
        widget(Stack::column(0.).height(LengthSpec::Px(1100.))).children(ui::views::editor(ui))
    });
    agent.flush().unwrap();
    let node = agent
        .accessibility_dump()
        .into_iter()
        .find(|n| {
            n.agent_path
                .as_deref()
                .is_some_and(|p| p.ends_with("editor-property-bind:skin.toml:dialogue.text.color"))
        })
        .unwrap();
    assert!(!node.disabled);
    let doc = agent.document().document();
    agent
        .document_mut()
        .context_mut()
        .apply_accessibility_action(
            doc,
            nana_ui::runtime::AccessibilityActionRequest {
                target: nana_ui::runtime::StableNodeId::new(node.id).unwrap(),
                action: nana_ui::runtime::AccessibilityAction::Focus,
            },
        )
        .unwrap();
    agent.flush().unwrap();
    agent
        .key_press("Enter", "Enter", Default::default())
        .unwrap();
    agent.flush().unwrap();
    let messages = agent.document_mut().context_mut().take_program_messages();
    let action = messages
        .into_iter()
        .filter_map(|m| m.downcast::<nanatype_editor::msg::Msg>().ok())
        .find_map(|m| match *m {
            nanatype_editor::msg::Msg::PackageEditor(Action::SetDeclaration {
                file,
                path,
                value,
                unbind,
            }) => Some((file, path, value, unbind)),
            _ => None,
        })
        .expect("bind action");
    editor::set_declaration(
        &mut s.project,
        &action.0,
        &action.1,
        action.2.as_deref(),
        None,
        action.3,
    )
    .unwrap();
    assert_eq!(session::validate(&registry, &s.project).errors(), 0);
    let source = std::str::from_utf8(&s.project.files["skin.toml"]).unwrap();
    assert!(source.contains("param") && source.contains("paint"));
}

#[test]
fn a_layer_selects_its_part_its_properties_and_its_source() {
    let dir = tempfile::tempdir().unwrap();
    let registry = SkinRegistry::new(Some(dir.path().join("packages")), "1.0.0");
    let mut s = session::create(editor::ProjectKind::Skin, "editor.layers").unwrap();
    let source = "[dialogue]\n[dialogue.nameplate]\nsize = 20\n";
    s.edit("skin.toml", source).unwrap();
    let (ticket, project) = s.validation_due().unwrap();
    s.validated(ticket, Arc::new(session::validate(&registry, &project)));
    let ui = Ui::new(
        Arc::new(Settings::default()),
        Arc::new(Default::default()),
        Locale::EnUs,
    );
    ui.package_editor.set(Some(Arc::new(s.view(None, false))));
    ui.mode.set(Mode::Design);
    ui.split_source.set(true);
    let mut agent = common::session().themed(false).mount(1440, 1000, move || {
        widget(Stack::column(0.).height(LengthSpec::Px(1000.))).children(ui::views::editor(ui))
    });
    agent.flush().unwrap();
    // A part picked on the canvas selects its layer too.
    ui.canvas_selection.set(Some("portrait".into()));
    agent.flush().unwrap();
    assert_eq!(
        ui.properties_path.get_untracked(),
        vec!["dialogue".to_owned(), "portrait".to_owned()]
    );
    assert!(
        ui.canvas_enabled.get_untracked(),
        "the design canvas takes gestures"
    );
    let reveal = ui::modes::design::select(ui, vec!["dialogue".into(), "nameplate".into()]);
    agent.flush().unwrap();
    assert_eq!(
        ui.properties_path.get_untracked(),
        vec!["dialogue".to_owned(), "nameplate".to_owned()]
    );
    assert_eq!(
        ui.canvas_selection.get_untracked().as_deref(),
        Some("nameplate")
    );
    let offset = source.find("[dialogue.nameplate]").unwrap();
    assert!(
        matches!(
            reveal,
            Some(nanatype_editor::msg::Msg::PackageEditor(Action::Reveal(ref d)))
                if d.path == "skin.toml" && d.offset == offset
        ),
        "the split source scrolls to the layer's table"
    );
    assert!(
        agent.accessibility_dump().iter().any(|n| n
            .agent_path
            .as_deref()
            .is_some_and(|p| p.ends_with("editor-section-text"))),
        "the inspector edits the table's own block"
    );
    let captures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("target/package-editor");
    std::fs::create_dir_all(&captures).unwrap();
    agent
        .screenshot_png(captures.join("design-split-1440.png"))
        .unwrap();
}
