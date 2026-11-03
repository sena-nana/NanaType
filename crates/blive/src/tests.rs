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
        join: Join::Blivechat(RoomKey::AuthCode("CODE".into())),
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

fn laplace_event(frame: Value) -> ChatEvent {
    match laplace::parse(&frame.to_string()) {
        Some(Incoming::Event(e)) => e,
        other => panic!("not an event: {other:?}"),
    }
}

#[test]
fn laplace_messages_read_their_author_and_emote() {
    let e = laplace_event(json!({
        "type": "message", "id": "m-1", "origin": 21452505, "uid": 42, "username": "观众",
        "avatar": "//i0.hdslb.com/bfs/face/a.jpg", "message": "晚上好", "userType": 100,
        "guardType": 3, "medal": {"level": 12, "name": "奈奈团", "room": 1},
        "timestamp": 1_700_000_000, "timestampNormalized": 1_700_000_000_123u64,
        "emote": {"url": "http://i0.hdslb.com/e.png", "emoticon_unique": "x"}
    }));
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
    assert_eq!((id.as_str(), content.as_str()), ("m-1", "晚上好"));
    assert_eq!(emoticon.as_deref(), Some("https://i0.hdslb.com/e.png"));
    assert_eq!(at_ms, 1_700_000_000_123);
    assert_eq!(author.uid, "42");
    assert_eq!(author.avatar_url, "https://i0.hdslb.com/bfs/face/a.jpg");
    assert_eq!(author.guard, Guard::Captain);
    assert!(author.owner && !author.admin);
    assert_eq!(
        (author.medal_level, author.medal_name.as_str()),
        (12, "奈奈团")
    );

    // The open platform: no uid, no emote, seconds only.
    let e = laplace_event(
        json!({"type": "message", "id": "m-2", "uid": 0, "username": "a",
        "message": "hi", "userType": 1, "timestamp": 1_700_000_000, "reply": {}}),
    );
    let ChatEvent::Text {
        author,
        emoticon,
        at_ms,
        ..
    } = e
    else {
        panic!("{e:?}");
    };
    assert_eq!(author.uid, "");
    assert!(author.admin);
    assert_eq!(emoticon, None);
    assert_eq!(at_ms, 1_700_000_000_000);
}

#[test]
fn laplace_gifts_guards_and_super_chats_read_their_prices() {
    let gift = |coin_type: &str| {
        laplace_event(json!({"type": "gift", "id": "g", "uid": 1, "username": "a",
            "giftName": "小花花", "giftAmount": 5, "price": 500, "priceNormalized": 0.5,
            "coinType": coin_type, "guardType": 2, "timestamp": 1}))
    };
    let ChatEvent::Gift {
        author,
        gift_name,
        num,
        total_coin,
        ..
    } = gift("gold")
    else {
        panic!()
    };
    assert_eq!((gift_name.as_str(), num, total_coin), ("小花花", 5, 500));
    assert_eq!(author.guard, Guard::Admiral);
    let ChatEvent::Gift { total_coin, .. } = gift("silver") else {
        panic!()
    };
    assert_eq!(total_coin, 0, "silver gifts are free");

    let ChatEvent::Member {
        author,
        num,
        unit,
        total_coin,
        ..
    } = laplace_event(
        json!({"type": "toast", "id": "t", "uid": 1, "username": "a",
        "toastType": 3, "toastAmount": 1, "toastAmountUnit": "月", "price": 138000,
        "priceNormalized": 138}),
    )
    else {
        panic!()
    };
    assert_eq!(author.guard, Guard::Captain);
    assert_eq!((num, unit.as_str(), total_coin), (1, "月", 138_000));

    let ChatEvent::SuperChat {
        id, price, content, ..
    } = laplace_event(json!({
        "type": "superchat", "id": "1700000000:1:1", "scId": 9876, "uid": 1, "username": "a",
        "message": "加油", "price": 30, "priceNormalized": 30}))
    else {
        panic!()
    };
    assert_eq!((id.as_str(), price, content.as_str()), ("9876", 30, "加油"));
    assert_eq!(
        laplace_event(json!({"type": "superchat-delete", "id": "d", "scIds": [9876, 1]})),
        ChatEvent::DeleteSuperChats {
            ids: vec!["9876".into(), "1".into()]
        },
        "deletions name Super Chats by the id they were read with"
    );
}

