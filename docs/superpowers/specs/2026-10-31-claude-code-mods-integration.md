# Claude Code mods integration: research

Date: 2026-10-31. Status: research, no product code. Claude Code version checked: 2.1.316.

Tags: **[doc]** means stated in the cited Claude Code documentation or the types file of this
build. **[poc]** means shown by the throwaway proof of concept below. **[repo]** means read in
this repository. **[inferred]** means my reasoning, not checked.

## Summary

1. A Claude Code mod is trusted JS/TS that runs inside Claude Code. It sees tool calls, turns and subagents, draws terminal UI, and reaches files, processes and HTTP. It cannot listen for connections.
2. Best direction: Claude Code to CALM. A mod in a session running inside a CALM terminal panel posts status and subagent events to CALM over a local Unix socket. A mod can already do this today **[poc]**.
3. CALM needs a control socket that serves the same dispatcher as `window.calm`, terminal env vars naming the socket and panel, `panel.setStatus`, relative placement and host events to plugins.
4. One TSX bundle cannot be both a CALM plugin and a mod pane. CALM plugins are HTML/DOM. Mod panes are a closed element tree with no DOM. Only logic can be shared.
5. Top idea: live agent status on terminal panels. Then a subagent board plugin, then `/calm` open commands. Prompting a session from CALM ranks last.

## What mods are

### Shape and runtime

