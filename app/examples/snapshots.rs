//! Render every window's view offscreen to PNG (no desktop windows).
//!
//! `cargo run -p nanatype --example snapshots -- <out-dir> [skin-id|id,id|all] [locale]`
//!
//! With `all`, every built-in skin is rendered into `<out-dir>/<skin-id>/`.
//! `NANATYPE_THEME_SNAPSHOTS_ONLY=1` captures dialogue, input, history and
//! the skin library. `NANATYPE_THEME_AUDIT=1` adds motion cycles, a partner
//! track, reduced-motion poses and long-name/narrow-region fixtures.
//! `NANATYPE_SNAPSHOT_SKINS_DIR` loads saved `id/skin.toml` manifests for
//! comparisons of built-in skins (self-contained manifests, no inheritance).
//! `NANATYPE_SNAPSHOT_APPEARANCE=dark` selects neutral dark application chrome.
//! `NANATYPE_SNAPSHOT_SCALE` captures at a physical pixel scale (default 1).
//! `NANATYPE_PREVIEW_SCALE_AUDIT=1` captures tall/maximum logical canvases
//! inside the settings preview, without allocating full-size desktop windows.
//! `NANATYPE_RESPONSIVE_AUDIT=1` reuses native windows across both writing modes,
//! static/effect/reduced states, and captures the matching settings previews.
//! `NANATYPE_SNAPSHOT_MOTION_FRAMES=128` captures one complete artwork cycle
//! in a single retained scene, checking the reading area, alpha and GPU caches.

use std::path::PathBuf;
use std::sync::Arc;

use nana_ui::WindowOutputStatus;
use nana_ui::runtime::{DocumentId, RuntimeDocument};
use nana_ui_devtools::agent::{AgentSession, RuntimeAgentSession};
use nanatype::assets::AssetLibrary;
use nanatype::chat::{ChatKind, ChatMessage};
use nanatype::ui::{
    LinkState, Status, StickerRow, Ui, danmaku, dialogue, history, input, link_text, settings,
};
use nanatype_bilibili::{ConnectionState, Guard};
use nanatype_bridge::{BridgeStatus, Sink};
use nanatype_core::{Cmd, Core};
use nanatype_i18n::Locale;
use nanatype_markup::parse;
use nanatype_model::{AssetRef, ChatUser, Document};
use nanatype_settings::Settings;
use nanatype_theme::SkinRegistry;

