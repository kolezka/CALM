# CALM

Infinite pan/zoom canvas for macOS hosting live Ghostty terminals, WKWebView browser
panels and trusted HTML/JS plugins. Rust + GPUI.

Spec: `docs/superpowers/specs/2026-10-31-calm-design.md`
Plan: `docs/superpowers/plans/2026-10-31-calm-prototype.md`
Architecture notes: `docs/architecture.md` (written as the prototype lands)

## Workspace

- `crates/calm-core`: geometry, camera, scene, interaction, layouts, preview policy. No GPUI,
  no AppKit. Everything here has unit tests.
- `crates/calm-native`: AppKit container view, Ghostty parenting, WKWebView. All `unsafe`
  lives here, each block with a SAFETY comment.
- `crates/calm-plugins`: plugin manifest discovery, JS bridge types, `gh` client.
- `crates/calm-app`: the GPUI binary. Thin: maps events to `calm-core`, places native views.
- `plugins/`: first-party example plugins. `templates/plugin-tsx`: starter for authors.

## Build and test

- Toolchain: Rust >= 1.95, Zig 0.16 (`ZIG` env, set in `.cargo/config.toml`), Xcode CLT.
  See `docs/dev-setup.md`.
- While iterating: `cargo test -p <crate>`. Before a PR: `cargo test --workspace`,
  `cargo clippy --workspace --all-targets -- -D warnings`, `cargo fmt --all -- --check`.
- GUI gate: `cargo run -p calm-app -- --smoke; echo rc=$?`. A green build is not a running
  window. Smoke must print its lines and exit 0.
- Benchmarks: `cargo run --release -p calm-app -- --bench 20`. Record results in
  `docs/benchmarks.md` with hardware and method. Never quote a number without a run.

## Rules that are easy to break

- Native views (Ghostty, WKWebView) draw above everything GPUI paints. Nothing GPUI draws may
  overlap a live native rect. Modal UI hides native views first.
- Camera zoom scales the container view. It never changes a terminal's logical grid. Only a
  panel resize does.
- A panel's process keeps running through pan, zoom, preview, minimize and culling. Only an
  explicit, confirmed close ends it.
- No credentials in layouts, manifests, docs, logs or tests. GitHub goes through `gh api` on
  the host. Plugin JS never sees a token.
- Plugins are trusted, explicitly installed code. Do not call them sandboxed anywhere.
- After structural changes run `repo-intel build` and check its exit code.
- Code changes go through a git worktree, never the main checkout.
