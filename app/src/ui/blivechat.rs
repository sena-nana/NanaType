//! The third-party chat page: a blivechat server, or 彗星号 through the relay.
use super::{
    Ui,
    settings::controls::{
        address, blivechat_badge, block, button, confirm, edit, primary, row, section, segmented,
        select_keys, switch, toggled_section,
    },
};
use crate::msg::Msg;
use crate::ui::kit::{self, Type, gap};
use nana_ui::ControlSize;
use nana_ui::runtime::view::{AnyView, IntoView, dynamic, signal, widget};
use nana_ui::runtime::{Activate, Button, Stack, Text, TextChanged, TextInput, TextSubmitted};
use nana_ui_core::{AlignSpec, LengthSpec, SemanticColorRole};
use nanatype_settings::{BliveRoomKind, BlivechatSource};

fn source_is(ui: Ui, source: BlivechatSource) -> impl Fn() -> bool + Send + 'static {
    move || {
        ui.settings.with(|s| {
            let b = &s.integrations.blivechat;
            b.enabled && b.source == source
        })
    }
}

pub fn settings_page(ui: Ui) -> AnyView {
    widget(Stack::column(0.0).gap_step(gap::step::SECTION))
        .children((
            toggled_section(
                ui,
                "",
                row(
                    ui,
                    "int-blive",
                    None,
                    crate::ui::kit::hstack((
                        blivechat_badge(ui),
                        switch(
                            ui,
                            |s| s.integrations.blivechat.enabled,
                            |s, v| s.integrations.blivechat.enabled = v,
                        ),
                    )),
                ),
                |s| s.integrations.blivechat.enabled,
                vec![row(ui, "int-blive-source", None, source_choice(ui))],
            ),
            widget(Stack::column(0.0))
                .visible(source_is(ui, BlivechatSource::Server))
                .children(server_section(ui)),
            widget(Stack::column(0.0))
                .visible(source_is(ui, BlivechatSource::Comet))
                .children(comet_section(ui)),
        ))
        .into_any()
}

fn source_choice(ui: Ui) -> AnyView {
    segmented(
        ui,
        select_keys(
            ui,
            &[
                ("server", "int-blive-source-server"),
                ("comet", "int-blive-source-comet"),
            ],
        ),
        |s| match s.integrations.blivechat.source {
            BlivechatSource::Server => "server".into(),
            BlivechatSource::Comet => "comet".into(),
        },
        |s, v| {
            s.integrations.blivechat.source = match v {
                "comet" => BlivechatSource::Comet,
                _ => BlivechatSource::Server,
            }
        },
    )
}

fn server_section(ui: Ui) -> AnyView {
    let mut rows = Vec::new();
    // A room number has blivechat read Bilibili's web danmaku; a build without
    // the unofficial sources offers only the open platform's auth code.
    if crate::distribution::UNOFFICIAL_CHAT {
        rows.push(row(
            ui,
            "int-blive-room-kind",
            None,
            segmented(
                ui,
                select_keys(
                    ui,
                    &[
                        ("auth_code", "int-blive-auth-code"),
                        ("room_id", "int-blive-room-id"),
                    ],
                ),
                |s| match s.integrations.blivechat.room_kind {
                    BliveRoomKind::AuthCode => "auth_code".into(),
                    BliveRoomKind::RoomId => "room_id".into(),
                },
                |s, v| {
                    s.integrations.blivechat.room_kind = match v {
                        "room_id" => BliveRoomKind::RoomId,
                        _ => BliveRoomKind::AuthCode,
                    }
                },
            ),
        ));
    }
    rows.extend([
        dynamic(
            move || {
                ui.settings
                    .with(|s| crate::distribution::blive_room_kind(&s.integrations.blivechat))
            },
            move |kind| match kind {
                BliveRoomKind::AuthCode => row(ui, "int-blive-auth-code", None, auth_field(ui)),
                BliveRoomKind::RoomId => row(ui, "int-blive-room-id", None, room_field(ui)),
            },
        )
        .into_any(),
        row(
            ui,
            "int-blive-host",
            None,
            address(
                ui,
                (
                    |s| s.integrations.blivechat.host.clone(),
                    |s, v| s.integrations.blivechat.host = v,
                ),
                (
                    |s| s.integrations.blivechat.port,
                    |s, v| s.integrations.blivechat.port = v,
                ),
            ),
        ),
    ]);
    section(ui, "int-blive-section", rows)
}

