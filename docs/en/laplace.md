[简体中文](../laplace.md) · [English](laplace.md) · [日本語](../ja/laplace.md)

# LAPLACE integration

[LAPLACE Chat](https://chat.laplace.live/) is a Bilibili live chat tool. It can broadcast the live-room events it receives through the [LAPLACE Event Bridge](https://github.com/laplace-live/event-bridge), and NanaType receives them as an Event Bridge client. LAPLACE handles the connection to Bilibili; NanaType does not connect to Bilibili itself.

## How the data flows

```
Bilibili live room
  └─ LAPLACE Chat dashboard (browser) or LAPLACE Event Fetcher (server)
       │  pushes events as the server role
       ▼
LAPLACE Event Bridge (ws://localhost:9696 by default, optional --auth token)
       │  broadcasts to every client
       ▼
NanaType (crates/blive/src/laplace.rs) → chat panel, history
```

## Setting it up

Pick one:

- **A local Event Bridge**: download `leb-server` from the [Event Bridge releases](https://github.com/laplace-live/event-bridge/releases) and run it (add `--auth your-token` if you want a token), then have the LAPLACE Chat dashboard connect to it. Keep the dashboard open.
- **Event Fetcher**: from v2.2.0, Event Fetcher has a bridge mode (`WEBSOCKET_BRIDGE=1`). It connects to Bilibili itself and serves the Event Bridge, so the dashboard need not stay open.

Then in NanaType:

1. **Settings › Live platforms › Third-party chat**: turn receiving on and choose **LAPLACE** as the source.
2. Paste the address into **Event Bridge link** and press **Save and connect**. You can paste:
   - `ws://localhost:9696`, or `ws://localhost:9696/?token=your-token` with a token;
   - a remote Event Fetcher's `wss://…` or `https://…` address;
   - just `host:port`; with no port, 9696 is used;
   - a LAPLACE Chat OBS link in Event Fetcher mode, from which NanaType takes `danmakuFetcherApi` and `eventFetcherAuth`.
3. Once the status reads **LAPLACE · Connected**, danmaku, gifts, guard purchases and Super Chats appear in the chat panel.

The token goes to the system keychain and only the token-free address is kept in the settings file; everything else in a pasted link is dropped.

### Why an OBS link may not connect

An OBS link with `connectionMode=direct` (`…/obs/<room>?connectionMode=direct&…`) means the page connects to Bilibili itself. The link holds only the room number and LAPLACE's login-sync credential (`loginSyncToken`), not an address to connect to. NanaType no longer connects to Bilibili directly, so such links are refused. Use an Event Bridge or Event Fetcher as above.

The `loginSyncToken` in an OBS link is your LAPLACE login credential; do not share it.

## Field mapping

| LAPLACE event | NanaType event | Notes |
| --- | --- | --- |
| `message` | Danmaku | `emote.url` is a sticker danmaku's picture; `userType` 100 / 1 mean streamer / moderator; `guardType` is the guard level |
| `gift` | Gift | Value is `priceNormalized` (yuan) × 1000 gold coins; free when `coinType` is `silver` |
| `toast` | Guard purchase | Amount and unit from `toastAmount` / `toastAmountUnit`, level from `toastType` |
| `superchat` | Super Chat | Identified by `scId`; price from `priceNormalized` (yuan) |
| `superchat-delete` | Super Chat removed | Removed by `scIds` |
| Others (entries, likes, `message-recall`, …) | — | Ignored |

On the open platform `uid` is 0, and no uid is recorded.

## Connection details

- The Go Event Bridge sends a WebSocket ping every 54 s; NanaType pings every 20 s and reconnects after 60 s without any data. The older Bun bridge sends JSON `{"type":"ping"}`, which NanaType answers with `pong`.
- A wrong token makes the handshake return 401; the status says the token was turned down and it retries every 30 s. Other drops reconnect after 3 s.
- `wss://` connects through rustls with built-in root certificates.

## Tests

```bash
cargo test -p nanatype-blive
```
