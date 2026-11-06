# 统一包整体设计（项目记录）

总任务：[NanaType #4](https://github.com/sena-nana/NanaType/issues/4)；分阶段验收见[分阶段计划](package-roadmap.md)，编辑器使用见 [package-editor.md](package-editor.md)。本文是目标格式与安全边界；未合并阶段不应当作当前稳定接口。

## 结论与边界

- 统一格式是目录或 `.ntpkg`，以严格解析的 `package.toml` 作为身份、能力和权限清单；皮肤、资源、声明式数据、音效与 Lua 可混装。
- 包内容以明文、可审查为原则。签名和选择性只读保护用于完整性与编辑边界，不是加密、不可绕过的 DRM，也不能证明现实作者身份。
- 导入、选择皮肤、读取资源和执行宿主处理管线都不运行 Lua。Lua 只有在用户逐插件确认可信来源后才启动；同意绑定包及依赖闭包的完整内容指纹。
- Lua 没有通用网络、文件、外部程序或宿主对象接口，只能通过宿主解析器按值读取明确声明的数据。

## `package.toml` 摘要

严格解析会拒绝未知键。当前 `format = 1`、`schema = 3`。必备身份字段为小写包 `id`（≤64 字符）、精确三段 `version`、`min_app`、`capabilities`；展示和授权字段包括 `name`、`description`、`tags`、`preview`、`author`、`license`、`created_at`、`licensed_to`。能力必须与内容一致：`skin` 对应 `skin.toml`，`sounds` 对应 `sounds.toml`，`lua` 对应 `[lua]` 和入口文件，`resources` 对应额外资源或 `[data]`。

```toml
format = 1
schema = 3
id = "example.dictionary"
version = "1.0.0"
capabilities = ["resources", "lua"]
author = "示例作者"
license = "MIT"

[dependencies]
"example.words" = "1.2.0"

[lua]
entry = "main.lua"
reads = ["phrases", "pkg:example.words/common"]

[data.phrases]
path = "data/phrases.json"
format = "json"
format_version = 1
source = "作者自制"
license = "MIT"
constraints = { type = "object", required = ["items"], max_depth = 8 }
```

## 命名空间、数据与管线

包内资源用相对路径，跨包资源用 `pkg:<包ID>/<路径>`，内置字体用 `builtin:fonts/...`。依赖必须精确声明且无缺失、冲突、循环或越界；导出将依赖放进 `dependencies/<包ID>/`。继承资源保留父包命名空间，子包不能遮蔽父资源。

`[data.<id>]` 支持 `text` v1（4 MiB）、`json` v1（8 MiB）和 `nthz` v1（32 MiB）。读取前检查路径、原始字节、解压量、嵌套、项目、字符串、数值和偏移预算；导入完整校验并按内容哈希缓存，解析不进入每帧路径。Lua 用 `nana.data.request(id, query)` 异步读取 `[lua].reads` 中的数据：每次分发最多 8 请求、每事件最多 4 轮、单结果 64 KiB、每轮 120 KiB。

`[pipelines.<name>]` 只能引用已注册处理器和版本，组成无环图；参数是处理器声明的有限数值，Shader 通过 `textures`/`uniforms` 接收输出。首个处理器 `text.stroke_time_map@1` 输出笔顺时间纹理和 `vec4` 尺寸；纹理边长≤2048，材质纹理预算 32 MiB。结果按数据、材质和排版指纹缓存，进度由 GPU 采样，避免逐帧重建。

## 完整性与同意

可选 `package.integrity.json` 用 Ed25519 覆盖保护策略、文件清单和摘要，`package.toml` 必须受保护；启用指纹是包及依赖闭包的 BLAKE3。策略版本 1 授权脚本与播放/聊天事件，版本 2 增加声明数据读取；能力范围变化会使旧同意失效。包或依赖变化时重新核对，运行中修改立即停止插件。界面应显示来源、签名状态、公钥指纹、入口、每项数据的格式/版本/大小/许可及禁止的系统能力。

## 编辑器与非目标

编辑器使用真实渲染器预览，草稿、撤销和导出都不改当前设置或已安装包；签名私钥只存系统受保护存储（macOS 钥匙串、Windows DPAPI），不进入包、日志或插件进程。Lua 调试仍需单独确认，运行在独立 Wasm 工作进程中，编辑即撤销确认。

不提供脚本任意路径、URL、本机模块、网络或持久化存储；不按扩展名猜格式，不把签名当作者认证。Windows 正式隔离与完整平台验收的状态见[插件性能、预算与隔离](plugin-performance.md)。
