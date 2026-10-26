use nanatype_editor::{
    recovery,
    session::{self, Session},
};
use nanatype_theme::{
    INTEGRITY_FILE, MANIFEST, PACKAGE_MANIFEST, SealIdentity, SkinRegistry,
    editor::{self, ProjectKind},
};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

fn skin() -> Session {
    session::create(ProjectKind::Skin, "editor.reliability").unwrap()
}

#[test]
fn dependencies_never_drop_malformed_or_duplicate_rows() {
    for text in [
        "missing.version",
        "a.b@",
        "a.b@1.0.0\na.b@2.0.0",
        "a.b@latest",
        "a.b@1.0.0\ninvalid",
    ] {
        assert!(editor::parse_dependencies(text).is_err(), "{text}");
    }
    let deps = editor::parse_dependencies(" a.b @ 1.0.0 \n\nc.d@2.0.0\n").unwrap();
    assert_eq!(deps.len(), 2);
    assert_eq!(deps["a.b"], "1.0.0");
}

#[test]
fn history_restores_content_and_seal_policy_together() {
    let mut s = skin();
    let original = s.project.files.clone();
    s.edit(MANIFEST, "[chrome]\naccent='#123456'\n").unwrap();
    let edited = s.project.files.clone();
    s.sign(&SealIdentity::generate().unwrap(), &Default::default())
        .unwrap();
    assert!(s.project.files.contains_key(INTEGRITY_FILE));
    s.undo();
    assert_eq!(s.project.files, edited);
    s.undo();
    assert_eq!(s.project.files, original);
    s.redo();
    assert_eq!(s.project.files, edited);
    s.redo();
    assert!(
        nanatype_theme::verify_integrity(&s.project)
            .unwrap()
            .is_some()
    );
}

#[test]
fn saving_an_older_snapshot_does_not_clear_newer_edits() {
    let mut s = skin();
    let project = s.project.clone();
    s.edit(MANIFEST, "# still editing\n").unwrap();
    s.saved_snapshot(s.info.clone(), project.clone());
    assert!(s.dirty());
    s.revert();
    assert_eq!(s.project.files, project.files);
    assert!(!s.dirty());
}

#[test]
fn recovery_keeps_manual_save_point_and_invalid_form_text() {
    let dir = tempfile::tempdir().unwrap();
    let mut s = skin();
    s.save(dir.path()).unwrap();
    let saved = s.project.files.clone();
    s.edit(MANIFEST, "# unfinished source\n[broken").unwrap();
    s.buffer(
        "metadata.dependencies".into(),
        Some(serde_json::json!("unfinished")),
    );
    let epoch = AtomicU64::new(s.version);
    recovery::write(dir.path(), s.recovery(), &epoch, s.version).unwrap();
    let snapshot = recovery::load(dir.path(), &s.info.id).unwrap();
    let mut restored = Session::restore(snapshot);
    assert!(restored.dirty());
    assert_eq!(restored.forms["metadata.dependencies"], "unfinished");
    assert!(restored.save(dir.path()).is_err());
    restored.revert();
    assert_eq!(restored.project.files, saved);
    assert!(!restored.dirty());
    assert_eq!(
        editor::load_draft(dir.path(), &s.info.id).unwrap().1.files,
        saved
    );
    epoch.store(0, Ordering::Release);
    recovery::remove(dir.path(), &s.info.id).unwrap();
    recovery::write(dir.path(), s.recovery(), &epoch, s.version).unwrap();
    assert!(
        recovery::list(dir.path()).is_empty(),
        "a cancelled writer cannot resurrect recovery"
    );
}

#[test]
fn invalid_edits_keep_the_previous_valid_preview_with_current_errors() {
    let dir = tempfile::tempdir().unwrap();
    let registry = SkinRegistry::new(Some(dir.path().join("packages")), "1.0.0");
    let mut s = skin();
    let (ticket, project) = s.validation_due().unwrap();
    s.validated(ticket, Arc::new(session::validate(&registry, &project)));
    let previous = s.view(None, false).report.skin.clone().unwrap();
    s.edit(MANIFEST, "[broken").unwrap();
    let (ticket, project) = s.validation_due().unwrap();
    s.validated(ticket, Arc::new(session::validate(&registry, &project)));
    let view = s.view(None, false);
    assert!(view.report.errors() > 0 && view.preview_stale);
    assert!(Arc::ptr_eq(&previous, view.report.skin.as_ref().unwrap()));
}

