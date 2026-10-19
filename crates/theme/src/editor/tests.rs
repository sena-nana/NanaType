use super::*;
use crate::SealIdentity;

fn registry(dir: &Path) -> SkinRegistry {
    SkinRegistry::new(Some(dir.join("installed")), "1.0.0")
}

#[test]
fn every_project_kind_starts_valid_and_round_trips_through_export_and_import() {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = registry(dir.path());
    for kind in ProjectKind::ALL {
        let id = format!("editor.{}", kind.key());
        let mut project = template(kind, &id).unwrap();
        let manifest = project.manifest().unwrap();
        assert_eq!(ProjectKind::of(&manifest), kind);
        assert_eq!(manifest.capabilities, kind.capabilities());
        let (_, skin) = registry.inspect_project(&project).unwrap();
        assert_eq!(
            skin.is_some(),
            kind.capabilities().contains(&PackageCapability::Skin)
        );
        // Edit, export, install and open the installed copy again.
        let mut metadata = Metadata::of(&manifest);
        metadata.author = "作者".into();
        metadata.created_at = "2026-10-19".into();
        metadata.licensed_to = "所有观众".into();
        metadata.license = "MIT".into();
        apply_metadata(&mut project, &metadata).unwrap();
        let archive = dir.path().join(format!("{id}.ntpkg"));
        project.export(&archive).unwrap();
        registry.install(&archive).unwrap();
        let reopened = PackageProject::capture(&*registry.package_files(&id).unwrap()).unwrap();
        assert_eq!(reopened.files, project.files, "{id}");
        assert_eq!(Metadata::of(&reopened.manifest().unwrap()), metadata);
        assert_eq!(read_archive(&archive).unwrap().files, project.files);
    }
}

#[test]
fn metadata_edits_keep_comments_and_add_starter_content() {
    let mut project = template(ProjectKind::Resources, "editor.meta").unwrap();
    let mut text = String::from_utf8(project.files[PACKAGE_MANIFEST].clone()).unwrap();
    text.insert_str(0, "# keep me\n");
    project.set(PACKAGE_MANIFEST, text.into_bytes()).unwrap();
    let mut metadata = Metadata::of(&project.manifest().unwrap());
    metadata.capabilities = ProjectKind::Mixed.capabilities();
    apply_metadata(&mut project, &metadata).unwrap();
    let text = String::from_utf8(project.files[PACKAGE_MANIFEST].clone()).unwrap();
    assert!(text.starts_with("# keep me\n"), "{text}");
    assert!(project.files.contains_key("main.lua"));
    assert!(project.files.contains_key(MANIFEST));
    assert_eq!(project.manifest().unwrap().entry(), Some("main.lua"));
    // A bad value leaves the project unchanged.
    let before = project.files.clone();
    metadata.created_at = "2026-02-30".into();
    assert!(apply_metadata(&mut project, &metadata).is_err());
    assert_eq!(project.files, before);
    // Turning Lua off removes its grant; the source file stays for review.
    metadata.created_at.clear();
    metadata.capabilities = vec![PackageCapability::Skin, PackageCapability::Resources];
    apply_metadata(&mut project, &metadata).unwrap();
    assert!(project.manifest().unwrap().lua.is_none());
}

#[test]
fn imports_check_names_sizes_and_that_content_matches_the_extension() {
    let mut project = template(ProjectKind::Skin, "editor.import").unwrap();
    let png = {
        let mut out = Vec::new();
        image::RgbaImage::new(2, 3)
            .write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    };
    let path = import_asset(&mut project, "images", "../../evil/a.png", png.clone()).unwrap();
    assert_eq!(path, "images/a.png", "only the file name is kept");
    let info = inventory(&project)
        .into_iter()
        .find(|f| f.path == path)
        .unwrap();
    assert_eq!(
        (info.kind, info.format.as_str()),
        (AssetKind::Image, "PNG 2×3")
    );
    assert!(!info.editable, "binary files are shown, not edited as text");
    assert!(
        import_asset(&mut project, "images", "a.png", png.clone()).is_err(),
        "no overwrite"
    );
    assert!(import_asset(&mut project, "", "fake.png", b"not an image".to_vec()).is_err());
    assert!(import_asset(&mut project, "", "image.lua", png.clone()).is_err());
    assert!(import_asset(&mut project, "", "tool.exe", b"MZ".to_vec()).is_err());
    assert!(import_asset(&mut project, "", "package.toml", b"x=1".to_vec()).is_err());
    assert!(import_asset(&mut project, "dependencies/x", "a.txt", b"x".to_vec()).is_err());
    assert!(import_asset(&mut project, "C:", "a.txt", b"x".to_vec()).is_err());
    let huge = vec![b'a'; crate::MAX_FILE_BYTES as usize + 1];
    assert!(import_asset(&mut project, "", "big.txt", huge).is_err());
    import_asset(&mut project, "data", "words.json", b"{\"a\":1}".to_vec()).unwrap();
}

