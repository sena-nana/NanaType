[简体中文](../fonts.md) · [English](../en/fonts.md) · [日本語](fonts.md)

# 内蔵スキンのフォント

内蔵スキンで使うフォントの同梱、サブセット化、ライセンス表示を説明します。フォントを追加する開発者は「フォントを追加する手順」から読み、利用者は「同梱するクレジットとライセンス」を確認してください。

**このページの案内**：仕組みとサイズ → 同梱フォント → サブセット化 → クレジットとライセンス → 追加手順

内蔵スキンがシステムフォント名だけを指定していると、そのフォントが入っていないパソコンでは普通の UI フォントで代用され、ポケットピクセルや水墨のようなスキンは持ち味を失ってしまいます。ここでは、アプリに同梱しているオープンソースフォント、それをアプリに組み込む方法、今後フォントを追加するときの決まりを説明します。

> **現状**：Fusion Pixel、霞鹜文楷（LXGW WenKai）、Tangerine Bold の 3 書体を同梱しています。前の 2 つは 2026-10-13 に公式 GitHub Release から、Tangerine は 2026-10-18 に Google Fonts から取得しました。ライセンス、入手元、SHA-256、サブセット化の記録は [crates/theme/fonts/README.md](../../crates/theme/fonts/README.md) にあります。

## 仕組み

- フォントは `crates/theme/fonts/<名前>.ttf`（または `.otf`）に置き、隣に同名のライセンス `<名前>.LICENSE.txt`（上流の `OFL.txt` を著作権表示行も含めてそのままコピーしたもの）が必須です。ライセンスがないとビルドに失敗します。
- スキンでは `file = "builtin:fonts/<名前>.ttf"` と書きます。ユーザースキンからも参照できます。パスは `crates/theme/` の外に出られません。
- `build.rs` がこのディレクトリを走査し、Cargo フィーチャー `builtin-fonts`（アプリではデフォルトで有効）が有効なときは `include_bytes!` でフォントとライセンスを埋め込みます。無効なら何も埋め込みません。
- NanaUI への登録は、そのフォントを使うスキンを選んだときだけ行います（`app/src/skin_fonts.rs`）。本文・名札・装飾の 3 つの役割や複数のスキンが同じフォントを共有していても登録は 1 回だけで、使わないスキンに切り替えると登録を解除します。登録にはフォントファイル自身のファミリー名（下表参照）を使い、`families` の先頭に置きます。
- 今回のビルドに含まれていないフォントはエラーにならず、そのまま `families` にフォールバックします。`fonts/` にまったく存在しない名前には警告を出します（たいていは綴りの間違いです）。
- スキン一覧で「代替フォント」の印が付くのは、`families` のどのフォントも使えず（インストール済み、または NanaUI 同梱の Noto Sans SC）、使える同梱フォントファイルもない場合だけです。

### サイズの切り替え

| 方法 | 効果 |
| --- | --- |
| `cargo build --no-default-features -p nanatype` | 内蔵フォントを一切埋め込まない |
| `NANATYPE_BUILTIN_FONTS_BUDGET_KB=<KiB>` | サイズ上限。デフォルトは 12288（12 MiB）。超えるとビルドに失敗する |

埋め込みはバイト単位そのままなので、実行ファイルの増分は `fonts/` 内のフォントファイルの合計サイズと同じです（圧縮なし。インストーラーの圧縮である程度は縮みます）。現在 2 書体の合計は 3,595,708 バイト（約 3.4 MiB）で、デバッグ版の `snapshots` サンプルは `builtin-fonts` の有無で 3,621,376 バイト違います（フォントとライセンス全文、わずかなコード）。

## 同梱フォント

「ソネット」には Tangerine Bold（Version 1.3、65,464 バイト、Toshi Omagari、OFL-1.1）も同梱し、デフォルトの英文筆記体として使います。中国語は霞鹜文楷を流用します。2026-10-18 に Google Fonts から完全版を取得し、改変もサブセット化もしていません。チェックサムとライセンスの記録はフォントディレクトリの README にあります。同梱 3 書体の合計は 3,661,172 バイトです。新たに省略可能な `[fonts.latin]` ロールを追加しました。項目はほかのフォントロールと同じで、フォント指定のないラテン文字の区間にだけ適用されます。明示的なフォント指定や個人のフォント設定が優先されます。

