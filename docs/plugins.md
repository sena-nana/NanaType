# 统一包与功能插件

包的身份来自 `package.toml`，样式来自 `skin.toml`。`capabilities` 可组合 `skin`、`resources`、`lua`。内置皮肤与用户包采用相同格式和校验规则，内置资源由构建工具扫描目录生成，清单在构建时生成可缓存的解析数据；运行时仍使用同一份格式与校验规则，开发目录每次重载重新读取。皮肤 ID 仅用于选择、索引和依赖解析，不参与绘制决策。

格式版本为 `format = 1`、`schema = 3`；统一存储于应用数据目录的 `packages/`。旧格式不兼容，旧 `skins/` 目录不自动迁移或删除。依赖使用 `[dependencies]` 的包 ID 与精确三段版本；资源引用为本包相对路径或 `pkg:<包ID>/<路径>`。常用字体可通过 `builtin:fonts/...` 引用。依赖缺失、冲突、循环及越界资源会拒绝加载。导出默认将依赖放入 `dependencies/<包ID>/`，携带素材授权。

## Lua API 2

功能包额外声明 `capabilities = ["lua"]` 和 `[lua]` 表：`entry` 为 UTF-8 源码入口，`reads` 列出脚本可读取的声明数据（默认为空）。脚本只接收值类型事件，并返回声明式视图；不接触应用内部对象或 Rust 消息枚举。

```toml
[lua]
entry = "main.lua"
reads = ["phrases"]          # 本包数据；依赖包数据写作 "pkg:<包ID>/<数据ID>"

[data.phrases]
path = "data/phrases.json"
format = "json"              # text、json、nthz；按声明而非扩展名解析
format_version = 1
source = "作者自制"
license = "MIT"
constraints = { type = "object", required = ["phrases"], max_depth = 3 }
```

```lua
local count = 0
return {
    on_event = function(event)
        if event.type == 'line_start' then count = count + 1 end
    end,
    view = function()
        return {views = {{slot = 'panel', root = {
            kind = 'text', id = 'counter', text = tostring(count)
        }}}}
    end,
}
```

- 事件包括 `init`（`reads` 列出可读数据）、`data`、`command`、播放开始／结束 `line_start`／`line_end`、`speaking`、`expression`、`portrait`、`shake`、`vts` 和 `chat_message`（平台 `bilibili`／`douyin`／`blivechat`／`youtube`／`twitch`、作者、纯文本）。事件字段是宿主已生成的本地值，不包含账户凭据。
- `commands = {{id = 'reset', label = 'Reset'}}` 声明本地命令。宿主显示命令入口，激活后向原插件发回 `{type='command', id='reset'}`；命令不能调用任意应用动作。
- 视图挂载点为 `display`、`history`、`input`、`panel`，分别对应展示、历史、输入内容区和插件面板。通用界面仍使用独立的中性主题。
- 组件为 `column`、`row`、`text`、`button`、`canvas`，每个组件都需稳定且唯一的 `id`。Lua 函数可以组合返回这些节点以定义可复用组件。
- 容器使用 `children`。`text` 使用字面文本，不执行富文本、链接或 HTML；`button` 通过 `command` 回传事件，支持宿主按钮已有的键盘和无障碍交互。
- `canvas` 声明 `width`、`height` 与 `draw`。绘制支持 `rect`（`rect=[x,y,w,h]`、RGBA `color`、`radius`）和 `path`（`points`、`color`、`closed`、`width`）；宿主绘制并缓存结果，Lua 不进入每帧绘制路径。皮肤的高级动效继续通过受限 WGSL 接口表达。

### 声明数据

