//! GPU renderer, R8 coverage upload, and pooled rest-frame baking.
use nana_ui::runtime::CustomRenderNode;
use nana_ui::{
    DeviceGeneration, GpuContext, GpuTextureFormat, SceneGpuNode, SceneGpuPassContext,
    SceneGpuPrepareContext, SceneGpuRenderContext, SceneGpuRenderer, SceneGpuRendererRegistry,
    ScenePass, ScenePresentationProfile, wgpu,
};
use nanatype_theme::MAX_SHADER_CACHE_BYTES;
use std::collections::{HashMap, HashSet};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, LazyLock, Mutex, Weak};
use std::time::Instant;

use super::super::dialogue::Source;
use super::super::effect_uniforms::{Scales, Uniforms, allocations, f32_bytes, output_key};
use super::super::effects::LruMap;
use super::compiler::{
    Blit, COMPILED, Pending, Pipeline, REFUSED, active_compiles, blit_pipeline, try_compile_async,
};
use super::policy::{MaterialGate, store};
use super::{
    CacheStats, DRAFT_EVENTS, MATERIALS, Material, RENDERER, REST_EVENTS, SLOT_BYTES, event_values,
    playing, stepped,
};

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

struct Prepared {
    material: Weak<Material>,
    uniforms: Uniforms,
    /// Pooled uniforms for rest-frame bakes (eliminates throwaway buffer/bindgroup allocs).
    bake_uniforms: Uniforms,
    pipeline: Arc<Pipeline>,
    textures: [wgpu::TextureView; 3],
    /// The material's param slots, written once when prepared.
    slots: wgpu::Buffer,
    bytes: usize,
    /// Frames drawn at rest, newest last, copied instead of shading again.
    frozen: Vec<Frozen>,
}

impl Prepared {
    fn held(&self) -> usize {
        self.bytes
            + self.uniforms.bytes()
            + self.bake_uniforms.bytes()
            + self
                .frozen
                .iter()
                .map(|f| f.bytes + f.blits.bytes())
                .sum::<usize>()
    }
}

/// Rest frames kept per prepared material: one per output size in use.
const FROZEN_PER_MASK: usize = 4;
/// What a rest frame's pixels depend on beyond its material: bounds size,
/// DPI scale and the sub-pixel origin of the drawn quad.
type FrozenKey = [u32; 5];

/// One rest frame: the program drawn once, frozen, at full opacity into a
/// texture of the destination's format. Node opacity is applied when copied.
struct Frozen {
    key: FrozenKey,
    view: wgpu::TextureView,
    bytes: usize,
    blits: Uniforms,
}

/// Where a rest frame sits: the quad the program covers, snapped outward to
/// whole destination pixels so the copy is texel-exact.
struct Shot {
    key: FrozenKey,
    /// Top-left of the texture in destination pixels.
    at: [f32; 2],
    /// The quad's origin inside the texture (the sub-pixel remainder).
    origin: [f32; 2],
    size: [u32; 2],
}

/// The rotated quad the program covers: `(cos, sin, width, height)`.
fn quad(m: &Material, width: f32, height: f32) -> (f32, f32, f32, f32) {
    let (sin, cos) = m.surface.rotate_deg.to_radians().sin_cos();
    (
        cos,
        sin,
        cos.abs() * width + sin.abs() * height,
        sin.abs() * width + cos.abs() * height,
    )
}

fn shot(m: &Material, c: &SceneGpuPassContext<'_>, scale: f32) -> Shot {
    let width = c.bounds.width as f32;
    let height = c.bounds.height as f32;
    let (_, _, draw_width, draw_height) = quad(m, width, height);
    let x = c.bounds.x as f32 + (width - draw_width) * 0.5;
    let y = c.bounds.y as f32 + (height - draw_height) * 0.5;
    let at = [x.floor(), y.floor()];
    let origin = [x - at[0], y - at[1]];
    Shot {
        key: [
            c.bounds.width,
            c.bounds.height,
            scale.to_bits(),
            origin[0].to_bits(),
            origin[1].to_bits(),
        ],
        at,
        origin,
        size: [
            ((origin[0] + draw_width).ceil() as u32).max(1),
            ((origin[1] + draw_height).ceil() as u32).max(1),
        ],
    }
}

