//! Skin-owned pixel functions, rendered beneath dialogue text. The host owns
//! bindings, targets, masks, clocks, error handling and every resource budget.
use super::{Stage, dialogue::Source, surface::Surface};
use nana_ui::runtime::view::{AnyView, IntoView, widget};
use nana_ui::runtime::{
    ComponentView, CustomRenderNode, GpuView, MutationQueue, NodeKind, StableNodeId, UiWorld,
};
use nana_ui::{
    DeviceGeneration, GpuContext, GpuTextureFormat, SceneGpuNode, SceneGpuPassContext,
    SceneGpuPrepareContext, SceneGpuRenderContext, SceneGpuRenderer, SceneGpuRendererRegistry,
    ScenePass, ScenePresentationProfile, wgpu,
};
use nana_ui_core::{LengthSpec, PointerEventsSpec, PositionSpec};
use nanatype_theme::{MAX_SHADER_CACHE_BYTES, ShaderProgram, ShaderSpec, ShaderTarget, Skin};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::{
        Arc, LazyLock, Mutex, OnceLock, Weak,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

const RENDERER: &str = "nanatype.skin-shader-v1";
static NEXT: AtomicU64 = AtomicU64::new(1);
static MATERIALS: LazyLock<Mutex<HashMap<u64, Weak<Material>>>> = LazyLock::new(Default::default);
static NOTICES: LazyLock<Mutex<Vec<String>>> = LazyLock::new(Default::default);
static DISABLED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Default::default);
/// Programs frozen for going over the GPU budget: drawn as if motion were off.
static THROTTLED: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(Default::default);
static GUARD: LazyLock<Mutex<super::shader_guard::Guard>> = LazyLock::new(Default::default);
/// What all skin programs together may cost in one second: per-pixel cost
/// (`ShaderProgram::cost`) times covered pixels, summed over every draw.
/// About 1.6× the heaviest built-in (3608) over a 3840×2160 canvas at 60 fps.
const DEFAULT_COST_BUDGET: u64 = 3_000_000_000_000;
static COST_BUDGET: AtomicU64 = AtomicU64::new(DEFAULT_COST_BUDGET);
const BUDGET_WINDOW: Duration = Duration::from_secs(1);
/// Programs drawn this close to a device's last draw share its loss.
const LOSS_WINDOW: Duration = Duration::from_secs(2);
static COMPILED: AtomicU64 = AtomicU64::new(0);
static COMPILING: AtomicUsize = AtomicUsize::new(0);
/// Compiles turned away while both compiler slots were busy; the draw retries
/// on a later frame (see `warming`).
static REFUSED: AtomicU64 = AtomicU64::new(0);
struct CompileSlot;
impl Drop for CompileSlot {
    fn drop(&mut self) {
        COMPILING.fetch_sub(1, Ordering::Relaxed);
    }
}
fn runnable(skin: &Skin, id: &str) -> bool {
    skin.shaders
        .get(id)
        .is_some_and(|s| s.intensity > 0. && s.animated)
        && skin
            .shader_programs
            .get(id)
            .is_some_and(|p| !stopped(&p.hash))
}
/// Disabled, or frozen for going over the GPU budget.
fn stopped(hash: &str) -> bool {
    DISABLED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(hash)
        || throttled(hash)
}
fn throttled(hash: &str) -> bool {
    THROTTLED
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .contains(hash)
}
fn short(hash: &str) -> &str {
    hash.get(..12).unwrap_or(hash)
}

/// Replace the GPU budget (for tests that drive programs over it); returns
/// the previous one.
#[doc(hidden)]
pub fn set_cost_budget(budget: u64) -> u64 {
    COST_BUDGET.swap(budget, Ordering::Relaxed)
}

/// Start the crash guard with its record at `path`. Programs that were on
/// trial when the last run ended are disabled, with a notice.
pub fn init_guard(path: PathBuf) {
    let (guard, crashed) = super::shader_guard::Guard::open(path);
    let mut state = SHARED.state.lock().unwrap_or_else(|e| e.into_inner());
    for hash in guard.blocked() {
        let why = crashed
            .contains(hash)
            .then_some("it was running when NanaType or the GPU driver stopped");
        state.disable(hash, why);
    }
    *GUARD.lock().unwrap_or_else(|e| e.into_inner()) = guard;
}
/// A clean exit: no program on trial crashed the process.
pub fn close_guard() {
    GUARD.lock().unwrap_or_else(|e| e.into_inner()).close();
}
/// The operator turned advanced effects back on: retry every disabled,
/// frozen or crash-blocked program.
pub fn retry_disabled() {
    GUARD.lock().unwrap_or_else(|e| e.into_inner()).reset();
    THROTTLED.lock().unwrap_or_else(|e| e.into_inner()).clear();
    DISABLED.lock().unwrap_or_else(|e| e.into_inner()).clear();
    let mut state = SHARED.state.lock().unwrap_or_else(|e| e.into_inner());
    state.failed.clear();
    state.reported.clear();
}
pub fn take_notices() -> Vec<String> {
    std::mem::take(&mut *NOTICES.lock().unwrap_or_else(|e| e.into_inner()))
}
fn notice(message: String) {
    NOTICES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(message);
}

pub fn enabled(ui: Stage) -> bool {
    !ui.reduced_motion.get_untracked() && ui.settings.with_untracked(|s| s.skin.advanced_effects)
}
pub fn animating(ui: Stage) -> bool {
    // A renderer can discover a compiler failure after Host::finish. Keep one
    // callback alive so its notice reaches the operator before the track sleeps.
    if !NOTICES.lock().unwrap_or_else(|e| e.into_inner()).is_empty() {
        return true;
    }
    if warming(ui, Source::Display) {
        return true;
    }
    enabled(ui)
        && !ui.display.with_untracked(|d| d.empty)
        && ui.skin.with_untracked(|s| {
            let resolved =
                nanatype_theme::responsive_dialogue(s, super::dialogue::canvas_size(ui, s));
            let s = &resolved;
            s.dialogue
                .shader_effects
                .iter()
                .map(|e| &e.shader)
                .chain(s.dialogue.decor.iter().filter_map(|d| d.shader.as_ref()))
                .any(|id| runnable(s, id))
        })
}
pub fn preview_animating(ui: Stage) -> bool {
    if !NOTICES.lock().unwrap_or_else(|e| e.into_inner()).is_empty() {
        return true;
    }
    if warming(ui, Source::Preview) {
        return true;
    }
    enabled(ui)
        && ui.shader_preview.get_untracked()
        && ui.live_preview.get()
        && MATERIALS
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .values()
            .filter_map(Weak::upgrade)
            .any(|m| {
                m.source == Source::Preview
                    && m.spec.animated
                    && m.spec.intensity > 0.
                    && !stopped(&m.program.hash)
            })
}

