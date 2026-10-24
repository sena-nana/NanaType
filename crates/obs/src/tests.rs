use std::net::TcpListener;
use std::thread;
use std::time::{Duration, Instant};

use crossbeam_channel::{Receiver, Sender, unbounded};
use serde_json::{Value, json};
use tungstenite::Message;
use tungstenite::protocol::CloseFrame;
use tungstenite::protocol::frame::coding::CloseCode;

use crate::protocol::{self, Incoming};
use crate::{ObsConfig, ObsEvent, ObsHandle, ObsStatus, ObsUpdate};

#[test]
fn auth_matches_the_protocol_documents_example() {
    // docs/generated/protocol.md, "Creating an authentication string".
    assert_eq!(
        protocol::auth_string(
            "supersecretpassword",
            "lM1GncleQOaCu9lT1yeUZhFYnqhsLLP1G5lAGo3ixaI=",
            "+IxH4CnCiqpX1rM9scsNynZzbOe4KhDeYcTNS3PDaeY=",
        ),
        "1Ct943GAT+6YQUUX47Ia/ncufilbe6+oD6lY+5kaCu4="
    );
}

#[test]
fn identify_asks_for_the_events_and_answers_the_challenge() {
    let open = json!({ "obsWebSocketVersion": "5.5.0", "rpcVersion": 1 });
    let v: Value = serde_json::from_str(&protocol::identify(&open, None).unwrap()).unwrap();
    assert_eq!(v["op"], 1);
    assert_eq!(v["d"]["eventSubscriptions"], protocol::EVENT_SUBSCRIPTIONS);
    assert!(v["d"].get("authentication").is_none());
    let locked = json!({ "rpcVersion": 1, "authentication": { "challenge": "c", "salt": "s" } });
    assert!(protocol::identify(&locked, None).is_err());
    let v: Value = serde_json::from_str(&protocol::identify(&locked, Some("pw")).unwrap()).unwrap();
    assert_eq!(
        v["d"]["authentication"],
        protocol::auth_string("pw", "s", "c")
    );
}

fn transform(bounds: Option<[f64; 2]>, size: [f64; 2]) -> Value {
    json!({
        "sourceWidth": size[0], "sourceHeight": size[1],
        "scaleX": 1.0, "scaleY": 1.0,
        "width": size[0], "height": size[1],
        "boundsType": if bounds.is_some() { "OBS_BOUNDS_SCALE_INNER" } else { "OBS_BOUNDS_NONE" },
        "boundsWidth": bounds.map_or(1.0, |b| b[0]),
        "boundsHeight": bounds.map_or(1.0, |b| b[1]),
    })
}

#[test]
fn scene_items_read_bounds_and_kinds() {
    let data = json!({ "sceneItems": [
        { "sceneItemId": 3, "sourceName": "NanaType", "inputKind": "spout_capture",
          "isGroup": false, "sceneItemTransform": transform(Some([960.0, 320.0]), [480.0, 160.0]) },
        { "sceneItemId": 4, "sourceName": "Camera", "inputKind": "dshow_input",
          "isGroup": false, "sceneItemTransform": transform(None, [1280.0, 720.0]) },
        { "sceneItemId": 5, "sourceName": "Nested", "inputKind": null, "isGroup": true },
    ]});
    let items = protocol::scene_items(&data);
    assert_eq!(items.len(), 3);
    assert!(items[0].is_capture() && !items[1].is_capture());
    assert_eq!(items[0].transform.bounds, Some([960.0, 320.0]));
    assert_eq!(items[1].transform.bounds, None);
    assert_eq!(items[1].transform.size, [1280.0, 720.0]);
    assert!(items[2].group && items[2].kind.is_empty());
}

#[test]
fn set_bounds_turns_on_a_centred_scale_inner_box() {
    let v: Value = serde_json::from_str(&protocol::set_bounds("7", "Live", 3, [960, 320])).unwrap();
    assert_eq!(v["op"], 6);
    assert_eq!(v["d"]["requestType"], "SetSceneItemTransform");
    let d = &v["d"]["requestData"];
    assert_eq!(d["sceneName"], "Live");
    assert_eq!(d["sceneItemId"], 3);
    let t = &d["sceneItemTransform"];
    assert_eq!(t["boundsType"], "OBS_BOUNDS_SCALE_INNER");
    assert_eq!(t["boundsAlignment"], 0);
    assert_eq!(
        (t["boundsWidth"].as_f64(), t["boundsHeight"].as_f64()),
        (Some(960.0), Some(320.0))
    );
}

