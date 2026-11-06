[简体中文](../steam.md) · [English](../en/steam.md) · [日本語](steam.md)

# Steam 版

Steam 版のビルド、設定共有、ワークショップ、公開手順をまとめています。利用者は [使い方ガイド](usage.md) を、配布担当者は「パッケージング」と「アップロードと公開」を参照してください。

**このページの案内**：版の違い → 設定と Steam ID → 開発 → ワークショップ → パッケージング → 公開

Steam 版と GitHub リリース版は同じコードを共有し、Cargo の feature で区別します。

| | GitHub リリース版（デフォルト） | Steam 版 |
| --- | --- | --- |
| ビルド | `cargo build -p nanatype` | `cargo build -p nanatype --no-default-features --features builtin-fonts,steam` |
| アップデート | GitHub Release を確認（`updates`、[アップデートの確認](updates.md)を参照） | SteamPipe で更新。アップデート確認のコードは含まない |
| ワークショップ | — | スキン・再生ページからサブスクライブ済みのスキンとサウンドパックを利用 |

`steam` は `updates` と同時に有効にできません（`app/src/lib.rs` の `compile_error!`）。`scripts/check-product-split.sh` と CI で、Steam 版が `nanatype-update` をリンクしていないこと、それ以外のビルドが Steamworks をリンクしていないことを確認します。

## 接続ソース

Twitch と blivechat／彗星号はどちらの版でも使えます。Steam 版には GitHub のアップデート確認コードは含まれません。blivechat のルーム番号モードと身分コードモードはどちらも残っています。

## GitHub 版との設定の共有

両版は同じアプリ ID（`dev.sena-nana.nanatype`）と bundle ID を使うため、設定、パッケージディレクトリ、キャッシュ、ログ、資格情報の保存場所は共通です。Steam のパッケージは nana-packager の `steam` バックエンドを使い、ポータブル版のマーカーを書き込まないので、ユーザーデータが Steam のインストールディレクトリに置かれることはありません。両版は単一インスタンスのロックを共有しており、同時には起動できません。

- ログイン時の自動起動は同じ起動項目（Windows `HKCU\…\Run\NanaType`、macOS LaunchAgent）に書き込まれ、最後に設定した版が有効になります。
- ワークショップのコンテンツは `packages/` にコピーされません。ワークショップのスキンやサウンドパックを選んだ状態で GitHub 版を開くと、一時的にデフォルトのスキンやスキン自身の効果音を使いますが、設定は書き換えません。Steam 版に戻ると元に戻ります。ワークショップのスキンから「**複製して編集**」で作ったスキンは、ワークショップのスキンを自身の `dependencies/` に同梱するので、両方の版で使えます。

## Steam ID とローカル設定

App ID と depot ID はリポジトリに含めません。

1. `.cargo/config.example.toml` を `.cargo/config.toml`（`.gitignore` 済み）にコピーし、`[env]` に記入します。
   - `NANATYPE_STEAM_APP_ID`：Steam の App ID。Cargo がビルド時に `crates/steam/build.rs` に渡し、プログラムに埋め込まれます。
   - `NANATYPE_STEAM_DEPOT_WINDOWS`、`NANATYPE_STEAM_DEPOT_MACOS`：パッケージングスクリプトが depot のビルドスクリプトに書き込みます。
   - 任意の `NANATYPE_STEAM_TAG_SKIN`、`NANATYPE_STEAM_TAG_SOUNDS`：ワークショップの閲覧ボタンが使うタグ。デフォルトは `Skin`、`Sound Pack`。
2. 環境変数として直接設定することもできます。CI では secrets で渡します。

Steam をリンクする release ビルドで App ID がない場合はエラーになります。debug ビルドでは Valve のテスト用アプリ Spacewar（480）にフォールバックし、警告を出します。プログラムは `Client::init_app` で App ID を指定するので `steam_appid.txt` は不要で、パッケージにも含めません。

## 開発とデバッグ

- `cargo run -p nanatype --no-default-features --features builtin-fonts,steam` はデフォルトでは Steam に接続せず、ワークショップは利用不可と表示されます。`NANATYPE_STEAM=1` を設定し、Steam クライアントを起動している場合にのみ接続します。
- Release 版を Steam の外から起動すると、Steam 経由で自分自身を再起動します（`restart_through_steam`、単一インスタンスのロックより前）。Steam が起動していなければそのまま起動し、ワークショップが使えないだけです。
- パッケージ検証用のセルフチェック実行（`NANA_PACKAGE_VALIDATE`）では Steam に接続せず、再起動もしません。

## ワークショップ

- サブスクライブ済みでインストール済みのアイテムは、読み取り専用の外部パッケージディレクトリとしてパッケージレジストリに渡されます（`SkinRegistry::set_external`）。アイテムのフォルダーのルートは完全なパッケージでなければなりません（`package.toml` が最上位にあること）。
- パッケージ ID が衝突した場合は、内蔵パッケージ、ユーザーがインストールしたパッケージ、アイテム ID が小さいワークショップアイテムの順に優先されます。隠されたアイテムにはサブスクリプション一覧に理由が表示されます。
- サブスクリプション一覧にはダウンロードの進捗、ダウンロード待ち、未ダウンロード、使用不可、衝突の状態が表示され、アイテムページを開く、再ダウンロード、サブスクライブ解除ができます。サブスクライブを解除するとアイテムはすぐに一覧とレジストリから外れ、Steam がアプリ終了後にファイルを削除します。スキンやサウンドパックとして選択中だった場合はデフォルトに戻ります。
- ワークショップのパッケージはエクスポートも削除もできません。「**プラグインとパッケージ**」ページでは「**削除**」が「**サブスクライブ解除**」に変わります。Lua の機能パッケージは引き続き 1 つずつ確認して同意する必要があります。
- 「**ワークショップを見る**」は、対応するタグのページをなるべく Steam オーバーレイで開きます。オーバーレイがオフのときは `steam://openurl/` で Steam クライアントに渡します。オーバーレイは透明な表示ウィンドウや Spout/Syphon 出力を含むすべてのウィンドウに重なるため、正式リリース前に配信画面に映り込まないか実際に確認し、必要なら Steamworks の管理画面でオーバーレイを無効にしてください。

