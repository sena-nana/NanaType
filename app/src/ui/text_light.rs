//! Finite light attached to retained text geometry. Raster data is immutable
//! between layout changes; animated frames upload only the clock uniforms.
use super::Ui;
use nana_ui::runtime::view::{AnyView, IntoView, NodeRef, signal, widget};
use nana_ui::runtime::{
    ComponentView, CustomRenderNode, GpuView, MutationQueue, NodeKind, SizeChanged, StableNodeId,
    Stack, TextShaper, UiWorld,
};
use nana_ui::{
    DeviceGeneration, GpuTextureFormat, SceneGpuNode, SceneGpuPassContext, SceneGpuPrepareContext,
    SceneGpuRenderContext, SceneGpuRenderer, SceneGpuRendererRegistry, ScenePass,
    ScenePresentationProfile, wgpu,
};
use nana_ui_core::{LengthSpec, PointerEventsSpec, PositionSpec};
use nanatype_theme::TextLightSpec;
use std::collections::HashMap;
use std::sync::{
    Arc, LazyLock, Mutex, Weak,
    atomic::{AtomicU64, Ordering},
};

const RENDERER: &str = "nanatype.text-light";
const HIDDEN: u32 = 0xff_ffff;
static NEXT: AtomicU64 = AtomicU64::new(1);
static LIVE: LazyLock<Mutex<HashMap<u64, Weak<Light>>>> = LazyLock::new(Default::default);

#[derive(Debug, Default)]
struct Geometry {
    revision: u64,
    paths: Vec<(tiny_skia::Path, u32)>,
    origin: [f32; 2],
    size: [f32; 2],
}
struct Light {
    id: u64,
    ui: Ui,
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
        LIVE.lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.id);
    }
}
impl Light {
    fn active(&self, now: u64) -> bool {
        if self.preview
            && (!self.ui.shader_preview.get_untracked()
                || self.ui.page.get_untracked() != super::settings::Page::Skins)
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
        let Some(text) = self.text.get_untracked() else {
            return;
        };
        let Some((_, layout)) = world.text_layout(text) else {
            return;
        };
        let (Some(tb), Some(lb)) = (world.layout_box(text), world.layout_box(layer)) else {
            return;
        };
        let Some(engine) = nana_ui::NanaTextShaper::default().text_engine() else {
            return;
        };
        let engine = engine.lock().unwrap_or_else(|e| e.into_inner());
        let mut geometry = self.geometry.lock().unwrap_or_else(|e| e.into_inner());
        geometry.origin = [tb.x - lb.x, tb.y - lb.y];
        geometry.size = [lb.width, lb.height];
        geometry.paths.clear();
        let vertical = layout.is_vertical();
        for line in &layout.lines {
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
                let centre = layout
                    .physical_x_of_block(line.metrics.baseline_y_px, layout.physical_size().0);
                let sideways = vertical && run.orientation == nana_text::RunOrientation::Sideways;
                let mut pen = run.origin_x_px;
                for glyph in &run.glyphs {
                    let mut outline = Outline(tiny_skia::PathBuilder::new());
                    face.outline_glyph(ttf_parser::GlyphId(glyph.glyph_id as u16), &mut outline);
                    let at = self.time(glyph.cluster as usize);
                    if let Some(path) = outline.0.finish() {
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
                        if let Some(path) = path.transform(transform) {
                            geometry.paths.push((path, at));
                        }
                    }
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
                for (pen, glyph) in run.glyph_cells() {
                    let mut outline = Outline(tiny_skia::PathBuilder::new());
                    face.outline_glyph(ttf_parser::GlyphId(glyph.glyph_id as u16), &mut outline);
                    if let Some(path) = outline.0.finish().and_then(|p| {
                        p.transform(tiny_skia::Transform::from_row(
                            scale,
                            0.,
                            0.,
                            -scale,
                            pen + glyph.offset_x_px,
                            ruby.baseline_y_px - glyph.offset_y_px,
                        ))
                    }) {
                        geometry.paths.push((path, self.time(ruby.range.start)));
                    }
                }
            }
        }
        geometry.revision += 1;
    }
}
struct Outline(tiny_skia::PathBuilder);
impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to(x, y);
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.0.quad_to(x1, y1, x, y);
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.0.cubic_to(x1, y1, x2, y2, x, y);
    }
    fn close(&mut self) {
        self.0.close();
    }
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
                self.light.id.to_string(),
                1,
            )),
        );
    }
}

