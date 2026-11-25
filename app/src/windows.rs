//! The windows and how settings map onto them.
//!
//! Each frame of state changes computes the *desired* native state of every
//! window from the settings and the skin, then [`WindowManager::reconcile`]
//! issues only the commands that differ from what was last applied.

use std::collections::HashMap;

use nana_ui::{MaterialEffect, WindowDescriptor, WindowHandle};
use nana_ui_platform::host::WindowCommand;
use nana_ui_platform::{WindowId, WindowRole};
use nanatype_frame::canvas::{self, Canvas};
pub use nanatype_frame::shell::tint_caption;
use nanatype_settings::{
    GeometryStore, InputPresence, Lock, MonitorInfo, OverlaySettings, Placement, Settings,
};
use nanatype_theme::Skin;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum WindowKind {
    Input,
    Display,
    History,
    /// Viewer danmaku & events log shown when `history.chat.separate` is on.
    Danmaku,
    Settings,
    /// The summon hotkey's one-line plain-text bar.
    Quick,
}

/// Height of the input window's toolbar.
pub const TOOLBAR_H: f32 = 48.0;
pub const INPUT_SIZE: (f32, f32) = (960.0, 480.0);
pub const INPUT_ACTIONS_H: f32 = 56.0;
pub const INPUT_MIN: (f64, f64) = (720.0, 360.0);
/// The quick input bar: one line, no chrome.
pub const QUICK_SIZE: (f32, f32) = (640.0, 56.0);
/// Smallest a broadcast window may be dragged to.
pub const OVERLAY_MIN: (f64, f64) = (160.0, 96.0);

/// Static metadata describing a [`WindowKind`] in one table.
#[derive(Debug, Clone, Copy)]
pub struct WindowMeta {
    pub id: WindowId,
    pub tag: &'static str,
    pub key: &'static str,
    pub title_key: &'static str,
    pub diag_code: u64,
    pub canvas: Option<Canvas>,
}

const INPUT_META: WindowMeta = WindowMeta {
    id: WindowId::PRIMARY,
    tag: "nt.input",
    key: "input",
    title_key: "window-input",
    diag_code: 0,
    canvas: None,
};
const DISPLAY_META: WindowMeta = WindowMeta {
    id: WindowId(2),
    tag: "nt.display",
    key: "display",
    title_key: "window-display",
    diag_code: 1,
    canvas: Some(Canvas::Display),
};
const HISTORY_META: WindowMeta = WindowMeta {
    id: WindowId(3),
    tag: "nt.history",
    key: "history",
    title_key: "window-history",
    diag_code: 3,
    canvas: Some(Canvas::History),
};
const DANMAKU_META: WindowMeta = WindowMeta {
    id: WindowId(14),
    tag: "nt.danmaku",
    key: "danmaku",
    title_key: "window-danmaku",
    diag_code: 12,
    canvas: Some(Canvas::Danmaku),
};
const SETTINGS_META: WindowMeta = WindowMeta {
    id: WindowId(4),
    tag: "nt.settings",
    key: "settings",
    title_key: "window-settings",
    diag_code: 4,
    canvas: None,
};
const QUICK_META: WindowMeta = WindowMeta {
    id: WindowId(7),
    tag: "nt.quick",
    key: "quick",
    title_key: "window-quick",
    diag_code: 5,
    canvas: None,
};

impl WindowKind {
    pub const OVERLAYS: [Self; 4] = [Self::Input, Self::Display, Self::History, Self::Danmaku];

    pub const fn meta(self) -> WindowMeta {
        match self {
            Self::Input => INPUT_META,
            Self::Display => DISPLAY_META,
            Self::History => HISTORY_META,
            Self::Danmaku => DANMAKU_META,
            Self::Settings => SETTINGS_META,
            Self::Quick => QUICK_META,
        }
    }

    pub const fn id(self) -> WindowId {
        self.meta().id
    }

    /// Windows the app manages, in reconcile order.
    pub const ALL: [Self; 6] = [
        Self::Input,
        Self::Display,
        Self::History,
        Self::Danmaku,
        Self::Settings,
        Self::Quick,
    ];

