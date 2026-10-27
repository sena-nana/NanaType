use std::collections::BTreeMap;
use std::sync::Arc;

use nanatype_model::{ParamValue, Rgba8};

use crate::*;

const APP: &str = "1.0.0";

const SHADER: &str = "fn effect(i: EffectInput) -> vec4<f32> { let c = param(1u); return vec4(c.rgb, param(0u).x); }";

const BASE: &str = r##"
[meta]
id = "user.params"
[param_groups.look]
name = { zh-CN = "外观", en-US = "Look" }
[params.fur]
type = "color"
group = "look"
default = "#fff7ec"
name = { en-US = "Fur" }
[params.ink]
type = "color"
default = "#4b3426"
[params.size]
type = "number"
default = 28
min = 16
max = 40
step = 2
[params.round]
type = "number"
default = 0.5
min = 0
max = 1
step = 0.05
[params.align]
type = "enum"
default = "start"
options = [{ id = "start" }, { id = "center" }]
[params.tail]
type = "bool"
default = true
[presets.night]
values = { fur = "#2b2730", ink = "#f5efe6" }
[[param_checks]]
foreground = "ink"
background = "fur"
min = 4.5
[dialogue.box]
color = { param = "fur" }
[dialogue.text]
color = { param = "ink" }
size = { param = "size" }
align = { param = "align" }
[shaders.art]
animated = false
source = "art.wgsl"
bindings = [["round", "tail", 3, "align"], "fur"]
[[dialogue.shader_effects]]
shader = "art"
target = "artwork"
"##;

fn load_str(manifest: &str) -> Result<LoadedSkin, Vec<SkinDiagnostic>> {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join(MANIFEST), manifest).unwrap();
    std::fs::write(dir.path().join("art.wgsl"), SHADER).unwrap();
    let files: Arc<dyn SkinFiles> = Arc::new(DirFiles::new(dir.path()));
    let out = load(files, SkinOrigin::Directory(dir.path().into()), &(), APP);
    drop(dir);
    out
}

fn errors_of(manifest: &str) -> Vec<String> {
    match load_str(manifest) {
        Ok(l) => panic!("loaded: {:?}", l.warnings),
        Err(d) => d
            .into_iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| d.to_string())
            .collect(),
    }
}

fn values(pairs: &[(&str, ParamValue)]) -> BTreeMap<String, ParamValue> {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), v.clone()))
        .collect()
}

fn text(s: &str) -> ParamValue {
    ParamValue::Text(s.into())
}

