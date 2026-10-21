//! Pictures that live outside NanaUI's image URL base must still be drawn.
//!
//! NanaUI only reads a local `url(...)` picture that canonicalizes under the
//! base set with `nana_ui::set_background_image_url_base` (the working
//! directory when none is set). NanaType's avatars, sticker folders and
//! picked files live elsewhere. This starts the media mirror the way the
//! app does, renders the pictures offscreen through the app's own
//! path → URL code and counts their pixels.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use nana_ui::runtime::Stack;
use nana_ui::runtime::view::widget;
use nana_ui_core::LengthSpec;
use nanatype::assets::AssetLibrary;
use nanatype::ui::settings::base64;
use nanatype_model::AssetRef;

const AVATAR: [u8; 3] = [255, 0, 255];
const STICKER: [u8; 3] = [0, 200, 255];
const CONTROL: [u8; 3] = [40, 220, 60];
const FRAME: [u8; 3] = [250, 140, 20];

fn solid(path: &Path, rgb: [u8; 3]) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    image::RgbaImage::from_pixel(32, 32, image::Rgba([rgb[0], rgb[1], rgb[2], 255]))
        .save(path)
        .unwrap();
}

/// `urls` side by side, each as a chat avatar draws it, rendered offscreen.
fn render(out: &Path, name: &str, urls: Vec<String>) -> image::RgbaImage {
    let ui = nanatype::ui::Ui::new(
        Arc::new(Default::default()),
        Arc::new(Default::default()),
        nanatype_i18n::Locale::ZhCn,
    );
    let mut session = common::session().mount(200, 64, move || {
        widget(Stack::row(8.0).padding(8.0)).children(
            urls.into_iter()
                .map(|url| {
                    widget(
                        Stack::row(0.0)
                            .width(LengthSpec::Px(48.0))
                            .height(LengthSpec::Px(48.0))
                            .with_layout(|l| l.position = nana_ui_core::PositionSpec::Relative),
                    )
                    .children(nanatype::ui::danmaku::reactive_picture(
                        *ui,
                        Some(url),
                        4.0,
                    ))
                })
                .collect::<Vec<_>>(),
        )
    });
    let path = out.join(format!("{name}.png"));
    session.screenshot_png(&path).expect("screenshot");
    image::open(&path).unwrap().into_rgba8()
}

fn count(img: &image::RgbaImage, rgb: [u8; 3]) -> usize {
    img.pixels()
        .filter(|p| p.0[..3].iter().zip(rgb).all(|(a, b)| a.abs_diff(b) <= 6))
        .count()
}

fn scratch() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("nanatype-outside-base-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// One test per process: NanaUI's base is process wide, first call wins.
#[test]
fn avatar_sticker_and_skin_frame_are_drawn() {
    let root = scratch();
    // The installed layout: data and cache are separate trees
    // (`%APPDATA%\<id>` and `%LOCALAPPDATA%\<id>\Cache`), stickers anywhere.
    let data = root.join("data");
    let blobs = root.join("cache").join("images");
    let stickers = root.join("stickers").join("cats");
    let skin = data.join("skins").join("test.frame");
    solid(&blobs.join("bili-face-1001.png"), AVATAR);
    solid(&stickers.join("hello.png"), STICKER);
    solid(&skin.join("frame.png"), FRAME);

    // What `App::new` does at startup.
    let media = nanatype::services::media::init(&data).expect("media init");
    assert_eq!(
        nanatype::services::media::effective_base().as_deref(),
        Some(media.base().as_path()),
        "NanaUI took the data folder as its image base"
    );

    let mut lib = AssetLibrary::new(blobs.clone());
    lib.scan(&[stickers.display().to_string()]);
    let avatar = lib
        .url(&AssetRef::Blob("bili-face-1001".into()), None)
        .expect("avatar url");
    let sticker = lib
        .url(&"lib:cats/hello".parse().unwrap(), None)
        .expect("sticker url");
    let frame = nanatype::assets::skin_urls(&nanatype_theme::DirFiles::new(&skin))
        .remove("skin:frame.png")
        .expect("frame url");
    println!("avatar url: {avatar}\nsticker url: {sticker}\nframe url: {frame}");
    let in_mirror = nanatype::assets::file_url(&media.dir());
    assert!(avatar.starts_with(&in_mirror), "{avatar}");
    assert!(sticker.starts_with(&in_mirror), "{sticker}");
    assert!(
        frame.starts_with(&in_mirror),
        "skin artwork uses content URLs"
    );
    assert_eq!(
        nanatype::assets::skin_urls(&nanatype_theme::DirFiles::new(&skin))["skin:frame.png"],
        frame,
        "unchanged artwork reuses its URL"
    );

    // The base alone is not enough: the same pictures by their own paths
    // stay refused, which is what the mirror is for.
    let raw = render(
        &root,
        "raw",
        vec![
            nanatype::assets::file_url(&blobs.join("bili-face-1001.png")),
            nanatype::assets::file_url(&stickers.join("hello.png")),
        ],
    );
    assert_eq!(
        count(&raw, AVATAR),
        0,
        "raw avatar path outside the base loaded?"
    );
    assert_eq!(
        count(&raw, STICKER),
        0,
        "raw sticker path outside the base loaded?"
    );

    let framed = render(&root, "frame", vec![frame.clone()]);
    let f = count(&framed, FRAME);
    println!("frame pixels: {f}");
    assert!(f > 1500, "skin frame was not drawn ({f} px)");

    solid(&skin.join("frame.png"), CONTROL);
    let reloaded = nanatype::assets::skin_urls(&nanatype_theme::DirFiles::new(&skin))
        .remove("skin:frame.png")
        .unwrap();
    assert_ne!(frame, reloaded, "an in-place edit changes the content URL");
    let revised = render(&root, "reloaded", vec![frame, reloaded]);
    assert!(
        count(&revised, FRAME) > 1500,
        "old references retain their pixels"
    );
    assert!(count(&revised, CONTROL) > 1500, "the edited frame is drawn");

    // Control: a `data:` picture is always allowed, so the renderer works.
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(8, 8, image::Rgba([CONTROL[0], CONTROL[1], CONTROL[2], 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let control = format!("data:image/png;base64,{}", base64(&png));

    let img = render(&root, "pictures", vec![control, avatar, sticker]);
    let (c, a, s) = (
        count(&img, CONTROL),
        count(&img, AVATAR),
        count(&img, STICKER),
    );
    println!("control pixels: {c}, avatar pixels: {a}, sticker pixels: {s}");
    assert!(c > 1500, "the data: control was not drawn ({c} px)");
    assert!(a > 1500, "avatar outside the base was not drawn ({a} px)");
    assert!(s > 1500, "sticker outside the base was not drawn ({s} px)");
    let _ = std::fs::remove_dir_all(&root);
}
