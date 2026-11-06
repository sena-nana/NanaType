# 六套程序版画（项目记录）

本记录定义蔷薇哥特、星月魔法、森林精灵、潮汐水族馆、朱砂华灯、柠檬汽水的静态设计与克制动效。先锁定二维构图、墨色、材质和阅读区域，再加入时间；每套只有一个 `artwork.wgsl`、一张透明纹理、零自定义数值参数和零装饰节点。姓名、正文、引用和继续提示仍由宿主排版。参考与源文件见[装饰来源](skin-art-sources.md)。

## 主题摘要

| 主题 | 画面母题 | 动效边界 |
| --- | --- | --- |
| 蔷薇哥特 | 酒红装订侧栏、十二瓣花窗、细金饰和尖拱窗棂 | 单道柔光沿金边；玻璃蔷薇轻呼吸，开句短增强 |
| 星月魔法 | 偏心天球仪、同心刻度、轨道和金色新月 | 轨道缓慢偏转，小星移动；开句星弧只在装饰面 |
| 森林精灵 | 无涂层纸、装订孔、羽状叶标本和卷芽 | 主轴先弯、羽片稍后轻屈；萤光点错相漂移 |
| 潮汐水族馆 | 起伏玻璃、水线、水母标本和深海色面 | 水母约 9° 倾角、伞体约 15% 收缩；触须滞后，反光与焦散留在水域 |
| 朱砂华灯 | 朱砂版画侧栏、菱格窗棂、垂灯与流苏 | 悬挂点轻摆，流苏延迟，暖光慢呼吸；金线弱扫 |
| 柠檬汽水 | 薄荷斜向包纸、网点底印、水果剖面和贴纸 | 三颗气泡错相上升并淡出；打字时轻微增强 |

所有主题循环统一 16 秒，整数谐波，首尾速度平滑归零。姓名、正文和框体不随装饰摇动；开句/换说话人的增强约 1 秒回落，正文与引用区保持稳定。应用仍按 30/60 FPS 调度，预览为 8fps 的真实 GPU 输出。

## 静态回退与配置

`still.png` 是同一 WGSL 在固定 16 秒、固定种子下由生产 GPU 代理烘焙的完整画面，用于编辑器、缩略图、减少动态、关闭高级动效和 GPU 编译失败。皮肤参数只烘焙默认值；运行时 `static_fallback=true` 可按当前参数冻结姿态。

```toml
[dialogue]
region = [920, 220]
padding = [36, 65, 217, 28]
shader_effects = [{ shader = "artwork", target = "artwork" }]
box = { frame = { image = "celestial/still.png", slice = [0, 0, 0, 0] } }

[shaders.artwork]
source = "celestial/artwork.wgsl"
period_s = 16
intensity = 1
textures = ["celestial/ornament.png"]
```

主画面在 WGSL 中绘制框体、材质、光线和事件响应；主清单只增加一条纹理引用。烘焙并编译：

```powershell
cargo run -p nanatype --example skin_artwork -- crates/theme/builtin
```

## 验证与预算

```powershell
cargo build -p nanatype --example snapshots
$env:NANATYPE_SNAPSHOT_MOTION_FRAMES='128'
foreach ($id in @('gothic','celestial','forest','tidal','lantern','citrus')) {
  & target/debug/examples/snapshots.exe "target/skin-motion/nanatype.$id" "nanatype.$id" zh-CN
}
Remove-Item Env:NANATYPE_SNAPSHOT_MOTION_FRAMES
```

测试固定 UI 状态，只推进渲染时钟；正文/引用逐像素不变、轮廓透明度不变、周期首尾一致，缓存和程序数量不增长。2026-10-17 RTX 5060/Vulkan 开发构建中，动态主题 GPU P95 为 0.1183–0.1599ms；三个场景双目标最大 1.0510ms，代理缓存约 17.19MiB，低于 64MiB。数字仅为本机观测，不是跨平台帧预算。
