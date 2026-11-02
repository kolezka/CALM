# Claude Code Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Claude Code sessions running in CALM terminals report their state and their teammates
to the canvas, and Claude can open and arrange CALM panels, through one control socket, a
`calm-mcp` server and a `calm` plugin for Claude Code (mod, skill, MCP config).

**Architecture:** CALM serves the existing plugin bridge dispatcher on a local Unix socket
(HTTP/1.1 + JSON, token, per-method allowlist). Every CALM terminal gets `CALM_SOCKET`,
`CALM_PANEL_ID` and `CALM_TOKEN_FILE`. Three clients sit on the socket: the `calm-mcp` stdio
server (tools for any MCP client), the mod inside the Claude Code plugin (events: status,
teammates, `/calm` commands), and a built-in `agent-board` web plugin that draws teammate cards.
Teammates stay in Claude Code's default in-process mode. Real per-teammate terminals through an
`it2` stand-in are an optional last phase with its own plan, decided after phase D is in use.

**Tech Stack:** Rust 2024 (`calm-core`, `calm-app`, new `calm-mcp`), std `UnixListener`, serde
JSON, Claude Code plugin with a TypeScript mod, a plain HTML first-party page (like
`plugins/calm-keys`) tested under bun.

**Spec:** `docs/superpowers/specs/2026-10-31-claude-code-mods-integration.md` (research, PoC,
security section) and `.ai/decisions.md` entries "External control socket approved" and
"Launching terminal's env is scrubbed". Binding product spec:
`docs/superpowers/specs/2026-10-31-calm-design.md`.

**Branches:** one per phase, each from `origin/main` in `.worktrees/<branch>`:
`feat/control-socket` (A), `feat/calm-mcp` (B), `feat/claude-code-plugin` (C),
`feat/agent-board` (D), `feat/claude-code-tui` (E). Each phase is one PR with a cross-family
review before merge. Written against `origin/main` at `3a6546a`; PR #69 (env scrub) is merged
as `18f72b6`, so phase A has no open dependency. Line numbers below are for `3a6546a`; find
code by name if they moved.

## Global Constraints

- Unix socket only, never TCP. Directory `$CALM_HOME/run` (default `~/.config/calm/run`) mode
  0700, socket `calm.sock` 0600, token file `token` 0600, created under umask 0177. Refuse to
  start the server if the directory is a symlink or not owned by the current uid; log and run
  without the socket, never exit.
- macOS `sun_path` limit: the socket path must be at most 103 bytes; longer means the server
  stays off with one logged line naming the path and the limit.
- Token: 32 random bytes, hex, in `run/token`, replaced on each launch and deleted on quit.
  Requests carry `Authorization: Bearer <token>`, compared in constant time, checked before
  routing. A wrong or missing token gets 401 and no body detail. Terminals get the token file
  path, never the token.
- Access classes, checked before `plugin_host::route` (storage services bypass `dispatch`):
  - **Read** (always allowed): `canvas.listPanels`, `panel.getBounds`, `theme.get`,
    `panel.setStatus`, `events.poll`, `events.publish`.
  - **Open** (`control.allow_open`, default false): `canvas.focusPanel`, and
    `canvas.createPanel` only for kind `browser`, or kind `terminal` without `command`.
  - **Exec** (`control.allow_exec`, default false), one unit: `canvas.createPanel` with a
    `command`, with kind `tui` (runs the manifest command) or kind `plugin` (starts plugin
    code), `process.exec`, and any terminal input. Never allow one without the others.
  - **Plugin-only** (refused externally): every other `BRIDGE_METHODS` entry, including
    `github.api`, `storage.*`, `settings.*`, `openExternal`, `panel.setTitle`,
    `panel.minimize`, `panel.close`, `keys.list`, `log`.
- Wire format of `POST /v1/call`: request `{ "id": u64, "method": string, "params": any }`;
  answer `{ "id": u64, "ok": true, "result": any }` or
  `{ "id": u64, "ok": false, "error": { "code": string, "message": string } }` (the existing
  `ApiError` serde shape). Produced by `BridgeResponse::to_json` (Task A2); `calm-mcp` and the
  mod parse this shape and nothing else.
