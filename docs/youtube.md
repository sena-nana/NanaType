# YouTube 直播聊天接入

## 来源与许可

核查日期：2026-10-21。

- 接收方式与 YouTube 网页版弹出式聊天（`live_chat?is_popout=1`）相同：读取页面里的 `ytcfg`（`INNERTUBE_API_KEY`、`INNERTUBE_CONTEXT`）与 `ytInitialData`，再循环调用 `youtubei/v1/live_chat/get_live_chat`。字段由观察公开网页客户端整理，用自行编写的 Rust 代码实现；没有复制 [chat-downloader](https://github.com/xenova/chat-downloader)、[pytchat](https://github.com/taizan-hokuto/pytchat) 等项目的源代码。
- 不需要 Google Cloud 项目、API Key、账号或配额，也不读取浏览器 Cookie。请求只携带拒绝非必要 Cookie 的 `SOCS=CAI`，避免欧盟地区跳转到同意页。
- 这是非官方接口：YouTube 改版时可能需要跟进。页面结构无法解析时显示“聊天页面已变化”，不会把错误页当作消息。

## 连接

在 **设置 › YouTube 直播** 填写下列任一形式，开启接收：

- 视频：`https://www.youtube.com/watch?v=…`、`https://youtu.be/…`、`https://www.youtube.com/live/…` 或 11 位视频 ID；
- 频道：`https://www.youtube.com/@handle`、`@handle`、`https://www.youtube.com/channel/UC…` 或频道 ID。

只接受 `youtube.com`、`www.youtube.com`、`m.youtube.com` 与 `youtu.be` 主机；重定向离开 `www.youtube.com`（例如登录或同意页）视为失败。

填写频道时，应用请求频道的 `/live` 地址；页面的 canonical 指向某个视频时即为正在直播（或即将开播且聊天已开放）。未开播时显示“尚未开播”，每分钟重新检查；直播结束后继续等待下一场。填写视频时，直播结束即停止。

打开聊天后选择“所有聊天”（而非“热门聊天”）。首个 reload 应答是近期历史，只用于取得下一个 continuation，不显示；之后按 YouTube 给出的 `timeoutMs`（限制在 1–5 秒）轮询。消息按 ID 去重（保留最近 4096 条），重新打开聊天也不会重复。连续三次轮询失败后重新打开聊天页；网络失败按 1、2、4…30 秒退避。

## 消息

| YouTube 项目 | 弹幕面板 |
| --- | --- |
| 文字消息 | 文字；频道自定义表情为内联图片，标准 emoji 保持为文字 |
| Super Chat | 加粗金额（YouTube 原样文本，如 `¥1,000`）+ 留言，卡片使用 YouTube 的颜色 |
| Super Sticker | 加粗金额 + 贴纸图片 |
| 会员加入／里程碑 | 事件行（YouTube 原文）+ 会员留言 |
| 赠送会员 | “赠送了 N 份会员” |
| 删除消息 | 从弹幕面板与历史记录移除 |

作者的频道 ID 以 `youtube:` 前缀命名空间化，与其他平台互不冲突。所有者、管理员标记与其他平台共用显示方式。Super Chat 与 Super Sticker 受“醒目留言”过滤控制，会员事件受“上舰／会员”过滤控制。

## 发送可行性

当前**不支持**向 YouTube 聊天发送。结论如下：

1. **官方 API（可行，但门槛高）**：`liveChatMessages.insert` 可以以用户身份发送消息。
   - 需要 OAuth 2.0 的 `https://www.googleapis.com/auth/youtube.force-ssl` 权限。这是敏感权限：面向公众发布前必须通过 Google 的 OAuth 应用验证；未验证的应用最多 100 名测试用户，并且会出现“未验证应用”警告页。
   - 需要 NanaType 自己的 Google Cloud 项目与桌面 OAuth 客户端（回环地址重定向 + PKCE）。
   - 每条消息消耗 20 配额单位；默认每天 10,000 单位，约 500 条（本项目读取走 InnerTube，不占配额）。超过需申请扩额。
2. **非官方接口（技术可行，不推荐）**：导入 Google 账号 Cookie，用 `SAPISIDHASH` 授权头调用 `youtubei/v1/live_chat/send_message`。这违反 YouTube 服务条款，需要保管价值很高的 Google 账号 Cookie，账号存在风险。
3. **建议**：若以后要支持，走官方 OAuth 路线，复用 Twitch 已有的发送队列（拆分、限速、失败阻塞队首、手动重试／跳过、回执去重）。上线前需要完成 Google 应用验证与配额规划。

## 验证

```bash
cargo test -p nanatype-youtube
```

单元测试覆盖地址白名单、页面解析、各类消息与 continuation、历史回放跳过、未开播等待和视频不存在。联网验收默认忽略，用环境变量指定正在直播的频道或视频：

```bash
NANATYPE_YOUTUBE_TARGET=@LofiGirl cargo test -p nanatype-youtube -- --ignored --nocapture
```

2026-10-21 在 macOS 上以 `@LofiGirl` 验收：解析频道直播、打开聊天并收到实时消息。
