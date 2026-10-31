# Developer setup

Target machine: an arm64 Mac on macOS 14 or newer. Other platforms are not supported yet.

## Prerequisites

- Rust 1.95 or newer: `rustup update stable`
- Zig 0.16 (the Ghostty build needs exactly this series): `brew install zig@0.16`
- Xcode command line tools (`xcode-select --install`)
- Xcode Metal Toolchain: `xcodebuild -downloadComponent MetalToolchain` (needed by the GPUI build)
- `gh` logged in (`gh auth login`) for the GitHub pull-request plugin
- `bun` for building TSX plugins (`curl -fsSL https://bun.sh/install | bash`)

`.cargo/config.toml` points `ZIG` at `/opt/homebrew/opt/zig@0.16/bin/zig`. If your Zig lives
elsewhere, set `ZIG=/path/to/zig` in the environment; it wins over the config file.

## Run

```sh
cargo run -p calm-app                 # windowed
cargo run -p calm-app -- --fullscreen
cargo run -p calm-app -- --layout work   # load ~/.config/calm/layouts/work.json
cargo run -p calm-app -- --smoke; echo rc=$?   # GUI gate, exits 0 when the window and terminals work
cargo run --release -p calm-app -- --bench 20   # see docs/benchmarks.md
```

Plugins are read from `~/.config/calm/plugins/<id>/` and from every directory in
`CALM_PLUGIN_PATH` (colon separated). For development: `CALM_PLUGIN_PATH=./plugins`.

## Tests

```sh
cargo test -p calm-core          # while iterating
cargo test --workspace           # before a PR
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
```

## Make targets

Run `make` or `make help` to list targets. `make setup-check` prints prerequisite versions and checks the local toolchain.

- `make build`, `make test`, `make lint`, and `make fmt` build, test, lint, and format the Rust workspace.
- `make smoke` runs the GUI smoke gate; `make run ARGS=--fullscreen` launches the app; `make bench N=20 SECONDS=30 ZOOM=1.0` runs a benchmark.
- `make bundle VERSION=0.1.0 OUT=dist` builds a macOS bundle.
- `make plugins` installs and builds each plugin package; `make plugins-check` type-checks each package.
- `make ci` runs lint, tests, smoke, and plugin checks in order; `make graph` refreshes the repo-intel graphs.
- `make clean` removes Rust and plugin build outputs.