/// A fake obs-websocket server with one program scene, "Live", holding
/// `NanaType` (id 3). Requests it saw go to `seen`; `events` are pushed.
fn fake_obs(password: &'static str, seen: Sender<Value>, events: Receiver<Value>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let hello = json!({ "op": 0, "d": { "obsWebSocketVersion": "5.5.0", "rpcVersion": 1,
            "authentication": { "challenge": "chal", "salt": "salt" } } });
        ws.send(Message::text(hello.to_string())).unwrap();
        let Ok(Message::Text(identify)) = ws.read() else {
            return;
        };
        let identify: Value = serde_json::from_str(identify.as_str()).unwrap();
        if identify["d"]["authentication"] != protocol::auth_string(password, "salt", "chal") {
            let _ = ws.close(Some(CloseFrame {
                code: CloseCode::from(protocol::CLOSE_AUTH_FAILED),
                reason: "Authentication failed.".into(),
            }));
            let _ = ws.flush();
            let _ = ws.read();
            return;
        }
        ws.send(Message::text(
            json!({ "op": 2, "d": { "negotiatedRpcVersion": 1 } }).to_string(),
        ))
        .unwrap();
        ws.get_ref()
            .set_read_timeout(Some(Duration::from_millis(20)))
            .unwrap();
        loop {
            while let Ok(event) = events.try_recv() {
                ws.send(Message::text(json!({ "op": 5, "d": event }).to_string()))
                    .unwrap();
            }
            let text = match ws.read() {
                Ok(Message::Text(text)) => text,
                Ok(_) => continue,
                Err(tungstenite::Error::Io(_)) => continue,
                Err(_) => return,
            };
            let v: Value = serde_json::from_str(text.as_str()).unwrap();
            let d = &v["d"];
            let data = match d["requestType"].as_str().unwrap() {
                "GetCurrentProgramScene" => json!({ "currentProgramSceneName": "Live" }),
                "GetSceneItemList" => json!({ "sceneItems": [
                    { "sceneItemId": 3, "sourceName": "NanaType", "inputKind": "spout_capture",
                      "isGroup": false, "sceneItemTransform": transform(None, [960.0, 320.0]) },
                ]}),
                _ => json!({}),
            };
            let _ = seen.send(d.clone());
            let reply = json!({ "op": 7, "d": {
                "requestType": d["requestType"], "requestId": d["requestId"],
                "requestStatus": { "result": true, "code": 100 }, "responseData": data } });
            ws.send(Message::text(reply.to_string())).unwrap();
        }
    });
    port
}

fn next(rx: &Receiver<ObsEvent>, until: impl Fn(&ObsUpdate) -> bool) -> ObsUpdate {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let left = deadline.saturating_duration_since(Instant::now());
        let event = rx.recv_timeout(left).expect("no such update in time");
        if until(&event.update) {
            return event.update;
        }
    }
}

