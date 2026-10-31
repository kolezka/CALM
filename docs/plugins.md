# Writing CALM plugins

A CALM plugin is a folder with a manifest and an HTML page. CALM shows the page in a panel on
the canvas and gives it a small JavaScript API, `window.calm`, for talking to the host.

Status: this page describes the plugin contract implemented in `crates/calm-plugins`. The host
side that shows plugin panels in `calm-app` is still being wired up.

## Trust

Plugins are trusted code. CALM does not sandbox them and has no permission system yet. A
plugin runs with the same reach as any page the host loads with its bridge, and a malicious
plugin could read private data. Only install plugins you have read or whose author you trust.

## Where plugins live

CALM looks for plugins in these folders, in this order:

1. Every entry of `CALM_PLUGIN_PATH` (a `:` separated list, like `PATH`).
2. `$CALM_HOME/plugins` if `CALM_HOME` is set, otherwise `~/.config/calm/plugins`.

Each plugin is one subfolder of those folders. To install a plugin, copy or symlink its folder
into `~/.config/calm/plugins`. Folders whose name starts with `.` are ignored. A missing plugin
folder is fine and is not an error.

If two plugins use the same `id`, the first one found wins and the second is reported as a
duplicate. A subfolder without a manifest, or with a broken one, is reported with its path and
skipped; the other plugins still load.

## Manifest

Every plugin folder holds a `calm-plugin.json`:

```json
{
  "id": "hello-world",
  "name": "Hello World",
  "version": "0.1.0",
  "entry": "index.html",
  "defaultSize": { "width": 320, "height": 200 }
}
```

| Field         | Meaning                                                                  |
| ------------- | ------------------------------------------------------------------------ |
| `id`          | Unique id, lowercase letters, digits and `-` only.                       |
| `name`        | Name shown in the Add menu.                                              |
| `version`     | Plugin version, free form.                                               |
| `entry`       | HTML file to load, relative to the plugin folder. No `..`, no `/` start. |
| `defaultSize` | Size of a new panel in canvas points.                                    |

## TUI apps

A TUI app plugin is a terminal panel that runs one command. Use it for programs with a text
interface, such as a Slack client, `htop` or `lazygit`. It needs no HTML and no build step: the
plugin folder holds only `calm-plugin.json`, with `"kind": "tui"`. It lives in the same plugin
folders as HTML plugins.

For example, to add "Slack" running the `slk` client, make
`~/.config/calm/plugins/slack/calm-plugin.json`:

```json
{
  "kind": "tui",
  "id": "slack",
  "name": "Slack",
  "command": "slk",
  "icon": "#"
}
```

Restart CALM. "Slack" now shows in the right-click Add menu, next to Terminal. Choosing it opens
a terminal panel titled "Slack" that runs `slk`.

| Field         | Required | Meaning                                                              |
| ------------- | -------- | -------------------------------------------------------------------- |
| `kind`        | yes      | `"tui"`. Without `kind` a manifest is an HTML plugin (`"web"`).      |
| `id`          | yes      | Unique id, lowercase letters, digits and `-` only.                   |
| `name`        | yes      | Name in the Add menu and the panel title.                            |
| `command`     | yes      | Program to run: a name looked up on `PATH`, or an absolute path.     |
| `args`        | no       | Array of strings, passed as they are. Spaces and quotes are safe.    |
| `cwd`         | no       | Working directory, absolute or starting with `~/`. Default: home.    |
| `env`         | no       | Object of string values, set for the command only.                   |
| `defaultSize` | no       | `{ "width", "height" }` in canvas points. Default 640 by 400.        |
| `icon`        | no       | 1 to 4 characters, or a `.png` or `.svg` file in the plugin folder.  |
| `title`       | no       | Title bar text. `{name}` is replaced by `name`. Default: `name`.     |
| `accentColor` | no       | Focus ring, join divider and text icon color. Hex, see below.        |
| `titleBarColor` | no     | Title bar background. Hex.                                           |
| `titleColor`  | no       | Title text color. Hex.                                               |
| `terminalColors` | no    | Terminal background, foreground and 16-color palette. See below.     |
| `description` | no       | Free text for people reading the manifest.                           |
| `restart`     | no       | `"prompt"` (default), `"on-exit"` or `"never"`. See below.           |
| `shell`       | no       | `true` (default) runs through your login shell. See below.           |
| `version`     | no       | Free form.                                                           |

