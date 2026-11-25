use nana_ui::{WindowOutputAlpha, WindowOutputConfig, WindowOutputExport, WindowOutputExtent};
use nanatype_settings::{Settings, SpoutOutput};

use crate::ui::LinkState;
use crate::windows::WindowKind;

/// What a Spout sender's failure is called, one key per kind of failure.
pub const SPOUT_ERROR_KEYS: [&str; 6] = [
    "output-spout-unsupported",
    "spout-error-backend",
    "spout-error-interop",
    "spout-error-format",
    "spout-error-publish",
    "spout-error-name",
];

/// Syphon failures. Kept beside the Spout keys so every locale is checked.
pub const SYPHON_ERROR_KEYS: [&str; 2] = ["output-syphon-start", "output-syphon-publish"];

pub const KINDS: [WindowKind; 3] = [
    WindowKind::Display,
    WindowKind::History,
    WindowKind::Danmaku,
];

fn kind_index(kind: WindowKind) -> Option<usize> {
    match kind {
        WindowKind::Display => Some(0),
        WindowKind::History => Some(1),
        WindowKind::Danmaku => Some(2),
        _ => None,
    }
}

/// Live OBS bounding-box sizes per output window, kept outside persisted [`Settings`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeOutputSizes {
    sizes: [Option<[u32; 2]>; 3],
}

impl RuntimeOutputSizes {
    pub fn get(&self, kind: WindowKind) -> Option<[u32; 2]> {
        kind_index(kind).and_then(|i| self.sizes[i])
    }

    pub fn set(&mut self, kind: WindowKind, size: Option<[u32; 2]>) -> bool {
        let Some(i) = kind_index(kind) else {
            return false;
        };
        if self.sizes[i] != size {
            self.sizes[i] = size;
            true
        } else {
            false
        }
    }

    pub fn extent_for(&self, kind: WindowKind, out: &SpoutOutput) -> Option<[u32; 2]> {
        self.get(kind).or(out.size)
    }
}

/// Dedicated UI status map for Spout/Syphon outputs, independent of other domain statuses.
#[derive(Debug, Clone, PartialEq)]
pub struct OutputStatuses {
    pub display: LinkState,
    pub history: LinkState,
    pub danmaku: LinkState,
}

impl Default for OutputStatuses {
    fn default() -> Self {
        Self {
            display: LinkState::Off,
            history: LinkState::Off,
            danmaku: LinkState::Off,
        }
    }
}

impl OutputStatuses {
    pub fn get(&self, kind: WindowKind) -> &LinkState {
        match kind {
            WindowKind::Display => &self.display,
            WindowKind::History => &self.history,
            WindowKind::Danmaku => &self.danmaku,
            _ => &self.display,
        }
    }

    pub fn set(&mut self, kind: WindowKind, state: LinkState) -> bool {
        let slot = match kind {
            WindowKind::Display => &mut self.display,
            WindowKind::History => &mut self.history,
            WindowKind::Danmaku => &mut self.danmaku,
            _ => return false,
        };
        if *slot != state {
            *slot = state;
            true
        } else {
            false
        }
    }

    pub fn any_connecting(&self) -> bool {
        [&self.display, &self.history, &self.danmaku]
            .into_iter()
            .any(|s| *s == LinkState::Connecting)
    }
}

pub fn settings_for(kind: WindowKind, s: &Settings) -> Option<&SpoutOutput> {
    match kind {
        WindowKind::Display => Some(&s.output.display),
        WindowKind::History => Some(&s.output.history),
        WindowKind::Danmaku => Some(&s.output.danmaku),
        _ => None,
    }
}

pub fn settings_for_mut(kind: WindowKind, s: &mut Settings) -> Option<&mut SpoutOutput> {
    match kind {
        WindowKind::Display => Some(&mut s.output.display),
        WindowKind::History => Some(&mut s.output.history),
        WindowKind::Danmaku => Some(&mut s.output.danmaku),
        _ => None,
    }
}

/// Whether `kind`'s window has someone to show: always, except the separate
/// danmaku window while `separate` is off.
pub(crate) fn active_window(kind: WindowKind, s: &Settings) -> bool {
    match kind {
        WindowKind::Danmaku => s.history.chat.separate,
        _ => true,
    }
}

pub fn name_conflicts(kind: WindowKind, s: &Settings) -> bool {
    let Some(output) = settings_for(kind, s) else {
        return false;
    };
    KINDS
        .into_iter()
        .filter(|other| {
            *other != kind && (*other != WindowKind::Danmaku || s.history.chat.separate)
        })
        .filter_map(|other| settings_for(other, s))
        .any(|other| {
            output.enabled
                && other.enabled
                && !output.name.trim().is_empty()
                && output.name.trim() == other.name.trim()
        })
}

pub fn config(kind: WindowKind, s: &Settings) -> Option<WindowOutputConfig> {
    resolve(kind, s).config(None)
}

#[derive(Clone, Copy)]
pub(crate) enum ResolvedOutput<'a> {
    Off,
    Invalid(&'static str),
    Ready(&'a SpoutOutput),
}

/// Configuration, sender lifetime and frame delivery share this decision.
pub(crate) fn resolve(kind: WindowKind, s: &Settings) -> ResolvedOutput<'_> {
    let Some(out) = settings_for(kind, s).filter(|o| o.enabled && active_window(kind, s)) else {
        return ResolvedOutput::Off;
    };
    if !(cfg!(windows) || cfg!(target_os = "macos")) {
        ResolvedOutput::Invalid("output-spout-unsupported")
    } else if out.name.trim().is_empty() {
        ResolvedOutput::Invalid("spout-error-name")
    } else if name_conflicts(kind, s) {
        ResolvedOutput::Invalid("output-name-conflict")
    } else {
        ResolvedOutput::Ready(out)
    }
}

impl<'a> ResolvedOutput<'a> {
    pub(crate) fn output(self) -> Option<&'a SpoutOutput> {
        match self {
            Self::Ready(out) => Some(out),
            _ => None,
        }
    }

    pub(crate) fn config(self, runtime_size: Option<[u32; 2]>) -> Option<WindowOutputConfig> {
        let out = self.output()?;
        let extent = match runtime_size.or(out.size) {
            Some([width, height]) => WindowOutputExtent::Fixed { width, height },
            None => WindowOutputExtent::MatchWindow,
        };
        // Windows shares a DX12 texture. macOS blits the same-device texture
        // into Syphon; NanaUI has no Metal export token.
        let export = if cfg!(windows) {
            WindowOutputExport::Native
        } else {
            WindowOutputExport::SameDevice
        };
        Some(
            WindowOutputConfig::default()
                .with_extent(extent)
                .with_alpha(if out.straight_alpha {
                    WindowOutputAlpha::Straight
                } else {
                    WindowOutputAlpha::Premultiplied
                })
                .with_export(export)
                .while_hidden(true),
        )
    }
}
