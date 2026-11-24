use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use nana_ui::runtime::view::Signal;
use nanatype_settings::Settings;

use super::Draft;
use crate::ui::Ui;

/// The overscan of skin `id` as the catalogue loaded it.
pub(super) fn overscan(ui: Ui, id: &str) -> Option<f32> {
    ui.skins.with_untracked(|rows| {
        rows.iter()
            .find(|r| r.id == id)
            .and_then(|r| r.skin.as_ref())
            .map(|s| s.dialogue.overscan)
    })
}

/// [`Draft::load`] with the overscan of the catalogue's skin `id`.
pub fn load_draft(settings: &Settings, id: &str, ui: Ui) -> Draft {
    Draft::load(settings, id, *ui, overscan(ui, id))
}

pub(in crate::ui::settings) fn draft(ui: Ui, id: &str) -> Draft {
    nanatype_frame::ui::preview::draft(*ui, id, overscan(ui, id))
}

pub(in crate::ui::settings) fn change(ui: Ui, id: &str, f: impl FnOnce(&mut Draft)) {
    nanatype_frame::ui::preview::change(*ui, id, overscan(ui, id), f)
}

/// Both skin catalogue layouts preview a skin on selection; the preview
/// asks for its stage once it shows (see [`super::view`]).
pub(in crate::ui::settings) fn select(
    previewing: Signal<Option<String>>,
    row: &crate::ui::SkinRow,
) {
    if row.error.is_none() {
        previewing.set(Some(row.id.clone()));
    }
}

/// Fingerprint of a catalogue row's availability, fonts, and parameter/preset
/// schema so the editor remounts only when the skin schema or validity changes,
/// never when `ui.skin_preview` finishes loading.
pub(super) fn param_schema_version(row: Option<&crate::ui::SkinRow>) -> u64 {
    let mut h = DefaultHasher::new();
    let Some(row) = row.filter(|r| r.error.is_none()) else {
        0u8.hash(&mut h);
        return h.finish();
    };
    let Some(skin) = row.skin.as_ref() else {
        1u8.hash(&mut h);
        return h.finish();
    };
    2u8.hash(&mut h);
    row.fallback_font.hash(&mut h);
    format!("{:?}", (&skin.params, &skin.presets)).hash(&mut h);
    h.finish()
}
