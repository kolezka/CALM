# CALM plugin API v2: ordered worker briefs

> Produced by the plugin API v2 orchestrator (a `route-fable` session, which cannot spawn
> workers, so it wrote briefs for the lead to dispatch). Moved into the repo on 2026-11-01.
>
> State on 2026-11-01, `origin/main` at `1f39de2`:
> - Phase 1 is done: Briefs A and C landed together as #35 (`feat/api-v2-p1-errors`).
> - Phase 6 core is done: Brief B landed as #34. The `Coalescer` is not wired.
> - Phase 2 (Brief D) is next. Its symbols exist on `main` as named below.
> - Open questions 1 to 5 live in `.ai/open-questions.md`. Progress per phase lives in
>   `.ai/status.md`, "Plugin API v2 progress".
>
> The text below is the original, with stale lines marked or fixed. Spec:
> `docs/superpowers/specs/2026-10-31-plugin-api-v2-design.md`.

Prepared 2026-10-31 by the architecture advisor (route-fable) for team-lead. Baseline
inspected: origin/main at 305fdd1 during analysis; at send time origin/main is 47c517c with
#23, #24, #25, #26 and #28 merged. Open: #22 (spec, 3c49aef4) and #29 (app icon).

UPDATE AT SEND TIME: because #23 and #28 are on main, the 1a/1b split below is no longer
needed. Dispatch Brief A and Brief C to ONE worker as a single phase 1 PR
(`feat/api-v2-p1-errors`), and Brief B to a second worker in parallel. The overlap analysis
stays as the record of why the order was chosen and as a warning for phase 2, which is still
a single-writer job on `bridge.rs`.

## Why briefs and not dispatch

My agent definition forbids spawning nested workers, so the lead's fallback applies: the
ordered briefs are below. Nothing was implemented.

## Finding that reorders phase 1 and 2

Both #23 and #28 edit `crates/calm-plugins/src/bridge.rs`, `crates/calm-plugins/src/lib.rs`,
`crates/calm-app/src/plugin_host.rs`, `crates/calm-app/src/canvas_view.rs` and
`docs/plugins.md`. #28 also edits `crates/calm-plugins/src/manifest.rs`, adds
`contributes.rs` and edits `packages/calm-plugin-sdk/calm.d.ts`. #23 adds 186 lines to
`plugin_host.rs` that construct `BridgeResponse::Err(id, String)` and 427 lines of
`plugin_services.rs`.

Phase 1 as written changes `BridgeResponse::Err` to carry `ApiError`, rewrites the prelude's
`resolve`, and adds manifest fields. Phase 2 rewrites `BRIDGE_METHODS`, the prelude and
`calm.d.ts`. Every one of those collides with #23 or #28, and the `Err` type change is a
semantic conflict: whichever lands second stops compiling. So phase 1 splits:

- 1a, pure types in a new module, no shared file touched except one `pub mod` line. Safe now.
- 1b, the wiring (response shape, prelude, manifest fields). After #23 and #28 are on main.
- Phase 2 after 1b.

Phase 6's `Coalescer` is pure and lives in a new module, so it can be built now in parallel
with 1a. That gives two disjoint workers today without waiting on the user's merges.

## Open questions and what they block

| # | Question | Blocks | Safe meanwhile |
| - | -------- | ------ | -------------- |
| 1 | `PanelInfo.joined` | phase 3 snapshot builder | phases 1a, 1b, 2, 6-core. Phase 2 can include the field in the Rust and d.ts type; dropping it later is a one-line removal. |
| 2 | `setAppearance` on every kind | phase 4 host checks | phases 1a, 1b, 2, 6-core. Parsing `panels.setAppearance(id, patch)` does not depend on which kinds accept it. |
| 3 | restart words | phase 14 manifest `backend.restart`, phase 1b manifest parsing of `backend` | parse `backend.restart` in 1b with the spec words `on-failure` and `never`; renaming later is a string change plus a test. Flag it in the PR body. |
| 4 | phase 11 earlier | nothing before phase 5 and 6 land | none of the briefs below. |
| 5 | Ghostty fork for phase 10 (added 2026-11-01) | phase 10 | everything except phase 10. |

