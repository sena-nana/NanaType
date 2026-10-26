use nana_ui::runtime::view::{AnyView, IntoView, dynamic, signal, widget};
use nana_ui::runtime::{Activate, Button, Stack, Text, TextChanged, TextInput};
use nana_ui::{ButtonKind, ControlSize};
use nana_ui_core::{AlignSpec, JustifySpec, LengthSpec, PositionSpec, SemanticColorRole};
use nanatype_p2p::Status;

use super::Ui;
use crate::msg::Msg;
use crate::p2p::{InvitePhase, SessionUi};
use crate::ui::kit::{self, Tone, Type, gap};

/// A form row's button beside its field.
const ACTION_W: f32 = 100.0;
/// The card's header line, and the card folded to it.
const HEADER_H: f32 = 32.0;
const COLLAPSED_H: f32 = 36.0;
/// The unfolded card, so its form does not jump as the status changes.
const EXPANDED_MIN_H: f32 = 150.0;
/// Invitation controls centered in the otherwise empty partner window.
const WAITING_CARD_W: f32 = 300.0;

fn field(stacked: bool) -> Stack {
    Stack::column(0.0)
        .width(LengthSpec::Fill)
        .with_layout(|layout| {
            layout.flex_grow = Some(if stacked { 0.0 } else { 1.0 });
            layout.flex_shrink = Some(1.0);
            layout.min_width = Some(LengthSpec::Px(0.0));
            layout.width = Some(if stacked {
                LengthSpec::Fill
            } else {
                LengthSpec::Px(0.0)
            });
        })
}

fn action(stacked: bool) -> Stack {
    Stack::row(0.0)
        .width(if stacked {
            LengthSpec::Fill
        } else {
            LengthSpec::Px(ACTION_W)
        })
        .with_layout(|layout| {
            layout.flex_shrink = Some(0.0);
        })
}

pub fn card(ui: Ui) -> AnyView {
    let invitation = signal(String::new());
    let password = signal(String::new());
    card_with_fields(ui, invitation, password, false)
}

