# 抖音直播接入

## 来源与许可

核查日期：2026-10-16。

- `skmcj/dycast` 没有许可证；作者在 [issue #36](https://github.com/skmcj/dycast/issues/36#issuecomment-3699081307) 中明确回复没有许可证，不采用该项目源代码。
- `qinant/dycast-desktop` 的 MIT 与 NOTICE 没有提供上游授权证明；其 `signature.js` 与 dycast 内容相同。不从该项目移植签名或解码器。
- 原生签名采用 `jwwsjlm/douyinLive` 的明确 MIT 来源，固定版本 `0e506f3cf935beede946296602af2e52d57ecea2` 下的 `internal/webcastsign/native.go`、`sign/ab_sign.go` 与 WebSocket 测试向量。代码改编为 Rust，保留作者、版本与 MIT 文本。连接参数和默认推送端点参照同版本的 `websocket_params.go`、`constants.go`。完整声明见仓库第三方声明和 `crates/douyin/LICENSE-DOUYINLIVE`。
- Protobuf 使用自行编写的最小字段定义和标准 prost 解码器，未复制上游生成代码；新字段默认忽略。
- 未捆绑提取的平台 SDK、dycast 脚本或 JavaScript 执行器。

## 连接与账号

在设置的“抖音直播”页填写 `live.douyin.com` 房间网址或房间号，点击连接。首次使用默认关闭接入。应用只使用原生 Rust 完成 HTTP、签名、TLS WebSocket、心跳、ACK、解压、解码和去重，不启动浏览器或外部采集器。

公开房间可匿名接收。账号与连接状态分别显示：收到弹幕不代表已登录。可选导入 Cookie，只有账号响应验证成功才保存到 Windows DPAPI 或 macOS Keychain；Cookie 不写入设置或日志。退出账号会停止采集并清除保存的凭据。

首次握手后必须在 15 秒内收到可解析的文字、表情或礼物，才显示连接成功。初始化失败会显示错误，可以手动重连。接收成功后，断网、重连拨号失败和 ACK 写入失败均进入退避重试，最大间隔 30 秒；长时间无有效数据时重新连接。服务端心跳字段按秒处理。

初始化使用 SDK 1.0.15 参数；HTTP 初始批次为空时，以默认官方推送端点及空历史游标建立已签名的新连接，由服务器下发状态，不填造历史游标或时间戳。初始化响应提供推送地址时优先使用 `push_server_v2`，再使用旧字段。

会话事件携带 epoch。切换房间、账号、配置或退出立即使旧事件失效；应用和采集线程都拒绝过期事件。重连保留有界消息去重集合；礼物按用户、收礼人、礼物和连击组计算数量增量。

房间、消息、用户与图片缓存按平台隔离。抖音礼物保留抖币单位；支持与 B 站同时接收、历史、@ 和引用，当前不支持发送抖音弹幕。

## 旧设置兼容

旧设置中的 `backend`（包括 `auto`、`browser`、`native`）和 `browser` 路径字段会被忽略，房间号、启用状态及其他平台设置保持原意；下次保存时不再写入这两个废弃字段。旧浏览器会话目录不再读取或使用。浏览器采集代码、网页登录入口、CDP 与自动后备已经移除。

## 验证

2026-10-16 在 Windows 11 公开直播间完成原生 Rust 验收，全程匿名接收，不启动浏览器或外部采集器：

| 项目 | 结果 |
| --- | --- |
| 运行与接收 | 188 秒，161 条解码消息 |
| 数据通道离线 | 20 秒，期间阻断 4 次重连拨号 |
| 恢复后首条消息 | 约 17 秒（含退避与初始化） |
| 恢复后持续接收 | 120 秒，110 条消息 |
| 原生连接 / 非礼物重复消息 | 2 次 / 0 |

采集线程、解码器和去重状态在故障期间保留，不手动刷新或重启。

默认测试不依赖网络或账号：

```powershell
cargo test -p nanatype-douyin
```

独立运行原生联网验收（默认及最低恢复后接收时长为 120 秒）：

```powershell
$env:NANATYPE_DOUYIN_ROOM = '待验收的正在直播房间号'
$env:NANATYPE_DOUYIN_NATIVE_SECONDS = '120'
cargo test -p nanatype-douyin --lib native_live_network_acceptance -j 2 -- --ignored --nocapture
```

测试只在本机回环转发真实 TLS 字节，TLS 仍由 Rust 客户端与官方服务器端到端验证；不解密、不伪造消息。首次接收后第 30 秒切断数据通道并阻止新拨号，20 秒后恢复；采集器须自行重新连接，并持续接收至少 2 分钟。HTTP 房间查询不受该故障注入影响，系统网络保持正常。日志只记录计数和稳定诊断码，不包含 Cookie、消息内容或完整签名地址。

本次已验证 Windows 匿名只读采集；账号登录与 macOS 仍需独立验证。