/// The room number: typed freely, applied on Enter or with the button so a
/// half-typed number does not connect.
fn room_field(ui: Ui) -> AnyView {
    let draft = signal(
        ui.settings
            .with_untracked(|s| s.integrations.blivechat.room_id.clone()),
    );
    let apply = move || {
        let room = draft.get_untracked().trim().to_owned();
        edit(move |s| s.integrations.blivechat.room_id = room)
    };
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::TIGHT)
            .align(AlignSpec::Center),
    )
    .children((
        widget(Stack::row(0.0).width(LengthSpec::Px(crate::ui::kit::field::SM))).children(
            widget(
                kit::text_input(draft.get_untracked())
                    .placeholder(ui.now_t("int-blive-room-placeholder"))
                    .size(ControlSize::Small),
            )
            .on(move |e: &TextChanged| draft.set(e.value.to_string()))
            .on_cx(move |_, _: &TextSubmitted, cx| cx.dispatch_program_all(apply())),
        ),
        primary(ui, "int-blive-connect", apply),
    ))
    .into_any()
}

/// The 身份码 goes to the system's secret store, never to the settings file;
/// once saved it shows as saved, with a way to forget it.
fn auth_field(ui: Ui) -> AnyView {
    let draft = signal(String::new());
    let save = move || {
        let code = draft.get_untracked().trim().to_owned();
        draft.set(String::new());
        Msg::BlivechatAuthCode(code)
    };
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::TIGHT)
            .align(AlignSpec::Center)
            .wrap(true),
    )
    .children((
        widget(Stack::row(0.0).width(LengthSpec::Px(crate::ui::kit::field::SM))).children(
            widget(
                kit::text_input("")
                    .secure(true)
                    .placeholder(ui.now_t("int-blive-auth-placeholder"))
                    .size(ControlSize::Small),
            )
            .bind(move |input: &mut TextInput| {
                let text = draft.get();
                if input.state.value.as_ref() != text.as_str() {
                    input.state.value = text.into();
                }
            })
            .on(move |e: &TextChanged| draft.set(e.value.chars().take(256).collect()))
            .on_cx(move |_, _: &TextSubmitted, cx| cx.dispatch_program_all(save())),
        ),
        primary(ui, "int-blive-auth-save", save),
        widget(
            Stack::row(0.0)
                .gap_step(gap::step::TIGHT)
                .align(AlignSpec::Center),
        )
        .visible(move || ui.blivechat.get().auth_saved)
        .children((
            widget(
                Text::new(ui.now_t("int-blive-auth-saved"))
                    .style(Type::Small.style())
                    .color(SemanticColorRole::Muted),
            ),
            button(ui, "int-blive-auth-clear", || {
                Msg::BlivechatAuthCode(String::new())
            }),
        )),
    ))
    .into_any()
}

fn comet_section(ui: Ui) -> AnyView {
    let address = widget(
        Text::new("")
            .style(Type::Caption.mono())
            .color(SemanticColorRole::Text),
    )
    .bind(move |t: &mut Text| {
        let v = ui
            .settings
            .with(|s| crate::app::blivechat::comet_address(&s.integrations.blivechat));
        if t.value != v {
            t.value = v;
        }
    });
    let reset = confirm(
        *ui,
        ui.now_t("int-comet-reset-question"),
        ui.now_t("int-comet-reset"),
        |armed| {
            widget(Button::new(ui.now_t("int-comet-reset")).size(ControlSize::Small))
                .on(move |_: &Activate| armed.set(true))
                .into_any()
        },
        || Msg::CometResetToken,
    );
    section(
        ui,
        "int-comet-section",
        vec![
            row(
                ui,
                "int-comet-address",
                Some("int-comet-address-hint"),
                widget(
                    Stack::row(0.0)
                        .gap_step(gap::step::TIGHT)
                        .align(AlignSpec::Center),
                )
                .children((
                    button(ui, "int-comet-copy", move || {
                        Msg::CopyText(ui.settings.with_untracked(|s| {
                            crate::app::blivechat::comet_address(&s.integrations.blivechat)
                        }))
                    }),
                    reset,
                ))
                .into_any(),
            ),
            // The address is long: on its own line, so the label keeps its width.
            block(address),
        ],
    )
}
