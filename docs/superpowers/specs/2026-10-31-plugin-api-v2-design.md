# CALM plugin API v2 design

Status: draft for review. Nothing here is implemented. Extends the plugin section of
`2026-10-31-calm-design.md`, which stays binding where the two disagree.

## Context

The v1 contract (`crates/calm-plugins/src/bridge.rs`, `packages/calm-plugin-sdk/calm.d.ts`)
declares 17 methods. The host in PR #5 (`crates/calm-app/src/plugin_host.rs`) serves four:
`log`, `panel.setTitle`, `settings.get`, `settings.set`. The other 13 reject with
`not implemented: <method>`.

The bridge: the prelude posts a JSON string to the `calm` WebKit message handler. The
handler pushes `(PanelId, String)` into a `futures` unbounded channel. A GPUI task drains
it and calls `CanvasView::plugin_message`, which parses a `BridgeRequest`, runs `dispatch`
against the calling panel and answers with `eval_js("window.__calm.resolve(...)")`. Host to
page events use `window.__calm.emit(event, payload)`.

### v1 methods and their intent

| Method               | Intent                                                    | PR #5 host |
| -------------------- | --------------------------------------------------------- | ---------- |
| `settings.get`       | This panel's settings object (stored with the layout)     | served     |
| `settings.set`       | Replace this panel's settings object                      | served     |
| `storage.get`        | Read a key from per-plugin data                           | rejects    |
| `storage.set`        | Write a key to per-plugin data                            | rejects    |
| `storage.remove`     | Delete a key from per-plugin data                         | rejects    |
| `panel.setTitle`     | Set this panel's chrome title                             | served     |
| `panel.getBounds`    | This panel's size and the camera zoom                     | rejects    |
| `panel.minimize`     | Minimize this panel, page keeps running                   | rejects    |
| `panel.close`        | Close this panel through the normal close path            | rejects    |
| `canvas.createPanel` | Open a terminal, browser or plugin panel                  | rejects    |
| `canvas.focusPanel`  | Focus a panel by id                                       | rejects    |
| `canvas.listPanels`  | `PanelInfo[]` for every panel                             | rejects    |
| `process.exec`       | Run a program on the host, return code, stdout, stderr    | rejects    |
| `github.api`         | `gh api <path>` on the host, parsed JSON                  | rejects    |
| `openExternal`       | Open a URL in the system browser                          | rejects    |
| `theme.get`          | `{ background }` as `#rrggbb`                             | rejects    |
| `log`                | Fire and forget line on CALM's stderr                     | served     |

v1 events: `focus`, `blur`, `resize`, `visibility`, `settingsChanged`, `themeChanged`. None
has a host sender yet.

### Models the API reads and writes

- `calm_core::scene`: `Scene { panels (back to front), camera }`, `Panel { id, kind, rect,
  title, minimized }`, `PanelKind::{Terminal { command, cwd }, Browser { url }, Plugin {
  plugin_id, settings }}`, `bring_to_front`, ids never reused.
