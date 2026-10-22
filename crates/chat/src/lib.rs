//! Live chat events every source normalizes into, and the lenient JSON
//! readers they share. Sources keep their own wire parsers.
pub mod json;
pub use json::{int, millis, string, url};

/// Guard membership level: 1 is the highest.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Guard {
    #[default]
    None,
    /// 总督
    Governor,
    /// 提督
    Admiral,
    /// 舰长
    Captain,
}

impl Guard {
    pub fn from_privilege(value: i64) -> Self {
        match value {
            1 => Self::Governor,
            2 => Self::Admiral,
            3 => Self::Captain,
            _ => Self::None,
        }
    }
}

/// Who sent a message.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Author {
    /// Bilibili UID; empty if the server does not supply sender identity.
    pub uid: String,
    pub name: String,
    /// `https://` avatar URL; empty when the server gave none.
    pub avatar_url: String,
    pub guard: Guard,
    /// The room's owner.
    pub owner: bool,
    /// A room moderator.
    pub admin: bool,
    pub medal_level: u32,
    pub medal_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChatEvent {
    /// A danmaku: text, or a single emoticon picture.
    Text {
        id: String,
        author: Author,
        content: String,
        /// `https://` URL of the emoticon when the danmaku is one.
        emoticon: Option<String>,
        /// Unix milliseconds.
        at_ms: u64,
    },
    Gift {
        id: String,
        author: Author,
        gift_name: String,
        num: u32,
        /// Paid value in gold coins (1000 = ¥1); 0 for free gifts.
        total_coin: u64,
        at_ms: u64,
    },
    /// Someone bought a guard membership (上舰).
    Member {
        id: String,
        author: Author,
        num: u32,
        /// "月" and the like.
        unit: String,
        total_coin: u64,
        at_ms: u64,
    },
    SuperChat {
        id: String,
        author: Author,
        /// Price in yuan.
        price: u64,
        content: String,
        at_ms: u64,
    },
    /// Super Chats taken down by the room.
    DeleteSuperChats { ids: Vec<String> },
}
