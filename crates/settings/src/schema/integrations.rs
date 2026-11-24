use std::collections::BTreeMap;

use nanatype_model::VtsReset;
use serde::{Deserialize, Serialize};

use super::{ChatFilter, trim_take};

pub const BLIVECHAT_PORT: u16 = 12450;
pub const COMET_PORT: u16 = 19400;
pub const LAPLACE_URL: &str = "ws://127.0.0.1:9696/";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VtsSettings {
    pub enabled: bool,
    pub port: u16,
    /// Plugin token granted by VTube Studio.
    pub token: Option<String>,
    /// Expression name → VTube Studio hotkey id.
    pub expressions: BTreeMap<String, String>,
    /// Move the model's mouth while the speaker is talking.
    pub lip_sync: bool,
    /// The parameter lip sync drives.
    pub mouth_param: String,
    /// How far the mouth opens (the parameter's peak value).
    pub mouth_amp: f32,
    /// When expressions turned on by a line are turned off again.
    pub reset_expressions: VtsReset,
}

impl Default for VtsSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            port: 8001,
            token: None,
            expressions: BTreeMap::new(),
            lip_sync: true,
            mouth_param: "MouthOpen".into(),
            mouth_amp: 1.0,
            reset_expressions: VtsReset::LineEnd,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OscSettings {
    pub enabled: bool,
    pub host: String,
    pub port: u16,
    pub prefix: String,
}

impl Default for OscSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            host: "127.0.0.1".into(),
            port: 9000,
            prefix: "/nanatype".into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WsSettings {
    pub enabled: bool,
    pub port: u16,
}

impl Default for WsSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            port: 41890,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatPanelStyle {
    #[default]
    Compact,
    History,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ChatSettings {
    /// What the chat panel lists.
    pub panel: ChatFilter,
    /// Presentation of the operator's panel, independent of its message filter.
    pub panel_style: ChatPanelStyle,
    /// Quote the answered message on the dialogue box.
    pub quote_on_display: bool,
}

impl Default for ChatSettings {
    fn default() -> Self {
        Self {
            panel: ChatFilter::default(),
            panel_style: ChatPanelStyle::default(),
            quote_on_display: true,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TwitchSettings {
    pub enabled: bool,
    /// A channel login or its twitch.tv address.
    pub channel: String,
}

/// Where Bilibili chat comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BilibiliSource {
    /// The danmaku page's OBS link: a blivechat room or a LAPLACE bridge.
    #[default]
    Link,
    /// The 彗星号 client's Bilibili rooms.
    Comet,
}

/// Which kind of link the user last pasted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BilibiliLink {
    /// A blivechat room, in [`BlivechatSettings`].
    #[default]
    Blivechat,
    /// A LAPLACE Event Bridge, in [`LaplaceSettings`].
    Laplace,
}

/// Bilibili live chat.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct BilibiliSettings {
    pub enabled: bool,
    pub source: BilibiliSource,
    pub link: BilibiliLink,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BliveRoomKind {
    /// The streamer's 身份码, kept in the system's secret store.
    #[default]
    AuthCode,
    RoomId,
}

/// A blivechat room, as read from its room link.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BlivechatSettings {
    pub host: String,
    pub port: u16,
    pub room_kind: BliveRoomKind,
    /// The room number when `room_kind` is `RoomId`. An auth code is never
    /// written here.
    pub room_id: String,
}

impl Default for BlivechatSettings {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".into(),
            port: BLIVECHAT_PORT,
            room_kind: BliveRoomKind::default(),
            room_id: String::new(),
        }
    }
}

/// A LAPLACE Event Bridge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LaplaceSettings {
    /// The bridge's link without its token (`ws://` or `wss://`), as read
    /// from what the user pasted. The token is kept in the system's secret
    /// store.
    pub url: String,
}

impl Default for LaplaceSettings {
    fn default() -> Self {
        Self {
            url: LAPLACE_URL.into(),
        }
    }
}

/// The 彗星号 client on this computer. One connection serves Bilibili (when
/// [`BilibiliSettings::source`] picks it) and the other platforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CometSettings {
    /// The port the client serves its feed on.
    pub port: u16,
    /// Read the rooms it listens to on platforms other than Bilibili.
    pub other_platforms: bool,
}