`nana.data.request(id, query)` 只能请求 `reads` 中的数据，立即返回 token；宿主随后以 `{type='data', results={{token=…, value=…}或{token=…, error=…}}}` 事件送回结果，因此第一次 `view()` 尚无数据。查询为 `{kind='info'}`、`{kind='whole'}`、文本 `{kind='lines', start, count}`、JSON `{kind='pointer', pointer='/a/0'}` 与 `{kind='slice', pointer, start, count}`、笔顺数据 `{kind='glyph', char='心'}`。结果只是值：不含路径、句柄或宿主对象；JSON `null` 在 Lua 中为 `nil`。

宿主按启用同意时的内容快照应答，导入时已按声明格式完整校验。每次分发最多 8 个请求，每个宿主事件最多 4 轮连锁请求，单个结果 64 KiB、每轮合计 120 KiB（包含错误回复），`lines`／`slice` 每次最多 1024 行；超过预算返回错误而不是截断，大数据须分块查询。下一个宿主事件到来时，未应答的请求作废。数据格式、约束与预算的完整说明见[统一包整体设计](package-design.md)。

完整可安装示例位于 `docs/examples/counter-plugin/`，打包好的 [counter-plugin.ntpkg](examples/counter-plugin.ntpkg) 可在“插件与资源包”导入并启用。它统计已播放台词，提供画布进度条和重置按钮。`docs/examples/phrase-plugin/`（[phrase-plugin.ntpkg](examples/phrase-plugin.ntpkg)）演示声明 JSON 数据并逐句按值查询。修改源码后用 `python scripts/assets/package-example-plugins.py` 重新生成压缩包。

## 安全与生命周期

设置中的“审查来源并请求启用”会逐个展示作者、制作日期、许可证、授权对象、脚本入口、可读取数据（所属包、格式版本、大小、来源、许可及是否受签名保护）、包位置、签名状态及内容指纹。用户须主动勾选已确认来源可信且了解风险，再点击“同意并启动此插件”。确认前不运行代码。旧设置中的启用 ID 不会生成新的信任记录；同意记录带策略版本，策略 2 增加声明数据读取，此前的同意需重新审查；同意绑定完整包及依赖内容，更新后必须重新确认。禁用同时撤销同意；重新加载只能复用仍与当前内容匹配的同意。运行中插件的包或依赖目录在磁盘上发生变化时，宿主在后台重新计算内容指纹，与启动时不一致即终止该插件并要求重新确认。

`package.toml` 的 `author`、`created_at`（可选 `YYYY-MM-DD`）、`licensed_to` 和 `license` 对所有能力一致适用。授权对象是作者声明的个人、组织或所有用户，不做账号绑定。缺失信息显示“未提供”，不推断作者身份或制作日期。

可选的 `package.integrity.json` 使用 Ed25519 签名覆盖保护策略、包内文件清单和受保护文件的 BLAKE3 摘要。`package.toml` 必须受保护；指定为 editable 的文件可修改，其余受保护内容或签名策略变化会拒绝加载。依赖包独立校验其签名，完整内容另由启用同意的指纹覆盖。签名包导出保留原始签名字节。源码始终可读，界面明确区分未签名、校验失败和签名校验通过，并显示脚本入口是否在保护范围内。

有效签名只表示内容与该公钥匹配，不代表作者身份认证；允许编辑的文件不保证仍为作者原作。去除签名不会保持“签名有效”状态，内容指纹也会变化，使旧的启用同意失效。此机制不是加密或不可绕过的 DRM。作者可在[包编辑器](package-editor.md)中选择保护文件并签发，私钥只保存在系统受保护存储中。

官方应用提供固定 Lua 解释器 Wasm，作者仅分发 UTF-8 Lua 源码。每个启用的插件在独立工作进程中运行；导入或引用资源不会自动执行脚本。禁用会终止工作进程并卸载对应视图，重新加载会创建新实例；Lua 内存状态不持久化。

解释器未连接 WASI 或操作系统实现。其编译产物的文件、进程等导入全部绑定为拒绝执行；环境接口只返回空环境，不传递宿主环境。插件没有网络、文件、外部程序、剪贴板、账户、持久化存储或系统访问 API。`io`、`os`、`package`、`debug`、`require`、`load`、`loadfile`、`dofile`、`string.dump` 均不提供；脚本不能加载本机模块、字节码或替换解释器。宿主 UI 协议不接受 URL、路径或任意消息派发。

