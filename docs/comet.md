[简体中文](comet.md) · [English](en/comet.md) · [日本語](ja/comet.md)

# 彗星号接入

[彗星号](https://huixinghao.cn/) 在本机提供 WebSocket 弹幕接口。NanaType 只连接 `127.0.0.1`，直接读取彗星号正在监听的直播间，不需要插件页面或中转服务。

## 使用

1. 打开彗星号并开始监听直播间。
2. NanaType 打开 **设置 › 直播平台 › Bilibili**，开启接收，来源选 **彗星号**；要接收抖音、快手等非 B 站直播间，在 **其他平台** 页开启彗星号。两处共用同一条连接，各自只接收自己平台的消息。
3. 填入与彗星号一致的端口（默认 `19400`）。状态显示“彗星号 · 已连接”后，消息会进入弹幕面板。

“未连上彗星号”通常表示客户端未运行或端口不一致。旧版端口 `12451` 会自动迁移到 `19400`；旧 token 不再使用，旧插件页面可以删除。

## 连接与安全边界

默认地址为 `ws://127.0.0.1:19400/`。二进制帧格式为 12 字节 nonce、AES-256-GCM 密文和 16 字节认证标签；文本帧是 JSON。解密失败的帧会跳过，不影响后续消息。客户端连接后发送 ping，此后每 10 秒发送一次；30 秒无任何帧视为断线，3 秒后重连。设置只允许本机地址。

消息形如 `{ type, platform, rid, data }`。`Chat` 映射弹幕，`Gift` 映射礼物（总价为 `price × num`），`Subscribe`/`Guard` 映射上舰或会员，`SuperChat` 映射醒目留言；`EnterRoom`、`Like`、`EndLive`、`ListenerStatus` 和 `WsClose` 忽略。用户资料取 `id`、`name`、`avatar`，粉丝牌取 `fans_medal_name`/`fans_medal_level`。非 B 站 uid 会加平台前缀，避免冲突。

## 开发验证

```bash
cargo test -p nanatype-blive
cargo run -p nanatype-blive --example mock_comet
```

模拟服务器在 `19400` 端口循环发送加密样例消息。已知限制：`Gift.price` 的单价语义仍需用真实礼物核对；舰队等级目前从 `item` 文本判断。
