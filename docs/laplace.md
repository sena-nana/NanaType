[简体中文](laplace.md) · [English](en/laplace.md) · [日本語](ja/laplace.md)

# LAPLACE 接入

[LAPLACE Chat](https://chat.laplace.live/) 负责连接 B 站，NanaType 作为 [LAPLACE Event Bridge](https://github.com/laplace-live/event-bridge) 客户端接收事件。**只支持身份码连接**：房间号直连（`connectionMode=direct`）使用 B 站网页的非公开接口，NanaType 不支持。

## 配置步骤

1. 在 LAPLACE Chat 或 Event Fetcher 中用主播身份码连接直播间。
2. 选择一种桥接方式：本机运行 [Event Bridge](https://github.com/laplace-live/event-bridge/releases)（需要认证时加 `--auth 令牌`），或在 Event Fetcher v2.2.0+ 开启 `WEBSOCKET_BRIDGE=1`。
3. NanaType 打开 **设置 › 直播平台 › LAPLACE**，开启接收。
4. 填入 `ws://localhost:9696`（默认端口），需要认证时使用 `ws://localhost:9696/?token=令牌`；远程服务使用 `wss://…` 或 `https://…`。保存并连接，状态显示“LAPLACE · 已连接”后即可接收弹幕、礼物、上舰和醒目留言。

OBS 链接不是 Event Bridge 地址，不能直接粘贴：它含房间号/身份码和 `loginSyncToken`，没有桥接端点。`loginSyncToken` 是登录凭据，不要分享。令牌写入系统钥匙串，设置文件只保存不带令牌的地址，其他查询参数会丢弃。

## 事件与连接

`message` 映射弹幕，`gift` 映射礼物，`toast` 映射上舰，`superchat`/`superchat-delete` 映射和撤下醒目留言；进房、点赞及 `message-recall` 忽略。身份码模式使用 open_id，`uid` 可能为 0，NanaType 不伪造 uid。

Go Event Bridge 每 54 秒发送 WebSocket ping；NanaType 每 20 秒 ping，60 秒无数据重连。令牌错误显示“令牌不正确”并每 30 秒重试，其他断线 3 秒后重连。`wss://` 使用 rustls 与内置根证书。

## 验证

```bash
cargo test -p nanatype-blive
```
