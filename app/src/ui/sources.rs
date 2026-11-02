//! The live chat sources that remain available in this build.

use super::Ui;
use crate::chat::Platform;
use nana_ui::runtime::view::AnyView;

/// A platform's settings page.
pub fn platform_page(ui: Ui, platform: Platform) -> AnyView {
    match platform {
        Platform::Twitch => super::twitch::settings_page(ui),
        Platform::Blivechat => super::blivechat::settings_page(ui),
    }
}

/// Twitch is the only source that can make the editor text plain-text-only.
pub fn plain_text_only(ui: Ui) -> bool {
    ui.twitch.get().sending
}
