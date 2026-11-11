//! 彗星号: the live assistant's own WebSocket feed.
//!
//! The 彗星号 client serves every room it listens to on
//! `ws://127.0.0.1:19400/` (`/<platform>/<rid>` narrows it to one room). A
//! frame is either JSON text or binary: a 12-byte nonce, then AES-256-GCM
//! ciphertext with its 16-byte tag, under a key every client shares. Both
//! hold one message, `{ type, platform, rid, data }`; `pong` answers a
//! `ping`.
//!
//! Chat, gifts, memberships and Super Chats are read into the same
//! [`ChatEvent`]s as blivechat's, every field leniently; entries, likes and
//! listener states are not chat. Users of other platforms than Bilibili get
//! their platform before their uid (`dou_yin:…`), so they never share one
//! with a Bilibili user.

use ring::aead::{AES_256_GCM, Aad, LessSafeKey, NONCE_LEN, Nonce, UnboundKey};
use serde_json::Value;

use crate::chat::{Author, ChatEvent, Guard, int, millis, string, url};
use crate::protocol::Incoming;

/// The port the 彗星号 client listens on by default.
pub const DEFAULT_PORT: u16 = 19400;

/// What NanaType sends to keep the line alive; the client answers `pong`.
pub const PING: &str = "ping";
const PONG: &str = "pong";

/// The key every 彗星号 client encrypts its binary frames with.
const KEY: [u8; 32] = [
    0xab, 0x76, 0x28, 0x4b, 0x0d, 0x53, 0x35, 0x5d, 0xb5, 0x00, 0x80, 0xba, 0x17, 0x26, 0x81, 0x46,
    0x0c, 0x59, 0x1c, 0xea, 0x69, 0xc7, 0xe8, 0x48, 0x92, 0x95, 0x3a, 0x46, 0xe1, 0x51, 0x13, 0x9b,
];

fn key() -> LessSafeKey {
    LessSafeKey::new(UnboundKey::new(&AES_256_GCM, &KEY).expect("a 256-bit key"))
}

/// The text of a binary frame: nonce, ciphertext, tag. `None` when it does
/// not open under the key.
pub fn decrypt(frame: &[u8]) -> Option<String> {
    if frame.len() < NONCE_LEN {
        return None;
    }
    let (nonce, sealed) = frame.split_at(NONCE_LEN);
    let nonce = Nonce::try_assume_unique_for_key(nonce).ok()?;
    let mut sealed = sealed.to_vec();
    let plain = key().open_in_place(nonce, Aad::empty(), &mut sealed).ok()?;
    String::from_utf8(plain.to_vec()).ok()
}

/// A frame as the client sends it, for tests and tools.
pub fn encrypt(text: &str, nonce: [u8; NONCE_LEN]) -> Vec<u8> {
    let mut sealed = text.as_bytes().to_vec();
    key()
        .seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce),
            Aad::empty(),
            &mut sealed,
        )
        .expect("a frame within AES-GCM's limits");
    let mut frame = nonce.to_vec();
    frame.extend(sealed);
    frame
}

/// Which rooms the client's feed is read for: one connection serves
/// Bilibili and the other platforms, each when asked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rooms {
    pub bilibili: bool,
    pub others: bool,
}

impl Rooms {
    pub const ALL: Self = Self {
        bilibili: true,
        others: true,
    };

    pub fn any(self) -> bool {
        self.bilibili || self.others
    }

    fn reads(self, platform: &str) -> bool {
        if platform == BILIBILI {
            self.bilibili
        } else {
            self.others
        }
    }
}

/// How the client names Bilibili; a message without a platform is one.
const BILIBILI: &str = "bili";

