#!/bin/sh
# Upload the Steam depots packaged by scripts/package.ps1 -Steam or
# scripts/package-macos.sh --steam with SteamCMD (docs/steam.md).
#   scripts/steam-upload.sh --account <build account> [--platform windows|macos|all] [--preview]
#       [--windows-content <dir>] [--macos-content <dir>]
# The account can also come from STEAM_BUILD_ACCOUNT; steamcmd comes from
# STEAMCMD or PATH. steamcmd asks for the password and Steam Guard code itself
# and caches the login; this script never reads, takes or stores a password.
# The build is not set live: do that on the Steamworks builds page.
set -eu

account=${STEAM_BUILD_ACCOUNT:-}
platform=
preview=0
windows_content=
macos_content=
while [ $# -gt 0 ]; do
    case "$1" in
        --account) account="$2"; shift ;;
        --platform) platform="$2"; shift ;;
        --windows-content) windows_content="$2"; shift ;;
        --macos-content) macos_content="$2"; shift ;;
        --preview) preview=1 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
    shift
done
if [ -z "$platform" ]; then
    case "$(uname -s)" in
        Darwin) platform=macos ;;
        MINGW* | MSYS* | CYGWIN*) platform=windows ;;
        *) echo "pass --platform windows|macos|all" >&2; exit 2 ;;
    esac
fi
case "$platform" in windows | macos | all) ;; *) echo "platform must be windows, macos or all" >&2; exit 2 ;; esac
if [ -z "$account" ]; then
    echo "No build account: pass --account <name> or set STEAM_BUILD_ACCOUNT" >&2; exit 2
fi

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"

# A Steam id from the environment, else from the gitignored .cargo/config.toml [env].
steam_id() {
    eval "value=\${$1:-}"
    if [ -z "$value" ] && [ -f .cargo/config.toml ]; then
        value=$(sed -n "s/^[[:space:]]*$1[[:space:]]*=[[:space:]]*\"\{0,1\}\([0-9]*\)\"\{0,1\}.*/\1/p" .cargo/config.toml | head -n 1)
    fi
    case "$value" in
        '' | 0 | *[!0-9]*)
            echo "$1 is not set: add it to .cargo/config.toml [env] (see .cargo/config.example.toml)" >&2
            exit 2 ;;
    esac
    echo "$value"
}
app_id=$(steam_id NANATYPE_STEAM_APP_ID)
app="$root/dist/app"
: "${windows_content:=$app/NanaType}"
: "${macos_content:=$app}"

# Generated scripts and SteamPipe output stay under the gitignored target/.
dir="$root/target/steampipe"
output="$dir/output"
mkdir -p "$output"
rm -f "$dir"/app_build_*.vdf "$dir"/depot_build_*.vdf
depots=""
names=""
# Writes the depot script for one platform and adds it to the app build.
add_depot() { # name, depot id, content root
    sed -e "s|@DEPOT_ID@|$2|" -e "s|@CONTENT_ROOT@|$3|" packaging/steam/depot_build.vdf.in \
        > "$dir/depot_build_$2.vdf"
    depots="$depots$(printf '\t\t"%s" "depot_build_%s.vdf"' "$2" "$2")
"
    names="${names:+$names+}$1"
}
if [ "$platform" != macos ]; then
    if [ ! -f "$windows_content/NanaType.exe" ]; then
        echo "No NanaType.exe in $windows_content: run pwsh scripts/package.ps1 -Steam first" >&2; exit 2
    fi
    add_depot windows "$(steam_id NANATYPE_STEAM_DEPOT_WINDOWS)" "$(cd "$windows_content" && pwd)"
fi
if [ "$platform" != windows ]; then
    if [ ! -d "$macos_content/NanaType.app" ]; then
        echo "No NanaType.app in $macos_content: run scripts/package-macos.sh --steam first" >&2; exit 2
    fi
    add_depot macos "$(steam_id NANATYPE_STEAM_DEPOT_MACOS)" "$(cd "$macos_content" && pwd)"
fi

version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
desc="NanaType $version ($(git rev-parse --short HEAD)) $names"
vdf="$dir/app_build_$app_id.vdf"
# awk inserts the multi-line depot list, which sed cannot take as a replacement.
DEPOTS="$depots" awk -v app_id="$app_id" -v desc="$desc" -v root="$app" -v out="$output" -v preview="$preview" '
    /^@DEPOTS@$/ { printf "%s", ENVIRON["DEPOTS"]; next }
    { gsub(/@APP_ID@/, app_id); gsub(/@DESC@/, desc); gsub(/@CONTENT_ROOT@/, root)
      gsub(/@BUILD_OUTPUT@/, out); gsub(/@PREVIEW@/, preview); print }
' packaging/steam/app_build.vdf.in > "$vdf"

echo "Uploading $desc with $vdf"
if ! "${STEAMCMD:-steamcmd}" +login "$account" +run_app_build "$vdf" +quit; then
    echo "steamcmd failed; logs in $output" >&2; exit 1
fi
if [ "$preview" = 1 ]; then
    echo "Preview only: nothing was uploaded; the file lists are in $output"
else
    echo "Uploaded. Set the build live at https://partner.steamgames.com/apps/builds/$app_id"
fi
