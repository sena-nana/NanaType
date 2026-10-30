//! The trailer's scripted moments, one per feature shot.

use super::*;
use crate::lines::{self, Lines};
use crate::rig::{Rig, ms, typed_end};
use crate::skin_lines;
use nanatype::workshop::{Availability, Kind, Row, RowState, View};
use nanatype_core::HistoryRow;
use nanatype_settings::{InputMode, SpeechMode};
use nanatype_speech::controller::Phase;
use std::path::Path;

pub const ALL: &[&str] = &[
    "hook", "type", "markup", "rhythm", "mirror", "sticker", "keys", "room", "twitch", "workshop",
    "skins", "effects", "complete", "telegram",
];

/// The skin each scenario is shown in when the skin argument is `auto`: the
/// trailer gives every feature its own look.
pub fn skin_for(scenario: &str) -> &'static str {
    match scenario {
        "keys" => "nanatype.citrus",
        "markup" => "nanatype.crayon",
        "rhythm" => "nanatype.comic",
        "voice" => "nanatype.tidal",
        "mirror" => "nanatype.terminal",
        "sticker" | "telegram" => "nanatype.bubble",
        "complete" => "nanatype.crayon",
        "room" => "nanatype.celestial",
        "twitch" => "nanatype.livechat",
        _ => "nanatype.halo",
    }
}

/// Skins whose `[chrome]` declares `appearance = "dark"`.
pub fn dark_chrome(skin_id: &str) -> bool {
    matches!(
        skin_id,
        "nanatype.tidal" | "nanatype.terminal" | "nanatype.celestial"
    )
}

/// A fresh scene with nothing on screen: the fixtures' fonts, skin,
/// stickers and avatars, this language's speaker, and `edit`'s settings.
fn setup(
    registry: &SkinRegistry,
    work: &Path,
    skin_id: &str,
    locale: Locale,
    edit: impl FnOnce(&mut Settings),
) -> (Scene, Core) {
    let scene = Scene::new(registry, work, skin_id, locale);
    let l = lines::of(locale);
    let mut s = scene.settings.clone();
    s.general.speaker_name = l.speaker.into();
    s.general.input_mode = InputMode::Compose;
    s.playback.speed_scale = l.speed;
    // Capture-clock timestamps are near the epoch; a clock would read 08:00.
    s.history.show_time = false;
    // Locked, as on stream: an empty box is clear, not the drag outline.
    for o in [
        &mut s.overlays.display,
        &mut s.overlays.peer_display,
        &mut s.overlays.history,
    ] {
        o.lock = nanatype_settings::Lock::Locked;
    }
    edit(&mut s);
    let ui = scene.ui;
    let core = Core::new(s.clone(), scene.skin.clone());
    ui.settings.set(Arc::new(s));
    ui.history.set(Arc::new(Vec::new()));
    ui.display.set(Arc::new(core.display().clone()));
    ui.draft.set(Arc::new(Document::default()));
    ui.editor_value.set(Default::default());
    let mut src = ui.source.get_untracked();
    src.text.clear();
    src.selection = (0, 0);
    src.revision += 1;
    ui.source.set(src);
    ui.reply.set(None);
    ui.chat.set(Arc::new(Vec::new()));
    ui.chat_target.set(None);
    (scene, core)
}

