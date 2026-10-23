# 日志

NanaType 与 NanaType Editor 只有一条日志管道：NanaUI 的 `nana-diagnostics`。它负责收集、缓冲、落盘、轮转、崩溃快照和导出；NanaType 在 `crates/diag`（`nanatype-diag`）里决定记录什么、保留多少、哪些内容绝不能写。

```text
任意线程 ──event!/report!/metric!──▶ 线程本地环形缓冲（满了就丢，不阻塞）
                                          │
                              nana-diagnostics 后台线程
                               ├─ 飞行记录器：最近 60 秒，内存中
                               └─ 批量写入 ──▶ 会话日志 .nlog
依赖库的 log / tracing ──bridge──▶ external.* 事件（限速、脱敏）
panic ──NanaUI 的 panic hook──▶ Fatal 事件 + 崩溃快照
```

生产者路径不做文件 I/O、不加锁、不等待；没有安装运行时（如插件 worker 进程）时每个调用点只是一次原子读取。

## 文件位置

| 平台 | 会话日志 | 崩溃快照 |
| --- | --- | --- |
| Windows | `%LOCALAPPDATA%\dev.sena-nana.nanatype\Logs` | `…\Crash` |
| macOS | `~/Library/Logs/dev.sena-nana.nanatype` | `…/Crash` |
| Linux | `$XDG_STATE_HOME/dev.sena-nana.nanatype/logs` | `…/crash` |
| 便携版 | `<程序目录>/data/logs` | `<程序目录>/data/crash` |

编辑器使用自己的应用 ID（`dev.sena-nana.nanatype-editor`）。单个文件超过 16 MiB 轮转；日志目录合计 128 MiB、32 个文件、14 天为上限，崩溃快照最多 16 个。「设置 › 关于 › 日志文件夹」直接打开该目录。

`.nlog` 是二进制格式，用 NanaUI 的 `nana-nlog` 查看：

```bash
cargo install --git https://github.com/sena-nana/NanaUI nana-diagnostics --bin nana-nlog
```

```bash
nana-nlog text <文件.nlog> --redact-home
```

## 级别与开关

| 情形 | 记录（飞行记录器） | 落盘 |
| --- | --- | --- |
| 默认（发布版） | Debug 及以上 | Warn 及以上、fault、marker、指标快照 |
| 「设置 › 关于 › 详细日志」 | Debug 及以上 | 全部 |
| `NANATYPE_DEBUG=1` | Trace 及以上 | 全部 |
| 调试构建 | Debug 及以上 | 全部 |

运行时总是记录 Debug，所以崩溃快照里有最近一分钟的调试上下文；开关只决定写进会话日志的量。设置在窗口打开前用 `nanatype_settings::peek_diagnostics` 只读地读取一次（不迁移、不隔离损坏文件），修改后重启生效。编辑器沿用 NanaType 设置里的同一个开关。

依赖库通过 `log`（以及 tracing 的 `log` 特性）输出，默认只接 Warn 及以上，详细日志时接 Info；`hyper`、`reqwest`、`ureq`、`rustls`、`h2`、`tungstenite` 只接 Error，因为它们更低级别的日志里有请求细节。每个 crate 每秒最多一条，被限掉的条数记在下一条的 `suppressed` 字段里。

## 隐私规则

日志默认就要能随问题反馈发出去，所以从源头就不写敏感内容：

- **绝不记录**：Cookie（`SESSDATA`、`bili_jct` 等）、令牌、授权码、密码、代理凭据；聊天消息正文、用户名、头像；用户输入的台词；房间号、频道名、视频 ID；音频设备名。
- **可以记录**：状态码、错误码、计数、耗时、字节数、窗口或来源的编号、布尔开关、皮肤 ID、插件 ID。
- 事件字段只能是数字或布尔（每个事件最多 4 个），天然不会带出字符串。
- 自由文本只能出现在 fault 消息里，而且必须经过 `nanatype_diag::report!`。它会调用 `redact::text`：URL 只保留协议、主机和路径；`token=`、`Cookie:`、`Authorization: Bearer` 一类的值换成 `***`；主目录换成 `~`；截断到 512 字节。脱敏是兜底，不是许可：不要把可能含个人信息的字符串格式化进消息。
- 用户看到的每条错误提示（`App::notice(…, true)`）都会以 `app.error_notice` 记录，因此提示文案同样不能拼接聊天内容或账号。

