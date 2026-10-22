//! The package editor: four project kinds round trip, protection and
//! signatures stay truthful, scripts only run in the isolated debugger, and
//! the editor works at narrow widths, in every language and from the keyboard.

mod common;

use nana_ui_devtools::agent::RuntimeAgentSession;
use nanatype_editor::{
    msg::Msg,
    session::{self as pe, Action, KeyStore, Session, Tab},
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
    Path::new(env!("CARGO_BIN_EXE_nanatype-editor"))
}

fn text(session: &Session, path: &str) -> String {
    String::from_utf8(session.project.files[path].clone()).unwrap()
}

#[test]
fn every_project_kind_opens_edits_exports_and_reimports() {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = registry(dir.path());
    let drafts = dir.path().join("drafts");
    for kind in ProjectKind::ALL {
        let id = format!("editor.{}", kind.key());
        let mut session = pe::create(kind, &id).unwrap();
        assert!(session.dirty(), "a new project is unsaved until saved");
        let (path, edited) = match kind {
            ProjectKind::Skin => (MANIFEST, "[chrome]\naccent = \"#204060\"\n".to_owned()),
            ProjectKind::Resources => {
                let mut spec = session.view(None, false).data["notes"].clone();
                spec.path = session.import("notes.json", b"edited\n".to_vec()).unwrap();
                session
                    .apply_data(Some("notes"), Some(("notes", &spec)))
                    .unwrap();
                ("assets/notes.json", "edited\n".to_owned())
            }
            ProjectKind::Sounds => (nanatype_theme::SOUNDS_MANIFEST, "[sounds.se]\n".to_owned()),
            ProjectKind::Lua | ProjectKind::Mixed => (
                "main.lua",
                text(&session, "main.lua").replace("waiting for a line", "edited"),
            ),
        };
        session.select(path);
        session.edit(path, &edited).unwrap();
        assert!(session.view(None, false).syntax.is_empty(), "{id}");
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
        .edit("data/notes.txt", &"x".repeat((4 << 20) + 1))
        .unwrap();
    let report = pe::validate(&registry, &session.project);
    assert!(report.errors() > 0);
    assert!(report.diagnostics.iter().any(|d| d.path == PACKAGE_MANIFEST
        && d.line > 1
        && d.message.contains("data.notes")
        && d.message.contains("4194305")));
    let archive = dir.path().join("broken.ntpkg");
    assert!(pe::export(&registry, &session.project, &archive).is_err());
    assert!(!archive.exists());
}

#[test]
fn protected_files_stay_read_only_and_the_signature_status_is_truthful() {
    let dir = tempfile::tempdir().unwrap();
    #[cfg(windows)]
    let keys = pe::SecretKeys::new(&dir.path().join("签名密钥 日本語"));
    #[cfg(not(windows))]
    let keys = pe::MemoryKeys::default();
    let key = SealIdentity::generate().unwrap();
    keys.save(&key).unwrap();
    #[cfg(windows)]
    let keys = pe::SecretKeys::new(&dir.path().join("签名密钥 日本語"));
    let key = keys.load().unwrap().unwrap();
    let mut session = pe::create(ProjectKind::Mixed, "editor.signed").unwrap();
    session
        .sign(&key, &BTreeSet::from(["main.lua".to_owned()]))
        .unwrap();
    let mut registry = registry(dir.path());
    let archive = dir.path().join("签名包.ntpkg");
    pe::export(&registry, &session.project, &archive).unwrap();
    registry.install(&archive).unwrap();
    let mut session = pe::open_installed(&registry, "editor.signed").unwrap();
    let view = session.view(Some(&key), false);
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
    assert!(matches!(session.view(Some(&key), false).seal, Ok(Some(_))));
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
    let view = copy.view(Some(&key), false);
    assert!(matches!(view.seal, Ok(None)));
    assert_eq!(view.derived_from, "editor.signed@1.0.0");
    assert!(!copy.project.files.contains_key(INTEGRITY_FILE));
    assert!(copy.dirty(), "a derived copy is unsaved");
    copy.edit("main.lua", "return {}").unwrap();
    session.unseal(Some(&key)).unwrap();
    session.edit("main.lua", "return {}").unwrap();
    #[cfg(windows)]
    {
        let store = nanatype_secrets::SecretStore::new(
            &dir.path().join("签名密钥 日本語"),
            "package-signing",
        );
        std::fs::write(
            dir.path().join("签名密钥 日本語/package-signing.dpapi"),
            b"broken",
        )
        .unwrap();
        assert!(keys.load().is_err());
        store.save(b"short key").unwrap();
        assert!(keys.load().is_err());
    }
}

#[test]
fn drafts_replace_saved_content_and_keep_unsaved_work_on_failure() {
    let dir = tempfile::tempdir().unwrap();
    let drafts = dir.path().join("草稿 日本語");
    let mut session = pe::create(ProjectKind::Lua, "editor.draft").unwrap();
    assert!(!session.request_close());
    assert!(session.view(None, false).confirm_close);
    session.cancel_close();
    assert!(!session.view(None, false).confirm_close);
    session.save(&drafts).unwrap();
    session.edit("main.lua", "return {changed = true}").unwrap();
    session.save(&drafts).unwrap();
    let saved = session.project.files.clone();
    session.edit("main.lua", "return {unsaved = true}").unwrap();
    let file = dir.path().join("not-a-directory");
    std::fs::write(&file, b"keep").unwrap();
    assert!(session.save(&file).is_err());
    assert!(session.dirty());
    assert!(!session.request_close());
    assert!(text(&session, "main.lua").contains("unsaved"));
    assert_eq!(
        pe::open_draft(&drafts, &session.info.id)
            .unwrap()
            .project
            .files,
        saved
    );
    session.revert();
    assert_eq!(session.project.files, saved);
    assert!(session.request_close());
}

#[test]
fn validation_results_cannot_replace_newer_edits_or_another_session() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(dir.path());
    let mut session = pe::create(ProjectKind::Skin, "editor.validation").unwrap();
    let (old_ticket, old_project) = session.validation_due().unwrap();
    let old_report = Arc::new(pe::validate(&registry, &old_project));
    session.edit(MANIFEST, "[chrome]\naccent = 3\n").unwrap();
    session.validated(old_ticket, old_report.clone());
    assert!(session.view(None, false).report.skin.is_none());
    let (ticket, project) = session.validation_due().unwrap();
    session.validated(ticket, Arc::new(pe::validate(&registry, &project)));
    assert!(session.view(None, false).report.errors() > 0);
    session.validated(old_ticket, old_report.clone());
    assert!(session.view(None, false).report.errors() > 0);
    let mut reopened = pe::create(ProjectKind::Skin, "editor.validation").unwrap();
    reopened.validation_due().unwrap();
    reopened.validated(old_ticket, old_report);
    assert!(reopened.view(None, false).validating);
    assert!(reopened.view(None, false).report.skin.is_none());
}

