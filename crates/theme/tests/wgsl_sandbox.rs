//! The WGSL sandbox against hostile skin packages. Every attack is rejected
//! by the layer meant to stop it, through the same load path the app uses,
//! and no input can panic the validator or need more than a small stack
//! from its caller (naga runs on its own thread).

use nanatype_theme::{
    DirFiles, MAX_SHADER_BYTES, MAX_SHADER_NODES, Severity, SkinOrigin, load, validate_material,
    validate_shader,
};
use proptest::prelude::*;
use std::sync::Arc;

/// Far below any platform's default thread stack: a validator that recursed
/// on the caller's stack overflows here and aborts the test binary.
/// Windows needs a little more headroom for the CRT before user code runs;
/// keep this well under the 1 MiB default so a caller-stack recursion still dies.
const CALLER_STACK: usize = 512 << 10;

fn on_small_stack<T: Send>(f: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|s| {
        std::thread::Builder::new()
            .stack_size(CALLER_STACK)
            .spawn_scoped(s, f)
            .unwrap()
            .join()
            .expect("the validator panicked")
    })
}

fn effect(body: &str) -> String {
    format!("fn effect(i: EffectInput) -> vec4<f32> {{ {body} }}")
}

const OK: &str = "fn effect(i: EffectInput) -> vec4<f32> { return vec4(i.accent.rgb, 0.5); }";
const ZERO: &str = "return vec4(0.);";

