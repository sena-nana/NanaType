//! Host execution of declarative skin pipelines that need the text layout.
//! Registered processors run natively; results are immutable material inputs
//! keyed by data, material and layout, so playback only advances GPU progress.
use nana_ui::runtime::view::{AnyView, IntoView, NodeRef, signal, widget};
use nana_ui::runtime::{
    ComponentView, MutationQueue, NodeKind, RichTextView, SizeChanged, StableNodeId, UiWorld,
};
use nana_ui_core::{MaterialRef, MaterialTexture};
use nanatype_theme::{BoundInput, BoundNode, DataValue, MaterialBinding};
use std::{
    collections::{BTreeMap, HashMap},
    hash::{Hash, Hasher},
    sync::{
        Arc, LazyLock, Mutex, Weak,
        atomic::{AtomicUsize, Ordering},
    },
};

static BUILDS: AtomicUsize = AtomicUsize::new(0);
static MAPS: LazyLock<Mutex<HashMap<u64, Weak<nana_ui_core::MaterialInstance>>>> =
    LazyLock::new(Default::default);

/// A material whose inputs come from layout-dependent pipelines.
#[derive(Clone, Debug)]
pub struct LayoutMaterial {
    pub binding: Arc<MaterialBinding>,
    /// `vec4` slots of static params preceding the pipeline uniforms.
    pub static_vec4: usize,
}
impl PartialEq for LayoutMaterial {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.binding, &other.binding) && self.static_vec4 == other.static_vec4
    }
}
impl LayoutMaterial {
    /// The bound entrance material of a skin, when it needs the text layout.
    pub fn entrance(skin: &nanatype_theme::Skin) -> Option<Self> {
        let name = skin.dialogue.reveal.entrance.material.as_deref()?;
        let binding = skin.material_bindings.get(name)?.clone();
        let static_vec4 = skin.materials.get(name)?.params.len().div_ceil(4);
        binding.text_layout.then_some(Self {
            binding,
            static_vec4,
        })
    }
}

#[derive(Clone)]
struct LayoutMaterialView {
    base: RichTextView,
    material: LayoutMaterial,
    size: [u32; 2],
}
impl PartialEq for LayoutMaterialView {
    fn eq(&self, other: &Self) -> bool {
        self.base == other.base && self.material == other.material && self.size == other.size
    }
}
impl ComponentView for LayoutMaterialView {
    fn share_layouts(&mut self, share: &mut dyn FnMut(&mut Arc<nana_ui_core::LayoutStyle>)) {
        self.base.share_layouts(share);
    }
    fn node_kind(&self) -> NodeKind {
        self.base.node_kind()
    }
    fn project(&self, id: StableNodeId, world: &UiWorld, mutations: &mut MutationQueue) {
        let mut base = self.base.clone();
        let current = world.glyph_presentation(id);
        let mut replacement = None;
        if let Some(p) = &mut base.presentation
            && let Some(mask) = &mut p.transitions.entrance
            && let Some(material) = build(world, id, &base.value, &self.material, &mask.material)
        {
            mask.material = material.clone();
            if let Some((_, layout)) = world.text_layout(id) {
                let (w, h) = layout.physical_size();
                // Anchor the immutable map to content coordinates. Ancestor
                // clipping still clips paint; scrolling never rebuilds maps.
                p.transitions.viewport =
                    Some(nana_ui_core::LogicalRect::new(0., 0., w.max(1.), h.max(1.)));
            }
            // A size/font change replaces only the immutable map. Keep the
            // running presentation clock, including a frozen retirement.
            if let Some(old) = current
                && old.effects == p.effects
                && old.effect_chains == p.effect_chains
                && old
                    .reveal
                    .as_ref()
                    .zip(p.reveal.as_ref())
                    .is_some_and(|(a, b)| {
                        a.at_s == b.at_s && a.limit == b.limit && a.intro == b.intro
                    })
            {
                let mut normalized = old.transitions.clone();
                normalized.viewport = p.transitions.viewport;
                if let (Some(a), Some(b)) = (&mut normalized.entrance, &p.transitions.entrance) {
                    a.start = b.start;
                    a.material = b.material.clone();
                }
                if let (Some(a), Some(b)) = (&mut normalized.exit, &p.transitions.exit) {
                    a.start = b.start;
                }
                if normalized.freeze_reveal.is_some() && p.transitions.freeze_reveal.is_some() {
                    normalized.freeze_reveal = p.transitions.freeze_reveal;
                }
                if normalized == p.transitions {
                    let mut keep = old.clone();
                    keep.transitions.viewport = p.transitions.viewport;
                    if let Some(m) = &mut keep.transitions.entrance {
                        m.material = material;
                    }
                    replacement = Some(keep);
                }
            }
        }
        base.project(id, world, mutations);
        if let Some(p) = replacement.filter(|p| current != Some(p)) {
            mutations.set_glyph_presentation(id, Some(p));
        }
    }
}

