//! Window placement that survives reboots, docking and monitor changes.
//!
//! Positions are stored per *monitor layout* (the set of connected monitors)
//! and relative to a monitor's work area, keyed by a stable monitor identity
//! (device path on Windows, display UUID on macOS) instead of a handle.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub const INPUT_SIDEBAR_DEFAULT: f32 = 320.0;
/// Narrow enough for a small window, wide enough to read live chat.
pub const INPUT_SIDEBAR_MIN: f32 = 240.0;
pub const INPUT_SIDEBAR_MAX: f32 = 560.0;
/// The pinned chat's height above the insert panels.
pub const INPUT_CHAT_DEFAULT: f32 = 220.0;
pub const INPUT_CHAT_MIN: f32 = 96.0;

pub fn input_sidebar_width(value: Option<f32>) -> f32 {
    value
        .filter(|v| v.is_finite())
        .unwrap_or(INPUT_SIDEBAR_DEFAULT)
        .clamp(INPUT_SIDEBAR_MIN, INPUT_SIDEBAR_MAX)
}

/// The input sidebar's panels, in rail order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InputDockTab {
    /// Pacing, effects, sounds and expressions to insert.
    #[default]
    Controls,
    Stickers,
    Phrases,
    /// Live chat, when it is not pinned above the others.
    Chat,
    #[serde(other)]
    Unknown,
}

impl InputDockTab {
    /// The panels that share the lower area beside pinned chat.
    pub const INSERTS: [Self; 3] = [Self::Controls, Self::Stickers, Self::Phrases];
}

/// The input sidebar: which panel shows, whether live chat sits pinned above
/// it, and whether the sidebar is folded to its rail.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct InputDockLayout {
    pub tab: InputDockTab,
    pub chat_pinned: bool,
    pub chat_height: f32,
    pub collapsed: bool,
}

impl Default for InputDockLayout {
    fn default() -> Self {
        Self {
            tab: InputDockTab::Controls,
            chat_pinned: false,
            chat_height: INPUT_CHAT_DEFAULT,
            collapsed: false,
        }
    }
}

impl InputDockLayout {
    /// Repair stale or unsupported entries from older or edited files.
    pub fn normalized(mut self) -> Self {
        if self.tab == InputDockTab::Unknown || (self.chat_pinned && self.tab == InputDockTab::Chat)
        {
            self.tab = InputDockTab::Controls;
        }
        self.chat_height = if self.chat_height.is_finite() {
            self.chat_height.max(INPUT_CHAT_MIN)
        } else {
            INPUT_CHAT_DEFAULT
        };
        self
    }

    /// Live chat is on screen, pinned or as the open panel.
    pub fn chat_visible(&self) -> bool {
        !self.collapsed && (self.chat_pinned || self.tab == InputDockTab::Chat)
    }

    /// `tab` is the panel on screen (pinned chat is not a panel).
    pub fn showing(&self, tab: InputDockTab) -> bool {
        if tab == InputDockTab::Chat {
            self.chat_visible()
        } else {
            !self.collapsed && self.tab == tab
        }
    }

    /// A rail icon was pressed: open its panel, or fold the sidebar to the
    /// rail when that panel is already open. Pinned chat's icon unpins it.
    pub fn rail(mut self, tab: InputDockTab) -> Self {
        if self.collapsed {
            self.collapsed = false;
            if tab != InputDockTab::Chat || !self.chat_pinned {
                self.tab = tab;
            }
        } else if tab == InputDockTab::Chat && self.chat_pinned {
            self.chat_pinned = false;
        } else if self.tab == tab {
            self.collapsed = true;
        } else {
            self.tab = tab;
        }
        self
    }

    /// Pinned chat takes the whole sidebar as its own panel; chat in a panel
    /// is pinned above the insert panels.
    pub fn toggle_pin(mut self) -> Self {
        self.collapsed = false;
        if self.chat_pinned {
            self.chat_pinned = false;
            self.tab = InputDockTab::Chat;
        } else {
            self.chat_pinned = true;
            if self.tab == InputDockTab::Chat {
                self.tab = InputDockTab::Controls;
            }
        }
        self
    }

