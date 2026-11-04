//! A stand-in 彗星号 client for trying the 彗星号 source without one.
//!
//! `cargo run -p nanatype-blive --example mock_comet [port]`
//!
//! It serves `ws://127.0.0.1:<port>/` (default 19400) as the 彗星号 client
//! does: Bilibili and Douyin comments, an emoticon, a gift, a membership and
//! a Super Chat in a loop, each encrypted as a binary frame, and `pong` for
//! every `ping`.

use std::net::{TcpListener, TcpStream};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use nanatype_blive::comet;
use serde_json::{Value, json};
use tungstenite::{Message, WebSocket};

const FACE: &str = "https://i0.hdslb.com/bfs/face/member/noface.jpg";
const EMOTE: &str = "https://i0.hdslb.com/bfs/live/a98e35996545509188fe4d24bd1a56518ea5af48.png";

fn message(n: u64) -> Value {
    let user = |id: &str, name: &str| {
        json!({"msgId": format!("m{n}"), "id": id, "name": name, "avatar": FACE,
            "fans_medal_name": "奈奈团", "fans_medal_level": 12})
    };
    let with = |mut base: Value, extra: Value| {
        base.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        base
    };
    let (kind, platform, data) = match n % 7 {
        0 => (
            "Chat",
            "bili",
            with(
                user("10001", "星野ほたる"),
                json!({"content": "主播晚上好！今天打什么字呀", "dm_type": 0}),
            ),
        ),
        1 => (
            "Chat",
            "dou_yin",
            with(
                user("MS4wLjAB", "抖音的猫"),
                json!({"content": "从抖音来的", "dm_type": 0}),
            ),
        ),
        2 => (
            "Chat",
            "bili",
            with(
                user("10003", "夜猫子"),
                json!({"content": "[dog]", "dm_type": 1, "emoji_img_url": EMOTE}),
            ),
        ),
        3 => (
            "Gift",
            "bili",
            with(
                user("10002", "路过的猫"),
                json!({"gid": "31036", "item": "小花花", "num": 5, "price": 0.1}),
            ),
        ),
        4 => (
            "Subscribe",
            "bili",
            with(
                user("10001", "星野ほたる"),
                json!({"action": "开通", "item": "舰长", "num": 1, "price": 138, "vip_unit": "月"}),
            ),
        ),
        5 => (
            "SuperChat",
            "bili",
            with(
                user("10004", "老板大气"),
                json!({"content": "今天也辛苦啦，早点休息", "price": 50}),
            ),
        ),
        _ => (
            "Like",
            "dou_yin",
            with(user("MS4wLjAB", "抖音的猫"), json!({"count": 10})),
        ),
    };
    json!({"type": kind, "platform": platform, "rid": "21452505", "data": data})
}

fn nonce(n: u64) -> [u8; 12] {
    let t = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u32);
    let mut nonce = [0u8; 12];
    nonce[..8].copy_from_slice(&n.to_le_bytes());
    nonce[8..].copy_from_slice(&t.to_le_bytes());
    nonce
}

fn serve(mut ws: WebSocket<TcpStream>) {
    ws.get_ref()
        .set_read_timeout(Some(Duration::from_millis(100)))
        .ok();
    let mut n = 0u64;
    let mut next = Instant::now();
    loop {
        if Instant::now() >= next {
            let frame = comet::encrypt(&message(n).to_string(), nonce(n));
            if ws.send(Message::binary(frame)).is_err() {
                return;
            }
            n += 1;
            next = Instant::now() + Duration::from_millis(1500);
        }
        match ws.read() {
            Ok(Message::Text(t)) if t.as_str() == comet::PING => {
                if ws.send(Message::text("pong")).is_err() {
                    return;
                }
            }
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
        .unwrap_or(comet::DEFAULT_PORT);
    let listener = TcpListener::bind(("127.0.0.1", port)).expect("bind");
    println!("mock 彗星号 on ws://127.0.0.1:{port}/");
    for stream in listener.incoming().flatten() {
        thread::spawn(move || match tungstenite::accept(stream) {
            Ok(ws) => serve(ws),
            Err(e) => println!("handshake: {e}"),
        });
    }
}
