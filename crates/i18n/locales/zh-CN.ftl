## 应用
app-name = NanaType
app-tagline = 虚拟主播打字机
history-count = { $count } 条

## 窗口标题
window-input = 输入框
input-management = 直播与联机
dock-tab-commands = 命令
dock-stickers = 表情包
dock-add-region = 添加区域
dock-close-region = 关闭当前区域
dock-more-regions = 更多区域
dock-wait = 停顿 0.5s
window-display = 展示框
window-history = 历史记录
window-settings = 设置

## 托盘
tray-mode = 输入模式
tray-skin = 皮肤
tray-quit = 退出 NanaType
tray-action-show-input = 显示输入框
tray-action-hide-input = 隐藏输入框
tray-action-show-display = 显示对话框
tray-action-hide-display = 隐藏对话框
tray-action-show-history = 显示历史记录
tray-action-hide-history = 隐藏历史记录
tray-action-show-chat = 打开弹幕面板
tray-action-hide-chat = 关闭弹幕面板
tray-action-lock = 锁定叠加窗口
tray-action-unlock = 解锁叠加窗口
tray-action-continue = 继续
tray-action-sample = 播放示例台词
tray-action-settings = 打开设置…
tray-action-obs = 接入 OBS…
tray-tooltip = NanaType — { $mode } · { $skin }
tray-tooltip-error = NanaType · ⚠ { $message }

## 输入模式
mode-compose = 编辑后发送
mode-compose-hint = 编辑完成后发送，展示框以打字机效果播放
mode-mirror = 实时镜像
mode-mirror-hint = 展示框实时跟随输入内容
mode-compose-short = 编辑
mode-mirror-short = 镜像

## 工具栏
tb-bold = 粗体
tb-italic = 斜体
tb-underline = 下划线
tb-strike = 删除线
tb-color = 文字颜色
tb-size = 字号
tb-stroke = 描边
tb-shadow = 阴影
tb-stroke-none = 去除描边
tb-shadow-none = 去除阴影
tb-ruby = 注音
tb-ruby-placeholder = 输入注音，例如 かんじ
tb-ruby-apply = 添加注音
tb-ruby-remove = 移除注音
tb-effects = 动效
tb-effect-shake = 抖动
tb-effect-wave = 波浪
tb-effect-jump = 跳跃
tb-effect-rainbow = 彩虹
tb-entrance-skin = 跟随皮肤
tb-entrance-none = 无
tb-entrance-fade = 淡入
tb-entrance-pop = 弹出
tb-entrance-slide = 上浮
tb-pacing = 节奏
tb-wait-ms = 停顿 { $ms } 毫秒
tb-speed-slow = 放慢
tb-speed-fast = 加快
tb-speed-instant = 瞬间显示
tb-speed-reset = 恢复速度
tb-pause = 等待继续
tb-pause-auto = 暂停后自动继续
tb-page = 翻页
tb-insert = 插入
tb-sticker = 贴纸
tb-sticker-search = 搜索贴纸
tb-sticker-series = 表情包系列
tb-sticker-all = 全部
tb-sticker-drop-hint = 将文件夹拖到此处添加表情包
tb-sticker-drop-now = 松开以导入表情包文件夹
tb-sticker-importing = 正在导入 { $name }…
tb-image = 图片…
tb-sound = 音效
tb-expression = 表情
tb-expression-placeholder = 表情名称，例如 smile
tb-screenshake = 震屏
tb-phrases = 快捷短语
tb-source = 标记源码
tb-send = 发送
tb-clear-format = 清除格式
tb-more = 更多

## 编辑器
editor-placeholder = 在这里输入台词…
input-plain-text = 纯文本
input-skin-preview = 皮肤预览
editor-source-placeholder = 使用 [b]标记[/b] 编辑，例如 [shake]哇！[/shake][wait=500]
editor-diagnostic = 第 { $pos } 个字符：{ $message }
marker-wait = 停 { $ms }ms
marker-pause = 等待
marker-pause-auto = 等 { $ms }ms
marker-page = 翻页
marker-sound = 音 { $name }
marker-expression = 表情 { $name }
marker-portrait = 立绘 { $name }
marker-shake = 震屏
marker-vts = VTS { $name }
status-mirror = 镜像中
status-queued-chip = 排队 { $position }
status-sent = 已发送
empty-phrases = 还没有快捷短语
empty-stickers = 还没有贴纸目录
empty-open-assets = 去 设置 › 素材 添加
empty-vts-hotkeys = 还没有读取到 VTube Studio 热键

