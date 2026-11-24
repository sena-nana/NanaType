//! Finite light attached to retained text geometry. Raster data is immutable
//! between layout changes; animated frames upload only the clock uniforms.
use super::Stage;
use super::effect_uniforms::{Uniforms, f32_bytes, output_key};
use super::effects::{
    LruMap, Outline, PlacementCache, Registry, bind_overlay_layer, capture_text_bounds,
    compiler_pool, linear, overlay_scale_and_crop, parse_resource_id, style_overlay_gpu,
    wrap_text_overlay,
};
use nana_ui::runtime::view::{AnyView, NodeRef, signal};
use nana_ui::runtime::{
    ComponentView, CustomRenderNode, GpuView, MutationQueue, NodeKind, StableNodeId, TextShaper,
    UiWorld,
};
use nana_ui::{
    DeviceGeneration, GpuTextureFormat, SceneGpuNode, SceneGpuPassContext, SceneGpuPrepareContext,
    SceneGpuRenderContext, SceneGpuRenderer, SceneGpuRendererRegistry, ScenePass,
    ScenePresentationProfile, wgpu,
};
use nanatype_theme::TextLightSpec;
use std::sync::{Arc, LazyLock, Mutex, OnceLock, Weak};

const RENDERER: &str = "nanatype.text-light";
const HIDDEN: u32 = 0xff_ffff;
static LIVE: LazyLock<Registry<Light>> = LazyLock::new(Default::default);

#[derive(Debug, Default)]
struct Geometry {
    revision: u64,
    /// Hash of what the paths were built from; an identical re-projection
    /// keeps the revision, and with it the rasterized masks.
    key: Option<blake3::Hash>,
    paths: Arc<[(tiny_skia::Path, u32)]>,
    origin: [f32; 2],
    size: [f32; 2],
}

struct Light {
    id: u64,
    ui: Stage,
    text: NodeRef,
    spec: TextLightSpec,
    born: u64,
    times: Vec<(usize, Option<u64>)>,
    entrance_ms: u16,
    exit: Option<(u64, u16)>,
    preview: bool,
    geometry: Mutex<Geometry>,
}

impl std::fmt::Debug for Light {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextLight").field("id", &self.id).finish()
    }
}

impl Drop for Light {
    fn drop(&mut self) {
        LIVE.remove(self.id);
    }
}

impl Light {
    fn active(&self, now: u64) -> bool {
        if self.preview && (!self.ui.shader_preview.get_untracked() || !self.ui.live_preview.get())
        {
            return false;
        }
        if !super::materials::enabled(self.ui) || self.ui.reduced_motion.get_untracked() {
            return false;
        }
        if let Some((at, ms)) = self.exit
            && now >= at
        {
            return now < at + u64::from(ms);
        }
        self.times.iter().any(|(_, at)| {
            at.is_some_and(|t| t > 0 && now >= t && now < t + u64::from(self.entrance_ms))
        })
    }

    fn time(&self, byte: usize) -> u32 {
        let end = self.times.partition_point(|(b, _)| *b <= byte);
        let at = end.checked_sub(1).and_then(|i| self.times[i].1);
        match at {
            None => HIDDEN,
            Some(0) => 0,
            Some(at) => (at.saturating_sub(self.born) + 1).min(u64::from(HIDDEN - 1)) as u32,
        }
    }

