//! A cached coverage mask and one in-pass draw, under the dialogue's text.
//! The host owns the device, submission and output targets. Only 48 uniform
//! bytes change on an animated frame; the retained node and mask stay put.
use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex, Weak};

use nana_ui::runtime::view::{AnyView, IntoView, widget};
use nana_ui::runtime::{
    ComponentView, CustomRenderNode, GpuView, MutationQueue, NodeKind, StableNodeId, UiWorld,
};
use nana_ui::{
    DeviceGeneration, GpuTextureFormat, SceneGpuNode, SceneGpuPassContext, SceneGpuPrepareContext,
    SceneGpuRenderContext, SceneGpuRenderer, SceneGpuRendererRegistry, ScenePass,
    ScenePresentationProfile, wgpu,
};
use nana_ui_core::{LengthSpec, PointerEventsSpec, PositionSpec};
use nanatype_theme::{ShapeCommand, SweepSpec};

use super::surface::{Surface, SurfaceRoute};

pub(super) fn effective(spec: &SweepSpec) -> bool {
    spec.enabled && spec.intensity > 0.0 && spec.color.a > 0
}

pub fn animating(ui: super::Stage) -> bool {
    super::shader_effects::motion(ui)
        && !ui.display.with_untracked(|d| d.empty)
        && ui.skin.with_untracked(|s| effective(&s.dialogue.sweep))
}

/// The input window's skin preview sweeps its box.
pub fn input_animating(ui: super::Stage) -> bool {
    super::shader_effects::motion(ui)
        && ui.settings.with_untracked(|s| s.general.input_skin_preview)
        && ui.skin.with_untracked(|s| effective(&s.dialogue.sweep))
}

const RENDERER: &str = "nanatype.dialogue-sweep";
static MATERIALS: LazyLock<super::effects::Registry<Material>> = LazyLock::new(Default::default);

#[derive(Debug, PartialEq)]
struct Material {
    id: u64,
    surface: Surface,
    spec: SweepSpec,
    animated: bool,
}
impl Drop for Material {
    fn drop(&mut self) {
        MATERIALS.remove(self.id);
    }
}

