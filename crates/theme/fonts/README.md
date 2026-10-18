# Built-in skin fonts

Fonts here are embedded in the binary (feature `builtin-fonts`) and named by
a built-in skin as `file = "builtin:fonts/<name>.ttf"`. Each `<name>.ttf` or
`<name>.otf` needs its licence beside it as `<name>.LICENSE.txt`, or the
build fails. See [docs/fonts.md](../../../docs/fonts.md) for why these fonts,
how they are subset and the size budget.

A build with no font files here works: skins fall back to their `families`.

## What is here

The first two fonts are subsets made on 2026-10-13 with fontTools 4.66.1. Neither
upstream licence declares a Reserved Font Name (checked in each `OFL.txt`,
in the component licences and in the fonts' name tables), so the subsets keep
the upstream family names and every name record.

| File | Size | Used by |
| --- | --- | --- |
| `FusionPixel12pxProportionalSC.ttf` | 1,453,472 bytes | `pixel`, `rpg` (body, name) |
| `LXGWWenKai-Regular.ttf` | 2,142,236 bytes | `ink` (body, name, accent), `crayon` and `sonnet` (body, name) |
| `Tangerine-Bold.ttf` | 65,464 bytes | `sonnet` (default Latin body) |

### Tangerine-Bold.ttf

Added 2026-10-18, unmodified full font (no subsetting), family `Tangerine`,
Bold, by Toshi Omagari. Source: [Google Fonts](https://github.com/google/fonts/tree/main/ofl/tangerine),
upstream source commit `9b57a9f9be5ecfe95e7f8c0e6b2b827eedb9ad67`.
SHA-256: `a35368c814c71c15928ffc60cbac32bc81461f03b895a4a607df9c64b3d1548b`.
The complete upstream OFL 1.1 and copyright notice are preserved in
`Tangerine-Bold.LICENSE.txt`, embedded and displayed on the About page.
Sonnet reuses the existing WenKai subset for Chinese.

### FusionPixel12pxProportionalSC.ttf

- Font: Fusion Pixel Font 12px Proportional, Simplified Chinese
  (family `Fusion Pixel 12px Prop zh-Hans`), version 2026.09.25, by TakWolf.
- Source: <https://github.com/TakWolf/fusion-pixel-font/releases/tag/2026.09.25>,
  asset `fusion-pixel-font-12px-proportional-ttf-v2026.09.25.zip`
  (49,414,580 bytes, SHA-256
  `47585b9c4b6d8d57380e9dec75bc7e8043fdbfa77d1843c3aaa19a1e7bd15d50`).
- File taken from the archive: `fusion-pixel-12px-proportional-zh_hans.ttf`
  (7,053,672 bytes, SHA-256
  `b2ee68647dc257fa697e4d0c78f9b59461bc504a023b9a3b5ad66f358bbb1613`,
  36,436 glyphs). Subset: 7,983 glyphs, 1,453,472 bytes.
- Licence: SIL OFL 1.1. `FusionPixel12pxProportionalSC.LICENSE.txt` holds,
  verbatim, the four licence files of the archive: `OFL.txt` (Fusion Pixel,
  identical to the repository's `LICENSE-OFL`), and for the glyphs it takes
  from other fonts `LICENSES/ark-pixel/OFL.txt`, `LICENSES/cubic-11/OFL.txt`
  and `LICENSES/galmuri/LICENSE.txt`.

### LXGWWenKai-Regular.ttf

- Font: LXGW WenKai (霞鹜文楷) Regular, version 1.522, by LXGW (based on
  Fontworks' Klee One).
- Source: <https://github.com/lxgw/LxgwWenKai/releases/tag/v1.522>, asset
  `LXGWWenKai-Regular.ttf` (25,575,676 bytes, SHA-256
  `39ad71264b588165b469e35e6afb162a378dacd1f95348160240ba9038ac3009`,
  46,867 glyphs). Subset: 5,479 glyphs, 2,142,236 bytes.
- Licence: SIL OFL 1.1. `LXGWWenKai-Regular.LICENSE.txt` is the repository's
  `OFL.txt` at tag v1.522, verbatim (SHA-256
  `c38b1994a5e48ac30ac7d1da7d0409fd8fd8127dfe28a13d6e787d5b1ef34a5e`).

## How the subsets were made

Character lists (UTF-8 text files):

- `gb2312.txt`: GB2312 levels 1 and 2, 6,763 hanzi (rows 0xB0–0xF7).
- `gb2312-l1.txt`: GB2312 level 1, the 3,755 most common hanzi (rows
  0xB0–0xD7). Stands in for the 3,500-character 通用规范汉字表 level 1, which
  is not available offline; the two lists overlap almost entirely.
- `app-text.txt`: every character above U+007E in
  `crates/i18n/locales/*.ftl`, `crates/theme/builtin/*/skin.toml`,
  `app/src/**/*.rs` and `app/examples/*.rs` (UI strings, skin names, sample
  dialogue): 940 characters, 281 of them outside GB2312.

```sh
python -c "import sys; sys.stdout.reconfigure(encoding='utf-8'); print(''.join(bytes([h, l]).decode('gb2312', 'ignore') for h in range(0xB0, 0xF8) for l in range(0xA1, 0xFF)), end='')" > gb2312.txt
python -c "import sys; sys.stdout.reconfigure(encoding='utf-8'); print(''.join(bytes([h, l]).decode('gb2312', 'ignore') for h in range(0xB0, 0xD8) for l in range(0xA1, 0xFF)), end='')" > gb2312-l1.txt

U="U+0020-007E,U+00A0-00FF,U+2010-206F,U+2190-21FF,U+2460-24FF,U+2500-257F,U+25A0-25FF,U+2600-266F,U+3000-303F,U+3040-30FF,U+31F0-31FF,U+FF00-FFEF"
COMMON="--unicodes=$U --layout-features=* --name-IDs=* --name-languages=* --name-legacy --notdef-outline --no-hinting --drop-tables+=vhea,vmtx"

pyftsubset fusion-pixel-12px-proportional-zh_hans.ttf \
  --text-file=gb2312.txt --text-file=app-text.txt $COMMON \
  --output-file=FusionPixel12pxProportionalSC.ttf
pyftsubset LXGWWenKai-Regular.ttf \
  --text-file=gb2312-l1.txt --text-file=app-text.txt $COMMON \
  --output-file=LXGWWenKai-Regular.ttf
```

The ranges are ASCII, Latin-1, general punctuation, arrows, enclosed
numbers, box drawing, geometric shapes, a few symbols, CJK punctuation, kana
and full-width forms; a font keeps only the ones it has. All layout features
are kept (GSUB/GPOS together are under 10 KB in either font), hinting and the
vertical metrics tables are dropped, and the glyph names go (fontTools'
default). Characters outside the subsets fall back to system fonts glyph by
glyph.