pub fn run(
    name: &str,
    registry: &SkinRegistry,
    out: &Path,
    work: &Path,
    skin_id: &str,
    locale: Locale,
) {
    let l = lines::of(locale);
    let plain = |_: &mut Settings| {};
    match name {
        "hook" => one_line(registry, out, work, skin_id, locale, "hook", l.claim, 105),
        "markup" => one_line(
            registry, out, work, skin_id, locale, "markup", l.markup, 105,
        ),
        "rhythm" => one_line(
            registry, out, work, skin_id, locale, "rhythm", l.rhythm, 120,
        ),
        "sticker" => sticker(registry, out, work, skin_id, locale, l),
        "keys" => {
            let (scene, core) = setup(registry, work, skin_id, locale, |s| {
                s.general.input_presence = nanatype_settings::InputPresence::Summon;
            });
            keys(scene, core, out, l);
        }
        "type" => {
            let (scene, core) = setup(registry, work, skin_id, locale, plain);
            type_line(scene, core, out, l)
        }
        "voice" => {
            let (scene, core) = setup(registry, work, skin_id, locale, |s| {
                s.speech.mode = SpeechMode::Auto
            });
            voice(scene, core, out, l)
        }
        "mirror" => {
            let (scene, core) = setup(registry, work, skin_id, locale, |s| {
                s.general.input_mode = InputMode::Mirror
            });
            mirror(scene, core, out, l)
        }
        "room" => {
            // Mirror: the local box speaks while it is typed, so it is never empty for long.
            let (scene, core) = setup(registry, work, skin_id, locale, |s| {
                s.general.input_mode = InputMode::Mirror
            });
            room(scene, core, out, l)
        }
        "twitch" => {
            let (scene, core) = setup(registry, work, skin_id, locale, |s| {
                s.integrations.twitch.enabled = true;
                s.integrations.twitch.channel = "nana_ch".into();
                s.history.chat.enabled = true;
            });
            twitch(scene, core, out, work, l)
        }
        "workshop" => {
            let (scene, core) = setup(registry, work, skin_id, locale, plain);
            workshop(scene, core, out, l)
        }
        "skins" => skins(registry, out, work, locale, l),
        "effects" => effects(registry, out, work, locale, l),
        "complete" => {
            let (scene, core) = setup(registry, work, skin_id, locale, |_| {});
            complete(scene, core, out, l);
        }
        "telegram" => {
            let (scene, core) = setup(registry, work, skin_id, locale, |_| {});
            telegram(scene, core, out, l);
        }
        "backdrops" => backdrops(registry, out, work, locale),
        other => panic!("unknown scenario {other}; one of {ALL:?}"),
    }
}

/// One line sent to the display box: its reveal, effects and cues.
#[allow(clippy::too_many_arguments)]
fn one_line(
    registry: &SkinRegistry,
    out: &Path,
    work: &Path,
    skin_id: &str,
    locale: Locale,
    name: &'static str,
    markup: &'static str,
    frames: u64,
) {
    let (scene, core) = setup(registry, work, skin_id, locale, |_| {});
    let mut rig = Rig::new(name, out, scene.ui, core, scene.theme.clone());
    rig.display("display", 2.0);
    rig.run(frames, |r, f| {
        if f == 4 {
            r.event(f, "type", format!("{name}-text"));
            r.cmd(Cmd::SendMarkup(markup.into()), ms(f));
        }
    });
}

/// In a fullscreen game: the summon hotkey brings up the input window, the line is
/// typed and sent (the window hides again), the box plays it over the game.
fn keys(scene: Scene, core: Core, out: &Path, l: &Lines) {
    let mut rig = Rig::new("keys", out, scene.ui, core, scene.theme.clone());
    rig.input("input", 2.0);
    rig.display("display", 2.0);
    let summon = 10;
    let (start, every) = (summon + 8, 1);
    let send = typed_end(start, every, l.keys) + 6;
    rig.run(send + 60, |r, f| {
        if f == summon {
            r.event(f, "show", "keys-summon");
        }
        if f == start {
            r.event(f, "type", "keys-text");
        }
        r.typing(f, start, every, l.keys);
        if f == send {
            r.event(f, "click", "keys-send");
            r.send(ms(f));
        }
    });
}

/// Animated GIF stickers for the sticker shot (`pv_capture`'s optional fifth argument:
/// a folder holding a `nana/` sticker library). Without it the shot uses the
/// snapshot fixtures' still shapes.
pub static GIF_STICKERS: std::sync::OnceLock<std::path::PathBuf> = std::sync::OnceLock::new();

