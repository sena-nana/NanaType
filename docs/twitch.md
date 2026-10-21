# Twitch 聊天接入

## 来源与许可

核查日期：2026-10-21。全部按 Twitch 官方开发者文档自行实现，未移植第三方代码：

- 接收：[Twitch IRC over WebSocket](https://dev.twitch.tv/docs/chat/irc/)（`wss://irc-ws.chat.twitch.tv:443`，IRCv3 标签与 `twitch.tv/tags`、`twitch.tv/commands` 能力）。
- 登录：[Device Code Grant Flow](https://dev.twitch.tv/docs/authentication/getting-tokens-oauth/#device-code-grant-flow)（公开客户端，无 client secret）。
- 发送：Helix [Send Chat Message](https://dev.twitch.tv/docs/api/reference/#send-chat-message)（`POST /helix/chat/messages`，`user:write:chat` 权限）。

## 接收

在 **设置 › Twitch** 填写频道名或 `https://www.twitch.tv/频道名`，开启接收。只接受 `twitch.tv`、`www.twitch.tv`、`m.twitch.tv` 主机和 3–25 位的 `[a-z0-9_]` 登录名。

读取聊天始终用匿名昵称 `justinfanNNNNN` 加入频道，不需要账号；登录与否不影响接收。收到 `ROOMSTATE` 后才显示已连接并开始显示消息，`room-id` 即发送时使用的频道 ID；15 秒内没有 `ROOMSTATE` 视为频道不可用。应用每 60 秒发送 PING，90 秒内没有任何数据就重连；收到 `RECONNECT` 立即重连；失败按 1、2、4…30 秒退避。

| IRC 消息 | 弹幕面板 |
| --- | --- |
| `PRIVMSG` | 文字；`emotes` 标签按 Unicode 字符区间切出表情图片，`/me` 去掉 ACTION 包装 |
| `PRIVMSG` + `bits` | 加粗“N Bits”+ 留言，Twitch 紫色卡片 |
| `USERNOTICE`（订阅、续订、赠订、raid、公告等） | Twitch 的 `system-msg` 原文 + 观众留言 |
| `CLEARMSG` | 从弹幕面板与历史记录移除该消息 |

作者 ID 以 `twitch:` 前缀命名空间化。主播与管理员标记与其他平台共用显示方式。IRC 不提供头像，Twitch 消息不显示观众头像。Bits 受“醒目留言”过滤控制，订阅类事件受“上舰／会员”过滤控制。

## 登录

1. 点击 **使用 Twitch 登录**，应用向 `id.twitch.tv/oauth2/device` 申请设备码，只请求 `user:write:chat` 权限。
2. 页面显示用户码，点击 **打开 twitch.tv/activate**（只允许打开 `https://www.twitch.tv/activate`），在浏览器登录并输入代码。
3. 应用按 Twitch 给出的间隔轮询令牌（收到 `slow_down` 时加长 5 秒），设备码过期后提示重新开始。
4. 取得令牌后调用 `oauth2/validate` 校验用户与权限，再用 Helix `users` 读取昵称和头像。

令牌对以 JSON 保存在系统保护存储（Windows DPAPI / macOS 钥匙串，服务名 `dev.sena-nana.nanatype.twitch`），不写入设置文件或日志；其他平台拒绝保存。公开客户端的 refresh token 只能使用一次，所以每次刷新后立即保存新令牌。启动时和每小时校验一次令牌；收到 401 时刷新一次，刷新被拒绝即清除凭据并显示“登录已过期”。

## 发送

登录且频道已连接后，在 **设置 › Twitch › 发送到聊天** 打开开关，确认弹窗后，之后发送的每句台词都会以你的身份发到当前频道。这与 B 站同步相互独立；两者都开时，一句台词会同时发到两个平台。同步期间不能插入图片（与 B 站相同的限制）。

- 台词去掉表演标记后转为纯文本，按 500 字符拆分，优先在标点处断开。
- 每 1.6 秒最多发送一条，低于普通用户 30 秒 20 条的限额。发送队列最多 100 句。
- Twitch 接收但未发出（`is_sent=false`，如 AutoMod 拦截、重复消息）、被拒绝（4xx）时标为失败；网络中断时标为“不确定”。两种情况都会阻塞队首，不自动重试，由用户选择重试、跳过或复制剩余内容。
- 401 时刷新令牌并重发一次；Twitch 在处理前就拒绝了请求，所以不会重复发送。
- 返回的 `message_id` 作为回执：自己发的消息经 IRC 回显时不会再次进入历史记录。

## 应用注册

内置 Client ID 是在 [dev.twitch.tv/console/apps](https://dev.twitch.tv/console/apps) 注册的公开客户端：

- **Client Type**：Public（桌面应用无法保管 secret，公开客户端也能刷新令牌）；
- **OAuth Redirect URLs**：`http://localhost`（设备码流程不使用回调，但控制台要求填写）；
- **Category**：Application Integration。

公开客户端 ID 不是机密。构建时可用 `NANATYPE_TWITCH_CLIENT_ID` 环境变量替换成自己的应用。

## 验证

```bash
cargo test -p nanatype-twitch
```

单元测试覆盖频道地址白名单、IRC 标签转义、表情区间、各类消息、本地假 IRC 服务器的匿名加入与 PING/PONG、设备码轮询、令牌校验与刷新、令牌只进入受保护存储，以及发送队列的拆分、失败阻塞、重试、跳过和过期 epoch。联网验收默认忽略：

```bash
NANATYPE_TWITCH_CHANNEL=频道名 cargo test -p nanatype-twitch -- --ignored --nocapture
```

2026-10-21 在 macOS 上匿名加入公开频道并收到实时消息。发送需要用真实账号在自己的频道手动验收。
