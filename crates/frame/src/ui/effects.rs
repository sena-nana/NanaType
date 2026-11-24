//! Pieces shared by the custom GPU effects (sweep, text light, text
//! scramble, skin shaders).
use nana_ui::DeviceGeneration;
use nana_ui::runtime::view::{AnyView, IntoView, NodeRef, Signal, widget};
use nana_ui::runtime::{SizeChanged, StableNodeId, Stack, UiWorld};
use nana_ui::{SceneGpuPassContext, SceneGpuPrepareContext};
use nana_ui_core::{LengthSpec, PointerEventsSpec, PositionSpec};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::hash::Hash;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, LazyLock, Mutex, MutexGuard, Weak};

/// The live materials of one effect. A view owns its material; renderers find
/// it by id from the scene node, and a dropped material simply disappears.
pub(super) struct Registry<T> {
    next: AtomicU64,
    live: Mutex<HashMap<u64, Weak<T>>>,
}

impl<T> Default for Registry<T> {
    fn default() -> Self {
        Self {
            next: AtomicU64::new(1),
            live: Mutex::default(),
        }
    }
}

impl<T> Registry<T> {
    pub fn next_id(&self) -> u64 {
        self.next.fetch_add(1, Ordering::Relaxed)
    }
    fn live(&self) -> MutexGuard<'_, HashMap<u64, Weak<T>>> {
        self.live.lock().unwrap_or_else(|e| e.into_inner())
    }
    pub fn insert(&self, id: u64, material: &Arc<T>) {
        self.live().insert(id, Arc::downgrade(material));
    }
    /// For the material's `Drop`.
    pub fn remove(&self, id: u64) {
        self.live().remove(&id);
    }
    pub fn get(&self, id: u64) -> Option<Arc<T>> {
        self.live().get(&id)?.upgrade()
    }
    pub fn contains(&self, id: u64) -> bool {
        self.live().contains_key(&id)
    }
    /// Strong references to every live material, taken with the lock released
    /// before the caller looks at them: reading signals, or dropping the last
    /// reference (whose `Drop` calls [`remove`](Self::remove)), must never
    /// happen under it.
    pub fn snapshot(&self) -> Vec<Arc<T>> {
        self.live().values().filter_map(Weak::upgrade).collect()
    }
}

/// Bounded O(log N) LRU cache replacing `.clear()`-all cliffs.
#[derive(Debug)]
pub(super) struct LruMap<K, V> {
    max_entries: usize,
    clock: u64,
    map: HashMap<K, (u64, V)>,
    order: BTreeMap<u64, K>,
}

impl<K: Clone + Eq + Hash, V> LruMap<K, V> {
    pub fn new(max_entries: usize) -> Self {
        Self {
            max_entries: max_entries.max(1),
            clock: 0,
            map: HashMap::new(),
            order: BTreeMap::new(),
        }
    }
    pub fn len(&self) -> usize {
        self.map.len()
    }
    pub fn contains_key(&self, key: &K) -> bool {
        self.map.contains_key(key)
    }
    pub fn get(&mut self, key: &K) -> Option<&V> {
        if !self.map.contains_key(key) {
            return None;
        }
        self.touch_stamp(key);
        self.map.get(key).map(|(_, v)| v)
    }
    pub fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        if !self.map.contains_key(key) {
            return None;
        }
        self.touch_stamp(key);
        self.map.get_mut(key).map(|(_, v)| v)
    }
    fn touch_stamp(&mut self, key: &K) {
        self.clock += 1;
        let next = self.clock;
        if let Some((stamp, _)) = self.map.get_mut(key) {
            let old = *stamp;
            *stamp = next;
            self.order.remove(&old);
            self.order.insert(next, key.clone());
        }
    }
    pub fn insert(&mut self, key: K, value: V) {
        if let Some((old_stamp, old_val)) = self.map.get_mut(&key) {
            let old = *old_stamp;
            self.clock += 1;
            *old_stamp = self.clock;
            *old_val = value;
            self.order.remove(&old);
            self.order.insert(self.clock, key);
            return;
        }
        while self.map.len() >= self.max_entries {
            if self.evict_oldest().is_none() {
                break;
            }
        }
        self.clock += 1;
        let stamp = self.clock;
        self.order.insert(stamp, key.clone());
        self.map.insert(key, (stamp, value));
    }
    pub fn evict_oldest(&mut self) -> Option<(K, V)> {
        let (&stamp, _) = self.order.iter().next()?;
        let key = self.order.remove(&stamp)?;
        let (_, value) = self.map.remove(&key)?;
        Some((key, value))
    }
    pub fn retain(&mut self, mut keep: impl FnMut(&K, &mut V) -> bool) {
        let order = &mut self.order;
        self.map.retain(|k, (stamp, v)| {
            let ok = keep(k, v);
            if !ok {
                order.remove(stamp);
            }
            ok
        });
    }
    pub fn values(&self) -> impl Iterator<Item = &V> {
        self.map.values().map(|(_, v)| v)
    }
}