/// The line with its stickers. GIFs go through NanaType's own sticker cache (its
/// decoder composites every frame); the app plays them from GPU texture slots, which an
/// offscreen document has none of, so each captured frame points the sticker at the
/// cached frame NanaType would be showing at that moment.
fn sticker(
    registry: &SkinRegistry,
    out: &Path,
    work: &Path,
    skin_id: &str,
    locale: Locale,
    l: &Lines,
) {
    let (scene, core) = setup(registry, work, skin_id, locale, |_| {});
    let mut markup = l.sticker.to_owned();
    let mut gifs = Vec::new();
    if let Some(dir) = GIF_STICKERS.get() {
        let mut lib = nanatype::assets::AssetLibrary::new(work.join("cache").join("images"));
        lib.scan(&[dir.join("nana").display().to_string()]);
        markup = l.sticker.split('[').next().unwrap_or(l.sticker).to_owned();
        for s in &lib.stickers {
            let image = nanatype::stickers::cache::prepare(
                &s.path,
                &work.join("data").join("sticker-cache"),
                &std::sync::atomic::AtomicBool::new(false),
            )
            .expect("sticker gif");
            let asset = s.asset().to_string();
            // large, as streamers post them: well above the line, the caption readable
            markup.push_str(&format!("[sticker={}/{} h=4.0]", s.lib, s.name));
            gifs.push((asset, image));
        }
    }
    let ui = scene.ui;
    let mut rig = Rig::new("sticker", out, scene.ui, core, scene.theme.clone());
    rig.display("display", 2.0);
    let start = 4;
    rig.run(90, move |r, f| {
        if !gifs.is_empty() {
            let mut urls = ui.asset_urls.get_untracked().as_ref().clone();
            for (asset, image) in &gifs {
                let (frame, _) = image.frame_at(ms(f.saturating_sub(start)));
                urls.insert(
                    asset.clone(),
                    nanatype::assets::file_url(&image.frame_path(frame)),
                );
            }
            ui.asset_urls.set(Arc::new(urls));
        }
        if f == start {
            r.event(f, "type", "sticker-text");
            r.cmd(Cmd::SendMarkup(markup.clone()), ms(f));
        }
    });
}

/// Type in the input window, send, and watch the box and the history.
fn type_line(scene: Scene, mut core: Core, out: &Path, l: &Lines) {
    for (i, line) in l.earlier.iter().enumerate() {
        core.handle(Cmd::SendMarkup((*line).into()), i as u64);
        core.handle(Cmd::Skip, i as u64);
    }
    core.handle(Cmd::ClearDisplay, 3);
    scene
        .ui
        .history
        .set(Arc::new(core.history().iter().cloned().collect()));
    scene.ui.display.set(Arc::new(core.display().clone()));
    let mut rig = Rig::new("type", out, scene.ui, core, scene.theme.clone());
    rig.input("input", 2.0);
    rig.display("display", 2.0);
    rig.history("history", 2.0);
    let (start, every) = (10, 2);
    let send = typed_end(start, every, l.line) + 8;
    rig.run(send + 80, |r, f| {
        if f == start {
            r.event(f, "type", "type-text");
        }
        r.typing(f, start, every, l.line);
        if f == send {
            r.event(f, "click", "type-send");
            r.send(ms(f));
        }
    });
}

/// Speech recognition fills the draft, counts down and sends it.
fn voice(scene: Scene, core: Core, out: &Path, l: &Lines) {
    use nanatype::app::speech::State;
    let ui = scene.ui;
    let mut rig = Rig::new("voice", out, ui, core, scene.theme.clone());
    rig.input("input", 2.0);
    rig.display("display", 2.0);
    let (start, every) = (12, 2);
    let heard = typed_end(start, every, l.voice) + 4;
    let send = heard + 30;
    let speech = |phase, partial: String, remaining| {
        ui.speech.set(Arc::new(State {
            phase,
            partial,
            remaining,
            ..Default::default()
        }))
    };
    rig.run(send + 80, |r, f| {
        if f == 0 {
            speech(Phase::Listening, String::new(), 0);
        }
        if f >= start && f < heard && (f - start).is_multiple_of(every) {
            if f == start {
                r.event(f, "show", "voice-hear");
            }
            let n = ((f - start) / every + 1) as usize;
            speech(Phase::Recognizing, l.voice.chars().take(n).collect(), 0);
        }
        if f == heard {
            r.event(f, "show", "voice-commit");
            speech(Phase::Countdown, String::new(), 2);
            r.type_text(l.voice, ms(f));
        }
        if f == heard + 15 {
            speech(Phase::Countdown, String::new(), 1);
        }
        if f == send {
            r.event(f, "click", "voice-send");
            speech(Phase::Listening, String::new(), 0);
            r.send(ms(f));
        }
    });
}

