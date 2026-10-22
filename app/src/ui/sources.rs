//! The UI's one door to the chat sources a build may leave out (Bilibili web,
//! Douyin, YouTube); see `crate::app::unofficial`. Without them the hooks
//! below draw nothing and report nothing in progress.
use super::Ui;
use super::danmaku::Link;
use crate::chat::Platform;
use nana_ui::runtime::view::AnyView;
use nanatype_settings::Settings;

/// A platform's settings page.
pub fn platform_page(ui: Ui, platform: Platform) -> AnyView {
    match platform {
        Platform::Twitch => super::twitch::settings_page(ui),
        Platform::Blivechat => super::blivechat::settings_page(ui),
        #[cfg(feature = "unofficial-chat")]
        Platform::Bilibili => super::bilibili::settings_page(ui),
        #[cfg(feature = "unofficial-chat")]
        Platform::Douyin => super::douyin::settings_page(ui),
        #[cfg(feature = "unofficial-chat")]
        Platform::Youtube => super::youtube::settings_page(ui),
        #[cfg(not(feature = "unofficial-chat"))]
        Platform::Bilibili | Platform::Douyin | Platform::Youtube => on::nothing(),
    }
}

#[cfg(feature = "unofficial-chat")]
mod on {
    use super::*;

    /// Lines also go to a live chat, which takes plain text only (tracked),
    /// as `App::synchronizing` decides.
    pub fn plain_text_only(ui: Ui) -> bool {
        ui.bilibili.get().sending || ui.twitch.get().sending
    }

    /// Name and avatar follow the signed-in Bilibili account (tracked).
    pub fn profile_bound(ui: Ui) -> bool {
        ui.settings.with(|s| s.integrations.bilibili.sync_profile)
            && matches!(
                ui.bilibili.get().account,
                nanatype_bilibili::AccountState::Ready(_)
            )
    }

    pub fn identity_hint(ui: Ui) -> AnyView {
        super::super::bilibili::identity_hint(ui)
    }

    /// The input footer's sync switch and delivery results.
    pub fn input_send_status(ui: Ui) -> AnyView {
        super::super::bilibili::input_send_status(ui)
    }

    /// The input footer's account button.
    pub fn account_button(ui: Ui) -> AnyView {
        super::super::bilibili::account_button(ui)
    }

    pub fn confirm_dialog(ui: Ui) -> AnyView {
        super::super::bilibili::confirm_dialog(ui)
    }

    /// The chat panel's dot, for these sources (tracked).
    pub fn links(ui: Ui) -> Vec<Link> {
        let douyin = match ui.douyin.get().connection {
            nanatype_douyin::ConnectionState::Connected { .. } => Link::Connected,
            nanatype_douyin::ConnectionState::Connecting => Link::Connecting,
            nanatype_douyin::ConnectionState::Error(_) => Link::Error,
            _ => Link::Off,
        };
        vec![
            ui.status.with(|x| x.bilibili_link),
            douyin,
            super::super::youtube::link(&ui.youtube.get().connection),
        ]
    }

    /// The chat panel's status line, for these sources (tracked). Bilibili's
    /// line always leads, as it did when it was the only source.
    pub fn status_parts(ui: Ui) -> Vec<String> {
        let mut parts = vec![ui.status.with(|s| s.bilibili.clone())];
        if ui.settings.with(|s| s.integrations.douyin.enabled) {
            parts.push(format!(
                "{}: {}",
                ui.now_t("settings-douyin"),
                super::super::douyin::connection(ui)
            ));
        }
        if ui.settings.with(|s| s.integrations.youtube.enabled) {
            parts.push(format!(
                "{}: {}",
                ui.now_t("settings-youtube"),
                super::super::youtube::connection(ui)
            ));
        }
        parts
    }

    /// Any of these sources has somewhere to read from.
    pub fn configured(s: &Settings) -> bool {
        let b = &s.integrations.bilibili;
        (b.enabled && !b.room.is_empty())
            || (s.integrations.douyin.enabled && !s.integrations.douyin.room.is_empty())
            || (s.integrations.youtube.enabled && !s.integrations.youtube.target.is_empty())
    }
}

#[cfg(not(feature = "unofficial-chat"))]
mod on {
    use super::*;
    use nana_ui::runtime::Stack;
    use nana_ui::runtime::view::{IntoView, widget};

    /// Takes no space.
    pub(super) fn nothing() -> AnyView {
        widget(Stack::row(0.0)).visible(|| false).into_any()
    }

    pub fn plain_text_only(ui: Ui) -> bool {
        ui.twitch.get().sending
    }

    pub fn profile_bound(_: Ui) -> bool {
        false
    }

    pub fn identity_hint(_: Ui) -> AnyView {
        nothing()
    }

    pub fn input_send_status(_: Ui) -> AnyView {
        nothing()
    }

    pub fn account_button(_: Ui) -> AnyView {
        nothing()
    }

    pub fn confirm_dialog(_: Ui) -> AnyView {
        nothing()
    }

    pub fn links(_: Ui) -> Vec<Link> {
        Vec::new()
    }

    pub fn status_parts(_: Ui) -> Vec<String> {
        Vec::new()
    }

    pub fn configured(_: &Settings) -> bool {
        false
    }
}

pub use on::*;