    fn capture(&self, world: &UiWorld, layer: StableNodeId) {
        let Some((layout, origin, size)) = capture_text_bounds(world, self.text, layer) else {
            return;
        };
        let Some(engine) = nana_ui::NanaTextShaper::default().text_engine() else {
            return;
        };
        let engine = engine.lock().unwrap_or_else(|e| e.into_inner());
        // Place every glyph first (cheap), then outline only when the
        // placement changed: re-projections at the same layout are common
        // and each new revision re-rasterizes the light's masks.
        let mut faces = Vec::new();
        let mut placed: Vec<Placed> = Vec::new();
        let vertical = layout.is_vertical();
        let physical_w = layout.physical_size().0;
        for line in &layout.lines {
            let centre = layout.physical_x_of_block(line.metrics.baseline_y_px, physical_w);
            for run in layout.line_runs(line) {
                if run.is_object() {
                    continue;
                }
                let Some(data) = engine.fonts().face_data(run.font) else {
                    continue;
                };
                let Ok(face) = ttf_parser::Face::parse(data.bytes(), data.index()) else {
                    continue;
                };
                let scale = run.font_size_px / f32::from(face.units_per_em());
                faces.push(data.clone());
                let font = faces.len() - 1;
                let sideways = vertical && run.orientation == nana_text::RunOrientation::Sideways;
                let mut pen = run.origin_x_px;
                for glyph in &run.glyphs {
                    let transform = if sideways {
                        tiny_skia::Transform::from_row(
                            0.,
                            scale,
                            scale,
                            0.,
                            centre - (run.metrics.ascent_px - run.metrics.descent_px) * 0.5
                                + glyph.offset_y_px,
                            pen + glyph.offset_x_px,
                        )
                    } else if vertical {
                        tiny_skia::Transform::from_row(
                            scale,
                            0.,
                            0.,
                            -scale,
                            centre + glyph.offset_x_px,
                            pen - glyph.offset_y_px,
                        )
                    } else {
                        tiny_skia::Transform::from_row(
                            scale,
                            0.,
                            0.,
                            -scale,
                            pen + glyph.offset_x_px,
                            line.metrics.baseline_y_px - glyph.offset_y_px,
                        )
                    };
                    placed.push(Placed {
                        font,
                        glyph: glyph.glyph_id as u16,
                        transform,
                        at: self.time(glyph.cluster as usize),
                    });
                    pen += glyph.advance_px;
                }
            }
        }
        // Ruby follows its base unit's reveal clock.
        for ruby in &layout.rubies {
            for run in &ruby.runs {
                let Some(data) = engine.fonts().face_data(run.font) else {
                    continue;
                };
                let Ok(face) = ttf_parser::Face::parse(data.bytes(), data.index()) else {
                    continue;
                };
                let scale = run.font_size_px / f32::from(face.units_per_em());
                faces.push(data.clone());
                let font = faces.len() - 1;
                for (pen, glyph) in run.glyph_cells() {
                    placed.push(Placed {
                        font,
                        glyph: glyph.glyph_id as u16,
                        transform: tiny_skia::Transform::from_row(
                            scale,
                            0.,
                            0.,
                            -scale,
                            pen + glyph.offset_x_px,
                            ruby.baseline_y_px - glyph.offset_y_px,
                        ),
                        at: self.time(ruby.range.start),
                    });
                }
            }
        }
        let faces: Vec<_> = faces.iter().map(|d| (d.bytes(), d.index())).collect();
        let key = placement_key(origin, size, &faces, &placed);
        let mut geometry = self.geometry.lock().unwrap_or_else(|e| e.into_inner());
        if geometry.key == Some(key) {
            return;
        }
        geometry.key = Some(key);
        geometry.origin = origin;
        geometry.size = size;
        geometry.paths = outline_all(&faces, &placed).into();
        geometry.revision += 1;
    }
}

/// One glyph to outline: its face (an index into the captured fonts),
/// glyph id, transform and reveal time.
struct Placed {
    font: usize,
    glyph: u16,
    transform: tiny_skia::Transform,
    at: u32,
}

type Faces<'a> = [(&'a [u8], u32)];

fn placement_key(
    origin: [f32; 2],
    size: [f32; 2],
    faces: &Faces<'_>,
    placed: &[Placed],
) -> blake3::Hash {
    let mut hash = blake3::Hasher::new();
    for v in origin.into_iter().chain(size) {
        hash.update(&v.to_bits().to_le_bytes());
    }
    for p in placed {
        let t = p.transform;
        let (bytes, index) = faces[p.font];
        hash.update(&(bytes.as_ptr() as usize).to_le_bytes());
        hash.update(&index.to_le_bytes());
        hash.update(&p.glyph.to_le_bytes());
        for v in [t.sx, t.kx, t.ky, t.sy, t.tx, t.ty] {
            hash.update(&v.to_bits().to_le_bytes());
        }
        hash.update(&p.at.to_le_bytes());
    }
    hash.finalize()
}