- Deadlines: an ordinary call gets 503 when the UI has not answered within 5 s;
  `events.poll` gets its own `timeoutMs + 5 s`. Request reading times out after 5 s.
- Request body at most 65,536 bytes (413 above). Status text at most 120 chars after trimming,
  control characters stripped, rendered as plain text. Event payloads at most 16,384 bytes of
  JSON.
- Settings are validated strictly, with these defaults: `control.enabled = true`,
  `control.allow_open = false`, `control.allow_exec = false`,
  `agents.board = "auto"` (`"auto" | "off"`), `agents.board_side = "right"` (`"right" | "below"`).
- Claude Code floor for the mod: 2.1.287 (tested on 2.1.316). The MCP server and skill must
  work with the mod disabled (`disableAllHooks`, `--safe-mode`).
- Outside CALM (`CALM_SOCKET` unset) the mod does nothing and the MCP tools answer
  "not running in a CALM terminal". Neither prints errors on startup.
- Plugins are trusted, explicitly installed code. Never call them sandboxed.
- Nothing GPUI draws overlaps a live native view; the status dot lives in panel chrome.
- No credentials in events, status, logs, layouts or tests. The token never enters a log line.
- All `unsafe` stays in `calm-native` with a `// SAFETY:` comment. Comments short and plain,
  no em or en dashes.
- Gates per PR: `cargo clean -p` the touched crates, `cargo check --workspace --tests
  --examples`, `gmake ci` (lint, test, smoke, plugins-check), smoke must print every new line.

## Review Focus

1. **Stale socket after a crash.** `calm.sock` exists but nothing listens. Start must connect
   first: a refused connect means remove and bind; a live answer means another CALM owns it, so
   this one runs without a socket and logs it. Test in Task A2.
2. **Two CALM processes, same `CALM_HOME`.** The second must not steal the first one's socket or
   rewrite its token. Same test as above, with a live listener.
3. **`CALM_PANEL_ID` names a panel that is gone.** A Claude session that outlives its panel id
   (closed, or restored under a new id) gets `not_found`, never acts on another panel. Restore
   must give a restored terminal its new id in the env. Tests in Tasks A3 and A4.
4. **Hostile text from the model.** 1 MB body, status with ANSI escapes and newlines, event
   payload with `</script>`. Expect 413, sanitized 120-char status, payload delivered to the
   board only through `BridgeEvent::to_js` escaping. Tests in Tasks A2, A5, D2.
5. **`allow_open` alone must not run anything.** A client with only `allow_open` asks for a
   `tui` or `plugin` panel. Expect 403. Tests `access_matrix` (A1) and
   `tui_create_is_403_with_only_allow_open` (A2).
6. **Many teammates, slow reader.** 20 teammates publishing every 2 s while the board is closed.
   The event backlog stays bounded (512 events) and the next poll reports `gap: true`. Test in
   Task A6.

---

## Phase A: control socket (`feat/control-socket`)

### Task A1: control model (`calm-core`)

**Files:**
- Create: `crates/calm-core/src/control.rs` (register in `lib.rs`)
- Modify: `crates/calm-core/src/settings.rs` (new `control: ControlSettings` and
  `agents: AgentSettings` fields on `Settings`, next to `keybindings`, serde default for files
  without them)
- Modify: `crates/calm-core/src/settings_form.rs` (a new `section("Integrations", ...)` pushed
  in `settings_form()`, like the existing "Background" and "Keys" sections)

