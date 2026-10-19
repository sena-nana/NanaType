//! The package editor: four project kinds round trip, protection and
//! signatures stay truthful, scripts only run in the isolated debugger, and
//! the editor works at narrow widths, in every language and from the keyboard.

mod common;

use nana_ui_devtools::agent::RuntimeAgentSession;
use nanatype::{
    msg::Msg,
    package_editor::{self as pe, Action, KeyStore, MemoryKeys, Session, Tab},
    ui::{self, Ui},
};
use nanatype_i18n::Locale;
use nanatype_plugins::WorkerResponse;
use nanatype_settings::Settings;
use nanatype_theme::editor::{ProjectKind, list_drafts};
use nanatype_theme::{INTEGRITY_FILE, MANIFEST, PACKAGE_MANIFEST, SealIdentity, SkinRegistry};
use std::{
    collections::BTreeSet,
    path::Path,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

fn registry(dir: &Path) -> SkinRegistry {
    SkinRegistry::new(Some(dir.join("installed")), env!("CARGO_PKG_VERSION"))
}

fn worker() -> &'static Path {
    Path::new(env!("CARGO_BIN_EXE_nanatype"))
}

fn text(session: &Session, path: &str) -> String {
    String::from_utf8(session.project.files[path].clone()).unwrap()
}

#[test]
fn four_project_kinds_open_edit_export_and_reimport() {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = registry(dir.path());
    let drafts = dir.path().join("drafts");
    for kind in ProjectKind::ALL {
        let id = format!("editor.{}", kind.key());
        let mut session = pe::create(kind, &id).unwrap();
        assert!(session.dirty(), "a new project is unsaved until saved");
        let (path, edited) = match kind {
            ProjectKind::Skin => (MANIFEST, "[chrome]\naccent = \"#204060\"\n".to_owned()),
            ProjectKind::Resources => ("data/notes.txt", "edited\n".to_owned()),
            ProjectKind::Lua | ProjectKind::Mixed => (
                "main.lua",
                text(&session, "main.lua").replace("waiting for a line", "edited"),
            ),
        };
        session.select(path);
        session.edit(path, &edited).unwrap();
        let report = pe::validate(&registry, &session.project);
        assert_eq!(report.errors(), 0, "{id}: {:?}", report.diagnostics);
        assert_eq!(
            report.skin.is_some(),
            kind.capabilities()
                .contains(&nanatype_theme::PackageCapability::Skin)
        );
        // Drafts save and reopen without validation and keep the edit.
        session.save(&drafts).unwrap();
        assert!(!session.dirty());
        let draft = list_drafts(&drafts)
            .into_iter()
            .find(|d| d.package == id)
            .unwrap();
        let reopened = pe::open_draft(&drafts, &draft.id).unwrap();
        assert_eq!(reopened.project.files, session.project.files);
        assert!(!reopened.dirty());
        // Export validates, then the package imports and opens again unchanged.
        let archive = dir.path().join(format!("{id}.ntpkg"));
        pe::export(&registry, &session.project, &archive).unwrap();
        registry.install(&archive).unwrap();
        let installed = pe::open_installed(&registry, &id).unwrap();
        assert_eq!(installed.project.files, session.project.files, "{id}");
        assert!(!installed.dirty());
        assert_eq!(text(&installed, path), edited);
    }
}

#[test]
fn export_refuses_invalid_packages_and_writes_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(dir.path());
    let mut session = pe::create(ProjectKind::Resources, "editor.broken").unwrap();
    session
        .edit(
            PACKAGE_MANIFEST,
            &text(&session, PACKAGE_MANIFEST).replace("data/notes.txt", "data/missing.txt"),
        )
        .unwrap();
    let report = pe::validate(&registry, &session.project);
    assert!(report.errors() > 0);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|d| d.path == PACKAGE_MANIFEST)
    );
    let archive = dir.path().join("broken.ntpkg");
    assert!(pe::export(&registry, &session.project, &archive).is_err());
    assert!(!archive.exists());
}