/// (attack, source, a fragment of the error that names the stopping layer)
fn effect_attacks() -> Vec<(&'static str, String, &'static str)> {
    let with = |decl: &str| format!("{decl}\n{}", effect(ZERO));
    let mut doubling =
        "fn f0(i: EffectInput) -> vec4<f32> { return vec4(particles(i.uv, i.phase, i.seed)); }\n"
            .to_owned();
    for n in 1..6 {
        doubling += &format!(
            "fn f{n}(i: EffectInput) -> vec4<f32> {{ return f{p}(i) + f{p}(i); }}\n",
            p = n - 1
        );
    }
    doubling += &effect("return f5(i);");
    let mut calls = "fn f0(i: EffectInput) -> f32 { return 0.; }\n".to_owned();
    for n in 1..10 {
        calls += &format!(
            "fn f{n}(i: EffectInput) -> f32 {{ return f{}(i); }}\n",
            n - 1
        );
    }
    calls += &effect("return vec4(f9(i));");
    let lets: String = (0..1500)
        .map(|n| format!("let a{n} = i.uv.x * 2.;\n"))
        .collect();
    let mut nested = ZERO.to_owned();
    for _ in 0..40 {
        nested = format!("if i.uv.x > 0. {{ {nested} }}");
    }
    let big = "array<mat4x4<f32>, 64>";
    vec![
        // Unbounded work.
        ("loop", effect("loop {} return vec4(0.);"), "loops"),
        (
            "for",
            effect("var x = 0.; for (var n = 0; n < 9; n++) { x += 1.; } return vec4(x);"),
            "loops",
        ),
        (
            "while",
            effect("var x = 0.; while x < 9. { x += 1.; } return vec4(x);"),
            "loops",
        ),
        (
            "recursion",
            with("fn f(x: f32) -> f32 { return f(x); }"),
            "error",
        ),
        ("expanded cost", doubling, "4096"),
        ("call depth", calls, "depth exceeds eight"),
        (
            "IR size",
            effect(&format!("{lets} return vec4(a0);")),
            "4096 IR nodes",
        ),
        (
            "operator chain",
            effect(&format!("return vec4({});", vec!["i.uv.x"; 8000].join("+"))),
            "operators",
        ),
        // Postfix chains: naga recursed once per link and overflowed its own
        // 32 MiB stack before they counted towards the operator limit.
        (
            "member chain",
            effect(&format!("return vec4(i{});", ".ai".repeat(9000))),
            "operators",
        ),
        (
            "index chain",
            effect(&format!(
                "var a: array<f32, 4>; return vec4(a{});",
                "[0]".repeat(9000)
            )),
            "operators",
        ),
        (
            "swizzle chain",
            effect(&format!("return vec4(i.accent{});", ".xyzw".repeat(6000))),
            "operators",
        ),
        (
            "deep expression",
            effect(&format!(
                "let x = i.uv.x; return vec4({});",
                vec!["x"; 200].join("+")
            )),
            "deeper than",
        ),
        (
            "nested calls",
            effect(&format!(
                "return vec4({}i.uv.x{});",
                "abs(".repeat(100),
                ")".repeat(100)
            )),
            "deeper than",
        ),
        (
            "nested blocks",
            effect(&format!("{nested} return vec4(0.);")),
            "nest deeper",
        ),
        // Compute and other stages.
        (
            "compute entry",
            with("@compute @workgroup_size(1) fn c() {}"),
            "`compute`",
        ),
        (
            "fragment entry",
            with("@fragment fn f() -> @location(0) vec4<f32> { return vec4(1.); }"),
            "entry points",
        ),
        (
            "vertex entry",
            with("@vertex fn v() -> @builtin(position) vec4<f32> { return vec4(0.); }"),
            "entry points",
        ),
        (
            "workgroup memory",
            with("var<workgroup> w: f32;"),
            "`workgroup`",
        ),
        (
            "barrier",
            effect("workgroupBarrier(); return vec4(0.);"),
            "`workgroupBarrier`",
        ),
        (
            "storage barrier",
            effect("storageBarrier(); return vec4(0.);"),
            "`storageBarrier`",
        ),
        (
            "subgroup",
            effect("return vec4(subgroupAdd(i.uv.x));"),
            "`subgroupAdd`",
        ),
        // Resources and memory.
        (
            "storage buffer",
            with("@group(0) @binding(9) var<storage, read_write> s: array<f32>;"),
            "`storage`",
        ),
        (
            "uniform binding",
            with("@group(1) @binding(0) var<uniform> u: vec4<f32>;"),
            "globals",
        ),
        (
            "extra texture",
            with("@group(0) @binding(8) var t: texture_2d<f32>;"),
            "globals",
        ),
        ("private global", with("var<private> p: f32;"), "globals"),
        (
            "storage texture",
            with("@group(0) @binding(9) var t: texture_storage_2d<rgba8unorm, write>;"),
            "`texture_storage_2d`",
        ),
        (
            "texture store",
            effect("textureStore(t, vec2(0), vec4(0.)); return vec4(0.);"),
            "`textureStore`",
        ),
        (
            "array length",
            effect("return vec4(f32(arrayLength(&a)));"),
            "`arrayLength`",
        ),
        ("atomic", with("var<private> a: atomic<u32>;"), "`atomic`"),
        (
            "pointer",
            with("fn f(p: ptr<function, f32>) -> f32 { return *p; }"),
            "`ptr`",
        ),
        (
            "huge array",
            effect("var a: array<f32, 1000000>; return vec4(a[0]);"),
            "at most 64 elements",
        ),
        (
            "huge type",
            with(&format!("struct S {{ a: {big}, b: f32 }}")),
            "4096 bytes",
        ),
        (
            "huge locals",
            effect(&format!(
                "var a: {big}; var b: {big}; var c: {big}; var d: {big}; var e: {big}; return a[0][0] + b[0][0] + c[0][0] + d[0][0] + e[0][0];"
            )),
            "16 KiB",
        ),
        // The host's interface.
        ("host slots", effect("return fx_slots.v[0];"), "reserved"),
        (
            "host texture",
            effect("return textureSampleLevel(fx_texture0, fx_sampler, i.uv, 0.);"),
            "reserved",
        ),
        (
            "material host",
            effect("return nm_parameters[0];"),
            "reserved",
        ),
        (
            "double underscore",
            with("fn __f() -> f32 { return 0.; }"),
            "reserved",
        ),
        (
            "redefined helper",
            with("fn sample0(uv: vec2<f32>) -> vec4<f32> { return vec4(1.); }"),
            "sample0",
        ),
        (
            "derivative",
            effect("return vec4(dpdx(i.uv.x));"),
            "only local arithmetic",
        ),
        ("discard", effect("discard; return vec4(0.);"), "discard"),
        (
            "wrong signature",
            "fn effect(i: f32) -> vec4<f32> { return vec4(i); }".into(),
            "expected fn effect",
        ),
        (
            "missing entry",
            "fn other() -> f32 { return 0.; }".into(),
            "missing fn effect",
        ),
        // Directives and source bytes.
        ("override", with("override o: f32 = 1.;"), "directives"),
        ("enable", format!("enable f16;\n{OK}"), "directives"),
        (
            "requires",
            format!("requires readonly_and_readwrite_storage_textures;\n{OK}"),
            "directives",
        ),
        (
            "diagnostic",
            format!("diagnostic(off, derivative_uniformity);\n{OK}"),
            "directives",
        ),
        ("NUL byte", format!("{OK}\0"), "control character"),
        ("escape byte", format!("{OK}\x1b[2J"), "control character"),
        (
            "oversized",
            format!("{OK}{}", " ".repeat(MAX_SHADER_BYTES)),
            "64 KiB",
        ),
    ]
}

