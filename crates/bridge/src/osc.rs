use std::net::{ToSocketAddrs, UdpSocket};

use nanatype_model::{LinkFault, VtsAction};
use rosc::{OscMessage, OscPacket, OscType, encoder};

use crate::{BridgeEvent, BridgeStatus, Sink, StatusFn};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OscConfig {
    pub host: String,
    pub port: u16,
    /// Address prefix, e.g. `/nanatype`.
    pub prefix: String,
}

pub(crate) struct OscSender {
    config: OscConfig,
    socket: UdpSocket,
    target: std::net::SocketAddr,
    status: StatusFn,
}

/// OSC address and arguments for an event, or `None` for events OSC skips.
pub(crate) fn message(prefix: &str, event: &BridgeEvent) -> Option<OscMessage> {
    let prefix = prefix.trim_end_matches('/');
    let (path, args) = match event {
        BridgeEvent::LineStart { text, .. } => ("line", vec![OscType::String(text.clone())]),
        BridgeEvent::LineEnd { .. } => ("line_end", vec![]),
        BridgeEvent::Expression { name, intensity } => {
            let mut args = vec![OscType::String(name.clone())];
            if let Some(i) = intensity {
                args.push(OscType::Float(*i));
            }
            ("expression", args)
        }
        BridgeEvent::Portrait { variant } => ("portrait", vec![OscType::String(variant.clone())]),
        BridgeEvent::Speaking { speaking } => {
            ("speaking", vec![OscType::Int(i32::from(*speaking))])
        }
        BridgeEvent::Shake { amp, ms } => (
            "shake",
            vec![OscType::Float(*amp), OscType::Int(*ms as i32)],
        ),
        BridgeEvent::Vts { action } => match action {
            VtsAction::Hotkey { hotkey } => ("vts/hotkey", vec![OscType::String(hotkey.clone())]),
            VtsAction::Expression { file, active, .. } => (
                "vts/expression",
                vec![
                    OscType::String(file.clone()),
                    OscType::Int(i32::from(*active)),
                ],
            ),
            VtsAction::Param { name, value, ms } => (
                "vts/param",
                vec![
                    OscType::String(name.clone()),
                    OscType::Float(*value),
                    OscType::Int(*ms as i32),
                ],
            ),
        },
    };
    Some(OscMessage {
        addr: format!("{prefix}/{path}"),
        args,
    })
}

impl OscSender {
    pub fn start(config: OscConfig, status: StatusFn) -> Option<Self> {
        let target = match (config.host.as_str(), config.port)
            .to_socket_addrs()
            .ok()
            .and_then(|mut a| a.next())
        {
            Some(t) => t,
            None => {
                status(BridgeStatus::Error(
                    Sink::Osc,
                    LinkFault::Unresolved(config.host.clone()),
                ));
                return None;
            }
        };
        let bind = if target.is_ipv6() {
            "[::]:0"
        } else {
            "0.0.0.0:0"
        };
        match UdpSocket::bind(bind) {
            Ok(socket) => {
                status(BridgeStatus::Connected(Sink::Osc));
                Some(Self {
                    config,
                    socket,
                    target,
                    status,
                })
            }
            Err(e) => {
                status(BridgeStatus::Error(Sink::Osc, LinkFault::from_io(&e)));
                None
            }
        }
    }

    pub fn config(&self) -> &OscConfig {
        &self.config
    }

    pub fn emit(&self, event: &BridgeEvent) {
        let Some(msg) = message(&self.config.prefix, event) else {
            return;
        };
        if let Ok(bytes) = encoder::encode(&OscPacket::Message(msg))
            && let Err(e) = self.socket.send_to(&bytes, self.target)
        {
            (self.status)(BridgeStatus::Error(Sink::Osc, LinkFault::from_io(&e)));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn events_map_to_prefixed_addresses() {
        let m = message(
            "/nanatype/",
            &BridgeEvent::Expression {
                name: "smile".into(),
                intensity: Some(0.5),
            },
        )
        .unwrap();
        assert_eq!(m.addr, "/nanatype/expression");
        assert_eq!(m.args.len(), 2);
        let m = message("/x", &BridgeEvent::Speaking { speaking: true }).unwrap();
        assert_eq!(m.args, vec![OscType::Int(1)]);
    }

    #[test]
    fn udp_delivery_round_trips() {
        let receiver = UdpSocket::bind("127.0.0.1:0").unwrap();
        receiver
            .set_read_timeout(Some(std::time::Duration::from_secs(2)))
            .unwrap();
        let port = receiver.local_addr().unwrap().port();
        let sender = OscSender::start(
            OscConfig {
                host: "127.0.0.1".into(),
                port,
                prefix: "/nt".into(),
            },
            std::sync::Arc::new(|_| {}),
        )
        .unwrap();
        sender.emit(&BridgeEvent::Shake { amp: 4.0, ms: 200 });
        let mut buf = [0u8; 256];
        let n = receiver.recv(&mut buf).unwrap();
        let (_, packet) = rosc::decoder::decode_udp(&buf[..n]).unwrap();
        match packet {
            OscPacket::Message(m) => assert_eq!(m.addr, "/nt/shake"),
            other => panic!("{other:?}"),
        }
    }
}
