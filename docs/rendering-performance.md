# 渲染优化验证记录

2026-10-15。应用基线为 `f45ad6515fc6c08a46a69bcb3279b60892b6e15a`，NanaUI 从 `147ee250c06b377309686ec188b741d8d01386a8` 更新至 `3b070aa5566d45b96191162bcd6f35147c25c39a`。框架在独立检出 `NanaUI-render-effects` 的 `codex/nanatype-render-effects` 分支修改并推送，应用依赖远程固定 revision；没有修改 Cargo 缓存源码或加入本地路径覆盖。

## 实现

- 逐字动效继续使用 GPU。效果链保留全部连续动效；位移相加，缩放、透明度相乘，彩虹提供颜色，揭示／入场同时参与。旧单效果 API 保留。
- 空效果、零幅度和减少动态效果不再留下持续动画需求。暂停／翻页边界用揭示上限表达；有限揭示只计算实际安排的文字。框架还会淘汰已结束入场轨道的残留引用。
- 应用更新根据文档待处理的响应式／Runtime 工作收集窗口刷新需求，包括编辑器命令、通知和输出状态回调。展示、伙伴、历史及设置提示使用独立时钟。
- 浮动、呼吸和继续提示用合成器轨道。64 段采样保留会话时间相位，正弦插值误差低于幅度的 0.13%；逐帧贴图仅在帧号变化时写入。
- 效果表按属性版本、皮肤参数和减少动态效果缓存；编辑器同步也包含属性版本。历史行按键索引，分组一次遍历；默认仍展示最近 30 条。弹幕面板采用测量高度的虚拟列表，保留跟随最新、用户滚动和回复操作。
- 新增 `[pulse]`、`[flicker]`、工具栏和三语文案。可选 `dialogue.sweep` 默认关闭，编辑／减少动态效果时显示静态预览。
- 扫光用宿主 `GpuView`、设备和提交；遮罩按表面、物理尺寸及 DPI 缓存，支持轮廓、圆角、渐变与九宫格图片透明度。缓存查找和准备共用一次加锁，目标视口变化无需重建相同遮罩。动画只上传 48 字节 uniform，使用预乘线性颜色，并实现稳定的 `preparation_version`。缓存达到 32 个尺寸变体或 64 MiB 时清理旧尺寸变体；无产品 CPU 回读路径。

## 测量口径

同一台 Windows 设备，Rust 1.99.0，Intel Core Ultra 7 265K，NVIDIA GeForce RTX 5060 Ti。离屏性能程序使用 `release`，Vulkan，960×540、1× DPI。混合核心的调度会影响短帧测量，因此 GPU 工作负载进程固定到逻辑 CPU 0。前后版本分别执行三次独立测量，每次含三组 trial，每组先预热 60 帧，再记录 120 帧；表格使用九组中位数的中位数。测量期间停止编译、截图和原生演示程序。

`gpu_perf` 的 CPU 数值包含 Runtime flush、GPU 命令录制和提交，不含等待 GPU、时间戳映射或图像回读。GPU 时间仅在适配器支持时间戳查询时记录；静态场景没有绘制，GPU 值为 `null`。同时记录实际目标绘制次数、塑形、布局、上传量及绘制调用。自己／伙伴／历史并行场景是三个真实场景、每场景两个离屏目标，不包含 OBS 接收端同步等待。

`render_perf` 单独记录 CPU flush。`NANATYPE_PERF` 记录真实原生窗口呈现／输出回调，丢弃前两秒，在退出时写 JSON；`prepare_to_callback` 是宿主墙钟时间，不能当作 GPU 时间。没有提供的原生 GPU 计数保留 `null`。

## 实测结果

