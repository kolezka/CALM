#!/usr/bin/env bash
# Portable-crate checks in archlinux:latest, for the self-hosted Linux runner.
# Usage: scripts/ci-arch.sh <ci|test>            (on the runner, needs Docker)
#        scripts/ci-arch.sh --inside <ci|test>   (inside the container, called by the above)
# ci runs fmt, tests and clippy for calm-core and calm-plugins; test runs only the tests.
set -euo pipefail

die() {
    echo "ci-arch: $*" >&2
    exit 1
}

check_mode() {
    case "$1" in
        ci | test) ;;
        *) die "unknown mode '$1', expected ci or test" ;;
    esac
}

inside() {
    local mode="$1"
    pacman -Syu --noconfirm --needed rustup base-devel git
    rustup default stable
    rustc --version
    cd /w
    if [[ "$mode" == "ci" ]]; then
        cargo fmt --all -- --check
    fi
    cargo test -p calm-core -p calm-plugins
    if [[ "$mode" == "ci" ]]; then
        cargo clippy -p calm-core -p calm-plugins --all-targets -- -D warnings
    fi
}

outside() {
    local mode="$1" repo_root name
    repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
    command -v docker >/dev/null || die "docker CLI not found on the runner"
    docker version >/dev/null || die "cannot reach a Docker daemon from the runner"
    name="calm-ci-${GITHUB_RUN_ID:-local}-${GITHUB_RUN_ATTEMPT:-1}-${GITHUB_JOB:-arch}"
    # A cancelled or timed-out job must not leave the container running on the host.
    # name is local, so expand it now: the trap runs after this function has returned.
    # shellcheck disable=SC2064
    trap "docker rm -f '$name' >/dev/null 2>&1 || true" EXIT
    # The runner reaches the host's Docker daemon, which cannot see the runner's own work
    # directory, so a bind mount would be empty. Stream the checkout in with tar instead.
    # Named volumes live on the host daemon and keep caches between runs.
    tar -C "$repo_root" --exclude=./target -cf - . |
        docker run --rm -i --init --pull=always --name "$name" \
            -v calm-arch-pacman:/var/cache/pacman/pkg \
            -v calm-arch-rustup:/root/.rustup \
            -v calm-arch-cargo:/root/.cargo \
            -v calm-arch-target:/w/target \
            -e CARGO_TERM_COLOR="${CARGO_TERM_COLOR:-auto}" \
            archlinux:latest \
            bash -c 'set -euo pipefail; mkdir -p /w; tar -C /w -xf -; exec bash /w/scripts/ci-arch.sh --inside "$1"' \
            ci-arch "$mode"
}

if [[ "${1:-}" == "--inside" ]]; then
    [[ $# -eq 2 ]] || die "usage: $0 --inside <ci|test>"
    check_mode "$2"
    inside "$2"
else
    [[ $# -eq 1 ]] || die "usage: $0 <ci|test>"
    check_mode "$1"
    outside "$1"
fi
