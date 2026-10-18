//! Versioned, deliberately small WGSL authoring interface. No GPU is needed
//! to inspect a skin; only validated immutable programs leave this module.
use crate::{Severity, Skin, SkinDiagnostic, SkinFiles};
use naga::{Expression as E, Statement as S};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    sync::{Arc, LazyLock, Mutex, Weak},
};

// Weak entries never retain decoded textures after a skin is released.
type ValidatedPrograms = Vec<(blake3::Hash, Weak<ShaderProgram>)>;
static VALIDATED: LazyLock<Mutex<ValidatedPrograms>> = LazyLock::new(Default::default);

pub const SHADER_ABI: u32 = 1;
pub const SHADER_WRAPPER: &str = include_str!("shader_host.wgsl");
pub const MAX_SHADER_BYTES: usize = 64 * 1024;
pub const MAX_SHADER_NODES: usize = 4096;
pub const MAX_SHADER_CACHE_BYTES: usize = 64 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShaderSpec {
    pub source: String,
    pub period_s: f32,
    pub intensity: f32,
    pub textures: Vec<String>,
    pub params: Vec<f32>,
    /// Render the same geometry with a frozen clock when motion is disabled.
    pub static_fallback: bool,
    pub animated: bool,
}
impl Default for ShaderSpec {
    fn default() -> Self {
        Self {
            source: String::new(),
            period_s: 8.0,
            intensity: 0.18,
            textures: vec![],
            params: vec![],
            static_fallback: false,
            animated: true,
        }
    }
}
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ShaderTarget {
    #[default]
    Background,
    Border,
    /// Complete, skin-owned artwork inside the dialogue canvas. Unlike an
    /// overlay, its silhouette is defined by the shader rather than the frame.
    Artwork,
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ShaderEffect {
    pub shader: String,
    #[serde(default)]
    pub target: ShaderTarget,
}
#[derive(Debug, Clone, PartialEq)]
pub struct ShaderTexture {
    pub size: [u32; 2],
    pub bytes: Arc<[u8]>,
}
#[derive(Debug, Clone, PartialEq)]
pub struct ShaderProgram {
    pub hash: String,
    pub source: Arc<str>,
    pub textures: Vec<ShaderTexture>,
}

/// All bindings and helper bodies belong to the host. Authors may only call
/// helpers; direct references to these globals are rejected in user functions.
pub const SHADER_PRELUDE: &str = r#"
struct EffectInput {
    uv: vec2<f32>, size: vec2<f32>, phase: f32, seed: f32,
    line_age: f32, speaker_age: f32, reveal: f32, waiting: f32,
    base: vec4<f32>, accent: vec4<f32>, border: vec4<f32>, text: vec4<f32>,
    params0: vec4<f32>, params1: vec4<f32>,
};
@group(0) @binding(3) var fx_texture0: texture_2d<f32>;
@group(0) @binding(4) var fx_texture1: texture_2d<f32>;
@group(0) @binding(2) var fx_sampler: sampler;
fn sample0(uv: vec2<f32>) -> vec4<f32> { return textureSampleLevel(fx_texture0, fx_sampler, clamp(uv, vec2(0.), vec2(1.)), 0.); }
fn sample1(uv: vec2<f32>) -> vec4<f32> { return textureSampleLevel(fx_texture1, fx_sampler, clamp(uv, vec2(0.), vec2(1.)), 0.); }
fn particles(uv: vec2<f32>, phase: f32, seed: f32) -> f32 {
    var result = 0.;
    for (var n = 0u; n < 16u; n++) {
        let k = f32(n) + seed;
        let x = fract(sin(k * 127.1 + 31.7) * 43758.5453);
        let y = fract(k * 0.618034 - phase);
        result += 1. - smoothstep(0.002, 0.009, length(uv - vec2(x, y)));
    }
    return clamp(result, 0., 1.);
}
"#;

fn forbidden_directive(source: &str) -> bool {
    let b = source.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if b[i..].starts_with(b"//") {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if b[i..].starts_with(b"/*") {
            i += 2;
            let mut depth = 1;
            while i < b.len() && depth > 0 {
                if b[i..].starts_with(b"/*") {
                    depth += 1;
                    i += 2;
                } else if b[i..].starts_with(b"*/") {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
        } else if b[i].is_ascii_alphabetic() || b[i] == b'_' {
            let start = i;
            i += 1;
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'_') {
                i += 1;
            }
            if matches!(
                &b[start..i],
                b"enable" | b"requires" | b"diagnostic" | b"override"
            ) {
                return true;
            }
        } else {
            i += 1;
        }
    }
    false
}

fn block_cost(block: &naga::Block, module: &naga::Module, depth: usize) -> Result<usize, String> {
    let mut cost = 0usize;
    for stmt in block.iter() {
        cost += 1;
        cost += match stmt {
            S::Emit(_) | S::Return { .. } | S::Store { .. } | S::Break => 0,
            S::Block(b) => block_cost(b, module, depth)?,
            S::If { accept, reject, .. } => {
                block_cost(accept, module, depth)? + block_cost(reject, module, depth)?
            }
            S::Switch { cases, .. } => {
                let mut n = 0;
                for c in cases {
                    n += block_cost(&c.body, module, depth)?;
                }
                n
            }
            S::Call { function, .. } => {
                function_cost(&module.functions[*function], module, depth + 1)?
            }
            _ => {
                return Err(
                    "loops, discard, atomics, barriers and resource writes are not allowed".into(),
                );
            }
        };
        if cost > MAX_SHADER_NODES {
            return Err("expanded shader exceeds 4096 nodes".into());
        }
    }
    Ok(cost)
}
fn trusted(f: &naga::Function, module: &naga::Module) -> bool {
    match module.global_variables.len() {
        3 => matches!(f.name.as_deref(), Some("sample0" | "sample1" | "particles")),
        4 => matches!(
            f.name.as_deref(),
            Some("material_param" | "material_sample0" | "material_sample1")
        ),
        _ => false,
    }
}
fn function_cost(f: &naga::Function, m: &naga::Module, depth: usize) -> Result<usize, String> {
    if depth > 8 {
        return Err("shader call depth exceeds eight".into());
    }
    if trusted(f, m) {
        return Ok(if f.name.as_deref() == Some("particles") {
            512
        } else {
            32
        });
    }
    for (_, e) in f.expressions.iter() {
        if !matches!(
            e,
            E::Literal(_)
                | E::Constant(_)
                | E::ZeroValue(_)
                | E::Compose { .. }
                | E::Access { .. }
                | E::AccessIndex { .. }
                | E::Splat { .. }
                | E::Swizzle { .. }
                | E::FunctionArgument(_)
                | E::LocalVariable(_)
                | E::Load { .. }
                | E::Unary { .. }
                | E::Binary { .. }
                | E::Select { .. }
                | E::Relational { .. }
                | E::Math { .. }
                | E::As { .. }
                | E::CallResult(_)
        ) {
            return Err("only local arithmetic and host sampling helpers are allowed".into());
        }
    }
    let n = f.expressions.len() + block_cost(&f.body, m, depth)?;
    if n > MAX_SHADER_NODES {
        return Err("expanded shader exceeds 4096 nodes".into());
    }
    Ok(n)
}

/// Shared sandbox for the effect and material fragment contracts.
pub(crate) fn validate_fragment(
    source: &str,
    prelude: &str,
    entry: &str,
    input: &str,
    bindings: usize,
) -> Result<(naga::Module, String), String> {
    if source.len() > MAX_SHADER_BYTES {
        return Err("shader exceeds 64 KiB".into());
    }
    // WGSL directives are module-scoped and can weaken diagnostics. Reject
    // them as tokens (comments are harmless); IR checks remain authoritative.
    if forbidden_directive(source) {
        return Err("WGSL extensions, overrides and diagnostic directives are not allowed".into());
    }
    // Keep the author's source first so WGSL line/column diagnostics map
    // directly to the packaged file. WGSL module declarations allow forwards.
    let combined = format!("{source}\n{prelude}");
    let module =
        naga::front::wgsl::parse_str(&combined).map_err(|e| e.emit_to_string(&combined))?;
    if !module.entry_points.is_empty()
        || module.global_variables.len() != bindings
        || !module.overrides.is_empty()
    {
        return Err("custom entry points, globals, overrides and bindings are not allowed".into());
    }
    if module.types.len()
        + module.global_expressions.len()
        + module
            .functions
            .iter()
            .map(|(_, f)| f.expressions.len())
            .sum::<usize>()
        > MAX_SHADER_NODES
    {
        return Err("shader exceeds 4096 IR nodes".into());
    }
    for (handle, ty) in module.types.iter() {
        if matches!(
            ty.inner,
            naga::TypeInner::Pointer { .. } | naga::TypeInner::Atomic(_)
        ) {
            return Err("pointer interfaces and atomic types are not allowed".into());
        }
        if let naga::TypeInner::Array { size, .. } = ty.inner {
            // Runtime arrays are permitted only for the host's storage bindings.
            let valid = match size {
                naga::ArraySize::Constant(n) => n.get() <= 64,
                naga::ArraySize::Dynamic => {
                    module.global_variables.iter().any(|(_, v)| v.ty == handle)
                }
                _ => false,
            };
            if !valid {
                return Err("only fixed arrays of at most 64 elements are allowed".into());
            }
        }
    }
    let mut layouts = naga::proc::Layouter::default();
    layouts.update(module.to_ctx()).map_err(|e| e.to_string())?;
    if module.types.iter().any(|(h, _)| layouts[h].size > 4096) {
        return Err("individual shader types must fit 4096 bytes".into());
    }
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map_err(|e| e.emit_to_string(&combined))?;
    let (_, fragment) = module
        .functions
        .iter()
        .find(|(_, f)| f.name.as_deref() == Some(entry))
        .ok_or_else(|| format!("missing fn {entry}(input: {input}) -> vec4<f32>"))?;
    let valid_arg = fragment.arguments.len() == 1
        && module.types[fragment.arguments[0].ty].name.as_deref() == Some(input);
    let valid_result = fragment.result.as_ref().is_some_and(|r| {
        matches!(
            module.types[r.ty].inner,
            naga::TypeInner::Vector {
                size: naga::VectorSize::Quad,
                scalar: naga::Scalar {
                    kind: naga::ScalarKind::Float,
                    width: 4
                }
            }
        )
    });
    if !valid_arg || !valid_result {
        return Err(format!("expected fn {entry}(input: {input}) -> vec4<f32>"));
    }
    for (_, f) in module.functions.iter() {
        function_cost(f, &module, 1)?;
        if f.local_variables
            .iter()
            .map(|(_, v)| layouts[v.ty].size as usize)
            .sum::<usize>()
            > 16384
        {
            return Err("shader locals exceed 16 KiB".into());
        }
    }
    Ok((module, combined))
}

pub fn validate_shader(source: &str) -> Result<ShaderProgram, String> {
    let (module, combined) = validate_fragment(source, SHADER_PRELUDE, "effect", "EffectInput", 3)?;
    if module
        .functions
        .iter()
        .any(|(_, f)| f.name.as_ref().is_some_and(|n| n.starts_with("fx_")))
    {
        return Err("fx_ names are reserved for the host".into());
    }
    let complete = format!("{combined}\n{SHADER_WRAPPER}");
    let full = naga::front::wgsl::parse_str(&complete).map_err(|e| e.emit_to_string(&complete))?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&full)
    .map_err(|e| e.emit_to_string(&complete))?;
    Ok(ShaderProgram {
        hash: blake3::hash(combined.as_bytes()).to_hex().to_string(),
        source: combined.into(),
        textures: vec![],
    })
}

pub(crate) fn load_shaders(skin: &mut Skin, files: &dyn SkinFiles, out: &mut Vec<SkinDiagnostic>) {
    let mut errors = vec![];
    if (!skin.shaders.is_empty()
        || !skin.dialogue.shader_effects.is_empty()
        || skin.dialogue.decor.iter().any(|d| d.shader.is_some()))
        && skin.meta.schema != crate::SKIN_SCHEMA
    {
        errors.push((
            "meta.schema".to_owned(),
            "WGSL effects require the current schema".to_owned(),
        ));
    }
    let instances = skin.dialogue.shader_effects.len()
        + skin
            .dialogue
            .decor
            .iter()
            .filter(|d| d.shader.is_some())
            .count();
    if instances > 4 {
        errors.push((
            "dialogue.shader_effects".into(),
            "at most four shader instances per box".into(),
        ));
    }
    if let Some(responsive) = &skin.dialogue.responsive {
        for layout in responsive.layouts() {
            let effects = layout
                .shader_effects
                .as_ref()
                .unwrap_or(&skin.dialogue.shader_effects);
            let decor = layout.decor.as_ref().unwrap_or(&skin.dialogue.decor);
            if effects.len() + decor.iter().filter(|d| d.shader.is_some()).count() > 4 {
                errors.push((
                    "dialogue.responsive".into(),
                    "at most four shader instances per layout".into(),
                ));
            }
        }
    }
    if skin.shaders.len() > 16 {
        errors.push((
            "shaders".into(),
            "at most sixteen shader definitions".into(),
        ));
    }
    if !errors.is_empty() {
        out.extend(errors.into_iter().map(|(field, message)| SkinDiagnostic {
            severity: Severity::Error,
            field,
            message,
        }));
        return;
    }
    let mut programs = BTreeMap::new();
    let mut texture_bytes = 0usize;
    for (id, spec) in &skin.shaders {
        let result = (|| {
            if id.is_empty()
                || id.len() > 64
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
            {
                return Err("invalid shader id".into());
            }
            if !spec.period_s.is_finite()
                || !(0.5..=3600.).contains(&spec.period_s)
                || !spec.intensity.is_finite()
                || !(0.0..=1.).contains(&spec.intensity)
            {
                return Err("period_s must be 0.5..3600 and intensity 0..1".into());
            }
            if spec.params.len() > 8
                || spec.params.iter().any(|v| !v.is_finite())
                || spec.textures.len() > 2
            {
                return Err("at most eight finite parameters and two textures".into());
            }
            let path = crate::clean_resource_path(&spec.source).map_err(|e| e.to_string())?;
            let bytes = files.read(&path).map_err(|e| e.to_string())?;
            let source = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
            if bytes.len() > MAX_SHADER_BYTES {
                return Err("shader source exceeds 64 KiB".into());
            }
            let encoded: Vec<_> = spec
                .textures
                .iter()
                .map(|path| {
                    let path = crate::clean_resource_path(path).map_err(|e| e.to_string())?;
                    let bytes = files.read(&path).map_err(|e| e.to_string())?;
                    Ok((path, bytes))
                })
                .collect::<Result<_, String>>()?;
            let mut key = blake3::Hasher::new();
            key.update(&SHADER_ABI.to_le_bytes());
            key.update(&(bytes.len() as u64).to_le_bytes());
            key.update(&bytes);
            for (_, bytes) in &encoded {
                key.update(&(bytes.len() as u64).to_le_bytes());
                key.update(bytes);
            }
            let key = key.finalize();
            {
                let cache = VALIDATED.lock().unwrap_or_else(|e| e.into_inner());
                if let Some(program) = cache
                    .iter()
                    .find(|(k, _)| *k == key)
                    .and_then(|(_, p)| p.upgrade())
                {
                    texture_bytes += program
                        .textures
                        .iter()
                        .map(|t| t.bytes.len())
                        .sum::<usize>();
                    if texture_bytes > 32 * 1024 * 1024 {
                        return Err("effect textures exceed 32 MiB decoded".into());
                    }
                    return Ok(program);
                }
            }
            let mut program = validate_shader(source).map_err(|e| format!("{path}: {e}"))?;
            for (path, bytes) in encoded {
                let size = imagesize::blob_size(&bytes).map_err(|e| e.to_string())?;
                if size.width == 0 || size.height == 0 || size.width > 2048 || size.height > 2048 {
                    return Err("effect textures must fit 2048×2048".into());
                }
                texture_bytes += size.width * size.height * 4;
                if texture_bytes > 32 * 1024 * 1024 {
                    return Err("effect textures exceed 32 MiB decoded".into());
                }
                let mut reader = image::ImageReader::new(std::io::Cursor::new(&bytes))
                    .with_guessed_format()
                    .map_err(|e| e.to_string())?;
                let mut limits = image::Limits::default();
                limits.max_image_width = Some(2048);
                limits.max_image_height = Some(2048);
                limits.max_alloc = Some(32 * 1024 * 1024);
                reader.limits(limits);
                let decoded = reader
                    .decode()
                    .map_err(|e| format!("{path}: {e}"))?
                    .to_rgba8();
                if decoded.width() != size.width as u32 || decoded.height() != size.height as u32 {
                    return Err("texture dimensions changed during decoding".into());
                }
                program.textures.push(ShaderTexture {
                    size: [size.width as u32, size.height as u32],
                    bytes: decoded.into_raw().into(),
                });
            }
            let mut hash = blake3::Hasher::new();
            hash.update(program.source.as_bytes());
            hash.update(&SHADER_ABI.to_le_bytes());
            for t in &program.textures {
                hash.update(&t.size[0].to_le_bytes());
                hash.update(&t.size[1].to_le_bytes());
                hash.update(&t.bytes);
            }
            program.hash = hash.finalize().to_hex().to_string();
            let program = Arc::new(program);
            let mut cache = VALIDATED.lock().unwrap_or_else(|e| e.into_inner());
            cache.retain(|(_, p)| p.strong_count() > 0);
            if cache.len() >= 64 {
                cache.remove(0);
            }
            cache.push((key, Arc::downgrade(&program)));
            Ok(program)
        })();
        match result {
            Ok(p) => {
                programs.insert(id.clone(), p);
            }
            Err(e) => errors.push((format!("shaders.{id}"), e)),
        }
    }
    for id in skin
        .dialogue
        .shader_effects
        .iter()
        .map(|e| &e.shader)
        .chain(skin.dialogue.decor.iter().filter_map(|d| d.shader.as_ref()))
        .chain(
            skin.dialogue
                .responsive
                .iter()
                .flat_map(|r| r.layouts())
                .flat_map(|l| {
                    l.shader_effects
                        .iter()
                        .flatten()
                        .map(|e| &e.shader)
                        .chain(l.decor.iter().flatten().filter_map(|d| d.shader.as_ref()))
                }),
        )
    {
        if !skin.shaders.contains_key(id) {
            errors.push((
                "dialogue.shader_effects".into(),
                format!("unknown shader `{id}`"),
            ));
        }
    }
    skin.shader_programs = programs;
    out.extend(errors.into_iter().map(|(field, message)| SkinDiagnostic {
        severity: Severity::Error,
        field,
        message,
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    const OK: &str = "fn effect(i: EffectInput) -> vec4<f32> { return vec4(i.accent.rgb, particles(i.uv,i.phase,i.seed)); }";
    #[test]
    fn pure_functions_have_stable_hashes_and_host_entrypoints_validate() {
        assert!(
            validate_shader(&format!(
                "// enable extra light\n/* diagnostic /* override */ requires */\n{OK}"
            ))
            .is_ok()
        );
        let a = validate_shader(OK).unwrap();
        let b = validate_shader(OK).unwrap();
        assert_eq!(a, b);
        assert_ne!(
            a.hash,
            validate_shader(&OK.replace("i.seed", "0.5")).unwrap().hash
        );
        assert!(
            validate_shader("fn effect(i: EffectInput) -> vec4<f32> { return sample0(i.uv); }")
                .is_ok()
        );
    }
    #[test]
    fn rejects_untrusted_control_flow_bindings_and_interfaces() {
        for source in [
            "fn effect(i: EffectInput) -> vec4<f32> { loop {} return vec4(0.); }",
            "fn effect(i: EffectInput) -> vec4<f32> { var x=0; for(var n=0;n<2;n++){x++;} return vec4(f32(x)); }",
            "fn effect(i: EffectInput) -> vec4<f32> { var x=0; while(x<2){x++;} return vec4(f32(x)); }",
            "fn effect(i: EffectInput) -> vec4<f32> { discard; return vec4(0.); }",
            "@fragment fn effect() -> @location(0) vec4<f32> { return vec4(0.); }",
            "@group(1) @binding(0) var custom: texture_2d<f32>; fn effect(i:EffectInput)->vec4<f32>{return vec4(0.);}",
            "var<private> x: f32; fn effect(i:EffectInput)->vec4<f32>{return vec4(x);}",
            "fn effect(i:EffectInput)->vec4<f32>{return textureSampleLevel(fx_texture0,fx_sampler,i.uv,0.);}",
            "fn effect(i:EffectInput)->vec4<f32>{var a:array<f32,1000000>;return vec4(a[0]);}",
            "fn effect(i:f32)->vec4<f32>{return vec4(i);}",
            "fn effect(i:EffectInput)->f32{return 0.;}",
            "diagnostic(off, derivative_uniformity); fn effect(i:EffectInput)->vec4<f32>{return vec4(0.);}",
        ] {
            assert!(validate_shader(source).is_err(), "accepted {source}");
        }
    }
    #[test]
    fn expanded_calls_and_depth_cannot_hide_work() {
        let mut source =
            "fn f0(i:EffectInput)->vec4<f32>{return vec4(particles(i.uv,i.phase,i.seed));}\n"
                .to_owned();
        for n in 1..5 {
            source += &format!(
                "fn f{n}(i:EffectInput)->vec4<f32>{{return f{}(i)+f{}(i);}}\n",
                n - 1,
                n - 1
            );
        }
        source += "fn effect(i:EffectInput)->vec4<f32>{return f4(i);}";
        assert!(validate_shader(&source).unwrap_err().contains("4096"));
        let mut source = "fn f0(i:EffectInput)->vec4<f32>{return vec4(0.);}\n".to_owned();
        for n in 1..9 {
            source += &format!(
                "fn f{n}(i:EffectInput)->vec4<f32>{{return f{}(i);}}\n",
                n - 1
            );
        }
        source += "fn effect(i:EffectInput)->vec4<f32>{return f8(i);}";
        assert!(validate_shader(&source).unwrap_err().contains("depth"));
        assert!(validate_shader(&" ".repeat(MAX_SHADER_BYTES + 1)).is_err());
    }
}
