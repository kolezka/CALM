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
| `name`        | Name shown in the toolbar.                                               |
| `version`     | Plugin version, free form.                                               |
| `entry`       | HTML file to load, relative to the plugin folder. No `..`, no `/` start. |
| `defaultSize` | Size of a new panel in canvas points.                                    |

## The `window.calm` API

CALM injects `window.calm` before any of your scripts run. Every call returns a Promise. A
failed call rejects with an `Error` whose message comes from the host.

| Call                         | Result                                                         |
| ---------------------------- | -------------------------------------------------------------- |
| `calm.settings.get()`        | This panel's saved settings object, or `null` if none yet.     |
| `calm.settings.set(obj)`     | Saves `obj` as this panel's settings. Saved with the layout.   |
| `calm.panel.setTitle(title)` | Sets the panel title.                                          |
| `calm.log(message)`          | Writes `message` to CALM's stderr, tagged with the plugin.     |
| `calm.github.api(path)`      | Runs `gh api <path>` on the host, resolves with parsed JSON.   |
| `calm.openExternal(url)`     | Opens `url` in the system browser.                             |

Settings are stored in the layout file, so never put a token or password in them.

`window.calm` only exists inside CALM. Guard calls with `if (window.calm)` if you also open the
page in a normal browser while developing.

### Wire format

For host and tooling authors. Each call posts one message to the `calm` WebKit message handler:

```json
{ "id": 7, "method": "panel.setTitle", "params": { "title": "Hello" } }
```

| `method`         | `params`                       |
| ---------------- | ------------------------------ |
| `settings.get`   | `null`                         |
| `settings.set`   | the settings object            |
| `panel.setTitle` | `{ "title": string }`          |
| `log`            | `{ "message": string }`        |
| `github.api`     | `{ "path": string }`           |
| `openExternal`   | `{ "url": string }`            |

The host answers by evaluating `window.__calm.resolve(id, ok, value)` in the page. With `ok`
true, `value` is the result. With `ok` false, `value` is an error message string.

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
`src/calm.d.ts` holds the types for `window.calm`.

## Development loop

Point CALM at the plugins in this repository and start it:

```sh
CALM_PLUGIN_PATH=./plugins cargo run -p calm-app
```

No rebuild of CALM is needed. HTML and JS changes show up in a newly opened panel. Manifest
changes need a CALM restart, because discovery runs at startup.
Broken manifests are printed to stderr with their path and counted in the toolbar.

## Examples

- `plugins/hello-world`: plain HTML and JS. Shows text and sets the panel title.
- `plugins/github-prs`: TSX. Lists open pull requests for the repositories saved in its
  settings (`{ "repos": ["owner/name"] }`), editable in the panel. Clicking a pull request
  opens it in the browser. Build it first with `bun install && bun run build` in its folder.
