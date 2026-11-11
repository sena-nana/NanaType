//! The blivechat page: a blivechat server or the 彗星号 client.
use super::{
    Ui,
    settings::controls::{
        Row, address, blivechat_badge, edit, port, primary, row, section, segmented, select_keys,
        switch, toggled_section,
    },
};
use crate::msg::{BliveMsg, Msg};
use crate::ui::kit::{self, Type, gap};
use nana_ui::ControlSize;
use nana_ui::runtime::view::{AnyView, IntoView, dynamic, signal, widget};
use nana_ui::runtime::{Stack, Text, TextChanged, TextInput, TextSubmitted};
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
    // The room link blivechat gives for OBS fills everything below at once.
    let mut rows = vec![row(
        ui,
        "int-blive-link",
        Some("int-blive-link-hint"),
        link_field(ui),
    )];
    // Room ids and auth codes are both accepted by blivechat.
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
    rows.extend([
        Row::inset(dynamic(
            move || {
                ui.settings
                    .with(|s| crate::distribution::blive_room_kind(&s.integrations.blivechat))
            },
            move |kind| match kind {
                BliveRoomKind::AuthCode => row(ui, "int-blive-auth-code", None, auth_field(ui)),
                BliveRoomKind::RoomId => row(ui, "int-blive-room-id", None, room_field(ui)),
            },
        )),
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

/// blivechat's room link, pasted whole. It may carry the 身份码, so it is
/// hidden while typed and never kept as it is.
fn link_field(ui: Ui) -> AnyView {
    let draft = signal(String::new());
    let save = move || {
        let link = draft.get_untracked().trim().to_owned();
        draft.set(String::new());
        Msg::Blive(BliveMsg::Link(link))
    };
    widget(
        Stack::row(0.0)
            .gap_step(gap::step::TIGHT)
            .align(AlignSpec::Center),
    )
    .children((
        widget(Stack::row(0.0).width(LengthSpec::Px(crate::ui::kit::field::SM))).children(
            widget(
                kit::text_input("")
                    .secure(true)
                    .placeholder(ui.now_t("int-blive-link-placeholder"))
                    .size(ControlSize::Small),
            )
            .bind(move |input: &mut TextInput| {
                let text = draft.get();
                if input.state.value.as_ref() != text.as_str() {
                    input.state.value = text.into();
                }
            })
            .on(move |e: &TextChanged| draft.set(e.value.chars().take(4096).collect()))
            .on_cx(move |_, _: &TextSubmitted, cx| cx.dispatch_program_all(save())),
        ),
        primary(ui, "int-blive-link-save", save),
    ))
    .into_any()
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
        Msg::Blive(BliveMsg::AuthCode(code))
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
            kit::tool(
                nana_ui::icons_tabler::ERASER,
                ui.now_t("int-blive-auth-clear"),
                || Msg::Blive(BliveMsg::AuthCode(String::new())),
            ),
        )),
    ))
    .into_any()
}

/// The 彗星号 client serves its feed on this computer; only its port is
/// asked, and the default is the client's own.
fn comet_section(ui: Ui) -> AnyView {
    section(
        ui,
        "int-comet-section",
        vec![row(
            ui,
            "int-comet-port",
            Some("int-comet-port-hint"),
            port(
                ui,
                |s| s.integrations.blivechat.comet_port,
                |s, v| s.integrations.blivechat.comet_port = v,
            ),
        )],
    )
}
