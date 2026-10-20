use super::Ui;
use super::settings::controls::{block, hint, row, section, switch, text};
use crate::msg::Msg;
use crate::ui::kit::gap;
use nana_ui::{
    ControlSize,
    runtime::view::{AnyView, IntoView, widget},
    runtime::{Activate, Button, Stack, Text, TextChanged, TextInput},
};
use nanatype_douyin::{AccountState, ConnectionState};
pub fn connection(ui: Ui) -> String {
    let state = ui.douyin.get();
    match &state.connection {
        ConnectionState::Off => ui.now_t("dy-off"),
        ConnectionState::Connecting => {
            format!("{} · {}", ui.now_t("dy-native"), ui.now_t("dy-connecting"))
        }
        ConnectionState::Connected { room } => format!(
            "{} · {} · {room}",
            ui.now_t("dy-native"),
            ui.now_t("dy-connected")
        ),
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
fn action(ui: Ui, key: &str, message: impl Fn() -> Msg + Send + 'static) -> AnyView {
    widget(Button::new(ui.now_t(key)).size(ControlSize::Small))
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(message()))
        .into_any()
}
pub fn settings_page(ui: Ui) -> AnyView {
    let status = widget(Text::new("")).bind(move |t: &mut Text| t.value = connection(ui));
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
    let warning = widget(Text::new(""))
        .visible(move || ui.douyin.get().warning.is_some())
        .bind(move |t: &mut Text| {
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
    widget(Stack::column(gap::SECTION))
        .children((
            section(
                ui,
                "dy-room",
                vec![
                    row(
                        ui,
                        "dy-room",
                        Some("dy-room-hint"),
                        text(
                            ui,
                            String::new(),
                            240.0,
                            |s| s.integrations.douyin.room.clone(),
                            |s, v| {
                                s.integrations.douyin.enabled = false;
                                s.integrations.douyin.room = v;
                            },
                        ),
                    ),
                    row(
                        ui,
                        "dy-enabled",
                        None,
                        switch(
                            ui,
                            |s| s.integrations.douyin.enabled,
                            |s, v| s.integrations.douyin.enabled = v,
                        ),
                    ),
                    block(status),
                    block(warning),
                    block(widget(Stack::row(gap::CONTROL)).children((
                        action(ui, "dy-connect", || Msg::DouyinRefresh),
                        action(ui, "dy-open-panel", || {
                            Msg::ShowWindow(crate::windows::WindowKind::Chat)
                        }),
                    ))),
                ],
            ),
            section(
                ui,
                "dy-account",
                vec![
                    block(account),
                    block(hint(ui.now_t("dy-account-hint"))),
                    block(action(ui, "dy-logout", || Msg::DouyinLogout)),
                    block(cookie),
                    block(action(ui, "dy-import", move || {
                        Msg::DouyinImport(ui.douyin_cookie.get_untracked())
                    })),
                ],
            ),
        ))
        .into_any()
}
