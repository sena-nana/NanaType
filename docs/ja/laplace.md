[简体中文](../laplace.md) · [English](../en/laplace.md) · [日本語](laplace.md)

# LAPLACE との連携

[LAPLACE Chat](https://chat.laplace.live/) は bilibili のライブコメントツールです。受け取ったライブルームのイベントを [LAPLACE Event Bridge](https://github.com/laplace-live/event-bridge) で配信でき、NanaType は Event Bridge のクライアントとしてそれを受け取ります。bilibili への接続は LAPLACE が担当し、NanaType は bilibili に直接つなぎません。

## データの流れ

```
bilibili のライブルーム
  └─ LAPLACE Chat のダッシュボード（ブラウザ）または LAPLACE Event Fetcher（サーバー）
       │  server ロールでイベントを送る
       ▼
LAPLACE Event Bridge（既定 ws://localhost:9696、--auth でトークン可）
       │  すべての client に配信
       ▼
NanaType（crates/blive/src/laplace.rs）→ コメントパネル、履歴
```

## 使い方

どちらかを選びます。

- **ローカルの Event Bridge**：[Event Bridge のリリース](https://github.com/laplace-live/event-bridge/releases)から `leb-server` をダウンロードして起動し（トークンが必要なら `--auth トークン`）、LAPLACE Chat のダッシュボードをそこにつなぎます。ダッシュボードは開いたままにします。
- **Event Fetcher**：v2.2.0 以降の Event Fetcher は bridge モード（`WEBSOCKET_BRIDGE=1`）を持ち、自分で bilibili につないで Event Bridge を提供します。ダッシュボードを開いておく必要はありません。

次に NanaType で：

1. **設定 › ライブ配信 › 外部コメント**で受信をオンにし、受信元に **LAPLACE** を選びます。
2. **Event Bridge のリンク**にアドレスを貼り付け、**保存して接続**を押します。貼り付けられるもの：
   - `ws://localhost:9696`、トークンがあれば `ws://localhost:9696/?token=トークン`
   - リモートの Event Fetcher の `wss://…` または `https://…`
   - `ホスト:ポート` だけ（ポートを省くと 9696）
   - Event Fetcher モードの LAPLACE Chat OBS リンク（`danmakuFetcherApi` と `eventFetcherAuth` を読み取ります）
3. 状態が **LAPLACE · 接続済み** になると、コメント・ギフト・大航海・スーパーチャットがコメントパネルに出ます。

トークンはシステムのキーチェーンに保存され、設定ファイルにはトークンを除いたアドレスだけが残ります。貼り付けたリンクのほかの部分は保存されません。

### OBS リンクでつながらない理由

`connectionMode=direct` の OBS リンク（`…/obs/ルーム番号?connectionMode=direct&…`）は、ページ自身が bilibili に直接つなぐものです。リンクにはルーム番号と LAPLACE のログイン同期用の資格情報（`loginSyncToken`）しかなく、接続先のアドレスがありません。NanaType は bilibili に直接つながないため、このリンクは受け付けません。上の Event Bridge か Event Fetcher を使ってください。

OBS リンクの `loginSyncToken` は LAPLACE のログイン情報です。人に渡さないでください。

## フィールドの対応

| LAPLACE のイベント | NanaType のイベント | 説明 |
| --- | --- | --- |
| `message` | コメント | `emote.url` はスタンプコメントの画像。`userType` 100 / 1 は配信者 / モデレーター、`guardType` は大航海の等級 |
| `gift` | ギフト | 価値は `priceNormalized`（元）× 1000 金瓜子。`coinType` が `silver` なら無料扱い |
| `toast` | 大航海 | 数量と単位は `toastAmount` / `toastAmountUnit`、等級は `toastType` |
| `superchat` | スーパーチャット | `scId` で識別、価格は `priceNormalized`（元） |
| `superchat-delete` | スーパーチャットの取り下げ | `scIds` で削除 |
| そのほか（入室、いいね、`message-recall` など） | — | 無視 |

オープンプラットフォームでは `uid` が 0 になり、uid は記録しません。

## 接続の詳細

- Go 版 Event Bridge は 54 秒ごとに WebSocket ping を送ります。NanaType は 20 秒ごとに ping を送り、60 秒なにも届かなければ再接続します。古い Bun 版は JSON の `{"type":"ping"}` を送り、NanaType は `pong` で答えます。
- トークンが違うとハンドシェイクが 401 を返し、状態に「トークンが拒否されました」と出て 30 秒ごとに再試行します。ほかの切断は 3 秒後に再接続します。
- `wss://` は rustls と内蔵のルート証明書でつなぎます。

## テスト

```bash
cargo test -p nanatype-blive
```