Recommendation to the lead: get 1 and 2 answered before the phase 3 brief goes out; the
spec's recommendations (include `joined`, allow `setAppearance` on every kind) are the
cheaper default and both are reversible.

## Rules every brief carries (paste into each)

- Worktree: `git -C /Users/me/Development/CALM fetch origin` then
  `git -C /Users/me/Development/CALM worktree add .worktrees/<slug> -b <branch> origin/main`.
  Confirm with `git worktree list` before any edit. Never edit the main checkout.
- Gates, each exit code captured on its own line, no pipes. First
  `cargo clean -p calm-core -p calm-plugins -p calm-native -p calm-app` (stale rlibs), then
  `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`,
  `cargo test --workspace`, `cargo check --workspace --tests --examples`,
  `gmake plugins-check`, `CALM_HOME=$(mktemp -d) cargo run -p calm-app -- --smoke`.
  Known flaky smoke lines, all paint stalls in an occluded window: `preview_snapshots_in_3s=[0,0]`,
  `window_visible=Some(false)`, the stats pill and FPS frame steps, and a z-order mismatch
  until #40 lands. If one of those is the only failure, note it, run the same smoke on
  `origin/main` as a control, and stop; do not loop. A run with a flake is not rc=0.
- Test budget: one full `cargo test --workspace` and one smoke run per phase, at the end.
  While iterating use `cargo test -p <crate>`.
- Every served method ships with unit tests (success and each error code), `calm.d.ts`
  types, a `docs/plugins.md` entry, and a smoke fixture plugin call through the real bridge
  (`write_smoke_plugin` in `crates/calm-app/src/main.rs`), never by calling handlers
  directly. A declared but unserved method is documented as "not served yet", never as
  working.
- Hard rules: never re-enter GPUI from an AppKit callback (channel plus `cx.defer`);
  plugins are trusted code, never "sandboxed"; CALM's own secrets never reach plugin JS; a
  live terminal never closes without the user's confirmation; browser panels never get the
  bridge; customization first.
- Never drive the GUI beyond `--smoke`. Never write to the real `~/.config/calm`. Never
  push main, merge or tag.
- Update `.ai/status.md` for what landed and `.ai/decisions.md` only if a decision was made.
- Writing: plain, short, no en or em dashes, anywhere (code comments, docs, PR body).
- PR: `gh pr create --base main`. Body: the phase, the methods added, each gate's rc, a
  manual check for the user. Post the body with `gh api -F body=@file` if it contains
  backticks or fences. Report back: branch, head sha, PR number, gate rcs, anything skipped.
- Stop and report instead of guessing on: a product decision, a blocked permission, two
  failed fix attempts, or a conflict with an open PR's deviation.
- Do not spawn reviewers. The controller dispatches a `kolezka-bootstrap:route-gpt-sol`
  review per PR.

## Brief A: phase 1a, pure API types (done, landed with Brief C as #35)

Worker type: `kolezka-bootstrap:route-opus`. Branch `feat/api-v2-p1-types`, worktree
`.worktrees/api-v2-p1-types`. Budget: about 40 tool calls, one gate pass.

Read first: `CLAUDE.md`, `.ai/conventions.md`, and the spec from PR #22
(`git show origin/docs/api-v2-refresh:docs/superpowers/specs/2026-10-31-plugin-api-v2-design.md`),
sections "Errors", "Rust types", "Capabilities", "Versioning and feature detection".

Write scope: new file `crates/calm-plugins/src/api.rs`; one line `pub mod api;` in
`crates/calm-plugins/src/lib.rs` (expect a trivial merge with #23 and #28, which also add
module lines there; mention it in the PR body). Nothing else. Do not touch `bridge.rs`,
`manifest.rs`, `plugin_host.rs`, `calm.d.ts` or docs.

Deliver, all pure and tested:

