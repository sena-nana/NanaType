use std::net::TcpListener;
use std::sync::mpsc;
use std::time::Duration;

use serde_json::{Value, json};
use tungstenite::Message;

use crate::protocol::{HEARTBEAT_MESSAGE, Incoming, join_message, parse};
use crate::*;

fn event(frame: Value) -> ChatEvent {
    match parse(&frame.to_string()) {
        Some(Incoming::Event(e)) => e,
        other => panic!("not an event: {other:?}"),
    }
}

#[test]
fn join_carries_the_room_key_and_the_legacy_room_id() {
    let v: Value = serde_json::from_str(&join_message(&RoomKey::RoomId(21_452_505))).unwrap();
    assert_eq!(v["cmd"], 1);
    assert_eq!(
        v["data"]["roomKey"],
        json!({"type": 1, "value": 21_452_505})
    );
    assert_eq!(v["data"]["roomId"], 21_452_505);
    let v: Value = serde_json::from_str(&join_message(&RoomKey::AuthCode("ABC".into()))).unwrap();
    assert_eq!(v["data"]["roomKey"], json!({"type": 2, "value": "ABC"}));
    assert!(v["data"].get("roomId").is_none());
    assert_eq!(parse(HEARTBEAT_MESSAGE), Some(Incoming::Heartbeat));
}

#[test]
fn room_keys_read_what_the_user_typed() {
    assert_eq!(RoomKey::parse(false, " 123 "), Some(RoomKey::RoomId(123)));
    assert_eq!(RoomKey::parse(false, "abc"), None);
    assert_eq!(RoomKey::parse(false, "0"), None);
    assert_eq!(
        RoomKey::parse(true, " X1 "),
        Some(RoomKey::AuthCode("X1".into()))
    );
    assert_eq!(RoomKey::parse(true, "  "), None);
}

#[test]
fn a_v1_10_text_message_reads_every_field() {
    let e = event(json!({"cmd": 2, "data": [
        "//i0.hdslb.com/bfs/face/a.jpg", 1_700_000_000, "观众", 1, "晚上好", 3, 0, 20, 0, 1,
        12, "msg-1", "", 0, [], [], "42", "奈奈团", 0
    ]}));
    let ChatEvent::Text {
        id,
        author,
        content,
        emoticon,
        at_ms,
    } = e
    else {
        panic!("{e:?}");
    };
    assert_eq!(id, "msg-1");
    assert_eq!(content, "晚上好");
    assert_eq!(emoticon, None);
    assert_eq!(at_ms, 1_700_000_000_000);
    assert_eq!(author.uid, "42");
    assert_eq!(author.name, "观众");
    assert_eq!(author.avatar_url, "https://i0.hdslb.com/bfs/face/a.jpg");
    assert_eq!(author.guard, Guard::Captain);
    assert_eq!(author.medal_level, 12);
    assert_eq!(author.medal_name, "奈奈团");
}

#[test]
fn a_v1_7_text_message_without_uid_still_reads() {
    let e = event(json!({"cmd": 2, "data": [
        "https://a/b.png", 1_700_000_000, "旧版", 3, "hi", 0, 0, 1, 0, 0, 0, 7, "", 0, [], []
    ]}));
    let ChatEvent::Text { id, author, .. } = e else {
        panic!("{e:?}");
    };
    assert_eq!(id, "7", "numeric ids become text");
    assert_eq!(author.uid, "");
    assert!(author.owner);
    assert_eq!(author.guard, Guard::None);
}

#[test]
fn an_emoticon_danmaku_carries_its_picture() {
    let e = event(json!({"cmd": 2, "data": [
        "", 1_700_000_000_123u64, "a", 0, "[dog]", 0, 0, 0, 0, 0, 0, "x", "", 1,
        ["//i0.hdslb.com/emote.png"], [], "1", "", 0
    ]}));
    let ChatEvent::Text {
        emoticon, at_ms, ..
    } = e
    else {
        panic!("{e:?}");
    };
    assert_eq!(emoticon.as_deref(), Some("https://i0.hdslb.com/emote.png"));
    assert_eq!(at_ms, 1_700_000_000_123, "millisecond stamps pass through");
}