/// Run `source` in the debugger until it answers, returning how long the
/// caller was blocked and what the session shows.
fn debug(source: &str, events: &[serde_json::Value]) -> (Duration, pe::DebugView, Session, u64) {
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
    let mut last_generation = 0;
    while Instant::now() < deadline {
        let Ok((generation, response)) = results.recv_timeout(Duration::from_millis(100)) else {
            continue;
        };
        let update = matches!(response, WorkerResponse::Update { .. });
        last_generation = generation;
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
    let view = session.view(None, false).debug;
    (blocked, view, session, last_generation)
}

#[test]
fn scripts_run_only_in_the_isolated_debugger_and_report_located_errors() {
    let (blocked, view, mut session, generation) = debug(
        "local n = 0\nreturn {on_event = function(e) if e.type == 'line_start' then n = n + 1 end end,\n view = function() return {views = {{slot = 'panel', root = {kind = 'text', id = 't', text = 'n=' .. n}}}} end}\n",
        &[serde_json::json!({"type": "line_start", "id": 1, "text": "x", "markup": "x"})],
    );
    assert!(blocked < Duration::from_millis(500), "{blocked:?}");
    assert!(view.running && view.error.is_none(), "{view:?}");
    let update = view.update.unwrap();
    assert!(format!("{update:?}").contains("n=1"), "{update:?}");
    // Stopped workers cannot publish success or failure, including after restart.
    session.stop_debug();
    let stopped = session.view(None, false).debug;
    let late = WorkerResponse::Error {
        message: "late".into(),
    };
    session.debug_result(
        generation,
        &WorkerResponse::Update {
            update,
            requests: vec![],
        },
    );
    session.debug_result(generation, &late);
    assert_eq!(session.view(None, false).debug, stopped);
    session.start_debug(worker(), |_, _| {}).unwrap();
    let running = session.view(None, false).debug;
    session.debug_result(generation, &late);
    assert_eq!(session.view(None, false).debug, running);
    // Editing revokes trust and stops the old instance.
    session.edit("main.lua", "return {}").unwrap();
    let view = session.view(None, false).debug;
    assert!(!view.trusted && !view.running);
    assert!(session.start_debug(worker(), |_, _| {}).is_err());

    // OS launch failures return through the same generation-checked result path.
    session.trust(true);
    let directory = tempfile::tempdir().unwrap();
    let (send, results) = mpsc::channel();
    session
        .start_debug(
            &directory.path().join("missing.exe"),
            move |generation, response| {
                let _ = send.send((generation, response));
            },
        )
        .unwrap();
    let (generation, response) = results.recv_timeout(Duration::from_secs(20)).unwrap();
    assert!(matches!(response, WorkerResponse::Error { .. }));
    session.debug_result(generation, &response);
    assert!(!session.debugging() && session.view(None, false).debug.error.is_some());

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
        let (blocked, view, session, _) = debug(source, &[]);
        assert!(
            blocked < Duration::from_millis(500),
            "{source}: {blocked:?}"
        );
        assert!(!view.running, "{source}: {view:?}");
        let error = view.error.clone().expect(source);
        if let Some(line) = line {
            let at = view.location.clone().unwrap_or_else(|| panic!("{error}"));
            assert_eq!((at.path.as_str(), at.line), ("main.lua", line), "{error}");
            session.view(None, false);
        }
    }
}

