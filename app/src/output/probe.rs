//! Debug-only observation of the real sender. Installing an observer neither
//! takes a native token nor changes its ready/release protocol.
use crate::windows::WindowKind;
use nana_ui::GpuContext;
use nanalive_spout::{
    GpuDx11PublishReport, GpuDx11SharedSender, SharedTextureFrame, SpoutOutputError,
};
use std::cell::RefCell;

pub type Observer = Box<
    dyn FnMut(
        WindowKind,
        &GpuContext,
        &SharedTextureFrame<'_>,
        &GpuDx11SharedSender,
        &Result<GpuDx11PublishReport, SpoutOutputError>,
        bool,
    ),
>;
thread_local! {
    static LIFECYCLE: RefCell<Option<LifecycleObserver>> = const { RefCell::new(None) };
    static OBSERVER: RefCell<Option<Observer>> = const { RefCell::new(None) };
}
/// Install on the window thread; handles in the callback are borrowed only.
pub fn install(observer: Observer) {
    OBSERVER.with(|slot| *slot.borrow_mut() = Some(observer));
}
pub fn clear() {
    LIFECYCLE.with(|slot| *slot.borrow_mut() = None);
    OBSERVER.with(|slot| *slot.borrow_mut() = None);
}
pub(super) fn observe(
    kind: WindowKind,
    gpu: &GpuContext,
    frame: &SharedTextureFrame<'_>,
    sender: &GpuDx11SharedSender,
    result: &Result<GpuDx11PublishReport, SpoutOutputError>,
    suspended: bool,
) {
    OBSERVER.with(|slot| {
        if let Some(observer) = slot.borrow_mut().as_mut() {
            observer(kind, gpu, frame, sender, result, suspended);
        }
    });
}

/// Lifecycle timings, outside the steady-state publish callback.
pub type LifecycleObserver = Box<dyn FnMut(&'static str, Option<WindowKind>, f64)>;
pub fn install_lifecycle(observer: LifecycleObserver) {
    LIFECYCLE.with(|slot| *slot.borrow_mut() = Some(observer));
}
pub(super) fn lifecycle(event: &'static str, kind: Option<WindowKind>, cpu_ms: f64) {
    LIFECYCLE.with(|slot| {
        if let Some(observer) = slot.borrow_mut().as_mut() {
            observer(event, kind, cpu_ms);
        }
    });
}
