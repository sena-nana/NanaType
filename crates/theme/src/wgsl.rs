//! Unified, versioned WGSL authoring sandbox for effect shaders and materials.
//! No GPU is needed to inspect a skin; only validated immutable programs leave
//! this module.

use crate::{Skin, SkinDiagnostic, SkinFiles};
use naga::{Expression as E, Statement as S};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, VecDeque},
    panic::{AssertUnwindSafe, catch_unwind},
    sync::{Arc, LazyLock, Mutex, OnceLock, Weak, mpsc},
};

/// 2: `param(i)` reads skin-param slots bound at `@binding(5)`.
pub const SHADER_ABI: u32 = 2;
/// Slots a shader may bind through `bindings`, one `vec4` each.
pub const MAX_SHADER_SLOTS: usize = 64;
pub const SHADER_WRAPPER: &str = include_str!("shader_host.wgsl");
pub const MAX_SHADER_BYTES: usize = 64 * 1024;
pub const MAX_SHADER_NODES: usize = 4096;
/// Operand chains such as `a+a+…+a` stay shallow: platform shader compilers
/// walk expression trees recursively and a deep one exhausts their stack.
pub const MAX_EXPRESSION_DEPTH: usize = 64;
/// Nested `if`/`switch`/`{}` levels inside one function.
pub const MAX_BLOCK_DEPTH: usize = 32;
/// Operators, calls, commas, member accesses and indexing between two
/// `;`/`{`/`}`. Naga parses and lowers expressions recursively (a postfix
/// chain such as `a.b.b.b…` or `a[0][0]…` included), so this bounds its stack
/// use before the source reaches it; the IR depth limit then applies to the
/// lowered tree.
pub const MAX_STATEMENT_OPERATORS: usize = 256;
/// Naga runs on a dedicated reusable worker thread with this stack.
const VALIDATOR_STACK: usize = 32 << 20;
pub const MAX_SHADER_CACHE_BYTES: usize = 64 * 1024 * 1024;
const MAX_TEXTURE_SIDE: usize = 2048;
const MAX_DECODED_TEXTURE_BYTES: usize = 32 * 1024 * 1024;
const MAX_CACHED_PROGRAMS: usize = 128;

/// `EffectInput::phase`, the loop clock.
pub const INPUT_PHASE: u32 = 1 << 2;
/// `line_age`, `speaker_age`, `reveal` and `waiting`.
pub const INPUT_EVENTS: u32 = 0b1111 << 4;

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
struct FxSlots { v: array<vec4<f32>, 64> };
@group(0) @binding(5) var<uniform> fx_slots: FxSlots;
fn param(i: u32) -> vec4<f32> { return fx_slots.v[min(i, 63u)]; }
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
    /// Skin params and literals packed into `param(i)`, one `vec4` per entry.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub bindings: Vec<SlotBinding>,
    /// An artwork that draws its own continue indicator while it plays; the
    /// host's `dialogue.continue` shows whenever the program is frozen or off.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub owns_continue: bool,
    /// When an animated program's pixels may change; empty means always.
    /// Outside every listed condition its output must not depend on time,
    /// and the host requests no frames for it.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub active: Vec<ShaderActive>,
    /// Changes per second of a stepped animation; 0 is continuous. The clock
    /// advances in these steps and the host wakes only at the next one.
    #[serde(skip_serializing_if = "is_zero")]
    pub cadence: f32,
}

fn is_zero(v: &f32) -> bool {
    *v == 0.
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
            bindings: vec![],
            owns_continue: false,
            active: vec![],
            cadence: 0.,
        }
    }
}

impl ShaderSpec {
    /// Every param id the bindings name.
    pub fn bound_params(&self) -> impl Iterator<Item = &str> {
        self.bindings.iter().flat_map(|b| {
            let (one, many): (Option<&str>, &[SlotComponent]) = match b {
                SlotBinding::Param(id) => (Some(id), &[]),
                SlotBinding::Number(_) => (None, &[]),
                SlotBinding::Tuple(parts) => (None, parts),
            };
            one.into_iter().chain(many.iter().filter_map(|c| match c {
                SlotComponent::Param(id) => Some(id.as_str()),
                SlotComponent::Number(_) => None,
            }))
        })
    }
}

/// One `active` condition: `always`, `typing` (the line is still revealing),
/// `waiting` (it waits for the reader), `line:<s>` / `speaker:<s>` (seconds
/// after a new line or speaker).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub enum ShaderActive {
    Always,
    Typing,
    Waiting,
    Line(f32),
    Speaker(f32),
}

impl TryFrom<String> for ShaderActive {
    type Error = String;
    fn try_from(s: String) -> Result<Self, String> {
        let seconds = |v: &str| {
            v.parse::<f32>()
                .ok()
                .filter(|v| v.is_finite() && *v > 0. && *v <= 60.)
                .ok_or_else(|| format!("`{s}`: seconds must be 0..60"))
        };
        Ok(match s.split_once(':') {
            None if s == "always" => Self::Always,
            None if s == "typing" => Self::Typing,
            None if s == "waiting" => Self::Waiting,
            Some(("line", v)) => Self::Line(seconds(v)?),
            Some(("speaker", v)) => Self::Speaker(seconds(v)?),
            _ => {
                return Err(format!(
                    "`{s}`: expected always, typing, waiting, line:<s> or speaker:<s>"
                ));
            }
        })
    }
}

