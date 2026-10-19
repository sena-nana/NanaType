# 统一包整体设计

总任务：[NanaType #4](https://github.com/sena-nana/NanaType/issues/4)；阶段与进度见[分阶段计划](package-roadmap.md)。本文描述统一包的目标格式与边界，各阶段按此实现；未合并的部分不视为当前可用接口。

## 目标与固定约束

- 只有一种包格式：目录或 `.ntpkg` 压缩包，以 `package.toml` 为唯一身份与权限清单。皮肤、资源（含声明式数据）、Lua 是可组合内容，同一包可以混装。
- 框架不根据包或皮肤 ID 添加行为；ID 只用于选择、索引和依赖解析。
- 所有内容保持可审查：源码、素材、数据声明和处理管线均以明文分发。选择性只读保护与签名校验不是加密或不可绕过的 DRM，签名指纹不能单独证明现实作者身份。
- 导入、选择皮肤、引用资源或数据、执行宿主处理管线都不会运行 Lua。脚本只有在用户逐插件确认来源可信并同意后才启动，同意绑定完整包内容与依赖闭包。
- 脚本没有通用网络、文件、外部程序或宿主内部对象访问接口；可读数据只经宿主解析并按值返回。

## `package.toml`

`package.toml` 是身份和权限的唯一来源，采用严格解析：未知根键、未知子表字段一律拒绝加载，避免拼写错误使声明被静默忽略。

| 键 | 说明 |
|---|---|
| `format` | 包格式，当前为 `1` |
| `schema` | 皮肤配置结构版本，当前为 `3` |
| `id`、`version`、`min_app` | 包 ID（小写字母、数字及 `._-`，≤64 字符）、精确三段版本、最低应用版本 |
| `capabilities` | `skin`、`resources`、`lua` 的组合，1–3 项 |
| `name`、`description`、`tags`、`preview` | 展示信息，名称与说明按语言区域提供 |
| `author`、`license`、`created_at`、`licensed_to` | 作者、许可证、制作日期（`YYYY-MM-DD`，可省略）、授权对象；缺失时界面显示“未提供”，不推断 |
| `base` | 可选，继承另一皮肤配置，须同时声明为依赖 |
| `catalogue_order` | 内置目录排序 |
| `[dependencies]` | `"包ID" = "x.y.z"`，精确版本，≤32 项 |
| `[lua]` | `entry`（UTF-8 Lua 源码入口）与 `reads`（脚本可读的数据列表，默认空） |
| `[data.<id>]` | 声明式数据资源，见下文 |

能力与内容必须一致：`skin` 当且仅当包内有 `skin.toml`；`lua` 当且仅当存在 `[lua]` 且入口文件存在；`resources` 要求包含清单、签名文件、样式和入口之外的文件，或至少一项 `[data]`。`[data]` 可出现在任何能力组合中，例如仅供 Shader 使用数据的皮肤包无需声明 `lua`。

```toml
format = 1
schema = 3
id = "example.dictionary"
version = "1.0.0"
capabilities = ["resources", "lua"]
author = "示例作者"
license = "MIT"
created_at = "2026-10-19"
licensed_to = "所有用户"

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

## 命名空间与依赖闭包

- 包内资源使用相对路径；跨包资源使用 `pkg:<包ID>/<路径>`；常用字体使用 `builtin:fonts/...`。
- 数据使用逻辑 ID 而非路径引用：本包为 `<数据ID>`，依赖包为 `pkg:<包ID>/<数据ID>`。数据 ID 不含 `/`，不能借数据引用拼接路径。
- 只有已声明且精确版本匹配的依赖闭包可见；缺失、版本冲突、循环、越界路径和未声明依赖均拒绝加载。导出把依赖放入 `dependencies/<包ID>/`，并携带授权文件。
- 继承来的相对资源（含字体、图片、音效、立绘变体、Shader 与数据引用）保留父包所属命名空间，子包同名文件不能遮蔽它们。

## 声明式数据资源

`[data.<id>]` 字段：`path`（本包相对路径）、`format`、`format_version`、`source`、`license`，以及仅用于 JSON 的 `constraints`。格式只按声明判断，不按扩展名猜测，也不执行内容；未知格式或版本拒绝。

| 格式 | 版本 | 原始上限 | 说明 |
|---|---|---|---|
| `text` | 1 | 4 MiB | 严格 UTF-8，去除 BOM，行数有上限 |
| `json` | 1 | 8 MiB | 解析后检查约束：根类型、必需键、最大深度、项目数、字符串长度、数值范围 |
| `nthz` | 1 | 32 MiB | 宿主注册的笔顺容器，复用现有校验；索引、偏移、块长度和解压量逐项检查 |

- 读取前先检查路径、存在性和字节预算；解析限制原始字节、解压总量、单块、嵌套、项目、字符串、数值、偏移和长度。
- 导入时完整校验每项数据，不运行 Lua。解析结果按（内容哈希、格式、格式版本、约束）缓存并在包之间共享，耗时解析不进入每帧路径。
- 数据文件和声明本身是普通包内容，纳入签名、选择性保护、启用指纹和完整导出。编辑器（#6）展示来源、许可、校验失败字段与预算诊断。

## 完整性、指纹与同意

- 可选 `package.integrity.json` 用 Ed25519 签名覆盖保护策略、文件清单和受保护文件摘要；`package.toml` 必须受保护。依赖包独立校验自己的签名。
- 启用指纹是包及依赖闭包全部文件的 BLAKE3 摘要。`[lua] reads` 与 `[data]` 声明位于 `package.toml`，数据字节是包内文件，因此都被指纹覆盖。
- 同意记录保存指纹、可信来源确认和策略版本。策略版本：`1` 为脚本与播放/聊天事件；`2` 增加声明数据读取。新增数据范围或宿主接口时提高策略版本，旧版本同意不能授权新能力，已启用插件需重新审查。
- 后端在启动、重载、包变化和依赖变化时都重新核对指纹；运行中插件的包内容被修改时立即停止并要求重新同意。旧设置中的启用 ID 不产生信任。
- 同意界面逐项展示作者与来源信息、签名状态及公钥指纹、脚本入口、`reads` 中每项数据的所属包、格式与版本、大小、来源、许可及是否受保护，以及禁止的系统能力。

## Lua 数据接口

Lua 不能直接调用宿主；数据读取采用异步请求/应答，复用现有事件驱动模型。

```lua
local pending
return {
    on_event = function(event)
        if event.type == 'init' then
            pending = nana.data.request('phrases', {kind = 'pointer', pointer = '/items/0'})
        elseif event.type == 'data' then
            for _, r in ipairs(event.results) do
                if r.token == pending then first = r.value end
            end
        end
    end,
}
```

- `nana.data.request(id, query)` 返回 token；结果随后以 `{type = 'data', results = {{token, value | error}}}` 事件送达。插件第一次 `view()` 时尚无数据。
- 只能请求 `[lua] reads` 中列出的数据。宿主在同意时对数据快照，按快照应答；请求中的路径、URL 或未声明 ID 返回错误。
- 查询：`info`；`whole`（结果不超预算时）；文本 `lines`；JSON `pointer`（RFC 6901）与 `slice`（数组项或对象条目）；NTHZ `glyph`（单个字符的笔画与中线）。
- 预算：每次分发最多 8 个请求，每个宿主事件最多 4 轮连锁请求；单个结果 64 KiB、每轮合计 120 KiB。脚本侧和宿主侧都检查，超限的请求返回错误而不是截断。大数据只能分块查询，不会整体进入 IPC。

## 声明式处理管线与 Shader 输入

仅供 Shader 使用的数据由宿主处理，不依赖也不启动 Lua。

1. **资源声明**：`[data.<id>]`。
2. **宿主处理管线**：`skin.toml` 的 `[pipelines.<名称>]` 选择已注册处理器及版本，输入为数据引用或其他管线的输出，参数只接受处理器签名中声明的有限数值。管线必须是有向无环图；不开放表达式、动态原生解析器或隐式脚本。上下文只包含当前绘制目标的文字排版、尺寸、时间和进度等明确值。
3. **Shader 绑定**：材质的 `textures` 槽可以是图片路径或管线纹理输出，`uniforms` 绑定管线的 `vec4` 输出；宿主校验类型、尺寸和槽位。首版支持纹理与 uniform，缓冲区与大数据查询按需求扩展。

```toml
[pipelines.stroke_map]
processor = "text.stroke_time_map"
version = 1
inputs = { strokes = { data = "strokes" } }

[materials.write]
source = "write.wgsl"
params = []
textures = [{ pipeline = "stroke_map", output = "time" }]
uniforms = [{ pipeline = "stroke_map", output = "extent" }]
```

- 首个处理器 `text.stroke_time_map@1`：输入 `nthz` 笔顺数据与当前文字排版，输出时间纹理 `time` 和尺寸 `extent`。
- 拒绝：未注册处理器或版本、环、类型不符、缺少或多余输入、参数越界、纹理槽超过 2、参数与 uniform 合计超过 4 个 `vec4`、依赖排版的材质用于入场以外位置。纹理边长不超过 2048，并计入材质纹理的 32 MiB 预算。
- 结果按数据哈希、材质与排版指纹缓存：只有文字、字体、排版、尺寸或书写方向变化才重建；播放进度由 GPU 根据揭示时间计算，不逐帧更新纹理。

## 情书回归案例

情书在 `package.toml` 声明 `[data.strokes]`（`nthz` v1，含 Make Me a Hanzi 来源与 Arphic 许可），在 `skin.toml` 用 `text.stroke_time_map` 管线为 `write` 材质提供时间纹理，WGSL 采样方式不变。它继续复用现有 NTHZ 校验、笔顺算法和缓存，不按皮肤 ID 特判，也不引入 Lua 中转。旧的 `dialogue.reveal.handwriting` 已删除，出现时加载失败并提示改用 `[data]` 与 `[pipelines]`。

## 延期与非目标

- 包编辑器、简单 IDE、隔离调试、签发与保护选择界面：#6。签名私钥保存在宿主专用位置（`crates/secrets`），不进入包、资源接口、日志或插件进程。
- 时间纹理等转换移到后台线程、1／4／8 插件压测、OS 级 AppContainer／App Sandbox 加固、Windows／macOS 实机验收：#9。转换处理器已写成纯函数，便于后续迁移。
- 不提供：脚本访问任意路径或 URL、本机模块、网络、持久化存储；按扩展名猜测格式；把签名视为作者身份认证。
