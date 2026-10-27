//! The live platforms: one page, a segment per platform, and the chat
//! panel's settings that every platform shares.

use super::*;

use crate::chat::Platform;
use nana_ui::runtime::view::segmented_option;
use nana_ui::runtime::{SegmentedControl, SegmentedOptionChosen};

/// The platform switch.
fn tabs(ui: Ui) -> AnyView {
    let options: Vec<AnyView> = Platform::ALL
        .iter()
        .copied()
        .map(|platform| {
            segmented_option(ui.now_t(platform.label_key()))
                .selected(move || ui.platform_tab.get() == platform)
                .on_cx(move |_, _: &SegmentedOptionChosen, cx| {
                    cx.dispatch_program_all(Msg::SettingsPlatform(platform))
                })
                .into_any()
        })
        .collect();
    widget(Stack::row(0.0))
        .children(widget(SegmentedControl::new().size(ControlSize::Small)).children(options))
        .into_any()
}

/// How the chat panel reads, whichever platforms feed it: its style, which
/// kinds of message it lists, and quoting the replied-to message on the box.
/// (The settings are shared by all chat sources.)
fn panel(ui: Ui) -> AnyView {
    section(
        ui,
        "chat-panel",
        vec![
            row(
                ui,
                "chat-panel-style",
                None,
                crate::ui::settings::chat_panel_style(ui),
            ),
            row(
                ui,
                "chat-filter-kinds",
                None,
                chip_toggles(
                    ui,
                    &[
                        (
                            "chat-filter-text",
                            |s| s.integrations.chat.panel.text,
                            |s, v| s.integrations.chat.panel.text = v,
                        ),
                        (
                            "chat-filter-super-chat",
                            |s| s.integrations.chat.panel.super_chat,
                            |s, v| s.integrations.chat.panel.super_chat = v,
                        ),
                        (
                            "chat-filter-gift",
                            |s| s.integrations.chat.panel.gift,
                            |s, v| s.integrations.chat.panel.gift = v,
                        ),
                        (
                            "chat-filter-member",
                            |s| s.integrations.chat.panel.member,
                            |s, v| s.integrations.chat.panel.member = v,
                        ),
                    ],
                ),
            ),
            toggle_row!(
                ui,
                "int-chat-quote-display",
                integrations.chat.quote_on_display
            ),
        ],
    )
}

pub(super) fn platforms(ui: Ui) -> AnyView {
    widget(Stack::column(0.0).gap_step(gap::step::SECTION))
        .children((
            tabs(ui),
            dynamic(
                move || ui.platform_tab.get(),
                move |platform| crate::ui::sources::platform_page(ui, *platform),
            ),
            panel(ui),
        ))
        .into_any()
}
