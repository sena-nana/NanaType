//! What a link's state says, in the viewer's language.
//!
//! The VTube Studio, OSC and WebSocket clients, Bilibili and the Spout2
//! output report typed states ([`LinkFault`], [`BridgeStatus`],
//! [`ConnectionState`], [`WindowOutputStatus`]); these turn them into the text
//! the settings pages and the chat panel show. The app and the snapshots
//! both go through here, so what a screenshot shows is what users read.

use nana_ui::WindowOutputStatus;
use nanatype_bilibili::ConnectionState;
use nanatype_blive::{BliveStatus, FatalKind, RelayStatus};
use nanatype_bridge::{BridgeStatus, Sink};
use nanatype_i18n::Locale;
use nanatype_model::LinkFault;
use nanatype_settings::{BliveRoomKind, BlivechatSettings, BlivechatSource};

use super::danmaku::Link;
use super::{I18N, LinkState};

/// Why a link dropped, as a short phrase.
pub fn fault(locale: Locale, f: &LinkFault) -> String {
    let t = |key: &str| I18N.t_in(locale, key);
    match f {
        LinkFault::Refused => t("link-fault-refused"),
        LinkFault::TimedOut => t("link-fault-timeout"),
        LinkFault::Closed => t("link-fault-closed"),
        LinkFault::PortInUse(port) => I18N.tr_in(
            locale,
            "link-fault-port-in-use",
            &[("port", port.to_string().into())],
        ),
        LinkFault::Unresolved(host) => I18N.tr_in(
            locale,
            "link-fault-unresolved",
            &[("host", host.as_str().into())],
        ),
        LinkFault::Silent => t("link-fault-silent"),
        LinkFault::Remote(m) if m.trim().is_empty() => t("link-fault-remote-empty"),
        LinkFault::Remote(m) => I18N.tr_in(
            locale,
            "link-fault-remote",
            &[("message", m.as_str().into())],
        ),
        // The system's own words (Windows already writes them in its
        // display language).
        LinkFault::Other(m) => m.clone(),
    }
}

/// The state a bridge report puts its sink in; `None` for reports that
/// carry data rather than a state (a token, the hotkey list).
pub fn bridge(locale: Locale, status: &BridgeStatus) -> Option<(Sink, LinkState)> {
    Some(match status {
        BridgeStatus::Off(sink) => (*sink, LinkState::Off),
        BridgeStatus::Connecting(sink) => (*sink, LinkState::Connecting),
        BridgeStatus::Connected(sink) => (
            *sink,
            LinkState::Connected(I18N.t_in(locale, "int-status-connected")),
        ),
        BridgeStatus::Clients(n) => (
            Sink::WebSocket,
            LinkState::Connected(I18N.tr_in(locale, "int-ws-clients", &[("count", (*n).into())])),
        ),
        BridgeStatus::VtsNeedsToken | BridgeStatus::VtsTokenRejected => {
            (Sink::Vts, LinkState::NeedsAuth)
        }
        BridgeStatus::Error(sink, f) => (
            *sink,
            LinkState::Error(I18N.tr_in(
                locale,
                "int-status-error",
                &[("message", fault(locale, f).into())],
            )),
        ),
        BridgeStatus::VtsToken(_) | BridgeStatus::VtsHotkeys(_) => return None,
    })
}

/// Bilibili's status line and the chat panel's dot for `status` in `room`.
pub fn bilibili(locale: Locale, status: &ConnectionState, _room: &str) -> (String, Link) {
    match status {
        ConnectionState::Off => (I18N.t_in(locale, "int-status-off"), Link::Off),
        ConnectionState::NeedsLogin => (I18N.t_in(locale, "bili-login-needed"), Link::Off),
        ConnectionState::Connecting => {
            (I18N.t_in(locale, "int-status-connecting"), Link::Connecting)
        }
        ConnectionState::Connected(room) => (format!("Bilibili · {}", room.id), Link::Connected),
        ConnectionState::Error(message) => (
            I18N.tr_in(
                locale,
                "chat-status-retrying",
                &[(
                    "message",
                    if message.is_empty() {
                        fault(locale, &LinkFault::Closed).into()
                    } else {
                        message.clone().into()
                    },
                )],
            ),
            Link::Error,
        ),
    }
}