struct Material {
    warming: AtomicBool,
    /// The untransformed layout canvas; preview scaling is paint-only.
    logical_size: Arc<Mutex<[f32; 2]>>,
    id: u64,
    ui: Stage,
    source: Source,
    surface: Surface,
    full_coverage: bool,
    program: Arc<ShaderProgram>,
    spec: ShaderSpec,
    palette: [[f32; 4]; 4],
    /// `param(i)` slots packed from `spec.bindings` and the skin's params.
    slots: Box<[f32; SLOT_FLOATS]>,
}
const SLOT_FLOATS: usize = nanatype_theme::MAX_SHADER_SLOTS * 4;
const SLOT_BYTES: usize = SLOT_FLOATS * 4;

/// Pack `bindings` one `vec4` each: scalars in `x`, colours as linear
/// straight-alpha `rgba` (as the palette), lists component by component.
fn pack_slots(
    bindings: &[nanatype_theme::SlotBinding],
    values: &std::collections::BTreeMap<String, nanatype_theme::Resolved>,
) -> Box<[f32; SLOT_FLOATS]> {
    use nanatype_theme::{Resolved, SlotBinding, SlotComponent};
    let mut out = Box::new([0f32; SLOT_FLOATS]);
    let scalar = |id: &str| values.get(id).and_then(Resolved::scalar).unwrap_or(0.);
    for (slot, binding) in out.as_chunks_mut::<4>().0.iter_mut().zip(bindings) {
        match binding {
            SlotBinding::Number(v) => slot[0] = *v,
            SlotBinding::Param(id) => match values.get(id) {
                Some(Resolved::Color(c)) => slot.copy_from_slice(&color(*c)),
                _ => slot[0] = scalar(id),
            },
            SlotBinding::Tuple(parts) => {
                for (dst, part) in slot.iter_mut().zip(parts) {
                    *dst = match part {
                        SlotComponent::Number(v) => *v,
                        SlotComponent::Param(id) => scalar(id),
                    };
                }
            }
        }
    }
    out
}

/// The artwork draws its own continue indicator right now: it declares
/// `owns_continue`, compiled, and is playing rather than frozen or off.
pub fn owns_continue(ui: Stage, source: Source, skin: &Skin) -> bool {
    let playing = !ui.reduced_motion.get()
        && ui.settings.with(|s| s.skin.advanced_effects)
        && (source != Source::Preview || ui.shader_preview.get());
    playing
        && skin.dialogue.shader_effects.iter().any(|e| {
            e.target == ShaderTarget::Artwork
                && skin.shaders.get(&e.shader).is_some_and(|s| s.owns_continue)
                && runnable(skin, &e.shader)
        })
}
fn warming(ui: Stage, source: Source) -> bool {
    // View reconciliation may create/drop materials while holding reactive
    // state. Never read a signal under the global material registry lock.
    let materials: Vec<_> = MATERIALS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .values()
        .filter_map(Weak::upgrade)
        .collect();
    materials.into_iter().any(|m| {
        m.source == source
            && m.spec.static_fallback
            && m.warming.load(Ordering::Relaxed)
            // Only this UI's materials: another UI's signals are not ours to
            // read (in tests they live on another thread).
            && if source == Source::Preview {
                // A sample shares the clock of the UI it previews for.
                m.ui.now == ui.now && ui.live_preview.get()
            } else {
                m.ui.display == ui.display && !ui.display.with_untracked(|d| d.empty)
            }
    })
}

fn playing(m: &Material) -> bool {
    m.spec.animated
        && !throttled(&m.program.hash)
        && enabled(m.ui)
        && (m.source != Source::Preview || m.ui.shader_preview.get_untracked())
}
impl PartialEq for Material {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}
#[derive(Clone, PartialEq)]
struct EffectView {
    base: GpuView,
    material: Arc<Material>,
}
impl ComponentView for EffectView {
    fn share_layouts(&mut self, share: &mut dyn FnMut(&mut Arc<nana_ui_core::LayoutStyle>)) {
        self.base.share_layouts(share);
    }
    fn node_kind(&self) -> NodeKind {
        self.base.node_kind()
    }
    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        self.base.project(id, world, mutations);
        mutations.set_custom_render(
            id,
            Some(CustomRenderNode::new(
                RENDERER,
                self.material.id.to_string(),
                1,
            )),
        );
    }
}
use super::effects::linear;
fn color(c: nanatype_model::Rgba8) -> [f32; 4] {
    [linear(c.r), linear(c.g), linear(c.b), c.a as f32 / 255.]
}

pub fn view(
    ui: Stage,
    source: Source,
    skin: &Skin,
    id: &str,
    mut surface: Surface,
    target: ShaderTarget,
) -> Option<AnyView> {
    let program = skin.shader_programs.get(id)?.clone();
    let spec = skin.shaders.get(id)?.clone();
    let slots = pack_slots(&spec.bindings, &skin.param_values);
    if spec.intensity == 0. {
        return None;
    }
    if target == ShaderTarget::Border {
        // Outline-only coverage uses the exact same path as the static frame.
        surface.fill = Some([0.; 4]);
        surface.gradient = None;
    }
    if target == ShaderTarget::Artwork {
        // A baked static frame is the fallback, never the coverage source for
        // its live counterpart. The shader owns alpha inside this canvas.
        surface = Surface {
            fill: Some([1.; 4]),
            ..Default::default()
        };
    }
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let logical_size = Arc::new(Mutex::new([0.0; 2]));
    let material = Arc::new(Material {
        warming: AtomicBool::new(spec.static_fallback),
        logical_size: logical_size.clone(),
        id,
        ui,
        source,
        surface,
        full_coverage: target == ShaderTarget::Artwork,
        program,
        spec,
        palette: [
            color(skin.dialogue.box_.color),
            color(skin.chrome.accent),
            color(skin.dialogue.box_.border_color),
            color(skin.dialogue.text.color),
        ],
        slots,
    });
    {
        let mut materials = MATERIALS.lock().unwrap_or_else(|e| e.into_inner());
        materials.retain(|_, m| m.strong_count() > 0);
        materials.insert(id, Arc::downgrade(&material));
    }
    let mut base = GpuView::new(id);
    let l = Arc::make_mut(&mut base.style.layout);
    l.position = PositionSpec::Absolute;
    l.offset_left = Some(LengthSpec::Px(0.));
    l.offset_top = Some(LengthSpec::Px(0.));
    l.width = Some(LengthSpec::Percent(100.));
    l.height = Some(LengthSpec::Percent(100.));
    l.pointer_events = Some(PointerEventsSpec::None);
    Some(
        widget(EffectView { base, material })
            .on(move |e: &nana_ui::runtime::SizeChanged| {
                *logical_size.lock().unwrap_or_else(|e| e.into_inner()) = [e.width, e.height];
            })
            .into_any(),
    )
}

