//! YouTube live chat: the stream to follow and its connection.
use super::Ui;
use super::danmaku::Link;
use super::settings::controls::{block, button, hint, row, section, switch, text};
use crate::msg::Msg;
use crate::ui::kit::gap;
use nana_ui::runtime::view::{AnyView, IntoView, widget};
use nana_ui::runtime::{Stack, Text};
use nanatype_youtube::ConnectionState;

pub fn link(state: &ConnectionState) -> Link {
    match state {
        ConnectionState::Connected { .. } => Link::Connected,
        ConnectionState::Resolving | ConnectionState::Connecting => Link::Connecting,
        ConnectionState::Error(_) => Link::Error,
        _ => Link::Off,
    }
}
pub fn connection(ui: Ui) -> String {
    match &ui.youtube.get().connection {
        ConnectionState::Off => ui.now_t("yt-off"),
        ConnectionState::Resolving => ui.now_t("yt-resolving"),
        ConnectionState::Waiting => ui.now_t("yt-waiting"),
        ConnectionState::Connecting => ui.now_t("yt-connecting"),
        ConnectionState::Connected { video } => format!("{} · {video}", ui.now_t("yt-connected")),
        ConnectionState::Ended => ui.now_t("yt-ended"),
        ConnectionState::Disconnected => ui.now_t("yt-disconnected"),
        ConnectionState::Error(code) => {
            format!("{} · {}", ui.now_t("yt-error"), diagnostic(ui, code))
        }
    }
}
fn diagnostic(ui: Ui, code: &str) -> String {
    ui.now_t(match code {
        "invalid-target" => "yt-invalid-target",
        "protocol-changed" => "yt-protocol-changed",
        "redirected" => "yt-redirected",
        _ => "yt-network-error",
    })
}
pub fn settings_page(ui: Ui) -> AnyView {
    let status = widget(Text::new("")).bind(move |t: &mut Text| t.value = connection(ui));
    widget(Stack::column(0.0).gap_step(gap::step::SECTION))
        .children((
            section(
                ui,
                "yt-stream",
                vec![
                    row(
                        ui,
                        "yt-target",
                        Some("yt-target-hint"),
                        text(
                            ui,
                            ui.now_t("yt-target-placeholder"),
                            crate::ui::kit::field::MD,
                            |s| s.integrations.youtube.target.clone(),
                            |s, v| {
                                s.integrations.youtube.enabled = false;
                                s.integrations.youtube.target = v;
                            },
                        ),
                    ),
                    row(
                        ui,
                        "yt-enabled",
                        None,
                        switch(
                            ui,
                            |s| s.integrations.youtube.enabled,
                            |s, v| s.integrations.youtube.enabled = v,
                        ),
                    ),
                    block(status),
                    block(
                        widget(Stack::row(0.0).gap_step(gap::step::CONTROL)).children((
                            button(ui, "yt-connect", || Msg::YoutubeRefresh),
                            button(ui, "yt-open-panel", || {
                                Msg::ShowWindow(crate::windows::WindowKind::Chat)
                            }),
                        )),
                    ),
                ],
            ),
            section(
                ui,
                "yt-send-section",
                vec![block(hint(ui.now_t("yt-send-unavailable")))],
            ),
        ))
        .into_any()
}
