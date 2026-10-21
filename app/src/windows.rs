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
    PeerDisplay,
    History,
    Settings,
    /// The live chat panel: an ordinary window for the streamer, never
    /// captured.
    Chat,
}

/// Height of the input window's toolbar.
pub const TOOLBAR_H: f32 = 48.0;
/// Height of the input window's plain-text / skin-preview switch row.
pub const INPUT_MODE_H: f32 = 36.0;
pub const INPUT_SIZE: (f32, f32) = (1200.0, 640.0);
pub const INPUT_ACTIONS_H: f32 = 56.0;
pub const INPUT_MIN: (f64, f64) = (720.0, 360.0);
/// Smallest a broadcast window may be dragged to.
pub const OVERLAY_MIN: (f64, f64) = (160.0, 96.0);

impl WindowKind {
    pub const OVERLAYS: [Self; 4] = [Self::Input, Self::Display, Self::PeerDisplay, Self::History];

    pub fn id(self) -> WindowId {
        match self {
            Self::Input => WindowId::PRIMARY,
            Self::Display => WindowId(2),
            Self::PeerDisplay => WindowId(6),
            Self::History => WindowId(3),
            Self::Settings => WindowId(4),
            Self::Chat => WindowId(5),
        }
    }

    /// Windows the app manages, in reconcile order.
    pub const ALL: [Self; 6] = [
        Self::Input,
        Self::Display,
        Self::PeerDisplay,
        Self::History,
        Self::Settings,
        Self::Chat,
    ];

    pub fn from_id(id: WindowId) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.id() == id)
    }

    pub fn tag(self) -> &'static str {
        match self {
            Self::Input => "nt.input",
            Self::Display => "nt.display",
            Self::PeerDisplay => "nt.peer-display",
            Self::History => "nt.history",
            Self::Settings => "nt.settings",
            Self::Chat => "nt.chat",
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Display => "display",
            Self::PeerDisplay => "peer_display",
            Self::History => "history",
            Self::Settings => "settings",
            Self::Chat => "chat",
        }
    }

    pub fn title_key(self) -> &'static str {
        match self {
            Self::Input => "window-input",
            Self::Display => "window-display",
            Self::PeerDisplay => "window-peer-display",
            Self::History => "window-history",
            Self::Settings => "window-settings",
            Self::Chat => "window-chat",
        }
    }

    /// The canvas this window shows, for the broadcast windows the skin draws.
    pub fn canvas(self) -> Option<Canvas> {
        match self {
            Self::Display => Some(Canvas::Display),
            Self::PeerDisplay => Some(Canvas::PeerDisplay),
            Self::History => Some(Canvas::History),
            Self::Input | Self::Settings | Self::Chat => None,
        }
    }

    pub fn overlay(self, s: &Settings) -> Option<&OverlaySettings> {
        match self {
            Self::Input => Some(&s.overlays.input),
            Self::Settings | Self::Chat => None,
            _ => self.canvas().map(|c| c.overlay(s)),
        }
    }

    pub fn overlay_mut(self, s: &mut Settings) -> Option<&mut OverlaySettings> {
        match self {
            Self::Input => Some(&mut s.overlays.input),
            Self::Display => Some(&mut s.overlays.display),
            Self::PeerDisplay => Some(&mut s.overlays.peer_display),
            Self::History => Some(&mut s.overlays.history),
            Self::Settings | Self::Chat => None,
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
            Self::Input | Self::Settings | Self::Chat => MaterialEffect::Solid,
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

/// Logical size of a window for the current skin and settings.
pub fn size_of(kind: WindowKind, skin: &Skin, s: &Settings) -> (f32, f32) {
    match kind {
        WindowKind::Input => INPUT_SIZE,
        WindowKind::Settings => (920.0, 660.0),
        WindowKind::Chat => (400.0, 640.0),
        _ => canvas::size_of(kind.canvas().expect("broadcast window"), skin, s),
    }
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
            d.minimum_size = INPUT_MIN;
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
            d.minimum_size = (720.0, 480.0);
            d.system_caption = true;
            d.resizable = true;
            d.visible = true;
            d.focus_on_show = true;
            d.role = WindowRole::Main;
        }
        WindowKind::Chat => {
            d.minimum_size = (300.0, 320.0);
            d.system_caption = true;
            d.resizable = true;
            d.focus_on_show = true;
            d.role = WindowRole::Main;
            // Shown once placed where it was last time.
            d.visible = false;
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
                WindowKind::Display | WindowKind::PeerDisplay | WindowKind::History
            ) {
                d.minimum_size = OVERLAY_MIN;
            }
            // Shown once placed, so nothing flashes at the default origin.
            d.visible = false;
        }
    }
    d
}

