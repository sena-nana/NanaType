# 插件性能、预算与隔离

对应 [#9](https://github.com/sena-nana/NanaType/issues/9)。本文给出可复现的测量方法、一次参考结果、运行预算和实测的隔离边界。参考结果来自单台机器，只用于比较量级和发现退化，不是性能保证；Windows 实机结果尚未补齐。

## 复现

```sh
# 真实工作进程：冷启动、1／4／8 插件端到端延迟、宿主调用开销、突发与队列饱和、内存
cargo test -p nanatype-plugins --release --test perf -- --ignored --nocapture
# 解释器内单次事件（不含进程与 IPC）
cargo run -p nanatype-plugins --release --example plugin_perf
# 满预算画布与 60 行文字在宿主 UI 线程的绘制开销（离屏，含 GPU 回读）
cargo test -p nanatype --release --test plugin_render_perf -- --ignored --nocapture
# 情书声明数据：冷／热加载与导入校验
cargo run -p nanatype-theme --release --example data_load_perf
```

`perf` 每项输出一行 JSON：样本数及 P50、P95、最大值。端到端延迟从宿主调用 `event()` 计时，到工作进程的视图回到宿主为止，包含排队、两次 IPC、Lua 执行和宿主侧校验；事件间隔 45ms，避开每 33ms 一次的发布节流。

## 参考结果（2026-10-19，macOS 27 arm64，release）

| 测量 | P50 | P95 | 最大 | 备注 |
|---|---|---|---|---|
| 冷启动（创建进程至首个视图） | 6.4 ms | 14.1 ms | 407 ms | 最大值为首次加载；含 Seatbelt 配置 |
| 启动并停止一次 | 6.8 ms | 14.7 ms | 408 ms | 停止时回收进程 |
| 端到端事件，1 个插件 | 0.51 ms | 0.71 ms | 0.95 ms | 工作进程 RSS 约 8.8 MB |
| 端到端事件，4 个插件 | 0.38 ms | 0.56 ms | 0.98 ms | 合计 RSS 约 35 MB |
| 端到端事件，8 个插件 | 0.30 ms | 0.72 ms | 1.56 ms | 合计 RSS 约 70 MB |
| 宿主 `event()` 调用（UI 线程） | ≤0.023 ms | ≤0.044 ms | 0.10 ms | 只入队，不等待 |
| 解释器内单次事件 | 0.022 ms | 0.029 ms | 0.12 ms | 计数器示例 |

**突发**：每个插件同时送出 200 个事件，队列接收 33 个，其余以 `Busy` 丢弃，宿主每次调用不超过 0.012 ms。工作进程连续处理积压，只发布最新视图：33 个事件约 40ms 内全部反映到界面，每个插件只发布 2 次。合并之前需要约 1.2 秒，界面依次显示中间状态。

**复杂视图**：一个满预算画布（2047 条圆角矩形）在工作进程中约 7ms 生成并读出。宿主改为通过 Lua C API 直接读取结果后，才能达到协议允许的 2048 条；此前在 Wasm 内用 Lua 编码 JSON，约 190 条就会耗尽燃料。宿主 UI 线程重绘这类画布约需 12ms（离屏，变化帧 17ms 对比静止帧 4ms），60 行文字约 1.7ms，主要开销在 NanaUI 绘制管线。因此宿主按绘制开销调节发布频率：每 512 条绘制指令增加一个 33ms 间隔，满预算画布每秒约重绘 6 次，普通视图仍为每秒 30 次。

**情书数据**：冷加载（含 NTHZ 索引解析与管线绑定）32ms；新注册表热加载中位 9ms，解析结果按内容哈希共享；导入时完整校验全部压缩块 53ms。130 字的笔顺时间图约 45ms，现在在后台线程生成，UI 线程只做排版快照。

## Windows 开发构建的满额视图（2026-10-19）

同机直接 IPC 测量，2048 条圆角矩形的完整视图在修复前约需 280–302ms，超过宿主 200ms 响应期限。开发／测试构建现在仅对 `nanatype-plugins` 和 `wasmtime` 使用 `opt-level = 2`，减少逐值读取 Lua 结果时的 Rust 与 Wasmtime 调用开销；release／dist、响应期限及沙箱限额均未调整。

修复后的正式 `nanatype.exe --plugin-worker` 连续 10 次响应约 37–40ms，中位 37.35ms、最大 40.01ms；每个响应仍包含 2048 条命令。这是单机 debug 参考结果，不等同于完整 release 性能验收。

原先失败的 `heavy_canvases_repaint_less_often_than_light_views` 已通过，并增加完整视图与工作进程错误检查；满额视图至少 150ms、轻量视图小于 100ms 的发布间隔断言保持不变。进程 5 项及 Lua 沙箱 15 项回归通过，正式应用工作进程生命周期回归通过。复现命令：

```sh
cargo test -p nanatype-plugins -- --test-threads=1
cargo test -p nanatype --test plugin_consent production_workers_require_approval_stop_on_changes_and_ignore_late_responses -- --exact --test-threads=1
```

本地证据为 `target/full-draw-plugin-tests.log`、`target/full-draw-app-worker.log`、`target/full-draw-profile-probe.json` 和 `target/full-draw-app-probe.json`。

## 运行预算

| 项目 | 上限 |
|---|---|
| 源码 | 128 KiB，仅 UTF-8 Lua 源码 |
| 单条 IPC | 256 KiB |
| Wasm 线性内存 | 32 MiB |
| 插件代码每次分发 | 2000 万 fuel、约 50ms 墙钟中断 |
| 宿主读取结果 | 5000 万 fuel、约 500ms 墙钟，最多 20 万个值、40 层 |
| 视图 | 8 个挂载、512 个组件、32 层嵌套、2048 条绘制指令、每条路径 256 点、合计 8192 点、文字合计 16 KiB |
| 事件 | 排队 32 个、单个 128 KiB；正常响应期限 200ms，启动 15 秒 |
| 视图发布 | 每插件约 30 次/秒，按绘制开销降低 |
| 数据请求 | 每次分发 8 个、每个宿主事件 4 轮、单个结果 64 KiB、每轮 120 KiB |
| 启用实例 | 8 个 |
| Windows 工作进程 | 已提交内存 256 MiB（作业对象） |

## 隔离边界（实测范围）

- **解释器**：固定的 Wasm Lua，不连接 WASI；文件、进程等导入一律拒绝，不提供 `io`、`os`、`load`、`require` 等库。宿主读取结果时不运行插件代码或元方法。
- **独立进程**：每个插件在独立工作进程中运行，环境变量被清空。崩溃、协议错误、超时、燃料耗尽或内存耗尽只结束该插件；进程停止后立即回收，反复启停不残留进程（`process` 测试）。
- **macOS**：工作进程在读取任何请求前应用拒绝优先的 Seatbelt 配置；宿主事先打开的管道仍可用。自执行测试实测读文件、写文件、列目录、网络连接与绑定、创建进程均被拒绝（`os_sandbox` 测试）。无法应用配置时，工作进程不运行任何插件代码。应用程序本身作为工作进程运行（`nanatype --plugin-worker`）也已验证。
- **Windows**：宿主在发送请求前把工作进程放入作业对象：单进程、禁止桌面、剪贴板、全局原子和系统设置访问、已提交内存 256 MiB、宿主关闭时终止。现已在 Windows 上验证正式应用工作进程的启动、事件处理、禁用或包／依赖变化后的终止与进程回收，作业对象创建成功；各项 OS 限制仍未逐项实测。AppContainer 未实现，因此不宣称 Windows 有文件或网络层面的 OS 隔离。
- **其他平台**：没有 OS 级隔离，只依赖解释器边界。

开放源码或可 fork 不削弱官方宿主的权限边界；签名有效不等于作者身份认证，部分只读保护不宣称不可绕过。

## 诊断

- `PluginProcess::id()` 给出工作进程 ID，供外部工具检查内存和存活状态。
- `shader_effects::cache_stats()` 中 `pending` 为编译中的程序；`refused` 为编译槽占满时推迟的次数，由后续帧重试。
- `pipeline::builds()` 统计排版相关材质的构建次数，稳定帧不应增长。
- 插件错误显示在“插件与资源包”中对应包的条目下，内容不足 1024 字符时完整保留。

## Windows 声明式管线观测（2026-10-19）

复用 `layout_materials` 的真实 GPU 用例，在 Windows 11（26300）、debug/test 构建中记录冷／热加载、重排和稳定帧。热加载保留同一注册表及已加载皮肤，测的是已验证结果的复用；稳定帧只推进播放进度，时间图构建计数不增长。用例另验证字体、窗口宽度及 1.5 倍 DPI 变化后替换时间图，保持替换前的当前揭示时钟。

```powershell
cargo test -p nanatype --test layout_materials -- --test-threads=1 --nocapture
```

未设置截图目录的最终精简复跑，结果记录在 `target/issue8-final-layout.log`：

| 项目 | 观测值 |
|---|---:|
| 情书首次加载 | 982.11 ms |
| 同一注册表热加载中位数，5 次 | 0.0003 ms |
| 首次 GPU 帧，包含首次绘制准备与读回 | 6979.02 ms |
| 稳定帧中位数，5 次，包含 GPU 读回 | 29.40 ms |
| 字体与排版变化后的帧，包含 GPU 读回 | 46.54 ms |

这些是单机 debug 测试观测，未控制系统负载，也没有 GPU 时间戳；时间图就绪的观测还包含首帧读回，不能当作转换函数的独立耗时。它们不与前文 macOS release 数字直接比较，完整 release 性能验收仍在阶段 5。开启 `NANATYPE_LAYOUT_MATERIAL_CAPTURES` 时还包含 PNG 编码与写盘，该轮只用于画面检查。

## 未完成

- Windows 实机：作业对象各项限制、完整 release 性能测量、安装包与签名。开发构建的满额绘制失败已修复，见上文；单机 debug 测量与启停／回收验证不代替完整平台验收。
- AppContainer 隔离；macOS 正式签名与 App Sandbox 授权。
- NanaUI 绘制大量画布指令的开销（约 12ms／2047 条），需要在上游优化。
