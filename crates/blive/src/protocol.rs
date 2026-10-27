//! blivechat's client protocol: `{"cmd": n, "data": ...}` frames.
//!
//! Text messages are positional arrays that grew over versions (16 fields in
//! v1.7, 19 in v1.10); gifts, memberships and Super Chats are objects. Every
//! field is read leniently, so a newer or older server never drops a message.
//! The events use the shared [`ChatEvent`] shape, so chat from
//! blivechat reaches the panel the same way as chat read directly.

use serde_json::{Value, json};

pub(crate) const HEARTBEAT: i64 = 0;
const JOIN_ROOM: i64 = 1;
pub(crate) const ADD_TEXT: i64 = 2;
const ADD_GIFT: i64 = 3;
const ADD_MEMBER: i64 = 4;
const ADD_SUPER_CHAT: i64 = 5;
pub(crate) const DEL_SUPER_CHAT: i64 = 6;
const FATAL_ERROR: i64 = 8;
/// What a source can push: text, gift, member, Super Chat, deletion.
pub(crate) const CHAT_COMMANDS: [i64; 5] = [
    ADD_TEXT,
    ADD_GIFT,
    ADD_MEMBER,
    ADD_SUPER_CHAT,
    DEL_SUPER_CHAT,
];
/// Index of the message id in an `ADD_TEXT` array.
pub(crate) const TEXT_ID: usize = 11;

/// Which room to join.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RoomKey {
    /// A live room number.
    RoomId(u64),
    /// The streamer's 身份码 (open platform auth code).
    AuthCode(String),
}

impl RoomKey {
    /// Read what the user typed: a room number or an auth code.
    pub fn parse(auth_code: bool, text: &str) -> Option<Self> {
        let text = text.trim();
        if text.is_empty() {
            return None;
        }
        if auth_code {
            Some(Self::AuthCode(text.to_owned()))
        } else {
            text.parse().ok().filter(|id| *id > 0).map(Self::RoomId)
        }
    }
}

pub use nanatype_chat::{Author, ChatEvent, Guard};
// The same lenient field readers as the relay protocol uses for its panel.
pub(crate) use nanatype_chat::{int, millis, string, url};

/// Why blivechat gave up on a room (`FATAL_ERROR`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FatalKind {
    /// The auth code is wrong; retrying will not help.
    AuthCode,
    TooManyRetries,
    TooManyConnections,
    Other,
}

/// One frame from the server.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    Heartbeat,
    Event(ChatEvent),
    /// The server gave up on the room.
    Fatal {
        kind: FatalKind,
        message: String,
    },
    /// Something this client does not use (translations, unknown commands).
    Other,
}

/// The join request. It carries `roomId` too, which v1.7 servers read.
pub fn join_message(room: &RoomKey) -> String {
    let (key, legacy) = match room {
        RoomKey::RoomId(id) => (json!({ "type": 1, "value": id }), json!(id)),
        RoomKey::AuthCode(code) => (json!({ "type": 2, "value": code }), Value::Null),
    };
    let mut data = json!({
        "roomKey": key,
        "config": { "autoTranslate": false },
    });
    if !legacy.is_null() {
        data["roomId"] = legacy;
    }
    json!({ "cmd": JOIN_ROOM, "data": data }).to_string()
}

pub const HEARTBEAT_MESSAGE: &str = r#"{"cmd":0,"data":{}}"#;

/// Read one text frame. `None` for frames that are not JSON at all.
pub fn parse(text: &str) -> Option<Incoming> {
    let frame: Value = serde_json::from_str(text).ok()?;
    let data = &frame["data"];
    Some(match frame["cmd"].as_i64()? {
        HEARTBEAT => Incoming::Heartbeat,
        ADD_TEXT => text_message(data).map_or(Incoming::Other, Incoming::Event),
        ADD_GIFT => Incoming::Event(ChatEvent::Gift {
            id: string(&data["id"]),
            author: object_author(data),
            gift_name: string(&data["giftName"]),
            num: int(&data["num"]).clamp(1, u32::MAX as i64) as u32,
            total_coin: int(&data["totalCoin"]).max(0) as u64,
            at_ms: millis(&data["timestamp"]),
        }),
        ADD_MEMBER => Incoming::Event(ChatEvent::Member {
            id: string(&data["id"]),
            author: object_author(data),
            num: int(&data["num"]).clamp(1, u32::MAX as i64) as u32,
            unit: string(&data["unit"]),
            total_coin: int(&data["total_coin"]).max(0) as u64,
            at_ms: millis(&data["timestamp"]),
        }),
        ADD_SUPER_CHAT => Incoming::Event(ChatEvent::SuperChat {
            id: string(&data["id"]),
            author: object_author(data),
            price: int(&data["price"]).max(0) as u64,
            content: string(&data["content"]),
            at_ms: millis(&data["timestamp"]),
        }),
        DEL_SUPER_CHAT => Incoming::Event(ChatEvent::DeleteSuperChats {
            ids: data["ids"]
                .as_array()
                .map(|ids| ids.iter().map(string).collect())
                .unwrap_or_default(),
        }),
        FATAL_ERROR => Incoming::Fatal {
            kind: match int(&data["type"]) {
                1 => FatalKind::AuthCode,
                2 => FatalKind::TooManyRetries,
                3 => FatalKind::TooManyConnections,
                _ => FatalKind::Other,
            },
            message: string(&data["msg"]),
        },
        _ => Incoming::Other,
    })
}

/// `ADD_TEXT`: `[avatarUrl, timestamp, authorName, authorType, content,
/// privilegeType, isGiftDanmaku, authorLevel, isNewbie, isMobileVerified,
/// medalLevel, id, translation, contentType, contentTypeParams,
/// textEmoticons, uid, medalName, isMirror]`.
fn text_message(data: &Value) -> Option<ChatEvent> {
    let a = data.as_array()?;
    let at = |i: usize| a.get(i).unwrap_or(&Value::Null);
    let author_type = int(at(3));
    let author = Author {
        uid: string(at(16)),
        name: string(at(2)),
        avatar_url: url(at(0)),
        guard: Guard::from_privilege(int(at(5))),
        owner: author_type == 3,
        admin: author_type == 2,
        medal_level: int(at(10)).max(0) as u32,
        medal_name: string(at(17)),
    };
    let emoticon = (int(at(13)) == 1)
        .then(|| at(14).get(0).map(url))
        .flatten()
        .filter(|u| !u.is_empty());
    Some(ChatEvent::Text {
        id: string(at(TEXT_ID)),
        author,
        content: string(at(4)),
        emoticon,
        at_ms: millis(at(1)),
    })
}

fn object_author(data: &Value) -> Author {
    Author {
        uid: string(&data["uid"]),
        name: string(&data["authorName"]),
        avatar_url: url(&data["avatarUrl"]),
        guard: Guard::from_privilege(int(&data["privilegeType"])),
        owner: false,
        admin: false,
        medal_level: int(&data["medalLevel"]).max(0) as u32,
        medal_name: string(&data["medalName"]),
    }
}