## 事件目录

每个 crate 在自己的 `diag` 模块里用 `static EventDescriptor` 声明事件，并列进 `EVENTS`；`app/tests/diag_catalog.rs` 检查编号、名字唯一，且名字以所在域的前缀开头。域在 `crates/diag/src/domains.rs` 统一分配：

| 域 | 前缀 | 内容 |
| --- | --- | --- |
| `0x0100` | `app.` | 生命周期、设置加载、窗口、错误提示；编号 100 起为 `nanatype-frame` 共用部分 |
| `0x0101` | `settings.` | 预留 |
| `0x0102` | `output.` | Spout/Syphon、窗口输出状态、VTS/OSC/WebSocket 桥接 |
| `0x0103` | `chat.` | 聊天图片等共用部分 |
| `0x0104`–`0x0107` | `bilibili.` `douyin.` `youtube.` `twitch.` | 连接（1）、连接错误（2）、账号（3）、账号失败（4）、警告（5）、发送失败（6）、收到的聊天事件计数（指标 50）；编号 100 起为各 crate 自己的重试与失败 |
| `0x0108` | `p2p.` | 端点、中继上线、资源传输 |
| `0x0109` | `plugins.` | worker 就绪耗时、失败（附 stderr 尾部）、脚本错误 |
| `0x010A` | `plugin_log.` | 插件用 `nana.log` 写的行 |
| `0x010B` | `update.` | 每个更新源的检查结果与耗时、下载、版本号解析失败 |
| `0x010C` `0x010D` `0x010F` | `steam.` `stickers.` `audio.` | Steam 不可用的原因、贴纸预览与上传失败、音频设备打开与丢失 |
| `0x010E` | `skin.` | 预留；皮肤加载和着色器错误目前以 `app.error_notice` 记录 |
| `0x0110` | `editor.` | 编辑器 |
| `0x01F0` | `external.` | 依赖库的日志 |

连接和账号状态统一编码：连接 `0` 关闭、`1` 连接中、`2` 已连接、`3` 断开或直播结束、`4` 错误、`5` 需要登录；账号 `0` 恢复中、`1` 未登录、`2` 等待扫码或校验、`3` 就绪、`4` 过期、`5` 失败。

### 新增一个事件

```rust
use nanatype_diag::domains::P2P;
use nanatype_diag::{EventDescriptor, FieldDescriptor, Severity};

pub static RELAY_SWITCHED: EventDescriptor = EventDescriptor::new(
    P2P, 5, "p2p.relay_switched", Severity::Info, &[FieldDescriptor::u64("ms")],
);
```

```rust
nanatype_diag::event!(crate::diag::RELAY_SWITCHED, ms = took.as_millis() as u64);
nanatype_diag::report!(crate::diag::ONLINE_TIMEOUT; "{error}");
```

然后把它加进本 crate 的 `EVENTS`。高频路径用 `metric!` 计数，或用 `Throttle` 限速；里程碑用 `nanatype_diag::marker`（只写固定文本）。会话开头的 `edition`、`features`、`os`、`locale`、`skin`、`verbose` 由 `set_session_info` 写入。

## 插件

插件 worker 是独立进程，不安装诊断运行时。宿主记录 worker 的就绪耗时、超时与协议错误；worker 异常结束时附上它 stderr 的最后几行（Windows AppContainer worker 没有 stderr）。插件脚本用 `nana.log(level, message)` 写的行经协议回传，由宿主以 `[插件 ID] 消息` 记录，规则见[插件文档](plugins.md#日志)。

## 诊断包

「设置 › 关于 › 诊断包 › 导出」在后台：

1. 写一份当前飞行记录器的快照，并把待写内容刷到磁盘；
2. 把最近 5 个会话和 5 个快照导出为文本（`.txt`）和 JSON Lines（`.jsonl`），主目录替换为 `~`，不含原始 `.nlog`；
3. 加上 `manifest.json` 和 `summary.json`（版本、构建号、发行版、系统、语言、皮肤 ID、启用了哪些平台和输出（只有布尔值）、插件数量、详细日志开关）；
4. 打包为日志目录下的 `dev.sena-nana.nanatype-diagnostics-<时间>.zip` 并打开该目录。

包只保存在本机，是否附到问题反馈由用户决定。上次运行崩溃（存在比 `diagnostics.crash_seen` 更新的 panic 快照）时，下次启动会提示一次。