/// The program's uniforms for a quad whose top-left sits at `origin` of a
/// `dest`-sized target.
#[allow(clippy::too_many_arguments)]
fn effect_uniforms(
    m: &Material,
    dest: [f32; 2],
    origin: [f32; 2],
    width: f32,
    height: f32,
    scale: f32,
    opacity: f32,
    now: u64,
    events: [f32; 4],
) -> [u8; 176] {
    let (cos, sin, draw_width, draw_height) = quad(m, width, height);
    let mut values = [0f32; 44];
    values[..4].copy_from_slice(&[dest[0], dest[1], origin[0], origin[1]]);
    values[4..8].copy_from_slice(&[
        width / scale,
        height / scale,
        (now as f64 / 1000. / f64::from(m.spec.period_s)).fract() as f32,
        0.731,
    ]);
    values[8..12].copy_from_slice(&events);
    for (i, c) in m.palette.iter().enumerate() {
        values[12 + i * 4..16 + i * 4].copy_from_slice(c);
    }
    values[28..28 + m.spec.params.len()].copy_from_slice(&m.spec.params);
    values[36..40].copy_from_slice(&[draw_width, draw_height, opacity * m.spec.intensity, 0.]);
    values[40..44].copy_from_slice(&[cos, sin, width, height]);
    f32_bytes(&values)
}

static REST_FRAMES: AtomicBool = AtomicBool::new(true);

/// Turn rest frames off (shade frozen programs on every draw, for tests that
/// compare the two); returns the previous setting.
#[doc(hidden)]
pub fn set_rest_frames(on: bool) -> bool {
    REST_FRAMES.swap(on, Ordering::Relaxed)
}

struct State {
    pipelines: HashMap<PipelineKey, Pending>,
    masks: LruMap<MaskKey, Prepared>,
    blits: LruMap<(DeviceGeneration, GpuTextureFormat), Arc<Blit>>,
    scales: Scales,
    reported: HashSet<PipelineKey>,
}

impl Default for State {
    fn default() -> Self {
        Self {
            pipelines: HashMap::new(),
            masks: LruMap::new(32),
            blits: LruMap::new(16),
            scales: Scales::default(),
            reported: HashSet::new(),
        }
    }
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
    nana_ui::set_material_policy(Some(Arc::new(MaterialGate)));
}

pub(super) fn clear_reported() {
    SHARED
        .state
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .reported
        .clear();
}

/// Release dead view bindings even when the next theme is entirely static.
pub fn collect_garbage() {
    let mut state = SHARED.state.lock().unwrap_or_else(|e| e.into_inner());
    state.masks.retain(|_, m| m.material.strong_count() > 0);
    state.scales.retain(|(_, id, _)| MATERIALS.contains(*id));
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
            + state.masks.values().map(|m| m.held()).sum::<usize>(),
        uniforms: state.masks.values().map(|m| m.uniforms.len()).sum(),
        scales: state.scales.len(),
        compilations: COMPILED.load(Ordering::Relaxed),
        refused: REFUSED.load(Ordering::Relaxed),
        uniform_allocations: allocations(),
        frozen: state.masks.values().map(|m| m.frozen.len()).sum(),
        pending: state
            .pipelines
            .values()
            .filter(|p| p.value.get().is_none())
            .count(),
    }
}

impl State {
    fn held(&self) -> usize {
        self.pipelines
            .values()
            .map(|p| p.texture_bytes)
            .sum::<usize>()
            + self.masks.values().map(Prepared::held).sum::<usize>()
    }

    fn blit(&mut self, gpu: &GpuContext, format: GpuTextureFormat) -> Arc<Blit> {
        let key = (gpu.generation(), format);
        if let Some(blit) = self.blits.get(&key) {
            return blit.clone();
        }
        let blit = Arc::new(blit_pipeline(gpu.wgpu().device(), format));
        self.blits.insert(key, blit.clone());
        blit
    }

