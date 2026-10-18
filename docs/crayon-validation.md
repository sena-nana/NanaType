# 蜡笔画皮肤与 UI 材质验收

验收日期：2026-10-18。环境为 Windows，使用实际 NanaUI 离屏 GPU 渲染；未在 Linux、macOS 上执行本次验收。

本页记录静态蜡笔材质阶段的验收。后续通用呈现遮罩、当前固定依赖及动画验收见[文字过渡验收](text-transition-validation.md)。

## 第一阶段交付

- NanaUI 通用材质实现已发布为提交 [`0ae9ee350adbaadf8dc5702bb943cfed75a3d150`](https://github.com/sena-nana/NanaUI/commit/0ae9ee350adbaadf8dc5702bb943cfed75a3d150)。其 `docs/reference/materials.md` 说明共享资源、片元接口、遮罩、缓存与错误回退。
- 该阶段 NanaType 的四个 NanaUI 依赖及锁文件均固定到该提交，`cargo metadata` 确认使用 Git 来源，没有本地路径覆盖。
- 第 25 套内置皮肤为 `nanatype.crayon`。文字与形状使用共享材质合同，输入编辑区继续使用普通文字；关闭高级效果后使用同源 shader 烘焙的框体图片。

![实际材质渲染](crayon-preview.png)

## 测试结果

| 范围 | 结果 |
| --- | --- |
| NanaUI 核心、文字绘制、GPU 材质与设备重建测试 | 11 项通过；包括两种不同示例材质、文字与形状资源复用、富文本覆盖、显式颜色、裁剪、透明度、纹理替换、无效程序/资源回退、彩色 emoji 与设备恢复 |
| 文字缓存 | 参数变化不重新塑形、不重新栅格化、不上传字形；无材质及恒等材质的输出与原生路径逐像素一致 |
| NanaType 固定 Git 依赖测试 | 主题 50 项、国际化 3 项、应用与集成 181 项通过；应用原有 2 项忽略 |
| 全部内置皮肤截图 | 25 套展示框及历史记录，共 50 张；已查看完整图集 |
| 旧皮肤像素回归 | 原有 24 套展示框与基线逐像素一致；历史差异仅为运行时的时间标签 |
| `chat_parity` | 25 套各验证默认宽度及 300px，共 50 个布局案例；回复、提及、输入、发送、换行、边界与控件断言通过 |
| 蜡笔画截图 | 展示框、历史、选择器预览、长姓名、引用及 300px 窄窗；另检查 2× 截图 |
| 材质静态行为 | 连续静态帧相同、不请求动画；减少动态效果保留材质；高级效果关闭再开启恢复原像素 |
| 文本内容 | 中文、Latin、注音、显式红色文字和彩色 emoji；复用原有打字机与文字动效测试路径 |
| 格式及边界 | 两仓库 `cargo fmt --all --check`、`git diff --check`，NanaUI 引擎与包边界检查通过 |

## 复现

NanaType 测试：

```powershell
cargo test -p nanatype-theme -p nanatype-i18n --lib
cargo test -p nanatype --lib --test skin_catalogue --test skin_shaders --test painter_surfaces
$env:NANATYPE_THEME_SNAPSHOTS_ONLY = '1'
$env:NANATYPE_SNAPSHOT_VIEWS = 'display,history'
cargo run -p nanatype --example snapshots -- target/material-snapshots all zh-CN
cargo run -p nanatype --example chat_parity -- target/material-parity all zh-CN audit history
```

蜡笔画的材质切换、静态帧和窄窗截图：

```powershell
Remove-Item Env:NANATYPE_SNAPSHOT_VIEWS -ErrorAction SilentlyContinue
$env:NANATYPE_MATERIAL_AUDIT = '1'
$env:NANATYPE_THEME_AUDIT = '1'
cargo run -p nanatype --example snapshots -- target/crayon-audit nanatype.crayon zh-CN
# 单独检查高 DPI；静态材质切换断言使用 1× 场景。
Remove-Item Env:NANATYPE_MATERIAL_AUDIT
$env:NANATYPE_SNAPSHOT_SCALE = '2'
cargo run -p nanatype --example snapshots -- target/crayon-2x nanatype.crayon zh-CN
```

本次本地产物：`target/material-pinned-theme.log`、`target/material-pinned-app.log`、`target/material-dependency-check.json`、`target/material-regression.json`、`target/material-parity.log`、`target/crayon-final-audit/material-check.json`。这些文件在构建目录中，不纳入发布资源。

资源格式与使用方法见 [皮肤文档](skins.md#通用材质)。