#[test]
fn protected_files_stay_read_only_and_the_signature_status_is_truthful() {
    let keys = MemoryKeys::default();
    let key = SealIdentity::generate().unwrap();
    keys.save(&key).unwrap();
    let key = keys.load().unwrap().unwrap();
    let mut session = pe::create(ProjectKind::Mixed, "editor.signed").unwrap();
    session
        .sign(&key, &BTreeSet::from(["main.lua".to_owned()]))
        .unwrap();
    let view = session.view(Some(&key));
    let seal = view.seal.clone().unwrap().unwrap();
    assert_eq!(seal.fingerprint, key.fingerprint());
    assert!(seal.protected.contains("main.lua") && seal.protected.contains(PACKAGE_MANIFEST));
    let row = |path: &str| view.files.iter().find(|f| f.path == path).unwrap().clone();
    assert!(row("main.lua").protected && !row("main.lua").editable);
    assert!(session.edit("main.lua", "return {}").is_err());
    assert_eq!(
        text(&session, "main.lua"),
        text(
            &pe::create(ProjectKind::Mixed, "editor.signed").unwrap(),
            "main.lua"
        )
    );
    // Editable files change and the seal still verifies.
    session
        .edit(MANIFEST, "[chrome]\naccent = \"#000000\"\n")
        .unwrap();
    assert!(matches!(session.view(Some(&key)).seal, Ok(Some(_))));
    // The private key never enters the project.
    let secret = key.secret_bytes();
    let hex: String = secret.iter().map(|b| format!("{b:02x}")).collect();
    for bytes in session.project.files.values() {
        assert!(!bytes.windows(32).any(|w| w == secret));
        assert!(!String::from_utf8_lossy(bytes).contains(&hex));
    }
    // Only the signer's own key may unseal; anyone may derive a labelled copy.
    let other = SealIdentity::generate().unwrap();
    assert!(session.unseal(Some(&other)).is_err());
    assert!(session.unseal(None).is_err());
    let mut copy = pe::create(ProjectKind::Mixed, "editor.signed").unwrap();
    copy.sign(&other, &BTreeSet::from(["main.lua".to_owned()]))
        .unwrap();
    copy.derive("editor.signed-copy").unwrap();
    let view = copy.view(Some(&key));
    assert!(matches!(view.seal, Ok(None)));
    assert_eq!(view.derived_from, "editor.signed@1.0.0");
    assert!(!copy.project.files.contains_key(INTEGRITY_FILE));
    assert!(copy.dirty(), "a derived copy is unsaved");
    copy.edit("main.lua", "return {}").unwrap();
    session.unseal(Some(&key)).unwrap();
    session.edit("main.lua", "return {}").unwrap();
}

/// Run `source` in the debugger until it answers, returning how long the
/// caller was blocked and what the session shows.
fn debug(source: &str, events: &[serde_json::Value]) -> (Duration, pe::DebugView, Session) {
    let mut session = pe::create(ProjectKind::Lua, "editor.debug").unwrap();
    session.edit("main.lua", source).unwrap();
    assert!(
        session.start_debug(worker(), |_, _| {}).is_err(),
        "nothing runs without trust"
    );
    session.trust(true);
    let (send, results) = mpsc::channel();
    let started = Instant::now();
    session
        .start_debug(worker(), move |generation, response| {
            let _ = send.send((generation, response));
        })
        .unwrap();
    let mut blocked = started.elapsed();
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut sent = false;
    while Instant::now() < deadline {
        let Ok((generation, response)) = results.recv_timeout(Duration::from_millis(100)) else {
            continue;
        };
        let update = matches!(response, WorkerResponse::Update { .. });
        session.debug_result(generation, &response);
        if !session.debugging() {
            break;
        }
        if update && !sent {
            sent = true;
            for event in events {
                let call = Instant::now();
                session.debug_event(event.clone()).unwrap();
                blocked = blocked.max(call.elapsed());
            }
            if events.is_empty() {
                break;
            }
        } else if update {
            break;
        }
    }
    let view = session.view(None).debug;
    (blocked, view, session)
}

