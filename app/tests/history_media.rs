//! History keeps inline pictures and non-ASCII glyphs visible in the rendered row.
//!
//! This is a regression test for the historical fallback failure where a font
//! face mapped a character but supplied an empty outline (the yen sign in a
//! Super Chat on macOS). The history surface must draw both that glyph and a
//! picture atom, not only preserve them in its model document.

mod common;

use std::collections::HashMap;
use std::sync::Arc;

use nana_ui::runtime::view::IntoView;
use nanatype::ui::{Ui, history};
use nanatype_core::{Cmd, Core};
use nanatype_i18n::Locale;
use nanatype_settings::Settings;

fn pixels_matching(image: &[u8], color: [u8; 3]) -> usize {
    image
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| {
            p[3] > 200
                && p[..3]
                    .iter()
                    .zip(color)
                    .all(|(actual, expected)| actual.abs_diff(expected) <= 8)
        })
        .count()
}

fn pink_text_pixels(image: &[u8]) -> usize {
    image
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| {
            let [r, g, b, a] = p;
            // The history bubble composites the requested #d00050 over its
            // warm surface. This hue test avoids counting the red test image.
            *a > 200 && *r > 170 && r.abs_diff(*g) > 20 && *b > *g
        })
        .count()
}

#[test]
fn history_renders_yen_emoji_and_inline_picture() {
    let skin = common::skin("nanatype.vanilla");
    let mut settings = Settings::default();
    settings.history.visible_items = 1;
    settings.history.show_avatar = false;
    settings.history.show_time = false;
    let ui = Ui::new(Arc::new(settings.clone()), skin.clone(), Locale::ZhCn);

    // Keep the text in a saturated colour so a missing fallback glyph cannot
    // be mistaken for a successful render merely because the row is present.
    let markup = "[color=#d00050]¥100 😀[/color] [img=blob:history-red alt=red]";
    let mut core = Core::new(settings, skin);
    core.handle(Cmd::SendMarkup(markup.into()), 0);
    core.handle(Cmd::Skip, 1);
    assert_eq!(core.history().len(), 1, "the line should enter history");
    assert!(core.history()[0].doc.plain_text().contains('¥'));
    assert!(core.history()[0].doc.plain_text().contains('😀'));

    // A data URL is deliberately used here: it exercises the history atom's
    // URL lookup without depending on a machine-local asset or media mirror.
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(24, 24, image::Rgba([250, 30, 30, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let image_url = format!(
        "data:image/png;base64,{}",
        nanatype::ui::settings::base64(&png)
    );
    ui.asset_urls.set(Arc::new(HashMap::from([(
        "blob:history-red".to_owned(),
        image_url,
    )])));
    ui.history
        .set(Arc::new(core.history().iter().cloned().collect()));
    ui.now.set(100_000);

    let mut session = common::session()
        .themed(false)
        .mount(796, 260, move || history::history_window(ui).into_any());
    session.set_gpu_renderers(nanatype::ui::sweep::renderers());
    session.flush().unwrap();
    let (_, rgba) = session.screenshot_rgba().unwrap();
    assert!(
        pixels_matching(&rgba, [250, 30, 30]) > 200,
        "inline history picture did not render"
    );
    assert!(
        pink_text_pixels(&rgba) > 3,
        "coloured yen/emoji text did not render"
    );
}
