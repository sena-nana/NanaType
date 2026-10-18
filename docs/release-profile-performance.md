# 发布配置的运行性能对比

后续的 [S/Z 混合优化实验](release-profile-hybrid.md)使用另一份固定源码，记录性能路径保留 `s`、服务依赖使用 `z` 的结果。本页保留原始纯 S/Z 快照数据，两轮绝对数值不直接混比。

2026-10-18。继续默认使用 `dist`（`opt-level = "s"`），`dist-small`（`z`）保留为可选。两者的应用功能、字体、解码器、GPU 后备渲染及异常恢复均保留。`z` 在本机仍有较多帧预算余量，但存在可重复的 CPU 处理代价；额外约 2 MiB 的便携包缩减不足以支持更换默认配置。

## 固定版本与方法

比较使用上一轮发布体积测试的固定源码，包含 24 个内置皮肤；NanaUI 固定为 `8ad85c8fd4b4029b327cfb252be1c4431cf95978`。工作区并行进行的皮肤和渲染改动没有混入。源码指纹为 `53679c7600e77de5f41e546e4e41587ddcbbe94b1d24ecbdbb1aa59bf1b092d3`，临时添加的性能程序入口在构建后已移除，原清单及源码字节保持不变。

设备为 AMD Ryzen 7 5800X、NVIDIA GeForce RTX 5060，Windows 11 build 26300，Rust 1.99.0。两组均使用 thin LTO、单 codegen unit、静态 CRT、unwind 和独立 PDB；主要变量为 `opt-level`。GPU 性能程序使用 Vulkan，960×540、1× DPI，固定到逻辑 CPU 0。原生程序固定到前四个逻辑 CPU。

为复用隔离的编译缓存，两组性能工具均以 `dist` 构建，分别固定 `CARGO_PROFILE_DIST_OPT_LEVEL=s/z`，对应两个发布配置的代码生成参数。离屏工具启用相同的 runtime-agent 辅助依赖；原生动画入口转发生产 Host 并注入固定消息。表中指标对应这些工作负载，原便携发布包的启动另外记录。

先交替执行每种配置三次完整的 `gpu_perf`，覆盖 49 个场景。每次含三组试验，每组先预热 60 tick，再记录 120 tick。所有场景的实际绘制、布局、文字塑形、上传量及场景图元计数一致，GPU 时间大致相同。完整矩阵部分运行期间有后台任务活动，原始负载记录予以保留。

默认配置的判断采用另行执行的七个关键场景复测：每种配置五次独立进程，交替顺序为 S/Z/Z/S/S/Z/Z/S/S/Z，每个进程仍执行三组试验。启动前要求连续三秒系统 CPU 低于 20%、GPU 不高于 10%；整次运行平均系统 CPU 超过 25% 的样本不予采用。实际接受的十次运行平均 CPU 约为 9%～17%。下表取每个进程三组试验中位数，再取五个进程结果的中位数；P95 同样汇总，不是将所有帧混为一个分布。

## 关键帧处理结果

CPU 时间包含 Runtime flush、GPU 命令录制及提交，GPU 完成等待、时间戳映射及图像回读不计入。多路输出是自己、伙伴、历史三个场景各两个离屏目标，不包含 OBS 接收端。

| 场景 | `dist` CPU 中位数 | `dist-small` CPU 中位数 | 变化 | `dist` P95 | `dist-small` P95 |
| --- | ---: | ---: | ---: | ---: | ---: |
| 逐字播放 | 0.1077 ms | 0.1274 ms | +18.3% | 0.2220 ms | 0.2743 ms |
| 波浪 | 0.1076 ms | 0.1265 ms | +17.6% | 0.2460 ms | 0.2664 ms |
| 波浪＋彩虹 | 0.1079 ms | 0.1261 ms | +16.9% | 0.2570 ms | 0.2278 ms |
| 装饰动画 | 0.1012 ms | 0.1191 ms | +17.7% | 0.1689 ms | 0.2342 ms |
| 三场景 × 双目标 | 0.2698 ms | 0.3405 ms | +26.2% | 0.4704 ms | 0.5552 ms |
| 扫光 | 0.1001 ms | 0.1166 ms | +16.5% | 0.2110 ms | 0.2250 ms |
| Tidal 三场景 × 双目标 | 0.2068 ms | 0.2631 ms | +27.2% | 0.4118 ms | 0.5236 ms |