Any other field, or a value of the wrong type, is an error: the plugin is skipped and listed
with the other plugin errors on stderr and in the status bar count. `entry` is an error too,
since a TUI app has no page. `command` may not start with `-` or contain `=`.

### PATH and the login shell

An app started from Finder or the Dock does not get the `PATH` your shell sets up, so a program
installed with Homebrew or `cargo install` would not be found. With `"shell": true` CALM starts
the command through your login shell (`$SHELL`, else `/bin/zsh`) as
`$SHELL -lc 'exec "$0" "$@"' command args...`, so your profile runs first and sets `PATH`. Every
argument is passed as its own word; nothing is pasted into a shell string. The login shell must
understand POSIX `sh` syntax (zsh, bash, sh and ksh do). With fish, use `"shell": false` and an
absolute `command`.

With `"shell": false` the command runs directly, with the `PATH` CALM itself has.

`env` values are set with `/usr/bin/env` right before the command, after your profile ran, so
they win over it. They are visible in the process list (`ps`) to other programs, so never put a
token or password in `env`. Let the app keep its own credentials, as `slk` and most TUIs do.

### When the program exits

The panel stays open. The terminal shows the program's last output and a line such as
`[calm] slk exited with code 0`, and the panel title says `Slack: exited, code 0`.

| `restart`  | After the program exits                                                     |
| ---------- | --------------------------------------------------------------------------- |
| `prompt`   | Stays exited. Restart from the panel's right-click menu, or press Enter.     |
| `on-exit`  | Starts again after 1 s. Quick exits (under 10 s) in a row wait 1, 2, 4, 8 s; after 5 in a row it gives up and offers Restart. |
| `never`    | Stays exited. No Restart.                                                   |

Enter only restarts once the program has exited; while it runs, every key goes to it.

If the command is not found, the terminal shows `[calm] command not found: slk` and the plugin
folder, and the title says `Slack: command not found: slk`. CALM never restarts that on its own:
install the program or fix `PATH`, then use Restart.

A TUI panel is a terminal in every other way: it moves, resizes, minimizes and turns into a
preview like one, and closing it while the program runs asks first. Layouts store only the
plugin `id`, never the command, `args` or `env`.

`plugins/top-example` runs macOS `top` with its own accent, title and SVG icon.
`templates/plugin-tui` is a starting point.

### Customizing TUI apps

Every part of a TUI panel's look can come from its manifest. Each field is optional and falls
back to the theme. A bad value is an error that names the field, never a silent default.

This Slack panel uses every field:

```json
{
  "kind": "tui",
  "id": "slack",
  "name": "Slack",
  "version": "1.0.0",
  "description": "Slack in the terminal, through slk.",
  "command": "slk",
  "args": ["--workspace", "my team"],
  "cwd": "~/work",
  "env": { "SLK_THEME": "dark" },
  "shell": true,
  "restart": "on-exit",
  "defaultSize": { "width": 900, "height": 600 },
  "title": "{name} (work)",
  "icon": "icons/slack.svg",
  "accentColor": "#4a154b",
  "titleBarColor": "#1a1d21",
  "titleColor": "#d1d2d3",
  "terminalColors": {
    "background": "#1a1d21",
    "foreground": "#d1d2d3",
    "palette": [
      "#1a1d21", "#e01e5a", "#2eb67d", "#ecb22e", "#36c5f0", "#4a154b", "#36c5f0", "#d1d2d3",
      "#616061", "#e01e5a", "#2eb67d", "#ecb22e", "#36c5f0", "#ae6ec8", "#36c5f0", "#ffffff"
    ]
  }
}
```

The folder then holds `calm-plugin.json` and `icons/slack.svg`.

- `title`: shown in the title bar. `{name}` is the only placeholder; any other `{` or `}` is an
  error. It may not be empty or blank, may not hold control characters, and is at most 120
  characters. Status text such as `: exited, code 0` is added after it.
