//! The one door to the chat sources read through reverse-engineered web
//! interfaces (Bilibili web, Douyin, YouTube). A build without the
//! `unofficial-chat` feature gets the inert half below and never links them;
//! their settings stay in the file, untouched, for the build that has them.
use super::App;
use crate::chat::Platform;
use nanatype_core::Event as CoreEvent;
use nanatype_settings::Integrations;

#[cfg(feature = "unofficial-chat")]
impl App {
    pub(super) fn unofficial_configure(&mut self, i: &Integrations) {
        let b = &i.bilibili;
        let config = nanatype_bilibili::BilibiliConfig {
            enabled: b.enabled
                && (b.room.is_empty() || b.room.parse::<u64>().is_ok_and(|id| id > 0)),
            room: b.room.trim().parse().ok().filter(|id| *id > 0),
        };
        let previous_epoch = self.bilibili.epoch();
        self.bilibili.set(config);
        if self.bilibili.epoch() != previous_epoch {
            self.bilibili_reconnecting();
        }
        self.sync_bilibili_speaker();
        self.douyin_configure();
        self.youtube_configure();
    }

    /// Lines are also being sent to Bilibili.
    pub(super) fn unofficial_sending(&self) -> bool {
        self.ui.bilibili.get_untracked().sending
    }

    pub(super) fn unofficial_submitted(&mut self, event: &CoreEvent) {
        self.bilibili_submitted(event);
    }

    /// The connection a message must belong to; `None` for sources this
    /// build does not have.
    pub(super) fn unofficial_epoch(&self, platform: Platform) -> Option<u64> {
        match platform {
            Platform::Bilibili => Some(self.bilibili.epoch()),
            Platform::Douyin => Some(self.douyin.epoch()),
            Platform::Youtube => Some(self.youtube.epoch()),
            Platform::Blivechat | Platform::Twitch => None,
        }
    }

    /// A QR sign-in only lives while its card is in view.
    pub(super) fn unofficial_page_left(&mut self, on_bilibili: bool) {
        if !on_bilibili
            && matches!(
                self.ui.bilibili.get_untracked().account,
                nanatype_bilibili::AccountState::Qr { .. }
            )
        {
            self.bilibili.cancel_login();
        }
    }

    /// The settings window closed: drop the sign-in and any open confirmation.
    pub(crate) fn unofficial_settings_closed(&mut self) {
        self.unofficial_page_left(false);
        self.ui
            .bilibili
            .update(|s| std::sync::Arc::make_mut(s).confirm = false);
    }
}

#[cfg(not(feature = "unofficial-chat"))]
impl App {
    pub(super) fn unofficial_configure(&mut self, _: &Integrations) {}

    pub(super) fn unofficial_sending(&self) -> bool {
        false
    }

    pub(super) fn unofficial_submitted(&mut self, _: &CoreEvent) {}

    pub(super) fn unofficial_epoch(&self, _: Platform) -> Option<u64> {
        None
    }

    pub(super) fn unofficial_page_left(&mut self, _: bool) {}

    pub(crate) fn unofficial_settings_closed(&mut self) {}

    // Only Bilibili messages reach these, and none arrive in this build.
    pub(super) fn bilibili_message_deleted(&self, _: &crate::chat::ChatMessage) -> bool {
        false
    }

    pub(super) fn bilibili_history(&mut self, _: &crate::chat::ChatMessage) -> bool {
        false
    }

    pub(super) fn bilibili_record_row(&mut self, _: &crate::chat::ChatMessage) {}
}