pub fn wrap(
    ui: Ui,
    text: AnyView,
    node: NodeRef,
    spec: TextLightSpec,
    at_ms: &[Option<u64>],
    value: &str,
    entrance_ms: u16,
    exit: Option<(u64, u16)>,
    preview: bool,
) -> AnyView {
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
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
            .unwrap_or_else(crate::clock::now_ms)
            .min(crate::clock::now_ms()),
        times: super::overflow::glyphs(value, at_ms)
            .into_iter()
            .map(|g| (g.byte, g.at))
            .collect(),
        entrance_ms,
        exit,
        preview,
        geometry: Mutex::new(Geometry::default()),
    });
    LIVE.lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id, Arc::downgrade(&light));
    let size = signal([0u32; 2]);
    let mut base = GpuView::new(id);
    let l = Arc::make_mut(&mut base.style.layout);
    let pad = light.spec.radius * 2.;
    l.position = PositionSpec::Absolute;
    l.offset_left = Some(LengthSpec::Px(-pad));
    l.offset_top = Some(LengthSpec::Px(-pad));
    l.width = Some(super::surface::calc(100., pad * 2.));
    l.height = Some(super::surface::calc(100., pad * 2.));
    l.pointer_events = Some(PointerEventsSpec::None);
    let layer = widget(LightView {
        base,
        light,
        size: [0; 2],
    })
    .on(move |e: &SizeChanged| {
        size.set_if_changed([e.width.to_bits(), e.height.to_bits()]);
    })
    .bind(move |v: &mut LightView| {
        v.size = size.get();
    });
    widget(Stack::column(0.).width(LengthSpec::Fill).with_layout(|l| {
        l.position = PositionSpec::Relative;
        l.min_width = Some(LengthSpec::Px(0.));
    }))
    .bind(move |v: &mut Stack| {
        let [w, h] = ui.display_size.get();
        let height = (h > w && w > 0.).then(|| LengthSpec::Px(ui.text_area_height.get().max(1.)));
        super::restyle(v, |l| l.height = height);
    })
    .children((text, layer))
    .into_any()
}

pub fn animating(ui: Ui, now: u64) -> bool {
    LIVE.lock()
        .unwrap_or_else(|e| e.into_inner())
        .values()
        .filter_map(Weak::upgrade)
        .any(|l| !l.preview && l.ui.display == ui.display && l.active(now))
}
pub fn preview_animating(ui: Ui, now: u64) -> bool {
    LIVE.lock()
        .unwrap_or_else(|e| e.into_inner())
        .values()
        .filter_map(Weak::upgrade)
        .any(|l| l.preview && l.ui.shader_preview == ui.shader_preview && l.active(now))
}
pub fn register(registry: &mut SceneGpuRendererRegistry) {
    registry.insert(RENDERER, SHARED.clone());
}
pub fn collect_garbage() {
    let mut state = SHARED.state.lock().unwrap_or_else(|e| e.into_inner());
    state.masks.retain(|_, m| m.light.strong_count() > 0);
    let live = LIVE.lock().unwrap_or_else(|e| e.into_inner());
    state
        .placements
        .retain(|(_, id, _), _| live.contains_key(id));
}
pub fn cache_counts() -> (usize, usize) {
    let s = SHARED.state.lock().unwrap_or_else(|e| e.into_inner());
    (s.masks.len(), s.masks.values().map(|p| p.bytes).sum())
}

type PipelineKey = (DeviceGeneration, GpuTextureFormat, ScenePresentationProfile);
type MaskKey = (PipelineKey, u64, u64, [u32; 2], u32);
#[derive(Debug)]
struct Pipeline {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
}
#[derive(Debug)]
struct Prepared {
    light: Weak<Light>,
    uniforms: super::effect_uniforms::Uniforms,
    layout: wgpu::BindGroupLayout,
    mask: wgpu::TextureView,
    glow: wgpu::TextureView,
    sampler: wgpu::Sampler,
    pipeline: wgpu::RenderPipeline,
    bytes: usize,
}
#[derive(Debug, Default)]
struct State {
    pipelines: HashMap<PipelineKey, Pipeline>,
    masks: HashMap<MaskKey, Prepared>,
    placements: HashMap<(DeviceGeneration, u64, [u32; 2]), (f32, [f32; 2], [f32; 2])>,
}
#[derive(Debug, Default)]
struct Renderer {
    state: Mutex<State>,
}
static SHARED: LazyLock<Arc<Renderer>> = LazyLock::new(|| Arc::new(Renderer::default()));

