# Strings only NanaType Editor shows (feature `editor`).

## 编辑器窗口
editor-app-title = NanaType 包编辑器
editor-app-subtitle = 新建、编辑、签名和调试 NanaType 包。导出的 .ntpkg 在 NanaType 中导入后才会生效。
editor-data-dir = 数据目录：{ $path }
editor-data-dir-open = 打开

# Package provenance and explicit script consent
pkgedit-title = 包编辑器
pkgedit-intro = 修改只保存在草稿里，导出 .ntpkg 并导入后才会生效。
pkgedit-kind-skin = 纯皮肤
pkgedit-kind-resources = 纯资源
pkgedit-kind-lua = 纯 Lua
pkgedit-kind-sounds = 纯音效包
pkgedit-kind-mixed = 混装
pkgedit-id-placeholder = 包 ID，例如 my.package
pkgedit-new = 新建
pkgedit-open-file = 打开 .ntpkg…
pkgedit-drafts = 草稿
pkgedit-drafts-none = 还没有保存的草稿。
pkgedit-open = 打开
pkgedit-discard = 丢弃草稿
pkgedit-discard-question = 永久删除这份草稿？已安装的包不受影响。
pkgedit-edit = 编辑
pkgedit-copy = 复制为新包
pkgedit-origin-new = 新建工程
pkgedit-origin-installed = 打开自已安装的包
pkgedit-origin-archive = 打开自文件
pkgedit-origin-copy = 衍生副本，来源
pkgedit-derived-from = 衍生自
pkgedit-derived-unsigned = 原签名已去除，此副本不代表原作者
pkgedit-unsaved = 有未保存的修改
pkgedit-saved-state = 草稿已保存
pkgedit-not-stored = 未修改
pkgedit-validating = 正在后台校验…
pkgedit-valid = 校验通过
pkgedit-errors = { $count } 个错误
pkgedit-save = 保存草稿
pkgedit-revert = 撤销到上次保存
pkgedit-export = 导出 .ntpkg…
pkgedit-close = 关闭
pkgedit-close-unsaved = 有未保存的修改。关闭前要保存为草稿吗？
pkgedit-close-save = 保存并关闭
pkgedit-close-discard = 不保存，关闭
pkgedit-tab-files = 文件
pkgedit-tab-package = 包信息
pkgedit-tab-signing = 保护与签名
pkgedit-tab-preview = 预览
pkgedit-import = 导入素材…
pkgedit-remove = 移除文件
pkgedit-remove-question = 从草稿中移除这个文件？
pkgedit-read-only = 只读
pkgedit-format = 格式化
pkgedit-search = 在文件中搜索
pkgedit-search-next = 下一个
pkgedit-search-none = 无匹配
pkgedit-no-file = 选择左侧的文件。
pkgedit-binary = 文件信息
pkgedit-file-format = 格式
pkgedit-file-size = 大小
pkgedit-file-dependency = 来自依赖包（只读）
pkgedit-data-declared = 声明为数据
pkgedit-problems = 问题（点击定位）
pkgedit-no-problems = 没有发现问题。
pkgedit-manifest-invalid = package.toml 无法解析，请先在“文件”中修正。
pkgedit-manifest-protected = package.toml 受签名保护，只能查看。创建衍生副本后才能修改。
pkgedit-version = 版本
pkgedit-capabilities = 能力（启用皮肤或 Lua 时会补上起始文件）
pkgedit-dependencies-hint = 依赖，每行一个“包ID@精确版本”；应用后从已安装的包打包进草稿。
pkgedit-apply = 应用
pkgedit-data = 声明的数据
pkgedit-data-none = 没有声明数据。
pkgedit-pipelines = 处理管线
pkgedit-pipelines-none = 没有声明处理管线。
pkgedit-data-missing = 文件不存在
pkgedit-data-add = 添加数据
pkgedit-data-id = 数据 ID
pkgedit-data-path = 文件路径
pkgedit-data-format = 格式
pkgedit-data-version = 格式版本
pkgedit-data-constraints = JSON 约束
pkgedit-data-constraints-hint = JSON 约束可选，只用于 JSON 数据，例如 {"{"}"type": "object", "required": ["items"]{"}"}。留空表示不限制。
pkgedit-entry-edit = 编辑
pkgedit-entry-remove = 删除
pkgedit-entry-remove-question = 删除这条声明？
pkgedit-pipeline-add = 添加管线
pkgedit-pipeline-name = 管线名称
pkgedit-pipeline-processor = 处理器
pkgedit-pipeline-inputs = 输入：填写数据 ID（依赖包的数据写 pkg:包ID/数据ID），或写 管线名.输出 接入另一条管线。
pkgedit-pipeline-params = 参数，每行一个 名称 = 数值
pkgedit-pipeline-outputs = 输出
pkgedit-pipeline-unknown = 此处理器未注册，只能在源码中修改。
pkgedit-pipeline-broken = 这条声明无法解析，请在 skin.toml 中修改。
pkgedit-pipelines-no-skin = 处理管线写在 skin.toml 中，启用皮肤能力后才能声明。
pkgedit-skin-protected = skin.toml 受签名保护，只能查看。创建衍生副本后才能修改。
pkgedit-seal-note = 签名只说明受保护文件与签名时一致，不能证明作者身份；可编辑文件修改后签名仍然有效。
pkgedit-key-load = 使用本机签名密钥
pkgedit-key-none = 本机还没有签名密钥。
pkgedit-key-create = 创建签名密钥
pkgedit-key-fingerprint = 本机签名密钥指纹
pkgedit-key-note = 私钥只保存在系统的受保护存储中（macOS 钥匙串、Windows DPAPI），不会写入包、日志、资源接口或插件进程。
pkgedit-protect = 选择要保护的文件。package.toml 总是受保护；签名后受保护文件只读，文件清单也会固定。
pkgedit-sign = 签发
pkgedit-signed-own = 此包由本机密钥签名。可以移除签名后修改，再重新签发。
pkgedit-signed-other = 此包由其他密钥签名，受保护文件只读。要修改它们，请创建去除原签名的衍生副本。
pkgedit-unseal = 移除签名以便修改
pkgedit-derive = 创建衍生副本
pkgedit-derive-hint = 衍生副本使用新的包 ID，记录来源并去除签名。
pkgedit-preview-none = 此工程没有皮肤，或皮肤还没通过校验。
pkgedit-debug-no-lua = 此工程没有声明 Lua 能力。
pkgedit-debug-risk = 调试会在独立的沙箱工作进程中运行当前草稿的脚本。它只能收到你发送的测试事件，结果只显示在这里；但脚本仍可能显示误导内容或占用资源。只运行你已经审查过的源码。修改任何文件都会停止脚本并需要重新确认。
pkgedit-debug-trust = 我已审查当前源码，信任它在调试进程中运行
pkgedit-debug-start = 启动
pkgedit-debug-stop = 停止
pkgedit-debug-clear = 清除结果
pkgedit-debug-events = 发送测试事件
pkgedit-debug-send = 发送
pkgedit-debug-invalid-json = 事件必须是带有字符串 type 字段的 JSON 对象。
pkgedit-debug-error = 脚本错误
pkgedit-debug-output = 返回的视图
pkgedit-debug-waiting = 等待脚本返回视图…
pkgedit-debug-idle = 脚本未运行。
pkgedit-debug-log = 记录
pkgedit-saved = 草稿已保存
pkgedit-open-failed = 无法打开工程
pkgedit-exported = 已导出
pkgedit-export-failed = 导出失败
pkgedit-debug-locate = 跳到出错位置