/// blivechat's status line and the chat panel's dot for `status` in `room`.
fn blive(locale: Locale, status: &BliveStatus, room: &str) -> (String, Link) {
    match status {
        BliveStatus::Off => (I18N.t_in(locale, "int-status-off"), Link::Off),
        BliveStatus::Connecting => (I18N.t_in(locale, "int-status-connecting"), Link::Connecting),
        BliveStatus::Connected => (
            I18N.tr_in(locale, "chat-status-joined", &[("room", room.into())]),
            Link::Connected,
        ),
        BliveStatus::Error(f) => (
            I18N.tr_in(
                locale,
                "chat-status-retrying",
                &[("message", fault(locale, f).into())],
            ),
            Link::Error,
        ),
        BliveStatus::Fatal(kind, message) => {
            let why = match kind {
                FatalKind::AuthCode => I18N.t_in(locale, "chat-fatal-auth-code"),
                FatalKind::TooManyRetries => I18N.t_in(locale, "chat-fatal-too-many-retries"),
                FatalKind::TooManyConnections => {
                    I18N.t_in(locale, "chat-fatal-too-many-connections")
                }
                // blivechat's own words.
                FatalKind::Other => message.clone(),
            };
            (
                I18N.tr_in(locale, "chat-fatal", &[("message", why.into())]),
                Link::Error,
            )
        }
    }
}

/// Third-party chat's status line and dot: a blivechat server's join, or
/// for 彗星号 the relay and how many plugin pages push to it.
pub fn blivechat(
    locale: Locale,
    settings: &BlivechatSettings,
    state: &crate::app::blivechat::State,
) -> (String, Link) {
    if !settings.enabled {
        return (I18N.t_in(locale, "int-status-off"), Link::Off);
    }
    match settings.source {
        BlivechatSource::Server => {
            if state.status == BliveStatus::Off {
                return (I18N.t_in(locale, "chat-status-no-room"), Link::Off);
            }
            // Never show the auth code itself.
            let room = match settings.room_kind {
                BliveRoomKind::RoomId => settings.room_id.clone(),
                BliveRoomKind::AuthCode => I18N.t_in(locale, "int-blive-auth-code"),
            };
            blive(locale, &state.status, &room)
        }
        BlivechatSource::Comet => match &state.relay {
            RelayStatus::Error(f) => (
                I18N.tr_in(
                    locale,
                    "int-comet-relay-error",
                    &[("message", fault(locale, f).into())],
                ),
                Link::Error,
            ),
            RelayStatus::Off => (I18N.t_in(locale, "int-status-connecting"), Link::Connecting),
            RelayStatus::Listening { publishers, .. } => match &state.status {
                BliveStatus::Connected if *publishers == 0 => {
                    (I18N.t_in(locale, "int-comet-waiting"), Link::Connecting)
                }
                BliveStatus::Connected => {
                    (I18N.t_in(locale, "int-comet-connected"), Link::Connected)
                }
                other => blive(locale, other, ""),
            },
        },
    }
}

/// A Spout2 output's state after NanaUI reports `status`; `None` leaves it
/// as it was. NanaUI's reasons are English diagnostics: the host logs them
/// (`NANATYPE_DEBUG`), the page says what they mean.
pub fn spout(locale: Locale, status: &WindowOutputStatus) -> Option<LinkState> {
    let unavailable = |key: &str| {
        LinkState::Error(I18N.tr_in(
            locale,
            "output-status-unavailable",
            &[("reason", I18N.t_in(locale, key).into())],
        ))
    };
    Some(match status {
        WindowOutputStatus::Active { extent, .. } => LinkState::Connected(format!(
            "{} · {}×{}",
            I18N.t_in(locale, "output-status-active"),
            extent[0],
            extent[1]
        )),
        WindowOutputStatus::NativeUnavailable { .. } => {
            unavailable("output-status-native-unavailable")
        }
        WindowOutputStatus::Failed { .. } => unavailable("output-status-failed"),
        WindowOutputStatus::Stopped => LinkState::Off,
        _ => return None,
    })
}

