# Strings both NanaType and NanaType Editor show (nanatype-frame).

## 編輯器
editor-placeholder = 在這裡輸入台詞… 輸入 @ 提及觀眾，輸入 / 插入標記與動效
editor-source-placeholder = 使用 [b]標記[/b] 編輯，例如 [shake]哇！[/shake][wait=500]
editor-source-label = 標記原始碼
editor-label = 台詞
marker-wait = 停 { $ms }ms
marker-pause = 等待
marker-pause-auto = 等 { $ms }ms
marker-page = 翻頁
marker-sound = 音 { $name }
marker-expression = 表情 { $name }
marker-portrait = 立繪 { $name }
marker-shake = 震動畫面
marker-vts = VTS { $name }

## 皮膚
skins-builtin = 內建
skins-preview-name = 奈奈

## 視窗
windows-input = 輸入框
windows-display = 展示框
windows-history = 歷史記錄
display-placeholder = 台詞會顯示在這裡 · 鎖定視窗後隱藏此框
history-placeholder = 歷史記錄會顯示在這裡 · 鎖定視窗後隱藏此框

## 通用
common-cancel = 取消

# Chat panel
history-role-host = 主播
history-role-viewer = 觀眾
history-search = 搜尋歷史記錄
history-search-close = 關閉搜尋
history-search-placeholder = 內容或名字

# Skin preview editor
skin-edit-invalid = 請輸入有效數值和十六進位顏色。留空表示皮膚預設值。
skin-preview-loading = 正在載入預覽…
skin-edit-sample = 你好，NanaType！這是一段用於檢查字號與自動換行的測試文字。Hello, world! 0123456789
    第二行：調整寬高、字型和行距，檢視實際排版效果。
# History preview, oldest first: host, grouped follow-up, long host line,
# viewer, grouped viewer line, long name, host quoting the viewer, gift, membership.
skin-edit-history-host = 大家好！歡迎來到今天的直播。
skin-edit-history-host-more = [b]字型、顏色與行距[/b]都可以分別調整，例如[color=#c45c26]強調[/color]。[ruby=なな]奈奈[/ruby] [at=1001 name="阿澈"] [emote=wave alt="揮手"]
skin-edit-history-long = 這是一條較長的主播訊息，用來檢查窄視窗中的換行、姓名與時間排版。Hello, NanaType! 0123456789
skin-edit-history-viewer-name = 小明
skin-edit-history-viewer = 主播好，今天的皮膚好好看！
skin-edit-history-viewer-more = 連續傳送的第二條會和上一條排在一起。
skin-edit-history-long-body = 長姓名旁邊仍要留出正文、時間和換行的位置。0123456789
skin-edit-history-reply = 收到，這條回覆會帶上剛才那條觀眾訊息。
skin-edit-history-fan-name = 小星
skin-edit-history-gift = 送出 小心心 ×10
skin-edit-history-captain-name = 艦長阿蘭
skin-edit-history-member = 開通了艦長
skin-edit-long-name = 用於檢查窄窗換行的長姓名 NanaType
packages-refresh = 重新載入
packages-installed = 已安裝
packages-none = 還沒有包。匯入一個包來新增皮膚、資源或功能。
packages-kind-skin = 皮膚
packages-kind-resources = 共享資源
packages-kind-function = 功能外掛
packages-kind-sounds = 音效包

# Package provenance and explicit script consent
package-author = 作者
package-created-at = 製作日期
package-licensed-to = 授權對象
package-license = 許可證
package-signature-valid = 簽名校驗通過（作者身份未認證）
package-protected-files = 個受保護檔案
package-unsigned = 未簽名：來源及完整性未經簽名驗證
package-integrity-failed = 完整性校驗失敗，禁止載入
package-source-builtin = 內建
package-source-workshop = Steam 創意工坊
package-source-workshop-by = Steam 創意工坊 · { $author }
package-source-third-party = 第三方匯入
package-third-party-warning = 第三方匯入的包來源未經驗證。只安裝你信任的作者提供的包，啟用功能包前請先審閱它的腳本。
package-data-source = 來源
package-data-license = 許可
package-data-protected = 受簽名保護
package-data-unprotected = 未受簽名保護


# Shared skin parameter controls
skin-params-title = 皮膚參數
skin-params-hint = 由皮膚提供的外觀選項，對整套皮膚生效
skin-params-default = 預設
skin-params-invalid-number = 請輸入數字
skin-params-out-of-range = 需在 { $min }–{ $max } 之間
skin-params-invalid-color = 顏色需寫成 #RRGGBB
skin-params-invalid-option = 不是可選的值
skin-params-text-too-long = 最多 { $max } 個字
skin-params-text-empty = 不能為空
skin-params-text-control = 不能包含換行或控制字元
skin-params-contrast = { $foreground }與{ $background }的對比度為 { $ratio }:1，建議不低於 { $min }:1
