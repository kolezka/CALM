#!/usr/bin/env bash
# Bump the workspace version in Cargo.toml and print the new version.
# Usage: scripts/bump-version.sh <patch|minor|major>
#        scripts/bump-version.sh --self-test
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

die() {
    echo "bump-version: $*" >&2
    exit 1
}

# Print the version from the [workspace.package] table of the given Cargo.toml.
read_version() {
    awk '
        /^\[/ { in_pkg = ($0 == "[workspace.package]") ; next }
        in_pkg && /^version[[:space:]]*=/ {
            line = $0
            sub(/^version[[:space:]]*=[[:space:]]*"/, "", line)
            sub(/".*$/, "", line)
            print line
            exit
        }
    ' "$1"
}

# Print the next version for a bump word, or fail on bad input.
next_version() {
    local current="$1" kind="$2"
    [[ "$current" =~ ^([0-9]+)\.([0-9]+)\.([0-9]+)$ ]] ||
        die "current version '$current' is not plain semver MAJOR.MINOR.PATCH"
    local major="${BASH_REMATCH[1]}" minor="${BASH_REMATCH[2]}" patch="${BASH_REMATCH[3]}"
    case "$kind" in
        patch) echo "$major.$minor.$((patch + 1))" ;;
        minor) echo "$major.$((minor + 1)).0" ;;
        major) echo "$((major + 1)).0.0" ;;
        *) die "unknown bump '$kind', expected patch, minor or major" ;;
    esac
}

# Rewrite the [workspace.package] version in place and print the new version.
bump_file() {
    local file="$1" kind="$2" current new tmp
    current="$(read_version "$file")"
    [[ -n "$current" ]] || die "no version in [workspace.package] of $file"
    new="$(next_version "$current" "$kind")"
    tmp="$(mktemp)"
    awk -v new="$new" '
        /^\[/ { in_pkg = ($0 == "[workspace.package]") }
        in_pkg && !done && /^version[[:space:]]*=/ {
            print "version = \"" new "\""
            done = 1
            next
        }
        { print }
    ' "$file" >"$tmp"
    cat "$tmp" >"$file"
    rm -f "$tmp"
    [[ "$(read_version "$file")" == "$new" ]] || die "rewrite of $file did not take"
    echo "$new"
}

self_test() {
    local dir fails=0
    dir="$(mktemp -d)"
    trap 'rm -rf "$dir"' RETURN

    check() {
        local start="$1" kind="$2" want="$3" got
        cat >"$dir/Cargo.toml" <<EOF
[workspace]
members = ["crates/*"]

[workspace.package]
edition = "2024"
version = "$start"

[workspace.dependencies]
serde = { version = "1", features = ["derive"] }
EOF
        got="$(bump_file "$dir/Cargo.toml" "$kind")"
        # The dependency's own version = "1" must survive untouched.
        if [[ "$got" == "$want" ]] && grep -q 'serde = { version = "1"' "$dir/Cargo.toml"; then
            echo "ok: $start $kind -> $got"
        else
            echo "FAIL: $start $kind -> got '$got', want '$want'"
            fails=$((fails + 1))
        fi
    }

    check 0.1.9 patch 0.1.10
    check 0.1.9 minor 0.2.0
    check 1.2.3 major 2.0.0

    # Bad inputs must fail.
    if (next_version 0.1.0 bogus) >/dev/null 2>&1; then
        echo "FAIL: unknown bump word accepted"
        fails=$((fails + 1))
    else
        echo "ok: unknown bump word rejected"
    fi
    if (next_version 0.1.0-beta.1 patch) >/dev/null 2>&1; then
        echo "FAIL: non-semver version accepted"
        fails=$((fails + 1))
    else
        echo "ok: non-semver version rejected"
    fi

    [[ "$fails" -eq 0 ]] || die "$fails self-test check(s) failed"
    echo "self-test: all checks passed"
}

[[ $# -eq 1 ]] || die "usage: $0 <patch|minor|major> | --self-test"

if [[ "$1" == "--self-test" ]]; then
    self_test
    exit 0
fi

case "$1" in
    patch | minor | major) ;;
    *) die "unknown bump '$1', expected patch, minor or major" ;;
esac

new="$(bump_file "$repo_root/Cargo.toml" "$1")"
(cd "$repo_root" && cargo update --workspace --quiet)
echo "$new"