#[derive(Debug, Clone, PartialEq)]
pub struct Desired {
    pub visible: bool,
    pub passthrough: bool,
    pub on_top: bool,
    pub resizable: bool,
    pub size: (f32, f32),
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
        WindowKind::Settings | WindowKind::Chat => true,
        _ => o.is_some_and(|o| o.visible),
    };
    let lock = o.map(|o| o.lock).unwrap_or(Lock::Interactive);
    Desired {
        visible,
        passthrough: kind != WindowKind::Input && lock == Lock::Locked,
        on_top: o.is_some_and(|o| o.always_on_top),
        resizable: matches!(kind, WindowKind::Input | WindowKind::Chat)
            || matches!(
                kind,
                WindowKind::History | WindowKind::Display | WindowKind::PeerDisplay
            ) && lock != Lock::Locked,
        size: size_of(kind, skin, s),
    }
}

/// Whether an already-open window should be forced back to `desired`'s size.
/// Ordinary input/chat windows and unlocked broadcast windows keep the user's size.
pub fn pins_size(kind: WindowKind, resizable: bool) -> bool {
    match kind {
        WindowKind::Input | WindowKind::Chat => false,
        WindowKind::Display | WindowKind::PeerDisplay | WindowKind::History if resizable => false,
        _ => true,
    }
}

#[derive(Default)]
struct Slot {
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
}

impl WindowManager {
    pub fn is_open(&self, kind: WindowKind) -> bool {
        self.slots.get(&kind).is_some_and(|s| s.handle.is_some())
    }

    pub fn handle(&self, kind: WindowKind) -> Option<&WindowHandle> {
        self.slots.get(&kind).and_then(|s| s.handle.as_ref())
    }

    pub fn ready(&mut self, kind: WindowKind, handle: WindowHandle) {
        let slot = self.slots.entry(kind).or_default();
        slot.handle = Some(handle);
        slot.applied = None;
        slot.placed = false;
    }

