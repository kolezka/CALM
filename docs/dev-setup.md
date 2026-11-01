# Developer setup

Target machine: an arm64 Mac on macOS 14 or newer. Other platforms are not supported yet.

## Prerequisites

- Rust 1.95 or newer: `rustup update stable`
- Zig 0.16 (the Ghostty build needs exactly this series): `brew install zig@0.16`
- Xcode command line tools (`xcode-select --install`)
- Xcode Metal Toolchain: `xcodebuild -downloadComponent MetalToolchain` (needed by the GPUI build)
- `gh` logged in (`gh auth login`) for the GitHub pull-request plugin
- `bun` for building TSX plugins (`curl -fsSL https://bun.sh/install | bash`)
- GNU Make 4 or newer: `brew install make` (Homebrew provides `gmake`)

`.cargo/config.toml` points `ZIG` at `/opt/homebrew/opt/zig@0.16/bin/zig`. If your Zig lives
elsewhere, set `ZIG=/path/to/zig` in the environment; it wins over the config file.

## Run

```sh
cargo run -p calm-app                 # windowed
cargo run -p calm-app -- --fullscreen
cargo run -p calm-app -- --smoke; echo rc=$?   # GUI gate, exits 0 when the window and terminals work
```

`--layout` and `--bench` are not built yet, and the canvas is not saved or restored between
launches. Settings live in `~/.config/calm/settings.json` (`$CALM_HOME/settings.json` when
`CALM_HOME` is set) and are edited in the Settings window (`cmd-,`). See
[configuration.md](configuration.md). Smoke runs should use a throwaway home, for example
`CALM_HOME=$(mktemp -d) cargo run -p calm-app -- --smoke`, so they never touch your real
settings.

### Smoke steps and the summary

The smoke runs a fixed list of named steps (`terminals`, `zorder`, `join`, `app_icon`,
`native_reorder`, `plugins`, `tui`, `show_stats`, `fps`, `stats`, `preview`,
`preview_cadence`, `grid`, `preview_input_targets`, `live`, `background`, `browser`,
`resize`, `blur`, `settings_dispatch`, `github_api`, `plugin_menus`, `session`). The list
lives in `crates/calm-app/src/smoke_runner.rs`. A failed step does not stop the run. Each
step prints `smoke: step <name> PASS`, `FAIL <label> <reason>` or `SKIPPED <reason>`. A step
is skipped only when a step it needs failed (for example `preview_cadence` needs `preview`).
Window, settings and panel setup still stop the smoke at once when they fail.

The run ends with a summary:

```text
smoke: summary pass=16 fail=4 skipped=3
smoke: summary labels known-flake:few-frames=1 known-flake:stats-pill=1 new=2
smoke: summary FAIL fps known-flake:few-frames only 4 frames with show_stats on
smoke: summary SKIPPED grid needs preview, which failed
```

The exit code is 0 only when every selected step passed. Each failure is labeled
`known-flake:<id>` when it matches a flake listed in `.ai/conventions.md` ("Smoke flakes"),
otherwise `new`. The label is a hint for reading the log, nothing more: a known flake is
still a failure and still exits 1. A failed step can leave panels or zoom behind, so read
the failures in order; a `new` failure right after another failure may be a follow-on.

Run only some steps with `CALM_SMOKE_ONLY` (comma separated names). The steps they need run
too, and the smoke prints the final list first. An unknown name stops the smoke with exit 1.

```sh
CALM_HOME=$(mktemp -d) CALM_SMOKE_ONLY=session,zorder cargo run -p calm-app -- --smoke; echo rc=$?
```

For the GitHub PRs plugin, `gh auth login` is enough. Or put `GITHUB_TOKEN=<your token>` in
`~/.config/calm/.env` (`chmod 600`); a dev build also reads `.env` at the checkout root,
which `.gitignore` covers. See [plugins.md](plugins.md#github-access).

Plugins are read from every directory in `CALM_PLUGIN_PATH` (colon separated), from
`~/.config/calm/plugins/<id>/` (or `$CALM_HOME/plugins`), and, in a development build, from the
repository's own `plugins/` folder. So the dev flow needs no setup:

```sh
gmake plugins && cargo run -p calm-app   # then right-click > Add
```

`gmake plugins` builds every folder with a `package.json` under `plugins/` and `templates/`
and fails when bun is missing. TUI app plugins such as `plugins/top-example` need no build.

`CALM_NO_BUILTIN_PLUGINS=1` or the setting `plugins.builtin: false` turns the built-in folders
off. Startup prints every searched folder and every plugin found to stderr. See
[plugins.md](plugins.md#where-plugins-live).

## Tests

```sh
cargo test -p calm-core          # while iterating
cargo test --workspace           # before a PR
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

Some `calm-plugins` tests run JavaScript through bun and fail without it.
`CALM_SKIP_BUN_TESTS=1` skips them.

CI, the release workflow and the macOS bundle script are described in `docs/release.md`.

## Make targets

Run `gmake help` to list targets. `gmake setup-check` prints prerequisite versions and checks the local toolchain.

- `gmake build`, `gmake test`, `gmake lint`, and `gmake fmt` build, test, lint, and format the Rust workspace.
- `gmake smoke` runs the GUI smoke gate; `gmake run ARGS=--fullscreen` launches the app; `gmake bench` exists but calls `--bench`, which is not built yet.
- `gmake bundle VERSION=0.1.0 OUT=dist` builds a macOS bundle.
- `gmake plugins` installs and builds each plugin package and fails when bun is missing; `gmake plugins-check` type-checks each package.
- `gmake ci` runs lint, tests, smoke, and plugin checks in order; `gmake graph` refreshes the repo-intel graphs.
- `gmake clean` removes Rust and plugin build outputs.