pub(crate) fn card_with_fields(
    ui: Ui,
    invitation: nana_ui::runtime::view::Signal<String>,
    password: nana_ui::runtime::view::Signal<String>,
    stacked: bool,
) -> AnyView {
    dynamic(
        move || (ui.peer_panel.get(), ui.locale.get(), ui.peer.get()),
        move |(expanded, _, state)| {
            let expanded = *expanded;
            let form_row = || {
                if stacked {
                    Stack::column(0.0)
                        .gap_step(gap::step::CONTROL)
                        .width(LengthSpec::Fill)
                } else {
                    Stack::row(0.0)
                        .gap_step(gap::step::TIGHT)
                        .width(LengthSpec::Fill)
                }
            };
            let status = match &state.status {
                Status::Starting | Status::Waiting if state.host => invite_caption(ui, state),
                Status::Starting => ui.now_t("peer-connecting"),
                Status::Waiting => ui.now_t("peer-waiting"),
                Status::Reconnecting => ui.now_t("peer-reconnecting"),
                Status::Connected { route, rtt_ms } => {
                    let route = ui.now_t(match route.as_str() {
                        "relay" => "peer-relay",
                        "direct" => "peer-direct",
                        _ => "peer-connecting",
                    });
                    if state.members >= 2 {
                        format!(
                            "{}/{} · {} · {route} · {rtt_ms} ms",
                            state.members,
                            nanatype_p2p::MAX_MEMBERS,
                            state.name
                        )
                    } else {
                        format!("{} · {route} · {rtt_ms} ms", state.name)
                    }
                }
                Status::Ended => ui.now_t("peer-off"),
                Status::Error(error) => {
                    let detail = if error == "Room is full" {
                        ui.now_t("peer-room-full")
                    } else {
                        error.clone()
                    };
                    format!("{}: {detail}", ui.now_t("peer-error"))
                }
            };
            let tone = match &state.status {
                Status::Connected { .. } => Tone::Success,
                Status::Starting | Status::Waiting | Status::Reconnecting => Tone::Warning,
                Status::Error(_) => Tone::Danger,
                Status::Ended => Tone::Off,
            };
            let header = widget(
                Stack::row(0.0)
                    .gap_step(gap::step::CONTROL)
                    .width(LengthSpec::Fill)
                    .align(AlignSpec::Center)
                    .height(LengthSpec::Px(HEADER_H)),
            )
            .children((
                state.active.then(|| crate::ui::kit::status_dot(tone)),
                widget(field(false)).children(widget(
                    Text::new(format!("{} · {status}", ui.now_t("peer-title"))).style({
                        let mut style = Type::Label.style();
                        let layout = std::sync::Arc::make_mut(&mut style.layout);
                        layout.white_space_nowrap = true;
                        layout.text_overflow_ellipsis = true;
                        layout.overflow_x = nana_ui_core::OverflowSpec::Hidden;
                        style
                    }),
                )),
                widget(
                    Button::new(ui.now_t(if expanded {
                        "peer-collapse"
                    } else {
                        "peer-expand"
                    }))
                    .size(ControlSize::Small),
                )
                .on_cx(move |_, _: &Activate, cx| {
                    cx.dispatch_program_all(Msg::PeerPanel(!expanded))
                }),
            ))
            .into_any();
            if !expanded {
                return widget(
                    Stack::column(0.0)
                        .padding_step(gap::step::HAIR, gap::step::HAIR)
                        .height(LengthSpec::Px(COLLAPSED_H)),
                )
                .children(header)
                .into_any();
            }
            let body = if state.active {
                let code = state.invitation.clone();
                widget(
                    Stack::row(0.0)
                        .gap_step(gap::step::CONTROL)
                        .with_layout(|l| l.flex_wrap = nana_ui_core::FlexWrap::Wrap),
                )
                .children((
                    (state.host && !code.is_empty()).then(|| {
                        widget(Button::new(ui.now_t("peer-invite")).size(ControlSize::Small)).on_cx(
                            move |_, _: &Activate, cx| {
                                cx.dispatch_program_all(Msg::PeerInviteFriends)
                            },
                        )
                    }),
                    (!code.is_empty()).then(|| {
                        widget(Button::new(ui.now_t("peer-copy")).size(ControlSize::Small)).on_cx(
                            move |_, _: &Activate, cx| {
                                cx.dispatch_program_all(Msg::CopyText(code.clone()))
                            },
                        )
                    }),
                    widget(Button::new(ui.now_t("peer-window-toggle")).size(ControlSize::Small))
                        .on_cx(move |_, _: &Activate, cx| {
                            cx.dispatch_program_all(Msg::ToggleWindow(
                                crate::windows::WindowKind::PeerDisplay,
                            ))
                        }),
                    widget(Button::new(ui.now_t("peer-leave")).size(ControlSize::Small))
                        .on_cx(move |_, _: &Activate, cx| cx.dispatch_program_all(Msg::PeerLeave)),
                ))
                .into_any()
            } else {
                widget(Stack::column(0.0).gap_step(gap::step::CONTROL))
                    .children((
                        widget(form_row()).children((
                            widget(field(stacked)).children(
                                widget(
                                    kit::text_input(invitation.get_untracked())
                                        .placeholder(ui.now_t("peer-invitation"))
                                        .size(ControlSize::Small),
                                )
                                .key("peer-invitation-input")
                                .bind(move |input: &mut TextInput| {
                                    let value = invitation.get();
                                    if input.state.value.as_str() != value {
                                        input.state.replace_value(value);
                                    }
                                })
                                .on(move |e: &TextChanged| invitation.set(e.value.to_string())),
                            ),
                            widget(action(stacked)).children(
                                widget(Button::new(ui.now_t("peer-join")).size(ControlSize::Small))
                                    .on_cx(move |_, _: &Activate, cx| {
                                        cx.dispatch_program_all(Msg::PeerStart {
                                            invitation: Some(invitation.get_untracked()),
                                            password: password.get_untracked(),
                                        })
                                    }),
                            ),
                        )),
                        widget(form_row()).children((
                            widget(field(stacked)).children(
                                widget(
                                    kit::text_input(password.get_untracked())
                                        .secure(true)
                                        .placeholder(ui.now_t("peer-password"))
                                        .size(ControlSize::Small),
                                )
                                .key("peer-password-input")
                                .bind(move |input: &mut TextInput| {
                                    let value = password.get();
                                    if input.state.value.as_str() != value {
                                        input.state.replace_value(value);
                                    }
                                })
                                .on(move |e: &TextChanged| {
                                    password.set(e.value.chars().take(64).collect())
                                }),
                            ),
                            widget(action(stacked)).children(
                                widget(
                                    Button::new(ui.now_t("peer-create"))
                                        .size(ControlSize::Small)
                                        .kind(ButtonKind::Primary),
                                )
                                .on_cx(
                                    move |_, _: &Activate, cx| {
                                        cx.dispatch_program_all(Msg::PeerStart {
                                            invitation: None,
                                            password: password.get_untracked(),
                                        })
                                    },
                                ),
                            ),
                        )),
                    ))
                    .into_any()
            };
            widget(
                Stack::column(0.0)
                    .gap_step(gap::step::CONTROL)
                    .padding(nana_ui::theme::space::SM)
                    .with_layout(|l| {
                        l.min_height = Some(LengthSpec::Px(EXPANDED_MIN_H));
                        l.flex_shrink = Some(0.0);
                    }),
            )
            .children((header, body))
            .into_any()
        },
    )
    .into_any()
}

