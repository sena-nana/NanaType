//! NanaType's own events (docs/logging.md). Typed fields carry codes and
//! counts; messages go through `nanatype_diag::report!`, which redacts them.

use nanatype_diag::domains::{
    APP, AUDIO, BILIBILI, CHAT, DOUYIN, OUTPUT, STEAM, STICKERS, TWITCH, YOUTUBE,
};
use nanatype_diag::{EventDescriptor, FieldDescriptor, Metric, Severity};

use crate::windows::WindowKind;

pub static SETTINGS_LOADED: EventDescriptor = EventDescriptor::new(
    APP,
    1,
    "app.settings_loaded",
    Severity::Info,
    // 0 fresh, 1 loaded, 2 migrated, 3 quarantined, 4 newer build.
    &[
        FieldDescriptor::u64("note"),
        FieldDescriptor::u64("from_schema"),
    ],
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

/// The output device opened; its name is not recorded.
pub static AUDIO_RUNNING: EventDescriptor = EventDescriptor::new(
    AUDIO,
    1,
    "audio.running",
    Severity::Info,
    &[FieldDescriptor::u64("rate")],
);
pub static AUDIO_FAILED: EventDescriptor = EventDescriptor::new(
    AUDIO,
    2,
    "audio.failed",
    Severity::Warn,
    // 0 could not open, 1 lost while playing.
    &[FieldDescriptor::bool("lost")],
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

/// The obs-websocket link failed, dropped or had a request refused.
pub static OBS_ERROR: EventDescriptor =
    EventDescriptor::new(OUTPUT, 4, "output.obs_error", Severity::Warn, &[]);
pub static OBS_SECRET_FAILED: EventDescriptor = EventDescriptor::new(
    OUTPUT,
    5,
    "output.obs_secret_failed",
    Severity::Warn,
    // 0 load, 1 store.
    &[FieldDescriptor::u64("op")],
);

pub static BLIVECHAT_SECRET_FAILED: EventDescriptor = EventDescriptor::new(
    BILIBILI,
    100,
    "bilibili.blivechat_secret_failed",
    Severity::Warn,
    // 0 load, 1 store.
    &[FieldDescriptor::u64("op")],
);
pub static BLIVECHAT_ERROR: EventDescriptor = EventDescriptor::new(
    BILIBILI,
    101,
    "bilibili.blivechat_error",
    Severity::Warn,
    &[],
);
pub static BLIVECHAT_FATAL: EventDescriptor = EventDescriptor::new(
    BILIBILI,
    102,
    "bilibili.blivechat_fatal",
    Severity::Error,
    &[],
);
pub static COMET_RELAY_ERROR: EventDescriptor = EventDescriptor::new(
    BILIBILI,
    103,
    "bilibili.comet_relay_error",
    Severity::Warn,
    &[],
);

/// One live platform's connection and account, in its own domain. States
/// are codes (see [`Conn`] and [`Account`]); a room, channel, account name or
/// chat line is never recorded.
pub struct Live {
    pub connection: EventDescriptor,
    pub connection_error: EventDescriptor,
    pub account: EventDescriptor,
    pub account_failed: EventDescriptor,
    pub warning: EventDescriptor,
    pub delivery_failed: EventDescriptor,
    /// Chat events received.
    pub chat: Metric,
}

const STATE: &[FieldDescriptor] = &[FieldDescriptor::u64("state")];

macro_rules! live {
    ($name:ident, $domain:expr, $prefix:literal) => {
        pub static $name: Live = Live {
            connection: EventDescriptor::new(
                $domain,
                1,
                concat!($prefix, ".connection"),
                Severity::Info,
                STATE,
            ),
            connection_error: EventDescriptor::new(
                $domain,
                2,
                concat!($prefix, ".connection_error"),
                Severity::Warn,
                &[],
            ),
            account: EventDescriptor::new(
                $domain,
                3,
                concat!($prefix, ".account"),
                Severity::Info,
                STATE,
            ),
            account_failed: EventDescriptor::new(
                $domain,
                4,
                concat!($prefix, ".account_failed"),
                Severity::Warn,
                &[],
            ),
            warning: EventDescriptor::new(
                $domain,
                5,
                concat!($prefix, ".warning"),
                Severity::Warn,
                &[],
            ),
            delivery_failed: EventDescriptor::new(
                $domain,
                6,
                concat!($prefix, ".delivery_failed"),
                Severity::Warn,
                // 0 failed, 1 uncertain.
                STATE,
            ),
            chat: Metric::counter($domain, 50, concat!($prefix, ".chat_events"), "count"),
        };
    };
}

live!(BILIBILI_LIVE, BILIBILI, "bilibili");
live!(DOUYIN_LIVE, DOUYIN, "douyin");
live!(YOUTUBE_LIVE, YOUTUBE, "youtube");
live!(TWITCH_LIVE, TWITCH, "twitch");

/// Connection states shared by every platform.
#[derive(Clone, Copy)]
pub enum Conn {
    Off = 0,
    /// Resolving, waiting for a broadcast or connecting.
    Connecting = 1,
    Connected = 2,
    /// Disconnected or the broadcast ended; the platform may retry.
    Disconnected = 3,
    Error = 4,
    NeedsLogin = 5,
}

/// Account states shared by every platform.
#[derive(Clone, Copy)]
pub enum Account {
    Restoring = 0,
    SignedOut = 1,
    /// A QR code or device code is waiting, or a cookie is unverified.
    Pending = 2,
    Ready = 3,
    Expired = 4,
    Failed = 5,
}

impl Live {
    pub fn connection(&'static self, state: Conn, error: Option<&str>) {
        nanatype_diag::event!(self.connection, state = state as u64);
        if let Some(error) = error {
            nanatype_diag::report!(self.connection_error; "{error}");
        }
    }

    pub fn account(&'static self, state: Account, error: Option<&str>) {
        nanatype_diag::event!(self.account, state = state as u64);
        if let Some(error) = error {
            nanatype_diag::report!(self.account_failed; "{error}");
        }
    }

    pub fn warning(&'static self, code: &str) {
        nanatype_diag::report!(self.warning; "{code}");
    }

    pub fn delivery_failed(&'static self, uncertain: bool) {
        nanatype_diag::event!(self.delivery_failed, state = u64::from(uncertain));
    }

    pub fn chat(&'static self) {
        nanatype_diag::metric!(self.chat);
    }

    fn events(&'static self) -> [&'static EventDescriptor; 6] {
        [
            &self.connection,
            &self.connection_error,
            &self.account,
            &self.account_failed,
            &self.warning,
            &self.delivery_failed,
        ]
    }
}

/// Every event NanaType declares, for the catalog test.
pub fn all_events() -> Vec<&'static EventDescriptor> {
    let mut all = EVENTS.to_vec();
    for live in [&BILIBILI_LIVE, &DOUYIN_LIVE, &YOUTUBE_LIVE, &TWITCH_LIVE] {
        all.extend(live.events());
    }
    all
}

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
    &OBS_ERROR,
    &OBS_SECRET_FAILED,
    &AUDIO_RUNNING,
    &AUDIO_FAILED,
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
