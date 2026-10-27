# Strings only NanaType Editor shows (feature `editor`).

## エディターウィンドウ
editor-app-title = NanaType エディター
editor-app-subtitle = NanaType パッケージの作成・編集・署名・デバッグを行います。書き出した .ntpkg は NanaType に読み込むと有効になります。
editor-data-dir = データフォルダー：{ $path }
editor-data-dir-open = 開く

# Package provenance and explicit script consent
pkgedit-title = パッケージエディター
pkgedit-intro = 変更は .ntpkg を書き出して読み込むまで下書きに留まります。
pkgedit-kind-skin = スキンのみ
pkgedit-kind-resources = リソースのみ
pkgedit-kind-lua = Lua のみ
pkgedit-kind-sounds = サウンドパックのみ
pkgedit-kind-mixed = 混在
pkgedit-id-placeholder = パッケージ ID（例: my.package）
pkgedit-new = 新規作成
pkgedit-open-file = .ntpkg を開く…
pkgedit-drafts = 下書き
pkgedit-drafts-none = 保存された下書きはまだありません。
pkgedit-open = 開く
pkgedit-discard = 下書きを破棄
pkgedit-discard-question = この下書きを完全に削除しますか？インストール済みのパッケージには影響しません。
pkgedit-edit = 編集
pkgedit-copy = 新しいパッケージとして複製
pkgedit-origin-new = 新規プロジェクト
pkgedit-origin-installed = インストール済みパッケージから開いた
pkgedit-origin-archive = ファイルから開いた
pkgedit-origin-copy = 派生コピー、元
pkgedit-derived-from = 派生元
pkgedit-derived-unsigned = 元の署名は削除済み。このコピーは元の作者を代表しません
pkgedit-unsaved = 未保存の変更があります
pkgedit-saved-state = 下書きを保存済み
pkgedit-not-stored = 変更なし
pkgedit-validating = バックグラウンドで検証中…
pkgedit-valid = エラーなし
pkgedit-errors = { $count } 件のエラー
pkgedit-save = 下書きを保存
pkgedit-revert = 前回の保存に戻す
pkgedit-export = .ntpkg を書き出す…
pkgedit-close = 閉じる
pkgedit-close-unsaved = 未保存の変更があります。閉じる前に下書きとして保存しますか？
pkgedit-close-save = 保存して閉じる
pkgedit-close-discard = 保存せずに閉じる
pkgedit-tab-files = ファイル
pkgedit-tab-package = パッケージ情報
pkgedit-tab-signing = 保護と署名
pkgedit-tab-preview = プレビュー
pkgedit-import = 素材を読み込む…
pkgedit-remove = ファイルを削除
pkgedit-remove-question = このファイルを下書きから削除しますか？
pkgedit-read-only = 読み取り専用
pkgedit-format = 整形
pkgedit-search = ファイル内を検索
pkgedit-search-next = 次へ
pkgedit-search-none = 一致なし
pkgedit-no-file = 左のファイルを選んでください。
pkgedit-binary = ファイル情報
pkgedit-file-format = 形式
pkgedit-file-size = サイズ
pkgedit-file-dependency = 依存パッケージのファイル（読み取り専用）
pkgedit-data-declared = データとして宣言
pkgedit-problems = 問題（クリックで移動）
pkgedit-no-problems = 問題は見つかりません。
pkgedit-manifest-invalid = package.toml を解析できません。先に「ファイル」で修正してください。
pkgedit-manifest-protected = package.toml は署名で保護されているため閲覧のみです。変更するには派生コピーを作成してください。
pkgedit-version = バージョン
pkgedit-capabilities = 機能（スキンや Lua を有効にすると初期ファイルを追加）
pkgedit-dependencies-hint = 依存パッケージ。1 行に 1 つ「パッケージID@正確なバージョン」。適用するとインストール済みパッケージから下書きに同梱します。
pkgedit-apply = 適用
pkgedit-data = 宣言済みデータ
pkgedit-data-none = データは宣言されていません。
pkgedit-pipelines = 処理パイプライン
pkgedit-pipelines-none = 処理パイプラインは宣言されていません。
pkgedit-data-missing = ファイルがありません
pkgedit-data-add = データを追加
pkgedit-data-id = データ ID
pkgedit-data-path = ファイルパス
pkgedit-data-format = 形式
pkgedit-data-version = 形式バージョン
pkgedit-data-constraints = JSON 制約
pkgedit-data-constraints-hint = JSON 制約は任意で、JSON データにのみ使います。例：{"{"}"type": "object", "required": ["items"]{"}"}。空欄なら制限しません。
pkgedit-entry-edit = 編集
pkgedit-entry-remove = 削除
pkgedit-entry-remove-question = この宣言を削除しますか？
pkgedit-pipeline-add = パイプラインを追加
pkgedit-pipeline-name = パイプライン名
pkgedit-pipeline-processor = プロセッサー
pkgedit-pipeline-inputs = 入力：データ ID（依存パッケージのデータは pkg:パッケージID/データID）、または別のパイプラインにつなぐ場合は パイプライン名.出力 を入力します。
pkgedit-pipeline-params = パラメーター（1 行に 名前 = 数値）
pkgedit-pipeline-outputs = 出力
pkgedit-pipeline-unknown = このプロセッサーは登録されていません。ソースで編集してください。
pkgedit-pipeline-broken = この宣言は解析できません。skin.toml で編集してください。
pkgedit-pipelines-no-skin = 処理パイプラインは skin.toml に書きます。スキン機能を有効にすると宣言できます。
pkgedit-skin-protected = skin.toml は署名で保護されているため閲覧のみです。変更するには派生コピーを作成してください。
pkgedit-seal-note = 署名は保護ファイルが署名時と一致することを示すだけで、作者の身元は証明しません。編集可能なファイルを変更しても署名は有効なままです。
pkgedit-key-load = このコンピューターの署名鍵を使う
pkgedit-key-none = このコンピューターにはまだ署名鍵がありません。
pkgedit-key-create = 署名鍵を作成
pkgedit-key-fingerprint = 署名鍵のフィンガープリント
pkgedit-key-note = 秘密鍵はシステムの保護されたストレージ（macOS キーチェーン、Windows DPAPI）にのみ保存され、パッケージ、ログ、リソース API、プラグインプロセスには入りません。
pkgedit-key-unavailable = 署名鍵を利用できません
pkgedit-protect = 保護するファイルを選んでください。package.toml は常に保護されます。署名後、保護ファイルは読み取り専用になり、ファイル一覧も固定されます。
pkgedit-sign = 署名
pkgedit-signed-own = このコンピューターの鍵で署名されています。署名を外して編集し、再度署名できます。
pkgedit-signed-other = 別の鍵で署名されています。保護ファイルは読み取り専用です。変更するには元の署名を外した派生コピーを作成してください。
pkgedit-unseal = 署名を外して編集
pkgedit-derive = 派生コピーを作成
pkgedit-derive-hint = 派生コピーは新しいパッケージ ID を使い、派生元を記録して署名を外します。
pkgedit-preview-none = このプロジェクトにはスキンがないか、まだ検証を通過していません。
pkgedit-debug-no-lua = このプロジェクトは Lua 機能を宣言していません。
pkgedit-debug-risk = デバッグでは下書きのスクリプトを独立したサンドボックスのワーカープロセスで実行します。受け取るのは送信したテストイベントだけで、結果はここにだけ表示されます。それでもスクリプトは誤解を招く内容を表示したりリソースを消費したりする可能性があります。確認済みのソースだけを実行してください。ファイルを編集するとスクリプトは停止し、再確認が必要です。
pkgedit-debug-trust = 現在のソースを確認し、デバッグプロセスでの実行を信頼します
pkgedit-debug-start = 開始
pkgedit-debug-stop = 停止
pkgedit-debug-clear = 結果を消去
pkgedit-debug-events = テストイベントを送信
pkgedit-debug-send = 送信
pkgedit-debug-invalid-json = イベントは文字列の type フィールドを持つ JSON オブジェクトである必要があります。
pkgedit-debug-error = スクリプトエラー
pkgedit-debug-output = 返されたビュー
pkgedit-debug-waiting = スクリプトのビューを待っています…
pkgedit-debug-idle = スクリプトは実行されていません。
pkgedit-debug-log = ログ
pkgedit-saved = 下書きを保存しました
pkgedit-unsaved-open = 開いているプロジェクトに未保存の変更があります。先に保存するか閉じてください。
pkgedit-open-failed = プロジェクトを開けませんでした
pkgedit-exported = 書き出しました
pkgedit-export-failed = 書き出しに失敗しました
pkgedit-debug-locate = エラー箇所へ移動

