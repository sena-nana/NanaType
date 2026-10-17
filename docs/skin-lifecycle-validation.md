# 皮肤切换、热重载与多输出验证

日期：2026-10-17。原始记录保存在 `E:/NanaType-validation-classic-20261017/lifecycle/`。

后续已补充 [真实 OBS / Spout 接收及共享围栏验证](obs-spout-validation.md)，并进一步修复原生保留输出的动态像素缓存。下文保留本组离屏测试的原始提交和数值。

## 修复范围

- 皮肤选择和文件热重载通过一个后台准备任务执行。连续修改只保留最新请求；导入、删除、复制或目录刷新会使旧结果失效。无效热重载保留上一个有效版本，继承皮肤同时监听父目录。
- 验证后的 WGSL 和纹理解码结果采用有界弱引用缓存；相同源码和纹理内容复用，不延长旧皮肤寿命。缩略图不再强持有被删除皮肤。
- 仅向普通图片加载器提供静态设计实际引用的图片；正文引用的其他皮肤图片按需解析。皮肤图片采用 BLAKE3 内容地址，原地保存也能正确刷新。失效版本解除会话固定，继续遵守媒体缓存的容量和过期策略。没有额外的 BMP 中间格式。
- 同一材质的多个输出分别保存参数绑定，避免单批命令中后一个目标覆盖前一个目标的参数。DPI、目标尺寸、边界和透明度参与区分；每个材质最多保留八组目标绑定和八组缩放记录。
- WGSL 和原有扫光支持有界的设备代次缓存；关闭、丢弃窗口或切换回静态皮肤时回收不再使用的材质、遮罩和缩放记录。
- NanaUI 上游修复：[d79fcc0d3e6b13b59688449b05af2e0d32a76149](https://github.com/sena-nana/NanaUI/commit/d79fcc0d3e6b13b59688449b05af2e0d32a76149)。静态与 GPU 混排切换只改变 MSAA 附件，保留兼容的颜色目标和绑定；重建尺寸相关资源时，命中管线缓存不再重复创建着色器模块。NanaType 已固定此远端提交，未修改 Cargo 缓存或使用本地路径覆盖。

## 工作负载与指标

`app/examples/skin_lifecycle.rs` 使用真实 Runtime 文档、Scene 和 GPU painter，保留主展示框、对方展示框及历史窗口三个文档，每个文档同时画入三个目标，共九个目标。包含不同原点和尺寸的两份 BGRA sRGB 输出及一份 RGBA16Float 输出。GPU 工作全部在宿主 FrameContext 中提交。

- 24 套皮肤切换四轮，共 96 次；每次首帧后再画五个稳定帧，断言稳定帧不再排版或整形，预热后不重新编译 WGSL。
- 在同一批命令中将同一材质画入不同原点、尺寸及 DPI 的目标，回读对应像素；WGSL 与原有扫光均检查，防止参数相互覆盖。
- 两个真实设备代次交替绘制 240 帧，检查编译数、参数分配数、遮罩、绑定及缓存字节不增长。
- 一个目标连续改变尺寸 80 次，检查绑定和缩放缓存有界。
- 后台目录热重载 32 次，包含五次非法循环；绘制不中断，非法版本不能替换当前皮肤，相同内容复用已验证程序。
- 关闭全部文档和 painter 后，检查实例遮罩、参数绑定和缩放记录归零。仍保留的程序属于有界预热缓存。

`record_ms` 为每次九目标首帧的 Runtime 更新、CPU 绘制录制及提交耗时，不包含后台皮肤准备。`switch_cpu_and_gpu_wait` 还包括测试工具的同步加载与 GPU 等待，不能当作应用 UI 阻塞时间。首次启动的冷编译与预热后的切换分开记录。完整数值由 `scripts/report-skin-lifecycle.py` 汇总到 `skin-lifecycle-perf.json`。

阶段数组 `stages_ms` 的前 12 项为三个文档各自的 flush 和三次 paint，第 12、13 项为提交和 GPU 等待；后 27 项为九次 paint 各自的 batch、upload、encode。原始尖峰主要位于 encode，不能仅凭静态图片同时出现就归因于解码。

## 实测结果

下表均为九目标合计耗时；切换列取后三轮的 72 次首帧，稳定列取最后一轮的 120 帧。

| 指标 | 修复前 Vulkan | 修复后 Vulkan | 修复后 DX12 |
| --- | ---: | ---: | ---: |
| 切换首帧 CPU P95 | 221.97 ms | 14.48 ms | 16.31 ms |
| 切换首帧 CPU 最大值 | 280.75 ms | 16.80 ms | 18.70 ms |
| 稳定帧 CPU P95 | 3.39 ms | 2.92 ms | 3.71 ms |
| 稳定帧 CPU + GPU 完成等待 P95 | 未记录 | 4.04 ms | 4.86 ms |
| 热重载期间绘制 CPU P95 | 12.95 ms | 13.22 ms | 13.89 ms |

Vulkan 切换首帧 CPU P95 降低约 93.5%。热重载的后台准备耗时与绘制耗时分别记录，不把后台验证的约 110 ms 尾部延迟计作 UI 阻塞。两种后端均通过相同的输出隔离、96 次切换、设备代次交替、80 次尺寸变化、32 次热重载及关闭清理断言；每轮预热后均无额外 WGSL 编译，两个设备交替时没有新增参数缓冲分配。

关闭全部输出后，实例遮罩、参数绑定、缩放记录与待编译任务均为零。8 个程序占约 10 MiB，属于限制内保留的预热缓存，不随稳定帧增长。

静态视觉回归：[skin-lifecycle-visual.json](skin-lifecycle-visual.json)。24 套主题与此前确认的羊皮纸边框版本逐像素完全一致。

回归命令和结果：

- `cargo test -p nanatype-theme --locked`：48 通过。
- `cargo test -p nanatype --locked --lib --test render_effects --test skin_catalogue --test skin_shaders`（使用下文相同依赖优化选项）：162 个应用单元测试及 8 个集成测试通过，2 个原有测试忽略。
- NanaUI 独立仓库：`cargo test -p nana-ui --lib --features gpu,wgpu-interop scene_paint -- --test-threads=1`（`nana-ui` opt-level 2、`image` opt-level 3）：476 通过。
- NanaUI：`python scripts/check-engine-boundary.py` 通过。
- 两仓库 `cargo fmt --all -- --check`、`git diff --check` 通过。构建使用上面的 E 盘 target 目录；应用构建继续存在原有 libvpx 调试符号链接警告。

## 复现

基线使用 NanaUI `8fca5bb6850e959608d371f7b0c01e285773f543`，修复后使用上述固定提交；两次均使用原始 PNG 和相同依赖优化选项。Windows NVIDIA RTX 5060，dev profile，`CARGO_PROFILE_DEV_DEBUG=0`；这些选项用于可重复比较，没有写入项目默认 dev profile。

```powershell
$env:CARGO_PROFILE_DEV_DEBUG='0'
cargo build -p nanatype --locked --example skin_lifecycle --example snapshots `
  --target-dir E:/NanaType-validation-classic-20261017/build `
  --config profile.dev.package.image.opt-level=3 `
  --config profile.dev.package.png.opt-level=3 `
  --config profile.dev.package.fdeflate.opt-level=3 `
  --config profile.dev.package.nana-ui.opt-level=2 `
  --config profile.dev.package.nana-ui-core.opt-level=2 `
  --config profile.dev.package.nana-ui-runtime.opt-level=2 `
  --config profile.dev.package.nana-ui-scene.opt-level=2
$env:WGPU_BACKEND='vulkan' # 然后单独用 dx12 再运行一次
E:/NanaType-validation-classic-20261017/build/debug/examples/skin_lifecycle.exe <report.json>
python scripts/report-skin-lifecycle.py --before <before.json> --after <after.json> --dx12 <dx12.json> --out docs/skin-lifecycle-perf.json
```

## 验证边界

这组测试验证真实多目标 GPU 绘制、格式、资源寿命和调度策略。它不启动桌面窗口或 OBS，不包含 OBS 接收器、Spout 消费方以及共享围栏往返的耗时；不能据此宣称实际直播接收链路已完成验证。设备代次交替也不等同于故障注入测试。

NanaUI 的普通本地图片首次加载仍可能同步解码；空闲资源按原有有界策略淘汰后允许重新加载。此次消除了反复切换时的无效资源重建，不承诺任意第三方皮肤和 GPU 驱动上的绝对帧时上限。
