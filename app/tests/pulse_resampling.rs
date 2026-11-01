//! A pulsing glyph is its laid-out bitmap drawn larger. It has to be
//! resampled, not have its texels repeated.
mod common;

use nanatype::ui::{Ui, dialogue};
use nanatype_core::{Cmd, Core};
use nanatype_i18n::Locale;
use nanatype_settings::Settings;
use std::{collections::HashSet, sync::Arc};

/// The display showing `markup`, its frames at 100 s plus each offset (ms).
fn frames(markup: &str, offsets: &[u64]) -> Vec<Vec<u8>> {
    let skin = common::skin("nanatype.vanilla");
    let settings = Settings::default();
    let ui = Ui::new(Arc::new(settings.clone()), skin.clone(), Locale::ZhCn);
    let mut core = Core::new(settings, skin);
    core.handle(Cmd::SendMarkup(markup.into()), 0);
    core.handle(Cmd::Skip, 1);
    ui.display.set(Arc::new(core.display().clone()));
    ui.now.set(100_000);
    let mut session =
        common::session().mount_at(100_000, 796, 236, move || dialogue::display_window(ui));
    offsets
        .iter()
        .map(|ms| {
            common::frame(
                &mut session,
                ui,
                100_000 + ms,
                250 + ms,
                &format!("pulse-{ms}"),
                "NANATYPE_PULSE_CAPTURE",
            )
        })
        .collect()
}

/// Distinct grey levels in the text, and how many inked columns repeat the
/// column before them exactly: what nearest sampling leaves when it stretches
/// a bitmap.
fn texture(rgba: &[u8]) -> (usize, usize) {
    let (x0, y0, x1, y1) = (60, 66, 334, 118);
    let luma = |x: u32, y: u32| {
        let [r, g, b, _] = common::pixel(rgba, 796, x, y);
        (u32::from(r) * 2126 + u32::from(g) * 7152 + u32::from(b) * 722) / 10_000
    };
    let levels: HashSet<_> = (y0..y1)
        .flat_map(|y| (x0..x1).map(move |x| (x, y)))
        .map(|(x, y)| luma(x, y))
        .collect();
    let repeats = (x0..x1 - 1)
        .filter(|&x| {
            (y0..y1).filter(|&y| luma(x, y) < 200).count() > 3
                && (y0..y1).all(|y| luma(x, y) == luma(x + 1, y))
        })
        .count();
    (levels.len(), repeats)
}

#[test]
fn a_pulse_resamples_its_glyphs_instead_of_repeating_texels() {
    let offsets = [0, 250, 500, 750];
    for frame in frames("[pulse amp=2]脉动永字 Pulse wave[/pulse]", &offsets) {
        let (levels, repeats) = texture(&frame);
        // Nearest sampling repeated 18–30 whole columns here and showed only
        // the bitmap's own 17–32 levels; filtered, the edges blend.
        assert!(repeats <= 8, "{repeats} repeated columns");
        assert!(levels >= 96, "only {levels} grey levels");
    }
}
