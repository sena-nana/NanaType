//! Animated offscreen footage for NanaPVMaker's Steam trailer.
//!
//! `cargo run -p nanatype --no-default-features --features builtin-fonts,steam --example pv_capture -- <out> [scenario|all] [skin-id|auto] [locale] [gif-sticker-dir]`
//!
//! Each scenario scripts real interaction (typing, clicks, speech, partner
//! lines, Twitch chat) on the deterministic capture clock and writes one
//! folder per surface: `<out>/<scenario>-<surface>/frame-%04d.png` plus the
//! NanaPVMaker capture contract `capture.json` (schema 1) with its events.
//! The Workshop subscriptions only exist in the Steam build.

#![allow(dead_code)]

#[path = "../snapshots/fixtures.rs"]
mod fixtures;
mod lines;
mod rig;
mod scenarios;
#[path = "../snapshots/scene.rs"]
mod scene;
mod skin_lines;

use fixtures::*;
use scene::Scene;

use std::path::PathBuf;
use std::sync::Arc;

use nana_ui::WindowOutputStatus;
use nanatype::assets::AssetLibrary;
use nanatype::chat::{ChatKind, ChatMessage};
use nanatype::ui::{LinkState, Status, StickerRow, Ui, danmaku, dialogue, link_text, settings};
use nanatype_bridge::{BridgeStatus, Sink};
use nanatype_chat::Guard;
use nanatype_core::{Cmd, Core};
use nanatype_i18n::Locale;
use nanatype_markup::parse;
use nanatype_model::{AssetRef, ChatUser, Document};
use nanatype_settings::Settings;
use nanatype_theme::SkinRegistry;

fn main() {
    // The rich views nest deeply; the default main-thread stack is too small.
    std::thread::Builder::new()
        .name("pv-capture".into())
        .stack_size(16 * 1024 * 1024)
        .spawn(run)
        .unwrap()
        .join()
        .unwrap();
}

fn run() {
    let _ = nanatype::clock::now_ms();
    let mut args = std::env::args().skip(1);
    let out = PathBuf::from(args.next().unwrap_or_else(|| "target/pv-capture".into()));
    let which = args.next().unwrap_or_else(|| "all".into());
    // `auto`: each scenario in its own skin (`scenarios::skin_for`)
    let skin_arg = args.next().unwrap_or_else(|| "nanatype.halo".into());
    let locale = args
        .next()
        .and_then(|l| Locale::from_tag(&l))
        .unwrap_or(Locale::ZhCn);
    if let Some(dir) = args.next() {
        scenarios::GIF_STICKERS.set(PathBuf::from(dir)).unwrap();
    }
    std::fs::create_dir_all(&out).unwrap();
    // Fixture pictures (avatars, stickers) and the data folder stay out of
    // the surface folders `pv capture` reads.
    let work = out.join("_work");
    nanatype::services::media::init(&work.join("data")).expect("media mirror");
    let registry = SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
    let names: Vec<&str> = if which == "all" {
        scenarios::ALL.to_vec()
    } else {
        which.split(',').map(str::trim).collect()
    };
    for name in names {
        let skin_id = if skin_arg == "auto" {
            scenarios::skin_for(name).to_owned()
        } else {
            skin_arg.clone()
        };
        // A skin whose chrome is declared dark is shown with dark application chrome,
        // the way its users run it. The shared snapshot scene reads this switch.
        let appearance = if scenarios::dark_chrome(&skin_id) {
            "dark"
        } else {
            "light"
        };
        // SAFETY: the capture thread is the only thread; nothing reads the environment concurrently.
        unsafe { std::env::set_var("NANATYPE_SNAPSHOT_APPEARANCE", appearance) };
        println!("pv_capture: {name} ({skin_id}, {})", locale.tag());
        scenarios::run(name, &registry, &out, &work, &skin_id, locale);
    }
}