    pub fn from_id(id: WindowId) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.id() == id)
    }

    pub const fn tag(self) -> &'static str {
        self.meta().tag
    }

    pub const fn key(self) -> &'static str {
        self.meta().key
    }

    pub const fn title_key(self) -> &'static str {
        self.meta().title_key
    }

    pub const fn diag_code(self) -> u64 {
        self.meta().diag_code
    }

    pub fn title(self, locale: nanatype_i18n::Locale) -> String {
        crate::ui::I18N.t_in(locale, self.title_key())
    }

    /// The canvas this window shows, for the broadcast windows the skin draws.
    pub const fn canvas(self) -> Option<Canvas> {
        self.meta().canvas
    }

    pub fn overlay(self, s: &Settings) -> Option<&OverlaySettings> {
        match self {
            Self::Input => Some(&s.overlays.input),
            Self::Settings | Self::Quick => None,
            _ => self.canvas().map(|c| c.overlay(s)),
        }
    }

    pub fn overlay_mut(self, s: &mut Settings) -> Option<&mut OverlaySettings> {
        match self {
            Self::Input => Some(&mut s.overlays.input),
            Self::Display => Some(&mut s.overlays.display),
            Self::History => Some(&mut s.overlays.history),
            Self::Danmaku => Some(&mut s.overlays.danmaku),
            Self::Settings | Self::Quick => None,
        }
    }

    pub fn saved_size(self, s: &Settings) -> Option<[f32; 2]> {
        match (self, s.skin.personalizations.get(&s.skin.id)) {
            (Self::Display, Some(p)) => p.display.size,
            (Self::History, Some(p)) => p.history.size,
            _ => self.overlay(s).and_then(|o| o.size),
        }
    }

    /// Dragging a personalized window must not change the legacy baseline of other skins.
    pub(crate) fn remember_size(
        self,
        s: &mut Settings,
        size: [f32; 2],
        legacy: Option<nanatype_settings::SkinPersonalization>,
    ) {
        if matches!(self, Self::Display | Self::History) {
            if let Some(legacy) = legacy {
                s.skin
                    .personalizations
                    .entry(s.skin.id.clone())
                    .or_insert(legacy);
            }
            if let Some(p) = s.skin.personalizations.get_mut(&s.skin.id) {
                match self {
                    Self::Display => p.display.size = Some(size),
                    _ => p.history.size = Some(size),
                }
                return;
            }
        }
        if let Some(o) = self.overlay_mut(s) {
            o.size = Some(size);
        }
    }

    pub(crate) fn clear_saved_size(self, s: &mut Settings) {
        if matches!(self, Self::Display | Self::History)
            && let Some(p) = s.skin.personalizations.get_mut(&s.skin.id)
        {
            match self {
                Self::Display => p.display.size = None,
                _ => p.history.size = None,
            }
            return;
        }
        if let Some(o) = self.overlay_mut(s) {
            o.size = None;
        }
    }

    pub fn material(self) -> MaterialEffect {
        match self {
            Self::Input | Self::Settings => MaterialEffect::Solid,
            Self::Quick if debug_opaque() => MaterialEffect::Solid,
            Self::Quick => MaterialEffect::Transparent,
            _ if debug_opaque() => MaterialEffect::Solid,
            _ => MaterialEffect::Transparent,
        }
    }
}

/// Development aid: `NANATYPE_OPAQUE=1` paints overlays on a solid
/// background so a screen grab of their rectangle shows only NanaType.
pub fn debug_opaque() -> bool {
    cfg!(debug_assertions) && std::env::var_os("NANATYPE_OPAQUE").is_some()
}

/// The settings window's size as designed, and the smallest it may be.
const SETTINGS_SIZE: (f32, f32) = (920.0, 660.0);
const SETTINGS_MIN: (f64, f64) = (720.0, 480.0);

/// How much `kind` zooms its whole content: the UI scale for the app's own
/// windows (`system_text` is the system's text size, where it has one),
/// none for the broadcast windows, whose picture is the skin's.
pub fn zoom(kind: WindowKind, s: &Settings, system_text: Option<f32>) -> f32 {
    match kind.canvas() {
        Some(_) => 1.0,
        None => s.accessibility.zoom(system_text),
    }
}

/// The smallest the app's own windows may be at `zoom`: as designed, zoomed,
/// so their content is never laid out smaller than it was made for.
/// Broadcast windows have no minimum here.
pub fn minimum_size(kind: WindowKind, zoom: f32) -> Option<(f64, f64)> {
    let base = match kind {
        WindowKind::Input => INPUT_MIN,
        WindowKind::Settings => SETTINGS_MIN,
        WindowKind::Quick => (f64::from(QUICK_SIZE.0), f64::from(QUICK_SIZE.1)),
        _ => return None,
    };
    let zoom = f64::from(zoom);
    Some(((base.0 * zoom).round(), (base.1 * zoom).round()))
}

/// The size an app's own window opens at, at `zoom`; `None` for broadcast
/// windows.
pub fn own_size(kind: WindowKind, zoom: f32) -> Option<(f32, f32)> {
    let size = match kind {
        WindowKind::Input => INPUT_SIZE,
        WindowKind::Settings => SETTINGS_SIZE,
        WindowKind::Quick => QUICK_SIZE,
        _ => return None,
    };
    let (w, h) = minimum_size(kind, zoom)?;
    Some((size.0.max(w as f32), size.1.max(h as f32)))
}

/// Logical size of a window for the current skin and settings.
pub fn size_of(kind: WindowKind, skin: &Skin, s: &Settings) -> (f32, f32) {
    own_size(kind, zoom(kind, s, None))
        .unwrap_or_else(|| canvas::size_of(kind.canvas().expect("broadcast window"), skin, s))
}

