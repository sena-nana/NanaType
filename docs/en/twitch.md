[简体中文](../twitch.md) · [English](twitch.md) · [日本語](../ja/twitch.md)

# Twitch chat

> **Purpose:** Receive Twitch chat anonymously, sign in securely, and send NanaType lines to a Twitch channel.
>
> **On this page:** [Receive](#receiving) · [Sign in](#signing-in) · [Send](#sending) · [API and licensing](#sources-and-licensing)


## Sources and licensing

The integration follows Twitch's official developer documentation; no third-party code was ported:

- Receiving: [Twitch IRC over WebSocket](https://dev.twitch.tv/docs/chat/irc/) (`wss://irc-ws.chat.twitch.tv:443`, IRCv3 tags with the `twitch.tv/tags` and `twitch.tv/commands` capabilities).
- Sign-in: [Device Code Grant Flow](https://dev.twitch.tv/docs/authentication/getting-tokens-oauth/#device-code-grant-flow) (public client, no client secret).
- Sending: Helix [Send Chat Message](https://dev.twitch.tv/docs/api/reference/#send-chat-message) (`POST /helix/chat/messages`, `user:write:chat` scope).

## Receiving

In **Settings › Twitch**, enter a channel name or `https://www.twitch.tv/<channel>` and turn on **Receive chat**. Only the hosts `twitch.tv`, `www.twitch.tv` and `m.twitch.tv`, and login names of 3–25 `[a-z0-9_]` characters, are accepted.

Chat is always read by joining the channel with an anonymous `justinfanNNNNN` nickname, so no account is needed; signing in has no effect on receiving.

The connection is shown as connected, and messages start appearing, only after `ROOMSTATE` arrives; its `room-id` is the channel ID used for sending.

If no `ROOMSTATE` arrives within 15 seconds, the channel is treated as unavailable.

The app sends a PING every 60 seconds and reconnects if nothing at all arrives for 90 seconds; it reconnects immediately on `RECONNECT`; failures back off at 1, 2, 4… up to 30 seconds.

| IRC message | Chat panel |
| --- | --- |
| `PRIVMSG` | Text; the `emotes` tag cuts out emote images by Unicode character ranges, and `/me` has its ACTION wrapper removed |
| `PRIVMSG` + `bits` | Bold "N Bits" plus the message, on a Twitch-purple card |
| `USERNOTICE` (subs, resubs, gift subs, raids, announcements, etc.) | Twitch's `system-msg` text plus the viewer's message |
| `CLEARMSG` | Removes that message from the chat panel and the history |

Author IDs are namespaced with a `twitch:` prefix. Broadcaster and moderator badges share the same display as on other platforms. IRC does not provide avatars, so Twitch messages show no viewer avatar. Bits are controlled by the **Super Chats** filter, and subscription events by the **Memberships** filter.

## Signing in

1. Click **Sign in with Twitch**. The app requests a device code from `id.twitch.tv/oauth2/device`, asking only for the `user:write:chat` scope.
2. The page shows a user code. Click **Open twitch.tv/activate** (only `https://www.twitch.tv/activate` can be opened), sign in in the browser and enter the code.
3. The app polls for the token at the interval Twitch gives (adding 5 seconds on `slow_down`); once the device code expires, it asks you to start over.
4. After getting the token, it calls `oauth2/validate` to check the user and scopes, then reads the nickname and avatar via Helix `users`.

The token pair is stored as JSON in the system's protected storage (Windows DPAPI / macOS Keychain, service name `dev.sena-nana.nanatype.twitch`), never in the settings file or logs; on other platforms it refuses to save.

A public client's refresh token can only be used once, so the new token is saved immediately after every refresh.

The token is validated at startup and once an hour; on a 401 it refreshes once, and if the refresh is rejected the credentials are cleared and **Sign-in expired** is shown.

## Sending

Once you are signed in and the channel is connected, turn on the switch under **Settings › Twitch › Send to chat** and confirm the dialog. From then on, every line you send is posted to the current channel as you. This is independent of Bilibili sync; with both on, one line goes to both platforms. Images can't be inserted while syncing (the same restriction as Bilibili).

- Lines are stripped of performance markup and converted to plain text, then split at 500 characters, preferring to break at punctuation.
- At most one message is sent every 1.6 seconds, below the regular-user limit of 20 per 30 seconds. The send queue holds up to 100 lines.
- If Twitch accepts a message but doesn't deliver it (`is_sent=false`, e.g. blocked by AutoMod or a duplicate message) or rejects it (4xx), it is marked as failed; if the network drops, it is marked as uncertain. Both block the head of the queue with no automatic retry; you choose whether to retry, skip or copy the remaining text.
- On a 401 the token is refreshed and the message is resent once; Twitch rejected the request before processing it, so nothing is sent twice.
- The returned `message_id` serves as a receipt: your own messages echoed back over IRC don't enter the history a second time.

## App registration

The built-in Client ID is a public client registered at [dev.twitch.tv/console/apps](https://dev.twitch.tv/console/apps):

- **Client Type**: Public (a desktop app can't keep a secret, and public clients can still refresh tokens);
- **OAuth Redirect URLs**: `http://localhost` (the device code flow uses no callback, but the console requires one);
- **Category**: Application Integration.

A public client ID is not a secret. At build time you can replace it with your own app via the `NANATYPE_TWITCH_CLIENT_ID` environment variable.

## Verification

```bash
cargo test -p nanatype-twitch
```

Unit tests cover the channel URL allowlist, IRC tag escaping, emote ranges, every message type, anonymous joining and PING/PONG against a local fake IRC server, device code polling, token validation and refresh, tokens going only into protected storage, and the send queue's splitting, failure blocking, retry, skip and stale epochs. Online acceptance tests are ignored by default:

```bash
NANATYPE_TWITCH_CHANNEL=频道名 cargo test -p nanatype-twitch -- --ignored --nocapture
```

On 2026-10-21, anonymously joining a public channel on macOS received live messages. Sending has to be accepted manually with a real account in your own channel.
