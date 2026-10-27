//! Installed skins, sample chat, stickers and link reports the shots show.

use super::*;

pub fn fixture_package(dir: &std::path::Path) {
    use nanatype_theme::{DirFiles, Meta, PackageCapability, PackageManifest, TomlTable};
    let mut table = nanatype_theme::read_manifest(&DirFiles::new(dir)).unwrap();
    let meta: Meta = table.remove("meta").unwrap().try_into().unwrap();
    let mut dependencies = std::collections::BTreeMap::new();
    if let Some(base) = &meta.base {
        let registry = SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
        dependencies.insert(
            base.clone(),
            registry.load(base).unwrap().skin.meta.version.clone(),
        );
    }
    let package = PackageManifest {
        format: 1,
        meta,
        capabilities: vec![PackageCapability::Skin],
        dependencies,
        catalogue_order: 0,
        lua: None,
        data: Default::default(),
        created_at: String::new(),
        licensed_to: String::new(),
        derived_from: String::new(),
    };
    std::fs::write(dir.join("package.toml"), package.to_toml().unwrap()).unwrap();
    let _: &TomlTable = &table;
    std::fs::write(dir.join("skin.toml"), table.to_string()).unwrap();
}

/// A user skin with pictures: render it with `snapshots <out> snap.framed`.
pub const FRAMED: &str = "snap.framed";

/// A user skin whose fonts are nowhere: the library flags it "Fallback
/// font". Its long name and tags show how a crowded row is cut.
pub const FALLBACK: &str = "snap.fallback";

pub fn fallback_skin(dir: &std::path::Path) {
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(
        dir.join("skin.toml"),
        format!(
            r#"[meta]
id = "{FALLBACK}"
version = "1.2.0"
min_app = "0.1.0"
base = "nanatype.parchment"
name = {{ zh-CN = "缺字体的超长名字手写信纸皮肤", en-US = "Handwritten Letter Paper With A Very Long Name", ja-JP = "フォントが無いとても長い名前の手紙スキン" }}
author = "snapshots"
tags = ["classic", "cute", "game"]

[fonts.body]
families = ["No Such Handwriting 7f3a", "Nor This One 7f3b"]
"#
        ),
    )
    .unwrap();
}

/// Writes the [`FRAMED`] skin: Halo with a nine-slice box frame, a framed
/// nameplate, a portrait and a two-frame continue sprite, all PNG files.
pub fn framed_skin(dir: &std::path::Path) {
    let images = dir.join("images");
    std::fs::create_dir_all(&images).unwrap();
    // 48×48 frame: 12 px dark-teal border with a notch, cream inside.
    image::RgbaImage::from_fn(48, 48, |x, y| {
        let edge = x < 12 || y < 12 || x >= 36 || y >= 36;
        let notch = !(4..44).contains(&x) && !(4..44).contains(&y);
        if notch {
            image::Rgba([0, 0, 0, 0])
        } else if edge {
            image::Rgba([22, 110, 120, 255])
        } else {
            image::Rgba([255, 248, 232, 240])
        }
    })
    .save(images.join("frame.png"))
    .unwrap();
    image::RgbaImage::from_fn(24, 24, |x, y| {
        if x < 6 || y < 6 || x >= 18 || y >= 18 {
            image::Rgba([200, 70, 40, 255])
        } else {
            image::Rgba([240, 120, 60, 255])
        }
    })
    .save(images.join("nameplate.png"))
    .unwrap();
    image::RgbaImage::from_fn(90, 110, |x, y| {
        let (dx, dy) = (x as f32 - 45.0, y as f32 - 40.0);
        if dx * dx + dy * dy < 30.0 * 30.0 || (y > 72 && (x as f32 - 45.0).abs() < 40.0) {
            image::Rgba([110, 90, 200, 255])
        } else {
            image::Rgba([0, 0, 0, 0])
        }
    })
    .save(images.join("portrait.png"))
    .unwrap();
    image::RgbaImage::from_fn(44, 22, |x, y| {
        let cx = if x < 22 { 11.0 } else { 33.0 };
        let (dx, dy) = (x as f32 - cx, y as f32 - 11.0);
        if dx.abs() + dy.abs() < 9.0 {
            image::Rgba([22, 110, 120, 255])
        } else {
            image::Rgba([0, 0, 0, 0])
        }
    })
    .save(images.join("continue.png"))
    .unwrap();
    std::fs::write(
        dir.join("skin.toml"),
        format!(
            r#"[meta]
id = "{FRAMED}"
version = "1.0.0"
min_app = "0.1.0"
base = "nanatype.halo"
name = {{ zh-CN = "贴图框", en-US = "Framed" }}
author = "snapshots"

[dialogue.box]
frame = {{ image = "images/frame.png", slice = [12, 12, 12, 12], scale = 1 }}

[dialogue.nameplate]
frame = {{ image = "images/nameplate.png", slice = [6, 6, 6, 6] }}

[dialogue.portrait]
visible = true
side = "left"
size = [90, 110]
image = "images/portrait.png"

[dialogue.continue]
image = "images/continue.png"
frames = 2
image_size = [22, 22]
"#
        ),
    )
    .unwrap();
}

