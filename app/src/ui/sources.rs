//! The live chat platforms, as the settings page groups them.

use super::Ui;
use nana_ui::runtime::view::AnyView;

/// A tab of the live platforms page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlatformTab {
    /// A chat page's OBS link (blivechat or LAPLACE), or 彗星号.
    Bilibili,
    Twitch,
    /// 彗星号's rooms on platforms other than Bilibili.
    Other,
}

impl PlatformTab {
    /// The tabs, in settings order.
    pub const ALL: &[Self] = &[Self::Bilibili, Self::Twitch, Self::Other];

    pub fn label_key(self) -> &'static str {
        match self {
            Self::Bilibili => "settings-bilibili",
            Self::Twitch => "settings-twitch",
            Self::Other => "settings-other-platforms",
        }
    }
}

/// A tab's settings page.
pub fn platform_page(ui: Ui, tab: PlatformTab) -> AnyView {
    match tab {
        PlatformTab::Bilibili => super::bilibili::settings_page(ui),
        PlatformTab::Twitch => super::twitch::settings_page(ui),
        PlatformTab::Other => super::bilibili::other_platforms_page(ui),
    }
}

/// Twitch is the only source that can make the editor text plain-text-only.
pub fn plain_text_only(ui: Ui) -> bool {
    ui.twitch.get().sending
}
