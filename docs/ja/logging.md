[简体中文](../logging.md) · [English](../en/logging.md) · [日本語](logging.md)

# ログ

NanaType と NanaType Editor のログの経路は 1 本だけで、NanaUI の `nana-diagnostics` です。収集、バッファリング、ディスクへの書き込み、ローテーション、クラッシュスナップショット、エクスポートを担当します。何を記録するか、どれだけ残すか、何を絶対に書かないかは NanaType 側の `crates/diag`（`nanatype-diag`）が決めます。

```text
任意线程 ──event!/report!/metric!──▶ 线程本地环形缓冲（满了就丢，不阻塞）
                                          │
                              nana-diagnostics 后台线程
                               ├─ 飞行记录器：最近 60 秒，内存中
                               └─ 批量写入 ──▶ 会话日志 .nlog
依赖库的 log / tracing ──bridge──▶ external.* 事件（限速、脱敏）
panic ──NanaUI 的 panic hook──▶ Fatal 事件 + 崩溃快照
```

（任意のスレッドが `event!`/`report!`/`metric!` でスレッドローカルのリングバッファに書き込みます。満杯なら捨て、ブロックしません。`nana-diagnostics` のバックグラウンドスレッドが直近 60 秒分のフライトレコーダーをメモリに保持し、セッションログ `.nlog` にまとめて書き込みます。依存ライブラリの `log` / `tracing` 出力はブリッジ経由でレート制限・マスク済みの `external.*` イベントになります。panic は NanaUI の panic フックから Fatal イベントとクラッシュスナップショットになります。）

生産側の経路ではファイル I/O もロックも待機も行いません。ランタイムがインストールされていない場所（プラグインのワーカープロセスなど）では、各呼び出し箇所はアトミックな読み取り 1 回だけです。

## ファイルの場所

| プラットフォーム | セッションログ | クラッシュスナップショット |
| --- | --- | --- |
| Windows | `%LOCALAPPDATA%\dev.sena-nana.nanatype\Logs` | `…\Crash` |
| macOS | `~/Library/Logs/dev.sena-nana.nanatype` | `…/Crash` |
| Linux | `$XDG_STATE_HOME/dev.sena-nana.nanatype/logs` | `…/crash` |
| ポータブル版 | `<程序目录>/data/logs` | `<程序目录>/data/crash` |

（`<程序目录>` はプログラムのディレクトリです。）エディターは独自のアプリ ID（`dev.sena-nana.nanatype-editor`）を使います。1 ファイルが 16 MiB を超えるとローテーションし、ログディレクトリ全体の上限は 128 MiB・32 ファイル・14 日、クラッシュスナップショットは最大 16 個です。「設定 › 情報 › ログフォルダー」でこのディレクトリを直接開けます。

`.nlog` はバイナリ形式なので、NanaUI の `nana-nlog` で閲覧します。

```bash
cargo install --git https://github.com/sena-nana/NanaUI nana-diagnostics --bin nana-nlog
```

```bash
nana-nlog text <文件.nlog> --redact-home
```

## レベルとスイッチ

| 状況 | 記録（フライトレコーダー） | ディスクに書き込む |
| --- | --- | --- |
| デフォルト（リリース版） | Debug 以上 | Warn 以上、fault、marker、メトリクスのスナップショット |
| 「設定 › 情報 › 詳細ログ」 | Debug 以上 | すべて |
| `NANATYPE_DEBUG=1` | Trace 以上 | すべて |
| デバッグビルド | Debug 以上 | すべて |

ランタイムは常に Debug を記録するので、クラッシュスナップショットには直近 1 分間のデバッグ情報が入ります。スイッチが決めるのはセッションログに書き込む量だけです。この設定はウィンドウを開く前に `nanatype_settings::peek_diagnostics` で 1 回だけ読み取り専用で読みます（マイグレーションも破損ファイルの隔離も行いません）。変更は再起動後に反映されます。エディターも NanaType の設定にある同じスイッチに従います。

依存ライブラリは `log`（および tracing の `log` フィーチャー）経由で出力し、デフォルトでは Warn 以上、詳細ログが有効なら Info 以上を取り込みます。`hyper`、`reqwest`、`ureq`、`rustls`、`h2`、`tungstenite` は下位レベルのログにリクエストの詳細が含まれるため Error のみです。各クレートは 1 秒に最大 1 行で、間引かれた行数は次の行の `suppressed` フィールドに記録されます。

## プライバシーのルール

ログはそのまま不具合報告に添付できるものでなければならないので、機微な内容はそもそも書きません。

- **絶対に記録しない**：Cookie（`SESSDATA`、`bili_jct` など）、トークン、認可コード、パスワード、プロキシの認証情報。チャットメッセージの本文、ユーザー名、アバター。ユーザーが入力したセリフ。ルーム番号、チャンネル名、動画 ID。オーディオデバイス名。
- **記録してよい**：ステータスコード、エラーコード、件数、所要時間、バイト数、ウィンドウやソースの番号、真偽値のスイッチ、スキン ID、プラグイン ID。
- イベントのフィールドは数値か真偽値のみ（1 イベント最大 4 個）なので、文字列が漏れることはありません。
- 自由テキストは fault メッセージにだけ現れ、必ず `nanatype_diag::report!` を通します。これは `redact::text` を呼び出し、URL はスキーム・ホスト・パスだけを残し、`token=`、`Cookie:`、`Authorization: Bearer` のような値を `***` に、ホームディレクトリを `~` に置き換え、512 バイトに切り詰めます。マスクは最後の安全網であって免罪符ではありません。個人情報を含みうる文字列をメッセージに埋め込まないでください。
- ユーザーに表示されるエラー通知（`App::notice(…, true)`）はすべて `app.error_notice` として記録されるので、通知の文言にもチャット内容やアカウント情報を連結してはいけません。