impl From<ShaderActive> for String {
    fn from(a: ShaderActive) -> Self {
        match a {
            ShaderActive::Always => "always".into(),
            ShaderActive::Typing => "typing".into(),
            ShaderActive::Waiting => "waiting".into(),
            ShaderActive::Line(s) => format!("line:{s}"),
            ShaderActive::Speaker(s) => format!("speaker:{s}"),
        }
    }
}

/// One `bindings` entry. A param fills `x` (numbers, bools as 0/1, enums as
/// the option index) or all of `rgba` (colours, linear and straight alpha);
/// a list packs up to four scalar params or literals into `xyzw`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SlotBinding {
    Param(String),
    Number(f32),
    Tuple(Vec<SlotComponent>),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum SlotComponent {
    Param(String),
    Number(f32),
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

/// Built only by this crate's validators, so every program a renderer sees
/// has passed the sandbox.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct ShaderProgram {
    pub hash: String,
    pub source: Arc<str>,
    pub textures: Vec<ShaderTexture>,
    /// Expanded per-pixel cost of the entry point, for runtime budgets.
    pub cost: u32,
    /// Bit `n` is set when the program may read field `n` of its input
    /// struct (all bits when it cannot tell).
    pub inputs: u32,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MaterialSpec {
    pub source: String,
    pub params: Vec<f32>,
    /// Image files or pipeline texture outputs, one per slot.
    pub textures: Vec<crate::TextureSource>,
    /// Pipeline `vec4` outputs, placed after the static params.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub uniforms: Vec<crate::PipelineOutput>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProgramKind {
    Effect,
    Material,
}

struct ProgramCache {
    entries: VecDeque<(blake3::Hash, Weak<ShaderProgram>)>,
}

impl ProgramCache {
    const fn new() -> Self {
        Self {
            entries: VecDeque::new(),
        }
    }

    fn get(&mut self, key: &blake3::Hash) -> Option<Arc<ShaderProgram>> {
        self.entries.retain(|(_, w)| w.strong_count() > 0);
        let pos = self.entries.iter().position(|(k, _)| k == key)?;
        let (k, w) = self.entries.remove(pos)?;
        let program = w.upgrade()?;
        self.entries.push_back((k, w));
        Some(program)
    }

    fn insert(&mut self, key: blake3::Hash, program: &Arc<ShaderProgram>) {
        self.entries
            .retain(|(k, w)| w.strong_count() > 0 && k != &key);
        while self.entries.len() >= MAX_CACHED_PROGRAMS {
            self.entries.pop_front();
        }
        self.entries.push_back((key, Arc::downgrade(program)));
    }
}

static VALIDATED: LazyLock<Mutex<ProgramCache>> = LazyLock::new(|| Mutex::new(ProgramCache::new()));

type ValidatorJob = Box<dyn FnOnce() + Send + 'static>;
static VALIDATOR_TX: Mutex<Option<mpsc::Sender<ValidatorJob>>> = Mutex::new(None);

fn spawn_validator_worker() -> Result<mpsc::Sender<ValidatorJob>, String> {
    let (tx, rx) = mpsc::channel::<ValidatorJob>();
    std::thread::Builder::new()
        .name("wgsl-validator".into())
        .stack_size(VALIDATOR_STACK)
        .spawn(move || {
            for job in rx {
                let _ = catch_unwind(AssertUnwindSafe(job));
            }
        })
        .map_err(|e| e.to_string())?;
    Ok(tx)
}

/// Runs a naga pass on the single lazy reusable 32 MiB validator worker thread,
/// isolating caller stacks and catching panics per shader.
fn run_on_validator<T: Send + 'static>(
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, String> {
    let (reply_tx, reply_rx) = mpsc::sync_channel::<Result<T, String>>(1);
    let make_job = |f: Box<dyn FnOnce() -> Result<T, String> + Send + 'static>| -> ValidatorJob {
        Box::new(move || {
            let res = catch_unwind(AssertUnwindSafe(f))
                .unwrap_or_else(|_| Err("WGSL validator failed".into()));
            let _ = reply_tx.send(res);
        })
    };

    let mut guard = VALIDATOR_TX.lock().unwrap_or_else(|e| e.into_inner());
    if guard.is_none() {
        *guard = Some(spawn_validator_worker()?);
    }
    let tx = guard.as_ref().expect("worker spawned").clone();
    drop(guard);

    if tx.send(make_job(Box::new(f))).is_err() {
        // Worker died unexpectedly; respawn once.
        return Err("WGSL validator failed".into());
    }
    reply_rx
        .recv()
        .unwrap_or_else(|_| Err("WGSL validator failed".into()))
}