| 场景 | CPU 前 → 后（ms） | CPU 差异 | GPU 前 → 后（ms） | 每 120 tick 绘制目标数 前 → 后 |
| --- | ---: | ---: | ---: | ---: |
| 静止 | 0.0287 → 0.0189 | -34.1% | — → — | 0 → 0 |
| 仅节奏标记 | 0.0603 → 0.0196 | -67.5% | 0.1383 → — | 120 → 0 |
| 减少动态效果 | 0.0591 → 0.0224 | -62.1% | 0.1381 → — | 120 → 0 |
| 逐字播放 | 0.0597 → 0.0530 | -11.2% | 0.1381 → 0.1287 | 120 → 120 |
| 波浪 | 0.0590 → 0.0494 | -16.3% | 0.1393 → 0.1276 | 120 → 120 |
| 波浪＋彩虹 | 0.0595 → 0.0501 | -15.8% | 0.1382 → 0.1277 | 120 → 120 |
| 装饰动画 | 0.1540 → 0.1082 | -29.7% | 0.1257 → 0.1282 | 120 → 120 |
| 1000 条弹幕面板 | 0.0196 → 0.0187 | -4.6% | — → — | 0 → 0 |
| 自己／伙伴／历史 × 双目标 | 0.2171 → 0.1725 | -20.5% | 0.5704 → 0.5322 | 720 → 720 |
| 脉动＋闪烁（新增） | — → 0.0542 | 新增 | — → 0.1272 | — → 120 |
| 扫光（新增） | — → 0.0514 | 新增 | — → 0.1307 | — → 120 |

GPU 的 `—` 表示该组没有绘制目标，没有把空帧时间冒充 GPU 渲染时间。未开启新增效果的场景，CPU／GPU 中位数均未出现超过 5% 的回退。装饰 GPU 时间由 0.1257 ms 变为 0.1282 ms（约 +2%），CPU 提交时间下降约 30%。多目标 CPU 提交时间下降约 21%。这些百分比只对应上述固定工作负载，不是对所有设备或整机 FPS 的预估。

有限揭示结束后，仅节奏和减少动态效果由每组 120 次目标绘制降为 0；波浪、组合、装饰与扫光稳态均没有重新塑形或布局。1000 条弹幕面板的场景图元由 6014 降至 110；回归测试还直接检查 Runtime 挂载节点数低于 1000，且增加视口高度会增加挂载节点，滚动与宽度重测后仍保持限制。

CPU flush 单测中，装饰从 0.0135 ms 降至 0.0019 ms。其余亚微秒项目受 0.0001 ms 计时量化影响，例如 1000 行历史 0.0014 → 0.0015 ms，不在这些单次 flush 数值上计算百分比；帧耗时门限使用上表包含实际 GPU 录制／提交的工作负载。

三次原生窗口复测，每次运行约 8 秒并丢弃前 2 秒：基线展示／历史窗口各持续呈现约 720 帧；修改后两者均为 0 帧，末两秒也均为 0。原生 GPU 计数不可用，仍保留 `null`。隐藏窗口独立验证中，桌面呈现均为 0，Spout 发送端仍输出 Display 180 帧、History 180 帧；日志确认两路 `native: true`。

装饰稳态每组 120 帧的绘制调用为 480 次，上传量由 8,071,680 字节变为 8,057,600 字节；迁移减少的是应用层更新。复杂轮廓的几何上传仍存在。扫描密集关键帧改为 GPU 二分查找，与 CPU `partition_point` 一致；另修复 0ms 挂载已有台词时入场停留在半透明状态的问题，避免额外透明合成。

240 组截图覆盖当时的 18 个内置皮肤（现为 28 套）、2 个图片／字体夹具、4 个窗口及 1×／1.5×／2× DPI。编辑窗口和皮肤库预览的 120 组像素一致；展示窗口 52/60 组一致，其余差异限于周期动画的采样；历史窗口差异位于真实墙钟时间标签。原始 JSON 与对比截图未纳入仓库，可按下文命令在本机重新生成。

## 回归与复现

```powershell
cargo test --workspace --all-targets --locked
cargo test -p nanatype --test render_effects --locked
cargo run --release -p nanatype --example render_perf -- target/render-perf/cpu-after.json
cargo run --release -p nanatype --example gpu_perf -- target/render-perf/gpu-after.json
```

混合核心机器固定亲和性的运行方式（可执行文件已嵌入 Windows Common Controls 清单，可直接运行）：

