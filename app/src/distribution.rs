//! What this build ships. Steam updates through SteamPipe while the regular
//! build uses the GitHub Releases updater.
use nanatype_settings::{BliveRoomKind, BlivechatSettings};

/// Built for Steam: Workshop content, no self-update.
pub const STEAM: bool = cfg!(feature = "steam");
/// The GitHub Releases update check.
pub const UPDATES: bool = cfg!(feature = "updates");

pub const LICENSE_LABEL: &str = "NanaType · MIT";
pub const LICENSE_TEXT: &str = concat!(
    include_str!("../../LICENSE"),
    "\n",
    include_str!("../../LICENSE-MIT")
);

/// How blivechat joins a room.
pub fn blive_room_kind(settings: &BlivechatSettings) -> BliveRoomKind {
    settings.room_kind
}