    pub fn closed(&mut self, kind: WindowKind) {
        self.slots.remove(&kind);
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
            && kind != WindowKind::Settings
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
            WindowKind::PeerDisplay => (x + (w - sw) / 2.0, y + h * 0.28 - sh / 2.0),
            WindowKind::History => (x + w - sw - margin, y + (h - sh) / 2.0),
            WindowKind::Settings => (x + (w - sw) / 2.0, y + (h - sh) / 2.0),
            WindowKind::Chat => (x + margin, y + (h - sh) / 2.0),
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

    /// Commands that bring every open window to its desired state.
    pub fn reconcile(&mut self, want: &[(WindowKind, Desired)]) -> Vec<WindowCommand> {
        let mut out = Vec::new();
        let monitors_known = !self.monitors.is_empty();
        for (kind, d) in want {
            let placement = self.geometry.restore(&self.monitors, kind.key());
            let default = self.default_place(*kind, d.size);
            let Some(slot) = self.slots.get_mut(kind) else {
                continue;
            };
            let Some(handle) = slot.handle.clone() else {
                continue;
            };
            let id = kind.id();
            let prev = slot.applied.clone();
            if !slot.placed && monitors_known {
                let (pos, size) = match (*kind, placement) {
                    // Overlays whose size follows the skin keep that size.
                    (WindowKind::Input | WindowKind::Chat, Some(p)) => {
                        let size = if *kind == WindowKind::Input {
                            (
                                (p.w as f32).max(INPUT_MIN.0 as f32),
                                (p.h as f32).max(INPUT_MIN.1 as f32),
                            )
                        } else {
                            (p.w as f32, p.h as f32)
                        };
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
                && prev.as_ref().is_some_and(|p| pins_size(*kind, p.resizable))
                && let Some(f) = slot.frame
            {
                out.push(WindowCommand::SetBounds {
                    id,
                    position: (
                        f.x as f32,
                        if *kind == WindowKind::Input {
                            f.y as f32 + f.h as f32 - d.size.1
                        } else {
                            f.y as f32
                        },
                    ),
                    size: d.size,
                });
            }
            if prev.as_ref().map(|p| p.passthrough) != Some(d.passthrough)
                && *kind != WindowKind::Settings
            {
                out.push(WindowCommand::SetMousePassthrough {
                    id,
                    enabled: d.passthrough,
                });
            }
            if prev.as_ref().map(|p| p.on_top) != Some(d.on_top) && *kind != WindowKind::Settings {
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
                crate::debug_log(&format!(
                    "{kind:?} set_visible({visible}) placed={} monitors={monitors_known}",
                    slot.placed
                ));
                drop(handle.set_visible(visible));
            }
            slot.applied = Some(Desired {
                visible,
                ..d.clone()
            });
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
        WindowKind::PeerDisplay.remember_size(&mut settings, [600.0, 300.0], None);
        assert_eq!(settings.overlays.peer_display.size, Some([600.0, 300.0]));
        assert_eq!(settings.overlays.display.size, Some([900.0, 200.0]));
    }

    #[test]
    fn saved_sizes_follow_the_selected_skin_and_peer_size_stays_independent() {
        let skin = skin();
        let mut s = Settings::default();
        s.overlays.display.size = Some([900.0, 200.0]);
        s.overlays.peer_display.size = Some([700.0, 180.0]);
        let mut p = nanatype_settings::SkinPersonalization::default();
        p.display.size = Some([160.0, 96.0]);
        p.history.size = Some([300.0, 500.0]);
        s.skin.personalizations.insert(s.skin.id.clone(), p);
        assert_eq!(size_of(WindowKind::Display, &skin, &s), (160.0, 96.0));
        assert_eq!(size_of(WindowKind::History, &skin, &s), (300.0, 500.0));
        assert_eq!(size_of(WindowKind::PeerDisplay, &skin, &s), (700.0, 180.0));
        s.overlays.peer_display.size = None;
        s.skin.overrides.region = Some([555.0, 140.0]);
        assert_eq!(
            size_of(WindowKind::PeerDisplay, &skin, &s),
            (
                555.0 + skin.dialogue.overscan * 2.0,
                140.0 + skin.dialogue.overscan * 2.0
            )
        );
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
        for kind in [
            WindowKind::Display,
            WindowKind::PeerDisplay,
            WindowKind::History,
        ] {
            assert_eq!(kind.overlay(&s).unwrap().lock, Lock::Interactive);
            let d = desired(kind, &skin, &s, false);
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
    fn the_chat_panel_is_an_ordinary_window() {
        let skin = skin();
        let d = desired(WindowKind::Chat, &skin, &Settings::default(), false);
        assert!(!d.passthrough);
        assert!(d.resizable && d.visible && !d.on_top);
        assert_eq!(WindowKind::from_id(WindowId(5)), Some(WindowKind::Chat));
    }

    #[test]
    fn a_size_override_wins_over_the_skin() {
        let skin = skin();
        let mut s = Settings::default();
        s.overlays.history.size = Some([300.0, 900.0]);
        assert_eq!(size_of(WindowKind::History, &skin, &s), (300.0, 900.0));
    }

    #[test]
    fn local_and_partner_windows_have_independent_layouts() {
        let skin = skin();
        let mut s = Settings::default();
        s.overlays.display.size = Some([760.0, 200.0]);
        s.overlays.display.lock = Lock::Locked;
        s.overlays.peer_display.size = Some([500.0, 300.0]);
        s.overlays.peer_display.lock = Lock::Interactive;
        s.overlays.peer_display.visible = false;
        let own = desired(WindowKind::Display, &skin, &s, false);
        let peer = desired(WindowKind::PeerDisplay, &skin, &s, false);
        assert_eq!(own.size, (760.0, 200.0));
        assert_eq!(peer.size, (500.0, 300.0));
        assert!(own.visible && !peer.visible);
        assert!(own.passthrough);
        assert!(!peer.passthrough);
        assert!(!own.resizable && peer.resizable);
        assert_ne!(WindowKind::Display.id(), WindowKind::PeerDisplay.id());
        assert_ne!(WindowKind::Display.key(), WindowKind::PeerDisplay.key());
        assert_eq!(
            WindowKind::from_id(WindowKind::PeerDisplay.id()),
            Some(WindowKind::PeerDisplay)
        );
    }

    #[test]
    fn a_live_resize_is_not_overwritten() {
        assert!(!pins_size(WindowKind::Display, true));
        assert!(!pins_size(WindowKind::History, true));
        assert!(pins_size(WindowKind::Display, false));
        assert!(pins_size(WindowKind::History, false));
        assert!(!pins_size(WindowKind::Chat, true));
        assert!(!pins_size(WindowKind::Input, false));
        assert!(!pins_size(WindowKind::Input, true));
    }

    #[test]
    fn broadcast_windows_can_shrink_below_the_descriptor_default() {
        let skin = skin();
        let s = Settings::default();
        for kind in [
            WindowKind::Display,
            WindowKind::PeerDisplay,
            WindowKind::History,
        ] {
            assert_eq!(
                descriptor(kind, kind.key(), &skin, &s).minimum_size,
                OVERLAY_MIN
            );
        }
    }
}