## イベント一覧

各クレートは自分の `diag` モジュールでイベントを `static EventDescriptor` として宣言し、`EVENTS` に並べます。`app/tests/diag_catalog.rs` が番号と名前の一意性、名前が所属ドメインのプレフィックスで始まることを確認します。ドメインは `crates/diag/src/domains.rs` で一括して割り当てています。

| ドメイン | プレフィックス | 内容 |
| --- | --- | --- |
| `0x0100` | `app.` | ライフサイクル、設定の読み込み、ウィンドウ、エラー通知。番号 100 以降は `nanatype-frame` の共通部分 |
| `0x0101` | `settings.` | 予約 |
| `0x0102` | `output.` | Spout/Syphon、ウィンドウ出力の状態、VTS/OSC/WebSocket ブリッジ |
| `0x0103` | `chat.` | チャット画像などの共通部分 |
| `0x0107` | `twitch.` | 接続（1）、接続エラー（2）、アカウント（3）、アカウント失敗（4）、警告（5）、送信失敗（6）、受信したチャットイベント数（メトリクス 50） |
| `0x0108` | `p2p.` | エンドポイント、リレーのオンライン化、リソース転送 |
| `0x0109` | `plugins.` | ワーカーの準備完了までの時間、失敗（stderr の末尾付き）、スクリプトエラー |
| `0x010A` | `plugin_log.` | プラグインが `nana.log` で書いた行 |
| `0x010B` | `update.` | 更新元ごとのチェック結果と所要時間、ダウンロード、バージョン番号の解析失敗 |
| `0x010C` `0x010D` `0x010F` | `steam.` `stickers.` `audio.` | Steam が使えない理由、スタンプのプレビューとアップロードの失敗、オーディオデバイスのオープンと喪失 |
| `0x010E` | `skin.` | 予約。スキンの読み込みとシェーダーのエラーは現在 `app.error_notice` として記録 |
| `0x0110` | `editor.` | エディター |
| `0x01F0` | `external.` | 依存ライブラリのログ |

接続とアカウントの状態は共通の値で表します。接続：`0` オフ、`1` 接続中、`2` 接続済み、`3` 切断または配信終了、`4` エラー、`5` ログインが必要。アカウント：`0` 復元中、`1` 未ログイン、`2` QR スキャンまたは確認待ち、`3` 準備完了、`4` 期限切れ、`5` 失敗。

### イベントを追加する

```rust
use nanatype_diag::domains::P2P;
use nanatype_diag::{EventDescriptor, FieldDescriptor, Severity};

pub static RELAY_SWITCHED: EventDescriptor = EventDescriptor::new(
    P2P, 5, "p2p.relay_switched", Severity::Info, &[FieldDescriptor::u64("ms")],
);
```

```rust
nanatype_diag::event!(crate::diag::RELAY_SWITCHED, ms = took.as_millis() as u64);
nanatype_diag::report!(crate::diag::ONLINE_TIMEOUT; "{error}");
```

そのあと、このクレートの `EVENTS` に加えます。頻繁に通る経路では `metric!` で数えるか `Throttle` でレート制限し、節目には `nanatype_diag::marker`（固定テキストのみ）を使います。セッション冒頭の `edition`、`features`、`os`、`locale`、`skin`、`verbose` は `set_session_info` が書き込みます。

## プラグイン

プラグインのワーカーは独立したプロセスで、診断ランタイムはインストールしません。ホストがワーカーの準備完了までの時間、タイムアウト、プロトコルエラーを記録し、ワーカーが異常終了したときは stderr の最後の数行を添えます（Windows の AppContainer ワーカーには stderr がありません）。プラグインスクリプトが `nana.log(level, message)` で書いた行はプロトコル経由で戻り、ホストが `[プラグイン ID] メッセージ` として記録します。ルールは[プラグインのドキュメント](plugins.md)を参照してください。

## 診断パッケージ

「設定 › 情報 › 診断パッケージ › エクスポート」はバックグラウンドで次を行います。

1. 現在のフライトレコーダーのスナップショットを書き出し、未書き込みの内容をディスクにフラッシュする。
2. 直近 5 セッションと 5 つのスナップショットをテキスト（`.txt`）と JSON Lines（`.jsonl`）にエクスポートする。ホームディレクトリは `~` に置き換え、元の `.nlog` は含めない。
3. `manifest.json` と `summary.json`（バージョン、ビルド番号、エディション、OS、言語、スキン ID、有効なプラットフォームと出力（真偽値のみ）、プラグイン数、詳細ログのスイッチ）を加える。
4. ログディレクトリに `dev.sena-nana.nanatype-diagnostics-<时间>.zip`（`<时间>` は日時）としてまとめ、そのディレクトリを開く。

パッケージは手元のパソコンにだけ保存され、不具合報告に添付するかどうかはユーザーが決めます。前回の実行がクラッシュしていた場合（`diagnostics.crash_seen` より新しい panic スナップショットがある場合）、次回起動時に一度だけ案内が出ます。