/// The editor in a body of definite height, as the settings window gives it.
fn body(ui: Ui, height: u32) -> impl nana_ui::runtime::view::IntoView {
    nana_ui::runtime::view::widget(
        nana_ui::runtime::Stack::column(0.).height(nana_ui_core::LengthSpec::Px(height as f32)),
    )
    .children(ui::views::editor(ui))
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

fn save_from_keyboard(agent: &mut RuntimeAgentSession) {
    let save = keyed(&agent.accessibility_dump(), "package-editor-save")
        .unwrap()
        .id;
    let focused = |agent: &RuntimeAgentSession| {
        agent
            .accessibility_dump()
            .iter()
            .any(|n| n.id == save && n.focused)
    };
    for _ in 0..40 {
        if focused(agent) {
            break;
        }
        agent.key_press("Tab", "Tab", Default::default()).unwrap();
    }
    assert!(focused(agent), "Save is reachable with Tab");
    agent
        .key_press("Enter", "Enter", Default::default())
        .unwrap();
    assert!(
        messages(agent)
            .iter()
            .any(|m| matches!(m, Msg::PackageEditor(Action::Save)))
    );
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
    let view = Arc::new(session.view(None, false));
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
                if tab == Tab::Files {
                    save_from_keyboard(&mut agent);
                }
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
                .mount(width, 600, move || ui::views::launcher(ui));
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
    let mut agent = common::session().themed(false).mount(900, 660, move || {
        ui::window::editor_window(ui, nana_ui::runtime::view::entity_ref())
    });
    agent.flush().unwrap();
    agent
        .screenshot_png(captures.join("editor-window-900.png"))
        .unwrap();
    save_from_keyboard(&mut agent);
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
    let view = Arc::new(session.view(None, false));
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
    let view = Arc::new(session.view(None, false));
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
        ui.package_editor_surface
            .set(ui::preview::Surface::of(history));
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

fn validated(registry: &SkinRegistry, mut session: Session) -> (Session, Arc<pe::EditorView>) {
    if let Some((ticket, project)) = session.validation_due() {
        session.validated(ticket, Arc::new(pe::validate(registry, &project)));
    }
    let view = Arc::new(session.view(None, false));
    (session, view)
}

fn keyed<'a>(
    nodes: &'a [nana_ui_devtools::agent::AccessibilityDumpNode],
    key: &str,
) -> Option<&'a nana_ui_devtools::agent::AccessibilityDumpNode> {
    nodes.iter().find(|n| {
        n.agent_path
            .as_deref()
            .is_some_and(|p| p.rsplit('/').next() == Some(key))
    })
}

