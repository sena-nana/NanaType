# 真实 OBS / Spout 验证

2026-10-17，Windows、RTX 5060、DX12。此报告补充 [多目标生命周期验证](skin-lifecycle-validation.md)，使用真实 OBS 接收器和生产 Spout 发送路径。

后续已优化 [Spout 快速重开与有期限回收](spout-reconnect-validation.md)，将短时开关改为透明暂停与资源复用。下文保留最初验证的记录和当时的重连空白。

## 修复

真实接收端暴露了一个此前离屏测试未覆盖的问题：正文进入静止状态后，自定义 GPU 节点的准备版本虽然没有变化，其像素仍会随时间变化；原生保留输出错误地复用了旧画面。修复前的复现中，潮汐动效在正文停止变化后冻结，发送端只产生了 19 帧。

NanaUI 增加独立的 `SceneGpuRenderer::content_version`，区分“绘制准备可复用”与“最终像素可复用”。默认兼容原有实现；NanaType 的 WGSL 和扫光声明像素随时间变化，同时继续复用已准备的命令和 GPU 资源。上游修复已推送至 [e2a053b2c5dbfdf5be453f5ddef95d72d25ee9cc](https://github.com/sena-nana/NanaUI/commit/e2a053b2c5dbfdf5be453f5ddef95d72d25ee9cc)，应用依赖固定至该提交。

新增 `native_outputs` 调试样例，直接委托真实 `Host`，使用隔离配置、真实窗口、正式 Spout sender 和 OBS `spout_capture`。观察器只在 Windows debug 构建存在；不改变生产围栏交接，也不在帧回调写文件。它从借用句柄打开同一共享围栏，在监测线程检查 ready/release 完成值。

## 环境和工作负载

- [OBS Studio 32.2.2](https://github.com/obsproject/obs-studio/releases/tag/32.2.2)，官方便携包 SHA256 `4d6e40e3ab155f56b30de517380566a206d74b63cdf5ad49aa596924768f97e1`。
- [OBS Spout2 Plugin 1.12.0](https://github.com/Off-World-Live/obs-spout2-plugin/releases/tag/1.12.0)，官方便携包 SHA256 `6c5a31d6f30a44277b1955d4f85a1da1c0baa97a13075594d2bbca475104ee8a`。
- 3 个原生窗口：输入、主展示框、历史；2 路真实 Spout 接收：Display 960×320、History 960×360。桌面 DPI 150%，窗口与固定尺寸输出同时运行。
- OBS 合成 1920×1080 / 60 fps，预乘透明度，NVENC 本地 MKV；皮肤循环请求 30 fps。只录测试源，音频静音，没有推流。
- 主压力测试约 4 分钟，24 套内置皮肤循环 3 轮，加额外切换共 79 次。20 次真实文件热保存，其中 16 次有效、4 次非法循环。另含隐藏原生窗口后继续输出、8 次尺寸交替、恢复原尺寸、5 次关闭/重启输出、动效开关及最终关闭清理。
- 另做两条历史记录的透明皮肤短测，避免 `echo_void` 的 `hide_latest=true` 将唯一条目主动隐藏，混淆闪帧判断。

## 结果

主压力测试记录 5,054 次实际发送：Display 4,914、History 140。全部为 `Sent`，SDK failed/skipped 和 released-without-copy 均为零。5,054 个 release 值全部完成，退出前 pending=0，没有设备丢失或围栏错误。

| 指标 | Display | History |
| --- | ---: | ---: |
| 生产发送回调 CPU 中位数 | 0.129 ms | 0.138 ms |
| 生产发送回调 CPU P95 | 0.224 ms | 0.847 ms |
| 测试中单次最大值 | 80.855 ms | 91.821 ms |

这些回调包括真实 Spout 发送及调试观察开销，排除最初 3 秒，不能当作全帧 GPU 耗时。最大值发生在主动重建 sender；普通暖切换也有一次约 22.44 ms 的发送回调。不能据此承诺每一帧都低于 16.7 ms。OBS 在整个主录像期间新增渲染跳帧 **0**、编码跳帧 **0**。

共享围栏 release 的轮询观测中位数 0.864 ms，P95 1.591 ms。ready 已被观测后至 release 的间隔 P95 1.513 ms、最大 5.976 ms。监测线程每 1 ms 轮询，时间包含线程调度和锁等待，是观测上界；从 send 返回到观测 release 的最大值约 493 ms，不能解读为 GPU 执行了 493 ms。单围栏协议在前一次 release 完成前不会发出下一次 ready，避免以更大 ready 值误判旧 release 已完成。

重复暖切换期间，两个 sender 的池代次均保持 1，没有因皮肤变化重新打开共享池。三轮内置皮肤预热后编译数停在 26；随后有效热保存产生新源码，最终为 60 次累计编译。相同稳定帧没有逐帧编译。

关闭输出并隐藏窗口后，实例遮罩、缩放记录、参数绑定和待编译任务均归零；只保留 4 个有界预热程序，约 5 MiB。输出反复重启时，进程句柄短暂升至 1,212，清理后回落至 960，线程从 122 回落至 80；OBS 内存约 561–562 MiB、句柄约 2,425–2,427，保持平台。应用私有内存预热后约 940–989 MiB，关闭后约 977 MiB，不能仅凭未立即归还进程工作集判定泄漏。应用退出后两个 sender 注册均消失，未残留幽灵发送源。

## 录像核验

主录像解码 14,117 帧，在降采样的两个源区域检查全空白与连续相同画面。Display 没有异常全空白候选；有动效的稳定段最多连续重复 1 帧，符合 30 fps 内容进入 60 fps OBS 的行为，修复前的长时间冻结未再出现。

History 检出 34 个待分类全空白候选：18 帧位于 `echo_void` 切向下一主题时，异步加载完成前仍保留合法的空历史画面；另 16 帧位于一次主动关闭/重启输出后的重新连接。该次 History 从开启命令到恢复可见约 0.77 秒。主动重连期间的空白单独保留，不宣称输出关闭再开启完全无缝。

主录像的命令时钟与视频 PTS 相差约 0.32 秒，由 5 次主动关闭边沿和最终关闭校准，各边沿差值约 0.310–0.336 秒。分析保留原始候选，不把命令下发时刻当作异步皮肤已经提交的时刻。等待 sender 就绪后，两条历史记录短测解码 1,879 帧，3 轮共 12 次透明/实体主题切换，Display 和 History 的全空白候选均为 **0**，OBS 新增渲染跳帧 **0**。数值见 [JSON 汇总](obs-spout-validation.json)。

空白检测只能识别整块源消失，不能自动证明每个装饰像素都无闪烁；保留完整录像和主题轮次截图供复核。现有 `prepare_to_callback` 包含调度间隔，尤其静态历史窗口，不能把其 P95 当作绘制 CPU P95；此处使用独立发送回调观测和 OBS 真实统计。

## 验证与复现

本轮应用回归：162 个单元测试、6 个 `render_effects`、1 个 `skin_catalogue`、1 个 `skin_shaders` 通过，2 个原有测试忽略。NanaUI 新接口相关 3 个测试、`scene_paint` 的 476 个测试通过；两仓库格式检查、diff 检查和上游 engine boundary 检查通过。

原始证据位于 `E:/NanaType-validation-classic-20261017/obs/`：`run01` 为冻结复现，`run02` 为完整压力测试，`run03` 包含过早录制冷启动的试跑，`run04` 为等待真实 sender 就绪后的非空历史短测。OBS 和配置均为独立便携实例；未改用户日常 OBS 场景。

构建优化选项与前一份生命周期报告一致，使用 `cargo build -p nanatype --locked --example native_outputs`。样例仅支持 Windows debug / DX12，运行前设置 `NANATYPE_NATIVE_PROBE` 指向全新隔离目录，准备 `app/data/config/settings.json`，指定两个测试 sender 名称并开启输出。它不读取或迁移常用安装目录。

```powershell
$env:WGPU_BACKEND='dx12'
$env:NANATYPE_NATIVE_PROBE='<isolated run directory>'
$env:NANATYPE_EXIT_AFTER='600000'
# 启动构建出的 native_outputs.exe 后，使用独立 OBS 实例：
python scripts/run-obs-native-stress.py <isolated run directory>
python scripts/analyze-obs-native.py <isolated run directory> --history-rows 2 --video-offset <measured offset>
# 短测：run-obs-native-stress.py ... --focus-history
```

OBS 控制脚本通过便携目录 `obs-auth.json` 的本地 websocket 连接工作，该文件只留在证据目录、不加入仓库。脚本只录制测试场景，不启动直播。`run-obs-native-stress.py` 会等待真实 sender 就绪；录像偏移应逐次校准。

本轮覆盖单台设备、单个 OBS 实例的两路接收和三个原生窗口；没有覆盖长达数小时的直播、跨适配器、实际设备丢失注入或不同显卡驱动。共享 release 证明 DX12→DX11 发送桥完成交接，真实 OBS 录像另行证明接收可见；OBS 接收端本身使用 Spout 的同步协议，不能将该围栏时间称作 OBS 显示延迟。
