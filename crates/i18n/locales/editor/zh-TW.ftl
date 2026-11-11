# Strings only NanaType Editor shows (feature `editor`).

## 編輯器視窗
editor-app-title = NanaType 包編輯器
editor-app-subtitle = 新增、編輯、簽名和除錯 NanaType 包。匯出的 .ntpkg 在 NanaType 中匯入後才會生效。
editor-data-dir = 資料目錄：{ $path }
editor-data-dir-open = 開啟

# Package provenance and explicit script consent
pkgedit-title = 包編輯器
pkgedit-intro = 修改只儲存在草稿裡，匯出 .ntpkg 並匯入後才會生效。
pkgedit-kind-skin = 純皮膚
pkgedit-kind-resources = 純資源
pkgedit-kind-lua = 純 Lua
pkgedit-kind-sounds = 純音效包
pkgedit-kind-mixed = 混裝
pkgedit-id-placeholder = 包 ID，例如 my.package
pkgedit-new = 新增
pkgedit-open-file = 開啟 .ntpkg…
pkgedit-drafts = 草稿
pkgedit-drafts-none = 還沒有儲存的草稿。
pkgedit-open = 開啟
pkgedit-discard = 丟棄草稿
pkgedit-discard-question = 永久刪除這份草稿？已安裝的包不受影響。
pkgedit-edit = 編輯
pkgedit-copy = 複製為新包
pkgedit-origin-new = 新增專案
pkgedit-origin-installed = 開啟自已安裝的包
pkgedit-origin-archive = 開啟自檔案
pkgedit-origin-copy = 衍生副本，來源
pkgedit-derived-from = 衍生自
pkgedit-derived-unsigned = 原簽名已去除，此副本不代表原作者
pkgedit-unsaved = 有未儲存的修改
pkgedit-saved-state = 草稿已儲存
pkgedit-not-stored = 未修改
pkgedit-validating = 正在後台校驗…
pkgedit-valid = 校驗通過
pkgedit-errors = { $count } 個錯誤
pkgedit-save = 儲存草稿
pkgedit-revert = 撤銷到上次儲存
pkgedit-export = 匯出 .ntpkg…
pkgedit-close = 關閉
pkgedit-close-unsaved = 有未儲存的修改。關閉前要儲存為草稿嗎？
pkgedit-close-save = 儲存並關閉
pkgedit-close-discard = 不儲存，關閉
pkgedit-tab-files = 檔案
pkgedit-tab-package = 包資訊
pkgedit-tab-signing = 釋出
pkgedit-tab-preview = 預覽
pkgedit-import = 匯入素材…
pkgedit-remove = 移除檔案
pkgedit-remove-question = 從草稿中移除這個檔案？
pkgedit-read-only = 只讀
pkgedit-format = 格式化
pkgedit-search = 在檔案中搜索
pkgedit-search-next = 下一個
pkgedit-search-none = 無匹配
pkgedit-no-file = 選擇左側的檔案。
pkgedit-binary = 檔案資訊
pkgedit-file-format = 格式
pkgedit-file-size = 大小
pkgedit-file-dependency = 來自依賴包（只讀）
pkgedit-data-declared = 宣告為資料
pkgedit-problems = 問題
pkgedit-no-problems = 沒有發現問題。
pkgedit-manifest-invalid = package.toml 無法解析，請先在“檔案”中修正。
pkgedit-manifest-protected = package.toml 受簽名保護，只能檢視。建立衍生副本後才能修改。
pkgedit-version = 版本
pkgedit-capabilities = 能力（啟用皮膚或 Lua 時會補上起始檔案）
pkgedit-dependencies-hint = 依賴按“包ID@精確版本”記錄，新增後從已安裝的包打包進草稿。
pkgedit-group-basic = 基本資訊
pkgedit-group-listing = 相容與展示
pkgedit-capabilities-title = 能力
pkgedit-dependencies-title = 依賴
pkgedit-dependency-add = 新增已安裝的包
pkgedit-data = 宣告的資料
pkgedit-data-none = 沒有宣告資料。
pkgedit-pipelines = 處理管線
pkgedit-pipelines-none = 沒有宣告處理管線。
pkgedit-data-missing = 檔案不存在
pkgedit-data-add = 新增資料
pkgedit-data-id = 資料 ID
pkgedit-data-path = 檔案路徑
pkgedit-data-format = 格式
pkgedit-data-version = 格式版本
pkgedit-data-constraints = JSON 約束
pkgedit-data-constraints-hint = JSON 約束可選，只用於 JSON 資料，例如 {"{"}"type": "object", "required": ["items"]{"}"}。留空表示不限制。
pkgedit-entry-remove = 刪除
pkgedit-entry-remove-question = 刪除這條宣告？
pkgedit-pipeline-add = 新增管線
pkgedit-pipeline-name = 管線名稱
pkgedit-pipeline-processor = 處理器
pkgedit-pipeline-inputs = 輸入：填寫資料 ID（依賴包的資料寫 pkg:包ID/資料ID），或寫 管線名.輸出 接入另一條管線。
pkgedit-pipeline-params = 參數，每行一個 名稱 = 數值
pkgedit-pipeline-outputs = 輸出
pkgedit-pipeline-unknown = 此處理器未註冊，只能在原始碼中修改。
pkgedit-pipeline-broken = 這條宣告無法解析，請在 skin.toml 中修改。
pkgedit-pipelines-no-skin = 處理管線寫在 skin.toml 中，啟用皮膚能力後才能宣告。
pkgedit-skin-protected = skin.toml 受簽名保護，只能檢視。建立衍生副本後才能修改。
pkgedit-seal-note = 簽名只說明受保護檔案與簽名時一致，不能證明作者身份；可編輯檔案修改後簽名仍然有效。
pkgedit-key-load = 使用本機簽名金鑰
pkgedit-key-none = 本機還沒有簽名金鑰
pkgedit-key-create = 建立簽名金鑰
pkgedit-key-fingerprint = 本機簽名金鑰指紋
pkgedit-key-note = 私鑰只儲存在系統的受保護儲存中（macOS 鑰匙串、Windows DPAPI），不會寫入包、日誌、資源介面或外掛程序。
pkgedit-protect = 選擇要保護的檔案。package.toml 總是受保護；簽名後受保護檔案只讀，檔案清單也會固定。
pkgedit-sign = 簽發
pkgedit-signed-own = 此包由本機金鑰簽名。可以移除簽名後修改，再重新簽發。
pkgedit-signed-other = 此包由其他金鑰簽名，受保護檔案只讀。要修改它們，請建立去除原簽名的衍生副本。
pkgedit-unseal = 移除簽名以便修改
pkgedit-derive = 建立衍生副本
pkgedit-derive-hint = 衍生副本使用新的包 ID，記錄來源並去除簽名。
pkgedit-signing-title = 簽名
pkgedit-key-title = 本機簽名金鑰
pkgedit-protect-title = 受保護的檔案
pkgedit-derive-title = 複製為新包…
pkgedit-derive-id = 新包 ID
pkgedit-preview-none = 此專案沒有皮膚，或皮膚還沒通過校驗。
pkgedit-debug-no-lua = 此專案沒有宣告 Lua 能力。
pkgedit-debug-risk = 除錯會在獨立的沙箱工作程序中執行目前草稿的腳本。它只能收到你傳送的測試事件，結果只顯示在這裡；但腳本仍可能顯示誤導內容或佔用資源。只執行你已經審查過的原始碼。修改任何檔案都會停止腳本並需要重新確認。
pkgedit-debug-trust = 我已審查目前原始碼，信任它在除錯程序中執行
pkgedit-debug-start = 啟動
pkgedit-debug-stop = 停止
pkgedit-debug-clear = 清除結果
pkgedit-debug-events = 測試事件
pkgedit-debug-send = 傳送
pkgedit-debug-invalid-json = 事件必須是帶有字串 type 欄位的 JSON 物件。
pkgedit-debug-error = 腳本錯誤
pkgedit-debug-output = 返回的檢視
pkgedit-debug-waiting = 等待腳本返回檢視…
pkgedit-debug-idle = 腳本未執行。
pkgedit-debug-log = 記錄
pkgedit-saved = 草稿已儲存
pkgedit-open-failed = 無法開啟專案
pkgedit-exported = 已匯出
pkgedit-export-failed = 匯出失敗
pkgedit-debug-locate = 跳到出錯位置