fn outline_all(faces: &Faces<'_>, placed: &[Placed]) -> Vec<(tiny_skia::Path, u32)> {
    let mut paths = Vec::with_capacity(placed.len());
    let mut face: Option<(usize, ttf_parser::Face<'_>)> = None;
    for p in placed {
        if face.as_ref().is_none_or(|(at, _)| *at != p.font) {
            let (bytes, index) = faces[p.font];
            face = ttf_parser::Face::parse(bytes, index)
                .ok()
                .map(|f| (p.font, f));
        }
        let Some((_, face)) = &face else {
            continue;
        };
        let mut outline = Outline(tiny_skia::PathBuilder::new());
        face.outline_glyph(ttf_parser::GlyphId(p.glyph), &mut outline);
        if let Some(path) = outline
            .0
            .finish()
            .and_then(|path| path.transform(p.transform))
        {
            paths.push((path, p.at));
        }
    }
    paths
}

#[derive(Clone)]
struct LightView {
    base: GpuView,
    light: Arc<Light>,
    size: [u32; 2],
}

impl PartialEq for LightView {
    fn eq(&self, other: &Self) -> bool {
        self.base == other.base && self.light.id == other.light.id && self.size == other.size
    }
}

impl ComponentView for LightView {
    fn share_layouts(&mut self, share: &mut dyn FnMut(&mut Arc<nana_ui_core::LayoutStyle>)) {
        self.base.share_layouts(share);
    }
    fn node_kind(&self) -> NodeKind {
        self.base.node_kind()
    }
    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        self.base.project(id, world, mutations);
        self.light.capture(world, id);
        mutations.set_custom_render(
            id,
            Some(CustomRenderNode::new(
                RENDERER,
                format!("{RENDERER}:{}", self.light.id),
                1,
            )),
        );
    }
}

/// Timing and source shared by retained text effects.
pub struct TextSchedule<'a> {
    pub at_ms: &'a [Option<u64>],
    pub value: &'a str,
    pub exit: Option<(u64, u16)>,
    pub preview: bool,
}

pub fn wrap(
    ui: Stage,
    text: AnyView,
    node: NodeRef,
    spec: TextLightSpec,
    schedule: TextSchedule<'_>,
    entrance_ms: u16,
) -> AnyView {
    let TextSchedule {
        at_ms,
        value,
        exit,
        preview,
    } = schedule;
    let id = LIVE.next_id();
    let frame = crate::clock::frame_ms();
    let light = Arc::new(Light {
        id,
        ui,
        text: node,
        spec,
        born: at_ms
            .iter()
            .flatten()
            .copied()
            .filter(|t| *t > 0)
            .min()
            .unwrap_or(frame)
            .min(frame),
        times: super::overflow::glyphs(value, at_ms)
            .into_iter()
            .map(|g| (g.byte, g.at))
            .collect(),
        entrance_ms,
        exit,
        preview,
        geometry: Mutex::new(Geometry::default()),
    });
    LIVE.insert(id, &light);
    let size = signal([0u32; 2]);
    let mut base = GpuView::new(id);
    style_overlay_gpu(&mut base, light.spec.radius * 2.);
    let layer = bind_overlay_layer(
        LightView {
            base,
            light,
            size: [0; 2],
        },
        size,
        move |v: &mut LightView| {
            v.size = size.get();
        },
    );
    wrap_text_overlay(ui, text, None, layer)
}

