//! YouTube live chat: the stream to follow and its connection.
use super::Ui;
use super::danmaku::Link;
use super::settings::controls::{button, row, row_live_hint, section, switch, text};
use crate::msg::Msg;
use nana_ui::runtime::view::AnyView;
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
    section(
        ui,
        "",
        vec![
            row(
                ui,
                "yt-target",
                None,
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
            row_live_hint(
                ui,
                "yt-enabled",
                move || Some(connection(ui)),
                crate::ui::kit::hstack((
                    button(ui, "yt-connect", || Msg::YoutubeRefresh),
                    switch(
                        ui,
                        |s| s.integrations.youtube.enabled,
                        |s, v| s.integrations.youtube.enabled = v,
                    ),
                )),
            ),
        ],
    )
}
