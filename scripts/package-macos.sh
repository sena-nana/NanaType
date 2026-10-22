#!/bin/sh
# Build a macOS release of NanaType or NanaType Editor and package it as an app.
#   scripts/package-macos.sh [--product app|editor] [--skip-build] [--profile dist|dist-small]
#   scripts/package-macos.sh --steam [--skip-build] [--profile ...]   # the Steam depot (docs/steam.md)
# Needs: nana-packager on PATH (cargo install --path <NanaUI>/crates/nana-packager).
# Produces an unsigned bundle, a zip and a disk image of it; see docs/plugin-performance.md
# for what is and is not verified. Each product builds only its own crate, so
# neither bundle contains the other's code, strings or assets.
set -eu

product=app
profile=dist
build=1
steam=0
while [ $# -gt 0 ]; do
    case "$1" in
        --product) product="$2"; shift ;;
        --skip-build) build=0 ;;
        --profile) profile="$2"; shift ;;
        --steam) steam=1 ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
    shift
done
case "$profile" in dist | dist-small) ;; *) echo "profile must be dist or dist-small" >&2; exit 2 ;; esac
case "$product" in
    app)
        package=nanatype; binary=nanatype; config=packaging/nana-package.toml
        bundle=NanaType; display=NanaType; prefix=NanaType ;;
    editor)
        package=nanatype-editor; binary=nanatype-editor; config=packaging/editor/nana-package.toml
        bundle=NanaTypeEditor; display="NanaType Editor"; prefix=NanaTypeEditor ;;
    *) echo "product must be app or editor" >&2; exit 2 ;;
esac
if [ "$steam" = 1 ] && [ "$product" != app ]; then
    echo "--steam packages NanaType's Steam depot only" >&2; exit 2
fi

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
target=$(rustc -vV | sed -n 's/^host: //p')
: "${NANATYPE_BUILD_ID:=$(git rev-parse --short HEAD)}"
export NANATYPE_BUILD_ID

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

features=""
licences=$package
if [ "$steam" = 1 ]; then
    # The Steam edition updates through SteamPipe and leaves the unofficial
    # chat sources out; it shares the GitHub release's settings folders.
    app_id=$(steam_id NANATYPE_STEAM_APP_ID)
    depot=$(steam_id NANATYPE_STEAM_DEPOT_MACOS)
    features="--no-default-features --features builtin-fonts,steam"
    licences=nanatype-steam
fi

if [ "$build" = 1 ]; then
    # A local build.incremental=true must not affect the release artifact.
    # shellcheck disable=SC2086 # $features is a list of arguments.
    NANATYPE_REQUIRE_RESOURCES=1 CARGO_INCREMENTAL=0 \
        cargo build --profile "$profile" -p "$package" --locked $features
fi
exe="$root/target/$profile/$binary"

if [ "$steam" = 1 ]; then
    # libsteam_api.dylib names itself @loader_path/, i.e. beside the
    # executable, where a bundle may not carry it: ship it in
    # Contents/Frameworks with Syphon and relink a copy of the executable.
    sdk=${STEAM_SDK_LOCATION:-$(cargo metadata --format-version 1 --offline --locked |
        python3 -c 'import json,sys,os; m=json.load(sys.stdin); print(next(os.path.dirname(p["manifest_path"]) for p in m["packages"] if p["name"] == "steamworks-sys") + "/lib/steam")')}
    mkdir -p target/steam
    cp "$sdk/redistributable_bin/osx/libsteam_api.dylib" target/steam/libsteam_api.dylib
    cp "$exe" "target/steam/$binary"
    exe="$root/target/steam/$binary"
    install_name_tool -change @loader_path/libsteam_api.dylib \
        @executable_path/../Frameworks/libsteam_api.dylib "$exe"
    codesign --force --sign - "$exe"
    sed -e "s|@ROOT@|$root|g" -e "s|@VERSION@|$version|" -e "s|@APP_ID@|$app_id|" \
        -e "s|@DEPOTS@|macos = $depot|" packaging/steam/nana-package.steam.toml \
        > target/steam/nana-package.toml
    config=target/steam/nana-package.toml
fi

# Licences, collected before nana-packager inventories the bundle.
python3 scripts/macos-licenses.py "$target" target/licenses/THIRD_PARTY_LICENSES.txt "$licences"

# Outside target/: NanaUI treats executables under a cargo target dir as dev builds.
out="$root/dist"
rm -rf "$out"
mkdir -p "$out"
nana-packager package --config "$config" --exe "$exe" --out "$out" \
    --target "$target" --profile "$profile" --build-id "$NANATYPE_BUILD_ID"
app="$out/app/$bundle.app"
nana-packager validate "$app" --run

if [ "$steam" = 1 ]; then
    # SteamPipe uploads the depot from dist/steam/depot_build_<depot>.vdf.
    echo "Packaged the Steam depot of $display $version into $out (see $out/steam)"
    exit 0
fi

# ditto keeps the framework's symlinks.
name="$prefix-$version-macos-$(uname -m)"
ditto -c -k --keepParent "$app" "$out/$name.zip"
# A disk image to drag the app into Applications from.
stage="$out/dmg"
mkdir -p "$stage"
ditto "$app" "$stage/$bundle.app"
ln -s /Applications "$stage/Applications"
hdiutil create -quiet -volname "$display" -srcfolder "$stage" -format UDZO -ov "$out/$name.dmg"
rm -rf "$stage"
echo "Packaged $display $version into $out"
