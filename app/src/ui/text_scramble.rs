//! Character substitution over retained native text geometry. Only draw uniforms
//! change during an animation; shaping, wrapping and the document remain intact.
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
use nana_ui_core::{
    LengthSpec, MaterialProgram, MaterialRef, PointerEventsSpec, PositionSpec, PresentationMask,
    PresentationMaskSpace, TextTransitions,
};
use nanatype_theme::ScrambleSpec;
use std::{
    collections::HashMap,
    sync::{
        Arc, LazyLock, Mutex, Weak,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};
use unicode_segmentation::UnicodeSegmentation;

const RENDERER: &str = "nanatype.text-scramble";
const ATLAS_CELL: [u32; 2] = [40, 64];
const ATLAS_COLUMNS: u32 = 16;
static NEXT: AtomicU64 = AtomicU64::new(1);
static LIVE: LazyLock<Mutex<HashMap<u64, Weak<Scramble>>>> = LazyLock::new(Default::default);

pub fn transitions(
    spec: &ScrambleSpec,
    exit: Option<(u64, u16)>,
    preview: bool,
) -> TextTransitions {
    // The native glyph appears only once substitution finishes, then briefly
    // brightens. Objects use the framework's regular reveal/retirement path.
    let source = format!(
        "fn material(i: MaterialInput) -> vec4<f32> {{ let lock = {}.; let age = i.progress * {}.; let flash = 1. + 0.22 * (1. - clamp((age-lock)/45.,0.,1.)); return vec4<f32>(i.color.rgb * flash, select(0.,1.,age >= lock)); }}",
        spec.entrance_ms,
        u32::from(spec.entrance_ms) + 45
    );
    let entrance = PresentationMask {
        material: MaterialRef::new(MaterialProgram::new(source)),
        space: PresentationMaskSpace::Local,
        start: Duration::ZERO,
        duration: Duration::from_millis(u64::from(spec.entrance_ms) + 45),
    };
    TextTransitions {
        entrance: Some(entrance),
        exit: exit.map(|exit| exit_mask(exit, preview, false)),
        freeze_reveal: exit.filter(|_| !preview).map(|_| Duration::ZERO),
        ..Default::default()
    }
}

/// A second retained text node paints only inline objects during retirement.
/// Its text has transparent paint but exactly the same shaping attributes.
pub fn objects_only(rich: &mut nana_ui_core::RichText) {
    let spans: Vec<_> = rich
        .spans()
        .iter()
        .map(|(range, style)| {
            let mut style = style.clone();
            style.paint = nana_ui_core::RichPaintStyle {
                color: Some(nana_ui_core::PaintColor::srgb([0.; 4])),
                shadows: Some(Arc::from([])),
                ..Default::default()
            };
            (range.clone(), style)
        })
        .collect();
    rich.set_span(
        0..rich.text().len(),
        nana_ui_core::RichSpanStyle::new().color(nana_ui_core::PaintColor::srgb([0.; 4])),
    );
    for (range, style) in spans {
        rich.set_span(range, style);
    }
}

pub fn object_exit(exit: (u64, u16), preview: bool) -> PresentationMask {
    exit_mask(exit, preview, true)
}

fn exit_mask((at, ms): (u64, u16), preview: bool, objects: bool) -> PresentationMask {
    let now = crate::clock::now_ms();
    let (delay, elapsed) = if preview {
        (at.saturating_sub(now), 0)
    } else {
        (0, now.saturating_sub(at))
    };
    let alpha = match (objects, delay > 0) {
        (true, true) => "select(1.-i.progress,0.,i.progress<=0.)",
        (true, false) => "1.-i.progress",
        (false, true) => "select(0.,1.,i.progress<=0.)",
        (false, false) => "0.",
    };
    let source = format!(
        "fn material(i: MaterialInput) -> vec4<f32> {{ return vec4<f32>(i.color.rgb, {alpha}); }}"
    );
    PresentationMask {
        material: MaterialRef::new(MaterialProgram::new(source)),
        space: PresentationMaskSpace::Target,
        start: Duration::from_millis(delay.saturating_sub(1)),
        duration: Duration::from_millis(u64::from(ms).saturating_sub(elapsed)),
    }
}

#[derive(Debug, Default)]
struct Geometry {
    revision: u64,
    cells: Vec<[f32; 8]>,
    atlas: Vec<u8>,
    atlas_size: [u32; 2],
    origin: [f32; 2],
    size: [f32; 2],
}
struct Scramble {
    id: u64,
    ui: Ui,
    text: NodeRef,
    spec: ScrambleSpec,
    color: nanatype_model::Rgba8,
    born: u64,
    times: Vec<(usize, usize, Option<u64>, bool)>,
    seed: u32,
    exit: Option<(u64, u16)>,
    preview: bool,
    geometry: Mutex<Geometry>,
}
impl std::fmt::Debug for Scramble {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Scramble").field("id", &self.id).finish()
    }
}
impl Drop for Scramble {
    fn drop(&mut self) {
        LIVE.lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(&self.id);
    }
}
impl Scramble {
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
        self.times.iter().any(|(_, _, at, blank)| {
            !blank
                && at.is_some_and(|t| {
                    t > 0 && now >= t && now < t + u64::from(self.spec.entrance_ms)
                })
        })
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
        let mut engine = engine.lock().unwrap_or_else(|e| e.into_inner());
        let mut geometry = self.geometry.lock().unwrap_or_else(|e| e.into_inner());
        geometry.origin = [tb.x - lb.x, tb.y - lb.y];
        geometry.size = [lb.width, lb.height];
        geometry.cells.clear();

