#!/usr/bin/env bash
# Build the macOS bundle on this Mac and attach it to an existing GitHub release.
# Usage: scripts/release-upload.sh <version>
# Run it from a checkout of tag v<version>, after the Release workflow created the release.
set -euo pipefail

die() {
    echo "release-upload: $*" >&2
    exit 1
}

[[ $# -eq 1 ]] || die "usage: $0 <version>"
version="$1"
tag="v$version"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "version '$version' is not MAJOR.MINOR.PATCH"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
bundle_script="$repo_root/scripts/bundle-macos.sh"

command -v gh >/dev/null || die "gh is not installed (brew install gh)"
command -v codesign >/dev/null || die "codesign is not available (install the Xcode command line tools)"
[[ -x "$bundle_script" ]] || die "bundle script $bundle_script is missing or not executable"

gh release view "$tag" --json tagName >/dev/null || die "release $tag not found; run the Release workflow first"

# The zip must match the tagged source, so refuse to build any other commit.
git -C "$repo_root" fetch --quiet origin "+refs/tags/$tag:refs/tags/$tag" ||
    die "could not fetch tag $tag from origin"
head_commit="$(git -C "$repo_root" rev-parse HEAD)"
tag_commit="$(git -C "$repo_root" rev-parse "refs/tags/$tag^{commit}")"
[[ "$head_commit" == "$tag_commit" ]] ||
    die "HEAD is $head_commit but $tag is $tag_commit; run git checkout $tag first"
[[ -z "$(git -C "$repo_root" status --porcelain --untracked-files=no)" ]] ||
    die "the checkout has uncommitted changes; the bundle must be built from the clean tag"

out_dir="$repo_root/dist"
zip="$out_dir/CALM-$version-macos-arm64.zip"
"$bundle_script" "$version" "$out_dir"

gh release upload "$tag" "$zip" "$zip.sha256" --clobber
gh release view "$tag" --json url --jq .url