#[test]
fn declaration_edits_preserve_comments_bindings_and_sparse_inheritance() {
    let mut s = skin();
    let original = s.project.files.clone();
    assert!(
        editor::set_declaration(
            &mut s.project,
            MANIFEST,
            &["chrome".into(), "accent".into()],
            Some("'#123456'\nignored='must not drop'"),
            None,
            false
        )
        .is_err()
    );
    assert_eq!(s.project.files, original);

    s.edit(
        MANIFEST,
        "# authored\n[dialogue.text]\n# keep this\nsize=24\ncolor={param='ink'}\n",
    )
    .unwrap();
    editor::set_declaration(
        &mut s.project,
        MANIFEST,
        &["dialogue".into(), "text".into(), "size".into()],
        Some("32"),
        None,
        false,
    )
    .unwrap();
    let text = std::str::from_utf8(&s.project.files[MANIFEST]).unwrap();
    assert!(
        text.contains("# authored") && text.contains("# keep this") && text.contains("param='ink'")
    );
    assert!(
        editor::set_declaration(
            &mut s.project,
            MANIFEST,
            &["dialogue".into(), "text".into(), "color".into()],
            Some("'#ffffff'"),
            None,
            false
        )
        .is_err()
    );
    let inherited = editor::parse_document(
        "[dialogue]\ndecor=[{text='one',size=[20,20]},{text='two',size=[30,30]}]\n",
    )
    .unwrap();
    editor::set_declaration(
        &mut s.project,
        MANIFEST,
        &["dialogue".into(), "decor".into(), "0".into(), "text".into()],
        Some("'changed'"),
        Some(&inherited),
        false,
    )
    .unwrap();
    let table =
        editor::parse_document(std::str::from_utf8(&s.project.files[MANIFEST]).unwrap()).unwrap();
    assert_eq!(
        editor::value_at(
            &table,
            &["dialogue".into(), "decor".into(), "1".into(), "text".into()]
        )
        .unwrap()
        .as_str(),
        Some("two")
    );
}

#[test]
fn directory_projects_merge_disjoint_changes_and_refuse_overwrites() {
    use nanatype_editor::directory;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let installed = dir.path().join("installed");
    let data = dir.path().join("data");
    let mut base = skin().project.clone();
    base.files.insert("notes.txt".into(), b"old".to_vec());
    for (name, bytes) in &base.files {
        std::fs::write(root.join(name), bytes).unwrap();
    }
    std::fs::create_dir(root.join(".git")).unwrap();
    std::fs::write(root.join(".git/config"), "private").unwrap();
    assert_eq!(
        directory::read(&root, &installed).unwrap().files,
        base.files
    );
    let mut local = base.clone();
    local.files.insert("notes.txt".into(), b"local".to_vec());
    let mut disk = base.clone();
    disk.files.insert(MANIFEST.into(), b"# external\n".to_vec());
    assert!(directory::merge(&base, &mut local, &disk).is_empty());
    assert_eq!(local.files[MANIFEST], disk.files[MANIFEST]);
    disk.files.insert("notes.txt".into(), b"disk".to_vec());
    assert!(directory::merge(&base, &mut local, &disk).contains("notes.txt"));
    directory::save(&root, &installed, &data, &base, &local).unwrap();
    assert_eq!(std::fs::read(root.join("notes.txt")).unwrap(), b"local");
    std::fs::write(root.join("notes.txt"), "changed again").unwrap();
    assert!(directory::save(&root, &installed, &data, &local, &base).is_err());
    assert_eq!(
        std::fs::read(root.join("notes.txt")).unwrap(),
        b"changed again"
    );
    assert!(directory::read(&root, &root).is_err());
}

