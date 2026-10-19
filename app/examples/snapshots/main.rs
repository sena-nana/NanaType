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
//! `NANATYPE_SNAPSHOT_WIDTH=300` overrides the logical width of selected views.
//! `NANATYPE_PREVIEW_SCALE_AUDIT=1` captures tall/maximum logical canvases
//! inside the settings preview, without allocating full-size desktop windows.
//! `NANATYPE_RESPONSIVE_AUDIT=1` reuses native windows across both writing modes,
//! static/effect/reduced states, and captures the matching settings previews.
//! `NANATYPE_SNAPSHOT_MOTION_FRAMES=128` captures one complete artwork cycle
//! in a single retained scene, checking the reading area, alpha and GPU caches.

mod fixtures;
mod full;
mod material;
mod motion;
mod preview_scale;
mod resize;
mod responsive;
mod scene;
mod theme;
mod transition;

use fixtures::*;
use scene::Scene;

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
    let w = std::env::var("NANATYPE_SNAPSHOT_WIDTH")
        .map(|w| {
            w.parse::<u32>()
                .expect("snapshot width is a positive integer")
        })
        .unwrap_or(w);
    assert!(w > 0, "snapshot width must be positive");
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
        // Retry compiles refused by the bounded compiler before capturing,
        // without advancing the deterministic presentation clock.
        nanatype::clock::capture_at(at, || {
            let start = std::time::Instant::now();
            loop {
                let refused = nanatype::ui::shader_effects::cache_stats().refused;
                let _ = session.screenshot_rgba().expect("shader warmup");
                while nanatype::ui::shader_effects::cache_stats().pending > 0 {
                    assert!(start.elapsed().as_secs() < 15, "shader warmup timed out");
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
                if nanatype::ui::shader_effects::cache_stats().refused == refused {
                    break;
                }
                assert!(
                    start.elapsed().as_secs() < 15,
                    "shader compiles kept being refused"
                );
            }
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
    for name in [FRAMED, FALLBACK] {
        fixture_package(&skins.join(name));
    }
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

fn render(registry: &SkinRegistry, out: &std::path::Path, skin_id: &str, locale: Locale) {
    let at = std::env::var("NANATYPE_SNAPSHOT_TIME_MS")
        .ok()
        .map(|s| s.parse::<u64>().expect("snapshot time in milliseconds"))
        .unwrap_or(100_000);
    nanatype::clock::capture_at(at, || render_at(registry, out, skin_id, locale));
}

fn render_at(registry: &SkinRegistry, out: &std::path::Path, skin_id: &str, locale: Locale) {
    std::fs::create_dir_all(out).unwrap();
    let scene = Scene::new(registry, out, skin_id, locale);
    let (ui, w, h, theme) = (scene.ui, scene.w, scene.h, scene.theme.clone());
    if std::env::var_os("NANATYPE_RESPONSIVE_AUDIT").is_some() {
        responsive::audit(ui, &scene.skin, out, theme);
        return;
    }
    if std::env::var_os("NANATYPE_PREVIEW_SCALE_AUDIT").is_some() {
        preview_scale::audit(ui, out, theme);
        return;
    }
    if std::env::var_os("NANATYPE_RESIZE_AUDIT").is_some() {
        resize::audit(ui, scene.skin.clone(), out, theme);
        return;
    }
    if std::env::var_os("NANATYPE_TRANSITION_AUDIT").is_some() {
        transition::audit(ui, out, w, h, theme);
        return;
    }
    if let Ok(frames) = std::env::var("NANATYPE_SNAPSHOT_MOTION_FRAMES") {
        motion::capture(
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
        theme::capture(&scene, out, ih);
        return;
    }
    full::capture(&scene, out, iw, ih);
}
