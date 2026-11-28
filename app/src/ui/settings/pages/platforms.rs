//! The live platforms: one page, a segment per platform. The platform
//! switch sits in the page header.

use super::*;
use crate::msg::WindowMsg;

use crate::ui::sources::PlatformTab;
use nana_ui::runtime::view::segmented_option;
use nana_ui::runtime::{SegmentedControl, SegmentedOptionChosen};

/// The platform switch, shown in the page header.
pub(super) fn tabs(ui: Ui) -> AnyView {
    let options: Vec<AnyView> = PlatformTab::ALL
        .iter()
        .copied()
        .map(|platform| {
            segmented_option(ui.now_t(platform.label_key()))
                .selected(move || ui.platform_tab.get() == platform)
                .on_cx(move |_, _: &SegmentedOptionChosen, cx| {
                    cx.dispatch_program_all(Msg::Window(WindowMsg::SettingsPlatform(platform)))
                })
                .into_any()
        })
        .collect();
    widget(
        SegmentedControl::new()
            .size(ControlSize::Small)
            .label(ui.now_t("settings-platforms")),
    )
    .children(options)
    .into_any()
}

pub(super) fn platforms(ui: Ui) -> AnyView {
    dynamic(
        move || ui.platform_tab.get(),
        move |platform| crate::ui::sources::platform_page(ui, *platform),
    )
    .into_any()
}