#[derive(Clone, PartialEq)]
struct SweepView {
    base: GpuView,
    material: Arc<Material>,
}
impl ComponentView for SweepView {
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

pub fn view(surface: Surface, spec: SweepSpec, animated: bool) -> AnyView {
    if !effective(&spec) {
        return widget(nana_ui::runtime::Stack::row(0.0)).into_any();
    }
    let id = MATERIALS.next_id();
    let material = Arc::new(Material {
        id,
        surface,
        spec,
        animated,
    });
    MATERIALS.insert(id, &material);
    let mut base = GpuView::new(id);
    let l = Arc::make_mut(&mut base.style.layout);
    l.position = PositionSpec::Absolute;
    l.offset_left = Some(LengthSpec::Px(0.0));
    l.offset_top = Some(LengthSpec::Px(0.0));
    l.width = Some(LengthSpec::Percent(100.0));
    l.height = Some(LengthSpec::Percent(100.0));
    l.pointer_events = Some(PointerEventsSpec::None);
    widget(SweepView { base, material }).into_any()
}

/// The same registry is used by native windows, OBS output and snapshot tools.
pub fn renderers() -> SceneGpuRendererRegistry {
    static REGISTRY: LazyLock<SceneGpuRendererRegistry> = LazyLock::new(|| {
        let mut registry = nana_ui::default_scene_gpu_renderers();
        registry.insert(RENDERER, SHARED.clone());
        super::shader_effects::register(&mut registry);
        super::text_light::register(&mut registry);
        super::text_scramble::register(&mut registry);
        registry
    });
    REGISTRY.clone()
}

type PipelineKey = (DeviceGeneration, GpuTextureFormat, ScenePresentationProfile);
type MaskKey = (PipelineKey, u64, [u32; 2], u32);
#[derive(Debug)]
struct Pipeline {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
}
#[derive(Debug)]
struct Prepared {
    material: Weak<Material>,
    uniforms: super::effect_uniforms::Uniforms,
    layout: wgpu::BindGroupLayout,
    mask: wgpu::TextureView,
    sampler: wgpu::Sampler,
    pipeline: wgpu::RenderPipeline,
    mask_bytes: usize,
}
#[derive(Debug, Default)]
struct State {
    generations: Vec<DeviceGeneration>,
    pipelines: HashMap<PipelineKey, Pipeline>,
    masks: HashMap<MaskKey, Prepared>,
    scales: super::effect_uniforms::Scales,
}
#[derive(Debug, Default)]
struct SweepRenderer {
    state: Mutex<State>,
}
static SHARED: LazyLock<Arc<SweepRenderer>> = LazyLock::new(|| Arc::new(SweepRenderer::default()));

pub fn collect_garbage() {
    let mut state = SHARED.state.lock().unwrap_or_else(|e| e.into_inner());
    state.masks.retain(|_, m| m.material.strong_count() > 0);
    state.scales.retain(|(_, id, _)| MATERIALS.contains(*id));
}

pub fn cache_counts() -> (usize, usize, usize) {
    let state = SHARED.state.lock().unwrap_or_else(|e| e.into_inner());
    (state.pipelines.len(), state.masks.len(), state.scales.len())
}

fn bytes(values: &[f32; 12]) -> [u8; 48] {
    let mut bytes = [0; 48];
    for (dest, value) in bytes.as_chunks_mut::<4>().0.iter_mut().zip(values) {
        dest.copy_from_slice(&value.to_ne_bytes());
    }
    bytes
}
use super::effects::linear;

impl State {
    fn prepare_mask(
        &mut self,
        material: &Arc<Material>,
        context: &SceneGpuPassContext<'_>,
        scale: f32,
    ) -> Option<&mut Prepared> {
        let pipeline_key = (
            context.gpu.generation(),
            context.target_format,
            context.presentation,
        );
        let extent = [context.bounds.width.max(1), context.bounds.height.max(1)];
        let max = context
            .gpu
            .wgpu()
            .device()
            .limits()
            .max_texture_dimension_2d;
        if extent[0] > max || extent[1] > max {
            return None;
        }
        // Coverage depends on size and DPI, not the destination viewport.
        let key = (pipeline_key, material.id, extent, scale.to_bits());
        if self.masks.contains_key(&key) {
            return self.masks.get_mut(&key);
        }
        self.masks
            .retain(|_, value| value.material.strong_count() > 0);
        // Bound stale resize/scale variants without growing during a long session.
        if self.masks.len() >= 32
            || self.masks.values().map(|m| m.mask_bytes).sum::<usize>()
                + extent[0] as usize * extent[1] as usize * 4
                > 64 * 1024 * 1024
        {
            self.masks.clear();
        }
        let generation = context.gpu.generation();
        if !self.generations.contains(&generation) {
            self.generations.push(generation);
            if self.generations.len() > 4 {
                let retired = self.generations.remove(0);
                self.pipelines.retain(|(g, _, _), _| *g != retired);
                self.masks.retain(|((g, _, _), _, _, _), _| *g != retired);
            }
        }
        self.scales
            .retain(|(g, id, _)| self.generations.contains(g) && MATERIALS.contains(*id));
        let device = context.gpu.wgpu().device();
        let pipeline = self.pipelines.entry(pipeline_key).or_insert_with(|| {
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("dialogue sweep"),
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
                ],
            });
            let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("dialogue sweep"),
                source: wgpu::ShaderSource::Wgsl(include_str!("sweep.wgsl").into()),
            });
            let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("dialogue sweep"),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("dialogue sweep"),
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
                        format: context.target_format.wgpu(),
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
        let mask = coverage(&material.surface, extent, scale);
        let mask_bytes = mask.len();
        if let Some(work) = context.gpu_work {
            work.record_upload(mask_bytes);
            work.record_realloc();
        }
        let texture = context
            .gpu
            .create_texture(&nana_ui::GpuTextureDescriptor {
                label: Some("dialogue sweep coverage"),
                width: extent[0],
                height: extent[1],
                format: GpuTextureFormat::RGBA8_UNORM,
                usage: nana_ui::GpuTextureUsages::SAMPLED | nana_ui::GpuTextureUsages::COPY_DST,
            })
            .ok()?;
        context
            .gpu
            .write_texture(
                &texture,
                nana_ui::GpuTextureRegion {
                    x: 0,
                    y: 0,
                    width: extent[0],
                    height: extent[1],
                },
                &mask,
                extent[0] * 4,
            )
            .ok()?;
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("dialogue sweep mask"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let layout = pipeline.layout.clone();
        let pipeline = pipeline.pipeline.clone();
        Some(self.masks.entry(key).or_insert_with(|| Prepared {
            material: Arc::downgrade(material),
            uniforms: Default::default(),
            layout,
            mask: texture.wgpu_view().clone(),
            sampler,
            pipeline,
            mask_bytes,
        }))
    }
}

