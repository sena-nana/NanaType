//! The chat panel: how it reads, whichever platforms feed it — its style,
//! which kinds of message it lists, and quoting the replied-to message on
//! the box. (The settings are shared by all chat sources.)

use super::*;

pub(super) fn chat_panel(ui: Ui) -> AnyView {
    section(
        ui,
        "chat-panel-display",
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
