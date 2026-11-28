//! NanaLive's control API (`ws://127.0.0.1:8312`, subprotocol
//! `nanalive-control-v2`, MessagePack binary frames), as NanaLiveSDK
//! describes it.
//!
//! The client itself is [`crate::avatar`]; this is NanaLive's side of it.
//! The token is asked for with a plugin identity and the scopes it needs;
//! the user approves it on NanaLive's plugin page. Hotkeys come from
//! NanaLive's hotkey catalog (the model's own hotkeys and the starred
//! performance items), expressions are turned on and off by file, and lip
//! sync and `[vts param=...]` write the model's Live2D parameters.

use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use nanatype_model::LinkFault;
use serde_json::{Map, Value, json};
use tungstenite::client::IntoClientRequest;
use tungstenite::http::HeaderValue;
use tungstenite::{Message, WebSocket};

use crate::Sink;
use crate::avatar::{
    ACTIVE_READ, AvatarHotkey, Call, Hold, PLUGIN_DEVELOPER, PLUGIN_NAME, Protocol, Reply,
};

pub const DEFAULT_PORT: u16 = 8312;
const API_NAME: &str = "NanaLiveControlAPI";
const API_VERSION: &str = "2.0";
const SUBPROTOCOL: &str = "nanalive-control-v2";
/// The plugin's reverse-domain id on NanaLive's plugin page.
const PLUGIN_ID: &str = "com.github.sena-nana.nanatype";
/// Only what the markers and lip sync use: hotkeys, expressions, parameter
/// writes, and model events to follow hotkey changes.
const SCOPES: &[&str] = &[
    "hotkey.read",
    "hotkey.trigger",
    "expression.trigger",
    "parameter.write",
    "model.read",
];
/// The stored token is no good: forget it and ask again.
const TOKEN_REFUSED: &[&str] = &["AUTHENTICATION_TOKEN_INVALID", "AUTHENTICATION_REVOKED"];

pub(crate) struct NanaLive;

/// A request envelope.
pub(crate) fn envelope(kind: &str, id: &str, data: Value) -> Value {
    json!({
        "apiName": API_NAME,
        "apiVersion": API_VERSION,
        "requestID": id,
        "messageType": kind,
        "data": data,
    })
}

fn encode(value: &Value) -> Vec<u8> {
    rmp_serde::to_vec(value).unwrap_or_default()
}

impl Protocol for NanaLive {
    const SINK: Sink = Sink::NanaLive;
    const THREAD: &'static str = "nanatype-nanalive";

