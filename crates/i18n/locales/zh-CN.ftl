# Strings both NanaType and NanaType Editor show (nanatype-frame).

## 编辑器
editor-placeholder = 在这里输入台词… 输入 @ 提及观众，输入 / 插入标记与动效
editor-source-placeholder = 使用 [b]标记[/b] 编辑，例如 [shake]哇！[/shake][wait=500]
marker-wait = 停 { $ms }ms
marker-pause = 等待
marker-pause-auto = 等 { $ms }ms
marker-page = 翻页
marker-sound = 音 { $name }
marker-expression = 表情 { $name }
marker-portrait = 立绘 { $name }
marker-shake = 震屏
marker-vts = VTS { $name }

## 皮肤
skins-builtin = 内置
skins-preview-name = 奈奈

## 窗口
windows-input = 输入框
windows-display = 展示框
windows-history = 历史记录
display-placeholder = 台词会显示在这里 · 锁定窗口后隐藏此框
history-placeholder = 历史记录会显示在这里 · 锁定窗口后隐藏此框

## 通用
common-cancel = 取消

# Bilibili
history-role-host = 主播
history-role-viewer = 观众

# Skin preview editor
skin-edit-invalid = 请输入有效数值和十六进制颜色。留空表示皮肤默认值。
skin-preview-loading = 正在加载预览…
skin-edit-sample = 你好，NanaType！这是一段用于检查字号与自动换行的测试文字。Hello, world! 0123456789
    第二行：调整宽高、字体和行距，查看实际排版效果。
# History preview, oldest first: host, grouped follow-up, long host line,
# viewer, grouped viewer line, long name, host quoting the viewer, gift, membership.
skin-edit-history-host = 大家好！欢迎来到今天的直播。
skin-edit-history-host-more = [b]字体、颜色与行距[/b]都可以分别调整，例如[color=#c45c26]强调[/color]。[ruby=なな]奈奈[/ruby] [at=1001 name="阿澈"] [emote=wave alt="挥手"]
skin-edit-history-long = 这是一条较长的主播消息，用来检查窄窗口中的换行、姓名与时间排版。Hello, NanaType! 0123456789
skin-edit-history-viewer-name = 小明
skin-edit-history-viewer = 主播好，今天的皮肤好好看！
skin-edit-history-viewer-more = 连续发送的第二条会和上一条排在一起。
skin-edit-history-long-body = 长姓名旁边仍要留出正文、时间和换行的位置。0123456789
skin-edit-history-reply = 收到，这条回复会带上刚才那条观众消息。
skin-edit-history-fan-name = 小星
skin-edit-history-gift = 送出 小心心 ×10
skin-edit-history-captain-name = 舰长阿兰
skin-edit-history-member = 开通了舰长
skin-edit-long-name = 用于检查窄窗换行的长姓名 NanaType
packages-refresh = 重新加载
packages-installed = 已安装
packages-none = 还没有包。导入一个包来添加皮肤、资源或功能。
packages-kind-skin = 皮肤
packages-kind-resources = 共享资源
packages-kind-function = 功能插件
packages-kind-sounds = 音效包

# Package provenance and explicit script consent
package-author = 作者
package-created-at = 制作日期
package-licensed-to = 授权对象
package-license = 许可证
package-signature-valid = 签名校验通过（作者身份未认证）
package-protected-files = 个受保护文件
package-unsigned = 未签名：来源及完整性未经签名验证
package-integrity-failed = 完整性校验失败，禁止加载
package-data-source = 来源
package-data-license = 许可
package-data-protected = 受签名保护
package-data-unprotected = 未受签名保护


# Shared skin parameter controls
skin-params-title = 皮肤参数
skin-params-hint = 由皮肤提供的外观选项，对整套皮肤生效
skin-params-default = 默认
skin-params-invalid-number = 请输入数字
skin-params-out-of-range = 需在 { $min }–{ $max } 之间
skin-params-invalid-color = 颜色需写成 #RRGGBB
skin-params-invalid-option = 不是可选的值
skin-params-text-too-long = 最多 { $max } 个字
skin-params-text-empty = 不能为空
skin-params-text-control = 不能包含换行或控制字符
skin-params-contrast = { $foreground }与{ $background }的对比度为 { $ratio }:1，建议不低于 { $min }:1