        // Resolve the platform's real monospace face through NanaText, including
        // its bundled fallback, rather than depending on a Windows font path.
        let query = nana_text::font::FontQuery {
            families: nana_text::font::FamilyList::parse_css("Consolas, Menlo, monospace"),
            ..Default::default()
        };
        let selected = engine.fonts_mut().select(&query);
        let data = selected.chain().find_map(|f| engine.fonts().face_data(f));
        let Some(data) = data else {
            return;
        };
        let Ok(face) = ttf_parser::Face::parse(data.bytes(), data.index()) else {
            return;
        };
        let (atlas, atlas_size) = atlas(&face, &self.spec.alphabet);
        geometry.atlas = atlas;
        geometry.atlas_size = atlas_size;

        // Each grapheme owns one slot, even if shaping produces multiple glyphs
        // (combining marks, variation selectors, ZWJ emoji).
        let mut slots: HashMap<usize, [f32; 4]> = HashMap::new();
        let vertical = layout.is_vertical();
        for line in &layout.lines {
            for run in layout.line_runs(line) {
                if run.is_object() {
                    continue;
                }
                let mut pen = run.origin_x_px;
                let centre = layout
                    .physical_x_of_block(line.metrics.baseline_y_px, layout.physical_size().0);
                for glyph in &run.glyphs {
                    let byte = glyph.cluster as usize;
                    if let Some(index) = self
                        .times
                        .partition_point(|(start, _, _, _)| *start <= byte)
                        .checked_sub(1)
                        && let (_, end, at, blank) = &self.times[index]
                        && byte < *end
                        && !blank
                        && at.is_some_and(|t| self.exit.is_none_or(|(end, _)| t <= end))
                    {
                        let rect = if vertical {
                            [
                                centre - run.font_size_px * 0.5,
                                pen,
                                run.font_size_px,
                                glyph.advance_px.max(1.),
                            ]
                        } else {
                            [
                                pen,
                                line.metrics.baseline_y_px - run.font_size_px * 0.82,
                                glyph.advance_px.max(1.),
                                run.font_size_px,
                            ]
                        };
                        slots
                            .entry(index)
                            .and_modify(|r| {
                                let right = (r[0] + r[2]).max(rect[0] + rect[2]);
                                let bottom = (r[1] + r[3]).max(rect[1] + rect[3]);
                                r[0] = r[0].min(rect[0]);
                                r[1] = r[1].min(rect[1]);
                                r[2] = right - r[0];
                                r[3] = bottom - r[1];
                            })
                            .or_insert(rect);
                    }
                    pen += glyph.advance_px;
                }
            }
        }
        let mut slots: Vec<_> = slots.into_iter().collect();
        slots.sort_unstable_by_key(|(i, _)| *i);
        let ranks = permutation(slots.len(), self.seed);
        let total = slots.len().max(1) as f32;
        for ((index, rect), rank) in slots.into_iter().zip(ranks) {
            let at = self.times[index].2.unwrap_or(0);
            let at = if at == 0 {
                -1.
            } else {
                at.saturating_sub(self.born) as f32
            };
            let fitted = fit_cell(rect);
            geometry.cells.push([
                fitted[0],
                fitted[1],
                fitted[2],
                fitted[3],
                at,
                rank as f32,
                total,
                index as f32,
            ]);
        }
        geometry.revision += 1;
    }
}

