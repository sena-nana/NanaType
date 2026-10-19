use super::*;
use crate::{DirFiles, PackageProject, SkinRegistry};
use std::fs;

const STROKES: &[u8] = include_bytes!("../../builtin/love_letter/strokes.nthz");

fn spec(path: &str, format: DataFormat) -> DataSpec {
    DataSpec {
        path: path.into(),
        format,
        format_version: 1,
        source: "test".into(),
        license: "CC0-1.0".into(),
        constraints: None,
    }
}

fn json(text: &str, constraints: JsonConstraints) -> Result<Arc<LoadedData>, String> {
    let mut spec = spec("d", DataFormat::Json);
    spec.constraints = Some(constraints);
    parse_data(&spec, text.as_bytes(), true)
}

fn write_package(root: &std::path::Path, id: &str, extra: &str) {
    let dir = root.join(id);
    fs::create_dir_all(&dir).unwrap();
    fs::write(
        dir.join(PACKAGE_MANIFEST),
        format!(
            "format=1\nschema=3\nid='{id}'\nversion='1.0.0'\ncapabilities=['resources']\n{extra}"
        ),
    )
    .unwrap();
}

#[test]
fn declared_format_wins_over_missing_or_wrong_extension() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("strokes"), STROKES).unwrap();
    fs::write(dir.path().join("strokes.json"), STROKES).unwrap();
    fs::write(dir.path().join("words.txt"), r#"{"a": [1, 2]}"#).unwrap();
    fs::write(dir.path().join("notes.json"), "not json\nsecond line\n").unwrap();
    let files = DirFiles::new(dir.path());
    for path in ["strokes", "strokes.json"] {
        let data = load_data(&files, &spec(path, DataFormat::Nthz), path, true).unwrap();
        assert!(matches!(&data.value, DataValue::Strokes(s) if s.len() == 6763));
        // The same bytes declared as JSON are refused, whatever the name says.
        assert!(load_data(&files, &spec(path, DataFormat::Json), path, true).is_err());
    }
    let words = load_data(
        &files,
        &spec("words.txt", DataFormat::Json),
        "words.txt",
        true,
    )
    .unwrap();
    assert!(matches!(&words.value, DataValue::Json(v) if v["a"][1] == 2));
    let notes = load_data(
        &files,
        &spec("notes.json", DataFormat::Text),
        "notes.json",
        true,
    )
    .unwrap();
    assert_eq!(
        notes
            .query(&DataQuery::Lines { start: 0, count: 5 })
            .unwrap(),
        serde_json::json!(["not json", "second line"])
    );
}

#[test]
fn unknown_formats_and_versions_are_rejected() {
    let base = "path='d'\nsource='s'\nlicense='l'\n";
    for bad in [
        "format='yaml'\nformat_version=1\n",
        "format='JSON'\nformat_version=1\n",
        "format='json'\nformat_version=1\nunknown=1\n",
    ] {
        assert!(
            toml::from_str::<DataSpec>(&format!("{base}{bad}")).is_err(),
            "{bad}"
        );
    }
    for (format, version) in [
        (DataFormat::Json, 2),
        (DataFormat::Nthz, 2),
        (DataFormat::Text, 0),
    ] {
        let mut spec = spec("d", format);
        spec.format_version = version;
        assert!(spec.validate("d").is_err());
        assert!(parse_data(&spec, b"{}", true).is_err());
    }
    let mut text = spec("d", DataFormat::Text);
    text.constraints = Some(JsonConstraints::default());
    assert!(text.validate("d").is_err(), "constraints are JSON-only");
    for (id, path) in [
        ("Bad", "d"),
        ("d", "../d"),
        ("d", "C:/d"),
        ("d", "pkg:other/d"),
        ("d/x", "d"),
    ] {
        assert!(
            spec(path, DataFormat::Json).validate(id).is_err(),
            "{id} {path}"
        );
    }
}

