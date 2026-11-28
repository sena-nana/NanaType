//! VTube Studio public API (`ws://127.0.0.1:<port>`, JSON text frames).
//!
//! The client itself is [`crate::avatar`]; this is VTube Studio's side of
//! it: message names, the plugin token popup, hotkeys of the current model
//! and parameter injection.

use std::net::{SocketAddr, TcpStream};
use std::time::Duration;

use nanatype_model::LinkFault;
use serde::Serialize;
use serde_json::{Value, json};
use tungstenite::{Message, WebSocket};

use crate::Sink;
use crate::avatar::{
    ACTIVE_READ, AvatarHotkey, Call, Hold, PLUGIN_DEVELOPER, PLUGIN_NAME, Protocol, Reply,
};

const API_NAME: &str = "VTubeStudioPublicAPI";

pub(crate) struct Vts;

pub(crate) fn request(kind: &str, id: &str, data: Value) -> String {
    json!({
        "apiName": API_NAME,
        "apiVersion": "1.0",
        "requestID": id,
        "messageType": kind,
        "data": data,
    })
    .to_string()
}

impl Protocol for Vts {
    const SINK: Sink = Sink::Vts;
    const THREAD: &'static str = "nanatype-vts";

    fn connect(port: u16) -> Result<WebSocket<TcpStream>, LinkFault> {
        let io = |e: std::io::Error| LinkFault::from_io(&e);
        let addr = SocketAddr::from(([127, 0, 0, 1], port));
        let stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2)).map_err(io)?;
        // Bound the handshake too: a port held by something else must not
        // hang the client.
        let timeout = Some(Duration::from_secs(2));
        stream.set_read_timeout(timeout).map_err(io)?;
        stream.set_write_timeout(timeout).map_err(io)?;
        let (socket, _) = tungstenite::client(format!("ws://{addr}"), stream)
            .map_err(LinkFault::from_handshake)?;
        socket
            .get_ref()
            .set_read_timeout(Some(ACTIVE_READ))
            .map_err(io)?;
        Ok(socket)
    }

    fn frame(kind: &str, id: &str, data: Value) -> Message {
        Message::text(request(kind, id, data))
    }

    fn parse(message: Message) -> Option<Value> {
        match message {
            Message::Text(text) => serde_json::from_str(&text).ok(),
            _ => None,
        }
    }

    fn authenticate(token: &str) -> Call {
        (
            "AuthenticationRequest",
            json!({
                "pluginName": PLUGIN_NAME,
                "pluginDeveloper": PLUGIN_DEVELOPER,
                "authenticationToken": token,
            }),
        )
    }

    fn request_token() -> Call {
        (
            "AuthenticationTokenRequest",
            json!({ "pluginName": PLUGIN_NAME, "pluginDeveloper": PLUGIN_DEVELOPER }),
        )
    }

    fn list_hotkeys() -> Call {
        ("HotkeysInCurrentModelRequest", json!({}))
    }

    fn subscriptions() -> Vec<Call> {
        vec![(
            "EventSubscriptionRequest",
            json!({ "eventName": "ModelLoadedEvent", "subscribe": true }),
        )]
    }

    fn hotkey(id: &str) -> Call {
        ("HotkeyTriggerRequest", json!({ "hotkeyID": id }))
    }

    fn expression(file: &str, active: bool, fade: Option<f32>) -> Call {
        let mut data = json!({ "expressionFile": file, "active": active });
        if let Some(f) = fade {
            data["fadeTime"] = json!(f);
        }
        ("ExpressionActivationRequest", data)
    }

    fn inject(buf: &mut Vec<u8>, id: &str, mouth: Option<(&str, f32)>, holds: &[Hold]) -> Message {
        format_inject_into(buf, id, mouth, holds);
        Message::text(std::str::from_utf8(buf).unwrap_or_default())
    }

    fn reply(v: &Value, _auth_id: Option<&str>) -> Reply {
        let data = &v["data"];
        match v["messageType"].as_str().unwrap_or_default() {
            "AuthenticationTokenResponse" => data["authenticationToken"]
                .as_str()
                .map_or(Reply::Ignore, |t| Reply::Token(t.to_owned())),
            "AuthenticationResponse" if data["authenticated"].as_bool().unwrap_or(false) => {
                Reply::Authenticated
            }
            // Revoked in VTube Studio, or from another install.
            "AuthenticationResponse" => Reply::AuthRejected,
            "HotkeysInCurrentModelResponse" => Reply::Hotkeys(parse_hotkeys(data)),
            "ModelLoadedEvent" => Reply::ModelChanged,
            // VTube Studio's own words; empty when it gave none.
            "APIError" => Reply::Error(data["message"].as_str().unwrap_or_default().to_owned()),
            _ => Reply::Ignore,
        }
    }
}

#[derive(Serialize)]
struct ParamValue<'a> {
    id: &'a str,
    value: f32,
}

