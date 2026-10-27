//! Twitch: the channel to read, the device-code sign-in and sending.
use super::Ui;
use super::danmaku::Link;
use super::settings::controls::{block, button, hint, row, row_live_hint, section, switch, text};
use crate::msg::Msg;
use crate::ui::kit::{Type, gap, inset, line};
use nana_ui::runtime::view::{AnyView, IntoView, dynamic, widget};
use nana_ui::runtime::{
    ConfirmDialog, ConfirmIntent, DialogCloseRequested, Stack, Switch, Text, ToggleChanged,
};
use nana_ui_core::SemanticColorRole;
use nanatype_twitch::{AccountState, ConnectionState, DeliveryState};

pub fn link(state: &ConnectionState) -> Link {
    match state {
        ConnectionState::Connected(_) => Link::Connected,
        ConnectionState::Connecting => Link::Connecting,
        ConnectionState::Error(_) => Link::Error,
        ConnectionState::Off => Link::Off,
    }
}
pub fn connection(ui: Ui) -> String {
    match &ui.twitch.get().connection {
        ConnectionState::Off => ui.now_t("tw-off"),
        ConnectionState::Connecting => ui.now_t("tw-connecting"),
        ConnectionState::Connected(room) => {
            format!("{} · #{}", ui.now_t("tw-connected"), room.login)
        }
        ConnectionState::Error(code) => {
            format!("{} · {}", ui.now_t("tw-error"), diagnostic(ui, code))
        }
    }
}
/// A stable code in the UI language; Twitch's own messages pass through.
pub fn diagnostic(ui: Ui, code: &str) -> String {
    let key = match code {
        "invalid-channel" => "tw-invalid-channel",
        "channel-unavailable" | "msg_channel_suspended" | "msg_room_not_found" => {
            "tw-channel-unavailable"
        }
        "msg_banned" | "tos_ban" => "tw-banned",
        "protected-storage-unavailable" => "tw-storage-error",
        "login-expired" => "tw-login-expired",
        "transport-connect-failed" | "disconnected" | "heartbeat-timeout" => "tw-network-error",
        "worker-start-failed" | "invalid-endpoint" => "tw-network-error",
        other => return other.to_owned(),
    };
    ui.now_t(key)
}
/// The account as one line: who (or what is happening) on the left, what
/// can be done on the right; a sign-in code unfolds under it.
fn account_card(ui: Ui) -> AnyView {
    dynamic(
        move || (ui.twitch.get().account.clone(), ui.locale.get()),
        move |(account, _)| {
            let title = match account {
                AccountState::Ready(p) => p.name.clone(),
                AccountState::Restoring => ui.now_t("tw-restoring"),
                AccountState::Pending { .. } => ui.now_t("tw-pending"),
                AccountState::Expired => ui.now_t("tw-expired"),
                AccountState::Failed(_) => ui.now_t("tw-login-failed"),
                AccountState::SignedOut => ui.now_t("tw-signed-out"),
            };
            let (detail, actions, code) = match account {
                AccountState::Ready(p) => (
                    Some(format!("@{}", p.login)),
                    vec![
                        button(ui, "tw-refresh", || Msg::TwitchRefresh),
                        button(ui, "tw-logout", || Msg::TwitchLogout),
                    ],
                    None,
                ),
                AccountState::Pending { code, .. } => (
                    None,
                    vec![
                        button(ui, "tw-open-activation", || Msg::TwitchOpenActivation),
                        button(ui, "common-cancel", || Msg::TwitchCancelLogin),
                    ],
                    Some(block((
                        hint(ui.now_t("tw-pending-hint")),
                        widget(Text::new(code.clone()).style(Type::PageTitle.mono())),
                    ))),
                ),
                AccountState::Restoring => (None, Vec::new(), None),
                AccountState::Failed(message) => (
                    Some(diagnostic(ui, message)),
                    vec![
                        button(ui, "tw-login", || Msg::TwitchLogin),
                        button(ui, "tw-refresh", || Msg::TwitchRefresh),
                    ],
                    None,
                ),
                _ => (
                    Some(ui.now_t("tw-login-hint")),
                    vec![button(ui, "tw-login", || Msg::TwitchLogin)],
                    None,
                ),
            };
            widget(Stack::column(0.0))
                .children((
                    line(
                        (
                            widget(Text::new(title).style(Type::Strong.one_line())),
                            detail.map(hint),
                        ),
                        actions,
                    ),
                    code,
                ))
                .into_any()
        },
    )
    .into_any()
}
fn send_toggle(ui: Ui) -> AnyView {
    widget(Switch::new(String::new(), false))
        .bind(move |s: &mut Switch| {
            let state = ui.twitch.get();
            s.checked = state.sending;
            s.disabled = !state.can_send() && !state.sending;
        })
        .on_cx(|_, _: &ToggleChanged, cx| cx.dispatch_program_all(Msg::TwitchSendToggle))
        .into_any()
}
fn deliveries(ui: Ui) -> AnyView {
    dynamic(
        move || (ui.twitch.get().deliveries.clone(), ui.locale.get()),
        move |(deliveries, _)| {
            let rows: Vec<AnyView> = deliveries
                .iter()
                .filter(|d| d.blocked)
                .chain(deliveries.iter().rev().filter(|d| !d.blocked).take(20))
                .map(|d| {
                    let line = d.line;
                    let remaining = d.remaining();
                    let label = ui.now_t(match d.state {
                        DeliveryState::Queued => "tw-queued",
                        DeliveryState::Sending => "tw-sending",
                        DeliveryState::Submitted => "tw-submitted",
                        DeliveryState::Failed => "tw-failed",
                        DeliveryState::Uncertain => "tw-uncertain",
                        DeliveryState::Cancelled => "tw-cancelled",
                    });
                    let mut row = vec![
                        widget(Text::new(d.chunks.concat()).style(Type::Chat.style())).into_any(),
                        hint(format!("{label} · {}/{}", d.completed, d.chunks.len())),
                    ];
                    if !d.detail.is_empty() {
                        row.push(hint(d.detail.clone()));
                    }
                    if matches!(d.state, DeliveryState::Failed | DeliveryState::Uncertain) {
                        row.push(button(ui, "tw-copy-remaining", move || {
                            Msg::CopyText(remaining.clone())
                        }));
                        if d.blocked {
                            row.push(button(ui, "tw-retry", move || Msg::TwitchRetry(line)));
                            row.push(button(ui, "tw-skip-failed", move || {
                                Msg::TwitchSkipFailed(line)
                            }));
                        }
                    }
                    widget(
                        Stack::column(0.0)
                            .gap_step(gap::step::CONTROL)
                            .padding_step(inset::step::PANEL, inset::step::PANEL)
                            .surface(SemanticColorRole::Subtle)
                            .radius(nana_ui_core::RadiusTier::Sm),
                    )
                    .children(row)
                    .into_any()
                })
                .collect();
            widget(Stack::column(0.0).gap_step(gap::step::CONTROL))
                .children(rows)
                .into_any()
        },
    )
    .into_any()
}
pub fn confirm_dialog(ui: Ui) -> AnyView {
    widget(
        ConfirmDialog::new(ui.now_t("tw-send-title"), ui.now_t("tw-send-warning"))
            .confirm_label(ui.now_t("tw-enable"))
            .cancel_label(ui.now_t("common-cancel")),
    )
    .bind(move |dialog: &mut ConfirmDialog| {
        let state = ui.twitch.get();
        let identity = match &state.account {
            AccountState::Ready(p) => p.name.clone(),
            _ => String::new(),
        };
        let room = match &state.connection {
            ConnectionState::Connected(r) => format!("#{}", r.login),
            _ => String::new(),
        };
        dialog.message = format!("{identity} · {room}\n\n{}", ui.now_t("tw-send-warning")).into();
    })
    .open(move || ui.twitch.get().confirm)
    .on_cx(|_, intent: &ConfirmIntent, cx| {
        cx.dispatch_program_all(Msg::TwitchSendConfirm(matches!(
            intent,
            ConfirmIntent::Confirm { .. }
        )))
    })
    .on_cx(|_, _: &DialogCloseRequested, cx| cx.dispatch_program_all(Msg::TwitchSendConfirm(false)))
    .into_any()
}
pub fn settings_page(ui: Ui) -> AnyView {
    widget(Stack::column(0.0).gap_step(gap::step::SECTION))
        .children((
            section(
                ui,
                "",
                vec![
                    row(
                        ui,
                        "tw-channel",
                        None,
                        text(
                            ui,
                            ui.now_t("tw-channel-placeholder"),
                            crate::ui::kit::field::MD,
                            |s| s.integrations.twitch.channel.clone(),
                            |s, v| {
                                s.integrations.twitch.enabled = false;
                                s.integrations.twitch.channel = v;
                            },
                        ),
                    ),
                    row_live_hint(
                        ui,
                        "tw-enabled",
                        move || Some(connection(ui)),
                        switch(
                            ui,
                            |s| s.integrations.twitch.enabled,
                            |s, v| s.integrations.twitch.enabled = v,
                        ),
                    ),
                ],
            ),
            section(ui, "tw-account", vec![account_card(ui)]),
            section(
                ui,
                "tw-send-section",
                vec![
                    row(ui, "tw-send-switch", None, send_toggle(ui)),
                    deliveries(ui),
                ],
            ),
        ))
        .into_any()
}