- A mod is a plugin with `.claude-plugin/plugin.json`, `hooks/hooks.json` naming one hooks module, and that module exporting `register(on, options)` **[doc]** ([reference, Files](https://code.claude.com/docs/en/plugins/mods/reference.md#files)).
- Every hook is `($, e, next)`. `$` is the mods API, `e` is frozen event data, `next(e)` runs the rest of the chain. A hook can observe, rewrite or answer an event **[doc]** ([overview](https://code.claude.com/docs/en/plugins/mods/overview.md#what-a-hook-can-do-with-an-event)).
- The module has no Node APIs, no `setTimeout` and no network or file access of its own. All outside access goes through `$` **[doc]** ([api, Reach files](https://code.claude.com/docs/en/plugins/mods/api.md#reach-files-processes-and-the-network)).
- A hook gets 10 s of its own execution time. Time spent awaiting `next` or a `$` call does not count **[doc]** ([reference, Limits](https://code.claude.com/docs/en/plugins/mods/reference.md#limits)).

### Lifecycle

- `session.start` fires once per loaded mod, before the first prompt, and again after a reload. Commands and tools are registered there **[doc]** ([reference, Session](https://code.claude.com/docs/en/plugins/mods/reference.md#session)).
- A reload re-runs `register` and `session.start`. `$.state` and `$.store` survive, module variables do not. Timers stop on reload **[doc]** (plugin-authoring skill SKILL.md; [api, Stop background work](https://code.claude.com/docs/en/plugins/mods/api.md#stop-background-work)).
- Installed as a plugin from a marketplace, or loaded for one session with `--plugin-dir`. `CLAUDE_CODE_PLUGIN_DIRS` does the same through env **[doc]** ([reference, Settings](https://code.claude.com/docs/en/plugins/mods/reference.md#settings-and-environment-variables)).

### UI surfaces

| Surface | How | Source |
| --- | --- | --- |
| Pane beside the transcript | `$.ui.open({ id, title })` plus a `ui.render` hook on `Pane` | [interface](https://code.claude.com/docs/en/plugins/mods/interface.md) **[doc]** |
| Band above the prompt | `ui.render` on `AbovePrompt` | same **[doc]** |
| Status line, toast, transcript log line, OS notification | `$.ui.status`, `$.ui.toast`, `$.ui.log`, `$.ui.notify` | [api](https://code.claude.com/docs/en/plugins/mods/api.md#show-something-without-starting-a-turn) **[doc]** |
| Restyle built-in parts (spinner, tool rows, messages) | `ui.render` on `Spinner`, `ToolUse` and others | [reference, Render sites](https://code.claude.com/docs/en/plugins/mods/reference.md#render-sites) **[doc]** |

Elements are `Box`, `Text`, `Button`, `Link`, `Code`, `Markdown`, `Input`, `Select`, `Svg`,
`Client`, `Raster` and `Image`. There is no HTML or webview element. `Client` is a region drawn
by a second file of the mod that "gets no mods API" and talks to hooks only by posting
`ui.message` events **[doc]** ([interface, elements table](https://code.claude.com/docs/en/plugins/mods/interface.md)).

### Hooks (events)

Groups **[doc]** ([reference, Events](https://code.claude.com/docs/en/plugins/mods/reference.md#events)):
tools (`tool.call`, `tool.check`, `tool.describe`), prompts (`prompt.submit`,
`prompt.compose` and others), commands (`command.run`), turns (`turn.start`, `turn.step`,
`turn.complete`), session (`session.start`, `session.end`, `session.receive`,
`session.send`, `session.attach`), subagents (`agent.offer`, `agent.spawn`), interface
(`ui.render`, `ui.press` and others), and every `$` call as its own event (`fs.read`,
`http.fetch`...). `turn.step` and `turn.complete` carry `e.agentId` for subagent turns **[doc]**
([events, Follow a turn](https://code.claude.com/docs/en/plugins/mods/events.md#follow-a-turn)).

### Tools and commands

- `$.command.register({ name, description })` adds `/name`, answered by `command.run`. It runs without a Claude turn **[doc]** ([api, Add a command](https://code.claude.com/docs/en/plugins/mods/api.md#add-a-command)). In `claude -p "/calmpoc"` the command's text printed and nothing else **[poc]**.
- `$.tool.register({ name, description, inputSchema })` gives Claude a tool named `mcp__<plugin>__<name>`, answered by a `tool.call` hook **[doc]** ([api, Add a tool](https://code.claude.com/docs/en/plugins/mods/api.md#add-a-tool)). The behavior-demo mod on this machine registers `roll_dice` this way (`~/.claude/plugins/marketplaces/claude-code-mods-demo/behavior-demo/hooks/register.tsx`) **[repo, local]**.
- `$.agent.list()` returns subagents and teammates with status `pending`, `running`, `waiting`, `idle`, `completed`, `failed` or `killed` **[doc]** ([reference, Mods API methods](https://code.claude.com/docs/en/plugins/mods/reference.md#mods-api-methods)).
- `$.prompt.submit({ text })` starts a turn when the session is idle **[doc]** ([api](https://code.claude.com/docs/en/plugins/mods/api.md#start-a-turn-from-a-background-job)).

### Talking to the outside world

| Channel | Facts | Tag |
| --- | --- | --- |
| Files | `$.fs.read/write/list/exists/stat`, 4 MiB per file, `write` not atomic | **[doc]** reference |
| Processes | `$.process.run(argv)` with no shell, 30 s default and 10 min max. `$.process.spawn` streams and lives as long as its loop | **[doc]** api and types file |
| HTTP | `$.http.fetch(url, init)` over http or https, 30 s per call, 4 MiB bodies | **[doc]** reference |
| Unix socket | `init.socketPath` sends the request over a Unix socket instead of TCP | **[doc]** types file only (`plugin-authoring/types/claude-code.d.ts`, `http.fetch`), **[poc]** works |
| Env | `$.env.get('NAME')`, with the name written as a literal | **[doc]** api |
| Other sessions | `$.session.send({ to, text })` reaches another session, a subagent or a teammate | **[doc]** api |
| Timers | `$.clock.every/after`, the only way to run between events | **[doc]** api |

### What a mod cannot do

- It cannot accept connections. No server or listen API exists in the documented namespaces or the types file **[doc]**, but I searched the types file for `listen` and `socket` only, so this absence is **[inferred]** from that search plus the namespace table.
- It cannot draw HTML or host a webview. A pane is an element tree **[doc]**.
- It cannot change the permission prompt **[doc]** ([overview](https://code.claude.com/docs/en/plugins/mods/overview.md#what-a-mod-can-reach)).
- It cannot draw under `claude -p`, the Agent SDK or cloud sessions, though hooks still run there **[doc]** ([overview, Where mods run](https://code.claude.com/docs/en/plugins/mods/overview.md#where-mods-run)).
- It is not sandboxed. A process a mod starts runs outside Claude Code's Bash sandbox **[doc]** (same page).

## Integration directions

### a. Claude Code to CALM

Setup: Claude Code runs in a CALM Ghostty terminal panel. CALM gives each terminal
`CALM_SOCKET`, `CALM_TOKEN_FILE` and `CALM_PANEL_ID`. A mod (`calm-bridge`) reads them with
`$.env.get`. If they are missing it does nothing, so the mod is harmless outside CALM **[inferred]**.

| Use | Mod side | CALM side | Feasible |
| --- | --- | --- | --- |
| Agent status on the panel | `turn.start`/`turn.complete` (main turn has no `agentId`), `tool.check` decision `ask` means waiting for the user | show state in panel chrome | Yes **[inferred]**, transport **[poc]** |
| One element per subagent | `agent.spawn`, `turn.complete` with `agentId`, `$.agent.list()` on a `$.clock.every` timer | a board plugin, or child badges | Yes **[inferred]** |
| Open a browser or terminal panel | `/calm open <url>` command or `mcp__calm-bridge__open` tool, POST `canvas.createPanel` | place it next to `CALM_PANEL_ID` | Yes **[inferred]** |
| Show the session's files or diff | after `turn.complete`, send changed paths seen in `tool.call` for Edit/Write | plugin runs `git diff` through `calm.process.exec` | Yes **[inferred]**, overlaps the built-in `/diff` mod **[doc]** |

Important: a mod tool that opens a CALM terminal with a `command` would let Claude run commands
without going through Claude Code's Bash permission check or sandbox. The external channel must
not accept `command` from a mod tool by default **[inferred]**.

### b. CALM to Claude Code

| Use | Path | Feasible |
| --- | --- | --- |
| Host Claude Code in a terminal panel | `claude` in a Ghostty panel. Works with no new code **[repo]** (terminals are live) | Yes now |
| Companion plugin panel beside it | plugin opened next to the terminal, fed by the mod's events | Needs the plugin host, which is not wired yet **[repo]** (`.ai/status.md`) |
| Show sessions | from the mod's pushed events, one entry per panel. Reading `~/.claude/projects` transcripts directly is possible but undocumented here **[inferred]** | Yes via the mod |
| Send a prompt | CALM queues text. The mod long-polls the socket on a timer (fetch has a 30 s cap) and calls `$.prompt.submit`. Simpler: paste into the terminal | Possible **[inferred]**, low value |
| Render mod panes inside CALM | Mods draw only on Claude Code's own surfaces. `session.attach` shows remote clients can attach **[doc]** (types file), but no third-party surface protocol is documented | No, not documented |

### c. Shared plugin format

| | CALM plugin | Claude Code mod |
| --- | --- | --- |
| Manifest | `calm-plugin.json`: id, name, version, entry, defaultSize **[repo]** | `.claude-plugin/plugin.json` plus `hooks/hooks.json` **[doc]** |
| Runtime | WKWebView page with DOM, `window.calm` via prelude **[repo]** | JS module in Claude Code, no DOM, `$` per hook **[doc]** |
| UI | HTML, CSS, Preact | element tree from `$.ui.resolve(e)`, JSX against global `h` **[doc]** |
| Host calls | Promise RPC over the `calm` message handler **[repo]** | `$.ns.method()` calls, each also an event **[doc]** |
| Events | `calm.events.on` with 6 events **[repo]** | `on(event, matcher, hook)` middleware chain **[doc]** |

Verdict: not one bundle. Different UI models, different globals and different manifests. A
shared TS core with no DOM and no host calls (formatters, state reducers) plus two thin adapters
is possible, but small plugins gain little from it **[inferred]**. I do not recommend it now.

## Required CALM changes

1. **Control socket.** HTTP/1.1 + JSON on a Unix socket, served by the same dispatcher as the plugin bridge, with an origin of `external` passed to handlers. HTTP keeps the mod side to one `$.http.fetch` call **[poc]**. Requests reuse `BridgeRequest { method, params }`, answers reuse `BridgeResponse`.
2. **Terminal env.** `CALM_SOCKET`, `CALM_TOKEN_FILE`, `CALM_PANEL_ID` set for each spawned terminal. The adapter's `terminal::options(command, cwd)` takes no env today **[repo]** (`crates/calm-app/src/terminal/mod.rs`). Whether `TerminalOptions` or libghostty accepts env is **unverified**.
3. **`panel.setStatus`** `{ panelId?, state, text? }`, shown in panel chrome. `canvas.listPanels` returns status.
4. **Relative placement.** `canvas.createPanel` takes `{ near: PanelId, side }`.
5. **Host events to plugins.** `panelStatusChanged`, `panelsChanged`, and a generic `external { topic, payload }` that external clients publish. CALM stays ignorant of Claude Code's model.
6. **Long-poll for the reverse direction.** `events.poll({ since, timeoutMs <= 25000 })`, because mods cannot keep a socket open.
7. **Prerequisite.** The plugin host and dispatcher (Tasks 11 and 12) are not wired **[repo]**. Ideas 2 to 5 depend on them. Idea 1 needs only the socket and chrome.

These needs were sent to the `plugin-api` agent. Its reply maps them onto the v2 design
(`docs/superpowers/specs/2026-10-31-plugin-api-v2-design.md`, commit 80b0d87 on
`feat/plugin-api`). I have not read that draft myself, so this mapping is as reported by that
agent:

- Fits already, or with a small change: the shared dispatcher, with callers attributed as `Caller::{Panel(PanelId, plugin_id), External(client_id)}`; `near`/`side` placement and terminal `cwd` on `PanelSpec`; panel events (`panelAdded`, `panelRemoved`, `panelChanged`, coalesced at 100 ms per panel) in place of `panelsChanged`; `external` events as bus publishes `from: { external: client_id }`; auth handled before dispatch on a server thread that only feeds the existing channel.
- New work: terminal env vars, `panel.setStatus` with chrome and a `panelStatusChanged` event, and `events.poll` with a per-client cursor and a bounded backlog that reports `gap: true` when it drops events.
- Errors in v2 are `{ code, message }`. An unknown method keeps the message `not implemented: <method>` and adds `code: "not_implemented"`.
- Whether the external channel goes into v2 at all is waiting on the lead.

## Security

The threat: any process that can reach the socket can create terminals, and a terminal runs
anything. Plugins stay trusted code, never described as sandboxed.

Proposal **[inferred]** unless tagged:

- **Unix socket, never TCP.** Web pages cannot reach a Unix socket. A localhost TCP port can be hit by browser pages through DNS rebinding or form posts. That includes CALM's own browser panels, which must never get host access (design spec, Plugins).
- **Filesystem permissions are the main gate.** Directory `$CALM_HOME/run` (default `~/.config/calm/run`) mode 0700, socket 0600, created under `umask 0177` **[poc]** (socket was `srw-------`). Keep the path short: macOS `sun_path` is 104 bytes. Check the owner and refuse to start if the directory is a symlink or owned by someone else.
- **Per-launch token as a second check.** 32 random bytes, hex, in `run/token` mode 0600, replaced on each launch and deleted on quit. `Authorization: Bearer` is required on every request, compared in constant time, 401 before dispatch **[poc]** (wrong token got 401). Terminals get the token file path, not the token, so it does not show up in `env` dumps or process listings.
- **Be honest about the limit.** Same-user processes can read the 0600 token anyway. The token protects against other users, stale sockets and accidental clients. It does not protect against malware running as you.
- **Capability allowlist per origin.** External capabilities are an explicit allowlist in Settings. The default is read-only methods plus `panel.setStatus` and `events.*`. Everything that can make a terminal run something counts as one capability: `createPanel` with `command`, writing input to a terminal (v2 `terminals.write`), and `process.exec`. These are refused or allowed together. Refusing `command` alone means nothing while a client can open a plain shell and type into it (point raised by the plugin-api design). Opening browser panels and empty terminals is a separate opt-in, and `github.api` stays plugin-only.
- **No credentials cross the channel.** GitHub stays on `gh api` on the host. Status text and events are display data. They must never be logged with tokens and never written to layouts.
- **Mod-side rule.** A mod tool that opens panels must not accept a shell command, so Claude cannot route around Claude Code's permission prompt and sandbox (see direction a).
- **Untrusted input.** Status text comes from model output. Render it as plain text in GPUI chrome, cap its length, and never eval it into a plugin page without the existing JSON escaping (`BridgeEvent::to_js`) **[repo]**.

## Ranked ideas

Value is for this user, who runs several agents in parallel. Cost is CALM work plus mod work.

| # | Idea | Value | Cost | Smallest useful version |
| --- | --- | --- | --- | --- |
| 1 | Agent status on terminal panels | High | Low to medium | Socket with `panel.setStatus` only, terminal env vars, a colored dot plus text in panel chrome. Mod: about 40 lines on `turn.start`, `turn.complete` and `tool.check` (ask). Glance at the canvas to see which agent waits for you. |
| 2 | Subagent board | High | Medium (needs plugin host) | A plugin panel that lists `external` events with topic `agent`, one row per `agentId` with status and last turn time. Mod sends `agent.spawn` and subagent `turn.complete`. |
| 3 | `/calm open` and an open tool | Medium | Low once 1 exists | `/calm open <url>` puts a browser panel right of the session panel (needs browser panels wired, Task 10). Then `/calm term` opens an empty terminal at the session cwd. No command argument. |
| 4 | Turn diff panel | Medium | Medium | After each main turn the mod publishes the changed paths, and a plugin shows `git diff --stat` for them via `process.exec`. Check first whether the built-in `/diff` pane already covers it. |
| 5 | Prompt a session from CALM | Low | Medium to high | Plugin input, `events.poll` in the mod, `$.prompt.submit`. Typing in the terminal already does this. |

## Proof of concept

Lived in `/tmp/calm-mods-poc`, outside the repo. A Python Unix-socket HTTP server (socket 0600,
token 0600 in a 0700 dir) and a mod loaded with `claude -p --plugin-dir` registered `/calmpoc`.
The command read the token with `$.fs.read`, then POSTed with `socketPath` twice. `claude plugin
validate` passed (rc=0, `calls: $.command.register, $.fs.read, $.http.fetch, $.session.cwd`).
Run result, claude rc=0:

```text
calm-poc: POC status=200 body={"ok": true, "panelId": 7} | status=401 body={"ok": false, "panelId": null}
```

The server log showed both requests with the JSON body `{"kind":"terminal","cwd":"/private/tmp"}`.
Proved: on Claude Code 2.1.316, a mod can reach a permission-restricted Unix socket with a bearer
token and read a 0600 file. Not proved: behavior in an interactive session, on other versions,
or under org policies that restrict `http.fetch`.

## Decisions (2026-10-31)

The user accepted the recommendations:

1. First version: agent status on terminal panels (idea 1). The subagent board follows once the plugin host is merged.
2. External callers get the allowlist from the Security section. The default is read-only methods plus `panel.setStatus` and `events.*`. Running something in a terminal (`command`, `terminals.write`, `process.exec`) is one unit, refused by default and only enabled together through a Settings switch.
3. The `calm-bridge` mod lives in this repo with a `marketplace.json`, so its version moves with CALM.
4. The external socket channel is approved to build as phase 11 of the plugin API v2 design, after the plugin host and browser panels are merged.

## Sources

- https://code.claude.com/docs/en/plugins/mods/overview.md
- https://code.claude.com/docs/en/plugins/mods/api.md
- https://code.claude.com/docs/en/plugins/mods/reference.md
- https://code.claude.com/docs/en/plugins/mods/interface.md
- https://code.claude.com/docs/en/plugins/mods/events.md (subagent and turn fields)
- Also fetched, only searched, not read in full: create, gallery, test, troubleshoot, admin, and https://code.claude.com/docs/en/plugins/security.md
- Types file of this build: `plugin-authoring/types/claude-code.d.ts` from the bundled skill, Claude Code 2.1.316 (`http.fetch` `socketPath`, `process.spawn`, `session.surfaces`, `AgentStatus`)
- plugin-authoring skill `SKILL.md` and `reference.md` (bundled with Claude Code 2.1.316)
- Demo mod: `~/.claude/plugins/marketplaces/claude-code-mods-demo/behavior-demo/`
- Repo: `.ai/system.md`, `.ai/status.md`, `docs/plugins.md`, `packages/calm-plugin-sdk/calm.d.ts`, `docs/superpowers/specs/2026-10-31-calm-design.md` (Plugins), `crates/calm-app/src/terminal/mod.rs`

No fetched page tried to give instructions. Nothing to quote.