## 设置导航
settings-general = 常规
settings-playback = 播放
settings-skins = 皮肤
settings-windows = 窗口
settings-history = 历史记录
settings-hotkeys = 快捷键
settings-output = 输出 / OBS
settings-integrations = 联动
settings-assets = 素材
settings-about = 关于

## 常规
general-language = 界面语言
general-language-system = 跟随系统
general-speaker = 角色名
general-speaker-hint = 显示在名牌和历史记录中，留空则隐藏名牌
general-avatar = 头像
general-avatar-pick = 选择图片…
general-avatar-clear = 移除
general-presence = 出现方式
general-presence-resident = 常驻
general-presence-summon = 快捷键唤出
general-presence-hint = 唤出模式下，发送或按 Esc 后输入框自动隐藏
general-input = 输入
general-mode = 输入模式
general-send-key = 发送键
general-send-key-enter = Enter
general-send-key-ctrl-enter = Ctrl+Enter
general-send-key-hint = 另一种组合用于换行
general-autostart = 开机启动

## 播放
playback-speed = 打字速度
playback-typing = 打字机效果
playback-typing-hint = 关闭后整句直接显示
playback-punct = 标点停顿
playback-punct-hint = 逗号、句号和省略号后稍作停顿
playback-entrance = 文字出场
playback-after = 播放结束后
playback-flow = 台词流转
playback-after-hold = 保持显示
playback-after-fade = 渐隐消失
playback-after-clear = 直接清除
playback-after-delay = 停留时长
playback-after-delay-value = { $seconds } 秒
playback-auto = 自动推进下一句
playback-auto-off = 关闭
playback-auto-value = { $seconds } 秒后
playback-queue = 正在播放时发送新台词
playback-queue-replace = 立即替换
playback-queue-finish = 先完整显示当前句
playback-queue-queue = 排队等待继续键
playback-queue-replace-short = 立即替换
playback-queue-finish-short = 先播完
playback-queue-queue-short = 排队
playback-reduced = 减少动态效果
playback-reduced-hint = 关闭逐字动效与震屏，适合对动态敏感的观众
playback-stagger = 镜像模式字间隔
playback-sound = 打字音效
playback-audio = 声音
playback-volume = 主音量
playback-volume-typing = 打字音量
playback-volume-effects = 音效音量
playback-device = 输出设备
playback-device-default = 系统默认

## 皮肤
skins-active = 当前
skins-library = 皮肤库
skins-import = 导入皮肤…
skins-export = 导出…
skins-duplicate = 复制为可编辑
skins-open-folder = 打开文件夹
skins-delete = 删除
skins-delete-confirm = 删除皮肤「{ $name }」？此操作无法撤销。
skins-builtin = 内置
skins-broken = 无法加载
skins-by = 作者 { $author }
skins-version = 版本 { $version }
skins-overrides = 个性化
skins-override-name = 名牌颜色
skins-override-box = 展示框颜色
skins-override-accent = 强调色
skins-override-overflow = 长文本
skins-override-overflow-hint = 一句话超出展示框高度时的处理方式
skins-override-overflow-skin = 跟随皮肤
skins-override-overflow-clip = 裁掉
skins-override-overflow-scroll = 跟随滚动
skins-override-overflow-page = 分页（等待后清空）
skins-override-page-auto = 满页自动翻页
skins-override-page-auto-hint = 分页模式下满一页后等待的秒数；0 表示等继续键
skins-override-reset = 恢复皮肤默认
general-appearance = 界面外观
skins-interface = 界面
general-appearance-system = 跟随系统
general-appearance-light = 浅色
general-appearance-dark = 深色
skins-hot-reload = 文件变化时自动重载
skins-advanced-effects = 皮肤高级效果
skins-advanced-effects-hint = 启用纸纹、笔触与光效；减少动态效果时保留静态纹理
skins-play-effects = 播放动效
skins-stop-effects = 停止动效
skins-hot-reload-hint = 制作皮肤时使用
skins-installed = 已安装「{ $name }」
skins-replaced = 已将「{ $name }」从 { $previous } 更新
skins-import-failed = 皮肤无法导入
skins-fallback = 皮肤「{ $id }」无法加载，已改用默认皮肤
skins-preview = 预览
skins-preview-name = 奈奈
skins-filter-all = 全部
skins-tag-light = 浅色
skins-tag-dark = 深色
skins-tag-classic = 经典
skins-tag-streaming = 直播
skins-tag-game = 游戏
skins-tag-retro = 复古
skins-tag-cute = 可爱
skins-tag-elegant = 雅致
skins-tag-minimal = 简约

## 窗口
windows-input = 输入框
windows-display = 展示框
windows-history = 历史记录
windows-visible = 显示
windows-lock = 交互
windows-lock-interactive = 可交互
windows-on-top = 置顶
windows-background = 捕获背景
windows-background-transparent = 透明
windows-background-chroma = 色键
windows-reset = 重置位置
windows-size = 尺寸
windows-size-skin = 跟随皮肤

