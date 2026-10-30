//! The live chat sources that remain available in this build.

use super::Ui;
use super::danmaku::Link;
use crate::chat::Platform;
use nana_ui::runtime::Stack;
use nana_ui::runtime::view::{AnyView, IntoView, widget};
use nanatype_settings::Settings;

/// A platform's settings page.
pub fn platform_page(ui: Ui, platform: Platform) -> AnyView {
    match platform {
        Platform::Twitch => super::twitch::settings_page(ui),
        Platform::Blivechat => super::blivechat::settings_page(ui),
    }
}

fn nothing() -> AnyView {
    widget(Stack::row(0.0)).visible(|| false).into_any()
}

/// Twitch is the only source that can make the editor text plain-text-only.
pub fn plain_text_only(ui: Ui) -> bool {
    ui.twitch.get().sending
}

pub fn profile_bound(_: Ui) -> bool {
    false
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
