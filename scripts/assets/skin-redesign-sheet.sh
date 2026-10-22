#!/usr/bin/env bash
# Development tool only: assembles before/after and thumbnail sheets for docs/skin-redesign/.
# Needs ImageMagick (`magick`).
#
#   scripts/assets/skin-redesign-sheet.sh <render-dir> <old-dir> <out-dir>
#
# <render-dir> is the output of render-skin-redesign.sh (uses <render-dir>/png/<id>.png).
# <old-dir> is a snapshots run of the current skins:
#   NANATYPE_THEME_SNAPSHOTS_ONLY=1 NANATYPE_SNAPSHOT_VIEWS=display NANATYPE_SNAPSHOT_TRANSPARENT=1 \
#     target/debug/examples/snapshots <old-dir> all zh-CN
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
render="$1"; old="$2"; out="$(mkdir -p "$3" && cd "$3" && pwd)"
# Labels use a full CJK system font: the bundled WenKai subset lacks characters such as 薇.
font="/System/Library/Fonts/Hiragino Sans GB.ttc"
tmp="$(mktemp -d)"; trap 'rm -rf "$tmp"' EXIT

ids=(halo vanilla livechat night terminal machine_learning
     adv rpg pixel comic bubble
     parchment ink crayon love_letter forest sakura
     gothic celestial tidal lantern citrus
     subtitle sonnet
     echo_vanilla echo_glass echo_bubble echo_void)
label() {
  case "$1" in
    halo) echo 光环 ;; vanilla) echo 香草 ;; livechat) echo 直播弹幕 ;; night) echo 夜色 ;;
    terminal) echo 终端 ;; machine_learning) echo 机器学习 ;; adv) echo ADV ;; rpg) echo RPG窗口 ;;
    pixel) echo 掌机像素 ;; comic) echo 漫画 ;; bubble) echo 气泡 ;; parchment) echo 羊皮纸 ;;
    ink) echo 水墨 ;; crayon) echo 蜡笔画 ;; love_letter) echo 情书 ;; forest) echo 森林精灵 ;;
    sakura) echo 樱花 ;; gothic) echo 蔷薇哥特 ;; celestial) echo 星月魔法 ;; tidal) echo 潮汐水族馆 ;;
    lantern) echo 朱砂华灯 ;; citrus) echo 柠檬汽水 ;; subtitle) echo 字幕 ;; sonnet) echo 十四行诗 ;;
    echo_vanilla) echo "Echo 原版" ;; echo_glass) echo "Echo 玻璃" ;;
    echo_bubble) echo "Echo 气泡" ;; echo_void) echo "Echo 虚空" ;;
  esac
}

# Transparent overlays are shown on a dark backdrop, everything else on a light one.
backdrop() {
  case "$1" in subtitle|sonnet|echo_glass|adv) echo "#2b3038" ;; *) echo "#e9ecf1" ;; esac
}

cell() { # <src> <w> <h> <bg> <dst>
  if [ -f "$1" ]; then
    magick "$1" -resize "${2}x${3}" -background "$4" -gravity center -extent "${2}x${3}" "$5"
  else
    magick -size "${2}x${3}" "xc:$4" -font "$font" -pointsize 18 -fill '#8a8f99' \
      -gravity center -annotate 0 '（无）' "$5"
  fi
}

rows=()
for id in "${ids[@]}"; do
  bg="$(backdrop "$id")"
  cell "$old/nanatype.$id/display.png" 640 220 "$bg" "$tmp/$id-old.png"
  cell "$render/png/$id.png" 640 220 "$bg" "$tmp/$id-new.png"
  cell "$render/png/$id-compact.png" 220 220 "$bg" "$tmp/$id-compact.png"
  magick -size 160x220 xc:white -font "$font" -pointsize 22 -fill '#1f2430' -gravity west \
    -annotate +16+0 "$(label "$id")" "$tmp/$id-label.png"
  magick "$tmp/$id-label.png" "$tmp/$id-old.png" "$tmp/$id-new.png" "$tmp/$id-compact.png" \
    -background white -splice 12x0 +append -splice 0x12 "$tmp/$id-row.png"
  rows+=("$tmp/$id-row.png")
done
head=()
for spec in "160:皮肤" "640:现状（当前版本）" "640:重设计稿" "220:紧凑态"; do
  w="${spec%%:*}"
  magick -size "${w}x48" xc:white -font "$font" -pointsize 20 -fill '#5a6070' -gravity southwest \
    -annotate +16+8 "${spec#*:}" "$tmp/head-${#head[@]}.png"
  head+=("$tmp/head-${#head[@]}.png")
done
magick "${head[@]}" -background white -splice 12x0 +append "$tmp/head.png"
magick "$tmp/head.png" "${rows[@]}" -background white -append +repage -depth 8 -quality 90 "$out/contact-sheet.webp"

# Catalogue view: every new design at selector-thumbnail width, in colour and in greyscale.
thumbs=()
for id in "${ids[@]}"; do
  bg="$(backdrop "$id")"
  cell "$render/png/$id.png" 240 84 "$bg" "$tmp/$id-thumb.png"
  thumbs+=("$tmp/$id-thumb.png")
done
magick montage -font "$font" "${thumbs[@]}" -tile 4x -geometry +8+8 -background white +repage -depth 8 -quality 90 "$out/catalogue.webp"
magick "$out/catalogue.webp" -colorspace Gray -quality 90 "$out/catalogue-gray.webp"
printf '%s\n' "$out/contact-sheet.webp" "$out/catalogue.webp" "$out/catalogue-gray.webp"
