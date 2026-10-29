//! Offscreen footage of the skin editor for NanaPVMaker's Steam trailer, written in
//! its capture contract (one folder per surface: `frame-%04d.png` + `capture.json`).
//!
//! `cargo run -p nanatype-editor --example pv_capture_editor -- <out> [locale]`
//!
//! A copy of the built-in citrus skin is opened in the design mode; the nameplate
//! layer is picked, its colour is changed (the canvas follows), and the source opens
//! beside the canvas at the nameplate's table. Then the counter Lua plugin opens
//! in the Lua mode and its returned view updates on each played line (`editor-lua`).

use nana_ui::runtime::view::widget;
use nana_ui::runtime::{DocumentId, RuntimeDocument, Stack};
use nana_ui_core::LengthSpec;
use nana_ui_devtools::agent::RuntimeAgentSession;
use nanatype_editor::session::{self, Mode, Session};
use nanatype_editor::ui::{self, Ui};
use nanatype_frame::ui::shader_effects::cache_stats;
use nanatype_i18n::Locale;
use nanatype_settings::Settings;
use nanatype_theme::SkinRegistry;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

const FPS: u64 = 30;
const SIZE: (u32, u32) = (1600, 1060);
const SCALE: f32 = 1.5;
const FRAMES: u64 = 120;
const SKIN: &str = include_str!("../../crates/theme/builtin/citrus/skin.toml");
// The first `color = "#ffc93c"` in the file is the nameplate's (the chrome's is
// `accent`); no line break in the pattern, so CRLF checkouts match too.
const PLATE: &str = "color = \"#ffc93c\"";
const PLATE_EDITED: &str = "color = \"#ff7aa8\"";

/// `(frame, kind, label)`: what happens, on the capture clock.
const STEPS: [(u64, &str, &str); 3] = [
    (30, "click", "editor-select"),
    (60, "type", "editor-edit"),
    (90, "show", "editor-source"),
];