- `icon`: text of 1 to 4 characters, or a path ending in `.png` or `.svg`, relative to the
  plugin folder. The image is scaled into the icon slot and decoded once, when the panel
  opens. A path that is absolute, uses `..`, or resolves through a symlink to outside the
  folder is an error, and so is a missing file. A file that exists but does not decode is
  logged on stderr and the panel falls back to the terminal icon. Images keep their own
  colors; `accentColor` tints only a text icon.
- Colors are `#rrggbb` or `#rrggbbaa` (alpha last), upper or lower case.
  `accentColor` colors the ring while the panel has focus, the divider between it and a
  joined neighbor while it has focus, and a text icon. `titleBarColor` and `titleColor` apply
  with or without focus; the accent ring is what marks focus. During a close confirmation the
  bar always uses the theme's warning colors.
- `terminalColors`: the Ghostty colors for this panel, laid over your Ghostty config (or over
  Ghostty's defaults when Settings turns the user config off). `background`, `foreground` and
  all 16 `palette` entries are required, each `#rrggbb` with no alpha. Other keys are errors.

Planned, not supported yet: `fontSize`, `fontFamily` and `padding`. gpui-libghostty 0.3.1 takes
only colors per terminal; fonts and padding come from your Ghostty config for all terminals.
They need the planned libghostty shim patch. Until then these three fields are rejected with
an error that says so, rather than being ignored.

Per-panel edits after a panel is added (rename, change icon, pick an accent from the
right-click menu) are not built yet.

## The `window.calm` API

CALM injects `window.calm` before any of your scripts run. The types live in one place,
`packages/calm-plugin-sdk/calm.d.ts` (package `@calm/plugin-sdk`), and this section follows
it. Every call except `calm.log` and `calm.events.on` returns a Promise. A failed call rejects
with an `Error` whose message comes from the host.

The app serves four requests so far. Check [Host support](#host-support) before you rely on one.

### Settings

| Call                       | Result                                                        |
| -------------------------- | ------------------------------------------------------------- |
| `calm.settings.get()`      | This panel's settings object.                                 |
| `calm.settings.set(value)` | Saves `value` as this panel's settings. Saved with the layout. |

Settings are stored in the layout file, so never put a token or password in them.

### Storage

Plugin data that is not tied to one panel. Stored in
`~/.config/calm/plugin-data/<plugin-id>.json`, not in layouts.

| Call                           | Result                                            |
| ------------------------------ | ------------------------------------------------- |
| `calm.storage.get<T>(key)`     | The stored value, or `undefined` if there is none. |
| `calm.storage.set(key, value)` | Stores a JSON value under `key`.                  |
| `calm.storage.remove(key)`     | Deletes `key`.                                    |

### Panel

| Call                         | Result                                                         |
| ---------------------------- | -------------------------------------------------------------- |
| `calm.panel.setTitle(title)` | Sets the panel title.                                          |
| `calm.panel.getBounds()`     | `{ width, height, zoom }`: size in canvas points, camera zoom. |
| `calm.panel.minimize()`      | Minimizes the panel. The page keeps running.                   |
| `calm.panel.close()`         | Asks to close the panel through the normal, confirmed path.    |

### Canvas

| Call                          | Result                                                  |
| ----------------------------- | ------------------------------------------------------- |
| `calm.canvas.createPanel(spec)` | Opens a new panel, resolves with its `PanelId`.       |
| `calm.canvas.focusPanel(id)`  | Focuses a panel.                                        |
| `calm.canvas.listPanels()`    | `PanelInfo[]`: `{ id, kind, title, rect, minimized }`.  |

`spec` is one of:

```ts
{ kind: "terminal", command?: string, cwd?: string, rect?: Rect }
{ kind: "browser", url: string, rect?: Rect }
{ kind: "plugin", pluginId: string, settings?: Record<string, unknown>, rect?: Rect }
```

`Rect` is `{ x, y, width, height }` in canvas points. `PanelId` is a number.

### Process

`calm.process.exec(command, args, options?)` runs a program on the host and resolves with
`{ code, stdout, stderr }`. `options` takes `cwd` and `timeoutMs`. It runs with your user's
rights. This is why plugins must be trusted code (see [Trust](#trust)).

### Other calls

| Call                     | Result                                                       |
| ------------------------ | ------------------------------------------------------------ |
| `calm.github.api(path)`  | Runs `gh api <path>` on the host, resolves with parsed JSON. |
| `calm.openExternal(url)` | Opens `url` in the system browser.                           |
| `calm.theme.get()`       | `{ background }`, a `"#rrggbb"` string.                      |
| `calm.log(message)`      | Writes `message` to CALM's stderr, tagged with the plugin. Fire and forget: returns nothing, never waits for the host. |

### Events

`calm.events.on(event, handler)` registers a handler and returns a function that removes it.
The host sends these events:

| Event             | Payload                                            |
| ----------------- | -------------------------------------------------- |
| `focus`           | none                                               |
| `blur`            | none                                               |
| `resize`          | `{ width, height, zoom }`                          |
| `visibility`      | `{ mode: "live" \| "preview" \| "hidden" }`        |
| `settingsChanged` | the new settings object                            |
| `themeChanged`    | `{ background }`                                   |

An unknown event name throws a `TypeError`. A handler that throws is logged through
`calm.log` and does not stop the other handlers.

`window.calm` only exists inside CALM. Guard calls with `if (window.calm)` if you also open the
page in a normal browser while developing.

### Host support

The prelude exposes every call above. The app's plugin host (`crates/calm-app/src/plugin_host.rs`)
serves the four methods marked below. Every other method rejects with
`not implemented: <method>`, so no call hangs. An unknown method rejects with
`unknown method: <method>`.

| `method`             | Host support                              |
| -------------------- | ----------------------------------------- |
| `settings.get`       | Served: the panel's settings object       |
| `settings.set`       | Served: replaces it, objects only         |
| `storage.get`        | Rejects with `not implemented: <method>` |
| `storage.set`        | Rejects with `not implemented: <method>` |
| `storage.remove`     | Rejects with `not implemented: <method>` |
| `panel.setTitle`     | Served: sets the chrome title             |
| `panel.getBounds`    | Rejects with `not implemented: <method>` |
| `panel.minimize`     | Rejects with `not implemented: <method>` |
| `panel.close`        | Rejects with `not implemented: <method>` |
| `canvas.createPanel` | Rejects with `not implemented: <method>` |
| `canvas.focusPanel`  | Rejects with `not implemented: <method>` |
| `canvas.listPanels`  | Rejects with `not implemented: <method>` |
| `process.exec`       | Rejects with `not implemented: <method>` |
| `github.api`         | Rejects with `not implemented: <method>` |
| `openExternal`       | Rejects with `not implemented: <method>` |
| `theme.get`          | Rejects with `not implemented: <method>` |
| `log`                | Served: printed to stderr, no answer       |

Events have no host sender yet, so handlers registered with `calm.events.on` do not fire
until the app wiring lands.

### Wire format

For host and tooling authors. Each call posts one message to the `calm` WebKit message handler,
as a JSON string (`JSON.stringify` of the object below). A string reaches the host as text; a
plain object would arrive as an NSDictionary, whose description is not JSON:

```json
{ "id": 7, "method": "panel.setTitle", "params": { "title": "Hello" } }
```

`BRIDGE_METHODS` in `crates/calm-plugins/src/bridge.rs` lists every `method`. Tests keep it,
the prelude and `calm.d.ts` in sync.

| `method`             | `params`                                                             |
| -------------------- | -------------------------------------------------------------------- |
| `settings.get`       | `null`                                                               |
| `settings.set`       | the settings object                                                  |
| `storage.get`        | `{ "key": string }`                                                  |
| `storage.set`        | `{ "key": string, "value": any }`                                    |
| `storage.remove`     | `{ "key": string }`                                                  |
| `panel.setTitle`     | `{ "title": string }`                                                |
| `panel.getBounds`    | `null`                                                               |
| `panel.minimize`     | `null`                                                               |
| `panel.close`        | `null`                                                               |
| `canvas.createPanel` | `{ "spec": PanelSpec }`                                              |
| `canvas.focusPanel`  | `{ "id": number }`                                                   |
| `canvas.listPanels`  | `null`                                                               |
| `process.exec`       | `{ "command": string, "args": string[], "cwd": string \| null, "timeoutMs": number \| null }` |
| `github.api`         | `{ "path": string }`                                                 |
| `openExternal`       | `{ "url": string }`                                                  |
| `theme.get`          | `null`                                                               |
| `log`                | `{ "message": string }`                                              |

The host answers by evaluating `window.__calm.resolve(id, ok, value)` in the page. With `ok`
true, `value` is the result. With `ok` false, `value` is an error message string. The host
answers a method it does not serve with `ok` false and `not implemented: <method>`
(`BridgeResponse::not_implemented`).

`log` is fire and forget. The prelude posts it with an `id` but keeps no pending entry, so the
host does not need to answer it, and an answer is ignored.

The host sends an event by evaluating `window.__calm.emit("<event>", <payload json>)`
(`BridgeEvent::to_js`). Both scripts JSON encode their values and escape U+2028 and U+2029.

## GitHub access

`calm.github.api(path)` runs the GitHub CLI on the host as `gh api <path>`. CALM never reads
or stores the GitHub token: `gh` keeps it, and plugin JS only ever sees the JSON response.

Setup, once per machine:

```sh
brew install gh
gh auth login
```

`path` is the part after `https://api.github.com/`, for example
`repos/OWNER/NAME/pulls?state=open&per_page=30`. It may only contain letters, digits and
`_ . / ? = & % -`, and may not start with `-`. Only GET requests are possible, and results are
not paginated.

A failed call rejects with one of these messages:

| Message starts with                      | Meaning                                                      |
| ---------------------------------------- | ------------------------------------------------------------ |
| `gh is not installed`                    | No `gh` on the host. Install it from https://cli.github.com. |
| `gh is not logged in, run gh auth login` | `gh` has no credentials. Run `gh auth login`.                |
| `gh api failed:`                         | GitHub or `gh` refused the call, e.g. 404 or no access.      |
| `invalid gh api path`                    | `path` broke the rules above. `gh` was not run.              |

## TypeScript and TSX

The runtime format is plain HTML and JS. To write a plugin in TypeScript or TSX, compile it to
JS first. `templates/plugin-tsx` is a starter that uses Preact for JSX and Bun to bundle:

```sh
cp -R templates/plugin-tsx ~/my-plugins/my-plugin
cd ~/my-plugins/my-plugin
bun install
bun run build    # writes dist/index.js, which index.html loads
```

Change `id` and `name` in `calm-plugin.json` before installing it. `dist/` is a build output
and is not committed, so build after every clone and after every source change.
The types come from `packages/calm-plugin-sdk/calm.d.ts` through the `@calm/plugin-sdk` entry
in `tsconfig.json` `paths`. That path is relative to this repository, so after copying the
template elsewhere point it at `calm.d.ts` in your CALM checkout, or copy the file next to your
sources. Type check with `bunx tsc --noEmit -p .`.

## Development loop

Point CALM at the plugins in this repository and start it:

```sh
CALM_PLUGIN_PATH=./plugins cargo run -p calm-app
```

No rebuild of CALM is needed. HTML and JS changes show up in a newly opened panel. Manifest
changes need a CALM restart, because discovery runs at startup. Installed plugins appear in
the right-click Add menu; choosing one opens a panel at the click point with the manifest's
default size.

If the entry page loads a local script that does not exist, for example `dist/index.js` of a
TSX plugin that was never built, the panel shows an error page naming the missing file and
the build command, its title ends in `(not built)`, and stderr prints the same message.
Broken manifests are printed to stderr with their path and counted in the status bar.

Set `CALM_SKIP_BUN_TESTS=1` to explicitly skip the Bun-backed bridge tests.

## Examples

- `plugins/top-example`: a TUI app running macOS `top`, sorted by CPU.
- `plugins/hello-world`: plain HTML and JS. Shows text and sets the panel title.
- `plugins/github-prs`: TSX. Lists open pull requests for the repositories saved in its
  settings (`{ "repos": ["owner/name"] }`), editable in the panel. Clicking a pull request
  opens it in the browser. Build it first with `bun install && bun run build` in its folder.