/// Mirror mode: the box follows the editor, erasing included.
fn mirror(scene: Scene, core: Core, out: &Path, l: &Lines) {
    let mut rig = Rig::new("mirror", out, scene.ui, core, scene.theme.clone());
    rig.input("input", 2.0);
    rig.display("display", 2.0);
    let (start, every) = (8, l.type_every);
    let back = typed_end(start, every, l.mirror) + 6;
    let back_every = 3;
    let rest = back + back_every * l.mirror_back as u64 + 6;
    let end = typed_end(rest, every, l.mirror_rest);
    rig.run(end + 45, |r, f| {
        if f == start {
            r.event(f, "type", "mirror-text");
        }
        r.typing(f, start, every, l.mirror);
        if f >= back
            && f < back + back_every * l.mirror_back as u64
            && (f - back).is_multiple_of(back_every)
        {
            if f == back {
                r.event(f, "key", "mirror-erase");
            }
            r.key("Backspace", "Backspace", ms(f));
        }
        if f == rest {
            r.event(f, "type", "mirror-rest");
        }
        r.typing(f, rest, every, l.mirror_rest);
    });
}

/// A Steam room: friends join, then everyone's box speaks in turn and the
/// shared history collects every line.
fn room(scene: Scene, core: Core, out: &Path, l: &Lines) {
    use nanatype::p2p::SessionUi;
    let ui = scene.ui;
    ui.peer_panel.set(true);
    let mut partners: Vec<Core> = l
        .partners
        .iter()
        .map(|name| {
            let mut s = (*ui.settings.get_untracked()).clone();
            s.general.speaker_name = (*name).into();
            Core::new(s, scene.skin.clone())
        })
        .collect();
    let mut rig = Rig::new("room", out, ui, core, scene.theme.clone());
    rig.input("input", 2.0);
    rig.display("display", 2.0);
    let skin = scene.skin.clone();
    let d = &skin.dialogue;
    let size = (
        (d.region[0] + d.overscan * 2.0) as u32,
        ((d.region[1] + d.overscan * 2.0) * 2.0) as u32,
    );
    rig.mount("partners", size, 2.0, true, move || {
        dialogue::peer_window(ui)
    });
    rig.history("history", 2.0);
    let session = |members: usize, names: &[&str], connected: bool| {
        ui.peer.set(Arc::new(SessionUi {
            active: true,
            host: true,
            invitation: "nanatype1:steam".into(),
            name: names.join(&ui.now_t("peer-name-separator")),
            status: if connected {
                nanatype_p2p::Status::Connected {
                    route: "relay".into(),
                    rtt_ms: 32,
                }
            } else {
                nanatype_p2p::Status::Waiting
            },
            invite_names: names.iter().map(|n| (*n).to_owned()).collect(),
            members,
            ..Default::default()
        }));
    };
    let (start, every) = (16, 1);
    let send = typed_end(start, every, l.room_local) + 4;
    let speak = [send + 20, send + 50];
    // Everyone's rows in one list, in the order they were said.
    let merge = |r: &Rig, partners: &[Core]| {
        let mut rows: Vec<HistoryRow> = r.core.history().iter().cloned().collect();
        for (i, p) in partners.iter().enumerate() {
            rows.extend(p.history().iter().cloned().map(|mut row| {
                row.item.source = nanatype_model::SourceId(format!("peer:{i}"));
                row
            }));
        }
        rows.sort_by_key(|row| row.item.at);
        for (key, row) in rows.iter_mut().enumerate() {
            row.key = key as u64 + 1;
        }
        rows
    };
    rig.run(speak[1] + 80, |r, f| {
        match f {
            0 => session(1, &[], false),
            4 => {
                r.event(f, "show", "room-join1");
                session(2, &l.partners[..1], true);
                ui.peer_seats.set(1);
            }
            10 => {
                r.event(f, "show", "room-join2");
                session(3, &l.partners, true);
                ui.peer_seats.set(2);
            }
            _ => {}
        }
        if f == start {
            r.event(f, "type", "room-text");
        }
        r.typing(f, start, every, l.room_local);
        if f == send {
            r.event(f, "click", "room-send");
            r.send(ms(f));
        }
        for (i, at) in speak.iter().enumerate() {
            if f == *at {
                r.event(f, "show", format!("room-partner{}", i + 1));
                partners[i].handle(Cmd::SendMarkup(l.room_partner[i].into()), ms(f));
                let lane = ui.peer_lanes[i];
                let mut view = partners[i].display().clone();
                view.revision = lane.display.get_untracked().revision + 1;
                lane.display.set(Arc::new(view));
            }
        }
        if f >= send {
            let rows = merge(r, &partners);
            if ui.history.with_untracked(|h| **h != rows) {
                ui.history.set(Arc::new(rows));
            }
        }
    });
}

