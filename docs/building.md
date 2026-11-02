[简体中文](building.md) · [English](en/building.md) · [日本語](ja/building.md)

# 从源码构建

构建、测试、打包与开发辅助选项。功能概览见 [README](../README.md)。

## 环境

需要 Rust 1.98+ 和 Windows 10 SDK（图标与版本资源）。NanaUI 与 nanalive-spout（Spout2 发送端）以固定 rev 通过 GitHub 远程仓库引用，首次构建时 cargo 会自动拉取，无需本地检出；构建 nanalive-spout 需要 git 可用（会拉取 Spout2 子模块）。

WebM 解码使用固定版本的 libvpx：Windows 和 Apple Silicon 首次构建会下载并校验上游预编译静态库，运行时无需额外安装。Intel Mac 使用源码构建，需 `brew install nasm` 和 `rustup component add llvm-tools`；其他需要源码构建的平台可启用 `shiguredo_libvpx/source-build` 特性。

```bash
cargo run -p nanatype
```

包编辑器是独立程序 NanaType Editor（[说明](package-editor.md)），与 NanaType 共用 `crates/frame`，各自构建和打包，互不包含对方的代码与文案：

```bash
cargo run -p nanatype-editor
```

开发构建保留工作区代码的完整调试信息，第三方依赖仅保留源码行号，以减少编译和链接时的调试数据；不影响应用功能，但调试第三方依赖时不包含变量和类型信息。

发布默认使用 `dist`：`opt-level = "s"`、thin LTO、单 codegen unit，并单独生成 PDB。`dist-small` 使用 `z`，保留相同功能；[性能对比](release-profile-performance.md)显示其便携包再缩小约 2 MiB，但关键场景 CPU 耗时增加约 16%～27%，因此保持可选。

[S/Z 混合实验](release-profile-performance.md#sz-混合实验)仅再缩小约 178 KiB（0.49%），部分 P95 增加，因此默认仍用纯 `s`。

日常检查可用 `cargo check -p nanatype` 避免链接，`cargo build -p nanatype --timings` 查看编译耗时。

```bash
cargo test --workspace --exclude nanatype
```

离屏渲染所有窗口的界面快照（不开桌面窗口）：

```bash
cargo run -p nanatype --example snapshots -- target/snapshots nanatype.night ja-JP
```

打包（需要 `nana-packager` 与 Inno Setup 6）：

```bash
pwsh scripts/package.ps1
pwsh scripts/package.ps1 -Product editor
```

`-Product editor` 打包 NanaType Editor（`NanaTypeEditor-<版本>-setup.exe`／`-portable.zip`），只构建 `nanatype-editor`。`sh scripts/check-product-split.sh` 检查两个程序的依赖与特性没有越界（CI 中运行）。更小的便携包可用 `pwsh scripts/package.ps1 -Portable -Profile dist-small`。打包脚本固定关闭发布构建的增量编译并使用锁文件；编译、许可证或发布自检失败时停止。便携包排除自检产生的 `data/` 用户数据，首次运行时由应用创建；内置字体、解码器、GPU 后备渲染和异常恢复均保留。

macOS 上打成 `NanaType.app`、zip 与 dmg（需要 `nana-packager`，产物未签名）：

```bash
scripts/package-macos.sh
scripts/package-macos.sh --product editor
```

Syphon 使用 nanalive-spout 仓库的 `nanalive-syphon` 静态编译进程序，无需附带 `Syphon.framework`；首次构建需要 Xcode Command Line Tools。完整许可文本（含 Syphon Metal 服务端声明）在 `Contents/Resources/THIRD_PARTY_LICENSES.txt`，由 `scripts/macos-licenses.py` 按 macOS 目标的依赖生成。

推送 `vX.Y.Z` 标签时，`.github/workflows/release.yml` 在 Windows 和 macOS 上运行这两个脚本，生成签名的更新清单并创建草稿 Release，见 [检查更新](updates.md)。

Steam 版用 `steam` feature 构建：不含更新检查，皮肤页与播放页可访问创意工坊订阅，设置与 GitHub 版共用。Steam ID 放在不提交的 `.cargo/config.toml`（参照 `.cargo/config.example.toml`），然后：

```bash
pwsh scripts/package.ps1 -Steam
```

```bash
scripts/package-macos.sh --steam
```

详见 [Steam 版](steam.md)。

开发辅助环境变量（仅调试构建或显式设置时生效）：

| 变量 | 作用 |
| --- | --- |
| `NANATYPE_DEBUG=1` | 会话日志记录到 Trace 级并全部落盘（同「详细日志」再加 Trace），见 [日志](logging.md) |
| `NANATYPE_DEMO=1` | 启动 1.5 秒后播放示例台词（调试构建） |
| `NANATYPE_DEMO_MARKUP=<标记文本>` | 覆盖自动播放的示例台词（需 `NANATYPE_DEMO`） |
| `NANATYPE_EXIT_AFTER=<毫秒>` | 到时正常退出（用于测量启动、刷出日志） |
| `NANATYPE_OPAQUE=1` | 叠加窗口改为不透明（调试构建） |
| `NANATYPE_PERF=<JSON 文件>` | 退出时保存各原生窗口呈现／输出帧数、宿主回调耗时与可用工作计数；丢弃前 2 秒 |

渲染流程、帧调度和表面分支见 [渲染管线](rendering-pipeline.md)；渲染测量、GPU 时间戳、截图比较及限制见 [渲染优化验证记录](rendering-performance.md)。

## 项目结构

| crate | 内容 |
| --- | --- |
| `crates/model` | 台词文档模型：文字段、注音、贴纸、标记 |
| `crates/markup` | 标记语言的解析与序列化、模板 |
| `crates/playback` | 揭示时间线、播放器状态机、实时镜像差分、队列 |
| `crates/theme` | 皮肤包：清单、继承、校验、安装与导出 |
| `crates/i18n` | Fluent 文案（zh-CN / en-US / ja-JP） |
| `crates/settings` | 设置结构、版本迁移、原子写入、窗口位置 |
| `crates/audio` | 解码、混音与按采样时刻调度 |
| `crates/bridge` | VTube Studio、OSC、WebSocket |
| `crates/twitch` | Twitch IRC 匿名接收、设备码登录与 Helix 发送 |
| `crates/blive` | blivechat 客户端（协议解析、连接与重连）和彗星号中转 |
| `integrations/comet` | 彗星号创意工坊插件页面：把彗星号事件转换成 blivechat 消息 |
| `crates/core` | 会话逻辑：草稿、发送、继续、队列、历史、回溯 |
| `crates/p2p` | Steam 联机（最多 8 人）、会话排序、重连与图片转发 |
| `crates/update` | 检查更新：签名的 Release 清单、文件代理、校验下载 |
| `crates/frame` | 两个程序共用的框架：皮肤渲染（展示框、历史、输入框）、界面组件、皮肤预览、插件宿主、媒体服务与路径 |
| `app` | NanaType：NanaUI 宿主、窗口、视图、热键、托盘、Spout |
| `editor` | NanaType Editor：包编辑器、签名与隔离调试 |

纯逻辑 crate 不依赖界面框架，可单独测试。