struct InjectParams<'a> {
    mouth: Option<(&'a str, f32)>,
    holds: &'a [Hold],
}

impl Serialize for InjectParams<'_> {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeSeq;
        let len = usize::from(self.mouth.is_some()) + self.holds.len();
        let mut seq = serializer.serialize_seq(Some(len))?;
        if let Some((id, value)) = self.mouth {
            seq.serialize_element(&ParamValue { id, value })?;
        }
        for h in self.holds {
            seq.serialize_element(&ParamValue {
                id: &h.name,
                value: h.value,
            })?;
        }
        seq.end()
    }
}

#[derive(Serialize)]
struct InjectData<'a> {
    #[serde(rename = "faceFound")]
    face_found: bool,
    mode: &'static str,
    #[serde(rename = "parameterValues")]
    parameter_values: InjectParams<'a>,
}

#[derive(Serialize)]
struct InjectEnvelope<'a> {
    #[serde(rename = "apiName")]
    api_name: &'static str,
    #[serde(rename = "apiVersion")]
    api_version: &'static str,
    #[serde(rename = "requestID")]
    request_id: &'a str,
    #[serde(rename = "messageType")]
    message_type: &'static str,
    data: InjectData<'a>,
}

/// `InjectParameterDataRequest` without `json!()` heap allocations: it goes
/// out thirty times a second while the mouth moves.
fn format_inject_into(
    buf: &mut Vec<u8>,
    request_id: &str,
    mouth: Option<(&str, f32)>,
    holds: &[Hold],
) {
    buf.clear();
    let envelope = InjectEnvelope {
        api_name: API_NAME,
        api_version: "1.0",
        request_id,
        message_type: "InjectParameterDataRequest",
        data: InjectData {
            face_found: false,
            mode: "set",
            parameter_values: InjectParams { mouth, holds },
        },
    };
    let _ = serde_json::to_writer(&mut *buf, &envelope);
}

fn parse_hotkeys(data: &Value) -> Vec<AvatarHotkey> {
    let text = |h: &Value, k: &str| h[k].as_str().unwrap_or_default().to_owned();
    data["availableHotkeys"]
        .as_array()
        .map(|list| {
            list.iter()
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

    #[test]
    fn requests_carry_the_api_envelope() {
        let r: Value = serde_json::from_str(&request(
            "HotkeyTriggerRequest",
            "1",
            json!({"hotkeyID": "h"}),
        ))
        .unwrap();
        assert_eq!(r["apiName"], "VTubeStudioPublicAPI");
        assert_eq!(r["messageType"], "HotkeyTriggerRequest");
        assert_eq!(r["data"]["hotkeyID"], "h");
    }

    #[test]
    fn expressions_turn_on_and_off_by_file() {
        let (kind, data) = Vts::expression("a.exp3.json", false, Some(0.5));
        assert_eq!(kind, "ExpressionActivationRequest");
        assert_eq!(data["active"], false);
        assert_eq!(data["fadeTime"], 0.5);
    }

    #[test]
    fn hotkey_lists_keep_their_files() {
        let data = json!({ "availableHotkeys": [
            { "hotkeyID": "abc", "name": "Smile", "type": "ToggleExpression", "file": "Smile.exp3.json" },
            { "hotkeyID": "def", "name": "Wave", "type": "TriggerAnimation" },
        ]});
        let keys = parse_hotkeys(&data);
        assert_eq!(keys[0].file, "Smile.exp3.json");
        assert_eq!(keys[0].kind, "ToggleExpression");
        assert_eq!(keys[1].file, "");
    }

    #[test]
    fn authentication_answers_are_told_apart() {
        let ok =
            json!({ "messageType": "AuthenticationResponse", "data": { "authenticated": true } });
        let no =
            json!({ "messageType": "AuthenticationResponse", "data": { "authenticated": false } });
        assert_eq!(Vts::reply(&ok, None), Reply::Authenticated);
        assert_eq!(Vts::reply(&no, None), Reply::AuthRejected);
    }

    #[test]
    fn inject_buffer_matches_api_envelope_without_json_macro() {
        let mut buf = Vec::new();
        let holds = vec![Hold {
            name: "EyeOpen".into(),
            value: 0.25,
            until: Instant::now(),
        }];
        format_inject_into(&mut buf, "nt-42", Some(("MouthOpen", 0.5)), &holds);
        let actual: Value = serde_json::from_slice(&buf).unwrap();
        let expected: Value = serde_json::from_str(&request(
            "InjectParameterDataRequest",
            "nt-42",
            json!({
                "faceFound": false,
                "mode": "set",
                "parameterValues": [
                    { "id": "MouthOpen", "value": 0.5 },
                    { "id": "EyeOpen", "value": 0.25 }
                ]
            }),
        ))
        .unwrap();
        assert_eq!(actual, expected);
    }
}