## 叠加窗口右键菜单
overlay-menu-skin = 切换皮肤（全局）
overlay-menu-adjust-skin = 调整皮肤…
overlay-menu-lock = 锁定窗口（鼠标穿透；从托盘或设置解锁）
overlay-menu-reset = 恢复默认位置与尺寸
overlay-menu-close = 关闭窗口
overlay-menu-continue = 继续播放
overlay-menu-skip = 跳过当前播放
overlay-menu-clear-display = 清空展示
overlay-menu-confirm-clear = 确认清空
overlay-menu-clear-history-peer = 清空历史记录（联机期间不可用）

## 历史
history-max = 保留条数
history-visible = 显示条数
history-fade = 自动淡出
history-avatar = 显示头像
history-name = 显示名字
history-time = 显示时间
history-sync = 出现时机
history-sync-submit = 发送时
history-sync-reveal = 随打字同步
history-sync-complete = 显示完成后
history-hide-latest = 隐藏最新一条
history-hide-latest-hint = 展示框上还显示着的那句先不进历史，展示框清空或换下一句后再出现（同 Echo-Live）
history-hide-latest-skin = 跟随皮肤
history-hide-latest-always = 隐藏
history-hide-latest-never = 不隐藏
history-clear = 清空历史记录
history-clear-confirm = 清空本次的全部历史记录？
history-session = 本场历史记录

## 快捷键
hotkeys-summon = 唤出输入框
hotkeys-continue = 继续
hotkeys-skip = 跳过当前句
hotkeys-clear = 清空展示框
hotkeys-toggle-mode = 切换输入模式
hotkeys-toggle-overlays = 显示/隐藏叠加窗口
hotkeys-record = 录制
hotkeys-clear-binding = 清除
hotkeys-ok = 已注册
hotkeys-conflict-os = 被其他程序占用
hotkeys-conflict-dup = 与「{ $action }」重复
hotkeys-unsafe = 单个按键会影响正常打字，请加上 Ctrl、Alt 或 Shift
hotkeys-badge-dup = 重复
hotkeys-badge-conflict = 冲突
hotkeys-badge-unsafe = 不安全
hotkeys-click-to-set = 点击设置
hotkeys-press-keys = 请按下按键 · Esc 取消
hotkeys-local = 输入框内快捷键
hotkeys-local-hint = Ctrl+B/I/U 设置样式，Ctrl+↑/↓ 调出已发送的台词，Esc 隐藏输入框

## 输出
output-display = 展示框
output-history = 历史记录
output-spout = Spout2 输出
output-syphon = Syphon 输出
output-spout-name = 发送端名称
output-spout-copy = 复制名称
output-spout-size = 输出分辨率
output-spout-size-window = 跟随窗口
output-spout-straight = 直通 Alpha
output-spout-straight-hint = 接收端不支持预乘 Alpha 时开启
output-status-active = 正在发送
output-status-unavailable = 不可用：{ $reason }
output-spout-unsupported = Spout2 仅支持 Windows，请使用窗口捕获
output-syphon-missing = 未找到 Syphon.framework
output-syphon-publish = 发布画面失败
output-sample = 播放示例台词
output-sample-line = [b]你好！[/b]这是 NanaType 的[wave]示例台词[/wave]。[wait=300][shake]看得到吗？[/shake]
output-obs-title = 在 OBS 中接入
output-obs-window = 也可以使用「窗口捕获」（捕获方式选 Windows 10 (1903 及以上)），展示框窗口背景为透明。
output-obs-mac = 也可以使用「macOS 屏幕捕获」并选择展示框窗口。

## 联动
int-vts = VTube Studio
int-vts-hint = 打字到 [expr=名称] 或 [vts …] 标记时，开启 VTube Studio 表情或触发热键
int-vts-port = 端口
int-vts-authorize = 请求授权
int-vts-authorize-hint = 点击后请在 VTube Studio 中允许 NanaType 插件
int-vts-map = 表情映射
int-vts-map-expression = 标记
int-vts-map-hotkey = VTS 热键
int-vts-map-auto = 自动（同名）
int-vts-refresh = 刷新热键列表
int-vts-lipsync = 口型同步
int-vts-lipsync-hint = 打字时让模型的嘴动起来
int-vts-mouth-param = 嘴部参数
int-vts-mouth-param-hint = 说话时驱动的输入参数（默认 MouthOpen）
int-vts-mouth-amp = 张嘴幅度
int-vts-reset = 表情复位
int-vts-reset-hint = 关闭一句台词中开启的表情
int-vts-reset-line-end = 台词离开展示框时
int-vts-reset-next-line = 下一句开始时
int-vts-reset-off = 不复位
int-osc = OSC
int-osc-hint = 向 VNyan、Warudo、VSeeFace 等工具发送 OSC 消息
int-osc-host = 目标地址
int-osc-port = 端口
int-osc-prefix = 地址前缀
int-ws = WebSocket 广播
int-ws-hint = 在本机端口广播台词与表情事件（JSON）
int-ws-port = 端口
int-ws-clients = 已连接 { $count } 个客户端
int-status-off = 未启用
int-status-connecting = 连接中…
int-status-connected = 已连接
int-status-needs-token = 需要授权
int-status-error = 错误：{ $message }