fn material(node: &SceneGpuNode) -> Option<Arc<Material>> {
    MATERIALS.get(node.custom.resource.parse().ok()?)
}

impl SceneGpuRenderer for SweepRenderer {
    fn preparation_version(&self, node: &CustomRenderNode) -> Option<u64> {
        Some(node.revision)
    }
    fn content_version(&self, _: &CustomRenderNode) -> Option<u64> {
        None // Time and dialogue events change pixels without rebuilding commands.
    }

    fn prepare(&self, node: &SceneGpuNode, context: SceneGpuPrepareContext<'_>) {
        let Some(material) = material(node) else {
            return;
        };
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.scales.record(
            context.gpu.generation(),
            material.id,
            context.dest_size,
            context.scale_factor,
        );
    }
    fn render(&self, node: &SceneGpuNode, mut context: SceneGpuRenderContext<'_>) {
        let pass_context = SceneGpuPassContext {
            gpu: context.gpu,
            target_format: context.target_format,
            presentation: context.presentation,
            bounds: context.bounds,
            clip: context.clip,
            dest_size: context.dest_size,
            gpu_work: context.gpu_work,
        };
        context.with_pass("dialogue sweep", |pass| {
            self.draw_in_pass(node, pass, pass_context);
        });
    }
    fn draw_in_pass(
        &self,
        node: &SceneGpuNode,
        pass: &mut ScenePass<'_, '_>,
        context: SceneGpuPassContext<'_>,
    ) -> bool {
        let Some(material) = material(node) else {
            return true;
        };
        if context.bounds.width == 0 || context.bounds.height == 0 {
            return true;
        }
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let scale = state
            .scales
            .get(context.gpu.generation(), material.id, context.dest_size);
        // Too large for the device or a lost device: draw nothing this frame.
        let Some(prepared) = state.prepare_mask(&material, &context, scale) else {
            return true;
        };
        let c = material.spec.color;
        let phase = if material.animated {
            (crate::clock::now_ms() as f64 / 1000.0 / f64::from(material.spec.period_s)).fract()
                as f32
        } else {
            0.5
        };
        let values = [
            context.dest_size[0] as f32,
            context.dest_size[1] as f32,
            context.bounds.x as f32,
            context.bounds.y as f32,
            context.bounds.width as f32,
            context.bounds.height as f32,
            phase,
            node.opacity * material.spec.intensity,
            linear(c.r),
            linear(c.g),
            linear(c.b),
            f32::from(c.a) / 255.0,
        ];
        let upload = bytes(&values);
        let interop = context.gpu.wgpu();
        let _submission = interop.lock_submission();
        let binding = prepared.uniforms.binding(
            context.gpu,
            &prepared.layout,
            std::slice::from_ref(&prepared.mask),
            &prepared.sampler,
            super::effect_uniforms::output_key(&context, scale),
            upload.len(),
            None,
        );
        interop.queue().write_buffer(&binding.buffer, 0, &upload);
        if let Some(work) = context.gpu_work {
            work.record_upload(upload.len());
            work.record_draw_call();
        }
        let raw = pass.wgpu();
        raw.set_pipeline(&prepared.pipeline);
        raw.set_bind_group(0, &binding.group, &[]);
        raw.draw(0..6, 0..1);
        true
    }
}

