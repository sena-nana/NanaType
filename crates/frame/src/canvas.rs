//! The broadcast canvases a skin draws on and their logical sizes, apart from
//! the native windows that show them.

use nanatype_settings::{OverlaySettings, Settings};
use nanatype_theme::Skin;

/// A surface the skin renderer lays out: the dialogue box (local or the
/// peer's track) or the history panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Canvas {
    Display,
    PeerDisplay,
    History,
}

impl Canvas {
    /// The overlay settings of the window that shows this canvas.
    pub fn overlay(self, s: &Settings) -> &OverlaySettings {
        match self {
            Self::Display => &s.overlays.display,
            Self::PeerDisplay => &s.overlays.peer_display,
            Self::History => &s.overlays.history,
        }
    }

    /// Vertical writing is the window's own switch; the skin only has to support it.
    pub fn vertical_text(self, s: &Settings, skin: &Skin) -> bool {
        skin.dialogue.responsive.is_some() && self.overlay(s).vertical_text
    }
}

/// Logical size of `canvas` for the current skin and settings.
pub fn size_of(canvas: Canvas, skin: &Skin, s: &Settings) -> (f32, f32) {
    let d = &skin.dialogue;
    let profile = s.skin.personalizations.get(&s.skin.id);
    let region = if profile.is_some() {
        d.region
    } else {
        s.skin.overrides.region.unwrap_or(d.region)
    };
    let pad = d.overscan * 2.0;
    match canvas {
        Canvas::Display => profile
            .map_or(s.overlays.display.size, |p| p.display.size)
            .map(|[w, h]| (w, h))
            .unwrap_or((region[0] + pad, region[1] + pad)),
        Canvas::PeerDisplay => s
            .overlays
            .peer_display
            .size
            .map(|[w, h]| (w, h))
            .unwrap_or_else(|| {
                let region = s.skin.overrides.region.unwrap_or(d.region);
                (region[0] + pad, region[1] + pad)
            }),
        Canvas::History => profile
            .map_or(s.overlays.history.size, |p| p.history.size)
            .map(|[w, h]| (w, h))
            .unwrap_or((skin.history.width, skin.history.height)),
    }
}
