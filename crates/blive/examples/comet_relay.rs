//! The 彗星号 path without the app: the relay, and a blivechat client joined
//! to it that prints what it reads.
//!
//! `cargo run -p nanatype-blive --example comet_relay [port] [token]`
//!
//! Then push chat with the plugin page, or with
//! `node scripts/comet_publish.mjs ws://127.0.0.1:<port>/comet?token=<token>`.

use std::time::Duration;

use nanatype_blive::relay::publish_url;
use nanatype_blive::{
    BliveConfig, BliveHandle, BliveUpdate, ChatEvent, RelayConfig, RelayHandle, RoomKey,
};

fn main() {
    let mut args = std::env::args().skip(1);
    let port = args.next().and_then(|p| p.parse().ok()).unwrap_or(12451);
    let token = args.next().unwrap_or_else(|| "devtoken".into());
    println!("publish to {}", publish_url(port, &token));
    let config = RelayConfig { port, token };

    let mut relay = RelayHandle::new(|s| println!("relay: {s:?}"));
    relay.set(Some(config));
    let mut client = BliveHandle::new(|e| match e.update {
        BliveUpdate::Status(s) => println!("client: {s:?}"),
        BliveUpdate::Chat(ChatEvent::Text {
            author, content, ..
        }) => println!("text  {} ({}): {content}", author.name, author.uid),
        BliveUpdate::Chat(ChatEvent::Gift {
            author,
            gift_name,
            num,
            total_coin,
            ..
        }) => println!(
            "gift  {}: {gift_name} ×{num} ({total_coin} coin)",
            author.name
        ),
        BliveUpdate::Chat(ChatEvent::SuperChat {
            author,
            price,
            content,
            ..
        }) => println!("sc    {} ¥{price}: {content}", author.name),
        BliveUpdate::Chat(ChatEvent::Member {
            author, num, unit, ..
        }) => println!("guard {}: {num}{unit}", author.name),
        BliveUpdate::Chat(other) => println!("other {other:?}"),
    });
    client.set(Some(BliveConfig {
        host: "127.0.0.1".into(),
        port,
        room: RoomKey::RoomId(1),
    }));
    loop {
        std::thread::sleep(Duration::from_secs(60));
    }
}
