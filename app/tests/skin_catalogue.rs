//! Catalogue refreshes reuse static artwork without preparing GPU effects.
use nanatype::{assets, ui};
use nanatype_i18n::Locale;
use nanatype_theme::{Localized, SkinRegistry, TomlTable};
use std::sync::{Arc, Mutex};

// These cases intentionally inspect the process-wide immutable image cache.
static PICKER_CACHE: Mutex<()> = Mutex::new(());

#[test]
fn picker_reuses_embedded_urls_and_resolves_only_static_preview_images() {
    let _cache = PICKER_CACHE.lock().unwrap();
    let root = std::env::temp_dir().join(format!("nanatype-catalogue-{}", std::process::id()));
    nanatype::services::media::init(&root.join("media")).unwrap();
    let mut registry = SkinRegistry::new(Some(root.join("skins")), env!("CARGO_PKG_VERSION"));
    let first = ui::skin_rows(&registry, Locale::ZhCn);
    assert_eq!(first.len(), registry.entries().len());
    for (entry, row) in registry.entries().iter().zip(&first) {
        let skin = entry.skin.as_ref().unwrap();
        assert!(skin.shader_programs.is_empty());
        assert!(skin.material_programs.is_empty());
        assert_eq!(
            *row.image_urls,
            assets::skin_preview_urls(skin, entry.files().unwrap())
        );
        let static_paths: std::collections::HashSet<_> = skin
            .referenced_files()
            .into_iter()
            .filter(|(field, _, kind)| {
                !matches!(*field, "shaders.textures" | "materials.textures")
                    && matches!(
                        kind,
                        nanatype_theme::FileKind::Image | nanatype_theme::FileKind::NineSlice(_)
                    )
            })
            .map(|(_, path, _)| path)
            .collect();
        for texture in skin
            .shaders
            .values()
            .flat_map(|s| &s.textures)
            .chain(skin.materials.values().flat_map(|s| &s.textures))
        {
            if !static_paths.contains(texture.as_str()) {
                assert!(
                    !row.image_urls.contains_key(&format!("skin:{texture}")),
                    "{}: GPU-only texture {texture}",
                    entry.id
                );
            }
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
    ui::prepare_skin_preview(ui, &registry, "nanatype.forest", true).unwrap();
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
    ui::prepare_skin_preview(ui, &registry, "nanatype.tidal", true).unwrap();
    assert!(!ui.shader_preview.get_untracked());
    assert!(
        registry
            .entry("nanatype.tidal")
            .unwrap()
            .prepared_skin()
            .is_some_and(|skin| skin.shaders["artwork"].static_fallback)
    );
    ui::prepare_skin_preview(ui, &registry, "nanatype.crayon", true).unwrap();
    assert_eq!(
        registry
            .entry("nanatype.crayon")
            .unwrap()
            .prepared_skin()
            .unwrap()
            .material_programs
            .len(),
        6
    );
    assert!(!ui::shader_effects::preview_animating(ui));
    assert!(
        !ui.shader_preview.get_untracked(),
        "static materials enabled the animation switch"
    );
    registry
        .duplicate(
            "nanatype.forest",
            "user.forest",
            &Localized::new(),
            &TomlTable::new(),
        )
        .unwrap();
    std::fs::write(
        root.join("skins/user.forest/unlisted.png"),
        include_bytes!("../../crates/theme/builtin/crayon/still.png"),
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
                &nanatype_model::AssetRef::Skin("unlisted.png".into()),
                Some(&loaded)
            )
            .is_some(),
        "unlisted document images must still resolve lazily from skin files"
    );
    ui.skins.set(Arc::new(rows));
    let path = root.join("skins/user.forest/skin.toml");
    let mut text = std::fs::read_to_string(&path).unwrap();
    text.push_str("\n[dialogue]\nregion = [900, 260]\n");
    std::fs::write(path, text).unwrap();
    ui.reduced_motion.set(false);
    ui::prepare_skin_preview(ui, &registry, "user.forest", true).unwrap();
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

#[test]
fn material_input_textures_are_not_loaded_by_static_previews() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../target/material-preview-inputs")
        .join(std::process::id().to_string());
    std::fs::create_dir_all(&root).unwrap();
    nanatype::services::media::init(&root.join("media")).unwrap();
    let png = include_bytes!("../../crates/theme/builtin/crayon/still.png");
    std::fs::write(root.join("fallback.png"), png).unwrap();
    std::fs::write(root.join("input.png"), png).unwrap();
    let mut skin = nanatype_theme::Skin::default();
    skin.meta.preview = Some("fallback.png".into());
    skin.materials.insert(
        "test".into(),
        nanatype_theme::MaterialSpec {
            source: "input.wgsl".into(),
            textures: vec!["input.png".into()],
            ..Default::default()
        },
    );
    let urls = assets::skin_preview_urls(&skin, &nanatype_theme::DirFiles::new(root));
    assert!(urls.contains_key("skin:fallback.png"));
    assert!(!urls.contains_key("skin:input.png"));
}

#[test]
fn selecting_static_artwork_prepares_only_the_preview_and_keeps_motion_off() {
    let _cache = PICKER_CACHE.lock().unwrap();
    let registry = SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
    let active = registry.load("nanatype.halo").unwrap().skin;
    let ui = ui::Ui::new(Arc::new(Default::default()), active.clone(), Locale::ZhCn);
    ui.skins
        .set(Arc::new(ui::skin_rows(&registry, Locale::ZhCn)));
    for id in ["nanatype.parchment", "nanatype.sakura", "nanatype.ink"] {
        ui::prepare_skin_preview(ui, &registry, id, false).unwrap();
        assert!(
            !ui.shader_preview.get_untracked(),
            "{id}: selection started motion"
        );
        assert!(registry.entry(id).unwrap().prepared_skin().is_some());
        let preview = ui.skins.with_untracked(|rows| {
            rows.iter()
                .find(|r| r.id == id)
                .unwrap()
                .skin
                .clone()
                .unwrap()
        });
        let portrait = nanatype_theme::responsive_dialogue(&preview, [300., 1200.]);
        let effect = &portrait.dialogue.shader_effects[0];
        assert!(portrait.shader_programs.contains_key(&effect.shader));
        assert!(Arc::ptr_eq(&ui.skin.get_untracked(), &active));
    }
    ui::prepare_skin_preview(ui, &registry, "nanatype.sakura", true).unwrap();
    assert!(ui.shader_preview.get_untracked());
    ui::prepare_skin_preview(ui, &registry, "nanatype.sakura", false).unwrap();
    assert!(!ui.shader_preview.get_untracked());
}