/// Lexical pre-check of the author's source (comments are skipped). It only
/// gives early, readable errors; the IR checks below remain authoritative.
fn precheck(source: &str) -> Result<(), String> {
    let b = source.as_bytes();
    if let Some(c) = b
        .iter()
        .find(|c| c.is_ascii_control() && !matches!(**c, b'\t' | b'\n' | b'\r'))
    {
        return Err(format!("control character 0x{c:02x} is not allowed"));
    }
    let mut i = 0;
    let mut operators = 0;
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
            let token = &source[start..i];
            // WGSL directives are module-scoped and can weaken diagnostics.
            if matches!(token, "enable" | "requires" | "diagnostic" | "override") {
                return Err(
                    "WGSL extensions, overrides and diagnostic directives are not allowed".into(),
                );
            }
            if matches!(
                token,
                "compute"
                    | "workgroup_size"
                    | "workgroup"
                    | "storage"
                    | "ptr"
                    | "textureStore"
                    | "workgroupBarrier"
                    | "storageBarrier"
                    | "textureBarrier"
                    | "workgroupUniformLoad"
                    | "arrayLength"
            ) || ["atomic", "texture_storage_", "subgroup"]
                .iter()
                .any(|p| token.starts_with(p))
            {
                return Err(format!(
                    "`{token}`: compute, storage, pointer, atomic and barrier features are not allowed"
                ));
            }
            if ["fx_", "nm_", "__"].iter().any(|p| token.starts_with(p)) {
                return Err(format!(
                    "`{token}`: fx_, nm_ and __ names are reserved for the host"
                ));
            }
        } else {
            if matches!(b[i], b';' | b'{' | b'}') {
                operators = 0;
            } else if b"+-*/%&|^<>!~(,[".contains(&b[i])
                // Member access, not the point of a number literal.
                || (b[i] == b'.' && b.get(i + 1).is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_'))
            {
                operators += 1;
                if operators > MAX_STATEMENT_OPERATORS {
                    return Err(format!(
                        "a statement has more than {MAX_STATEMENT_OPERATORS} operators; split it with `let`"
                    ));
                }
            }
            i += 1;
        }
    }
    Ok(())
}

/// Longest operand chain in a function whose expressions passed the
/// whitelist. Naga handles only point backwards, so one forward pass works.
fn expression_depth(f: &naga::Function) -> usize {
    let mut depth = vec![0usize; f.expressions.len()];
    let mut max = 0;
    for (h, e) in f.expressions.iter() {
        let mut d = 0;
        let mut see = |o: naga::Handle<naga::Expression>| d = d.max(depth[o.index()]);
        match e {
            E::Compose { components, .. } => components.iter().copied().for_each(&mut see),
            E::Access { base, index } => {
                see(*base);
                see(*index);
            }
            E::AccessIndex { base, .. } => see(*base),
            E::Splat { value, .. } => see(*value),
            E::Swizzle { vector, .. } => see(*vector),
            E::Load { pointer } => see(*pointer),
            E::Unary { expr, .. } | E::As { expr, .. } => see(*expr),
            E::Relational { argument, .. } => see(*argument),
            E::Binary { left, right, .. } => {
                see(*left);
                see(*right);
            }
            E::Select {
                condition,
                accept,
                reject,
            } => {
                see(*condition);
                see(*accept);
                see(*reject);
            }
            E::Math {
                arg,
                arg1,
                arg2,
                arg3,
                ..
            } => {
                see(*arg);
                [arg1, arg2, arg3]
                    .into_iter()
                    .flatten()
                    .copied()
                    .for_each(&mut see);
            }
            _ => {}
        }
        depth[h.index()] = d + 1;
        max = max.max(d + 1);
    }
    max
}

type Helpers = &'static [(&'static str, usize)];
const EFFECT_HELPERS: Helpers = &[
    ("sample0", 32),
    ("sample1", 32),
    ("particles", 512),
    ("param", 4),
];
const MATERIAL_HELPERS: Helpers = &[
    ("material_param", 32),
    ("material_sample0", 32),
    ("material_sample1", 32),
];

fn block_cost(
    block: &naga::Block,
    module: &naga::Module,
    depth: usize,
    nesting: usize,
    helpers: Helpers,
    limit: usize,
) -> Result<usize, String> {
    if nesting > MAX_BLOCK_DEPTH {
        return Err(format!("blocks nest deeper than {MAX_BLOCK_DEPTH} levels"));
    }
    let inner = nesting + 1;
    let mut cost = 0usize;
    for stmt in block.iter() {
        cost += 1;
        cost += match stmt {
            S::Emit(_) | S::Return { .. } | S::Store { .. } | S::Break => 0,
            S::Block(b) => block_cost(b, module, depth, inner, helpers, limit)?,
            S::If { accept, reject, .. } => {
                block_cost(accept, module, depth, inner, helpers, limit)?
                    + block_cost(reject, module, depth, inner, helpers, limit)?
            }
            S::Switch { cases, .. } => {
                let mut n = 0;
                for c in cases {
                    n += block_cost(&c.body, module, depth, inner, helpers, limit)?;
                }
                n
            }
            S::Call { function, .. } => function_cost(
                &module.functions[*function],
                module,
                depth + 1,
                helpers,
                limit,
            )?,
            _ => {
                return Err(
                    "loops, discard, atomics, barriers and resource writes are not allowed".into(),
                );
            }
        };
        if cost > limit {
            return Err("expanded shader exceeds 4096 nodes".into());
        }
    }
    Ok(cost)
}