#[test]
fn gifts_memberships_and_super_chats_read_their_objects() {
    let author = json!({"avatarUrl": "//x/a.jpg", "authorName": "老板", "uid": 9, "privilegeType": 1,
        "medalLevel": 3, "medalName": "m"});
    let mut gift = author.clone();
    gift.as_object_mut().unwrap().extend(
        json!({"id": "g", "timestamp": 1_700_000_000, "giftName": "小花花", "num": 5,
            "totalCoin": 500, "totalFreeCoin": 0})
        .as_object()
        .unwrap()
        .clone(),
    );
    match event(json!({"cmd": 3, "data": gift})) {
        ChatEvent::Gift {
            author,
            gift_name,
            num,
            total_coin,
            ..
        } => {
            assert_eq!((gift_name.as_str(), num, total_coin), ("小花花", 5, 500));
            assert_eq!(author.uid, "9");
            assert_eq!(author.guard, Guard::Governor);
        }
        e => panic!("{e:?}"),
    }
    let mut member = author.clone();
    member.as_object_mut().unwrap().extend(
        json!({"id": "m", "timestamp": 1, "num": 1, "unit": "月", "total_coin": 198_000})
            .as_object()
            .unwrap()
            .clone(),
    );
    match event(json!({"cmd": 4, "data": member})) {
        ChatEvent::Member {
            unit, total_coin, ..
        } => assert_eq!((unit.as_str(), total_coin), ("月", 198_000)),
        e => panic!("{e:?}"),
    }
    let mut sc = author;
    sc.as_object_mut().unwrap().extend(
        json!({"id": "s", "timestamp": 1, "price": 30, "content": "加油"})
            .as_object()
            .unwrap()
            .clone(),
    );
    match event(json!({"cmd": 5, "data": sc})) {
        ChatEvent::SuperChat { price, content, .. } => {
            assert_eq!((price, content.as_str()), (30, "加油"))
        }
        e => panic!("{e:?}"),
    }
    assert_eq!(
        event(json!({"cmd": 6, "data": {"ids": ["s", 2]}})),
        ChatEvent::DeleteSuperChats {
            ids: vec!["s".into(), "2".into()]
        }
    );
    assert_eq!(
        parse(&json!({"cmd": 8, "data": {"type": 1, "msg": "bad"}}).to_string()),
        Some(Incoming::Fatal {
            kind: FatalKind::AuthCode,
            message: "bad".into()
        })
    );
}

#[test]
fn unknown_and_broken_frames_are_ignored() {
    assert_eq!(
        parse(r#"{"cmd": 7, "data": ["x", "y"]}"#),
        Some(Incoming::Other)
    );
    assert_eq!(parse(r#"{"cmd": 2, "data": {}}"#), Some(Incoming::Other));
    assert_eq!(parse("not json"), None);
}

#[test]
fn the_client_joins_answers_heartbeats_and_delivers_messages() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (joined_tx, joined_rx) = mpsc::channel();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let mut ws = tungstenite::accept(stream).unwrap();
        let join = ws.read().unwrap();
        joined_tx
            .send(join.into_text().unwrap().to_string())
            .unwrap();
        ws.send(Message::text(HEARTBEAT_MESSAGE)).unwrap();
        let echo = ws.read().unwrap();
        ws.send(Message::text(
            json!({"cmd": 2, "data": ["", 1, "观众", 0, "你好", 0, 0, 0, 0, 0, 0, "1", "", 0, [], [], "5", "", 0]})
                .to_string(),
        ))
        .unwrap();
        // Hold the socket until the client goes away.
        let _ = ws.read();
        echo.into_text().unwrap().to_string()
    });
    let (status_tx, status_rx) = mpsc::channel();
    let (event_tx, event_rx) = mpsc::channel();
    let mut handle = BliveHandle::new(move |e: BliveEvent| {
        if e.epoch != 1 {
            // `set(None)` below reports Off under the next epoch.
            assert_eq!(e.update, BliveUpdate::Status(BliveStatus::Off));
            return;
        }
        let _ = match e.update {
            BliveUpdate::Status(s) => status_tx.send(s).map_err(drop),
            BliveUpdate::Chat(c) => event_tx.send(c).map_err(drop),
        };
    });
    handle.set(Some(BliveConfig {
        host: "127.0.0.1".into(),
        port,
        room: RoomKey::AuthCode("CODE".into()),
    }));
    let join: Value =
        serde_json::from_str(&joined_rx.recv_timeout(Duration::from_secs(5)).unwrap()).unwrap();
    assert_eq!(join["data"]["roomKey"]["value"], "CODE");
    match event_rx.recv_timeout(Duration::from_secs(5)).unwrap() {
        ChatEvent::Text {
            content, author, ..
        } => {
            assert_eq!(content, "你好");
            assert_eq!(author.uid, "5");
        }
        e => panic!("{e:?}"),
    }
    assert_eq!(status_rx.recv().unwrap(), BliveStatus::Connecting);
    assert_eq!(status_rx.recv().unwrap(), BliveStatus::Connected);
    assert_eq!(handle.epoch(), 1);
    handle.set(None);
    assert_eq!(handle.epoch(), 2, "stopping starts a new epoch");
    let echo: Value = serde_json::from_str(&server.join().unwrap()).unwrap();
    assert_eq!(echo["cmd"], 0, "the heartbeat is answered");
}