pkgedit-undo = 撤销

pkgedit-redo = 重做

pkgedit-discard-form = 放弃表单修改

pkgedit-recovery = 发现恢复快照

pkgedit-recover = 恢复

pkgedit-width = 宽度

pkgedit-height = 高度

pkgedit-simulate = 模拟 NanaType 个人配置

pkgedit-landscape = 横版

pkgedit-portrait = 竖版

pkgedit-narrow = 窄窗

pkgedit-default-size = 默认尺寸

pkgedit-static = 静态／停止

pkgedit-play = 播放

pkgedit-reduced = 减少动态

pkgedit-effects-off = 关闭高级效果

pkgedit-replay = 重播

pkgedit-fit = 适应窗口

pkgedit-checker = 透明棋盘

pkgedit-white = 白色背景

pkgedit-dark = 深色背景

pkgedit-speaker = 示例说话人

pkgedit-preview-stale = 当前内容尚未通过校验，显示上一有效预览

pkgedit-preview-size-error = 请输入有效逻辑尺寸：宽 160–8192，高 96–8192

pkgedit-operation-stale = 工程已发生变化，请重新应用本次操作


pkgedit-property-select = 请选择声明分组

pkgedit-up = 返回上一级

pkgedit-add-property-hint = 添加字段：名称与 TOML 值；分组可用内联表，列表使用数组

