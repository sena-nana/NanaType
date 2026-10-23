//! NanaType's own events (docs/logging.md). Typed fields carry codes and
//! counts; messages go through `nanatype_diag::report!`, which redacts them.

use nanatype_diag::domains::{APP, BILIBILI, CHAT, OUTPUT, STEAM, STICKERS};
use nanatype_diag::{EventDescriptor, FieldDescriptor, Severity};

use crate::windows::WindowKind;

pub static SETTINGS_LOADED: EventDescriptor = EventDescriptor::new(
    APP,
    1,
    "app.settings_loaded",
    Severity::Info,
    // 0 fresh, 1 loaded, 2 migrated, 3 quarantined, 4 newer build.
    &[FieldDescriptor::u64("note"), FieldDescriptor::u64("from_schema")],
);
pub static SETTINGS_QUARANTINED: EventDescriptor =
    EventDescriptor::new(APP, 2, "app.settings_quarantined", Severity::Error, &[]);
/// Every error notice the user sees, as shown.
pub static ERROR_NOTICE: EventDescriptor =
    EventDescriptor::new(APP, 3, "app.error_notice", Severity::Warn, &[]);
pub static EDIT_FAILED: EventDescriptor = EventDescriptor::new(
    APP,
    4,
    "app.edit_failed",
    Severity::Warn,
    // 0 editor action, 1 source edit, 2 editor bindings, 3 rich edit.
    &[FieldDescriptor::u64("kind")],
);
pub static INPUT_READY: EventDescriptor = EventDescriptor::new(
    APP,
    5,
    "app.input_ready",
    Severity::Info,
    &[FieldDescriptor::u64("since_launch_ms")],
);
pub static WINDOW_VISIBILITY: EventDescriptor = EventDescriptor::new(
    APP,
    6,
    "app.window_visibility",
    Severity::Debug,
    &[
        FieldDescriptor::u64("window"),
        FieldDescriptor::bool("visible"),
        FieldDescriptor::bool("placed"),
        FieldDescriptor::bool("monitors_known"),
    ],
);
pub static DISPLAYS_UNAVAILABLE: EventDescriptor =
    EventDescriptor::new(APP, 7, "app.displays_unavailable", Severity::Warn, &[]);
pub static WATCH_UNAVAILABLE: EventDescriptor =
    EventDescriptor::new(APP, 8, "app.watch_unavailable", Severity::Warn, &[]);
pub static PERSIST_FAILED: EventDescriptor =
    EventDescriptor::new(APP, 9, "app.persist_failed", Severity::Error, &[]);

pub static SPOUT_FAILED: EventDescriptor = EventDescriptor::new(
    OUTPUT,
    1,
    "output.spout_failed",
    Severity::Warn,
    &[FieldDescriptor::u64("window")],
);
pub static WINDOW_OUTPUT: EventDescriptor = EventDescriptor::new(
    OUTPUT,
    2,
    "output.window_status",
    Severity::Info,
    &[FieldDescriptor::u64("window")],
);
pub static BRIDGE_ERROR: EventDescriptor = EventDescriptor::new(
    OUTPUT,
    3,
    "output.bridge_error",
    Severity::Warn,
    // 0 VTube Studio, 1 OSC, 2 WebSocket.
    &[FieldDescriptor::u64("sink")],
);

pub static STEAM_UNAVAILABLE: EventDescriptor =
    EventDescriptor::new(STEAM, 1, "steam.unavailable", Severity::Info, &[]);

pub static STICKER_PREVIEW_FAILED: EventDescriptor =
    EventDescriptor::new(STICKERS, 1, "stickers.preview_failed", Severity::Warn, &[]);
pub static STICKER_FRAME_REBUILD: EventDescriptor =
    EventDescriptor::new(STICKERS, 2, "stickers.frame_rebuild", Severity::Warn, &[]);
pub static STICKER_UPLOAD_FAILED: EventDescriptor =
    EventDescriptor::new(STICKERS, 3, "stickers.upload_failed", Severity::Warn, &[]);

pub static CHAT_PICTURE_FAILED: EventDescriptor =
    EventDescriptor::new(CHAT, 1, "chat.picture_failed", Severity::Debug, &[]);

pub static BLIVECHAT_SECRET_FAILED: EventDescriptor = EventDescriptor::new(
    BILIBILI,
    100,
    "bilibili.blivechat_secret_failed",
    Severity::Warn,
    // 0 load, 1 store.
    &[FieldDescriptor::u64("op")],
);
pub static BLIVECHAT_ERROR: EventDescriptor =
    EventDescriptor::new(BILIBILI, 101, "bilibili.blivechat_error", Severity::Warn, &[]);
pub static BLIVECHAT_FATAL: EventDescriptor =
    EventDescriptor::new(BILIBILI, 102, "bilibili.blivechat_fatal", Severity::Error, &[]);
pub static COMET_RELAY_ERROR: EventDescriptor =
    EventDescriptor::new(BILIBILI, 103, "bilibili.comet_relay_error", Severity::Warn, &[]);

pub static EVENTS: &[&EventDescriptor] = &[
    &SETTINGS_LOADED,
    &SETTINGS_QUARANTINED,
    &ERROR_NOTICE,
    &EDIT_FAILED,
    &INPUT_READY,
    &WINDOW_VISIBILITY,
    &DISPLAYS_UNAVAILABLE,
    &WATCH_UNAVAILABLE,
    &PERSIST_FAILED,
    &SPOUT_FAILED,
    &WINDOW_OUTPUT,
    &BRIDGE_ERROR,
    &STEAM_UNAVAILABLE,
    &STICKER_PREVIEW_FAILED,
    &STICKER_FRAME_REBUILD,
    &STICKER_UPLOAD_FAILED,
    &CHAT_PICTURE_FAILED,
    &BLIVECHAT_SECRET_FAILED,
    &BLIVECHAT_ERROR,
    &BLIVECHAT_FATAL,
    &COMET_RELAY_ERROR,
];

/// A window's code in event fields.
pub fn window(kind: WindowKind) -> u64 {
    match kind {
        WindowKind::Input => 0,
        WindowKind::Display => 1,
        WindowKind::PeerDisplay => 2,
        WindowKind::History => 3,
        WindowKind::Settings => 4,
        WindowKind::Chat => 5,
    }
}

/// The facts every session log starts with. Called once settings are
/// loaded; nothing here identifies the user.
pub fn session_info(settings: &nanatype_settings::Settings, locale: &str) {
    use crate::distribution::{STEAM, UNOFFICIAL_CHAT, UPDATES};
    let edition = if STEAM { "steam" } else { "standalone" };
    nanatype_diag::set_session_info("edition", edition);
    nanatype_diag::set_session_info(
        "features",
        format!("updates={UPDATES} unofficial_chat={UNOFFICIAL_CHAT}"),
    );
    nanatype_diag::set_session_info(
        "os",
        format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
    );
    nanatype_diag::set_session_info("locale", locale);
    nanatype_diag::set_session_info("skin", settings.skin.id.as_str());
    nanatype_diag::set_session_info("verbose", nanatype_diag::verbose().to_string());
}