#[test]
fn every_effect_attack_is_rejected_by_its_layer() {
    for (attack, source, expected) in effect_attacks() {
        let result = on_small_stack(|| validate_shader(&source));
        let err = result.expect_err(attack);
        assert!(err.contains(expected), "{attack}: {err}");
    }
}

#[test]
fn every_material_attack_is_rejected() {
    let material = |body: &str| format!("fn material(i: MaterialInput) -> vec4<f32> {{ {body} }}");
    for (attack, source, expected) in [
        (
            "host parameters",
            material("return nm_parameters[0];"),
            "reserved",
        ),
        (
            "array length",
            material("return vec4(f32(arrayLength(&nm_parameters)));"),
            "`arrayLength`",
        ),
        (
            "compute",
            format!(
                "@compute @workgroup_size(1) fn c() {{}}\n{}",
                material(ZERO)
            ),
            "`compute`",
        ),
        (
            "storage",
            format!(
                "@group(2) @binding(9) var<storage, read_write> s: array<f32>;\n{}",
                material(ZERO)
            ),
            "`storage`",
        ),
        (
            "private global",
            format!("var<private> p: f32;\n{}", material(ZERO)),
            "globals",
        ),
        (
            "effect host name",
            material("return vec4(fx_x);"),
            "reserved",
        ),
        ("loop", material("loop {} return vec4(0.);"), "loops"),
        (
            "deep expression",
            material(&format!(
                "let x = i.uv.x; return vec4({});",
                vec!["x"; 200].join("+")
            )),
            "deeper than",
        ),
    ] {
        let err = on_small_stack(|| validate_material(&source)).expect_err(attack);
        assert!(err.contains(expected), "{attack}: {err}");
    }
    let ok = on_small_stack(|| {
        validate_material(
            "fn material(i: MaterialInput) -> vec4<f32> { return material_param(0u) * material_sample0(i.uv); }",
        )
    });
    assert!(ok.unwrap().cost > 0);
}

#[test]
fn shaders_at_the_limits_still_validate() {
    // Shallow but wide: a statement just under the operator cap, split into
    // lets so no single expression nests past the depth limit.
    let lets: String = (0..8)
        .map(|n| format!("let s{n} = {};\n", vec!["i.uv.x"; 30].join("+")))
        .collect();
    let sum = (0..8)
        .map(|n| format!("s{n}"))
        .collect::<Vec<_>>()
        .join("+");
    for source in [
        OK.to_owned(),
        effect(&format!("{lets} return vec4({sum});")),
        effect(&format!("return vec4({});", vec!["i.uv.x"; 60].join("+"))),
        // Comments and identifiers that only contain banned words.
        format!("// compute storage ptr atomic fx_ \u{0}\n/* enable */\n{OK}").replace('\u{0}', ""),
        format!(
            "fn storage_tint(c: vec4<f32>) -> vec4<f32> {{ return c; }}\n{}",
            effect("return storage_tint(i.accent);")
        ),
    ] {
        let program = on_small_stack(|| validate_shader(&source))
            .unwrap_or_else(|e| panic!("rejected {source:?}: {e}"));
        assert!(program.cost as usize <= MAX_SHADER_NODES);
    }
}

