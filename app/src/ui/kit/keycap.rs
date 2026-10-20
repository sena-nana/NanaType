//! Keycaps: keys drawn as small raised caps, wherever the app names a key
//! (the hotkey rows in settings, the input window's shortcut sheet). The caps
//! are NanaUI's; the app gives arrow keys a drawn arrow.

use nana_ui::icons_tabler as ti;
use nana_ui::runtime::view::{AnyView, chord_with, kbd, kbd_glyph};

/// A keycap's height (NanaUI's cap), for marks drawn beside keycaps.
pub const H: f32 = 24.0;

pub use nana_ui::runtime::view::{
    chord_joiner as joiner, keycap_face as face, keycap_label as label,
};

/// One key as a keycap. Arrow keys show a drawn arrow: the text arrows
/// (↑ ↓ ← →) come from a fallback font in most UI faces and sit oversized
/// and off the baseline next to the letters.
pub fn keycap(key: impl Into<String>) -> AnyView {
    let key = key.into();
    match arrow(&key) {
        Some(glyph) => kbd_glyph(glyph),
        None => kbd(key),
    }
}

/// The drawn arrow for an arrow key's label.
fn arrow(key: &str) -> Option<nana_ui::runtime::Icon> {
    Some(match key {
        "↑" | "Up" | "ArrowUp" => ti::ARROW_UP,
        "↓" | "Down" | "ArrowDown" => ti::ARROW_DOWN,
        "←" | "Left" | "ArrowLeft" => ti::ARROW_LEFT,
        "→" | "Right" | "ArrowRight" => ti::ARROW_RIGHT,
        _ => return None,
    })
}

/// The muted "+" between the keys of a chord.
pub fn plus() -> nana_ui::runtime::Text {
    joiner("+")
}

/// A chord ("Ctrl+B") as keycaps joined by "+".
pub fn chord(keys: &str) -> AnyView {
    chord_with(keys, |k| keycap(k))
}
