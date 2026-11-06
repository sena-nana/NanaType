# 渲染优化验证记录（项目记录）

记录日期 2026-10-15。应用基线为 `f45ad6515fc6c08a46a69bcb3279b60892b6e15a`，NanaUI 从 `147ee250c06b377309686ec188b741d8d01386a8` 更新到 `3b070aa5566d45b96191162bcd6f35147c25c39a`；框架修改在独立分支完成，应用只依赖固定 revision。

## 结论摘要

优化将空效果、有限揭示、减少动态效果和弹幕历史变成按需刷新；逐字、波浪、脉动、扫光仍由 GPU 合成器驱动。固定 Windows release/Vulkan 负载下，CPU 提交时间：静止 0.0287→0.0189ms、逐字 0.0597→0.0530ms、装饰 0.1540→0.1082ms、多目标 0.2171→0.1725ms；未开启新增效果的场景没有超过 5% 回退。1000 条弹幕场景图元从 6014 降到 110，有限揭示结束后不再重绘。

扫光仅在启用、强度非零且颜色不透明时建 GPU 节点；遮罩按表面、尺寸和 DPI 缓存，最多 32 个尺寸变体或 64 MiB，动画只上传 48 字节 uniform。编辑器预览和减少动态效果使用静态相位。展示/伙伴/历史/设置拥有独立时钟，历史最多显示最近 30 条可见记录。

皮肤切换、热重载和多输出的离屏记录显示，修复后九目标切换首帧 CPU P95 从 221.97ms 降到 14.48ms（Vulkan），稳定帧 2.92ms；这不包含 OBS/Syphon 接收。原生窗口与 Spout 已在本机验证，OBS 接收、macOS Syphon、粒子和全屏后处理未验证。

## 测量口径

设备为 Intel Core Ultra 7 265K、RTX 5060 Ti、Rust 1.99；release、Vulkan、960×540、1× DPI；进程固定逻辑 CPU 0。前后各 3 次，每次预热 60 帧、记录 120 帧，取九组中位数的中位数。CPU 包含 Runtime flush、命令录制与提交；GPU 只在支持时间戳时记录，静态无绘制用 `null`。`NANATYPE_PERF` 的 `prepare_to_callback` 是宿主墙钟时间，不是 GPU 时间。

## 复现

```powershell
cargo test --workspace --all-targets --locked
cargo test -p nanatype --test render_effects --locked
cargo run --release -p nanatype --example render_perf -- target/render-perf/cpu-after.json
cargo run --release -p nanatype --example gpu_perf -- target/render-perf/gpu-after.json
$env:NANATYPE_THEME_SNAPSHOTS_ONLY='1'
$env:NANATYPE_SNAPSHOT_TRANSPARENT='1'
$env:NANATYPE_SNAPSHOT_SCALE='1.5'
cargo run --release -p nanatype --example snapshots -- target/snapshots all zh-CN
```

原生观察使用隔离 `APPDATA`/`LOCALAPPDATA`，并可设置 `NANATYPE_DEMO=1`、`NANATYPE_DEMO_MARKUP`、`NANATYPE_EXIT_AFTER=8000` 和 `NANATYPE_PERF=<JSON>`。截图检查 1/1.5/2× DPI、固定会话时钟、正文区域和历史真实时间标签；不把周期动画采样差异当成渲染回归。

## 框架边界

普通形状、图片和九宫格继续由框架样式绘制；图案和复杂轮廓由 retained Painter；两者共用 Surface 路由但不强行统一，因为能力与 GPU 成本不同。报告中的百分比只适用于固定工作负载，不等于设备 FPS 或跨平台帧预算。