/// Snapshot the registry first: reading signals (or dropping the last strong
/// reference, whose `Drop` takes the same lock) must not happen under it.
fn track_animating(ui: Stage, now: u64, preview: bool) -> bool {
    LIVE.snapshot().into_iter().any(|l| {
        l.preview == preview
            && if preview {
                l.ui.shader_preview == ui.shader_preview
            } else {
                l.ui.display == ui.display
            }
            && l.active(now)
    })
}

pub fn animating(ui: Stage, now: u64) -> bool {
    track_animating(ui, now, false)
}

pub fn preview_animating(ui: Stage, now: u64) -> bool {
    track_animating(ui, now, true)
}

pub fn register(registry: &mut SceneGpuRendererRegistry) {
    registry.insert(RENDERER, SHARED.clone());
}

pub fn collect_garbage() {
    let mut state = SHARED.state.lock().unwrap_or_else(|e| e.into_inner());
    state.masks.retain(|_, m| m.light.strong_count() > 0);
    state
        .pending_masks
        .retain(|(id, _, _, _), _| LIVE.contains(*id));
    state.placements.retain_live(&LIVE);
}

pub fn cache_counts() -> (usize, usize) {
    let s = SHARED.state.lock().unwrap_or_else(|e| e.into_inner());
    (s.masks.len(), s.masks.values().map(|p| p.bytes).sum())
}

/// Masks belonging to one stage, independent of other stages' renders.
pub fn cache_counts_for(ui: Stage) -> (usize, usize) {
    let s = SHARED.state.lock().unwrap_or_else(|e| e.into_inner());
    s.masks
        .values()
        .filter(|p| {
            p.light
                .upgrade()
                .is_some_and(|l| l.ui.display == ui.display)
        })
        .fold((0, 0), |(count, bytes), p| (count + 1, bytes + p.bytes))
}

type PipelineKey = (DeviceGeneration, GpuTextureFormat, ScenePresentationProfile);
type MaskKey = (PipelineKey, u64, u64, [u32; 2], u32);
type PendingMaskKey = (u64, u64, [u32; 2], u32);

struct RasterResult {
    data: Vec<u8>,
    glow_data: Vec<u8>,
}

#[derive(Debug)]
struct Pipeline {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
}

#[derive(Debug)]
struct Prepared {
    light: Weak<Light>,
    uniforms: Uniforms,
    layout: wgpu::BindGroupLayout,
    mask: wgpu::TextureView,
    glow: wgpu::TextureView,
    sampler: wgpu::Sampler,
    pipeline: wgpu::RenderPipeline,
    bytes: usize,
}

struct State {
    pipelines: LruMap<PipelineKey, Arc<OnceLock<Pipeline>>>,
    masks: LruMap<MaskKey, Prepared>,
    pending_masks: LruMap<PendingMaskKey, Arc<OnceLock<RasterResult>>>,
    placements: PlacementCache,
}

impl Default for State {
    fn default() -> Self {
        Self {
            pipelines: LruMap::new(16),
            masks: LruMap::new(16),
            pending_masks: LruMap::new(16),
            placements: PlacementCache::default(),
        }
    }
}

#[derive(Default)]
struct Renderer {
    state: Mutex<State>,
}

impl std::fmt::Debug for Renderer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TextLightRenderer")
    }
}

static SHARED: LazyLock<Arc<Renderer>> = LazyLock::new(|| Arc::new(Renderer::default()));