// Fit the complete atlas cell uniformly into the retained grapheme advance.
// Narrow letters and punctuation reduce both dimensions instead of squeezing
// a full-height glyph into a sliver. No text layout metrics are modified.
fn fit_cell(slot: [f32; 4]) -> [f32; 4] {
    let scale = (slot[2] / ATLAS_CELL[0] as f32).min(slot[3] / ATLAS_CELL[1] as f32);
    let width = ATLAS_CELL[0] as f32 * scale;
    let height = ATLAS_CELL[1] as f32 * scale;
    [
        slot[0] + (slot[2] - width) * 0.5,
        slot[1] + (slot[3] - height) * 0.5,
        width,
        height,
    ]
}

fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846ca68b);
    x ^ (x >> 16)
}
fn permutation(n: usize, mut seed: u32) -> Vec<usize> {
    let mut order: Vec<_> = (0..n).collect();
    for i in (1..n).rev() {
        seed = hash(seed.wrapping_add(i as u32));
        order.swap(i, seed as usize % (i + 1));
    }
    let mut ranks = vec![0; n];
    for (rank, index) in order.into_iter().enumerate() {
        ranks[index] = rank;
    }
    ranks
}

struct Outline(tiny_skia::PathBuilder);
impl ttf_parser::OutlineBuilder for Outline {
    fn move_to(&mut self, x: f32, y: f32) {
        self.0.move_to(x, y);
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.0.line_to(x, y);
    }
    fn quad_to(&mut self, x: f32, y: f32, u: f32, v: f32) {
        self.0.quad_to(x, y, u, v);
    }
    fn curve_to(&mut self, a: f32, b: f32, c: f32, d: f32, e: f32, f: f32) {
        self.0.cubic_to(a, b, c, d, e, f);
    }
    fn close(&mut self) {
        self.0.close();
    }
}
fn atlas(face: &ttf_parser::Face<'_>, alphabet: &str) -> (Vec<u8>, [u32; 2]) {
    let rows = (alphabet.len() as u32).div_ceil(ATLAS_COLUMNS).max(1);
    let size = [ATLAS_CELL[0] * ATLAS_COLUMNS, ATLAS_CELL[1] * rows];
    let mut pixmap = tiny_skia::Pixmap::new(size[0], size[1]).unwrap();
    let mut paint = tiny_skia::Paint::default();
    paint.set_color_rgba8(255, 255, 255, 255);
    let em_scale = ATLAS_CELL[1] as f32 / f32::from(face.units_per_em());
    for (i, ch) in alphabet.chars().enumerate() {
        let Some(id) = face.glyph_index(ch) else {
            continue;
        };
        let mut outline = Outline(tiny_skia::PathBuilder::new());
        let Some(bounds) = face.outline_glyph(id, &mut outline) else {
            continue;
        };
        let Some(path) = outline.0.finish() else {
            continue;
        };
        // Ink bounds can overhang the advance or the assumed baseline on a
        // fallback face. Keep every outline inside its own cell, with a filter
        // gutter on all sides so adjacent atlas entries cannot cut or bleed.
        let ink_width = f32::from(bounds.x_max) - f32::from(bounds.x_min);
        let ink_height = f32::from(bounds.y_max) - f32::from(bounds.y_min);
        let scale = em_scale
            .min((ATLAS_CELL[0] as f32 - 4.) / ink_width.max(1.))
            .min((ATLAS_CELL[1] as f32 - 4.) / ink_height.max(1.));
        let x = (i as u32 % ATLAS_COLUMNS * ATLAS_CELL[0]) as f32
            + (ATLAS_CELL[0] as f32 - ink_width * scale) * 0.5
            - f32::from(bounds.x_min) * scale;
        let y = (i as u32 / ATLAS_COLUMNS * ATLAS_CELL[1]) as f32
            + (ATLAS_CELL[1] as f32 - ink_height * scale) * 0.5
            + f32::from(bounds.y_max) * scale;
        pixmap.fill_path(
            &path,
            &paint,
            tiny_skia::FillRule::Winding,
            tiny_skia::Transform::from_row(scale, 0., 0., -scale, x, y),
            None,
        );
    }
    (pixmap.take(), size)
}

