//! A blivechat room link, as blivechat's home page gives it for OBS, read
//! into the server and room NanaType joins.
//!
//! The page's link is `http://host:port/room/<key>?roomKeyType=…&…`, where
//! `roomKeyType` 1 (or none) is a room number and 2 a 身份码. A server set
//! up with a loader (the release's default, `file:///…/loader.html`) wraps
//! that link as the loader's `url` parameter instead.

use crate::protocol::RoomKey;

/// A room on a blivechat server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoomLink {
    pub host: String,
    pub port: u16,
    pub room: RoomKey,
}

/// Why a pasted link names no room NanaType can join.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoomLinkError {
    /// Not an `http://` link to a blivechat room.
    Invalid,
    /// A room on blivechat's public server (blive.chat), which NanaType does
    /// not connect to: its relay runs on the author's server.
    PublicServer,
}

/// The port blivechat serves on unless told otherwise.
pub const BLIVECHAT_PORT: u16 = 12450;

/// The public server's site and its API hosts.
const PUBLIC_HOST: &str = "blive.chat";

impl RoomLink {
    pub fn parse(text: &str) -> Result<Self, RoomLinkError> {
        Self::parse_at(text.trim(), true)
    }

    fn parse_at(text: &str, may_unwrap: bool) -> Result<Self, RoomLinkError> {
        let url = url::Url::parse(text).map_err(|_| RoomLinkError::Invalid)?;
        let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
        if host == PUBLIC_HOST || host.ends_with(".blive.chat") {
            return Err(RoomLinkError::PublicServer);
        }
        let key = url
            .path()
            .strip_prefix("/room/")
            .filter(|k| !k.is_empty() && !k.contains('/'));
        let Some(key) = key else {
            // A loader page carries the room's own link.
            let inner = url
                .query_pairs()
                .find(|(k, _)| k == "url")
                .map(|(_, v)| v.into_owned());
            return match inner {
                Some(inner) if may_unwrap => Self::parse_at(inner.trim(), false),
                _ => Err(RoomLinkError::Invalid),
            };
        };
        // Only plain blivechat servers are reached; one behind TLS is not.
        if url.scheme() != "http" || host.is_empty() {
            return Err(RoomLinkError::Invalid);
        }
        let key = percent_decode(key).ok_or(RoomLinkError::Invalid)?;
        let kind = url
            .query_pairs()
            .find(|(k, _)| k == "roomKeyType")
            .map(|(_, v)| v.trim().to_owned());
        // blivechat reads a missing or unknown type as a room number.
        let room =
            RoomKey::parse(kind.as_deref() == Some("2"), &key).ok_or(RoomLinkError::Invalid)?;
        Ok(Self {
            host: url
                .host_str()
                .unwrap_or_default()
                .trim_start_matches('[')
                .trim_end_matches(']')
                .to_owned(),
            port: url.port_or_known_default().ok_or(RoomLinkError::Invalid)?,
            room,
        })
    }
}

/// A path segment as text; `None` when it is not UTF-8.
fn percent_decode(segment: &str) -> Option<String> {
    let hex = |b: Option<&u8>| b.and_then(|&b| (b as char).to_digit(16));
    let bytes = segment.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let (Some(h), Some(l)) = (hex(bytes.get(i + 1)), hex(bytes.get(i + 2)))
        {
            out.push((h * 16 + l) as u8);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}
