use crate::{app::App, chat::Platform, msg::Msg};
use nanatype_core::Event as CoreEvent;
use nanatype_model::{Document, outgoing};
use nanatype_twitch::{
    AccountState, ConnectionState, Delivery, DeliveryState, Event, SessionEvent,
};
use std::sync::Arc;

#[derive(Debug, Clone, Default, PartialEq)]
pub struct State {
    pub account: AccountState,
    pub connection: ConnectionState,
    pub avatar: Option<String>,
    pub sending: bool,
    pub confirm: bool,
    pub deliveries: Vec<Delivery>,
}
impl State {
    pub fn can_send(&self) -> bool {
        matches!(self.account, AccountState::Ready(_))
            && matches!(self.connection, ConnectionState::Connected(_))
    }
    pub fn pending(&self) -> usize {
        self.deliveries
            .iter()
            .filter(|d| matches!(d.state, DeliveryState::Queued | DeliveryState::Sending))
            .count()
    }
    fn confirm_sending(&mut self, draft: &Document, accept: bool) -> Result<bool, ()> {
        if !self.confirm {
            return Ok(self.sending);
        }
        self.confirm = false;
        self.sending = false;
        if accept && self.can_send() {
            if !draft.is_blank() && outgoing::plain_text(draft).is_err() {
                return Err(());
            }
            self.sending = true;
        }
        Ok(self.sending)
    }
}

/// Only Twitch's own activation page may be opened from the device flow.
fn activation_page(uri: &str) -> bool {
    url::Url::parse(uri).is_ok_and(|u| {
        u.scheme() == "https" && u.host_str() == Some("www.twitch.tv") && u.path() == "/activate"
    })
}