fn raster(geometry: &Geometry, size: [u32; 2], scale: f32) -> Vec<u8> {
    let [w, h] = size;
    let mut out = vec![0; w as usize * h as usize * 4];
    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    let transform = tiny_skia::Transform::from_row(
        scale,
        0.,
        0.,
        scale,
        geometry.origin[0] * scale,
        geometry.origin[1] * scale,
    );
    for (path, time) in &geometry.paths {
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

impl State {
    fn prepare_mask(
        &mut self,
        light: &Arc<Light>,
        c: &SceneGpuPassContext<'_>,
        scale: f32,
        size: [u32; 2],
    ) -> Option<&mut Prepared> {
        let geometry = light.geometry.lock().unwrap_or_else(|e| e.into_inner());
        if geometry.paths.is_empty() {
            return None;
        }
        let pk = (c.gpu.generation(), c.target_format, c.presentation);
        if size[0] > 4096 || size[1] > 4096 || u64::from(size[0]) * u64::from(size[1]) > 4_194_304 {
            return None;
        }
        let key = (pk, light.id, geometry.revision, size, scale.to_bits());
        if self.masks.contains_key(&key) {
            return self.masks.get_mut(&key);
        }
        self.masks.retain(|_, p| p.light.strong_count() > 0);
        if self.masks.len() >= 16
            || self.masks.values().map(|p| p.bytes).sum::<usize>()
                + size[0] as usize * size[1] as usize * 8
                > 64 * 1024 * 1024
        {
            self.masks.clear();
        }
        if self.pipelines.len() >= 16 && !self.pipelines.contains_key(&pk) {
            self.pipelines.clear();
            self.masks.clear();
        }
        let device = c.gpu.wgpu().device();
        let pipeline = self.pipelines.entry(pk).or_insert_with(|| {
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
                source: wgpu::ShaderSource::Wgsl(include_str!("text_light.wgsl").into()),
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
                        format: c.target_format.wgpu(),
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
        });
        let data = raster(&geometry, size, scale);
        let glow_data = glow_mask(&data, size, light.spec.radius * scale * 0.35);
        let bytes = data.len() * 2;
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
                &glow_data,
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
                &data,
                size[0] * 4,
            )
            .ok()?;
        if let Some(work) = c.gpu_work {
            work.record_upload(bytes);
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
                bytes,
            },
        );
        self.masks.get_mut(&key)
    }
}
fn material(node: &SceneGpuNode) -> Option<Arc<Light>> {
    LIVE.lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&node.custom.resource.parse().ok()?)?
        .upgrade()
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
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.placements.len() >= 128 {
                state.placements.clear();
            }
            state.placements.insert(
                (c.gpu.generation(), light.id, c.dest_size),
                (
                    c.scale_factor,
                    [c.bounds.x, c.bounds.y],
                    [c.bounds.width, c.bounds.height],
                ),
            );
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
        let now = crate::clock::now_ms();
        if !light.active(now) || c.bounds.width == 0 || c.bounds.height == 0 {
            return true;
        }
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let (dpi, position, extent) = state
            .placements
            .get(&(c.gpu.generation(), light.id, c.dest_size))
            .copied()
            .unwrap_or((
                1.,
                [c.bounds.x as f32, c.bounds.y as f32],
                [c.bounds.width as f32, c.bounds.height as f32],
            ));
        let (origin, logical_size) = {
            let geometry = light.geometry.lock().unwrap_or_else(|e| e.into_inner());
            (geometry.origin, geometry.size)
        };
        let scale = extent[0] * dpi / logical_size[0].max(1.);
        if !scale.is_finite() || scale <= 0. {
            return true;
        }
        let crop = [
            (c.bounds.x as f32 - position[0] * dpi) / scale,
            (c.bounds.y as f32 - position[1] * dpi) / scale,
        ];
        let size = [
            (extent[0] * dpi).ceil().max(1.) as u32,
            (extent[1] * dpi).ceil().max(1.) as u32,
        ];
        let Some(p) = state.prepare_mask(&light, &c, scale, size) else {
            return true;
        };
        let exit = light
            .exit
            .filter(|(at, _)| now >= *at)
            .map_or(-1., |(at, ms)| {
                now.saturating_sub(at) as f32 / f32::from(ms.max(1))
            });
        let color = light.spec.color;
        let linear = |v: u8| {
            let v = f32::from(v) / 255.;
            if v <= 0.04045 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
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
        let upload = values.map(f32::to_ne_bytes);
        let upload = upload.as_flattened();
        let interop = c.gpu.wgpu();
        let _submission = interop.lock_submission();
        let binding = p.uniforms.binding(
            c.gpu,
            &p.layout,
            &[p.mask.clone(), p.glow.clone()],
            &p.sampler,
            super::effect_uniforms::output_key(&c, scale, node.opacity),
            upload.len(),
        );
        interop.queue().write_buffer(&binding.buffer, 0, upload);
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