int-off-hint = 开启后可在此设置连接选项
output-off-hint = 开启后可在此设置名称与尺寸
## 素材
telegram-stickers = Telegram 贴纸
telegram-hint = 使用 Telegram 官方搜索及整包下载。首次登录需在 my.telegram.org 获取 API ID 和 API Hash；再次连接可恢复会话。支持 WebP、TGS、WebM，下载后离线使用。
telegram-api-id = API ID
telegram-api-hash = API Hash
telegram-phone = 手机号（含 + 国家代码）
telegram-proxy = 下载代理
telegram-proxy-hint = 留空直连；支持 SOCKS5（含认证，账号密码会以明文保存在设置中）。修改后重新连接，登录、搜索和下载均通过此代理。
telegram-connect = 连接 / 登录
telegram-code = Telegram 验证码
telegram-password = 两步验证密码
telegram-verify = 验证
telegram-working = 正在连接或处理请求…
telegram-offline = 尚未连接 Telegram
telegram-code-sent = 已发送验证码，请查看 Telegram 或短信。
telegram-password-required = 此账号已启用两步验证，请输入密码。
telegram-ready = 已登录，可搜索并下载贴纸包。
telegram-query = 搜索词、贴纸包名称或 t.me/addstickers 链接
telegram-search = 搜索贴纸包
telegram-download = 按链接 / 名称下载
telegram-no-results = 未找到贴纸包，请尝试其他关键词或直接粘贴链接。
telegram-logout = 退出登录并清除会话
telegram-source = Telegram 来源
telegram-count = { $count } 个贴纸
assets-stickers = 表情包
assets-stickers-hint = 下载预设或添加本地文件夹，支持 Echo-Live 表情包。导入时准备图片与动画缓存，完成后即可离线使用。
stickers-preset-pixel = 绵羊的像素头像（静态）
stickers-preset-animated = 绵羊的动态表情包
stickers-not-downloaded = 未下载
stickers-downloading = 下载中 { $done } / { $total }
stickers-processing = 正在准备缓存 { $done } / { $total }
stickers-ready = 可用 · { $count } 个表情
stickers-failed = 处理失败：{ $message }
stickers-repository = GitHub 仓库
stickers-download = 下载
stickers-retry = 重试
assets-add-folder = 添加目录…
assets-remove = 移除
assets-remove-folder-confirm = 从贴纸中移除这个文件夹？
assets-phrases = 快捷短语
assets-phrase-label = 名称
assets-phrase-markup = 内容（支持标记）
assets-phrase-send = 直接发送
assets-phrase-add = 添加短语
assets-phrase-remove-confirm = 删除这条短语？
assets-phrase-hotkey = 快捷键

## 关于
about-build = 构建 { $build }
about-logs = 打开日志文件夹
about-data = 打开数据文件夹
about-homepage = 项目主页

## 首次启动
onboard-welcome = 欢迎使用 NanaType
onboard-welcome-body = 只需几步即可开始在直播中使用打字机。
onboard-language = 选择语言
onboard-speaker = 你的角色名
onboard-skin = 选择皮肤
onboard-hotkeys = 快捷键
onboard-hotkeys-body = 按 { $summon } 唤出输入框，按 { $continue } 继续播放。
onboard-obs = 接入 OBS
onboard-next = 下一步
onboard-back = 上一步
onboard-skip = 跳过
onboard-done = 开始使用

## 通用
common-cancel = 取消
common-apply = 应用
common-reset = 重置
common-copied = 已复制
common-ms = { $ms } 毫秒
common-skin-default = 跟随皮肤