impl App {
    /// Twitch sign-in, chat and sending.
    pub(super) fn twitch_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Twitch(event) => self.twitch_event(event),
            Msg::TwitchLogin => self.twitch_login(),
            Msg::TwitchCancelLogin => {
                self.twitch.cancel_login();
                self.twitch_disable_send();
            }
            Msg::TwitchOpenActivation => {
                if let AccountState::Pending { uri, .. } = &self.ui.twitch.get_untracked().account
                    && activation_page(uri)
                {
                    crate::services::open_url(uri);
                }
            }
            Msg::TwitchRefresh => {
                self.twitch.refresh();
                self.twitch_disable_send();
            }
            Msg::TwitchLogout => {
                self.twitch.logout();
                self.twitch_disable_send();
                self.twitch_state(|s| s.avatar = None);
            }
            Msg::TwitchSendToggle => self.twitch_toggle(),
            Msg::TwitchSendConfirm(accept) => self.twitch_confirm(accept),
            Msg::TwitchRetry(line) => self.twitch.retry(line),
            Msg::TwitchSkipFailed(line) => self.twitch.skip_failed(line),
            _ => unreachable!("not a twitch message"),
        }
    }

    fn twitch_state(&self, edit: impl FnOnce(&mut State)) {
        self.ui.twitch.update(|state| edit(Arc::make_mut(state)));
    }
    fn twitch_disable_send(&mut self) {
        self.twitch.set_sending(false);
        self.twitch_state(|s| {
            s.sending = false;
            s.confirm = false;
            for d in &mut s.deliveries {
                d.blocked = false;
                if matches!(d.state, DeliveryState::Queued | DeliveryState::Sending) {
                    d.state = DeliveryState::Cancelled;
                }
            }
        });
    }
    fn twitch_login(&mut self) {
        self.twitch_disable_send();
        self.update(Msg::SettingsPlatform(Platform::Twitch));
        self.twitch.login();
    }
    fn twitch_toggle(&mut self) {
        let state = self.ui.twitch.get_untracked();
        if state.sending {
            self.twitch_disable_send();
            return;
        }
        if state.can_send() {
            self.twitch_state(|s| s.confirm = true);
        }
        self.update(Msg::SettingsPlatform(Platform::Twitch));
    }
    fn twitch_confirm(&mut self, accept: bool) {
        let state = self.ui.twitch.get_untracked();
        if !state.confirm {
            return;
        }
        let mut state = (*state).clone();
        let result = state.confirm_sending(self.core.draft(), accept);
        self.twitch.set_sending(state.sending);
        self.ui.twitch.set(Arc::new(state));
        if result.is_err() {
            self.notice(self.ui.now_t("bili-images-unavailable"), true);
        }
    }
    pub(super) fn twitch_submitted(&mut self, event: &CoreEvent) {
        if !self.ui.twitch.get_untracked().sending {
            return;
        }
        if let CoreEvent::LineSubmitted(line) = event {
            match outgoing::plain_text(&line.doc) {
                Ok(text) => {
                    if !self.twitch.send(line.id, text) {
                        self.notice(self.ui.now_t("bili-queue-full"), true);
                    }
                }
                Err(_) => self.notice(self.ui.now_t("bili-images-unavailable"), true),
            }
        }
    }
    pub(super) fn twitch_configure(&mut self) {
        let settings = &self.settings.integrations.twitch;
        let epoch = self.twitch.epoch();
        self.twitch.set(nanatype_twitch::Config {
            enabled: settings.enabled,
            channel: settings.channel.clone(),
        });
        if self.twitch.epoch() != epoch {
            self.twitch_disable_send();
        }
    }
    fn twitch_event(&mut self, session: SessionEvent) {
        // Receipts are kept even from an old epoch: the lines were sent.
        if let Event::Delivery(delivery) = &session.event {
            self.live_sent(&delivery.receipts);
        }
        if session.epoch != self.twitch.epoch() {
            return;
        }
        match session.event {
            Event::Account(account) => {
                if let AccountState::Ready(profile) = &account {
                    let asset = crate::services::chat_assets::cache_profile_avatar(
                        self.paths.blobs(),
                        &format!("twitch-{}", profile.user_id),
                        &profile.avatar,
                        {
                            let ctx = self.ctx.clone();
                            move |asset| ctx.dispatch(Msg::ChatAssetReady(asset))
                        },
                    );
                    self.twitch_state(|s| s.avatar = asset.map(|a| a.to_string()));
                } else {
                    self.twitch_disable_send();
                }
                self.twitch_state(|s| s.account = account);
            }
            Event::Connection(connection) => {
                if !matches!(connection, ConnectionState::Connected(_)) {
                    self.twitch_disable_send();
                }
                self.twitch_state(|s| s.connection = connection);
            }
            Event::Chat { channel, event } => {
                self._chat_assets.twitch(session.epoch, channel, event)
            }
            Event::Deleted { channel, id } => self.live_deleted(Platform::Twitch, &channel, &id),
            Event::Delivery(delivery) => {
                self.twitch_state(|s| {
                    if let Some(old) = s.deliveries.iter_mut().find(|d| d.line == delivery.line) {
                        *old = delivery.clone();
                    } else {
                        s.deliveries.push(delivery.clone());
                    }
                    while s.deliveries.len() > 200 {
                        if let Some(index) = s.deliveries.iter().position(|d| {
                            !d.blocked
                                && !matches!(
                                    d.state,
                                    DeliveryState::Queued | DeliveryState::Sending
                                )
                        }) {
                            s.deliveries.remove(index);
                        } else {
                            break;
                        }
                    }
                });
                if matches!(
                    delivery.state,
                    DeliveryState::Failed | DeliveryState::Uncertain
                ) {
                    self.notice(
                        format!(
                            "{}: {}",
                            self.ui.now_t("tw-delivery-failed"),
                            delivery.detail
                        ),
                        true,
                    );
                }
            }
            Event::Warning(code) => {
                let text = crate::ui::twitch::diagnostic(self.ui, &code);
                self.notice(text, true)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nanatype_twitch::{Profile, Room};

    fn ready() -> State {
        State {
            account: AccountState::Ready(Profile::default()),
            connection: ConnectionState::Connected(Room::default()),
            confirm: true,
            ..State::default()
        }
    }

    #[test]
    fn sending_starts_only_after_confirmation_with_readable_text() {
        let mut state = ready();
        assert_eq!(
            state.confirm_sending(&Document::plain("hi"), false),
            Ok(false)
        );
        let mut state = ready();
        assert_eq!(
            state.confirm_sending(&Document::plain("hi"), true),
            Ok(true)
        );
        let mut offline = State {
            connection: ConnectionState::Connecting,
            ..ready()
        };
        assert_eq!(offline.confirm_sending(&Document::new(), true), Ok(false));
    }

    #[test]
    fn only_twitchs_activation_page_opens() {
        assert!(activation_page(
            "https://www.twitch.tv/activate?device-code=ABCD"
        ));
        assert!(!activation_page("https://www.twitch.tv.evil.test/activate"));
        assert!(!activation_page("http://www.twitch.tv/activate"));
        assert!(!activation_page("https://www.twitch.tv/settings"));
    }
}