pkgedit-undo = 撤銷

pkgedit-redo = 重做


pkgedit-recovery = 發現恢復快照

pkgedit-recover = 恢復

pkgedit-width = 寬度

pkgedit-height = 高度

pkgedit-simulate = 模擬 NanaType 個人配置

pkgedit-landscape = 橫版

pkgedit-portrait = 豎版

pkgedit-narrow = 窄窗

pkgedit-default-size = 預設尺寸

pkgedit-static = 靜態

pkgedit-play = 播放

pkgedit-reduced = 減少動態

pkgedit-effects-off = 關閉高階效果

pkgedit-replay = 重播

pkgedit-fit = 適應視窗

pkgedit-checker = 透明棋盤

pkgedit-white = 白色背景

pkgedit-dark = 深色背景

pkgedit-speaker = 範例說話人

pkgedit-preview-stale = 目前內容尚未通過校驗，顯示上一有效預覽

pkgedit-preview-size-error = 請輸入有效邏輯尺寸：寬 160–8192，高 96–8192

pkgedit-operation-stale = 專案已發生變化，請重新應用本次操作


pkgedit-property-select = 請選擇宣告分組

pkgedit-up = 返回上一級

pkgedit-add-property-hint = 新增欄位：名稱與 TOML 值；分組可用內聯表，列表使用陣列