| スキン | フォント（登録されるファミリー名） | バージョン | 作者 | ライセンス | 上流ファイル | サブセット | ファイル |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `pixel`、`rpg`（本文、名札） | Fusion Pixel 12px Proportional 簡体字（`Fusion Pixel 12px Prop zh-Hans`） | 2026.09.25 | TakWolf | OFL-1.1、予約フォント名なし | `fusion-pixel-12px-proportional-zh_hans.ttf`、7,053,672 バイト、36,436 グリフ | GB2312 第一・第二水準 + UI 使用文字 + かなと約物：7,983 グリフ、1,453,472 バイト | `FusionPixel12pxProportionalSC.ttf` |
| `ink`（本文、名札、装飾） | 霞鹜文楷 Regular（`LXGW WenKai`） | 1.522 | LXGW（フォントワークスの Klee One がベース） | OFL-1.1、予約フォント名なし | `LXGWWenKai-Regular.ttf`、25,575,676 バイト、46,867 グリフ | GB2312 第一水準（常用 3,755 字）+ UI 使用文字 + かなと約物：5,479 グリフ、2,142,236 バイト | `LXGWWenKai-Regular.ttf` |
| `echo_*` | Noto Sans SC（NanaUI 同梱） | — | — | OFL-1.1 | — | — | ファイル不要：`families` の末尾に `"Noto Sans SC"` を加えてあり、前のフォントがどれも入っていなければこれを使う |

ダウンロードした元ファイルと SHA-256：

| ファイル | 入手元 | サイズ | SHA-256 |
| --- | --- | --- | --- |
| `fusion-pixel-font-12px-proportional-ttf-v2026.09.25.zip` | <https://github.com/TakWolf/fusion-pixel-font/releases/tag/2026.09.25> | 49,414,580 | `47585b9c4b6d8d57380e9dec75bc7e8043fdbfa77d1843c3aaa19a1e7bd15d50` |
| └ `fusion-pixel-12px-proportional-zh_hans.ttf` | 上の圧縮ファイル内 | 7,053,672 | `b2ee68647dc257fa697e4d0c78f9b59461bc504a023b9a3b5ad66f358bbb1613` |
| `LXGWWenKai-Regular.ttf` | <https://github.com/lxgw/LxgwWenKai/releases/tag/v1.522> | 25,575,676 | `39ad71264b588165b469e35e6afb162a378dacd1f95348160240ba9038ac3009` |
| `OFL.txt`（文楷、tag v1.522） | <https://github.com/lxgw/LxgwWenKai/blob/v1.522/OFL.txt> | 4,448 | `c38b1994a5e48ac30ac7d1da7d0409fd8fd8127dfe28a13d6e787d5b1ef34a5e` |
| `LICENSE-OFL`（Fusion Pixel、tag 2026.09.25） | <https://github.com/TakWolf/fusion-pixel-font/blob/2026.09.25/LICENSE-OFL> | 4,418 | `bc518cf64b8032c07690f33cc270c35c179255a6ac8efa7c165ebae7e8f76a63`（圧縮ファイル内の `OFL.txt` と同一） |

Fusion Pixel のグリフの一部は Ark Pixel（OFL-1.1、TakWolf）、Cubic 11（OFL-1.1、JF Dot M+H 12 と M+ BITMAP FONTS の自由ライセンス表示を含む）、Galmuri（OFL-1.1、Lee Minseo）に由来します。圧縮ファイル内のこの 3 つのライセンスは、Fusion Pixel 自身の `OFL.txt` と一緒に `FusionPixel12pxProportionalSC.LICENSE.txt` にそのまま収めています。

スキンの `families` は変更しておらず、引き続きフォールバックチェーンとして働きます。フォントなしのビルド（`--no-default-features`）は以前とまったく同じです。ピクセルフォントは 12 px の整数倍で表示したときが最もくっきりし、12 の倍数でない文字サイズでは字画の縁がにじみます。`pixel` と `rpg` の本文、名札、履歴の文字（コメント行を含む）はすべて 24 px、行の高さ 1.5（36 px）で、`pixel_font_skins_stay_on_the_12px_grid` テストがこれを確認します。

### 今は同梱しないもの

| スキン | フォント | 理由 |
| --- | --- | --- |
| `parchment` | Noto Serif SC | 600 と 700 の 2 ウェイトが必要で、サブセット後もそれぞれ約 1.5–3 MB。SimSun（宋体）で代用しても許容範囲 |
| `terminal` | Cascadia Mono | Windows 11 に標準搭載されており、Consolas / Menlo へのフォールバックで十分。ライセンスが「Cascadia Code」の名を予約しているため、サブセットには改名が必要 |
| — | Zpix（最像素） | OFL ではない独自ライセンスで、ソフトウェアと一緒に配布するには別途許諾が必要。Fusion Pixel で置き換え済み |

## サブセット化

fontTools（`pip install fonttools`）の `pyftsubset` を使います。コマンドと文字表の作り方は [crates/theme/fonts/README.md](../../crates/theme/fonts/README.md) を参照してください。要点：