#[test]
fn json_constraints_cover_type_required_items_depth_strings_and_numbers() {
    let ok = JsonConstraints {
        root: Some(JsonType::Object),
        required: vec!["items".into()],
        max_depth: Some(4),
        max_items: Some(10),
        max_string: Some(8),
        number_range: Some([0., 10.]),
    };
    assert!(json(r#"{"items": [1, 2, "short"]}"#, ok.clone()).is_ok());
    for (text, why) in [
        ("[1]", "root type"),
        (r#"{"other": 1}"#, "required"),
        (r#"{"items": [[[[1]]]]}"#, "depth"),
        (r#"{"items": [1,1,1,1,1,1,1,1,1,1,1]}"#, "items"),
        (r#"{"items": "far too long"}"#, "string"),
        (r#"{"items": 1, "very long key": 1}"#, "key"),
        (r#"{"items": 11}"#, "range"),
        (r#"{"items": -1}"#, "range"),
        (r#"{"items": 1"#, "syntax"),
    ] {
        assert!(json(text, ok.clone()).is_err(), "{why}");
    }
}

#[test]
fn deep_json_nesting_fails_without_exhausting_the_stack() {
    let deep = format!("{}{}", "[".repeat(10_000), "]".repeat(10_000));
    assert!(json(&deep, JsonConstraints::default()).is_err());
    let within = format!("{}{}", "[".repeat(40), "]".repeat(40));
    assert!(
        json(&within, JsonConstraints::default()).is_err(),
        "default depth is 32"
    );
    let limit = JsonConstraints {
        max_depth: Some(64),
        ..Default::default()
    };
    let data = json(&within, limit).unwrap();
    assert!(
        data.query(&DataQuery::Whole {}).is_err(),
        "results deeper than the script boundary allows are refused"
    );
}

#[test]
fn malformed_nthz_offsets_and_inflation_bombs_are_rejected() {
    let nthz = spec("s", DataFormat::Nthz);
    assert!(parse_data(&nthz, &STROKES[..100], true).is_err());
    let metadata_len = u32::from_le_bytes(STROKES[8..12].try_into().unwrap()) as usize;
    let count = u32::from_le_bytes(STROKES[12..16].try_into().unwrap()) as usize;
    let table = 24 + metadata_len + count * 12;
    let mut offset = STROKES.to_vec();
    offset[table..table + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(
        parse_data(&nthz, &offset, true).is_err(),
        "block offset overflow"
    );
    // A block declaring a tiny raw length inflates past it: a decompression bomb.
    let mut bomb = STROKES.to_vec();
    bomb[table + 8..table + 12].copy_from_slice(&1u32.to_le_bytes());
    assert!(parse_data(&nthz, &bomb, true).is_err());
}

#[test]
fn raw_budgets_are_checked_before_reading() {
    #[derive(Debug)]
    struct Oversized;
    impl SkinFiles for Oversized {
        fn read(&self, _: &str) -> Result<Vec<u8>, FileError> {
            panic!("oversized data must never be read");
        }
        fn file_len(&self, _: &str) -> Result<u64, FileError> {
            Ok(DataFormat::Json.max_raw() + 1)
        }
        fn list(&self) -> Vec<String> {
            vec!["d".into()]
        }
    }
    assert!(load_data(&Oversized, &spec("d", DataFormat::Json), "d", true).is_err());
    assert!(parse_data(&spec("d", DataFormat::Text), &vec![b'a'; 5 << 20], true).is_err());
}

#[test]
fn queries_are_bounded_and_typed_per_format() {
    let big: Vec<_> = (0..20_000).map(|i| format!("entry-{i}")).collect();
    let data = json(
        &serde_json::to_string(&big).unwrap(),
        JsonConstraints::default(),
    )
    .unwrap();
    let error = data.query(&DataQuery::Whole {}).unwrap_err();
    assert!(error.contains("exceeds"), "{error}");
    let slice = data
        .query(&DataQuery::Slice {
            pointer: String::new(),
            start: 3,
            count: 2,
        })
        .unwrap();
    assert_eq!(slice, serde_json::json!(["entry-3", "entry-4"]));
    assert!(
        data.query(&DataQuery::Slice {
            pointer: String::new(),
            start: 0,
            count: MAX_QUERY_ROWS + 1,
        })
        .is_err()
    );
    assert_eq!(
        data.query(&DataQuery::Pointer {
            pointer: "/7".into()
        })
        .unwrap(),
        "entry-7"
    );
    assert!(
        data.query(&DataQuery::Pointer {
            pointer: "/x".into()
        })
        .is_err()
    );
    assert_eq!(data.query(&DataQuery::Info {}).unwrap()["items"], 20_000);
    assert!(
        data.query(&DataQuery::Glyph {
            character: "心".into()
        })
        .is_err(),
        "queries are typed by the declared format"
    );
    let strokes = parse_data(&spec("s", DataFormat::Nthz), STROKES, false).unwrap();
    let glyph = strokes
        .query(&DataQuery::Glyph {
            character: "心".into(),
        })
        .unwrap();
    assert_eq!(glyph["strokes"].as_array().unwrap().len(), 4);
    assert_eq!(glyph["strokes"][0][0][0], "M");
    assert!(
        strokes
            .query(&DataQuery::Glyph {
                character: "心心".into()
            })
            .is_err()
    );
    assert!(
        strokes.query(&DataQuery::Whole {}).is_err(),
        "binary data is never returned whole"
    );
}

#[test]
fn parsed_values_are_shared_by_content_hash_and_format_version() {
    let a = parse_data(&spec("a", DataFormat::Text), b"same", true).unwrap();
    let b = parse_data(&spec("other/b", DataFormat::Text), b"same", true).unwrap();
    assert!(Arc::ptr_eq(&a, &b));
    let c = parse_data(&spec("a", DataFormat::Json), b"\"same\"", true).unwrap();
    assert!(!Arc::ptr_eq(&a, &c));
    assert!(
        json(
            "\"same\"",
            JsonConstraints {
                max_string: Some(3),
                ..Default::default()
            }
        )
        .is_err(),
        "cached bytes must still satisfy this declaration's constraints"
    );
}

#[test]
fn same_package_and_dependency_data_resolve_but_nothing_else_does() {
    let dir = tempfile::tempdir().unwrap();
    write_package(
        dir.path(),
        "example.words",
        "[data.common]\npath='common.json'\nformat='json'\nformat_version=1\nsource='s'\nlicense='l'\n",
    );
    fs::write(dir.path().join("example.words/common.json"), "[\"hello\"]").unwrap();
    fs::write(
        dir.path().join("example.words/private.json"),
        "[\"secret\"]",
    )
    .unwrap();
    write_package(
        dir.path(),
        "example.app",
        "[dependencies]\n'example.words'='1.0.0'\n[data.local]\npath='local.txt'\nformat='text'\nformat_version=1\nsource='s'\nlicense='l'\n",
    );
    fs::write(dir.path().join("example.app/local.txt"), "own").unwrap();
    let registry = SkinRegistry::new(Some(dir.path().into()), "1.0.0");
    let files = registry.package_files("example.app").unwrap();
    let (local, path) = resolve_data(&*files, "local").unwrap();
    assert_eq!(
        (local.format, path.as_str()),
        (DataFormat::Text, "local.txt")
    );
    let (_, path) = resolve_data(&*files, "pkg:example.words/common").unwrap();
    assert_eq!(path, "pkg:example.words/common.json");
    for reference in [
        "unknown",
        "pkg:example.words/private",
        "pkg:example.other/common",
        "pkg:example.words/../common",
        "../local",
        "C:/local",
        "https://example.com/data",
    ] {
        assert!(resolve_data(&*files, reference).is_err(), "{reference}");
    }
    // A declaration pointing outside the package fails the whole package.
    write_package(
        dir.path(),
        "example.escape",
        "[data.x]\npath='../example.words/private.json'\nformat='json'\nformat_version=1\nsource='s'\nlicense='l'\n",
    );
    fs::write(dir.path().join("example.escape/note.txt"), "x").unwrap();
    assert!(registry.package_files("example.escape").is_err());
    write_package(
        dir.path(),
        "example.missing",
        "[data.x]\npath='absent.json'\nformat='json'\nformat_version=1\nsource='s'\nlicense='l'\n",
    );
    fs::write(dir.path().join("example.missing/note.txt"), "x").unwrap();
    assert!(registry.package_files("example.missing").is_err());
}

#[test]
fn plugin_access_is_limited_to_declared_reads_of_the_reviewed_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    write_package(
        dir.path(),
        "example.words",
        "[data.common]\npath='common.json'\nformat='json'\nformat_version=1\nsource='Word list'\nlicense='CC0-1.0'\n",
    );
    fs::write(
        dir.path().join("example.words/common.json"),
        r#"{"greeting": "hello"}"#,
    )
    .unwrap();
    let app = dir.path().join("example.app");
    fs::create_dir_all(&app).unwrap();
    fs::write(
        app.join(PACKAGE_MANIFEST),
        "format=1\nschema=3\nid='example.app'\nversion='1.0.0'\ncapabilities=['lua','resources']\n\
         [dependencies]\n'example.words'='1.0.0'\n\
         [lua]\nentry='main.lua'\nreads=['pkg:example.words/common']\n\
         [data.hidden]\npath='hidden.txt'\nformat='text'\nformat_version=1\nsource='s'\nlicense='l'\n",
    )
    .unwrap();
    fs::write(app.join("main.lua"), "return {}").unwrap();
    fs::write(app.join("hidden.txt"), "not granted").unwrap();
    let registry = SkinRegistry::new(Some(dir.path().into()), "1.0.0");
    let snapshot =
        PackageProject::capture(&*registry.package_files("example.app").unwrap()).unwrap();
    let manifest = snapshot.manifest().unwrap();
    let access = DataAccess::for_plugin(&snapshot, &manifest).unwrap();
    let disclosed: Vec<_> = access.disclosures().collect();
    assert_eq!(
        disclosed.len(),
        1,
        "undeclared package data is not disclosed or readable"
    );
    assert_eq!(
        (
            disclosed[0].owner.as_str(),
            disclosed[0].source.as_str(),
            disclosed[0].protected
        ),
        ("example.words", "Word list", false)
    );
    let query = serde_json::json!({"kind": "pointer", "pointer": "/greeting"});
    assert_eq!(
        access.answer("pkg:example.words/common", &query).unwrap(),
        "hello"
    );
    // Disk changes after review never reach the approved snapshot.
    fs::write(
        dir.path().join("example.words/common.json"),
        r#"{"greeting": "changed"}"#,
    )
    .unwrap();
    assert_eq!(
        access.answer("pkg:example.words/common", &query).unwrap(),
        "hello"
    );
    for (reference, query) in [
        ("hidden", serde_json::json!({"kind": "whole"})),
        ("common", serde_json::json!({"kind": "whole"})),
        (
            "pkg:example.words/common",
            serde_json::json!({"kind": "path", "path": "/etc/passwd"}),
        ),
        (
            "pkg:example.words/common",
            serde_json::json!({"kind": "whole", "url": "https://x"}),
        ),
        ("/etc/passwd", serde_json::json!({"kind": "whole"})),
    ] {
        assert!(
            access.answer(reference, &query).is_err(),
            "{reference} {query}"
        );
    }
}

#[test]
fn love_letter_declares_its_stroke_data_with_source_and_license_and_no_script() {
    let registry = SkinRegistry::new(None, "1.0.0");
    let files = registry.package_files("nanatype.love_letter").unwrap();
    let manifest = PackageManifest::read(&*files).unwrap();
    assert!(manifest.lua.is_none());
    assert!(
        !manifest
            .capabilities
            .contains(&crate::PackageCapability::Lua)
    );
    let (spec, path) = resolve_data(&*files, "strokes").unwrap();
    assert_eq!((spec.format, spec.format_version), (DataFormat::Nthz, 1));
    assert!(spec.source.contains("Make Me a Hanzi") && spec.license.contains("Arphic"));
    let data = load_data(&*files, &spec, &path, true).unwrap();
    assert!(matches!(&data.value, DataValue::Strokes(s) if s.len() == 6763));
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("letter.ntpkg");
    registry.export("nanatype.love_letter", &archive).unwrap();
    let exported = registry.inspect_package_archive(&archive).unwrap();
    for file in ["strokes.nthz", "ARPHICPL.TXT", "SOURCES.txt"] {
        assert!(exported.exists(file), "{file}");
    }
    assert!(resolve_data(&*exported, "strokes").is_ok());
}