## 错误与提示
err-settings-quarantined = 设置文件已损坏，已备份到 { $path } 并使用默认设置
err-settings-newer = 设置文件来自更新版本的 NanaType，本次修改不会保存
err-audio = 音频设备不可用：{ $message }
err-image = 图片无法读取：{ $message }
err-spout = Spout 输出失败：{ $message }
err-syphon = Syphon 输出失败：{ $message }
# Bilibili
window-chat = 弹幕
chat-reply = 回复
chat-at-short = 提及
chat-style-compact = 紧凑
chat-style-history = 气泡
chat-panel-style = 操作面板样式
chat-replying = 回复 { $name }
chat-reply-cancel = 取消回复
chat-clear = 清空
chat-filter-text = 弹幕
chat-filter-super-chat = 醒目留言
chat-filter-gift = 礼物
chat-filter-member = 上舰
chat-empty = 等待弹幕…
chat-not-configured = 还没有连接直播间
chat-gift = 赠送 { $gift } ×{ $count }
chat-member = 开通了 { $guard } ×{ $count }{ $unit }
chat-guard-governor = 总督
chat-guard-admiral = 提督
chat-guard-captain = 舰长
chat-min-gift = 礼物最低价值
chat-min-gift-hint = 单位：元；免费礼物价值为 0
chat-status-retrying = 连接断开，正在重试：{ $message }
history-chat = 弹幕
history-chat-show = 在历史记录中显示弹幕
history-chat-show-hint = Bilibili 的弹幕、醒目留言、礼物与上舰会和台词一起出现在历史记录里
history-show-quote = 显示回复引用
history-show-quote-hint = 回复弹幕的台词上方显示被回复的弹幕
history-role-host = 主播
history-role-viewer = 观众
int-bilibili-open-panel = 打开弹幕面板
int-bilibili-quote-display = 对话框显示回复引用

## wave3-input
tb-cheatsheet = 标记速查
tb-stroke-white = 白色描边
tb-stroke-black = 黑色描边
tb-stroke-accent = 主题色描边
tb-shadow-drop = 投影
tb-shadow-glow = 发光
cheat-markers = 标记
cheat-shortcuts = 快捷键
cheat-wait = 停顿指定毫秒后继续
cheat-pause = 停住，按继续后再往下打
cheat-page = 翻到新的一页
cheat-expr = 切换立绘表情
cheat-se = 播放音效
cheat-sticker = 插入贴纸
cheat-at = 提到一位观众
cheat-newline = 换行
cheat-recall = 上一句 / 下一句
cheat-hide = 隐藏输入窗
## end wave3-input

## wave3-settings
settings-group-input = 输入
settings-group-look = 外观
settings-group-stream = 直播
settings-general-sub = 语言、你的名字，以及输入框的行为
settings-playback-sub = 台词如何逐字出现、推进与发声
settings-skins-sub = 为对话框挑选外观，并按你的喜好调整
settings-windows-sub = 显示、锁定或重置展示框、历史记录和输入框
settings-history-sub = 历史记录保留和显示哪些内容
settings-hotkeys-sub = 全局快捷键，在游戏或 OBS 里同样有效
settings-output-sub = 把展示框和历史记录送进 OBS
settings-integrations-sub = 弹幕、VTube Studio 与其他应用联动
settings-assets-sub = 输入框可用的贴纸和快捷短语
settings-about-sub = 版本、文件夹与链接
output-obs-step-plugin = 为 OBS 安装 Spout2 插件
output-obs-step-enable = 先在 NanaType 开启展示框输出，等待状态就绪：
output-obs-enable = 开启展示框输出
output-obs-enabled = 展示框输出已开启
output-obs-step-source = 添加「Spout2 Capture」来源，选择这个发送端：
output-obs-step-alpha = 将合成模式设为「预乘 Alpha」
output-obs-step-syphon = 在 OBS 中添加「Syphon Client」来源（macOS 版 OBS 自带）
output-obs-step-syphon-source = 选择这个发送端：
output-obs-other = 其他方式
int-vts-authorized = 已授权
int-vts-map-empty = 连接并授权后即可列出你的 VTS 热键
skins-more = 更多操作
## end wave3-settings

## wave4-skins
skins-view-grid = 卡片
skins-view-list = 列表
windows-lock-locked-short = 锁定
windows-lock-tip = 可交互（默认）：按住鼠标左键拖动以移动，滚轮缩放，拖动边缘调整尺寸。锁定：鼠标操作穿透到下层窗口。
windows-layout-hint = 展示框、联机伙伴展示框和历史记录窗口默认可直接操作：按住窗口内任意位置拖动即可移动；滚轮向上放大、向下缩小，保持宽高比例。右键可关闭窗口、切换或调整全局皮肤、置顶、锁定和恢复默认布局；展示框另有播放操作，历史记录另有显示选项和清空确认。位置和尺寸会自动保存。锁定后鼠标操作会穿透，请从托盘或设置解锁。关闭窗口不停止已启用的 OBS 输出。
windows-background-tip = 透明：为 OBS 保留透明度。色键：用绿色填满窗口，供不支持透明度的捕获方式抠像。
windows-reset-tip = 重置按钮会把窗口移回皮肤设定的位置，并恢复皮肤的尺寸
windows-input-na = “—”表示不适用：输入框始终可交互，也不会被捕获。
history-window-link = 窗口与捕获
history-window-link-hint = 显示、锁定历史记录窗口并调整尺寸，或发送到 OBS
## end wave4-skins

