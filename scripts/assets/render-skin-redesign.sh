#!/usr/bin/env bash
# Development tool only: renders the redesign mockups in docs/skin-redesign/.
# Needs rsvg-convert, fontconfig and ImageMagick (`magick`).
#
#   scripts/assets/render-skin-redesign.sh <out-dir> [ids...]
#
# Mockups reference existing ornaments as `ornament/<id>.png` and `ornament/ink-bamboo.png`;
# librsvg only loads images beside the SVG, so everything is staged into <out-dir>/stage first.
# Fonts resolve through a private fontconfig that adds crates/theme/fonts to the system fonts.
# Pango parses '12px' inside a family name as a size: quote it, font-family="'Fusion Pixel 12px Prop zh-Hans'".
set -euo pipefail

root="$(cd "$(dirname "$0")/../.." && pwd)"
out="$(mkdir -p "$1" && cd "$1" && pwd)"
shift
stage="$out/stage"
mkdir -p "$stage/ornament" "$out/png"

builtin="$root/crates/theme/builtin"
for id in celestial citrus forest gothic lantern parchment sakura tidal; do
  cp "$builtin/$id/ornament.png" "$stage/ornament/$id.png"
done
cp "$builtin/gothic/rose.png" "$stage/ornament/gothic-rose.png"
cp "$builtin/gothic/tracery.png" "$stage/ornament/gothic-tracery.png"
cp "$builtin/ink/bamboo.png" "$stage/ornament/ink-bamboo.png"
cp "$root"/docs/skin-redesign/*.svg "$stage/"

cat > "$out/fonts.conf" <<EOF
<?xml version="1.0"?>
<!DOCTYPE fontconfig SYSTEM "fonts.dtd">
<fontconfig>
  <dir>$root/crates/theme/fonts</dir>
  <dir>/System/Library/Fonts</dir>
  <dir>/System/Library/AssetsV2/com_apple_MobileAsset_Font8</dir>
  <dir>/Library/Fonts</dir>
  <dir>~/Library/Fonts</dir>
  <cachedir>$out/fc-cache</cachedir>
</fontconfig>
EOF
export FONTCONFIG_FILE="$out/fonts.conf"
# macOS Pango otherwise goes through CoreText and ignores FONTCONFIG_FILE.
export PANGOCAIRO_BACKEND=fc

if [ "$#" -gt 0 ]; then
  files=()
  for id in "$@"; do
    files+=("$stage/$id.svg")
    [ -f "$stage/$id-compact.svg" ] && files+=("$stage/$id-compact.svg")
  done
else
  files=("$stage"/*.svg)
fi

for svg in "${files[@]}"; do
  name="$(basename "$svg" .svg)"
  rsvg-convert -z 2 "$svg" -o "$out/png/$name.png"
  printf '%s\n' "$name"
done