pkgedit-undo = 元に戻す

pkgedit-redo = やり直す

pkgedit-discard-form = フォームの変更を破棄

pkgedit-recovery = 復元データがあります

pkgedit-recover = 復元

pkgedit-width = 幅

pkgedit-height = 高さ

pkgedit-simulate = NanaType の個人設定を再現

pkgedit-landscape = 横向き

pkgedit-portrait = 縦向き

pkgedit-narrow = 狭いウィンドウ

pkgedit-default-size = 既定サイズ

pkgedit-static = 静止・停止

pkgedit-play = 再生

pkgedit-reduced = 動きを減らす

pkgedit-effects-off = 高度な効果をオフ

pkgedit-replay = 再再生

pkgedit-fit = 全体を表示

pkgedit-checker = 透明グリッド

pkgedit-white = 白い背景

pkgedit-dark = 暗い背景

pkgedit-speaker = サンプル話者

pkgedit-preview-stale = 現在の内容は検証未完了です。最後の有効なプレビューを表示します。

pkgedit-preview-size-error = 論理サイズ：幅 160–8192、高さ 96–8192

pkgedit-operation-stale = プロジェクトが変更されました。操作を再適用してください。


pkgedit-property-select = 宣言グループを選択

pkgedit-up = 上へ

pkgedit-add-property-hint = 名前と TOML 値で追加。グループはインラインテーブル、リストは配列。