## wave4-onboarding
onboard-step = 第 { $step } 步，共 { $total } 步
onboard-skin-hint = 为对话框挑一个外观，下方实时预览。更多调整在「皮肤」页。
onboard-mode-body = 选择台词怎样进入展示框，之后随时可在输入框里切换。
onboard-obs-hint = 通过 Spout2 把展示框送进直播画面
onboard-reopen = 之后可以在「关于」页重新打开这份引导。
about-setup-guide = 打开新手引导
playback-reduced-system-on = 系统设置：开
playback-reduced-system-off = 系统设置：关
## end wave4-onboarding

## wave5-fonts
skins-fallback-font = 替代字体
## end wave5-fonts

## wave5-fonts-licences
about-fonts = 开源字体
about-fonts-hint = 内置皮肤随附的字体，以及它们的许可全文
about-fonts-show = 查看许可
about-fonts-hide = 收起
about-fonts-author = 作者：{ $author }
about-fonts-version = 版本：{ $version }
about-fonts-license-other = 许可见下文
about-interfaces = 接口来源与许可
about-interfaces-hint = 弹幕接口依据与签名实现来自以下社区项目，NanaType 仅负责接入、移植和适配。
about-design = 设计来源
about-design-echo = Echo-Live 原版、玻璃、气泡、虚空四套皮肤的设计参考自 sheep-realms/Echo-Live，原作者为 Sheep-realms 及项目贡献者。NanaType 使用本地皮肤格式实现和适配这些样式。
about-design-upstream-license = 原项目授权说明
about-license-project = 原创代码采用 MIT；继承资料保留原许可。B 站组件及完整应用同时包含 CC BY-NC 4.0 材料，须遵守其署名及非商业条件。
about-source-api-collect = 原始 B 站协议资料来源：SocialSisterYi 及项目贡献者。继承的直播协议说明与 WBI 材料经过适配，保留 CC BY-NC 4.0 署名及非商业条件。原仓库目前不可用，可访问镜像。
about-source-douyin = 抖音 WebSocket 签名与 HTTP a_bogus 实现移植自版本 0e506f3，连接参数参考同版本。Copyright (c) 2025 今晚打打。
about-source-repository = 来源仓库
about-source-mirror = 文档镜像
## end wave5-fonts-licences
## round2-skins
skins-fallback-font-missing = 未安装 { $family }
## end round2-skins
## round2-status
link-fault-refused = 对方程序没有在运行（连接被拒绝）
link-fault-timeout = 连接超时
link-fault-closed = 对方断开了连接
link-fault-port-in-use = 端口 { $port } 已被其他程序占用
link-fault-unresolved = 找不到主机 { $host }
link-fault-silent = 长时间没有收到数据
link-fault-remote = 对方报告：{ $message }
link-fault-remote-empty = 对方拒绝了请求
output-status-native-unavailable = 显卡不支持共享纹理输出
output-status-failed = 无法生成输出画面
spout-error-backend = Spout 组件无法启动
spout-error-interop = 显卡之间无法共享画面
spout-error-format = 画面格式或尺寸不受支持
spout-error-publish = 画面没能发送出去
spout-error-name = 发送端名称无效
err-tray = 托盘图标不可用：{ $message }
skins-export-failed = 皮肤导出失败：{ $message }
skins-delete-failed = 皮肤删除失败：{ $message }
## end round2-status

## Chat interaction
chat-at-action = 艾特
chat-latest = 回到最新
chat-inserted = 已插入 @{ $name }
chat-queued = 已排队 · { $count } 条
chat-room-owner = 房主
chat-moderator = 房管

# Two-person sessions
peer-title = 双人联机
peer-expand = 展开
peer-collapse = 收起
peer-off = 未联机
peer-connecting = 正在连接…
peer-waiting = 等待对方加入
peer-reconnecting = 正在重连…
peer-direct = 直连
peer-relay = 中继
peer-error = 联机失败
peer-create = 创建联机
peer-join = 加入
peer-leave = 离开联机
peer-copy = 复制邀请
peer-invitation = 粘贴对方的联机邀请
peer-relay-placeholder = 自定义中继 URL（留空使用公共中继）
peer-own-controls = 自己与伙伴使用独立展示窗口；快捷键只控制自己的台词。
peer-paused = 等待联机恢复后再发送或控制播放；可以继续编辑草稿。

