use nana_ui::{WindowOutputAlpha, WindowOutputConfig, WindowOutputExport, WindowOutputExtent};
use nanatype_settings::{PARTNER_SEATS, Settings, SpoutOutput};

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

pub const KINDS: [WindowKind; 3 + PARTNER_SEATS] = [
    WindowKind::Display,
    WindowKind::History,
    WindowKind::Danmaku,
    WindowKind::PeerDisplay(0),
    WindowKind::PeerDisplay(1),
    WindowKind::PeerDisplay(2),
    WindowKind::PeerDisplay(3),
    WindowKind::PeerDisplay(4),
    WindowKind::PeerDisplay(5),
    WindowKind::PeerDisplay(6),
];

fn kind_index(kind: WindowKind) -> Option<usize> {
    match kind {
        WindowKind::Display => Some(0),
        WindowKind::History => Some(1),
        WindowKind::Danmaku => Some(2),
        WindowKind::PeerDisplay(seat) if usize::from(seat) < PARTNER_SEATS => {
            Some(3 + usize::from(seat))
        }
        _ => None,
    }
}

/// Live OBS bounding-box sizes per output window, kept outside persisted [`Settings`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuntimeOutputSizes {
    sizes: [Option<[u32; 2]>; 3 + PARTNER_SEATS],
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
    pub partners: Vec<LinkState>,
    pub history: LinkState,
    pub danmaku: LinkState,
}

impl Default for OutputStatuses {
    fn default() -> Self {
        Self {
            display: LinkState::Off,
            partners: vec![LinkState::Off; PARTNER_SEATS],
            history: LinkState::Off,
            danmaku: LinkState::Off,
        }
    }
}

impl OutputStatuses {
    pub fn partner(&self, seat: usize) -> &LinkState {
        self.partners.get(seat).unwrap_or(&self.display)
    }

    pub fn get(&self, kind: WindowKind) -> &LinkState {
        match kind {
            WindowKind::Display => &self.display,
            WindowKind::History => &self.history,
            WindowKind::Danmaku => &self.danmaku,
            WindowKind::PeerDisplay(seat) => self
                .partners
                .get(usize::from(seat))
                .unwrap_or(&self.display),
            _ => &self.display,
        }
    }

    pub fn set(&mut self, kind: WindowKind, state: LinkState) -> bool {
        let slot = match kind {
            WindowKind::Display => &mut self.display,
            WindowKind::History => &mut self.history,
            WindowKind::Danmaku => &mut self.danmaku,
            WindowKind::PeerDisplay(seat) => {
                let idx = usize::from(seat);
                if idx >= PARTNER_SEATS {
                    return false;
                }
                if self.partners.len() <= idx {
                    self.partners.resize(PARTNER_SEATS, LinkState::Off);
                }
                &mut self.partners[idx]
            }
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
            .chain(&self.partners)
            .any(|s| *s == LinkState::Connecting)
    }
}

pub fn settings_for(kind: WindowKind, s: &Settings) -> Option<&SpoutOutput> {
    match kind {
        WindowKind::Display => Some(&s.output.display),
        WindowKind::PeerDisplay(seat) => s.output.partners.get(usize::from(seat)),
        WindowKind::History => Some(&s.output.history),
        WindowKind::Danmaku => Some(&s.output.danmaku),
        _ => None,
    }
}

pub fn settings_for_mut(kind: WindowKind, s: &mut Settings) -> Option<&mut SpoutOutput> {
    match kind {
        WindowKind::Display => Some(&mut s.output.display),
        WindowKind::PeerDisplay(seat) => s.output.partners.get_mut(usize::from(seat)),
        WindowKind::History => Some(&mut s.output.history),
        WindowKind::Danmaku => Some(&mut s.output.danmaku),
        _ => None,
    }
}

/// Whether `kind`'s window has someone to show: always, except a partner seat
/// nobody sits in or the separate danmaku window while `separate` is off.
pub(crate) fn active_window(kind: WindowKind, s: &Settings, partners: usize) -> bool {
    match kind {
        WindowKind::PeerDisplay(seat) => usize::from(seat) < partners,
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

/// What `kind`'s window sends with every seat taken.
pub fn config(kind: WindowKind, s: &Settings) -> Option<WindowOutputConfig> {
    resolve(kind, s, PARTNER_SEATS).config(None)
}

#[derive(Clone, Copy)]
pub(crate) enum ResolvedOutput<'a> {
    Off,
    Invalid(&'static str),
    Ready(&'a SpoutOutput),
}

/// Configuration, sender lifetime and frame delivery share this decision.
/// Enabled names remain reserved even while their seat is empty.
pub(crate) fn resolve(kind: WindowKind, s: &Settings, partners: usize) -> ResolvedOutput<'_> {
    let Some(out) = settings_for(kind, s).filter(|o| o.enabled && active_window(kind, s, partners))
    else {
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
