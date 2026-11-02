[简体中文](../twitch.md) · [English](../en/twitch.md) · [日本語](twitch.md)

# Twitch チャット連携

## 出典とライセンス

確認日：2026-10-21。すべて Twitch 公式の開発者ドキュメントに基づいて独自に実装しており、サードパーティのコードは移植していません。

- 受信：[Twitch IRC over WebSocket](https://dev.twitch.tv/docs/chat/irc/)（`wss://irc-ws.chat.twitch.tv:443`、IRCv3 タグと `twitch.tv/tags`・`twitch.tv/commands` ケイパビリティ）。
- ログイン：[Device Code Grant Flow](https://dev.twitch.tv/docs/authentication/getting-tokens-oauth/#device-code-grant-flow)（パブリッククライアント、client secret なし）。
- 送信：Helix [Send Chat Message](https://dev.twitch.tv/docs/api/reference/#send-chat-message)（`POST /helix/chat/messages`、`user:write:chat` スコープ）。

## 受信

**設定 › Twitch** でチャンネル名または `https://www.twitch.tv/チャンネル名` を入力し、「**チャットを受信**」をオンにします。受け付けるのは `twitch.tv`、`www.twitch.tv`、`m.twitch.tv` のホストと、3〜25 文字の `[a-z0-9_]` からなるログイン名のみです。

チャットの読み取りは常に匿名ニックネーム `justinfanNNNNN` でチャンネルに参加して行うため、アカウントは不要です。ログインの有無は受信に影響しません。`ROOMSTATE` を受け取ってから接続済みと表示し、メッセージの表示を始めます。`room-id` が送信時に使うチャンネル ID です。15 秒以内に `ROOMSTATE` が来なければ、チャンネルは利用できないものとみなします。アプリは 60 秒ごとに PING を送り、90 秒間まったくデータがなければ再接続します。`RECONNECT` を受け取ると即座に再接続し、失敗時は 1、2、4…最大 30 秒の間隔で再試行します。

| IRC メッセージ | コメントパネル |
| --- | --- |
| `PRIVMSG` | テキスト。`emotes` タグに従い Unicode 文字範囲でエモート画像を切り出し、`/me` は ACTION の包装を外す |
| `PRIVMSG` + `bits` | 太字の「N Bits」とメッセージ、Twitch の紫色のカード |
| `USERNOTICE`（サブスク、継続サブスク、ギフトサブスク、レイド、お知らせなど） | Twitch の `system-msg` の原文と視聴者のメッセージ |
| `CLEARMSG` | そのメッセージをコメントパネルと履歴から削除 |

投稿者 ID は `twitch:` プレフィックスで名前空間を分けています。配信者・モデレーターのバッジは他のプラットフォームと同じ表示です。IRC はアバターを提供しないため、Twitch のメッセージには視聴者のアバターが表示されません。Bits は「**スーパーチャット**」フィルター、サブスク系のイベントは「**メンバー加入**」フィルターで制御されます。

## ログイン

1. 「**Twitch でログイン**」をクリックすると、アプリが `id.twitch.tv/oauth2/device` にデバイスコードを要求します。要求するのは `user:write:chat` スコープのみです。
2. 画面にユーザーコードが表示されます。「**twitch.tv/activate を開く**」をクリックし（開けるのは `https://www.twitch.tv/activate` のみ）、ブラウザでログインしてコードを入力します。
3. アプリは Twitch が指定した間隔でトークンをポーリングし（`slow_down` を受け取ると 5 秒延長）、デバイスコードの期限が切れたらやり直すよう案内します。
4. トークンを取得したら `oauth2/validate` でユーザーとスコープを検証し、Helix `users` でニックネームとアバターを取得します。

トークンのペアは JSON としてシステムの保護されたストレージ（Windows DPAPI / macOS キーチェーン、サービス名 `dev.sena-nana.nanatype.twitch`）に保存され、設定ファイルやログには書き込まれません。それ以外のプラットフォームでは保存を拒否します。パブリッククライアントの refresh token は 1 回しか使えないため、更新のたびに新しいトークンをすぐ保存します。トークンは起動時と 1 時間ごとに検証し、401 を受け取ったら 1 回だけ更新します。更新が拒否されると資格情報を消去し、「**ログインの有効期限が切れました**」と表示します。

## 送信

ログインしてチャンネルに接続したら、**設定 › Twitch › チャットに送信** のスイッチをオンにし、確認ダイアログで承認します。以降に送信するセリフはすべて、あなたのアカウントで現在のチャンネルに投稿されます。これは bilibili 同期とは独立しており、両方オンにすると 1 つのセリフが両方のプラットフォームに送られます。同期中は画像を挿入できません（bilibili と同じ制限）。

- セリフは演出用のマークアップを除いたプレーンテキストに変換し、500 文字ごとに分割します。分割はできるだけ句読点の位置で行います。
- 送信は 1.6 秒に最大 1 件で、一般ユーザーの上限（30 秒に 20 件）を下回ります。送信キューは最大 100 セリフです。
- Twitch が受け付けたものの送信しなかった場合（`is_sent=false`、AutoMod によるブロックや重複メッセージなど）や拒否した場合（4xx）は失敗として扱い、ネットワークが切断された場合は「不確定」とします。どちらの場合もキューの先頭で止まり、自動での再試行はしません。再試行・スキップ・残りのテキストのコピーはユーザーが選びます。
- 401 のときはトークンを更新して 1 回だけ再送します。Twitch は処理前にリクエストを拒否しているため、二重送信にはなりません。
- 返された `message_id` を受領確認として使います。自分が送ったメッセージが IRC でエコーされても、履歴に重複して入ることはありません。

## アプリの登録

内蔵の Client ID は [dev.twitch.tv/console/apps](https://dev.twitch.tv/console/apps) で登録したパブリッククライアントです。

- **Client Type**：Public（デスクトップアプリは secret を保管できず、パブリッククライアントでもトークンを更新できるため）
- **OAuth Redirect URLs**：`http://localhost`（デバイスコードフローではコールバックを使わないが、コンソールで入力が必須）
- **Category**：Application Integration

パブリッククライアントの ID は秘密情報ではありません。ビルド時に環境変数 `NANATYPE_TWITCH_CLIENT_ID` で自分のアプリに置き換えられます。

## 検証

```bash
cargo test -p nanatype-twitch
```

ユニットテストは、チャンネル URL の許可リスト、IRC タグのエスケープ、エモートの範囲、各種メッセージ、ローカルの偽 IRC サーバーでの匿名参加と PING/PONG、デバイスコードのポーリング、トークンの検証と更新、トークンが保護されたストレージにのみ保存されること、送信キューの分割・失敗時のブロック・再試行・スキップ・古い epoch をカバーしています。オンラインでの受け入れテストはデフォルトで無視されます。

```bash
NANATYPE_TWITCH_CHANNEL=频道名 cargo test -p nanatype-twitch -- --ignored --nocapture
```

2026-10-21 に macOS で公開チャンネルに匿名で参加し、リアルタイムのメッセージを受信できました。送信は実際のアカウントを使い、自分のチャンネルで手動で受け入れ確認する必要があります。