pkgedit-property-name = 欄位名

pkgedit-add = 新增



pkgedit-inherit = 恢復繼承

pkgedit-unbind = 明確解除參數繫結

pkgedit-bound-warning = 此值綁定了參數；替換前需要明確解除繫結

pkgedit-accept-disk = 接受磁碟版本

pkgedit-add-decor = 新增裝飾

pkgedit-base = 基礎皮膚



pkgedit-cover = 封面

pkgedit-description = 描述

pkgedit-disk-conflict = 磁碟衝突

pkgedit-duplicate = 複製元素

pkgedit-event-sequence = 事件序列（JSON 陣列，最多 32 項）

pkgedit-file-target = 新增／移動到的包內路徑

pkgedit-keep-local = 保留本機修改

pkgedit-layer-down = 下移一層

pkgedit-layer-up = 上移一層

pkgedit-layout-base = 基礎佈局

pkgedit-layout-rule = 斷點規則

pkgedit-log-filter = 篩選日誌

pkgedit-min-app = 最低應用版本

pkgedit-name = 名稱

pkgedit-new-file = 新增文字檔案

pkgedit-open-directory = 開啟專案目錄…

pkgedit-origin-directory = 目錄專案

pkgedit-reference-auto = 清單引用，可自動修改

pkgedit-reference-manual = 程式引用，需要人工確認

pkgedit-rename = 重新命名／移動


pkgedit-replace = 替換檔案…

pkgedit-replace-all = 全部替換

pkgedit-replace-one = 替換目前匹配

pkgedit-replacement = 替換為

pkgedit-save-copy = 另存本機副本…

pkgedit-search-previous = 上一個

pkgedit-tags = 標籤（逗號分隔）

pkgedit-unused-asset = 沒有已辨識的引用

pkgedit-tab-sounds = 音效

pkgedit-add-responsive = 建立橫豎版佈局

pkgedit-template = 填入範本

pkgedit-property-exists = 請輸入一個尚未宣告的欄位名

pkgedit-reload-directory = 重新載入磁碟專案

pkgedit-source-too-large = 原始碼超過可編輯大小限制（1 MiB）。

pkgedit-layout-matched = 目前命中順序，後層覆蓋前層

pkgedit-bind = 繫結參數

pkgedit-pending-forms = 請先應用或放棄尚未提交的表單輸入。

