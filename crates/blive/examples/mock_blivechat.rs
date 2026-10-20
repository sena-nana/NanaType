//! A stand-in blivechat server for trying the chat panel without a live room.
//!
//! `cargo run -p nanatype-blive --example mock_blivechat [port]`
//!
//! It accepts clients on `ws://127.0.0.1:<port>/api/chat` (default 12450),
//! waits for the join, then replays comments, an emoticon, a Super Chat, a
//! gift and a membership in a loop, in blivechat v1.10's format, and answers
//! heartbeats.

use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};
use tungstenite::{Message, WebSocket};

const FACE: &str = "//i0.hdslb.com/bfs/face/member/noface.jpg";
const EMOTE: &str = "//i0.hdslb.com/bfs/live/a98e35996545509188fe4d24bd1a56518ea5af48.png";

fn now_s() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn text(n: u64, uid: &str, name: &str, content: &str, guard: u8, emote: bool) -> Value {
    json!({"cmd": 2, "data": [
        FACE, now_s(), name, if guard > 0 { 1 } else { 0 }, content, guard, 0, 10, 0, 1,
        if guard > 0 { 12 } else { 0 }, format!("t{n}"), "", if emote { 1 } else { 0 },
        if emote { json!([EMOTE]) } else { json!([]) }, [], uid,
        if guard > 0 { "奈奈团" } else { "" }, 0
    ]})
}

fn script(n: u64) -> Value {
    let author = |uid: &str, name: &str, guard: u8| {
        json!({"avatarUrl": FACE, "authorName": name, "uid": uid, "privilegeType": guard,
            "medalLevel": 12, "medalName": "奈奈团", "timestamp": now_s()})
    };
    let with = |mut base: Value, extra: Value| {
        base.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        base
    };
    match n % 8 {
        0 => text(
            n,
            "10001",
            "星野ほたる",
            "主播晚上好！今天打什么字呀",
            3,
            false,
        ),
        1 => text(n, "10002", "路过的猫", "来了来了", 0, false),
        2 => text(n, "10003", "夜猫子", "[dog]", 0, true),
        3 => json!({"cmd": 5, "data": with(author("10004", "老板大气", 2),
            json!({"id": format!("s{n}"), "price": 50, "content": "今天也辛苦啦，早点休息"}))}),
        4 => json!({"cmd": 3, "data": with(author("10002", "路过的猫", 0),
            json!({"id": format!("g{n}"), "giftName": "小花花", "num": 5, "totalCoin": 500,
                "totalFreeCoin": 0, "giftId": 31036}))}),
        5 => text(
            n,
            "10005",
            "很长很长的名字的观众",
            "这个字幕效果好可爱，想学怎么做的！",
            0,
            false,
        ),
        6 => json!({"cmd": 4, "data": with(author("10001", "星野ほたる", 3),
            json!({"id": format!("m{n}"), "num": 1, "unit": "月", "total_coin": 198000}))}),
        _ => text(n, "10006", "admin", "注意文明发言哦", 0, false),
    }
}

fn serve(mut ws: WebSocket<TcpStream>) {
    // The join comes first.
    match ws.read() {
        Ok(Message::Text(t)) => println!("join: {t}"),
        other => {
            println!("no join: {other:?}");
            return;
        }
    }
    ws.get_ref()
        .set_read_timeout(Some(Duration::from_millis(100)))
        .ok();
    let mut n = 0u64;
    let mut next = Instant::now();
    let mut beat = Instant::now();
    loop {
        if Instant::now() >= next {
            if ws.send(Message::text(script(n).to_string())).is_err() {
                return;
            }
            n += 1;
            next = Instant::now() + Duration::from_millis(1500);
        }
        if beat.elapsed() >= Duration::from_secs(10) {
            if ws.send(Message::text(r#"{"cmd":0,"data":{}}"#)).is_err() {
                return;
            }
            beat = Instant::now();
        }
        match ws.read() {
            Ok(Message::Close(_)) => return,
            Ok(_) => {}
            Err(tungstenite::Error::Io(e))
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(e) => {
                println!("client gone: {e}");
                return;
            }
        }
    }
}

fn main() {
    let port: u16 = std::env::args()
        .nth(1)
        .and_then(|p| p.parse().ok())
        .unwrap_or(12450);
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("bind");
    println!("mock blivechat on ws://127.0.0.1:{port}/api/chat");
    for stream in listener.incoming().flatten() {
        thread::spawn(move || match tungstenite::accept(stream) {
            Ok(ws) => serve(ws),
            Err(e) => println!("handshake: {e}"),
        });
    }
}