fn function_cost(
    f: &naga::Function,
    m: &naga::Module,
    depth: usize,
    helpers: Helpers,
    limit: usize,
) -> Result<usize, String> {
    if depth > 8 {
        return Err("shader call depth exceeds eight".into());
    }
    if let Some((_, cost)) = helpers.iter().find(|(n, _)| f.name.as_deref() == Some(*n)) {
        return Ok(*cost);
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
    let n = f.expressions.len() + block_cost(&f.body, m, depth, 0, helpers, limit)?;
    if n > limit {
        return Err("expanded shader exceeds 4096 nodes".into());
    }
    Ok(n)
}

type HostGlobal = (
    Option<String>,
    naga::AddressSpace,
    Option<naga::ResourceBinding>,
);

struct HostSpec {
    globals: Vec<HostGlobal>,
    /// Extra IR nodes added by the wrapper (0 for materials).
    wrapper_nodes: usize,
}

fn module_ir_nodes(module: &naga::Module) -> usize {
    module.types.len()
        + module.global_expressions.len()
        + module
            .functions
            .iter()
            .map(|(_, f)| f.expressions.len())
            .sum::<usize>()
}

fn host_spec(kind: ProgramKind) -> Result<&'static HostSpec, String> {
    static EFFECT_HOST: OnceLock<Result<HostSpec, String>> = OnceLock::new();
    static MATERIAL_HOST: OnceLock<Result<HostSpec, String>> = OnceLock::new();
    let slot = match kind {
        ProgramKind::Effect => &EFFECT_HOST,
        ProgramKind::Material => &MATERIAL_HOST,
    };
    slot.get_or_init(|| match kind {
        ProgramKind::Effect => {
            let stub = "fn effect(i: EffectInput) -> vec4<f32> { return vec4(0.); }";
            let with_stub = format!("{stub}\n{SHADER_PRELUDE}");
            let base = naga::front::wgsl::parse_str(&with_stub)
                .map_err(|e| e.emit_to_string(&with_stub))?;
            let full_src = format!("{with_stub}\n{SHADER_WRAPPER}");
            let full =
                naga::front::wgsl::parse_str(&full_src).map_err(|e| e.emit_to_string(&full_src))?;
            let globals = full
                .global_variables
                .iter()
                .map(|(_, v)| (v.name.clone(), v.space, v.binding))
                .collect();
            let wrapper_nodes = module_ir_nodes(&full).saturating_sub(module_ir_nodes(&base));
            Ok(HostSpec {
                globals,
                wrapper_nodes,
            })
        }
        ProgramKind::Material => {
            let module = naga::front::wgsl::parse_str(nana_ui_core::MATERIAL_ABI)
                .map_err(|e| e.emit_to_string(nana_ui_core::MATERIAL_ABI))?;
            let globals = module
                .global_variables
                .iter()
                .map(|(_, v)| (v.name.clone(), v.space, v.binding))
                .collect();
            Ok(HostSpec {
                globals,
                wrapper_nodes: 0,
            })
        }
    })
    .as_ref()
    .map_err(Clone::clone)
}

fn is_wrapper_fn(kind: ProgramKind, name: Option<&str>) -> bool {
    kind == ProgramKind::Effect && matches!(name, Some("fx_vertex" | "fx_fragment"))
}

fn input_fields(module: &naga::Module, kind: ProgramKind, input: &str) -> u32 {
    let is_input = |ty: naga::Handle<naga::Type>| module.types[ty].name.as_deref() == Some(input);
    let mut read = 0;
    for (_, f) in module.functions.iter() {
        if is_wrapper_fn(kind, f.name.as_deref()) {
            continue;
        }
        if f.local_variables.iter().any(|(_, v)| is_input(v.ty)) {
            return u32::MAX;
        }
        for (_, e) in f.expressions.iter() {
            if let E::AccessIndex { base, index } = *e
                && let E::FunctionArgument(a) = f.expressions[base]
                && is_input(f.arguments[a as usize].ty)
            {
                read |= 1u32.checked_shl(index).unwrap_or(0);
            }
        }
    }
    read
}

