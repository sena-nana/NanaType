//! Third-party chat over the blivechat protocol: a blivechat server, or the
//! 彗星号 workshop plugin through NanaType's own relay (the plugin pushes
//! blivechat frames to the relay, and the same client joins its `/api/chat`).

use std::path::Path;
use std::sync::Arc;

use nanatype_blive::{
    BliveConfig, BliveEvent, BliveStatus, BliveUpdate, ChatEvent, RelayConfig, RelayStatus, RoomKey,
};
use nanatype_secrets::SecretStore;
use nanatype_settings::{BliveRoomKind, BlivechatSettings, BlivechatSource};

use crate::{app::App, chat::Platform, msg::Msg};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    pub status: BliveStatus,
    pub relay: RelayStatus,
    /// A 身份码 is kept in the secret store.
    pub auth_saved: bool,
}

/// The 身份码, read from the secret store the first time it is needed.
pub struct AuthCode {
    store: SecretStore,
    value: Option<Option<String>>,
}

impl AuthCode {
    pub fn new(config_dir: &Path) -> Self {
        Self {
            store: SecretStore::new(config_dir, "blivechat-auth"),
            value: None,
        }
    }

    fn get(&mut self) -> Option<String> {
        self.value
            .get_or_insert_with(|| match self.store.load() {
                Ok(bytes) => bytes
                    .and_then(|b| String::from_utf8(b).ok())
                    .filter(|c| !c.trim().is_empty()),
                Err(e) => {
                    nanatype_diag::report!(crate::diag::BLIVECHAT_SECRET_FAILED, op = 0u64; "{e}");
                    None
                }
            })
            .clone()
    }

    /// Keep `code` (or forget it when empty). It stays for this run even when
    /// the system will not store it.
    fn set(&mut self, code: &str) -> std::io::Result<()> {
        let code = code.trim();
        self.value = Some((!code.is_empty()).then(|| code.to_owned()));
        if code.is_empty() {
            self.store.clear()
        } else {
            self.store.save(code.as_bytes())
        }
    }
}