    fn retire_generation(&mut self, retired: DeviceGeneration) {
        self.pipelines.retain(|(g, _, _, _), _| *g != retired);
        self.masks.retain(|(g, _, _, _, _, _), _| *g != retired);
        self.blits.retain(|(g, _), _| *g != retired);
        self.scales.retain(|(g, _, _)| *g != retired);
    }

    fn prepare(
        &mut self,
        material: &Arc<Material>,
        c: &SceneGpuPassContext<'_>,
        scale: f32,
        disabled: bool,
        prebuilt_mask: Option<Vec<u8>>,
        compile_failure: &mut Option<(String, String)>,
    ) -> Option<&mut Prepared> {
        let generation = c.gpu.generation();
        let bounds = [c.bounds.width, c.bounds.height];
        if bounds.contains(&0)
            || bounds
                .iter()
                .any(|x| *x > 8192.min(c.gpu.wgpu().device().limits().max_texture_dimension_2d))
        {
            return None;
        }
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
        self.scales.retain(|(_, id, _)| MATERIALS.contains(*id));
        self.masks.retain(|_, m| m.material.strong_count() > 0);
        // R8Unorm uses 1 byte per texel for coverage.
        let mask_bytes = extent[0] as usize * extent[1] as usize;
        if extent.contains(&0)
            || extent
                .iter()
                .any(|x| *x > 8192.min(c.gpu.wgpu().device().limits().max_texture_dimension_2d))
            || mask_bytes > MAX_SHADER_CACHE_BYTES / 2
        {
            return None;
        }
        if disabled {
            return None;
        }
        let pk = (
            generation,
            c.target_format,
            c.presentation,
            material.program.hash.clone(),
        );
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
                let live: HashSet<String> = MATERIALS
                    .snapshot()
                    .into_iter()
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
            let pending = try_compile_async(
                c.gpu.wgpu().device().clone(),
                c.target_format,
                material.program.clone(),
                texture_bytes,
            )?;
            self.pipelines.insert(pk.clone(), pending);
        }
        let pending = self.pipelines.get(&pk)?;
        let pipeline = match pending.value.get()? {
            Ok(p) => p,
            Err(e) => {
                if self.reported.insert(pk.clone()) {
                    *compile_failure = Some((material.program.hash.clone(), e.clone()));
                }
                return None;
            }
        };
        let texture_used = self
            .pipelines
            .values()
            .map(|p| p.texture_bytes)
            .sum::<usize>();
        while self.masks.len() >= 32
            || self.masks.values().map(Prepared::held).sum::<usize>()
                + texture_used
                + mask_bytes
                + 8 * 176
                + SLOT_BYTES
                > MAX_SHADER_CACHE_BYTES
        {
            self.masks.evict_oldest()?;
        }
        let mask = prebuilt_mask.unwrap_or_else(|| build_coverage(material, extent, scale));
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
            format: wgpu::TextureFormat::R8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let upload =
            |texture: &wgpu::Texture, size: [u32; 2], bytes_per_pixel: u32, bytes: &[u8]| {
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
                        bytes_per_row: Some(size[0] * bytes_per_pixel),
                        rows_per_image: Some(size[1]),
                    },
                    wgpu::Extent3d {
                        width: size[0],
                        height: size[1],
                        depth_or_array_layers: 1,
                    },
                );
            };
        upload(&mask_texture, extent, 1, &mask);
        let slots = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("skin shader param slots"),
            size: SLOT_BYTES as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let slot_bytes: [u8; SLOT_BYTES] = f32_bytes(&material.slots);
        gpu.queue().write_buffer(&slots, 0, &slot_bytes);
        // Textures are immutable; upload once with their first prepared mask.
        let first = !pipeline.uploaded.swap(true, Ordering::Relaxed);
        if first {
            for (i, t) in pipeline.textures.iter().enumerate() {
                if let Some(source) = material.program.textures.get(i) {
                    upload(t, source.size, 4, &source.bytes);
                } else {
                    upload(t, [1, 1], 4, &[0; 4]);
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
                bake_uniforms: Default::default(),
                textures: [
                    mask_texture.create_view(&Default::default()),
                    pipeline.textures[0].create_view(&Default::default()),
                    pipeline.textures[1].create_view(&Default::default()),
                ],
                pipeline: pipeline.clone(),
                slots,
                bytes: mask_bytes + SLOT_BYTES,
                frozen: Vec::new(),
            },
        );
        self.masks.get_mut(&key)
    }
}