**Interfaces:**
- Produces:
  - `pub const SOCKET_PATH_MAX: usize = 103;`
  - `pub fn run_dir(calm_home: &Path) -> PathBuf` (`<calm_home>/run`)
  - `pub fn socket_path(calm_home: &Path) -> Result<PathBuf, ControlError>` (`TooLong { path, len }`)
  - `pub enum Access { Read, Open, Exec, PluginOnly }`
  - `pub fn method_access(method: &str, params: &serde_json::Value) -> Access`, exactly the
    classes in Global Constraints. Unknown methods are `PluginOnly`.
  - `pub fn allowed(access: Access, settings: &ControlSettings) -> bool`
  - `pub fn token_matches(given: &[u8], expected: &[u8]) -> bool` (constant time over
    `expected.len()`, false on length mismatch)
  - `pub fn sanitize_status(text: &str) -> String`
  - `pub struct ControlSettings { enabled: bool, allow_open: bool, allow_exec: bool }`
  - `pub enum Side { Right, Below }` (serde `"right"`, `"below"`) and
    `pub enum BoardMode { Auto, Off }`
  - `pub struct AgentColors { working: Rgba, waiting: Rgba, idle: Rgba, error: Rgba }`
    (defaults `#4f8cff`, `#ffb020`, `#7a7a7a`, `#ff4d4f`, parsed like other color settings)
  - `pub struct AgentSettings { board: BoardMode, board_side: Side, colors: AgentColors }`

- [ ] **Step 1: Write failing tests** in `control.rs`:
  `socket_path_rejects_104_bytes` (a home making the path exactly 104 bytes gives `TooLong`,
  103 gives `Ok`), `access_matrix` (table test: each of the 18 `BRIDGE_METHODS` on `3a6546a` (including `keys.list`) plus
  `panel.setStatus`, `events.poll`, `events.publish`, and `canvas.createPanel` with kinds
  `browser`, `terminal`, `terminal`+`command`, `tui`, `plugin`, each against the four
  combinations of `allow_open`/`allow_exec`; expected values are the Global Constraints
  classes), `tui_and_plugin_creates_need_allow_exec`, `token_match_rejects_prefix_and_length`,
  `exec_needs_allow_exec_even_with_allow_open`,
  `status_is_trimmed_stripped_and_capped` (input `"\x1b[31mhi\nthere" + 200 × "x"` gives
  `"hi there" + ...` of 120 chars, no `\x1b`, no `\n`), and in `settings.rs`
  `control_defaults_are_closed` plus `unknown_board_mode_is_rejected`.
- [ ] **Step 2:** `cargo test -p calm-core control` fails (module missing).
- [ ] **Step 3:** Implement the items above. Newlines and tabs become one space; other control
  characters are dropped; the cap counts chars, not bytes.
- [ ] **Step 4:** `cargo test -p calm-core` passes.
- [ ] **Step 5:** Commit `feat(core): control socket model and settings`.

### Task A2: socket server (`calm-app`)

**Files:**
- Create: `crates/calm-app/src/control_server.rs`
- Create: `crates/calm-app/src/control_http.rs` (bounded HTTP/1.1 parse and write, no new crate)
- Modify: `crates/calm-plugins/src/bridge.rs` (`BridgeResponse::to_json`)
- Modify: `crates/calm-app/src/main.rs` (start after settings load, stop on quit)

**Interfaces:**
- Consumes: Task A1 (`socket_path`, `method_access`, `allowed`, `token_matches`).
- Produces:
  - `impl BridgeResponse { pub fn to_json(&self) -> serde_json::Value }` with the wire shape
    in Global Constraints.
  - `pub struct ControlServer` with `pub fn start(calm_home: &Path, settings: ControlSettings, tx: async_channel::Sender<ControlCall>) -> Result<ControlServer, String>` and `Drop` that removes the socket and token.
  - `pub struct ControlCall { pub panel: PanelId, pub request: BridgeRequest, pub reply: async_channel::Sender<BridgeResponse> }`, the reply channel made with
    `async_channel::bounded(1)` (`async-channel` is already a `calm-app` dependency).
  - `pub fn token_file(calm_home: &Path) -> PathBuf`
  - HTTP: `POST /v1/call`, headers `Authorization: Bearer`, `X-Calm-Panel: <u64>`, body
    `{ "id", "method", "params" }`; answer 200 with `BridgeResponse::to_json`. 400 bad JSON or
    header, 401 token, 403 not allowed (body
    `{ "id", "ok": false, "error": { "code": "not_allowed", "message": "<method> needs control.allow_open|allow_exec" } }`),
    404 path, 413 size, 503 per the deadlines in Global Constraints.

