//! Catalogue refreshes reuse static artwork without preparing GPU effects.
use nanatype::{assets, ui};
use nanatype_i18n::Locale;
use nanatype_theme::{Localized, SkinRegistry, TomlTable};
use std::sync::Arc;

#[test]
fn picker_reuses_embedded_urls_and_resolves_only_static_preview_images() {
    let root = std::env::temp_dir().join(format!("nanatype-catalogue-{}", std::process::id()));
    nanatype::services::media::init(&root.join("media")).unwrap();
    let mut registry = SkinRegistry::new(Some(root.join("skins")), env!("CARGO_PKG_VERSION"));
    let first = ui::skin_rows(&registry, Locale::ZhCn);
    assert_eq!(first.len(), 24);
    for (entry, row) in registry.entries().iter().zip(&first) {
        let skin = entry.skin.as_ref().unwrap();
        assert!(skin.shader_programs.is_empty());
        assert_eq!(
            *row.image_urls,
            assets::skin_preview_urls(skin, entry.files().unwrap())
        );
        for texture in skin.shaders.values().flat_map(|s| &s.textures) {
            assert!(!row.image_urls.contains_key(&format!("skin:{texture}")));
        }
    }
    registry.rescan();
    let second = ui::skin_rows(&registry, Locale::EnUs);
    for (a, b) in first.iter().zip(&second) {
        assert!(Arc::ptr_eq(&a.image_urls, &b.image_urls));
    }
    let active = registry.load("nanatype.halo").unwrap().skin;
    let ui = ui::Ui::new(Arc::new(Default::default()), active.clone(), Locale::ZhCn);
    ui.skins.set(Arc::new(second));
    ui::prepare_skin_preview(ui, &registry, "nanatype.forest").unwrap();
    assert!(ui.shader_preview.get_untracked());
    assert!(Arc::ptr_eq(&ui.skin.get_untracked(), &active));
    assert_eq!(
        ui.skins.with_untracked(|rows| rows
            .iter()
            .filter(|r| !r.skin.as_ref().unwrap().shader_programs.is_empty())
            .count()),
        1
    );
    // Refresh/rescan must retain the currently playing embedded preview.
    registry.rescan();
    let refreshed = ui::skin_rows(&registry, Locale::ZhCn);
    assert!(
        !refreshed
            .iter()
            .find(|r| r.id == "nanatype.forest")
            .unwrap()
            .skin
            .as_ref()
            .unwrap()
            .shader_programs
            .is_empty()
    );
    ui.reduced_motion.set(true);
    ui::prepare_skin_preview(ui, &registry, "nanatype.tidal").unwrap();
    assert!(!ui.shader_preview.get_untracked());
    assert!(
        registry
            .entry("nanatype.tidal")
            .unwrap()
            .prepared_skin()
            .is_none()
    );
    registry
        .duplicate(
            "nanatype.forest",
            "user.forest",
            &Localized::new(),
            &TomlTable::new(),
        )
        .unwrap();
    let rows = ui::skin_rows(&registry, Locale::ZhCn);
    let parent = rows.iter().find(|r| r.id == "nanatype.forest").unwrap();
    let child = rows.iter().find(|r| r.id == "user.forest").unwrap();
    assert_eq!(parent.image_urls, child.image_urls);
    // Document images still have access to every packaged image, including
    // files absent from the static thumbnail's manifest references.
    let loaded = registry.load_scanned("user.forest").unwrap();
    let active = assets::skin_urls(&*loaded.files);
    assert!(active.len() > child.image_urls.len());
    assert!(active.contains_key("skin:forest/ornament.png"));
    let library = assets::AssetLibrary::new(root.join("blobs"));
    assert!(
        library
            .url(
                &nanatype_model::AssetRef::Skin("forest/ornament.png".into()),
                Some(&loaded)
            )
            .is_some(),
        "unlisted document images must still resolve lazily from embedded bases"
    );
    ui.skins.set(Arc::new(rows));
    let path = root.join("skins/user.forest/skin.toml");
    let mut text = std::fs::read_to_string(&path).unwrap();
    text.push_str("\n[dialogue]\nregion = [900, 260]\n");
    std::fs::write(path, text).unwrap();
    ui.reduced_motion.set(false);
    ui::prepare_skin_preview(ui, &registry, "user.forest").unwrap();
    assert_eq!(
        ui.skins.with_untracked(|rows| rows
            .iter()
            .find(|r| r.id == "user.forest")
            .unwrap()
            .skin
            .as_ref()
            .unwrap()
            .dialogue
            .region),
        [900., 260.]
    );
    // Saving a local image in place must give the painter a new immutable URL;
    // repeated loads of the same bytes must still reuse the existing URL.
    let frame_path = root.join("skins/user.forest/forest/still.png");
    std::fs::create_dir_all(frame_path.parent().unwrap()).unwrap();
    let frame = |pixel| image::RgbaImage::from_pixel(2, 2, image::Rgba(pixel));
    frame([10, 20, 30, 40]).save(&frame_path).unwrap();
    let before = assets::skin_preview_urls(&loaded.skin, &*loaded.files);
    assert_eq!(
        before,
        assets::skin_preview_urls(&loaded.skin, &*loaded.files)
    );
    frame([40, 30, 20, 10]).save(&frame_path).unwrap();
    let after = assets::skin_preview_urls(&loaded.skin, &*loaded.files);
    assert_ne!(
        before["skin:forest/still.png"],
        after["skin:forest/still.png"]
    );
}