fn build_coverage(material: &Material, extent: [u32; 2], scale: f32) -> Vec<u8> {
    if material.full_coverage {
        vec![255; 1]
    } else {
        super::super::sweep::coverage_for(
            &material.surface,
            extent,
            scale,
            material.surface.fill == Some([0.; 4]) && material.surface.gradient.is_none(),
        )
    }
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
        self.bake(node, &p, c.wgpu_encoder());
        c.with_pass("skin shader", |pass| {
            self.draw(node, pass, p, false);
        });
    }

    fn draw_in_pass(
        &self,
        node: &SceneGpuNode,
        pass: &mut ScenePass<'_, '_>,
        c: SceneGpuPassContext<'_>,
    ) -> bool {
        self.draw(node, pass, c, true)
    }
}

fn material_of(node: &SceneGpuNode) -> Option<Arc<Material>> {
    let id = node.custom.resource.parse::<u64>().ok()?;
    MATERIALS.get(id)
}

/// Whether `m` draws anything: frozen programs only with a static fallback,
/// and the display only while it shows a line.
fn draws(m: &Material, animated: bool) -> bool {
    (animated || m.spec.static_fallback)
        && !(m.source == Source::Display && m.ui.display.with_untracked(|v| v.empty))
}

/// The paint-only scale of `m` in this output.
fn scale_of(state: &mut State, m: &Material, c: &SceneGpuPassContext<'_>) -> f32 {
    let registered = state.scales.get(c.gpu.generation(), m.id, c.dest_size);
    let logical = *m.logical_size.lock().unwrap_or_else(|e| e.into_inner());
    if logical[0] > 0.0 && logical[1] > 0.0 {
        (c.bounds.width as f32 / logical[0])
            .min(c.bounds.height as f32 / logical[1])
            .max(0.0001)
    } else {
        registered
    }
}

/// Bytes a rest frame for `shot` takes, if it fits beside what the other
/// materials hold. `others` excludes this material's own rest frames, which
/// a new one may replace.
fn rest_bytes(c: &SceneGpuPassContext<'_>, shot: &Shot, others: usize) -> Option<usize> {
    let max = 8192.min(c.gpu.wgpu().device().limits().max_texture_dimension_2d);
    if shot.size.iter().any(|x| *x > max) {
        return None;
    }
    let texel = c.target_format.wgpu().block_copy_size(None).unwrap_or(8) as usize;
    let bytes = shot.size[0] as usize * shot.size[1] as usize * texel;
    (others + bytes <= MAX_SHADER_CACHE_BYTES).then_some(bytes)
}

fn own_rest_bytes(prepared: &Prepared) -> usize {
    prepared
        .frozen
        .iter()
        .map(|f| f.bytes + f.blits.bytes())
        .sum()
}

fn prebuild_coverage_if_missing(
    renderer: &Renderer,
    m: &Material,
    c: &SceneGpuPassContext<'_>,
) -> Option<(f32, Vec<u8>)> {
    if m.full_coverage {
        return None;
    }
    let bounds = [c.bounds.width, c.bounds.height];
    if bounds.contains(&0) {
        return None;
    }
    let (scale, needs) = {
        let mut state = renderer.state.lock().unwrap_or_else(|e| e.into_inner());
        let scale = scale_of(&mut state, m, c);
        let key = (
            c.gpu.generation(),
            c.target_format,
            c.presentation,
            m.id,
            bounds,
            scale.to_bits(),
        );
        (scale, !state.masks.contains_key(&key))
    };
    needs.then(|| (scale, build_coverage(m, bounds, scale)))
}

