//! LAPLACE Event Bridge: the WebSocket bridge LAPLACE Chat (or its event
//! fetcher) broadcasts Bilibili live events through.
//!
//! A client connects to the bridge's link (`ws://localhost:9696`, or `wss://`
//! for one behind TLS), with its token as `?token=` when it has one, and
//! receives one JSON event per frame, named by
//! its `type` (`message`, `gift`, `superchat`, `toast`, ...). There is no
//! room to join: the bridge sends whatever rooms LAPLACE Chat watches. The Go
//! bridge keeps the line alive with WebSocket pings; the older Bun bridge
//! sends `{"type":"ping"}`, which wants a `pong` back.
//!
//! Only LAPLACE joining the room by auth code (the open platform) is
//! supported; a LAPLACE that reads Bilibili's web interface by room number
//! is the unofficial path NanaType does not take, bridged or not.
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

/// A bridge's link as the user pasted it, split into the address, which may
/// be shown and kept in settings, and the token, which may not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    /// `ws://` or `wss://` with host, port and path; no token.
    pub address: String,
    /// Empty when the bridge has none.
    pub token: String,
}

/// Why a pasted link names no bridge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkError {
    /// Not a `ws`, `wss`, `http` or `https` address.
    Invalid,
    /// A LAPLACE Chat page that joins the room itself (by room number, which
    /// is not supported, or by auth code) and so names no bridge to read from.
    DirectPage,
}

impl Link {
    /// Read a pasted link: `ws://` or `wss://` (`http(s)://`, as a browser
    /// shows the same address, is taken for them), or a bare `host[:port]`,
    /// which is a local bridge on [`DEFAULT_PORT`] unless it names a port.
    ///
    /// A LAPLACE Chat page link (its OBS link, `…/obs/<room>?…`) reads as
    /// the event fetcher it was set to use (`danmakuFetcherApi`, with
    /// `eventFetcherAuth` for the token). Only the token is kept from a
    /// query: a page link carries its own login token among the rest.
    pub fn parse(text: &str) -> Result<Self, LinkError> {
        let text = text.trim();
        let bare = !text.contains("://");
        let parsed = if bare {
            url::Url::parse(&format!("ws://{text}"))
        } else {
            url::Url::parse(text)
        };
        let mut url = parsed.map_err(|_| LinkError::Invalid)?;
        let pairs: Vec<(String, String)> = url.query_pairs().into_owned().collect();
        let query = |name: &str| {
            pairs
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.trim().to_owned())
        };
        if query("connectionMode").is_some() || url.path().starts_with("/obs/") {
            return match query("danmakuFetcherApi").filter(|api| !api.is_empty()) {
                Some(api) => {
                    let mut link = Self::parse(&api)?;
                    link.token = query("eventFetcherAuth").unwrap_or_default();
                    Ok(link)
                }
                None => Err(LinkError::DirectPage),
            };
        }
        let scheme = match url.scheme() {
            "ws" | "http" => "ws",
            "wss" | "https" => "wss",
            _ => return Err(LinkError::Invalid),
        };
        url.set_scheme(scheme).map_err(|_| LinkError::Invalid)?;
        if url.host_str().is_none_or(str::is_empty) {
            return Err(LinkError::Invalid);
        }
        if bare && url.port().is_none() {
            url.set_port(Some(DEFAULT_PORT))
                .map_err(|_| LinkError::Invalid)?;
        }
        let token = query("token").unwrap_or_default();
        let _ = url.set_username("");
        let _ = url.set_password(None);
        url.set_fragment(None);
        url.set_query(None);
        Ok(Self {
            address: url.into(),
            token,
        })
    }

    /// What the client connects to: the address with the token put back.
    /// `None` if the address is not one [`Link::parse`] made.
    pub fn url(&self) -> Option<String> {
        let mut url = url::Url::parse(&self.address).ok()?;
        if !self.token.is_empty() {
            url.query_pairs_mut().append_pair("token", &self.token);
        }
        Some(url.into())
    }
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