impl Default for CometSettings {
    fn default() -> Self {
        Self {
            port: COMET_PORT,
            other_platforms: false,
        }
    }
}

/// Which 彗星号 rooms are read: Bilibili's, the other platforms', or both.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CometRooms {
    pub bilibili: bool,
    pub others: bool,
}

/// Where live chat comes from; one at a time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatSource {
    Bilibili,
    Twitch,
    /// 彗星号's rooms on platforms other than Bilibili.
    OtherPlatforms,
}

impl ChatSource {
    /// Settings order.
    pub const ALL: &[Self] = &[Self::Bilibili, Self::Twitch, Self::OtherPlatforms];
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Integrations {
    pub vts: VtsSettings,
    pub osc: OscSettings,
    pub websocket: WsSettings,
    pub chat: ChatSettings,
    pub bilibili: BilibiliSettings,
    pub blivechat: BlivechatSettings,
    pub laplace: LaplaceSettings,
    pub comet: CometSettings,
    pub twitch: TwitchSettings,
}

impl Integrations {
    fn bilibili_link(&self, link: BilibiliLink) -> bool {
        let b = &self.bilibili;
        b.enabled && b.source == BilibiliSource::Link && b.link == link
    }

    /// Bilibili chat comes from a blivechat room.
    pub fn blivechat_on(&self) -> bool {
        self.bilibili_link(BilibiliLink::Blivechat)
    }

    /// Bilibili chat comes from a LAPLACE bridge.
    pub fn laplace_on(&self) -> bool {
        self.bilibili_link(BilibiliLink::Laplace)
    }

    /// What the 彗星号 connection is for; it is off when neither is read.
    pub fn comet_rooms(&self) -> CometRooms {
        let b = &self.bilibili;
        CometRooms {
            bilibili: b.enabled && b.source == BilibiliSource::Comet,
            others: self.comet.other_platforms,
        }
    }

    /// The one chat source that is on. Should a file have more (an older
    /// one, or one edited by hand), the first in settings order wins.
    pub fn chat_source(&self) -> Option<ChatSource> {
        ChatSource::ALL
            .iter()
            .copied()
            .find(|&s| self.chat_source_on(s))
    }

    pub fn chat_source_on(&self, source: ChatSource) -> bool {
        match source {
            ChatSource::Bilibili => self.bilibili.enabled,
            ChatSource::Twitch => self.twitch.enabled,
            ChatSource::OtherPlatforms => self.comet.other_platforms,
        }
    }

    /// Turn `source` on or off. Only one connects at a time, so turning one
    /// on turns the others off.
    pub fn set_chat_source(&mut self, source: ChatSource, on: bool) {
        for s in ChatSource::ALL.iter().copied() {
            let value = if s == source {
                on
            } else {
                !on && self.chat_source_on(s)
            };
            match s {
                ChatSource::Bilibili => self.bilibili.enabled = value,
                ChatSource::Twitch => self.twitch.enabled = value,
                ChatSource::OtherPlatforms => self.comet.other_platforms = value,
            }
        }
    }

    pub(crate) fn sanitize(&mut self) -> bool {
        let mut dirty = false;
        if let Some(source) = self.chat_source() {
            for s in ChatSource::ALL.iter().copied() {
                let want = s == source;
                if self.chat_source_on(s) != want {
                    dirty = true;
                }
            }
            if dirty {
                self.set_chat_source(source, true);
            }
        }
        trim_take(&mut self.twitch.channel, 2048, &mut dirty);
        trim_take(&mut self.blivechat.host, 253, &mut dirty);
        if self.blivechat.host.is_empty() {
            self.blivechat.host = "127.0.0.1".into();
            dirty = true;
        }
        if self.blivechat.port == 0 {
            self.blivechat.port = BLIVECHAT_PORT;
            dirty = true;
        }
        trim_take(&mut self.blivechat.room_id, 20, &mut dirty);
        if self.comet.port == 0 {
            self.comet.port = COMET_PORT;
            dirty = true;
        }
        trim_take(&mut self.laplace.url, 2048, &mut dirty);
        if self.laplace.url.is_empty() {
            self.laplace.url = LAPLACE_URL.into();
            dirty = true;
        }
        dirty
    }
}
