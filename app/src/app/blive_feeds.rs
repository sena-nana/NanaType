//! Unified Bilibili-shaped chat feeds (`blivechat`, LAPLACE Event Bridge,
//! and 彗星号) managed behind [`BliveFeedManager`].

use nanatype_blive::{
    BliveConfig, BliveEvent, BliveHandle, BliveStatus, BliveUpdate, ChatEvent, Join, RoomKey, comet,
};
use nanatype_i18n::Locale;
use nanatype_settings::{BilibiliLink, BilibiliSource, BliveRoomKind, Integrations, Settings};

use super::App;
use super::secret::LazySecret;
use crate::chat::Platform;
use crate::msg::{BliveFeedMsg, Msg};
pub use crate::ui::BliveFeedKind;
use crate::ui::{BliveFeedState, EditShared, Ui, danmaku::Link};

/// Owns the three [`BliveHandle`] workers and their lazy credentials.
pub struct BliveFeedManager {
    handles: [BliveHandle; 3],
    pub(super) blivechat_auth: LazySecret,
    pub(super) laplace_token: LazySecret,
}

impl BliveFeedManager {
    pub fn new(
        post: impl Fn(Msg) + Clone + Send + Sync + 'static,
        blivechat_auth: LazySecret,
        laplace_token: LazySecret,
    ) -> Self {
        let spawn = |kind: BliveFeedKind| {
            let post = post.clone();
            BliveHandle::new(move |event| post(Msg::BliveFeed(BliveFeedMsg::Event(kind, event))))
        };
        Self {
            handles: [
                spawn(BliveFeedKind::Blivechat),
                spawn(BliveFeedKind::Laplace),
                spawn(BliveFeedKind::Comet),
            ],
            blivechat_auth,
            laplace_token,
        }
    }

    pub fn epoch(&self, kind: BliveFeedKind) -> u64 {
        self.handle(kind).epoch()
    }

    pub fn handle(&self, kind: BliveFeedKind) -> &BliveHandle {
        &self.handles[kind.index()]
    }

    pub fn handle_mut(&mut self, kind: BliveFeedKind) -> &mut BliveHandle {
        &mut self.handles[kind.index()]
    }

    pub fn configure_all(&mut self, ui: Ui, settings: &Settings) {
        for kind in BliveFeedKind::ALL {
            self.configure(kind, ui, settings);
        }
    }

    pub fn configure(&mut self, kind: BliveFeedKind, ui: Ui, settings: &Settings) {
        let mut secret_saved = None;
        let client = match kind {
            BliveFeedKind::Blivechat => {
                if settings.integrations.blivechat_on() {
                    let b = &settings.integrations.blivechat;
                    let room = match b.room_kind {
                        BliveRoomKind::AuthCode => {
                            // Only read the secret store once the code is wanted.
                            let code = self.blivechat_auth.get();
                            secret_saved = Some(code.is_some());
                            code.and_then(|c| RoomKey::parse(true, &c))
                        }
                        BliveRoomKind::RoomId => RoomKey::parse(false, &b.room_id),
                    };
                    room.map(|room| BliveConfig {
                        host: b.host.clone(),
                        port: b.port,
                        join: Join::Blivechat(room),
                    })
                } else {
                    None
                }
            }
            BliveFeedKind::Laplace => {
                if settings.integrations.laplace_on() {
                    // Only read the secret store once the token is wanted.
                    let token = self.laplace_token.get();
                    secret_saved = Some(token.is_some());
                    let link = nanatype_blive::laplace::Link {
                        address: settings.integrations.laplace.url.clone(),
                        token: token.unwrap_or_default(),
                    };
                    link.url().map(|url| BliveConfig {
                        host: String::new(),
                        port: 0,
                        join: Join::Laplace { url },
                    })
                } else {
                    None
                }
            }
            BliveFeedKind::Comet => {
                let wanted = settings.integrations.comet_rooms();
                let rooms = comet::Rooms {
                    bilibili: wanted.bilibili,
                    others: wanted.others,
                };
                rooms.any().then(|| BliveConfig {
                    host: "127.0.0.1".into(),
                    port: settings.integrations.comet.port,
                    join: Join::Comet(rooms),
                })
            }
        };
        let handle = self.handle_mut(kind);
        let epoch = handle.epoch();
        let joining = client.is_some();
        handle.set(client);
        let epoch_changed = handle.epoch() != epoch;
        ui.chat_source.edit(|cs| {
            let slot = cs.get_mut(kind);
            if let Some(saved) = secret_saved {
                slot.secret_saved = saved;
            }
            if epoch_changed {
                slot.status = if joining {
                    BliveStatus::Connecting
                } else {
                    BliveStatus::Off
                };
            }
        });
        self.update_status_line(kind, ui, &settings.integrations);
    }