- [ ] **Step 1: Write failing tests** (`#[cfg(test)]`, temp `CALM_HOME`, a test thread that
  answers `ControlCall`s): `response_json_round_trip` (in `bridge.rs`: one `Ok` and one `Err`
  encode to the exact wire shape), `refuses_wrong_token_with_401`,
  `refuses_missing_token_with_401`, `rejects_body_over_64k_with_413`,
  `exec_method_is_403_by_default`, `tui_create_is_403_with_only_allow_open`,
  `stale_socket_is_replaced` (bind, drop listener without unlink, start again: ok),
  `live_socket_is_not_stolen` (second start returns `Err`, first still answers, token file
  bytes unchanged), `files_are_private` (dir 0700, socket and token 0600),
  `symlinked_run_dir_is_refused`, `ordinary_call_times_out_with_503` (test thread never
  answers), `poll_outlives_the_ordinary_deadline` (a `timeoutMs: 8000` poll answered at 7 s
  gets 200, not 503).
- [ ] **Step 2:** `cargo test -p calm-app control_` fails.
- [ ] **Step 3:** Implement. One accept thread, one short-lived thread per connection,
  `Connection: close`, umask 0177 around bind. Liveness check is a connect.
- [ ] **Step 4:** Tests pass.
- [ ] **Step 5:** Commit `feat(app): serve the bridge on a private Unix socket`.

### Task A3: route socket calls through the bridge (`calm-app`)

**Files:**
- Modify: `crates/calm-app/src/canvas_view.rs` (drain `ControlCall`s near the plugin call site
  at `plugin_host::route`, about line 1730 on `3a6546a`). The access check from A1
  runs before `route`, because `route` sends storage calls to background services first.
- Modify: `crates/calm-app/src/plugin_host.rs` (`Origin` on `HostContext`)

**Interfaces:**
- Produces: `pub enum Origin { Plugin, External }` and field `pub origin: Origin` on
  `HostContext`, next to the `keybindings` field added by #62. `dispatch` answers `settings.*` and `storage.*` with `wrong_kind` for
  `External`. The external caller is the panel from `X-Calm-Panel`; any panel kind may call.

- [ ] **Step 1: Failing tests** in `plugin_host.rs`: `external_list_panels_from_terminal`,
  `external_unknown_panel_is_not_found` (closed id 999), `external_settings_get_is_refused`.
- [ ] **Steps 2 to 4:** Implement, run `cargo test -p calm-app plugin_host`, pass.
- [ ] **Step 5:** Commit `feat(app): answer socket calls on the UI thread`.

### Task A4: socket env in every terminal

**Files:**
- Modify: `crates/calm-app/src/terminal_profiles.rs` (`options` gains `panel: PanelId`) and
  `crates/calm-app/src/tui_host.rs` (env passed to `LaunchSpec`)
- Modify: `crates/calm-app/src/main.rs` (smoke step `control_socket`, registered in
  `smoke_runner.rs` `STEPS`)

**Interfaces:**
- Produces: env `CALM_SOCKET`, `CALM_PANEL_ID` (decimal), `CALM_TOKEN_FILE`, `CALM_MCP_BIN`
  (absolute path of `calm-mcp` in the same directory as the running `calm-app`: the bundle's
  `Contents/MacOS`, or `target/<profile>` in a dev build; omitted when that file is missing).
  Merged before profile env, so a profile can override them. Missing socket means the vars
  are not set. Never the token itself.

- [ ] **Step 1: Failing tests** `launch_env_names_the_panel` in `terminal_profiles.rs`,
  `app_launch_env_names_the_panel` in `tui_host.rs`, and a restore test
  `restored_terminal_gets_its_new_panel_id`.
