//! What this build ships. The default build is the GitHub release; a Steam
//! build (`--no-default-features --features builtin-fonts,steam`) updates
//! through SteamPipe and leaves the reverse-engineered chat sources out
//! (docs/steam.md). Both read and write the same settings, so these switches
//! only hide and skip things; they never rewrite what the other build saved.
use nanatype_settings::{BliveRoomKind, BlivechatSettings};

/// Built for Steam: Workshop content, no self-update.
pub const STEAM: bool = cfg!(feature = "steam");
/// The GitHub Releases update check.
pub const UPDATES: bool = cfg!(feature = "updates");
/// Bilibili web login and danmaku, Douyin and YouTube.
pub const UNOFFICIAL_CHAT: bool = cfg!(feature = "unofficial-chat");

/// The project licence row on the About page: the Steam build leaves out the
/// CC-BY-NC-4.0 Bilibili material, so its combined licence is plain MIT.
#[cfg(feature = "unofficial-chat")]
pub const LICENSE_LABEL: &str = "NanaType · MIT AND CC-BY-NC-4.0";
#[cfg(feature = "unofficial-chat")]
pub const LICENSE_TEXT: &str = concat!(
    include_str!("../../LICENSE"),
    "\n",
    include_str!("../../LICENSE-MIT")
);
#[cfg(not(feature = "unofficial-chat"))]
pub const LICENSE_LABEL: &str = "NanaType · MIT";
#[cfg(not(feature = "unofficial-chat"))]
pub const LICENSE_TEXT: &str = concat!(
    include_str!("../../packaging/steam/LICENSE"),
    "\n",
    include_str!("../../LICENSE-MIT")
);

/// How blivechat joins a room. A room number makes blivechat read Bilibili's
/// web danmaku; without the unofficial sources only the open platform's auth
/// code is offered, whatever the settings say.
pub fn blive_room_kind(settings: &BlivechatSettings) -> BliveRoomKind {
    if UNOFFICIAL_CHAT {
        settings.room_kind
    } else {
        BliveRoomKind::AuthCode
    }
}