/// Twitch chat arrives, the streamer quotes one viewer and answers.
fn twitch(scene: Scene, core: Core, out: &Path, work: &Path, l: &Lines) {
    use nanatype::app::twitch::State;
    use nanatype_twitch::{AccountState, ConnectionState, Profile, Room};
    let ui = scene.ui;
    ui.twitch.set(Arc::new(State {
        account: AccountState::Ready(Profile {
            user_id: "1".into(),
            login: "nana_ch".into(),
            name: l.speaker.into(),
            avatar: String::new(),
        }),
        connection: ConnectionState::Connected(Room {
            login: "nana_ch".into(),
            id: "1".into(),
        }),
        ..Default::default()
    }));
    let messages = viewers(work, ui, l);
    let mut rig = Rig::new("twitch", out, ui, core, scene.theme.clone());
    let input = rig.input("input", 1.0);
    rig.surfaces[input].shoot = false;
    let panel = rig.mount("panel", (400, 560), 2.0, false, move || {
        danmaku::chat_panel(ui)
    });
    rig.display("display", 2.0);
    rig.history("history", 2.0);
    let arrive = [8, 22, 36];
    let reply = 58;
    let (start, every) = (66, l.type_every);
    let send = typed_end(start, every, l.reply) + 8;
    rig.run(send + 75, |r, f| {
        for (n, at) in arrive.iter().enumerate() {
            if f == *at {
                r.event(f, "show", format!("twitch-c{}", n + 1));
                let m = Arc::new(messages[n].clone());
                let mut rows = (*ui.chat.get_untracked()).clone();
                rows.push(m.clone());
                ui.chat.set(Arc::new(rows));
                r.cmd(
                    Cmd::External(m.history_item(ui.locale.get_untracked())),
                    ms(f),
                );
            }
        }
        if f == reply {
            r.event(f, "click", "twitch-reply");
            let label = ui.now_t("chat-reply");
            r.click(panel, &label, 1, ms(f));
        }
        if f == start {
            r.event(f, "type", "twitch-text");
        }
        r.typing(f, start, every, l.reply);
        if f == send {
            r.event(f, "click", "twitch-send");
            r.send(ms(f));
        }
    });
}

/// Viewers with gradient avatars, as Twitch delivers their messages.
fn viewers(work: &Path, ui: Ui, l: &Lines) -> Vec<ChatMessage> {
    let dir = work.join("cache").join("images");
    std::fs::create_dir_all(&dir).unwrap();
    let blobs = AssetLibrary::new(dir.clone());
    let mut urls = (*ui.asset_urls.get_untracked()).clone();
    let colors = [[145u8, 70, 255], [232, 106, 146], [61, 155, 233]];
    let out = l
        .chat
        .iter()
        .enumerate()
        .map(|(n, (name, text))| {
            let [r, g, b] = colors[n];
            let img = image::RgbaImage::from_fn(64, 64, |x, y| {
                let shade = 1.0 - (x + y) as f32 / 256.0;
                let f = |c: u8| (f32::from(c) * shade) as u8;
                image::Rgba([f(r), f(g), f(b), 255])
            });
            let id = format!("twitch-face-{n}");
            img.save(dir.join(format!("{id}.png"))).unwrap();
            let asset = AssetRef::Blob(id);
            urls.insert(asset.to_string(), blobs.url(&asset, None).unwrap());
            ChatMessage {
                platform: nanatype::chat::Platform::Twitch,
                epoch: 0,
                room: "nana_ch".into(),
                seq: n as u64 + 1,
                id: format!("tw-{n}"),
                kind: ChatKind::Text,
                user: ChatUser {
                    uid: format!("{}", 100 + n),
                    name: (*name).into(),
                    avatar: Some(asset),
                },
                guard: Guard::None,
                owner: false,
                admin: false,
                medal_level: 0,
                medal_name: String::new(),
                doc: Document::plain(text),
                at: 1_760_000_000_000 + n as u64,
            }
        })
        .collect();
    ui.asset_urls.set(Arc::new(urls));
    out
}

