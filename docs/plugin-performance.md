# 插件性能、预算与隔离（项目记录）

对应 [#9](https://github.com/sena-nana/NanaType/issues/9)。数字来自单台设备和固定负载，用来复现退化与比较量级，不是跨平台性能保证；包阶段状态见[路线图](package-roadmap.md)。

## 复现

```sh
cargo test -p nanatype-plugins --release --test perf -- --ignored --nocapture
cargo run -p nanatype-plugins --release --example plugin_perf
cargo test -p nanatype --release --test plugin_render_perf -- --ignored --nocapture
cargo run -p nanatype-theme --release --example data_load_perf
```

Windows 指定正式应用作为工作进程：

```powershell
cargo build -p nanatype --release --locked
$env:NANATYPE_PLUGIN_WORKER = (Resolve-Path target/release/nanatype.exe).Path
cargo test -p nanatype-plugins --release --test perf -- --ignored --nocapture --test-threads=1
$env:NANATYPE_GPU_CASES = 'plugin_heavy'
cargo run -p nanatype --release --example gpu_perf -- target/issue9-gpu.json
```

端到端延迟包含排队、两次 IPC、Lua、宿主校验，到视图回到宿主为止；工作集和累计 CPU 包含初始化/预热，不是峰值或每帧占用。离屏绘制数据含 GPU 读回，不能直接换算 FPS。

## 参考结果

2026-10-20，Windows 11 build 26300、Ryzen 7 5800X、RTX 5060、Rust 1.99、未签名 release 便携布局：冷启动至首视图 P50/P95/最大为 58.77/61.19/66.44ms；1/4/8 插件端到端 P95 为 0.430/1.870/12.060ms，8 插件最大 107.348ms；工作集约 17/68/136 MiB。突发每插件 200 事件时接收 33/134/289，其余以 `Busy` 丢弃，发布 2/9/16 次。macOS M4 release 的 1/4/8 插件端到端 P95 为 0.71/0.56/0.72ms，测试工作进程约 8.8MB/插件；两台机器和构建不能互相比较。

Windows 满额视图（2048 条绘制）开发构建曾因未优化 Wasmtime 读取超过 200ms；仅 `nanatype-plugins` 与 `wasmtime` 使用 `opt-level=2` 后，正式 worker 连续 10 次约 37–40ms（中位 37.35ms），预算和 200ms 期限未放宽。macOS 重型画布的 CPU 录制/提交从约 13ms 降到约 1.2–1.6ms；Windows 尚未用同一负载复测。

## 运行预算

| 项目 | 上限 |
| --- | --- |
| Lua 源码 | 128 KiB，UTF-8 |
| 单条 IPC | 256 KiB |
| Wasm 线性内存 | 32 MiB |
| 插件分发 | 2000 万 fuel、约 50ms |
| 宿主读结果 | 5000 万 fuel、约 500ms、20 万值、40 层 |
| 视图 | 8 挂载、512 组件、32 层、2048 绘制指令、8192 路径点、16 KiB 文字 |
| 事件 | 队列 32、单个 128 KiB、通常 200ms、启动 15s |
| 实例/工作进程 | 8 个启用实例；Windows 提交内存 256 MiB |
| 数据请求 | 每次分发 8 个、每事件 4 轮、单结果 64 KiB、每轮 120 KiB |

## 隔离结论

固定 Wasm Lua 不连接 WASI；拒绝 `io`、`os`、`load`、`require` 等能力。每个插件独立 worker，环境变量清空；崩溃、协议错误、超时、fuel/内存耗尽只结束该插件。macOS Seatbelt 实测拒绝读写文件、列目录、网络和创建进程。Windows 作业对象实测限制单进程、桌面/剪贴板/全局原子/系统设置、256 MiB，并在宿主关闭时终止；它不提供文件/网络隔离。其他平台只依赖解释器边界。

AppContainer 通过 `NANATYPE_PLUGIN_APPCONTAINER=1` 选择，但本机只完成链接和严格 Clippy，未在 Windows 实跑，默认关闭。完成探针后才可宣称 Windows OS 级文件/网络隔离：

```powershell
cargo test -p nanatype-plugins app_container -- --nocapture --test-threads=1
$env:NANATYPE_PLUGIN_APPCONTAINER = '1'
cargo test -p nanatype-plugins -- --test-threads=1
cargo test -p nanatype --test plugin_consent -- --test-threads=1
```

macOS App Sandbox 未采用；正式签名、公证、Linux 托盘实机、Windows dist 静态 CRT/安装器/DPI/IME/系统文件选择器仍是限制。完整原始 JSON 和日志在 `target/issue9-*.json`/`.log`。
