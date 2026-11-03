[简体中文](laplace.md) · [English](en/laplace.md) · [日本語](ja/laplace.md)

# LAPLACE 接入

[LAPLACE Chat](https://chat.laplace.live/) 是 B 站弹幕工具。它可以把收到的直播间事件通过 [LAPLACE Event Bridge](https://github.com/laplace-live/event-bridge) 广播出去，NanaType 作为 Event Bridge 的客户端接收这些事件。B 站的连接由 LAPLACE 负责，NanaType 不直连 B 站。

> **只支持身份码连接。** LAPLACE Chat 或 Event Fetcher 必须用主播的身份码（B 站直播开放平台）连接直播间。用房间号直连（`connectionMode=direct`）的 LAPLACE 走的是 B 站网页的非公开接口，NanaType 不支持，即使经过 Event Bridge 转发也一样。

## 数据怎么走

```
B 站直播间
  └─ LAPLACE Chat 控制台（浏览器）或 LAPLACE Event Fetcher（服务器）
       │  以 server 身份推送事件
       ▼
LAPLACE Event Bridge（默认 ws://localhost:9696，可设 --auth 令牌）
       │  广播给所有 client
       ▼
NanaType（crates/blive/src/laplace.rs）→ 弹幕面板、历史记录
```

## 使用步骤

先在 LAPLACE 里用**身份码**连接你的直播间，然后二选一：

- **本机 Event Bridge**：从 [Event Bridge 发布页](https://github.com/laplace-live/event-bridge/releases) 下载 `leb-server` 并运行（需要令牌时加 `--auth 你的令牌`），再让 LAPLACE Chat 控制台连上这个 Event Bridge。保持控制台开着。
- **Event Fetcher**：Event Fetcher v2.2.0 起可以开启 bridge 模式（`WEBSOCKET_BRIDGE=1`），它自己连 B 站并直接提供 Event Bridge，不需要一直开着控制台。

然后在 NanaType：

1. **设置 › 直播平台 › 第三方弹幕**，开启接收，来源选 **LAPLACE**。
2. 在“Event Bridge 链接”里粘贴地址，按“保存并连接”。可以粘贴：
   - `ws://localhost:9696`，有令牌时写成 `ws://localhost:9696/?token=你的令牌`；
   - 远程 Event Fetcher 的 `wss://…` 或 `https://…` 地址；
   - 只写 `主机:端口`，不写端口时默认 9696；
   - 处于 Event Fetcher 模式的 LAPLACE Chat OBS 链接，NanaType 从中取出 `danmakuFetcherApi` 和 `eventFetcherAuth`。
3. 状态变为“LAPLACE · 已连接”后，弹幕、礼物、上舰和醒目留言会出现在弹幕面板。

令牌存进系统钥匙串，设置文件里只保存不带令牌的地址；粘贴链接里的其他参数一律丢弃。

### 为什么 OBS 链接连不上

LAPLACE Chat 的 OBS 链接（`…/obs/房间号或身份码?…`）表示这个页面自己连接直播间，链接里没有 Event Bridge 地址，只有房间号或身份码和 LAPLACE 的登录同步凭据（`loginSyncToken`），所以会被拒绝。房间号直连（`connectionMode=direct`）本身也不受支持。请用身份码连接，再按上面的步骤开启 Event Bridge 或 Event Fetcher。

OBS 链接里的 `loginSyncToken` 是你的 LAPLACE 登录凭据，不要发给别人。

## 字段映射

| LAPLACE 事件 | NanaType 事件 | 说明 |
| --- | --- | --- |
| `message` | 弹幕 | `emote.url` 为表情弹幕图片；`userType` 100 / 1 对应主播 / 房管；`guardType` 为舰队等级 |
| `gift` | 礼物 | 价值取 `priceNormalized`（元）× 1000 金瓜子；`coinType` 为 `silver` 时计为免费 |
| `toast` | 上舰 | 数量与单位取 `toastAmount` / `toastAmountUnit`，等级取 `toastType` |
| `superchat` | 醒目留言 | 以 `scId` 作为 id，价格取 `priceNormalized`（元） |
| `superchat-delete` | 撤下醒目留言 | 按 `scIds` 移除 |
| 其他（进房、点赞、`message-recall` 等） | — | 忽略 |

身份码连接时 B 站用 open_id 代替 uid，`uid` 为 0，NanaType 不记录 uid；开放平台也不告诉谁是房管，`userType` 只能认出主播（如果 LAPLACE 给出）。

## 连接细节

- Go 版 Event Bridge 每 54 秒发 WebSocket ping；NanaType 每 20 秒发一次 ping，60 秒收不到任何数据就重连。旧的 Bun 版发 JSON `{"type":"ping"}`，NanaType 回复 `pong`。
- 令牌错误时 Event Bridge 握手返回 401，状态显示“令牌不正确”，每 30 秒重试一次；其他断线 3 秒后重连。
- `wss://` 通过 rustls 连接，使用内置的根证书。

## 测试

```bash
cargo test -p nanatype-blive
```