pub fn view(
    base: RichTextView,
    material: LayoutMaterial,
    update: impl Fn(&mut RichTextView) + Send + 'static,
    node: Option<NodeRef>,
) -> AnyView {
    let size = signal([0; 2]);
    let v = widget(LayoutMaterialView {
        base,
        material,
        size: [0; 2],
    })
    .on(move |e: &SizeChanged| {
        size.set_if_changed([e.width.to_bits(), e.height.to_bits()]);
    })
    .bind(move |v: &mut LayoutMaterialView| {
        update(&mut v.base);
        v.size = size.get();
    });
    match node {
        Some(node) => v.node_ref(node).into_any(),
        None => v.into_any(),
    }
}

/// Number of layout-dependent material inputs built, for rendering regression tests.
pub fn builds() -> usize {
    BUILDS.load(Ordering::Relaxed)
}

enum PortValue {
    Texture(MaterialTexture),
    Vec4([f32; 4]),
}

/// Run one registered processor; `None` when its context is unavailable.
/// No registered processor consumes another's output yet.
fn execute(
    node: &BoundNode,
    world: &UiWorld,
    id: StableNodeId,
    text: &nana_ui_core::RichText,
) -> Option<BTreeMap<&'static str, PortValue>> {
    let data = |name: &str| match node.inputs.get(name)? {
        BoundInput::Data(data) => Some(data.clone()),
        BoundInput::Node { .. } => None,
    };
    match (node.processor.name, node.processor.version) {
        ("text.stroke_time_map", 1) => {
            let strokes = data("strokes")?;
            let DataValue::Strokes(strokes) = &strokes.value else {
                return None;
            };
            let (time, extent) = super::handwriting::stroke_time_map(world, id, text, strokes)?;
            Some(BTreeMap::from([
                ("time", PortValue::Texture(time)),
                ("extent", PortValue::Vec4(extent)),
            ]))
        }
        _ => None,
    }
}

/// Whether the host implements a processor; every registered one must be.
pub fn implemented(name: &str, version: u32) -> bool {
    matches!((name, version), ("text.stroke_time_map", 1))
}

fn build(
    world: &UiWorld,
    id: StableNodeId,
    text: &nana_ui_core::RichText,
    material: &LayoutMaterial,
    template: &MaterialRef,
) -> Option<MaterialRef> {
    let (_, layout) = world.text_layout(id)?;
    let physical = layout.physical_size();
    let size = [physical.0.max(1.), physical.1.max(1.)];
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    material.binding.key.hash(&mut hasher);
    template.key().hash(&mut hasher);
    text.text().hash(&mut hasher);
    size.map(f32::to_bits).hash(&mut hasher);
    layout.is_vertical().hash(&mut hasher);
    for line in &layout.lines {
        line.metrics.baseline_y_px.to_bits().hash(&mut hasher);
        for run in layout.line_runs(line) {
            format!("{:?}", run.font).hash(&mut hasher);
            run.font_size_px.to_bits().hash(&mut hasher);
            run.origin_x_px.to_bits().hash(&mut hasher);
            for g in &run.glyphs {
                g.glyph_id.hash(&mut hasher);
                g.cluster.hash(&mut hasher);
                [g.offset_x_px, g.offset_y_px, g.advance_px]
                    .map(f32::to_bits)
                    .hash(&mut hasher);
            }
        }
    }
    let key = hasher.finish();
    let mut cache = MAPS.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(material) = cache.get(&key).and_then(MaterialRef::upgrade) {
        return Some(material);
    }
    let mut computed = Vec::with_capacity(material.binding.nodes.len());
    for node in &material.binding.nodes {
        let outputs = execute(node, world, id, text)?;
        computed.push(outputs);
    }
    let mut output = |(node, port): (usize, &'static str)| computed[node].remove(port);
    // Static params first, then pipeline uniforms; image slots keep their texture.
    let mut parameters: Vec<[f32; 4]> = template
        .parameters()
        .iter()
        .take(material.static_vec4)
        .copied()
        .collect();
    for slot in &material.binding.uniforms {
        match output(*slot)? {
            PortValue::Vec4(v) => parameters.push(v),
            PortValue::Texture(_) => return None,
        }
    }
    let mut textures = template.textures().to_vec();
    for (index, slot) in material.binding.textures.iter().enumerate() {
        if let Some(slot) = slot {
            let PortValue::Texture(texture) = output(*slot)? else {
                return None;
            };
            *textures.get_mut(index)? = texture;
        }
    }
    if parameters.is_empty() {
        parameters.push([0.; 4]);
    }
    let built = template
        .clone()
        .with_parameters(Arc::<[[f32; 4]]>::from(parameters))
        .with_textures(Arc::<[MaterialTexture]>::from(textures));
    cache.retain(|_, v| v.strong_count() > 0);
    cache.insert(key, built.downgrade());
    BUILDS.fetch_add(1, Ordering::Relaxed);
    Some(built)
}

#[cfg(test)]
mod tests {
    #[test]
    fn every_registered_processor_has_a_host_implementation() {
        for p in nanatype_theme::PROCESSORS {
            assert!(super::implemented(p.name, p.version), "{}", p.name);
        }
    }
}