#[test]
fn signed_projects_keep_protected_files_read_only_and_report_truthfully() {
    let mut project = template(ProjectKind::Mixed, "editor.signed").unwrap();
    let key = SealIdentity::generate().unwrap();
    project
        .seal(&key, &BTreeSet::from(["main.lua".into()]))
        .unwrap();
    let files = inventory(&project);
    let row = |path: &str| files.iter().find(|f| f.path == path).unwrap().clone();
    assert!(row(PACKAGE_MANIFEST).protected && !row(PACKAGE_MANIFEST).editable);
    assert!(row("main.lua").protected && !row("main.lua").editable);
    assert!(row(MANIFEST).editable && !row(MANIFEST).protected);
    assert!(project.set("main.lua", b"return {}".to_vec()).is_err());
    let metadata = Metadata::of(&project.manifest().unwrap());
    assert!(apply_metadata(&mut project, &metadata).is_err());
    assert!(
        remove_file(&mut project, MANIFEST).is_err(),
        "inventory is signed"
    );
    // Editable files change; the seal still verifies, honestly.
    project
        .set(MANIFEST, b"[chrome]\naccent = \"#000000\"\n".to_vec())
        .unwrap();
    assert!(verify_integrity(&project).unwrap().is_some());
    // A tampered snapshot makes every local file read-only.
    let mut tampered = project.clone();
    tampered.files.insert("main.lua".into(), b"x".to_vec());
    assert!(inventory(&tampered).iter().all(|f| !f.editable));
    // A derived copy drops the seal, records its source and is editable.
    let mut copy = derive_copy(&project, "editor.signed-copy").unwrap();
    assert!(!copy.files.contains_key(INTEGRITY_FILE));
    let manifest = copy.manifest().unwrap();
    assert_eq!(manifest.derived_from, "editor.signed@1.0.0");
    assert_eq!(manifest.meta.id, "editor.signed-copy");
    copy.set("main.lua", b"return {}".to_vec()).unwrap();
    assert!(derive_copy(&project, "editor.signed").is_err());
    // The owner may unseal and sign again.
    unseal(&mut project);
    project.set("main.lua", b"return {}".to_vec()).unwrap();
    project.seal(&key, &BTreeSet::new()).unwrap();
}

#[test]
fn formatting_keeps_comments_order_and_meaning() {
    let toml = "# head\n  a=1   # one\nb   =  \"x\"  \n\n[t]\n    c=[1,2]\n";
    let formatted = format_toml(toml).unwrap();
    assert_eq!(
        formatted,
        "# head\na = 1 # one\nb = \"x\"\n\n[t]\nc = [1,2]\n"
    );
    assert_eq!(format_toml(&formatted).unwrap(), formatted);
    assert!(format_toml("a = ").is_err());
    let json = r#"{"z":[1,{"y":"a,b"}],"a":{} ,"e":[]}"#;
    let formatted = format_json(json).unwrap();
    assert_eq!(
        formatted,
        "{\n  \"z\": [\n    1,\n    {\n      \"y\": \"a,b\"\n    }\n  ],\n  \"a\": {},\n  \"e\": []\n}\n"
    );
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&formatted).unwrap(),
        serde_json::from_str::<serde_json::Value>(json).unwrap()
    );
    assert!(format_json("{").is_err());
}