### アイテムの公開

NanaType Editor で `.ntpkg` をエクスポートしてフォルダーに展開するか、パッケージディレクトリをそのまま使い、SteamCMD の `workshop_build_item` でアップロードします。コンテンツのルートはパッケージディレクトリです。スキンにはタグ `Skin`、サウンドパックにはタグ `Sound Pack`（またはローカル設定で指定したタグ）を付けます。

## パッケージング

`nana-packager`（[リリース手順](updates.md)と同じ）と、`.cargo/config.toml` の ID が必要です。

```bash
pwsh scripts/package.ps1 -Steam
```

```bash
scripts/package-macos.sh --steam
```

- どちらのスクリプトも Steam の feature でビルドし、`packaging/steam/nana-package.steam.toml` に ID を埋めて `target/steam/nana-package.toml` に書き出してから、パッケージングと検証を行います。インストーラー、zip、dmg は生成しません。
- Windows では steamworks-sys に付属する `steam_api64.dll` を `NanaType.exe` の隣に置きます。
- macOS では `libsteam_api.dylib` を `Contents/Frameworks` に入れ、実行ファイルのロードパスを書き換えてから ad-hoc 署名し直します。バンドル内の `Contents/MacOS` には実行ファイルしか置けないためです。
- ライセンスファイルは `third-party-notices.ps1 -Steam` と `macos-licenses.py … nanatype-steam` で生成し、Steam 版が実際にリンクする crate だけを列挙します。
- 成果物は `dist/app/` に出力されます。nana-packager はさらに `dist/steam/` に depot のビルドスクリプトとファイル一覧（`depot_build_<depot>.vdf`、`nana-steam-build.json`）を書き出しますが、アップロードにはこの depot ビルドスクリプトではなく下記のスクリプトを使ってください（除外ルールがないため）。

アップロードの資格情報は SteamCMD だけが管理し、リポジトリやスクリプトが読み取ることはありません。

## アップロードと公開

[SteamCMD](https://partner.steamgames.com/doc/sdk/uploading)（PATH 上にあるか、`STEAMCMD` で指定。macOS では通常 `steamcmd.sh`）、そのアプリへのアップロード権限を持つ Steam のビルド用アカウント、パッケージングと同じ `.cargo/config.toml` の ID が必要です。

```bash
pwsh scripts/steam-upload.ps1 -Account <构建账号>
```

```bash
sh scripts/steam-upload.sh --account <构建账号>
```

- アカウントは環境変数 `STEAM_BUILD_ACCOUNT` でも指定できます。スクリプトはアカウント名を `steamcmd +login <账号> +run_app_build <脚本> +quit` に渡すだけで、パスワードと Steam Guard のコードは SteamCMD 自身が尋ねてログインをキャッシュします。スクリプトがパスワードを受け取ったり保存したりすることはありません。
- スクリプトは `packaging/steam/app_build.vdf.in` と `depot_build.vdf.in` に ID と絶対パスを埋め、`target/steampipe/`（`app_build_<app>.vdf`、`depot_build_<depot>.vdf`）に書き出します。ビルドログ、マニフェスト、チャンクキャッシュは `target/steampipe/output/` に置かれ、いずれもリポジトリには入りません。
- コンテンツのルート：Windows の depot は `dist/app/NanaType`（`NanaType.exe` のあるディレクトリ）、macOS の depot は `dist/app`（`NanaType.app` を含む）です。アップロード時は `steam_appid.txt`、`*.pdb`、`.DS_Store` を除外します。
- デフォルトでは自分のプラットフォームの depot だけをアップロードします（どちらのパッケージングスクリプトも `dist/` を空にするため、1 台のマシンには自分のプラットフォームの成果物しかありません）。`-Platform windows|macos|all`（`--platform`）で指定でき、`-WindowsContent`、`-MacosContent`（`--windows-content`、`--macos-content`）で別のマシンからコピーした成果物を指定できます。2 つの depot を一度にアップロードするには、Windows の `dist/app/NanaType` フォルダーを Mac にコピーし、`--platform all --windows-content <ディレクトリ>` を使います。逆に `.app` を Windows にコピーしないでください。`.app` バンドル内のシンボリックリンクとメタデータが失われます。
- `-Preview`（`--preview`）はファイル一覧とログを生成するだけでアップロードしないので、除外ルールの確認に使えます。
- スクリプトは公開ブランチを設定しません（`SetLive` は空）。`default` ブランチは自動で公開できません。アップロード後、Steamworks の SteamPipe → ビルド（`https://partner.steamgames.com/apps/builds/<App ID>`）で新しいビルド（説明は `NanaType <バージョン> (<コミット>) <プラットフォーム>`）を探し、「default」ブランチを選んで「変更をプレビュー」で depot を確認してから公開します。
- 2 つのプラットフォームを別々にアップロードすると、depot を 1 つずつ含む 2 つのビルドができます。公開前に「変更をプレビュー」で、もう一方の depot が削除されないことを確認してください。最も確実なのは、両方の depot を含む 1 つのビルドで公開することです。