/// A new 128-bit 彗星号 token.
fn new_token() -> String {
    let mut bytes = [0u8; 16];
    if getrandom::fill(&mut bytes).is_err() {
        // No system randomness: the clock still keeps other pages guessing.
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        bytes = nanos.to_le_bytes();
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The address the 彗星号 page connects to.
pub fn comet_address(settings: &BlivechatSettings) -> String {
    nanatype_blive::relay::publish_url(settings.comet_port, &settings.comet_token)
}

impl App {
    pub(super) fn blivechat_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Blivechat(event) => self.blivechat_event(event),
            Msg::CometRelay(status) => {
                if let RelayStatus::Error(fault) = &status {
                    nanatype_diag::report!(crate::diag::COMET_RELAY_ERROR; "{fault}");
                }
                self.blive_state(|s| s.relay = status);
                self.blivechat_status_line();
            }
            Msg::BlivechatAuthCode(code) => {
                if let Err(e) = self.blivechat_auth.set(&code) {
                    nanatype_diag::report!(crate::diag::BLIVECHAT_SECRET_FAILED, op = 1u64; "{e}");
                    let text = crate::ui::I18N
                        .t_in(self.ui.locale.get_untracked(), "int-blive-auth-not-stored");
                    self.notice(text, true);
                }
                let saved = !code.trim().is_empty();
                self.edit(|s| s.integrations.blivechat.enabled |= saved);
                // A changed code is a new room key; the same code again is a
                // retry after blivechat refused it, which is not retried alone.
                self.blivechat_configure();
                if matches!(
                    self.ui.blivechat.get_untracked().status,
                    BliveStatus::Fatal(..)
                ) {
                    self.blivechat.reconnect();
                }
            }
            Msg::CometResetToken => {
                self.edit(|s| s.integrations.blivechat.comet_token = new_token())
            }
            _ => unreachable!("not a blivechat message"),
        }
    }

    fn blivechat_event(&mut self, event: BliveEvent) {
        if event.epoch != self.blivechat.epoch() {
            return;
        }
        match event.update {
            BliveUpdate::Status(status) => {
                match &status {
                    BliveStatus::Error(fault) => {
                        nanatype_diag::report!(crate::diag::BLIVECHAT_ERROR; "{fault}");
                    }
                    BliveStatus::Fatal(kind, message) => {
                        nanatype_diag::report!(crate::diag::BLIVECHAT_FATAL; "{kind:?}: {message}");
                    }
                    _ => {}
                }
                let fatal = matches!(status, BliveStatus::Fatal(..));
                self.blive_state(|s| s.status = status);
                let text = self.blivechat_status_line();
                if fatal {
                    self.notice(text, true);
                }
            }
            BliveUpdate::Chat(ChatEvent::DeleteSuperChats { ids }) => {
                self.chat.retain(|m| {
                    !(m.platform == Platform::Blivechat
                        && matches!(m.kind, crate::chat::ChatKind::SuperChat { .. })
                        && ids.contains(&m.id))
                });
                self.publish_chat();
            }
            BliveUpdate::Chat(chat) => self._chat_assets.blivechat(event.epoch, chat),
        }
    }

    fn blive_state(&self, f: impl FnOnce(&mut State)) {
        self.ui.blivechat.update(|s| f(Arc::make_mut(s)));
    }

    /// Connect, reconnect or stop after a settings change.
    pub(super) fn blivechat_configure(&mut self) {
        let b = &self.settings.integrations.blivechat;
        let (relay, client) = match (b.enabled, b.source) {
            (false, _) => (None, None),
            (true, BlivechatSource::Comet) if b.comet_token.is_empty() => {
                // First use, or hand-edited settings: make one, then start.
                self.ctx.dispatch(Msg::CometResetToken);
                (None, None)
            }
            // The relay's port is open once `set` returns, so the client can
            // join it straight after. It serves every room the same feed.
            (true, BlivechatSource::Comet) => (
                Some(RelayConfig {
                    port: b.comet_port,
                    token: b.comet_token.clone(),
                }),
                Some(BliveConfig {
                    host: "127.0.0.1".into(),
                    port: b.comet_port,
                    room: RoomKey::RoomId(1),
                }),
            ),
            (true, BlivechatSource::Server) => {
                let room = match crate::distribution::blive_room_kind(b) {
                    BliveRoomKind::AuthCode => {
                        // Only read the secret store once the code is wanted.
                        let code = self.blivechat_auth.get();
                        let saved = code.is_some();
                        self.blive_state(|s| s.auth_saved = saved);
                        code.and_then(|code| RoomKey::parse(true, &code))
                    }
                    BliveRoomKind::RoomId => RoomKey::parse(false, &b.room_id),
                };
                let b = &self.settings.integrations.blivechat;
                (
                    None,
                    room.map(|room| BliveConfig {
                        host: b.host.clone(),
                        port: b.port,
                        room,
                    }),
                )
            }
        };
        self.comet_relay.set(relay);
        let epoch = self.blivechat.epoch();
        let joining = client.is_some();
        self.blivechat.set(client);
        if self.blivechat.epoch() != epoch {
            self.blive_state(|s| {
                s.status = if joining {
                    BliveStatus::Connecting
                } else {
                    BliveStatus::Off
                }
            });
        }
        self.blivechat_status_line();
    }

    /// Put the status line and the chat panel's dot in step with the state.
    fn blivechat_status_line(&mut self) -> String {
        let locale = self.ui.locale.get_untracked();
        let state = self.ui.blivechat.get_untracked();
        let (text, link) =
            crate::ui::link_text::blivechat(locale, &self.settings.integrations.blivechat, &state);
        self.edit_status(|s| {
            s.blivechat = text.clone();
            s.blivechat_link = link;
        });
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_128_bit_hex_and_differ() {
        let a = new_token();
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, new_token());
    }
}
