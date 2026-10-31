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
- Benchmarks are recorded with hardware and method. No number is quoted without a run.

## Tests

- `calm-core` is fully unit tested; new model code ships with tests.
- A bug fix ships with a regression test that fails on the old code and passes on the fix.
  Lock the invariant, not the single symptom. Run both sides and keep the output.
- Tests that depend on tools (`gh`, bun) fail loudly when the tool is missing unless an
  explicit skip variable is set (`CALM_SKIP_BUN_TESTS=1`). A green skip is not a pass.
- Fake `gh` tests share a mutex (`FAKE_GH_LOCK`) because of a Linux ETXTBSY race.

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
