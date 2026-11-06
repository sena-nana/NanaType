[简体中文](logging.md) · [English](en/logging.md) · [日本語](ja/logging.md)

# 日志与诊断包

NanaType 与 NanaType Editor 共用 NanaUI 的 `nana-diagnostics` 管道。生产者只写线程本地环形缓冲，不做文件 I/O、不等待；后台线程负责 60 秒飞行记录器、`.nlog` 批量写入、轮转、崩溃快照和导出。

## 查找与查看

| 平台 | 会话日志 | 崩溃快照 |
| --- | --- | --- |
| Windows | `%LOCALAPPDATA%\\dev.sena-nana.nanatype\\Logs` | `…\\Crash` |
| macOS | `~/Library/Logs/dev.sena-nana.nanatype` | `…/Crash` |
| Linux | `$XDG_STATE_HOME/dev.sena-nana.nanatype/logs` | `…/crash` |
| 便携版 | `<程序目录>/data/logs` | `<程序目录>/data/crash` |

编辑器使用 `dev.sena-nana.nanatype-editor`。单文件超过 16 MiB 轮转；日志目录最多 128 MiB、32 文件、14 天，崩溃快照最多 16 个。**设置 › 关于 › 日志文件夹**可直接打开目录。

`.nlog` 是二进制格式，可安装查看器并脱敏主目录：

```bash
cargo install --git https://github.com/sena-nana/NanaUI nana-diagnostics --bin nana-nlog
nana-nlog text <文件.nlog> --redact-home
```

## 级别与隐私

默认发布版飞行记录器记录 Debug 及以上，落盘 Warn、fault、marker 和指标；“设置 › 关于 › 详细日志”落盘全部；`NANATYPE_DEBUG=1` 记录并落盘 Trace；调试构建默认全部落盘。修改设置后重启生效。

日志**绝不记录** Cookie、令牌、授权码、密码、代理凭据、聊天正文、用户名头像、台词、房间/频道名、视频 ID 或音频设备名。可记录状态码、错误码、计数、耗时、字节数、编号、布尔开关、皮肤 ID 和插件 ID。事件字段只允许数字或布尔，最多 4 个；自由文本仅可经 `nanatype_diag::report!`，会脱敏 URL、token、Cookie、Authorization、主目录并截断到 512 字节。错误提示文案也不得拼接用户内容。

## 开发者约定

新增事件需声明唯一的 `EventDescriptor`、使用域前缀并加入本 crate 的 `EVENTS`；高频数据用 `metric!` 或 `Throttle`，里程碑只用固定文本 marker。插件 worker 不安装诊断运行时，由宿主记录就绪耗时、超时、协议错误和 stderr 尾部；`nana.log(level, message)` 经协议回传并按插件 ID 记录。

## 导出诊断包

**设置 › 关于 › 诊断包 › 导出**会刷盘并导出最近 5 个会话和 5 个崩溃快照为 `.txt` 与 `.jsonl`，附 `manifest.json`、`summary.json`，替换主目录为 `~`，不包含原始 `.nlog`，最后生成并打开 zip。诊断包只保存在本机，是否提交由用户决定。
