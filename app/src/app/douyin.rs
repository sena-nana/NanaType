use crate::{app::App, msg::Msg};
use nanatype_douyin::{AccountState, ConnectionState, Event, SessionEvent};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    pub account: AccountState,
    pub connection: ConnectionState,
    pub warning: Option<String>,
}
impl App {
    /// Douyin login and chat.
    pub(super) fn douyin_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Douyin(event) => self.douyin_event(event),
            Msg::DouyinRefresh => self.douyin_refresh(),
            Msg::DouyinLogout => self.douyin_logout(),
            Msg::DouyinImport(cookie) => self.douyin_import(cookie),
            _ => unreachable!("not a douyin message"),
        }
    }

    pub(super) fn douyin_event(&mut self, session: SessionEvent) {
        if session.epoch != self.douyin.epoch() {
            return;
        }
        match session.event {
            Event::Chat { room, event } => {
                crate::diag::DOUYIN_LIVE.chat();
                self._chat_assets.douyin(session.epoch, room, event)
            }
            Event::Account(account) => {
                use crate::diag::Account;
                let code = match &account {
                    AccountState::SignedOut => Account::SignedOut,
                    AccountState::Unverified => Account::Pending,
                    AccountState::Ready { .. } => Account::Ready,
                    AccountState::Expired => Account::Expired,
                };
                crate::diag::DOUYIN_LIVE.account(code, None);
                self.ui
                    .douyin
                    .update(|state| std::sync::Arc::make_mut(state).account = account)
            }
            Event::Connection(connection) => {
                use crate::diag::Conn;
                let (code, error) = match &connection {
                    ConnectionState::Off => (Conn::Off, None),
                    ConnectionState::Connecting => (Conn::Connecting, None),
                    ConnectionState::Connected { .. } => (Conn::Connected, None),
                    ConnectionState::Disconnected => (Conn::Disconnected, None),
                    ConnectionState::Error(e) => (Conn::Error, Some(e.as_str())),
                };
                crate::diag::DOUYIN_LIVE.connection(code, error);
                self.ui
                    .douyin
                    .update(|state| std::sync::Arc::make_mut(state).connection = connection)
            }
            Event::Warning(code) => {
                crate::diag::DOUYIN_LIVE.warning(code);
                self.ui
                    .douyin
                    .update(|state| std::sync::Arc::make_mut(state).warning = Some(code.into()))
            }
        }
    }
    pub(super) fn douyin_logout(&mut self) {
        self.edit(|s| s.integrations.douyin.enabled = false);
        self.ui.douyin_cookie.set(String::new());
        self.douyin.logout();
    }
    pub(super) fn douyin_import(&mut self, cookie: String) {
        self.edit(|s| s.integrations.douyin.enabled = false);
        self.ui.douyin_cookie.set(String::new());
        self.douyin.import_cookie(cookie);
    }
    pub(super) fn douyin_configure(&mut self) {
        let settings = &self.settings.integrations.douyin;
        self.douyin.set(nanatype_douyin::Config {
            enabled: settings.enabled,
            room: settings.room.clone(),
        });
    }
    pub(super) fn douyin_refresh(&mut self) {
        let epoch = self.douyin.epoch();
        self.ui
            .douyin
            .update(|s| std::sync::Arc::make_mut(s).warning = None);
        self.edit(|s| s.integrations.douyin.enabled = true);
        if self.douyin.epoch() == epoch {
            self.douyin.refresh();
        }
    }
}
