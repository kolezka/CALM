# CALM knowledge map

Start here. `CLAUDE.md` holds the rules that bind every session; this folder holds the
knowledge behind them. Read in this order for a cold start: `system.md`, `status.md`,
`conventions.md`. Open `decisions.md` before re-opening a settled question.

## Files in `.ai/`

| File | What it answers | Refresh when |
| --- | --- | --- |
| `system.md` | What CALM is, crate boundaries, event and paint flow, native view model, plugin bridge | A crate's public surface or the paint flow changes |
| `conventions.md` | How work is done: worktrees, gates, tests, commits, agents, security rules | A rule is added or a ruling changes a rule |
| `decisions.md` | Settled decisions with rationale and cost if wrong | A decision is made or reversed |
| `status.md` | Done, pending, not wired, unverified | A task lands or a gap is found |

## Other documents

| Path | Purpose |
| --- | --- |
| `CLAUDE.md` | Session contract: rules that are easy to break, commands, bindings |
| `docs/dev-setup.md` | Prerequisites, run commands, Make targets |
| `docs/architecture.md` | Terminal hosting and preview mode in detail, with what was verified on real hardware |
| `docs/plugins.md` | Plugin manifest, `window.calm` API, wire format, GitHub access, TSX loop |
| `docs/release.md` | CI jobs, the self-hosted runner, cutting and recovering a release, the bundle |
| `docs/superpowers/specs/2026-10-31-calm-design.md` | The design spec. Binding authority when plan and code disagree |
| `docs/superpowers/plans/2026-10-31-calm-prototype.md` | The task plan the prototype follows |
| `packages/calm-plugin-sdk/calm.d.ts` | The plugin API as TypeScript types |
| `.superpowers/sdd/2026-10-31-calm-prototype/progress.md` | SDD ledger with every ruling (git-ignored, main checkout only) |

## Keeping this folder honest

- Every claim here is either verified against code or labelled as planned or unverified.
- Prefer updating a file over adding one. Add a file only for a new kind of knowledge.
- No secrets, tokens, Outline links or runner credentials anywhere in `.ai/`.
- Plain language, short sentences, no long dashes (see `conventions.md`).
