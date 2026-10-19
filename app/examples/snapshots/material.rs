//! `NANATYPE_MATERIAL_AUDIT`: the skin's materials, on and off.

use super::*;

pub fn audit(ui: Ui, out: &std::path::Path, w: u32, h: u32, theme: Arc<nana_ui::CompiledTheme>) {
    let original = ui.display.get_untracked();
    let settings = ui.settings.get_untracked();
    let reduced = ui.reduced_motion.get_untracked();
    let now = ui.now.get_untracked();
    let text=parse("深色彩铅 Colored pencil [color=#B94F49]红色笔触[/color]\n[ruby=ものがたり]故事[/ruby] 与中文、Latin、😀").doc;
    ui.display.update(|d| {
        let d = Arc::make_mut(d);
        d.doc = Arc::new(text.clone());
        d.reveal = nanatype_core::Reveal::full(text.unit_count() as u32);
        d.revision += 1;
    });
    ui.now.set(120_000);
    ui.reduced_motion.set(false);
    let id = DocumentId::new(1).unwrap();
    let mut doc = RuntimeDocument::new(id);
    doc.context_mut().set_theme_tokens(theme).unwrap();
    doc.context_mut()
        .mount_view_root(id, move || dialogue::display_window(ui))
        .unwrap();
    let mut session = RuntimeAgentSession::new(doc, w, h).unwrap();
    session.set_clear(Some([0.; 4]));
    let capture = |session: &mut RuntimeAgentSession| {
        nanatype::clock::capture_at(120_000, || session.screenshot_rgba().unwrap().1)
    };
    let first = capture(&mut session);
    let steady = capture(&mut session);
    assert_eq!(first, steady, "static material changed without an event");
    assert!(
        !nanatype::ui::shader_effects::animating(ui),
        "static material requested animation"
    );
    ui.reduced_motion.set(true);
    assert_eq!(
        first,
        capture(&mut session),
        "reduced motion removed static material"
    );
    ui.settings
        .update(|s| Arc::make_mut(s).skin.advanced_effects = false);
    let _ = capture(&mut session);
    let fallback = capture(&mut session);
    assert_ne!(first, fallback, "advanced effects did not disable material");
    let emoji_pixels = |pixels: &[u8]| {
        (130..215)
            .flat_map(|y| (150..600).map(move |x| (y * w as usize + x) * 4))
            .filter(|&p| {
                pixels[p] > 200 && pixels[p + 1] > 100 && pixels[p + 2] < 90 && pixels[p + 3] > 150
            })
            .count()
    };
    assert!(emoji_pixels(&first) > 40, "emoji fixture was not colored");
    assert!(
        emoji_pixels(&fallback) > 40,
        "material fallback lost the color emoji"
    );
    ui.settings.set(settings.clone());
    ui.reduced_motion.set(false);
    assert_eq!(
        first,
        capture(&mut session),
        "material toggle did not restore the original pixels"
    );
    for (name, bytes) in [("material-enabled", first), ("material-fallback", fallback)] {
        image::RgbaImage::from_raw(w, h, bytes)
            .unwrap()
            .save(out.join(format!("{name}.png")))
            .unwrap();
    }
    std::fs::write(out.join("material-check.json"),serde_json::to_vec_pretty(&serde_json::json!({"static_frames_identical":true,"reduced_motion_preserves_materials":true,"advanced_effects_toggle_roundtrips":true,"requests_animation":false})).unwrap()).unwrap();
    ui.display.set(original);
    ui.settings.set(settings);
    ui.reduced_motion.set(reduced);
    ui.now.set(now);
}