/// A Spout sender's failure (`key` from [`crate::output::Spout::frame`]).
pub fn spout_send(locale: Locale, key: &str) -> LinkState {
    let wrapper = if crate::output::SYPHON_ERROR_KEYS.contains(&key) {
        "err-syphon"
    } else {
        "err-spout"
    };
    LinkState::Error(I18N.tr_in(
        locale,
        wrapper,
        &[("message", I18N.t_in(locale, key).into())],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every fault, report and output state, in every language: none of
    /// them shows a key or (outside zh/ja's own words) the English source.
    #[test]
    fn every_state_reads_in_every_language() {
        let faults = [
            LinkFault::Refused,
            LinkFault::TimedOut,
            LinkFault::Closed,
            LinkFault::PortInUse(8765),
            LinkFault::Unresolved("example.invalid".into()),
            LinkFault::Silent,
            LinkFault::Remote(String::new()),
            LinkFault::Remote("Plugin not allowed".into()),
        ];
        for locale in [Locale::ZhCn, Locale::EnUs, Locale::JaJp] {
            let mut texts = Vec::new();
            for f in &faults {
                texts.push(fault(locale, f));
                let (_, state) =
                    bridge(locale, &BridgeStatus::Error(Sink::Osc, f.clone())).unwrap();
                let LinkState::Error(t) = state else {
                    panic!("an error state")
                };
                texts.push(t);
            }

            for state in [
                ConnectionState::Off,
                ConnectionState::NeedsLogin,
                ConnectionState::Connecting,
                ConnectionState::Connected(nanatype_bilibili::Room {
                    id: 1,
                    owner: 1,
                    title: String::new(),
                    max_chars: 20,
                }),
                ConnectionState::Error(String::new()),
            ] {
                texts.push(bilibili(locale, &state, "").0);
            }
            let fatals = [
                FatalKind::AuthCode,
                FatalKind::TooManyRetries,
                FatalKind::TooManyConnections,
            ];
            for status in [
                BliveStatus::Off,
                BliveStatus::Connecting,
                BliveStatus::Connected,
            ]
            .into_iter()
            .chain(faults.iter().cloned().map(BliveStatus::Error))
            .chain(fatals.map(|k| BliveStatus::Fatal(k, String::new())))
            {
                texts.push(blive(locale, &status, "1").0);
            }
            use BlivechatSource::{Comet, Server};
            let listening = |publishers| RelayStatus::Listening { publishers };
            for (source, relay, status) in [
                (Server, RelayStatus::Off, BliveStatus::Off),
                (Server, RelayStatus::Off, BliveStatus::Connected),
                (Comet, RelayStatus::Off, BliveStatus::Off),
                (
                    Comet,
                    RelayStatus::Error(LinkFault::PortInUse(1)),
                    BliveStatus::Off,
                ),
                (Comet, listening(0), BliveStatus::Connected),
                (Comet, listening(2), BliveStatus::Connected),
            ] {
                let settings = BlivechatSettings {
                    enabled: true,
                    source,
                    ..Default::default()
                };
                let state = crate::app::blivechat::State {
                    status,
                    relay,
                    auth_saved: false,
                };
                texts.push(blivechat(locale, &settings, &state).0);
            }
            for status in [
                WindowOutputStatus::Active {
                    extent: [1920, 1080],
                    native: true,
                },
                WindowOutputStatus::NativeUnavailable {
                    reason: "no GPU".into(),
                },
                WindowOutputStatus::Failed {
                    reason: "device lost".into(),
                },
            ] {
                match spout(locale, &status).unwrap() {
                    LinkState::Connected(t) | LinkState::Error(t) => texts.push(t),
                    other => panic!("{other:?}"),
                }
            }
            for key in crate::output::SPOUT_ERROR_KEYS
                .into_iter()
                .chain(crate::output::SYPHON_ERROR_KEYS)
            {
                let LinkState::Error(t) = spout_send(locale, key) else {
                    panic!()
                };
                texts.push(t);
            }
            for t in &texts {
                assert!(!t.is_empty());
                // A missing key reads back as the key itself.
                assert!(!t.contains("link-fault-") && !t.contains("-status-"), "{t}");
                assert!(
                    !t.contains("spout-error-") && !t.contains("chat-fatal-"),
                    "{t}"
                );
                if locale != Locale::EnUs {
                    for english in [
                        "Sending",
                        "Unavailable",
                        "Disconnected",
                        "retrying",
                        "no GPU",
                    ] {
                        assert!(!t.contains(english), "{locale:?}: {t}");
                    }
                }
            }
        }
    }

    #[test]
    fn data_reports_leave_the_state_alone() {
        assert_eq!(
            bridge(Locale::ZhCn, &BridgeStatus::VtsToken("t".into())),
            None
        );
        assert_eq!(
            spout(
                Locale::ZhCn,
                &WindowOutputStatus::NativeRetired { pool_generation: 1 }
            ),
            None
        );
    }
}