/// Static alpha coverage, built once per physical size and surface revision.
pub(super) fn coverage(surface: &Surface, size: [u32; 2], scale: f32) -> Vec<u8> {
    coverage_for(surface, size, scale, false)
}
pub(super) fn coverage_for(
    surface: &Surface,
    size: [u32; 2],
    scale: f32,
    stroke_only: bool,
) -> Vec<u8> {
    let [w, h] = size;
    let logical = [w as f32 / scale, h as f32 / scale];
    if surface.route() == SurfaceRoute::Frame {
        // A failed image decode must not fall through to a rectangular mask.
        let mut mask = frame_coverage(surface, size, scale)
            .unwrap_or_else(|| vec![0; w as usize * h as usize * 4]);
        if stroke_only && let Some((_, widths)) = &surface.frame {
            let [top, right, bottom, left] = widths.map(|v| v * scale);
            for y in 0..h {
                for x in 0..w {
                    if x as f32 >= left
                        && (x as f32) < w as f32 - right
                        && y as f32 >= top
                        && (y as f32) < h as f32 - bottom
                    {
                        mask[((y * w + x) * 4 + 3) as usize] = 0;
                    }
                }
            }
        }
        return mask;
    }
    let polygon = surface
        .polygon
        .as_ref()
        .filter(|_| !surface.is_vector())
        .and_then(|points| {
            let mut builder = tiny_skia::PathBuilder::new();
            for (i, [x, y]) in points.iter().copied().enumerate() {
                let (x, y) = (x * w as f32 / 100.0, y * h as f32 / 100.0);
                if i == 0 {
                    builder.move_to(x, y);
                } else {
                    builder.line_to(x, y);
                }
            }
            builder.close();
            let path = builder.finish()?;
            let mut p = tiny_skia::Pixmap::new(w, h)?;
            let mut paint = tiny_skia::Paint::default();
            paint.set_color_rgba8(255, 255, 255, 255);
            p.fill_path(
                &path,
                &paint,
                tiny_skia::FillRule::Winding,
                tiny_skia::Transform::identity(),
                None,
            );
            Some(p.take())
        });
    let mut out = vec![0; w as usize * h as usize * 4];
    let mut pixmap = tiny_skia::Pixmap::new(w, h).expect("positive sweep size");
    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    let transform = tiny_skia::Transform::from_scale(scale, scale);
    let stroke_alpha = surface
        .border
        .as_ref()
        .map(|b| b.color)
        .or(surface.fill)
        .map_or(0.0, |c| c[3]);
    // Reuse one raster buffer for every part; only ink and opacity differ.
    let mut raster = |path: &tiny_skia::Path, even_odd, stroke: Option<f32>, opacity: f32| {
        pixmap.fill(tiny_skia::Color::TRANSPARENT);
        if let Some(width) = stroke {
            pixmap.stroke_path(
                path,
                &paint,
                &tiny_skia::Stroke {
                    width,
                    line_cap: tiny_skia::LineCap::Round,
                    line_join: tiny_skia::LineJoin::Round,
                    ..Default::default()
                },
                transform,
                None,
            );
        } else {
            pixmap.fill_path(path, &paint, fill_rule(even_odd), transform, None);
        }
        for (i, (target, mask)) in out
            .as_chunks_mut::<4>()
            .0
            .iter_mut()
            .zip(pixmap.data().as_chunks::<4>().0.iter())
            .enumerate()
        {
            let ink = if stroke.is_some() {
                stroke_alpha
            } else {
                fill_alpha(
                    surface,
                    [
                        ((i as u32 % w) as f32 + 0.5) / scale,
                        ((i as u32 / w) as f32 + 0.5) / scale,
                    ],
                    logical,
                )
            };
            let clip = polygon
                .as_ref()
                .map_or(1.0, |p| f32::from(p[i * 4 + 3]) / 255.0);
            let alpha = (f32::from(mask[3]) / 255.0 * opacity * ink * clip).clamp(0.0, 1.0);
            target[3] = (alpha * 255.0 + f32::from(target[3]) * (1.0 - alpha)).round() as u8;
        }
    };
    if let SurfaceRoute::Motif(motif) = surface.route() {
        for part in nanatype_theme::motif_paths(motif, logical) {
            let mut builder = tiny_skia::PathBuilder::new();
            append_path(&mut builder, &part.path);
            if let Some(path) = builder.finish() {
                raster(
                    &path,
                    part.path.even_odd,
                    (!part.fill).then_some(part.stroke_width),
                    part.opacity,
                );
            }
        }
    } else {
        let mut builder = tiny_skia::PathBuilder::new();
        let mut even_odd = false;
        if let Some(shape) = surface.contour_path(logical) {
            even_odd = shape.even_odd;
            append_path(&mut builder, &shape);
        } else {
            let [w, h] = logical;
            let [tl, tr, br, bl] = surface.radii.map(|v| v.max(0.0).min(w.min(h) * 0.5));
            let k = 0.5522848;
            builder.move_to(tl, 0.0);
            builder.line_to(w - tr, 0.0);
            builder.cubic_to(w - tr + k * tr, 0.0, w, tr - k * tr, w, tr);
            builder.line_to(w, h - br);
            builder.cubic_to(w, h - br + k * br, w - br + k * br, h, w - br, h);
            builder.line_to(bl, h);
            builder.cubic_to(bl - k * bl, h, 0.0, h - bl + k * bl, 0.0, h - bl);
            builder.line_to(0.0, tl);
            builder.cubic_to(0.0, tl - k * tl, tl - k * tl, 0.0, tl, 0.0);
            builder.close();
        }
        if let Some(path) = builder.finish() {
            let stroke = stroke_only.then(|| {
                surface
                    .border
                    .as_ref()
                    .map_or(1., |b| b.widths.into_iter().fold(0., f32::max))
            });
            raster(&path, even_odd, stroke, 1.0);
        }
    }
    out
}

