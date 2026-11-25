#!/bin/sh
# Build a macOS release of NanaType or NanaType Editor and package it as an app.
#   scripts/package-macos.sh [--product app|editor] [--skip-build] [--profile dist|dist-small]
# Needs: nana-packager on PATH (cargo install --path <NanaUI>/crates/nana-packager).
# Produces an unsigned bundle, a zip and a disk image of it; see docs/plugin-performance.md
# for what is and is not verified. Each product builds only its own crate, so
# neither bundle contains the other's code, strings or assets.
set -eu

product=app
profile=dist
build=1
while [ $# -gt 0 ]; do
    case "$1" in
        --product) product="$2"; shift ;;
        --skip-build) build=0 ;;
        --profile) profile="$2"; shift ;;
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

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
target=$(rustc -vV | sed -n 's/^host: //p')
: "${NANATYPE_BUILD_ID:=$(git rev-parse --short HEAD)}"
export NANATYPE_BUILD_ID

if [ "$build" = 1 ]; then
    # The recognizer must not specialize to the packaging host's CPU.
    GGML_NATIVE=OFF
    export GGML_NATIVE
    # A local build.incremental=true must not affect the release artifact.
    NANATYPE_REQUIRE_RESOURCES=1 CARGO_INCREMENTAL=0 \
        cargo build --profile "$profile" -p "$package" --locked
fi
exe="$root/target/$profile/$binary"
# The `cargo metadata --offline` calls below read the whole locked graph,
# including crates this build never compiled (other platforms, dev-only):
# download them all first, or a clean machine such as CI cannot list them.
cargo fetch --locked

# Licences, collected before nana-packager inventories the bundle.
python3 scripts/macos-licenses.py "$target" target/licenses/THIRD_PARTY_LICENSES.txt "$package"

# The built-in skins ship as a folder in the bundle's resources, not in the
# executable (which keeps only the default skin, as a fallback). The SVG design
# masters and dotfiles stay in the repository: skins load only rendered PNGs.
rm -rf target/skins
cp -R crates/theme/builtin target/skins
find target/skins \( -name '.*' -o -name '*.svg' \) -type f -delete
# Their fonts, each with its licence (docs/fonts.md).
rm -rf target/fonts
mkdir -p target/fonts
find crates/theme/fonts -maxdepth 1 -type f \
    \( -name '*.ttf' -o -name '*.otf' -o -name '*.LICENSE.txt' \) -exec cp {} target/fonts/ \;

# Outside target/: NanaUI treats executables under a cargo target dir as dev builds.
out="$root/dist"
rm -rf "$out"
mkdir -p "$out"
nana-packager package --config "$config" --exe "$exe" --out "$out" \
    --target "$target" --profile "$profile" --build-id "$NANATYPE_BUILD_ID"
app="$out/app/$bundle.app"
nana-packager validate "$app" --run

# ditto preserves the app bundle's metadata and symlinks.
name="$prefix-$version-macos-$(uname -m)"
ditto -c -k --keepParent "$app" "$out/$name.zip"
# A disk image to drag the app into Applications from.
stage="$out/dmg"
mkdir -p "$stage"
ditto "$app" "$stage/$bundle.app"
ln -s /Applications "$stage/Applications"
# Not hdiutil -quiet: it closes stderr, so a failure exits with no message.
# create -srcfolder also fails now and then on hosted macOS runners
# ("Resource busy"), so flush and try again a few times.
echo "Creating $out/$name.dmg"
sync
i=0
until hdiutil create -volname "$display" -srcfolder "$stage" -format UDZO -ov "$out/$name.dmg"; do
    i=$((i + 1))
    if [ "$i" -ge 5 ]; then
        echo "hdiutil create failed after $i attempts" >&2
        exit 1
    fi
    echo "hdiutil create failed, retrying ($i)" >&2
    sleep $((i * 2))
    sync
done
rm -rf "$stage"
echo "Packaged $display $version into $out"