# Independent partner output
window-peer-display = 联机伙伴展示框
windows-peer-display = 联机伙伴
output-peer-display = 展示框 · 联机伙伴
peer-window-toggle = 伙伴窗口
output-name-conflict = 输出名称重复，请为每个输出设置不同的名称。

output-peer-hint = 联机期间独立输出伙伴画面；隐藏窗口仍会输出。

tb-effect-pulse = 脉动
tb-effect-flicker = 闪烁

# Editor context menu
edit-undo = 撤销
edit-redo = 重做
edit-cut = 剪切
edit-copy = 复制
edit-paste = 粘贴
edit-select-all = 全选

# Bilibili direct connection and authenticated delivery
settings-bilibili = B 站直播
settings-bilibili-sub = 登录账号、直播间弹幕与台词同步发送
bili-account = B 站账号
bili-login = 扫码登录 B 站
bili-manage = 管理 B 站直播
bili-login-needed = 请先登录 B 站
bili-restoring = 正在验证登录状态…
bili-expired = 登录已失效，请重新扫码
bili-login-failed = 登录暂不可用
bili-qr-waiting = 使用哔哩哔哩 App 扫码登录
bili-qr-scanned = 已扫码，请在手机上确认
bili-qr-expired = 二维码已过期
bili-qr-refresh = 刷新二维码
bili-refresh = 刷新账号资料
bili-logout = 退出登录
bili-sync-profile = 使用 B 站昵称和头像
bili-sync-profile-hint = 自动更新主播身份，关闭后恢复自定义昵称和头像
bili-identity-hint = 主播昵称和头像由 B 站账号同步，可在 B 站直播设置中关闭。
bili-room = 目标直播间
bili-room-placeholder = 留空使用本人直播间
bili-room-hint = 接收和发送使用同一直播间，支持直播间短号
bili-own-room = 使用我的直播间
bili-connect = 连接
bili-receive = 接收直播间弹幕
bili-send-section = 台词同步发送
bili-send-switch = 同步弹幕
bili-send-hint = 仅同步主动提交的台词，程序启动时默认关闭
bili-send-title = 开启台词同步发送？
bili-send-warning = 开启后，提交的台词将同时发送到此直播间。图片和贴图不可用；文字样式与特殊演出在弹幕中转为纯文本，长台词自动分条发送。本地展示保留原有演出。
bili-enable = 开启同步
bili-images-unavailable = 同步弹幕时不能使用图片或贴图，请先移除图片并保留可读文字。
bili-panel = 弹幕面板
bili-queued = 等待发送
bili-sending = 正在发送
bili-submitted = B 站已接受提交
bili-failed = 发送失败
bili-uncertain = 发送结果不确定，请勿直接重发
bili-cancelled = 已取消后续发送
bili-delivery-failed = 弹幕未能完成同步
bili-copy-remaining = 复制未确认发送的文字
bili-retry = 重试未成功部分
bili-skip-failed = 跳过此条，继续队列
bili-dedup-unconfirmed = 部分消息未返回标识，无法确认回流去重
bili-invalid-room = 请输入有效的正整数直播间号，或留空使用本人直播间。
bili-results = 发送记录
bili-queue-full = 弹幕队列已满，这条台词仅在本地展示。

# Douyin live chat
settings-douyin = 抖音直播
settings-douyin-sub = 使用原生 Rust 接收抖音直播弹幕。
dy-room = 直播间
dy-room-hint = 输入 live.douyin.com 网址或房间号；修改后点击连接。
dy-enabled = 接收弹幕
dy-native = 原生 Rust
dy-connect = 连接 / 重连
dy-open-panel = 打开弹幕面板
dy-account = 登录会话
dy-account-hint = 公开房间可匿名接收。Cookie 仅在账号验证成功后保存；收到弹幕不代表已登录。
dy-logout = 退出并清除凭据
dy-import = 验证并导入 Cookie
dy-cookie-placeholder = 粘贴完整 Cookie（可选）
dy-off = 未连接
dy-connecting = 正在连接，等待直播间消息
dy-connected = 已连接
dy-disconnected = 已断开，请点击重连
dy-error = 连接失败
dy-account-ready = 已验证登录
dy-account-out = 未登录
dy-account-unverified = 登录状态待确认
dy-account-expired = 登录已失效
dy-invalid-room = 请填写有效的抖音直播间网址或房间号。
dy-storage-error = 系统保护存储不可用，未保存凭据。
dy-clear-error = 无法清除登录凭据，请重试。
dy-room-offline = 直播间未开播。
dy-invalid-frame = 跳过了一条无法解析的直播消息。
dy-no-messages = 未收到可验证的直播消息。
dy-native-error = 原生连接失败，请检查网络、直播间状态或 Cookie 后重连。
dy-diamonds = { $count } 抖币

