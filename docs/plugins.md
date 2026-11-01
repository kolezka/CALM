# Writing CALM plugins

A CALM plugin is a folder with a manifest and an HTML page. CALM shows the page in a panel on
the canvas and gives it a small JavaScript API, `window.calm`, for talking to the host.

Status: plugin panels and TUI app panels work. Of the `window.calm` calls, only four are
served by the host so far (see [Host support](#host-support)). Plugin API v2 is a design, not
built.

## Trust

Plugins are trusted code. CALM does not sandbox them and has no permission system yet. A
plugin runs with the same reach as any page the host loads with its bridge, and a malicious
plugin could read private data. Only install plugins you have read or whose author you trust.

## Where plugins live

CALM looks for plugins in these folders, in this order. The first three are user folders,
the last two are built-in folders with the plugins that ship with CALM:

1. Every entry of `CALM_PLUGIN_PATH` (a `:` separated list, like `PATH`).
2. `$CALM_HOME/plugins` if `CALM_HOME` is set, otherwise `~/.config/calm/plugins`.
3. Built-in, development: the repository's own `plugins/` folder. Used by a debug build, or
   by any build that runs from a cargo `target` folder (`cargo run`, `cargo run --release`).
   The path is fixed when CALM is compiled.
4. Built-in, app bundle: `CALM.app/Contents/Resources/plugins`. `gmake bundle` copies the
   built first-party plugins there.

Built-in folders that do not exist are skipped. Turn built-ins off with
`"plugins": { "builtin": false }` in `settings.json`, or for one run with
`CALM_NO_BUILTIN_PLUGINS=1`. Both are read once at startup.

Each plugin is one subfolder of those folders. To install a plugin, copy or symlink its folder
into `~/.config/calm/plugins`. Folders whose name starts with `.` are ignored. A missing plugin
folder is fine and is not an error.

A user plugin with the same `id` as a built-in one wins, and stderr says which built-in it
hides. Any other repeated `id` is an error: the first one found wins and the second is reported
as a duplicate. A subfolder without a manifest, or with a broken one, is reported with its path
and skipped; the other plugins still load.

At startup stderr lists every folder searched (`calm: plugin dir user ...`,
`calm: plugin dir builtin ...`) and every plugin found, hidden or skipped. When no plugin is
found, the Add menu shows "No plugins found" and a Searched Folders submenu with the same list.

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
| `resizable`   | Optional. `false` keeps the panel at the size it was created with.       |
| `minSize`     | Optional `{ "width", "height" }`. Smallest size a resize may reach.      |
| `maxSize`     | Optional `{ "width", "height" }`. Largest size a resize may reach.       |
| `contributes` | Optional. Context menu items, see Context menu contributions.            |
| `apiVersion`  | Optional integer, default 1. See API version and backend fields.         |
| `capabilities`| Optional list of words. Shown to the user, never checked.                |
| `main`        | Optional. Backend entry for bun. Parsed, not run yet.                    |
| `backend`     | Optional. When the backend starts and restarts. Needs `main`.            |
| `engines`     | Optional `{ "bun": "<semver range>" }`. Needs `main`.                    |
| `background`  | Optional `true` or `false`, default `false`. Parsed, not served yet.     |

Every panel can be resized by default, plugins included. Drag the right edge, the bottom
edge or the corner. The handle reaches a few pixels outside the panel, so it works over a
web view too. Sizes include the 28 point title bar.

The resize fields are checked when CALM loads the plugin. A bad value is a manifest error,
and the plugin is skipped with a message on stderr; CALM never guesses a default. Errors:

- `resizable` that is not `true` or `false`, or any field set to `null`;
- a size that is not an object with exactly `width` and `height` as positive numbers;
- a `minSize` or `maxSize` below the smallest panel, 120 by 80;
- a `maxSize` smaller than `minSize` on either axis;
- `minSize` or `maxSize` together with `"resizable": false`;
- a `defaultSize` outside `minSize` and `maxSize`.

```json
{
  "id": "clock",
  "name": "Clock",
  "version": "0.1.0",
  "entry": "index.html",
  "defaultSize": { "width": 240, "height": 160 },
  "minSize": { "width": 200, "height": 120 },
  "maxSize": { "width": 480, "height": 320 }
}
```

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

Restart CALM. "Slack" now shows in the right-click Add menu, under Plugins. Choosing it opens
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

### API version and backend fields

These fields come from plugin API v2. CALM reads and checks them today; it does not run a
backend or a background page yet.

```json
{
  "id": "agent-board",
  "name": "Agent Board",
  "version": "0.2.0",
  "entry": "index.html",
  "defaultSize": { "width": 420, "height": 300 },
  "apiVersion": 2,
  "capabilities": ["panels.create", "terminal.write"],
  "main": "dist/main.js",
  "backend": { "start": "on-demand", "restart": "on-failure" },
  "engines": { "bun": ">=1.2" },
  "background": false
}
```

- `apiVersion` is a positive integer. Missing means 1. This host serves version 2. A plugin
  that asks for more is skipped with `requires API v<n>, host has v<m>`.
- `capabilities` says what the plugin does, in the words of the table below. It is declared
  and shown to the user, never a gate: CALM does not check a call against it. A word CALM
  does not know is a warning, not an error. The plugin still loads, stderr names the words,
  and the status bar counts it as `plugins: <n> warnings`.
- `main` is a file relative to the plugin folder, checked on disk like `entry`. A plugin with
  `main` runs a background program with full access to your files, network and programs.
  CALM does not start it yet.
- `backend.start` is `on-demand` (default) or `at-launch`. `backend.restart` is `on-failure`
  (default) or `never`. These words may still change before backends ship.
- `engines.bun` is an npm-style semver range such as `>=1.2`, `^1.2.3`, `>=1.2 <2`,
  `1.1 || ^1.2` or `1.1.0 - 1.2.0`. Prerelease and build parts are allowed
  (`^1.2.3-rc-1`, `^1.2.3+build.4`). An invalid range is an error. Only the syntax is
  checked for now; no bun is looked up.
- `backend` and `engines` without `main` are errors.
- `background` is `true` or `false`. Parsed, not served yet.
- A TUI app takes `apiVersion` and `capabilities`, and refuses `main`, `backend`, `engines`
  and `background`.
- `contributes` does not need `apiVersion` 2.

| Capability         | What it covers                                                        |
| ------------------ | --------------------------------------------------------------------- |
| `panels.create`    | Creating panels                                                       |
| `panels.control`   | Focusing, moving, resizing and restyling other panels                 |
| `panels.close`     | Closing other panels                                                  |
| `camera.control`   | Moving the camera                                                     |
| `terminal.read`    | Reading terminal content                                              |
| `terminal.write`   | Typing into terminals                                                 |
| `terminal.control` | Clearing, scrolling and other terminal controls                       |
| `browser.control`  | Navigating browser panels                                             |
| `browser.script`   | Running scripts and styles in browser panels                          |
| `browser.data`     | Browser cookies and site data                                         |
| `plugins.call`     | Calling other plugins                                                 |
| `settings.write`   | Changing app settings                                                 |
| `process.exec`     | Running programs                                                      |
| `github`           | `calm.github.api`                                                     |
| `open.external`    | `calm.openExternal`                                                   |
| `layouts`          | Saving, opening and deleting layouts                                  |
| `backend`          | A bun backend process (`main`)                                        |

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
  "apiVersion": 2,
  "capabilities": ["terminal.read"],
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
  },
  "contributes": {
    "menus": [
      { "id": "home", "title": "Slack home workspace", "location": "canvas",
        "run": ["--workspace", "home"] }
    ]
  }
}
```

`contributes` adds context menu items, see Context menu contributions. The folder then holds `calm-plugin.json` and `icons/slack.svg`.

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

## Context menu contributions

A plugin can add items to CALM's right-click menus with `contributes.menus` in
`calm-plugin.json`. Web and TUI plugins both can.

```json
"contributes": {
  "menus": [
    { "id": "open-repo", "title": "Open repo in browser", "location": "panel",
      "when": { "plugin": ["github-prs"] } },
    { "id": "new-panel", "title": "New GitHub PRs panel here", "location": "canvas",
      "open": "new" }
  ]
}
```

| Field      | Meaning                                                                          |
| ---------- | -------------------------------------------------------------------------------- |
| `id`       | Required. `[a-z0-9-]+`, unique within the plugin. Sent back in the command.      |
| `title`    | Required. The menu text, 1 to 80 characters.                                     |
| `location` | Required. `"canvas"`: right click on empty canvas. `"panel"`: right click on a panel's title bar or minimized chip. |
| `when`     | Panel items only. Which panels show the item, see below. Absent: every panel.     |
| `group`    | Items with the same group sit together, groups are split by a separator. Default: no group, listed first. |
| `order`    | Integer, sorts items inside a group. Default 0; ties keep manifest order.         |
| `icon`     | 1 to 4 characters drawn before the title.                                        |
| `open`     | Web plugins. `"reuse"` (default) sends the command to an open panel of the plugin; `"new"` always opens a new panel for it. |
| `run`      | TUI plugins. Arguments appended to the manifest's `args` for the panel the item opens. |

`when` is a small filter, not an expression language. Every key is optional and the ones
you set must all match:

- `kind`: a list of `terminal`, `browser`, `plugin`, `tui`.
- `plugin`: a list of plugin ids. Matches plugin and TUI panels of those plugins, so
  `["<your id>"]` limits an item to your own panels.
- `minimized`: `true` or `false`.

Validation is strict. A missing `title`, an unknown `location`, an unknown field (in an item,
in `when` or in `contributes`), a duplicate `id`, `when` on a canvas item, `run` on a web
plugin or more than 32 items is a manifest error that names the field, and the plugin is
skipped like any other broken manifest.

**Where items show.** Built-in items always come first and plugins cannot remove or change
them. After them comes a separator, then each plugin's items, plugins in discovery order and
split by separators. A plugin with more than 3 visible items in one menu gets a submenu
named after the plugin.

**What a click does.** For a web plugin CALM sends a `command` event to a panel of the
plugin: the right-clicked panel if it is one, else the focused one, else the plugin's
frontmost panel. If none is open, or the item says `"open": "new"`, CALM opens a new panel
at the click point (for a panel item, to the right of the clicked panel) and delivers the
command once the page has loaded. The prelude keeps a command until the page registers a
`command` handler, so a handler added after an `await` still gets it. A command that cannot
be delivered (the panel closed, or the page did not load within 30 s) is logged to stderr,
never dropped silently.

```ts
calm.events.on("command", (command) => {
  // { id, location: "canvas" | "panel", panel: {...} | null, world: { x, y } | null }
  if (command.id === "open-repo") openRepos();
});
```

`panel` describes the right-clicked panel (`id`, `kind`, `pluginId`, `title`, `rect` in world
coordinates, `minimized`) and is null for canvas items. `world` is the right-click point for
canvas items and null for panel items. The type is `MenuCommand` in
`packages/calm-plugin-sdk/calm.d.ts`.

For a TUI plugin an item opens a new panel of the app at the click point, with the item's
`run` arguments appended. A TUI app has no page, so it gets no event. Duplicate on such a
panel starts the manifest's command without the extra arguments.

**User settings.** In `~/.config/calm/settings.json`:

```json
"plugins": { "menus": { "hidden": ["github-prs", "hello-world/wave"], "submenu_above": 3 } }
```

`hidden` takes a plugin id, which hides all its items, or `<plugin-id>/<item-id>` for one
item. `submenu_above` is how many items a plugin may show before they move into a submenu,
0 to 50; 0 puts every plugin in a submenu. Bad values are settings errors. Both are read each
time a menu opens.

Not supported yet: runtime items (`calm.menus.register`), keyboard shortcuts on items
(`shortcut` is rejected as an unknown field) and `contributes.commands` from the Plugin API
v2 design.

## The `window.calm` API

CALM injects `window.calm` before any of your scripts run. The types live in one place,
`packages/calm-plugin-sdk/calm.d.ts` (package `@calm/plugin-sdk`), and this section follows
it. Every call except `calm.log`, `calm.events.on` and `calm.has` returns a Promise. A failed
call rejects with an `Error` whose `message` comes from the host and whose `code` is one of the
words in [Errors](#errors).

`calm.has(method)` answers synchronously whether this host serves a method, for example
`calm.has("panel.setTitle")`. It reads `window.__calmHost = { apiVersion, methods }`, which the
host injects before the prelude, so it works before any call. A method the host lists only as
declared answers `false`.

```js
if (calm.has("canvas.createPanel")) {
  await calm.canvas.createPanel({ kind: "terminal" });
}
```

The app serves all 17 methods. An older CALM may not, so check `calm.has` before you rely
on one.

### Settings

| Call                       | Result                                                        |
| -------------------------- | ------------------------------------------------------------- |
| `calm.settings.get()`      | This panel's settings object.                                 |
| `calm.settings.set(value)` | Saves `value` as this panel's settings. Saved with the layout. |

Settings are stored in the layout file, so never put a token or password in them.

### Storage

Plugin data that is not tied to one panel. Stored in
`~/.config/calm/plugin-data/<plugin-id>.json` (`$CALM_HOME/plugin-data` when `CALM_HOME` is
set), not in layouts. Keys are 1 to 256 bytes. A write that would make the file larger than
5 MiB is refused. Writes go to a temp file that is renamed over the old one. A file that
does not parse makes every call reject and is left as it is, never replaced by `{}`.

| Call                           | Result                                            |
| ------------------------------ | ------------------------------------------------- |
| `calm.storage.get<T>(key)`     | The stored value, or `undefined` if there is none. |
| `calm.storage.set(key, value)` | Stores a JSON value under `key`. `undefined` rejects with `invalid_params`; `null` is stored. |
| `calm.storage.remove(key)`     | Deletes `key`.                                    |

### Panel

| Call                         | Result                                                         |
| ---------------------------- | -------------------------------------------------------------- |
| `calm.panel.setTitle(title)` | Sets the panel title.                                          |
| `calm.panel.getBounds()`     | `{ width, height, zoom }`: size in canvas points with the title bar, camera zoom. |
| `calm.panel.minimize()`      | Minimizes the panel. The page keeps running. A second call changes nothing. |
| `calm.panel.close()`         | Closes the panel through the normal close path. A plugin panel has no process, so it closes at once. |

### Canvas

| Call                          | Result                                                  |
| ----------------------------- | ------------------------------------------------------- |
| `calm.canvas.createPanel(spec)` | Opens a new panel, resolves with its `PanelId`.       |
| `calm.canvas.focusPanel(id)`  | Restores a minimized panel, raises it and focuses it. `not_found` for a bad id. |
| `calm.canvas.listPanels()`    | `PanelInfo[]` for every panel, back to front, not only this plugin's. |

`spec` is one of:

```ts
{ kind: "terminal", command?: string, cwd?: string, rect?: Rect, focus?: boolean }
{ kind: "browser", url: string, rect?: Rect, focus?: boolean }
{ kind: "plugin", pluginId: string, settings?: Record<string, unknown>, rect?: Rect, focus?: boolean }
{ kind: "tui", pluginId: string, rect?: Rect, focus?: boolean }
```

`Rect` is `{ x, y, width, height }` in world units, title bar included. `PanelId` is a number.

- With `rect`, the panel goes exactly there. Without it, it opens at the center of the view
  at the kind's default size: the terminal size from settings, 800x600 for a browser, the
  manifest `defaultSize` for a plugin or TUI app.
- The size is held within the panel's resize limits, as a drag is. A plugin with
  `"resizable": false` rejects a `rect` of any other size with `invalid_params`.
- `command` is a shell line, as in a terminal profile. `cwd` must be an absolute path to a
  directory. `url` goes through the address bar rules, so `example.com` opens
  `https://example.com`.