pkgedit-mode-design = 設計
pkgedit-mode-data = 資料與管線
pkgedit-mode-lua = Lua
pkgedit-project-menu = 專案選單
pkgedit-toolbar = 編輯器工具欄
pkgedit-status = 專案狀態
pkgedit-split-source = 畫布旁顯示原始碼
pkgedit-files-preview = 顯示即時預覽
pkgedit-drawer-close = 收起面板
pkgedit-custom-size = 自訂尺寸
pkgedit-stage-settings = 畫布設定
pkgedit-stage-size = 預覽邏輯尺寸（寬 × 高）
pkgedit-params-none = 這個皮膚沒有宣告參數。
pkgedit-preview-params = 預覽參數
pkgedit-layers = 圖層
pkgedit-canvas = 畫布
pkgedit-inspector = 屬性
pkgedit-add-field = 新增欄位
pkgedit-section-reveal = 在原始碼中定位
pkgedit-files-none = 沒有這類檔案。
pkgedit-lua-sources = Lua 原始碼
pkgedit-sound-files = 音訊檔案
pkgedit-layer-dialogue = 對話方塊
pkgedit-layer-nameplate = 姓名牌
pkgedit-layer-portrait = 頭像
pkgedit-layer-continue = 繼續標識
pkgedit-layer-text = 正文
pkgedit-layer-decor = 裝飾
pkgedit-layer-history = 歷史
pkgedit-layer-chrome = 介面色
pkgedit-layer-effects = 效果
pkgedit-layer-fonts = 字型
pkgedit-layer-materials = 材質
pkgedit-layer-mention = 提及
pkgedit-layer-params = 參數
pkgedit-layer-param_groups = 參數分組
pkgedit-layer-presets = 預設
pkgedit-layer-shaders = 著色器
pkgedit-layer-sounds = 音效
pkgedit-layer-responsive = 響應式
pkgedit-layer-pipelines = 管線
pkgedit-layer-frame = 框體
pkgedit-cover-none = 無封面
pkgedit-data-select = 在左側選擇一項資料或管線，或用 + 新增。
pkgedit-debug-running = 腳本執行中
pkgedit-debug-stopped = 腳本未執行
pkgedit-debug-title = 除錯
pkgedit-layout-target = 編輯層
pkgedit-pipeline-inputs-title = 輸入
pkgedit-pipeline-params-title = 參數
pkgedit-problem-error = 錯誤
pkgedit-problem-warning = 警告
pkgedit-problems-more = 還有 { $count } 項未顯示
pkgedit-protect-all = 全部保護
pkgedit-protect-none = 只保護 package.toml
pkgedit-publish-check = 校驗
pkgedit-seal-status = 簽名狀態
pkgedit-side-toggle = 顯示或隱藏側欄
pkgedit-sound-add-variant = 新增變體
pkgedit-sound-advance = 繼續
pkgedit-sound-audition = 試聽
pkgedit-sound-effects = 命名音效
pkgedit-sound-effects-hint = 文字中的 [se=名稱] 播放這裡宣告的音效。
pkgedit-sound-lines = 對話事件
pkgedit-sound-none = 無
pkgedit-sound-open = 對話開始
pkgedit-sound-se-add = 新增音效
pkgedit-sound-se-file = 選擇音訊即新增
pkgedit-sound-se-invalid = 音效名稱只能使用字母、數字、_ 和 -，且不能與已有名稱重複。
pkgedit-sound-se-name = 名稱
pkgedit-sound-select = 在左側選擇一個音訊檔案來試聽。
pkgedit-sound-typing = 打字音
pkgedit-sound-typing-hint = 每個字元從變體中取一個播放；下面的數值決定音量、音高浮動和播放間隔。
pkgedit-sound-typing-none = 還沒有打字音變體。
pkgedit-sound-used = 被這些事件使用
pkgedit-stage-background = 背景
pkgedit-stage-sample = 範例說話人與文字
pkgedit-stop = 停止
pkgedit-tab-close = 關閉標籤
pkgedit-volume = 試聽音量
pkgedit-id = 包 ID
pkgedit-sound-volume = 音量
pkgedit-sound-pitch-jitter = 音高浮動
pkgedit-sound-every-n = 每幾個字元播放
pkgedit-sound-min-interval = 最小間隔（毫秒）
pkgedit-sound-skip-punct = 跳過標點