type PipelineKey = (
    DeviceGeneration,
    GpuTextureFormat,
    ScenePresentationProfile,
    String,
);
type MaskKey = (
    DeviceGeneration,
    GpuTextureFormat,
    ScenePresentationProfile,
    u64,
    [u32; 2],
    u32,
);
struct Pipeline {
    uploaded: AtomicBool,
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
    textures: [wgpu::Texture; 2],
    sampler: wgpu::Sampler,
}
struct Pending {
    value: Arc<OnceLock<Result<Arc<Pipeline>, String>>>,
    texture_bytes: usize,
}
struct Prepared {
    material: Weak<Material>,
    uniforms: super::effect_uniforms::Uniforms,
    pipeline: Arc<Pipeline>,
    textures: [wgpu::TextureView; 3],
    /// The material's param slots, written once when prepared.
    slots: wgpu::Buffer,
    bytes: usize,
}
#[derive(Default, Clone, Copy)]
struct Spend {
    cost: u64,
    animated: bool,
}
#[derive(Debug, PartialEq)]
enum Verdict {
    Freeze,
    Disable,
}
/// When one window's draws together went over `budget`: the program that
/// spent most, frozen if it was playing and disabled if it already was not.
fn budget_verdict(spent: &HashMap<String, Spend>, budget: u64) -> Option<(&str, Verdict)> {
    let total = spent.values().fold(0u64, |t, s| t.saturating_add(s.cost));
    if total <= budget {
        return None;
    }
    let (hash, s) = spent.iter().max_by_key(|(hash, s)| (s.cost, *hash))?;
    Some((
        hash,
        if s.animated {
            Verdict::Freeze
        } else {
            Verdict::Disable
        },
    ))
}
#[derive(Default)]
struct State {
    generations: Vec<DeviceGeneration>,
    /// Each generation's context, to tell a lost device from an added one.
    devices: Vec<GpuContext>,
    /// When each program last drew on each device.
    last_drawn: HashMap<(DeviceGeneration, String), Instant>,
    window: Option<Instant>,
    spent: HashMap<String, Spend>,
    pipelines: HashMap<PipelineKey, Pending>,
    masks: HashMap<MaskKey, Prepared>,
    scales: super::effect_uniforms::Scales,
    failed: HashMap<String, u8>,
    reported: HashSet<PipelineKey>,
}
#[derive(Default)]
struct Renderer {
    state: Mutex<State>,
}
impl std::fmt::Debug for Renderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SkinShaderRenderer")
    }
}
static SHARED: LazyLock<Arc<Renderer>> = LazyLock::new(|| Arc::new(Renderer::default()));
pub fn register(registry: &mut SceneGpuRendererRegistry) {
    registry.insert(RENDERER, SHARED.clone());
}

/// Release dead view bindings even when the next theme is entirely static.
pub fn collect_garbage() {
    let mut state = SHARED.state.lock().unwrap_or_else(|e| e.into_inner());
    let mut materials = MATERIALS.lock().unwrap_or_else(|e| e.into_inner());
    materials.retain(|_, m| m.strong_count() > 0);
    state.masks.retain(|_, m| m.material.strong_count() > 0);
    state.scales.retain(|(_, id, _)| materials.contains_key(id));
}
#[derive(Debug, serde::Serialize)]
pub struct CacheStats {
    pub programs: usize,
    pub masks: usize,
    pub bytes: usize,
    pub pending: usize,
    pub uniforms: usize,
    pub scales: usize,
    pub compilations: u64,
    /// Compiles deferred because both compiler slots were busy.
    pub refused: u64,
    pub uniform_allocations: u64,
}
pub fn cache_stats() -> CacheStats {
    let state = SHARED.state.lock().unwrap_or_else(|e| e.into_inner());
    CacheStats {
        programs: state.pipelines.len(),
        masks: state.masks.len(),
        bytes: state
            .pipelines
            .values()
            .map(|p| p.texture_bytes)
            .sum::<usize>()
            + state
                .masks
                .values()
                .map(|m| m.bytes + m.uniforms.bytes())
                .sum::<usize>(),
        uniforms: state.masks.values().map(|m| m.uniforms.len()).sum(),
        scales: state.scales.len(),
        compilations: COMPILED.load(Ordering::Relaxed),
        refused: REFUSED.load(Ordering::Relaxed),
        uniform_allocations: super::effect_uniforms::allocations(),
        pending: state
            .pipelines
            .values()
            .filter(|p| p.value.get().is_none())
            .count(),
    }
}