默认上限：源码 128 KiB、单条 IPC 256 KiB、Wasm 线性内存 32 MiB、插件代码每次执行 2000 万 fuel 与约 50ms 中断预算、512 个组件、32 层组件嵌套、2048 条绘制指令、每条路径 256 个点、总计 8192 个几何点、组件文字合计 16 KiB、32 个排队事件、每插件最多约 30 次视图更新/秒（满预算画布约 6 次）、最多 8 个启用实例。宿主通过 Lua C API 直接读取视图，不运行插件代码或元方法，并逐项校验有限数值、尺寸、文本长度、ID 唯一性及未知字段；不信任子进程自行完成的验证。完整预算与实测结果见[插件性能、预算与隔离](plugin-performance.md)。

工作进程启动可等待 15 秒，正常事件响应期限 200ms；超时、崩溃和非法响应会终止该插件，进程随即回收。事件积压时工作进程连续处理，只发布最新视图，界面不会依次补放中间状态。固定解释器在 Rust 构建时预编译，启动时从应用内嵌产物加载；IPC、包审查、签名校验、导入导出均在后台进行，UI 线程不等待。Wasmtime 陷阱和未捕获的 Lua 异常会使实例失效；插件出错后需显式重新加载。Lua 的 `pcall` 可以捕获普通脚本异常，但不能绕过执行限额。

操作系统隔离：macOS 工作进程在读取请求前应用拒绝优先的 Seatbelt 配置，此前在 macOS 实测文件、网络和进程创建均被拒绝。Windows 由宿主放入作业对象，正式应用工作进程的启停及回收已验证；原生测试另核对内核中的限制，验证子进程创建被拒绝、256 MiB 提交内存上限及关闭作业句柄后的终止。桌面、剪贴板等 UI 限制目前只核对配置，未逐项执行破坏性系统操作。AppContainer 尚未实现，作业对象不隔离文件和网络。当前预算、测量方法及验证范围见[插件性能与隔离](plugin-performance.md)。

## 调试

[包编辑器](package-editor.md)可以在与正式插件相同的独立工作进程中调试草稿脚本。调试需要单独确认信任当前内容，任何编辑都会停止实例并撤销确认。测试事件由用户手动发送，返回的视图只显示在调试面板中。调试确认不是启用同意：导出并导入后，仍须在“插件与资源包”中审查并启用。

## 验证

`cargo test -p nanatype-plugins` 验证正常事件、不可用系统接口、恶意输入、死循环与内存耗尽；`cargo test -p nanatype-theme` 验证包格式、依赖、继承、导入导出及皮肤配置。全部皮肤仍须通过展示／历史／输入截图与默认、300px 窄窗布局检查。

解释器来源、校验值和许可证见 `crates/plugins/runtime/README.md`。插件运行时不需要用户安装 Lua、Node.js 或 Wasmtime。

2026-10-19 统一包迁移的验收（Windows x86_64）：28 个内置皮肤 × 展示／历史／输入共 84 张截图与迁移前逐像素一致，`chat_parity` 默认及 300px 窄窗布局全部通过。冷启动皮肤注册表由约 341 ms 降至 219 ms，内置清单在构建时生成解析数据；渲染宿主基准各场景中位耗时均在 ±0.002 ms 以内，未观察到明显退化。release 构建中解释器初始化约 2.1 ms，预热后单次事件中位 0.17 ms、P95 0.32 ms（VM 内执行，不含进程创建与 IPC，可用 `cargo run -p nanatype-plugins --release --example plugin_perf` 复测）。macOS 构建与签名尚未验证。

后续编辑器与 Windows 实机验收按[分阶段计划](package-roadmap.md)跟踪；未完成阶段不视为当前可用接口。
