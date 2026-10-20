//! Account management stays in settings; the editor has two compact entry points.
use super::{
    Ui,
    settings::{
        Page,
        controls::{hint, row, section, switch},
    },
};
use crate::msg::Msg;
use crate::ui::kit::{Tone, Type, gap, icon, inset};
use nana_ui::icons_tabler as ti;
use nana_ui::runtime::view::{AnyView, IntoView, dynamic, signal, widget};
use nana_ui::runtime::{
    Activate, Button, ConfirmDialog, ConfirmIntent, DialogCloseRequested, IconGlyph, Popover,
    QrCode, Stack, Switch, Text, TextChanged, TextInput, TextSubmitted, ToggleChanged,
};
use nana_ui::{ButtonKind, ControlSize};
use nana_ui_core::{AlignSpec, LengthSpec, RadiusTier, SemanticColorRole};
use nanatype_bilibili::{AccountState, ConnectionState, DeliveryState, QrStatus};

fn button(ui: Ui, key: &str, message: impl Fn() -> Msg + Send + 'static) -> AnyView {
    widget(Button::new(ui.now_t(key)).size(ControlSize::Small))
        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(message()))
        .into_any()
}
pub fn account_label(ui: Ui, account: &AccountState) -> String {
    match account {
        AccountState::Ready(p) => p.name.clone(),
        AccountState::Restoring => ui.now_t("bili-restoring"),
        AccountState::Expired => ui.now_t("bili-expired"),
        AccountState::Failed(_) => ui.now_t("bili-login-failed"),
        AccountState::Qr { status, .. } => ui.now_t(match status {
            QrStatus::Waiting => "bili-qr-waiting",
            QrStatus::Scanned => "bili-qr-scanned",
            QrStatus::Expired => "bili-qr-expired",
            QrStatus::Confirmed => "bili-restoring",
        }),
        AccountState::SignedOut => ui.now_t("bili-login"),
    }
}
fn avatar(ui: Ui, size: f32) -> AnyView {
    let face = dynamic(
        move || ui.bilibili.get().avatar.clone(),
        move |asset| {
            widget(
                Stack::row(0.0)
                    .width(LengthSpec::Px(size))
                    .height(LengthSpec::Px(size))
                    .radius(RadiusTier::Full)
                    .with_layout(move |l| {
                        l.flex_shrink = Some(0.0);
                        l.position = nana_ui_core::PositionSpec::Relative;
                        l.overflow_x = nana_ui_core::OverflowSpec::Hidden;
                        l.overflow_y = nana_ui_core::OverflowSpec::Hidden;
                    }),
            )
            .children((
                widget(IconGlyph::new(ti::USER_CIRCLE).size(size)),
                super::danmaku::reactive_picture(ui, asset.clone(), size / 2.0),
            ))
            .into_any()
        },
    );
    widget(
        Stack::row(0.0)
            .width(LengthSpec::Px(size))
            .height(LengthSpec::Px(size))
            .with_layout(|l| l.flex_shrink = Some(0.0)),
    )
    .children(face)
    .into_any()
}
pub fn account_button(ui: Ui) -> AnyView {
    let face = widget(
        Stack::row(0.0)
            .width(LengthSpec::Px(28.0))
            .height(LengthSpec::Px(28.0))
            .align(AlignSpec::Center)
            .with_layout(|l| l.position = nana_ui_core::PositionSpec::Relative),
    )
    .children((
        avatar(ui, 24.0),
        // Needs attention: a warning dot on the avatar's corner.
        widget(
            crate::ui::kit::dot_frame()
                .surface(Tone::Warning.role())
                .with_layout(|l| {
                    l.position = nana_ui_core::PositionSpec::Absolute;
                    l.offset_right = Some(LengthSpec::Px(0.0));
                    l.offset_top = Some(LengthSpec::Px(0.0));
                }),
        )
        .visible(move || {
            matches!(
                ui.bilibili.get().account,
                AccountState::Expired | AccountState::Failed(_)
            )
        }),
    ));
    widget(
        Popover::new()
            .trigger(ui.now_t("bili-account"))
            .width(320.0)
            .bare_trigger(true),
    )
    .trigger(face)
    .children(
        widget(Stack::column(gap::GROUP).padding(inset::CARD)).children((
            dynamic(
                move || ui.bilibili.get().account.clone(),
                move |a| {
                    widget(Text::new(account_label(ui, a)).style(Type::Title.style())).into_any()
                },
            ),
            dynamic(
                move || ui.bilibili.get().connection.clone(),
                move |c| hint(super::link_text::bilibili(ui.locale.get_untracked(), c, "").0),
            ),
            button(ui, "bili-manage", || Msg::SettingsPage(Page::Bilibili)),
            widget(Stack::column(0.0))
                .visible(move || !matches!(ui.bilibili.get().account, AccountState::Ready(_)))
                .children(button(ui, "bili-login", || Msg::BilibiliLogin)),
        )),
    )
    .into_any()
}
pub fn send_switch(ui: Ui) -> AnyView {
    send_toggle(ui, true)
}
fn send_toggle(ui: Ui, labelled: bool) -> AnyView {
    let mut switch = Switch::new(
        if labelled {
            ui.now_t("bili-send-switch")
        } else {
            String::new()
        },
        false,
    );
    if labelled {
        switch = switch.size(ControlSize::Small);
    }
    let control = widget(switch)
        .bind(move |s: &mut Switch| {
            if labelled {
                s.label = ui.now_t("bili-send-switch");
            }
            let state = ui.bilibili.get();
            s.checked = state.sending;
            s.disabled = !state.can_send() && !state.sending;
        })
        .on_cx(|_, _: &ToggleChanged, cx| cx.dispatch_program_all(Msg::BilibiliSendToggle))
        .into_any();
    widget(
        Stack::row(0.0)
            .width(LengthSpec::Px(if labelled { 156.0 } else { 32.0 }))
            .with_layout(|l| l.flex_shrink = Some(0.0)),
    )
    .children(control)
    .into_any()
}
pub fn send_status(ui: Ui) -> AnyView {
    widget(
        Stack::row(gap::TIGHT)
            .align(AlignSpec::Center)
            .with_layout(|l| l.flex_shrink = Some(0.0)),
    )
    .children((
        widget(
            Button::new(ui.now_t("bili-results"))
                .kind(ButtonKind::Text)
                .size(ControlSize::Small),
        )
        .bind(move |button: &mut Button| {
            let _locale = ui.locale.get();
            let s = ui.bilibili.get();
            button.label = if let Some(d) = s
                .deliveries
                .iter()
                .find(|d| d.state == DeliveryState::Sending)
            {
                format!("{}/{} · {}", d.completed, d.chunks.len(), s.pending())
            } else if s.pending() > 0 {
                s.pending().to_string()
            } else {
                ui.now_t("bili-results")
            };
        })
        .on_cx(|_, _: &Activate, cx| cx.dispatch_program_all(Msg::SettingsPage(Page::Bilibili))),
        send_switch(ui),
    ))
    .into_any()
}