#[test]
fn diagnostics_point_at_the_line_and_key() {
    let bad = "a = 1\nb = \n";
    let d = check_source("skin.toml", bad);
    assert_eq!(d.len(), 1);
    assert_eq!((d[0].line, d[0].level), (2, Level::Error));
    let d = check_source("data/x.json", "{\n  \"a\": 1,\n}");
    assert_eq!(d[0].line, 3);
    let manifest = "format = 1\nschema = 3\nid = \"a.b\"\nversion = \"1.0.0\"\ncapabilities = [\"skin\"]\nauthr = \"x\"\n";
    let d = check_source(PACKAGE_MANIFEST, manifest);
    assert_eq!((d[0].line, d[0].column), (6, 1), "{d:?}");
    assert_eq!(line_column("一\n二三", "一\n二".len()), (2, 2));
    // Validation results, warnings included, are placed on their key.
    let mut project = template(ProjectKind::Skin, "editor.diag").unwrap();
    project
        .set(
            MANIFEST,
            b"[chrome]\naccent = \"#123456\"\n\n[dialogue]\nfont_size = -4\n".to_vec(),
        )
        .unwrap();
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(dir.path());
    let (_, skin) = registry.inspect_project(&project).unwrap();
    let located = locate(&project, &skin.unwrap().warnings[0]);
    assert_eq!(
        (located.path.as_str(), located.line),
        (MANIFEST, 5),
        "{located:?}"
    );
    assert_eq!(located.level, Level::Warning);
    project
        .set(
            MANIFEST,
            b"[chrome]\naccent = \"#123456\"\n\n[dialogue.text]\nsize = \"big\"\n".to_vec(),
        )
        .unwrap();
    let errors = registry.inspect_project(&project).unwrap_err();
    let located = locate(&project, &errors[0]);
    assert_eq!(located.path, MANIFEST);
    assert!(located.line >= 4, "{located:?} from {errors:?}");
}

#[test]
fn dependencies_are_bundled_from_the_registry_and_follow_the_manifest() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(dir.path());
    let mut project = template(ProjectKind::Skin, "editor.child").unwrap();
    let mut metadata = Metadata::of(&project.manifest().unwrap());
    let base = registry.package_files(crate::DEFAULT_SKIN).unwrap();
    let version = PackageManifest::read(&*base).unwrap().meta.version;
    metadata
        .dependencies
        .insert(crate::DEFAULT_SKIN.into(), version);
    apply_metadata(&mut project, &metadata).unwrap();
    bundle_dependencies(&mut project, &registry).unwrap();
    let prefix = format!("dependencies/{}/", crate::DEFAULT_SKIN);
    assert!(project.files.keys().any(|p| p.starts_with(&prefix)));
    assert!(
        inventory(&project)
            .iter()
            .filter(|f| f.path.starts_with(&prefix))
            .all(|f| !f.editable && f.dependency.as_deref() == Some(crate::DEFAULT_SKIN))
    );
    registry.inspect_project(&project).unwrap();
    metadata.dependencies.clear();
    apply_metadata(&mut project, &metadata).unwrap();
    bundle_dependencies(&mut project, &registry).unwrap();
    assert!(!project.files.keys().any(|p| p.starts_with("dependencies/")));
}

#[test]
fn drafts_save_list_load_and_discard_without_validation() {
    let dir = tempfile::tempdir().unwrap();
    let mut project = template(ProjectKind::Lua, "editor.draft").unwrap();
    project.set("main.lua", b"unfinished(".to_vec()).unwrap();
    project.set(PACKAGE_MANIFEST, b"broken =".to_vec()).unwrap();
    let info = DraftInfo {
        id: "d1".into(),
        package: "editor.draft".into(),
        origin: DraftOrigin::Copy {
            from: "a@1.0.0".into(),
        },
        saved_at: 7,
    };
    save_draft(dir.path(), &info, &project).unwrap();
    assert_eq!(list_drafts(dir.path()), vec![info.clone()]);
    let (loaded, files) = load_draft(dir.path(), "d1").unwrap();
    assert_eq!((loaded, files.files), (info, project.files));
    assert!(load_draft(dir.path(), "../d1").is_err());
    discard_draft(dir.path(), "d1").unwrap();
    assert!(list_drafts(dir.path()).is_empty());
}

#[test]
fn builtin_packages_open_as_projects_and_copy_into_valid_derived_packages() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(dir.path());
    let project =
        PackageProject::capture(&*registry.package_files("nanatype.love_letter").unwrap()).unwrap();
    let files = inventory(&project);
    let strokes = files.iter().find(|f| f.path == "strokes.nthz").unwrap();
    assert_eq!(strokes.kind, AssetKind::Data(DataFormat::Nthz));
    assert!(strokes.data.as_ref().unwrap().1.license.contains("Arphic"));
    assert!(pipelines(&project).iter().any(|(name, p)| {
        name == "stroke_map"
            && p.as_ref()
                .is_ok_and(|p| p.processor == "text.stroke_time_map" && p.version == 1)
    }));
    let copy = derive_copy(&project, "my.love-letter").unwrap();
    registry.inspect_project(&copy).unwrap();
}