- `focus` defaults to `true`, as for a panel from the Add menu.
- `pluginId` must name an installed web plugin for `"plugin"` and a TUI app for `"tui"`,
  else the call rejects with `not_installed`.
- Fields of the v2 design that this CALM does not build yet (`near`, `side`, `size`, `gap`,
  `title`, `args`, `env`, `appearance`) reject with `unavailable`. Any other unknown field
  rejects with `invalid_params`.

`PanelInfo` is `{ id, kind, title, rect, minimized, z, focused, pluginId?, url?, resize }`.
`kind` is `"terminal"`, `"browser"`, `"plugin"`, `"tui"` or `"unsupported"` (a panel from a
layout of a newer CALM). `z` is 0 for the backmost panel. `pluginId` is set for plugin and TUI
panels, `url` for browsers. `resize` is `{ resizable, minSize, maxSize }`, `maxSize` `null`
when there is no limit.

### Process

`calm.process.exec(command, args, options?)` runs a program on the host and resolves with
`{ code, stdout, stderr }`. It runs with your user's rights. This is why plugins must be
trusted code (see [Trust](#trust)).

- No shell: `command` is the program, `args` its arguments, each passed as is. Run
  `/bin/sh -c "..."` yourself when you need one.
- A bare name is looked up in CALM's `PATH`. Started from the Dock or Finder, that is the
  short system `PATH` without Homebrew. Use an absolute path, or `/bin/sh -lc "command -v gh"`
  to ask the login shell.
- `options.cwd` is an absolute path to a directory. `options.timeoutMs` is 1 to 600000,
  default 30000. Past it CALM kills the program and everything it started (its process
  group) and the call rejects with `timeout`.
- `code` is the exit code, or 128 plus the signal number when a signal ended the program.
- `stdout` and `stderr` are decoded as UTF-8; invalid bytes become U+FFFD. Each is capped
  at 4 MiB; more rejects with `too_large`.
- A program that cannot start rejects with `host_error`. stdin is empty. The program gets
  CALM's environment.

### Other calls

| Call                     | Result                                                       |
| ------------------------ | ------------------------------------------------------------ |
| `calm.github.api(path)`  | Runs `gh api <path>` on the host, resolves with parsed JSON. |
| `calm.openExternal(url)` | Opens an `http` or `https` URL in the default browser (`open`). Other schemes reject. |
| `calm.theme.get()`       | `{ background }`, a `"#rrggbb"` string: the flat color that stands for the canvas background, also for a gradient or an image. |
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
| `command`         | a chosen menu item, see Context menu contributions |

An unknown event name throws a `TypeError`. A handler that throws is logged through
`calm.log` and does not stop the other handlers.

`window.calm` only exists inside CALM. Guard calls with `if (window.calm)` if you also open the
page in a normal browser while developing.

### Host support

The prelude exposes every call above, and the app's plugin host
(`crates/calm-app/src/plugin_host.rs`) serves all of them. `github.api`, `storage.*`,
`openExternal` and `process.exec` run on a background thread, so a slow program never
stalls the canvas. An unknown method rejects with code `unknown_method`.

| `method`             | Host support                              |
| -------------------- | ----------------------------------------- |
| `settings.get`       | Served: the panel's settings object       |
| `settings.set`       | Served: replaces it, objects only         |
| `storage.get`        | Served: the value, `undefined` if unset   |
| `storage.set`        | Served: atomic write of the plugin file   |
| `storage.remove`     | Served                                    |
| `panel.setTitle`     | Served: sets the chrome title             |
| `panel.getBounds`    | Served: size with the title bar, zoom     |
| `panel.minimize`     | Served: the page keeps running            |
| `panel.close`        | Served: the normal close path             |
| `canvas.createPanel` | Served: terminal, browser, plugin, TUI app |
| `canvas.focusPanel`  | Served: restores, raises, focuses         |
| `canvas.listPanels`  | Served: every panel, back to front        |
| `process.exec`       | Served: no shell, own process group       |
| `github.api`         | Served: `gh api <path>` on the host       |
| `openExternal`       | Served: `http` and `https` only           |
| `theme.get`          | Served: the background's base color       |
| `log`                | Served: printed to stderr, no answer       |

### Errors

A rejected call's `Error` has a `code`. The codes below are the whole list for plugin API v2.
This host produces `unknown_method`, `invalid_params`, `not_found` (`canvas.focusPanel`),
`wrong_kind` (`settings.*` or `storage.*` from a panel that is not a plugin), `unavailable`
(a `createPanel` field not built yet), `not_installed` (`createPanel` with an unknown
`pluginId`), `too_large`, `storage_corrupt`, the four `gh_*` codes, `timeout` and
`host_error` today. `not_implemented` is for a method a host knows but does not serve; this
one serves them all. The rest belong to methods of the v2 design.

| `code`             | When                                                                |
| ------------------ | ------------------------------------------------------------------- |
| `unknown_method`   | Not a method of the API                                             |
| `not_implemented`  | Known, but this host does not serve it yet                          |
| `invalid_params`   | Missing, wrong type or out of range; the message names the field    |
| `not_found`        | No panel with that id                                               |
| `wrong_kind`       | A terminal method on a browser panel, and so on                     |
| `not_alive`        | The terminal process exited                                         |
| `busy`             | Another close confirmation is pending, or a TUI restart is running  |
| `cancelled`        | The user cancelled a confirmation                                   |
| `unavailable`      | Needs a later CALM                                                  |
| `too_large`        | Payload, storage or output over its limit                           |
| `storage_corrupt`  | The plugin data file does not parse                                 |
| `gh_not_installed` | `gh` is not installed                                               |
| `gh_not_logged_in` | `gh` is not logged in                                               |
| `gh_failed`        | `gh api` failed; the message has its error                          |
| `gh_invalid_path`  | The `github.api` path is not a plain API path                       |
| `timeout`          | The call ran past its limit (`gh api`, `process.exec`)              |
| `not_allowed`      | An external caller asked for a method outside its allowlist          |
| `not_installed`    | `createPanel` or `plugins.call` names a plugin that is not installed |
| `not_running`      | `plugins.call` to an export no running page serves                  |
| `method_not_found` | `plugins.call` to a method the callee does not export               |
| `remote_error`     | The called plugin's handler threw                                   |
| `host_error`       | Anything else; the message says what                                |

Only `command` has a host sender so far. Handlers for the other events do not fire until
their app wiring lands.

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
true, `value` is the result. With `ok` false, `value` is `{ "code": string, "message": string }`
(`BridgeResponse::Err` carries an `ApiError`). The prelude turns it into an `Error` with that
`message` and a `code` property. A plain string, the v1 shape, still works and gets
`code: "host_error"`. A host answers a method it knows but does not serve with
`{ "code": "not_implemented", "message": "not implemented: <method>" }`
(`BridgeResponse::not_implemented`).

Before the prelude, the host injects `window.__calmHost = { apiVersion, methods }`
(`host_info_js`), where `methods` is the list the host serves (`served_methods` in
`plugin_host.rs`). `calm.has` reads it.

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

Instead of `gh auth login` you can give `gh` a token. CALM looks in this order and uses the
first one it finds:

1. `GH_TOKEN`, then `GITHUB_TOKEN`, in the environment CALM was started with.
2. `$CALM_HOME/.env`, by default `~/.config/calm/.env`.
3. Dev builds only (`cargo run`): `.env` at the root of the checkout. `.gitignore` covers it.

If none is set, `gh` uses its own login. A `.env` file looks like this:

```sh
# ~/.config/calm/.env
GITHUB_TOKEN=<your token>
```

Lines are `KEY=VALUE`, with optional quotes, `#` comments and an `export ` prefix. Only
`GH_TOKEN` and `GITHUB_TOKEN` are read (`GH_TOKEN` wins within one file); every other key is
ignored and exported nowhere. A line that does not parse is reported on stderr by its line
number. Keep the file private with `chmod 600`: CALM warns, naming the path, when other
users can read it, but still loads it.

The token goes to the `gh` process as `GH_TOKEN` in its environment, never on its command
line. Plugin JS never sees it, and CALM replaces it with `<redacted>` in error messages and
responses. Logs name only where it came from.

`path` is the part after `https://api.github.com/`, for example
`repos/OWNER/NAME/pulls?state=open&per_page=30`. It may only contain letters, digits and
`_ . / ? = & % -`, and may not start with `-`. Only GET requests are possible, and results are
not paginated. `gh` is killed after 30 s, and a response over 8 MiB is refused.

A failed call rejects with one of these codes and messages. Match on `code`; the message
text may change.

| `code`             | Message starts with                      | Meaning                                                      |
| ------------------ | ---------------------------------------- | ------------------------------------------------------------ |
| `gh_not_installed` | `gh is not installed`                    | No `gh` on the host. Install it from https://cli.github.com. |
| `gh_not_logged_in` | `gh is not logged in, run gh auth login` | `gh` has no credentials. Run `gh auth login`.                |
| `gh_failed`        | `gh api failed:`                         | GitHub or `gh` refused the call, e.g. 404 or no access.      |
| `gh_invalid_path`  | `invalid gh api path`                    | `path` broke the rules above. `gh` was not run.              |
| `timeout`          | `gh api timed out`                       | `gh` ran longer than 30 s and was stopped.                   |
| `too_large`        | `gh api response is over`                | The response was larger than 8 MiB.                          |

An HTTP 401 from GitHub also counts as not logged in. A rate limit shows as
`gh api failed: ... API rate limit exceeded ...`.

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

Build the plugins in this repository and start CALM from it:

```sh
gmake plugins          # needs bun; fails with a message when bun is missing
cargo run -p calm-app
```

The repository's `plugins/` folder is a built-in folder in a development build, so its plugins
are in the Add menu with no further setup. To work on a plugin somewhere else, add its parent
folder to `CALM_PLUGIN_PATH`.

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
- `plugins/hello-world`: plain HTML and JS. Shows text and sets the panel title. Its panel
  menu item "Say hello" changes the title from a `command` handler.
- `plugins/github-prs`: TSX. Lists open pull requests for the repositories saved in its
  settings (`{ "repos": ["owner/name"] }`), editable in the panel. Clicking a pull request
  opens it in the browser. Build it first with `bun install && bun run build` in its folder.
  Menu item: "Open repo in browser" on its own panels. Opening a URL goes through `openExternal`,
  which the host does not serve yet, so for now the item logs the rejection.