fn fill_alpha(surface: &Surface, point: [f32; 2], logical: [f32; 2]) -> f32 {
    let mut alpha = surface.fill.map_or(0.0, |c| c[3]);
    if let Some(nana_ui_core::BackgroundImage::Gradient(gradient)) = &surface.gradient {
        let (at, stops) = match gradient {
            nana_ui_core::CssGradient::Linear(g) => {
                let angle = g.angle_deg.to_radians();
                let dir = [angle.sin(), -angle.cos()];
                let span = dir[0].abs() * logical[0] + dir[1].abs() * logical[1];
                (
                    0.5 + ((point[0] - logical[0] * 0.5) * dir[0]
                        + (point[1] - logical[1] * 0.5) * dir[1])
                        / span.max(0.001),
                    &g.stops,
                )
            }
            nana_ui_core::CssGradient::Radial(g) => {
                let center = g
                    .resolved_center(logical[0], logical[1])
                    .unwrap_or([0.5, 0.5]);
                let center = [center[0] * logical[0], center[1] * logical[1]];
                let (dx, dy) = (
                    center[0].max(logical[0] - center[0]).max(0.1),
                    center[1].max(logical[1] - center[1]).max(0.1),
                );
                let (rx, ry) = if g.circle {
                    let r = dx.hypot(dy);
                    (r, r)
                } else {
                    (dx * 2.0_f32.sqrt(), dy * 2.0_f32.sqrt())
                };
                (
                    ((point[0] - center[0]) / rx).hypot((point[1] - center[1]) / ry),
                    &g.stops,
                )
            }
        };
        let g = gradient_alpha(stops, at);
        alpha = g + alpha * (1.0 - g);
    }
    alpha.clamp(0.0, 1.0)
}