#[test]
fn defaults_fill_placeholders_and_values_resolve_without_recompiling() {
    let loaded = load_str(BASE).unwrap();
    assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
    let skin = &loaded.skin;
    assert_eq!(skin.dialogue.box_.color, Rgba8::rgb(0xff, 0xf7, 0xec));
    assert_eq!(skin.dialogue.text.size, 28.0);
    assert_eq!(skin.param_sites.len(), 4);
    assert_eq!(skin.param_values.get("tail"), Some(&Resolved::Bool(true)));

    let (resolved, warnings) = resolve_params(
        skin,
        &values(&[
            ("fur", text("#102030")),
            ("size", ParamValue::Number(36.0)),
            ("align", text("center")),
        ]),
        APP,
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    assert_eq!(resolved.dialogue.box_.color, Rgba8::rgb(0x10, 0x20, 0x30));
    assert_eq!(resolved.dialogue.text.size, 36.0);
    assert_eq!(resolved.dialogue.text.align, TextAlign::Center);
    assert!(Arc::ptr_eq(
        &resolved.shader_programs["art"],
        &skin.shader_programs["art"]
    ));
    assert!(Arc::ptr_eq(&resolved.param_sites, &skin.param_sites));
    assert_eq!(resolved.referenced_files(), skin.referenced_files());

    // Defaults written explicitly resolve to the loaded skin.
    let (same, _) = resolve_params(skin, &values(&[("size", ParamValue::Number(28.0))]), APP);
    assert_eq!(&same, &**skin);
}

#[test]
fn invalid_saved_values_fall_back_to_defaults_with_warnings() {
    let loaded = load_str(BASE).unwrap();
    let (resolved, warnings) = resolve_params(
        &loaded.skin,
        &values(&[
            ("size", ParamValue::Number(99.0)),
            ("fur", text("pink")),
            ("align", text("justify")),
            ("tail", text("yes")),
            ("gone", ParamValue::Bool(true)),
        ]),
        APP,
    );
    assert_eq!(warnings.len(), 5, "{warnings:?}");
    assert_eq!(&resolved, &*loaded.skin);
    let mut kept = values(&[
        ("size", ParamValue::Number(28.0)),
        ("round", ParamValue::Number(0.7)),
        ("fur", text("xyz")),
    ]);
    retain_valid_params(&loaded.skin, &mut kept);
    assert_eq!(kept, values(&[("round", ParamValue::Number(0.7))]));
}

#[test]
fn presets_and_contrast_checks_warn_but_never_refuse() {
    let skin = load_str(BASE).unwrap().skin;
    let night = skin.preset_values("night").unwrap();
    assert_eq!(night["fur"], Resolved::Color(Rgba8::rgb(0x2b, 0x27, 0x30)));
    assert_eq!(night["size"], Resolved::Number(28.0));
    assert!(skin.failed_checks(&night).is_empty());
    let mut low = skin.param_defaults();
    low.insert("ink".into(), Resolved::Color(Rgba8::rgb(0xee, 0xee, 0xee)));
    assert_eq!(skin.failed_checks(&low).len(), 1);

    let warned = load_str(&BASE.replace("ink = \"#f5efe6\"", "ink = \"#3a3a3a\"")).unwrap();
    assert!(
        warned.warnings.iter().any(|w| w.field == "presets.night"),
        "{:?}",
        warned.warnings
    );
}

#[test]
fn schema_rules_are_enforced() {
    for (patch, expect) in [
        ("[params.Bad]\ntype='bool'\ndefault=true", "params.Bad"),
        ("[params.n]\ndefault=1", "needs `min` and `max`"),
        ("[params.n]\ndefault=3\nmin=0\nmax=10\nstep=2", "step"),
        ("[params.n]\ndefault=1\nmin=5\nmax=1", "below `max`"),
        ("[params.n]\ndefault=20\nmin=0\nmax=10", "params.n.default"),
        (
            "[params.e]\ntype='enum'\ndefault='a'\noptions=[{id='a'},{id='a'}]",
            "duplicate",
        ),
        (
            "[params.e]\ntype='enum'\ndefault='b'\noptions=[{id='a'}]",
            "params.e.default",
        ),
        (
            "[params.t]\ntype='text'\ndefault='hi'\nmax_len=100",
            "max_len",
        ),
        ("[params.t]\ntype='text'\ndefault=''", "params.t.default"),
        ("[params.c]\ntype='color'\ndefault='#ff000080'", "opaque"),
        (
            "[params.b]\ntype='bool'\ndefault=true\ngroup='nope'",
            "group",
        ),
        ("[presets.x]\nvalues={ size = 3 }", "presets.x.values.size"),
        (
            "[presets.y]\nvalues={ nothing = 3 }",
            "not a declared param",
        ),
        (
            "[[param_checks]]\nforeground='size'\nbackground='fur'\nmin=3",
            "param_checks.1.foreground",
        ),
    ] {
        let errors = errors_of(&format!("{BASE}\n{patch}\n"));
        assert!(
            errors.iter().any(|e| e.contains(expect)),
            "{patch}: {errors:?}"
        );
    }
}

#[test]
fn placeholders_cannot_reach_files_or_mismatch_their_field() {
    for (patch, expect) in [
        (
            "[dialogue.box.frame]\nimage={ param = 'ink' }",
            "cannot set files",
        ),
        (
            "[fonts.body]\nweight={ param = 'size' }",
            "cannot set files",
        ),
        (
            "[shaders.art2]\nsource={ param = 'ink' }",
            "cannot set files",
        ),
        (
            "[dialogue.nameplate]\ncolor={ param = 'tail' }",
            "cannot fill this field",
        ),
        (
            "[dialogue.nameplate]\ncolor={ param = 'missing' }",
            "not a declared param",
        ),
        (
            "[dialogue.nameplate]\ncolor={ param = 'ink', x = 1 }",
            "whole value",
        ),
        (
            "[dialogue.reveal]\nbase_ms={ param = 'round' }",
            "whole number",
        ),
    ] {
        let errors = errors_of(&format!("{BASE}\n{patch}\n"));
        assert!(
            errors.iter().any(|e| e.contains(expect)),
            "{patch}: {errors:?}"
        );
    }
}

#[test]
fn every_value_a_param_can_take_is_validated_where_it_lands() {
    // A whole-number field takes a whole-number param.
    let ok = load_str(&format!(
        "{BASE}\n[params.ms]\ndefault=50\nmin=10\nmax=200\n[dialogue.reveal]\nbase_ms={{ param = 'ms' }}\n"
    ))
    .unwrap();
    let (fast, _) = resolve_params(&ok.skin, &values(&[("ms", ParamValue::Number(10.0))]), APP);
    assert_eq!(fast.dialogue.reveal.base_ms, 10);
    // Text bound to decor text must fit its 8-character limit.
    let decor = "[[dialogue.decor]]\nsize=[20,20]\ntext={ param = 'sign' }\n";
    let errors = errors_of(&format!(
        "{BASE}\n[params.sign]\ntype='text'\ndefault='喵'\nmax_len=12\n{decor}"
    ));
    assert!(
        errors.iter().any(|e| e.contains("params.sign")),
        "{errors:?}"
    );
    assert!(
        load_str(&format!(
            "{BASE}\n[params.sign]\ntype='text'\ndefault='喵'\nmax_len=4\n{decor}"
        ))
        .is_ok()
    );
    // A size param whose range leaves the field's limits is refused.
    let errors = errors_of(&BASE.replace("max = 40", "max = 500"));
    assert!(
        errors.iter().any(|e| e.contains("params.size = 500")),
        "{errors:?}"
    );
}

#[test]
fn bindings_name_scalar_or_colour_params() {
    for (bindings, expect) in [
        ("['nope']", "not a declared param"),
        ("[['fur', 1]]", "not a scalar"),
        ("[[1, 2, 3, 4, 5]]", "1–4"),
    ] {
        let errors = errors_of(&BASE.replace(
            "bindings = [[\"round\", \"tail\", 3, \"align\"], \"fur\"]",
            &format!("bindings = {bindings}"),
        ));
        assert!(
            errors.iter().any(|e| e.contains(expect)),
            "{bindings}: {errors:?}"
        );
    }
    let text = format!("{BASE}\n[params.t]\ntype='text'\ndefault='a'\n");
    let errors = errors_of(&text.replace("\"fur\"]", "\"t\"]"));
    assert!(
        errors.iter().any(|e| e.contains("text params")),
        "{errors:?}"
    );
}

#[test]
fn unused_params_warn() {
    let loaded = load_str(&format!(
        "{BASE}\n[params.spare]\ntype='bool'\ndefault=false\n"
    ))
    .unwrap();
    assert!(loaded.warnings.iter().any(|w| w.field == "params.spare"));
}

#[test]
fn export_keeps_placeholders_and_reloads_equal() {
    let skin = load_str(BASE).unwrap().skin;
    let manifest = standalone_manifest(&skin).unwrap();
    let text = toml::to_string(&manifest).unwrap();
    assert!(text.contains("param = \"fur\""), "{text}");
    let reloaded = load_str(&text).unwrap().skin;
    assert_eq!(reloaded.dialogue, skin.dialogue);
    assert_eq!(reloaded.params, skin.params);
    assert_eq!(reloaded.param_sites, skin.param_sites);
}

#[test]
fn input_parsing_and_grapheme_limits() {
    let skin = load_str(BASE).unwrap().skin;
    let size = &skin.params["size"];
    assert_eq!(size.parse_input(" 30 "), Ok(ParamValue::Number(30.0)));
    assert!(matches!(size.parse_input("x"), Err(ParamError::NotANumber)));
    assert!(matches!(
        size.parse_input("80"),
        Err(ParamError::OutOfRange { .. })
    ));
    let fur = &skin.params["fur"];
    assert_eq!(fur.parse_input("#ABC"), Ok(text("#aabbcc")));
    assert_eq!(fur.parse_input("nope"), Err(ParamError::BadColor));
    let spec = ParamSpec {
        kind: ParamType::Text,
        default: Some(text("a")),
        max_len: Some(2),
        ..Default::default()
    };
    assert!(spec.coerce(&text("👩‍👩‍👧‍👦🐱")).is_ok());
    assert_eq!(text_len("👩‍👩‍👧‍👦🐱"), 2);
    assert!(matches!(
        spec.coerce(&text("abc")),
        Err(ParamError::TooLong { max: 2, len: 3 })
    ));
    assert_eq!(
        spec.coerce(&text("a\nb")),
        Err(ParamError::ControlCharacter)
    );
}

#[test]
fn ordered_listing_follows_groups_then_ids() {
    let skin = load_str(BASE).unwrap().skin;
    let listing = skin.ordered_params();
    assert_eq!(listing[0].0, Some("look"));
    assert_eq!(listing[0].1[0].0, "fur");
    assert_eq!(listing[1].0, None);
    assert!(listing[1].1.iter().any(|(id, _)| *id == "size"));
}

#[test]
fn shader_param_helper_is_host_owned() {
    assert!(
        validate_shader("fn effect(i: EffectInput) -> vec4<f32> { return param(3u); }").is_ok()
    );
    for source in [
        "fn param(i: u32) -> vec4<f32> { return vec4(0.); } fn effect(i: EffectInput) -> vec4<f32> { return param(0u); }",
        "fn effect(i: EffectInput) -> vec4<f32> { return fx_slots.v[0]; }",
    ] {
        assert!(validate_shader(source).is_err(), "accepted {source}");
    }
    assert!(
        validate_material("fn material(i: MaterialInput) -> vec4<f32> { return param(0u); }")
            .is_err()
    );
    let (ir, expanded, depth) = shader_cost(SHADER).unwrap();
    assert!(
        ir > 0 && expanded >= 8 && depth > 0,
        "{ir} {expanded} {depth}"
    );
}

#[test]
fn every_builtin_roundtrips_through_a_toml_value_losslessly() {
    let registry = SkinRegistry::new(None, APP);
    for entry in registry.entries() {
        let skin = registry.load(&entry.id).unwrap().skin;
        let mut restored: Skin = toml::Value::try_from(&*skin).unwrap().try_into().unwrap();
        restored.material_bindings = skin.material_bindings.clone();
        restored.material_programs = skin.material_programs.clone();
        restored.shader_programs = skin.shader_programs.clone();
        restored.param_sites = skin.param_sites.clone();
        restored.param_values = skin.param_values.clone();
        assert_eq!(restored, *skin, "{}", entry.id);
    }
}
