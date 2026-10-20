# 彗星号接入

[彗星号](https://huixinghao.cn/) 是直播辅助工具，它的创意工坊插件都是静态网页，运行在彗星号画布的 iframe 里，通过 `window.postMessage` 收到直播间事件（[开发文档](https://gitee.com/t_tgl/hxh-dev-docs)）。NanaType 用一个插件页面把这些事件转发出来，再经过 blivechat 协议接收，和连接一台 blivechat 服务器走的是同一条路。

## 数据怎么走

```
彗星号客户端 / OBS 浏览器源
  └─ 插件页面 integrations/comet/index.html
       postMessage 收到 LIVE_OPEN_PLATFORM_DM / SEND_GIFT / SUPER_CHAT / GUARD
       toBlivechat() 转换成 blivechat 帧 {cmd, data}
       │  ws://127.0.0.1:12451/comet?token=…   （页面主动连本机）
       ▼
NanaType 中转（crates/blive/src/relay.rs）
       校验 token，按消息 id 去重，转发给 /api/chat 的所有客户端
       │  ws://127.0.0.1:12451/api/chat        （标准 blivechat 端点）
       ▼
NanaType blivechat 客户端（crates/blive/src/client.rs）→ 弹幕面板、历史记录
```

- 浏览器页面只能发起连接、不能监听端口，所以中间需要一个服务端：中转对外提供 blivechat 的 `/api/chat`，其他兼容 blivechat 的工具也能连上来读同一份弹幕。
- 中转只监听 `127.0.0.1`。推送端必须带上设置页生成的 128 位 token，否则握手直接返回 401，防止本机其他网页伪造弹幕。token 可在设置页重新生成，旧地址随即失效。
- 同一画布可能同时在彗星号预览和 OBS 里加载，同一条事件会推两次；中转记住最近 1024 个消息 id，只转发一次。
- 插件在配置状态（`?env=1`）下只连接、显示状态，不转发，避免把配置时的模拟数据送进 NanaType；直播状态下页面完全透明。

## 使用步骤

1. NanaType：**设置 › 第三方弹幕**，开启接收，来源选 **彗星号**。
2. 插件页面是仓库里的 `integrations/comet/index.html`；设置页不提供插件文件，由发布者上传到创意工坊后供主播添加。
3. 登录彗星号官网进入创意工坊，作品类型选“自定插件”，上传 `index.html`，存入草稿箱。私密作品无需审核即可在自己的作品集里使用。需要配置项时，把 `integrations/comet/settings.json` 的内容填进“自定义作品配置”，它提供一个“NanaType 连接地址”输入框。
4. 把插件加到直播画布，在右侧配置里粘贴 NanaType 设置页显示的连接地址。
5. 设置页状态变为“彗星号 · 已连接”后，彗星号收到的弹幕会出现在弹幕面板。

中转端口默认 12451，设置页不显示；需要改时编辑设置文件里的 `integrations.blivechat.comet_port`。

也可以不填配置项，直接在插件地址后加 `?address=` 参数传入连接地址。

## 字段映射

| 彗星号事件 | blivechat 命令 | 说明 |
| --- | --- | --- |
| 弹幕 `message` | `2` ADD_TEXT（v1.10 数组） | `dmType = 1` 为表情弹幕，图片取 `emojiImgUrl`；主播 / 房管 / 舰队分别对应 authorType 3 / 2 / 1 |
| 礼物 `gift` | `3` ADD_GIFT | `totalCoin = 单价 × 数量 × 1000`（金瓜子，1000 = ¥1）；免费礼物计入 `totalFreeCoin`；盲盒按爆出的礼物计 |
| 醒目留言 `superChat` | `5` ADD_SUPER_CHAT | `price` 为元 |
| 大航海 / 守护 / 会员 `guard` | `4` ADD_MEMBER | `total_coin = 价格 × 1000`，`unit` 取 `guardUnit` |
| 进房、点赞、开播、下播 | — | blivechat 协议没有对应命令，忽略 |

粉丝勋章只在佩戴时带上。非 B 站平台的用户 uid 加平台前缀（如 `dou_yin:…`），避免与 B 站 uid 冲突。

## 测试

```bash
node --test integrations/comet/convert.test.mjs
cargo test -p nanatype-blive
```

`integrations/comet/fixtures.json` 同时被两边使用：Node 测试检查插件页面的转换结果，Rust 测试检查这些帧被客户端读回成预期的事件。`scripts/comet_publish.mjs` 用页面自己的转换函数把样例推给正在运行的中转，每条推两遍，可以直接看到去重效果。

## 尚未验证

彗星号客户端内嵌网页是否对连接 `ws://127.0.0.1` 有额外限制（CSP、本地网络访问提示），需要在真实的彗星号客户端里上传插件后确认。若被拦截，插件配置状态下会显示“未连接”。