fn compile(
    device: &wgpu::Device,
    format: GpuTextureFormat,
    program: &ShaderProgram,
) -> Result<Pipeline, String> {
    let oom = device.push_error_scope(wgpu::ErrorFilter::OutOfMemory);
    let internal = device.push_error_scope(wgpu::ErrorFilter::Internal);
    let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
    let result = (|| {
        let texture_entry = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("skin shader ABI 2"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: std::num::NonZeroU64::new(176),
                    },
                    count: None,
                },
                texture_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                texture_entry(3),
                texture_entry(4),
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: std::num::NonZeroU64::new(SLOT_BYTES as u64),
                    },
                    count: None,
                },
            ],
        });
        let source = format!("{}\n{}", program.source, nanatype_theme::SHADER_WRAPPER);
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("validated skin shader"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("skin shader"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("skin shader"),
            layout: Some(&pl),
            vertex: wgpu::VertexState {
                module: &shader,
                entry_point: Some("fx_vertex"),
                buffers: &[],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &shader,
                entry_point: Some("fx_fragment"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: format.wgpu(),
                    blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: Default::default(),
            depth_stencil: None,
            multisample: Default::default(),
            multiview_mask: None,
            cache: None,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("skin clamp sampler"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let mut textures = vec![];
        for i in 0..2 {
            let size = program.textures.get(i).map_or([1, 1], |t| t.size);
            textures.push(device.create_texture(&wgpu::TextureDescriptor {
                label: Some("skin texture"),
                size: wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Rgba8UnormSrgb,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            }));
        }
        Ok(Pipeline {
            uploaded: AtomicBool::new(false),
            pipeline,
            layout,
            textures: textures.try_into().map_err(|_| "texture count")?,
            sampler,
        })
    })();
    let errors = [
        pollster::block_on(scope.pop()),
        pollster::block_on(internal.pop()),
        pollster::block_on(oom.pop()),
    ];
    if let Some(e) = errors.into_iter().flatten().next() {
        return Err(e.to_string());
    }
    result
}

impl State {
    /// Stop drawing `hash` for the rest of the run. Two strikes, like a
    /// twice-failed compile, so it survives a device change. `why` is shown
    /// to the operator; `None` disables quietly.
    fn disable(&mut self, hash: &str, why: Option<&str>) {
        let failed = self.failed.entry(hash.to_owned()).or_default();
        *failed = (*failed).max(2);
        let new = DISABLED
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(hash.to_owned());
        if let Some(why) = why.filter(|_| new) {
            nanatype_diag::report!(crate::diag::SHADER_DISABLED; "{}: {why}", short(hash));
            notice(format!("Skin shader {} disabled: {why}", short(hash)));
        }
    }

    /// Count one draw against the GPU budget. At the end of each window the
    /// heaviest program is frozen, or disabled if it already was frozen.
    fn spend(&mut self, hash: &str, cost: u64, animated: bool, now: Instant) {
        if self
            .window
            .is_none_or(|start| now.duration_since(start) >= BUDGET_WINDOW)
        {
            let verdict = budget_verdict(&self.spent, COST_BUDGET.load(Ordering::Relaxed)).map(|(h, v)| (h.to_owned(), v));
            match verdict {
                // The draw that froze it was still charged as playing.
                Some((hash, Verdict::Freeze)) if !throttled(&hash) => {
                    THROTTLED
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(hash.clone());
                    let why = "over the GPU budget, now frozen";
                    nanatype_diag::report!(crate::diag::SHADER_DISABLED; "{}: {why}", short(&hash));
                    notice(format!("Skin shader {} {why}", short(&hash)));
                }
                Some((hash, _)) => {
                    self.disable(&hash, Some("over the GPU budget even when frozen"));
                }
                None => {}
            }
            self.spent.clear();
            self.window = Some(now);
        }
        let s = self.spent.entry(hash.to_owned()).or_default();
        s.cost = s.cost.saturating_add(cost);
        s.animated |= animated;
    }

    /// A device was lost: programs drawn in its last moments get a strike.
    /// The second strike disables one, so a program that resets the GPU runs
    /// on at most two devices.
    fn attribute_loss(&mut self, generation: DeviceGeneration) {
        let drawn: Vec<(String, Instant)> = self
            .last_drawn
            .iter()
            .filter(|((g, _), _)| *g == generation)
            .map(|((_, h), at)| (h.clone(), *at))
            .collect();
        self.last_drawn.retain(|(g, _), _| *g != generation);
        self.strike(drawn);
    }

    /// One strike for each program drawn within [`LOSS_WINDOW`] of the lost
    /// device's last draw.
    fn strike(&mut self, drawn: Vec<(String, Instant)>) {
        let Some(latest) = drawn.iter().map(|(_, at)| *at).max() else {
            return;
        };
        for (hash, at) in drawn {
            if latest.duration_since(at) > LOSS_WINDOW {
                continue;
            }
            let failed = self.failed.entry(hash.clone()).or_default();
            *failed += 1;
            if *failed >= 2 {
                self.disable(&hash, Some("the GPU device was lost twice while it ran"));
            }
        }
    }

    fn prepare(
        &mut self,
        material: &Arc<Material>,
        c: &SceneGpuPassContext<'_>,
        scale: f32,
    ) -> Option<&mut Prepared> {
        let generation = c.gpu.generation();
        if !self.generations.contains(&generation) {
            // The host replaces a lost device with a new one; an added device
            // (another adapter) is not a loss and charges nothing.
            let lost: Vec<_> = self
                .devices
                .iter()
                .filter(|d| d.is_lost())
                .map(GpuContext::generation)
                .collect();
            for g in lost {
                self.attribute_loss(g);
            }
            self.generations.push(generation);
            self.devices.push(c.gpu.clone());
            if self.generations.len() > 4 {
                let retired = self.generations.remove(0);
                self.devices.remove(0);
                self.pipelines.retain(|(g, _, _, _), _| *g != retired);
                self.masks.retain(|(g, _, _, _, _, _), _| *g != retired);
                self.scales.retain(|(g, _, _)| *g != retired);
                self.last_drawn.retain(|(g, _), _| *g != retired);
            }
            DISABLED
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .retain(|hash| self.failed.get(hash).copied().unwrap_or(0) >= 2);
        }
        let bounds = [c.bounds.width, c.bounds.height];
        if bounds.contains(&0)
            || bounds
                .iter()
                .any(|x| *x > 8192.min(c.gpu.wgpu().device().limits().max_texture_dimension_2d))
        {
            return None;
        }
        // Complete artwork supplies its own alpha. One white texel is exact
        // at every size/DPI; logical dimensions still update in the uniform.
        let extent = if material.full_coverage {
            [1, 1]
        } else {
            bounds
        };
        let key = (
            generation,
            c.target_format,
            c.presentation,
            material.id,
            extent,
            if material.full_coverage {
                0
            } else {
                scale.to_bits()
            },
        );
        if self.masks.contains_key(&key) {
            return self.masks.get_mut(&key);
        }
        {
            let materials = MATERIALS.lock().unwrap_or_else(|e| e.into_inner());
            self.scales.retain(|(g, id, _)| {
                self.generations.contains(g)
                    && materials.get(id).is_some_and(|m| m.strong_count() > 0)
            });
        }
        self.masks.retain(|(g, _, _, _, _, _), m| {
            self.generations.contains(g) && m.material.strong_count() > 0
        });
        let mask_bytes = extent[0] as usize * extent[1] as usize * 4;
        if extent.contains(&0)
            || extent
                .iter()
                .any(|x| *x > 8192.min(c.gpu.wgpu().device().limits().max_texture_dimension_2d))
            || mask_bytes > MAX_SHADER_CACHE_BYTES / 2
        {
            return None;
        }
        let pk = (
            generation,
            c.target_format,
            c.presentation,
            material.program.hash.clone(),
        );
        if self
            .failed
            .get(&material.program.hash)
            .copied()
            .unwrap_or(0)
            >= 2
        {
            return None;
        }
        let texture_bytes = material
            .program
            .textures
            .iter()
            .map(|t| t.size[0] as usize * t.size[1] as usize * 4)
            .sum::<usize>()
            .max(8);
        let used_textures = self
            .pipelines
            .values()
            .map(|p| p.texture_bytes)
            .sum::<usize>();
        if !self.pipelines.contains_key(&pk) {
            if self.pipelines.len() >= 32
                || used_textures + texture_bytes > MAX_SHADER_CACHE_BYTES / 2
            {
                // Evict only programs not held by any live node. Live programs
                // cannot displace one another and repeatedly compile each frame.
                let live: HashSet<String> = MATERIALS
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .values()
                    .filter_map(Weak::upgrade)
                    .map(|m| m.program.hash.clone())
                    .collect();
                self.pipelines
                    .retain(|(_, _, _, hash), p| live.contains(hash) || p.value.get().is_none());
                if self.pipelines.len() >= 32
                    || self
                        .pipelines
                        .values()
                        .map(|p| p.texture_bytes)
                        .sum::<usize>()
                        + texture_bytes
                        > MAX_SHADER_CACHE_BYTES / 2
                {
                    return None;
                }
            }
            // Rapid switching/hot reload must not spawn an unbounded number
            // of compiler threads or release a pending texture reservation.
            if COMPILING
                .compare_exchange(0, 1, Ordering::Relaxed, Ordering::Relaxed)
                .is_err()
                && COMPILING
                    .compare_exchange(1, 2, Ordering::Relaxed, Ordering::Relaxed)
                    .is_err()
            {
                REFUSED.fetch_add(1, Ordering::Relaxed);
                return None;
            }
            COMPILED.fetch_add(1, Ordering::Relaxed);
            let slot = CompileSlot;
            let value = Arc::new(OnceLock::new());
            let result = value.clone();
            let device = c.gpu.wgpu().device().clone();
            let program = material.program.clone();
            let format = c.target_format;
            // Platform shader compilers recurse over the program; the sandbox
            // bounds its depth and this stack leaves room beyond that.
            let spawned = std::thread::Builder::new()
                .name("skin-shader-compiler".into())
                .stack_size(16 << 20)
                .spawn(move || {
                    let _slot = slot;
                    let p = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        compile(&device, format, &program).map(Arc::new)
                    }))
                    .unwrap_or_else(|_| Err("GPU shader compiler failed".into()));
                    let _ = result.set(p);
                });
            if let Err(e) = spawned {
                let _ = value.set(Err(e.to_string()));
            }
            self.pipelines.insert(
                pk.clone(),
                Pending {
                    value,
                    texture_bytes,
                },
            );
        }
        let pending = self.pipelines.get(&pk)?;
        let pipeline = match pending.value.get()? {
            Ok(p) => p,
            Err(e) => {
                if self.reported.insert(pk.clone()) {
                    *self
                        .failed
                        .entry(material.program.hash.clone())
                        .or_default() += 1;
                    DISABLED
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .insert(material.program.hash.clone());
                    notice(format!(
                        "Skin shader {} disabled: {e}",
                        &material.program.hash[..12]
                    ));
                }
                return None;
            }
        };
        let texture_used = self
            .pipelines
            .values()
            .map(|p| p.texture_bytes)
            .sum::<usize>();
        if self.masks.len() >= 32
            || self
                .masks
                .values()
                .map(|m| m.bytes + m.uniforms.bytes())
                .sum::<usize>()
                + texture_used
                + mask_bytes
                + 8 * 176
                + SLOT_BYTES
                > MAX_SHADER_CACHE_BYTES
        {
            self.masks
                .retain(|(_, _, _, id, _, _), _| *id != material.id);
            if self.masks.len() >= 32
                || self
                    .masks
                    .values()
                    .map(|m| m.bytes + m.uniforms.bytes())
                    .sum::<usize>()
                    + texture_used
                    + mask_bytes
                    + 8 * 176
                    + SLOT_BYTES
                    + SLOT_BYTES
                    > MAX_SHADER_CACHE_BYTES
            {
                return None;
            }
        }
        let mask = if material.full_coverage {
            vec![255; 4]
        } else {
            super::sweep::coverage_for(
                &material.surface,
                extent,
                scale,
                material.surface.fill == Some([0.; 4]) && material.surface.gradient.is_none(),
            )
        };
        let gpu = c.gpu.wgpu();
        let device = gpu.device();
        let mask_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("skin coverage"),
            size: wgpu::Extent3d {
                width: extent[0],
                height: extent[1],
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let _submission = gpu.lock_submission();
        let upload = |texture: &wgpu::Texture, size: [u32; 2], bytes: &[u8]| {
            gpu.queue().write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                bytes,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(size[0] * 4),
                    rows_per_image: Some(size[1]),
                },
                wgpu::Extent3d {
                    width: size[0],
                    height: size[1],
                    depth_or_array_layers: 1,
                },
            );
        };
        upload(&mask_texture, extent, &mask);
        let slots = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("skin shader param slots"),
            size: SLOT_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let slot_bytes: Vec<u8> = material
            .slots
            .iter()
            .flat_map(|v| v.to_ne_bytes())
            .collect();
        gpu.queue().write_buffer(&slots, 0, &slot_bytes);
        // Textures are immutable; upload once with their first prepared mask.
        let first = !pipeline.uploaded.swap(true, Ordering::Relaxed);
        if first {
            for (i, t) in pipeline.textures.iter().enumerate() {
                if let Some(source) = material.program.textures.get(i) {
                    upload(t, source.size, &source.bytes);
                } else {
                    upload(t, [1, 1], &[0; 4]);
                }
            }
        }
        if let Some(w) = c.gpu_work {
            w.record_realloc();
            w.record_upload(mask.len() + SLOT_BYTES + if first { texture_bytes } else { 0 });
        }
        self.masks.insert(
            key,
            Prepared {
                material: Arc::downgrade(material),
                uniforms: Default::default(),
                textures: [
                    mask_texture.create_view(&Default::default()),
                    pipeline.textures[0].create_view(&Default::default()),
                    pipeline.textures[1].create_view(&Default::default()),
                ],
                pipeline: pipeline.clone(),
                slots,
                bytes: mask_bytes + SLOT_BYTES,
            },
        );
        self.masks.get_mut(&key)
    }
}