#[test]
fn follows_the_program_scene_and_sets_bounds() {
    let (seen_tx, seen) = unbounded();
    let (events_tx, events) = unbounded();
    let port = fake_obs("pw", seen_tx, events);
    let (tx, rx) = unbounded();
    let mut obs = ObsHandle::new(move |e| {
        let _ = tx.send(e);
    });
    obs.set(Some(ObsConfig {
        host: "127.0.0.1".into(),
        port,
        password: Some("pw".into()),
    }));
    next(&rx, |u| *u == ObsUpdate::Status(ObsStatus::Connected));
    let ObsUpdate::Scene(Some(scene)) = next(&rx, |u| matches!(u, ObsUpdate::Scene(Some(_))))
    else {
        unreachable!()
    };
    assert_eq!(scene.name, "Live");
    let item = scene.item("NanaType").unwrap();
    assert_eq!((item.id, item.transform.bounds), (3, None));

    // A drag: several transforms, reported once it rests.
    let started = Instant::now();
    for w in [700.0, 800.0, 900.0] {
        events_tx
            .send(
                json!({ "eventType": "SceneItemTransformChanged", "eventIntent": 1 << 19,
                "eventData": { "sceneName": "Live", "sceneItemId": 3,
                    "sceneItemTransform": transform(Some([w, 300.0]), [960.0, 320.0]) } }),
            )
            .unwrap();
    }
    let ObsUpdate::Scene(Some(scene)) = next(&rx, |u| matches!(u, ObsUpdate::Scene(Some(_))))
    else {
        unreachable!()
    };
    assert!(started.elapsed() >= Duration::from_millis(250));
    assert_eq!(
        scene.item("NanaType").unwrap().transform.bounds,
        Some([900.0, 300.0])
    );

    // Another scene's items are not ours.
    events_tx
        .send(
            json!({ "eventType": "SceneItemTransformChanged", "eventIntent": 1 << 19,
            "eventData": { "sceneName": "Other", "sceneItemId": 3,
                "sceneItemTransform": transform(Some([10.0, 10.0]), [10.0, 10.0]) } }),
        )
        .unwrap();

    obs.set_bounds("NanaType", [960, 320]);
    obs.set_bounds("Missing", [1, 1]);
    let deadline = Instant::now() + Duration::from_secs(5);
    let request = loop {
        let d = seen.recv_timeout(deadline - Instant::now()).unwrap();
        if d["requestType"] == "SetSceneItemTransform" {
            break d;
        }
    };
    assert_eq!(request["requestData"]["sceneItemId"], 3);
    assert_eq!(
        request["requestData"]["sceneItemTransform"]["boundsWidth"],
        960.0
    );
    assert!(
        rx.recv_timeout(Duration::from_millis(400))
            .map(|e| !matches!(e.update, ObsUpdate::Scene(_)))
            .unwrap_or(true),
        "the other scene's transform was reported"
    );

    obs.set(None);
    let epoch = obs.epoch();
    let off = rx.recv_timeout(Duration::from_secs(1)).unwrap();
    assert_eq!(off.epoch, epoch);
}

#[test]
fn a_wrong_password_is_not_retried() {
    let (seen_tx, _seen) = unbounded();
    let (_events_tx, events) = unbounded();
    let port = fake_obs("right", seen_tx, events);
    let (tx, rx) = unbounded();
    let mut obs = ObsHandle::new(move |e| {
        let _ = tx.send(e);
    });
    obs.set(Some(ObsConfig {
        host: "127.0.0.1".into(),
        port,
        password: Some("wrong".into()),
    }));
    next(&rx, |u| *u == ObsUpdate::Status(ObsStatus::AuthFailed));
    assert!(
        rx.recv_timeout(Duration::from_millis(3500))
            .map(|e| e.update != ObsUpdate::Status(ObsStatus::Connecting))
            .unwrap_or(true)
    );
}

#[test]
fn a_locked_server_without_a_password_needs_one() {
    let (seen_tx, _seen) = unbounded();
    let (_events_tx, events) = unbounded();
    let port = fake_obs("right", seen_tx, events);
    let (tx, rx) = unbounded();
    let mut obs = ObsHandle::new(move |e| {
        let _ = tx.send(e);
    });
    obs.set(Some(ObsConfig {
        host: "127.0.0.1".into(),
        port,
        password: None,
    }));
    next(&rx, |u| *u == ObsUpdate::Status(ObsStatus::NeedsPassword));
}

#[test]
fn nothing_listening_is_an_error_and_retried() {
    let port = {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        l.local_addr().unwrap().port()
    };
    let (tx, rx) = unbounded();
    let mut obs = ObsHandle::new(move |e| {
        let _ = tx.send(e);
    });
    obs.set(Some(ObsConfig {
        host: "127.0.0.1".into(),
        port,
        password: None,
    }));
    next(&rx, |u| matches!(u, ObsUpdate::Status(ObsStatus::Error(_))));
}

#[test]
fn parse_reads_every_opcode_used() {
    assert!(matches!(
        protocol::parse(r#"{"op":0,"d":{"rpcVersion":1}}"#),
        Some(Incoming::Hello(_))
    ));
    assert_eq!(
        protocol::parse(r#"{"op":2,"d":{}}"#),
        Some(Incoming::Identified)
    );
    let Some(Incoming::Response {
        id, ok, comment, ..
    }) = protocol::parse(
        r#"{"op":7,"d":{"requestId":"9","requestStatus":{"result":false,"code":600,"comment":"No source"}}}"#,
    )
    else {
        panic!()
    };
    assert_eq!(
        (id.as_str(), ok, comment.as_str()),
        ("9", false, "No source")
    );
    assert_eq!(protocol::parse("not json"), None);
}
