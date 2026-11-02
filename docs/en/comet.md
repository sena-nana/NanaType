[简体中文](../comet.md) · [English](comet.md) · [日本語](../ja/comet.md)

# Comet (彗星号) integration

[彗星号 (Comet)](https://huixinghao.cn/) is a livestreaming helper. Its Workshop plugins are static web pages that run in an iframe on the Comet canvas and receive live-room events through `window.postMessage` ([developer docs](https://gitee.com/t_tgl/hxh-dev-docs), Chinese). NanaType uses a plugin page to forward those events and receives them over the blivechat protocol, the same path used when connecting to a blivechat server.

## How the data flows

```
彗星号客户端 / OBS 浏览器源
  └─ 插件页面 integrations/comet/index.html
       postMessage 收到 LIVE_OPEN_PLATFORM_DM / SEND_GIFT / SUPER_CHAT / GUARD
       toBlivechat() 转换成 blivechat 帧 {cmd, data}
       │  ws://127.0.0.1:12451/comet?token=…   （页面主动连本机）
       ▼
NanaType 中转（crates/blive/src/relay.rs）
       校验 token，按消息 id 去重，转发给 /api/chat 的所有客户端
       │  ws://127.0.0.1:12451/api/chat        （标准 blivechat 端点）
       ▼
NanaType blivechat 客户端（crates/blive/src/client.rs）→ 弹幕面板、历史记录
```

(The Comet client or an OBS browser source loads the plugin page, which receives `LIVE_OPEN_PLATFORM_DM` / `SEND_GIFT` / `SUPER_CHAT` / `GUARD` via postMessage, converts them with `toBlivechat()` into blivechat frames, and connects out to NanaType's local relay. The relay checks the token, de-duplicates by message id and forwards to every client on `/api/chat`, including NanaType's own blivechat client, which feeds the chat panel and history.)

- A browser page can only open connections, not listen on a port, so a server is needed in the middle: the relay exposes blivechat's `/api/chat`, and other blivechat-compatible tools can connect to read the same chat.
- The relay listens only on `127.0.0.1`. The pushing side must include the 128-bit token generated on the settings page, otherwise the handshake returns 401 straight away; this stops other web pages on the same machine from forging chat. You can regenerate the token on the settings page, which immediately invalidates the old address.
- The same canvas may be loaded in both the Comet preview and OBS, so each event can be pushed twice; the relay remembers the last 1024 message ids and forwards each only once.
- In configuration mode (`?env=1`) the plugin only connects and shows its status without forwarding, so mock data from configuration never reaches NanaType; in live mode the page is fully transparent.

## Setup

1. In NanaType: **Settings › Third-party chat**, turn on **Receive third-party chat** and set **Source** to **彗星号 (Comet)**.
2. The plugin page is `integrations/comet/index.html` in the repository. The settings page does not provide the plugin file; the publisher uploads it to the Workshop, and streamers add it from there.
3. Sign in on the Comet website, open the Workshop, choose the "自定插件" (custom plugin) work type, upload `index.html` and save it to drafts. A private work can be used from your own collection without review. If you need the settings field, paste the contents of `integrations/comet/settings.json` into "自定义作品配置" (custom work configuration); it provides a "NanaType 连接地址" (NanaType connection address) input.
4. Add the plugin to your stream canvas and paste the connection address shown on NanaType's settings page into its configuration panel on the right.
5. Once the settings page shows "彗星号 · Connected", chat received by Comet appears in the chat panel.

The relay port defaults to 12451 and is not shown on the settings page; to change it, edit `integrations.blivechat.comet_port` in the settings file.

You can also skip the settings field and pass the connection address by appending an `?address=` parameter to the plugin URL.

## Field mapping

| Comet event | blivechat command | Notes |
| --- | --- | --- |
| Chat `message` | `2` ADD_TEXT (v1.10 array) | `dmType = 1` is an emote message, image from `emojiImgUrl`; streamer / moderator / guard map to authorType 3 / 2 / 1 |
| Gift `gift` | `3` ADD_GIFT | `totalCoin = unit price × count × 1000` (gold melon seeds, 1000 = ¥1); free gifts count towards `totalFreeCoin`; blind boxes count as the gift they produced |
| Super Chat `superChat` | `5` ADD_SUPER_CHAT | `price` is in yuan |
| Guard / membership `guard` | `4` ADD_MEMBER | `total_coin = price × 1000`, `unit` from `guardUnit` |
| Room entry, likes, stream start, stream end | — | The blivechat protocol has no matching command; ignored |

Fan medals are included only when worn. User uids from platforms other than Bilibili get a platform prefix (such as `dou_yin:…`) so they never collide with Bilibili uids.

## Tests

```bash
node --test integrations/comet/convert.test.mjs
cargo test -p nanatype-blive
```

`integrations/comet/fixtures.json` is used by both sides: the Node test checks the plugin page's conversion output, and the Rust test checks that the client reads those frames back as the expected events. `scripts/comet_publish.mjs` uses the page's own conversion function to push samples to a running relay, sending each one twice so you can see the de-duplication directly.

## Not yet verified

Whether the Comet client's embedded browser places extra restrictions on connecting to `ws://127.0.0.1` (CSP, local network access prompts) still needs to be confirmed by uploading the plugin in a real Comet client. If it is blocked, the plugin shows "未连接" (not connected) in configuration mode.
