#!/bin/sh
# Build a macOS release of NanaType and package it as NanaType.app.
#   scripts/package-macos.sh [--skip-build] [--profile dist|dist-small]
# Needs: nana-packager on PATH (cargo install --path <NanaUI>/crates/nana-packager).
# Produces an unsigned bundle and a zip of it; see docs/plugin-performance.md
# for what is and is not verified.
set -eu

profile=dist
build=1
while [ $# -gt 0 ]; do
    case "$1" in
        --skip-build) build=0 ;;
        --profile) profile="$2"; shift ;;
        *) echo "unknown argument: $1" >&2; exit 2 ;;
    esac
    shift
done
case "$profile" in dist | dist-small) ;; *) echo "profile must be dist or dist-small" >&2; exit 2 ;; esac

root=$(cd "$(dirname "$0")/.." && pwd)
cd "$root"
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
target=$(rustc -vV | sed -n 's/^host: //p')
: "${NANATYPE_BUILD_ID:=$(git rev-parse --short HEAD)}"
export NANATYPE_BUILD_ID

if [ "$build" = 1 ]; then
    # A local build.incremental=true must not affect the release artifact.
    NANATYPE_REQUIRE_RESOURCES=1 CARGO_INCREMENTAL=0 \
        cargo build --profile "$profile" -p nanatype --locked
fi
exe="$root/target/$profile/nanatype"

# Licences, collected before nana-packager inventories the bundle.
python3 scripts/macos-licenses.py "$target" target/licenses/THIRD_PARTY_LICENSES.txt

# Outside target/: NanaUI treats executables under a cargo target dir as dev builds.
out="$root/dist"
rm -rf "$out"
mkdir -p "$out"
nana-packager package --config packaging/nana-package.toml --exe "$exe" --out "$out" \
    --target "$target" --profile "$profile" --build-id "$NANATYPE_BUILD_ID"
app="$out/app/NanaType.app"
nana-packager validate "$app" --run

# ditto keeps the framework's symlinks.
ditto -c -k --keepParent "$app" "$out/NanaType-$version-macos-$(uname -m).zip"
echo "Packaged NanaType $version into $out"