fn shot(
    name: &str,
    out: &std::path::Path,
    w: u32,
    h: u32,
    theme: Arc<nana_ui::CompiledTheme>,
    mount: impl FnOnce(&mut RuntimeDocument, DocumentId),
) {
    if let Ok(views) = std::env::var("NANATYPE_SNAPSHOT_VIEWS")
        && !views.split(',').any(|view| view.trim() == name)
    {
        return;
    }
    let at = nanatype::clock::now_ms();
    nanatype::clock::capture_at(at.saturating_sub(1000), || {
        let id = DocumentId::new(1).unwrap();
        let mut doc = RuntimeDocument::new(id);
        let _ = doc.context_mut().set_theme_tokens(theme);
        mount(&mut doc, id);
        let scale = std::env::var("NANATYPE_SNAPSHOT_SCALE")
            .ok()
            .map(|s| s.parse::<f32>().expect("snapshot scale is a number"))
            .unwrap_or(1.0);
        let mut session = RuntimeAgentSession::new_scaled(doc, w, h, scale).expect("session");
        session.set_gpu_renderers(nanatype::ui::sweep::renderers());
        if std::env::var_os("NANATYPE_SNAPSHOT_TRANSPARENT").is_some() {
            session.set_clear(Some([0.0; 4]));
        }
        session.flush().expect("initial snapshot layout");
        session
            .document_mut()
            .context_mut()
            .advance_animations(std::time::Duration::from_secs(1));
        session
            .document_mut()
            .sync_presentation_clock(std::time::Duration::from_secs(1));
        let path = out.join(format!("{name}.png"));
        // Pipeline creation is asynchronous in production. Warm one GPU draw
        // before the captured frame, without advancing the deterministic clock.
        nanatype::clock::capture_at(at, || {
            let _ = session.screenshot_rgba().expect("shader warmup");
            let start = std::time::Instant::now();
            while nanatype::ui::shader_effects::cache_stats().pending > 0
                && start.elapsed().as_secs() < 10
            {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            assert_eq!(
                nanatype::ui::shader_effects::cache_stats().pending,
                0,
                "shader warmup timed out"
            );
        });
        nanatype::clock::capture_at(at, || match session.screenshot_png(&path) {
            Ok(stats) => println!("{name}: {stats:?}"),
            Err(e) => panic!("{name}: {e:?}"),
        });
    });
}

fn main() {
    // Start the session clock now: the lines below are sent at 0 ms, and the
    // render later must see their reveal (and fade-in) as finished.
    let _ = nanatype::clock::now_ms();
    let mut args = std::env::args().skip(1);
    let out = PathBuf::from(args.next().unwrap_or_else(|| "snapshots".into()));
    std::fs::create_dir_all(&out).unwrap();
    // The app's own start-up: `out/data` stands in for the data folder and
    // becomes NanaUI's image base. Avatars (`out/cache/images`) and stickers
    // (`out/stickers`) live outside it, like they do on a real install.
    nanatype::services::media::init(&out.join("data")).expect("media mirror");
    let skin_id = args.next().unwrap_or_else(|| "nanatype.halo".into());
    let locale = args
        .next()
        .and_then(|l| Locale::from_tag(&l))
        .unwrap_or(Locale::ZhCn);
    tray_strip(&out);
    // Installed skins live in the data folder, like the app's.
    let skins = out.join("data").join("skins");
    framed_skin(&skins.join(FRAMED));
    fallback_skin(&skins.join(FALLBACK));
    let registry = SkinRegistry::new(Some(skins), env!("CARGO_PKG_VERSION"));
    if skin_id == "all" || skin_id.contains(',') {
        let ids: Vec<String> = if skin_id == "all" {
            registry.entries().iter().map(|e| e.id.clone()).collect()
        } else {
            skin_id.split(',').map(|id| id.trim().to_owned()).collect()
        };
        for id in ids {
            render(&registry, &out.join(&id), &id, locale);
        }
    } else {
        render(&registry, &out, &skin_id, locale);
    }
}

/// A user skin with pictures: render it with `snapshots <out> snap.framed`.
const FRAMED: &str = "snap.framed";

/// A user skin whose fonts are nowhere: the library flags it "Fallback
/// font". Its long name and tags show how a crowded row is cut.
const FALLBACK: &str = "snap.fallback";

fn fallback_skin(dir: &std::path::Path) {
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
fn framed_skin(dir: &std::path::Path) {
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
fn tray_strip(out: &std::path::Path) {
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
fn sample_chat(
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
        img.save(dir.join(format!("bili-face-{uid}.png"))).unwrap();
        let asset = AssetRef::Blob(format!("bili-face-{uid}"));
        let url = blobs.url(&asset, None).expect("avatar url");
        urls.insert(asset.to_string(), url);
    }
    let message = |seq: u64, who: usize, kind: ChatKind, text: &str| {
        let (uid, name, _, guard) = viewers[who];
        ChatMessage {
            platform: nanatype::chat::Platform::Bilibili,
            epoch: 0,
            room: String::new(),
            seq,
            id: seq.to_string(),
            kind,
            user: ChatUser {
                uid: uid.into(),
                name: name.into(),
                avatar: Some(AssetRef::Blob(format!("bili-face-{uid}"))),
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
fn sample_stickers(
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
fn reported(
    locale: Locale,
    bridge: &[BridgeStatus],
    [display, history]: [WindowOutputStatus; 2],
    bilibili: &ConnectionState,
    room: &str,
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
    (s.bilibili, s.bilibili_link) = link_text::bilibili(locale, bilibili, room);
    s
}

/// A fixture never shows a switch on beside an "Off" badge, or the other
/// way round: each link's state says off exactly when its switch is off.
fn assert_consistent(settings: &Settings, status: &Status) {
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
            "Bilibili",
            i.bilibili.enabled,
            status.bilibili_link != danmaku::Link::Off,
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

fn render(registry: &SkinRegistry, out: &std::path::Path, skin_id: &str, locale: Locale) {
    let at = std::env::var("NANATYPE_SNAPSHOT_TIME_MS")
        .ok()
        .map(|s| s.parse::<u64>().expect("snapshot time in milliseconds"))
        .unwrap_or(100_000);
    nanatype::clock::capture_at(at, || render_at(registry, out, skin_id, locale));
}
fn render_at(registry: &SkinRegistry, out: &std::path::Path, skin_id: &str, locale: Locale) {
    std::fs::create_dir_all(out).unwrap();
    // A saved manifest set makes before/after comparisons use identical
    // fonts, sample content and rendering rather than stale screenshots.
    let loaded = match std::env::var_os("NANATYPE_SNAPSHOT_SKINS_DIR") {
        Some(dir) if skin_id.starts_with("nanatype.") => {
            let folder = PathBuf::from(dir).join(skin_id);
            nanatype_theme::load(
                Arc::new(nanatype_theme::DirFiles::new(folder.clone())),
                nanatype_theme::SkinOrigin::Directory(folder),
                &(),
                env!("CARGO_PKG_VERSION"),
            )
            .expect("snapshot manifest")
        }
        _ => registry.load(skin_id).expect("skin"),
    };
    // The skin's built-in fonts, registered as the app does; kept alive
    // until every shot is taken. None without the `builtin-fonts` feature.
    let mut builtin = nanatype::skin_fonts::BuiltinFontSet::default();
    let f = &loaded.skin.fonts;
    let roles = [&f.body, &f.name, &f.accent];
    let builtin_ref = |s: &&nanatype_theme::FontSpec| {
        s.file
            .clone()
            .filter(|p| nanatype_theme::is_builtin_font(p))
    };
    let refs: Vec<String> = roles
        .iter()
        .copied()
        .chain(f.latin.as_ref())
        .filter_map(|s| builtin_ref(&s))
        .collect();
    for e in builtin.sync(
        refs.iter().map(String::as_str),
        nanatype_theme::builtin_font,
        nanatype::skin_fonts::register_with_nana_ui,
    ) {
        println!("font: {e}");
    }
    let families = roles.map(|s| builtin_ref(&s).and_then(|r| builtin.family(&r)));
    let mut skin = Arc::new(nanatype::skin_theme::prepare(
        &loaded.skin,
        &Default::default(),
        &families,
        locale.tag(),
    ));
    if std::env::var_os("NANATYPE_SNAPSHOT_SWEEP").is_some() {
        Arc::make_mut(&mut skin).dialogue.sweep.enabled = true;
    }
    let mut settings = Settings::default();
    settings.general.speaker_name = "奈奈".into();
    settings.skin.id = skin_id.to_owned();
    settings.history.show_time = true;
    settings.history.chat.enabled = true;
    settings.integrations.bilibili.enabled = true;
    settings.integrations.bilibili.room = "21452505".into();
    let dark = std::env::var("NANATYPE_SNAPSHOT_APPEARANCE").is_ok_and(|v| v == "dark");
    settings.skin.chrome = if dark {
        nanatype_settings::ChromeMode::Dark
    } else {
        nanatype_settings::ChromeMode::Light
    };
    if std::env::var_os("NANATYPE_SNAPSHOT_MATERIALS_OFF").is_some() {
        settings.skin.advanced_effects = false;
    }
    let theme = nanatype::skin_theme::compile(dark);
    let ui = Ui::new(Arc::new(settings.clone()), skin.clone(), locale);
    if std::env::var_os("NANATYPE_SNAPSHOT_STATIC").is_some() {
        ui.reduced_motion.set(true);
    }
    ui.skins
        .set(Arc::new(nanatype::ui::skin_rows(registry, locale)));
    let mut urls = nanatype::assets::skin_preview_urls(&loaded.skin, &*loaded.files);
    let chat = sample_chat(out, &mut urls);
    let stickers = sample_stickers(out, &mut urls);
    assert_eq!(stickers.len(), 4, "every sample sticker resolves");
    ui.stickers.set(Arc::new(stickers));
    ui.asset_urls.set(Arc::new(urls));
    // The portrait the way `App::refresh_assets` finds it.
    if let (Some(root), Some(img)) = (loaded.files.root(), &skin.dialogue.portrait.image)
        && let Some(url) = nanatype::assets::skin_file_url(root, img)
    {
        ui.portraits.set(Arc::new(
            [("default".to_owned(), url)].into_iter().collect(),
        ));
    }

    let mut core = Core::new(settings.clone(), skin.clone());
    for line in [
        "大家好！今天也来[b]打字[/b]啦～",
        "[color=#e8505b]红色[/color]、[wave]波浪[/wave]和[ruby=かんじ]漢字[/ruby]注音。[stroke=#e86a92,3][color=#ffffff]描边文字[/color][/stroke][shadow=#0876d0,0,0,8]发光[/shadow]",
    ] {
        core.handle(Cmd::SendMarkup(line.into()), 0);
        core.handle(Cmd::Skip, 1);
    }
    // Live chat joins the history; the last line answers a viewer.
    for m in &chat {
        core.handle(Cmd::External(m.history_item(locale)), 0);
    }
    core.handle(Cmd::SetReply(Some(chat[0].quote(locale))), 0);
    core.handle(
        Cmd::DraftChanged(
            parse("[at=1001 name=星野ほたる avatar=blob:bili-face-1001] 晚上好！今天也谢谢你来～")
                .doc,
        ),
        0,
    );
    core.handle(Cmd::Send, 0);
    core.handle(Cmd::Skip, 1);
    if let Ok(line) = std::env::var("NANATYPE_SNAPSHOT_MARKUP") {
        core.handle(Cmd::SendMarkup(line), 0);
        core.handle(Cmd::Skip, 1);
    }
    ui.chat
        .set(Arc::new(chat.iter().cloned().map(Arc::new).collect()));
    ui.reply.set(Some(chat[1].quote(locale)));
    let mut status = (*ui.status.get_untracked()).clone();
    (status.bilibili, status.bilibili_link) = link_text::bilibili(
        locale,
        &ConnectionState::Connected(nanatype_bilibili::Room {
            id: 21452505,
            owner: 1,
            title: String::new(),
            max_chars: 20,
        }),
        "21452505",
    );
    assert_consistent(&settings, &status);
    ui.status.set(Arc::new(status));

    let draft = "[at=1002 name=路过的猫 avatar=blob:bili-face-1002] [b]粗体[/b]和[shake]抖动[/shake]，[wait=500]然后[pause]继续。[sticker=nana/heart][se=coin]";
    let parsed = parse(draft);
    ui.draft.set(Arc::new(parsed.doc.clone()));
    let look = dialogue::Base::editor(&skin).look();
    ui.editor_value.set(nanatype::ui::rich::to_rich(
        &parsed.doc,
        &nanatype::ui::rich::Env {
            look: &look,
            urls: &ui.asset_urls.get_untracked(),
            editor: true,
            locale,
        },
    ));
    let mut src = ui.source.get_untracked();
    src.text = draft.into();
    ui.source.set(src);
    ui.display.set(Arc::new(core.display().clone()));
    ui.history
        .set(Arc::new(core.history().iter().cloned().collect()));
    ui.now.set(nanatype::clock::now_ms());
    ui.peer_now.set(nanatype::clock::now_ms());
    ui.history_now.set(nanatype::clock::now_ms());

    let d = &skin.dialogue;
    let (w, h) = (
        (d.region[0] + d.overscan * 2.0) as u32,
        (d.region[1] + d.overscan * 2.0) as u32,
    );
    if std::env::var_os("NANATYPE_RESPONSIVE_AUDIT").is_some() {
        responsive_audit(ui, &skin, out, theme);
        return;
    }
    if std::env::var_os("NANATYPE_PREVIEW_SCALE_AUDIT").is_some() {
        preview_scale_audit(ui, out, theme);
        return;
    }
    if std::env::var_os("NANATYPE_RESIZE_AUDIT").is_some() {
        resize_audit(ui, skin.clone(), out, theme);
        return;
    }
    if std::env::var_os("NANATYPE_TRANSITION_AUDIT").is_some() {
        transition_audit(ui, out, w, h, theme);
        return;
    }
    if let Ok(frames) = std::env::var("NANATYPE_SNAPSHOT_MOTION_FRAMES") {
        motion_capture(
            ui,
            out,
            w,
            h,
            theme,
            frames.parse().expect("motion frame count"),
        );
        return;
    }
    shot("display", out, w, h, theme.clone(), |doc, id| {
        doc.context_mut()
            .mount_view_root(id, move || {
                dialogue::dialogue_box(ui, dialogue::Source::Display, None)
            })
            .unwrap();
    });
    let (iw, ih) = (
        nanatype::windows::INPUT_SIZE.0 as u32,
        nanatype::windows::INPUT_SIZE.1 as u32,
    );
    shot("input", out, iw, ih, theme.clone(), |doc, id| {
        doc.context_mut()
            .mount_view_root(id, move || input::input_window(ui))
            .unwrap();
    });
    // Focused theme review avoids rendering unrelated onboarding and link
    // settings for every skin. The usual full capture stays available.
    if std::env::var_os("NANATYPE_THEME_SNAPSHOTS_ONLY").is_some() {
        if std::env::var_os("NANATYPE_PAINTER_AUDIT").is_some() {
            let surface = nanatype::ui::surface::Surface::dialogue_box(&skin, None);
            let [mt, mr, mb, ml] = d.box_.margin;
            let overscan = d.overscan;
            let fraction = d.box_.width_fraction;
            shot("shape-only", out, w, h, theme.clone(), |doc, id| {
                doc.context_mut()
                    .mount_view_root(id, move || {
                        use nana_ui::runtime::Stack;
                        use nana_ui::runtime::view::widget;
                        use nana_ui_core::{AlignSpec, LengthSpec};
                        widget(Stack::fill_column(0.0).with_layout(move |l| {
                            l.padding_left = Some(LengthSpec::Px(overscan + ml));
                            l.padding_right = Some(LengthSpec::Px(overscan + mr));
                            l.padding_top = Some(LengthSpec::Px(overscan + mt));
                            l.padding_bottom = Some(LengthSpec::Px(overscan + mb));
                            l.align_items = AlignSpec::Center;
                        }))
                        .children(widget(surface.stack(
                            Stack::fill_column(0.0).width(LengthSpec::Percent(fraction * 100.0)),
                        )))
                    })
                    .unwrap();
            });
        }
        shot(
            "history",
            out,
            skin.history.width as u32,
            skin.history.height as u32,
            theme.clone(),
            |doc, id| {
                doc.context_mut()
                    .mount_view_root(id, move || history::history_window(ui))
                    .unwrap();
            },
        );
        ui.page.set(settings::Page::Skins);
        shot(
            "settings-Skins",
            out,
            920,
            1700,
            theme.clone(),
            |doc, id| {
                doc.context_mut()
                    .mount_view_root(id, move || settings::settings_window(ui))
                    .unwrap();
            },
        );
        if std::env::var_os("NANATYPE_MATERIAL_AUDIT").is_some() && !skin.materials.is_empty() {
            material_audit(ui, out, w, h, theme.clone());
        }
        if std::env::var_os("NANATYPE_THEME_AUDIT").is_some() {
            theme_audit(ui, out, w, h, ih, theme);
        }
        return;
    }
    let before_dy_settings = ui.settings.get_untracked();
    let before_dy_chat = ui.chat.get_untracked();
    let before_dy_state = ui.douyin.get_untracked();
    let before_dy_page = ui.page.get_untracked();
    let mut dy_settings = (*before_dy_settings).clone();
    dy_settings.integrations.douyin.enabled = true;
    dy_settings.integrations.douyin.room = "123456789".into();
    ui.settings.set(Arc::new(dy_settings));
    ui.douyin.set(Arc::new(nanatype::app::douyin::State {
        account: nanatype_douyin::AccountState::SignedOut,
        connection: nanatype_douyin::ConnectionState::Connected {
            room: "123456789".into(),
        },
        warning: None,
    }));
    ui.page.set(settings::Page::Douyin);
    shot("dy-settings", out, 920, 1050, theme.clone(), |doc, id| {
        doc.context_mut()
            .mount_view_root(id, move || settings::settings_window(ui))
            .unwrap();
    });
    let mut mixed = (*before_dy_chat).clone();
    for (n, content) in [
        nanatype_douyin::Content::Text("抖音观众：你好！Hello from Douyin 👋".into()),
        nanatype_douyin::Content::Gift {
            name: "小心心".into(),
            count: 3,
            diamonds: Some(1),
        },
    ]
    .into_iter()
    .enumerate()
    {
        let mut message = ChatMessage::from_douyin(
            nanatype_douyin::ChatEvent {
                id: format!("dy-{n}"),
                author: nanatype_douyin::Author {
                    uid: "42".into(),
                    name: "抖音观众".into(),
                    avatar: String::new(),
                },
                content,
                at: nanatype_model::unix_millis() + n as u64,
            },
            None,
            None,
            9000 + n as u64,
        );
        message.room = "123456789".into();
        mixed.push(Arc::new(message));
    }
    ui.chat.set(Arc::new(mixed));
    shot("dy-chat", out, 620, 700, theme.clone(), |doc, id| {
        doc.context_mut()
            .mount_view_root(id, move || danmaku::chat_window(ui))
            .unwrap();
    });
    ui.chat.set(before_dy_chat);
    ui.settings.set(before_dy_settings);
    ui.douyin.set(before_dy_state);
    ui.page.set(before_dy_page);
    ui.peer_panel.set(true);
    shot("p2p-create", out, iw, ih, theme.clone(), |doc, id| {
        doc.context_mut()
            .mount_view_root(id, move || input::input_window(ui))
            .unwrap();
    });
    let mut peer_settings = settings.clone();
    peer_settings.general.speaker_name = "联机伙伴 / Partner".into();
    let mut peer_core = Core::new(peer_settings, skin.clone());
    peer_core.handle(
        Cmd::SendMarkup(
            "[color=#689fd7]我这边也能看到你的台词！[/color]\n两个人可以同时显示，也可以独立继续。"
                .into(),
        ),
        0,
    );
    peer_core.handle(Cmd::Skip, 1);
    ui.peer_display.set(Arc::new(peer_core.display().clone()));
    ui.peer.set(Arc::new(nanatype::p2p::SessionUi {
        active: true,
        host: true,
        invitation: "nanatype1:preview".into(),
        name: "联机伙伴".into(),
        status: nanatype_p2p::Status::Connected {
            route: "direct".into(),
            rtt_ms: 28,
        },
    }));
    shot("p2p-input", out, iw, ih, theme.clone(), |doc, id| {
        doc.context_mut()
            .mount_view_root(id, move || input::input_window(ui))
            .unwrap();
    });
    shot("p2p-local-display", out, w, h, theme.clone(), |doc, id| {
        doc.context_mut()
            .mount_view_root(id, move || dialogue::display_window(ui))
            .unwrap();
    });
    shot(
        "p2p-partner-display",
        out,
        w,
        h,
        theme.clone(),
        |doc, id| {
            doc.context_mut()
                .mount_view_root(id, move || dialogue::peer_window(ui))
                .unwrap();
        },
    );
    for page in [settings::Page::Windows, settings::Page::Output] {
        ui.page.set(page);
        shot(
            &format!("p2p-settings-{page:?}"),
            out,
            920,
            900,
            theme.clone(),
            |doc, id| {
                doc.context_mut()
                    .mount_view_root(id, move || settings::settings_window(ui))
                    .unwrap();
            },
        );
    }
    ui.page.set(settings::Page::Windows);
    shot(
        "p2p-settings-Windows-narrow",
        out,
        720,
        660,
        theme.clone(),
        |doc, id| {
            doc.context_mut()
                .mount_view_root(id, move || settings::settings_window(ui))
                .unwrap();
        },
    );
    let before_output = ui.settings.get_untracked();
    let mut enabled_output = (*before_output).clone();
    enabled_output.output.display.enabled = true;
    enabled_output.output.peer_display.enabled = true;
    enabled_output.output.history.enabled = true;
    ui.settings.set(Arc::new(enabled_output));
    let before_output_status = ui.status.get_untracked();
    let mut output_status = (*before_output_status).clone();
    output_status.spout_display = LinkState::Connecting;
    output_status.spout_peer_display = LinkState::Connecting;
    output_status.spout_history = LinkState::Connecting;
    ui.status.set(Arc::new(output_status));
    ui.page.set(settings::Page::Output);
    shot(
        "p2p-settings-Output-enabled",
        out,
        920,
        1400,
        theme.clone(),
        |doc, id| {
            doc.context_mut()
                .mount_view_root(id, move || settings::settings_window(ui))
                .unwrap();
        },
    );
    ui.settings.set(before_output);
    ui.status.set(before_output_status);
    ui.peer.set(Arc::new(nanatype::p2p::SessionUi::default()));
    ui.peer_panel.set(false);
    if std::env::var_os("NANATYPE_P2P_SNAPSHOTS_ONLY").is_some() {
        return;
    }
    // The toolbar's `?` sheet as its popover shows it.
    shot(
        "input-cheatsheet",
        out,
        620,
        250,
        theme.clone(),
        |doc, id| {
            doc.context_mut()
                .mount_view_root(id, move || {
                    nana_ui::runtime::view::widget(
                        nana_ui::runtime::Stack::column(0.0)
                            .padding(8.0)
                            .surface(nana_ui_core::SemanticColorRole::Surface),
                    )
                    .children(input::cheat_sheet(ui))
                })
                .unwrap();
        },
    );
    // The toolbar's sticker panel, listing the sticker folder.
    shot("input-stickers", out, 380, 200, theme.clone(), |doc, id| {
        doc.context_mut()
            .mount_view_root(id, move || {
                nana_ui::runtime::view::widget(
                    nana_ui::runtime::Stack::column(0.0)
                        .padding(8.0)
                        .surface(nana_ui_core::SemanticColorRole::Surface),
                )
                .children(input::sticker_panel(
                    ui,
                    nana_ui::runtime::view::signal(true),
                ))
            })
            .unwrap();
    });
    shot(
        "history",
        out,
        skin.history.width as u32,
        skin.history.height as u32,
        theme.clone(),
        |doc, id| {
            doc.context_mut()
                .mount_view_root(id, move || history::history_window(ui))
                .unwrap();
        },
    );
    // The newest rows alone: the quoted chat message and the reply that
    // quotes it and @-mentions the viewer, unclipped at the list's foot.
    // The box has cleared, so skins that hide the line still on it (the
    // echo_* logs, `hide_latest`) list the reply too.
    let all_rows = ui.history.get_untracked();
    ui.history.set(Arc::new(
        all_rows[all_rows.len().saturating_sub(3)..].to_vec(),
    ));
    let on_screen = ui.display.get_untracked();
    ui.display.set(Arc::new(nanatype_core::DisplayView::empty(
        nanatype_model::Entrance {
            kind: nanatype_model::EntranceKind::None,
            ms: 0,
        },
    )));
    shot(
        "history-reply",
        out,
        skin.history.width as u32,
        skin.history.height as u32,
        theme.clone(),
        |doc, id| {
            doc.context_mut()
                .mount_view_root(id, move || history::history_window(ui))
                .unwrap();
        },
    );
    ui.history.set(all_rows);
    ui.display.set(on_screen);
    shot("chat", out, 400, 640, theme.clone(), |doc, id| {
        doc.context_mut()
            .mount_view_root(id, move || danmaku::chat_window(ui))
            .unwrap();
    });
    let mut card_settings = (*ui.settings.get_untracked()).clone();
    card_settings.integrations.bilibili.panel_style = nanatype_settings::ChatPanelStyle::History;
    ui.settings.set(Arc::new(card_settings));
    shot("chat-cards", out, 400, 640, theme.clone(), |doc, id| {
        doc.context_mut()
            .mount_view_root(id, move || danmaku::chat_window(ui))
            .unwrap();
    });
    ui.settings.set(Arc::new(settings.clone()));
    for page in settings::Page::ALL {
        ui.page.set(page);
        shot(
            &format!("settings-{page:?}"),
            out,
            920,
            660,
            theme.clone(),
            |doc, id| {
                doc.context_mut()
                    .mount_view_root(id, move || settings::settings_window(ui))
                    .unwrap();
            },
        );
    }
    // The first-run guide, one shot per step.
    ui.page.set(settings::Page::General);
    for n in 0..4u8 {
        ui.onboard_step.set(n);
        shot(
            &format!("settings-General-onboard-{}", n + 1),
            out,
            920,
            760,
            theme.clone(),
            |doc, id| {
                doc.context_mut()
                    .mount_view_root(id, move || settings::settings_window(ui))
                    .unwrap();
            },
        );
    }
    ui.onboard_step.set(0);

    // About's "Open-source fonts" panel, as it opens under its row.
    shot(
        "settings-About-fonts",
        out,
        760,
        380,
        theme.clone(),
        |doc, id| {
            doc.context_mut()
                .mount_view_root(id, move || {
                    nana_ui::runtime::view::widget(
                        nana_ui::runtime::Stack::column(0.0)
                            .surface(nana_ui_core::SemanticColorRole::Surface),
                    )
                    .children(settings::font_licence_panel(ui))
                })
                .unwrap();
        },
    );

    // Pages taller than the window, whole: the skin grid and the history
    // page's last row.
    for (page, h) in [
        (settings::Page::Skins, 1700),
        (settings::Page::History, 1100),
        (settings::Page::About, 1600),
    ] {
        ui.page.set(page);
        shot(
            &format!("settings-{page:?}-full"),
            out,
            920,
            h,
            theme.clone(),
            |doc, id| {
                doc.context_mut()
                    .mount_view_root(id, move || settings::settings_window(ui))
                    .unwrap();
            },
        );
    }
    // The skin library as a list (the view switch's other state), in the
    // window and whole.
    ui.page.set(settings::Page::Skins);
    ui.skins_grid.set(false);
    for (name, h) in [
        ("settings-Skins-list", 660),
        ("settings-Skins-list-full", 1900),
    ] {
        shot(name, out, 920, h, theme.clone(), |doc, id| {
            doc.context_mut()
                .mount_view_root(id, move || settings::settings_window(ui))
                .unwrap();
        });
    }
    ui.skins_grid.set(true);

    // An empty draft shows the editor's placeholder.
    ui.draft.set(Arc::new(Document::new()));
    ui.editor_value.set(nana_ui_core::RichText::new(""));
    ui.reply.set(None);
    shot("input-empty", out, iw, ih, theme.clone(), |doc, id| {
        doc.context_mut()
            .mount_view_root(id, move || input::input_window(ui))
            .unwrap();
    });

    // The same two pages with every link switched on, one in each state.
    // The states come from the clients' own reports, turned into text the
    // way the app does, so the pages read in the shot's language.
    let mut on = settings.clone();
    on.integrations.vts.enabled = true;
    on.integrations.osc.enabled = true;
    on.integrations.websocket.enabled = true;
    on.output.display.enabled = true;
    on.output.history.enabled = true;
    let status = reported(
        locale,
        &[
            BridgeStatus::VtsNeedsToken,
            BridgeStatus::Connecting(Sink::Osc),
            BridgeStatus::Connected(Sink::WebSocket),
            BridgeStatus::Clients(2),
        ],
        [
            WindowOutputStatus::Active {
                extent: [1920, 1080],
                native: true,
            },
            WindowOutputStatus::NativeUnavailable {
                reason: "no GPU".into(),
            },
        ],
        &ConnectionState::Error("connection refused".into()),
        &on.integrations.bilibili.room,
    );
    assert_consistent(&on, &status);
    ui.settings.set(Arc::new(on));
    ui.status.set(Arc::new(status));
    for page in [settings::Page::Integrations, settings::Page::Output] {
        ui.page.set(page);
        shot(
            &format!("settings-{page:?}-on"),
            out,
            920,
            1100,
            theme.clone(),
            |doc, id| {
                doc.context_mut()
                    .mount_view_root(id, move || settings::settings_window(ui))
                    .unwrap();
            },
        );
    }

    // VTube Studio authorised with a model's hotkeys listed (one mapped by
    // hand), and quick phrases with a hotkey.
    let mut on = (*ui.settings.get_untracked()).clone();
    on.integrations.bilibili.enabled = false;
    on.integrations.vts.token = Some("token".into());
    on.integrations
        .vts
        .expressions
        .insert("blush".into(), "hk-3".into());
    on.assets.phrases = vec![
        nanatype_settings::QuickPhrase {
            label: "开场".into(),
            markup: "大家好，我是[b]{speaker}[/b]！".into(),
            send: true,
            hotkey: Some(nanatype_settings::Hotkey {
                alt: true,
                ..nanatype_settings::Hotkey::ctrl("Digit1")
            }),
        },
        nanatype_settings::QuickPhrase {
            label: "感谢".into(),
            markup: "谢谢[wave]大家[/wave]～".into(),
            send: false,
            hotkey: None,
        },
    ];
    // The other links stay on, and say so.
    let mut status = reported(
        locale,
        &[
            BridgeStatus::Connected(Sink::Vts),
            BridgeStatus::Connected(Sink::Osc),
            BridgeStatus::Connected(Sink::WebSocket),
            BridgeStatus::Clients(1),
        ],
        [
            WindowOutputStatus::Active {
                extent: [1920, 1080],
                native: true,
            },
            WindowOutputStatus::Active {
                extent: [420, 560],
                native: true,
            },
        ],
        &ConnectionState::Off,
        &on.integrations.bilibili.room,
    );
    status.vts_hotkeys = vec![
        ("hk-1".into(), "Smile".into()),
        ("hk-2".into(), "Angry Face".into()),
        ("hk-3".into(), "Heart Eyes".into()),
    ];
    assert_consistent(&on, &status);
    ui.settings.set(Arc::new(on));
    ui.status.set(Arc::new(status));
    for page in [settings::Page::Integrations, settings::Page::Assets] {
        ui.page.set(page);
        shot(
            &format!("settings-{page:?}-filled"),
            out,
            920,
            1100,
            theme.clone(),
            |doc, id| {
                doc.context_mut()
                    .mount_view_root(id, move || settings::settings_window(ui))
                    .unwrap();
            },
        );
    }
}

/// Repeatable animation poses and layout stress cases for the theme pass.
fn motion_capture(
    ui: Ui,
    out: &std::path::Path,
    w: u32,
    h: u32,
    theme: Arc<nana_ui::CompiledTheme>,
    frames: u32,
) {
    use nanatype::{clock, ui::shader_effects};
    let frames = frames.clamp(4, 512);
    let skin = ui.skin.get_untracked();
    let effect = skin
        .dialogue
        .shader_effects
        .first()
        .expect("artwork effect");
    let period = (skin.shaders[&effect.shader].period_s * 1000.).round() as u64;
    // Object motion follows dialogue status. Freeze the relevant real status
    // for these captures rather than requiring the completed state to animate.
    let status = match skin.meta.id.as_str() {
        "nanatype.livechat" => {
            ui.display.update(|v| {
                let v = Arc::make_mut(v);
                v.reveal.shown /= 2;
                v.reveal.segment = None;
                v.continue_indicator = false;
            });
            "typing"
        }
        "nanatype.rpg" => {
            ui.display
                .update(|v| Arc::make_mut(v).continue_indicator = true);
            "waiting"
        }
        _ => "default",
    };
    let start = period * 8;
    let id = DocumentId::new(1).unwrap();
    ui.now.set(start);
    let mut session = clock::capture_at(start - period, || {
        let mut doc = RuntimeDocument::new(id);
        doc.context_mut().set_theme_tokens(theme).unwrap();
        doc.context_mut()
            .mount_view_root(id, move || {
                dialogue::dialogue_box(ui, dialogue::Source::Display, None)
            })
            .unwrap();
        let mut session = RuntimeAgentSession::new(doc, w, h).unwrap();
        session.set_gpu_renderers(nanatype::ui::sweep::renderers());
        session.set_clear(Some([0.; 4]));
        let _ = session.screenshot_rgba().unwrap();
        let deadline = std::time::Instant::now();
        while shader_effects::cache_stats().pending > 0 && deadline.elapsed().as_secs() < 20 {
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        assert_eq!(shader_effects::cache_stats().pending, 0);
        // The first successful GPU draw establishes dialogue event timestamps.
        // Do it before the loop starts, after asynchronous compilation finishes.
        let _ = session.screenshot_rgba().unwrap();
        session
    });
    clock::capture_at(start, || {
        for _ in 0..3 {
            let _ = session.screenshot_rgba().unwrap();
        }
    });
    let cache = shader_effects::cache_stats();
    // Capture the real static fallback in the same retained scene. This also
    // supplies the gallery without a second GPU context or a fake redraw.
    let original_settings = ui.settings.get_untracked();
    ui.settings
        .update(|s| Arc::make_mut(s).skin.advanced_effects = false);
    clock::capture_at(start, || {
        session.screenshot_png(out.join("display.png")).unwrap()
    });
    ui.settings.set(original_settings);
    let [left, top, right, bottom] = skin.dialogue.padding;
    let inset = skin.dialogue.overscan;
    let [mt, mr, mb, ml] = skin.dialogue.box_.margin;
    let available_width = w as f32 - inset * 2. - ml - mr;
    let side_space = available_width * (1. - skin.dialogue.box_.width_fraction) * 0.5;
    let reading = [
        (inset + ml + side_space + left).ceil() as u32,
        (inset + mt + top).ceil() as u32,
        (w as f32 - inset - mr - side_space - right).floor() as u32,
        (h as f32 - inset - mb - bottom).floor() as u32,
    ];
    let mut first = Vec::new();
    let mut moving = 0;
    for frame in 0..=frames {
        let (_, rgba) = clock::capture_at(
            start + period * u64::from(frame) / u64::from(frames),
            || session.screenshot_rgba().unwrap(),
        );
        if frame == 0 {
            first = rgba.clone();
        }
        let mut changed = 0;
        for (index, (a, b)) in first
            .as_chunks::<4>()
            .0
            .iter()
            .zip(rgba.as_chunks::<4>().0.iter())
            .enumerate()
        {
            let (x, y) = (index as u32 % w, index as u32 / w);
            assert_eq!(a[3], b[3], "moving outer alpha at frame {frame}, {x},{y}");
            if x >= reading[0] && x < reading[2] && y >= reading[1] && y < reading[3] {
                assert_eq!(a, b, "reading area moved at frame {frame}, {x},{y}");
            }
            changed += usize::from(a != b);
        }
        moving = moving.max(changed);
        let file = if frame == frames {
            "motion-loop.png".into()
        } else {
            format!("motion-{frame:03}.png")
        };
        image::RgbaImage::from_raw(w, h, rgba.clone())
            .unwrap()
            .save(out.join(file))
            .unwrap();
        if frame == frames {
            assert!(
                first == rgba,
                "artwork does not close its loop ({changed} changed pixels)"
            );
        }
    }
    // A quiet status light may occupy only a few dozen pixels; avoid making
    // restrained designs animate a larger area just to satisfy the harness.
    assert!(moving > 16, "no visible ornamental motion");
    // Exercise opening and speaker-change accents in the same scene. Metadata
    // changes trigger the shader events without replacing the reading fixture.
    ui.display.update(|v| {
        let v = Arc::make_mut(v);
        v.line = Some(nanatype_model::LineId::next());
        v.speaker.name = "宁宁".into();
    });
    let event_samples = [0u64, 125, 250, 500, 1000, 2000];
    for offset in event_samples {
        let (_, rgba) = clock::capture_at(start + period * 2 + offset, || {
            session.screenshot_rgba().unwrap()
        });
        for y in reading[1]..reading[3] {
            let a = ((y * w + reading[0]) * 4) as usize;
            let b = ((y * w + reading[2]) * 4) as usize;
            assert!(
                first[a..b] == rgba[a..b],
                "event changed reading pixels at {offset}ms, row {y}"
            );
        }
        image::RgbaImage::from_raw(w, h, rgba)
            .unwrap()
            .save(out.join(format!("event-{offset:04}.png")))
            .unwrap();
    }
    // Status decorations settle when their job is done. Compare complete
    // retained scenes across clock values after changing only that status.
    let resting_status_stable = if matches!(
        skin.meta.id.as_str(),
        "nanatype.livechat" | "nanatype.rpg" | "nanatype.terminal"
    ) {
        ui.display.update(|v| {
            let v = Arc::make_mut(v);
            v.reveal.shown = v.reveal.hidden_from;
            v.reveal.segment = None;
            v.continue_indicator = skin.meta.id == "nanatype.terminal";
        });
        let (_, a) = clock::capture_at(start + period * 4, || session.screenshot_rgba().unwrap());
        let (_, b) = clock::capture_at(start + period * 4 + period / 3, || {
            session.screenshot_rgba().unwrap()
        });
        assert!(a == b, "completed status still moves: {}", skin.meta.id);
        image::RgbaImage::from_raw(w, h, a)
            .unwrap()
            .save(out.join("status-resting.png"))
            .unwrap();
        Some(true)
    } else {
        None
    };
    let after = shader_effects::cache_stats();
    assert_eq!(
        (cache.programs, cache.masks, cache.bytes),
        (after.programs, after.masks, after.bytes)
    );
    assert!(shader_effects::take_notices().is_empty());
    std::fs::write(
        out.join("motion-check.json"),
        serde_json::to_vec_pretty(&serde_json::json!({
            "frames": frames, "period_ms": period, "status": status, "reading": reading, "moving_pixels_max": moving,
            "resting_status_stable": resting_status_stable,
            "reading_stable": true, "alpha_stable": true, "loop_exact": true, "cache_stable": true,
        "event_reading_stable": true, "event_samples_ms": event_samples,
        }))
        .unwrap(),
    )
    .unwrap();
    println!(
        "motion: {frames} frames, {period} ms, {moving} animated pixels; reading, alpha, loop and caches stable"
    );
}

fn theme_audit(
    ui: Ui,
    out: &std::path::Path,
    w: u32,
    h: u32,
    ih: u32,
    theme: Arc<nana_ui::CompiledTheme>,
) {
    let display_shot = |name: &str, ui: Ui, w, h| {
        nanatype::clock::capture_at(ui.now.get_untracked(), || {
            shot(name, out, w, h, theme.clone(), |doc, id| {
                doc.context_mut()
                    .mount_view_root(id, move || {
                        dialogue::dialogue_box(ui, dialogue::Source::Display, None)
                    })
                    .unwrap();
            })
        });
    };
    let input_shot = |name: &str, w, h| {
        shot(name, out, w, h, theme.clone(), |doc, id| {
            doc.context_mut()
                .mount_view_root(id, move || input::input_window(ui))
                .unwrap();
        });
    };
    let skin = ui.skin.get_untracked();
    let period_ms = match skin.meta.id.as_str() {
        "nanatype.night" => 6000,
        "nanatype.bubble" => 5000,
        "nanatype.sakura" => 24000, // both the 6s and 8s petals return to rest
        _ => 0,
    };
    if period_ms > 0 {
        let frames = if period_ms == 24000 { 16 } else { 8 };
        ui.peer_display.set(ui.display.get_untracked());
        for i in 0..=frames {
            ui.now.set(120_000 + i * period_ms / frames);
            ui.peer_now.set(120_000 + i * period_ms / frames);
            display_shot(&format!("motion-{i:02}"), ui, w, h);
            display_shot(&format!("motion-peer-{i:02}"), ui.remote(), w, h);
        }
        for i in 0..2 {
            ui.reduced_motion.set(true);
            ui.now.set(120_000 + i * 1750);
            display_shot(&format!("reduced-{i}"), ui, w, h);
            ui.reduced_motion.set(false);
            input_shot(&format!("input-still-{i}"), w, ih);
        }
        ui.reduced_motion.set(false);
    }
    // Long names plus quote/@, ruby and line breaks use the real composer
    // and display builders, including the text viewport's clipping.
    // Enlarge the skin region as well as the window: the editor deliberately
    // retains the configured region height rather than stretching with chrome.
    let mut stress_skin = (*skin).clone();
    stress_skin.dialogue.region[1] += 140.0;
    ui.skin.set(Arc::new(stress_skin));
    let mut settings = (*ui.settings.get_untracked()).clone();
    settings.general.speaker_name = "奈奈与联机伙伴・A Very Long Speaker Name".into();
    ui.settings.set(Arc::new(settings));
    let text = parse("[ruby=ものがたり]故事[/ruby]继续，今天也请多关照。\n第二行是长台词，用来检查装饰、换行与边界。\n第三行仍然清晰。[at=1001 name=星野ほたる]").doc;
    ui.draft.set(Arc::new(text.clone()));
    ui.editor_value.set(nanatype::ui::rich::to_rich(
        &text,
        &nanatype::ui::rich::Env {
            look: &dialogue::Base::editor(&skin).look(),
            urls: &ui.asset_urls.get_untracked(),
            editor: true,
            locale: ui.locale.get_untracked(),
        },
    ));
    let mut view = (*ui.display.get_untracked()).clone();
    view.doc = Arc::new(text.clone());
    view.speaker.name = "奈奈与联机伙伴・A Very Long Speaker Name".into();
    view.reveal = nanatype_core::Reveal::full(text.unit_count() as u32);
    view.revision += 1;
    ui.display.set(Arc::new(view));
    ui.now.set(150_000);
    // Wide visual-novel skins retain their original columns and menu row;
    // review them at 960px, compact dialogue skins at 560px.
    let narrow_width = w.min(if skin.meta.id == "nanatype.crayon" {
        300
    } else if skin.dialogue.region[0] >= 1100.0 {
        960
    } else {
        560
    });
    display_shot("display-long", ui, w, h + 140);
    display_shot("display-narrow", ui, narrow_width, h + 140);
    input_shot("input-narrow", narrow_width, ih + 140);
}

fn material_audit(
    ui: Ui,
    out: &std::path::Path,
    w: u32,
    h: u32,
    theme: Arc<nana_ui::CompiledTheme>,
) {
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

/// Deterministic real retained-scene entry/retirement frames, with Core's
/// authoritative timing. No CPU recreation of the author's shader effect.
fn transition_audit(
    ui: Ui,
    out: &std::path::Path,
    w: u32,
    h: u32,
    theme: Arc<nana_ui::CompiledTheme>,
) {
    use nana_ui::runtime::view::{AnyView, IntoView};
    use std::time::Duration;
    let base = 100_000;
    let skin = ui.skin.get_untracked();
    let requested_width = std::env::var("NANATYPE_TRANSITION_WIDTH")
        .ok()
        .map(|v| v.parse().unwrap());
    let width = requested_width.unwrap_or(w);
    let history_width = requested_width.unwrap_or(skin.history.width as u32);
    let history_height = skin.history.height as u32;
    let mut settings = (*ui.settings.get_untracked()).clone();
    settings.playback.punctuation_pauses = false;
    settings.history.hide_latest = nanatype_settings::HideLatest::Never;
    settings.overlays.history.size = Some([history_width as f32, history_height as f32]);
    ui.settings.set(Arc::new(settings.clone()));
    let mut core = Core::new(settings, skin);
    core.handle(
        Cmd::SetReducedMotion(ui.reduced_motion.get_untracked()),
        base,
    );
    core.handle(
        Cmd::SendMarkup(
            "[ruby=いろ]彩铅[/ruby] Crayon 😀\n[color=#B94F49]斜向擦入，斜向擦除[/color]".into(),
        ),
        base,
    );
    ui.display.set(Arc::new(core.display().clone()));
    ui.history
        .set(Arc::new(core.history().iter().cloned().collect()));
    ui.now.set(base);
    let scale = std::env::var("NANATYPE_SNAPSHOT_SCALE")
        .ok()
        .map(|v| v.parse().unwrap())
        .unwrap_or(1.);
    let make_session = |width, height, view: fn(Ui) -> AnyView| {
        let id = DocumentId::new(1).unwrap();
        let mut doc = RuntimeDocument::new(id);
        doc.context_mut().set_theme_tokens(theme.clone()).unwrap();
        nanatype::clock::capture_at(base, || {
            doc.context_mut()
                .mount_view_root(id, move || view(ui))
                .unwrap()
        });
        let mut session = RuntimeAgentSession::new_scaled(doc, width, height, scale).unwrap();
        session.set_clear(Some([0.; 4]));
        session
    };
    let mut history_session = make_session(history_width, history_height, |ui| {
        history::history_window(ui).into_any()
    });
    let mut session = make_session(width, h + 140, |ui| dialogue::display_window(ui).into_any());
    let mut ink = Vec::new();
    let capture = |session: &mut RuntimeAgentSession, t: u64, name: &str| {
        ui.now.set(base + t);
        session
            .document_mut()
            .sync_presentation_clock(Duration::from_millis(t));
        nanatype::clock::capture_at(base + t, || {
            let path = out.join(format!("{name}.png"));
            session.screenshot_png(&path).unwrap();
            image::open(path).unwrap().to_rgba8()
        })
    };
    for t in [
        0, 40, 80, 120, 180, 240, 360, 600, 900, 1200, 1500, 1800, 2200,
    ] {
        capture(&mut session, t, &format!("enter-{t:04}"));
        capture(&mut history_session, t, &format!("history-enter-{t:04}"));
    }
    let history_pixels = image::open(out.join("history-enter-2200.png"))
        .unwrap()
        .to_rgba8();
    let fx = core.handle(Cmd::ClearDisplay, base + 2400);
    let wake = fx
        .wakes
        .iter()
        .find(|w| w.reason == nanatype_playback::WakeReason::PresentationDone)
        .copied();
    ui.display.set(Arc::new(core.display().clone()));
    ui.history
        .set(Arc::new(core.history().iter().cloned().collect()));
    for t in [0, 40, 80, 120, 160, 200, 240, 280, 320] {
        let image = capture(&mut session, 2400 + t, &format!("exit-{t:03}"));
        assert!(
            capture(
                &mut history_session,
                2400 + t,
                &format!("history-exit-{t:03}")
            ) == history_pixels,
            "display retirement changed history at {t}ms"
        );
        let count = image
            .enumerate_pixels()
            .filter(|(x, y, p)| {
                *y > (65. * scale) as u32
                    && *y < (210. * scale) as u32
                    && *x > (30. * scale) as u32
                    && p[0] < 130
                    && p[1] < 130
                    && p[2] < 130
                    && p[3] > 100
            })
            .count();
        ink.push(count);
    }
    if wake.is_some() {
        assert!(
            ink[0] > ink[8] + 80,
            "retirement mask did not erase the actual body: {ink:?}"
        );
    }
    if let Some(w) = wake {
        core.handle(
            Cmd::Wake {
                epoch: w.epoch,
                reason: w.reason,
            },
            w.at_ms,
        );
        assert!(core.display().empty);
    }
    ui.display.set(Arc::new(core.display().clone()));
    ui.history
        .set(Arc::new(core.history().iter().cloned().collect()));
    assert!(
        capture(&mut history_session, 3000, "history-after-clear") == history_pixels,
        "clearing the display changed completed history"
    );
    std::fs::write(out.join("transition-check.json"),serde_json::to_vec_pretty(&serde_json::json!({"entry_ms":180,"exit_ms":320,"scale":scale,"width":width,"history_width":history_width,"history_survives_clear":true,"exit_ink":ink,"finite_deadline":wake.is_some(),"advanced_effects":ui.settings.with_untracked(|s|s.skin.advanced_effects),"reduced_motion":ui.reduced_motion.get_untracked()})).unwrap()).unwrap();
}

fn responsive_audit(
    ui: Ui,
    skin: &nanatype_theme::Skin,
    out: &std::path::Path,
    theme: Arc<nana_ui::CompiledTheme>,
) {
    if std::env::var_os("NANATYPE_RESPONSIVE_LONG_TEXT").is_some() {
        let text = format!("[at=1001 name=星野ほたる avatar=blob:bili-face-1001] [sticker=nana/heart]{}", "中文竖排：你好，世界！Hello 2026。[b]字体[/b]与[color=#E8505B]颜色[/color]。[ruby=かんじ]漢字[/ruby]。\n".repeat(12));
        let doc = Arc::new(parse(&text).doc);
        ui.display.update(|view| {
            let view = Arc::make_mut(view);
            view.reveal = nanatype_core::Reveal::full(doc.unit_count() as u32);
            view.doc = doc;
            view.reply = None;
            view.revision += 1;
        });
    }
    let requested = |name: &str| {
        !std::env::var("NANATYPE_SNAPSHOT_VIEWS")
            .is_ok_and(|views| !views.split(',').any(|v| v.trim() == name))
    };
    let (mut windows, mut previews) = (0, 0);
    let default = [
        (skin.dialogue.region[0] + 2. * skin.dialogue.overscan) as u32,
        (skin.dialogue.region[1] + 2. * skin.dialogue.overscan) as u32,
    ];
    let id = DocumentId::new(1).unwrap();
    let mut doc = RuntimeDocument::new(id);
    doc.context_mut().set_theme_tokens(theme.clone()).unwrap();
    doc.context_mut()
        .mount_view_root(id, move || dialogue::display_window(ui))
        .unwrap();
    let mut session = RuntimeAgentSession::new(doc, default[0], default[1]).unwrap();
    session.set_gpu_renderers(nanatype::ui::sweep::renderers());
    session.set_clear(Some([0.; 4]));
    for (state, enabled, reduced) in [
        ("static", false, false),
        ("effects", true, false),
        ("reduced", true, true),
    ] {
        ui.settings
            .update(|s| Arc::make_mut(s).skin.advanced_effects = enabled);
        ui.reduced_motion.set(reduced);
        for (case, [w, h]) in [
            ("default", default),
            ("narrow", [300, 240]),
            ("portrait", [300, 1200]),
            ("tall", [800, 2400]),
            ("minimum", [160, 96]),
            ("wide-short", [1200, 120]),
            ("max-height", [160, 8192]),
            ("max-width", [8192, 96]),
            ("square", [400, 400]),
            ("portrait-boundary", [400, 401]),
        ] {
            let name = format!("responsive-{state}-{case}");
            if !requested(&name) {
                continue;
            }
            let scale = (1080. / w.max(h) as f32).min(1.);
            session.set_viewport(w, h, scale).unwrap();
            session.flush().unwrap();
            if std::env::var_os("NANATYPE_RESPONSIVE_LONG_TEXT").is_some() {
                if let Some(node) = ui.text_node.get_untracked() {
                    let world = session.document().context().world();
                    if let Some((_, layout)) = world.text_layout(node) {
                        if layout.is_vertical() {
                            let body = world.layout_box(node).unwrap();
                            let port = world
                                .layout_box(ui.text_port.get_untracked().unwrap())
                                .unwrap();
                            assert!(
                                body.width + 0.5 >= layout.physical_size().0,
                                "{name}: column stack is truncated"
                            );
                            assert!(
                                (body.height - port.height).abs() < 1.,
                                "{name}: wrapping height differs from viewport"
                            );
                            assert!(
                                (body.x + body.width - port.x - port.width).abs() < 1.,
                                "{name}: first column is not right aligned"
                            );
                            if let Some(first) = layout.objects.first() {
                                let rect = layout.page_rect(first.rect, body.width);
                                assert!(
                                    body.x + rect.x >= port.x
                                        && body.x + rect.x + rect.width <= port.x + port.width + 1.,
                                    "{name}: leading inline object was clipped"
                                );
                            }
                        }
                    }
                }
            }
            // Static authored backgrounds also prepare their GPU pipeline lazily.
            let _ = session.screenshot_rgba().unwrap();
            let started = std::time::Instant::now();
            while nanatype::ui::shader_effects::cache_stats().pending > 0
                && started.elapsed().as_secs() < 10
            {
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
            if !enabled || reduced {
                let first = nanatype::clock::capture_at(0, || session.screenshot_rgba().unwrap().1);
                let later =
                    nanatype::clock::capture_at(123_456, || session.screenshot_rgba().unwrap().1);
                // Native gradient avatars can differ by two byte levels at
                // antialiased edges between GPU submissions. Actual movement
                // or a changed print must still fail, without logging whole frames.
                let largest_delta = first
                    .iter()
                    .zip(&later)
                    .map(|(a, b)| a.abs_diff(*b))
                    .max()
                    .unwrap_or(0);
                assert!(
                    largest_delta <= 2,
                    "{name}: frozen artwork changed with time (maximum channel delta {largest_delta})"
                );
                if case == "portrait" && skin.meta.id == "nanatype.citrus" {
                    let pixel_width = (w as f32 * scale).round() as usize;
                    let sample = |x: f32, y: f32| {
                        let index = ((y * scale).round() as usize * pixel_width
                            + (x * scale).round() as usize)
                            * 4;
                        &first[index..index + 3]
                    };
                    let left = sample(skin.dialogue.overscan + 12., h as f32 * 0.6);
                    let middle = sample(w as f32 * 0.37, h as f32 * 0.6);
                    assert!(
                        left.iter().zip(middle).all(|(a, b)| a.abs_diff(*b) < 12),
                        "{name}: the portrait still has the extra left wrapper stripe"
                    );
                }
                if case == "portrait" && skin.meta.id == "nanatype.parchment" {
                    let pixel_width = (w as f32 * scale).round() as usize;
                    let mut difference = 0u64;
                    let mut samples = 0u64;
                    for y in 400..600usize {
                        for x in 45..80usize {
                            let index = (y * pixel_width + x) * 4;
                            for channel in 0..3 {
                                difference += u64::from(
                                    first[index + channel].abs_diff(first[index + 4 + channel]),
                                );
                                samples += 1;
                            }
                        }
                    }
                    assert!(
                        difference as f64 / samples as f64 > 0.45,
                        "{name}: the original paper fibres were replaced by a flat gradient"
                    );
                }
                assert!(
                    !nanatype::ui::shader_effects::animating(ui),
                    "{name}: static artwork requests continuous frames"
                );
            }
            session
                .screenshot_png(&out.join(format!("{name}.png")))
                .unwrap();
            windows += 1;
        }
    }
    ui.page.set(settings::Page::Skins);
    ui.skins.update(|rows| {
        if let Some(row) = Arc::make_mut(rows)
            .iter_mut()
            .find(|r| r.id == skin.meta.id)
        {
            row.skin = Some(Arc::new(skin.clone()));
        }
    });
    ui.reduced_motion.set(false);
    for (case, [w, h]) in [
        ("landscape", [920, 220]),
        ("portrait", [300, 1200]),
        ("maximum", [8192, 8192]),
    ] {
        let skin_id = ui.settings.with_untracked(|s| s.skin.id.clone());
        let mut draft = settings::preview::Draft::load(&ui.settings.get_untracked(), &skin_id, ui);
        draft.display.width = w.to_string();
        draft.display.height = h.to_string();
        if case == "maximum" {
            draft.display.size = "200".into();
        }
        ui.skin_drafts.update(|d| {
            d.insert(skin_id.clone(), draft);
        });
        for (mode, play) in [("static", false), ("effects", true)] {
            let name = format!("responsive-preview-{mode}-{case}");
            if !requested(&name) {
                continue;
            }
            ui.shader_preview.set(play);
            shot(&name, out, 920, 1000, theme.clone(), |doc, id| {
                doc.context_mut()
                    .mount_view_root(id, move || settings::settings_window(ui))
                    .unwrap();
            });
            previews += 1;
        }
    }
    println!(
        "responsive: {} passed {windows} window and {previews} settings captures",
        skin.meta.id
    );
}

fn preview_scale_audit(ui: Ui, out: &std::path::Path, theme: Arc<nana_ui::CompiledTheme>) {
    ui.page.set(settings::Page::Skins);
    let id = ui.settings.with_untracked(|s| s.skin.id.clone());
    for (name, [w, h], font) in [
        ("tall", [300, 1200], 48),
        ("very-tall", [800, 2400], 48),
        ("maximum-height", [160, 8192], 200),
        ("maximum-square", [8192, 8192], 200),
        ("maximum-width", [8192, 96], 48),
    ] {
        let mut draft = settings::preview::Draft::load(&ui.settings.get_untracked(), &id, ui);
        for pane in [&mut draft.display, &mut draft.history] {
            pane.width = w.to_string();
            pane.height = h.to_string();
            pane.size = font.to_string();
        }
        ui.skin_drafts.update(|drafts| {
            drafts.insert(id.clone(), draft);
        });
        for history in [false, true] {
            ui.skin_preview_history.set(history);
            let mode = if history { "history" } else { "display" };
            shot(
                &format!("preview-{mode}-{name}"),
                out,
                920,
                1000,
                theme.clone(),
                |doc, id| {
                    doc.context_mut()
                        .mount_view_root(id, move || settings::settings_window(ui))
                        .unwrap();
                },
            );
        }
    }
}

/// All-skin native layout captures at independently varied client sizes.
fn resize_audit(
    ui: Ui,
    base: Arc<nanatype_theme::Skin>,
    out: &std::path::Path,
    theme: Arc<nana_ui::CompiledTheme>,
) {
    use nanatype_settings::{SkinPersonalization, SurfacePersonalization};
    let default_display = [
        base.dialogue.region[0] + 2.0 * base.dialogue.overscan,
        base.dialogue.region[1] + 2.0 * base.dialogue.overscan,
    ];
    let default_history = [base.history.width, base.history.height];
    for (name, display, history_size, font) in [
        ("default", default_display, default_history, None),
        ("narrow", [300.0, 240.0], [300.0, 560.0], None),
        ("minimum", [160.0, 96.0], [160.0, 96.0], None),
        ("wide-short", [1200.0, 120.0], [1200.0, 120.0], None),
        ("large-font", [300.0, 240.0], [300.0, 560.0], Some(48.0)),
    ] {
        let mut settings = (*ui.settings.get_untracked()).clone();
        let p = SkinPersonalization {
            display: SurfacePersonalization {
                size: Some(display),
                text_size: font,
                line_height: font.map(|_| 1.8),
                ..Default::default()
            },
            history: SurfacePersonalization {
                size: Some(history_size),
                text_size: font,
                line_height: font.map(|_| 1.8),
                ..Default::default()
            },
        };
        settings
            .skin
            .personalizations
            .insert(settings.skin.id.clone(), p);
        let skin = Arc::new(nanatype::skin_theme::personalized(
            &base,
            &settings,
            &settings.skin.id,
        ));
        ui.settings.set(Arc::new(settings));
        ui.skin.set(skin);
        shot(
            &format!("display-{name}"),
            out,
            display[0] as u32,
            display[1] as u32,
            theme.clone(),
            |doc, id| {
                doc.context_mut()
                    .mount_view_root(id, move || dialogue::display_window(ui))
                    .unwrap();
            },
        );
        shot(
            &format!("history-{name}"),
            out,
            history_size[0] as u32,
            history_size[1] as u32,
            theme.clone(),
            |doc, id| {
                doc.context_mut()
                    .mount_view_root(id, move || history::history_window(ui))
                    .unwrap();
            },
        );
    }
    ui.page.set(settings::Page::Skins);
    for history_mode in [false, true] {
        ui.skin_preview_history.set(history_mode);
        shot(
            if history_mode {
                "preview-history"
            } else {
                "preview-display"
            },
            out,
            920,
            1700,
            theme.clone(),
            |doc, id| {
                doc.context_mut()
                    .mount_view_root(id, move || settings::settings_window(ui))
                    .unwrap();
            },
        );
    }
}
