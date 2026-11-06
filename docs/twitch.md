# Twitch 集成

Twitch 连接通过本机 Event Bridge 将直播事件交给 NanaType；应用不会把频道令牌交给皮肤或插件。连接、令牌和事件处理都在本机完成，详细安全边界见[Laplace Event Bridge](laplace.md)。

## 配置步骤

1. 在 Twitch 开发者控制台创建应用，登记回调地址和所需 scopes；只申请功能必需权限。
2. 在 NanaType 设置中填写 Client ID，并通过浏览器授权；不要把 Client Secret、OAuth token 或 refresh token 写进 Markdown、包或日志。
3. 确认本机 Bridge 地址、端口和令牌，使用 `wss`/TLS 时验证证书；令牌保存于系统钥匙串。
4. 连接后用测试事件确认 follow、subscription、raid、channel point 和 chat 事件映射，再启用自动响应。

令牌失效、scope 变更或频道撤销授权时，清除钥匙串凭据并重新授权。仅监听明确配置的频道；不把聊天原文、用户身份或令牌上传到第三方。

## 事件与映射

| Twitch 事件 | NanaType 处理 |
| --- | --- |
| follow / subscription | 显示事件卡或交给声明的 data pipeline |
| raid | 显示来源频道和人数，按设置节流 |
| channel point | 仅处理已启用 reward，拒绝未知字段 |
| chat | 经过长度、频率和内容限制后进入事件队列 |

事件队列有界；重复事件应使用事件 ID 去重，超时或无效 schema 只丢弃该事件并写脱敏诊断。插件不能凭 Twitch 事件取得网络、文件或系统权限。

## 调试

```powershell
cargo test -p nanatype-laplace --locked
cargo test --workspace --all-targets --locked
```

先用本机 mock Event Bridge 测试连接、重连、过期令牌、未知事件和断电恢复，再做真实频道验证。报告问题时只提供版本、事件类型、时间、脱敏状态码和日志路径；不要提供 token、Cookie、Client Secret、完整聊天内容或观众个人资料。