/// The app icon for window title bars and the taskbar.
pub fn icon() -> Option<nana_ui_platform::WindowIcon> {
    static ICON: std::sync::OnceLock<Option<(Vec<u8>, u32, u32)>> = std::sync::OnceLock::new();
    let (rgba, w, h) = ICON
        .get_or_init(|| {
            let img = image::load_from_memory(include_bytes!("../assets/tray-64.png"))
                .ok()?
                .into_rgba8();
            let (w, h) = img.dimensions();
            Some((img.into_raw(), w, h))
        })
        .clone()?;
    nana_ui_platform::WindowIcon::from_rgba(rgba, w, h).ok()
}

pub fn descriptor(kind: WindowKind, title: &str, skin: &Skin, s: &Settings) -> WindowDescriptor {
    let (w, h) = size_of(kind, skin, s);
    let mut d = WindowDescriptor::new(title).initial_size(f64::from(w), f64::from(h));
    d.tag = Some(kind.tag().into());
    d.icon = icon();
    match kind {
        WindowKind::Input => {
            d.minimum_size = minimum_size(kind, zoom(kind, s, None)).unwrap_or(INPUT_MIN);
            d.system_caption = true;
            d.resizable = true;
            d.focus_on_show = true;
            d.always_on_top = s.overlays.input.always_on_top;
            d.constrain_to_work_area = true;
            d.role = WindowRole::Main;
            // Restore the frame before showing it, like the chat panel.
            d.visible = false;
        }
        WindowKind::Settings => {
            d.minimum_size = minimum_size(kind, zoom(kind, s, None)).unwrap_or(SETTINGS_MIN);
            // A large UI scale can ask for more than a small screen has.
            d.constrain_to_work_area = true;
            d.system_caption = true;
            d.resizable = true;
            d.visible = true;
            d.focus_on_show = true;
            d.role = WindowRole::Main;
        }
        WindowKind::Quick => {
            d.system_caption = false;
            d.resizable = false;
            d.minimum_size = (f64::from(w), f64::from(h));
            d.skip_taskbar = true;
            d.always_on_top = true;
            d.focus_on_show = true;
            d.role = WindowRole::Tool;
            // Centred before it is shown, on every summon.
            d.visible = false;
            d.transparent = !debug_opaque();
            // Transparent windows otherwise get a dark companion shadow.
            d.shadow = nana_ui::WindowShadow::None;
        }
        _ => {
            d.transparent = !debug_opaque();
            d.shadow = nana_ui::WindowShadow::None;
            d.always_on_top = kind.overlay(s).is_some_and(|o| o.always_on_top);
            d.skip_taskbar = true;
            d.role = WindowRole::Tool;
            d.focus_on_show = kind == WindowKind::Input;
            d.constrain_to_work_area = true;
            d.resizable = kind.overlay(s).is_some_and(|o| o.lock != Lock::Locked);
            d.system_caption = false;
            // The descriptor default (760×520) is clamped down to whatever
            // size the window opens at, which freezes a broadcast window
            // there. A small floor still stops a resize from collapsing it.
            if matches!(
                kind,
                WindowKind::Display | WindowKind::History | WindowKind::Danmaku
            ) {
                d.minimum_size = OVERLAY_MIN;
            }
            // Shown once placed, so nothing flashes at the default origin.
            d.visible = false;
        }
    }
    d
}

/// Quick input bar visibility, focus request, and native handle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuickBarState {
    Hidden { hwnd: isize },
    Visible { focus: bool, hwnd: isize },
}

impl Default for QuickBarState {
    fn default() -> Self {
        Self::Hidden { hwnd: 0 }
    }
}

impl QuickBarState {
    pub fn is_visible(self) -> bool {
        matches!(self, Self::Visible { .. })
    }

    pub fn hwnd(self) -> isize {
        match self {
            Self::Hidden { hwnd } | Self::Visible { hwnd, .. } => hwnd,
        }
    }

    pub fn set_hwnd(&mut self, next: isize) {
        match self {
            Self::Hidden { hwnd } | Self::Visible { hwnd, .. } => *hwnd = next,
        }
    }

    pub fn show(&mut self) {
        *self = Self::Visible {
            focus: true,
            hwnd: self.hwnd(),
        };
    }

    pub fn hide(&mut self) {
        *self = Self::Hidden { hwnd: self.hwnd() };
    }

    pub fn take_focus(&mut self) -> bool {
        if let Self::Visible { focus, .. } = self
            && *focus
        {
            *focus = false;
            true
        } else {
            false
        }
    }

