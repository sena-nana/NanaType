[简体中文](../comet.md) · [English](comet.md) · [日本語](../ja/comet.md)

# Comet (彗星号) integration

> **Purpose:** Connect NanaType to a local [Comet (彗星号)](https://huixinghao.cn/) client and receive its live-room chat.
>
> **On this page:** [Data flow](#how-the-data-flows) · [Setup](#getting-started) · [Field mapping](#field-mapping) · [Testing](#testing)


[Comet (彗星号)](https://huixinghao.cn/) is a live-streaming assistant. The Comet client serves a chat WebSocket on this computer itself; NanaType connects to it directly and reads every room it listens to, with no plugin page and no relay.

## How the data flows

```
Comet client (listening to rooms)
       │  ws://127.0.0.1:19400/      the root path carries every room's messages
       │  binary frames are AES-256-GCM encrypted; text frames are JSON; ping → pong
       ▼
NanaType connection thread (crates/blive/src/client.rs, Join::Comet)
       comet::decrypt() decrypts, comet::parse() turns it into chat events
       ▼
Chat panel, history
```

- A binary frame is a 12-byte nonce, then the ciphertext with its 16-byte authentication tag at the end, under the fixed key Comet publishes. A frame that does not decrypt is skipped; later messages are unaffected.
- Each message is `{ type, platform, rid, data }`. NanaType sends one `ping` as soon as it connects and one every 10 seconds after; 30 seconds without any frame counts as a dropped connection, retried after 3 seconds.
- It connects to `127.0.0.1` only, on port 19400 by default, which the settings page can change.

## Getting started

1. Open the Comet client and start listening to your room.
2. In NanaType: **Settings › Third-party chat**, turn it on and choose **彗星号 (Comet)** as the source. The port only needs to match the one in Comet's settings (19400 by default).
3. Once the status reads "彗星号 · Connected", chat Comet receives shows up in the chat panel. "Cannot reach 彗星号" means the client is not open, or the port is wrong.

## Field mapping

| Comet `type` | NanaType event | Notes |
| --- | --- | --- |
| `Chat` | Comment | `content` is the text; `dm_type = 1` is an emoticon comment, its picture from `emoji_img_url` |
| `Gift` | Gift | `item` is the gift's name; `price` is taken as the unit price in yuan, `total = price × num`; a price of 0 is a free gift |
| `Subscribe` / `Guard` | Guard / membership | Bilibili guards and Douyin memberships come as `Subscribe`, 星守护 as `Guard`; an `item` naming 总督 / 提督 / 舰长 sets that guard level, `vip_unit` is the period |
| `SuperChat` | Super Chat | `price` is in yuan |
| `EnterRoom`, `Like`, `EndLive`, `ListenerStatus`, `WsClose` | — | Not chat; ignored |

The user is read from `id`, `name` and `avatar`, the fan medal from `fans_medal_name` and `fans_medal_level`; the message id is `msgId`, made from the content when missing. Users of platforms other than Bilibili get their platform before their uid (such as `dou_yin:…`), so they never clash with a Bilibili uid.

## Testing

```bash
cargo test -p nanatype-blive
```

Without a Comet client, a mock server can stand in; it sends encrypted sample messages on port 19400 in a loop:

```bash
cargo run -p nanatype-blive --example mock_comet
```


## Upgrading from earlier versions

Earlier versions had a Workshop plugin page push events to NanaType's own relay (port 12451, with a token in the address). That path is gone: a saved port of 12451 becomes 19400 on its own, the old token is no longer used, and the old plugin can be removed from the canvas.
