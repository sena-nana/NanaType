//! Host execution of declarative skin pipelines that need the text layout.
//! Registered processors run natively; results are immutable material inputs
//! keyed by data, material and layout, so playback only advances GPU progress.
//!
//! The UI thread only snapshots the layout. With a waker installed (the
//! application), processors run on a background worker; until the result
//! arrives the material's placeholder slots give the shader's plain fade, and
//! the finished map replaces it without restarting the reveal clock. Stale jobs
//! for a view whose layout has since changed are skipped. Without a waker
//! (offscreen tests, tools) processors run inline.
use nana_ui::runtime::view::{AnyView, IntoView, NodeRef, Signal, signal, widget};
use nana_ui::runtime::{
    ComponentView, MutationQueue, NodeKind, RichTextView, SizeChanged, StableNodeId, UiWorld,
};
use nana_ui_core::{MaterialRef, MaterialTexture};
use nanatype_theme::{BoundInput, BoundNode, DataValue, MaterialBinding};
use std::{
    collections::{BTreeMap, HashMap},
    hash::{Hash, Hasher},
    sync::{
        Arc, LazyLock, Mutex, OnceLock, Weak,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
};

static BUILDS: AtomicUsize = AtomicUsize::new(0);
static MAPS: LazyLock<Mutex<HashMap<u64, Weak<nana_ui_core::MaterialInstance>>>> =
    LazyLock::new(Default::default);
/// Finished background results awaiting pickup, newest last; bounded.
static DONE: LazyLock<Mutex<Vec<(u64, Outputs)>>> = LazyLock::new(Default::default);
const MAX_DONE: usize = 8;
const MAX_DONE_BYTES: usize = 128 * 1024 * 1024;
static WAKER: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();
static WORKER: OnceLock<mpsc::SyncSender<Job>> = OnceLock::new();

type Outputs = Vec<BTreeMap<&'static str, PortValue>>;
type Prepared = Box<dyn FnOnce() -> Option<BTreeMap<&'static str, PortValue>> + Send>;
/// Owned by the view; dropping it also invalidates queued and running work.
type Slot = Arc<Mutex<Option<u64>>>;
struct Job {
    slot: Option<Weak<Mutex<Option<u64>>>>,
    key: u64,
    nodes: Vec<Prepared>,
}
impl Job {
    fn current(&self) -> bool {
        self.slot.as_ref().is_none_or(|slot| {
            slot.upgrade().is_some_and(|slot| {
                *slot.lock().unwrap_or_else(|e| e.into_inner()) == Some(self.key)
            })
        })
    }
}

fn output_bytes(outputs: &Outputs) -> usize {
    outputs
        .iter()
        .flat_map(BTreeMap::values)
        .map(|v| match v {
            PortValue::Texture(t) => t.rgba.len(),
            PortValue::Vec4(_) => 16,
        })
        .sum()
}

fn remember(key: u64, outputs: Outputs) {
    let mut done = DONE.lock().unwrap_or_else(|e| e.into_inner());
    done.retain(|(old, _)| *old != key);
    done.push((key, outputs));
    while done.len() > MAX_DONE
        || done.iter().map(|(_, o)| output_bytes(o)).sum::<usize>() > MAX_DONE_BYTES
    {
        done.remove(0);
    }
}

/// Run processors in the background and call `wake` (from any thread) when a
/// result is ready; the receiver bumps the layout epoch on the UI thread.
pub fn set_waker(wake: impl Fn() + Send + Sync + 'static) {
    let _ = WAKER.set(Box::new(wake));
}

/// Wait until queued jobs are finished; for tests and shutdown audits.
pub fn settle() {
    let (done, wait) = mpsc::channel();
    if worker()
        .send(Job {
            slot: None,
            key: 0,
            nodes: vec![Box::new(move || {
                let _ = done.send(());
                None
            })],
        })
        .is_ok()
    {
        let _ = wait.recv_timeout(std::time::Duration::from_secs(30));
    }
}

fn worker() -> &'static mpsc::SyncSender<Job> {
    WORKER.get_or_init(|| {
        let (send, receive) = mpsc::sync_channel::<Job>(MAX_DONE);
        let spawned = std::thread::Builder::new()
            .name("layout-material".into())
            .spawn(move || {
                while let Ok(job) = receive.recv() {
                    if job.current() {
                        let outputs: Option<Outputs> =
                            job.nodes.into_iter().map(|run| run()).collect();
                        if let Some(slot) = job.slot.as_ref().and_then(Weak::upgrade) {
                            let mut wanted = slot.lock().unwrap_or_else(|e| e.into_inner());
                            // A layout change or unmount during conversion cannot publish.
                            if *wanted == Some(job.key) {
                                if let Some(outputs) = outputs {
                                    remember(job.key, outputs);
                                }
                                *wanted = None;
                            }
                        }
                    }
                    // Also retry views refused by a full queue when stale jobs drain.
                    if job.slot.is_some()
                        && let Some(wake) = WAKER.get()
                    {
                        wake();
                    }
                }
            });
        if let Err(error) = spawned {
            nanatype_diag::report!(crate::diag::WORKER_UNAVAILABLE; "layout-material: {error}");
        }
        send
    })
}

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
    /// Identifies this view's newest request to the background worker.
    slot: Slot,
    /// Advances when background results arrive, so finished maps project.
    epoch: u64,
}
impl PartialEq for LayoutMaterialView {
    fn eq(&self, other: &Self) -> bool {
        self.base == other.base
            && self.material == other.material
            && self.size == other.size
            && self.epoch == other.epoch
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
            && let Some(material) = build(
                world,
                id,
                &base.value,
                &self.material,
                &mask.material,
                &self.slot,
            )
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
    epoch: Signal<u64>,
    update: impl Fn(&mut RichTextView) + Send + 'static,
    node: Option<NodeRef>,
) -> AnyView {
    let size = signal([0; 2]);
    let v = widget(LayoutMaterialView {
        base,
        material,
        size: [0; 2],
        slot: Arc::new(Mutex::new(None)),
        epoch: 0,
    })
    .on(move |e: &SizeChanged| {
        size.set_if_changed([e.width.to_bits(), e.height.to_bits()]);
    })
    .bind(move |v: &mut LayoutMaterialView| {
        update(&mut v.base);
        v.size = size.get();
        v.epoch = epoch.get();
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

#[derive(Clone)]
enum PortValue {
    Texture(MaterialTexture),
    Vec4([f32; 4]),
}

/// Snapshot one registered processor's context on the UI thread; the
/// returned work owns everything it needs. `None` when the context is
/// unavailable. No registered processor consumes another's output yet.
fn prepare(
    node: &BoundNode,
    world: &UiWorld,
    id: StableNodeId,
    text: &nana_ui_core::RichText,
) -> Option<Prepared> {
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
            let job = super::handwriting::stroke_time_job(world, id, text, strokes)?;
            Some(Box::new(move || {
                let (time, extent) = job.run();
                Some(BTreeMap::from([
                    ("time", PortValue::Texture(time)),
                    ("extent", PortValue::Vec4(extent)),
                ]))
            }))
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
    slot: &Slot,
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
    if let Some(material) = MAPS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get(&key)
        .and_then(MaterialRef::upgrade)
    {
        *slot.lock().unwrap_or_else(|e| e.into_inner()) = None;
        return Some(material);
    }
    let finished = {
        let mut done = DONE.lock().unwrap_or_else(|e| e.into_inner());
        done.iter()
            .position(|(k, _)| *k == key)
            .map(|at| done.remove(at).1)
    };
    let computed = match finished {
        Some(outputs) => {
            *slot.lock().unwrap_or_else(|e| e.into_inner()) = None;
            outputs
        }
        None => {
            let background = WAKER.get().is_some();
            let mut wanted = slot.lock().unwrap_or_else(|e| e.into_inner());
            if background && *wanted == Some(key) {
                return None; // Already queued; its result wakes this view.
            }
            *wanted = None;
            let nodes = material
                .binding
                .nodes
                .iter()
                .map(|node| prepare(node, world, id, text))
                .collect::<Option<Vec<Prepared>>>()?;
            if background {
                *wanted = Some(key);
                if worker()
                    .try_send(Job {
                        slot: Some(Arc::downgrade(slot)),
                        key,
                        nodes,
                    })
                    .is_err()
                {
                    *wanted = None;
                }
                return None; // Backpressure keeps the placeholder; never block the UI.
            }
            drop(wanted);
            nodes.into_iter().map(|run| run()).collect::<Option<_>>()?
        }
    };
    let output = |(node, port): (usize, &'static str)| computed.get(node)?.get(port).cloned();
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
    let mut cache = MAPS.lock().unwrap_or_else(|e| e.into_inner());
    cache.retain(|_, v| v.strong_count() > 0);
    cache.insert(key, built.downgrade());
    BUILDS.fetch_add(1, Ordering::Relaxed);
    Some(built)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registered_processor_has_a_host_implementation() {
        for p in nanatype_theme::PROCESSORS {
            assert!(super::implemented(p.name, p.version), "{}", p.name);
        }
    }

    #[test]
    fn background_queue_discards_obsolete_work_and_bounds_results() {
        use std::{sync::atomic::AtomicUsize, time::Duration};
        let slot = Arc::new(Mutex::new(Some(1)));
        let removed = Arc::new(Mutex::new(Some(2)));
        let (entered, started) = mpsc::channel();
        let (release, gate) = mpsc::channel();
        worker()
            .try_send(Job {
                slot: Some(Arc::downgrade(&slot)),
                key: 1,
                nodes: vec![Box::new(move || {
                    entered.send(()).unwrap();
                    gate.recv_timeout(Duration::from_secs(30)).unwrap();
                    Some(BTreeMap::new())
                })],
            })
            .unwrap_or_else(|_| panic!("worker available"));
        started.recv_timeout(Duration::from_secs(30)).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        for i in 0..=MAX_DONE {
            let calls = calls.clone();
            let pending = if i % 2 == 0 { &slot } else { &removed };
            let result = worker().try_send(Job {
                slot: Some(Arc::downgrade(pending)),
                key: if i % 2 == 0 { 1 } else { 2 },
                nodes: vec![Box::new(move || {
                    calls.fetch_add(1, Ordering::Relaxed);
                    Some(BTreeMap::new())
                })],
            });
            assert_eq!(result.is_ok(), i < MAX_DONE, "bounded queue");
        }
        // The running job becomes obsolete; queued jobs change layout or unmount.
        *slot.lock().unwrap() = Some(3);
        drop(removed);
        release.send(()).unwrap();
        settle();
        assert_eq!(calls.load(Ordering::Relaxed), 0);
        assert_eq!(*slot.lock().unwrap(), Some(3));
        assert!(
            !DONE
                .lock()
                .unwrap()
                .iter()
                .any(|(key, _)| *key == 1 || *key == 2)
        );
        worker()
            .try_send(Job {
                slot: Some(Arc::downgrade(&slot)),
                key: 3,
                nodes: vec![Box::new(|| Some(BTreeMap::new()))],
            })
            .unwrap_or_else(|_| panic!("queue drained"));
        settle();
        assert_eq!(*slot.lock().unwrap(), None);
        assert!(DONE.lock().unwrap().iter().any(|(key, _)| *key == 3));
        // Shared payloads keep this check small while accounting every output.
        let texture = MaterialTexture {
            size: [2048; 2],
            rgba: vec![0; 2048 * 2048 * 4].into(),
        };
        for key in 4..16 {
            remember(
                key,
                vec![BTreeMap::from([("time", PortValue::Texture(texture.clone()))]); 3],
            );
        }
        let mut done = DONE.lock().unwrap();
        assert!(done.len() <= MAX_DONE);
        assert!(done.iter().map(|(_, o)| output_bytes(o)).sum::<usize>() <= MAX_DONE_BYTES);
        assert_eq!(done.last().unwrap().0, 15);
        done.clear();
    }
}
