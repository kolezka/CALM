# CALM plugin API v2 design

Status: design for review, with the user's decisions of 2026-10-31 folded in. Nothing
here is implemented. Extends the plugin section of
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
- `calm_core::settings::Settings`, schema version 2 from PR #7 (`feat/settings-modal`,
  not merged yet): `background` (`calm_core::background::BackgroundSettings`: `kind` solid,
  gradient or image; `solid` color; `gradient` linear or radial with 2 to 6 stops and an
  angle; `image` with path, fit and filters), `grid`, `snap`, `panels` (`confirm_close`,
  `terminal_user_config`, default terminal size, `preview_below_zoom`). `Settings::validate`
  checks every range and names the field. Version 1 files migrate on read.
  `feat/grid-styles` adds grid kinds on top. The API targets this schema, not the v1
  preset enum, which survives only for migration.
- `calm_core::menu`: Add submenu, panel actions (PR #2 adds zoom to fit, bring to front,
  send to back, duplicate, delete).
- Browser panels (PR #6, `calm-native/src/browser.rs`): `BrowserView::open`, `url()`,
  `BrowserEvent::{TitleChanged, UrlChanged, Clicked}`. No `calm` message handler.
- Terminals: `NativeSurface::text(&CStr)` exists and is public. Ghostty emits
  `GHOSTTY_ACTION_SET_TITLE` and `GHOSTTY_ACTION_PWD`, but the gpui-libghostty 0.3.1 shim
  (`shim/ghostty_surface.m`, `runtime_action`) handles only `RENDER` and drops the rest.
  Terminal title and cwd are not observable today.

## Goals

- Plugin panels are free-form UI surfaces: any HTML, CSS, JS or TSX the author wants.
- Plugins create and control every panel kind: terminals, browsers, other plugin panels.
- Plugins get a native browser API (full control of CALM browser panels), a native
  terminal API (input, keys, screen and scrollback text, output, process, title, cwd,
  scrolling, selection, font size) and the API of other plugins (RPC).
- Plugins read a full picture of the canvas: panels, focus, camera, app settings, version,
  browser url and title, terminal metadata when Ghostty exposes it.
- Plugins influence the app: move the camera, navigate browsers, type into terminals,
  add context menu items and key bindings, change the background, show a short notice.
- Plugins react to changes through events that stay cheap during pan and zoom.
- Plugins talk to each other and keep their own data.
- The manifest says what a plugin does, so the user can read it before installing.
- v1 plugins keep working unchanged.

## Non-goals

- A sandbox, a permission system or any gate on what an installed plugin may do. The
  user decided: plugins may use the full Node and Bun API, and what someone installs is
  their business. Capabilities are a declaration shown to the user, never enforced.
- Giving browser panels the bridge. Remote pages stay without host access. A plugin with
  `browser.script` reaches into a page; the page never reaches the host.
- Exposing CALM's own secrets to plugin JS. The GitHub token stays behind `gh api` on the
  host, and the control socket token never reaches JS. Site sessions in browser panels and
  text printed in terminals are a different matter, see the threat model.
- A raw byte stream of terminal output. libghostty's embedding API has no output callback
  (see the terminal table); output events are text diffs.
- Closing a live terminal without the user's confirmation.
- Remote or marketplace install, plugin updates, hot reload of manifests.
- Layout save and load (Task 13). The API leaves room for it but does not depend on it.

## Principles

1. **Calls are attributed.** Every request carries a `Caller`, set by the transport, never
   by a param. `Caller::Panel` is a plugin page: the WebKit channel tags each message with
   the calling `PanelId`, and the host resolves the plugin id through the scene.
   `Caller::Backend` is a plugin's backend process. `Caller::External` is a client of the
   control socket (phase 11). "Self" methods, event delivery and the external allowlist
   all take the `Caller`, so one dispatcher serves every transport. Only `External` is ever
   refused for policy reasons.
2. **One dispatcher, pure core.** Parsing, the external allowlist, snapshots and coalescing are
   pure Rust with unit tests. `plugin_host.rs` only applies effects to GPUI state.
3. **Never re-enter GPUI from an AppKit callback.** WebKit and NSMenu callbacks only push
   into a channel. All scene, camera and native view work happens in the GPUI task.
4. **Fail loud.** Every request settles. Unknown, malformed, undeclared and unserved calls
   reject with a code. No silent defaults.
5. **Additive versioning.** New methods and events only. A v1 method keeps its name and
   shape forever; v2 adds new names next to it.

## Plugin panels are free-form UI surfaces

*[lead's reading, confirm in review]*

A plugin panel is a full WKWebView with the normal web platform: DOM, CSS, `fetch`,
canvas 2D, WebGL, Web Workers, `localStorage` and IndexedDB, media, and anything else
WebKit on macOS 14 gives a page loaded from a local file. CALM imposes no layout, no
widget set and no styling. The panel chrome strip (title, status, buttons) is the only CALM
UI on a plugin panel; everything inside the content rect is the plugin's. Authors use plain
HTML and JS, or TSX compiled to JS (Preact in the template, any framework works).

Two limits come from the platform, not from CALM: the page loads from `file://` with read
access to its own plugin folder only, and web storage lives in the plugin web views' data
store, not in `calm.storage`. A plugin that wants data kept with CALM and visible to its
background page uses `calm.storage`.

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
  "main": "dist/main.js",
  "backend": { "start": "on-demand", "restart": "on-failure" },
  "engines": { "bun": ">=1.2" },
  "exports": [
    { "name": "spawnAgents", "description": "Open n agent terminals", "uses": ["panels.create"] }
  ],
  "requires": { "github-prs": "^0.1.0" },
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
| `capabilities` | Declarative. Strings from the capability table, shown to the user. Never a gate. |
| `main`         | Backend entry run under bun, relative to the plugin folder. See Plugin backend. |
| `backend`      | `start`: `on-demand` (default) or `at-launch`; `restart`: `on-failure` (default) or `never`. |
| `engines.bun`  | Semver range of bun the backend needs. Checked before start.                  |
| `background`   | Opt-in hidden page. Default false. Mostly replaced by `main`, see below.      |
| `exports`      | Methods other plugins may call through `calm.plugins.call`. See plugin RPC.   |
| `requires`     | Plugin ids with a semver range of their `version`. See plugin RPC.            |
| `contributes`  | Commands, menu entries and key bindings. Requires `apiVersion` 2.             |

**Background pages (decided: opt-in; now secondary to backends).** A plugin with
`"background": true` gets one hidden
WKWebView, started at launch, never placed on the canvas, with the same prelude and bridge
as its panels. It serves contributed commands, events, bus messages and RPC calls when no
panel of the plugin is open. Its `Caller` is `Panel` with a reserved per-plugin id that is
never in the scene, so `panel.*` self methods reject with `wrong_kind`. Cost: one web
content process per such plugin, so it stays opt-in. With backend processes (below), the
recommendation is to use `main` for headless work and keep background pages only for
headless code that needs the web platform (DOM, WebAudio, canvas rendering). They move to
the last phase and are built only if a plugin needs one.

Validation stays in `PluginManifest::validate` and reports the path and field, like every
other manifest error. A broken `contributes` block skips the plugin, it does not half load.

## Capabilities

**Reversed: capabilities are declarative only.** The earlier draft rejected undeclared calls
with `capability_missing`. The user decided plugins are unrestricted ("plugins may use the
full node/bun API, we don't restrict, what someone installs and uses is their business"),
and a plugin with a backend can do anything the user can without asking CALM at all. A gate
on the bridge would only inconvenience honest plugins. So `capabilities` is a list the
author writes to tell the user what the plugin does. CALM shows it at install (the plugin
list and a plugin info view) and never checks a call against it. There is no
`capability_missing` error.

Discovery warns, without skipping the plugin, when `capabilities` names an unknown string, so
typos still surface. A plugin with a backend gets a fixed note in its info view: "runs a
background program with full access to your files, network and programs".

The table names what each word stands for:

| Capability        | Methods                                                                     |
| ----------------- | --------------------------------------------------------------------------- |
| (no word needed)  | `log`, `app.info`, `plugins.list`, `plugins.serve`, `panel.*` on the plugin's own panel, `settings.*` (own panel), `storage.*`, `panels.list`, `panels.get`, `camera.get`, `app.getSettings`, `theme.get`, `bus.*`, `events.*`, `ui.notify`, `backend.*` |
| `panels.create`   | `panels.create`, `canvas.createPanel`                                       |
| `panels.control`  | `panels.focus`, `move`, `resize`, `setRect`, `setTitle`, `minimize`, `restore`, `bringToFront`, `sendToBack` on other panels; `canvas.focusPanel` |
| `panels.close`    | `panels.close` on other panels                                              |
| `camera.control`  | `camera.set`, `camera.centerOn`, `camera.fit`                               |
| `terminal.read`   | `terminals.info`, `readScreen`, `readScrollback`, `readSelection`, `terminalOutput` and other terminal content events |
| `terminal.write`  | `terminals.write`, `terminals.sendKeys`                                     |
| `terminal.control`| `terminals.clear`, `scroll`, `selectAll`, `copy`, `setFontSize`, `jumpToPrompt` |
| `browser.control` | `browsers.info`, `navigate`, `reload`, `back`, `forward`, `history`, `setUserAgent`, `setZoom`, `stop` |
| `browser.script`  | `browsers.eval`, `addUserScript`, `addUserStyle`, `removeUserScript`, `readText`, `readHtml`, `snapshot`, `setNavigationRules`, `onNavigate` hook, page-to-plugin messages |
| `browser.data`    | `browsers.cookies.*`, `browsers.clearData`                                  |
| `plugins.call`    | `plugins.call`                                                              |
| `settings.write`  | `app.setSettings`                                                           |
| `process.exec`    | `process.exec`                                                              |
| `github`          | `github.api`                                                                |
| `open.external`   | `openExternal`                                                              |
| `backend`         | implied by `main`: a bun process with the full Node and Bun API             |

The browser table's "Capability" column below uses these words as labels, not checks.

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

export type Side = "right" | "below" | "left" | "above";

/** Where a new panel goes. Give `rect`, or `near` (optionally with `side` and `size`), or
 *  neither. Both `rect` and `near` is `invalid_params`. */
export interface Placement {
  rect?: Rect;
  near?: PanelId;
  side?: Side;                          // default "right"
  size?: Size;                          // default: the kind's default size
}

export type PanelSpec =
  | ({ kind: "terminal"; command?: string; args?: string[]; cwd?: string;
       env?: Record<string, string>; title?: string; focus?: boolean } & Placement)
  | ({ kind: "browser"; url: string; title?: string; focus?: boolean } & Placement)
  | ({ kind: "plugin"; pluginId: string; settings?: Record<string, unknown>;
       title?: string; focus?: boolean } & Placement);

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
  capabilities: string[];               // as declared; informational
  methods: string[];                    // methods this host serves
}

// The settings.json v2 shape, exactly as serde writes it (snake_case keys). The SDK
// mirrors calm_core::settings::Settings; grid fields follow whatever feat/grid-styles lands.
export type Rgb = string;               // "#rrggbb"
export interface GradientStop { color: Rgb; position: number }   // position 0..1
export interface BackgroundSettings {
  kind: "solid" | "gradient" | "image";
  solid: Rgb;
  gradient: { kind: "linear" | "radial"; angle_deg: number; stops: GradientStop[] };
  image: {
    path: string | null;
    fit: "cover" | "contain" | "tile" | "center";
    filters: { blur: number; brightness: number; contrast: number; saturation: number;
               grayscale: boolean; dim: number };
  };
}
export interface AppSettings {
  version: number;                      // 2
  background: BackgroundSettings;
  grid: Record<string, unknown> & { show: boolean };
  snap: { enabled: boolean; grid: number; edge_distance: number };
  panels: { confirm_close: boolean; terminal_user_config: boolean;
            default_terminal_width: number; default_terminal_height: number;
            preview_below_zoom: number };
}
export type DeepPartial<T> = { [K in keyof T]?: T[K] extends object ? DeepPartial<T[K]> : T[K] };

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
| `getSettings()`          | `app.getSettings` | `AppSettings`, the full v2 settings object                     |
| `setSettings(patch)`     | `app.setSettings` | `DeepPartial<AppSettings>`. Resolves with the merged result. Writes `settings.json`. See below. |
| `calm.has(method)`       | none              | Sync. True if `method` is in the served list. Feature detection. |

`setSettings` applies the patch to the current settings as a JSON merge patch (RFC 7386):
objects merge key by key, arrays such as `background.gradient.stops` are replaced whole,
`null` resets a field to its default. The host then deserializes the result into
`calm_core::settings::Settings` and runs `Settings::validate`. Any failure rejects with
`invalid_params` and the field name from `validate`, and nothing is written or applied. On
success the host saves through the same atomic save path the Settings window uses, applies
the result live and emits `appSettingsChanged` to every subscribed page. This covers solid
colors, linear and radial gradients and image backgrounds with filters, with no extra
plugin-only background format.

Rules on top of `validate`:

- `panels.confirm_close` is **not** writable by plugins: a plugin that turns off
  confirmation would defeat the close rule. A patch that touches it rejects with
  `invalid_params`, even when the value is unchanged. `version` is not writable either,
  nor is the `external` block of phase 11, which only the Settings window changes.
- `background.image.path` must be an absolute path to an existing regular file that the
  host can decode as an image. CALM reads it in place and does not copy it, as the Settings
  window does. A bad path rejects before anything is saved.
- Unknown keys reject, so a typo does not silently do nothing. When `feat/grid-styles`
  lands, its grid fields become writable because they are part of `Settings`, with no API
  change.
- Writes are coalesced: a plugin animating the background may call `setSettings` often,
  but the file is saved at most once per second, with a trailing save. Live apply is not
  delayed.

`theme.get` (v1) keeps its shape and returns `{ background: base_color }`, where
`base_color` is `BackgroundSettings::base_color()`, the flat color that stands for a
gradient or image background.

### `calm.panel` (the calling panel, v1, now served)

`setTitle`, `getBounds`, `minimize`, `close` as in v1. Added: `info()` (`panel.info`,
`PanelInfo` of itself), `setRect(rect)` (`panel.setRect`). `close` on itself never needs
confirmation: a plugin page has no process the user can lose.

### `calm.panels`

| Method                    | Wire                  | Result and rules                                                    |
| ------------------------- | --------------------- | ------------------------------------------------------------------- |
| `create(spec)`            | `panels.create`       | `PanelId`. Placement rules below. Rect is clamped to `MIN_PANEL_SIZE`. |
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

**Placement.** `rect` places the panel exactly. `near: id` places it beside panel `id`:
`right` puts it at the target's right edge plus a 16 world unit gap, top edges aligned;
`below` at the bottom edge plus the gap, left edges aligned; `left` and `above` mirror
those. The size is `size`, or the kind's default (terminal default from
`panels.default_terminal_width/height` in settings, browser 800x600, plugin from its
manifest). The result is snapped to the snap grid when snapping is on. A minimized target
counts with its full rect. There is no collision avoidance in v2: the result is predictable
from the inputs, and a caller that wants a free spot reads `panels.list` first. A missing
target is `not_found`. With neither `rect` nor `near`, the panel goes to the viewport
center at its default size. The math is a pure function in calm-core,
`placement::place_near(target: Rect, side, size, gap) -> Rect`, with a test per side.
`near` works without `command`, so a caller can place a plain shell next to a panel.

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

### `calm.terminals` (native terminal API)

*[lead's reading, confirm in review]*

Every method takes a terminal `PanelId` and rejects `wrong_kind` for another kind and
`not_alive` once the process has exited (except `info`, `readScreen`, `readScrollback`).

**Evidence.** CALM drives Ghostty through gpui-libghostty 0.3.1. Its Rust surface
(`src/native.rs`) and ObjC shim (`shim/ghostty_surface.m`) expose only: new, free, tick,
is_alive, frame_count, update_theme, snapshot, set_frame, set_visible,
set_hidden_rendering, set_focus, key, text, mouse_position, mouse_button, mouse_scroll.
The shim's `runtime_action` handles only `GHOSTTY_ACTION_RENDER` and drops every other
action. The vendored libghostty C API (`vendor/ghostty/include/ghostty.h`) offers more:
`ghostty_surface_read_text` with a `ghostty_selection_s` over `GHOSTTY_POINT_VIEWPORT`,
`_SCREEN` or `_ACTIVE`, `ghostty_surface_has_selection` and `_read_selection`,
`ghostty_surface_size` (columns, rows, cell size), `ghostty_surface_foreground_pid`,
`ghostty_surface_tty_name`, `ghostty_surface_process_exited`,
`ghostty_surface_binding_action(name)`, and the actions `SET_TITLE`, `PWD`,
`SHOW_CHILD_EXITED`, `COMMAND_FINISHED` (exit code and duration), `RING_BELL`,
`DESKTOP_NOTIFICATION`, `PROGRESS_REPORT`. The binding action names used below exist in
`vendor/ghostty/src/input/Binding.zig` (checked: `clear_screen`, `scroll_page_up`,
`scroll_to_top`, `scroll_to_bottom`, `scroll_page_lines`, `increase_font_size`,
`decrease_font_size`, `reset_font_size`, `set_font_size`, `select_all`,
`copy_to_clipboard`, `write_screen_file`, `jump_to_prompt`). There is no output callback
and no function to set a selection range or the terminal's own title.

"Shim patch" means phase 10: a CALM-owned fork of gpui-libghostty through
`[patch.crates-io]` that adds shim functions forwarding those C calls and queues the
dropped actions for CALM to drain on its tick (no callback into GPUI).

| Method                                   | Wire                        | Availability | How |
| ---------------------------------------- | --------------------------- | ------------ | --- |
| `info(id)`                               | `terminals.info`            | now, partial | spawn command and cwd, `alive` from `is_alive`; title, live cwd, pid, grid need the patch |
| `write(id, text)`                        | `terminals.write`           | now          | `NativeSurface::text`, typed input, max 64 KiB, NUL rejected |
| `sendKeys(id, keys)`                     | `terminals.sendKeys`        | now          | `NativeSurface::send_key` with press and release; `keys` like `["ctrl-c", "enter", "up"]` in GPUI keystroke syntax |
| `resize(id, size)` / `{ columns, rows }` | `panels.resize`             | now / patch  | world size through `set_frame` now; columns and rows need `ghostty_surface_size` for the cell size |
| `exited` state, `terminalExited` event   | `terminals.info`, event     | now          | `is_alive` polled on tick |
| exit code                                | `terminalExited.code`       | patch        | `SHOW_CHILD_EXITED` action; `null` until then |
| `readScreen(id)`                         | `terminals.readScreen`      | patch        | `read_text` over `GHOSTTY_POINT_VIEWPORT` |
| `readScrollback(id, { maxLines })`     | `terminals.readScrollback`  | patch        | `read_text` over `GHOSTTY_POINT_SCREEN`, capped at 4 MiB |
| `terminalOutput` event                   | event                       | patch, approximated | no output callback in libghostty; CALM diffs `read_text` of the active area at most every 250 ms while subscribed and sends appended or changed lines |
| raw output byte stream                   | none                        | not possible | Ghostty owns the PTY; no API delivers its bytes |
| title get, `terminalTitleChanged`        | `terminals.info`, event     | patch        | `SET_TITLE` action |
| title set (chrome)                       | `panels.setTitle`           | now          | CALM chrome title, overrides the Ghostty title in the strip |
| title set (inside Ghostty)               | none                        | not possible | no setter in the surface API; titles come from the program's output |
| cwd get, `terminalCwdChanged`            | `terminals.info`, event     | patch        | `PWD` action; needs shell integration (OSC 7) in the shell |
| cwd set                                  | none                        | not possible | process state; a plugin types `cd` through `write` |
| running process                          | `terminals.info.process`    | patch        | `ghostty_surface_foreground_pid` and `_tty_name`; name and path from the pid through `proc_pidpath` in calm-native |
| command finished                         | `terminalCommandFinished`   | patch        | `COMMAND_FINISHED` action, exit code and duration; needs shell integration |
| `clear(id)`                              | `terminals.clear`           | patch        | `binding_action("clear_screen")` |
| `scroll(id, to)`                         | `terminals.scroll`          | now, approximate / patch | `mouse_scroll` now; exact `top`, `bottom`, `{ pages }`, `{ lines }` through `scroll_to_top`, `scroll_to_bottom`, `scroll_page_up`, `scroll_page_lines` |
| `selectAll(id)`                          | `terminals.selectAll`       | patch        | `binding_action("select_all")` |
| select a range                           | none                        | not possible | no set-selection function; `read_text` of a range gives the text instead |
| `readSelection(id)`                      | `terminals.readSelection`   | patch        | `has_selection` and `read_selection` |
| `copy(id)`                               | `terminals.copy`            | patch        | `binding_action("copy_to_clipboard")`, through CALM's clipboard service |
| `setFontSize(id, points \| "+1" \| "-1" \| "reset")` | `terminals.setFontSize` | patch | `set_font_size`, `increase_font_size`, `decrease_font_size`, `reset_font_size` |
| `jumpToPrompt(id, delta)`                | `terminals.jumpToPrompt`    | patch        | `jump_to_prompt`; needs shell integration |
| bell, notification, progress             | events                      | patch        | `RING_BELL`, `DESKTOP_NOTIFICATION`, `PROGRESS_REPORT` actions |

A method that needs the patch rejects with `unavailable` until phase 10 lands, so plugins
can feature detect with `calm.has` or by the code.

Font size changes the terminal's grid (more or fewer columns in the same panel), like a
panel resize. It never changes camera zoom, and camera zoom never changes it.

`write` and `sendKeys` are input, not paste: `"ls\r"` runs `ls`. They work on any terminal,
not only ones the plugin created, because a plugin with `process.exec` already has that
power. The declared capability makes it visible.

### `calm.browsers` (native browser API)

*[lead's reading, confirm in review]*

**Decision reversed.** The earlier draft said "no script injection into browser panels and
no reading page content". The user asked for the native browser API, so a plugin with
`browser.script` may evaluate JS in a page, inject scripts and styles and read the page. The
reason for the old line stands as a rule that still holds: the page never gets the `calm`
bridge, so remote content cannot reach the host. Only the plugin reaches into the page.

Every method takes a browser `PanelId`. The calls map onto WKWebView API available on
macOS 14 (the app's floor). The objc2 bindings for each are to verify in phase 12.

| Method                                   | Wire                         | Capability       | WebKit                                   |
| ---------------------------------------- | ---------------------------- | ---------------- | ---------------------------------------- |
| `info(id)`                               | `browsers.info`              | `browser.control`| `URL`, `title`, `isLoading`, `estimatedProgress`, `canGoBack`, `canGoForward` |
| `navigate(id, url)`                      | `browsers.navigate`          | `browser.control`| `BrowserView::open`, address bar normalization |
| `reload(id)`, `stop(id)`                 | `browsers.reload`, `.stop`   | `browser.control`| `reload`, `stopLoading`                  |
| `back(id)`, `forward(id)`                | `browsers.back`, `.forward`  | `browser.control`| `goBack`, `goForward`                    |
| `history(id)`                            | `browsers.history`           | `browser.control`| `backForwardList`: back and forward items with url and title |
| `setUserAgent(id, ua \| null)`          | `browsers.setUserAgent`      | `browser.control`| `customUserAgent`                        |
| `setZoom(id, factor)`                    | `browsers.setZoom`           | `browser.control`| `pageZoom`, 0.25 to 5. Page zoom, not camera zoom |
| `eval(id, source, { world?, args? })`  | `browsers.eval`              | `browser.script` | `callAsyncJavaScript:arguments:inFrame:inContentWorld:`; result must be JSON, max 4 MiB, default timeout 10 s |
| `addUserScript(id, source, { at, allFrames, world? })` | `browsers.addUserScript` | `browser.script` | `WKUserScript`, document start or end; returns a handle |
| `addUserStyle(id, css)`                  | `browsers.addUserStyle`      | `browser.script` | a document start user script that adds a `<style>`; returns a handle |
| `removeUserScript(id, handle)`           | `browsers.removeUserScript`  | `browser.script` | `removeAllUserScripts`, then re-add the rest |
| `readText(id)`, `readHtml(id)`           | `browsers.readText`, `.readHtml` | `browser.script` | `eval` of `document.body.innerText` or `documentElement.outerHTML` in the isolated world, max 4 MiB |
| `snapshot(id, { rect? })`              | `browsers.snapshot`          | `browser.script` | `takeSnapshotWithConfiguration:`, PNG as a data URL, max 8 MiB |
| `setNavigationRules(id, rules)`          | `browsers.setNavigationRules`| `browser.script` | `decidePolicyForNavigationAction:` evaluated in Rust: `{ match, action: "allow" \| "deny" \| "external" }` |
| `onNavigate` hook                        | `browsers.setNavigationHook` | `browser.script` | the decision handler is held and the plugin answers through `browsers.decide`; no answer within 500 ms means allow |
| `cookies.get(id, { url? })`, `cookies.set`, `cookies.delete` | `browsers.cookies.*` | `browser.data` | `WKHTTPCookieStore` of the panel's data store |
| `clearData(id, types)`                   | `browsers.clearData`         | `browser.data`   | `WKWebsiteDataStore removeDataOfTypes:` (cookies, local storage, IndexedDB, cache) |

Content worlds. Plugin scripts run in a CALM content world (`WKContentWorld worldWithName:`)
by default, so page scripts cannot see or tamper with them. `world: "page"` runs in the
page's own world, for plugins that need page globals. A script in the CALM world can post
to a `calmPage` message handler registered only in that world; the message reaches the
plugin that injected it as a `browserMessage` event. The payload is page data and is
untrusted. The page's own world has no handler.

Data stores. Today browser panels share the default persistent store: `BrowserView::new` uses a plain `WKWebViewConfiguration::new` and never sets a data store (PR #6, `calm-native/src/browser.rs`). Phase 12
gives each browser panel the option of its own persistent store through
`WKWebsiteDataStore dataStoreForIdentifier:` (macOS 14+), set at creation:
`panels.create({ kind: "browser", url, dataStore: "shared" | "panel" | "<name>" })`.
`browser.data` acts on the panel's store; with the shared store it touches every panel that
shares it, and the docs say so.

Events: `browserNavigation { id, phase: "started" | "committed" | "finished" | "failed",
url, error? }` and `browserMessage { id, data }`; url and title changes also arrive as
`panelChanged` with `url` or `title`. These come from the `BrowserEvent` channel PR #6
already has, extended.

### `calm.plugins` (plugin to plugin RPC)

*[lead's reading, confirm in review]*

The bus is fan-out with no answer. Plugin RPC is a call with a result.

| Method                                     | Wire              | Result                                              |
| ------------------------------------------ | ----------------- | --------------------------------------------------- |
| `list()`                                   | `plugins.list`    | `PluginEntry[]`                                     |
| `call(pluginId, method, params?, { timeoutMs? })` | `plugins.call` | Promise of the callee's result               |
| `serve(method, handler)`                   | `plugins.serve`   | Sync. Registers a handler in this page for an exported method; returns an unregister function |

```ts
export interface PluginEntry {
  id: string; name: string; version: string; apiVersion: number;
  exports: { name: string; description?: string; uses: string[] }[];
  capabilities: string[];
  running: { backend: "stopped" | "starting" | "ready" | "failed"; background: boolean; panels: PanelId[] };
  requires: Record<string, string>;
}
```

Manifest `exports` lists the methods others may call: `name` matches
`[a-zA-Z][a-zA-Z0-9_.]*`, `uses` lists the capabilities the method uses on the callee's
side, for the caller's author and the user to read. It is not checked. `requires` maps plugin ids to semver ranges of their `version`. A plugin whose
`requires` is not met (missing plugin, version outside the range, non-semver version) is
skipped at discovery with an error naming the dependency, like any broken manifest. Optional
dependencies are not declared; the plugin checks `plugins.list()` instead.

Call flow:

1. No capability check. The earlier "capability checks apply to the caller" rule went
   with enforcement; `uses` is shown in the plugin info view instead.
2. The host picks the server: the callee's backend if it declares `main` (started on
   demand if it is not running, waiting up to 5 s for it to be ready), else its background
   page if it has registered `method`, else its most recently focused panel that has, else
   none. The host never opens a panel to serve a call.
3. The host sends the call to the backend over its IPC channel, or to a page as an internal
   event; the backend SDK or the prelude runs the handler and answers with an internal
   `plugins.reply`. The caller's Promise settles with the
   result, or with the callee's error.
4. Params and results are JSON, at most 1 MiB each. Default timeout 10 s, max 60 s.
5. If the serving page closes or the backend exits mid-call, the call rejects with
   `not_running`.

Errors: `not_installed` (no such plugin), `not_running` (exported, but no running page has
registered it), `method_not_found` (not in the callee's `exports`), `timeout`,
`remote_error` (the handler threw; `cause` carries its code and message). External callers
(phase 11) cannot call plugins.

### Plugin backend process

**User decision:** plugins may use the full Node and Bun API, unrestricted.

A plugin may declare `"main"` in its manifest. CALM runs that file as a child process under
bun, with the full Node and Bun API: `fs`, `child_process`, `net`, HTTP servers, native
modules, anything bun runs. CALM does not restrict, inspect or proxy what it does.

**Process.** `bun run <main>` with the plugin folder as working directory, in its own
process group, with stdin closed. Environment: the user's environment as CALM got it, plus
`CALM_PLUGIN_ID`, `CALM_PLUGIN_DIR`, `CALM_DATA_DIR` (`$CALM_HOME/plugin-data/<id>/`, the
backend's own folder), `CALM_API_VERSION` and `CALM_IPC_FD=3`. CALM's own secrets are not
in it: no socket token, no `gh` token. `main` follows the same path rules as `entry`
(relative, inside the folder, a regular file).

**Lifecycle.**

- `backend.start: "on-demand"` (default): started on the first of these: a panel of the
  plugin opens, a page calls `calm.backend.*`, a `plugins.call` targets the plugin, or a
  contributed command fires. `"at-launch"`: started when CALM starts.
- Ready when the backend's SDK sends its `hello` frame. Calls arriving before that wait up to
  5 s, then reject with `timeout`.
- Restart (`backend.restart: "on-failure"`, default): on a nonzero exit or a signal, restart
  after 1, 2, 4, 8 and 16 s. More than 5 failures within 60 s stops the retries; the
  backend is marked `failed`, the status bar error count goes up, and pages get
  `backendStatus { state: "failed", code, signal }`. A page call or a contributed command
  retries a failed backend once. A clean exit (code 0) is not restarted. `"never"` never
  restarts.
- Stop at quit: SIGTERM to the process group, SIGKILL after 3 s. Anything the backend
  spawned in its group goes too, consistent with "processes do not survive app quit". A
  backend that deliberately detaches its own children is its business.
- No idle stop in v2. A running backend runs until quit or until it exits.
- Logs: stdout and stderr go to `$CALM_HOME/logs/plugins/<id>.log` (rotated at 5 MiB, one
  old file kept) and to CALM's stderr prefixed `calm: plugin <id>:`. The log is the
  plugin's output; CALM does not filter it, and the docs tell authors not to print secrets.

**IPC.** An inherited socket pair on fd 3: no path on disk, no token, nothing another
process can connect to, and it closes when either side dies. Frames are newline-delimited
JSON with the same shapes as the page bridge: `BridgeRequest` (`{ id, method, params }`),
`{ id, ok, value | error }` and `{ event, payload }`. Max 1 MiB per frame. Fallback if bun
cannot use an inherited fd as a stream (to verify in phase 14): a per-plugin Unix socket at
`$CALM_HOME/run/plugins/<id>.sock`, directory 0700, socket 0600, path in
`CALM_IPC_SOCKET`, with a one-time nonce in the environment that the backend sends first.
stdio stays for logs, so a stray `console.log` never corrupts the channel.

**Host API from the backend.** The backend SDK module `@calm/plugin-sdk/backend` exports a
`calm` object with the same namespaces as `window.calm` (panels, camera, terminals,
browsers, app, storage, bus, plugins, ui, events), so a backend can spawn panels, drive
terminals and browsers and read the scene. Calls run as `Caller::Backend { plugin_id }`. The
`panel.*` self methods and per-panel `settings.*` reject with `wrong_kind`: a backend has no
panel. `process.exec` and `github.api` exist for symmetry, but a backend can just use
`child_process` and run `gh` itself.

**Pages and backend.**

```ts
// in a plugin page (window.calm)
calm.backend.call(method: string, params?: unknown, opts?: { timeoutMs?: number }): Promise<unknown>;
calm.backend.on(event: string, handler: (payload: unknown) => void): () => void;
calm.backend.status(): Promise<{ state: "stopped" | "starting" | "ready" | "failed" }>;

// in the backend (@calm/plugin-sdk/backend)
import { calm, pages } from "@calm/plugin-sdk/backend";
pages.handle(method: string, handler: (params: unknown, from: { panelId: PanelId }) => unknown | Promise<unknown>): () => void;
pages.emit(event: string, payload: unknown, opts?: { panelId?: PanelId }): void;  // all pages of the plugin, or one
calm.plugins.serve(method, handler);                                               // exports, see plugin RPC
```

Page to backend traffic goes through the host: page bridge, host, fd 3, and back. Only the
plugin's own pages reach its backend; other plugins go through `exports`. Errors:
`not_running` (backend failed or exited), `method_not_found` (no `pages.handle` for it),
`timeout` (default 10 s, max 60 s), `remote_error`. A backend crash rejects every pending
call with `not_running`.

**Backends and exports.** A backend serves `exports` with `calm.plugins.serve`, without any
hidden page. Recommendation: backends replace hidden background pages for most cases
(services, watchers, servers, RPC, contributed commands). Contributed commands go to the
backend first when `main` is declared. Background pages stay in the design for headless
code that needs the web platform, built last and only on demand.

**Bun runtime source.** For the user to decide. Two options:

| | A: bun on the user's system (recommended) | B: pinned bun inside CALM.app |
| --- | --- | --- |
| Setup | user installs bun once (`brew install oven-sh/bun/bun` or the install script) | none |
| App size | no change | tens of MB more per arch (measure before quoting) |
| Signing | nothing new | the nested binary must be signed and notarized with the app, once CALM is distributed signed |
| Updates | bun security fixes arrive when the user updates bun | fixes wait for a CALM release; CALM owns bun upkeep |
| Version drift | plugins may meet different bun versions; `engines.bun` catches mismatches | one known version, fewer surprises |
| Fits today | yes: first-party plugins already need bun to build, and the macOS zip is built locally | adds a download and packaging step to `gmake release-upload` |

Recommendation: **A**, with a fixed lookup order and a loud failure. Order: the
`plugins.bun_path` setting if set, then `PATH` from a login shell (`$SHELL -lc 'command -v
bun'`, run once at startup, because an app started from Finder does not get the shell
`PATH`), then `~/.bun/bin/bun`, `/opt/homebrew/bin/bun`, `/usr/local/bin/bun`. If none is
found, or `bun --version` is outside `engines.bun`, the backend does not start, the plugin's
panels show "bun not found: install it with `brew install oven-sh/bun/bun` or set
`plugins.bun_path`", and the status bar error count goes up. Reason: CALM's users are
developers, the toolchain already requires bun, and B adds size, signing and an upkeep duty
for little gain today. B stays possible later if CALM is distributed to non-developers; the
lookup order would then end with the bundled copy.

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
| `notify(text, opts?)`   | `ui.notify`  | Shows `text` in the floating status bar for `opts.seconds` (default 4, max 15), prefixed with the plugin name. |

The status bar (PR #4, `calm_core::status_bar`, drawn by `calm-native/src/status_bar.rs`)
is a native view that draws above terminals, so nothing covers it. That makes it the cheap
toast. No modal UI from plugins in v2.

### Other v1 calls, now served

`process.exec` runs on GPUI's background executor, with `timeoutMs` default 30 s and
stdout and stderr capped at 4 MiB each (`too_large` beyond). `github.api` runs `GhClient`
on the background executor. `openExternal` allows `http`, `https` and `mailto` only.
`theme.get` returns `{ background }`, the `#rrggbb` of `BackgroundSettings::base_color()`.

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
| `appSettingsChanged`  | `AppSettings`, full v2 object                            | latest wins, 100 ms             |
| `terminalExited`      | `{ id, code: number \| null }`                          | none                            |
| `terminalOutput`      | `{ id, lines: string[], replaced: boolean }` (patch)     | per terminal, 250 ms            |
| `terminalTitleChanged`, `terminalCwdChanged` | `{ id, title }`, `{ id, cwd }` (patch) | per terminal, latest wins, 100 ms |
| `terminalCommandFinished` | `{ id, code, durationMs }` (patch)                   | none                            |
| `terminalBell`        | `{ id }` (patch)                                         | per terminal, 1 s               |
| `browserNavigation`   | `{ id, phase, url, error? }`                             | none                            |
| `browserMessage`      | `{ id, data }`, from the plugin's own injected scripts   | none                            |
| `pluginsChanged`      | `PluginEntry[]` when a background page or panel starts or stops serving | latest wins, 100 ms |
| `command`             | `{ command, panelId?, world? }` (from `contributes`)     | none                            |
| `bus`                 | used by `calm.bus.subscribe`, not public                 | none                            |

v1 events `focus`, `blur`, `resize`, `visibility`, `settingsChanged`, `themeChanged` get
their senders in the same phase. `themeChanged` fires when `base_color()` changes, so a v1
plugin still learns about background changes made through the v2 settings. `resize` and `visibility` use the same coalescing as
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
- `terminalOutput`: one `read_text` of the active area per subscribed terminal per 250 ms
  at most, and none for terminals nobody subscribed to.

The smoke records the count of event batches sent in its window, so a regression that
floods shows up as a number.

## Contributions

`contributes.commands` declares ids and titles. `menus.canvas` adds items to the native
canvas menu under a submenu named after the plugin. `menus.panel` adds items to the panel
menu; `when` takes `kind == terminal|browser|plugin` only, nothing richer in v2.
`keybindings` binds a key in the `Canvas` key context only, never while a terminal or
browser has keyboard focus, so a plugin cannot steal keys from a shell. A binding that
collides with a built-in or with an earlier plugin is skipped and reported on stderr and in
the status bar error count.

Choosing an item sends a `command` event to the plugin's background page if it has one,
otherwise to its most recently focused panel, otherwise CALM opens a new panel of the
plugin and delivers it there once the page has loaded.

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
| `not_found`          | No panel with that id                                             |
| `wrong_kind`         | Terminal method on a browser, and so on                           |
| `not_alive`          | Terminal process exited                                           |
| `busy`               | Another close confirmation is pending                             |
| `unavailable`        | Needs the Ghostty shim patch (phase 10) or a later phase          |
| `too_large`          | Payload, storage or output over its limit                         |
| `storage_corrupt`    | Plugin data file does not parse                                   |
| `gh_not_installed`, `gh_not_logged_in`, `gh_failed`, `gh_invalid_path` | Existing `gh` messages, now with codes |
| `timeout`            | `process.exec`, `plugins.call` or `browsers.eval` ran past its limit |
| `not_allowed`        | External caller, method not in the external allowlist (phase 11)  |
| `not_installed`      | `plugins.call` to a plugin that is not installed                  |
| `not_running`        | `plugins.call` to an export no running page serves                |
| `method_not_found`   | `plugins.call` to a method not in the callee's `exports`          |
| `remote_error`       | The callee's handler threw; `cause` has its code and message      |
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
    Terminal(PanelId, TerminalOp),           // info, write, sendKeys, read*, clear, scroll, ...
    Browser(PanelId, BrowserOp),             // info, navigate, eval, scripts, cookies, ...
    PluginsList, PluginsCall { plugin: String, method: String, params: Value, timeout_ms: u32 },
    PluginsServe(String), PluginsReply { call: u64, result: Result<Value, ApiError> },
    StorageGet(String), StorageSet(String, Value), StorageRemove(String), StorageKeys,
    BusPublish(String, Value), BusSubscribe(String), BusUnsubscribe(String),
    EventsSubscribe(String), EventsUnsubscribe(String),
    UiNotify(String, u32),
    ProcessExec(ExecSpec), GithubApi(String), OpenExternal(String), ThemeGet,
}

impl ApiRequest {
    /// v1 aliases map here too. Errors carry `ErrorCode::InvalidParams` and the field.
    pub fn parse(method: &str, params: &Value) -> Result<ApiRequest, ApiError>;
    /// The capability word that describes this call, for the plugin info view and logs.
    pub fn capability(&self) -> Option<Capability>;
}

/// Set by the transport, never parsed from params.
pub enum Caller {
    Panel { panel: PanelId, plugin_id: String },
    Backend { plugin_id: String },            // a plugin's bun process
    External { client: ClientId },            // phase 11
}

pub enum Capability { PanelsCreate, PanelsControl, PanelsClose, CameraControl,
    TerminalRead, TerminalWrite, TerminalControl, BrowserControl, BrowserScript, BrowserData,
    PluginsCall, SettingsWrite, ProcessExec, Github, OpenExternal }

pub struct ApiError { pub code: ErrorCode, pub message: String }

/// Only `Caller::External` is checked: against the external allowlist from settings
/// (phase 11), rejecting with `not_allowed`. `Panel` and `Backend` always pass.
pub fn check(allowlist: &ExternalAllowlist, request: &ApiRequest, caller: &Caller) -> Result<(), ApiError>;

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
fn dispatch(view: &mut CanvasView, caller: &Caller, request: ApiRequest,
            window: &mut Window, cx: &mut Context<CanvasView>) -> Result<HostOutcome, ApiError>;
```

`PendingReply` is the caller plus the request id, held by the view. For a panel it settles
through `eval_js`; for a backend through a frame on its IPC channel; for an external client
(phase 11) through a oneshot sender back to the socket thread. Close confirmations settle it
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
- Each backend's IPC channel is read and written by one thread per backend (blocking I/O
  on fd 3). It only parses frames and sends `(Caller::Backend, BridgeRequest)` into the same
  channel; it never touches GPUI. Process spawn, wait and restart timers run on the
  background executor and report through the channel too.
- Order: requests from one page are dispatched in arrival order. Replies for pending
  requests can arrive out of order; the prelude matches by id.

## Security rules that still hold

- CALM does not hand its own secrets to plugins: no API result, page script or backend
  environment contains the GitHub token or the control socket token. `gh` keeps the GitHub
  token, `github.api` returns JSON only. `app.getSettings` has no secret fields.
  `terminals.info` does not include `env`. This is about CALM not leaking, not about
  stopping plugins: a backend can run `gh` itself, and it can read any file the user can,
  including the socket token file.
- Browser panels never get the `calm` message handler or the prelude. Plugin scripts run in
  a CALM content world the page cannot see. The `calmPage` handler exists only in that
  world.

### Threat model for plugins

**An installed plugin has the same power as any program the user runs.** With a backend it
is a program the user runs: it can read and write the user's files, use the network, start
processes, read the keychain items the user's login allows, and talk to CALM over the
bridge. Without a backend, a plugin page still reaches the host through `process.exec`.
CALM enforces nothing against installed plugins. Capabilities tell the user what the
author says the plugin does; they are not checked. The only protection is the user's choice
of what to install, which is why install stays explicit. What follows describes reach, so
the docs can say it next to each capability word:

- `browser.script` and `browser.data` expose the user's logged-in sessions on every site
  open in a browser panel to that plugin: it can read pages, act as the user on them, and
  read and set cookies, including session cookies. This is consistent with plugins being
  trusted, and it is a change from "no credentials reach plugin JS", which now means CALM's
  own secrets. The docs must say this next to the capability.
- `terminal.read` gives the plugin everything printed in any terminal, including secrets a
  program prints (tokens from CLI tools, `env` output, pasted keys). The host never logs
  read results.
- `terminal.write` and `process.exec` run commands as the user. `terminal.control` changes
  what the user sees but runs nothing.
- `plugins.call` lets one plugin use another's exports with no check; `uses` shows the
  reach in the info view.
- `backend` (any `main`): full user rights, as above.
- The host never logs params or results for `terminals.write`, `sendKeys`, `read*`,
  `browsers.eval`, `readText`, `readHtml`, `cookies.*`, `storage.set`, `bus.publish` or
  `plugins.call`.
- Plugins are trusted and unrestricted. Docs say capabilities are a declaration, not a
  sandbox and not a permission.
- The API never ends a live terminal process without the user's confirmation, unless the
  user has turned `confirm_close` off, and the API cannot turn that setting off. This is
  kept on purpose: it protects the user's running work from a plugin bug, not the user
  from the plugin. It is not a guarantee: a backend can find the shell's pid and kill it,
  or edit `settings.json`. Flagged for review, kept as the lead asked.
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

## External control channel (phase 11)

**Status: approved by the user on 2026-10-31. Build after PRs #5 and #6 merge.** Nothing
in phases 1 to 10 depends on it, apart from the `Caller` type, which v2 has from phase 1.
The allowlist follows PR #9 (`docs/superpowers/specs/2026-10-31-claude-code-mods-integration.md`
on `docs/mods-integration`). The first integration is agent status on terminal panels. The
`calm-bridge` Claude Code mod lives in this repo with a `marketplace.json`, so its version
moves with CALM.

### Why

Claude Code mods (in-process JS plugins of Claude Code) and scripts in a CALM terminal
want to drive CALM: report subagent status on a panel, place a new terminal next to the
current one, push lifecycle events to a board plugin. A mod can only call out
(`$.http.fetch` with `socketPath`, a 30 s limit per fetch) or spawn a helper. It cannot
listen. So CALM is the server.

### Threat model

Stated plainly: **any process running as the user that can read the token file can drive
CALM.** With the defaults below that means reading the canvas, moving panels and the
camera, and setting panel status. If the user turns on `run`, it also means spawning
terminals with any command and env, **typing into any terminal** and `process.exec`, which
is running arbitrary commands as the user. The channel adds no power a process
running as the user does not already have (it can run commands itself), but it reaches
into running shells, which is exactly what the user sees and trusts. It is not a defense
against malware already running as the user, and docs must not suggest it is.

What it does defend against: other users on the machine (directory 0700, socket and token
0600, peer uid check), stale tokens (rotated every launch), accidental exposure (off by
default, never on a TCP port, never in env, never in logs).

### Enabling

Settings gain an `external` block, edited in the Settings window. Plugins cannot write it:
`app.setSettings` rejects any patch that touches `external`, like `panels.confirm_close`.

```json
"external": { "enabled": false, "allow": ["read", "status", "events"] }
```

When `enabled` is false, no socket and no token file exist, and terminals get no `CALM_*`
env.

### Transport and token lifecycle

- Socket path (per PR #9): `$CALM_HOME/run/ctl.sock`, default `~/.config/calm/run`. The
  directory is created 0700 under `umask 0177`. If it exists, CALM checks that it is not a
  symlink, that the owner is the current uid and the mode is 0700, and refuses to start the
  channel otherwise (logged, shown in the status bar error count). A path over the 104 byte
  `sun_path` limit is refused with a message naming the limit and the path.
- At launch: remove a stale socket and token in that directory, generate 32 random bytes
  from the OS RNG, write them hex encoded to `<dir>/token` with `create_new` and mode 0600,
  then bind the socket and chmod it 0600. At quit: remove both. A crash leaves them; the
  next launch replaces them, so an old token never works against a new CALM.
- HTTP/1.1 over the Unix socket, one request per connection, `Content-Length` required,
  body at most 1 MiB (413 beyond). `Authorization: Bearer <token>` is checked in constant
  time before the body is parsed: missing or wrong is 401 with no dispatch. On accept the
  server also checks the peer uid with `getpeereid` and drops a mismatch.
- `POST /v1/call` takes a `BridgeRequest` and answers 200 with `{ id, ok: true, value }` or
  `{ id, ok: false, error: { code, message } }`, the same shapes plugin pages get. An
  unknown method keeps the message `not implemented: <method>` with code
  `not_implemented`.
- `X-Calm-Client: <label>` names the client for logs and for `from` in events. It is a
  label, not authentication. Missing means `anonymous`.
- The token never reaches plugin JS, layouts, logs or any API result.

### Terminal env

With the channel enabled, every terminal CALM spawns gets `CALM_SOCKET` (socket path),
`CALM_TOKEN_FILE` (token path, never the token) and `CALM_PANEL_ID`. A Claude Code session
in that terminal reads the token file and knows which panel it is.

### Caller and allowlist

External requests run as `Caller::External { client }`. They are checked against
`external.allow`, not against any manifest. Groups:

| Group             | Methods                                                                    |
| ----------------- | -------------------------------------------------------------------------- |
| `read` (default)  | `app.info`, `panels.list`, `panels.get`, `camera.get`, `terminals.info`, `browsers.info` |
| `status` (default)| `panel.setStatus`                                                          |
| `events` (default)| `events.poll`, `events.publish`                                            |
| `layout`          | `panels.create` **without** `command`, `args` or `env`; `panels.focus`, `move`, `resize`, `setRect`, `setTitle`, `minimize`, `restore`, z-order; `camera.set`, `centerOn`, `fit` |
| `run`             | `panels.create` **with** `command`, `args` or `env`, **and** `terminals.write`, `terminals.sendKeys`, **and** `process.exec` |
| `browser`         | `browsers.navigate`, `browsers.reload`                                     |
| `close`           | `panels.close`, through the same user confirmation as plugins              |

**Running something is one group (`run`).** Spawning with a command, typing into a
terminal and `process.exec` are refused together by default, or allowed together through
one Settings switch and documented together. They never get split: refusing
`command` while allowing `terminals.write` is not a limit, because a client can open a
plain shell and type the command into it. `env` is in the same group because variables
like `PROMPT_COMMAND` run code. `cwd` stays in `layout`; the docs note that shell hooks such
as direnv can run code on entering a directory.

Never offered externally: `github.api` (plugin-only per PR #9), `openExternal`,
`app.setSettings`, `storage.*`, `bus.subscribe`, `plugins.*`, `browser.script`,
`browser.data`, `terminal.read` content methods, and the `panel.*` "self" methods (an external caller has no
panel; it names `panelId`). A method outside `allow` rejects with `not_allowed`.

### Panel status

`panel.setStatus({ panelId?, state, text?, detail? })`, available to both origins. A plugin
page sets its own panel or names another with `panelId`; a backend names `panelId`. An
external caller must name `panelId`. `state` is `"idle" | "running" | "waiting" | "error" |
"done"`, `text` at most 80 characters, `detail` at most 1 KiB, `null` clears. The status is
drawn in the panel's chrome strip as a colored dot and the text. The strip is GPUI paint
outside the native content rect, so it never overlaps a live native view. `detail` is API
only, not drawn. `PanelInfo` gains `status`. The event `panelStatusChanged { panelId,
status }` goes to pages and external queues. Status is not saved in layouts.

### Event queue and long polling

- Per client label, a ring buffer of 1024 events with a sequence number that rises for the
  whole launch. Events pass through the same `Coalescer` as page events before they enter
  the queue, so a pan does not fill it.
- `events.poll({ since, timeoutMs, events? })`: returns at once if events after `since`
  exist, else holds the request up to `timeoutMs` (at most 25000, below the 30 s mod
  fetch limit). The result is `{ events: [{ seq, event, payload }], next, gap }`. `gap` is
  true when events between `since` and the oldest kept one were dropped. `events` filters
  by name.
- Limits: 8 held polls per client and 32 in total, beyond that `busy`. A client queue with
  no poll for 60 s is dropped.
- `events.publish({ topic, payload })`: payload at most 64 KiB. It is delivered to plugin
  pages as the `external` event `{ topic, payload, from: { client } }` and to other
  external clients' queues. CALM does not model Claude Code; topics are free strings.

### Threading

The socket server runs on its own thread with blocking std `UnixListener` I/O. It parses
HTTP and checks the token and peer uid there, then sends `(Caller::External, BridgeRequest,
oneshot reply)` into the same channel the WebKit handler uses, and waits on the oneshot. It
never touches GPUI state. The GPUI task dispatches and answers through the oneshot. Long
polls wait on the client queue, an `Arc<Mutex<..>>` with a `Condvar` that the GPUI task
fills after each coalescer flush. The HTTP parser is a small pinned dependency
(`httparse`), not a framework.

### Tests

Pure parts in calm-plugins with unit tests: allowlist check including the
`run` bundle (a `command` spec, `terminals.write` and `process.exec` all rejected by the
default, all accepted when the group is on), queue sequence and gap reporting, poll limits,
token file mode and `create_new`, symlink and owner refusal, path length refusal. An integration test binds a socket
in a temp dir and checks 401 with no or wrong token, 200 with the right one, and 413.

## Phased implementation

Each phase ends with `cargo test` for the touched crates and lands as its own commit.

1. **Error codes and manifest.** `ApiError`, `ErrorCode`, structured rejects in the prelude
   with v1 string fallback. Manifest `apiVersion`, `capabilities` (declarative, unknown
   words warn), `main`, `backend`, `engines`, `background`, `contributes` parsing and
   validation. The `Caller` type. Pure, tested.
2. **Request parsing.** `ApiRequest::parse` for every method and v1 alias, with a test per
   method for a good and a bad param. `BRIDGE_METHODS`, prelude, `calm.d.ts` extended;
   conformance tests updated.
3. **Read-only host.** `app.info`, `app.getSettings`, `theme.get`, `panels.list`,
   `panels.get`, `panel.info`, `panel.getBounds`, `camera.get`, `browsers.info`,
   `terminals.info` (spawn values, `title: null`). Snapshot builders in calm-plugins.
4. **Control.** `panels.create` (all kinds), focus, move, resize, setRect, setTitle,
   minimize, restore, z-order, `panel.*` on self, `camera.set`, `centerOn`, `fit`,
   `browsers.navigate`, `reload`, `terminals.write`, `app.setSettings`, `ui.notify`.
   `app.setSettings` needs PR #7 merged: merge patch, `Settings::validate`, the
   `confirm_close` and `version` refusal, the image path check, and a test per rule.
5. **Close.** `panels.close` and `panel.close` through `request_close`, `PendingReply`
   settled from Enter and Escape, `busy`. Regression test: a plugin close of a live terminal
   with `confirm_close` on does not remove the panel until Enter.
6. **Events.** Subscribe and unsubscribe, `Coalescer` with injected time, batch emit, all
   v1 and v2 senders. Tests for latest wins, trailing flush, removal drops pending change,
   discrete ordering.
7. **Storage, bus, process, github, openExternal.** Atomic storage file with limits and
   corrupt-file error. Bus fan out. Background executor for exec and gh.
8. **Contributions.** Commands, menu items, key bindings, `command` event routing
   (backend first, then last focused panel, then a new panel).
9. **SDK, docs, examples, smoke.** `calm.d.ts`, `docs/plugins.md`, `templates/plugin-tsx`,
   an example plugin that spawns a terminal and a browser, lists panels and reacts to
   `panelAdded`. Smoke fixture plugin: create a terminal through the bridge, list panels and
   assert the new id with kind `terminal`, receive `panelAdded` for it, report each through
   `log` lines the smoke parses, exit nonzero on any miss.

10. **Ghostty shim patch (planned, user decision).** CALM-owned fork of gpui-libghostty
    0.3.1 through `[patch.crates-io]`: shim functions for `read_text`, `has_selection`,
    `read_selection`, `size`, `foreground_pid`, `tty_name`, `binding_action`, and a queue
    for `SET_TITLE`, `PWD`, `SHOW_CHILD_EXITED`, `COMMAND_FINISHED`, `RING_BELL`,
    `DESKTOP_NOTIFICATION`, `PROGRESS_REPORT`, drained on tick. Then every "patch" row of
    the terminal table, and `terminalOutput` by diffing. New unsafe code carries SAFETY
    comments; `proc_pidpath` goes in calm-native.
11. **External control channel (approved).** The section above, after #5 and #6 merge:
    socket and token lifecycle, terminal env, allowlist with the `run` bundle,
    `panel.setStatus` and status in chrome, per-client queue with long polling,
    `events.publish`, and the `calm-bridge` mod with `marketplace.json` for agent status.
12. **Native browser API.** Content world, `eval`, user scripts and styles, read text and
    HTML, snapshot, navigation rules and hook, history, user agent, page zoom, per-panel
    data stores, cookies and clear data, `browserNavigation` and `browserMessage`.
13. **Plugin RPC.** `exports`, `requires` with semver ranges at discovery, `plugins.list`,
    `call`, `serve`, server choice (backend, background page, panel), timeouts,
    `pluginsChanged`.
14. **Plugin backend.** bun lookup and `engines.bun` check, spawn in a process group, fd 3
    IPC (or the socket fallback), `hello` handshake, restart policy, logs, stop at quit,
    `@calm/plugin-sdk/backend`, `calm.backend.*` in pages, `backendStatus`. Tests: a fixture
    backend that answers a page call, spawns a terminal through `calm.panels.create`, and
    a crash fixture that checks restart and the `failed` state.
15. **Background pages,** only if a plugin needs headless web platform code.

Order: 1 to 9, then 14, 13 and 12, then 10 and 11 as the merged PRs allow. 10 can start any
time because it only touches the fork.

Phase 1 also lands the `Caller` type and phase 4 the `near`/`side` placement, so phase 11
adds a transport, not a new dispatcher.

## Decisions of 2026-10-31

From the user, through the lead:

1. Plugin panels are free-form HTML/TSX surfaces, and plugins get the native browser API,
   the native terminal API and the API of other plugins. The concrete shape of each is the
   lead's reading, marked *[lead's reading, confirm in review]* in its section.
2. The Claude Code mods integration is accepted as in PR #9: the external socket channel
   (phase 11) is approved to build after #5 and #6 merge, with PR #9's allowlist (read-only
   plus `panel.setStatus` and `events.*` by default; `command`, `terminals.write` and
   `process.exec` as one unit, refused by default). First integration: agent status on
   terminal panels. The `calm-bridge` mod lives in this repo with a `marketplace.json`.
3. Terminal title and cwd ship later through the Ghostty shim patch, now a planned phase
   (10) because the user wants the full native terminal API.
4. Background pages are opt-in hidden pages. Superseded in practice by decision 7:
   backends cover most headless needs, background pages are built last if at all.
5. Custom backgrounds: settings schema v2 from PR #7, no plugin-specific format.
6. **Reversed:** capabilities are declarative only, shown at install and in the plugin
   info view, never a gate. There is no `capability_missing`. `confirm_close` stays
   user-owned in the API.
7. Plugins may use the full Node and Bun API, unrestricted, through an optional backend
   process (`main`) run under bun.
8. Unchanged by 6 and 7: the external socket allowlist (it governs outside processes, not
   installed plugins), browser panels never get the `calm` bridge, and CALM never hands its
   own credentials to plugins.

## Open points for review

1. The four *[lead's reading, confirm in review]* sections: free-form panels, native browser API, native terminal API,
   plugin RPC.
2. "No credentials reach plugin JS" narrows to "CALM never hands over its own secrets",
   because plugins are unrestricted: the browser and terminal APIs expose site sessions and
   printed secrets, and a backend can read any file the user can. `CLAUDE.md` says "Plugin JS never sees a token"; that line
   should name the GitHub and socket tokens explicitly when the implementation lands.
3. `terminalOutput` is a text diff at 250 ms, not a byte stream. Enough for agent boards and
   log watchers, not for a terminal recorder.
4. Whether browser panels default to the shared data store (today's behavior, checked on
   PR #6) or a per-panel store.
5. `terminals.setFontSize` changes a terminal's grid. `CLAUDE.md` says only a panel resize
   changes the logical grid; that rule should read "only a panel resize or a font size
   change, never camera zoom" once phase 10 lands.
6. Bun runtime source: A (bun on the system, recommended) or B (pinned bun in the app).
   The user decides.
7. The `confirm_close` line in the API stays, but a backend can bypass it by killing the
   shell's process. Keep it as a guard against plugin bugs, or drop it as theater.
8. Whether fd 3 IPC works with bun as a stream, or the per-plugin socket fallback is needed.
