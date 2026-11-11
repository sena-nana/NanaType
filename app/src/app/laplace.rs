//! Bilibili chat from a LAPLACE Event Bridge, when the pasted link is one.

use nanatype_blive::{BliveConfig, BliveEvent, BliveStatus, BliveUpdate, Join};

use crate::ui::EditShared;

use crate::{app::App, chat::Platform, msg::LaplaceMsg};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    pub status: BliveStatus,
    /// A bridge token is kept in the secret store.
    pub token_saved: bool,
}

impl App {
    pub(super) fn laplace_msg(&mut self, msg: LaplaceMsg) {
        match msg {
            LaplaceMsg::Event(event) => self.laplace_event(event),
        }
    }

    fn laplace_event(&mut self, event: BliveEvent) {
        if event.epoch != self.laplace.epoch() {
            return;
        }
        match event.update {
            BliveUpdate::Status(status) => {
                match &status {
                    BliveStatus::Error(fault) => {
                        nanatype_diag::report!(crate::diag::LAPLACE_ERROR; "{fault}");
                    }
                    BliveStatus::Fatal(kind, message) => {
                        nanatype_diag::report!(crate::diag::LAPLACE_FATAL; "{kind:?}: {message}");
                    }
                    _ => {}
                }
                let fatal = matches!(status, BliveStatus::Fatal(..));
                self.ui.laplace.edit(|s| s.status = status);
                let text = self.laplace_status_line();
                if fatal {
                    self.notice(text, true);
                }
            }
            BliveUpdate::Chat(chat) => self.blive_chat(Platform::Laplace, event.epoch, chat),
        }
    }

    /// Connect, reconnect or stop after a settings change.
    pub(super) fn laplace_configure(&mut self) {
        let client = if self.settings.integrations.laplace_on() {
            // Only read the secret store once the token is wanted.
            let token = self.laplace_token.get();
            let saved = token.is_some();
            self.ui.laplace.edit(|s| s.token_saved = saved);
            let link = nanatype_blive::laplace::Link {
                address: self.settings.integrations.laplace.url.clone(),
                token: token.unwrap_or_default(),
            };
            link.url().map(|url| BliveConfig {
                host: String::new(),
                port: 0,
                join: Join::Laplace { url },
            })
        } else {
            None
        };
        let epoch = self.laplace.epoch();
        let joining = client.is_some();
        self.laplace.set(client);
        if self.laplace.epoch() != epoch {
            self.ui.laplace.edit(|s| {
                s.status = if joining {
                    BliveStatus::Connecting
                } else {
                    BliveStatus::Off
                }
            });
        }
        self.laplace_status_line();
    }

    /// Put the status line and the chat panel's dot in step with the state.
    fn laplace_status_line(&mut self) -> String {
        let locale = self.ui.locale.get_untracked();
        let state = self.ui.laplace.get_untracked();
        let (text, link) =
            crate::ui::link_text::laplace(locale, &self.settings.integrations, &state);
        self.edit_status(|s| {
            s.laplace = text.clone();
            s.laplace_link = link;
        });
        text
    }
}