/// What the host sees while a friend has not connected yet.
pub(crate) fn invite_caption(ui: Ui, state: &SessionUi) -> String {
    match state.invite {
        InvitePhase::Idle => ui.now_t("peer-invite-idle"),
        InvitePhase::Waiting => ui.now_t("peer-invite-waiting"),
        InvitePhase::Accepted if state.invite_names.is_empty() => {
            ui.now_t("peer-invite-accepted-unnamed")
        }
        InvitePhase::Accepted if state.invite_names.len() == 1 => ui.tr(
            "peer-invite-accepted",
            &[("name", state.invite_names[0].as_str().into())],
        ),
        InvitePhase::Accepted => ui.tr(
            "peer-invite-accepted-many",
            &[("n", state.invite_names.len().into())],
        ),
    }
}

/// Invite controls on the partner window, only while that window has no line.
pub(crate) fn waiting_room(ui: Ui) -> AnyView {
    dynamic(
        move || {
            let state = ui.peer.get();
            let empty = ui.peer_seats.get() == 0 && ui.peer_display.with(|view| view.empty);
            (state, empty, ui.locale.get())
        },
        move |(state, empty, _)| {
            let show = state.active && *empty && !matches!(state.status, Status::Connected { .. });
            if !show {
                return widget(Stack::column(0.0)).into_any();
            }
            let host = state.host;
            let caption = match &state.status {
                Status::Error(error) => {
                    let detail = if error == "Room is full" {
                        ui.now_t("peer-room-full")
                    } else {
                        error.clone()
                    };
                    format!("{}: {detail}", ui.now_t("peer-error"))
                }
                Status::Reconnecting => ui.now_t("peer-reconnecting"),
                _ if host => invite_caption(ui, state),
                _ => ui.now_t("peer-connecting"),
            };
            let invitation = state.invitation.clone();
            let card = widget(
                Stack::column(0.0)
                    .gap_step(gap::step::CONTROL)
                    .padding(nana_ui::theme::space::SM)
                    .width(LengthSpec::Px(WAITING_CARD_W))
                    .align(AlignSpec::Center)
                    .surface(SemanticColorRole::Background),
            )
            .children((
                widget(Text::new(caption).style(Type::Label.style())),
                (host && !invitation.is_empty()).then(|| {
                    let code = invitation.clone();
                    widget(
                        Stack::row(0.0)
                            .gap_step(gap::step::CONTROL)
                            .justify(JustifySpec::Center),
                    )
                    .children((
                        widget(
                            Button::new(ui.now_t("peer-invite"))
                                .kind(ButtonKind::Primary)
                                .size(ControlSize::Small),
                        )
                        .on_cx(move |_, _: &Activate, cx| {
                            cx.dispatch_program_all(Msg::PeerInviteFriends)
                        }),
                        widget(Button::new(ui.now_t("peer-copy")).size(ControlSize::Small)).on_cx(
                            move |_, _: &Activate, cx| {
                                cx.dispatch_program_all(Msg::CopyText(code.clone()))
                            },
                        ),
                    ))
                }),
            ));
            widget(
                Stack::column(0.0)
                    .align(AlignSpec::Center)
                    .justify(JustifySpec::Center)
                    .with_layout(|layout| {
                        layout.position = PositionSpec::Absolute;
                        layout.offset_left = Some(LengthSpec::Px(0.0));
                        layout.offset_top = Some(LengthSpec::Px(0.0));
                        layout.offset_right = Some(LengthSpec::Px(0.0));
                        layout.offset_bottom = Some(LengthSpec::Px(0.0));
                    }),
            )
            .children(card)
            .into_any()
        },
    )
    .into_any()
}