    pub fn refresh_status_lines(&self, ui: Ui, integrations: &Integrations) {
        for kind in BliveFeedKind::ALL {
            self.update_status_line(kind, ui, integrations);
        }
    }

    pub fn update_status_line(
        &self,
        kind: BliveFeedKind,
        ui: Ui,
        integrations: &Integrations,
    ) -> String {
        let locale = ui.locale.get_untracked();
        let state = ui.chat_source.with_untracked(|cs| cs.get(kind).clone());
        let (text, link) = status_line_for(kind, locale, integrations, &state);
        ui.chat_source.edit(|cs| {
            let slot = cs.get_mut(kind);
            slot.text = text.clone();
            slot.link = link;
        });
        text
    }
}

fn status_line_for(
    kind: BliveFeedKind,
    locale: Locale,
    integrations: &Integrations,
    state: &BliveFeedState,
) -> (String, Link) {
    match kind {
        BliveFeedKind::Blivechat => crate::ui::link_text::blivechat(locale, integrations, state),
        BliveFeedKind::Laplace => crate::ui::link_text::laplace(locale, integrations, state),
        BliveFeedKind::Comet => crate::ui::link_text::comet(locale, integrations, state),
    }
}

impl App {
    pub(super) fn blive_configure_all(&mut self) {
        self.integrations
            .blive
            .configure_all(self.ui, &self.settings);
    }

    pub(super) fn blive_feed_msg(&mut self, msg: BliveFeedMsg) {
        match msg {
            BliveFeedMsg::Event(kind, event) => self.blive_feed_event(kind, event),
            BliveFeedMsg::Link(text) => self.bilibili_link(&text),
        }
    }

    /// A chat page's OBS link: blivechat's room or LAPLACE's bridge, which
    /// then carries Bilibili's chat.
    fn bilibili_link(&mut self, text: &str) {
        use nanatype_blive::obs_link::{ObsLink, ObsLinkError};
        let link = match ObsLink::parse(text) {
            Ok(link) => link,
            Err(e) => {
                let text = self.ui.now_t(match e {
                    ObsLinkError::Invalid => "int-bili-link-invalid",
                    ObsLinkError::BlivechatHome => "int-bili-link-blivechat-home",
                    ObsLinkError::BlivechatPublic => "int-bili-link-blivechat-public",
                    ObsLinkError::LaplaceAnonymous => "int-bili-link-laplace-anonymous",
                    ObsLinkError::LaplaceOpenPlatform => "int-bili-link-laplace-open-platform",
                });
                self.notice(text, true);
                return;
            }
        };
        let kind = match link {
            ObsLink::Blivechat(room) => {
                let room_kind = match &room.room {
                    RoomKey::AuthCode(code) => {
                        if let Err(e) = self.integrations.blive.blivechat_auth.set(code) {
                            nanatype_diag::report!(crate::diag::BLIVECHAT_SECRET_FAILED, op = 1u64; "{e}");
                            let text = self.ui.now_t("int-blive-auth-not-stored");
                            self.notice(text, true);
                        }
                        BliveRoomKind::AuthCode
                    }
                    RoomKey::RoomId(_) => BliveRoomKind::RoomId,
                };
                self.edit(|s| {
                    let b = &mut s.integrations.blivechat;
                    b.host = room.host;
                    b.port = room.port;
                    b.room_kind = room_kind;
                    if let RoomKey::RoomId(id) = room.room {
                        b.room_id = id.to_string();
                    }
                });
                BilibiliLink::Blivechat
            }
            ObsLink::Laplace(bridge) => {
                // A link without a token is a bridge without one.
                if let Err(e) = self.integrations.blive.laplace_token.set(&bridge.token) {
                    nanatype_diag::report!(crate::diag::LAPLACE_SECRET_FAILED, op = 1u64; "{e}");
                    let text = self.ui.now_t("int-laplace-token-not-stored");
                    self.notice(text, true);
                }
                self.edit(|s| s.integrations.laplace.url = bridge.address);
                BilibiliLink::Laplace
            }
        };
        self.edit(|s| {
            s.integrations
                .set_chat_source(nanatype_settings::ChatSource::Bilibili, true);
            let b = &mut s.integrations.bilibili;
            b.source = BilibiliSource::Link;
            b.link = kind;
        });
        let feed = match kind {
            BilibiliLink::Blivechat => BliveFeedKind::Blivechat,
            BilibiliLink::Laplace => BliveFeedKind::Laplace,
        };
        self.integrations
            .blive
            .configure(feed, self.ui, &self.settings);
        if self
            .ui
            .chat_source
            .with_untracked(|cs| matches!(cs.get(feed).status, BliveStatus::Fatal(..)))
        {
            self.integrations.blive.handle_mut(feed).reconnect();
        }
    }

