# Releases and CI

## CI (`.github/workflows/ci.yml`)

Runs on every push to any branch and on pull requests. Two jobs, one of them opt-in:

- `linux-arch` (`archlinux:latest` on the self-hosted runner, via `scripts/ci-arch.sh`): `cargo fmt --check`,
  then tests and clippy for `calm-core` and `calm-plugins` only.
- `macos` (`macos-15`, arm64), skipped unless the repository variable `CALM_MACOS_HOSTED` is
  `true`: installs Zig 0.16 from ziglang.org (checksum pinned in
  `.github/actions/macos-toolchain/action.yml`) and the Metal Toolchain, then fmt, build, test and
  clippy for the whole workspace, an informational `--smoke` run, and a bundle uploaded as the
  workflow artifact `CALM-<version>-macos-arm64`.

The smoke step has `continue-on-error: true`. Hosted runners may not give the app a usable window
server, so a failed smoke does not turn the job red. The next step, `Smoke result`, reads the
smoke step's outcome and adds a warning annotation and a line in the job summary when it did not
pass. Read the smoke log before trusting it either way.

### Hosted macOS job is opt-in

GitHub-hosted macOS minutes are currently blocked by an account billing problem, so the `macos`
job has `if: vars.CALM_MACOS_HOSTED == 'true'`. Without the variable the job shows as skipped,
not failed. To turn it back on once billing works:

```sh
gh variable set CALM_MACOS_HOSTED --body true
```

`gh variable delete CALM_MACOS_HOSTED` turns it off again. While it is off, nothing in CI builds
or tests `calm-native` or `calm-app`: run `make ci` on a Mac before merging.

## Self-hosted Linux runner

Every Linux job (`linux-arch` in both workflows and the release `guard`) runs on
`[self-hosted, linux, calm]`: a `myoung34/github-runner` container on the OVH host, deployed
with Coolify and registered to this repository with labels `self-hosted,linux,x64,ovh,calm`.
If no runner with those labels is online, the jobs wait in the queue rather than fail.

The Arch steps run through `scripts/ci-arch.sh`, not a job-level `container:`. The runner talks
to the host's Docker daemon through its socket, and the daemon cannot see the runner's own work
directory, so `container:` (and any `-v "$GITHUB_WORKSPACE":...` bind mount) gets an empty
workspace: the first run failed with `sh: /__w/_temp/<id>.sh: No such file or directory`. The
script streams the checkout into `archlinux:latest` with `tar` and keeps pacman, rustup, cargo
and `target` caches in named Docker volumes (`calm-arch-*`) on the host. To clear the caches:
`docker volume rm calm-arch-pacman calm-arch-rustup calm-arch-cargo calm-arch-target`. `.github/actionlint.yaml` lists the custom
labels so `actionlint` accepts them. The `release` job in `release.yml` also runs on this runner.
Only the opt-in `macos` CI job uses GitHub-hosted `macos-15`.

Fork pull requests never run on the self-hosted runner because it has access to the host Docker daemon. The workflows enforce a same-repository check for every self-hosted job. The repository setting also blocks fork workflows today, but the job guard protects the runner if that setting changes.

## Linux limitation

The app is macOS only. `calm-native` is AppKit and WebKit, and `calm-app` depends on it, so
Linux CI checks the portable crates and nothing else. It does not build or ship a Linux app. When
a Linux native backend exists, change the `linux-arch` job to build and test the full workspace.

## Cutting a release

A release has two halves. The `Release` workflow runs on the self-hosted Linux runner: it bumps
the version, tags it, and creates the GitHub release with a source tarball. The macOS zip cannot
be built on Linux, so you build it on a Mac and attach it with `scripts/release-upload.sh`.

1. GitHub, Actions tab, `Release`, `Run workflow`, branch `main`.
2. Pick `bump`: `patch` (0.1.9 to 0.1.10), `minor` (0.1.9 to 0.2.0) or `major` (1.2.3 to 2.0.0).
3. Leave `dry_run` checked the first time. The dry run bumps the version on the runner, builds
   `CALM-<version>-source.tar.gz` from the dispatched commit, uploads it as a workflow artifact
   and prints the tag it would create. It commits, tags and pushes nothing.