fn fill_rule(even_odd: bool) -> tiny_skia::FillRule {
    if even_odd {
        tiny_skia::FillRule::EvenOdd
    } else {
        tiny_skia::FillRule::Winding
    }
}

fn append_path(builder: &mut tiny_skia::PathBuilder, shape: &nanatype_theme::ShapePath) {
    for command in &shape.commands {
        match *command {
            ShapeCommand::Move([x, y]) => builder.move_to(x, y),
            ShapeCommand::Line([x, y]) => builder.line_to(x, y),
            ShapeCommand::Quad([a, b], [x, y]) => builder.quad_to(a, b, x, y),
            ShapeCommand::Cubic([a, b], [c, d], [x, y]) => builder.cubic_to(a, b, c, d, x, y),
            ShapeCommand::Close => builder.close(),
        }
    }
}

fn gradient_alpha(stops: &[nana_ui_core::GradientStop], t: f32) -> f32 {
    let Some(first) = stops.first() else {
        return 0.0;
    };
    if t <= first.position {
        return first.color[3];
    }
    for pair in stops.windows(2) {
        if t <= pair[1].position {
            let f = ((t - pair[0].position) / (pair[1].position - pair[0].position).max(0.0001))
                .clamp(0.0, 1.0);
            return pair[0].color[3] * (1.0 - f) + pair[1].color[3] * f;
        }
    }
    stops.last().unwrap().color[3]
}