#[derive(Default)]
struct EventState {
    line: Option<nanatype_model::LineId>,
    speaker: String,
    line_at: u64,
    speaker_at: u64,
}
static EVENTS: LazyLock<Mutex<HashMap<usize, EventState>>> = LazyLock::new(Default::default);
fn event_values(ui: Stage, now: u64) -> [f32; 4] {
    let v = ui.display.get_untracked();
    let mut states = EVENTS.lock().unwrap_or_else(|e| e.into_inner());
    let s = states.entry(ui.track_slot).or_default();
    if s.line != v.line {
        s.line = v.line;
        s.line_at = now;
    }
    if s.speaker != v.speaker.name {
        s.speaker = v.speaker.name.clone();
        s.speaker_at = now;
    }
    let total = v
        .reveal
        .hidden_from
        .saturating_sub(v.reveal.visible_from)
        .max(1);
    let mut shown = v.reveal.shown;
    if let Some(seg) = &v.reveal.segment {
        shown = shown.max(
            seg.start
                + seg
                    .reveal_ms
                    .partition_point(|offset| seg.origin_ms + u64::from(*offset) <= now)
                    as u32,
        );
    }
    [
        now.saturating_sub(s.line_at) as f32 / 1000.,
        now.saturating_sub(s.speaker_at) as f32 / 1000.,
        (shown.saturating_sub(v.reveal.visible_from) as f32 / total as f32).clamp(0., 1.),
        if v.continue_indicator { 1. } else { 0. },
    ]
}
impl SceneGpuRenderer for Renderer {
    fn preparation_version(&self, node: &CustomRenderNode) -> Option<u64> {
        Some(node.revision)
    }
    fn content_version(&self, _: &CustomRenderNode) -> Option<u64> {
        None // Time and dialogue events change pixels without rebuilding commands.
    }