#[test]
fn every_builtin_shader_validates_on_a_small_stack() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("builtin");
    let mut checked = 0;
    for skin in std::fs::read_dir(root).unwrap() {
        for file in std::fs::read_dir(skin.unwrap().path()).unwrap() {
            let path = file.unwrap().path();
            if path.extension().is_none_or(|e| e != "wgsl") {
                continue;
            }
            let source = std::fs::read_to_string(&path).unwrap();
            let result = on_small_stack(|| {
                if source.contains("fn effect(") {
                    validate_shader(&source)
                } else {
                    validate_material(&source)
                }
            });
            if let Err(e) = result {
                panic!("{}: {e}", path.display());
            }
            checked += 1;
        }
    }
    assert!(checked > 20, "found only {checked} built-in shaders");
}

/// A package the app would import, with `shaders` mapped to their sources.
fn package(shaders: &[(&str, &str)]) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("package.toml"),
        r#"format = 1
capabilities = ["skin"]
schema = 3
id = "test.hostile"
version = "1.0.0"
name = { "en-US" = "Hostile" }
"#,
    )
    .unwrap();
    let mut manifest = String::new();
    for (id, source) in shaders {
        std::fs::write(dir.path().join(format!("{id}.wgsl")), source).unwrap();
        manifest += &format!(
            "[shaders.{id}]\nsource = \"{id}.wgsl\"\n\n[[dialogue.shader_effects]]\nshader = \"{id}\"\n\n"
        );
    }
    std::fs::write(dir.path().join("skin.toml"), manifest).unwrap();
    dir
}

fn load_package(
    dir: &tempfile::TempDir,
) -> Result<nanatype_theme::LoadedSkin, Vec<nanatype_theme::SkinDiagnostic>> {
    let path = dir.path().to_owned();
    on_small_stack(move || {
        load(
            Arc::new(DirFiles::new(&path)),
            SkinOrigin::Directory(path.clone()),
            &(),
            env!("CARGO_PKG_VERSION"),
        )
    })
}

#[test]
fn a_hostile_package_is_refused_with_a_diagnostic_on_the_offending_shader() {
    let good = package(&[("calm", OK)]);
    let loaded = load_package(&good).unwrap_or_else(|d| panic!("{d:?}"));
    assert!(loaded.skin.shader_programs["calm"].cost > 0);

    let deep = effect(&format!(
        "return vec4({});",
        vec!["i.uv.x"; 5000].join(" + ")
    ));
    let compute = format!("@compute @workgroup_size(64) fn c() {{}}\n{OK}");
    let hostile = package(&[("calm", OK), ("deep", &deep), ("compute", &compute)]);
    let Err(diagnostics) = load_package(&hostile) else {
        panic!("a hostile package loaded");
    };
    let error = |field: &str| {
        diagnostics
            .iter()
            .find(|d| d.severity == Severity::Error && d.field == field)
            .map(|d| d.message.clone())
    };
    assert!(
        error("shaders.deep").is_some_and(|m| m.contains("deep.wgsl") && m.contains("operators"))
    );
    assert!(error("shaders.compute").is_some_and(|m| m.contains("`compute`")));
    assert_eq!(error("shaders.calm"), None, "{diagnostics:?}");
}

