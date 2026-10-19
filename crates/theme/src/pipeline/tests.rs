use super::*;
use crate::{
    BaseResolver, DataValue, DirFiles, SkinOrigin, SkinRegistry, TomlTable, ZipFiles, load,
};

struct Offline;
impl BaseResolver for Offline {
    fn base(&self, _: &str) -> Option<(TomlTable, Arc<dyn SkinFiles>)> {
        None
    }
}

/// Test-only processors: a texture filter and a param-checked generator.
const TEST: &[ProcessorSignature] = &[
    ProcessorSignature {
        name: "test.source",
        version: 1,
        inputs: &[(
            "values",
            PortType::Data {
                format: DataFormat::Json,
                version: 1,
            },
        )],
        outputs: &[("image", PortType::Texture), ("size", PortType::Vec4)],
        params: &[ParamSig {
            name: "gain",
            min: 0.,
            max: 2.,
        }],
        text_layout: false,
    },
    ProcessorSignature {
        name: "test.blur",
        version: 1,
        inputs: &[("image", PortType::Texture)],
        outputs: &[("image", PortType::Texture)],
        params: &[],
        text_layout: false,
    },
    PROCESSORS[0],
];

fn skin(toml: &str) -> Skin {
    let toml::Value::Table(mut table) = toml::Value::try_from(Skin::default()).unwrap() else {
        unreachable!()
    };
    crate::deep_merge(&mut table, &toml::from_str(toml).unwrap());
    toml::Value::Table(table).try_into().unwrap()
}

fn errors(toml: &str) -> Vec<String> {
    validate(&skin(toml), TEST)
        .into_iter()
        .filter(|d| d.severity == Severity::Error)
        .map(|d| format!("{}: {}", d.field, d.message))
        .collect()
}

const SOURCE: &str = r#"
[pipelines.src]
processor = "test.source"
version = 1
inputs = { values = { data = "values" } }
[materials.m]
source = "m.wgsl"
textures = [{ pipeline = "src", output = "image" }]
uniforms = [{ pipeline = "src", output = "size" }]
"#;

#[test]
fn registered_processors_with_typed_inputs_and_outputs_validate() {
    assert_eq!(errors(SOURCE), Vec::<String>::new());
    let chained = format!(
        "{SOURCE}[pipelines.blur]\nprocessor='test.blur'\nversion=1\ninputs={{image={{pipeline='src',output='image'}}}}\n\
         [materials.n]\nsource='n.wgsl'\ntextures=[{{pipeline='blur',output='image'}}]\n"
    );
    assert_eq!(errors(&chained), Vec::<String>::new());
}

#[test]
fn unregistered_processors_and_versions_are_rejected() {
    for (from, to) in [
        ("test.source", "test.unknown"),
        ("version = 1", "version = 2"),
    ] {
        let e = errors(&SOURCE.replacen(from, to, 1));
        assert!(e.iter().any(|e| e.contains("unregistered")), "{e:?}");
    }
}

#[test]
fn unbound_undeclared_and_mistyped_ports_are_rejected() {
    let unbound = SOURCE.replace("inputs = { values = { data = \"values\" } }", "");
    assert!(errors(&unbound).iter().any(|e| e.contains("not bound")));
    let undeclared = SOURCE.replace(
        "values = { data = \"values\" }",
        "values = { data = \"values\" }, extra = { data = \"x\" }",
    );
    assert!(
        errors(&undeclared)
            .iter()
            .any(|e| e.contains("no input `extra`"))
    );
    let both = SOURCE.replace(
        "{ data = \"values\" }",
        "{ data = \"values\", pipeline = \"src\", output = \"image\" }",
    );
    assert!(errors(&both).iter().any(|e| e.contains("either")));
    // A texture where a uniform belongs, and data where a texture belongs.
    let swapped = SOURCE.replace("output = \"size\" }]", "output = \"image\" }]");
    assert!(errors(&swapped).iter().any(|e| e.contains("vec4")));
    let data_into_texture = format!(
        "{SOURCE}[pipelines.blur]\nprocessor='test.blur'\nversion=1\ninputs={{image={{data='values'}}}}\n"
    );
    assert!(
        errors(&data_into_texture)
            .iter()
            .any(|e| e.contains("expects"))
    );
    let missing_output = SOURCE.replace("output = \"image\"", "output = \"nothing\"");
    assert!(
        errors(&missing_output)
            .iter()
            .any(|e| e.contains("no output"))
    );
    let bad_reference = SOURCE.replace("data = \"values\"", "data = \"../values\"");
    assert!(!errors(&bad_reference).is_empty());
}