tb-inherit = 跟随皮肤
tb-mixed = 混合
tb-size-relative = 相对字号
tb-size-pixels = 像素字号
tb-apply-relative = 应用倍率
tb-apply-pixels = 应用像素
tb-font = 字体
tb-font-body = 正文
tb-font-name = 名称
tb-font-accent = 强调
tb-font-family = 输入字体族名称
tb-align = 段落对齐
tb-align-start = 左对齐
tb-align-center = 居中
tb-align-end = 右对齐
tb-current-value = 当前：{ $value }

# Skin preview editor
skin-edit-font = 正文字体
skin-edit-font-presets = 字体选项
skin-edit-size = 正文字号（8–200 px）
skin-edit-line = 行高（1–3 倍）
skin-edit-color = 文字颜色
skin-edit-width = 宽度（160–8192 px）
skin-edit-height = 高度（96–8192 px）
skin-edit-test-text = 测试文本（仅用于预览）
skin-edit-sample-reset = 恢复默认样例
skin-edit-reset = 恢复皮肤默认
skin-edit-discard = 放弃修改
skin-edit-invalid = 请输入有效数值和十六进制颜色。留空表示皮肤默认值。
skin-edit-font-missing = 此字体不可用，将使用备用字体。
skin-edit-draft-hint = 改动仅在预览中显示；保存后应用到此皮肤，不会切换当前皮肤。留空使用皮肤默认值。
skin-edit-unavailable = 此皮肤暂时无法预览。
skin-edit-sample = 你好，NanaType！这是一段用于检查字号与自动换行的测试文字。Hello, world! 0123456789
    第二行：调整宽高、字体和行距，查看实际排版效果。
skin-edit-history-sample = 大家好！欢迎来到今天的直播。Hello, NanaType!
    这是一条较长的历史消息，用于检查窄窗口中的换行、姓名与时间排版。0123456789
    第三条消息：字体、颜色与行距都可以分别调整。
skin-edit-long-name = 用于检查窄窗换行的长姓名 NanaType
skin-edit-save = 保存
skin-edit-saved = 已保存到此皮肤。

settings-packages = 插件与资源包
settings-packages-sub = 管理皮肤、共享资源和功能插件
packages-import = 导入包
packages-refresh = 重新加载
packages-dependencies = 依赖
packages-remove = 移除
packages-access = 插件无法访问网络、用户文件或外部程序。
packages-kind-skin = 皮肤
packages-kind-resources = 共享资源
packages-kind-function = 功能插件

# Package provenance and explicit script consent
package-author = 作者
package-created-at = 制作日期
package-licensed-to = 授权对象
package-license = 许可证
package-unspecified = 未提供
package-signature-valid = 签名校验通过（作者身份未认证）
package-protected-files = 个受保护文件
package-unsigned = 未签名：来源及完整性未经签名验证
package-integrity-failed = 完整性校验失败，禁止加载
packages-review-enable = 审查来源并请求启用
packages-disable = 禁用并撤销同意
packages-consent-required = 尚未获得对此版本内容的可信来源确认和运行同意，请重新审查后启用。
packages-changed-during-review = 包内容已在审查期间变化，请取消并重新请求启用。
packages-consent-title = 脚本插件启用确认
packages-consent-risk = 仅在你已确认来源可信时启用。脚本可能显示误导内容、误用提供的数据或占用资源；隔离和限额不能保证绝对安全。签名有效只证明内容匹配该公钥，不代表作者身份已认证。
packages-consent-data = 运行后会接收播放事件、台词及弹幕的作者和纯文本，并可在展示、历史、输入内容区或插件面板绘制界面。本版没有网络、文件、外部程序、剪贴板和凭据访问接口。
packages-content-digest = 本次同意绑定的内容指纹
packages-review-source = 查看完整脚本源码
packages-source-preview-limit = 默认展示前 16000 个字符；可查看完整源码。同意仅适用于当前内容及依赖，包变更后需重新确认。
packages-source-trusted = 我已审查来源，确认可信，并知悉上述风险
packages-consent-confirm = 同意并启动此插件
packages-consent-cancel = 取消，不授予新的同意
packages-location = 包位置
packages-script-entry = 脚本入口
packages-entry-protected = 脚本入口在签名保护范围内。
packages-entry-unprotected = 脚本入口未受签名保护，或作者允许修改它；请审查当前源码。