fn click_key(agent: &mut RuntimeAgentSession, key: &str) {
    agent.flush().unwrap();
    let node = keyed(&agent.accessibility_dump(), key)
        .unwrap_or_else(|| panic!("missing {key}"))
        .clone();
    // Text areas take focus from a pointer press, not an accessibility click.
    if !agent.click_node(node.id).unwrap() {
        let b = node.bounds;
        agent
            .click_xy(b.x + b.width / 2.0, b.y + b.height / 2.0)
            .unwrap();
    }
    agent.flush().unwrap();
}

fn type_into(agent: &mut RuntimeAgentSession, key: &str, text: &str) {
    click_key(agent, key);
    agent
        .key_press("a", "KeyA", nana_ui::runtime::KeyModifiers::primary())
        .unwrap();
    agent.type_text(text).unwrap();
    agent.flush().unwrap();
}

#[test]
fn data_and_pipeline_forms_write_in_place_and_refuse_invalid_edits() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(dir.path());
    let mut session = pe::copy_installed(&registry, "nanatype.love_letter", "my.letter").unwrap();
    let manifest = text(&session, PACKAGE_MANIFEST)
        .replace("[data.strokes]", "# stroke order data\n[data.strokes]");
    session.edit(PACKAGE_MANIFEST, &manifest).unwrap();
    let view = session.view(None, false);
    let mut strokes = view.data["strokes"].clone();
    strokes.license.push_str(" (checked)");
    session
        .apply_data(Some("strokes"), Some(("strokes", &strokes)))
        .unwrap();
    let manifest = text(&session, PACKAGE_MANIFEST);
    assert!(
        manifest.contains("# stroke order data\n[data.strokes]"),
        "{manifest}"
    );
    assert_eq!(session.view(None, false).data["strokes"], strokes);
    // Refused edits change nothing and are reported.
    let before = session.project.files.clone();
    let mut wrong = strokes.clone();
    wrong.path = "missing.nthz".into();
    assert!(
        session
            .apply_data(Some("strokes"), Some(("strokes", &wrong)))
            .is_err()
    );
    let (_, stroke_map) = session.view(None, false).pipelines[0].clone();
    let mut pipeline = stroke_map.unwrap();
    assert!(
        session.apply_pipeline(Some("stroke_map"), None).is_err(),
        "a material still reads it"
    );
    pipeline.inputs.get_mut("strokes").unwrap().data = Some("absent".into());
    assert!(
        session
            .apply_pipeline(Some("stroke_map"), Some(("stroke_map", &pipeline)))
            .is_err()
    );
    assert_eq!(session.project.files, before);
    // A renamed data declaration is still valid once the pipeline follows.
    session
        .apply_pipeline(
            None,
            Some((
                "spare",
                &session.view(None, false).pipelines[0].1.clone().unwrap(),
            )),
        )
        .unwrap();
    let (mut session, view) = validated(&registry, session);
    assert_eq!(view.report.errors(), 0, "{:?}", view.report.diagnostics);
    assert!(view.pipelines.iter().any(|(n, _)| n == "spare"));
    assert!(view.report.skin.is_some());
    let archive = dir.path().join("letter.ntpkg");
    pe::export(&registry, &session.project, &archive).unwrap();
    session
        .edit(
            MANIFEST,
            &text(&session, MANIFEST).replace("text.stroke_time_map", "unknown.processor"),
        )
        .unwrap();
    assert!(
        pe::validate(&registry, &session.project)
            .diagnostics
            .iter()
            .any(|d| d.path == MANIFEST
                && d.line > 1
                && d.message.contains("pipelines.stroke_map.processor"))
    );

    // Signed manifests and skins show the forms read-only.
    let mut signed = pe::create(ProjectKind::Mixed, "editor.signed-forms").unwrap();
    let key = SealIdentity::generate().unwrap();
    signed
        .sign(&key, &BTreeSet::from([MANIFEST.to_owned()]))
        .unwrap();
    let notes = signed.view(None, false).data["notes"].clone();
    assert!(signed.apply_data(None, Some(("more", &notes))).is_err());
    assert!(signed.apply_data(Some("notes"), None).is_err());
    let (_, view) = validated(&registry, signed);
    let ui = Ui::new(
        Arc::new(Settings::default()),
        Arc::new(Default::default()),
        Locale::ZhCn,
    );
    ui.package_editor.set(Some(view));
    ui.package_editor_tab.set(Tab::Package);
    let mut agent = common::session()
        .themed(false)
        .mount(900, 1400, move || body(ui, 1400));
    agent.flush().unwrap();
    let nodes = agent.accessibility_dump();
    for key in [
        "package-editor-data-add",
        "package-editor-pipeline-add",
        "package-editor-data:notes:edit",
    ] {
        assert!(
            keyed(&nodes, key).is_none(),
            "{key} is offered on a signed package"
        );
    }
    for key in ["pkgedit-manifest-protected", "pkgedit-skin-protected"] {
        assert!(
            nodes
                .iter()
                .any(|n| n.label.as_deref() == Some(ui.now_t(key).as_str())),
            "{key}"
        );
    }
}