- [ ] **Steps 2 to 4:** Implement; `cargo test -p calm-app` passes.
- [ ] **Step 5:** Smoke step `control_socket`, proving the env reaches real children:
  open a typed-command terminal running `/bin/sh -c 'env > "$1"' sh <smoke dir>/env-cmd.txt`
  (a quick command; since #76 the status wrapper keeps it past Ghostty's 250 ms
  abnormal-exit check),
  and an app from a smoke-only TUI manifest in the smoke plugin dir with the same command
  writing `env-app.txt`. Wait for both files, decode them, and require in each
  `CALM_PANEL_ID` equal to that panel's id, `CALM_SOCKET` and `CALM_TOKEN_FILE` present, and
  the token's value absent. Then, using the decoded `CALM_SOCKET` and the token file,
  POST `canvas.listPanels` with that `CALM_PANEL_ID` (require 200 and the panel listed) and
  once with a bad token (require 401). Print
  `smoke: control_socket env_cmd=ok env_app=ok status=200 bad_token=401`.
  `cargo run -p calm-app -- --smoke; echo rc=$?` exits 0.
- [ ] **Step 6:** Commit `feat(app): give terminals the control socket env`.

### Task A5: `panel.setStatus` and the chrome dot

**Files:**
- Modify: `crates/calm-core/src/scene.rs` (`status: Option<PanelStatus>`, `#[serde(skip)]`)
- Modify: `crates/calm-plugins/src/bridge.rs` (`BRIDGE_METHODS`, prelude),
  `packages/calm-plugin-sdk/calm.d.ts`, `crates/calm-app/src/plugin_host.rs`
- Modify: chrome paint in `crates/calm-app/src/canvas_view.rs`

**Interfaces:**
- Produces: `pub enum AgentState { Working, Waiting, Idle, Error }`,
  `pub struct PanelStatus { pub state: AgentState, pub text: String, pub at_ms: u64 }`,
  method `panel.setStatus { state: "working"|"waiting"|"idle"|"error"|"clear", text?: string, panelId?: u64 }`
  (`panelId` defaults to the caller). `canvas.listPanels` items gain `status`.
  Dot colors come from `AgentSettings.colors` (Task A1).

- [ ] **Step 1: Failing tests:** `set_status_sanitizes_text`, `clear_removes_status`,
  `status_is_not_saved_in_layouts` (round-trip a scene, status gone), and the existing
  `dts_declares_every_bridge_method` in `bridge.rs` fails until `calm.d.ts` declares
  `panel.setStatus`.
- [ ] **Steps 2 to 4:** Implement. The dot and text sit in the title bar, so they never overlap the
  native view. Text is cut with an ellipsis to the bar width.
- [ ] **Step 5:** Smoke: set `waiting` on the smoke terminal over the socket, print
  `smoke: panel_status state=waiting`.
- [ ] **Step 6:** Commit `feat: agent status dot on panels`.

### Task A6: `events.poll` and `events.publish`

**Files:**
- Create: `crates/calm-core/src/event_log.rs`
- Modify: `crates/calm-app/src/plugin_host.rs`, `crates/calm-plugins/src/bridge.rs`,
  `packages/calm-plugin-sdk/calm.d.ts`

**Interfaces:**
- Produces: `pub struct EventLog` with `new(capacity: usize)` (512 in the app),
  `push(topic: &str, payload: Value) -> u64` (sequence), and
  `since(cursor: u64, topics: &[String]) -> Polled { events: Vec<LoggedEvent>, next: u64, gap: bool }`.
  Methods: `events.publish { topic, payload }` (topic `[a-z0-9.]{1,64}`, payload at most 16 KiB),
  `events.poll { since, topics?, timeoutMs <= 25000 }` held without blocking the UI thread
  (answer when an event arrives or the timeout fires; `timeoutMs: 0` answers at once, which is
  how a page replays the backlog). CALM itself pushes `panelsChanged` and
  `panelStatusChanged`. Plugin pages get three new host event names, added to the prelude's
  `EVENTS` list and to `calm.d.ts`: `panelsChanged`, `panelStatusChanged`, and `external`
  with payload `{ topic, payload, seq }` for every `events.publish`. Pages filter by `topic`
  themselves; `calm.events.on("agent")` stays an error.

