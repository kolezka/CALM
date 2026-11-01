# Conventions

How work on CALM is done. `CLAUDE.md` repeats the ones that are easiest to break.

## Workflow

- Spec first, then plan, then tasks. The spec in `docs/superpowers/specs/` is the binding
  authority; the plan argues from it. Conflicts resolve toward the spec.
- Execution follows superpowers subagent-driven development, with Agent Teams when lanes
  are independent. Parallel lanes need disjoint write scopes in separate worktrees.
- Rulings are recorded in the SDD ledger as `Ruling: what, why, cost if wrong`. A ruling
  that changes a rule also lands in `decisions.md` and, if it binds sessions, `CLAUDE.md`.
- Every code change happens in a git worktree under `.worktrees/`, never in the main
  checkout. Branch names: `feat/<slug>`, `fix/<slug>`, `docs/<slug>`, `ci/<slug>`.
- Merges to `main` happen only on the user's explicit instruction, and only after the full
  gate passes on the merged tree. Pushing `main` is the user's call.

## Gates

- While iterating: `cargo test -p <crate>`, once per phase. Do not rerun to double check.
- Before a merge: `gmake ci` (clippy `-D warnings`, fmt check, workspace tests, smoke,
  plugin type checks). The GUI gate is `cargo run -p calm-app -- --smoke; echo rc=$?` and
  must exit 0 with all its lines printed. A green build is not a running window.
- Capture each command's own exit code. Never read a status through a pipe to `tail` or
  `grep`.
- Clean before a pre-merge gate: `cargo clean -p calm-core -p calm-plugins -p calm-native
  -p calm-app`, or only the touched crates when time matters. Once `cargo test -p calm-app`
  linked an old `calm-plugins` rlib and failed with missing symbols.