#[test]
fn the_forms_send_their_values_and_fit_every_width_and_language() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(dir.path());
    let session = pe::copy_installed(&registry, "nanatype.love_letter", "my.letter").unwrap();
    let (mut session, view) = validated(&registry, session);
    let captures = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("target/package-editor");
    std::fs::create_dir_all(&captures).unwrap();
    for locale in Locale::ALL {
        for (width, height) in [(900, 1700), (440, 2300), (300, 2900)] {
            let ui = Ui::new(
                Arc::new(Settings::default()),
                Arc::new(Default::default()),
                locale,
            );
            ui.package_editor.set(Some(view.clone()));
            ui.package_editor_tab.set(Tab::Package);
            let mut agent = common::session()
                .themed(false)
                .mount(width, height, move || body(ui, height));
            click_key(&mut agent, "package-editor-data-add");
            click_key(&mut agent, "package-editor-pipeline:stroke_map:edit");
            // The remove confirmation fits beside its question too.
            click_key(&mut agent, "package-editor-data:strokes:remove");
            let nodes = agent.accessibility_dump();
            assert!(
                nodes
                    .iter()
                    .any(|node| node.label.as_deref().is_some_and(|label| label
                        .contains(&ui.now_t("pkgedit-pipeline-outputs"))
                        && label.contains("time: Texture")
                        && label.contains("extent: Vec4"))),
                "{locale:?} {width}: typed pipeline outputs"
            );
            for key in [
                "package-editor-data-form",
                "package-editor-pipeline-form",
                "package-editor-pipeline-input:strokes",
            ] {
                assert!(keyed(&nodes, key).is_some(), "{locale:?} {width}: {key}");
            }
            for node in nodes.iter().filter(|n| {
                n.role == "button"
                    || n.agent_path
                        .as_deref()
                        .is_some_and(|p| p.contains("package-editor-field:") || p.contains("-form"))
            }) {
                assert!(
                    node.bounds.x >= -0.5
                        && node.bounds.x + node.bounds.width <= width as f32 + 0.5,
                    "{locale:?} form control off-screen at {width}px: {:?} {:?}",
                    node.label,
                    node.bounds
                );
            }
            agent
                .screenshot_png(
                    captures.join(format!("{}-forms-{width}.png", locale.tag()).to_lowercase()),
                )
                .unwrap();
        }
    }
    // Typing into the data form sends one declaration; the session writes it.
    session
        .import("words.txt", b"{\"words\": [\"a\"]}".to_vec())
        .unwrap();
    let (mut session, view) = validated(&registry, session);
    let ui = Ui::new(
        Arc::new(Settings::default()),
        Arc::new(Default::default()),
        Locale::EnUs,
    );
    ui.package_editor.set(Some(view));
    ui.package_editor_tab.set(Tab::Package);
    let mut agent = common::session()
        .themed(false)
        .mount(900, 1700, move || body(ui, 1700));
    click_key(&mut agent, "package-editor-data-add");
    for (key, value) in [
        ("package-editor-field:pkgedit-data-id", "words"),
        ("package-editor-field:pkgedit-data-path", "assets/words.txt"),
        ("package-editor-field:package-data-source", "original work"),
        ("package-editor-field:package-data-license", "CC0-1.0"),
        (
            "package-editor-data-constraints",
            "{\"type\": \"object\", \"required\": [\"words\"]}",
        ),
    ] {
        type_into(&mut agent, key, value);
    }
    // JSON constraints with the text format are refused by the session.
    messages(&mut agent);
    click_key(&mut agent, "package-editor-data-apply");
    let Some(Msg::PackageEditor(Action::ApplyData { previous, entry })) =
        messages(&mut agent).into_iter().next()
    else {
        panic!("the form sends its declaration");
    };
    let (id, mut spec) = entry.unwrap();
    assert_eq!((previous, id.as_str()), (None, "words"));
    assert_eq!(spec.path, "assets/words.txt");
    assert_eq!(
        spec.constraints.as_ref().unwrap().required,
        vec!["words".to_owned()]
    );
    let before = session.project.files.clone();
    assert!(session.apply_data(None, Some((&id, &spec))).is_err());
    assert_eq!(session.project.files, before);
    spec.format = nanatype_theme::DataFormat::Json;
    session.apply_data(None, Some((&id, &spec))).unwrap();
    assert_eq!(session.view(None, false).data["words"], spec);
    session.select("assets/words.txt");
    session.format().unwrap();
    assert!(text(&session, "assets/words.txt").contains("\n  \"words\""));
    // Bad constraints never leave the form.
    type_into(
        &mut agent,
        "package-editor-data-constraints",
        "{\"typo\": 1}",
    );
    click_key(&mut agent, "package-editor-data-apply");
    assert!(
        messages(&mut agent)
            .iter()
            .all(|m| matches!(m, Msg::Notice { error: true, .. }))
    );
    // The pipeline form sends the edited input binding.
    click_key(&mut agent, "package-editor-data-cancel");
    click_key(&mut agent, "package-editor-pipeline:stroke_map:edit");
    type_into(
        &mut agent,
        "package-editor-pipeline-input:strokes",
        "pkg:other.pack/strokes",
    );
    messages(&mut agent);
    click_key(&mut agent, "package-editor-pipeline-apply");
    let Some(Msg::PackageEditor(Action::ApplyPipeline { previous, entry })) =
        messages(&mut agent).into_iter().next()
    else {
        panic!("the form sends its pipeline");
    };
    let (name, spec) = entry.unwrap();
    assert_eq!(
        (previous.as_deref(), name.as_str()),
        (Some("stroke_map"), "stroke_map")
    );
    assert_eq!(spec.processor, "text.stroke_time_map");
    assert_eq!(
        spec.inputs["strokes"].data.as_deref(),
        Some("pkg:other.pack/strokes")
    );
    // Declared JSON is formattable even with a text filename.
    ui.package_editor
        .set(Some(Arc::new(session.view(None, false))));
    ui.package_editor_tab.set(Tab::Files);
    click_key(&mut agent, "package-editor-format");
    assert!(
        messages(&mut agent)
            .iter()
            .any(|m| matches!(m, Msg::PackageEditor(Action::Format)))
    );
    agent
        .screenshot_png(captures.join("declared-json-txt-900.png"))
        .unwrap();
}

