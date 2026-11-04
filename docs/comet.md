[简体中文](comet.md) · [English](en/comet.md) · [日本語](ja/comet.md)

# 彗星号接入

[彗星号](https://huixinghao.cn/) 是直播辅助工具。彗星号客户端自己在本机提供一个 WebSocket 弹幕接口，NanaType 直接连上去读取它监听的所有直播间，不需要插件页面，也不需要中转。

## 数据怎么走

```
彗星号客户端（正在监听直播间）
       │  ws://127.0.0.1:19400/      根路径收到全部房间的消息
       │  二进制帧 AES-256-GCM 加密；文本帧是 JSON；ping → pong
       ▼
NanaType 连接线程（crates/blive/src/client.rs，Join::Comet）
       comet::decrypt() 解密，comet::parse() 转成弹幕事件
       ▼
弹幕面板、历史记录
```

- 二进制帧的前 12 字节是 nonce，后面是密文和末尾 16 字节认证标签，密钥是彗星号公开的固定密钥。解不开的帧直接跳过，不影响后续消息。
- 每条消息形如 `{ type, platform, rid, data }`。NanaType 连上后先发一次 `ping`，之后每 10 秒一次；30 秒没收到任何帧视为断线，3 秒后重连。
- 只连 `127.0.0.1`，端口默认 19400，可在设置页修改。

## 使用步骤

1. 打开彗星号客户端，开始监听直播间。
2. NanaType：**设置 › 第三方弹幕**，开启接收，来源选 **彗星号**。端口与彗星号设置里的一致即可（默认 19400）。
3. 状态变为“彗星号 · 已连接”后，彗星号收到的弹幕会出现在弹幕面板。显示“未连上彗星号”说明客户端没开，或端口不对。

## 字段映射

| 彗星号 `type` | NanaType 事件 | 说明 |
| --- | --- | --- |
| `Chat` | 弹幕 | `content` 为内容；`dm_type = 1` 为表情弹幕，图片取 `emoji_img_url` |
| `Gift` | 礼物 | `item` 为礼物名；`price` 按单价（元）计，`总价 = price × num`；价格为 0 视为免费礼物 |
| `Subscribe` / `Guard` | 上舰 / 会员 | B 站大航海和抖音会员走 `Subscribe`，星守护走 `Guard`；`item` 含“总督 / 提督 / 舰长”时记为对应舰队等级，`vip_unit` 为周期 |
| `SuperChat` | 醒目留言 | `price` 为元 |
| `EnterRoom`、`Like`、`EndLive`、`ListenerStatus`、`WsClose` | — | 不是弹幕，忽略 |

用户资料取 `id`、`name`、`avatar`，粉丝勋章取 `fans_medal_name`、`fans_medal_level`；消息 id 取 `msgId`，缺失时由内容拼一个。非 B 站平台的用户 uid 加平台前缀（如 `dou_yin:…`），避免与 B 站 uid 冲突。

## 测试

```bash
cargo test -p nanatype-blive
```

没有彗星号客户端时，可以用模拟服务器代替，它在 19400 端口循环发送加密的样例消息：

```bash
cargo run -p nanatype-blive --example mock_comet
```

## 尚未验证

- `Gift` 的 `price` 是单价还是总价、单位是否为元，目前按单价（元）处理，需要用真实礼物核对。
- 舰队等级目前只从 `item` 的文字判断；彗星号若在 `data` 里另给等级字段，可再补上。

## 旧版本升级

此前的版本通过创意工坊插件页面把事件推给 NanaType 的中转（端口 12451、带 token 的连接地址）。这条路已经移除：设置里保存的旧端口 12451 会自动改为 19400，旧 token 不再使用，画布里的旧插件可以删掉。
