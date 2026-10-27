//! Live chat through a running [blivechat] server.
//!
//! NanaType joins a room on blivechat's own client endpoint
//! (`ws://host:port/api/chat`), the one its OBS page uses: it needs no token,
//! and joining is what makes blivechat open its configured live chat connection. The
//! server speaks the same JSON from v1.7 to v1.10; [`protocol`] reads every
//! version, filling fields an older server does not send with defaults.
//!
//! Messages arrive on a thread of their own through the callback given to
//! [`BliveHandle::new`]; nothing here touches UI state.
//!
//! [`relay`] is a small blivechat-compatible server for sources that can only
//! push: the 彗星号 workshop plugin connects to it and sends blivechat frames,
//! and a [`BliveHandle`] joins its `/api/chat` like any blivechat server.
//!
//! [blivechat]: https://github.com/xfgryujk/blivechat

#![forbid(unsafe_code)]

mod client;
pub mod protocol;
pub mod relay;

#[cfg(test)]
mod tests;

pub use client::{BliveConfig, BliveEvent, BliveHandle, BliveStatus, BliveUpdate};
pub use protocol::{Author, ChatEvent, FatalKind, Guard, RoomKey};
pub use relay::{RelayConfig, RelayHandle, RelayStatus};