fn spec(path: &str, format: DataFormat) -> DataSpec {
    DataSpec {
        path: path.into(),
        format,
        format_version: 1,
        source: "original work".into(),
        license: "CC0-1.0".into(),
        constraints: None,
    }
}

#[test]
fn data_forms_edit_rename_and_remove_declarations_in_place() {
    let mut project = template(ProjectKind::Mixed, "editor.data").unwrap();
    let text = String::from_utf8(project.files[PACKAGE_MANIFEST].clone()).unwrap();
    let text = text
        .replace("[data.notes]", "# the notes\n[data.notes]")
        .replace("format = \"text\"", "format = \"text\" # never guessed");
    project.set(PACKAGE_MANIFEST, text.into_bytes()).unwrap();
    project
        .set("data/words.json", b"{\"words\": [1, 2]}".to_vec())
        .unwrap();
    // Add a JSON declaration with constraints.
    let mut words = spec("data/words.json", DataFormat::Json);
    words.constraints = Some(crate::JsonConstraints {
        root: Some(crate::JsonType::Object),
        required: vec!["words".into()],
        number_range: Some([0.0, 2.5]),
        ..Default::default()
    });
    apply_data(&mut project, None, Some(("words", &words))).unwrap();
    let manifest = project.manifest().unwrap();
    assert_eq!(manifest.data["words"], words);
    // Rename and edit the text declaration: comments stay, reads follow.
    let mut notes = spec("data/notes.txt", DataFormat::Text);
    notes.license = "MIT".into();
    apply_data(&mut project, Some("notes"), Some(("lines", &notes))).unwrap();
    let text = String::from_utf8(project.files[PACKAGE_MANIFEST].clone()).unwrap();
    assert!(text.contains("# the notes\n[data.lines]"), "{text}");
    assert!(text.contains("format = \"text\" # never guessed"), "{text}");
    let manifest = project.manifest().unwrap();
    assert_eq!(manifest.data["lines"], notes);
    assert!(!manifest.data.contains_key("notes"));
    assert_eq!(manifest.lua.unwrap().reads, vec!["lines".to_owned()]);
    let dir = tempfile::tempdir().unwrap();
    registry(dir.path()).inspect_project(&project).unwrap();
    // Invalid edits leave the project unchanged.
    let before = project.files.clone();
    let rejected = [
        (
            None,
            Some(("words", spec("data/words.json", DataFormat::Json))),
        ),
        (
            None,
            Some(("Bad ID", spec("data/words.json", DataFormat::Json))),
        ),
        (
            None,
            Some(("missing", spec("data/missing.txt", DataFormat::Text))),
        ),
        (
            None,
            Some(("pkg", spec(PACKAGE_MANIFEST, DataFormat::Text))),
        ),
        (
            Some("lines"),
            Some(("lines", spec("data/notes.txt", DataFormat::Text))),
        ),
        (
            None,
            Some((
                "v2",
                DataSpec {
                    format_version: 2,
                    ..notes.clone()
                },
            )),
        ),
        (
            None,
            Some((
                "nosrc",
                DataSpec {
                    source: " ".into(),
                    ..notes.clone()
                },
            )),
        ),
        (
            None,
            Some((
                "textc",
                DataSpec {
                    constraints: words.constraints.clone(),
                    ..notes.clone()
                },
            )),
        ),
        // Still read by the script.
        (Some("lines"), None),
        (Some("absent"), None),
    ];
    for (i, (previous, entry)) in rejected.iter().enumerate() {
        let entry = entry.as_ref().map(|(id, s)| (*id, s));
        if i == 4 {
            // Renaming onto an existing ID is refused.
            assert!(apply_data(&mut project, Some("words"), entry).is_err());
            assert_eq!(project.files, before);
            continue;
        }
        assert!(apply_data(&mut project, *previous, entry).is_err(), "{i}");
        assert_eq!(project.files, before, "{i}");
    }
    // Removing an unread declaration works; the last one drops the table.
    apply_data(&mut project, Some("words"), None).unwrap();
    let manifest = project.manifest().unwrap();
    assert_eq!(manifest.data.keys().collect::<Vec<_>>(), ["lines"]);
    // Signed manifests are read-only.
    let key = SealIdentity::generate().unwrap();
    project.seal(&key, &BTreeSet::new()).unwrap();
    let before = project.files.clone();
    assert!(apply_data(&mut project, None, Some(("more", &notes))).is_err());
    assert_eq!(project.files, before);
}