    fn blive_feed_event(&mut self, kind: BliveFeedKind, event: BliveEvent) {
        if event.epoch != self.integrations.blive.handle(kind).epoch() {
            return;
        }
        match event.update {
            BliveUpdate::Status(status) => {
                match &status {
                    BliveStatus::Error(fault) => match kind {
                        BliveFeedKind::Blivechat => {
                            nanatype_diag::report!(crate::diag::BLIVECHAT_ERROR; "{fault}");
                        }
                        BliveFeedKind::Laplace => {
                            nanatype_diag::report!(crate::diag::LAPLACE_ERROR; "{fault}");
                        }
                        BliveFeedKind::Comet => {
                            nanatype_diag::report!(crate::diag::COMET_ERROR; "{fault}");
                        }
                    },
                    BliveStatus::Fatal(fatal_kind, message) => match kind {
                        BliveFeedKind::Blivechat => {
                            nanatype_diag::report!(
                                crate::diag::BLIVECHAT_FATAL;
                                "{fatal_kind:?}: {message}"
                            );
                        }
                        BliveFeedKind::Laplace => {
                            nanatype_diag::report!(
                                crate::diag::LAPLACE_FATAL;
                                "{fatal_kind:?}: {message}"
                            );
                        }
                        BliveFeedKind::Comet => {
                            nanatype_diag::report!(
                                crate::diag::COMET_ERROR;
                                "{fatal_kind:?}: {message}"
                            );
                        }
                    },
                    _ => {}
                }
                let fatal =
                    matches!(status, BliveStatus::Fatal(..)) && kind != BliveFeedKind::Comet;
                self.ui
                    .chat_source
                    .edit(|cs| cs.get_mut(kind).status = status);
                let text = self.integrations.blive.update_status_line(
                    kind,
                    self.ui,
                    &self.settings.integrations,
                );
                if fatal {
                    self.notice(text, true);
                }
            }
            BliveUpdate::Chat(chat) => self.blive_chat(kind.platform(), event.epoch, chat),
        }
    }

    /// Chat from a blivechat-shaped feed: a withdrawn super chat leaves the
    /// list, anything else goes to have its pictures saved.
    pub(super) fn blive_chat(&mut self, platform: Platform, epoch: u64, chat: ChatEvent) {
        match chat {
            ChatEvent::DeleteSuperChats { ids } => {
                self.session.chat.retain(|m| {
                    !(m.platform == platform
                        && matches!(m.kind, crate::chat::ChatKind::SuperChat { .. })
                        && ids.contains(&m.id))
                });
                self.publish_chat();
            }
            chat => self.integrations.chat_assets.blive(platform, epoch, chat),
        }
    }
}
