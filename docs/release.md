# Releases and CI

## CI (`.github/workflows/ci.yml`)

Runs on every push to any branch and on pull requests. Two jobs:

- `linux-arch` (`archlinux:latest` container): `cargo fmt --check`, then tests and clippy for
  `calm-core` and `calm-plugins` only.
- `macos` (`macos-15`, arm64): installs Zig 0.16 from ziglang.org (checksum pinned in
  `.github/actions/macos-toolchain/action.yml`) and the Metal Toolchain, then fmt, build, test and
  clippy for the whole workspace, an informational `--smoke` run, and a bundle uploaded as the
  workflow artifact `CALM-<version>-macos-arm64`.

The smoke step has `continue-on-error: true`. Hosted runners may not give the app a usable window
server, so a failed smoke shows as a warning on the run, not a red job. Read its log before
trusting it either way.

## Linux limitation

The app is macOS only. `calm-native` is AppKit and WebKit, and `calm-app` depends on it, so
Linux CI checks the portable crates and nothing else. It does not build or ship a Linux app. When
a Linux native backend exists, change the `linux-arch` job to build and test the full workspace.

## Cutting a release

1. GitHub, Actions tab, `Release`, `Run workflow`, branch `main`.
2. Pick `bump`: `patch` (0.1.9 to 0.1.10), `minor` (0.1.9 to 0.2.0) or `major` (1.2.3 to 2.0.0).
3. Leave `dry_run` checked the first time. The dry run bumps the version on the runner, builds
   the bundle, uploads the zip as a workflow artifact and prints the tag it would create. It
   commits, tags and pushes nothing.
4. Run again with `dry_run` unchecked to publish.

The workflow refuses to run from any branch other than `main`, and fails if the tag already
exists. Only one release runs at a time (concurrency group `release`).

## What a real release changes

- `Cargo.toml`: `[workspace.package] version` (the single source of truth; all four crates use
  `version.workspace = true`). `Cargo.lock` follows via `cargo update --workspace`.
- A commit `chore(release): v<version>` pushed to `main` by `github-actions[bot]`.
- An annotated tag `v<version>`.
- A GitHub release `v<version>` with generated notes and two assets:
  `CALM-<version>-macos-arm64.zip` and `CALM-<version>-macos-arm64.zip.sha256`.

The push uses the workflow's `GITHUB_TOKEN` with `contents: write`. If `main` gets branch
protection that blocks direct pushes, the release step fails at the push and nothing is tagged
on origin; allow the Actions bot or switch to a release PR.

## The bundle

`scripts/bundle-macos.sh <version> <out-dir>` builds `calm-app` in release mode and assembles
`CALM.app` (bundle id `pl.raqz.calm`, minimum macOS 14.0). It is ad-hoc signed, not notarized.
After downloading, macOS Gatekeeper blocks it; open it once with right click, `Open`, or run
`xattr -dr com.apple.quarantine CALM.app`. Check the zip first:
`shasum -a 256 -c CALM-<version>-macos-arm64.zip.sha256`.

To bump locally without a release: `scripts/bump-version.sh patch`. Its self-test:
`scripts/bump-version.sh --self-test`.

## Cost note

The repository is private, so Actions minutes come from the account's monthly allowance, and
macOS runner minutes are billed at a multiple of Linux minutes (10x under GitHub's published
multipliers when this was written; check the current billing docs). The `macos` CI job runs on
every push to every branch, and a cold-cache build takes roughly 15 to 25 minutes. If the
allowance runs short, restrict the `macos` job to `main` and pull requests.
