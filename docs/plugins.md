[简体中文](plugins.md) · [English](en/plugins.md) · [日本語](ja/plugins.md)

# 插件与统一包

这份说明面向皮肤作者、资源作者和 Lua 插件作者。建议先阅读[包编辑器](package-editor.md)，再按需要查 API 和安全限制。

## 入门

### 包的组成

包的身份来自 `package.toml`，皮肤样式来自 `skin.toml`。`capabilities` 可以组合：

- `skin`：皮肤与布局；
- `resources`：图片、字体、音效和声明式数据；
- `sounds`：音效包；
- `lua`：Lua 插件入口。

当前格式为 `format = 1`、`schema = 3`。依赖使用精确三段版本，资源使用本包相对路径或 `pkg:<包ID>/<路径>`。依赖缺失、冲突、循环和越界路径会拒绝加载。

### 最小 Lua 包

```toml
format = 1
schema = 3
id = "example.counter"
version = "1.0.0"
capabilities = ["lua"]

[lua]
entry = "main.lua"
reads = []
```

Lua 入口必须返回插件对象。下面的示例在每句台词开始时计数，并把结果显示在插件面板：

```lua
local count = 0

return {
    on_event = function(event)
        if event.type == 'line_start' then
            count = count + 1
        end
    end,
    view = function()
        return {views = {{slot = 'panel', root = {
            kind = 'text', id = 'counter', text = tostring(count)
        }}}}
    end,
}
```

完整示例位于 `docs/examples/counter-plugin/` 和 `docs/examples/phrase-plugin/`；打包示例可在 NanaType 的“插件与资源包”中导入。

## 操作

### 创建、安装和发布

1. 在 NanaType Editor 中新建包，填写 ID、版本、作者和许可证。
2. 添加 `package.toml`、资源、`skin.toml` 或 `main.lua`。
3. 在编辑器中校验、预览并测试，再导出 `.ntpkg`。
4. 在 NanaType 的“设置 › 插件与资源包”中导入。
5. 第一次启用时审查来源、权限、数据读取和签名状态，再明确同意。

导出会把依赖放到 `dependencies/<包ID>/` 并携带授权信息。Steam 创意工坊包是只读外部目录，不复制到应用的 `packages/`。

### 声明式数据

在 `package.toml` 中声明数据，并在 `[lua].reads` 列出插件需要读取的 ID：

```toml
[lua]
entry = "main.lua"
reads = ["phrases"]

[data.phrases]
path = "data/phrases.json"
format = "json"
format_version = 1
source = "作者自制"
license = "MIT"
constraints = { type = "object", required = ["phrases"], max_depth = 3 }
```

支持 `text`、`json` 和 `nthz`。格式按声明判断，不按扩展名猜测；导入时会校验字节大小、结构、深度、项目数、字符串长度和数值范围。

### 日志

使用 `nana.log(level, message)` 写入 NanaType 会话日志：

- `level` 为 `debug`、`info`、`warn` 或 `error`；
- `message` 必须是 UTF-8，超过 512 字节会在字符边界截断；
- `debug` / `info` 只在详细日志开启时记录，`warn` / `error` 始终记录；
- 日志会脱敏 URL 查询串、Cookie、令牌和用户目录。

不要把聊天内容、账号或其他个人信息写入日志。调试器会显示全部级别。

## API 参考

### 事件与命令

插件可以接收：

- 生命周期与数据：`init`、`data`；
- 播放：`line_start`、`line_end`、`speaking`、`translation`；
- 画面与联动：`expression`、`portrait`、`shake`、`vts`；
- 输入：`command`、`chat_message`（平台、作者和纯文本）。

事件字段是宿主生成的本地值，不包含账户凭据。通过 `commands = {{id = 'reset', label = 'Reset'}}` 声明命令；用户点击后收到 `{type = 'command', id = 'reset'}`。命令不能调用任意应用动作。

### 视图

