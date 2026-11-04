[简体中文](../building.md) · [English](../en/building.md) · [日本語](building.md)

# ソースからのビルド

ビルド、テスト、パッケージングと開発用の補助オプションです。機能の概要は [README](../../README.ja.md) をご覧ください。

## 環境

Rust 1.98 以上と Windows 10 SDK（アイコンとバージョンリソース用）が必要です。NanaUI と nanalive-spout（Spout2 送信側）は固定 rev の GitHub リモートリポジトリとして参照され、初回ビルド時に cargo が自動で取得するため、ローカルにチェックアウトする必要はありません。nanalive-spout のビルドには git が必要です（Spout2 のサブモジュールを取得します）。

WebM のデコードには固定バージョンの libvpx を使います。Windows と Apple Silicon では初回ビルド時に上流のビルド済み静的ライブラリをダウンロードして検証するので、実行時に追加のインストールは不要です。Intel Mac ではソースからビルドするため、`brew install nasm` と `rustup component add llvm-tools` が必要です。ソースビルドが必要なその他のプラットフォームでは `shiguredo_libvpx/source-build` フィーチャーを有効にできます。

```bash
cargo run -p nanatype
```

パッケージエディタは独立したプログラム NanaType Editor（[説明](package-editor.md)）です。NanaType と `crates/frame` を共有しますが、それぞれ別にビルド・パッケージングされ、互いのコードや文言は含みません。

```bash
cargo run -p nanatype-editor
```

開発ビルドでは、ワークスペースのコードは完全なデバッグ情報を保持し、サードパーティ依存は行番号のみを保持して、コンパイルとリンク時のデバッグデータを減らしています。アプリの機能には影響しませんが、サードパーティ依存をデバッグするときは変数や型の情報がありません。

リリースは既定で `dist` を使います：`opt-level = "s"`、thin LTO、単一 codegen unit、PDB は別途生成。`dist-small` は `z` を使い、機能は同じです。[性能比較](../release-profile-performance.md)ではポータブル版がさらに約 2 MiB 小さくなる一方、主要シナリオの CPU 時間が約 16%〜27% 増えるため、オプション扱いのままです。

