//! obs-websocket 5.x messages: the handshake, the few requests NanaType
//! sends and the events it follows. Pure functions; the thread is in
//! [`crate::client`].

use base64::Engine;
use base64::engine::general_purpose::STANDARD;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

/// The RPC version NanaType speaks.
pub const RPC_VERSION: u64 = 1;

/// Scenes (program scene changes, renames), Inputs (renames), SceneItems
/// (added, removed) and the high-volume SceneItemTransformChanged.
pub const EVENT_SUBSCRIPTIONS: u64 = (1 << 2) | (1 << 3) | (1 << 7) | (1 << 19);

/// The close code obs-websocket sends after a wrong password.
pub const CLOSE_AUTH_FAILED: u16 = 4009;

/// `OBS_BOUNDS_NONE`: the item is scaled, it has no bounding box.
pub const BOUNDS_NONE: &str = "OBS_BOUNDS_NONE";
/// What [`set_bounds`] turns on: fit inside the box, keeping the aspect.
pub const BOUNDS_SCALE_INNER: &str = "OBS_BOUNDS_SCALE_INNER";

/// The answer to the server's challenge:
/// `base64(sha256(base64(sha256(password + salt)) + challenge))`.
pub fn auth_string(password: &str, salt: &str, challenge: &str) -> String {
    let secret = STANDARD.encode(Sha256::digest(format!("{password}{salt}")));
    STANDARD.encode(Sha256::digest(format!("{secret}{challenge}")))
}

/// The Identify message for a Hello's `d`. Fails when the server wants a
/// password and none was given.
pub fn identify(hello: &Value, password: Option<&str>) -> Result<String, NeedsPassword> {
    let mut d = json!({
        "rpcVersion": RPC_VERSION,
        "eventSubscriptions": EVENT_SUBSCRIPTIONS,
    });
    if let Some(auth) = hello.get("authentication") {
        let password = password.ok_or(NeedsPassword)?;
        let field = |k: &str| auth.get(k).and_then(Value::as_str).unwrap_or_default();
        d["authentication"] = auth_string(password, field("salt"), field("challenge")).into();
    }
    Ok(json!({ "op": 1, "d": d }).to_string())
}

/// The server asks for a password and none is set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NeedsPassword;

pub fn request(kind: &str, id: &str, data: Value) -> String {
    json!({
        "op": 6,
        "d": { "requestType": kind, "requestId": id, "requestData": data },
    })
    .to_string()
}

/// Turn on a bounding box of `size` for one scene item, centred.
pub fn set_bounds(id: &str, scene: &str, item: i64, size: [u32; 2]) -> String {
    request(
        "SetSceneItemTransform",
        id,
        json!({
            "sceneName": scene,
            "sceneItemId": item,
            "sceneItemTransform": {
                "boundsType": BOUNDS_SCALE_INNER,
                "boundsAlignment": 0,
                "boundsWidth": f64::from(size[0]),
                "boundsHeight": f64::from(size[1]),
            },
        }),
    )
}

/// One message from the server.
#[derive(Debug, Clone, PartialEq)]
pub enum Incoming {
    Hello(Value),
    Identified,
    Event {
        kind: String,
        data: Value,
    },
    Response {
        id: String,
        ok: bool,
        comment: String,
        data: Value,
    },
    Other,
}

pub fn parse(text: &str) -> Option<Incoming> {
    let v: Value = serde_json::from_str(text).ok()?;
    let d = v.get("d").cloned().unwrap_or(Value::Null);
    let str_of = |v: &Value, k: &str| {
        v.get(k)
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned()
    };
    Some(match v.get("op")?.as_u64()? {
        0 => Incoming::Hello(d),
        2 => Incoming::Identified,
        5 => Incoming::Event {
            kind: str_of(&d, "eventType"),
            data: d.get("eventData").cloned().unwrap_or(Value::Null),
        },
        7 => {
            let status = d.get("requestStatus").cloned().unwrap_or(Value::Null);
            Incoming::Response {
                id: str_of(&d, "requestId"),
                ok: status.get("result").and_then(Value::as_bool) == Some(true),
                comment: str_of(&status, "comment"),
                data: d.get("responseData").cloned().unwrap_or(Value::Null),
            }
        }
        _ => Incoming::Other,
    })
}

/// One source placed in the program scene.
#[derive(Debug, Clone, PartialEq)]
pub struct ObsItem {
    pub id: i64,
    pub source: String,
    /// The input kind (`spout_capture`, `syphon-input`, …); empty for a
    /// nested scene.
    pub kind: String,
    pub group: bool,
    pub transform: Transform,
}

impl ObsItem {
    /// A Spout2 Capture or Syphon Client source.
    pub fn is_capture(&self) -> bool {
        let k = self.kind.to_ascii_lowercase();
        k.contains("spout") || k.contains("syphon")
    }
}

/// The parts of a scene item's transform NanaType reads.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Transform {
    /// The size it is drawn at without a bounding box (source × scale).
    pub size: [f64; 2],
    /// The bounding box, when one is on.
    pub bounds: Option<[f64; 2]>,
}

pub fn transform(v: &Value) -> Transform {
    let num = |k: &str| v.get(k).and_then(Value::as_f64).unwrap_or(0.0);
    let kind = v
        .get("boundsType")
        .and_then(Value::as_str)
        .unwrap_or(BOUNDS_NONE);
    let size = if num("width") > 0.0 {
        [num("width"), num("height")]
    } else {
        [
            num("sourceWidth") * num("scaleX").abs(),
            num("sourceHeight") * num("scaleY").abs(),
        ]
    };
    Transform {
        size,
        bounds: (kind != BOUNDS_NONE).then(|| [num("boundsWidth"), num("boundsHeight")]),
    }
}

/// `GetSceneItemList`'s `responseData`.
pub fn scene_items(data: &Value) -> Vec<ObsItem> {
    data.get("sceneItems")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    Some(ObsItem {
                        id: item.get("sceneItemId")?.as_i64()?,
                        source: item.get("sourceName")?.as_str()?.to_owned(),
                        kind: item
                            .get("inputKind")
                            .and_then(Value::as_str)
                            .unwrap_or_default()
                            .to_owned(),
                        group: item.get("isGroup").and_then(Value::as_bool) == Some(true),
                        transform: item
                            .get("sceneItemTransform")
                            .map(transform)
                            .unwrap_or_default(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `GetCurrentProgramScene`'s `responseData`, or a
/// `CurrentProgramSceneChanged` event's data.
pub fn program_scene(data: &Value) -> Option<String> {
    ["currentProgramSceneName", "sceneName"]
        .into_iter()
        .find_map(|k| data.get(k).and_then(Value::as_str))
        .map(str::to_owned)
}