pkgedit-property-name = フィールド名

pkgedit-add = 追加



pkgedit-inherit = 継承に戻す

pkgedit-unbind = パラメーターの関連付けを解除

pkgedit-bound-warning = 値はパラメーターに関連付けられています。置換前に解除してください。

pkgedit-accept-disk = ディスクの内容を採用

pkgedit-add-decor = 装飾を追加

pkgedit-base = 基本スキン



pkgedit-cover = カバー画像のパス

pkgedit-description = 説明

pkgedit-disk-conflict = ディスクとの競合

pkgedit-duplicate = 要素を複製

pkgedit-event-sequence = テストイベント列（JSON 配列、最大 32 件）

pkgedit-file-target = 作成・移動先のパッケージ内パス

pkgedit-keep-local = ローカルの変更を保持

pkgedit-layer-down = 一つ下のレイヤーへ

pkgedit-layer-up = 一つ上のレイヤーへ

pkgedit-layout-base = 基本レイアウト

pkgedit-layout-rule = ブレークポイントルール

pkgedit-log-filter = ログを絞り込み

pkgedit-min-app = アプリの最低バージョン

pkgedit-name = 名前

pkgedit-new-file = テキストファイルを作成

pkgedit-open-directory = プロジェクトフォルダーを開く…

pkgedit-origin-directory = フォルダープロジェクト

pkgedit-reference-auto = 宣言内の参照・自動更新

pkgedit-reference-manual = プログラム内の参照・手動確認が必要

pkgedit-rename = 名前変更・移動

pkgedit-rename-question = ファイルパスと以下の参照を変更しますか。プログラム内の参照は手動で確認してください。

pkgedit-replace = ファイルを置換…

pkgedit-replace-all = すべて置換

pkgedit-replace-one = 現在の一致を置換

pkgedit-replacement = 置換後の文字列

pkgedit-save-copy = ローカルのコピーを別名保存…

pkgedit-search-previous = 前へ

pkgedit-tags = タグ（コンマ区切り）

pkgedit-unused-asset = 認識された参照はありません

pkgedit-tab-sounds = 効果音

pkgedit-add-responsive = 縦横レイアウトを作成

pkgedit-template = テンプレートを入力

pkgedit-property-exists = 未宣言のフィールド名を入力してください

pkgedit-reload-directory = フォルダープロジェクトを再読み込み

pkgedit-source-too-large = ソースが編集上限（1 MiB）を超えています。

pkgedit-layout-matched = 適用順序（後のレイヤーが上書き）

pkgedit-bind = パラメーターに関連付け

pkgedit-pending-forms = 先に未適用のフォーム変更を適用または破棄してください。

pkgedit-mode-design = デザイン
pkgedit-mode-data = データとパイプライン
pkgedit-mode-lua = Lua
pkgedit-project-menu = プロジェクトメニュー
pkgedit-toolbar = エディターツールバー
pkgedit-status = プロジェクトの状態
pkgedit-split-source = キャンバスの横にソースを表示
pkgedit-files-preview = ライブプレビューを表示
pkgedit-drawer-close = パネルを閉じる
pkgedit-custom-size = カスタムサイズ
pkgedit-stage-settings = キャンバス設定
pkgedit-stage-size = プレビューの論理サイズ（幅 × 高さ）
pkgedit-params-none = このスキンにはパラメーターがありません。
pkgedit-preview-params = プレビューのパラメーター
pkgedit-layers = レイヤー
pkgedit-canvas = キャンバス
pkgedit-inspector = インスペクター
pkgedit-add-field = フィールドを追加
pkgedit-section-source = このテーブルの TOML
pkgedit-section-reveal = ソースで表示
pkgedit-section-inline = このテーブルには独自の見出しがありません。含まれるテーブルで編集してください。
pkgedit-file-operations = 新規・移動・置き換え
pkgedit-files-none = この種類のファイルはありません。
pkgedit-lua-sources = Lua ソース
pkgedit-sound-files = 音声ファイル
pkgedit-sound-declarations = 効果音の宣言
pkgedit-layer-dialogue = 会話ボックス
pkgedit-layer-nameplate = 名札
pkgedit-layer-portrait = アバター
pkgedit-layer-continue = 続行マーク
pkgedit-layer-text = 本文
pkgedit-layer-decor = 装飾
pkgedit-layer-history = 履歴
pkgedit-layer-chrome = クローム
pkgedit-layer-effects = エフェクト
pkgedit-layer-fonts = フォント
pkgedit-layer-materials = マテリアル
pkgedit-layer-mention = メンション
pkgedit-layer-params = パラメーター
pkgedit-layer-param_groups = パラメーターグループ
pkgedit-layer-presets = プリセット
pkgedit-layer-shaders = シェーダー
pkgedit-layer-sounds = 効果音
pkgedit-layer-responsive = レスポンシブ
pkgedit-layer-pipelines = パイプライン
pkgedit-layer-frame = フレーム
pkgedit-layer-remove = 装飾を削除