#[test]
fn laplace_frames_it_does_not_use_are_ignored() {
    assert_eq!(
        laplace::parse(r#"{"type":"established","clientId":"client-1","version":"4.1.0"}"#),
        Some(Incoming::Other)
    );
    assert_eq!(
        laplace::parse(r#"{"type":"like-click","uid":1}"#),
        Some(Incoming::Other)
    );
    assert_eq!(
        laplace::parse(r#"{"type":"ping","timestamp":5}"#),
        Some(Incoming::Heartbeat)
    );
    assert_eq!(laplace::parse("[1]"), None);
    assert_eq!(laplace::parse("not json"), None);
}

#[test]
// tungstenite's handshake callback returns its own large error type.
#[allow(clippy::result_large_err)]
fn the_client_reads_a_laplace_bridge_and_answers_its_pings() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let server = std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let path = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
        let seen = path.clone();
        let mut ws = tungstenite::accept_hdr(
            stream,
            move |req: &tungstenite::handshake::server::Request, res| {
                *seen.lock().unwrap() = req.uri().to_string();
                Ok(res)
            },
        )
        .unwrap();
        ws.send(Message::text(
            json!({"type": "established", "clientId": "client-1", "version": "4.1.0"}).to_string(),
        ))
        .unwrap();
        ws.send(Message::text(r#"{"type":"ping","timestamp":7}"#))
            .unwrap();
        let pong = ws.read().unwrap().into_text().unwrap().to_string();
        ws.send(Message::text(
            json!({"type": "message", "id": "m", "uid": 5, "username": "观众", "message": "你好"})
                .to_string(),
        ))
        .unwrap();
        let _ = ws.read();
        let path = path.lock().unwrap().clone();
        (path, pong)
    });
    let (event_tx, event_rx) = mpsc::channel();
    let mut handle = BliveHandle::new(move |e: BliveEvent| {
        if let BliveUpdate::Chat(c) = e.update {
            let _ = event_tx.send(c);
        }
    });
    handle.set(Some(BliveConfig {
        host: "127.0.0.1".into(),
        port,
        join: Join::Laplace {
            url: laplace::Link::parse(&format!("127.0.0.1:{port}/?token=secret"))
                .unwrap()
                .url()
                .unwrap(),
        },
    }));
    match event_rx.recv_timeout(Duration::from_secs(5)).unwrap() {
        ChatEvent::Text {
            content, author, ..
        } => {
            assert_eq!(content, "你好");
            assert_eq!(author.uid, "5");
        }
        e => panic!("{e:?}"),
    }
    handle.set(None);
    let (path, pong) = server.join().unwrap();
    assert_eq!(path, "/?token=secret");
    let pong: Value = serde_json::from_str(&pong).unwrap();
    assert_eq!(
        (pong["type"].as_str(), &pong["respondingTo"]),
        (Some("pong"), &json!(7))
    );
}

#[test]
// tungstenite's handshake callback returns its own large error type.
#[allow(clippy::result_large_err)]
fn a_bridge_that_turns_the_token_down_reports_it() {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        let (stream, _) = listener.accept().unwrap();
        let _ = tungstenite::accept_hdr(stream, |_: &_, _| {
            let mut refused = tungstenite::handshake::server::ErrorResponse::new(None);
            *refused.status_mut() = tungstenite::http::StatusCode::UNAUTHORIZED;
            Err(refused)
        });
    });
    let (status_tx, status_rx) = mpsc::channel();
    let mut handle = BliveHandle::new(move |e: BliveEvent| {
        if let BliveUpdate::Status(s) = e.update {
            let _ = status_tx.send(s);
        }
    });
    handle.set(Some(BliveConfig {
        host: "127.0.0.1".into(),
        port,
        join: Join::Laplace {
            url: format!("ws://127.0.0.1:{port}/?token=wrong"),
        },
    }));
    assert_eq!(status_rx.recv().unwrap(), BliveStatus::Connecting);
    assert_eq!(
        status_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        BliveStatus::Error(nanatype_model::LinkFault::Unauthorized)
    );
}

#[test]
fn laplace_links_read_as_pasted() {
    let link = |text: &str| laplace::Link::parse(text).map(|l| (l.address, l.token));
    let pair = |a: &str, t: &str| Ok((a.to_owned(), t.to_owned()));
    assert_eq!(
        link("ws://localhost:9696"),
        pair("ws://localhost:9696/", "")
    );
    assert_eq!(
        link(" wss://event-fetcher.laplace.cn/?token=laplace "),
        pair("wss://event-fetcher.laplace.cn/", "laplace")
    );
    assert_eq!(
        link("https://example.com/bridge?a=1&token=x%20y#top"),
        pair("wss://example.com/bridge", "x y"),
        "only the token is kept from the query"
    );
    assert_eq!(
        link("http://10.0.0.2:9000"),
        pair("ws://10.0.0.2:9000/", "")
    );
    // A bare address is a local bridge on its usual port.
    assert_eq!(link("localhost"), pair("ws://localhost:9696/", ""));
    assert_eq!(link("192.168.1.5:9697"), pair("ws://192.168.1.5:9697/", ""));
    for bad in ["", "   ", "ftp://host", "ws://", "not a link at all"] {
        assert_eq!(link(bad), Err(laplace::LinkError::Invalid), "{bad:?}");
    }
    // A LAPLACE Chat OBS link reading Bilibili itself names no bridge.
    assert_eq!(
        link(
            "https://chat.laplace.live/obs/5146751?connectionMode=direct&loginSyncToken=secret&showGift=true"
        ),
        Err(laplace::LinkError::DirectPage)
    );
    // One set to an event fetcher reads as that fetcher, and nothing else of
    // the page's query (its login token among it) is kept.
    assert_eq!(
        link(
            "https://chat.laplace.live/obs/5146751?connectionMode=fetcher\
             &danmakuFetcherApi=https%3A%2F%2Fevent-fetcher.example.com&eventFetcherAuth=k3y\
             &loginSyncToken=secret"
        ),
        pair("wss://event-fetcher.example.com/", "k3y")
    );
    let l = laplace::Link {
        address: "wss://example.com/".into(),
        token: "a b&c".into(),
    };
    assert_eq!(l.url().unwrap(), "wss://example.com/?token=a+b%26c");
    let l = laplace::Link::parse("ws://h:1").unwrap();
    assert_eq!(l.url().unwrap(), "ws://h:1/", "no token, no query");
}