#[test]
fn scripts_run_only_in_the_isolated_debugger_and_report_located_errors() {
    let (blocked, view, mut session) = debug(
        "local n = 0\nreturn {on_event = function(e) if e.type == 'line_start' then n = n + 1 end end,\n view = function() return {views = {{slot = 'panel', root = {kind = 'text', id = 't', text = 'n=' .. n}}}} end}\n",
        &[serde_json::json!({"type": "line_start", "id": 1, "text": "x", "markup": "x"})],
    );
    assert!(blocked < Duration::from_millis(500), "{blocked:?}");
    assert!(view.running && view.error.is_none(), "{view:?}");
    let update = view.update.unwrap();
    assert!(format!("{update:?}").contains("n=1"), "{update:?}");
    // Editing revokes trust and stops the old instance.
    session.edit("main.lua", "return {}").unwrap();
    let view = session.view(None).debug;
    assert!(!view.trusted && !view.running);
    assert!(session.start_debug(worker(), |_, _| {}).is_err());

    // Hostile scripts end as errors without blocking the caller.
    for (source, line) in [
        (
            "return {\n  on_event = function() end,\n  view = function()\n    while true do end\n  end,\n}\n",
            None,
        ),
        ("local x = 1\nwhile true do x = x + 1 end\n", None),
        (
            "return {\n  view = function()\n    error('boom')\n  end,\n}\n",
            Some(3),
        ),
        ("return {\n  view = function(\n", Some(3)),
        (
            "return {view = function() return {views = {{slot = 'panel', root = {kind = 'canvas', id = 'c', width = 1e9, height = 1, draw = {}}}}} end}",
            None,
        ),
    ] {
        let (blocked, view, session) = debug(source, &[]);
        assert!(
            blocked < Duration::from_millis(500),
            "{source}: {blocked:?}"
        );
        assert!(!view.running, "{source}: {view:?}");
        let error = view.error.clone().expect(source);
        if let Some(line) = line {
            let at = view.location.clone().unwrap_or_else(|| panic!("{error}"));
            assert_eq!((at.path.as_str(), at.line), ("main.lua", line), "{error}");
            session.view(None);
        }
    }
}

/// The editor in a body of definite height, as the settings window gives it.
fn body(ui: Ui, height: u32) -> impl nana_ui::runtime::view::IntoView {
    nana_ui::runtime::view::widget(
        nana_ui::runtime::Stack::column(0.).height(nana_ui_core::LengthSpec::Px(height as f32)),
    )
    .children(ui::package_editor::editor(ui))
}

fn messages(session: &mut RuntimeAgentSession) -> Vec<Msg> {
    session
        .document_mut()
        .context_mut()
        .take_program_messages()
        .into_iter()
        .filter_map(|m| m.downcast::<Msg>().ok().map(|m| *m))
        .collect()
}