#[derive(Clone)]
struct ScrambleView {
    base: GpuView,
    scramble: Arc<Scramble>,
    size: [u32; 2],
}
impl PartialEq for ScrambleView {
    fn eq(&self, other: &Self) -> bool {
        self.base == other.base && self.scramble.id == other.scramble.id && self.size == other.size
    }
}
impl ComponentView for ScrambleView {
    fn share_layouts(&mut self, share: &mut dyn FnMut(&mut Arc<nana_ui_core::LayoutStyle>)) {
        self.base.share_layouts(share);
    }
    fn node_kind(&self) -> NodeKind {
        self.base.node_kind()
    }
    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        self.base.project(id, world, mutations);
        self.scramble.capture(world, id);
        mutations.set_custom_render(
            id,
            Some(CustomRenderNode::new(
                RENDERER,
                format!("{RENDERER}:{}", self.scramble.id),
                1,
            )),
        );
    }
}

pub fn wrap(
    ui: Ui,
    text: AnyView,
    objects: Option<AnyView>,
    node: NodeRef,
    spec: ScrambleSpec,
    color: nanatype_model::Rgba8,
    schedule: super::text_light::TextSchedule<'_>,
) -> AnyView {
    let super::text_light::TextSchedule {
        at_ms,
        value,
        exit,
        preview,
    } = schedule;
    let text = if exit.is_none_or(|(at, _)| crate::clock::now_ms() < at) {
        let lock_times: Vec<_> = at_ms
            .iter()
            .map(|t| {
                t.map(|t| {
                    if t == 0 {
                        0
                    } else {
                        t + u64::from(spec.entrance_ms)
                    }
                })
            })
            .collect();
        super::text_light::wrap(
            ui,
            text,
            node,
            nanatype_theme::TextLightSpec {
                color,
                intensity: 0.22,
                radius: 1.,
            },
            super::text_light::TextSchedule {
                at_ms: &lock_times,
                value,
                exit: None,
                preview,
            },
            45,
        )
    } else {
        text
    };
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    let mut hasher = blake3::Hasher::new();
    hasher.update(value.as_bytes());
    if !preview {
        let display = ui.display.get_untracked();
        hasher.update(
            &display
                .line
                .map_or(display.revision, |id| id.0)
                .to_le_bytes(),
        );
    }
    let seed = u32::from_le_bytes(hasher.finalize().as_bytes()[..4].try_into().unwrap());
    let scramble = Arc::new(Scramble {
        id,
        ui,
        text: node,
        spec,
        color,
        // Keep GPU times close to zero even after days of mirror editing.
        born: crate::clock::now_ms().saturating_sub(2000),
        times: value
            .grapheme_indices(true)
            .zip(at_ms.iter().copied())
            .map(|((byte, g), at)| {
                (
                    byte,
                    byte + g.len(),
                    at,
                    g.chars().all(char::is_whitespace) || g == "\u{fffc}",
                )
            })
            .collect(),
        seed,
        exit,
        preview,
        geometry: Mutex::new(Geometry::default()),
    });
    LIVE.lock()
        .unwrap_or_else(|e| e.into_inner())
        .insert(id, Arc::downgrade(&scramble));
    let size = signal([0u32; 2]);
    let mut base = GpuView::new(id);
    let l = Arc::make_mut(&mut base.style.layout);
    l.position = PositionSpec::Absolute;
    l.offset_left = Some(LengthSpec::Px(0.));
    l.offset_top = Some(LengthSpec::Px(0.));
    l.width = Some(LengthSpec::Percent(100.));
    l.height = Some(LengthSpec::Percent(100.));
    l.pointer_events = Some(PointerEventsSpec::None);
    let layer = widget(ScrambleView {
        base,
        scramble,
        size: [0; 2],
    })
    .on(move |e: &SizeChanged| {
        size.set_if_changed([e.width.to_bits(), e.height.to_bits()]);
    })
    .bind(move |v: &mut ScrambleView| {
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
    .children((text, objects, layer))
    .into_any()
}

pub fn animating(ui: Ui, now: u64) -> bool {
    track_animating(ui, now, false)
}
pub fn preview_animating(ui: Ui, now: u64) -> bool {
    track_animating(ui, now, true)
}
fn track_animating(ui: Ui, now: u64, preview: bool) -> bool {
    let live: Vec<_> = LIVE
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .values()
        .filter_map(Weak::upgrade)
        .collect();
    live.into_iter().any(|s| {
        s.preview == preview
            && if preview {
                s.ui.shader_preview == ui.shader_preview
            } else {
                s.ui.display == ui.display
            }
            && s.active(now)
    })
}
pub fn register(registry: &mut SceneGpuRendererRegistry) {
    registry.insert(RENDERER, SHARED.clone());
}
pub fn collect_garbage() {
    let mut state = SHARED.state.lock().unwrap_or_else(|e| e.into_inner());
    state.prepared.retain(|_, p| p.scramble.strong_count() > 0);
    let live = LIVE.lock().unwrap_or_else(|e| e.into_inner());
    state.scales.retain(|(_, id, _)| live.contains_key(id));
    state
        .placements
        .retain(|(_, id, _), _| live.contains_key(id));
}
pub fn cache_counts() -> (usize, usize) {
    let state = SHARED.state.lock().unwrap_or_else(|e| e.into_inner());
    (
        state.prepared.len(),
        state.prepared.values().map(|p| p.bytes).sum(),
    )
}

type PipelineKey = (DeviceGeneration, GpuTextureFormat, ScenePresentationProfile);
type PreparedKey = (PipelineKey, u64, u64);
#[derive(Debug)]
struct Pipeline {
    pipeline: wgpu::RenderPipeline,
    layout: wgpu::BindGroupLayout,
}
#[derive(Debug)]
struct Prepared {
    scramble: Weak<Scramble>,
    uniforms: super::effect_uniforms::Uniforms,
    layout: wgpu::BindGroupLayout,
    atlas: wgpu::TextureView,
    sampler: wgpu::Sampler,
    pipeline: wgpu::RenderPipeline,
    instances: wgpu::Buffer,
    count: u32,
    bytes: usize,
}
type Placements = HashMap<(DeviceGeneration, u64, [u32; 2]), ([f32; 2], [f32; 2])>;
#[derive(Debug, Default)]
struct State {
    pipelines: HashMap<PipelineKey, Pipeline>,
    prepared: HashMap<PreparedKey, Prepared>,
    scales: super::effect_uniforms::Scales,
    placements: Placements,
}
#[derive(Debug, Default)]
struct Renderer {
    state: Mutex<State>,
}
static SHARED: LazyLock<Arc<Renderer>> = LazyLock::new(|| Arc::new(Renderer::default()));

impl State {
    fn prepare_cells(
        &mut self,
        scramble: &Arc<Scramble>,
        c: &SceneGpuPassContext<'_>,
    ) -> Option<&mut Prepared> {
        use wgpu::util::DeviceExt;
        let geometry = scramble.geometry.lock().unwrap_or_else(|e| e.into_inner());
        if geometry.cells.is_empty() || geometry.atlas.is_empty() {
            return None;
        }
        let pk = (c.gpu.generation(), c.target_format, c.presentation);
        let key = (pk, scramble.id, geometry.revision);
        if self.prepared.contains_key(&key) {
            return self.prepared.get_mut(&key);
        }
        self.prepared.retain(|_, p| p.scramble.strong_count() > 0);
        if self.prepared.len() >= 32
            || self.prepared.values().map(|p| p.bytes).sum::<usize>() > 32 * 1024 * 1024
        {
            self.prepared.clear();
        }
        if self.pipelines.len() >= 16 && !self.pipelines.contains_key(&pk) {
            self.pipelines.clear();
            self.prepared.clear();
        }
        let device = c.gpu.wgpu().device();
        let pipeline = self.pipelines.entry(pk).or_insert_with(|| {
            let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some(RENDERER),
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
                label: Some(RENDERER),
                source: wgpu::ShaderSource::Wgsl(include_str!("text_scramble.wgsl").into()),
            });
            let pl = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(RENDERER),
                bind_group_layouts: &[Some(&layout)],
                immediate_size: 0,
            });
            let attributes = wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4];
            let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(RENDERER),
                layout: Some(&pl),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vertex_main"),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: 32,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &attributes,
                    })],
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
        let texture = c
            .gpu
            .create_texture(&nana_ui::GpuTextureDescriptor {
                label: Some("scramble monospace atlas"),
                width: geometry.atlas_size[0],
                height: geometry.atlas_size[1],
                format: GpuTextureFormat::RGBA8_UNORM,
                usage: nana_ui::GpuTextureUsages::SAMPLED | nana_ui::GpuTextureUsages::COPY_DST,
            })
            .ok()?;
        c.gpu
            .write_texture(
                &texture,
                nana_ui::GpuTextureRegion {
                    x: 0,
                    y: 0,
                    width: geometry.atlas_size[0],
                    height: geometry.atlas_size[1],
                },
                &geometry.atlas,
                geometry.atlas_size[0] * 4,
            )
            .ok()?;
        let data: Vec<u8> = geometry
            .cells
            .iter()
            .flatten()
            .flat_map(|v| v.to_ne_bytes())
            .collect();
        let instances = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("retained scramble slots"),
            contents: &data,
            usage: wgpu::BufferUsages::VERTEX,
        });
        let bytes = data.len() + geometry.atlas.len();
        if let Some(work) = c.gpu_work {
            work.record_upload(bytes);
            work.record_realloc();
        }
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some(RENDERER),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        self.prepared.insert(
            key,
            Prepared {
                scramble: Arc::downgrade(scramble),
                uniforms: Default::default(),
                layout: pipeline.layout.clone(),
                atlas: texture.wgpu_view().clone(),
                sampler,
                pipeline: pipeline.pipeline.clone(),
                instances,
                count: geometry.cells.len() as u32,
                bytes,
            },
        );
        self.prepared.get_mut(&key)
    }
}
fn material(node: &SceneGpuNode) -> Option<Arc<Scramble>> {
    LIVE.lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(
            &node
                .custom
                .resource
                .strip_prefix("nanatype.text-scramble:")?
                .parse()
                .ok()?,
        )?
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
        if let Some(s) = material(node) {
            let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
            state
                .scales
                .record(c.gpu.generation(), s.id, c.dest_size, c.scale_factor);
            if state.placements.len() >= 128 {
                state.placements.clear();
            }
            state.placements.insert(
                (c.gpu.generation(), s.id, c.dest_size),
                ([c.bounds.x, c.bounds.y], [c.bounds.width, c.bounds.height]),
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
        c.with_pass(RENDERER, |pass| {
            self.draw_in_pass(node, pass, pc);
        });
    }
    fn draw_in_pass(
        &self,
        node: &SceneGpuNode,
        pass: &mut ScenePass<'_, '_>,
        c: SceneGpuPassContext<'_>,
    ) -> bool {
        let Some(s) = material(node) else {
            return true;
        };
        let now = crate::clock::now_ms();
        if !s.active(now) || c.bounds.width == 0 || c.bounds.height == 0 {
            return true;
        }
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let dpi = state.scales.get(c.gpu.generation(), s.id, c.dest_size);
        let (position, extent) = state
            .placements
            .get(&(c.gpu.generation(), s.id, c.dest_size))
            .copied()
            .unwrap_or((
                [c.bounds.x as f32 / dpi, c.bounds.y as f32 / dpi],
                [c.bounds.width as f32 / dpi, c.bounds.height as f32 / dpi],
            ));
        let (origin, size) = {
            let g = s.geometry.lock().unwrap_or_else(|e| e.into_inner());
            (g.origin, g.size)
        };
        let scale = extent[0] * dpi / size[0].max(1.);
        if !scale.is_finite() || scale <= 0. {
            return true;
        }
        let crop = [
            (c.bounds.x as f32 - position[0] * dpi) / scale,
            (c.bounds.y as f32 - position[1] * dpi) / scale,
        ];
        let Some(p) = state.prepare_cells(&s, &c) else {
            return true;
        };
        let exit = s
            .exit
            .filter(|(at, _)| now >= *at)
            .map_or(-1., |(at, _)| now.saturating_sub(at) as f32);
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
            node.opacity,
            linear(s.color.r),
            linear(s.color.g),
            linear(s.color.b),
            f32::from(s.color.a) / 255.,
            now.saturating_sub(s.born) as f32,
            f32::from(s.spec.entrance_ms),
            exit,
            f32::from(s.spec.interval_ms),
            origin[0],
            origin[1],
            crop[0],
            crop[1],
            f32::from(s.spec.exit_hold_ms),
            f32::from(s.exit.map_or(s.spec.exit_ms, |(_, ms)| ms)),
            s.spec.alphabet.len() as f32,
            (s.seed & 0xffff) as f32,
        ];
        let upload: Vec<u8> = values.iter().flat_map(|v| v.to_ne_bytes()).collect();
        let interop = c.gpu.wgpu();
        let _submission = interop.lock_submission();
        let binding = p.uniforms.binding(
            c.gpu,
            &p.layout,
            std::slice::from_ref(&p.atlas),
            &p.sampler,
            super::effect_uniforms::output_key(&c, scale, node.opacity),
            upload.len(),
        );
        interop.queue().write_buffer(&binding.buffer, 0, &upload);
        if let Some(work) = c.gpu_work {
            work.record_upload(upload.len());
            work.record_draw_call();
        }
        let raw = pass.wgpu();
        raw.set_pipeline(&p.pipeline);
        raw.set_bind_group(0, &binding.group, &[]);
        raw.set_vertex_buffer(0, p.instances.slice(..));
        raw.draw(0..6, 0..p.count);
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrow_slots_preserve_the_complete_cells_aspect_ratio() {
        for (width, height) in [(1., 28.), (6., 28.), (14., 28.), (28., 28.), (28., 6.)] {
            let slot = [17., 31., width, height];
            let cell = fit_cell(slot);
            assert!(cell[0] >= slot[0] && cell[1] >= slot[1]);
            assert!(cell[0] + cell[2] <= slot[0] + width + 0.0001);
            assert!(cell[1] + cell[3] <= slot[1] + height + 0.0001);
            assert!((cell[2] / 40. - cell[3] / 64.).abs() < 0.0001);
            assert!((cell[0] + cell[2] * 0.5 - (slot[0] + width * 0.5)).abs() < 0.0001);
            assert!((cell[1] + cell[3] * 0.5 - (slot[1] + height * 0.5)).abs() < 0.0001);
        }
    }

    #[test]
    fn atlas_ink_stays_inside_every_cell_with_a_filter_gutter() {
        let shaper = nana_ui::NanaTextShaper::default();
        let engine = shaper.text_engine().unwrap();
        let mut engine = engine.lock().unwrap();
        let query = nana_text::font::FontQuery {
            families: nana_text::font::FamilyList::parse_css("Consolas, Menlo, monospace"),
            ..Default::default()
        };
        let selected = engine.fonts_mut().select(&query);
        let data = selected
            .chain()
            .find_map(|font| engine.fonts().face_data(font))
            .unwrap();
        let face = ttf_parser::Face::parse(data.bytes(), data.index()).unwrap();
        let alphabet = ScrambleSpec::default().alphabet;
        let (rgba, size) = atlas(&face, &alphabet);
        for (i, ch) in alphabet.chars().enumerate() {
            let mut covered = false;
            for y in 0..64 {
                for x in 0..40 {
                    let at =
                        ((i as u32 / 16 * 64 + y) * size[0] + i as u32 % 16 * 40 + x) as usize * 4;
                    let alpha = rgba[at + 3];
                    covered |= alpha > 0;
                    if x == 0 || x == 39 || y == 0 || y == 63 {
                        assert_eq!(alpha, 0, "{ch}: ink touches the atlas cell edge");
                    }
                }
            }
            assert!(covered, "{ch}: replacement glyph is missing");
        }
    }

    #[test]
    fn disappearance_is_a_reproducible_permutation() {
        for n in [0, 1, 2, 17, 300, 10000] {
            let ranks = permutation(n, 12345);
            assert_eq!(ranks, permutation(n, 12345));
            let mut sorted = ranks.clone();
            sorted.sort_unstable();
            assert_eq!(sorted, (0..n).collect::<Vec<_>>());
            if n > 2 {
                assert_ne!(ranks, sorted);
            }
        }
    }
}
