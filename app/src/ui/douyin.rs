use super::Ui;
use super::settings::controls::{
    block, button, info_tip, row, row_live_hint, section, section_with, shown_if, switch, text,
};
use crate::msg::Msg;
use crate::ui::kit::{gap, hstack, line};
use nana_ui::{
    ControlSize,
    runtime::view::{AnyView, IntoView, widget},
    runtime::{Stack, Text, TextChanged, TextInput},
};
use nanatype_douyin::{AccountState, ConnectionState};
pub fn connection(ui: Ui) -> String {
    let state = ui.douyin.get();
    match &state.connection {
        ConnectionState::Off => ui.now_t("dy-off"),
        ConnectionState::Connecting => ui.now_t("dy-connecting"),
        ConnectionState::Connected { room } => format!("{} · {room}", ui.now_t("dy-connected")),
        ConnectionState::Disconnected => ui.now_t("dy-disconnected"),
        ConnectionState::Error(code) => {
            format!("{} · {}", ui.now_t("dy-error"), diagnostic(ui, code))
        }
    }
}
fn diagnostic(ui: Ui, code: &str) -> String {
    ui.now_t(match code {
        "invalid-room" => "dy-invalid-room",
        "protected-storage-unavailable" => "dy-storage-error",
        "credentials-clear-failed" => "dy-clear-error",
        "account-unverified" | "invalid-cookie" => "dy-account-unverified",
        "room-offline" => "dy-room-offline",
        "invalid-frame" => "dy-invalid-frame",
        "native-no-messages" => "dy-no-messages",
        _ => "dy-native-error",
    })
}
pub fn settings_page(ui: Ui) -> AnyView {
    let account = widget(Text::new("")).bind(move |t: &mut Text| {
        t.value = match &ui.douyin.get().account {
            AccountState::Ready { name, .. } => {
                format!("{} · {name}", ui.now_t("dy-account-ready"))
            }
            AccountState::SignedOut => ui.now_t("dy-account-out"),
            AccountState::Unverified => ui.now_t("dy-account-unverified"),
            AccountState::Expired => ui.now_t("dy-account-expired"),
        }
    });
    let warning = widget(Text::new("")).bind(move |t: &mut Text| {
        t.value = ui
            .douyin
            .get()
            .warning
            .as_deref()
            .map(|s| diagnostic(ui, s))
            .unwrap_or_default();
    });
    let cookie = widget(
        TextInput::new("")
            .secure(true)
            .placeholder(ui.now_t("dy-cookie-placeholder"))
            .size(ControlSize::Small),
    )
    .bind(move |input: &mut TextInput| {
        let text = ui.douyin_cookie.get();
        if input.state.value.as_ref() != text.as_str() {
            input.state.value = text.into();
        }
    })
    .on(move |event: &TextChanged| {
        ui.douyin_cookie
            .set(event.value.chars().take(65536).collect())
    });
    widget(Stack::column(0.0).gap_step(gap::step::SECTION))
        .children((
            section_with(
                ui,
                "",
                vec![
                    (
                        row(
                            ui,
                            "dy-room",
                            None,
                            text(
                                ui,
                                ui.now_t("dy-room-placeholder"),
                                crate::ui::kit::field::MD,
                                |s| s.integrations.douyin.room.clone(),
                                |s, v| {
                                    s.integrations.douyin.enabled = false;
                                    s.integrations.douyin.room = v;
                                },
                            ),
                        ),
                        None,
                    ),
                    (
                        row_live_hint(
                            ui,
                            "dy-enabled",
                            move || Some(connection(ui)),
                            hstack((
                                button(ui, "dy-connect", || Msg::DouyinRefresh),
                                switch(
                                    ui,
                                    |s| s.integrations.douyin.enabled,
                                    |s, v| s.integrations.douyin.enabled = v,
                                ),
                            )),
                        ),
                        None,
                    ),
                    shown_if(block(warning), move || ui.douyin.get().warning.is_some()),
                ],
            ),
            section(
                ui,
                "dy-account",
                vec![
                    line(
                        account,
                        (
                            info_tip(ui, "dy-account-hint"),
                            button(ui, "dy-logout", || Msg::DouyinLogout),
                        ),
                    ),
                    line(
                        cookie,
                        button(ui, "dy-import", move || {
                            Msg::DouyinImport(ui.douyin_cookie.get_untracked())
                        }),
                    ),
                ],
            ),
        ))
        .into_any()
}