    fn prepare(&self, node: &SceneGpuNode, c: SceneGpuPrepareContext<'_>) {
        let Ok(material_id) = node.custom.resource.parse::<u64>() else {
            return;
        };
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .scales
            .record(c.gpu.generation(), material_id, c.dest_size, c.scale_factor);
    }
    fn render(&self, node: &SceneGpuNode, mut c: SceneGpuRenderContext<'_>) {
        let p = SceneGpuPassContext {
            gpu: c.gpu,
            target_format: c.target_format,
            presentation: c.presentation,
            bounds: c.bounds,
            clip: c.clip,
            dest_size: c.dest_size,
            gpu_work: c.gpu_work,
        };
        c.with_pass("skin shader", |pass| {
            self.draw_in_pass(node, pass, p);
        });
    }
    fn draw_in_pass(
        &self,
        node: &SceneGpuNode,
        pass: &mut ScenePass<'_, '_>,
        c: SceneGpuPassContext<'_>,
    ) -> bool {
        let Some(m) = node.custom.resource.parse::<u64>().ok().and_then(|id| {
            MATERIALS
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .get(&id)
                .and_then(Weak::upgrade)
        }) else {
            return true;
        };
        let animated = playing(&m);
        if (!animated && !m.spec.static_fallback)
            || (m.source == Source::Display && m.ui.display.with_untracked(|v| v.empty))
        {
            return true;
        }
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.failed.get(&m.program.hash).copied().unwrap_or(0) >= 2 {
            return true;
        }
        let registered_scale = state.scales.get(c.gpu.generation(), m.id, c.dest_size);
        let logical = *m.logical_size.lock().unwrap_or_else(|e| e.into_inner());
        let scale = if logical[0] > 0.0 && logical[1] > 0.0 {
            (c.bounds.width as f32 / logical[0])
                .min(c.bounds.height as f32 / logical[1])
                .max(0.0001)
        } else {
            registered_scale
        };
        let Some(prepared) = state.prepare(&m, &c, scale) else {
            m.warming.store(
                m.spec.static_fallback && COMPILING.load(Ordering::Relaxed) > 0,
                Ordering::Relaxed,
            );
            return true;
        };
        m.warming.store(false, Ordering::Relaxed);
        let instant = Instant::now();
        if !GUARD
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .draw(&m.program.hash, instant)
        {
            return true;
        }
        let now = if animated { crate::clock::now_ms() } else { 0 };
        let events = if !animated {
            [2., 2., 1., 0.]
        } else if m.source == Source::Preview {
            [2., 2., 1., 1.]
        } else {
            event_values(m.ui, now)
        };
        let (sin, cos) = m.surface.rotate_deg.to_radians().sin_cos();
        let width = c.bounds.width as f32;
        let height = c.bounds.height as f32;
        let draw_width = cos.abs() * width + sin.abs() * height;
        let draw_height = sin.abs() * width + cos.abs() * height;
        let mut values = [0f32; 44];
        values[..4].copy_from_slice(&[
            c.dest_size[0] as f32,
            c.dest_size[1] as f32,
            c.bounds.x as f32 + (width - draw_width) * 0.5,
            c.bounds.y as f32 + (height - draw_height) * 0.5,
        ]);
        values[4..8].copy_from_slice(&[
            c.bounds.width as f32 / scale,
            c.bounds.height as f32 / scale,
            (now as f64 / 1000. / f64::from(m.spec.period_s)).fract() as f32,
            0.731,
        ]);
        values[8..12].copy_from_slice(&events);
        for (i, c) in m.palette.iter().enumerate() {
            values[12 + i * 4..16 + i * 4].copy_from_slice(c);
        }
        values[28..28 + m.spec.params.len()].copy_from_slice(&m.spec.params);
        values[36..40].copy_from_slice(&[
            draw_width,
            draw_height,
            node.opacity * m.spec.intensity,
            0.,
        ]);
        values[40..44].copy_from_slice(&[cos, sin, width, height]);
        let mut bytes = [0u8; 176];
        for (v, b) in values.iter().zip(bytes.as_chunks_mut::<4>().0) {
            b.copy_from_slice(&v.to_ne_bytes());
        }
        let gpu = c.gpu.wgpu();
        let _submission = gpu.lock_submission();
        let pipeline = &prepared.pipeline;
        let binding = prepared.uniforms.binding(
            c.gpu,
            &pipeline.layout,
            &prepared.textures,
            &pipeline.sampler,
            super::effect_uniforms::output_key(&c, scale),
            bytes.len(),
            Some(&prepared.slots),
        );
        gpu.queue().write_buffer(&binding.buffer, 0, &bytes);
        if let Some(w) = c.gpu_work {
            w.record_upload(bytes.len());
            w.record_draw_call();
        }
        let raw = pass.wgpu();
        raw.set_pipeline(&pipeline.pipeline);
        raw.set_bind_group(0, &binding.group, &[]);
        raw.draw(0..6, 0..1);
        let hash = m.program.hash.clone();
        let cost = u64::from(m.program.cost.max(1))
            * u64::from(c.bounds.width)
            * u64::from(c.bounds.height);
        state
            .last_drawn
            .insert((c.gpu.generation(), hash.clone()), instant);
        state.spend(&hash, cost, animated, instant);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_program_drawn_on_two_lost_devices_is_disabled() {
        let mut state = State::default();
        let t = Instant::now();
        let lost_once = |at| {
            vec![
                ("loss-culprit".to_owned(), at + LOSS_WINDOW * 2),
                // Last drawn long before the loss: not a suspect.
                ("loss-bystander".to_owned(), at),
            ]
        };
        state.strike(lost_once(t));
        assert_eq!(state.failed.get("loss-culprit"), Some(&1));
        assert_eq!(state.failed.get("loss-bystander"), None);
        assert!(!stopped("loss-culprit"), "one loss is not proof");
        state.strike(lost_once(t));
        assert!(stopped("loss-culprit") && !stopped("loss-bystander"));
        assert!(
            take_notices()
                .iter()
                .any(|n| n.contains("loss-culpri") && n.contains("lost twice")),
        );
        // No draws on the lost device: nobody is charged.
        state.strike(vec![]);
    }
    #[test]
    fn the_heaviest_program_is_frozen_then_disabled_over_budget() {
        let spend = |cost, animated| Spend { cost, animated };
        let mut spent = HashMap::from([
            ("light".to_owned(), spend(10, true)),
            ("heavy".to_owned(), spend(80, true)),
        ]);
        assert_eq!(budget_verdict(&spent, 100), None);
        spent.insert("light".into(), spend(30, true));
        assert_eq!(
            budget_verdict(&spent, 100),
            Some(("heavy", Verdict::Freeze))
        );
        spent.insert("heavy".into(), spend(80, false));
        assert_eq!(
            budget_verdict(&spent, 100),
            Some(("heavy", Verdict::Disable))
        );
        spent.insert("huge".into(), spend(u64::MAX, true));
        assert_eq!(budget_verdict(&spent, 100), Some(("huge", Verdict::Freeze)));
        // Every built-in, at its cost over a 3840×2160 canvas at 60 fps,
        // stays inside the budget.
        let registry = nanatype_theme::SkinRegistry::new(None, "0.1.0");
        for entry in registry.entries() {
            let skin = registry.load(&entry.id).unwrap().skin;
            for program in skin.shader_programs.values() {
                let frame = u64::from(program.cost) * 3840 * 2160 * 60;
                assert!(frame < DEFAULT_COST_BUDGET, "{}: {}", entry.id, program.cost);
            }
        }
    }
    fn ui() -> Stage {
        let skin = nanatype_theme::SkinRegistry::new(None, "0.1.0")
            .load("nanatype.tidal")
            .unwrap()
            .skin;
        Stage::new(
            Arc::new(nanatype_settings::Settings::default()),
            skin,
            nanatype_i18n::Locale::ZhCn,
        )
    }
    #[test]
    fn events_are_shared_by_outputs_and_not_retriggered_by_repaints() {
        let ui = ui();
        ui.display.update(|v| {
            let v = Arc::make_mut(v);
            v.empty = false;
            v.line = Some(nanatype_model::LineId(100));
            v.speaker.name = "A".into();
        });
        assert_eq!(&event_values(ui, 1000)[..2], &[0., 0.]);
        assert_eq!(&event_values(ui, 1500)[..2], &[0.5, 0.5]);
        assert_eq!(event_values(ui, 1500), event_values(ui, 1500));
        ui.display.update(|v| {
            let v = Arc::make_mut(v);
            v.line = Some(nanatype_model::LineId(101));
            v.continue_indicator = true;
        });
        assert_eq!(event_values(ui, 2000), [0., 1., 0., 1.]);
        ui.peer_display.update(|v| {
            let v = Arc::make_mut(v);
            v.empty = false;
            v.line = Some(nanatype_model::LineId(900));
            v.speaker.name = "B".into();
        });
        assert_eq!(&event_values(ui.remote(), 2000)[..2], &[0., 0.]);
        assert_eq!(&event_values(ui, 2500)[..2], &[0.5, 1.5]);
    }
    #[test]
    fn slots_pack_params_as_vec4s_in_the_palette_colour_space() {
        use nanatype_theme::{Resolved, SlotBinding, SlotComponent};
        let values: std::collections::BTreeMap<String, Resolved> = [
            ("size".to_owned(), Resolved::Number(36.)),
            ("on".to_owned(), Resolved::Bool(true)),
            (
                "kind".to_owned(),
                Resolved::Enum {
                    id: "dog".into(),
                    index: 2,
                },
            ),
            (
                "fur".to_owned(),
                Resolved::Color(nanatype_model::Rgba8::rgba(255, 128, 0, 64)),
            ),
        ]
        .into();
        let slots = pack_slots(
            &[
                SlotBinding::Tuple(vec![
                    SlotComponent::Param("size".into()),
                    SlotComponent::Number(4.),
                    SlotComponent::Param("on".into()),
                    SlotComponent::Param("kind".into()),
                ]),
                SlotBinding::Param("fur".into()),
                SlotBinding::Number(7.),
                SlotBinding::Param("missing".into()),
            ],
            &values,
        );
        assert_eq!(&slots[..4], &[36., 4., 1., 2.]);
        assert_eq!(
            &slots[4..8],
            &color(nanatype_model::Rgba8::rgba(255, 128, 0, 64))
        );
        assert_eq!(&slots[8..12], &[7., 0., 0., 0.]);
        assert!(slots[12..].iter().all(|v| *v == 0.));
    }
    #[test]
    fn an_artwork_owns_the_continue_mark_only_while_it_plays() {
        let ui = ui();
        let mut skin = (*ui.skin.get_untracked()).clone();
        let artwork = skin
            .dialogue
            .shader_effects
            .iter()
            .find(|e| e.target == ShaderTarget::Artwork)
            .map(|e| e.shader.clone())
            .expect("tidal has an artwork");
        assert!(!owns_continue(ui, Source::Display, &skin));
        skin.shaders.get_mut(&artwork).unwrap().owns_continue = true;
        assert!(owns_continue(ui, Source::Display, &skin));
        // A preview owns it only while it plays effects.
        assert!(!owns_continue(ui, Source::Preview, &skin));
        ui.reduced_motion.set(true);
        assert!(!owns_continue(ui, Source::Display, &skin));
        ui.reduced_motion.set(false);
        ui.settings
            .update(|s| Arc::make_mut(s).skin.advanced_effects = false);
        assert!(!owns_continue(ui, Source::Display, &skin));
    }
    #[test]
    fn only_enabled_nonempty_display_tracks_request_frames() {
        let ui = ui();
        assert!(!animating(ui));
        ui.display.update(|v| Arc::make_mut(v).empty = false);
        assert!(animating(ui));
        ui.reduced_motion.set(true);
        assert!(!animating(ui));
        ui.reduced_motion.set(false);
        ui.settings
            .update(|s| Arc::make_mut(s).skin.advanced_effects = false);
        assert!(!animating(ui));
        assert!(!preview_animating(ui));
    }
    #[test]
    fn printed_classics_have_no_animation_frame_schedule() {
        let ui = ui();
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        ui.display.update(|v| Arc::make_mut(v).empty = false);
        for name in [
            "halo",
            "parchment",
            "adv",
            "vanilla",
            "comic",
            "ink",
            "echo_vanilla",
            "echo_glass",
            "echo_bubble",
            "subtitle",
            "echo_void",
        ] {
            let skin = registry.load(&format!("nanatype.{name}")).unwrap().skin;
            assert!(skin.shaders.values().all(|s| !s.animated));
            assert!(skin.dialogue.shader_effects.is_empty());
            ui.skin.set(skin);
            for size in [[952., 252.], [300., 1200.], [300., 240.], [160., 96.]] {
                ui.display_size.set(size);
                assert!(!animating(ui), "{name} {size:?}");
            }
        }
    }

    #[test]
    fn frozen_authored_artwork_does_not_request_an_animation_loop() {
        let ui = ui();
        let registry = nanatype_theme::SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        ui.display.update(|v| Arc::make_mut(v).empty = false);
        for name in [
            "gothic",
            "celestial",
            "forest",
            "tidal",
            "lantern",
            "citrus",
            "crayon",
        ] {
            ui.skin
                .set(registry.load(&format!("nanatype.{name}")).unwrap().skin);
            for size in [[952., 252.], [300., 1200.], [300., 240.], [160., 96.]] {
                ui.display_size.set(size);
                ui.reduced_motion.set(true);
                assert!(!animating(ui), "{name} {size:?}: reduced motion");
                ui.reduced_motion.set(false);
                ui.settings
                    .update(|s| Arc::make_mut(s).skin.advanced_effects = false);
                assert!(!animating(ui), "{name} {size:?}: effects disabled");
                ui.settings
                    .update(|s| Arc::make_mut(s).skin.advanced_effects = true);
                assert_eq!(animating(ui), name != "crayon", "{name} {size:?}");
            }
        }
    }

    #[test]
    fn border_coverage_leaves_the_center_clear() {
        let surface = Surface {
            fill: Some([0.; 4]),
            border: Some(super::super::surface::Border {
                widths: [3.; 4],
                color: [1.; 4],
                ..Default::default()
            }),
            radii: [18.; 4],
            ..Default::default()
        };
        let mask = super::super::sweep::coverage_for(&surface, [160, 80], 1., true);
        assert_eq!(mask[(40 * 160 + 80) * 4 + 3], 0);
        assert!(mask[(160 + 80) * 4 + 3] > 100);
        assert_eq!(mask[3], 0);
    }

    #[test]
    fn framed_border_masks_do_not_shade_the_nine_slice_center() {
        let path = std::env::temp_dir().join(format!(
            "nanatype-shader-frame-{}-{}.png",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        image::RgbaImage::from_pixel(16, 16, image::Rgba([255; 4]))
            .save(&path)
            .unwrap();
        let mut frame = nana_ui_core::BorderImageSpec::from_source(
            nana_ui_core::BackgroundImage::url(crate::assets::file_url(&path)),
        );
        frame.slice = [nana_ui_core::BorderImageSlice::Number(4.); 4];
        frame.fill = true;
        let surface = Surface {
            frame: Some((frame, [4.; 4])),
            ..Default::default()
        };
        let full = super::super::sweep::coverage_for(&surface, [80, 40], 1., false);
        let edge = super::super::sweep::coverage_for(&surface, [80, 40], 1., true);
        let _ = std::fs::remove_file(&path);
        assert_eq!(full[(20 * 80 + 40) * 4 + 3], 255);
        assert_eq!(edge[(20 * 80 + 40) * 4 + 3], 0);
        assert_eq!(edge[(80 + 40) * 4 + 3], 255);
    }
}