/// Tracks up to 4 active GPU device generations, retiring the oldest.
#[derive(Debug, Default)]
pub(super) struct DeviceGenerations(Vec<DeviceGeneration>);

impl DeviceGenerations {
    pub fn touch(&mut self, generation: DeviceGeneration) -> Option<DeviceGeneration> {
        if self.0.contains(&generation) {
            return None;
        }
        self.0.push(generation);
        (self.0.len() > 4).then(|| self.0.remove(0))
    }
    pub fn contains(&self, generation: DeviceGeneration) -> bool {
        self.0.contains(&generation)
    }
}

/// Shared persistent 2-worker compilation / rasterization pool.
/// Replaces per-compile 16 MiB OS thread spawns across `shader_effects` and `text_light`.
pub(super) struct CompilerPool {
    queue: Mutex<VecDeque<Box<dyn FnOnce() + Send>>>,
    wake: Condvar,
    active: AtomicUsize,
}

static COMPILER_POOL: LazyLock<Arc<CompilerPool>> = LazyLock::new(|| CompilerPool::start(2));

pub(super) fn compiler_pool() -> &'static Arc<CompilerPool> {
    &COMPILER_POOL
}

impl CompilerPool {
    fn start(workers: usize) -> Arc<Self> {
        let pool = Arc::new(Self {
            queue: Mutex::new(VecDeque::new()),
            wake: Condvar::new(),
            active: AtomicUsize::new(0),
        });
        for i in 0..workers {
            let shared = Arc::clone(&pool);
            let spawned = std::thread::Builder::new()
                .name(format!("nanatype-gpu-worker-{i}"))
                .stack_size(16 << 20)
                .spawn(move || {
                    loop {
                        let task = {
                            let mut queue = shared.queue.lock().unwrap_or_else(|e| e.into_inner());
                            loop {
                                if let Some(task) = queue.pop_front() {
                                    break task;
                                }
                                queue = shared.wake.wait(queue).unwrap_or_else(|e| e.into_inner());
                            }
                        };
                        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(task));
                    }
                });
            if let Err(err) = spawned {
                nanatype_diag::report!(crate::diag::WORKER_UNAVAILABLE; "gpu-worker-{i}: {err}");
            }
        }
        pool
    }

    pub fn active_slots(&self) -> usize {
        self.active.load(Ordering::Relaxed)
    }

    /// Try to acquire one of the 2 bounded slots (non-blocking).
    /// Returns `false` if both slots are busy.
    pub fn try_submit(&self, task: impl FnOnce() + Send + 'static) -> bool {
        loop {
            let cur = self.active.load(Ordering::Relaxed);
            if cur >= 2 {
                return false;
            }
            if self
                .active
                .compare_exchange(cur, cur + 1, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
        }
        let active_ref = Arc::clone(&COMPILER_POOL);
        let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        queue.push_back(Box::new(move || {
            struct Slot<'a>(&'a AtomicUsize);
            impl Drop for Slot<'_> {
                fn drop(&mut self) {
                    self.0.fetch_sub(1, Ordering::Relaxed);
                }
            }
            let _slot = Slot(&active_ref.active);
            task();
        }));
        self.wake.notify_one();
        true
    }

    /// Submit background GPU/CPU work without slot refusal.
    pub fn submit(&self, task: impl FnOnce() + Send + 'static) {
        let mut queue = self.queue.lock().unwrap_or_else(|e| e.into_inner());
        queue.push_back(Box::new(task));
        self.wake.notify_one();
    }
}

type PlacementKey = (DeviceGeneration, u64, [u32; 2]);
type PlacementEntry = (f32, [f32; 2], [f32; 2]);

/// Shared glyph-layout capture & viewport placement cache for `text_light` and `text_scramble`.
#[derive(Debug)]
pub(super) struct PlacementCache {
    entries: LruMap<PlacementKey, PlacementEntry>,
}

impl Default for PlacementCache {
    fn default() -> Self {
        Self {
            entries: LruMap::new(128),
        }
    }
}

impl PlacementCache {
    pub fn record(&mut self, id: u64, c: &SceneGpuPrepareContext<'_>) {
        self.entries.insert(
            (c.gpu.generation(), id, c.dest_size),
            (
                c.scale_factor,
                [c.bounds.x, c.bounds.y],
                [c.bounds.width, c.bounds.height],
            ),
        );
    }