4. Run again with `dry_run` unchecked to publish.
5. On a Mac, check out the new tag and attach the bundle:

   ```sh
   git fetch --tags origin
   git checkout v<version>
   scripts/release-upload.sh <version>     # or: make release-upload VERSION=<version>
   ```

   The script checks that `gh release view v<version>` succeeds, that `HEAD` is the tagged commit
   and that the checkout has no uncommitted changes. It then runs
   `scripts/bundle-macos.sh <version> dist`, uploads `CALM-<version>-macos-arm64.zip` and its
   `.sha256` with `gh release upload --clobber` (so a rerun replaces them), and prints the release
   URL. It needs `gh` (authenticated) and `codesign` from the Xcode command line tools.

The workflow refuses to run from any branch other than `main`. Only one release runs at a time
(concurrency group `release`). Both jobs check out the commit the run was dispatched on
(`github.sha`). `linux-arch` runs the portable-crate tests in Arch first, and `release` only
starts if they pass. The final push is not forced: if `main` moved after the dispatch, it is
rejected and nothing is tagged.

The runner image has no Rust, and `scripts/bump-version.sh` runs `cargo update` to refresh
`Cargo.lock`. The `release` job therefore installs a minimal stable toolchain with rustup into
the job's temp directory when `cargo` is missing. When `gh` is missing it downloads a pinned
`gh` release for linux amd64 into the temp directory, checks the tarball against that release's
checksums file and against a SHA-256 pinned in the workflow, and puts it on `PATH`. To move to a
newer `gh`, update `GH_CLI_VERSION` and `GH_CLI_SHA256` in `release.yml` from
`gh_<version>_checksums.txt` of the new release. The job also checks that `git`, `gh`, `tar`, `gzip`,
`sha256sum` and `curl` are on the runner and fails with a named error if one is missing.

The release commit is pushed with `GITHUB_TOKEN`, and GitHub does not trigger other workflows
from such pushes, so it gets no separate `ci` run. It changes only the version in `Cargo.toml`
and `Cargo.lock`. Nothing in the release workflow builds the app: the macOS build and its
checks happen when you run `scripts/release-upload.sh`. Run `make ci` on the Mac first if the
hosted macOS job is off.

## Recovering a failed release

The push of the commit and tag happens before `gh release create`. If the release creation or the
asset upload fails, the tag is on origin without a release. Recover by running the workflow again:

- Re-run the failed run, or dispatch a new run on `main` with the same `bump`. The `Plan version`
  step sees that tag `v<version>` exists, that the tagged commit's `Cargo.toml` has that version
  and that no release exists. It then checks out the tag, skips the bump, commit, tag and push,
  and goes straight to the source tarball and `gh release create`.
- If a draft release for that tag exists (an upload that failed halfway can leave one), the run
  stops and asks you to delete it: `gh release delete v<version>`. Then run again.
- If the tag already has a published release, or the tag's `Cargo.toml` version differs from the
  tag, the run fails with a message and changes nothing.

If `scripts/release-upload.sh` fails after the release exists, fix the cause and run it again.
`--clobber` replaces any half-uploaded asset.

## What a real release changes

- `Cargo.toml`: `[workspace.package] version` (the single source of truth; all four crates use
  `version.workspace = true`). `Cargo.lock` follows via `cargo update --workspace`.
- A commit `chore(release): v<version>` pushed to `main` by `github-actions[bot]`.
- An annotated tag `v<version>`.
- A GitHub release `v<version>` with generated notes, a first line saying the macOS bundle is
  built and uploaded locally with `scripts/release-upload.sh <version>`, and two assets:
  `CALM-<version>-source.tar.gz` (`git archive` of the tag, prefix `CALM-<version>/`) and
  `CALM-<version>-source.tar.gz.sha256`.
- After `scripts/release-upload.sh`: two more assets, `CALM-<version>-macos-arm64.zip` and
  `CALM-<version>-macos-arm64.zip.sha256`.

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
multipliers when this was written; check the current billing docs). With `CALM_MACOS_HOSTED`
set, the `macos` CI job runs on every push to every branch, and a cold-cache build takes roughly
15 to 25 minutes. If the allowance runs short, unset the variable or restrict the job to `main`
and pull requests. The self-hosted Linux jobs use no hosted minutes.