#[test]
fn the_editor_fits_narrow_widths_in_every_language_and_works_from_the_keyboard() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(dir.path());
    let captures = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("target/package-editor");
    std::fs::create_dir_all(&captures).unwrap();
    let mut session = pe::create(ProjectKind::Mixed, "editor.screens").unwrap();
    session.edit(MANIFEST, "[chrome]\naccent = 3\n").unwrap();
    let (ticket, project) = {
        let mut s = session;
        let due = s.validation_due().unwrap();
        session = s;
        due
    };
    session.validated(ticket, Arc::new(pe::validate(&registry, &project)));
    session.select(MANIFEST);
    let view = Arc::new(session.view(None));
    assert!(view.report.errors() > 0);
    for locale in Locale::ALL {
        for (width, height) in [(900, 660), (440, 900), (300, 1100)] {
            for tab in [Tab::Files, Tab::Package, Tab::Signing, Tab::Debug] {
                let ui = Ui::new(
                    Arc::new(Settings::default()),
                    Arc::new(Default::default()),
                    locale,
                );
                ui.package_editor.set(Some(view.clone()));
                ui.package_editor_tab.set(tab);
                let mut agent = common::session()
                    .themed(false)
                    .mount(width, height, move || body(ui, height));
                agent.flush().unwrap();
                let nodes = agent.accessibility_dump();
                let fields = [
                    "package-editor-debug-custom",
                    "package-editor-derive-id",
                    "package-editor-search",
                ];
                for node in nodes.iter().filter(|n| {
                    n.role == "button"
                        || n.agent_path
                            .as_deref()
                            .is_some_and(|p| fields.iter().any(|f| p.ends_with(f)))
                }) {
                    assert!(
                        node.bounds.x >= -0.5
                            && node.bounds.x + node.bounds.width <= width as f32 + 0.5,
                        "{locale:?} {tab:?} button off-screen at {width}px: {:?} {:?}",
                        node.label,
                        node.bounds
                    );
                }
                agent
                    .screenshot_png(
                        captures
                            .join(format!("{}-{tab:?}-{width}.png", locale.tag()).to_lowercase()),
                    )
                    .unwrap();
            }
        }
    }
    // The launcher on the packages page fits too.
    for locale in Locale::ALL {
        for width in [900, 300] {
            let ui = Ui::new(
                Arc::new(Settings::default()),
                Arc::new(Default::default()),
                locale,
            );
            let mut agent = common::session()
                .themed(false)
                .mount(width, 600, move || ui::package_editor::launcher(ui));
            agent.flush().unwrap();
            for node in agent.accessibility_dump().iter().filter(|n| {
                n.role == "button"
                    || n.agent_path
                        .as_deref()
                        .is_some_and(|p| p.ends_with("package-editor-id"))
            }) {
                assert!(
                    node.bounds.x + node.bounds.width <= width as f32 + 0.5,
                    "{locale:?} launcher control off-screen at {width}px: {:?}",
                    node.label
                );
            }
        }
    }
    // Keyboard: Tab reaches Save, Enter activates it.
    let ui = Ui::new(
        Arc::new(Settings::default()),
        Arc::new(Default::default()),
        Locale::EnUs,
    );
    ui.package_editor.set(Some(view.clone()));
    ui.page.set(ui::settings::Page::Packages);
    let mut agent = common::session()
        .themed(false)
        .mount(900, 660, move || ui::settings::settings_window(ui));
    agent.flush().unwrap();
    agent
        .screenshot_png(captures.join("settings-window-900.png"))
        .unwrap();
    let save = agent
        .accessibility_dump()
        .into_iter()
        .find(|n| {
            n.agent_path
                .as_deref()
                .is_some_and(|p| p.ends_with("package-editor-save"))
        })
        .unwrap();
    let focused = |agent: &RuntimeAgentSession| {
        agent
            .accessibility_dump()
            .iter()
            .any(|n| n.id == save.id && n.focused)
    };
    for _ in 0..40 {
        if focused(&agent) {
            break;
        }
        agent.key_press("Tab", "Tab", Default::default()).unwrap();
    }
    assert!(focused(&agent), "Save is reachable with Tab");
    agent
        .key_press("Enter", "Enter", Default::default())
        .unwrap();
    assert!(
        messages(&mut agent)
            .iter()
            .any(|m| matches!(m, Msg::PackageEditor(Action::Save)))
    );
    // Typing into the editable source sends the edit for that file.
    let source = agent
        .accessibility_dump()
        .into_iter()
        .find(|n| {
            n.agent_path
                .as_deref()
                .is_some_and(|p| p.contains("package-editor-source:skin.toml"))
        })
        .unwrap();
    agent.click_node(source.id).unwrap();
    agent.type_text("#").unwrap();
    agent.flush().unwrap();
    assert!(messages(&mut agent).iter().any(
        |m| matches!(m, Msg::PackageEditor(Action::Edit { path, text }) if path == MANIFEST && text.contains('#'))
    ));
    // Search counts matches in the open file.
    let search = agent
        .accessibility_dump()
        .into_iter()
        .find(|n| {
            n.agent_path
                .as_deref()
                .is_some_and(|p| p.ends_with("package-editor-search"))
        })
        .unwrap();
    agent.click_node(search.id).unwrap();
    agent.type_text("ACCENT").unwrap();
    agent.flush().unwrap();
    assert!(
        agent
            .accessibility_dump()
            .iter()
            .any(|n| n.label.as_deref() == Some("1 / 1")),
        "case-insensitive search finds `accent`"
    );
    // A problem row reveals its location in the source.
    let problem = agent
        .accessibility_dump()
        .into_iter()
        .find(|n| {
            n.agent_path
                .as_deref()
                .is_some_and(|p| p.contains("package-editor-problem:skin.toml"))
        })
        .expect("the skin.toml problem is listed");
    assert!(agent.click_node(problem.id).unwrap());
    assert!(
        messages(&mut agent)
            .iter()
            .any(|m| matches!(m, Msg::PackageEditor(Action::Reveal(d)) if d.path == MANIFEST))
    );
}