/// The fields `expect` in the fixtures names, read from an event.
fn summary(event: &ChatEvent) -> Value {
    let author = |a: &Author| {
        json!({"uid": a.uid, "name": a.name, "guard": a.guard as u8, "medal_level": a.medal_level,
            "owner": a.owner, "admin": a.admin})
    };
    let (kind, id, a, extra) = match event {
        ChatEvent::Text {
            id,
            author: a,
            content,
            emoticon,
            ..
        } => (
            "text",
            id,
            a,
            json!({"content": content, "emoticon": emoticon}),
        ),
        ChatEvent::Gift {
            id,
            author: a,
            gift_name,
            num,
            total_coin,
            ..
        } => (
            "gift",
            id,
            a,
            json!({"gift_name": gift_name, "num": num, "total_coin": total_coin}),
        ),
        ChatEvent::Member {
            id,
            author: a,
            num,
            unit,
            total_coin,
            ..
        } => (
            "member",
            id,
            a,
            json!({"num": num, "unit": unit, "total_coin": total_coin}),
        ),
        ChatEvent::SuperChat {
            id,
            author: a,
            price,
            content,
            ..
        } => (
            "super_chat",
            id,
            a,
            json!({"price": price, "content": content}),
        ),
        ChatEvent::DeleteSuperChats { .. } => panic!("not a message"),
    };
    let mut all = author(a);
    all["kind"] = json!(kind);
    all["id"] = json!(id);
    all.as_object_mut()
        .unwrap()
        .extend(extra.as_object().unwrap().clone());
    all
}

/// The frames the 彗星号 page sends (integrations/comet/fixtures.json, the
/// same file its converter test checks) read back as the events it meant.
#[test]
fn frames_from_the_comet_page_read_as_their_events() {
    let fixtures: Value =
        serde_json::from_str(include_str!("../../../integrations/comet/fixtures.json")).unwrap();
    let mut checked = 0;
    for fixture in fixtures.as_array().unwrap() {
        if fixture["blivechat"].is_null() {
            continue;
        }
        let got = summary(&event(fixture["blivechat"].clone()));
        let name = fixture["name"].as_str().unwrap();
        for (key, want) in fixture["expect"].as_object().unwrap() {
            assert_eq!(&got[key], want, "{name}: {key}");
        }
        // Gifts, memberships and Super Chats never name the owner or a moderator.
        for flag in ["owner", "admin"] {
            if fixture["expect"].get(flag).is_none() {
                assert_eq!(got[flag], false, "{name}: {flag}");
            }
        }
        checked += 1;
    }
    assert!(checked >= 8, "every fixture with a frame was read");
}
