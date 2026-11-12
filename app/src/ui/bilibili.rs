//! The Bilibili page: a chat page's OBS link (blivechat or LAPLACE, told
//! apart on paste) or 彗星号; and the other platforms' page, 彗星号 alone.
use super::{
    Ui,
    settings::controls::{
        bilibili_badge, block, comet_badge, port, primary, row, row_status, section, segmented,
        select_keys, switch, toggled_section,
    },
};
use crate::msg::{BliveMsg, Msg};
use crate::ui::kit::{self, Type, gap};
use nana_ui::ControlSize;
use nana_ui::runtime::view::{AnyView, IntoView, signal, widget};
use nana_ui::runtime::{Stack, Text, TextChanged, TextInput, TextSubmitted};
use nana_ui_core::{AlignSpec, LengthSpec, SemanticColorRole};
use nanatype_settings::{BilibiliLink, BilibiliSource, BliveRoomKind, ChatSource, Settings};

fn source_is(ui: Ui, source: BilibiliSource) -> impl Fn() -> bool + Send + 'static {
    move || {
        ui.settings.with(|s| {
            let b = &s.integrations.bilibili;
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
                row_status(
                    ui,
                    "int-bili",
                    None,
                    bilibili_badge(ui),
                    switch(
                        ui,
                        |s| s.integrations.bilibili.enabled,
                        |s, v| s.integrations.set_chat_source(ChatSource::Bilibili, v),
                    ),
                ),
                |s| s.integrations.bilibili.enabled,
                vec![row(ui, "int-bili-source", None, source_choice(ui))],
            ),
            widget(Stack::column(0.0))
                .visible(source_is(ui, BilibiliSource::Link))
                .children(section(
                    ui,
                    "",
                    vec![
                        row(
                            ui,
                            "int-bili-link",
                            Some("int-bili-link-hint"),
                            link_field(ui),
                        ),
                        // The address is long: on its own line, so the label
                        // keeps its width.
                        block(saved_link(ui)),
                    ],
                )),
            widget(Stack::column(0.0))
                .visible(source_is(ui, BilibiliSource::Comet))
                .children(section(
                    ui,
                    "",
                    vec![row(
                        ui,
                        "int-comet-port",
                        Some("int-comet-bili-hint"),
                        comet_port(ui),
                    )],
                )),
        ))
        .into_any()
}

/// 彗星号's rooms on the other platforms: its switch and port.
pub fn other_platforms_page(ui: Ui) -> AnyView {
    toggled_section(
        ui,
        "",
        row_status(
            ui,
            "int-comet-other",
            Some("int-comet-other-hint"),
            comet_badge(ui),
            switch(
                ui,
                |s| s.integrations.comet.other_platforms,
                |s, v| {
                    s.integrations
                        .set_chat_source(ChatSource::OtherPlatforms, v)
                },
            ),
        ),
        |s| s.integrations.comet.other_platforms,
        vec![row(
            ui,
            "int-comet-port",
            Some("int-comet-port-hint"),
            comet_port(ui),
        )],
    )
}

fn source_choice(ui: Ui) -> AnyView {
    segmented(
        ui,
        select_keys(
            ui,
            &[
                ("link", "int-bili-source-link"),
                ("comet", "int-bili-source-comet"),
            ],
        ),
        |s| match s.integrations.bilibili.source {
            BilibiliSource::Link => "link".into(),
            BilibiliSource::Comet => "comet".into(),
        },
        |s, v| {
            s.integrations.bilibili.source = match v {
                "comet" => BilibiliSource::Comet,
                _ => BilibiliSource::Link,
            }
        },
    )
}

/// The one 彗星号 port both pages share.
fn comet_port(ui: Ui) -> AnyView {
    port(
        ui,
        |s| s.integrations.comet.port,
        |s, v| s.integrations.comet.port = v,
    )
}

/// The chat page's OBS link, pasted whole. It may carry a 身份码 or a token,
/// so it is hidden while typed and never kept as it is.
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
                    .placeholder(ui.now_t("int-bili-link-placeholder"))
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
        primary(ui, "int-bili-link-save", save),
        kit::tool(
            nana_ui::icons_tabler::BOOK,
            ui.now_t("int-laplace-guide"),
            move || Msg::OpenUrl(laplace_guide(ui.locale.get_untracked())),
        ),
    ))
    .into_any()
}

/// What the saved link connects to, with no 身份码 or token shown.
fn saved_link(ui: Ui) -> AnyView {
    widget(
        Text::new("")
            .style(Type::Caption.mono())
            .color(SemanticColorRole::Muted),
    )
    .bind(move |t: &mut Text| {
        let auth_saved = ui.blivechat.get().auth_saved;
        let token_saved = ui.laplace.get().token_saved;
        let auth = ui.now_t("int-blive-auth-code");
        let v = ui.settings.with(|s| {
            saved_link_text(s, auth_saved.then_some(auth.as_str()), token_saved)
                .unwrap_or_else(|| ui.now_t("int-bili-link-none"))
        });
        if t.value != v {
            t.value = v;
        }
    })
    .into_any()
}

/// `None` while the link in use names nothing to join. `auth` names a saved
/// 身份码.
fn saved_link_text(s: &Settings, auth: Option<&str>, token_saved: bool) -> Option<String> {
    let i = &s.integrations;
    match i.bilibili.link {
        BilibiliLink::Blivechat => {
            let b = &i.blivechat;
            let room = match crate::distribution::blive_room_kind(b) {
                BliveRoomKind::AuthCode => auth?.to_owned(),
                BliveRoomKind::RoomId if !b.room_id.is_empty() => b.room_id.clone(),
                _ => return None,
            };
            Some(format!("blivechat · {}:{} · {room}", b.host, b.port))
        }
        BilibiliLink::Laplace => {
            let address = &i.laplace.url;
            Some(if token_saved {
                let sep = if address.contains('?') { '&' } else { '?' };
                format!("LAPLACE · {address}{sep}token=••••")
            } else {
                format!("LAPLACE · {address}")
            })
        }
    }
}

/// How to set up LAPLACE for NanaType, in the interface's language.
fn laplace_guide(locale: nanatype_i18n::Locale) -> &'static str {
    use nanatype_i18n::Locale;
    match locale {
        Locale::JaJp => "https://github.com/sena-nana/NanaType/blob/main/docs/ja/laplace.md",
        Locale::EnUs => "https://github.com/sena-nana/NanaType/blob/main/docs/en/laplace.md",
        _ => "https://github.com/sena-nana/NanaType/blob/main/docs/laplace.md",
    }
}