fn raster(
    paths: &[(tiny_skia::Path, u32)],
    origin: [f32; 2],
    size: [u32; 2],
    scale: f32,
) -> Vec<u8> {
    let [w, h] = size;
    let mut out = vec![0; w as usize * h as usize * 4];
    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    let transform =
        tiny_skia::Transform::from_row(scale, 0., 0., scale, origin[0] * scale, origin[1] * scale);
    for (path, time) in paths {
        if *time == HIDDEN {
            continue;
        }
        // Scan only the glyph's physical bounding rectangle.
        let Some(bounds) = path.bounds().transform(transform) else {
            continue;
        };
        let x0 = (bounds.left().floor() as i32 - 1).max(0) as u32;
        let y0 = (bounds.top().floor() as i32 - 1).max(0) as u32;
        let x1 = (bounds.right().ceil() as u32 + 1).min(w);
        let y1 = (bounds.bottom().ceil() as u32 + 1).min(h);
        if x1 <= x0 || y1 <= y0 {
            continue;
        }
        let mut mask = tiny_skia::Pixmap::new(x1 - x0, y1 - y0).unwrap();
        mask.fill_path(
            path,
            &paint,
            tiny_skia::FillRule::Winding,
            transform.post_translate(-(x0 as f32), -(y0 as f32)),
            None,
        );
        for y in y0..y1 {
            for x in x0..x1 {
                let i = ((y * w + x) * 4) as usize;
                let a = mask.data()[(((y - y0) * (x1 - x0) + (x - x0)) * 4 + 3) as usize];
                if a > out[i + 3] {
                    out[i..i + 4].copy_from_slice(&[
                        (*time & 255) as u8,
                        ((*time >> 8) & 255) as u8,
                        ((*time >> 16) & 255) as u8,
                        a,
                    ]);
                }
            }
        }
    }
    out
}

/// Blur coverage once; propagate the nearest glyph's exact reveal time into
/// transparent pixels, rather than interpolating time bytes in the shader.
fn glow_mask(data: &[u8], size: [u32; 2], radius: f32) -> Vec<u8> {
    let [w, h] = size;
    let alpha = image::GrayImage::from_fn(w, h, |x, y| {
        image::Luma([data[((y * w + x) * 4 + 3) as usize]])
    });
    let blurred = image::imageops::blur(&alpha, radius.max(0.5));
    let n = (w * h) as usize;
    let mut nearest: Vec<u32> = (0..n)
        .map(|i| {
            if data[i * 4 + 3] > 0 {
                i as u32
            } else {
                u32::MAX
            }
        })
        .collect();
    let mut distance: Vec<u32> = nearest
        .iter()
        .map(|i| if *i == u32::MAX { u32::MAX / 2 } else { 0 })
        .collect();
    for y in 0..h as usize {
        for x in 0..w as usize {
            let i = y * w as usize + x;
            for j in [
                x.checked_sub(1).map(|_| i - 1),
                y.checked_sub(1).map(|_| i - w as usize),
            ]
            .into_iter()
            .flatten()
            {
                if distance[j] + 1 < distance[i] {
                    distance[i] = distance[j] + 1;
                    nearest[i] = nearest[j];
                }
            }
        }
    }
    for y in (0..h as usize).rev() {
        for x in (0..w as usize).rev() {
            let i = y * w as usize + x;
            for j in [
                (x + 1 < w as usize).then_some(i + 1),
                (y + 1 < h as usize).then_some(i + w as usize),
            ]
            .into_iter()
            .flatten()
            {
                if distance[j] + 1 < distance[i] {
                    distance[i] = distance[j] + 1;
                    nearest[i] = nearest[j];
                }
            }
        }
    }
    let mut out = vec![0; data.len()];
    for i in 0..n {
        if nearest[i] != u32::MAX {
            let src = nearest[i] as usize * 4;
            out[i * 4..i * 4 + 3].copy_from_slice(&data[src..src + 3]);
            out[i * 4 + 3] = blurred.as_raw()[i];
        }
    }
    out
}

fn rasterize_pair(
    paths: &[(tiny_skia::Path, u32)],
    origin: [f32; 2],
    size: [u32; 2],
    scale: f32,
    blur_radius: f32,
) -> RasterResult {
    let data = raster(paths, origin, size, scale);
    let glow_data = glow_mask(&data, size, blur_radius);
    RasterResult { data, glow_data }
}