- `pub const API_VERSION: u32 = 2;`
- `ErrorCode` enum with every row of the spec's error table, `serde(rename_all =
  "snake_case")`, `as_str()`, `Display`. Test: every variant round-trips through serde to
  the exact wire word in the table (table-driven, one assertion per word).
- `ApiError { code: ErrorCode, message: String }`, `Serialize` as `{ "code", "message" }`,
  `std::error::Error` via thiserror, constructors used later by the host:
  `invalid_params(field: &str, why: &str)`, `not_found(id: PanelId)`, `wrong_kind(id,
  expected: &str)`, `not_alive(id)`, `unknown_method(method)`, `not_implemented(method)`,
  `host_error(message)`. Message rules: `invalid_params` names the field; `not_found` names
  the id. Test the serialized JSON of two constructors.
- `Caller` enum per spec: `Panel { panel: PanelId, plugin_id: String }`, `Backend {
  plugin_id: String }`, `External { client: ClientId }` with `ClientId(pub u64)`. It derives
  `Debug, Clone, PartialEq` and deliberately not `Deserialize` (set by the transport, never
  parsed). Add `Caller::plugin_id(&self) -> Option<&str>`.
- `Capability` enum with every word of the spec's capability table (including `backend`),
  `parse(&str) -> Option<Capability>`, `as_str()`, `ALL: &[Capability]`, and
  `unknown_capabilities(words: &[String]) -> Vec<String>` that returns the words `parse`
  rejects, for the discovery warning in 1b. Tests: every table word parses and round-trips;
  a typo returns in `unknown_capabilities`; `ALL` has no duplicates.
- Leave out `check()`, `ApiRequest`, `PanelInfo`, `panel_info`, `camera_info`: they come in
  phase 2 and 3.

Acceptance: `cargo test -p calm-plugins` green with the new tests; gates green; no served
method changed, so no d.ts, docs or smoke fixture change is required (smoke still runs).

## Brief B: phase 6 core, the Coalescer (done, #34)

Worker type: `kolezka-bootstrap:route-opus`. Branch `feat/api-v2-p6-coalescer`, worktree
`.worktrees/api-v2-p6-coalescer`. Budget: about 50 tool calls, one gate pass.

Read first: spec sections "Events", "Coalescing model", "Performance budget".

Write scope: new file `crates/calm-plugins/src/events.rs`; one line `pub mod events;` in
`crates/calm-plugins/src/lib.rs` (same trivial-merge note as Brief A). If a JS literal
escaper is needed for the batch script, add a private copy in `events.rs` rather than
touching `bridge.rs`; phase 2 will unify them.

Deliver, pure, `std::time::Instant` passed in, no timers, no GPUI:

- `pub struct Coalescer` holding one subscribed page's pending events. The host will own
  one per page (that is how "one eval_js per page per flush" and a per-page `throttleMs`
  fall out). Constructor takes the default window (`Duration`, spec default 100 ms).
- `set_window(event: &str, window: Duration)` clamped to 16 to 5000 ms; discrete events
  ignore it.
- `push(event: &str, panel: Option<PanelId>, payload: Value, kind: EventKind, now:
  Instant)` with `EventKind::{Discrete, Coalesced}`. Key is `(event, panel)`. Coalesced
  keys keep only the latest payload. Discrete events are never merged and keep arrival
  order relative to each other.
- `due(now: Instant) -> Vec<(String, Value)>`: a key flushes when its window has passed
  since its last send; a trailing flush runs once the stream goes quiet so the final
  payload always arrives. A `panelChanged` for a panel is dropped if a `panelRemoved` for
  that panel is pending.
- `next_deadline(&self) -> Option<Instant>` so the host can run its 100 ms timer only while
  keys are pending, and `is_empty()`.
- `pub fn emit_batch_js(batch: &[(String, Value)]) -> String` rendering
  `window.__calm.emitBatch([[event, payload], ...])` with U+2028 and U+2029 escaped like
  `bridge.rs::js_literal` does.
- A `pub fn classify(event: &str) -> EventKind` with the spec table's discrete set
  (`panelAdded`, `panelRemoved`, `terminalExited`, `tuiExited`, `command`,
  `terminalCommandFinished`, `browserNavigation`, `browserMessage`, `layoutSaved`,
  `layoutOpened`) and everything else coalesced.

Tests, each driving time by hand: latest wins within a window; trailing flush after quiet;
a removal drops the pending change for that panel and keeps others; discrete ordering
survives interleaved coalesced pushes; `set_window` clamps; a 16 ms window yields at most
one flush per 16 ms over a simulated 1 s stream (counts the perf-budget claim without a
clock); `emit_batch_js` escapes U+2028; `next_deadline` is `None` when empty.

Acceptance: `cargo test -p calm-plugins` green; gates green; nothing served changes. Do not
wire it into the host; that is phase 6 wiring after phase 2.

## Brief C: phase 1b, wiring (done, landed with Brief A as #35)

Worker type: `kolezka-bootstrap:route-opus`. Branch `feat/api-v2-p1-wiring`, worktree
`.worktrees/api-v2-p1-wiring` from origin/main after the merges (verify with
`git log --oneline origin/main | head` that #23 and #28 merge commits are present). Budget:
about 90 tool calls, one gate pass.

Read first: spec sections "Errors", "Manifest additions", "Capabilities", "Versioning and
feature detection"; the merged `docs/plugins.md` and `.ai/decisions.md` for #28's
documented deviations from v2 (do not undo them; if one contradicts this brief, stop and
report).

Write scope: `crates/calm-plugins/src/{bridge.rs,manifest.rs,api.rs,lib.rs}`,
`crates/calm-app/src/{plugin_host.rs,plugin_services.rs,canvas_view.rs,main.rs}` (error
construction sites and the smoke fixture only), `packages/calm-plugin-sdk/calm.d.ts`,
`docs/plugins.md`, `.ai/status.md`.

Deliver:

- `BridgeResponse::Err(u64, ApiError)`. `to_js` renders the object. `not_implemented` and
  the unknown-method path use `ErrorCode::NotImplemented` and `ErrorCode::UnknownMethod`.
  Convert every existing `Err(id, String)` site in `plugin_host.rs` and
  `plugin_services.rs` to a code: `invalid_params` for bad params, `gh_not_installed`,
  `gh_not_logged_in`, `gh_failed`, `gh_invalid_path` for the gh client, `storage_corrupt`
  and `too_large` for storage, `host_error` for the rest. `rg 'BridgeResponse::Err\('`
  must show no string construction left.
- Prelude `resolve(id, ok, value)`: an object with `code` becomes an `Error` with
  `message` and a `code` property; a plain string (v1 shape) becomes `code: "host_error"`.
  Test through `run_prelude_under_bun` (both shapes).
- `window.__calmHost = { apiVersion, methods }` injected as a document start script before
  the prelude (find the prelude injection in `PluginPanel::new`, `plugin_host.rs`), and a
  synchronous `calm.has(method)` in the prelude that reads it. `methods` is the served
  list, which the host owns: add `pub fn served_methods() -> &'static [&'static str]` in
  `plugin_host.rs` and a test that every served method is in `BRIDGE_METHODS`.
- Manifest fields on `PluginManifest` (web kind): `api_version: u32` (default 1; above
  `API_VERSION` skips the plugin with `requires API v<n>, host has v<m>`),
  `capabilities: Vec<String>` (unknown words produce a warning that discovery surfaces
  without skipping; find how `Discovery` reports errors today and add a parallel
  `warnings` list that the status bar pill counts the same way, or report that it cannot
  without a GPUI change), `main: Option<PathBuf>` (relative, inside the plugin folder,
  checked on disk like `entry`), `backend: { start: OnDemand | AtLaunch, restart:
  OnFailure | Never }` with defaults (the words are open question 3; use the spec's words
  and say so in the PR body), `engines: { bun: Option<String> }` (a semver range string;
  validate the syntax only, no semver crate yet unless one is already in the workspace),
  `background: bool` default false. `contributes` requires `apiVersion >= 2` per spec;
  #28 may have decided otherwise: follow #28's decision and report the difference. A TUI
  manifest rejects `main`, `contributes` and `background` with the existing per-field
  error style. Tests: each field good and bad, default values, the version skip message.
- `calm.d.ts`: `ErrorCode`, `CalmError`, `has`, the manifest additions. `docs/plugins.md`:
  the error table, `calm.has`, the new manifest fields (declared, shown to the user, never a
  gate; "runs a background program with full access to your files, network and programs"
  note for `main`).
- Smoke: extend the fixture page in `write_smoke_plugin` (or add a sibling fixture) to
  call an unknown method and a known unserved method, and to read `calm.has("panel.setTitle")`
  and `calm.has("nope")`; it reports through `calm.log` and the smoke prints one line
  `smoke: plugin errors unknown=unknown_method unserved=not_implemented has=[true,false]`
  and fails on any miss.

Acceptance: all gates green; the regression test
`every_other_method_rejects_instead_of_hanging` updated to the new `Err` shape; no
`BridgeResponse::Err(_, String)` left; `.ai/status.md` updated.

## Brief D: phase 2, request parsing and the full contract (next; Brief C merged as #35)

Worker type: `kolezka-bootstrap:route-opus`. Branch `feat/api-v2-p2-requests`, worktree
`.worktrees/api-v2-p2-requests`. Budget: about 120 tool calls, one gate pass. This is the
single-writer bottleneck for `bridge.rs`; run nothing else that touches it in parallel.

Read first: spec sections "Namespaces and methods" (all tables), "Types", "Events" (the
`events.subscribe` and `events.unsubscribe` wire methods and `throttleMs`), "Rust types".

Write scope: `crates/calm-plugins/src/{api.rs,bridge.rs}`, `packages/calm-plugin-sdk/calm.d.ts`,
`crates/calm-app/src/plugin_host.rs` (dispatch signature only), `docs/plugins.md` (a
"declared, not served yet" list), `.ai/status.md`.

Deliver:

- Rust wire types in `api.rs` with `serde(rename_all = "camelCase")`: `PanelKind`,
  `PanelMode`, `Side`, `ResizeInfo`, `JoinedNeighbor`, `PanelAppearance`,
  `TerminalColors`, `TuiInfo`, `PanelInfo` (include `joined` per the spec's recommendation;
  one field to drop if the user says no), `Placement`, `PanelSpec` (tagged on `kind`, every
  variant takes `appearance`), `PanelFilter`, `CameraInfo`, `CameraPatch`, `AppInfo`,
  `TerminalInfo`, `BrowserInfo`, `SettingsPatch` (a `Value`), the `*Op` enums the
  `ApiRequest` variants carry.
- `ApiRequest` enum per spec and `ApiRequest::parse(method, &Value) -> Result<ApiRequest,
  ApiError>` for every method in the spec's tables plus the v1 aliases
  (`canvas.createPanel`, `canvas.focusPanel`, `canvas.listPanels`). Unknown method:
  `unknown_method`. Bad or missing param: `invalid_params` naming the field. Range rules
  that are pure go here: `Placement` with both `rect` and `near` rejects; `gap` 0 to 256;
  terminal `env` keys `[A-Z_][A-Z0-9_]*` and values without NUL; `throttleMs` 16 to 5000;
  colors `#rrggbb`; icon 1 to 4 chars or a `.png`/`.svg` path (existence is checked by the
  host later). `ApiRequest::capability()` per the capability table.
