# WGSL 主题验证记录

验证日期：2026-10-17。六套完整程序版画、受限代理及两个作者示例已接入实际 NanaUI 渲染器。设计及静态烘焙说明见[平面设计文档](skin-art-direction.md)。

## 自动检查

- 皮肤格式：42 项通过，含源码与资源限制、压缩包往返、继承、独立示例安装及静态画面的尺寸与透明边缘。
- 应用：157 项通过、2 项原有测试忽略。设置与国际化分别为 27、3 项通过。
- 真实 GPU 集成：纹理采样、轮廓遮罩、完整画布不被原有圆角裁切、减少动态效果、关闭动效、稳定帧缓存及编译失败降级通过。
- 固定时钟截图覆盖全部 24 套内置主题；六套新版另有静态回退、长姓名与窄窗口、1.5 倍 DPI 截图，以及皮肤库缩略图和设置预览。本轮保留原布局，细化了边框和名牌连接，并通过实际布局回归检查姓名不侵入装饰区；姓名渐变的全部色标通过对比度检查。
- 六套动效均通过同一保留场景的 128 帧完整周期检查：正文与引用像素、外轮廓透明度稳定，周期首尾完全一致，GPU 程序与尺寸遮罩缓存不增长；开句和换说话人增强另有 6 个时间点的阅读区域检查。原速预览见 [skins-motion.webp](skins-motion.webp)，结果见 [skin-motion-checks.json](skin-motion-checks.json)。
- 每套新版为一个 WGSL、一张透明装饰纹理、零自定义参数、零装饰节点。复杂装饰有可编辑 SVG 源图和参考来源记录；静态回退由同一程序经生产 GPU 代理烘焙。

## GPU 测量

设备：NVIDIA GeForce RTX 5060；后端：Vulkan。开发构建，单个目标为 960×540；每轮预热 60 帧、测量 120 帧，共三轮。静态基线包含真实回退图片。下表取三轮 GPU P95 的最大值，包含场景绘制与呈现，未包含 OBS 接收端或 Spout 接收同步。

| 主题 | 静态 GPU P95（ms） | 动态 GPU P95（ms） |
|---|---:|---:|
| 蔷薇哥特 | 0.0985 | 0.1183 |
| 星月魔法 | 0.0980 | 0.1262 |
| 森林精灵 | 0.0979 | 0.1262 |
| 潮汐水族馆 | 0.1032 | 0.1226 |
| 朱砂华灯 | 0.1322 | 0.1599 |
| 柠檬汽水 | 0.1006 | 0.1545 |

混合输出：三个场景、每场景两个目标（BGRA8 sRGB 与 RGBA16Float），GPU P95 最大为 1.0510 ms。全部十三个场景的着色器错误数为零。

六套动态主题在测量帧中均没有文字塑形或布局重算；一个效果实例每帧上传 176 字节参数，未重传纹理。顺序切换六套后的代理缓存约 10.55 MiB，混合输出后约 17.19 MiB，均低于 64 MiB 限制；这里不计 NanaUI 通用静态图片缓存。测量结果只代表上述设备及工作负载。

可复现命令：

```powershell
cargo test -p nanatype-theme -p nanatype-settings -p nanatype-i18n --lib
cargo test -p nanatype --lib --test skin_shaders
$env:NANATYPE_GPU_CASES='shader_gothic_static,shader_gothic,shader_celestial_static,shader_celestial,shader_forest_static,shader_forest,shader_tidal_static,shader_tidal,shader_lantern_static,shader_lantern,shader_citrus_static,shader_citrus,shader_tidal_multi_output'
cargo run -p nanatype --example gpu_perf -- target/artwork-perf-delivery.json
```

完整性能数据见 [shader-effects-perf.json](shader-effects-perf.json)。作者接口和示例见 [skins.md](skins.md#受限-wgsl-动效schema-2)。

![六套新主题](skins-v2-preview.png)