impl Renderer {
    /// Draw `node` into `pass`. A frozen program is shaded once into a rest
    /// frame and copied from then on; with no rest frame yet and `defer`,
    /// return `false` so the painter calls `render`, which bakes one first.
    fn draw(
        &self,
        node: &SceneGpuNode,
        pass: &mut ScenePass<'_, '_>,
        c: SceneGpuPassContext<'_>,
        defer: bool,
    ) -> bool {
        let Some(m) = material_of(node) else {
            return true;
        };
        let animated = playing(&m);
        if !draws(&m, animated) {
            return true;
        }
        let (disabled, retired) = {
            let mut policy = store();
            let retired = policy.observe(c.gpu.generation(), Some(c.gpu));
            (policy.is_disabled(&m.program.hash), retired)
        };
        let prebuilt = if disabled {
            None
        } else {
            prebuild_coverage_if_missing(self, &m, &c).map(|(_, bytes)| bytes)
        };
        let mut compile_failure: Option<(String, String)> = None;
        let instant = Instant::now();
        let cost = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(retired) = retired {
                state.retire_generation(retired);
            }
            if disabled {
                return true;
            }
            let scale = scale_of(&mut state, &m, &c);
            let held = state.held();
            let blit = (!animated && REST_FRAMES.load(Ordering::Relaxed))
                .then(|| state.blit(c.gpu, c.target_format));
            let Some(prepared) =
                state.prepare(&m, &c, scale, disabled, prebuilt, &mut compile_failure)
            else {
                m.warming.store(
                    m.spec.static_fallback && active_compiles() > 0,
                    Ordering::Relaxed,
                );
                drop(state);
                if let Some((hash, err)) = compile_failure {
                    store().record_compile_error(&hash, &err);
                }
                return true;
            };
            m.warming.store(false, Ordering::Relaxed);
            if let Some(blit) = blit {
                let shot = shot(&m, &c, scale);
                if let Some(i) = prepared.frozen.iter().position(|f| f.key == shot.key) {
                    let mut frozen = prepared.frozen.remove(i);
                    copy_rest(&mut frozen, &blit, pass, &c, &shot, scale, node.opacity);
                    prepared.frozen.push(frozen);
                    return true;
                }
                let others = held.saturating_sub(own_rest_bytes(prepared));
                if defer && rest_bytes(&c, &shot, others).is_some() {
                    return false;
                }
            } else if !prepared.frozen.is_empty() {
                // Playing again (or rest frames are off): they are stale memory.
                prepared.frozen.clear();
            }
            if !store().draw_guard(&m.program.hash, instant) {
                return true;
            }
            let now = if animated {
                crate::clock::frame_ms()
            } else {
                0
            };
            let events = if !animated {
                REST_EVENTS
            } else if m.source == Source::Draft {
                DRAFT_EVENTS
            } else {
                event_values(m.ui, m.source, now)
            };
            let width = c.bounds.width as f32;
            let height = c.bounds.height as f32;
            let (_, _, draw_width, draw_height) = quad(&m, width, height);
            let bytes = effect_uniforms(
                &m,
                [c.dest_size[0] as f32, c.dest_size[1] as f32],
                [
                    c.bounds.x as f32 + (width - draw_width) * 0.5,
                    c.bounds.y as f32 + (height - draw_height) * 0.5,
                ],
                width,
                height,
                scale,
                node.opacity,
                stepped(&m.spec, now),
                events,
            );
            let gpu = c.gpu.wgpu();
            let pipeline = &prepared.pipeline;
            let binding = prepared.uniforms.binding(
                c.gpu,
                &pipeline.layout,
                &prepared.textures,
                &pipeline.sampler,
                output_key(&c, scale),
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
            u64::from(m.program.cost.max(1))
                * u64::from(c.bounds.width)
                * u64::from(c.bounds.height)
        };
        store().note_draw(
            c.gpu.generation(),
            m.program.hash.clone(),
            cost,
            animated,
            instant,
        );
        true
    }

