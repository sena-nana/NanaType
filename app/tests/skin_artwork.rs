//! Embedded and inherited image decorations must render through the allowed base.

mod common;

use nana_ui::runtime::Stack;
use nana_ui::runtime::view::widget;
use nana_ui_core::LengthSpec;
use nana_ui_devtools::agent::AgentSession;
use nanatype::ui::surface::Surface;
use nanatype_theme::{Localized, SkinRegistry, TomlTable};

#[test]
fn scanned_ink_loads_from_embedded_and_inherited_skin_files() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../target/skin-artwork-test")
        .join(std::process::id().to_string());
    nanatype::services::media::init(&root.join("data")).unwrap();
    let mut registry = SkinRegistry::new(Some(root.join("skins")), env!("CARGO_PKG_VERSION"));
    let loaded = registry.load("nanatype.ink").unwrap();
    let urls = nanatype::assets::skin_urls(&*loaded.files);
    assert!(urls.contains_key("skin:bamboo.png"));
    assert!(urls.contains_key("skin:still.png"));
    let names = Localized::from([("en-US".to_owned(), "My Ink".to_owned())]);
    registry
        .duplicate("nanatype.ink", "my-ink", &names, &TomlTable::new())
        .unwrap();
    let copy = registry.load("my-ink").unwrap();
    let copy_urls = nanatype::assets::skin_urls(&*copy.files);
    for (path, url) in &urls {
        let reference = format!(
            "skin:pkg:nanatype.ink/{}",
            path.strip_prefix("skin:").unwrap()
        );
        assert_eq!(copy_urls.get(&reference), Some(url));
    }
    for row in nanatype::ui::skin_rows(&registry, nanatype_i18n::Locale::ZhCn) {
        if row.id == "my-ink" || row.id == "nanatype.ink" {
            for (_, path, kind) in row.skin.as_ref().unwrap().referenced_files() {
                if matches!(
                    kind,
                    nanatype_theme::FileKind::Image | nanatype_theme::FileKind::NineSlice(_)
                ) {
                    let key = format!("skin:{path}");
                    assert!(
                        row.image_urls.contains_key(&key),
                        "picker resolves inherited artwork {key}"
                    );
                }
            }
        }
    }
    let decor = loaded
        .skin
        .dialogue
        .decor
        .iter()
        .find(|d| d.image.is_some())
        .unwrap();
    let surface = Surface::decor_image(
        &loaded.skin,
        decor,
        urls.get(&format!("skin:{}", decor.image.as_ref().unwrap()))
            .cloned(),
    );
    assert!(!surface.is_vector());
    let mut session = common::session().mount(200, 180, move || {
        widget(Stack::row(0.0).padding(10.0)).children(widget(
            surface.stack(
                Stack::row(0.0)
                    .width(LengthSpec::Px(180.0))
                    .height(LengthSpec::Px(160.0)),
            ),
        ))
    });
    session.set_clear(Some([0.0; 4]));
    let (_, rgba) = session.screenshot_rgba().unwrap();
    let pixels = rgba.as_chunks::<4>().0;
    let ink = pixels.iter().filter(|p| p[3] > 10).count();
    let translucent = pixels.iter().filter(|p| p[3] > 10 && p[3] < 220).count();
    assert!(ink > 300, "packaged artwork was blank: {ink}");
    assert!(
        translucent > 200,
        "scanned alpha grain was lost: {translucent}"
    );
    assert_eq!(
        rgba[3], 0,
        "transparent PNG must not paint a rectangular backdrop"
    );
    // Contain preserves the portrait artwork's aspect ratio with side margins.
    // CSS background images repeat by default; an ornament must draw only once.
    for y in 10..170_usize {
        for x in (10..22_usize).chain(178..190_usize) {
            assert_eq!(
                rgba[(y * 200 + x) * 4 + 3],
                0,
                "artwork tiled into the letterbox at ({x},{y})"
            );
        }
    }
}