    fn connect(port: u16) -> Result<WebSocket<TcpStream>, LinkFault> {
        let io = |e: std::io::Error| LinkFault::from_io(&e);
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).map_err(io)?;
        let timeout = Some(Duration::from_secs(2));
        stream.set_read_timeout(timeout).map_err(io)?;
        stream.set_write_timeout(timeout).map_err(io)?;
        let mut request = format!("ws://{addr}")
            .into_client_request()
            .map_err(|e| LinkFault::from_ws(&e))?;
        // NanaLive turns down connections without its subprotocol.
        request.headers_mut().insert(
            "Sec-WebSocket-Protocol",
            HeaderValue::from_static(SUBPROTOCOL),
        );
        let (socket, _) =
            tungstenite::client(request, stream).map_err(LinkFault::from_handshake)?;
        socket
            .get_ref()
            .set_read_timeout(Some(ACTIVE_READ))
            .map_err(io)?;
        Ok(socket)
    }

    fn frame(kind: &str, id: &str, data: Value) -> Message {
        Message::binary(encode(&envelope(kind, id, data)))
    }

    fn parse(message: Message) -> Option<Value> {
        match message {
            Message::Binary(bytes) => rmp_serde::from_slice(&bytes).ok(),
            _ => None,
        }
    }

    fn authenticate(token: &str) -> Call {
        (
            "AuthenticationRequest",
            json!({ "authenticationToken": token }),
        )
    }

    fn request_token() -> Call {
        (
            "AuthenticationTokenRequest",
            json!({
                "pluginID": PLUGIN_ID,
                "pluginName": PLUGIN_NAME,
                "pluginDeveloper": PLUGIN_DEVELOPER,
                "pluginVersion": env!("CARGO_PKG_VERSION"),
                "scopes": SCOPES,
            }),
        )
    }

    fn list_hotkeys() -> Call {
        ("HotkeyListRequest", json!({}))
    }

    fn subscriptions() -> Vec<Call> {
        ["ModelLoadedEvent", "ModelConfigChangedEvent"]
            .into_iter()
            .map(|event| {
                (
                    "EventSubscriptionRequest",
                    json!({ "eventName": event, "subscribe": true }),
                )
            })
            .collect()
    }

    fn hotkey(id: &str) -> Call {
        ("HotkeyTriggerRequest", json!({ "hotkeyID": id }))
    }

    fn expression(file: &str, active: bool, fade: Option<f32>) -> Call {
        let mut data = json!({
            "expressionFile": file,
            "action": if active { "activate" } else { "deactivate" },
        });
        if let Some(f) = fade {
            data["fadeTime"] = json!(f);
        }
        ("ExpressionTriggerRequest", data)
    }

    fn inject(buf: &mut Vec<u8>, id: &str, mouth: Option<(&str, f32)>, holds: &[Hold]) -> Message {
        let mut parameters = Map::new();
        for h in holds {
            parameters.insert(h.name.clone(), json!(h.value));
        }
        if let Some((param, value)) = mouth {
            parameters.insert(param.to_owned(), json!(value));
        }
        let message = envelope(
            "ParameterWriteRequest",
            id,
            json!({ "parameters": parameters }),
        );
        buf.clear();
        let _ = rmp_serde::encode::write(buf, &message);
        Message::binary(buf.clone())
    }

    fn reply(v: &Value, auth_id: Option<&str>) -> Reply {
        let data = &v["data"];
        match v["messageType"].as_str().unwrap_or_default() {
            "AuthenticationTokenResponse" => data["authenticationToken"]
                .as_str()
                .filter(|t| !t.is_empty())
                .map_or(Reply::Ignore, |t| Reply::Token(t.to_owned())),
            "AuthenticationResponse" if data["authenticated"].as_bool().unwrap_or(false) => {
                Reply::Authenticated
            }
            "AuthenticationResponse" => Reply::AuthRejected,
            "HotkeyListResponse" => Reply::Hotkeys(parse_hotkeys(data)),
            "ModelLoadedEvent" | "ModelConfigChangedEvent" => Reply::ModelChanged,
            "APIError" => {
                let code = data["errorCode"].as_str().unwrap_or_default();
                let for_auth = auth_id.is_some_and(|id| v["requestID"].as_str() == Some(id));
                if for_auth && TOKEN_REFUSED.contains(&code) {
                    Reply::AuthRejected
                } else {
                    // NanaLive's own words; its code when it gave none.
                    let message = data["message"].as_str().filter(|m| !m.is_empty());
                    Reply::Error(message.unwrap_or(code).to_owned())
                }
            }
            _ => Reply::Ignore,
        }
    }
}

