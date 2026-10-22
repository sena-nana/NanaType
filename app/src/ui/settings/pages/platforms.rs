//! The live platforms: one page, a segment per platform, and the chat
//! panel's settings that every platform shares.

use super::*;

use crate::chat::Platform;
use nana_ui::runtime::view::segmented_option;
use nana_ui::runtime::{SegmentedControl, SegmentedOptionChosen};

/// The platform switch. Choosing goes through the program, which ends a
/// Bilibili QR sign-in left behind.
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
/// (The settings keep their Bilibili-era home.)
fn panel(ui: Ui) -> AnyView {
    section(
        ui,
        "bili-panel",
        vec![
            row(
                ui,
                "chat-panel-style",
                None,
                crate::ui::settings::chat_panel_style(ui),
            ),
            toggle_row!(ui, "chat-filter-text", integrations.bilibili.panel.text),
            toggle_row!(
                ui,
                "chat-filter-super-chat",
                integrations.bilibili.panel.super_chat
            ),
            toggle_row!(ui, "chat-filter-gift", integrations.bilibili.panel.gift),
            toggle_row!(ui, "chat-filter-member", integrations.bilibili.panel.member),
            toggle_row!(
                ui,
                "int-bilibili-quote-display",
                integrations.bilibili.quote_on_display
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
