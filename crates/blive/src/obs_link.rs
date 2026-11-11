//! The OBS link a danmaku page gives, read as what NanaType connects to:
//! a blivechat room or a LAPLACE Event Bridge.

use crate::laplace::{self, LinkError};
use crate::room_link::{RoomLink, RoomLinkError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObsLink {
    Blivechat(RoomLink),
    Laplace(laplace::Link),
}

/// Why a pasted link names nothing NanaType connects to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ObsLinkError {
    /// Neither page's link.
    Invalid,
    /// blivechat's home page, not a room's link.
    BlivechatHome,
    /// A room on blivechat's public server.
    BlivechatPublic,
    /// LAPLACE Chat joining anonymously, through Bilibili's web interface.
    LaplaceAnonymous,
    /// LAPLACE Chat joining by 身份码 through LAPLACE's own servers.
    LaplaceOpenPlatform,
}

impl ObsLink {
    /// A blivechat room link (or its loader) is blivechat's; anything else
    /// is read as LAPLACE's: a LAPLACE Chat OBS link or a bridge's address.
    pub fn parse(text: &str) -> Result<Self, ObsLinkError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(ObsLinkError::Invalid);
        }
        match RoomLink::parse(text) {
            Ok(room) => return Ok(Self::Blivechat(room)),
            Err(RoomLinkError::PublicServer) => return Err(ObsLinkError::BlivechatPublic),
            Err(RoomLinkError::Invalid) => {}
        }
        if let Ok(url) = url::Url::parse(text) {
            let path = url.path();
            // A blivechat link that did not read is not a bridge either.
            if path.starts_with("/room/") || path.ends_with("/loader.html") {
                return Err(ObsLinkError::Invalid);
            }
            if url.scheme() == "http"
                && url.port() == Some(crate::room_link::BLIVECHAT_PORT)
                && path.trim_end_matches('/').is_empty()
            {
                return Err(ObsLinkError::BlivechatHome);
            }
        }
        laplace::Link::parse(text)
            .map(Self::Laplace)
            .map_err(|e| match e {
                LinkError::Invalid => ObsLinkError::Invalid,
                LinkError::AnonymousPage => ObsLinkError::LaplaceAnonymous,
                LinkError::OpenPlatformPage => ObsLinkError::LaplaceOpenPlatform,
            })
    }
}