fn validate_program(source: &str, kind: ProgramKind) -> Result<ShaderProgram, String> {
    if source.len() > MAX_SHADER_BYTES {
        return Err("shader exceeds 64 KiB".into());
    }
    precheck(source)?;
    let owned_source = source.to_owned();
    let (stored_source, cost, inputs) = run_on_validator(move || {
        let (entry, input, helpers, combined, stored_len) = match kind {
            ProgramKind::Effect => {
                let complete = format!("{owned_source}\n{SHADER_PRELUDE}\n{SHADER_WRAPPER}");
                let stored_len = owned_source.len() + 1 + SHADER_PRELUDE.len();
                (
                    "effect",
                    "EffectInput",
                    EFFECT_HELPERS,
                    complete,
                    stored_len,
                )
            }
            ProgramKind::Material => {
                let combined = format!("{owned_source}\n{}", nana_ui_core::MATERIAL_ABI);
                let stored_len = owned_source.len();
                (
                    "material",
                    "MaterialInput",
                    MATERIAL_HELPERS,
                    combined,
                    stored_len,
                )
            }
        };
        let module = naga::front::wgsl::parse_str(&combined).map_err(|e| {
            let msg = e.emit_to_string(&combined);
            if kind == ProgramKind::Effect
                && msg.contains("no definition in scope for identifier: `effect`")
                && naga::front::wgsl::parse_str(&combined[..stored_len]).is_ok()
            {
                return format!("missing fn {entry}(input: {input}) -> vec4<f32>");
            }
            msg
        })?;
        let host = host_spec(kind)?;
        let entries_ok = match kind {
            ProgramKind::Effect => {
                let mut stages: Vec<_> = module
                    .entry_points
                    .iter()
                    .map(|e| (e.name.as_str(), e.stage))
                    .collect();
                stages.sort_by_key(|(name, _)| *name);
                stages
                    == [
                        ("fx_fragment", naga::ShaderStage::Fragment),
                        ("fx_vertex", naga::ShaderStage::Vertex),
                    ]
            }
            ProgramKind::Material => module.entry_points.is_empty(),
        };
        if !entries_ok
            || !module.overrides.is_empty()
            || module.global_variables.len() != host.globals.len()
            || module
                .global_variables
                .iter()
                .any(|(_, v)| !host.globals.contains(&(v.name.clone(), v.space, v.binding)))
        {
            return Err(
                "custom entry points, globals, overrides and bindings are not allowed".into(),
            );
        }
        if module_ir_nodes(&module).saturating_sub(host.wrapper_nodes) > MAX_SHADER_NODES {
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
        let (entry_handle, fragment) = module
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
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .map_err(|e| e.emit_to_string(&combined))?;
        let mut cost = 0;
        for (handle, f) in module.functions.iter() {
            if is_wrapper_fn(kind, f.name.as_deref()) {
                continue;
            }
            let n = function_cost(f, &module, 1, helpers, MAX_SHADER_NODES)?;
            if handle == entry_handle {
                cost = n;
            }
            let helper = helpers.iter().any(|(n, _)| f.name.as_deref() == Some(*n));
            if !helper && expression_depth(f) > MAX_EXPRESSION_DEPTH {
                return Err(format!(
                    "an expression nests deeper than {MAX_EXPRESSION_DEPTH} operations; split it with `let`"
                ));
            }
            if f.local_variables
                .iter()
                .map(|(_, v)| layouts[v.ty].size as usize)
                .sum::<usize>()
                > 16384
            {
                return Err("shader locals exceed 16 KiB".into());
            }
        }
        let inputs = input_fields(&module, kind, input);
        let stored_source: Arc<str> = Arc::from(&combined[..stored_len]);
        Ok((stored_source, cost, inputs))
    })?;

    let mut hasher = blake3::Hasher::new();
    match kind {
        ProgramKind::Effect => {
            hasher.update(stored_source.as_bytes());
        }
        ProgramKind::Material => {
            hasher.update(stored_source.as_bytes());
            hasher.update(b"\n");
            hasher.update(nana_ui_core::MATERIAL_ABI.as_bytes());
        }
    }
    Ok(ShaderProgram {
        hash: hasher.finalize().to_hex().to_string(),
        source: stored_source,
        textures: vec![],
        cost: cost as u32,
        inputs,
    })
}

pub fn validate_shader(source: &str) -> Result<ShaderProgram, String> {
    validate_program(source, ProgramKind::Effect)
}

pub fn validate_material(source: &str) -> Result<ShaderProgram, String> {
    validate_program(source, ProgramKind::Material)
}

/// IR nodes, the expanded cost (every call counts the callee again) and the
/// deepest expression that the sandbox limits measure, without enforcing
/// them: for authors sizing an effect. Parses but does not validate.
pub fn shader_cost(source: &str) -> Result<(usize, usize, usize), String> {
    let owned = source.to_owned();
    run_on_validator(move || {
        let combined = format!("{owned}\n{SHADER_PRELUDE}");
        let module =
            naga::front::wgsl::parse_str(&combined).map_err(|e| e.emit_to_string(&combined))?;
        let ir = module_ir_nodes(&module);
        let (_, entry) = module
            .functions
            .iter()
            .find(|(_, f)| f.name.as_deref() == Some("effect"))
            .ok_or("missing fn effect")?;
        let depth = module
            .functions
            .iter()
            .filter(|(_, f)| {
                !EFFECT_HELPERS
                    .iter()
                    .any(|(n, _)| f.name.as_deref() == Some(*n))
            })
            .map(|(_, f)| expression_depth(f))
            .max()
            .unwrap_or(0);
        Ok((
            ir,
            function_cost(entry, &module, 1, EFFECT_HELPERS, usize::MAX)?,
            depth,
        ))
    })
}

fn check_bindings(
    bindings: &[SlotBinding],
    params: &BTreeMap<String, crate::ParamSpec>,
) -> Result<(), String> {
    use crate::ParamType;
    if bindings.len() > MAX_SHADER_SLOTS {
        return Err(format!("at most {MAX_SHADER_SLOTS} bindings"));
    }
    let kind = |id: &str| {
        params
            .get(id)
            .map(|p| p.kind)
            .ok_or_else(|| format!("binding names `{id}`, which is not a declared param"))
    };
    for (i, b) in bindings.iter().enumerate() {
        match b {
            SlotBinding::Param(id) => {
                if kind(id)? == ParamType::Text {
                    return Err(format!("binding {i}: text params cannot be bound"));
                }
            }
            SlotBinding::Number(v) if !v.is_finite() => {
                return Err(format!("binding {i} is not finite"));
            }
            SlotBinding::Number(_) => {}
            SlotBinding::Tuple(parts) => {
                if !(1..=4).contains(&parts.len()) {
                    return Err(format!("binding {i}: a list packs 1–4 values"));
                }
                for part in parts {
                    match part {
                        SlotComponent::Param(id) => {
                            if matches!(kind(id)?, ParamType::Text | ParamType::Color) {
                                return Err(format!(
                                    "binding {i}: `{id}` is not a scalar; bind colours on their own"
                                ));
                            }
                        }
                        SlotComponent::Number(v) if !v.is_finite() => {
                            return Err(format!("binding {i} is not finite"));
                        }
                        SlotComponent::Number(_) => {}
                    }
                }
            }
        }
    }
    Ok(())
}

fn valid_program_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'-'))
}

