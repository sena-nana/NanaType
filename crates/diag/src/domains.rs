//! Event domains. NanaUI keeps `0x0000..=0x00FF`; applications start at
//! `0x0100`. An event's name starts with its domain's prefix.

use nana_diagnostics::Domain;

pub const APP: Domain = Domain(0x0100);
pub const SETTINGS: Domain = Domain(0x0101);
/// Spout, Syphon, OBS and VTube Studio.
pub const OUTPUT: Domain = Domain(0x0102);
pub const CHAT: Domain = Domain(0x0103);
/// Bilibili web danmaku and blivechat.
pub const BILIBILI: Domain = Domain(0x0104);
pub const DOUYIN: Domain = Domain(0x0105);
pub const YOUTUBE: Domain = Domain(0x0106);
pub const TWITCH: Domain = Domain(0x0107);
pub const P2P: Domain = Domain(0x0108);
pub const PLUGINS: Domain = Domain(0x0109);
/// What plugins write with `nana.log`.
pub const PLUGIN_LOG: Domain = Domain(0x010A);
pub const UPDATE: Domain = Domain(0x010B);
pub const STEAM: Domain = Domain(0x010C);
pub const STICKERS: Domain = Domain(0x010D);
pub const SKIN: Domain = Domain(0x010E);
pub const AUDIO: Domain = Domain(0x010F);
pub const EDITOR: Domain = Domain(0x0110);
/// Dependencies' `log` / `tracing` output.
pub const EXTERNAL: Domain = Domain(0x01F0);

/// Every domain with the name prefix of its events.
pub const ALL: &[(Domain, &str)] = &[
    (APP, "app."),
    (SETTINGS, "settings."),
    (OUTPUT, "output."),
    (CHAT, "chat."),
    (BILIBILI, "bilibili."),
    (DOUYIN, "douyin."),
    (YOUTUBE, "youtube."),
    (TWITCH, "twitch."),
    (P2P, "p2p."),
    (PLUGINS, "plugins."),
    (PLUGIN_LOG, "plugin_log."),
    (UPDATE, "update."),
    (STEAM, "steam."),
    (STICKERS, "stickers."),
    (SKIN, "skin."),
    (AUDIO, "audio."),
    (EDITOR, "editor."),
    (EXTERNAL, "external."),
];

/// The name prefix of `domain`'s events, if it is one of NanaType's.
pub fn prefix(domain: Domain) -> Option<&'static str> {
    ALL.iter().find(|(d, _)| *d == domain).map(|(_, p)| *p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn domains_are_application_range_and_unique() {
        for (i, (domain, prefix)) in ALL.iter().enumerate() {
            assert!(domain.0 >= Domain::APPLICATION_MIN, "{prefix}");
            assert!(prefix.ends_with('.'), "{prefix}");
            for (other, other_prefix) in &ALL[i + 1..] {
                assert_ne!(domain, other, "{prefix} / {other_prefix}");
                assert_ne!(prefix, other_prefix);
            }
        }
    }
}
