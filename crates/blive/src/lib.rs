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
//! [`laplace`] reads a LAPLACE Event Bridge instead: the same connection
//! thread, with [`Join::Laplace`] in place of a room.
//!
//! [`room_link`] reads the room link blivechat gives for OBS into the server
//! and room to join.
//!
//! [`comet`] reads the 彗星号 client's own feed (`ws://127.0.0.1:19400/`),
//! its binary frames decrypted, with [`Join::Comet`].
//!
//! [blivechat]: https://github.com/xfgryujk/blivechat

#![forbid(unsafe_code)]

pub mod chat;
mod client;
pub mod comet;
pub mod laplace;
pub mod protocol;
pub mod room_link;

#[cfg(test)]
mod tests;

pub use client::{BliveConfig, BliveEvent, BliveHandle, BliveStatus, BliveUpdate, Join};
pub use protocol::{Author, ChatEvent, FatalKind, Guard, RoomKey};