- `cargo build --workspace` compiles neither tests nor examples.
  `crates/calm-native/examples/native_smoke.rs` stayed broken on `main` after a signature
  change (#17, #18) because merges were checked with build only. Run
  `cargo check --workspace --tests --examples`.
- Build and check after every merge, not only after a conflict. #31 changed `PanelKind`
  after #23 and #28 added code that used it: the text merge was clean and the build broke.
- Benchmarks are recorded with hardware and method. No number is quoted without a run.

### Smoke flakes

When the smoke window is occluded, AppKit reports `window_visible=Some(false)` and GPUI
stops painting. Everything driven by paint then stalls: preview refresh
(`preview_snapshots_in_3s=[0,0]`), FPS frames ("only N frames with show_stats on"), the
stats pill ("sampler has CPU but the status bar does not show it") and, until #40 lands,
the native restack (a `zorder` vs scene mismatch). Machine load was ruled out: `main` failed
at load 17 too.

- Treat those lines as known flakes. Any other smoke failure is real.
- A run that hit a flake did not pass. Do not report it as rc=0.
- Before blaming a branch, run the same smoke on `origin/main` as a control.
- Unconfirmed: a TUI "second exit not reported after restart" failure, seen on branches,
  0 of 1 runs on `main`.

## Tests

- `calm-core` is fully unit tested; new model code ships with tests.
- A bug fix ships with a regression test that fails on the old code and passes on the fix.
  Lock the invariant, not the single symptom. Run both sides and keep the output.
- Tests that depend on tools (`gh`, bun) fail loudly when the tool is missing unless an
  explicit skip variable is set (`CALM_SKIP_BUN_TESTS=1`). A green skip is not a pass.
- Fake `gh` tests share a mutex (`FAKE_GH_LOCK`) because of a Linux ETXTBSY race.
- A RED run must fail at the target assertion. A run that stops at an earlier step, such as
  a flaky smoke line, proves nothing about the fix. This holds for delegated RED reports
  too: read the failure line before accepting it.
- macOS `printf` ignores `\x` escapes. Write control files for dash counts (en and em dash)
  with python or octal escapes, and check the control is counted before trusting a zero.

## Code style

- Rust 2024 edition, stable toolchain from `rust-toolchain.toml`, pinned dependencies.
- `unsafe` only in `calm-native`, each block with a SAFETY comment.
- Comments explain why, one line is the norm. No commented-out code.
- Descriptive names. Fail loud: no silent catches, no fail-open defaults in shared layers.
- Plain writing everywhere: short sentences, everyday words, no long dashes (en or em).
- Customization first: user-visible look and behavior goes through settings or manifest
  fields with defaults and strict validation; a hard-coded choice needs a reason in
  `.ai/decisions.md`.

## Agents

- Agents never drive the GUI beyond `--smoke`. Synthetic keystrokes landed in the user's
  terminal and in other agents' windows. Manual GUI passes are the user's.
- Agents never run the Release workflow, tag, create releases or push `main`.
- Implementers do not spawn reviewers. Review is dispatched by the controller, on a model
  from a different family than the one that wrote the code.
- Briefs carry exact files, commands, the test budget (one run per suite per phase) and
  what is out of scope.
- Shut down idle agents: once a teammate or subagent is done (result reported and verified,
  or failed), the lead sends a shutdown request or stops it, confirms it left the agent
  list, and starts a new session for the next task.
- The lead dispatches all workers. Every implementation and every docs change runs in a new
  agent session; the lead coordinates and verifies.
- `kolezka-bootstrap:route-gpt-astra` and `kolezka-bootstrap:route-fable` refuse to spawn
  nested workers. An orchestrator on those types can only return ordered worker briefs
  (example: `docs/superpowers/plans/2026-11-01-plugin-api-v2-worker-briefs.md`). Plan for
  that up front.
- Routing seen to work: `route-opus` implements, `route-gpt-sol` reviews Claude-written
  code, `route-opus` reviews GPT-written code. `route-gpt-terra` failed on 2026-10-31 with
  "unknown provider for model gpt-5.6-terra".
- Teammate messages arrive late and out of order; a report often predates the lead's last
  instruction. Check `gh pr view <n>` and `git rev-parse` before acting on one.

## Review and merging

Review before merge is mandatory for every non-trivial PR. Each cross-family review so far
found real defects:

- #31: `saved_at` not stamped, `plugin_version` never filled.
- #35: `storage.set` with `undefined` overwrote data; semver grammar gaps.
- #36: rotation removed `session.json` before the new write; url tokens leaked through page
  titles; backups were 0644; no 8 KiB cap; no directory fsync.
- #39: live session settings ignored, a duplicate lost the plugin version, read-only bridge
  calls dirtied the scene.
- #41: autosave gate bypass after a read failure, restored titles overwritten, a missing cwd
  not falling back to home, an invalid restored url.

Fix findings in a new session, then verify again.

Merge loop that worked:

- One long-lived merge session. The lead sends it a PR only after verifying it, as
  `ready: #N <sha>`, and the merge is pinned to that head.
- Clean the touched crates, then build and check the merged tree. Docs-only PRs are not
  built.
- Merge order matters. PRs that touch `canvas_view.rs`, `main.rs`, `settings.rs` or
  `.ai/status.md` conflict with each other: merge one, then prepare the rest again.
- Stacked PRs: #11 (grid styles) was based on `feat/settings-modal` and merged into that
  branch after #7 had already landed it on `main`, so grid styles never reached `main`
  (re-landed as #20). Retarget to `main` first, then confirm with
  `git merge-base --is-ancestor <merge-commit> origin/main`.
- The auto-mode classifier blocks `gh pr merge` from agents unless the user has added an
  allow rule. Merges need the user's current go-ahead. A denied command is reported, never
  reshaped.

## Working with this user

- Writes Polish or English, tersely. Match the language of the last message.
- Does not want repeated reminders. State a pending decision once; open ones live in
  `.ai/open-questions.md`.
- Customization first (in `CLAUDE.md`). Plugins are unrestricted trusted code; capabilities
  are declarative only.
- The Linux port stays a plan.
- Prefers agents for all work and idle agents closed. When the user asks for a merge loop
  "without tests or review", the lead still verifies a PR before marking it ready.

## Security rules

- No credentials in layouts, manifests, docs, logs, tests or `.ai/`. GitHub access goes
  through `gh api` on the host; plugin JS never sees a token. The CI runner's PAT lives only
  in Coolify and is never read.
- Plugins are trusted, explicitly installed code. Never describe them as sandboxed.
- The self-hosted CI runner has Docker socket access. Every self-hosted job keeps the
  same-repo guard so fork pull requests never run there.
- Outline stores no secrets. Outline links never appear in the repo, commits or messages
  outside the session.

## Documentation

- `CLAUDE.md` for binding rules, `.ai/` for knowledge, `docs/` for human-facing guides.
- Update `.ai/status.md` when a task lands or a gap is found. Update `.ai/decisions.md`
  when a decision is made or reversed.
- After structural changes run `repo-intel build` and check its exit code.
