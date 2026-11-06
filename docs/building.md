[简体中文](building.md) · [English](en/building.md) · [日本語](ja/building.md)

# 从源码构建

这页给开发者：先按“快速开始”编译，再按目标选择测试、快照或打包命令。功能说明见 [README](../README.md)。

## 快速开始

需要 Rust 1.98+、Windows 10 SDK 和可用的 Git。首次构建会由 Cargo 拉取固定 revision 的 NanaUI、nanalive-spout 及 Spout2 子模块。

```bash
cargo run -p nanatype
cargo run -p nanatype-editor
cargo check -p nanatype
cargo test --workspace --exclude nanatype
```

`nanatype-editor` 是独立的 NanaType Editor，与应用共享 `crates/frame`，不会把编辑器代码或文案带进 NanaType。

WebM 使用固定版本 libvpx：Windows 与 Apple Silicon 首次构建下载并校验预编译库；Intel Mac 需要 `brew install nasm` 和 `rustup component add llvm-tools`。其他源码构建平台可启用 `shiguredo_libvpx/source-build`。

## 快照与打包

离屏生成所有窗口快照，不打开桌面窗口：

```bash
cargo run -p nanatype --example snapshots -- target/snapshots nanatype.night ja-JP
```

Windows 需要 `nana-packager` 与 Inno Setup 6：

```powershell
pwsh scripts/package.ps1
pwsh scripts/package.ps1 -Product editor
pwsh scripts/package.ps1 -Portable -Profile dist-small
```

macOS 产出 `.app`、zip 和 dmg（未签名）：

```bash
scripts/package-macos.sh
scripts/package-macos.sh --product editor
```

`-Product editor` 只构建编辑器；脚本固定使用锁文件、关闭发布增量编译，并在编译、许可或发布自检失败时停止。便携包不携带自检生成的 `data/` 用户数据，字体、解码器、GPU 后备渲染和异常恢复仍保留。macOS 的 Syphon 由 `nanalive-syphon` 静态编译；许可文本写入 `Contents/Resources/THIRD_PARTY_LICENSES.txt`。

推送 `vX.Y.Z` 标签后，CI 会在 Windows 和 macOS 执行打包脚本，生成签名更新清单并创建草稿 Release；见 [检查更新](updates.md)。Steam 构建另见 [Steam 版](steam.md)。

## 开发辅助

仅调试构建或显式设置时生效：

| 变量 | 作用 |
| --- | --- |
| `NANATYPE_DEBUG=1` | Trace 级并全部落盘，见 [日志](logging.md) |
| `NANATYPE_DEMO=1` | 启动 1.5 秒后播放示例台词 |
| `NANATYPE_DEMO_MARKUP=<标记文本>` | 覆盖示例台词 |
| `NANATYPE_EXIT_AFTER=<毫秒>` | 到时正常退出 |
| `NANATYPE_OPAQUE=1` | 叠加窗口改为不透明 |
| `NANATYPE_PERF=<JSON 文件>` | 保存原生窗口呈现、输出帧数和宿主回调耗时；丢弃前 2 秒 |

渲染调度见 [渲染管线](rendering-pipeline.md)，验证口径见 [渲染优化验证记录](rendering-performance.md)。

## 代码地图

`crates/model` 台词模型；`crates/markup` 标记；`crates/playback` 时间线；`crates/theme` 皮肤包；`crates/i18n` 文案；`crates/settings` 设置；`crates/audio` 音频；`crates/bridge` VTube Studio/OSC/WebSocket；`crates/twitch` Twitch；`crates/blive` 弹幕接入；`crates/core` 会话逻辑；`crates/p2p` Steam 联机；`crates/update` 更新；`crates/frame` 两个程序共用的渲染与插件宿主；`app` 应用；`editor` 编辑器。纯逻辑 crate 不依赖界面框架，可单独测试。

渲染流程、GPU 测量与发布配置属于项目记录，不是跨设备性能承诺；对应报告保留固定 revision、命令和限制。