#[test]
fn params_and_material_slot_budgets_are_enforced() {
    for gain in ["3.0", "-1.0", "nan"] {
        let e = errors(&SOURCE.replace(
            "inputs = { values",
            &format!("params = {{ gain = {gain} }}\ninputs = {{ values"),
        ));
        assert!(e.iter().any(|e| e.contains("params.gain")), "{gain}: {e:?}");
    }
    let undeclared = SOURCE.replace(
        "inputs = { values",
        "params = { other = 1.0 }\ninputs = { values",
    );
    assert!(
        errors(&undeclared)
            .iter()
            .any(|e| e.contains("undeclared parameter"))
    );
    let crowded = SOURCE.replace(
        "source = \"m.wgsl\"",
        "source = \"m.wgsl\"\nparams = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13]",
    );
    assert!(errors(&crowded).iter().any(|e| e.contains("vec4 slots")));
}

#[test]
fn cycles_and_deep_graphs_are_rejected() {
    let cycle = r#"
[pipelines.a]
processor = "test.blur"
version = 1
inputs = { image = { pipeline = "b", output = "image" } }
[pipelines.b]
processor = "test.blur"
version = 1
inputs = { image = { pipeline = "a", output = "image" } }
"#;
    assert!(errors(cycle).iter().any(|e| e.contains("cycle")));
    let mut deep = SOURCE.to_owned();
    let mut previous = "src".to_owned();
    for i in 0..MAX_PIPELINE_DEPTH {
        deep += &format!(
            "[pipelines.b{i}]\nprocessor='test.blur'\nversion=1\ninputs={{image={{pipeline='{previous}',output='image'}}}}\n"
        );
        previous = format!("b{i}");
    }
    assert!(errors(&deep).iter().any(|e| e.contains("deep")));
}

#[test]
fn layout_materials_only_feed_the_dialogue_entrance_and_unused_pipelines_warn() {
    let layout = r#"
[pipelines.map]
processor = "text.stroke_time_map"
version = 1
inputs = { strokes = { data = "strokes" } }
[materials.write]
source = "write.wgsl"
textures = [{ pipeline = "map", output = "time" }]
"#;
    let entrance = format!("{layout}[dialogue.reveal.entrance]\nmaterial='write'\n");
    assert_eq!(errors(&entrance), Vec::<String>::new());
    assert!(skin(&entrance).entrance_needs_text_layout());
    let elsewhere = format!("{layout}[history.text]\nmaterial='write'\n");
    assert!(errors(&elsewhere).iter().any(|e| e.contains("text layout")));
    let unused = format!(
        "{SOURCE}[pipelines.idle]\nprocessor='test.blur'\nversion=1\ninputs={{image={{pipeline='src',output='image'}}}}\n"
    );
    assert!(
        validate(&skin(&unused), TEST)
            .iter()
            .any(|d| d.severity == Severity::Warning && d.field == "pipelines.idle")
    );
}

#[test]
fn every_registered_processor_has_typed_ports() {
    for p in PROCESSORS {
        assert!(!p.outputs.is_empty(), "{}", p.name);
        assert!(processor(p.name, p.version).is_some());
    }
}

fn love_letter_strokes(skin: &Skin) -> Arc<LoadedData> {
    let binding = &skin.material_bindings["write"];
    assert!(binding.text_layout);
    assert_eq!(binding.textures.len(), 1);
    assert_eq!(binding.uniforms.len(), 1);
    match &binding.nodes[0].inputs["strokes"] {
        BoundInput::Data(data) => data.clone(),
        input => panic!("{input:?}"),
    }
}

#[test]
fn love_letter_roundtrips_its_data_pipeline_without_the_original_directory() {
    let registry = SkinRegistry::new(None, "1.0.0");
    let original = registry.load("nanatype.love_letter").unwrap();
    let data = love_letter_strokes(&original.skin);
    let DataValue::Strokes(strokes) = &data.value else {
        panic!("stroke data expected")
    };
    assert_eq!(strokes.len(), 6763);
    assert_eq!(strokes.glyph('心').unwrap().strokes.len(), 4);
    assert!(original.skin.entrance_needs_text_layout());
    // Parsed data is shared with any other skin loading the same bytes.
    let again = SkinRegistry::new(None, "1.0.0")
        .load("nanatype.love_letter")
        .unwrap();
    assert!(Arc::ptr_eq(&data, &love_letter_strokes(&again.skin)));
    let dir = tempfile::tempdir().unwrap();
    let archive = dir.path().join("love-letter.ntpkg");
    registry.export("nanatype.love_letter", &archive).unwrap();
    let files = Arc::new(ZipFiles::open(&archive).unwrap());
    let restored = load(
        files.clone(),
        SkinOrigin::Archive(archive.clone()),
        &Offline,
        "1.0.0",
    )
    .unwrap();
    assert_eq!(love_letter_strokes(&restored.skin), data);
    for path in ["ARPHICPL.TXT", "SOURCES.txt", "write.wgsl", "strokes.nthz"] {
        assert!(!files.read(path).unwrap().is_empty(), "{path}");
    }
    if let Ok(destination) = std::env::var("NANATYPE_LOVE_LETTER_EXPORT") {
        std::fs::copy(archive, destination).unwrap();
    }
}