- `BRIDGE_METHODS` grows to the full v2 list and stays the single source of truth. The
  prelude gains every namespace and method (`app`, `panel.*` additions, `panels`,
  `camera`, `terminals`, `browsers`, `plugins`, `storage.keys`, `bus`, `ui`, `layouts`,
  `session`, `events.on` with `{ throttleMs }` that sends `events.subscribe` on the first
  handler and `events.unsubscribe` after the last, `__calm.emitBatch`). Keep #28's
  command queue behavior. The existing conformance tests (`prelude_exposes_every_bridge_method`,
  `dts_declares_every_bridge_method`, `bridge_methods_match_contract`) must stay green
  without weakening; extend `declared_calm_functions` if nested namespaces need it.
- `plugin_host::dispatch` takes `(caller: &Caller, request: ApiRequest)` and serves
  exactly what it serves today plus nothing; everything else returns
  `ApiError::not_implemented(method)`. `canvas_view::plugin_message` builds
  `Caller::Panel` from the panel it already has.
- `calm.d.ts`: every type from the spec's Types section and every method, with a header
  comment `apiVersion: 2` that a test reads; unserved methods get a JSDoc line
  "not served yet, rejects with not_implemented".
- Tests: table-driven, one good and one bad param per method (the spec asks for this
  explicitly); every v1 alias maps to the v2 variant; `capability()` matches the table.