#[test]
fn lua_compile_only_never_runs_user_code() {
    let exe = std::path::Path::new(env!("CARGO_BIN_EXE_nanatype-editor"));
    nanatype_plugins::check_source_in_worker(exe, "while true do end".into()).unwrap();
    nanatype_plugins::check_source_in_worker(exe, "error('must not run')".into()).unwrap();
    let error = nanatype_plugins::check_source_in_worker(exe, "local x = )".into()).unwrap_err();
    assert!(error.contains("plugin") && error.contains("1"), "{error}");
}

#[test]
fn renames_update_declared_references_and_disclose_program_references() {
    let mut s = skin();
    s.import("picture.txt", b"content".to_vec()).unwrap();
    s.edit(
        MANIFEST,
        "[dialogue.portrait]\nimage='assets/picture.txt'\n",
    )
    .unwrap();
    s.project
        .files
        .insert("main.lua".into(), b"-- assets/picture.txt".to_vec());
    assert!(
        editor::references(&s.project)
            .iter()
            .any(|r| !r.automatic && r.target == "assets/picture.txt")
    );
    editor::rename_asset(&mut s.project, "assets/picture.txt", "pictures/new.txt").unwrap();
    assert!(
        std::str::from_utf8(&s.project.files[MANIFEST])
            .unwrap()
            .contains("pictures/new.txt")
    );
    assert!(
        std::str::from_utf8(&s.project.files["main.lua"])
            .unwrap()
            .contains("assets/picture.txt")
    );
}

#[test]
fn conflict_copies_never_overwrite_source_or_installed_files() {
    use nanatype_editor::directory::check_copy_destination;
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.txt");
    std::fs::write(&source, "original").unwrap();
    let installed = dir.path().join("packages");
    std::fs::create_dir(&installed).unwrap();
    assert!(check_copy_destination(&source, Some(&source), &installed).is_err());
    assert!(check_copy_destination(&installed.join("new.txt"), Some(&source), &installed).is_err());
    assert!(
        check_copy_destination(&dir.path().join("copy.txt"), Some(&source), &installed).is_ok()
    );
}

#[test]
fn unicode_search_ranges_are_original_utf8_boundaries() {
    let source = "İ before αΒ after İ";
    for needle in ["i", "İ", "Αβ"] {
        for (start, len) in editor::find_matches(source, needle) {
            assert!(source.is_char_boundary(start) && source.is_char_boundary(start + len));
            assert!(!source[start..start + len].is_empty());
        }
    }
    assert_eq!(editor::find_matches(source, "Αβ").len(), 1);
}

#[test]
fn directory_reload_and_recovery_preserve_the_manual_save_point() {
    use nanatype_editor::directory;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let original = skin().project.clone();
    for (p, b) in &original.files {
        std::fs::write(root.join(p), b).unwrap();
    }
    let installed = dir.path().join("installed");
    let data = dir.path().join("data");
    let drafts = data.join("drafts");
    let mut s = session::open_directory(&root, &installed, &data).unwrap();
    let external = b"# external edit\n[chrome]\naccent='#112233'\n";
    std::fs::write(root.join(MANIFEST), external).unwrap();
    s.merge_disk(directory::read(&root, &installed).unwrap());
    assert!(
        !s.dirty(),
        "automatic reload acknowledges disk without making a local edit"
    );
    s.edit(MANIFEST, "# local edit\n").unwrap();
    let epoch = AtomicU64::new(s.version);
    recovery::write(&drafts, s.recovery(), &epoch, s.version).unwrap();
    let mut restored = Session::restore(recovery::load(&drafts, &s.info.id).unwrap());
    assert_eq!(restored.disk_baseline().files[MANIFEST], external);
    restored.revert();
    assert_eq!(restored.project.files, original.files);
    assert!(
        restored.dirty(),
        "restoring the earlier manual point is an explicit local change against the latest disk baseline"
    );
    directory::save(
        &root,
        &installed,
        &data,
        restored.disk_baseline(),
        &restored.project,
    )
    .unwrap();
    assert_eq!(
        std::fs::read(root.join(MANIFEST)).unwrap(),
        original.files[MANIFEST]
    );
}