#[test]
fn inherited_letters_export_the_data_they_depend_on() {
    let dir = tempfile::tempdir().unwrap();
    let mut registry = SkinRegistry::new(Some(dir.path().join("skins")), "1.0.0");
    let name = [("en-US".into(), "My letter".into())].into_iter().collect();
    registry
        .duplicate(
            "nanatype.love_letter",
            "test.inherited-letter",
            &name,
            &TomlTable::new(),
        )
        .unwrap();
    let child = registry.load("test.inherited-letter").unwrap();
    assert_eq!(
        love_letter_strokes(&child.skin).bytes,
        love_letter_strokes(&registry.load("nanatype.love_letter").unwrap().skin).bytes
    );
    let archive = dir.path().join("child.ntpkg");
    registry.export("test.inherited-letter", &archive).unwrap();
    let loaded = registry.inspect_archive(&archive).unwrap();
    assert!(loaded.skin.entrance_needs_text_layout());
    love_letter_strokes(&loaded.skin);
}

#[test]
fn missing_mistyped_or_corrupt_data_fails_the_skin() {
    let dir = tempfile::tempdir().unwrap();
    let strokes = include_bytes!("../../builtin/love_letter/strokes.nthz");
    let mut corrupt = strokes.to_vec();
    let metadata_len = u32::from_le_bytes(strokes[8..12].try_into().unwrap()) as usize;
    let count = u32::from_le_bytes(strokes[12..16].try_into().unwrap()) as usize;
    let payload = 24 + metadata_len + count * 12 + count.div_ceil(64) * 12;
    corrupt[payload + 10] ^= 0xff;
    std::fs::write(dir.path().join("bad.nthz"), corrupt).unwrap();
    std::fs::write(dir.path().join("words.json"), "[]").unwrap();
    std::fs::write(
        dir.path().join("write.wgsl"),
        include_str!("../../builtin/love_letter/write.wgsl"),
    )
    .unwrap();
    for (format, path, why) in [
        ("nthz", "missing.nthz", "missing"),
        ("nthz", "bad.nthz", "corrupt block"),
        ("json", "words.json", "wrong declared format"),
    ] {
        std::fs::write(
            dir.path().join("package.toml"),
            format!(
                "format=1\nschema=3\nid='test.letter'\nversion='1.0.0'\ncapabilities=['skin']\n\
                 [data.strokes]\npath='{path}'\nformat='{format}'\nformat_version=1\nsource='s'\nlicense='l'\n"
            ),
        )
        .unwrap();
        std::fs::write(
            dir.path().join("skin.toml"),
            "[dialogue.reveal.entrance]\nmaterial='write'\n\
             [pipelines.map]\nprocessor='text.stroke_time_map'\nversion=1\ninputs={strokes={data='strokes'}}\n\
             [materials.write]\nsource='write.wgsl'\ntextures=[{pipeline='map',output='time'}]\n",
        )
        .unwrap();
        let errors = load(
            Arc::new(DirFiles::new(dir.path())),
            SkinOrigin::Directory(dir.path().into()),
            &Offline,
            "1.0.0",
        )
        .unwrap_err();
        assert!(
            errors
                .iter()
                .any(|e| e.field == "pipelines.map.inputs.strokes"),
            "{why}: {errors:?}"
        );
    }
}

#[test]
fn removed_handwriting_key_fails_with_a_migration_hint() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("skin.toml"),
        "[meta]\nid='test.old'\n[dialogue.reveal.handwriting]\ndata='strokes.nthz'\n",
    )
    .unwrap();
    let errors = load(
        Arc::new(DirFiles::new(dir.path())),
        SkinOrigin::Directory(dir.path().into()),
        &Offline,
        "1.0.0",
    )
    .unwrap_err();
    assert!(
        errors
            .iter()
            .any(|e| e.field == "dialogue.reveal.handwriting"
                && e.message.contains("text.stroke_time_map"))
    );
}