/// The tray image's states (plain, mirroring, overlays hidden, locked, all
/// three) at 16, 24 and 32 px, one row per size, each cell enlarged 4× so
/// the pixels can be read.
pub fn tray_strip(out: &std::path::Path) {
    use nanatype::services::tray::{Badge, badged, image_at};
    let badges = [
        Badge::default(),
        Badge {
            mirroring: true,
            ..Badge::default()
        },
        Badge {
            hidden: true,
            ..Badge::default()
        },
        Badge {
            locked: true,
            ..Badge::default()
        },
        Badge {
            mirroring: true,
            hidden: true,
            locked: true,
        },
    ];
    let (cell, zoom) = (36u32 * 4, 4u32);
    let sizes = [16u32, 24, 32];
    let mut strip = image::RgbaImage::from_pixel(
        cell * badges.len() as u32,
        cell * sizes.len() as u32,
        image::Rgba([32, 32, 36, 255]),
    );
    for (row, px) in sizes.into_iter().enumerate() {
        let base = image_at(px).expect("icon");
        for (col, badge) in badges.into_iter().enumerate() {
            let img = badged(&base, badge);
            let big = image::imageops::resize(
                &img,
                px * zoom,
                px * zoom,
                image::imageops::FilterType::Nearest,
            );
            image::imageops::overlay(
                &mut strip,
                &big,
                i64::from(col as u32 * cell + 8),
                i64::from(row as u32 * cell + 8),
            );
        }
    }
    strip.save(out.join("tray-badges.png")).unwrap();
}

/// A few viewers with drawn avatars, and what they said. The avatars are
/// saved where the chat saver keeps them and resolved like the app does.
pub fn sample_chat(
    out: &std::path::Path,
    urls: &mut std::collections::HashMap<String, String>,
) -> Vec<ChatMessage> {
    let dir = out.join("cache").join("images");
    std::fs::create_dir_all(&dir).unwrap();
    let blobs = AssetLibrary::new(dir.clone());
    let viewers = [
        ("1001", "星野ほたる", [232u8, 106, 146], Guard::Captain),
        ("1002", "路过的猫", [61, 155, 233], Guard::None),
        ("1003", "老板大气", [247, 201, 72], Guard::Admiral),
        ("1004", "夜猫子", [123, 201, 111], Guard::None),
    ];
    for (uid, _, [r, g, b], _) in viewers {
        let img = image::RgbaImage::from_fn(64, 64, |x, y| {
            let shade = 1.0 - (x + y) as f32 / 256.0;
            let f = |c: u8| (f32::from(c) * shade) as u8;
            image::Rgba([f(r), f(g), f(b), 255])
        });
        img.save(dir.join(format!("blive-face-{uid}.png"))).unwrap();
        let asset = AssetRef::Blob(format!("blive-face-{uid}"));
        let url = blobs.url(&asset, None).expect("avatar url");
        urls.insert(asset.to_string(), url);
    }
    let message = |seq: u64, who: usize, kind: ChatKind, text: &str| {
        let (uid, name, _, guard) = viewers[who];
        ChatMessage {
            platform: nanatype::chat::Platform::Blivechat,
            epoch: 0,
            room: String::new(),
            seq,
            id: seq.to_string(),
            kind,
            user: ChatUser {
                uid: uid.into(),
                name: name.into(),
                avatar: Some(AssetRef::Blob(format!("blive-face-{uid}"))),
            },
            guard,
            owner: false,
            admin: false,
            medal_level: if guard == Guard::None { 0 } else { 12 },
            medal_name: if guard == Guard::None {
                String::new()
            } else {
                "奈奈团".into()
            },
            doc: Document::plain(text),
            at: 1_760_000_000_000 + seq * 7_000,
        }
    };
    vec![
        message(1, 0, ChatKind::Text, "主播晚上好！今天打什么字呀"),
        message(2, 1, ChatKind::Text, "来了来了"),
        message(
            3,
            2,
            ChatKind::SuperChat { price: 50 },
            "今天也辛苦啦，早点休息",
        ),
        message(
            4,
            3,
            ChatKind::Gift {
                name: "小花花".into(),
                num: 5,
                coin: 500,
            },
            "",
        ),
        message(
            5,
            0,
            ChatKind::Member {
                num: 1,
                unit: "月".into(),
                coin: 198_000,
            },
            "",
        ),
        message(6, 1, ChatKind::Text, "这个字幕效果好可爱"),
    ]
}