fn validate(registry: &SkinRegistry, s: &mut Session) {
    if let Some((ticket, project)) = s.validation_due() {
        s.validated(ticket, Arc::new(session::validate(registry, &project)));
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let out = PathBuf::from(args.next().unwrap_or_else(|| "target/pv-editor".into()));
    let locale = args
        .next()
        .and_then(|l| Locale::from_tag(&l))
        .unwrap_or(Locale::ZhCn);
    let dir = out.join("editor-window");
    std::fs::create_dir_all(&dir).unwrap();

    let registry = SkinRegistry::new(None, env!("CARGO_PKG_VERSION"));
    let mut s = session::copy_installed(&registry, "nanatype.citrus", "my.citrus").unwrap();
    validate(&registry, &mut s);
    let ui = Ui::new(
        Arc::new(Settings::default()),
        Arc::new(Default::default()),
        locale,
    );
    ui.package_editor.set(Some(Arc::new(s.view(None, false))));
    ui.mode.set(Mode::Design);

    capture(&dir, ui, |f| match f {
        30 => {
            ui.canvas_selection.set(Some("nameplate".into()));
            true
        }
        60 => {
            s.edit("skin.toml", &SKIN.replacen(PLATE, PLATE_EDITED, 1))
                .unwrap();
            validate(&registry, &mut s);
            ui.package_editor.set(Some(Arc::new(s.view(None, false))));
            true
        }
        90 => {
            s.select("skin.toml");
            ui.package_editor.set(Some(Arc::new(s.view(None, false))));
            let _ = ui::modes::design::select(ui, vec!["dialogue".into(), "nameplate".into()]);
            ui.split_source.set(true);
            true
        }
        _ => f == 0,
    });
    write_manifest(&dir, "editor-window", &STEPS);
    lua(&out, &registry, locale);
    println!("pv_capture_editor: {FRAMES} frames ({})", locale.tag());
}

/// Run `step` for every frame (it returns whether the picture changed) and write the
/// frames, shooting only changed states and copying the last picture otherwise.
fn capture(dir: &Path, ui: Ui, mut step: impl FnMut(u64) -> bool) {
    let id = DocumentId::new(1).unwrap();
    let mut doc = RuntimeDocument::new(id);
    doc.context_mut()
        .set_theme_tokens(nanatype_frame::ui::kit::theme::compile(false))
        .unwrap();
    doc.context_mut()
        .mount_view_root(id, move || {
            widget(Stack::column(0.).height(LengthSpec::Px(SIZE.1 as f32)))
                .children(ui::views::editor(ui))
        })
        .unwrap();
    let mut agent = RuntimeAgentSession::new_scaled(doc, SIZE.0, SIZE.1, SCALE).unwrap();
    agent.set_gpu_renderers(nanatype_frame::ui::sweep::renderers());
    let mut shot: Option<PathBuf> = None;
    for f in 0..FRAMES {
        let path = dir.join(format!("frame-{f:04}.png"));
        if step(f) {
            // the canvas's shaders compile off-thread: wait for them before shooting
            agent.flush().unwrap();
            let start = Instant::now();
            loop {
                let _ = agent.screenshot_rgba().unwrap();
                if cache_stats().pending == 0 {
                    break;
                }
                assert!(start.elapsed().as_secs() < 30, "shader warmup timed out");
                std::thread::sleep(Duration::from_millis(5));
                agent.flush().unwrap();
            }
            agent.screenshot_png(&path).unwrap();
            shot = Some(path);
        } else {
            // a still state: the same picture as the last change
            std::fs::copy(shot.as_ref().unwrap(), &path).unwrap();
        }
    }
}

/// A Lua plugin in the editor's Lua mode: the counter example's source, and its
/// returned view updating as lines play (the script runs in the real Lua sandbox).
fn lua(out: &Path, registry: &SkinRegistry, locale: Locale) {
    const PLAYS: [u64; 4] = [24, 48, 72, 96];
    let dir = out.join("editor-lua");
    std::fs::create_dir_all(&dir).unwrap();
    let archive =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/examples/counter-plugin.ntpkg");
    let mut s = session::open_archive(&archive).unwrap();
    validate(registry, &mut s);
    s.select("main.lua");
    let mut sandbox = nanatype_plugins::LuaSandbox::new(include_str!(
        "../../docs/examples/counter-plugin/main.lua"
    ))
    .unwrap();
    let mut log = Vec::new();
    let mut view = |s: &Session, event: &str, log: &mut Vec<String>| {
        let update = sandbox.event(&serde_json::json!({"type": event})).unwrap();
        log.push(format!("→ {event}"));
        let mut v = s.view(None, false);
        v.debug.trusted = true;
        v.debug.running = true;
        v.debug.update = Some(update);
        v.debug.log = log.clone();
        Arc::new(v)
    };
    let ui = Ui::new(
        Arc::new(Settings::default()),
        Arc::new(Default::default()),
        locale,
    );
    ui.package_editor.set(Some(view(&s, "start", &mut log)));
    ui.mode.set(Mode::Lua);
    capture(&dir, ui, |f| {
        if PLAYS.contains(&f) {
            ui.package_editor
                .set(Some(view(&s, "line_start", &mut log)));
            return true;
        }
        f == 0
    });
    let steps: Vec<_> = PLAYS.iter().map(|f| (*f, "show", "lua-line")).collect();
    write_manifest(&dir, "editor-lua", &steps);
}

fn write_manifest(dir: &Path, name: &str, steps: &[(u64, &str, &str)]) {
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .args(args)
            .current_dir(env!("CARGO_MANIFEST_DIR"))
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
    };
    let rev = git(&["rev-parse", "--short=9", "HEAD"]).unwrap_or_else(|| "unknown".into());
    let dirty =
        git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
    let events: Vec<_> = steps
        .iter()
        .map(|(f, kind, label)| serde_json::json!({"t_ms": f * 1000 / FPS, "kind": kind, "label": label}))
        .collect();
    let manifest = serde_json::json!({
        "schema": 1,
        "name": name,
        "source": {"repo": "NanaType", "rev": rev, "dirty": dirty},
        "fps": FPS,
        "size": [(SIZE.0 as f32 * SCALE) as u32, (SIZE.1 as f32 * SCALE) as u32],
        "scale": SCALE,
        "theme": "light",
        "count": FRAMES,
        "alpha": "straight",
        "dir": ".",
        "pattern": "frame-%04d.png",
        "events": events,
        "capturedAt": utc_now(),
    });
    std::fs::write(
        dir.join("capture.json"),
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .unwrap();
}

/// ISO-8601 UTC wall time (metadata only; frames use the capture clock).
fn utc_now() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let (days, rem) = (secs.div_euclid(86_400), secs.rem_euclid(86_400));
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}
