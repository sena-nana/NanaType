//! blivechat-protocol chat: a blivechat server or the 彗星号 client's own
//! feed.

use crate::ui::EditShared;

use nanatype_blive::{BliveConfig, BliveEvent, BliveStatus, BliveUpdate, ChatEvent, Join, RoomKey};
use nanatype_settings::{BliveRoomKind, BlivechatSource};

use crate::{app::App, chat::Platform, msg::BliveMsg};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    pub status: BliveStatus,
    /// A 身份码 is kept in the secret store.
    pub auth_saved: bool,
}

impl App {
    pub(super) fn blivechat_msg(&mut self, msg: BliveMsg) {
        match msg {
            BliveMsg::Event(event) => self.blivechat_event(event),
            BliveMsg::AuthCode(code) => {
                if let Err(e) = self.blivechat_auth.set(&code) {
                    nanatype_diag::report!(crate::diag::BLIVECHAT_SECRET_FAILED, op = 1u64; "{e}");
                    let text = self.ui.now_t("int-blive-auth-not-stored");
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
                self.ui.blivechat.edit(|s| s.status = status);
                let text = self.blivechat_status_line();
                if fatal {
                    self.notice(text, true);
                }
            }
            BliveUpdate::Chat(chat) => self.blive_chat(Platform::Blivechat, event.epoch, chat),
        }
    }

    /// Connect, reconnect or stop after a settings change.
    pub(super) fn blivechat_configure(&mut self) {
        let b = &self.settings.integrations.blivechat;
        let client = match (b.enabled, b.source) {
            (false, _) => None,
            (true, BlivechatSource::Comet) => Some(BliveConfig {
                host: "127.0.0.1".into(),
                port: b.comet_port,
                join: Join::Comet,
            }),
            (true, BlivechatSource::Server) => {
                let room = match crate::distribution::blive_room_kind(b) {
                    BliveRoomKind::AuthCode => {
                        // Only read the secret store once the code is wanted.
                        let code = self.blivechat_auth.get();
                        let saved = code.is_some();
                        self.ui.blivechat.edit(|s| s.auth_saved = saved);
                        code.and_then(|code| RoomKey::parse(true, &code))
                    }
                    BliveRoomKind::RoomId => RoomKey::parse(false, &b.room_id),
                };
                let b = &self.settings.integrations.blivechat;
                room.map(|room| BliveConfig {
                    host: b.host.clone(),
                    port: b.port,
                    join: Join::Blivechat(room),
                })
            }
        };
        let epoch = self.blivechat.epoch();
        let joining = client.is_some();
        self.blivechat.set(client);
        if self.blivechat.epoch() != epoch {
            self.ui.blivechat.edit(|s| {
                s.status = if joining {
                    BliveStatus::Connecting
                } else {
                    BliveStatus::Off
                }
            });
        }
        self.blivechat_status_line();
    }

    /// Chat from a blivechat-shaped feed: a withdrawn super chat leaves the
    /// list, anything else goes to have its pictures saved.
    pub(super) fn blive_chat(&mut self, platform: Platform, epoch: u64, chat: ChatEvent) {
        match chat {
            ChatEvent::DeleteSuperChats { ids } => {
                self.chat.retain(|m| {
                    !(m.platform == platform
                        && matches!(m.kind, crate::chat::ChatKind::SuperChat { .. })
                        && ids.contains(&m.id))
                });
                self.publish_chat();
            }
            chat => self.chat_assets.blive(platform, epoch, chat),
        }
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
