# 发布配置性能对比（项目记录）

记录日期 2026-10-18。默认继续使用 `dist`（`opt-level = "s"`）；`dist-small`（`z`）仅作可选。两者都保留应用功能、内置字体、解码器、GPU 后备渲染和异常恢复。

## 结论

在固定 Windows 11（Ryzen 7 5800X、RTX 5060、Rust 1.99、Vulkan、960×540、1× DPI）工作负载中，`dist-small` 的关键场景 CPU 中位数比 `dist` 高约 16.5–27.2%，皮肤首载高 29.3%，VP9 解码高 19.9%，TGS 栅格化高 39.3%。两种配置都远低于 16.67ms 的 60Hz 离屏预算；原生 Display/History 约 119 FPS，未观察到稳定帧率退化。

Windows x64 产物：EXE `dist` 64.92 MiB、`dist-small` 59.90 MiB（少 7.7%）；便携 zip 34.94/32.94 MiB（仅少约 2.00 MiB）。体积收益不足以抵消 CPU 余量和并行负载风险，因此默认仍是 `dist`。混合 S/Z 只少约 177.6 KiB，关键场景中位数差在 ±1.2%，不提供混合 profile。

这些数字来自固定源码、NanaUI `8ad85c8fd4b4029b327cfb252be1c4431cf95978` 和源码指纹 `53679c7600e77de5f41e546e4e41587ddcbbe94b1d24ecbdbb1aa59bf1b092d3`，不应与新版本直接相减。GPU 只记录相同场景的相对结果；没有测 OBS 接收、合成、录制、其他设备或操作系统。

## 复测方法

每种配置交替启动五个独立进程（S/Z/Z/S/S/Z/Z/S/S/Z），每进程三组试验，每组预热 60 tick、记录 120 tick；启动前 CPU <20%、GPU ≤10%，平均 CPU >25% 的样本弃用。CPU 数字含 Runtime flush、GPU 命令录制和提交，不含 GPU 等待、时间戳映射、回读。

```powershell
$env:CARGO_INCREMENTAL = '0'
$env:RUSTFLAGS = '-C target-feature=+crt-static'
cargo build -p nanatype --profile dist --example gpu_perf --example render_perf --example skin_load_perf --locked
cargo build -p nanatype --profile dist-small --example gpu_perf --example render_perf --example skin_load_perf --locked
$env:WGPU_BACKEND = 'dx12'
target/dist/examples/gpu_perf.exe target/profile-perf/s-1.json
target/dist-small/examples/gpu_perf.exe target/profile-perf/z-1.json
```

本次记录在 Vulkan 上测得；此后 Windows 构建只编入 DX12 后端（应用的透明窗口走 DirectComposition），复测改用 `dx12`，结果不能与上面的 Vulkan 数字直接比较。示例会带上 dev 依赖，仍可设 `vulkan` 对照，但那不是发布程序的后端。

皮肤/解码测试使用相同动画像素、帧数和尺寸，排除夹具生成、首次校验与 PNG 写盘；启动从主入口到输入窗口 Ready，不是首个可见像素。更新实现后，应保存新的源码指纹、负载和原始数据，不能混入本次记录。
