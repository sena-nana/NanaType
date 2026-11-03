//! LAPLACE Event Bridge: the WebSocket bridge LAPLACE Chat (or its event
//! fetcher) broadcasts Bilibili live events through.
//!
//! A client connects to `ws://host:port/`, with the bridge's token as
//! `?token=` when it has one, and receives one JSON event per frame, named by
//! its `type` (`message`, `gift`, `superchat`, `toast`, ...). There is no
//! room to join: the bridge sends whatever rooms LAPLACE Chat watches. The Go
//! bridge keeps the line alive with WebSocket pings; the older Bun bridge
//! sends `{"type":"ping"}`, which wants a `pong` back.
//!
//! Events are read into the same [`ChatEvent`]s as blivechat's, every field
//! leniently, so a newer bridge never drops a message.
//!
//! See <https://github.com/laplace-live/event-bridge> and
//! <https://chat.laplace.live/event-types/>.

use serde_json::{Value, json};

use crate::chat::{Author, ChatEvent, Guard, int, millis, string, url};
use crate::protocol::Incoming;

/// The port LAPLACE Event Bridge listens on by default.
pub const DEFAULT_PORT: u16 = 9696;

/// `userType` LAPLACE gives the room's owner; 1 is a moderator.
const OWNER: i64 = 100;
const MODERATOR: i64 = 1;

/// The path to connect to, carrying `token` when there is one.
pub fn path(token: &str) -> String {
    if token.is_empty() {
        return "/".into();
    }
    let mut path = String::from("/?token=");
    for b in token.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            path.push(b as char);
        } else {
            path.push_str(&format!("%{b:02X}"));
        }
    }
    path
}

/// The answer to a bridge's JSON ping.
pub fn pong(ping_at: &Value, now_ms: u64) -> String {
    json!({ "type": "pong", "timestamp": now_ms, "respondingTo": ping_at }).to_string()
}

/// Read one text frame. `None` for frames that are not JSON objects.
/// A JSON ping reads as [`Incoming::Heartbeat`].
pub fn parse(text: &str) -> Option<Incoming> {
    let e: Value = serde_json::from_str(text).ok()?;
    let kind = e.as_object()?.get("type")?.as_str()?;
    Some(match kind {
        "ping" => Incoming::Heartbeat,
        "message" => Incoming::Event(ChatEvent::Text {
            id: string(&e["id"]),
            author: author(&e, int(&e["guardType"])),
            content: string(&e["message"]),
            emoticon: Some(url(&e["emote"]["url"])).filter(|u| !u.is_empty()),
            at_ms: at(&e),
        }),
        "gift" => Incoming::Event(ChatEvent::Gift {
            id: string(&e["id"]),
            author: author(&e, int(&e["guardType"])),
            gift_name: string(&e["giftName"]),
            num: int(&e["giftAmount"]).clamp(1, u32::MAX as i64) as u32,
            // Silver gifts are bought with free currency.
            total_coin: if e["coinType"].as_str() == Some("silver") {
                0
            } else {
                coins(&e)
            },
            at_ms: at(&e),
        }),
        "toast" => Incoming::Event(ChatEvent::Member {
            id: string(&e["id"]),
            author: author(&e, int(&e["toastType"])),
            num: int(&e["toastAmount"]).clamp(1, u32::MAX as i64) as u32,
            unit: string(&e["toastAmountUnit"]),
            total_coin: coins(&e),
            at_ms: at(&e),
        }),
        "superchat" => Incoming::Event(ChatEvent::SuperChat {
            id: superchat_id(&e),
            author: author(&e, int(&e["guardType"])),
            price: yuan(&e).unwrap_or_else(|| int(&e["price"]).max(0) as f64) as u64,
            content: string(&e["message"]),
            at_ms: at(&e),
        }),
        "superchat-delete" => Incoming::Event(ChatEvent::DeleteSuperChats {
            ids: e["scIds"]
                .as_array()
                .map(|ids| ids.iter().map(string).collect())
                .unwrap_or_default(),
        }),
        _ => Incoming::Other,
    })
}

/// A Super Chat by `scId`, the id `superchat-delete` names; `id` when a
/// bridge leaves it out.
fn superchat_id(e: &Value) -> String {
    match int(&e["scId"]) {
        0 => string(&e["id"]),
        id => id.to_string(),
    }
}

fn author(e: &Value, guard: i64) -> Author {
    let user_type = int(&e["userType"]);
    Author {
        // The open platform hides uids behind open ids and sends 0.
        uid: Some(string(&e["uid"]))
            .filter(|u| u != "0")
            .unwrap_or_default(),
        name: string(&e["username"]),
        avatar_url: url(&e["avatar"]),
        guard: Guard::from_privilege(guard),
        owner: user_type == OWNER,
        admin: user_type == MODERATOR,
        medal_level: int(&e["medal"]["level"]).max(0) as u32,
        medal_name: string(&e["medal"]["name"]),
    }
}

/// The price in yuan, when the bridge gives it.
fn yuan(e: &Value) -> Option<f64> {
    e["priceNormalized"]
        .as_f64()
        .filter(|p| p.is_finite() && *p >= 0.0)
}

/// The price in gold coins (1000 = ¥1): from yuan when the bridge gives
/// them, otherwise `price`, which is in coins already.
fn coins(e: &Value) -> u64 {
    match yuan(e) {
        Some(yuan) => (yuan * 1000.0).round() as u64,
        None => int(&e["price"]).max(0) as u64,
    }
}

fn at(e: &Value) -> u64 {
    match millis(&e["timestampNormalized"]) {
        0 => millis(&e["timestamp"]),
        t => t,
    }
}