[S/Z 混合実験](../release-profile-performance.md#sz-混合实验)ではさらに約 178 KiB（0.49%）しか縮まらず、一部の P95 が悪化したため、既定は純粋な `s` のままです。

日常のチェックには、リンクを避けられる `cargo check -p nanatype` と、コンパイル時間を確認できる `cargo build -p nanatype --timings` が使えます。

```bash
cargo test --workspace --exclude nanatype
```

すべてのウィンドウの UI スナップショットをオフスクリーンで描画します（デスクトップにウィンドウは開きません）。

```bash
cargo run -p nanatype --example snapshots -- target/snapshots nanatype.night ja-JP
```

パッケージング（`nana-packager` と Inno Setup 6 が必要）：

```bash
pwsh scripts/package.ps1
pwsh scripts/package.ps1 -Product editor
```

`-Product editor` は NanaType Editor（`NanaTypeEditor-<バージョン>-setup.exe`／`-portable.zip`）をパッケージングし、`nanatype-editor` だけをビルドします。`sh scripts/check-product-split.sh` は 2 つのプログラムの依存とフィーチャーが越境していないかを確認します（CI で実行）。より小さいポータブル版は `pwsh scripts/package.ps1 -Portable -Profile dist-small` で作れます。パッケージングスクリプトはリリースビルドのインクリメンタルコンパイルを常に無効にし、ロックファイルを使います。コンパイル、ライセンス、リリースの自己チェックのいずれかに失敗すると停止します。ポータブル版には自己チェックで生成された `data/` のユーザーデータは含まれず、初回起動時にアプリが作成します。内蔵フォント、デコーダー、GPU フォールバック描画、クラッシュからの復旧はすべて含まれます。

macOS では `NanaType.app`、zip、dmg を作ります（`nana-packager` が必要、成果物は未署名）。

```bash
scripts/package-macos.sh
scripts/package-macos.sh --product editor
```

Syphon は nanalive-spout リポジトリの `nanalive-syphon` をプログラムに静的リンクしているため、`Syphon.framework` を同梱する必要はありません。初回ビルドには Xcode Command Line Tools が必要です。ライセンス全文（Syphon Metal サーバーの表記を含む）は `Contents/Resources/THIRD_PARTY_LICENSES.txt` にあり、`scripts/macos-licenses.py` が macOS ターゲットの依存から生成します。

`vX.Y.Z` タグを push すると、`.github/workflows/release.yml` が Windows と macOS でこの 2 つのスクリプトを実行し、署名付きの更新マニフェストを生成してドラフト Release を作成します。[アップデートの確認](updates.md) を参照してください。

Steam 版は `steam` フィーチャーでビルドします。更新確認はなく、スキンと再生のページから Steam ワークショップの購読アイテムにアクセスでき、設定は GitHub 版と共通です。Steam ID はコミットしない `.cargo/config.toml` に書き（`.cargo/config.example.toml` を参照）、次を実行します。

```bash
pwsh scripts/package.ps1 -Steam
```

```bash
scripts/package-macos.sh --steam
```

詳しくは [Steam 版](steam.md) を参照してください。

開発用の環境変数（デバッグビルドか、明示的に設定したときのみ有効）：

| 変数 | 効果 |
| --- | --- |
| `NANATYPE_DEBUG=1` | セッションログを Trace レベルで記録し、すべてディスクに書き出します（「詳細ログ」に Trace を加えたもの）。[ログ](logging.md) を参照 |
| `NANATYPE_DEMO=1` | 起動 1.5 秒後にサンプルのセリフを再生（デバッグビルド） |
| `NANATYPE_DEMO_MARKUP=<マークアップ>` | 自動再生するサンプルのセリフを上書き（`NANATYPE_DEMO` が必要） |
| `NANATYPE_EXIT_AFTER=<ミリ秒>` | 時間になったら正常終了（起動時間の計測、ログのフラッシュ用） |
| `NANATYPE_OPAQUE=1` | オーバーレイウィンドウを不透明にする（デバッグビルド） |
| `NANATYPE_PERF=<JSON ファイル>` | 終了時に各ネイティブウィンドウの表示／出力フレーム数、ホストのコールバック時間、利用可能な作業数を保存。最初の 2 秒は破棄 |

描画の流れ、フレームスケジューリング、サーフェスの分岐は [描画パイプライン](../rendering-pipeline.md)、描画の計測、GPU タイムスタンプ、スクリーンショット比較と制限は [描画最適化の検証記録](../rendering-performance.md) を参照してください（いずれも中国語）。

## プロジェクト構成

| crate | 内容 |
| --- | --- |
| `crates/model` | セリフのドキュメントモデル：文字区間、ルビ、スタンプ、マーカー |
| `crates/markup` | マークアップ言語の解析とシリアライズ、テンプレート |
| `crates/playback` | 表示タイムライン、プレイヤーの状態機械、ミラーの差分、キュー |
| `crates/theme` | スキンパッケージ：マニフェスト、継承、検証、インストールとエクスポート |
| `crates/i18n` | Fluent の文言（zh-CN / en-US / ja-JP） |
| `crates/settings` | 設定の構造、バージョン移行、アトミックな書き込み、ウィンドウ位置 |
| `crates/audio` | デコード、ミキシング、サンプル単位のスケジューリング |
| `crates/bridge` | VTube Studio、OSC、WebSocket |
| `crates/twitch` | Twitch IRC の匿名受信、デバイスコードによるログインと Helix 送信 |
| `crates/blive` | blivechat・LAPLACE Event Bridge・彗星号のクライアント（プロトコル解析、復号、接続と再接続） |
| `crates/core` | セッションロジック：下書き、送信、続行、キュー、履歴、巻き戻し |
| `crates/p2p` | Steam のオンライン接続（最大 8 人）、セッションの順序付け、再接続と画像の転送 |
| `crates/update` | アップデートの確認：署名付き Release マニフェスト、ファイルプロキシ、検証付きダウンロード |
| `crates/frame` | 2 つのプログラムで共有するフレームワーク：スキン描画（表示ウィンドウ、履歴、入力ウィンドウ）、UI コンポーネント、スキンプレビュー、プラグインホスト、メディアサービスとパス |
| `app` | NanaType：NanaUI ホスト、ウィンドウ、ビュー、ホットキー、トレイ、Spout |
| `editor` | NanaType Editor：パッケージエディタ、署名と隔離デバッグ |

純粋なロジックの crate は UI フレームワークに依存せず、単独でテストできます。
