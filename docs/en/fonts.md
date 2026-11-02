[简体中文](../fonts.md) · [English](fonts.md) · [日本語](../ja/fonts.md)

# Fonts for the built-in skins

If a built-in skin only named system fonts, any computer without those fonts would fall back to the ordinary UI font, and skins like Pocket Pixel and Ink Wash would lose their character. This page covers which open-source fonts ship with the app, how they are compiled in, and the rules for adding more.

> **Status**: Three fonts ship: Fusion Pixel, LXGW WenKai and Tangerine Bold. The first two were fetched from their official GitHub Releases on 2026-10-13, Tangerine from Google Fonts on 2026-10-18. Licences, sources, SHA-256 hashes and subsetting records are in [crates/theme/fonts/README.md](../../crates/theme/fonts/README.md).

## How it works

- Fonts live in `crates/theme/fonts/<name>.ttf` (or `.otf`), and each must sit next to a licence with the same name, `<name>.LICENSE.txt` (the upstream `OFL.txt` copied verbatim, copyright line included). A missing licence fails the build.
- A skin writes `file = "builtin:fonts/<name>.ttf"`. User skins can reference these too. The path may not leave `crates/theme/`.
- `build.rs` scans that directory. With the Cargo feature `builtin-fonts` enabled (on by default for the app) it embeds the fonts and licences with `include_bytes!`; otherwise it embeds nothing.
- Fonts are registered with NanaUI only when a skin that uses them is selected (`app/src/skin_fonts.rs`). If the body, name tag and decoration roles, or several skins, share one font, it is registered once, and it is unregistered when you switch to a skin that does not use it. Registration uses the font file's own family name (see the table below), placed first in `families`.
- A font left out of the current build is not an error; the skin simply falls back to `families`. A name that does not exist in `fonts/` at all produces a warning (most likely a typo).
- The skin library marks a skin with "Fallback font" only when none of the fonts in `families` is available (installed, or NanaUI's bundled Noto Sans SC) and no bundled font file is available either.

### Size switches

| Method | Effect |
| --- | --- |
| `cargo build --no-default-features -p nanatype` | Embed no built-in fonts |
| `NANATYPE_BUILTIN_FONTS_BUDGET_KB=<KiB>` | Size limit, default 12288 (12 MiB). Exceeding it fails the build |

Embedding is byte-for-byte, so the binary grows by exactly the total size of the font files in `fonts/` (uncompressed; installer compression recovers some of it). The two pixel/brush fonts currently total 3,595,708 bytes (about 3.4 MiB); the debug `snapshots` example differs by 3,621,376 bytes with `builtin-fonts` on versus off (fonts plus full licence texts and a little code).

## Bundled fonts

Sonnet additionally bundles Tangerine Bold (Version 1.3, 65,464 bytes, Toshi Omagari, OFL-1.1) as its default English script face, reusing LXGW WenKai for Chinese. The complete font was fetched from Google Fonts on 2026-10-18 and was not modified or subset; its checksum and licence record are in the font directory's README. The three bundled fonts total 3,661,172 bytes. A new optional `[fonts.latin]` role takes the same fields as the other font roles and applies only to Latin runs with no font specified; explicit fonts and personal font settings take precedence.

| Skin | Font (registered family name) | Version | Author | Licence | Upstream file | Subset | File |
| --- | --- | --- | --- | --- | --- | --- | --- |
| `pixel`, `rpg` (body, name tag) | Fusion Pixel 12px Proportional Simplified Chinese (`Fusion Pixel 12px Prop zh-Hans`) | 2026.09.25 | TakWolf | OFL-1.1, no Reserved Font Name | `fusion-pixel-12px-proportional-zh_hans.ttf`, 7,053,672 bytes, 36,436 glyphs | GB2312 levels 1 and 2 + UI characters + kana and punctuation: 7,983 glyphs, 1,453,472 bytes | `FusionPixel12pxProportionalSC.ttf` |
| `ink` (body, name tag, decoration) | LXGW WenKai Regular (`LXGW WenKai`) | 1.522 | LXGW (based on Fontworks' Klee One) | OFL-1.1, no Reserved Font Name | `LXGWWenKai-Regular.ttf`, 25,575,676 bytes, 46,867 glyphs | GB2312 level 1 (3,755 common characters) + UI characters + kana and punctuation: 5,479 glyphs, 2,142,236 bytes | `LXGWWenKai-Regular.ttf` |
| `echo_*` | Noto Sans SC (bundled with NanaUI) | — | — | OFL-1.1 | — | — | No file needed: `"Noto Sans SC"` is appended to `families` and used when none of the earlier fonts is installed |

Original downloads and SHA-256:

| File | Source | Size | SHA-256 |
| --- | --- | --- | --- |
| `fusion-pixel-font-12px-proportional-ttf-v2026.09.25.zip` | <https://github.com/TakWolf/fusion-pixel-font/releases/tag/2026.09.25> | 49,414,580 | `47585b9c4b6d8d57380e9dec75bc7e8043fdbfa77d1843c3aaa19a1e7bd15d50` |
| └ `fusion-pixel-12px-proportional-zh_hans.ttf` | Inside the archive above | 7,053,672 | `b2ee68647dc257fa697e4d0c78f9b59461bc504a023b9a3b5ad66f358bbb1613` |
| `LXGWWenKai-Regular.ttf` | <https://github.com/lxgw/LxgwWenKai/releases/tag/v1.522> | 25,575,676 | `39ad71264b588165b469e35e6afb162a378dacd1f95348160240ba9038ac3009` |
| `OFL.txt` (WenKai, tag v1.522) | <https://github.com/lxgw/LxgwWenKai/blob/v1.522/OFL.txt> | 4,448 | `c38b1994a5e48ac30ac7d1da7d0409fd8fd8127dfe28a13d6e787d5b1ef34a5e` |
| `LICENSE-OFL` (Fusion Pixel, tag 2026.09.25) | <https://github.com/TakWolf/fusion-pixel-font/blob/2026.09.25/LICENSE-OFL> | 4,418 | `bc518cf64b8032c07690f33cc270c35c179255a6ac8efa7c165ebae7e8f76a63` (identical to the `OFL.txt` in the archive) |

Some of Fusion Pixel's glyphs come from Ark Pixel (OFL-1.1, TakWolf), Cubic 11 (OFL-1.1, including the free-licence notices of JF Dot M+H 12 and M+ BITMAP FONTS) and Galmuri (OFL-1.1, Lee Minseo). Those three licences from the archive are copied verbatim into `FusionPixel12pxProportionalSC.LICENSE.txt` together with Fusion Pixel's own `OFL.txt`.

The skins' `families` are unchanged and still act as the fallback chain: a build without fonts (`--no-default-features`) behaves exactly as before. Pixel fonts look sharpest at integer multiples of 12 px; at other sizes the stroke edges blur. The body, name tag and history text (chat rows included) of `pixel` and `rpg` all use 24 px with a line height of 1.5 (36 px), which the `pixel_font_skins_stay_on_the_12px_grid` test checks.

### Not bundled for now

| Skin | Font | Reason |
| --- | --- | --- |
| `parchment` | Noto Serif SC | Needs both the 600 and 700 weights, roughly 1.5–3 MB each after subsetting; SimSun is an acceptable fallback |
| `terminal` | Cascadia Mono | Ships with Windows 11, and the Consolas / Menlo fallbacks are enough; its licence reserves the name "Cascadia Code", so a subset would have to be renamed |
| — | Zpix | A custom non-OFL licence; redistributing it with software requires separate permission. Replaced by Fusion Pixel |

## Subsetting

Use `pyftsubset` from fontTools (`pip install fonttools`). The command and how the character list is generated are in [crates/theme/fonts/README.md](../../crates/theme/fonts/README.md). Key points:

- Keep every name record: the OFL requires the copyright and licence notices to stay with the font, and the family name has to stay as is so skins still match it.
- Character list: GB2312 (levels 1 and 2 for the pixel font; level 1's 3,755 characters for WenKai instead of the 3,500 of the Table of General Standard Chinese Characters) + every non-ASCII character found by scanning `*.ftl`, the built-in skins and the `app/` sources + a few ranges such as kana, CJK punctuation and full-width forms.
- Drop hinting and vertical metrics (`vhea`/`vmtx`), keep every OpenType feature (GSUB/GPOS are under 10 KB in both fonts).
- Characters outside the subset fall back to a system font one by one, so a line occasionally contains a stray SimSun/SimHei character. That is the price of the smaller size.

**Reserved Font Names (RFN)**: Under OFL clause 3, if a font declares a Reserved Font Name, a modified version (a subset counts) may not use that name. Before subsetting, check whether the copyright line in the upstream `OFL.txt` says "with Reserved Font Name …", then check the copyright (ID 0) and licence (ID 13) records in the font's name table:

- Fusion Pixel 2026.09.25, Ark Pixel, Cubic 11, Galmuri and LXGW WenKai 1.522 declare **no** Reserved Font Name (verified in both the licence files and the name tables), so the subsets keep their original names.
- If there is a reserved name: either don't subset, or confirm the reserved name is not part of the family name; otherwise rename the family and update the skin's `families` to match. Examples: Noto Sans SC 2.004, bundled with NanaUI, reserves "Source", which does not affect "Noto Sans SC"; Cascadia reserves "Cascadia Code".

## Bundled attribution and licences

OFL 1.1 clause 2 requires every copy to carry the copyright notice and the full licence text. This repository does it like this:

- `crates/theme/fonts/<name>.LICENSE.txt`: the upstream licence copied verbatim (enforced by the build) and compiled into the app alongside the font (`BuiltinFont::license`).
- **About page**: an "Open-source fonts" row whose toggle expands a scrollable panel listing every font embedded in this build: family name, author, version, licence name and full licence text (all read from the embedded font and licence; builds without fonts do not show the row).
- **Installer**: `packaging/nanatype.iss` installs `crates/theme/fonts/*.LICENSE.txt`, `packaging/licenses/` (`NotoSansSC-OFL.txt` for the Noto Sans SC bundled with NanaUI), `THIRD_PARTY_NOTICES.md` from the repo root and the `rust-crates.txt` generated at packaging time (each Rust dependency's own licence files) into `{app}\licenses\`; the archive from `scripts/package.ps1 -Portable` contains the same `licenses/` folder.
- [THIRD_PARTY_NOTICES.md](../../THIRD_PARTY_NOTICES.md) at the repo root collects the attributions: fonts, `dxcompiler.dll` and every Rust dependency (the latter generated by `pwsh scripts/third-party-notices.ps1` from `cargo metadata --offline`; CI runs it with `-Check` to verify it is up to date and that every dependency has a licence expression).

## Adding a font

1. Download it from the official Release and check the licence (OFL-1.1, or another licence that likewise allows redistribution with software) and any Reserved Font Name; note the version and SHA-256.
2. Subset it as described above, put it in `crates/theme/fonts/` with a `<name>.LICENSE.txt` beside it; record the source, version, SHA-256, subset command and date in `crates/theme/fonts/README.md`, and add the attribution to `THIRD_PARTY_NOTICES.md`.
3. In the skin, write `file = "builtin:fonts/<name>.ttf"` and leave `families` unchanged as the fallback.
4. `cargo test -p nanatype-theme`: the tests check that every embedded font parses, carries a licence and stays within the budget in total, and that every `builtin:` reference in a built-in skin exists in `fonts/`.