    pub fn closed(&mut self) {
        *self = Self::Hidden { hwnd: 0 };
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Desired {
    pub visible: bool,
    pub passthrough: bool,
    pub on_top: bool,
    pub resizable: bool,
    pub size: (f32, f32),
}

/// Declarative per-window target state for [`WindowManager::reconcile`].
pub struct WindowSpec {
    pub open: bool,
    pub focus: bool,
    pub desired: Desired,
}

/// Apply an explicit show/hide action while preserving summon-mode preference.
pub(crate) fn set_input_visibility(s: &mut Settings, summoned: &mut bool, visible: bool) {
    *summoned = visible;
    if visible || s.general.input_presence == InputPresence::Resident {
        s.overlays.input.visible = visible;
    }
}

pub fn desired(kind: WindowKind, skin: &Skin, s: &Settings, input_summoned: bool) -> Desired {
    let o = kind.overlay(s);
    let visible = match kind {
        WindowKind::Input => {
            s.overlays.input.visible
                && (s.general.input_presence == InputPresence::Resident || input_summoned)
        }
        WindowKind::Settings => true,
        // Shown by [`QuickBarState`].
        WindowKind::Quick => false,
        _ => o.is_some_and(|o| o.visible),
    };
    let lock = o.map(|o| o.lock).unwrap_or(Lock::Interactive);
    Desired {
        visible,
        passthrough: kind != WindowKind::Input && lock == Lock::Locked,
        on_top: kind == WindowKind::Quick || o.is_some_and(|o| o.always_on_top),
        resizable: matches!(kind, WindowKind::Input | WindowKind::Settings)
            || matches!(
                kind,
                WindowKind::History | WindowKind::Danmaku | WindowKind::Display
            ) && lock != Lock::Locked,
        size: size_of(kind, skin, s),
    }
}

/// Whether an already-open window should be forced back to `desired`'s size.
/// Ordinary windows and unlocked broadcast windows keep the user's size.
pub fn pins_size(kind: WindowKind, resizable: bool) -> bool {
    match kind {
        WindowKind::Input | WindowKind::Settings => false,
        WindowKind::Display | WindowKind::History | WindowKind::Danmaku if resizable => false,
        _ => true,
    }
}

#[derive(Default)]
struct Slot {
    /// An open was asked for and the window is not ready yet.
    opening: bool,
    handle: Option<WindowHandle>,
    applied: Option<Desired>,
    /// Logical top-left and size last reported by the window.
    frame: Option<Placement>,
    placed: bool,
}

#[derive(Default)]
pub struct WindowManager {
    slots: HashMap<WindowKind, Slot>,
    pub monitors: Vec<MonitorInfo>,
    pub geometry: GeometryStore,
    pub geometry_dirty: bool,
    /// Declarative open/focus intent for windows opened on demand (`Settings`).
    settings_wanted: bool,
    focus_requests: Vec<WindowKind>,
}

impl WindowManager {
    pub fn is_open(&self, kind: WindowKind) -> bool {
        self.slots.get(&kind).is_some_and(|s| s.handle.is_some())
    }

    pub fn request_settings(&mut self) {
        self.settings_wanted = true;
        if self.is_open(WindowKind::Settings) {
            self.request_focus(WindowKind::Settings);
        }
    }

    pub fn close_settings(&mut self) {
        self.settings_wanted = false;
    }

    pub fn request_focus(&mut self, kind: WindowKind) {
        if !self.focus_requests.contains(&kind) {
            self.focus_requests.push(kind);
        }
    }

    pub fn take_focus_requests(&mut self) -> Vec<WindowKind> {
        std::mem::take(&mut self.focus_requests)
    }

    /// Whether to ask for `kind`'s window: it is neither open nor asked for.
    /// Asking once more before it is ready would open it twice.
    pub fn should_open(&mut self, kind: WindowKind) -> bool {
        let slot = self.slots.entry(kind).or_default();
        let open = slot.handle.is_none() && !slot.opening;
        slot.opening = true;
        open
    }

    pub fn handle(&self, kind: WindowKind) -> Option<&WindowHandle> {
        self.slots.get(&kind).and_then(|s| s.handle.as_ref())
    }

    pub fn ready(&mut self, kind: WindowKind, handle: WindowHandle) {
        let slot = self.slots.entry(kind).or_default();
        slot.handle = Some(handle);
        slot.opening = false;
        slot.applied = None;
        slot.placed = false;
    }

    pub fn closed(&mut self, kind: WindowKind) {
        self.slots.remove(&kind);
        if kind == WindowKind::Settings {
            self.settings_wanted = false;
        }
    }

    pub fn frame(&self, kind: WindowKind) -> Option<Placement> {
        self.slots.get(&kind).and_then(|s| s.frame)
    }

    /// Maximized and minimized input bounds must not replace its restored frame.
    pub(crate) fn record_geometry(
        &mut self,
        kind: WindowKind,
        geometry: nana_ui_platform::WindowGeometry,
    ) {
        if kind == WindowKind::Input
            && (geometry.maximized
                || geometry.logical_size.0 <= 0.0
                || geometry.logical_size.1 <= 0.0)
        {
            return;
        }
        self.moved(kind, geometry.logical_position, geometry.logical_size);
    }

    /// The window moved or resized (logical coordinates).
    pub fn moved(&mut self, kind: WindowKind, position: Option<(f32, f32)>, size: (f32, f32)) {
        let slot = self.slots.entry(kind).or_default();
        let prev = slot.frame;
        let (x, y) = position
            .map(|(x, y)| (f64::from(x), f64::from(y)))
            .or_else(|| prev.map(|p| (p.x, p.y)))
            .unwrap_or((0.0, 0.0));
        let frame = Placement {
            x,
            y,
            w: f64::from(size.0),
            h: f64::from(size.1),
        };
        slot.frame = Some(frame);
        if slot.placed
            && prev != Some(frame)
            && !matches!(kind, WindowKind::Settings | WindowKind::Quick)
            && !self.monitors.is_empty()
        {
            self.geometry.remember(&self.monitors, kind.key(), frame);
            self.geometry_dirty = true;
        }
    }

    /// Default spot: input bottom-centre of the primary work area, display
    /// above it, history at the right edge.
    fn default_place(&self, kind: WindowKind, size: (f32, f32)) -> Option<(f32, f32)> {
        let m = self
            .monitors
            .iter()
            .find(|m| m.primary)
            .or_else(|| self.monitors.first())?;
        let [x, y, w, h] = m.work_area;
        let (sw, sh) = (f64::from(size.0), f64::from(size.1));
        let margin = 24.0;
        let (px, py) = match kind {
            WindowKind::Input => (x + (w - sw) / 2.0, y + h - sh - margin),
            WindowKind::Display => (x + (w - sw) / 2.0, y + h * 0.55 - sh / 2.0),
            WindowKind::History => (x + w - sw - margin, y + (h - sh) / 2.0),
            WindowKind::Danmaku => (x + margin, y + (h - sh) / 2.0),
            WindowKind::Settings => (x + (w - sw) / 2.0, y + (h - sh) / 2.0),
            WindowKind::Quick => (x + (w - sw) / 2.0, y + h * 0.25),
        };
        Some((px.max(x) as f32, py.max(y) as f32))
    }

    pub fn forget(&mut self, kind: WindowKind) {
        self.geometry.forget(kind.key());
        self.geometry_dirty = true;
        if let Some(slot) = self.slots.get_mut(&kind) {
            slot.placed = false;
            slot.applied = None;
        }
    }

    /// Take window positions from an imported file: every window is placed
    /// again from them.
    pub fn replace_geometry(&mut self, geometry: GeometryStore) {
        self.geometry = geometry;
        self.geometry_dirty = true;
        for slot in self.slots.values_mut() {
            slot.placed = false;
            slot.applied = None;
        }
    }

    /// Declaratively reconcile every managed window: open missing windows whose
    /// `spec.open` is true (once `WindowKind::Input` is open), apply bounds /
    /// passthrough / topmost / resizable / visibility / focus, and return the
    /// resulting platform commands.
    pub fn reconcile(
        &mut self,
        skin: &Skin,
        settings: &Settings,
        locale: nanatype_i18n::Locale,
        system_text: Option<f32>,
        input_summoned: bool,
        quick_visible: bool,
    ) -> Vec<WindowCommand> {
        let mut out = Vec::new();
        let monitors_known = !self.monitors.is_empty();
        let input_open = self.is_open(WindowKind::Input);
        let focus_requests = self.take_focus_requests();

        for kind in WindowKind::ALL {
            let mut d = desired(kind, skin, settings, input_summoned);
            d.size = own_size(kind, zoom(kind, settings, system_text))
                .unwrap_or_else(|| size_of(kind, skin, settings));
            if kind == WindowKind::Danmaku && !settings.history.chat.separate {
                d.visible = false;
            }
            if kind == WindowKind::Quick {
                d.visible = quick_visible;
            }

            let wants_open = match kind {
                WindowKind::Input => false, // Primary window opens at startup.
                WindowKind::Display | WindowKind::History => input_open,
                WindowKind::Danmaku => input_open && settings.history.chat.separate,
                WindowKind::Quick => {
                    input_open
                        && (settings.general.input_presence == InputPresence::Summon
                            || quick_visible)
                }
                WindowKind::Settings => self.settings_wanted,
            };

            if wants_open && self.should_open(kind) {
                let desc = descriptor(kind, &kind.title(locale), skin, settings);
                out.push(WindowCommand::Open {
                    id: kind.id(),
                    settings: desc,
                });
            }

            let placement = self.geometry.restore(&self.monitors, kind.key());
            let default = self.default_place(kind, d.size);
            let Some(slot) = self.slots.get_mut(&kind) else {
                continue;
            };
            let Some(handle) = slot.handle.clone() else {
                continue;
            };
            let id = kind.id();
            let prev = slot.applied.clone();
            // The quick bar opens centred each time, wherever it was left.
            if kind == WindowKind::Quick && d.visible && !prev.as_ref().is_some_and(|p| p.visible) {
                slot.placed = false;
            }
            if !slot.placed && monitors_known {
                let (pos, size) = match (kind, placement) {
                    // Overlays whose size follows the skin keep that size.
                    (WindowKind::Input, Some(p)) => {
                        let size = (
                            (p.w as f32).max(INPUT_MIN.0 as f32),
                            (p.h as f32).max(INPUT_MIN.1 as f32),
                        );
                        ((p.x as f32, p.y as f32), size)
                    }
                    (_, Some(p)) => ((p.x as f32, p.y as f32), d.size),
                    (_, None) => (default.unwrap_or((100.0, 100.0)), d.size),
                };
                out.push(WindowCommand::SetBounds {
                    id,
                    position: pos,
                    size,
                });
                slot.placed = true;
            } else if prev.as_ref().is_some_and(|p| p.size != d.size)
                && prev.as_ref().is_some_and(|p| pins_size(kind, p.resizable))
                && let Some(f) = slot.frame
            {
                out.push(WindowCommand::SetBounds {
                    id,
                    position: (
                        f.x as f32,
                        if kind == WindowKind::Input {
                            f.y as f32 + f.h as f32 - d.size.1
                        } else {
                            f.y as f32
                        },
                    ),
                    size: d.size,
                });
            }
            if prev.as_ref().map(|p| p.passthrough) != Some(d.passthrough)
                && kind != WindowKind::Settings
            {
                out.push(WindowCommand::SetMousePassthrough {
                    id,
                    enabled: d.passthrough,
                });
            }
            if prev.as_ref().map(|p| p.on_top) != Some(d.on_top) && kind != WindowKind::Settings {
                out.push(WindowCommand::SetAlwaysOnTop {
                    id,
                    always_on_top: d.on_top,
                });
            }
            if prev.as_ref().map(|p| p.resizable) != Some(d.resizable) {
                drop(handle.set_resizable(d.resizable));
            }
            let visible = d.visible && (slot.placed || !monitors_known);
            if prev.as_ref().map(|p| p.visible) != Some(visible) {
                nanatype_diag::event!(
                    crate::diag::WINDOW_VISIBILITY,
                    window = crate::diag::window(kind),
                    visible = visible,
                    placed = slot.placed,
                    monitors_known = monitors_known,
                );
                drop(handle.set_visible(visible));
            }
            if focus_requests.contains(&kind) {
                out.push(WindowCommand::Focus(id));
            }
            slot.applied = Some(Desired { visible, ..d });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn dragging_and_resetting_one_skin_preserve_other_skins_legacy_geometry() {
        let skin = skin();
        let mut settings = Settings::default();
        settings.overlays.display.size = Some([900.0, 200.0]);
        let active = settings.skin.id.clone();
        WindowKind::Display.remember_size(&mut settings, [300.0, 160.0], Some(Default::default()));
        assert_eq!(
            size_of(WindowKind::Display, &skin, &settings),
            (300.0, 160.0)
        );
        assert_eq!(settings.overlays.display.size, Some([900.0, 200.0]));
        settings.skin.id = "other".into();
        assert_eq!(
            size_of(WindowKind::Display, &skin, &settings),
            (900.0, 200.0)
        );
        settings.skin.id = active;
        WindowKind::Display.clear_saved_size(&mut settings);
        assert_eq!(
            settings.skin.personalizations[&settings.skin.id]
                .display
                .size,
            None
        );
        assert_eq!(settings.overlays.display.size, Some([900.0, 200.0]));
    }

    #[test]
    fn saved_sizes_follow_the_selected_skin() {
        let skin = skin();
        let mut s = Settings::default();
        s.overlays.display.size = Some([900.0, 200.0]);
        let mut p = nanatype_settings::SkinPersonalization::default();
        p.display.size = Some([160.0, 96.0]);
        p.history.size = Some([300.0, 500.0]);
        s.skin.personalizations.insert(s.skin.id.clone(), p);
        assert_eq!(size_of(WindowKind::Display, &skin, &s), (160.0, 96.0));
        assert_eq!(size_of(WindowKind::History, &skin, &s), (300.0, 500.0));
        s.skin.id = "other".into();
        assert_eq!(size_of(WindowKind::Display, &skin, &s), (900.0, 200.0));
    }

    use super::*;

    fn skin() -> Skin {
        nanatype_theme::SkinRegistry::new(None, "1.0.0")
            .load("nanatype.halo")
            .unwrap()
            .skin
            .as_ref()
            .clone()
    }

    #[test]
    fn only_the_apps_own_windows_zoom_and_their_minimum_grows_with_it() {
        let mut s = Settings::default();
        s.accessibility.ui_scale = 150;
        for kind in [WindowKind::Input, WindowKind::Settings, WindowKind::Quick] {
            assert_eq!(zoom(kind, &s, None), 1.5);
        }
        for kind in [
            WindowKind::Display,
            WindowKind::History,
            WindowKind::Danmaku,
        ] {
            assert_eq!(zoom(kind, &s, Some(2.0)), 1.0);
            assert_eq!(minimum_size(kind, 1.5), None);
        }
        assert_eq!(minimum_size(WindowKind::Input, 1.5), Some((1080.0, 540.0)));
        assert_eq!(own_size(WindowKind::Settings, 1.0), Some(SETTINGS_SIZE));
        assert_eq!(own_size(WindowKind::Settings, 2.0), Some((1440.0, 960.0)));
        // Following the system takes its text size where it has one.
        s.accessibility.ui_scale_follows_system = true;
        assert_eq!(zoom(WindowKind::Settings, &s, Some(1.25)), 1.25);
        assert_eq!(zoom(WindowKind::Settings, &s, None), 1.5);
    }

    #[test]
    fn input_is_an_ordinary_window_independent_of_the_broadcast_canvas() {
        let skin = skin();
        let s = Settings::default();
        let (dw, _) = size_of(WindowKind::Display, &skin, &s);
        assert_eq!(dw, skin.dialogue.region[0] + skin.dialogue.overscan * 2.0);
        assert_eq!(size_of(WindowKind::Input, &skin, &s), INPUT_SIZE);
        let descriptor = descriptor(WindowKind::Input, "input", &skin, &s);
        assert!(descriptor.system_caption && descriptor.resizable && !descriptor.transparent);
        assert!(!descriptor.skip_taskbar);
        assert_eq!(descriptor.role, WindowRole::Main);
        assert_eq!(descriptor.minimum_size, INPUT_MIN);
        assert_eq!(WindowKind::Input.material(), MaterialEffect::Solid);
        let mut resized = s;
        resized.skin.overrides.region = Some([1400.0, 600.0]);
        assert_eq!(size_of(WindowKind::Input, &skin, &resized), INPUT_SIZE);
        assert!(desired(WindowKind::Input, &skin, &resized, false).resizable);
        let d = desired(WindowKind::Settings, &skin, &resized, false);
        assert!(
            d.resizable
                && super::descriptor(WindowKind::Settings, "settings", &skin, &resized).resizable
        );
        assert!(!pins_size(WindowKind::Settings, d.resizable));
    }

    #[test]
    fn input_geometry_keeps_the_restored_frame_through_maximize_and_minimize() {
        use nana_ui_platform::WindowGeometry;
        use nanatype_settings::MonitorKey;
        let mut manager = WindowManager::default();
        manager.monitors.push(MonitorInfo {
            key: MonitorKey("test".into()),
            work_area: [0.0, 0.0, 1920.0, 1080.0],
            primary: true,
        });
        manager.slots.entry(WindowKind::Input).or_default().placed = true;
        let normal = WindowGeometry {
            logical_position: Some((100.0, 200.0)),
            logical_size: (1000.0, 500.0),
            ..Default::default()
        };
        manager.record_geometry(WindowKind::Input, normal);
        let saved = manager
            .geometry
            .restore(&manager.monitors, "input")
            .unwrap();
        manager.geometry_dirty = false;
        for transient in [
            WindowGeometry {
                maximized: true,
                logical_size: (1920.0, 1080.0),
                ..normal
            },
            WindowGeometry {
                logical_size: (0.0, 0.0),
                ..normal
            },
        ] {
            manager.record_geometry(WindowKind::Input, transient);
            assert_eq!(manager.frame(WindowKind::Input), Some(saved));
            assert_eq!(
                manager.geometry.restore(&manager.monitors, "input"),
                Some(saved)
            );
            assert!(!manager.geometry_dirty);
        }
    }

    #[test]
    fn explicit_input_hide_and_summon_work_in_both_presence_modes() {
        let skin = skin();
        for mode in [InputPresence::Resident, InputPresence::Summon] {
            let mut s = Settings::default();
            s.general.input_presence = mode;
            let mut summoned = true;
            set_input_visibility(&mut s, &mut summoned, false);
            assert!(!desired(WindowKind::Input, &skin, &s, summoned).visible);
            assert_eq!(s.overlays.input.visible, mode == InputPresence::Summon);
            set_input_visibility(&mut s, &mut summoned, true);
            assert!(desired(WindowKind::Input, &skin, &s, summoned).visible);
            assert_eq!(s.general.input_presence, mode);
        }
    }

    #[test]
    fn default_overlays_receive_gestures_and_keep_user_sizes() {
        let skin = skin();
        let s = Settings::default();
        assert!(desired(WindowKind::Input, &skin, &s, false).visible);
        for kind in [
            WindowKind::Display,
            WindowKind::History,
            WindowKind::Danmaku,
        ] {
            assert_eq!(kind.overlay(&s).unwrap().lock, Lock::Interactive);
            let d = desired(kind, &skin, &s, false);
            assert!(!d.visible);
            assert!(!d.passthrough);
            assert!(d.resizable);
            assert!(!pins_size(kind, d.resizable));
            assert!(descriptor(kind, kind.key(), &skin, &s).resizable);
        }
    }

    #[test]
    fn lock_modes_map_to_passthrough() {
        let skin = skin();
        let mut s = Settings::default();
        s.overlays.display.lock = Lock::Locked;
        assert!(desired(WindowKind::Display, &skin, &s, false).passthrough);
        s.overlays.display.lock = Lock::Interactive;
        let d = desired(WindowKind::Display, &skin, &s, false);
        assert!(!d.passthrough);
        assert!(d.resizable);
        assert!(!desired(WindowKind::Input, &skin, &s, false).passthrough);
    }

    #[test]
    fn summon_presence_hides_the_input_until_summoned() {
        let skin = skin();
        let mut s = Settings::default();
        s.general.input_presence = InputPresence::Summon;
        assert!(!desired(WindowKind::Input, &skin, &s, false).visible);
        assert!(desired(WindowKind::Input, &skin, &s, true).visible);
    }

    #[test]
    fn the_quick_bar_is_a_frameless_topmost_line_the_app_shows() {
        let skin = skin();
        for mode in [InputPresence::Resident, InputPresence::Summon] {
            let mut s = Settings::default();
            s.general.input_presence = mode;
            let d = descriptor(WindowKind::Quick, "quick", &skin, &s);
            assert!(!d.system_caption && !d.resizable && !d.visible);
            assert_eq!(d.transparent, !debug_opaque());
            assert_eq!(d.shadow, nana_ui::WindowShadow::None);
            assert_eq!(
                WindowKind::Quick.material(),
                if debug_opaque() {
                    MaterialEffect::Solid
                } else {
                    MaterialEffect::Transparent
                }
            );
            assert!(d.skip_taskbar && d.always_on_top && d.focus_on_show);
            assert_eq!(d.role, WindowRole::Tool);
            let want = desired(WindowKind::Quick, &skin, &s, true);
            assert!(!want.visible && want.on_top && !want.resizable && !want.passthrough);
            assert_eq!(want.size, QUICK_SIZE);
            assert!(pins_size(WindowKind::Quick, want.resizable));
        }
        assert!(!WindowKind::OVERLAYS.contains(&WindowKind::Quick));
        assert_eq!(
            WindowKind::from_id(WindowKind::Quick.id()),
            Some(WindowKind::Quick)
        );
    }

    #[test]
    fn the_quick_bar_opens_centred_and_is_never_remembered() {
        use nanatype_settings::MonitorKey;
        let mut manager = WindowManager::default();
        manager.monitors.push(MonitorInfo {
            key: MonitorKey("test".into()),
            work_area: [0.0, 0.0, 1920.0, 1080.0],
            primary: true,
        });
        let (x, y) = manager
            .default_place(WindowKind::Quick, QUICK_SIZE)
            .unwrap();
        assert_eq!(x + QUICK_SIZE.0 / 2.0, 960.0);
        assert!(y < 1080.0 / 2.0);
        manager.slots.entry(WindowKind::Quick).or_default().placed = true;
        manager.moved(WindowKind::Quick, Some((10.0, 10.0)), QUICK_SIZE);
        assert!(!manager.geometry_dirty);
        assert_eq!(manager.geometry.restore(&manager.monitors, "quick"), None);
    }

    #[test]
    fn a_size_override_wins_over_the_skin() {
        let skin = skin();
        let mut s = Settings::default();
        s.overlays.history.size = Some([300.0, 900.0]);
        assert_eq!(size_of(WindowKind::History, &skin, &s), (300.0, 900.0));
    }

    #[test]
    fn every_window_has_its_own_id_tag_and_key() {
        let mut ids = std::collections::HashSet::new();
        let mut tags = std::collections::HashSet::new();
        let mut keys = std::collections::HashSet::new();
        for kind in WindowKind::ALL {
            assert!(ids.insert(kind.id()), "{kind:?}");
            assert!(tags.insert(kind.tag()), "{kind:?}");
            assert!(keys.insert(kind.key()), "{kind:?}");
            assert_eq!(WindowKind::from_id(kind.id()), Some(kind));
        }
    }

    #[test]
    fn a_live_resize_is_not_overwritten() {
        assert!(!pins_size(WindowKind::Display, true));
        assert!(!pins_size(WindowKind::History, true));
        assert!(!pins_size(WindowKind::Danmaku, true));
        assert!(pins_size(WindowKind::Display, false));
        assert!(pins_size(WindowKind::History, false));
        assert!(pins_size(WindowKind::Danmaku, false));
        assert!(!pins_size(WindowKind::Input, false));
        assert!(!pins_size(WindowKind::Input, true));
    }

    #[test]
    fn broadcast_windows_can_shrink_below_the_descriptor_default() {
        let skin = skin();
        let mut s = Settings::default();
        s.overlays.danmaku.size = Some([320.0, 540.0]);
        assert_eq!(size_of(WindowKind::Danmaku, &skin, &s), (320.0, 540.0));
        for kind in [
            WindowKind::Display,
            WindowKind::History,
            WindowKind::Danmaku,
        ] {
            assert_eq!(
                descriptor(kind, kind.key(), &skin, &s).minimum_size,
                OVERLAY_MIN
            );
        }
    }
}
