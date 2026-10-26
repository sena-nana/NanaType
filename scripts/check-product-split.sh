#!/bin/sh
# Fail when NanaType links NanaType Editor's code or strings, or the editor
# links NanaType's own features; or when NanaType's Steam edition links the
# unofficial chat sources or the updater, or another build links Steamworks.
# Both products share nanatype-frame; each release builds only its own crate
# (`cargo build -p ...`), so what `cargo tree -p` resolves for it is exactly
# what ships.
#   scripts/check-product-split.sh
set -eu
cd "$(dirname "$0")/.."

fail=0
deps() { cargo tree --locked -q -p "$@" -e normal --prefix none -f '{p}' | sed 's/ .*//' | sort -u; }
features() { cargo tree --locked -q -p "$1" -e features -i "$2" --prefix none 2>/dev/null || true; }
forbid() { # product, what, list
    if [ -n "$3" ]; then
        echo "$1 must not link $2:" >&2
        echo "$3" | sed 's/^/  /' >&2
        fail=1
    fi
}

app=$(deps nanatype)
editor=$(deps nanatype-editor)
# The Steam edition's feature set (docs/steam.md).
steam=$(deps nanatype --no-default-features --features builtin-fonts,steam)

forbid nanatype "the editor" "$(echo "$app" | grep -x 'nanatype-editor' || true)"
forbid nanatype "package authoring (nanatype-theme/editor)" \
    "$(features nanatype nanatype-theme | grep -F 'feature "editor"' || true)"
forbid nanatype "the editor's strings (nanatype-i18n/editor)" \
    "$(features nanatype nanatype-i18n | grep -F 'feature "editor"' || true)"

forbid nanatype "the Steamworks SDK" "$(echo "$app" | grep -xE 'steamworks(-sys)?' || true)"
forbid "nanatype (Steam edition)" "the unofficial chat sources or the updater" \
    "$(echo "$steam" | grep -xE 'nanatype-(bilibili|douyin|youtube|update)' || true)"
if ! echo "$steam" | grep -qx steamworks; then
    echo "nanatype (Steam edition) must link the Steamworks SDK" >&2
    fail=1
fi

forbid nanatype-editor "NanaType itself" "$(echo "$editor" | grep -x 'nanatype' || true)"
forbid nanatype-editor "NanaType's live, output or session crates" \
    "$(echo "$editor" | grep -xE 'nanatype-(speech|bilibili|blive|bridge|douyin|p2p|steam|twitch|youtube|update)' || true)"
forbid nanatype-editor "NanaType's strings (nanatype-i18n/app)" \
    "$(features nanatype-editor nanatype-i18n | grep -F 'feature "app"' || true)"
forbid nanatype-editor "native window export (Spout/Syphon)" \
    "$(features nanatype-editor nana-ui | grep -F 'feature "native-export"' || true)"
forbid nanatype-editor "NanaType's chat, sticker and output libraries" \
    "$(echo "$editor" | grep -xE 'grammers-client|shiguredo_libvpx|tray-icon|rasterlottie|nanalive-spout|nanalive-syphon|whisper-rs|whisper-rs-sys|webrtc-vad' || true)"

if [ "$fail" = 0 ]; then
    echo "Product split holds: NanaType $(echo "$app" | wc -l | tr -d ' ') crates, its Steam edition $(echo "$steam" | wc -l | tr -d ' '), NanaType Editor $(echo "$editor" | wc -l | tr -d ' ') crates."
fi
exit "$fail"
