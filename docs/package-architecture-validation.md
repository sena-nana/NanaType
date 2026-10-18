# 统一包架构验收记录

验证日期：2026-10-18 至 2026-10-19，Windows x86_64。本文记录本次迁移的实测结果；时间数据取决于机器、构建配置与负载。

## 架构与兼容范围

全部 28 个内置皮肤均使用 `crates/theme/builtin/<名称>/package.toml`、`skin.toml` 和包内素材。完整清单见[皮肤文档](skins.md#内置皮肤)。构建脚本扫描包目录生成内嵌资源表，不再维护按皮肤 ID 分支的素材清单。

框架提供透明、无框、静态的中性默认值。光环的分组、间距、头像和字号已变成包中明确声明的普通字段；绘制流程不根据皮肤 ID 修改这些值。28 个包分别更换 ID 后，加载所得完整样式仍与原包一致，自动测试覆盖此约束。光环仅保留为应用默认选择与回退包。

统一格式为 `format = 1`、`schema = 3`、扩展名 `.ntpkg`，能力可组合 `skin`、`resources`、`lua`。不兼容旧包格式；旧 `skins/` 目录不会自动迁移或删除。包使用精确版本依赖及 `pkg:<ID>/<路径>` 资源命名空间；导出包含依赖闭包和授权文件。继承的资源保留所属包，不能被子包同名文件暗中覆盖。

## 安全边界

功能插件分发 Lua 源码，运行于官方固定解释器 Wasm 和独立工作进程。只显式启用的插件才执行；导入、预览及资源依赖不触发代码。插件没有网络、文件、外部程序、剪贴板或凭据访问接口，宿主没有相应代理。Wasm 未连接 WASI，未知或系统导入拒绝执行；子进程环境清空。

执行使用内存、fuel、期限、队列和协议预算。宿主再次验证子进程返回的节点、绘制数据、唯一 ID、总文字及几何量；超限和异常使实例终止。Lua `pcall` 可处理普通脚本错误，无法捕获宿主执行限额。UI 线程不等待 Lua；视图由宿主保留并绘制，插件不进入每帧渲染循环。

系统级 AppContainer／App Sandbox 加固后置；本次验证不等于已建立操作系统级隔离，也不承诺运行时不存在漏洞。macOS 构建、实机行为与签名尚未在本轮验证。详细接口和全部限额见[插件文档](plugins.md)。

## 视觉与功能验证

- 28 个皮肤 × 展示／历史／输入三个视图，共 84 张图片逐像素一致，无缺失。基准使用迁移前完整解析的样式配置，在同一渲染器中固定时间重新生成；原先光环的宿主布局规则以等价字段编码进基准。此比较验证包迁移的样式等价性。
- 已查看全部皮肤的展示／历史总览；`chat_parity` 对全部 28 包执行默认及 300px 窄窗检查，覆盖长昵称、正文、引用和历史布局。
- GPU 测试验证 Lua 示例的文字及画布随事件更新、空闲时保留相同输出、卸载后无残留像素。
- 最终应用验收通过 200 项库测试、19 项界面集成测试；2 项原有库测试保持忽略。主题 70 项、核心 45 项、设置 29 项及插件 10 项测试通过。
- 主题测试覆盖包格式、依赖闭包、版本、命名空间、导入导出、资源预算、归档路径及大小写碰撞。插件测试覆盖不可用系统 API、注入字符串、非法协议／绘制数据、死循环、内存耗尽、保护调用和独立进程故障。

主要复验命令：

```powershell
cargo test -p nanatype-theme -p nanatype-core -p nanatype-settings -p nanatype-plugins
cargo test -p nanatype --lib --test skin_catalogue --test skin_shaders --test skin_artwork --test plugin_ui --test chat_panel --test input_window
cargo build -p nanatype --bin nanatype --example snapshots --example chat_parity
$env:NANATYPE_THEME_SNAPSHOTS_ONLY='1'
$env:NANATYPE_SNAPSHOT_VIEWS='display,history,input'
$env:NANATYPE_SNAPSHOT_TRANSPARENT='1'
& target/debug/examples/snapshots.exe target/package-review all zh-CN
& target/debug/examples/chat_parity.exe target/package-layout-review all zh-CN audit history
cargo run -p nanatype-plugins --release --example plugin_perf
```

本轮本地证据位于 `target/package-pixel-comparison.json`、`target/package-final-layout.log`、`target/package-theme-final.log`、`target/package-acceptance-tests.log` 和 `target/package-runtime-final-tests.log`。应用可执行文件构建成功，其工作进程返回四条合法响应，计数按 `0 → 1 → 0` 更新，标准错误为空；结果见 `target/package-worker-final-smoke.json`。格式检查、差异空白检查及第三方授权清单检查通过。这些文件是本机验证产物，不随源码分发。

## 性能

皮肤加载基准对照提交 `5726b7ea624a9fdb4766c09160f10c897dd47a81` 与当前实现，交替运行三个进程取中位数。内置不可变清单在构建时生成解析数据，开发目录仍重新读取。

| 项目 | 迁移前（ms） | 迁移后（ms） |
|---|---:|---:|
| 冷启动皮肤注册表 | 341.221 | 219.064 |
| 已加载包读取 | 0.0090 | 0.0088 |
| 已热身选择器 | 0.1921 | 0.1846 |
| 重新扫描 | 0.0654 | 0.0711 |

渲染测量限制到相同两个逻辑 CPU，交替执行前后版本，每场景各九次试验。以下为宿主基准记录的中位耗时，不代表显示器整帧耗时。

| 场景 | 迁移前（ms） | 迁移后（ms） |
|---|---:|---:|
| 静态 | 0.0078 | 0.0066 |
| 仅播放节奏 | 0.0116 | 0.0066 |
| 文字显示 | 0.0071 | 0.0067 |
| 特效 | 0.0081 | 0.0068 |
| 减少动态效果 | 0.0086 | 0.0102 |
| 装饰 | 0.0088 | 0.0066 |
| 30 条历史 | 0.0077 | 0.0074 |
| 1000 条历史 | 0.0585 | 0.0457 |

减少动态效果场景增加 0.0016ms，重新扫描增加 0.0057ms；不能据此声称每个场景相对增幅都小于 5%。大多数测量改善，整体没有观察到明显性能退化。微秒级差值应结合测量噪声理解。原始对照见本机 `target/package-load-comparison.json` 与 `target/package-render-isolated-comparison.json`。

固定解释器在构建阶段预编译。最终 release 计数器基准：解释器初始化 2.0575ms；预热后 1000 次事件中位 0.1664ms、P95 0.3205ms、最大 0.9209ms。这是 VM 内执行，未包含操作系统创建进程与 IPC。另已通过正式应用可执行文件的工作进程入口完成初始化、台词计数和重置协议检查；应分别评估冷进程启动和正常事件延迟。

## 可安装示例

[计数器插件包](examples/counter-plugin.ntpkg)包含 `package.toml`、`main.lua` 和许可证。在“插件与资源包”导入并启用，可检查计数、画布和重置按钮。可通过 `python scripts/package-example-plugins.py` 从[示例源码](examples/counter-plugin/main.lua)重新生成。