/// Keep delivery history and the real sync switch reachable in a single footer.
pub(super) fn input_send_status(ui: Ui) -> AnyView {
    widget(Stack::row(0.0))
        .children((
            widget(Stack::row(0.0))
                .visible(move || ui.input_width.get() >= 1000.0)
                .children(send_status(ui)),
            widget(Stack::row(gap::TIGHT).align(AlignSpec::Center))
                .visible(move || ui.input_width.get() < 1000.0)
                .children((
                    widget(super::input::footer_icon(
                        ti::HISTORY,
                        ui.now_t("bili-results"),
                    ))
                    .on_cx(|_, _: &Activate, cx| {
                        cx.dispatch_program_all(Msg::SettingsPage(Page::Bilibili))
                    }),
                    widget(
                        Popover::new()
                            .trigger(ui.now_t("bili-send-switch"))
                            .placement(nana_ui::PopoverPlacement::Top)
                            .width(230.0)
                            .bare_trigger(true),
                    )
                    .trigger(
                        widget(
                            Stack::row(0.0)
                                .width(LengthSpec::Px(8.0))
                                .height(LengthSpec::Px(14.0))
                                .align(AlignSpec::Center)
                                .justify(nana_ui_core::JustifySpec::Center),
                        )
                        .children(
                            widget(IconGlyph::new(ti::SEND).size(icon::SM).decorative()).bind(
                                move |glyph: &mut IconGlyph| {
                                    glyph.style.foreground = Some(if ui.bilibili.get().sending {
                                        SemanticColorRole::Accent
                                    } else {
                                        SemanticColorRole::Muted
                                    });
                                },
                            ),
                        ),
                    )
                    .children(
                        widget(Stack::column(0.0).padding(inset::PANEL)).children(send_switch(ui)),
                    ),
                )),
        ))
        .into_any()
}
pub fn confirm_dialog(ui: Ui) -> AnyView {
    widget(
        ConfirmDialog::new(ui.now_t("bili-send-title"), ui.now_t("bili-send-warning"))
            .confirm_label(ui.now_t("bili-enable"))
            .cancel_label(ui.now_t("common-cancel")),
    )
    .bind(move |dialog: &mut ConfirmDialog| {
        let state = ui.bilibili.get();
        let identity = if let AccountState::Ready(p) = &state.account {
            p.name.clone()
        } else {
            String::new()
        };
        let room = if let ConnectionState::Connected(r) = &state.connection {
            r.id
        } else {
            0
        };
        dialog.message = format!("{identity} · {room}\n\n{}", ui.now_t("bili-send-warning")).into();
    })
    .open(move || ui.bilibili.get().confirm)
    .on_cx(|_, intent: &ConfirmIntent, cx| {
        cx.dispatch_program_all(Msg::BilibiliSendConfirm(matches!(
            intent,
            ConfirmIntent::Confirm { .. }
        )))
    })
    .on_cx(|_, _: &DialogCloseRequested, cx| {
        cx.dispatch_program_all(Msg::BilibiliSendConfirm(false))
    })
    .into_any()
}