#[test]
fn the_input_preview_draws_the_draft_skin_without_touching_the_input_window() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(dir.path());
    // A decorated built-in skin, copied: images and fonts come from the draft.
    let session = pe::copy_installed(&registry, "nanatype.sakura", "my.sakura").unwrap();
    let (_, view) = validated(&registry, session);
    assert!(view.report.skin.is_some(), "{:?}", view.report.diagnostics);
    let captures = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("target/package-editor");
    std::fs::create_dir_all(&captures).unwrap();
    for locale in Locale::ALL {
        for width in [900, 440, 300] {
            let ui = Ui::new(
                Arc::new(Settings::default()),
                Arc::new(Default::default()),
                locale,
            );
            let typed = Arc::new(nanatype_model::Document::plain("live draft"));
            ui.draft.set(typed.clone());
            let (skin, settings, editor) = (
                ui.skin.get_untracked(),
                ui.settings.get_untracked(),
                ui.editor_value.get_untracked(),
            );
            ui.package_editor.set(Some(view.clone()));
            ui.package_editor_tab.set(Tab::Preview);
            ui.package_editor_surface.set(ui::preview::Surface::Input);
            let mut agent = common::session()
                .themed(false)
                .mount(width, 900, move || body(ui, 900));
            agent.flush().unwrap();
            agent.flush().unwrap();
            let nodes = agent.accessibility_dump();
            let stage = keyed(&nodes, "skin-preview-box").expect("the stage renders");
            assert!(
                stage.bounds.x >= -0.5 && stage.bounds.x + stage.bounds.width <= width as f32 + 0.5,
                "{locale:?} {width}: {:?}",
                stage.bounds
            );
            assert!(keyed(&nodes, "skin-preview-input").is_some());
            // The sample editor shows the localized sample, takes no focus or input.
            let sample = nodes
                .iter()
                .find(|n| {
                    n.role == "text-input"
                        && n.agent_path
                            .as_deref()
                            .is_some_and(|p| p.contains("skin-preview-input"))
                })
                .unwrap_or_else(|| {
                    panic!(
                        "{locale:?}: sample editor in {:#?}",
                        nodes
                            .iter()
                            .map(|n| (&n.role, &n.agent_path))
                            .collect::<Vec<_>>()
                    )
                });
            assert!(sample.disabled, "{sample:?}");
            for _ in 0..40 {
                agent.key_press("Tab", "Tab", Default::default()).unwrap();
                agent.type_text("x").unwrap();
                assert!(
                    !agent.accessibility_dump().iter().any(|n| n.focused
                        && n.agent_path
                            .as_deref()
                            .is_some_and(|p| p.contains("skin-preview-input"))),
                    "the sample editor never takes focus"
                );
            }
            let sent = agent.document_mut().context_mut().take_program_messages();
            assert!(
                !sent.iter().any(|m| m.is::<Msg>()),
                "the sample editor sends nothing"
            );
            assert!(Arc::ptr_eq(&typed, &ui.draft.get_untracked()));
            assert!(Arc::ptr_eq(&skin, &ui.skin.get_untracked()));
            assert!(Arc::ptr_eq(&settings, &ui.settings.get_untracked()));
            assert_eq!(editor, ui.editor_value.get_untracked());
            agent
                .screenshot_png(
                    captures
                        .join(format!("{}-preview-input-{width}.png", locale.tag()).to_lowercase()),
                )
                .unwrap();
        }
    }
}