- name レコードはすべて残します。OFL は著作権表示とライセンス表示をフォントに残すよう求めており、ファミリー名もそのまま残さないとスキンと一致しません。
- 文字表：GB2312（ピクセルフォントは第一・第二水準、文楷は《通用規範漢字表》の 3,500 字の代わりに第一水準の 3,755 字）+ `*.ftl`、内蔵スキン、`app/` のソースを走査して得たすべての非 ASCII 文字 + かな、CJK 約物、全角文字などいくつかの範囲。
- ヒンティングと縦書きメトリクス（`vhea`/`vmtx`）を削除し、OpenType 機能はすべて残します（どちらのフォントも GSUB/GPOS は 10 KB 未満）。
- サブセットに含まれない文字は 1 文字ずつシステムフォントにフォールバックするので、セリフにときどき宋体や黒体の文字が混じります。これはサイズと引き換えの代償です。

**予約フォント名（RFN）**：OFL 第 3 条により、予約フォント名を宣言しているフォントは、改変版（サブセットも含む）でその名前を使えません。サブセット化の前に、上流の `OFL.txt` の著作権表示行に「with Reserved Font Name …」があるかを確かめ、さらにフォントの name テーブルの著作権（ID 0）とライセンス（ID 13）のレコードを確認します。

- Fusion Pixel 2026.09.25、Ark Pixel、Cubic 11、Galmuri、霞鹜文楷 1.522：いずれも予約フォント名を宣言して**いません**（ライセンスファイルと name テーブルで確認済み）。サブセットは元の名前のままです。
- 予約名がある場合：サブセット化しないか、予約名がファミリー名に含まれていないことを確認します。含まれているならファミリー名を変え、スキンの `families` も合わせて変更します。例：NanaUI 同梱の Noto Sans SC 2.004 が予約しているのは「Source」なので「Noto Sans SC」は影響を受けません。Cascadia は「Cascadia Code」を予約しています。

## 同梱するクレジットとライセンス

OFL 1.1 第 2 条は、すべての複製に著作権表示とライセンス全文を添えるよう求めています。このリポジトリでは次のようにしています。

- `crates/theme/fonts/<名前>.LICENSE.txt`：上流のライセンスをそのままコピーしたもの（ビルドで必須）。フォントと一緒にアプリに組み込まれます（`BuiltinFont::license`）。
- **情報ページ**：「オープンソースフォント」の行のトグルを開くとスクロールできるパネルが展開し、今回のビルドに埋め込まれたフォントを 1 つずつ一覧します。ファミリー名、作者、バージョン、ライセンス名とライセンス全文です（どれも埋め込んだフォントとライセンスから読み取ります。フォントなしのビルドではこの行は表示されません）。
- **インストーラー**：`packaging/nanatype.iss` が `crates/theme/fonts/*.LICENSE.txt`、`packaging/licenses/`（NanaUI 同梱の Noto Sans SC 用の `NotoSansSC-OFL.txt`）、リポジトリ直下の `THIRD_PARTY_NOTICES.md`、パッケージ作成時に生成する `rust-crates.txt`（各 Rust 依存クレート自身のライセンスファイル）を `{app}\licenses\` にインストールします。`scripts/package.ps1 -Portable` の圧縮ファイルにも同じ `licenses/` フォルダーが入っています。
- リポジトリ直下の [THIRD_PARTY_NOTICES.md](../../THIRD_PARTY_NOTICES.md) にクレジットをまとめています。フォント、`dxcompiler.dll`、すべての Rust 依存クレートです（後者は `pwsh scripts/third-party-notices.ps1` が `cargo metadata --offline` から生成し、CI は `-Check` で内容が古くなっていないか、各依存にライセンス表記があるかを確認します）。

## フォントを追加する手順

1. 公式 Release からダウンロードし、ライセンス（OFL-1.1、または同様にソフトウェアとの同梱配布を認めるライセンス）と予約フォント名を確認して、バージョンと SHA-256 を控えます。
2. 上の方法でサブセット化して `crates/theme/fonts/` に置き、隣に `<名前>.LICENSE.txt` を置きます。`crates/theme/fonts/README.md` に入手元、バージョン、SHA-256、サブセット化コマンドと日付を記録し、`THIRD_PARTY_NOTICES.md` にクレジットを加えます。
3. スキンに `file = "builtin:fonts/<名前>.ttf"` と書き、`families` はフォールバックとしてそのままにします。
4. `cargo test -p nanatype-theme`：埋め込んだ各フォントが解析できること、ライセンスが付いていること、合計が予算内であること、各内蔵スキンの `builtin:` 参照が `fonts/` に存在することをテストが確認します。