- Docs: `docs/plugins.md` gets the served list and a separate "declared, not served yet"
  list. Nothing new is documented as working.
- Smoke: the fixture calls one newly declared method and asserts `err.code ===
  "not_implemented"`; extend the Brief C smoke line rather than adding another.

Acceptance: gates green; `BRIDGE_METHODS`, prelude and d.ts in sync by the conformance
tests; `rg 'not implemented:'` finds no string-shaped reject.

## After D: phases 3, 4, 5, 6 wiring (briefs to write once questions 1 and 2 are answered)

- Phase 3, read-only host: `app.info`, `app.getSettings`, `theme.get`, `panels.list`,
  `panels.get`, `panel.info`, `panel.getBounds`, `camera.get`, `browsers.info`,
  `terminals.info`, `plugins.list`; `panel_info(scene, joins, id, extras)` and
  `camera_info` pure in `api.rs`. Needs question 1 settled. Coordinate `PanelExtras`
  (mode, focus, url, appearance, tui) with persist-orch, because the persistence plan's
  per-panel `overrides` is the same data at rest.
- Phase 4, control: split into two PRs to keep a single writer per file: 4a `panels.*`,
  `panel.*`, `camera.*`, `placement::place_near` in `calm-core` (new module, test per
  side), `ResizePolicy::clamp` on every size; 4b `app.setSettings` (merge patch, unknown
  key check, `confirm_close` and `version` refusal, image path check, coalesced save) and
  `ui.notify`. Needs question 2 settled for `setAppearance` on non-plugin kinds.