#[test]
fn pipeline_forms_write_valid_declarations_and_refuse_new_errors() {
    let dir = tempfile::tempdir().unwrap();
    let registry = registry(dir.path());
    let mut project =
        PackageProject::capture(&*registry.package_files("nanatype.love_letter").unwrap()).unwrap();
    project.files.remove(INTEGRITY_FILE);
    let skin = String::from_utf8(project.files[MANIFEST].clone()).unwrap();
    let skin = skin.replace(
        "[pipelines.stroke_map]",
        "# stroke timing\n[pipelines.stroke_map]",
    );
    project.set(MANIFEST, skin.into_bytes()).unwrap();
    let (_, original) = pipelines(&project).remove(0);
    let original = original.unwrap();
    // A second, unused pipeline is allowed (validation warns about it).
    apply_pipeline(&mut project, None, Some(("spare", &original))).unwrap();
    assert_eq!(pipelines(&project).len(), 2);
    registry.inspect_project(&project).unwrap();
    // Editing in place keeps the comment and the material binding.
    apply_pipeline(
        &mut project,
        Some("stroke_map"),
        Some(("stroke_map", &original)),
    )
    .unwrap();
    let text = String::from_utf8(project.files[MANIFEST].clone()).unwrap();
    assert!(
        text.contains("# stroke timing\n[pipelines.stroke_map]"),
        "{text}"
    );
    assert!(
        text.contains("inputs = { strokes = { data = \"strokes\" } }"),
        "{text}"
    );
    // Refused: unknown processor, missing input, undeclared or mistyped data,
    // a pipeline input that is not a texture-compatible port, a cycle, and
    // removing the pipeline a material reads.
    let before = project.files.clone();
    let with = |f: &dyn Fn(&mut PipelineSpec)| {
        let mut p = original.clone();
        f(&mut p);
        p
    };
    let cases = [
        (Some("spare"), Some(("spare", with(&|p| p.version = 9)))),
        (Some("spare"), Some(("spare", with(&|p| p.inputs.clear())))),
        (
            Some("spare"),
            Some((
                "spare",
                with(&|p| {
                    p.inputs.get_mut("strokes").unwrap().data = Some("nothing".into());
                }),
            )),
        ),
        (
            Some("spare"),
            Some((
                "spare",
                with(&|p| {
                    p.inputs.insert(
                        "strokes".into(),
                        crate::PortSource {
                            pipeline: Some("stroke_map".into()),
                            output: Some("time".into()),
                            data: None,
                        },
                    );
                }),
            )),
        ),
        (
            Some("spare"),
            Some((
                "spare",
                with(&|p| {
                    p.params.insert("speed".into(), 1.0);
                }),
            )),
        ),
        (None, Some(("stroke_map", original.clone()))),
        (None, Some(("bad name", original.clone()))),
        (Some("stroke_map"), None),
    ];
    for (i, (previous, entry)) in cases.iter().enumerate() {
        let entry = entry.as_ref().map(|(n, s)| (*n, s));
        assert!(
            apply_pipeline(&mut project, *previous, entry).is_err(),
            "{i}"
        );
        assert_eq!(project.files, before, "{i}");
    }
    // Renaming and removing an unused pipeline.
    apply_pipeline(&mut project, Some("spare"), Some(("extra", &original))).unwrap();
    apply_pipeline(&mut project, Some("extra"), None).unwrap();
    assert_eq!(pipelines(&project).len(), 1);
    registry.inspect_project(&project).unwrap();
    // A malformed declaration is reported, not hidden.
    let skin = String::from_utf8(project.files[MANIFEST].clone()).unwrap()
        + "\n[pipelines.broken]\nprocessor = 3\n";
    project.set(MANIFEST, skin.into_bytes()).unwrap();
    assert!(
        pipelines(&project)
            .iter()
            .any(|(n, p)| n == "broken" && p.is_err())
    );
    // A signed skin is read-only.
    let mut sealed = template(ProjectKind::Skin, "editor.sealed-pipes").unwrap();
    let key = SealIdentity::generate().unwrap();
    sealed
        .seal(&key, &BTreeSet::from([MANIFEST.to_owned()]))
        .unwrap();
    assert!(apply_pipeline(&mut sealed, None, Some(("p", &original))).is_err());
}