    /// Shade a frozen program once into a rest frame, recorded on `encoder`
    /// ahead of the pass that copies it. Charged like one ordinary draw.
    fn bake(
        &self,
        node: &SceneGpuNode,
        c: &SceneGpuPassContext<'_>,
        encoder: &mut wgpu::CommandEncoder,
    ) {
        let Some(m) = material_of(node) else {
            return;
        };
        if playing(&m) || !draws(&m, false) || !REST_FRAMES.load(Ordering::Relaxed) {
            return;
        }
        let (disabled, retired) = {
            let mut policy = store();
            let retired = policy.observe(c.gpu.generation(), Some(c.gpu));
            (policy.is_disabled(&m.program.hash), retired)
        };
        if disabled {
            return;
        }
        let prebuilt = prebuild_coverage_if_missing(self, &m, c).map(|(_, bytes)| bytes);
        let instant = Instant::now();
        let mut compile_failure: Option<(String, String)> = None;
        let baked_cost = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(retired) = retired {
                state.retire_generation(retired);
            }
            let scale = scale_of(&mut state, &m, c);
            let held = state.held();
            let shot = shot(&m, c, scale);
            let Some(prepared) = state.prepare(&m, c, scale, false, prebuilt, &mut compile_failure)
            else {
                drop(state);
                if let Some((hash, err)) = compile_failure {
                    store().record_compile_error(&hash, &err);
                }
                return;
            };
            if prepared.frozen.iter().any(|f| f.key == shot.key) {
                return;
            }
            let own = own_rest_bytes(prepared);
            let Some(bytes) = rest_bytes(c, &shot, held.saturating_sub(own)) else {
                return;
            };
            if held + bytes > MAX_SHADER_CACHE_BYTES {
                // Older sizes of this material make room for the one in use.
                prepared.frozen.clear();
            }
            if !store().draw_guard(&m.program.hash, instant) {
                return;
            }
            let gpu = c.gpu.wgpu();
            let device = gpu.device();
            let texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("skin rest frame"),
                size: wgpu::Extent3d {
                    width: shot.size[0],
                    height: shot.size[1],
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: c.target_format.wgpu(),
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                    | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let view = texture.create_view(&Default::default());
            // Full opacity: the copy applies the node's.
            let uniforms = effect_uniforms(
                &m,
                [shot.size[0] as f32, shot.size[1] as f32],
                shot.origin,
                c.bounds.width as f32,
                c.bounds.height as f32,
                scale,
                1.,
                0,
                REST_EVENTS,
            );
            // Reuse the pooled bake_uniforms buffer & BindGroup instead of a throwaway Uniforms::default().
            let bake_key = [
                shot.size[0],
                shot.size[1],
                shot.key[3],
                shot.key[4],
                scale.to_bits(),
            ];
            let binding = prepared.bake_uniforms.binding(
                c.gpu,
                &prepared.pipeline.layout,
                &prepared.textures,
                &prepared.pipeline.sampler,
                bake_key,
                uniforms.len(),
                Some(&prepared.slots),
            );
            gpu.queue().write_buffer(&binding.buffer, 0, &uniforms);
            let mut raw = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("skin rest frame"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            raw.set_pipeline(&prepared.pipeline.pipeline);
            raw.set_bind_group(0, &binding.group, &[]);
            raw.draw(0..6, 0..1);
            drop(raw);
            if let Some(w) = c.gpu_work {
                w.record_realloc();
                w.record_upload(uniforms.len());
                w.record_draw_call();
            }
            if prepared.frozen.len() >= FROZEN_PER_MASK {
                prepared.frozen.remove(0);
            }
            prepared.frozen.push(Frozen {
                key: shot.key,
                view,
                bytes,
                blits: Default::default(),
            });
            u64::from(m.program.cost.max(1)) * u64::from(shot.size[0]) * u64::from(shot.size[1])
        };
        store().note_draw(
            c.gpu.generation(),
            m.program.hash.clone(),
            baked_cost,
            false,
            instant,
        );
    }
}

/// Copy a rest frame into `pass` at the node's opacity.
fn copy_rest(
    frozen: &mut Frozen,
    blit: &Blit,
    pass: &mut ScenePass<'_, '_>,
    c: &SceneGpuPassContext<'_>,
    shot: &Shot,
    scale: f32,
    opacity: f32,
) {
    let values = [
        c.dest_size[0] as f32,
        c.dest_size[1] as f32,
        shot.at[0],
        shot.at[1],
        shot.size[0] as f32,
        shot.size[1] as f32,
        opacity,
        0.,
    ];
    let bytes: [u8; 32] = f32_bytes(&values);
    let gpu = c.gpu.wgpu();
    let binding = frozen.blits.binding(
        c.gpu,
        &blit.layout,
        std::slice::from_ref(&frozen.view),
        &blit.sampler,
        output_key(c, scale),
        bytes.len(),
        None,
    );
    gpu.queue().write_buffer(&binding.buffer, 0, &bytes);
    if let Some(w) = c.gpu_work {
        w.record_upload(bytes.len());
        w.record_draw_call();
    }
    let raw = pass.wgpu();
    raw.set_pipeline(&blit.pipeline);
    raw.set_bind_group(0, &binding.group, &[]);
    raw.draw(0..6, 0..1);
}

#[cfg(test)]
mod tests {
    use super::super::*;
    use super::*;
    use crate::ui::shader_effects::policy::{PolicyStore, Verdict, budget_verdict, stopped};
    use std::time::Duration;