fn cache_key(
    kind: ProgramKind,
    source: &[u8],
    encoded: &[Option<(String, Vec<u8>)>],
) -> blake3::Hash {
    let mut key = blake3::Hasher::new();
    match kind {
        ProgramKind::Effect => {
            key.update(b"effect\0");
            key.update(&SHADER_ABI.to_le_bytes());
        }
        ProgramKind::Material => {
            key.update(b"material\0");
            key.update(nana_ui_core::MATERIAL_ABI.as_bytes());
        }
    }
    key.update(&(source.len() as u64).to_le_bytes());
    key.update(source);
    for slot in encoded {
        match slot {
            Some((_, bytes)) => {
                key.update(b"\x01");
                key.update(&(bytes.len() as u64).to_le_bytes());
                key.update(bytes);
            }
            None => {
                key.update(b"\x00pipeline");
            }
        }
    }
    key.finalize()
}

fn decode_textures(
    kind: ProgramKind,
    encoded: Vec<Option<(String, Vec<u8>)>>,
    texture_bytes: &mut usize,
) -> Result<Vec<ShaderTexture>, String> {
    let (dim_err, budget_err) = match kind {
        ProgramKind::Effect => (
            "effect textures must fit 2048×2048",
            "effect textures exceed 32 MiB decoded",
        ),
        ProgramKind::Material => (
            "material textures must fit 2048x2048",
            "material textures exceed 32 MiB decoded",
        ),
    };
    let mut textures = Vec::with_capacity(encoded.len());
    for slot in encoded {
        let Some((path, bytes)) = slot else {
            textures.push(ShaderTexture {
                size: [1, 1],
                bytes: Arc::from([0u8; 4]),
            });
            continue;
        };
        let size = imagesize::blob_size(&bytes).map_err(|e| e.to_string())?;
        if size.width == 0
            || size.height == 0
            || size.width > MAX_TEXTURE_SIDE
            || size.height > MAX_TEXTURE_SIDE
        {
            return Err(dim_err.into());
        }
        *texture_bytes += size.width * size.height * 4;
        if *texture_bytes > MAX_DECODED_TEXTURE_BYTES {
            return Err(budget_err.into());
        }
        let mut reader = image::ImageReader::new(std::io::Cursor::new(&bytes))
            .with_guessed_format()
            .map_err(|e| e.to_string())?;
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(MAX_TEXTURE_SIDE as u32);
        limits.max_image_height = Some(MAX_TEXTURE_SIDE as u32);
        limits.max_alloc = Some(MAX_DECODED_TEXTURE_BYTES as u64);
        reader.limits(limits);
        let decoded = reader
            .decode()
            .map_err(|e| format!("{path}: {e}"))?
            .to_rgba8();
        if decoded.width() != size.width as u32 || decoded.height() != size.height as u32 {
            return Err("texture dimensions changed during decoding".into());
        }
        textures.push(ShaderTexture {
            size: [decoded.width(), decoded.height()],
            bytes: decoded.into_raw().into(),
        });
    }
    Ok(textures)
}

fn wgsl_diagnostic(field: String, source_path: Option<String>, message: String) -> SkinDiagnostic {
    let mut diag = SkinDiagnostic::error(field, message.clone());
    if let Some(file) = source_path {
        if let Some((line, column)) = message.lines().find_map(|l| {
            let (_, p) = l.split_once("wgsl:")?;
            let mut p = p.split(':');
            Some((
                p.next()?.parse::<usize>().ok()?,
                p.next()?.trim().parse::<usize>().ok()?,
            ))
        }) {
            diag = diag.with_target(crate::DiagnosticTarget::Source { file, line, column });
        } else {
            diag = diag.with_target(crate::DiagnosticTarget::Source {
                file,
                line: 1,
                column: 1,
            });
        }
    }
    diag
}