视图挂载点：`display`、`history`、`input`、`panel`。组件类型：`column`、`row`、`text`、`button`、`canvas`。

- 每个组件必须有稳定且唯一的 `id`。
- 容器使用 `children`；`text` 只显示字面文本，不执行 HTML、链接或富文本。
- `button` 通过 `command` 回传事件，并继承宿主的键盘和无障碍交互。
- `canvas` 声明 `width`、`height` 和 `draw`，支持 `rect` 与 `path`；宿主会缓存绘制结果，Lua 不进入每帧绘制路径。
- 高级皮肤动效使用受限 WGSL，不通过 Lua 绘制。

### 数据查询

`nana.data.request(id, query)` 立即返回 token；结果随后以 `data` 事件返回。只能请求 `[lua].reads` 中的数据：

```lua
pending = nana.data.request('phrases', {
    kind = 'pointer',
    pointer = '/items/0',
})
```

查询类型：`info`、`whole`、文本 `lines`、JSON `pointer` / `slice`、NTHZ `glyph`。结果只有值，不包含路径、句柄或宿主对象；JSON `null` 在 Lua 中为 `nil`。

请求限制：每次分发最多 8 个请求，每个宿主事件最多 4 轮连锁请求；单个结果 64 KiB，每轮合计 120 KiB，`lines` / `slice` 每次最多 1024 行。超过预算会返回错误而不是截断；大数据应分块查询。未在下一个宿主事件前完成的请求会作废。

## 安全与生命周期

### 启用前审查

启用确认页会显示作者、制作日期、许可证、授权对象、脚本入口、可读取数据、包位置、签名状态和内容指纹。用户必须确认来源可信后，插件才会启动。

同意绑定完整包及依赖内容。内容、依赖或策略版本变化后必须重新审查；禁用会撤销同意。运行中的包被修改时，宿主会停止插件并要求重新确认。

### 沙箱与禁止能力

插件在独立 Wasm 工作进程中运行。它没有网络、文件、外部程序、剪贴板、账户、持久化存储或系统访问 API；不能加载本机模块、字节码或替换解释器。`io`、`os`、`package`、`debug`、`require`、`load`、`loadfile`、`dofile` 和 `string.dump` 均不可用。

macOS 使用拒绝优先的 Seatbelt 配置；Windows 使用作业对象。AppContainer 尚未实现，作业对象不隔离文件和网络。

### 签名

可选的 `package.integrity.json` 使用 Ed25519 签名覆盖保护策略、文件清单和受保护文件的 BLAKE3 摘要；`package.toml` 必须受保护。签名只证明内容与公钥匹配，不证明作者身份，也不是加密或 DRM。私钥由包编辑器保存在系统安全存储中。

### 运行限制

默认限制包括：源码 128 KiB、单条 IPC 256 KiB、Wasm 内存 32 MiB、单次执行约 2000 万 fuel / 50 ms、512 个组件、32 层嵌套、2048 条绘制指令、每条路径 256 个点、总计 8192 个几何点、文字总计 16 KiB、32 个排队事件和最多 8 个启用实例。视图更新约 30 次/秒，满预算画布约 6 次/秒。

工作进程启动最多等待 15 秒，普通事件响应期限为 200 ms。超时、崩溃、死循环或非法响应会终止并回收进程；插件不会自动重启。`pcall` 可以捕获普通脚本异常，但不能绕过执行限额。

## 调试与验证

包编辑器可以在正式插件同样的独立工作进程中调试草稿。调试需要单独确认当前内容，修改文件会停止实例；调试返回的视图只显示在调试面板，不会获得正式启用权限。

不需要用户安装 Lua、Node.js 或 Wasmtime。修改示例后可运行：

```bash
python scripts/assets/package-example-plugins.py
cargo test -p nanatype-plugins
cargo test -p nanatype-theme
```

当前 API、包格式和安全边界以源码校验与测试为准。性能、预算、隔离方式和平台验证的详细测量不在本页维护，见[插件性能、预算与隔离](plugin-performance.md)。