#[test]
fn protected_sources_show_read_only_and_drafts_never_touch_installed_packages() {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = registry(dir.path());
    let mut session = pe::create(ProjectKind::Lua, "editor.installed").unwrap();
    let archive = dir.path().join("p.ntpkg");
    pe::export(&registry, &session.project, &archive).unwrap();
    registry.install(&archive).unwrap();
    let installed_dir = dir.path().join("installed/editor.installed");
    let before = std::fs::read(installed_dir.join("main.lua")).unwrap();
    let mut opened = pe::open_installed(&registry, "editor.installed").unwrap();
    opened.edit("main.lua", "return {changed = true}").unwrap();
    opened.save(&dir.path().join("drafts")).unwrap();
    assert_eq!(
        std::fs::read(installed_dir.join("main.lua")).unwrap(),
        before
    );
    // A signed source renders as a read-only editor.
    let key = SealIdentity::generate().unwrap();
    session
        .sign(&key, &BTreeSet::from(["main.lua".to_owned()]))
        .unwrap();
    session.select("main.lua");
    let view = Arc::new(session.view(None));
    assert!(!view.editable);
    let ui = Ui::new(
        Arc::new(Settings::default()),
        Arc::new(Default::default()),
        Locale::ZhCn,
    );
    ui.package_editor.set(Some(view));
    let mut agent = common::session()
        .themed(false)
        .mount(900, 660, move || body(ui, 660));
    agent.flush().unwrap();
    let nodes = agent.accessibility_dump();
    let source = nodes
        .iter()
        .find(|n| {
            n.agent_path
                .as_deref()
                .is_some_and(|p| p.contains("package-editor-source:main.lua"))
        })
        .expect("source editor");
    let source = source.id;
    assert!(
        nodes
            .iter()
            .any(|n| n.label.as_deref() == Some(ui.now_t("pkgedit-read-only").as_str()))
    );
    // Typing into the protected source changes nothing and sends no edit.
    agent.click_node(source).unwrap();
    agent.type_text("evil()").unwrap();
    agent.flush().unwrap();
    assert!(
        !messages(&mut agent)
            .iter()
            .any(|m| matches!(m, Msg::PackageEditor(Action::Edit { .. })))
    );
}

#[test]
fn skin_previews_reuse_the_real_renderer_without_touching_the_active_skin() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(dir.path());
    let mut session = pe::create(ProjectKind::Skin, "editor.preview").unwrap();
    session
        .edit(
            MANIFEST,
            "[chrome]\naccent = \"#c03060\"\n\n[dialogue.box_]\ncolor = \"#203040\"\n",
        )
        .unwrap();
    let (ticket, project) = session.validation_due().unwrap();
    session.validated(ticket, Arc::new(pe::validate(&registry, &project)));
    let view = Arc::new(session.view(None));
    assert!(view.report.skin.is_some(), "{:?}", view.report.diagnostics);
    let captures = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("target/package-editor");
    std::fs::create_dir_all(&captures).unwrap();
    for (width, history) in [(900, false), (900, true), (300, false)] {
        let ui = Ui::new(
            Arc::new(Settings::default()),
            Arc::new(Default::default()),
            Locale::ZhCn,
        );
        let active = ui.skin.get_untracked();
        ui.package_editor.set(Some(view.clone()));
        ui.package_editor_tab.set(Tab::Preview);
        ui.skin_preview_history.set(history);
        let mut agent = common::session()
            .themed(false)
            .mount(width, 900, move || body(ui, 900));
        agent.flush().unwrap();
        agent.flush().unwrap();
        let nodes = agent.accessibility_dump();
        assert!(
            nodes.iter().any(|n| n
                .agent_path
                .as_deref()
                .is_some_and(|p| p.ends_with("skin-preview-box"))),
            "the preview stage renders"
        );
        assert!(
            Arc::ptr_eq(&active, &ui.skin.get_untracked()),
            "the active skin is untouched"
        );
        agent
            .screenshot_png(captures.join(format!(
                "preview-{}-{width}.png",
                if history { "history" } else { "display" }
            )))
            .unwrap();
    }
}