fn load_program_cached(
    kind: ProgramKind,
    source_path: &str,
    encoded_textures: Vec<Option<(String, Vec<u8>)>>,
    files: &dyn SkinFiles,
    texture_bytes: &mut usize,
) -> Result<Arc<ShaderProgram>, (Option<String>, String)> {
    let path = crate::clean_resource_path(source_path).map_err(|e| (None, e.to_string()))?;
    let bytes = files.read(&path).map_err(|e| (None, e.to_string()))?;
    if bytes.len() > MAX_SHADER_BYTES {
        return Err((Some(path), "shader source exceeds 64 KiB".into()));
    }
    let source = std::str::from_utf8(&bytes).map_err(|e| (Some(path.clone()), e.to_string()))?;
    let key = cache_key(kind, &bytes, &encoded_textures);
    {
        let mut cache = VALIDATED.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(program) = cache.get(&key) {
            *texture_bytes += program
                .textures
                .iter()
                .map(|t| t.bytes.len())
                .sum::<usize>();
            if *texture_bytes > MAX_DECODED_TEXTURE_BYTES {
                return Err((
                    None,
                    match kind {
                        ProgramKind::Effect => "effect textures exceed 32 MiB decoded",
                        ProgramKind::Material => "material textures exceed 32 MiB decoded",
                    }
                    .into(),
                ));
            }
            return Ok(program);
        }
    }
    let mut program =
        validate_program(source, kind).map_err(|e| (Some(path.clone()), format!("{path}: {e}")))?;
    program.textures =
        decode_textures(kind, encoded_textures, texture_bytes).map_err(|e| (None, e))?;
    program.hash = {
        let mut h = blake3::Hasher::new();
        match kind {
            ProgramKind::Effect => {
                h.update(program.source.as_bytes());
                h.update(&SHADER_ABI.to_le_bytes());
            }
            ProgramKind::Material => {
                h.update(b"material\0");
                h.update(nana_ui_core::MATERIAL_ABI.as_bytes());
                h.update(program.source.as_bytes());
            }
        }
        for texture in &program.textures {
            h.update(&texture.size[0].to_le_bytes());
            h.update(&texture.size[1].to_le_bytes());
            h.update(&texture.bytes);
        }
        h.finalize().to_hex().to_string()
    };
    let program = Arc::new(program);
    let mut cache = VALIDATED.lock().unwrap_or_else(|e| e.into_inner());
    cache.insert(key, &program);
    Ok(program)
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
        out.extend(
            errors
                .into_iter()
                .map(|(field, message)| SkinDiagnostic::error(field, message)),
        );
        return;
    }
    let mut programs = BTreeMap::new();
    let mut texture_bytes = 0usize;
    let mut error_diags = Vec::new();
    for (id, spec) in &skin.shaders {
        let result = (|| -> Result<Arc<ShaderProgram>, (Option<String>, String)> {
            if !valid_program_id(id) {
                return Err((None, "invalid shader id".into()));
            }
            if !spec.period_s.is_finite()
                || !(0.5..=3600.).contains(&spec.period_s)
                || !spec.intensity.is_finite()
                || !(0.0..=1.).contains(&spec.intensity)
            {
                return Err((None, "period_s must be 0.5..3600 and intensity 0..1".into()));
            }
            if !(spec.cadence == 0. || (0.1..=60.).contains(&spec.cadence)) {
                return Err((None, "cadence must be 0 (continuous) or 0.1..60".into()));
            }
            if spec.params.len() > 8
                || spec.params.iter().any(|v| !v.is_finite())
                || spec.textures.len() > 2
            {
                return Err((
                    None,
                    "at most eight finite parameters and two textures".into(),
                ));
            }
            check_bindings(&spec.bindings, &skin.params).map_err(|e| (None, e))?;
            let encoded: Vec<Option<(String, Vec<u8>)>> = spec
                .textures
                .iter()
                .map(|path| {
                    let path = crate::clean_resource_path(path).map_err(|e| e.to_string())?;
                    let bytes = files.read(&path).map_err(|e| e.to_string())?;
                    Ok(Some((path, bytes)))
                })
                .collect::<Result<_, String>>()
                .map_err(|e| (None, e))?;
            load_program_cached(
                ProgramKind::Effect,
                &spec.source,
                encoded,
                files,
                &mut texture_bytes,
            )
        })();
        match result {
            Ok(p) => {
                programs.insert(id.clone(), p);
            }
            Err((src, e)) => error_diags.push(wgsl_diagnostic(format!("shaders.{id}"), src, e)),
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
    for (id, spec) in &skin.shaders {
        let artwork = skin
            .dialogue
            .shader_effects
            .iter()
            .any(|e| &e.shader == id && e.target == ShaderTarget::Artwork);
        if spec.owns_continue && !artwork {
            out.push(SkinDiagnostic::warning(
                format!("shaders.{id}.owns_continue"),
                "only an artwork shader can own the continue indicator",
            ));
        }
        let always = spec.active.is_empty() || spec.active.contains(&ShaderActive::Always);
        let inputs = programs.get(id).map_or(u32::MAX, |p| p.inputs);
        if spec.animated && inputs & (INPUT_PHASE | INPUT_EVENTS) == 0 {
            out.push(SkinDiagnostic::warning(
                format!("shaders.{id}.animated"),
                "reads neither `phase` nor an event, so it never moves; \
                 set `animated = false`",
            ));
        } else if spec.animated && always && inputs & INPUT_PHASE == 0 {
            out.push(SkinDiagnostic::warning(
                format!("shaders.{id}.active"),
                "never reads `phase`, so it moves only on events; list them in `active` \
                 (typing, waiting, line:<s>, speaker:<s>) to stop redrawing the rest",
            ));
        }
    }
    skin.shader_programs = programs;
    out.extend(error_diags);
    out.extend(
        errors
            .into_iter()
            .map(|(field, message)| SkinDiagnostic::error(field, message)),
    );
}

pub(crate) fn load_materials(
    skin: &mut Skin,
    files: &dyn SkinFiles,
    diagnostics: &mut Vec<SkinDiagnostic>,
) {
    if skin.materials.len() > 16
        || (!skin.materials.is_empty() && skin.meta.schema != crate::SKIN_SCHEMA)
    {
        diagnostics.push(SkinDiagnostic::error(
            "materials",
            "materials require the current schema and at most sixteen definitions",
        ));
        return;
    }
    let mut programs = BTreeMap::new();
    let mut texture_bytes = 0usize;
    for (id, spec) in &skin.materials {
        let result = (|| -> Result<Arc<ShaderProgram>, (Option<String>, String)> {
            if !valid_program_id(id) {
                return Err((None, "invalid material id".into()));
            }
            if spec.params.len() > 8
                || spec.params.iter().any(|v| !v.is_finite())
                || spec.textures.len() > 2
            {
                return Err((
                    None,
                    "materials allow eight finite parameters and two textures".into(),
                ));
            }
            // Pipeline slots hold an empty placeholder until the host fills them.
            let encoded: Vec<Option<(String, Vec<u8>)>> = spec
                .textures
                .iter()
                .map(|texture| {
                    texture
                        .image()
                        .map(|path| {
                            let path =
                                crate::clean_resource_path(path).map_err(|e| e.to_string())?;
                            let bytes = files.read(&path).map_err(|e| e.to_string())?;
                            Ok((path, bytes))
                        })
                        .transpose()
                })
                .collect::<Result<_, String>>()
                .map_err(|e| (None, e))?;
            load_program_cached(
                ProgramKind::Material,
                &spec.source,
                encoded,
                files,
                &mut texture_bytes,
            )
        })();
        match result {
            Ok(program) => {
                programs.insert(id.clone(), program);
            }
            Err((src, message)) => {
                diagnostics.push(wgsl_diagnostic(format!("materials.{id}"), src, message))
            }
        }
    }
    for (field, reference) in skin.material_references() {
        if !skin.materials.contains_key(reference) {
            diagnostics.push(SkinDiagnostic::error(
                field,
                format!("unknown material `{reference}`"),
            ));
        }
    }
    skin.material_programs = programs;
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

    #[test]
    fn deep_expressions_and_blocks_are_rejected_before_any_gpu_compiler() {
        // Past the lexical cap naga never sees it; under it, the IR depth.
        for (terms, expected) in [(8_000, "operators"), (200, "deeper than")] {
            let chain = vec!["x"; terms].join("+");
            let err = validate_shader(&format!(
                "fn effect(i:EffectInput)->vec4<f32>{{let x=i.uv.x; return vec4({chain});}}"
            ))
            .unwrap_err();
            assert!(err.contains(expected), "{terms}: {err}");
        }
        let calls = format!("{}i.uv.x{}", "abs(".repeat(200), ")".repeat(200));
        assert!(
            validate_shader(&format!(
                "fn effect(i:EffectInput)->vec4<f32>{{return vec4({calls});}}"
            ))
            .is_err()
        );
        let short = vec!["i.uv.x"; 40].join(" + ");
        let program = validate_shader(&format!(
            "fn effect(i:EffectInput)->vec4<f32>{{return vec4({short});}}"
        ))
        .unwrap();
        assert!(program.cost > 40, "{}", program.cost);
        let mut nested = "return vec4(1.);".to_owned();
        for _ in 0..MAX_BLOCK_DEPTH + 2 {
            nested = format!("if i.uv.x > 0. {{ {nested} }}");
        }
        let err = validate_shader(&format!(
            "fn effect(i:EffectInput)->vec4<f32>{{{nested} return vec4(0.);}}"
        ))
        .unwrap_err();
        assert!(err.contains("nest"), "{err}");
    }

    #[test]
    fn compute_storage_pointers_and_host_names_are_rejected() {
        const E: &str = "fn effect(i:EffectInput)->vec4<f32>{return vec4(0.);}";
        for source in [
            format!("@compute @workgroup_size(1) fn c(){{}} {E}"),
            format!("var<workgroup> w: f32; {E}"),
            format!("@group(0) @binding(9) var<storage, read_write> s: array<f32>; {E}"),
            format!("fn f(p: ptr<function, f32>) -> f32 {{ return *p; }} {E}"),
            format!("var<private> a: atomic<u32>; {E}"),
            "fn effect(i:EffectInput)->vec4<f32>{workgroupBarrier(); return vec4(0.);}".into(),
            "fn effect(i:EffectInput)->vec4<f32>{textureStore(fx_texture0, vec2(0), vec4(0.)); return vec4(0.);}".into(),
            format!("fn fx_x() -> f32 {{ return 0.; }} {E}"),
            format!("fn __x() -> f32 {{ return 0.; }} {E}"),
            format!("fn nm_x() -> f32 {{ return 0.; }} {E}"),
            format!("{E}\0"),
            format!("{E}\x1b"),
        ] {
            assert!(validate_shader(&source).is_err(), "accepted {source:?}");
        }
        for source in [
            "fn fx_x() -> f32 { return 0.; } fn material(i: MaterialInput) -> vec4<f32> { return vec4(fx_x()); }",
            "fn material(i: MaterialInput) -> vec4<f32> { return nm_parameters[0]; }",
            "fn material(i: MaterialInput) -> vec4<f32> { return vec4(f32(arrayLength(&nm_parameters))); }",
            "@compute @workgroup_size(1) fn c(){} fn material(i: MaterialInput) -> vec4<f32> { return vec4(0.); }",
        ] {
            assert!(validate_material(source).is_err(), "accepted {source:?}");
        }
        assert!(
            validate_material(
                "fn material(i: MaterialInput) -> vec4<f32> { return material_param(0u); }"
            )
            .is_ok()
        );
        // Comments and identifiers that merely contain a banned word are fine.
        assert!(
            validate_shader(&format!(
                "// compute storage ptr\nfn storage_tint(c: vec4<f32>) -> vec4<f32> {{ return c; }} {}",
                "fn effect(i:EffectInput)->vec4<f32>{return storage_tint(i.accent);}"
            ))
            .is_ok()
        );
    }

    #[test]
    fn validator_panics_reject_one_shader_and_worker_recovers() {
        let err = run_on_validator::<()>(|| panic!("naga bug")).unwrap_err();
        assert!(err.contains("validator failed"), "{err}");
        // Subsequent validation on the same reusable worker still succeeds.
        assert!(validate_shader(OK).is_ok());
    }
}