/// Read one message, already decrypted. `None` for text that is not a JSON
/// message; `pong` reads as [`Incoming::Heartbeat`], and a message from a
/// platform `rooms` does not read as [`Incoming::Other`].
pub fn parse(text: &str, now_ms: u64, rooms: Rooms) -> Option<Incoming> {
    if text.trim() == PONG {
        return Some(Incoming::Heartbeat);
    }
    let m: Value = serde_json::from_str(text).ok()?;
    let kind = m.as_object()?.get("type")?.as_str()?;
    let platform = m["platform"].as_str().unwrap_or(BILIBILI);
    if !rooms.reads(platform) {
        return Some(Incoming::Other);
    }
    let d = &m["data"];
    let at_ms = match millis(&d["timestamp"]) {
        0 => now_ms,
        t => t,
    };
    let id = || match string(&d["msgId"]) {
        id if !id.is_empty() => id,
        // No id: one made from what the message says, so it still keys.
        _ => format!(
            "{platform}-{kind}-{}-{at_ms}-{}",
            string(&d["id"]),
            string(&d["content"])
        ),
    };
    let count = |v: &Value| int(v).clamp(1, u32::MAX as i64) as u32;
    Some(Incoming::Event(match kind {
        "Chat" => ChatEvent::Text {
            id: id(),
            author: author(platform, d, Guard::None),
            content: string(&d["content"]),
            emoticon: (int(&d["dm_type"]) == 1)
                .then(|| url(&d["emoji_img_url"]))
                .filter(|u| !u.is_empty()),
            at_ms,
        },
        "Gift" => {
            let num = count(&d["num"]);
            ChatEvent::Gift {
                id: id(),
                author: author(platform, d, Guard::None),
                gift_name: string(&d["item"]),
                num,
                // `price` is one gift's, in yuan; free gifts have none.
                total_coin: coins(&d["price"]) * u64::from(num),
                at_ms,
            }
        }
        "Subscribe" | "Guard" => {
            let guard = guard_of(&string(&d["item"]));
            ChatEvent::Member {
                id: id(),
                author: author(platform, d, guard),
                num: count(&d["num"]),
                unit: string(&d["vip_unit"]),
                total_coin: coins(&d["price"]),
                at_ms,
            }
        }
        "SuperChat" => ChatEvent::SuperChat {
            id: id(),
            author: author(platform, d, Guard::None),
            price: yuan(&d["price"]).round() as u64,
            content: string(&d["content"]),
            at_ms,
        },
        _ => return Some(Incoming::Other),
    }))
}

fn author(platform: &str, d: &Value, guard: Guard) -> Author {
    let uid = Some(string(&d["id"]))
        .filter(|u| !u.is_empty() && u != "0")
        .map(|u| {
            if platform == BILIBILI {
                u
            } else {
                format!("{platform}:{u}")
            }
        })
        .unwrap_or_default();
    let guard = match guard {
        Guard::None => Guard::from_privilege(int(&d["guard_level"])),
        g => g,
    };
    Author {
        uid,
        name: string(&d["name"]),
        avatar_url: url(&d["avatar"]),
        guard,
        owner: false,
        admin: false,
        medal_level: int(&d["fans_medal_level"]).max(0) as u32,
        medal_name: string(&d["fans_medal_name"]),
    }
}

/// Bilibili's guard by name, as `item` gives it.
fn guard_of(item: &str) -> Guard {
    if item.contains("总督") {
        Guard::Governor
    } else if item.contains("提督") {
        Guard::Admiral
    } else if item.contains("舰长") {
        Guard::Captain
    } else {
        Guard::None
    }
}

fn yuan(v: &Value) -> f64 {
    let p = match v {
        Value::Number(n) => n.as_f64().unwrap_or(0.0),
        Value::String(s) => s.trim().parse().unwrap_or(0.0),
        _ => 0.0,
    };
    if p.is_finite() { p.max(0.0) } else { 0.0 }
}

/// Yuan as gold coins (1000 = ¥1).
fn coins(v: &Value) -> u64 {
    (yuan(v) * 1000.0).round() as u64
}