- [ ] **Step 1: Failing tests** in `event_log.rs`: `poll_returns_events_after_cursor`,
  `overflow_reports_gap` (capacity 4, push 6, poll from 0: 4 events, `gap: true`),
  `topic_filter`, `zero_timeout_answers_at_once`; in `plugin_host.rs`
  `publish_rejects_large_payload` and `publish_emits_external_to_plugin_pages`; in
  `bridge.rs` the prelude test `events_on_accepts_external` and the existing
  `dts_declares_every_bridge_method` for `events.poll` and `events.publish`.
- [ ] **Steps 2 to 4:** Implement; a held poll is a stored waiter answered from the UI thread
  when an event is pushed, or by a background-executor timer at `timeoutMs`. The UI thread
  never sleeps.
- [ ] **Step 5:** Update `.ai/status.md` (socket, env, status, events now wired), the
  "Not wired yet" line in `CLAUDE.md` (it says "all 18 bridge methods" and "plugin events
  other than `command`"; give the new count and drop the events claim), and
  `docs/plugins.md` (new methods, allowlist, settings). Commit `feat: event log over the socket`.

---

## Phase B: `calm-mcp` (`feat/calm-mcp`)

### Task B1: placement beside a panel (`calm-core`)

**Files:** Create `crates/calm-core/src/placement.rs` (register in `lib.rs`); modify
`crates/calm-app/src/plugin_host.rs` `create_panel` and `PanelSpec` in
`packages/calm-plugin-sdk/calm.d.ts`.

**Interfaces:**
- Produces: `pub fn place_beside(anchor: Rect, size: Size, side: Side, gap: f64, others: &[Rect]) -> Rect`.
  First free spot on `side` at `gap`, stepping along the same side, then the other side, never
  overlapping `others`. Every caller passes `gap = JOIN_SNAP_GAP` and `others` = the rects of
  all other panels in the scene. `canvas.createPanel` spec gains
  `near?: u64` and `side?: "right"|"below"` (the spec's `{ near: PanelId, side }`); `side`
  defaults to `agents.board_side`. An unknown `near` is `not_found`.

- [ ] Tests: `beside_right_of_anchor`, `skips_occupied_slot`, `falls_back_to_below`, and in
  `plugin_host.rs` `create_near_unknown_panel_is_not_found`.
  Implement, `cargo test -p calm-core placement`, commit `feat(core): place a panel beside another`.

### Task B2: the MCP server crate

**Files:**
- Create: `crates/calm-mcp/` (`Cargo.toml`, `src/main.rs`, `src/protocol.rs`,
  `src/client.rs`, `src/tools.rs`); add to the workspace. No dependency on other CALM crates:
  it parses the wire format from Global Constraints with `serde_json`.
- Modify: `scripts/bundle-macos.sh`: its line `(cd "$repo_root" && cargo build --release -p calm-app)`
  becomes `cargo build --release -p calm-app -p calm-mcp`
  and copy `calm-mcp` to `Contents/MacOS/calm-mcp`. In dev, `cargo build -p calm-mcp` puts it
  in `target/debug` next to `calm-app`, where A4 finds it; `docs/dev-setup.md` says so.
- Outside CALM the plugin's fallback is `calm-mcp` on `PATH`. If it is not installed, Claude
  Code shows the `calm` MCP server as failed in `/mcp`; nothing else breaks. `docs/claude-code.md`
  documents both (and a symlink from the bundle into `~/.local/bin` for terminal use).

**Interfaces:**
- Consumes: socket protocol from A2, env from A4, placement from B1.
- Produces: stdio JSON-RPC 2.0, one message per line. Methods `initialize`, `tools/list`,
  `tools/call`, `ping`; notifications ignored. Server name `calm`. Protocol version: echo the
  client's if it is in `["2025-06-18", "2025-03-26", "2024-11-05"]`, else `"2025-06-18"`.
  Tools:
  - `list_panels {}` → id, kind, title, status, focused, and which one is "you"
  - `open_browser { url, side? }` (needs `allow_open`)
  - `open_terminal { cwd?, side? }` (needs `allow_open`, never a command)
  - `focus_panel { id }` (needs `allow_open`)
  - `set_status { state, text? }`
  A 403 becomes a tool error naming the setting to turn on. No CALM env means every tool
  returns `isError: true` with "not running in a CALM terminal".

- [ ] **Step 1: Failing tests:** `protocol.rs` unit tests for initialize and tools/list shape;
  `tests/stdio.rs` spawns the binary with a fake socket server (a test thread on a temp
  socket) and checks `list_panels` round-trips and `open_terminal` refuses a `command` field.
- [ ] **Steps 2 to 4:** Implement with std only (no tokio). Blocking client, 5 s timeout.
- [ ] **Step 5:** Manual check through Claude Code is owed to the user (GUI); record it in
  `.ai/status.md` under "Unverified". Commit `feat: calm-mcp stdio server`.

---

## Phase C: Claude Code plugin `calm` (`feat/claude-code-plugin`)

### Task C1: plugin skeleton, MCP config, skill

**Files:**
- Create: `integrations/claude-code/.claude-plugin/marketplace.json` (marketplace `calm`)
- Create: `integrations/claude-code/calm/.claude-plugin/plugin.json` (name `calm`; placement
  settings live in CALM, not here)
- Create: `integrations/claude-code/calm/.mcp.json`: server `calm`, command
  `${CALM_MCP_BIN:-calm-mcp}`
- Create: `integrations/claude-code/calm/skills/calm/SKILL.md`: when to open a browser beside
  the session (docs, a dev server), when to set `waiting`, never to open terminals unasked.

- [ ] Verification: `claude plugin validate integrations/claude-code/calm` rc=0, and
  `claude -p --plugin-dir integrations/claude-code/calm "list my CALM panels"` run outside CALM
  answers with the "not running in a CALM terminal" error, not a crash. Commit
  `feat: calm plugin for Claude Code (mcp + skill)`.

### Task C2: the mod

**Files:**
- Create: `integrations/claude-code/calm/hooks/hooks.json`, `hooks/register.ts`,
  `hooks/socket.ts` (token read with `$.fs.read`, `$.http.fetch` with `socketPath`),
  `hooks/register.test.ts`, `tsconfig.json`

**Interfaces:**
- Consumes: A5 `panel.setStatus`, A6 `events.publish` and `events.poll`.
- Produces:
  - `turn.start` → `working`; `turn.complete` → `idle`; a permission or question prompt
    (`tool.check` answering ask) → `waiting` with the tool name; an API error → `error`.
  - A session id: the one the mods API reports for this session if the reference has one
    (check `plugins/mods/reference.md`), else a random 16-hex id made in `register`.
  - `agent.spawn` and a 2 s `$.clock.every` over `$.agent.list()` publish topic `agent` with
    `{ lead: CALM_PANEL_ID, session, agentId, name, status, lastTurnAt }` on change, and the
    full current list every 30 s even without a change, so a board opened late or after a
    `gap` catches up.
  - A long-poll loop on topic `agent.message` with `timeoutMs: 20000`, cursor kept between
    polls. A payload `{ lead, session, to, text }` is delivered with
    `$.session.send({ to, text })` only when `lead` equals this `CALM_PANEL_ID` and `session`
    equals this session id; others are skipped. Stops on the first 401 or 403.
  - Commands `/calm open <url>` and `/calm board`.
  - With `CALM_SOCKET` unset, `register` registers nothing.

- [ ] **Step 1: Failing tests** with the documented mods test runner
  (`https://code.claude.com/docs/en/plugins/mods/test.md`; read it first and use its harness):
  `no_socket_registers_nothing`, `turn_start_sets_working`, `agent_list_change_publishes_once`,
  `full_list_republished_every_30s`, `message_poll_calls_session_send`,
  `message_for_other_lead_or_session_is_skipped`, `forbidden_stops_polling`.
- [ ] **Steps 2 to 4:** Implement; tests pass; `claude plugin validate` lists only
  `$.fs.read`, `$.http.fetch`, `$.agent.list`, `$.session.send`, `$.clock.every`,
  `$.command.register` under `calls:`.
- [ ] **Step 5:** Docs `docs/claude-code.md` (install with `/plugin marketplace add` on the
  repo dir, then `/plugin install calm@calm`; what each piece does; settings). Commit
  `feat: calm mod reports status and teammates`.

---

## Phase D: teammate cards (`feat/agent-board`)

### Task D1: `agent-board` web plugin

**Files:**
- Create: `plugins/agent-board/calm-plugin.json` (web plugin, `"addMenu": false` so it is
  only opened by CALM, see D2) and `plugins/agent-board/index.html`: a plain HTML page with
  inline script, no build step, following `plugins/calm-keys`.
- Create: `crates/calm-plugins/tests/agent_board_page.js` and a test in
  `crates/calm-plugins/tests/first_party_pages.rs` that runs it with `run_page_test`, as
  `calm_keys_page.js` does.

**Interfaces:**
- Consumes: `calm.events.on("external")` filtered to `topic === "agent"`,
  `events.poll`, `canvas.listPanels`, `events.publish`.
- Produces: on load, replay with `events.poll({ since: 0, topics: ["agent"], timeoutMs: 0 })`,
  then subscribe. One card per `(lead, session, agentId)` under its lead panel's title: name,
  status pill, time since last turn, a `waiting` highlight, a text box that publishes
  `agent.message { lead, session, to, text }`. Cards of an older `session` for the same lead
  are dropped when a newer one appears. Clicking a card focuses the lead panel.

- [ ] Tests (in `agent_board_page.js`, run under bun): `cards_group_by_lead`, `waiting_sorts_first`, `escapes_names`
  (name `<img onerror>` renders as text), `late_open_shows_existing_agents` (replay returns two
  agents, no live event: two cards), `new_session_replaces_old_cards`,
  `message_carries_lead_and_session`. `cargo test -p calm-plugins --test first_party_pages`
  passes. Commit
  `feat: agent board plugin`.

### Task D2: open the board beside the lead

**Files:** Modify `crates/calm-app/src/plugin_host.rs` / `canvas_view.rs`.

- [ ] When `agents.board = "auto"` and an `agent` event arrives for a lead with no board,
  CALM opens `agent-board` at
  `place_beside(lead.rect, default plugin size, agents.board_side, JOIN_SNAP_GAP, &other_rects)`.
  One board per lead. Closing it records `(lead, session)` as dismissed; the board opens again
  only for an `agent` event from that lead with a different `session`. Tests:
  `first_agent_event_opens_one_board`, `board_off_opens_nothing`,
  `dismissed_board_stays_closed_for_same_session`, `new_session_reopens_board`,
  `payload_reaches_page_escaped` (payload with `</script>` through `BridgeEvent::to_js`).
  Smoke: publish one `agent` event, print `smoke: agent_board opened=1`. Commit
  `feat: open the agent board beside the lead`.

---

## Phase E: "Claude Code" TUI plugin (`feat/claude-code-tui`)

### Task E1: built-in app

**Files:** Create `plugins/claude-code/calm-plugin.json` and `plugins/claude-code/icon.svg`.

- [ ] Manifest: `kind: "tui"`, `id: "claude-code"`, `name: "Claude Code"`, `command: "claude"`,
  `shell: true` (login `PATH`), `restart: "prompt"`, `defaultSize` 960×640, accent color in
  the manifest. If `claude` is missing, the panel shows the existing exit line. Test: the
  manifest loads in `crates/calm-plugins` discovery tests (`builtin_claude_code_manifest_is_valid`).
  Update `README.md` and `docs/claude-code.md`. Commit `feat: Claude Code app in the Add menu`.

---

## Phase F (optional, separate plan): teammates as live terminals

Only if, after using phase D, per-teammate interactive terminals are still wanted. Shape found
in Claude Code 2.1.316: backend `iterm2`, detected by `ITERM_SESSION_ID`, driven through
`it2 session split|run|send|list|close`, `it2` found with `$SHELL -lc 'command -v it2'`.
Needs: `it2` stand-in on the socket (uses the `allow_exec` unit), `ITERM_SESSION_ID` set only
by the Claude Code TUI app, and a `ZDOTDIR` wrapper so the stand-in wins the `PATH` lookup.
Depends on undocumented Claude Code internals; write its own plan with a contract test first.