/// Workshop subscriptions download and become ready to use.
fn workshop(scene: Scene, core: Core, out: &Path, l: &Lines) {
    let ui = scene.ui;
    ui.page.set(settings::Page::Skins);
    let rows = |done: u8| View {
        availability: Availability::Ready,
        rows: [
            (l.workshop[0], Kind::Skin, "community.sakura-letter"),
            (l.workshop[1], Kind::Skin, "community.retro-arcade"),
            (l.workshop[2], Kind::SoundPack, "community.pixel-se"),
        ]
        .into_iter()
        .enumerate()
        .map(|(i, (title, kind, package))| Row {
            item: 3_100_000_000 + i as u64,
            title: title.into(),
            kind,
            state: if done >= 100 || i > 0 {
                RowState::Ready
            } else {
                RowState::Downloading(Some(done))
            },
            package: Some(package.into()),
            folder: None,
        })
        .collect(),
    };
    let mut rig = Rig::new("workshop", out, ui, core, scene.theme.clone());
    // Tall enough to hold the subscriptions under the skin library; the
    // trailer crops to them (offscreen wheel scrolling does not move the page).
    rig.mount("settings", (920, 2800), 1.0, false, move || {
        settings::settings_window(ui)
    });
    let ready = 45;
    rig.run(90, |r, f| {
        if f <= ready {
            let done = (f * 100 / ready) as u8;
            if f == ready {
                r.event(f, "show", "workshop-ready");
            }
            ui.workshop.set(Arc::new(rows(done)));
        }
    });
}

/// The skins the trailer's scenes are shown in.
pub const SCENE_SKINS: &[&str] = &[
    "nanatype.citrus",
    "nanatype.halo",
    "nanatype.crayon",
    "nanatype.comic",
    "nanatype.tidal",
    "nanatype.terminal",
    "nanatype.bubble",
    "nanatype.celestial",
    "nanatype.livechat",
    "nanatype.love_letter",
];

/// Each scene skin's own artwork at backdrop size: the box with no speaker and no
/// text, its region stretched to 16:9, so the skin's responsive artwork, ornaments
/// and materials draw a whole frame of its world.
fn backdrops(registry: &SkinRegistry, out: &Path, work: &Path, locale: Locale) {
    for id in SCENE_SKINS {
        let (scene, _) = setup(registry, work, id, locale, |s| {
            s.general.speaker_name = String::new();
        });
        let mut skin = scene.ui.skin.get_untracked();
        Arc::make_mut(&mut skin).dialogue.region = [960.0, 540.0];
        scene.ui.skin.set(skin.clone());
        let core = Core::new(scene.ui.settings.get_untracked().as_ref().clone(), skin);
        let short: &'static str =
            Box::leak(format!("bg-{}", id.trim_start_matches("nanatype.")).into());
        let mut rig = Rig::new("backdrop", out, scene.ui, core, scene.theme.clone());
        rig.display(short, 2.0);
        rig.run(60, |r, f| {
            if f == 0 {
                // one invisible character: the box shows, with nothing written in it
                r.cmd(Cmd::SendMarkup("[color=#00000000].[/color]".into()), 0);
                r.cmd(Cmd::Skip, 0);
            }
        });
    }
}

/// Skins whose text or artwork is an effect of its own: written stroke by stroke,
/// condensed from light, decoded, lit like neon. Each box full size, its line played
/// out and then cleared, so the exit effect shows too.
pub const EFFECT_SKINS: &[&str] = &[
    "nanatype.love_letter",
    "nanatype.sonnet",
    "nanatype.machine_learning",
    "nanatype.night",
];

fn effects(registry: &SkinRegistry, out: &Path, work: &Path, locale: Locale, l: &Lines) {
    for id in EFFECT_SKINS {
        let (scene, core) = setup(registry, work, id, locale, |_| {});
        let short: &'static str = Box::leak(id.trim_start_matches("nanatype.").to_owned().into());
        let mut rig = Rig::new("effects", out, scene.ui, core, scene.theme.clone());
        rig.display(short, 1.0);
        rig.run(110, |r, f| {
            if f == 4 {
                r.cmd(
                    Cmd::SendMarkup(skin_lines::of(id, locale, l.skins).into()),
                    ms(f),
                );
            }
            if f == 80 {
                r.cmd(Cmd::ClearDisplay, ms(f));
            }
        });
    }
    // Vertical text: the ink skin's box set top to bottom, as a per-window switch.
    let (scene, core) = setup(registry, work, "nanatype.ink", locale, |s| {
        s.overlays.display.vertical_text = true;
    });
    let mut rig = Rig::new("effects", out, scene.ui, core, scene.theme.clone());
    rig.vertical_display("vertical", (300, 640), 1.0);
    rig.run(110, |r, f| {
        if f == 4 {
            r.cmd(
                Cmd::SendMarkup(skin_lines::of("nanatype.ink", locale, l.skins).into()),
                ms(f),
            );
        }
    });
}