- `calm_core::camera`: `offset`, `zoom` in `[0.1, 4.0]`, `center_on`, `fit` (PR #1).
- `calm_core::interaction`: close goes through `InputHandler::request_close(id, alive)`,
  which sets `confirming` and returns `ConfirmClose`; Enter or a second request closes,
  Escape cancels. One panel can be confirming at a time.
- `calm_core::settings::Settings`: `background` (a `BackgroundPreset`), `show_grid`,
  `confirm_close`, `terminal_user_config`.
- `calm_core::menu`: Add submenu, panel actions (PR #2 adds zoom to fit, bring to front,
  send to back, duplicate, delete).
- Browser panels (PR #6, `calm-native/src/browser.rs`): `BrowserView::open`, `url()`,
  `BrowserEvent::{TitleChanged, UrlChanged, Clicked}`. No `calm` message handler.
- Terminals: `NativeSurface::text(&CStr)` exists and is public. Ghostty emits
  `GHOSTTY_ACTION_SET_TITLE` and `GHOSTTY_ACTION_PWD`, but the gpui-libghostty 0.3.1 shim
  (`shim/ghostty_surface.m`, `runtime_action`) handles only `RENDER` and drops the rest.
  Terminal title and cwd are not observable today.

## Goals

- Plugins create and control every panel kind: terminals, browsers, other plugin panels.
- Plugins read a full picture of the canvas: panels, focus, camera, app settings, version,
  browser url and title, terminal metadata when Ghostty exposes it.
- Plugins influence the app: move the camera, navigate browsers, type into terminals,
  add context menu items and key bindings, change the background, show a short notice.
- Plugins react to changes through events that stay cheap during pan and zoom.
- Plugins talk to each other and keep their own data.
- The manifest says what a plugin does, so the user can read it before installing.
- v1 plugins keep working unchanged.

## Non-goals

- A sandbox or a permission system that protects the user from a plugin. Plugins are
  trusted, explicitly installed code. Capabilities are a declaration, not a wall.
- Giving browser panels the bridge. Remote pages stay without host access.
- Exposing any credential to plugin JS. GitHub stays behind `gh api` on the host.
- Reading terminal screen contents or scrollback. Not in libghostty's surface API here.
- Closing a live terminal without the user's confirmation.
- Remote or marketplace install, plugin updates, hot reload of manifests.
- Layout save and load (Task 13). The API leaves room for it but does not depend on it.

## Principles

1. **Calls are attributed.** The channel tags each message with the calling `PanelId`. The
   host resolves it to the plugin id through the scene. No method takes a "caller" param.
2. **One dispatcher, pure core.** Parsing, capability checks, snapshots and coalescing are
   pure Rust with unit tests. `plugin_host.rs` only applies effects to GPUI state.
3. **Never re-enter GPUI from an AppKit callback.** WebKit and NSMenu callbacks only push
   into a channel. All scene, camera and native view work happens in the GPUI task.
4. **Fail loud.** Every request settles. Unknown, malformed, undeclared and unserved calls
   reject with a code. No silent defaults.
5. **Additive versioning.** New methods and events only. A v1 method keeps its name and
   shape forever; v2 adds new names next to it.

## Manifest additions

```json
{
  "id": "agent-board",
  "name": "Agent Board",
  "version": "0.2.0",
  "entry": "index.html",
  "defaultSize": { "width": 420, "height": 300 },
  "apiVersion": 2,
  "capabilities": ["panels.create", "panels.control", "terminal.write", "camera.control"],
  "background": false,
  "contributes": {
    "commands": [{ "id": "spawn-agents", "title": "Spawn agent terminals" }],
    "menus": {
      "canvas": [{ "command": "spawn-agents" }],
      "panel": [{ "command": "spawn-agents", "when": "kind == terminal" }]
    },
    "keybindings": [{ "command": "spawn-agents", "key": "cmd-shift-a" }]
  }
}
```

| Field          | Meaning                                                                       |
| -------------- | ----------------------------------------------------------------------------- |
| `apiVersion`   | Integer. Missing means 1. The host refuses a manifest above its own version.  |
| `capabilities` | Strings from the capability table. Unknown strings fail the manifest.         |
| `background`   | Opt-in hidden page started at launch (see open question 3). Default false.    |
| `contributes`  | Commands, menu entries and key bindings. Requires `apiVersion` 2.             |

Validation stays in `PluginManifest::validate` and reports the path and field, like every
other manifest error. A broken `contributes` block skips the plugin, it does not half load.

## Capabilities

A capability is a word in the manifest that names a group of methods. The host checks it on
every call and rejects an undeclared one with `capability_missing`.

**Decision: undeclared calls are rejected.** Why: the declared list is only useful to the
user if it is true. A warn-only check lets the manifest drift from the code and turns the
list into decoration. A hard reject fails on the author's first test run, which is the
cheapest moment to fix it. This is not a security boundary: a trusted plugin with
`process.exec` can do anything the user can. Docs must say so in those words. Cost if
wrong: a little author friction; reversing it later to warn-only is a one-line change.

v1 manifests (no `apiVersion`) get the implicit set `v1` below, so nothing that works today
breaks. Methods with no capability are always allowed.

| Capability        | Methods                                                                     |
| ----------------- | --------------------------------------------------------------------------- |
| (none)            | `log`, `app.info`, `panel.*` on the plugin's own panel, `settings.*` (own panel), `storage.*`, `panels.list`, `panels.get`, `camera.get`, `app.getSettings`, `theme.get`, `bus.*`, `events.*`, `ui.notify` |
| `panels.create`   | `panels.create`, `canvas.createPanel`                                       |
| `panels.control`  | `panels.focus`, `move`, `resize`, `setRect`, `setTitle`, `minimize`, `restore`, `bringToFront`, `sendToBack` on other panels; `canvas.focusPanel` |
| `panels.close`    | `panels.close` on other panels                                              |
| `camera.control`  | `camera.set`, `camera.centerOn`, `camera.fit`                               |
| `terminal.read`   | `terminals.info`                                                            |
| `terminal.write`  | `terminals.write`                                                           |
| `browser.control` | `browsers.navigate`, `browsers.reload`, `browsers.info`                     |
| `settings.write`  | `app.setSettings`                                                           |
| `process.exec`    | `process.exec`                                                              |
| `github`          | `github.api`                                                                |
| `open.external`   | `openExternal`                                                              |

Implicit `v1` set: `panels.create`, `panels.control`, `process.exec`, `github`,
`open.external`. That matches what v1 already promises.

## Namespaces and methods

All calls return a Promise except `log`, `events.on`, `bus.subscribe` and `calm.has`. Wire
method names are the dotted names below. `PanelId` stays a number.

### Types

```ts
export type PanelId = number;
export interface Point { x: number; y: number }
export interface Size { width: number; height: number }
export interface Rect { x: number; y: number; width: number; height: number }
export type PanelKind = "terminal" | "browser" | "plugin";
export type PanelMode = "live" | "preview" | "hidden";

export interface PanelInfo {            // v1 fields first, unchanged
  id: PanelId;
  kind: PanelKind;
  title: string;
  rect: Rect;                           // world units, chrome strip included
  minimized: boolean;
  z: number;                            // 0 is backmost
  mode: PanelMode;
  focused: boolean;
  pluginId?: string;                    // kind "plugin"
  url?: string;                         // kind "browser", committed URL
  createdBy?: string;                   // plugin id, when a plugin created it
}

export type PanelSpec =
  | { kind: "terminal"; command?: string; args?: string[]; cwd?: string;
      env?: Record<string, string>; title?: string; rect?: Rect; focus?: boolean }
  | { kind: "browser"; url: string; title?: string; rect?: Rect; focus?: boolean }
  | { kind: "plugin"; pluginId: string; settings?: Record<string, unknown>;
      title?: string; rect?: Rect; focus?: boolean };

export interface CameraInfo {
  x: number; y: number;                 // world point at the viewport origin
  zoom: number;
  minZoom: number; maxZoom: number;
  viewport: Rect;                       // visible world rect
}

export interface AppInfo {
  version: string;                      // CARGO_PKG_VERSION of calm-app
  apiVersion: number;                   // highest API the host serves
  platform: "macos";
  pluginId: string;
  panelId: PanelId;                     // the calling panel
  capabilities: string[];               // as declared, or the implicit v1 set
  methods: string[];                    // methods this host serves
}

export interface AppSettings {
  background: string;                   // "#rrggbb"
  backgroundPreset: string | null;
  showGrid: boolean;
  confirmClose: boolean;
  terminalUserConfig: boolean;
}

export interface TerminalInfo {
  id: PanelId;
  alive: boolean;
  command: string | null;               // from the spawn spec
  cwd: string | null;                   // spawn cwd; live cwd once Ghostty reports PWD
  title: string | null;                 // Ghostty title, null until the shim reports it
  grid?: { columns: number; rows: number };  // only if the adapter exposes it
}

export interface BrowserInfo { id: PanelId; url: string | null; title: string; loading: boolean }

export interface CalmError extends Error { code: ErrorCode }
```

### `calm.app`

| Method                   | Wire              | Result                                                         |
| ------------------------ | ----------------- | -------------------------------------------------------------- |
| `info()`                 | `app.info`        | `AppInfo`                                                      |
| `getSettings()`          | `app.getSettings` | `AppSettings`                                                  |
| `setSettings(patch)`     | `app.setSettings` | Merged `AppSettings`. Writes `settings.json`. See below.       |
| `calm.has(method)`       | none              | Sync. True if `method` is in the served list. Feature detection. |

`setSettings` takes a partial `AppSettings`. `background` accepts a preset name or, if the
user approves open question 2, any `#rrggbb`. `confirmClose` is **not** writable by
plugins: a plugin that turns off confirmation would defeat the close rule. It rejects with
`invalid_params`.

### `calm.panel` (the calling panel, v1, now served)

`setTitle`, `getBounds`, `minimize`, `close` as in v1. Added: `info()` (`panel.info`,
`PanelInfo` of itself), `setRect(rect)` (`panel.setRect`). `close` on itself never needs
confirmation: a plugin page has no process the user can lose.

### `calm.panels`

| Method                    | Wire                  | Result and rules                                                    |
| ------------------------- | --------------------- | ------------------------------------------------------------------- |
| `create(spec)`            | `panels.create`       | `PanelId`. No `rect`: placed at the viewport center at default size, snapped. Rect is clamped to `MIN_PANEL_SIZE`. |
| `list(filter?)`           | `panels.list`         | `PanelInfo[]` back to front. `filter`: `{ kind?, pluginId? }`.      |
| `get(id)`                 | `panels.get`          | `PanelInfo`, or `not_found`.                                        |
| `focus(id)`               | `panels.focus`        | Focuses and brings to front. Restores a minimized panel first.      |
| `move(id, point)`         | `panels.move`         | Moves the rect origin. No snapping unless `{ snap: true }`.         |
| `resize(id, size)`        | `panels.resize`       | Changes the logical size, so a terminal reflows. Never zoom.        |
| `setRect(id, rect)`       | `panels.setRect`      | Move and resize in one step.                                        |
| `setTitle(id, title)`     | `panels.setTitle`     | Chrome title.                                                       |
| `minimize(id)` / `restore(id)` | `panels.minimize` / `panels.restore` | Process keeps running.                         |
| `bringToFront(id)` / `sendToBack(id)` | `panels.bringToFront` / `panels.sendToBack` | Scene order, then native restack. |
| `close(id)`               | `panels.close`        | `"closed"` or `"cancelled"`. See the close rule.                    |

v1 `canvas.createPanel`, `canvas.focusPanel`, `canvas.listPanels` stay as aliases of
`panels.create`, `panels.focus`, `panels.list`. The v1 `PanelSpec` is a subset of v2.

Terminal `env`: keys must match `[A-Z_][A-Z0-9_]*`, values have no NUL. The host merges them
over its own environment. Docs tell authors not to pass tokens through `env`; the host never
logs env values. `args` exists so a command with spaces needs no shell quoting; with `args`
present, `command` is the program, not a shell line.

**Close rule.** `panels.close(id)` on a terminal with a live process goes through
`InputHandler::request_close(id, alive)` exactly as a click on the close button does. If
`confirm_close` is on, the panel enters the existing confirmation and the Promise stays
pending until the user presses Enter (`"closed"`), presses Escape or acts elsewhere
(`"cancelled"`). If another panel is already confirming, the call rejects with `busy`. If
`confirm_close` is off, or the terminal's process has exited, it closes at once. Browser and
plugin panels close at once. A plugin can never pass a "force" flag.

### `calm.camera`

| Method                 | Wire              | Result                                                             |
| ---------------------- | ----------------- | ------------------------------------------------------------------ |
| `get()`                | `camera.get`      | `CameraInfo`                                                       |
| `set(partial)`         | `camera.set`      | `{ x?, y?, zoom? }`, zoom clamped to the limits. Returns `CameraInfo`. |
| `centerOn(point)`      | `camera.centerOn` | `Camera::center_on`, keeps zoom.                                   |
| `fit(ids?, margin?)`   | `camera.fit`      | `Camera::fit` over the union of `ids`, or all panels.              |

No animation in v2. Plugins that want one call `set` from `requestAnimationFrame`; the
coalescing below keeps that from flooding events.

### `calm.terminals`

| Method               | Wire               | Result                                                          |
| -------------------- | ------------------ | --------------------------------------------------------------- |
| `info(id)`           | `terminals.info`   | `TerminalInfo`. `wrong_kind` for a non-terminal.                |
| `write(id, text)`    | `terminals.write`  | Sends `text` as typed input through `NativeSurface::text`. Max 64 KiB per call. `not_alive` if the process exited. |

`write` is input, not paste: Ghostty sees it as typed text, so `"ls\r"` runs `ls`. NUL is
rejected (`CString`). It works on any terminal, not only ones the plugin created, because a
plugin with `process.exec` already has that power. The capability makes it visible.

### `calm.browsers`

| Method                | Wire                  | Result                                     |
| --------------------- | --------------------- | ------------------------------------------ |
| `info(id)`            | `browsers.info`       | `BrowserInfo`                              |
| `navigate(id, url)`   | `browsers.navigate`   | `BrowserView::open` with the same address normalization as the address bar. |
| `reload(id)`          | `browsers.reload`     | Reload.                                    |

No script injection into browser panels and no reading page content. That would hand
remote pages a path to the host through the plugin.

### `calm.storage` (v1, now served)

Per-plugin JSON in `$CALM_HOME/plugin-data/<plugin-id>.json` (default
`~/.config/calm/plugin-data`). Added: `keys()` (`storage.keys`). Limits: key up to 256
bytes, file up to 5 MiB after a write, else `too_large`. Writes go to a temp file then a
rename, so a crash never leaves half a file. A corrupt file rejects reads with
`storage_corrupt` and is never silently replaced by `{}`.

### `calm.bus` (cross-plugin messaging)

| Method                          | Wire              | Result                                                |
| ------------------------------- | ----------------- | ----------------------------------------------------- |
| `publish(topic, data)`          | `bus.publish`     | Number of pages it was delivered to.                  |
| `subscribe(topic, handler)`     | `bus.subscribe`   | Sync, returns an unsubscribe function. Prelude keeps handlers; the host keeps the topic set per page. |

Topics are strings up to 128 bytes. Payload up to 64 KiB of JSON, else `too_large`. Delivery
is in order per publisher, to every page subscribed to the topic, the publisher included
only if it subscribed. Handlers get `{ topic, data, from: { pluginId, panelId } }`. No
persistence, no replay: a page that subscribes later misses earlier messages.

### `calm.ui`

| Method                  | Wire         | Result                                                         |
| ----------------------- | ------------ | -------------------------------------------------------------- |
| `notify(text, opts?)`   | `ui.notify`  | Shows `text` in the status strip for `opts.seconds` (default 4, max 15), prefixed with the plugin name. |

The status strip sits above the canvas viewport, so no native view covers it. That makes it
the cheap toast. No modal UI from plugins in v2.

### Other v1 calls, now served

`process.exec` runs on GPUI's background executor, with `timeoutMs` default 30 s and
stdout and stderr capped at 4 MiB each (`too_large` beyond). `github.api` runs `GhClient`
on the background executor. `openExternal` allows `http`, `https` and `mailto` only.
`theme.get` returns `{ background }` from app settings.

## Events

`calm.events.on(event, handler)` as in v1, with these additions. The prelude tells the host
when the first handler for an event registers and when the last one leaves
(`events.subscribe` and `events.unsubscribe`, internal wire methods). The host only sends a
page the events it subscribed to, so an idle plugin costs nothing.

| Event                 | Payload                                                  | Coalescing                      |
| --------------------- | -------------------------------------------------------- | ------------------------------- |
| `panelAdded`          | `PanelInfo`                                              | none                            |
| `panelRemoved`        | `{ id }`                                                 | none                            |
| `panelChanged`        | `PanelInfo` plus `changes: ("rect"\|"title"\|"minimized"\|"z"\|"mode"\|"url")[]` | per panel, latest wins, 100 ms |
| `panelFocused`        | `{ id: PanelId \| null }`                                | per frame, latest wins          |
| `cameraChanged`       | `CameraInfo`                                             | latest wins, 100 ms, trailing edge always sent |
| `appSettingsChanged`  | `AppSettings`                                            | none                            |
| `terminalExited`      | `{ id }`                                                 | none                            |
| `command`             | `{ command, panelId?, world? }` (from `contributes`)     | none                            |
| `bus`                 | used by `calm.bus.subscribe`, not public                 | none                            |

v1 events `focus`, `blur`, `resize`, `visibility`, `settingsChanged`, `themeChanged` get
their senders in the same phase. `resize` and `visibility` use the same coalescing as
`panelChanged`.

### Coalescing model

`calm_plugins::events::Coalescer` is pure and takes `Instant` as a parameter, so tests drive
time. Keys are `(event, panel id or none)`. Each key holds only its latest payload. A key
flushes when its window (100 ms) has passed since its last send. A trailing flush runs after
the stream goes quiet, so the final camera position and the final drag rect always arrive.
Discrete events (`panelAdded`, `panelRemoved`, `terminalExited`, `command`) are never
merged and keep their order relative to each other. A `panelChanged` for a panel is dropped
if a `panelRemoved` for it is queued.

The host calls `Coalescer::due(now)` once per frame after paint and once from a 100 ms
timer that runs only while keys are pending. One `eval_js` per page per flush carries a
batch: `window.__calm.emitBatch([[event, payload], ...])`.

### Performance budget

Targets to verify in `docs/benchmarks.md`, not measured claims:

- Continuous pan or zoom: at most 10 `cameraChanged` per second per subscribed page.
- Dragging one panel: at most 10 `panelChanged` per second for that panel.
- Host work for bridge dispatch plus event flush under 1 ms per frame at the 20 terminal,
  5 plugin workload.
- Zero `eval_js` calls for a page with no subscriptions.

The smoke records the count of event batches sent in its window, so a regression that
floods shows up as a number.

## Contributions

`contributes.commands` declares ids and titles. `menus.canvas` adds items to the native
canvas menu under a submenu named after the plugin. `menus.panel` adds items to the panel
menu; `when` takes `kind == terminal|browser|plugin` only, nothing richer in v2.
`keybindings` binds a key in the `Canvas` key context only, never while a terminal or
browser has keyboard focus, so a plugin cannot steal keys from a shell. A binding that
collides with a built-in or with an earlier plugin is skipped and reported on stderr and in
the toolbar error count.

Choosing an item sends a `command` event to the plugin. Where it goes depends on open
question 3: to the plugin's background page, or to its most recently focused panel, opening
a new panel if none is open.

The NSMenu callback returns the chosen id to the GPUI side like today; only the GPUI task
emits the event.

## Errors

The host rejects with a structured value: `window.__calm.resolve(id, false, { code, message })`.
The prelude turns it into an `Error` with a `code` property. A string value (v1 shape) still
works and gets `code: "host_error"`.

| Code                 | When                                                              |
| -------------------- | ----------------------------------------------------------------- |
| `unknown_method`     | Not in `BRIDGE_METHODS`                                           |
| `not_implemented`    | Known but not served by this host                                 |
| `invalid_params`     | Missing, wrong type or out of range; message names the field      |
| `capability_missing` | Not declared; message names the capability to add                 |
| `not_found`          | No panel with that id                                             |
| `wrong_kind`         | Terminal method on a browser, and so on                           |
| `not_alive`          | Terminal process exited                                           |
| `busy`               | Another close confirmation is pending                             |
| `unavailable`        | Data the host cannot read yet (Ghostty title before the shim change) |
| `too_large`          | Payload, storage or output over its limit                         |
| `storage_corrupt`    | Plugin data file does not parse                                   |
| `gh_not_installed`, `gh_not_logged_in`, `gh_failed`, `gh_invalid_path` | Existing `gh` messages, now with codes |
| `timeout`            | `process.exec` ran past `timeoutMs`                               |
| `host_error`         | Anything else; message says what                                  |

## Rust types

New module `crates/calm-plugins/src/api.rs`. `calm-plugins` gains a dependency on
`calm-core` (both are pure, no GPUI) for geometry and scene types.

```rust
pub const API_VERSION: u32 = 2;

pub enum ApiRequest {
    Log { message: String },
    AppInfo, AppGetSettings, AppSetSettings(SettingsPatch),
    PanelSelf(SelfOp),                       // setTitle, getBounds, minimize, close, info, setRect
    PanelSettingsGet, PanelSettingsSet(Value),
    PanelsCreate(PanelSpec), PanelsList(PanelFilter), PanelsGet(PanelId),
    PanelsOp(PanelId, PanelOp),              // focus, move, resize, setRect, setTitle, ...
    PanelsClose(PanelId),
    CameraGet, CameraSet(CameraPatch), CameraCenterOn(Point), CameraFit(Option<Vec<PanelId>>, f64),
    TerminalInfo(PanelId), TerminalWrite(PanelId, String),
    BrowserInfo(PanelId), BrowserNavigate(PanelId, String), BrowserReload(PanelId),
    StorageGet(String), StorageSet(String, Value), StorageRemove(String), StorageKeys,
    BusPublish(String, Value), BusSubscribe(String), BusUnsubscribe(String),
    EventsSubscribe(String), EventsUnsubscribe(String),
    UiNotify(String, u32),
    ProcessExec(ExecSpec), GithubApi(String), OpenExternal(String), ThemeGet,
}

impl ApiRequest {
    /// v1 aliases map here too. Errors carry `ErrorCode::InvalidParams` and the field.
    pub fn parse(method: &str, params: &Value) -> Result<ApiRequest, ApiError>;
    pub fn capability(&self, caller: PanelId) -> Option<Capability>;
}

pub enum Capability { PanelsCreate, PanelsControl, PanelsClose, CameraControl,
    TerminalRead, TerminalWrite, BrowserControl, SettingsWrite, ProcessExec, Github, OpenExternal }

pub struct ApiError { pub code: ErrorCode, pub message: String }

pub fn check(granted: &CapabilitySet, request: &ApiRequest, caller: PanelId) -> Result<(), ApiError>;

/// Pure snapshot from the scene plus the host's per-panel extras (mode, focus, url).
pub fn panel_info(scene: &Scene, id: PanelId, extras: &PanelExtras) -> Option<PanelInfo>;
pub fn camera_info(camera: &Camera, viewport: Size) -> CameraInfo;
```

`BridgeResponse::Err` changes from `(u64, String)` to `(u64, ApiError)`; `to_js` renders the
object. `BRIDGE_METHODS` grows to the full v2 list and stays the single source of truth, with
the existing tests keeping it, the prelude and `calm.d.ts` in sync.

The host side in `plugin_host.rs`:

```rust
pub enum HostOutcome {
    Reply(Value),                 // settle now
    Pending(PendingReply),        // close confirmation, process.exec, github.api
    NoReply,                      // log
}
fn dispatch(view: &mut CanvasView, caller: PanelId, request: ApiRequest,
            window: &mut Window, cx: &mut Context<CanvasView>) -> Result<HostOutcome, ApiError>;
```

`PendingReply` is a `(PanelId, request id)` held by the view. Close confirmations settle it
from the Enter or Escape handler. Background work settles it from a GPUI task. A page that
closes drops its pending replies; nothing is evaluated in a dead web view.

## Threading model

- WebKit calls the script message handler on the main thread, inside an AppKit callback.
  GPUI may be mid-update at that moment, so the handler does one thing: `unbounded_send`
  of `(PanelId, String)`. It never touches the scene, the view or another web view. PR #5
  already does this; v2 keeps it.
- The same rule covers every new AppKit callback: `BrowserEvent` (title, url), NSMenu
  results for contributed items, and Ghostty title and pwd if the shim gains them. Each
  sends into a channel; the GPUI task applies it and feeds the coalescer.
- Dispatch runs in the GPUI task that drains the channel, with `&mut CanvasView`. It may
  create native views (`NativeChild::new` needs the window), so the drain task passes the
  window through `this.update_in`. If that is not available on `gpui-pre 0.3.4`, panel
  creation is queued as an `Effect::CreatePanel` and applied at the next render, which
  already has the window.
- `eval_js` replies run in the GPUI task. WebKit runs them asynchronously, so a reply never
  re-enters the dispatcher in the same stack.
- Blocking work (`process.exec`, `gh api`, storage file I/O over 64 KiB) runs on the
  background executor and settles through a `PendingReply`.
- Order: requests from one page are dispatched in arrival order. Replies for pending
  requests can arrive out of order; the prelude matches by id.

## Security rules that still hold

- No credential reaches plugin JS. `gh` keeps the token, `github.api` returns JSON only.
  `app.getSettings` has no secret fields. `terminals.info` does not include `env`.
  `process.exec` output is whatever the program prints, which is the user's trusted plugin
  asking the user's machine.
- Browser panels never get the `calm` message handler or the prelude. `browsers.navigate`
  loads a URL; nothing a plugin does injects script into a browser panel.
- Plugins are trusted. Docs say capabilities are a declaration for clarity, not a sandbox.
- A plugin cannot end a live terminal process without the user's confirmation, unless the
  user has turned `confirm_close` off, and it cannot turn that setting off.
- The host never logs request params for `terminals.write`, `storage.set`, `bus.publish` or
  terminal `env`. Logs carry the method and the error code only.

## Versioning and feature detection

- Manifest `apiVersion` (integer, default 1). A manifest above `API_VERSION` is skipped with
  `requires API v<n>, host has v<m>`.
- `calm.app.info()` returns `apiVersion` and `methods`. The host also injects
  `window.__calmHost = { apiVersion, methods }` as a document start script before the
  prelude, so `calm.has("terminals.write")` is synchronous and works before any call.
- Additive only. A method never changes shape; a new shape gets a new name. Removing a
  method needs a new major `apiVersion` and a decisions entry.
- The SDK `calm.d.ts` gets `apiVersion` in a header comment, and the conformance test reads
  it.

## Phased implementation

Each phase ends with `cargo test` for the touched crates and lands as its own commit.

1. **Error codes and manifest.** `ApiError`, `ErrorCode`, structured rejects in the prelude
   with v1 string fallback. Manifest `apiVersion`, `capabilities`, `background`,
   `contributes` parsing and validation. Capability table and `check`. Pure, tested.
2. **Request parsing.** `ApiRequest::parse` for every method and v1 alias, with a test per
   method for a good and a bad param. `BRIDGE_METHODS`, prelude, `calm.d.ts` extended;
   conformance tests updated.
3. **Read-only host.** `app.info`, `app.getSettings`, `theme.get`, `panels.list`,
   `panels.get`, `panel.info`, `panel.getBounds`, `camera.get`, `browsers.info`,
   `terminals.info` (spawn values, `title: null`). Snapshot builders in calm-plugins.
4. **Control.** `panels.create` (all kinds), focus, move, resize, setRect, setTitle,
   minimize, restore, z-order, `panel.*` on self, `camera.set`, `centerOn`, `fit`,
   `browsers.navigate`, `reload`, `terminals.write`, `app.setSettings`, `ui.notify`.
5. **Close.** `panels.close` and `panel.close` through `request_close`, `PendingReply`
   settled from Enter and Escape, `busy`. Regression test: a plugin close of a live terminal
   with `confirm_close` on does not remove the panel until Enter.
6. **Events.** Subscribe and unsubscribe, `Coalescer` with injected time, batch emit, all
   v1 and v2 senders. Tests for latest wins, trailing flush, removal drops pending change,
   discrete ordering.
7. **Storage, bus, process, github, openExternal.** Atomic storage file with limits and
   corrupt-file error. Bus fan out. Background executor for exec and gh.
8. **Contributions.** Commands, menu items, key bindings, `command` event routing per open
   question 3.
9. **SDK, docs, examples, smoke.** `calm.d.ts`, `docs/plugins.md`, `templates/plugin-tsx`,
   an example plugin that spawns a terminal and a browser, lists panels and reacts to
   `panelAdded`. Smoke fixture plugin: create a terminal through the bridge, list panels and
   assert the new id with kind `terminal`, receive `panelAdded` for it, report each through
   `log` lines the smoke parses, exit nonzero on any miss.

Optional phase 10, only if the user approves open question 1: CALM-owned patch of the
gpui-libghostty shim for `SET_TITLE` and `PWD`, `terminals.info` title and live cwd,
`panelChanged` with `title` for terminals.

## Open questions for the user

1. **Terminal title and live cwd need a patched Ghostty shim.** gpui-libghostty 0.3.1 drops
   `SET_TITLE` and `PWD` in `runtime_action`. Options: a CALM-owned fork of the crate
   through `[patch.crates-io]` that forwards them through a channel (more upkeep on every
   upstream bump), or ship v2 with spawn values only and `title: null`. Recommendation: ship
   without, add the patch as phase 10 if the user wants it.
2. **Custom background colors.** `Settings.background` is a preset enum. Allowing any
   `#rrggbb` from plugins (and the Settings menu) needs a settings format change to version
   2 with a migration. Recommendation: allow it, presets stay as named shortcuts.
3. **Background pages.** Contributed menu items, key bindings, events and bus messages need
   a running page. Options: (a) an opt-in hidden page per plugin (`"background": true`)
   started at launch, one WKWebView each; (b) no background pages, commands go to the
   plugin's last focused panel and open one if none exists. Recommendation: (a) opt-in,
   because "react to events" and "influence the app" mostly mean a plugin with no visible
   panel.
