use crate::{app::App, chat::Platform, msg::Msg};
use nanatype_youtube::{ConnectionState, Event, SessionEvent};
use std::sync::Arc;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    pub connection: ConnectionState,
}
impl App {
    /// YouTube chat.
    pub(super) fn youtube_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Youtube(event) => self.youtube_event(event),
            Msg::YoutubeRefresh => self.youtube_refresh(),
            _ => unreachable!("not a youtube message"),
        }
    }

    fn youtube_event(&mut self, session: SessionEvent) {
        if session.epoch != self.youtube.epoch() {
            return;
        }
        match session.event {
            Event::Chat { video, event } => {
                crate::diag::YOUTUBE_LIVE.chat();
                self._chat_assets.youtube(session.epoch, video, event)
            }
            Event::Deleted { video, id } => self.live_deleted(Platform::Youtube, &video, &id),
            Event::Connection(connection) => {
                use crate::diag::Conn;
                let (code, error) = match &connection {
                    ConnectionState::Off => (Conn::Off, None),
                    ConnectionState::Resolving
                    | ConnectionState::Waiting
                    | ConnectionState::Connecting => (Conn::Connecting, None),
                    ConnectionState::Connected { .. } => (Conn::Connected, None),
                    ConnectionState::Ended | ConnectionState::Disconnected => {
                        (Conn::Disconnected, None)
                    }
                    ConnectionState::Error(e) => (Conn::Error, Some(e.as_str())),
                };
                crate::diag::YOUTUBE_LIVE.connection(code, error);
                self.ui
                    .youtube
                    .update(|state| Arc::make_mut(state).connection = connection)
            }
        }
    }
    pub(super) fn youtube_configure(&mut self) {
        let settings = &self.settings.integrations.youtube;
        self.youtube.set(nanatype_youtube::Config {
            enabled: settings.enabled,
            target: settings.target.clone(),
        });
    }
    fn youtube_refresh(&mut self) {
        let epoch = self.youtube.epoch();
        self.edit(|s| s.integrations.youtube.enabled = true);
        if self.youtube.epoch() == epoch {
            self.youtube.refresh();
        }
    }
}