/// A sticker folder outside the data folder, scanned and resolved like the
/// app does (Settings › Assets › sticker folders).
pub fn sample_stickers(
    out: &std::path::Path,
    urls: &mut std::collections::HashMap<String, String>,
) -> Vec<StickerRow> {
    let dir = out.join("stickers").join("nana");
    std::fs::create_dir_all(&dir).unwrap();
    let shapes: [(&str, [u8; 3]); 4] = [
        ("heart", [232, 80, 120]),
        ("star", [247, 201, 72]),
        ("leaf", [92, 184, 92]),
        ("drop", [61, 155, 233]),
    ];
    for (i, (name, [r, g, b])) in shapes.into_iter().enumerate() {
        let img = image::RgbaImage::from_fn(64, 64, |x, y| {
            let (dx, dy) = (x as f32 - 31.5, y as f32 - 31.5);
            let inside = match i {
                0 => (dx.abs() + dy.abs()) < 28.0,
                1 => dx.abs().max(dy.abs()) < 24.0,
                2 => (dx * dx + dy * dy).sqrt() < 28.0,
                _ => (dx * dx / 2.0 + dy * dy).sqrt() < 22.0,
            };
            if inside {
                image::Rgba([r, g, b, 255])
            } else {
                image::Rgba([0, 0, 0, 0])
            }
        });
        img.save(dir.join(format!("{name}.png"))).unwrap();
    }
    let mut lib = AssetLibrary::new(out.join("cache").join("images"));
    lib.scan(&[dir.display().to_string()]);
    lib.stickers
        .iter()
        .filter_map(|s| {
            let url = s.url()?;
            urls.insert(s.asset().to_string(), url.clone());
            Some(StickerRow {
                asset: s.asset().to_string(),
                name: s.name.clone(),
                lib: s.lib.clone(),
                url,
                height_em: 1.6,
                pack_title: s.lib.clone(),
            })
        })
        .collect()
}

/// The settings pages' link states after these reports from the clients,
/// in `locale`, the way the app turns them into text.
pub fn reported(
    locale: Locale,
    bridge: &[BridgeStatus],
    [display, history]: [WindowOutputStatus; 2],
) -> Status {
    let mut s = Status::default();
    for report in bridge {
        if let Some((sink, state)) = link_text::bridge(locale, report) {
            match sink {
                Sink::Vts => s.vts = state,
                Sink::Osc => s.osc = state,
                Sink::WebSocket => s.ws = state,
            }
        }
    }
    s.spout_display = link_text::spout(locale, &display).unwrap_or_default();
    s.spout_history = link_text::spout(locale, &history).unwrap_or_default();
    s
}

/// A fixture never shows a switch on beside an "Off" badge, or the other
/// way round: each link's state says off exactly when its switch is off.
pub fn assert_consistent(settings: &Settings, status: &Status) {
    let i = &settings.integrations;
    let links = [
        ("VTube Studio", i.vts.enabled, status.vts != LinkState::Off),
        ("OSC", i.osc.enabled, status.osc != LinkState::Off),
        (
            "WebSocket",
            i.websocket.enabled,
            status.ws != LinkState::Off,
        ),
        (
            "blivechat",
            i.blivechat.enabled,
            status.blivechat_link != danmaku::Link::Off,
        ),
        (
            "Spout display",
            settings.output.display.enabled,
            status.spout_display != LinkState::Off,
        ),
        (
            "Spout history",
            settings.output.history.enabled,
            status.spout_history != LinkState::Off,
        ),
    ];
    for (name, enabled, live) in links {
        assert_eq!(enabled, live, "{name}: switch {enabled}, state live {live}");
    }
}