- Phase 5, close: `PendingReply`, `request_close`, settle from Enter and Escape, `busy`;
  regression test that a live terminal is not removed before Enter.
- Phase 6 wiring: one `Coalescer` per subscribed page in the view, `due` after paint and
  from a 100 ms timer only while pending, all v1 and v2 senders, the smoke counting
  batches in its window.

Review per PR: `kolezka-bootstrap:route-gpt-sol`, dispatched by the controller, given the
diff and the brief, not the implementer's summary.

## Phase 11 entry (external control channel), for the dispatcher

Depends on: phase 1 (`Caller::External`), phase 2 (dispatcher takes `Caller`), phases 5 and
6 per the spec's order. If the user moves it earlier (Q4), a reduced phase 11 can ship after
phase 2 and before 5: socket plus token lifecycle, terminal env, allowlist, `panel.setStatus`
and the chrome dot; `events.poll` follows phase 6.

Acceptance items team-lead asked for, to be listed in the PR body and in the report:
- terminal env `CALM_SOCKET`, `CALM_TOKEN_FILE` (a path, never the token) and
  `CALM_PANEL_ID` on every terminal CALM spawns while the channel is enabled;
- `panel.setStatus({ panelId?, state, text?, detail?, color? })` with state
  `idle|running|waiting|error|done`, `text` at most 80 chars, `detail` at most 1 KiB,
  `null` clears; drawn in the chrome strip only, never over a native rect;
- `PanelInfo.status` and the `panelStatusChanged { panelId, status }` event;
- per-client queue with long polling (`events.poll`), allowlist with the `run` bundle
  refused by default, `not_allowed` for the rest;
- status is never saved in layouts.

Report line when the PR is open: `phase 11 ready: #N <sha>`, plus one sentence saying
whether `panel.setStatus` and the terminal env are in it. The user waits on it to start the
Claude Code mods work (`calm-bridge`).

## Items for the lead that are not briefs

- Phase 10 (Ghostty fork via `[patch.crates-io]`) needs the user's go before any worker
  touches `Cargo.toml`.
- Phase 11 after 5 and 6; question 4 decides whether it jumps ahead of 7 to 9.
- Phase 16 contract with the persistence lane, agreed by both lanes and recorded in
  `.ai/decisions.md`, "Panel overrides contract". Short form: the persistence plan owns the layout file format and the
  `overrides` struct in `calm-core`; `calm-plugins::api` owns the wire types; phase 16 maps
  one to the other and adds nothing to the file format on its own.