/// The hotkey catalog, without the ones NanaLive cannot run.
fn parse_hotkeys(data: &Value) -> Vec<AvatarHotkey> {
    let text = |h: &Value, k: &str| h[k].as_str().unwrap_or_default().to_owned();
    data["hotkeys"]
        .as_array()
        .map(|list| {
            list.iter()
                .filter(|h| h["executable"].as_bool() != Some(false))
                .map(|h| AvatarHotkey {
                    id: text(h, "hotkeyID"),
                    name: text(h, "name"),
                    kind: text(h, "type"),
                    file: text(h, "file"),
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use super::*;

    fn decode(message: Message) -> Value {
        NanaLive::parse(message).expect("a MessagePack frame")
    }

    #[test]
    fn requests_go_out_as_messagepack_envelopes() {
        let frame = NanaLive::frame("HotkeyTriggerRequest", "nt-1", json!({ "hotkeyID": "h" }));
        assert!(matches!(frame, Message::Binary(_)));
        let v = decode(frame);
        assert_eq!(v["apiName"], "NanaLiveControlAPI");
        assert_eq!(v["apiVersion"], "2.0");
        assert_eq!(v["requestID"], "nt-1");
        assert_eq!(v["messageType"], "HotkeyTriggerRequest");
        assert_eq!(v["data"]["hotkeyID"], "h");
        assert_eq!(NanaLive::parse(Message::text("{}")), None);
    }

    #[test]
    fn the_token_request_names_the_plugin_and_its_scopes() {
        let (kind, data) = NanaLive::request_token();
        assert_eq!(kind, "AuthenticationTokenRequest");
        assert_eq!(data["pluginID"], PLUGIN_ID);
        assert_eq!(data["pluginName"], "NanaType");
        let scopes: Vec<&str> = data["scopes"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(Value::as_str)
            .collect();
        assert!(scopes.contains(&"expression.trigger") && scopes.contains(&"parameter.write"));
    }

    #[test]
    fn expressions_are_activated_and_deactivated_by_file() {
        let (kind, data) = NanaLive::expression("Smile.exp3.json", true, None);
        assert_eq!(kind, "ExpressionTriggerRequest");
        assert_eq!(data["expressionFile"], "Smile.exp3.json");
        assert_eq!(data["action"], "activate");
        assert!(data.get("fadeTime").is_none());
        let (_, data) = NanaLive::expression("Smile.exp3.json", false, Some(0.3));
        assert_eq!(data["action"], "deactivate");
        assert!((data["fadeTime"].as_f64().unwrap() - 0.3).abs() < 1e-6);
    }

    #[test]
    fn parameters_are_written_as_one_map() {
        let holds = vec![Hold {
            name: "ParamEyeLOpen".into(),
            value: 0.25,
            until: Instant::now(),
        }];
        let mut buf = Vec::new();
        let v = decode(NanaLive::inject(
            &mut buf,
            "nt-7",
            Some(("ParamMouthOpenY", 0.5)),
            &holds,
        ));
        assert_eq!(v["messageType"], "ParameterWriteRequest");
        assert_eq!(v["requestID"], "nt-7");
        let params = &v["data"]["parameters"];
        assert_eq!(params["ParamMouthOpenY"], 0.5);
        assert_eq!(params["ParamEyeLOpen"], 0.25);
    }

    #[test]
    fn the_hotkey_catalog_keeps_what_can_run() {
        let data = json!({ "hotkeys": [
            { "hotkeyID": "abc", "name": "Smile", "type": "ToggleExpression", "file": "Smile.exp3.json", "executable": true },
            { "hotkeyID": "perf-1", "name": "Wave", "type": "Performance", "executable": true },
            { "hotkeyID": "x", "name": "Shot", "type": "TakeScreenshot", "executable": false },
        ]});
        let keys = parse_hotkeys(&data);
        assert_eq!(keys.len(), 2);
        assert_eq!(keys[0].file, "Smile.exp3.json");
        assert_eq!(keys[1].id, "perf-1");
    }

    /// A NanaLive stand-in: takes the handshake only with the subprotocol,
    /// answers authentication and the hotkey list, and hands every request
    /// it gets to the test.
    #[allow(clippy::result_large_err)] // tungstenite fixes the callback's error type.
    fn fake_nanalive() -> (u16, crossbeam_channel::Receiver<Value>) {
        use tungstenite::handshake::server::{Request, Response};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (tx, rx) = crossbeam_channel::unbounded();
        std::thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut socket = tungstenite::accept_hdr(stream, |req: &Request, mut res: Response| {
                let asked = req
                    .headers()
                    .get("Sec-WebSocket-Protocol")
                    .and_then(|v| v.to_str().ok());
                assert_eq!(asked, Some(SUBPROTOCOL));
                res.headers_mut().insert(
                    "Sec-WebSocket-Protocol",
                    HeaderValue::from_static(SUBPROTOCOL),
                );
                Ok(res)
            })
            .unwrap();
            while let Ok(message) = socket.read() {
                let Message::Binary(bytes) = message else {
                    continue;
                };
                let request: Value = rmp_serde::from_slice(&bytes).unwrap();
                let id = request["requestID"].as_str().unwrap_or_default().to_owned();
                let answer = match request["messageType"].as_str().unwrap_or_default() {
                    "AuthenticationRequest" => Some((
                        "AuthenticationResponse",
                        json!({ "authenticated": true, "scopes": SCOPES }),
                    )),
                    "HotkeyListRequest" => Some((
                        "HotkeyListResponse",
                        json!({ "hotkeys": [{
                            "hotkeyID": "abc", "name": "Smile", "type": "ToggleExpression",
                            "file": "Smile.exp3.json", "executable": true,
                        }]}),
                    )),
                    _ => None,
                };
                if let Some((kind, data)) = answer {
                    let reply = encode(&envelope(kind, &id, data));
                    socket.send(Message::binary(reply)).unwrap();
                }
                if tx.send(request).is_err() {
                    break;
                }
            }
        });
        (port, rx)
    }

    #[test]
    fn markers_reach_a_running_nanalive() {
        use crate::{AvatarConfig, BridgeEvent, BridgeHandle, BridgeStatus};
        let (port, requests) = fake_nanalive();
        let (status_tx, status) = crossbeam_channel::unbounded();
        let mut bridge = BridgeHandle::new(move |s| {
            let _ = status_tx.send(s);
        });
        bridge.set_nanalive(Some(AvatarConfig {
            port,
            token: Some("token".into()),
            expressions: Default::default(),
            lip_sync: None,
            reset: nanatype_model::VtsReset::LineEnd,
        }));
        let wait = Duration::from_secs(5);
        let hotkeys = loop {
            match status.recv_timeout(wait).expect("a status") {
                BridgeStatus::Hotkeys(Sink::NanaLive, keys) => break keys,
                BridgeStatus::Error(_, fault) => panic!("{fault:?}"),
                _ => {}
            }
        };
        assert_eq!(hotkeys[0].name, "Smile");
        bridge.emit(BridgeEvent::Expression {
            name: "smile".into(),
            intensity: None,
        });
        let kinds: Vec<String> = std::iter::from_fn(|| requests.recv_timeout(wait).ok())
            .inspect(|r| {
                if r["messageType"] == "ExpressionTriggerRequest" {
                    assert_eq!(r["data"]["expressionFile"], "Smile.exp3.json");
                    assert_eq!(r["data"]["action"], "activate");
                }
            })
            .map(|r| r["messageType"].as_str().unwrap_or_default().to_owned())
            .take_while(|k| k != "ExpressionTriggerRequest")
            .collect();
        assert_eq!(kinds[0], "AuthenticationRequest");
        assert!(kinds.contains(&"HotkeyListRequest".to_owned()), "{kinds:?}");
        assert!(
            kinds.contains(&"EventSubscriptionRequest".to_owned()),
            "{kinds:?}"
        );
    }

    #[test]
    fn a_refused_token_is_forgotten_but_other_errors_are_shown() {
        let error = |id: &str, code: &str| {
            json!({
                "messageType": "APIError",
                "requestID": id,
                "data": { "errorCode": code, "message": "nope" },
            })
        };
        assert_eq!(
            NanaLive::reply(&error("nt-3", "AUTHENTICATION_TOKEN_INVALID"), Some("nt-3")),
            Reply::AuthRejected
        );
        assert_eq!(
            NanaLive::reply(&error("nt-3", "AUTHENTICATION_REVOKED"), Some("nt-3")),
            Reply::AuthRejected
        );
        // Another request's error, or the auth's for another reason.
        assert_eq!(
            NanaLive::reply(&error("nt-4", "AUTHENTICATION_TOKEN_INVALID"), Some("nt-3")),
            Reply::Error("nope".into())
        );
        assert_eq!(
            NanaLive::reply(&error("nt-3", "RATE_LIMITED"), Some("nt-3")),
            Reply::Error("nope".into())
        );
        let ok =
            json!({ "messageType": "AuthenticationResponse", "data": { "authenticated": true } });
        assert_eq!(NanaLive::reply(&ok, Some("nt-3")), Reply::Authenticated);
        let event = json!({ "messageType": "ModelConfigChangedEvent", "requestID": "" });
        assert_eq!(NanaLive::reply(&event, None), Reply::ModelChanged);
    }
}
