# 插件性能、预算与隔离

对应 [#9](https://github.com/sena-nana/NanaType/issues/9)。本文给出可复现的测量方法、参考结果、运行预算和实测的隔离边界。各平台结果来自单台机器，只用于比较量级和发现退化，不是性能保证；Windows 发布验收的剩余限制见文末。

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

Windows 上复用同一测量用例，指定正式应用作为工作进程；不指定时使用插件 crate 的独立测试工作进程：

```powershell
cargo build -p nanatype --release --locked
$env:NANATYPE_PLUGIN_WORKER = (Resolve-Path target/release/nanatype.exe).Path
cargo test -p nanatype-plugins --release --test perf -- --ignored --nocapture --test-threads=1
# 重型插件的宿主 CPU 录制／提交及 GPU 时间戳，执行与结果解析不计入这段计时
$env:NANATYPE_GPU_CASES = 'plugin_heavy'
cargo run -p nanatype --release --example gpu_perf -- target/issue9-gpu.json
```

Windows 的工作集由 `GetProcessMemoryInfo` 读取，CPU 为 `GetProcessTimes` 给出的工作进程累计内核与用户时间，包含初始化和预热；不可当作每帧耗时或 CPU 占用率。跨平台不可用的指标输出 `null`。冷启动指新建进程，不会清除系统文件缓存；端到端测量截止于宿主回调，不包含窗口消息循环和画面呈现。

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
- **Windows**：宿主在发送请求前把工作进程放入作业对象：单进程、禁止桌面、剪贴板、全局原子和系统设置访问、已提交内存 256 MiB、宿主关闭时终止。正式应用工作进程的启动、事件处理、禁用或包／依赖变化后的终止与进程回收已验证。新增原生测试读取内核中的实际配置，并验证子进程创建被拒绝、超过 256 MiB 的提交申请失败，以及最后一个作业句柄关闭后等待中的进程结束；不能以退出码是否非零判断作业终止。UI 限制只核对内核配置，未执行退出登录、修改显示设置、写剪贴板等操作。AppContainer 未实现，因此不宣称 Windows 有文件或网络层面的 OS 隔离。
- **其他平台**：没有 OS 级隔离，只依赖解释器边界。

开放源码或可 fork 不削弱官方宿主的权限边界；签名有效不等于作者身份认证，部分只读保护不宣称不可绕过。

AppContainer／LPAC 是进一步收窄 Windows 权限的候选方案，需要包 SID、访问控制、进程创建属性及继承句柄一起验证；不能由当前作业对象替代。此轮保持现有启动与 IPC 结构，尚未实现该隔离层。设计依据见 [Microsoft 的 AppContainer 启动说明](https://learn.microsoft.com/en-us/windows/win32/secauthz/implementing-an-appcontainer)。

2026-10-20 核对 GitHub 告警：`rustls` 与 `sharp` 的历史告警已修复。[GHSA-wrw7-89jp-8q8g](https://github.com/advisories/GHSA-wrw7-89jp-8q8g)（`glib` < 0.20）只经 Linux 托盘的 `libappindicator`／GTK 3 依赖引入。托盘升级到 `tray-icon` 0.26 并改用纯 Rust 的 `ksni`（D-Bus StatusNotifierItem）后端，`Cargo.lock` 不再包含 `glib`、`gtk` 及其系统库绑定；应用的托盘代码不用修改；新版本在 Windows 与 macOS 上的托盘行为尚未实机复查。按 `tray-icon` 的说明，旧后端要求应用初始化 GTK 并运行其事件循环，而应用从未这样做。Linux 没有交叉 C 工具链，只用独立工程按应用同样的托盘调用对 `x86_64-unknown-linux-gnu` 做了编译检查，未在 Linux 实机运行。

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

## Windows release 观测（2026-10-20）

Windows 11 build 26300、Ryzen 7 5800X、RTX 5060、Rust 1.99.0，正式应用的 `release` 工作进程，未签名。最终便携布局位于 `dist/issue9-final/app/NanaType`。以下测量期间没有本项目的编译或其他 GPU 用例并行运行，但共享机器上仍有其他编译负载；不能把不同轮次的差异全部归因于代码，也不与 macOS 数字直接比较。

| 测量 | P50 | P95 | 最大 | 样本 |
|---|---:|---:|---:|---:|
| 冷启动至首个视图 | 58.77 ms | 61.19 ms | 66.44 ms | 20 |
| 启动并停止 | 62.16 ms | 64.69 ms | 70.10 ms | 20 |
| 端到端，1 插件 | 0.311 ms | 0.430 ms | 0.722 ms | 200 |
| 端到端，4 插件 | 0.321 ms | 1.870 ms | 10.397 ms | 800 |
| 端到端，8 插件 | 1.000 ms | 12.060 ms | 107.348 ms | 1600 |
| 宿主调用，1 插件 | 0.0149 ms | 0.0202 ms | 0.0454 ms | 230 |
| 宿主调用，4 插件 | 0.0055 ms | 0.0206 ms | 1.2195 ms | 920 |
| 宿主调用，8 插件 | 0.0054 ms | 0.0403 ms | 2.3966 ms | 1840 |
| 解释器内事件，不含进程与 IPC | 0.054 ms | 0.102 ms | 1.548 ms | 1000 |

1／4／8 插件的端到端、宿主调用、工作集、累计 CPU 和突发结果见 `target/issue9-performance-final.log`；原始轮次保留在 `issue9-performance.log`，不挑选其中最快的样本拼接。每个插件先预热 30 次，再测 200 次；工作集与累计 CPU 在循环结束时采样，不是峰值内存或持续 CPU 占用率。

1／4／8 插件工作集合计分别为 17088／68396／136408 KiB，累计工作进程 CPU 为 125／437.5／968.75ms。突发每插件 200 个事件，分别接收 33／134／289、以 `Busy` 拒绝 167／666／1311 个，排空约 39.60／139.75／51.08ms；宿主单次突发调用最大 0.011／1.547／0.629ms，更新发布合计 2／9／16 次。8 插件的长尾与首轮不同，仍在 200ms 响应期限内；未通过重试过滤较慢结果。

### 重型视图

同一 Lua 夹具包含 2047 个圆角矩形与 60 行文字。插件画布改用直接圆角矩形操作，路径仍保留原来的填充／描边；绘制缓存键直接哈希字段，消除每次更新的 Debug 字符串构造。既有 GPU 用例检查圆角矩形、路径描边及填充的实际像素更新、静止复用与移除视图。

GPU 时间戳工具使用 Vulkan、960×540、每轮预热 60 帧及测量 120 帧，共三轮，不读回像素；Lua 执行和视图解析在 CPU 计时之前。优化前后都只为该测量进程设置 AboveNormal 优先级及逻辑处理器 0、2，未修改系统或其他进程。

| 指标 | 优化前（三轮） | 优化后（三轮） |
|---|---|---|
| CPU 录制与提交 P50 | 64.16／51.64／46.63 ms | 43.67／36.03／35.30 ms |
| CPU 录制与提交 P95 | 95.89／70.78／60.24 ms | 56.86／42.11／40.71 ms |
| GPU 时间戳 P50 | 0.0626／0.0622／0.0623 ms | 0.0665／0.0661／0.0663 ms |

每轮均保留 2107 个场景图元、360 次绘制调用与 7200 次文字整形；GPU 时间没有同步下降，仍以宿主 CPU 开销为主。前后原始结果为 `target/issue9-gpu-controlled.json`、`issue9-gpu-after-paint.json`。这里的 CPU 数字是墙钟时间，不能由 GPU 时间推算可达帧率。

原有 340×1400 离屏测量包含 GPU 读回：变化帧 P50 从 70.88ms 降为 35.40ms、P95 从 96.61ms 降为 49.91ms；静止帧 P50 从 15.43ms 降为 7.67ms。它与上述时间戳的视口及测量范围不同；用例虽名为 `within_frame_budget`，实为显式忽略的测量，没有帧率阈值，不能据运行通过宣称达到 60fps。

情书独立数据测量：冷加载 68.43ms，新注册表热加载中位 42.94ms，导入完整校验 188.70ms。保留同一注册表的管线用例热加载中位 0.0001ms，稳定帧中位 0.81ms、重排帧 6.60ms，均含 GPU 读回；冷首帧 4275.93ms 包含首次 GPU 准备，时间图就绪的观测也包含它，不是转换函数的独立计时。证据为 `target/issue9-{data-performance,layout-release,render-final,vm-performance}.log`。

## macOS 画布 CPU 优化（2026-10-20）

Apple M4、macOS 27.0.1、Rust 1.99.0、release，同一重型夹具（2047 个圆角矩形加 60 行文字，每个事件全部变化）。采样（`/usr/bin/sample`）显示，变化帧的宿主时间主要不在绘制，而在场景排序：每个自绘图元排序时都要查询 `draw_default()` 录在哪个阶段，查询逐个比较整份录制，2047 个方块每帧约 800 万次比较，插入和排序各做一遍，约占主线程样本的 43%。NanaUI 修复两处（[b427b84](https://github.com/sena-nana/NanaUI/commit/b427b84cd7ede8809f3d0b0a33697b4ba9990bb2)）：

- 只为节点的内建图元查询该阶段，查找改为按变体匹配；
- 相邻且圆角与变换相同、只有实心 sRGB 填充的方块合成一个 `QuadColorBatch` 图元。中间有路径、文字、图层或裁剪时断开，绘制顺序不变；带边框、阴影或作者色彩空间的方块仍单独绘制。新增的 GPU 测试逐像素比较合并与分开绘制的半透明重叠方块（最大差 1／255），并按混合公式核对重叠处颜色。

同一基线（NanaUI `main`，不含这两处改动）、只修排序、两处都修的三个二进制交替运行，避免机器负载差异偏向某一组：

| 测量 | 修复前 | 只修排序 | 两处都修 |
|---|---:|---:|---:|
| 离屏变化帧 P50（340×1400，含读回，三轮） | 15.53–15.66 ms | 6.46–6.68 ms | 3.83–3.93 ms |
| 离屏变化帧 P95 | 15.94–16.14 ms | 6.86–7.55 ms | 4.38–4.69 ms |
| 离屏静止帧 P50 | 3.68–3.75 ms | 3.43–3.61 ms | 2.21–2.31 ms |
| CPU 录制与提交 P50（960×540，两轮各三次） | 12.31–13.59 ms | 4.32–5.82 ms | 2.05–2.27 ms |
| CPU 录制与提交 P95 | 12.47–19.10 ms | 4.64–14.11 ms | 2.24–4.39 ms |
| 场景图元 | 2107 | 2107 | 61 |

每帧绘制调用仍为 3 次（120 帧共 360 次），文字整形仍为每帧 60 次。修复后剩余的约 2ms 中，60 行变化文字的整形约占三分之一，其余为布局、系统工作和画家编码；插件 Lua 执行在 `gpu_perf` 的计时之外。

在此基础上，宿主不再在每次插件更新时重建整棵视图：挂载位按结构签名（节点种类、ID、子节点、按钮命令、画布尺寸）决定是否重建，结构不变时文字、按钮标签和画布绘制原地写入已有组件，其他插件或其他挂载位的更新也不再触发重建。新测试确认同结构连续更新后组件 ID 不变、画面与重新挂载逐像素一致，增删子节点后照常重建。同样交替运行：

| 测量 | 仅 NanaUI 修复 | 再加原地更新 |
|---|---:|---:|
| 离屏变化帧 P50（三轮） | 4.14–4.20 ms | 3.31–3.51 ms |
| 离屏静止帧 P50 | 2.36–2.42 ms | 2.18–2.32 ms |
| CPU 录制与提交 P50（未受干扰的三轮，各三次） | 1.74–2.21 ms | 1.15–1.56 ms |

`gpu_perf` 两组各有一轮明显受机器负载干扰（仅 NanaUI 2.59–5.52ms，原地更新 2.63–3.55ms），原样保留在 `target/issue9-mac-gpu-inplace-ab.log`，未计入上表。测量时机器上有其他编译负载（负载均值约 7–10），P95 受其影响较大，中位数在三组之间稳定。

```sh
cargo test -p nanatype --release --test plugin_render_perf -- --ignored --nocapture
NANATYPE_GPU_CASES=plugin_heavy cargo run -p nanatype --release --example gpu_perf -- target/issue9-mac-gpu.json
```

原始结果为 `target/issue9-mac-render-ab.log`、`target/issue9-mac-gpu-ab.log` 与 `target/issue9-mac-gpu-*.json`，采样为 `target/issue9-mac-sample-*.txt`。Windows 上尚未复测；改动在平台无关的场景层，预期同样受益，但不以 macOS 数字代替 Windows 结果。

## 未完成

- Windows `dist` 静态 CRT 发行构建、安装／卸载和原生多窗口 DPI／IME／系统文件选择器。已校验的便携包采用 `release`，仍依赖 VC++ 运行库；八项布局、文件、身份、自检与篡改检查通过，不代表安装器通过。首次重型 debug 进程测量在并发冷编译时曾超过 200ms，停止本项目编译后的原用例通过，未延长期限或降低负载。
- 正式签名按用户要求跳过；Inno Setup 下载并静默安装被自动审批以 `blocked by policy` 拒绝，安装器没有构建。macOS 本轮没有可用实机，历史记录不等于当前发布验收。
- AppContainer／LPAC 文件和网络隔离，macOS 正式签名与 App Sandbox 授权；Linux 托盘的实机运行。
- 重型画布的宿主 CPU 开销：macOS 上变化帧录制与提交已从约 13ms 降到约 1.2–1.6ms（见上节），Windows 尚未用同一负载复测。