const TOKENS: &[&str] = &[
    "(",
    ")",
    "{",
    "}",
    "[",
    "]",
    ";",
    ",",
    "+",
    "-",
    "*",
    "/",
    "%",
    "<",
    ">",
    "=",
    "!",
    "&",
    "|",
    "^",
    "~",
    ".",
    "@",
    "loop",
    "for",
    "while",
    "if",
    "else",
    "switch",
    "return",
    "discard",
    "let",
    "var",
    "fn",
    "struct",
    "const",
    "override",
    "enable",
    "@compute",
    "@fragment",
    "@vertex",
    "@group(0)",
    "@binding(9)",
    "var<storage>",
    "var<workgroup>",
    "var<private>",
    "ptr<function, f32>",
    "atomic<u32>",
    "array<f32, 99999>",
    "fx_slots",
    "fx_texture0",
    "nm_parameters",
    "sample0(i.uv)",
    "particles(i.uv, i.phase, i.seed)",
    "param(0u)",
    "effect(i)",
    "textureStore",
    "workgroupBarrier()",
    "arrayLength",
    "dpdx(i.uv.x)",
    "i.uv.x",
    "1e38",
    "0xffffffffu",
    "-2147483648i",
    "vec4<f32>",
    "mat4x4<f32>",
    "/*",
    "*/",
    "//",
    "\n",
    "\0",
    "\u{202e}",
    "é",
    ".ai",
    ".xyzw",
    "[0]",
    "i.accent",
];

fn mutated() -> impl Strategy<Value = String> {
    let seeds = [
        OK.to_owned(),
        effect(
            "let w = 0.5 + 0.5 * sin(i.uv.x * 12. + i.phase * 6.28318); return vec4(i.accent.rgb, w);",
        ),
        effect(
            "if i.uv.y > 0.5 { return sample0(i.uv); } return vec4(particles(i.uv, i.phase, i.seed));",
        ),
    ];
    let edit = prop_oneof![
        // Insert a token at a position.
        (any::<prop::sample::Index>(), prop::sample::select(TOKENS)).prop_map(|(at, t)| (
            0u8,
            at,
            at,
            t.to_owned(),
            1usize
        )),
        // Delete a span.
        (any::<prop::sample::Index>(), any::<prop::sample::Index>()).prop_map(|(a, b)| (
            1u8,
            a,
            b,
            String::new(),
            1usize
        )),
        // Repeat a span many times.
        (
            any::<prop::sample::Index>(),
            any::<prop::sample::Index>(),
            2usize..400
        )
            .prop_map(|(a, b, n)| (2u8, a, b, String::new(), n)),
    ];
    (
        prop::sample::select(seeds.to_vec()),
        prop::collection::vec(edit, 1..12),
    )
        .prop_map(|(mut source, edits)| {
            for (kind, a, b, token, n) in edits {
                let chars: Vec<char> = source.chars().collect();
                let len = chars.len().max(1);
                let (x, y) = {
                    let (x, y) = (a.index(len), b.index(len));
                    (x.min(y), x.max(y).min(chars.len()))
                };
                let x = x.min(chars.len());
                let before: String = chars[..x].iter().collect();
                let span: String = chars[x..y].iter().collect();
                let after: String = chars[y..].iter().collect();
                source = match kind {
                    0 => format!("{before}{token}{span}{after}"),
                    1 => format!("{before}{after}"),
                    _ => format!("{before}{}{after}", span.repeat(n)),
                };
                source.truncate(source.floor_char_boundary(MAX_SHADER_BYTES + 64));
            }
            source
        })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 400, ..ProptestConfig::default() })]

    /// Mutated shaders never panic or exhaust the stack, and anything that
    /// passes stays inside the sandbox.
    #[test]
    fn mutated_shaders_are_rejected_or_stay_inside_the_sandbox(source in mutated()) {
        let result = on_small_stack(|| validate_shader(&source));
        if let Ok(program) = result {
            prop_assert!(program.cost as usize <= MAX_SHADER_NODES);
            for banned in ["@compute", "var<storage", "var<workgroup", "atomic<", "fx_slots", "nm_"] {
                prop_assert!(!program.source.split(nanatype_theme::SHADER_PRELUDE).next().unwrap().contains(banned), "{banned} in {source:?}");
            }
        }
    }

    /// Arbitrary text is just rejected.
    #[test]
    fn arbitrary_text_never_panics(source in "(?s).{0,600}") {
        let _ = on_small_stack(|| validate_shader(&source));
        let _ = on_small_stack(|| validate_material(&source));
    }
}