```powershell
$env:WGPU_BACKEND='vulkan'
$benchmark = Start-Process (Join-Path $PWD 'target/release/examples/gpu_perf.exe') `
    -ArgumentList 'target/render-perf/gpu-after.json' -PassThru -WindowStyle Hidden
$benchmark.ProcessorAffinity = [IntPtr]1
$benchmark.WaitForExit()
```

原生观察请使用隔离的 `APPDATA` / `LOCALAPPDATA`，避免覆盖自己的设置。调试构建设置 `NANATYPE_DEMO=1`、`NANATYPE_DEMO_MARKUP`、`NANATYPE_EXIT_AFTER=8000` 和 `NANATYPE_PERF=<JSON 路径>` 后启动。报告中的 `presented_after_6s` / `output_after_6s` 可区分播放结束后的窗口刷新与后台输出。

截图工具提供固定会话时钟，截图错误直接失败：

```powershell
$env:NANATYPE_THEME_SNAPSHOTS_ONLY='1'
$env:NANATYPE_SNAPSHOT_TRANSPARENT='1'
$env:NANATYPE_SNAPSHOT_SCALE='1.5' # 同样检查 1、2
cargo run --release -p nanatype --example snapshots -- target/snapshots all zh-CN
```

`NANATYPE_SNAPSHOT_VIEWS=display,input` 可重录指定视图；`NANATYPE_SNAPSHOT_SWEEP=1` 临时开启扫光；`NANATYPE_SNAPSHOT_TIME_MS=102000` 固定扫光相位；`NANATYPE_SNAPSHOT_MARKUP` 替换末条台词。全部截图与工作日志位于 `target/render-perf`。

应用最终 `cargo test --workspace --all-targets --locked` 共 281 项测试通过；已删除仅验证旧装饰刷新辅助函数的两项测试，实际合成器行为由集成测试覆盖。框架验证包括 glyph CPU 求值、Scene 动画截止时间、24 项合成器用例、RichTextView API、实际 GPU 的 CPU／Shader 位置一致性、引擎／包边界检查，以及加密打包夹具启动和篡改测试。应用用例覆盖有限揭示停止、减少动态效果、装饰轨道、组合效果、变高虚拟列表、伙伴内容隔离、三档 DPI 的轮廓与预乘 Alpha。

## 皮肤切换、热重载与多输出

2026-10-17，RTX 5060。`app/examples/skin_lifecycle.rs` 用真实 Runtime、Scene 和 GPU painter 保留三个文档，每个文档画入三个目标（两份不同原点与尺寸的 BGRA sRGB 及一份 RGBA16Float），共九个目标；覆盖当时 24 套皮肤四轮共 96 次切换、两个设备代次交替 240 帧、单目标连续改尺寸 80 次和 32 次后台热重载（含 5 次非法循环）。

| 九目标合计 | 修复前 Vulkan | 修复后 Vulkan | 修复后 DX12 |
| --- | ---: | ---: | ---: |
| 切换首帧 CPU P95 | 221.97 ms | 14.48 ms | 16.31 ms |
| 稳定帧 CPU P95 | 3.39 ms | 2.92 ms | 3.71 ms |
| 热重载期间绘制 CPU P95 | 12.95 ms | 13.22 ms | 13.89 ms |

修复后皮肤准备在后台任务执行，WGSL 与纹理解码结果按内容复用；同一材质的多个输出各自保存参数绑定。预热后无额外 WGSL 编译，设备交替不新增参数缓冲；关闭全部输出后实例遮罩、参数绑定、缩放记录与待编译任务归零，只保留约 10 MiB 的有界预热程序。该组为离屏测试，不含 OBS 接收端，真实接收见 [OBS / Spout 输出](obs-spout.md)。

## 验证范围

Windows 原生窗口与 Spout 发送端在本机验证。OBS 接收端的合成／录制、macOS Syphon 和其他显卡后端未实测；离屏多目标数据不能替代这些结果。本次没有加入立绘转场、粒子或全画面后处理。