/// Without a project the window is the launcher: NanaType's packages, each
/// to copy (and user packages to edit), in every language and at narrow widths.
#[test]
fn launcher_lists_installed_packages_to_edit_or_copy() {
    let captures = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join("target/package-editor");
    std::fs::create_dir_all(&captures).unwrap();
    let rows = Arc::new(nanatype_frame::packages::package_rows(&SkinRegistry::new(
        None,
        env!("CARGO_PKG_VERSION"),
    )));
    assert!(!rows.is_empty());
    for locale in Locale::ALL {
        for width in [900, 440] {
            let ui = Ui::new(
                Arc::new(Settings::default()),
                Arc::new(Default::default()),
                locale,
            );
            ui.packages.set(rows.clone());
            ui.data_dir.set("/data/dev.sena-nana.nanatype".into());
            let mut agent = common::session().themed(false).mount(width, 900, move || {
                ui::window::editor_window(ui, nana_ui::runtime::view::entity_ref())
            });
            agent.flush().unwrap();
            agent
                .screenshot_png(
                    captures.join(format!("{}-launcher-{width}.png", locale.tag()).to_lowercase()),
                )
                .unwrap();
            let nodes = agent.accessibility_dump();
            for row in rows.iter() {
                let id = &row.manifest.meta.id;
                assert!(
                    keyed(&nodes, &format!("editor-package:{id}")).is_some(),
                    "{locale:?} {id} is listed"
                );
                assert!(keyed(&nodes, &format!("package-copy:{id}")).is_some());
                // Built-ins can be copied, never edited in place.
                assert!(keyed(&nodes, &format!("package-edit:{id}")).is_none());
            }
            assert!(keyed(&nodes, "package-editor-new").is_some());
            for node in nodes.iter().filter(|n| n.role == "button") {
                assert!(
                    node.bounds.x + node.bounds.width <= width as f32 + 0.5,
                    "{locale:?} launcher button off-screen at {width}px: {:?}",
                    node.label
                );
            }
        }
    }
}