    /// Bring chat into view by pinning it, or put it away.
    pub fn toggle_chat(mut self) -> Self {
        if !self.chat_visible() {
            self.chat_pinned = true;
            self.collapsed = false;
            if self.tab == InputDockTab::Chat {
                self.tab = InputDockTab::Controls;
            }
        } else if self.chat_pinned {
            self.chat_pinned = false;
        } else {
            self.tab = InputDockTab::Controls;
        }
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MonitorKey(pub String);

/// A monitor as the host sees it now.
#[derive(Debug, Clone, PartialEq)]
pub struct MonitorInfo {
    pub key: MonitorKey,
    /// Work area in logical (desktop) coordinates: `[x, y, w, h]`.
    pub work_area: [f64; 4],
    pub primary: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedFrame {
    pub monitor: MonitorKey,
    /// Top-left corner as a fraction of the work area (0..1).
    pub rel: [f64; 2],
    pub size: [f64; 2],
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct GeometryStore {
    /// Layout signature → window key → frame.
    pub profiles: BTreeMap<String, BTreeMap<String, SavedFrame>>,
    /// Preferred logical width, independent of temporary small-window limits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_sidebar_width: Option<f32>,
    /// The input sidebar's panel and chat placement, shared across monitor layouts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_dock: Option<InputDockLayout>,
}

/// Placement result in logical desktop coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

pub fn layout_signature(monitors: &[MonitorInfo]) -> String {
    let mut keys: Vec<&str> = monitors.iter().map(|m| m.key.0.as_str()).collect();
    keys.sort_unstable();
    keys.join("|")
}

fn to_placement(frame: &SavedFrame, monitor: &MonitorInfo) -> Placement {
    let [x, y, w, h] = monitor.work_area;
    let size_w = frame.size[0].min(w.max(1.0));
    let size_h = frame.size[1].min(h.max(1.0));
    let px = (x + frame.rel[0] * w).clamp(x, x + (w - size_w).max(0.0));
    let py = (y + frame.rel[1] * h).clamp(y, y + (h - size_h).max(0.0));
    Placement {
        x: px,
        y: py,
        w: size_w,
        h: size_h,
    }
}

impl GeometryStore {
    pub fn remember(&mut self, monitors: &[MonitorInfo], window: &str, rect: Placement) {
        let Some(monitor) = monitor_for(monitors, rect) else {
            return;
        };
        let [x, y, w, h] = monitor.work_area;
        let frame = SavedFrame {
            monitor: monitor.key.clone(),
            rel: [
                ((rect.x - x) / w.max(1.0)).clamp(0.0, 1.0),
                ((rect.y - y) / h.max(1.0)).clamp(0.0, 1.0),
            ],
            size: [rect.w, rect.h],
        };
        self.profiles
            .entry(layout_signature(monitors))
            .or_default()
            .insert(window.to_owned(), frame);
    }

    /// Exact layout first, then the newest frame on a monitor that is still
    /// connected. `None` means "use the default placement".
    pub fn restore(&self, monitors: &[MonitorInfo], window: &str) -> Option<Placement> {
        let find = |frame: &SavedFrame| {
            monitors
                .iter()
                .find(|m| m.key == frame.monitor)
                .map(|m| to_placement(frame, m))
        };
        if let Some(frame) = self
            .profiles
            .get(&layout_signature(monitors))
            .and_then(|p| p.get(window))
            && let Some(p) = find(frame)
        {
            return Some(p);
        }
        self.profiles
            .values()
            .filter_map(|p| p.get(window))
            .find_map(find)
    }

    pub fn forget(&mut self, window: &str) {
        for profile in self.profiles.values_mut() {
            profile.remove(window);
        }
    }
}

/// The monitor holding the rectangle's centre, else the primary one.
pub fn monitor_for(monitors: &[MonitorInfo], rect: Placement) -> Option<&MonitorInfo> {
    let cx = rect.x + rect.w / 2.0;
    let cy = rect.y + rect.h / 2.0;
    monitors
        .iter()
        .find(|m| {
            let [x, y, w, h] = m.work_area;
            cx >= x && cx < x + w && cy >= y && cy < y + h
        })
        .or_else(|| monitors.iter().find(|m| m.primary))
        .or_else(|| monitors.first())
}
