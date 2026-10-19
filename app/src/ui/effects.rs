//! Pieces shared by the custom GPU effects (sweep, text light, text
//! scramble, skin shaders).
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, Weak};

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
        let live: Vec<_> = self.live().values().filter_map(Weak::upgrade).collect();
        live
    }
}

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