/// The `/` effect list: typing `/` opens it, typing on narrows it.
fn complete(scene: Scene, core: Core, out: &Path, l: &Lines) {
    let mut rig = Rig::new("complete", out, scene.ui, core, scene.theme.clone());
    rig.input("input", 2.0);
    let (start, every) = (6, 2);
    let end = typed_end(start, every, l.complete);
    let slash = l.complete.chars().position(|c| c == '/').unwrap() as u64;
    rig.run(end + 40, |r, f| {
        if f == start {
            r.event(f, "type", "complete-text");
        }
        r.typing(f, start, every, l.complete);
        if f >= start + slash * every && f <= end && (f - start).is_multiple_of(every) {
            let typed: String = l
                .complete
                .chars()
                .take(((f - start) / every + 1) as usize)
                .collect();
            if f == start + slash * every {
                r.event(f, "show", "complete-open");
            }
            r.completion(Some(&typed));
        }
    });
}

/// Telegram sticker search in the settings: a query, the packs found, one downloading.
fn telegram(scene: Scene, core: Core, out: &Path, l: &Lines) {
    use nanatype::stickers::telegram::{Pack, Phase, State};
    let ui = scene.ui;
    ui.page.set(settings::Page::Assets);
    let (query, packs) = l.telegram;
    let state = |busy: bool, results: bool, progress: Option<(usize, usize)>| State {
        phase: Phase::Ready,
        busy,
        results: if results {
            packs
                .iter()
                .map(|(name, title, count)| Pack {
                    name: (*name).into(),
                    title: (*title).into(),
                    count: *count,
                })
                .collect()
        } else {
            Vec::new()
        },
        searched: results,
        progress,
        error: None,
    };
    ui.telegram.set(Arc::new(state(false, false, None)));
    let mut rig = Rig::new("telegram", out, ui, core, scene.theme.clone());
    // The assets page down to the Telegram panel, at 2x: the trailer crops to the panel.
    let page = rig.mount("settings", (920, 680), 2.0, false, move || {
        settings::settings_window(ui)
    });
    let total = packs[0].2;
    rig.run(100, |r, f| {
        match f {
            6 => {
                r.event(f, "type", "telegram-query");
                r.type_into(page, "telegram-query-input", query, ms(f));
            }
            16 => {
                r.event(f, "click", "telegram-search");
                ui.telegram.set(Arc::new(state(true, false, None)));
            }
            26 => {
                r.event(f, "show", "telegram-results");
                ui.telegram.set(Arc::new(state(false, true, None)));
            }
            44 => r.event(f, "click", "telegram-download"),
            _ => {}
        }
        if (44..=80).contains(&f) {
            let done = ((f - 44) as usize * total / 36).min(total);
            ui.telegram.set(Arc::new(state(
                f < 80,
                true,
                (f < 80).then_some((done, total)),
            )));
            if f == 80 {
                r.event(f, "show", "telegram-done");
            }
        }
    });
}

/// Every built-in skin's box with its artwork moving, small enough for a wall.
fn skins(registry: &SkinRegistry, out: &Path, work: &Path, locale: Locale, l: &Lines) {
    let ids: Vec<String> = registry
        .entries()
        .iter()
        .map(|e| e.id.clone())
        .filter(|id| id.starts_with("nanatype."))
        .collect();
    for id in ids {
        let (scene, core) = setup(registry, work, &id, locale, |_| {});
        let short: &'static str = Box::leak(id.trim_start_matches("nanatype.").to_owned().into());
        let mut rig = Rig::new("skin", out, scene.ui, core, scene.theme.clone());
        rig.display(short, 0.5);
        // long enough to play straight through the wall's shot (no loop, no restart)
        rig.run(170, |r, f| {
            if f == 0 {
                r.cmd(
                    Cmd::SendMarkup(skin_lines::of(&id, locale, l.skins).into()),
                    0,
                );
                r.cmd(Cmd::Skip, 0);
            }
        });
    }
}