pkgedit-property-name = 字段名

pkgedit-add = 添加



pkgedit-inherit = 恢复继承

pkgedit-unbind = 明确解除参数绑定

pkgedit-bound-warning = 此值绑定了参数；替换前需要明确解除绑定

pkgedit-accept-disk = 接受磁盘版本

pkgedit-add-decor = 添加装饰

pkgedit-base = 基础皮肤



pkgedit-cover = 封面路径

pkgedit-description = 描述

pkgedit-disk-conflict = 磁盘冲突

pkgedit-duplicate = 复制元素

pkgedit-event-sequence = 测试事件序列（JSON 数组，最多 32 项）

pkgedit-file-target = 新建／移动到的包内路径

pkgedit-keep-local = 保留本地修改

pkgedit-layer-down = 下移一层

pkgedit-layer-up = 上移一层

pkgedit-layout-base = 基础布局

pkgedit-layout-rule = 断点规则

pkgedit-log-filter = 筛选日志

pkgedit-min-app = 最低应用版本

pkgedit-name = 名称

pkgedit-new-file = 新建文本文件

pkgedit-open-directory = 打开工程目录…

pkgedit-origin-directory = 目录工程

pkgedit-reference-auto = 清单引用，可自动修改

pkgedit-reference-manual = 程序引用，需要人工确认

pkgedit-rename = 重命名／移动

pkgedit-rename-question = 确认修改文件路径及以下引用；程序中的引用需要自行检查。

pkgedit-replace = 替换文件…

pkgedit-replace-all = 全部替换

pkgedit-replace-one = 替换当前匹配

pkgedit-replacement = 替换为

pkgedit-save-copy = 另存本地副本…

pkgedit-search-previous = 上一个

pkgedit-tags = 标签（逗号分隔）

pkgedit-unused-asset = 没有已识别的引用

pkgedit-tab-sounds = 音效

pkgedit-add-responsive = 创建横竖版布局

pkgedit-template = 填入模板

pkgedit-property-exists = 请输入一个尚未声明的字段名

pkgedit-reload-directory = 重新加载磁盘工程

pkgedit-source-too-large = 源码超过可编辑大小限制（1 MiB）。

pkgedit-layout-matched = 当前命中顺序，后层覆盖前层

pkgedit-bind = 绑定参数

pkgedit-pending-forms = 请先应用或放弃尚未提交的表单输入。

pkgedit-mode-design = 设计
pkgedit-mode-data = 数据与管线
pkgedit-mode-lua = Lua
pkgedit-project-menu = 工程菜单
pkgedit-toolbar = 编辑器工具栏
pkgedit-status = 工程状态
pkgedit-split-source = 画布旁显示源码
pkgedit-files-preview = 显示实时预览
pkgedit-drawer-close = 收起面板
pkgedit-custom-size = 自定义尺寸
pkgedit-stage-settings = 画布设置
pkgedit-stage-size = 预览逻辑尺寸（宽 × 高）
pkgedit-params-none = 这个皮肤没有声明参数。
pkgedit-preview-params = 预览参数
pkgedit-layers = 图层
pkgedit-canvas = 画布
pkgedit-inspector = 属性
pkgedit-add-field = 新增字段
pkgedit-section-source = 本节 TOML
pkgedit-section-reveal = 在源码中定位
pkgedit-section-inline = 这一节没有单独的表头，在源码中所在的表里修改。
pkgedit-file-operations = 新建、移动与替换
pkgedit-files-none = 没有这类文件。
pkgedit-lua-sources = Lua 源码
pkgedit-sound-files = 音频文件
pkgedit-sound-declarations = 音效声明
pkgedit-layer-dialogue = 对话框
pkgedit-layer-nameplate = 姓名牌
pkgedit-layer-portrait = 头像
pkgedit-layer-continue = 继续标识
pkgedit-layer-text = 正文
pkgedit-layer-decor = 装饰
pkgedit-layer-history = 历史
pkgedit-layer-chrome = 界面色
pkgedit-layer-effects = 效果
pkgedit-layer-fonts = 字体
pkgedit-layer-materials = 材质
pkgedit-layer-mention = 提及
pkgedit-layer-params = 参数
pkgedit-layer-param_groups = 参数分组
pkgedit-layer-presets = 预设
pkgedit-layer-shaders = 着色器
pkgedit-layer-sounds = 音效
pkgedit-layer-responsive = 响应式
pkgedit-layer-pipelines = 管线
pkgedit-layer-frame = 框体
