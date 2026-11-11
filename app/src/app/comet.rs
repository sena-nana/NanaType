//! The 彗星号 client's feed: Bilibili's rooms when the Bilibili page picks
//! it, the other platforms' when their switch is on, over one connection.

use crate::ui::EditShared;

use nanatype_blive::{BliveConfig, BliveEvent, BliveStatus, BliveUpdate, Join, comet};

use crate::{app::App, chat::Platform};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    pub status: BliveStatus,
}

impl App {
    pub(super) fn comet_event(&mut self, event: BliveEvent) {
        if event.epoch != self.comet.epoch() {
            return;
        }
        match event.update {
            BliveUpdate::Status(status) => {
                match &status {
                    BliveStatus::Error(fault) => {
                        nanatype_diag::report!(crate::diag::COMET_ERROR; "{fault}");
                    }
                    BliveStatus::Fatal(kind, message) => {
                        nanatype_diag::report!(crate::diag::COMET_ERROR; "{kind:?}: {message}");
                    }
                    _ => {}
                }
                self.ui.comet.edit(|s| s.status = status);
                self.comet_status_line();
            }
            BliveUpdate::Chat(chat) => self.blive_chat(Platform::Comet, event.epoch, chat),
        }
    }

    /// Connect, reconnect or stop after a settings change. A change of the
    /// platforms read is a new configuration, so it connects again.
    pub(super) fn comet_configure(&mut self) {
        let i = &self.settings.integrations;
        let wanted = i.comet_rooms();
        let rooms = comet::Rooms {
            bilibili: wanted.bilibili,
            others: wanted.others,
        };
        let client = rooms.any().then(|| BliveConfig {
            host: "127.0.0.1".into(),
            port: i.comet.port,
            join: Join::Comet(rooms),
        });
        let epoch = self.comet.epoch();
        let joining = client.is_some();
        self.comet.set(client);
        if self.comet.epoch() != epoch {
            self.ui.comet.edit(|s| {
                s.status = if joining {
                    BliveStatus::Connecting
                } else {
                    BliveStatus::Off
                }
            });
        }
        self.comet_status_line();
    }

    /// Put the status line and the chat panel's dot in step with the state.
    fn comet_status_line(&mut self) {
        let locale = self.ui.locale.get_untracked();
        let state = self.ui.comet.get_untracked();
        let (text, link) = crate::ui::link_text::comet(locale, &self.settings.integrations, &state);
        self.edit_status(|s| {
            s.comet = text;
            s.comet_link = link;
        });
    }
}