#[test]
fn directory_saves_allow_declared_dependency_selection_and_reject_payload_edits() {
    use nanatype_editor::directory;
    let dir = tempfile::tempdir().unwrap();
    let installed = dir.path().join("installed");
    let mut registry = SkinRegistry::new(Some(installed.clone()), "1.0.0");
    let dependency = session::create(ProjectKind::Resources, "editor.dep")
        .unwrap()
        .project
        .clone();
    let archive = dir.path().join("dep.ntpkg");
    session::export(&registry, &dependency, &archive).unwrap();
    registry.install(&archive).unwrap();
    let root = dir.path().join("project");
    std::fs::create_dir(&root).unwrap();
    let base = skin().project.clone();
    for (p, b) in &base.files {
        std::fs::write(root.join(p), b).unwrap();
    }
    let mut next = base.clone();
    let mut metadata = editor::Metadata::of(&next.manifest().unwrap());
    metadata
        .dependencies
        .insert("editor.dep".into(), "1.0.0".into());
    editor::apply_metadata(&mut next, &metadata).unwrap();
    editor::bundle_dependencies(&mut next, &registry).unwrap();
    let data = dir.path().join("data");
    directory::save(&root, &installed, &data, &base, &next).unwrap();
    let mut tampered = next.clone();
    tampered.files.insert(
        "dependencies/editor.dep/data/notes.txt".into(),
        b"unauthorized".to_vec(),
    );
    assert!(directory::save(&root, &installed, &data, &next, &tampered).is_err());
    assert_eq!(
        directory::read(&root, &installed).unwrap().files,
        next.files
    );
}

#[test]
fn large_source_diagnostics_use_background_validation_and_keep_the_valid_preview() {
    let dir = tempfile::tempdir().unwrap();
    let registry = SkinRegistry::new(Some(dir.path().join("packages")), "1.0.0");
    let mut s = skin();
    let source = (0..8000)
        .map(|i| format!("key_{i}=1\n"))
        .collect::<String>();
    assert!(source.len() > 64 * 1024);
    let path = s.import("large.toml", source.as_bytes().to_vec()).unwrap();
    let (ticket, p) = s.validation_due().unwrap();
    s.validated(ticket, Arc::new(session::validate(&registry, &p)));
    let previous = s.view(None, false).report.skin.clone().unwrap();
    s.edit(&path, &format!("{source}broken = [")).unwrap();
    let view = s.view(None, false);
    assert!(
        view.syntax.is_empty() && view.validating,
        "large syntax work stays off the input path"
    );
    let (ticket, p) = s.validation_due().unwrap();
    s.validated(ticket, Arc::new(session::validate(&registry, &p)));
    let view = s.view(None, false);
    assert!(
        view.report
            .diagnostics
            .iter()
            .any(|d| d.path == path && d.level == editor::Level::Error)
    );
    assert!(view.preview_stale && Arc::ptr_eq(&previous, view.report.skin.as_ref().unwrap()));
}

#[test]
fn source_cannot_change_package_identity_and_recovery_keeps_that_rule() {
    let mut s = skin();
    let original = s.project.files.clone();
    let text = std::str::from_utf8(&s.project.files[PACKAGE_MANIFEST])
        .unwrap()
        .replace("editor.reliability", "editor.renamed");
    assert!(s.edit(PACKAGE_MANIFEST, &text).is_err());
    assert_eq!(s.project.files, original);
    s.edit(PACKAGE_MANIFEST, "# temporarily invalid source\n[broken")
        .unwrap();
    let mut restored = Session::restore(s.recovery());
    assert!(restored.edit(PACKAGE_MANIFEST, &text).is_err());
    restored.revert();
    assert_eq!(
        restored.project.files, s.project.files,
        "new projects have no manual point to revert to"
    );
    let mut s = skin();
    s.derive("editor.renamed").unwrap();
    assert_eq!(s.project.manifest().unwrap().meta.id, "editor.renamed");
    assert!(!s.project.manifest().unwrap().derived_from.is_empty());
}