impl State {
    fn upload_mask(
        &mut self,
        light: &Arc<Light>,
        c: &SceneGpuPassContext<'_>,
        scale: f32,
        size: [u32; 2],
        revision: u64,
        rasterized: &RasterResult,
    ) -> Option<&mut Prepared> {
        let pk = (c.gpu.generation(), c.target_format, c.presentation);
        let key = (pk, light.id, revision, size, scale.to_bits());
        if self.masks.contains_key(&key) {
            return self.masks.get_mut(&key);
        }
        self.masks.retain(|_, p| p.light.strong_count() > 0);
        let needed_bytes = rasterized.data.len() + rasterized.glow_data.len();
        while self.masks.len() >= 16
            || self.masks.values().map(|p| p.bytes).sum::<usize>() + needed_bytes > 64 * 1024 * 1024
        {
            if self.masks.evict_oldest().is_none() {
                break;
            }
        }
        let device = c.gpu.wgpu().device();
        let compile = if let Some(slot) = self.pipelines.get(&pk) {
            slot.clone()
        } else {
            let off_thread = OFF_THREAD.load(std::sync::atomic::Ordering::Relaxed);
            let slot = compile(device, c.target_format.wgpu(), off_thread);
            self.pipelines.insert(pk, slot.clone());
            slot
        };
        let pipeline = compile.get()?;
        let texture = c
            .gpu
            .create_texture(&nana_ui::GpuTextureDescriptor {
                label: Some("text light glyph mask"),
                width: size[0],
                height: size[1],
                format: GpuTextureFormat::RGBA8_UNORM,
                usage: nana_ui::GpuTextureUsages::SAMPLED | nana_ui::GpuTextureUsages::COPY_DST,
            })
            .ok()?;
        let glow_texture = c
            .gpu
            .create_texture(&nana_ui::GpuTextureDescriptor {
                label: Some("text light blurred mask"),
                width: size[0],
                height: size[1],
                format: GpuTextureFormat::RGBA8_UNORM,
                usage: nana_ui::GpuTextureUsages::SAMPLED | nana_ui::GpuTextureUsages::COPY_DST,
            })
            .ok()?;
        c.gpu
            .write_texture(
                &glow_texture,
                nana_ui::GpuTextureRegion {
                    x: 0,
                    y: 0,
                    width: size[0],
                    height: size[1],
                },
                &rasterized.glow_data,
                size[0] * 4,
            )
            .ok()?;
        c.gpu
            .write_texture(
                &texture,
                nana_ui::GpuTextureRegion {
                    x: 0,
                    y: 0,
                    width: size[0],
                    height: size[1],
                },
                &rasterized.data,
                size[0] * 4,
            )
            .ok()?;
        if let Some(work) = c.gpu_work {
            work.record_upload(needed_bytes);
            work.record_realloc();
        }
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("text light"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        self.masks.insert(
            key,
            Prepared {
                light: Arc::downgrade(light),
                uniforms: Default::default(),
                layout: pipeline.layout.clone(),
                mask: texture.wgpu_view().clone(),
                glow: glow_texture.wgpu_view().clone(),
                sampler,
                pipeline: pipeline.pipeline.clone(),
                bytes: needed_bytes,
            },
        );
        self.masks.get_mut(&key)
    }
}

/// Compile the light's pipeline and rasterize masks off the UI/render thread
/// from now on: the driver and CPU blur take a noticeable moment the first
/// time a light is drawn, and that frame would stall every window. For the
/// hosted app, whose frames keep coming while a light plays; offscreen renders
/// build synchronously on the frame that draws it.
pub fn compile_off_thread() {
    OFF_THREAD.store(true, std::sync::atomic::Ordering::Relaxed);
}
static OFF_THREAD: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The pipeline for `format`, ready now or, when `off_thread`, compiled on the
/// shared 2-worker pool.
fn compile(
    device: &wgpu::Device,
    format: wgpu::TextureFormat,
    off_thread: bool,
) -> Arc<OnceLock<Pipeline>> {
    let slot = Arc::new(OnceLock::new());
    if off_thread {
        let (done, device) = (Arc::clone(&slot), device.clone());
        compiler_pool().submit(move || {
            let _ = done.set(build_pipeline(&device, format));
        });
        return slot;
    }
    let _ = slot.set(build_pipeline(device, format));
    slot
}

fn build_pipeline(device: &wgpu::Device, format: wgpu::TextureFormat) -> Pipeline {
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("text light"),
        entries: &[
            wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 1,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                count: None,
            },
            wgpu::BindGroupLayoutEntry {
                binding: 3,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: true },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            },
        ],
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("text light"),
        source: wgpu::ShaderSource::Wgsl(
            format!(
                "{}\n{}",
                super::effects::FULLSCREEN_QUAD_WGSL,
                include_str!("text_light.wgsl")
            )
            .into(),
        ),
    });
    let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("text light"),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("text light"),
        layout: Some(&pl),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: Some("vertex_main"),
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: Some("fragment_main"),
            targets: &[Some(wgpu::ColorTargetState {
                format,
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
    Pipeline { pipeline, layout }
}

fn material(node: &SceneGpuNode) -> Option<Arc<Light>> {
    LIVE.get(parse_resource_id(&node.custom.resource, RENDERER)?)
}

impl SceneGpuRenderer for Renderer {
    fn preparation_version(&self, node: &CustomRenderNode) -> Option<u64> {
        Some(node.revision)
    }
    fn content_version(&self, _: &CustomRenderNode) -> Option<u64> {
        None
    }
    fn prepare(&self, node: &SceneGpuNode, c: SceneGpuPrepareContext<'_>) {
        if let Some(light) = material(node) {
            self.state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .placements
                .record(light.id, &c);
        }
    }
    fn render(&self, node: &SceneGpuNode, mut c: SceneGpuRenderContext<'_>) {
        let pc = SceneGpuPassContext {
            gpu: c.gpu,
            target_format: c.target_format,
            presentation: c.presentation,
            bounds: c.bounds,
            clip: c.clip,
            dest_size: c.dest_size,
            gpu_work: c.gpu_work,
        };
        c.with_pass("text light", |pass| {
            self.draw_in_pass(node, pass, pc);
        });
    }
    fn draw_in_pass(
        &self,
        node: &SceneGpuNode,
        pass: &mut ScenePass<'_, '_>,
        c: SceneGpuPassContext<'_>,
    ) -> bool {
        let Some(light) = material(node) else {
            return true;
        };
        let now = crate::clock::frame_ms();
        if !light.active(now) || c.bounds.width == 0 || c.bounds.height == 0 {
            return true;
        }
        let (revision, paths, origin, logical_size) = {
            let geometry = light.geometry.lock().unwrap_or_else(|e| e.into_inner());
            (
                geometry.revision,
                geometry.paths.clone(),
                geometry.origin,
                geometry.size,
            )
        };
        if paths.is_empty() {
            return true;
        }
        let pk = (c.gpu.generation(), c.target_format, c.presentation);
        let (scale, crop, size, cached_or_pending) = {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            let (dpi, position, extent) = state.placements.get(light.id, &c, 1.);
            let Some((scale, crop)) =
                overlay_scale_and_crop(&c, dpi, position, extent, logical_size)
            else {
                return true;
            };
            let size = [
                (extent[0] * dpi).ceil().max(1.) as u32,
                (extent[1] * dpi).ceil().max(1.) as u32,
            ];
            if size[0] > 4096
                || size[1] > 4096
                || u64::from(size[0]) * u64::from(size[1]) > 4_194_304
            {
                return true;
            }
            let mask_key = (pk, light.id, revision, size, scale.to_bits());
            let has_mask = state.masks.contains_key(&mask_key);
            if has_mask {
                (scale, crop, size, Some(None))
            } else {
                let off_thread = OFF_THREAD.load(std::sync::atomic::Ordering::Relaxed);
                // Kick off pipeline compilation eagerly so it overlaps with rasterization.
                if !state.pipelines.contains_key(&pk) {
                    let slot = compile(c.gpu.wgpu().device(), c.target_format.wgpu(), off_thread);
                    state.pipelines.insert(pk, slot);
                }
                if off_thread {
                    let pkey = (light.id, revision, size, scale.to_bits());
                    let slot = if let Some(existing) = state.pending_masks.get(&pkey) {
                        existing.clone()
                    } else {
                        let slot = Arc::new(OnceLock::new());
                        state.pending_masks.insert(pkey, slot.clone());
                        let done = Arc::clone(&slot);
                        let blur = light.spec.radius * scale * 0.35;
                        let paths = paths.clone();
                        compiler_pool().submit(move || {
                            let res = rasterize_pair(&paths, origin, size, scale, blur);
                            let _ = done.set(res);
                        });
                        slot
                    };
                    if slot.get().is_some() {
                        (scale, crop, size, Some(Some(slot)))
                    } else {
                        return true;
                    }
                } else {
                    (scale, crop, size, None)
                }
            }
        };

        // Outside SHARED.state lock: build synchronous raster result if off_thread was disabled.
        let inline_raster = match cached_or_pending {
            Some(Some(_)) | Some(None) => None,
            None => Some(rasterize_pair(
                &paths,
                origin,
                size,
                scale,
                light.spec.radius * scale * 0.35,
            )),
        };

        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let mask_key = (pk, light.id, revision, size, scale.to_bits());
        let p = if state.masks.contains_key(&mask_key) {
            state.masks.get_mut(&mask_key)
        } else if let Some(ref res) = inline_raster {
            state.upload_mask(&light, &c, scale, size, revision, res)
        } else {
            let pkey = (light.id, revision, size, scale.to_bits());
            let Some(slot) = state.pending_masks.get(&pkey).cloned() else {
                return true;
            };
            let Some(res) = slot.get() else {
                return true;
            };
            state.upload_mask(&light, &c, scale, size, revision, res)
        };
        let Some(p) = p else {
            return true;
        };
        let exit = light
            .exit
            .filter(|(at, _)| now >= *at)
            .map_or(-1., |(at, ms)| {
                now.saturating_sub(at) as f32 / f32::from(ms.max(1))
            });
        let color = light.spec.color;
        let values = [
            c.dest_size[0] as f32,
            c.dest_size[1] as f32,
            c.bounds.x as f32,
            c.bounds.y as f32,
            c.bounds.width as f32,
            c.bounds.height as f32,
            scale,
            node.opacity * light.spec.intensity,
            linear(color.r),
            linear(color.g),
            linear(color.b),
            f32::from(color.a) / 255.,
            now.saturating_sub(light.born) as f32,
            f32::from(light.entrance_ms.max(1)),
            exit,
            light.spec.radius,
            origin[0],
            origin[1],
            crop[0],
            crop[1],
        ];
        let upload: [u8; 80] = f32_bytes(&values);
        let interop = c.gpu.wgpu();
        let binding = p.uniforms.binding(
            c.gpu,
            &p.layout,
            &[p.mask.clone(), p.glow.clone()],
            &p.sampler,
            output_key(&c, scale),
            upload.len(),
            None,
        );
        interop.queue().write_buffer(&binding.buffer, 0, &upload);
        if let Some(work) = c.gpu_work {
            work.record_upload(upload.len());
            work.record_draw_call();
        }
        let raw = pass.wgpu();
        raw.set_pipeline(&p.pipeline);
        raw.set_bind_group(0, &binding.group, &[]);
        raw.draw(0..6, 0..1);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn off_thread_the_pipeline_arrives_later_and_in_place_at_once() {
        let painter = nana_ui_devtools::offscreen::OffscreenSnapshots::new().unwrap();
        let device = painter.gpu.wgpu().device();
        let format = wgpu::TextureFormat::Bgra8Unorm;
        assert!(
            compile(device, format, false).get().is_some(),
            "an offscreen render compiles on the frame that draws the light"
        );
        let slot = compile(device, format, true);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while slot.get().is_none() {
            assert!(
                std::time::Instant::now() < deadline,
                "the pipeline never arrived"
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }
}
