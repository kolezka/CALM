#!/usr/bin/env bash
# Build calm-app in release mode and package it as an ad-hoc signed CALM.app plus a zip.
# Usage: scripts/bundle-macos.sh <version> <out-dir>
set -euo pipefail

die() {
    echo "bundle-macos: $*" >&2
    exit 1
}

[[ $# -eq 2 ]] || die "usage: $0 <version> <out-dir>"
version="$1"
out_dir="$2"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "version '$version' is not MAJOR.MINOR.PATCH"
[[ "$(uname -s)" == "Darwin" ]] || die "macOS only"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
target_dir="${CARGO_TARGET_DIR:-$repo_root/target}"

(cd "$repo_root" && cargo build --release -p calm-app)

mkdir -p "$out_dir"
out_dir="$(cd "$out_dir" && pwd)"
app="$out_dir/CALM.app"
zip_name="CALM-$version-macos-arm64.zip"

# Start clean so stale files from an older bundle never ship.
rm -rf "${app:?}" "${out_dir:?}/$zip_name" "${out_dir:?}/$zip_name.sha256"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"

cp "$target_dir/release/calm-app" "$app/Contents/MacOS/calm-app"
cp "$repo_root/assets/brand/CALM.icns" "$app/Contents/Resources/CALM.icns"
# First-party plugins ship in Resources/plugins, where the app looks for built-ins.
mkdir -p "$app/Contents/Resources/plugins"
for manifest in "$repo_root"/plugins/*/calm-plugin.json; do
    plugin_dir="$(dirname "$manifest")"
    name="$(basename "$plugin_dir")"
    if [[ -f "$plugin_dir/package.json" && ! -d "$plugin_dir/dist" ]]; then
        die "plugin $name is not built; run gmake plugins first"
    fi
    rsync -a --exclude node_modules --exclude src --exclude '*.lock' \
        --exclude package.json --exclude tsconfig.json \
        "$plugin_dir/" "$app/Contents/Resources/plugins/$name/"
done

sed "s/@VERSION@/$version/g" "$repo_root/scripts/Info.plist.template" >"$app/Contents/Info.plist"
plutil -lint "$app/Contents/Info.plist" >/dev/null

# Ad-hoc signature: runs locally, not notarized.
codesign --force --deep --sign - "$app"
codesign --verify --verbose "$app"

# ditto keeps the bundle layout and signature intact, unlike plain zip.
(cd "$out_dir" && ditto -c -k --keepParent CALM.app "$zip_name")
(cd "$out_dir" && shasum -a 256 "$zip_name" >"$zip_name.sha256")

echo "$out_dir/$zip_name"
cat "$out_dir/$zip_name.sha256"