pub fn identity_hint(ui: Ui) -> AnyView {
    widget(Stack::column(gap::CONTROL))
        .visible(move || {
            ui.settings.with(|s| s.integrations.bilibili.sync_profile)
                && matches!(ui.bilibili.get().account, AccountState::Ready(_))
        })
        .children((
            hint(ui.now_t("bili-identity-hint")),
            button(ui, "bili-manage", || Msg::SettingsPage(Page::Bilibili)),
        ))
        .into_any()
}
fn account_card(ui: Ui) -> AnyView {
    dynamic(
        move || (ui.bilibili.get().account.clone(), ui.locale.get()),
        move |(account, _)| {
            let mut body = vec![
                widget(Stack::row(gap::GROUP).align(AlignSpec::Center))
                    .children((
                        avatar(ui, 48.0),
                        widget(Text::new(account_label(ui, account)).style(Type::Title.style())),
                    ))
                    .into_any(),
            ];
            match account {
                AccountState::Ready(p) => {
                    body.push(hint(format!("UID {}", p.uid)));
                    body.push(
                        widget(Stack::row(gap::CONTROL))
                            .children((
                                button(ui, "bili-refresh", || Msg::BilibiliRefresh),
                                button(ui, "bili-logout", || Msg::BilibiliLogout),
                            ))
                            .into_any(),
                    );
                }
                AccountState::Qr { url, status } => {
                    if *status != QrStatus::Expired
                        && let Ok(code) = QrCode::encode(url.as_bytes(), 224.0)
                    {
                        body.push(widget(code.label(ui.now_t("bili-login"))).into_any());
                    }
                    body.push(
                        widget(Stack::row(gap::CONTROL))
                            .children((
                                button(ui, "bili-qr-refresh", || Msg::BilibiliLogin),
                                button(ui, "common-cancel", || Msg::BilibiliCancelLogin),
                            ))
                            .into_any(),
                    );
                }
                AccountState::Restoring => {}
                AccountState::Failed(message) => {
                    body.push(hint(message.clone()));
                    body.push(button(ui, "bili-login", || Msg::BilibiliLogin));
                    body.push(button(ui, "bili-refresh", || Msg::BilibiliRefresh));
                    body.push(button(ui, "bili-logout", || Msg::BilibiliLogout));
                }
                _ => body.push(button(ui, "bili-login", || Msg::BilibiliLogin)),
            }
            widget(Stack::column(gap::GROUP).padding(inset::CARD))
                .children(body)
                .into_any()
        },
    )
    .into_any()
}
fn room_edit(ui: Ui, value: String) -> Msg {
    let value = value.trim().to_owned();
    if !value.is_empty() && !value.parse::<i64>().is_ok_and(|id| id > 0) {
        return Msg::Notice {
            text: ui.now_t("bili-invalid-room"),
            error: true,
        };
    }
    Msg::Edit(Box::new(move |settings| {
        settings.integrations.bilibili.room = value;
        settings.integrations.bilibili.enabled = true;
    }))
}
fn room_control(ui: Ui) -> AnyView {
    let pending = signal(
        ui.settings
            .get_untracked()
            .integrations
            .bilibili
            .room
            .clone(),
    );
    let applied = signal(pending.get_untracked());
    widget(
        Stack::row(gap::CONTROL)
            .width(LengthSpec::Px(360.0))
            .align(AlignSpec::Center)
            .wrap(true),
    )
    .children((
        widget(Stack::row(0.0).width(LengthSpec::Px(180.0))).children(
            widget(
                TextInput::new(pending.get_untracked())
                    .placeholder(ui.now_t("bili-room-placeholder"))
                    .size(ControlSize::Small),
            )
            .bind(move |input: &mut TextInput| {
                let room = ui.settings.with(|s| s.integrations.bilibili.room.clone());
                if room != applied.get_untracked() {
                    applied.set(room.clone());
                    pending.set(room.clone());
                    input.state = TextInput::new(room).state;
                }
            })
            .on(move |e: &TextChanged| pending.set(e.value.to_string()))
            .on_cx(move |_, _: &TextSubmitted, cx| {
                cx.dispatch_program_all(room_edit(ui, pending.get_untracked()))
            }),
        ),
        widget(Button::new(ui.now_t("bili-connect")).size(ControlSize::Small)).on_cx(
            move |_, _: &Activate, cx| {
                cx.dispatch_program_all(room_edit(ui, pending.get_untracked()))
            },
        ),
        button(ui, "bili-own-room", || Msg::BilibiliOwnRoom),
    ))
    .into_any()
}
fn deliveries(ui: Ui) -> AnyView {
    dynamic(
        move || (ui.bilibili.get().deliveries.clone(), ui.locale.get()),
        move |(deliveries, _)| {
            let rows: Vec<AnyView> = deliveries
                .iter()
                .filter(|d| d.blocked)
                .chain(deliveries.iter().rev().filter(|d| !d.blocked).take(20))
                .map(|d| {
                    let line = d.line;
                    let remaining = d.remaining();
                    let label = ui.now_t(match d.state {
                        DeliveryState::Queued => "bili-queued",
                        DeliveryState::Sending => "bili-sending",
                        DeliveryState::Submitted => "bili-submitted",
                        DeliveryState::Failed => "bili-failed",
                        DeliveryState::Uncertain => "bili-uncertain",
                        DeliveryState::Cancelled => "bili-cancelled",
                    });
                    let mut row = vec![
                        widget(Text::new(d.chunks.concat()).style(Type::Chat.style())).into_any(),
                        hint(format!("{label} · {}/{}", d.completed, d.chunks.len())),
                    ];
                    if !d.detail.is_empty() {
                        row.push(hint(d.detail.clone()));
                    }
                    if d.completed > 0 && !d.dedup_confirmed() {
                        row.push(hint(ui.now_t("bili-dedup-unconfirmed")));
                    }
                    if matches!(d.state, DeliveryState::Failed | DeliveryState::Uncertain) {
                        row.push(button(ui, "bili-copy-remaining", move || {
                            Msg::CopyText(remaining.clone())
                        }));
                        if d.state == DeliveryState::Failed && d.blocked {
                            row.push(button(ui, "bili-retry", move || Msg::BilibiliRetry(line)));
                        }
                        if d.blocked {
                            row.push(button(ui, "bili-skip-failed", move || {
                                Msg::BilibiliSkipFailed(line)
                            }));
                        }
                    }
                    widget(
                        Stack::column(gap::CONTROL)
                            .padding(inset::PANEL)
                            .surface(SemanticColorRole::Subtle)
                            .radius(nana_ui_core::RadiusTier::Sm),
                    )
                    .children(row)
                    .into_any()
                })
                .collect();
            widget(Stack::column(gap::CONTROL))
                .children(rows)
                .into_any()
        },
    )
    .into_any()
}
pub fn settings_page(ui: Ui) -> AnyView {
    widget(Stack::column(gap::SECTION))
        .children((
            section(
                ui,
                "bili-account",
                vec![
                    account_card(ui),
                    row(
                        ui,
                        "bili-sync-profile",
                        Some("bili-sync-profile-hint"),
                        switch(
                            ui,
                            |s| s.integrations.bilibili.sync_profile,
                            |s, v| s.integrations.bilibili.sync_profile = v,
                        ),
                    ),
                ],
            ),
            section(
                ui,
                "bili-room",
                vec![
                    row(ui, "bili-room", Some("bili-room-hint"), room_control(ui)),
                    row(
                        ui,
                        "bili-receive",
                        None,
                        switch(
                            ui,
                            |s| s.integrations.bilibili.enabled,
                            |s, v| s.integrations.bilibili.enabled = v,
                        ),
                    ),
                    super::settings::controls::bilibili_badge(ui),
                ],
            ),
            section(
                ui,
                "bili-send-section",
                vec![
                    row(
                        ui,
                        "bili-send-switch",
                        Some("bili-send-hint"),
                        send_toggle(ui, false),
                    ),
                    deliveries(ui),
                ],
            ),
            section(
                ui,
                "bili-panel",
                vec![
                    row(
                        ui,
                        "chat-panel-style",
                        None,
                        super::settings::chat_panel_style(ui),
                    ),
                    button(ui, "int-bilibili-open-panel", || {
                        Msg::ShowWindow(crate::windows::WindowKind::Chat)
                    }),
                    row(
                        ui,
                        "chat-filter-text",
                        None,
                        switch(
                            ui,
                            |s| s.integrations.bilibili.panel.text,
                            |s, v| s.integrations.bilibili.panel.text = v,
                        ),
                    ),
                    row(
                        ui,
                        "chat-filter-super-chat",
                        None,
                        switch(
                            ui,
                            |s| s.integrations.bilibili.panel.super_chat,
                            |s, v| s.integrations.bilibili.panel.super_chat = v,
                        ),
                    ),
                    row(
                        ui,
                        "chat-filter-gift",
                        None,
                        switch(
                            ui,
                            |s| s.integrations.bilibili.panel.gift,
                            |s, v| s.integrations.bilibili.panel.gift = v,
                        ),
                    ),
                    row(
                        ui,
                        "chat-filter-member",
                        None,
                        switch(
                            ui,
                            |s| s.integrations.bilibili.panel.member,
                            |s, v| s.integrations.bilibili.panel.member = v,
                        ),
                    ),
                    row(
                        ui,
                        "int-bilibili-quote-display",
                        None,
                        switch(
                            ui,
                            |s| s.integrations.bilibili.quote_on_display,
                            |s, v| s.integrations.bilibili.quote_on_display = v,
                        ),
                    ),
                ],
            ),
        ))
        .into_any()
}