fn frame_coverage(surface: &Surface, size: [u32; 2], scale: f32) -> Option<Vec<u8>> {
    let (frame, widths) = surface.frame.as_ref()?;
    let nana_ui_core::BackgroundImage::Url { url, .. } = &frame.source else {
        return None;
    };
    let path = crate::assets::file_path(url)?;
    let path = path.as_path();
    // These are already resolved media-library URLs. Keep the decoder limits
    // identical to the product image boundary; a missing frame emits no light.
    if std::fs::metadata(path).ok()?.len() > crate::media::MAX_FILE_BYTES {
        return Some(vec![0; (size[0] * size[1] * 4) as usize]);
    }
    let dimensions = imagesize::size(path).ok()?;
    if dimensions.width > nanatype_theme::MAX_IMAGE_SIDE
        || dimensions.height > nanatype_theme::MAX_IMAGE_SIDE
    {
        return Some(vec![0; (size[0] * size[1] * 4) as usize]);
    }
    let image = image::open(path).ok()?.to_rgba8();
    let (iw, ih) = (image.width() as f32, image.height() as f32);
    let slices = [
        frame.slice[0].to_px(ih),
        frame.slice[1].to_px(iw),
        frame.slice[2].to_px(ih),
        frame.slice[3].to_px(iw),
    ];
    let axis = |value: f32, extent: f32, near: f32, far: f32, src: f32, snear: f32, sfar: f32| {
        let factor = (extent / (near + far).max(0.001)).min(1.0);
        let (near, far) = (near * factor, far * factor);
        if value < near {
            value / near.max(0.001) * snear
        } else if value >= extent - far {
            src - sfar + (value - (extent - far)) / far.max(0.001) * sfar
        } else {
            snear
                + (value - near) / (extent - near - far).max(0.001) * (src - snear - sfar).max(0.0)
        }
    };
    let mut out = vec![0; (size[0] * size[1] * 4) as usize];
    for y in 0..size[1] {
        for x in 0..size[0] {
            let sx = axis(
                (x as f32 + 0.5) / scale,
                size[0] as f32 / scale,
                widths[3],
                widths[1],
                iw,
                slices[3],
                slices[1],
            );
            let sy = axis(
                (y as f32 + 0.5) / scale,
                size[1] as f32 / scale,
                widths[0],
                widths[2],
                ih,
                slices[0],
                slices[2],
            );
            let center =
                sx >= slices[3] && sx < iw - slices[1] && sy >= slices[0] && sy < ih - slices[2];
            if frame.fill || !center {
                out[((y * size[0] + x) * 4 + 3) as usize] = image.get_pixel(
                    (sx.floor() as u32).min(image.width() - 1),
                    (sy.floor() as u32).min(image.height() - 1),
                )[3];
            }
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn motif_coverage_uses_stroke_ink_and_wins_over_contour_and_polygon() {
        let mut surface = Surface {
            motif: Some(nanatype_theme::MotifPreset::Orbit),
            contour: Some(nanatype_theme::ContourPreset::Cloud),
            polygon: Some(vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]),
            border: Some(super::super::surface::Border {
                widths: [1.0; 4],
                color: [1.0, 1.0, 1.0, 0.5],
                ..Default::default()
            }),
            ..Default::default()
        };
        let masked = coverage(&surface, [100, 60], 1.0);
        assert!(masked.as_chunks::<4>().0.iter().any(|p| p[3] > 0));
        assert_eq!(masked[3], 0);
        surface.contour = None;
        surface.polygon = None;
        assert_eq!(masked, coverage(&surface, [100, 60], 1.0));
    }

    #[test]
    fn invisible_sweeps_do_not_animate() {
        let ui = super::super::Stage::new(
            Arc::new(nanatype_settings::Settings::default()),
            Arc::new(nanatype_theme::Skin::default()),
            nanatype_i18n::Locale::ZhCn,
        );
        ui.display.update(|d| Arc::make_mut(d).empty = false);
        let spec = SweepSpec {
            enabled: true,
            ..Default::default()
        };
        for spec in [
            SweepSpec {
                enabled: false,
                ..spec.clone()
            },
            SweepSpec {
                intensity: 0.0,
                ..spec.clone()
            },
            SweepSpec {
                color: nanatype_model::Rgba8::TRANSPARENT,
                ..spec.clone()
            },
        ] {
            ui.skin
                .update(|s| Arc::make_mut(s).dialogue.sweep = spec.clone());
            assert!(!animating(ui));
        }
        ui.skin.update(|s| Arc::make_mut(s).dialogue.sweep = spec);
        assert!(animating(ui));
        ui.reduced_motion.set(true);
        assert!(!animating(ui));
    }

    #[test]
    fn failed_nine_slice_masks_never_fall_back_to_contours_or_boxes() {
        use super::super::surface::nine_slice;
        let mut surface = Surface {
            fill: Some([1.0; 4]),
            contour: Some(nanatype_theme::ContourPreset::Cloud),
            frame: Some(nine_slice(
                &nanatype_theme::NineSlice {
                    image: "missing.png".into(),
                    slice: [4; 4],
                    scale: 1.0,
                    repeat: false,
                    layout_inset: true,
                    fill: true,
                },
                "file:///nonexistent-nanatype-render-test/frame.png".into(),
            )),
            ..Default::default()
        };
        for scale in [1.0, 1.5, 2.0] {
            let size = [(80.0 * scale) as u32, (40.0 * scale) as u32];
            assert!(coverage(&surface, size, scale).iter().all(|b| *b == 0));
        }
        surface.frame = None;
        assert!(
            coverage(&surface, [80, 40], 1.0)
                .as_chunks::<4>()
                .0
                .iter()
                .any(|p| p[3] > 0)
        );
    }
}