    pub fn get(
        &mut self,
        id: u64,
        c: &SceneGpuPassContext<'_>,
        fallback_dpi: f32,
    ) -> (f32, [f32; 2], [f32; 2]) {
        self.entries
            .get(&(c.gpu.generation(), id, c.dest_size))
            .copied()
            .unwrap_or((
                fallback_dpi,
                [
                    c.bounds.x as f32 / fallback_dpi,
                    c.bounds.y as f32 / fallback_dpi,
                ],
                [
                    c.bounds.width as f32 / fallback_dpi,
                    c.bounds.height as f32 / fallback_dpi,
                ],
            ))
    }

    pub fn retain_live<T>(&mut self, registry: &Registry<T>) {
        self.entries.retain(|(_, id, _), _| registry.contains(*id));
    }
}

/// Computes `(scale, crop)` for a retained text overlay in `draw_in_pass`.
pub(super) fn overlay_scale_and_crop(
    c: &SceneGpuPassContext<'_>,
    dpi: f32,
    position: [f32; 2],
    extent: [f32; 2],
    logical_size: [f32; 2],
) -> Option<(f32, [f32; 2])> {
    let scale = extent[0] * dpi / logical_size[0].max(1.);
    if !scale.is_finite() || scale <= 0. {
        return None;
    }
    let crop = [
        (c.bounds.x as f32 - position[0] * dpi) / scale,
        (c.bounds.y as f32 - position[1] * dpi) / scale,
    ];
    Some((scale, crop))
}

/// Snapshot the shaped text layout and relative `[origin, size]` against the overlay layer.
pub(super) fn capture_text_bounds(
    world: &UiWorld,
    text_node: NodeRef,
    layer: StableNodeId,
) -> Option<(Arc<nana_text::TextLayout>, [f32; 2], [f32; 2])> {
    let text = text_node.get_untracked()?;
    let (_, layout) = world.text_layout(text)?;
    let (tb, lb) = (world.layout_box(text)?, world.layout_box(layer)?);
    Some((
        layout.clone(),
        [tb.x - lb.x, tb.y - lb.y],
        [lb.width, lb.height],
    ))
}

/// Wrap a text node and an effect overlay layer in a shared responsive container.
pub(super) fn wrap_text_overlay(
    ui: super::Stage,
    text: AnyView,
    objects: Option<AnyView>,
    layer: AnyView,
) -> AnyView {
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

/// Apply standard absolute overlay layout (100% + `pad` on each side).
pub(super) fn style_overlay_gpu(base: &mut nana_ui::runtime::GpuView, pad: f32) {
    let l = Arc::make_mut(&mut base.style.layout);
    l.position = PositionSpec::Absolute;
    l.offset_left = Some(LengthSpec::Px(-pad));
    l.offset_top = Some(LengthSpec::Px(-pad));
    l.width = Some(super::surface::calc(100., pad * 2.));
    l.height = Some(super::surface::calc(100., pad * 2.));
    l.pointer_events = Some(PointerEventsSpec::None);
}

/// Parse `"<prefix>:<id>"` resource string into `u64`.
pub(super) fn parse_resource_id(resource: &str, prefix: &str) -> Option<u64> {
    resource
        .strip_prefix(prefix)?
        .strip_prefix(':')?
        .parse()
        .ok()
}

/// Wire `SizeChanged` + `bind` onto an overlay component view.
pub(super) fn bind_overlay_layer<V>(
    view: V,
    size: Signal<[u32; 2]>,
    bind: impl Fn(&mut V) + Send + 'static,
) -> AnyView
where
    V: nana_ui::runtime::ComponentView + Clone + PartialEq + 'static,
{
    widget(view)
        .on(move |e: &SizeChanged| {
            size.set_if_changed([e.width.to_bits(), e.height.to_bits()]);
        })
        .bind(bind)
        .into_any()
}

/// Shared WGSL fullscreen quad vertex helper (6-vertex triangle pair).
pub(super) const FULLSCREEN_QUAD_WGSL: &str = r#"
fn fullscreen_quad_uv(i: u32) -> vec2<f32> {
    let points = array<vec2<f32>, 6>(
        vec2(0., 0.), vec2(1., 0.), vec2(0., 1.),
        vec2(0., 1.), vec2(1., 0.), vec2(1., 1.)
    );
    return points[i];
}
"#;

/// An sRGB channel in linear light.
pub(super) fn linear(v: u8) -> f32 {
    let v = f32::from(v) / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// Collects a glyph outline as a tiny-skia path.
pub(super) struct Outline(pub tiny_skia::PathBuilder);
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