所有关键场景的 CPU 中位数均回退，部分 P95 也增加；组合动效的 P95 则下降。两种配置在这些离屏工作负载上的耗时都远低于 60 Hz 对应的 16.67 ms。该数据说明本机有较多余量，同时表明 `z` 会减少 CPU 余量；它不直接给出整机 FPS 或其他设备的表现。

## 补充工作负载

| 测量项 | `dist` | `dist-small` | 变化 |
| --- | ---: | ---: | ---: |
| 24 个皮肤首次加载累计 | 54.92 ms | 70.99 ms | +29.3% |
| VP9 WebM 解码及 YUV 转换，512×512 × 30 帧 | 155.59 ms | 186.63 ms | +19.9% |
| TGS 栅格化及透明度转换，512×512 × 30 帧 | 97.15 ms | 135.34 ms | +39.3% |
| 便携发布版输入窗口就绪 | 2482 ms | 2549 ms | +2.7% |

皮肤加载及解码各执行每种配置三个独立进程。解码测试直接调用固定源码中的生产 `formats.rs`，每进程三次预热、二十次计时；测试动画的全部帧像素 BLAKE3、帧数、尺寸和采样结果均一致。编码夹具生成、首次完整像素校验及 PNG／磁盘写入不计入计时。这些处理发生在贴纸预处理阶段，不能将其变化当作播放帧率的变化。

启动测试直接运行上一轮便携 ZIP 的原始可执行文件，各五次，使用相同的隔离配置和 Tidal 皮肤；计时来自主入口到输入窗口 Ready 的日志，不是首个可见像素时间。两组范围重叠，未观察到实质性的启动收益。

另使用生产 Host 注入三十条历史和波浪／彩虹／脉动组合，开启 Display、History 两路 Spout，记录真实窗口呈现和输出回调。日志确认两路均为 `native: true`；该项用于核对持续动画与原生发送路径。后台负载变化会影响这些窗口的实际帧率，完整负载及帧数据一起保留，不将其单独作为配置效率的归因依据。尚未测量 OBS 接收、合成及录制，也未覆盖其他设备或操作系统。

原生动画各执行三个独立进程，每次运行十秒。使用六秒后的计数与剩余实际时长计算稳态速率，再取进程结果的中位数：Display、History 的呈现与发送均为 `dist` 约 119.05 FPS、`dist-small` 约 119.11 FPS。Display 的准备至回调 P95 为 1.023／1.029 ms，History 为 1.285／1.338 ms。该回调时间是宿主墙钟耗时，包含 GPU／呈现调度，不能与离屏 CPU 录制时间直接混算。

因此，`dist-small` 在本机可保持流畅，全功能小体积构建可以使用。继续默认 `dist` 的依据是 CPU 处理效率和并行负载余量，以及仅约 2 MiB 的额外包体收益；本轮没有观察到 `z` 导致稳定的原生帧率回退。

## 体积收益与复现

| Windows x64 产物 | `dist` | `dist-small` | 额外缩减 |
| --- | ---: | ---: | ---: |
| EXE | 64.92 MiB | 59.90 MiB | 7.7% |
| 便携 ZIP | 34.94 MiB | 32.94 MiB | 5.7%，约 2.00 MiB |

关键样本、矩阵计数一致性及结论见 [结构化测量汇总](performance/2026-10-18/dist-profiles.json)。完整矩阵计时另存本机 `target/dist-hybrid-audit/delivered-full-dist-profiles.json`，其哈希随汇总记录；独立进程日志、额外工作负载源码和执行脚本保存在 `target/dist-runtime-audit/`，原体积测试在 `target/dist-size-audit/`。测试使用预热后的文件及驱动缓存，没有清除系统缓存或进行冷启动重启。

可用现有性能程序对当前代码复测；下列构建应在同一源码上执行，并在编译结束后交替运行，固定 CPU 亲和性：

```powershell
$env:CARGO_INCREMENTAL = '0'
$env:RUSTFLAGS = '-C target-feature=+crt-static'
cargo build -p nanatype --profile dist --example gpu_perf --example render_perf --example skin_load_perf --locked
cargo build -p nanatype --profile dist-small --example gpu_perf --example render_perf --example skin_load_perf --locked
$env:WGPU_BACKEND = 'vulkan'
# 各自执行至少五次，JSON 路径每次使用不同名称。
target/dist/examples/gpu_perf.exe target/profile-perf/s-1.json
target/dist-small/examples/gpu_perf.exe target/profile-perf/z-1.json
```

复测当前代码会包含当时的皮肤和渲染实现，结果应作为新版本的记录保存，避免与本次固定快照直接混算。