    #[test]
    fn a_program_drawn_on_two_lost_devices_is_disabled() {
        let mut policy = PolicyStore::default();
        let t = Instant::now();
        let lost_once = |at| {
            vec![
                ("loss-culprit".to_owned(), at + Duration::from_secs(4)),
                // Last drawn long before the loss: not a suspect.
                ("loss-bystander".to_owned(), at),
            ]
        };
        policy.strike(lost_once(t));
        assert!(!policy.is_stopped("loss-culprit"), "one loss is not proof");
        assert!(!policy.is_stopped("loss-bystander"));
        policy.strike(lost_once(t));
        assert!(policy.is_disabled("loss-culprit") && !policy.is_stopped("loss-bystander"));
        assert!(
            policy
                .take_notices()
                .iter()
                .any(|n| n.contains("loss-culpri") && n.contains("lost twice")),
        );
        // No draws on the lost device: nobody is charged.
        policy.strike(vec![]);
        let _ = stopped;
    }

    #[test]
    fn the_heaviest_program_is_frozen_then_disabled_over_budget() {
        let spend = |cost, animated| super::super::policy::Spend { cost, animated };
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
            for program in skin
                .shader_programs
                .values()
                .chain(skin.material_programs.values())
            {
                let frame = u64::from(program.cost) * 3840 * 2160 * 60;
                assert!(
                    frame < super::super::policy::DEFAULT_COST_BUDGET,
                    "{}: {}",
                    entry.id,
                    program.cost
                );
            }
        }
    }

    #[test]
    fn border_coverage_leaves_the_center_clear() {
        let surface = Surface {
            fill: Some([0.; 4]),
            border: Some(super::super::super::surface::Border {
                widths: [3.; 4],
                color: [1.; 4],
                ..Default::default()
            }),
            radii: [18.; 4],
            ..Default::default()
        };
        let mask = super::super::super::sweep::coverage_for(&surface, [160, 80], 1., true);
        assert_eq!(mask[40 * 160 + 80], 0);
        assert!(mask[160 + 80] > 100);
        assert_eq!(mask[0], 0);
    }

    #[test]
    fn framed_border_masks_do_not_shade_the_nine_slice_center() {
        let path = std::env::temp_dir().join(format!(
            "nanatype-shader-frame-{}-{}.png",
            std::process::id(),
            MATERIALS.next_id()
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
        let full = super::super::super::sweep::coverage_for(&surface, [80, 40], 1., false);
        let edge = super::super::super::sweep::coverage_for(&surface, [80, 40], 1., true);
        let _ = std::fs::remove_file(&path);
        assert_eq!(full[20 * 80 + 40], 255);
        assert_eq!(edge[20 * 80 + 40], 0);
        assert_eq!(edge[40], 255);
    }
}
