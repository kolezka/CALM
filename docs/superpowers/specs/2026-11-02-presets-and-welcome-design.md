# Presets and the welcome screen

Status: draft for review, 2026-11-02.

## Intent

A new user opens CALM to an empty canvas and has no idea what it does or how to drive it.
The fix is a **default preset**: a ready-made canvas that a first launch opens, with a
welcome panel, the CALM website and the key bindings already laid out. Because the welcome
screen is just a preset, the same mechanism gives users **Save Preset** and **Load Preset**
for their own arrangements, and gives developers a **Show Welcome Screen** item that
replays exactly what a new user sees.

## Goals

- Named presets: canvas arrangements saved to disk and loaded back from the app.
- A built-in default preset `welcome` that ships with CALM.
- A first launch (no `session.json`) opens the default preset.
- Save Preset… and Load Preset… in the app.
- A Developer menu item, Show Welcome Screen, that loads the default preset.
- A first-party welcome plugin and a first-party key bindings plugin.
- A manifest field that keeps a plugin out of the Add menu.

## Non-goals

- Music. The user dropped it (2026-11-02). No audio player, no tracker modules.
- Merging a preset into the current canvas. Loading replaces it (see below).
- Preset sharing, import or export beyond the files on disk.
- Wiring the `ask` restore dialog, `--bench` or the event coalescer.

## Anti-goals

- The welcome screen coming back on its own once a session exists. The autosaved session
  always wins, even an empty one. "First launch" means "no `session.json`"; there is no
  separate first-run marker (user, 2026-11-02: autosave covers it). Accepted edge cases:
  with `session.autosave` off, or after a corrupt session was moved aside and nothing was
  saved since, the next launch finds no session and opens the default preset again.
- Losing work silently. Loading a preset never ends a process without a confirmation.
- A special case keyed on the welcome plugin's id anywhere in the app.

## Decided by the user

1. **Presets are a real feature**, with save and load, not a single first-run file.
2. **Default preset contents:** a welcome panel, a browser panel on the CALM website,
   and the key bindings.
3. **Welcome is a plugin**, and it must not appear in the Add context menu.
4. **Autosave owns the canvas after the first launch.** If a user closes panels and
   quits, the next launch restores that state, not the preset.
5. **Loading a preset closes the current panels and ends their processes.**
6. **Loading asks first** when that would end anything. Confirm ends them, cancel
   changes nothing.
7. **Welcome panel content:** the animated logo from `assets/brand/`, the title, a short
   instruction, 2005-era keygen styling, ending with the line
   "Open Source for life - Long Live Tech".
8. **No music.**
9. **No first-run marker.** Autosave decides: no session file means first launch.
10. **Website:** `https://calm.raqz.link`.

## Design

### Presets are layouts

A preset is a layout file: the existing `LayoutFile` v2 format in `crates/calm-core/src/layout.rs`
(camera, panels in scene order, per-panel overrides). No new format. `save_layout_in` and
`load_layout_from` already exist; `--layout NAME` already opens `layouts/NAME.json`.
"Preset" is the user-facing word; code and files keep "layout".

- **User presets** live in `layouts_dir()` (`$CALM_HOME/layouts`, default
  `~/.config/calm/layouts`).
- **Built-in presets** ship with the app, found the same way built-in plugins are
  (`builtin_dirs_from` in `crates/calm-plugins/src/manifest.rs`): a `presets/` dir in the
  workspace for dev builds and `Contents/Resources/presets` in the bundle.
- **Lookup order:** a user preset shadows a built-in one with the same name, as user
  plugins shadow built-in plugins. Load Preset lists both, marking built-ins.
- `--layout NAME` uses the same lookup, so `--layout welcome` works on a clean machine.

### The default preset

`presets/welcome.json`, three panels, centered on the world origin so `cmd-shift-h` brings
it back:

- **Welcome** plugin panel (`calm-welcome`), the largest, in front.
- **Browser** panel on `https://calm.raqz.link`, the CALM website. The repo's
  `calm.raqz.pl` (`web/astro.config.mjs`, `web/README.md`) is a mistake per the user
  (2026-11-02) and gets fixed separately.
- **Key bindings** plugin panel (`calm-keys`) beside the welcome panel.

Which preset a first launch uses is a setting:
`session.first_launch_preset`, string, default `"welcome"`, `""` means an empty canvas.
Validated: must be a safe layout name (same rule as `layout_path`). Exposed in the
Settings window's Session section.

### Launch behavior

Only the "no session file" branch changes. In `plan_restore` (`crates/calm-core/src/restore.rs`),
`Err(LayoutError::NotFound(_))` for the session currently means "start empty". New rule:

| Situation | Result |
|---|---|
| `session.json` missing, restore on | Load `first_launch_preset` if set and found; else empty |
| `session.json` present, any content (including zero panels) | Restore it, as today |
| `session.json` malformed, moved aside | Empty canvas with the existing notice. Never the preset. |
| `--no-restore` or `restore_on_launch: off` | Empty, as today |
| `--layout NAME` | That layout, as today, now with built-in lookup |
| Preset named in the setting but not found | Empty, with a notice naming it |

Autosave does not write an untouched restored canvas: `AutosaveClock::new` counts the
restored scene as saved (`crates/calm-core/src/autosave.rs:89`) and `save_on_quit` skips an
unchanged scene (`canvas_view.rs:666`). So when a preset opens on first launch, the app
writes `session.json` once right away (when autosave is on), and the second launch restores
the session even if the user touched nothing. Panels a preset opens are ordinary panels: they are
saved, closed and restored like any other.

### Save Preset…

A menu item opens a native prompt for a name (NSAlert with a text field, or equivalent
native sheet). It writes the current canvas with `save_layout_in` into `layouts_dir()`,
through the same `write_layout_file` path the session uses, so URL scrubbing
(`browser_query`) applies. Saving over an existing user preset asks to replace it. A name
that matches a built-in creates a user preset that shadows it; the prompt says so.

### Load Preset…

A menu item opens a native list of presets (user first, then built-in). On choice:

1. If the canvas has panels whose close would confirm today (a live terminal or TUI app,
   or any browser, per `panels.confirm_close`; see `canvas_view.rs:771`), show one
   confirmation that names the counts. Web plugin panels do not confirm on their own today
   and do not trigger the prompt here either (lead's call), but the prompt lists them when
   it shows, for example "Load preset welcome? This closes 3 terminals and 1 browser."
   It must be native or hide native views first (the overlap rule in `CLAUDE.md`).
2. Confirm: close every panel through the normal close path (processes end), then apply
   the layout the way launch restore does (`restore_layout` in `canvas_view.rs`).
3. Cancel: nothing changes.
4. Empty canvas or `confirm_close` off: no prompt.

Autosave then records the loaded state as the session.

### Developer: Show Welcome Screen

A new item in the Developer menu (`crates/calm-app/src/dev_menu.rs`). It runs Load Preset
with `first_launch_preset` (falling back to `welcome` when the setting is empty), with the
same confirmation. It exists so a developer can see the first-launch experience without
deleting `session.json`.

### Hiding plugins from the Add menu

New optional manifest field `"addMenu": false` (default `true`) in `calm-plugin.json`.
`canvas_menu` in `crates/calm-core/src/menu.rs` skips plugins with it off. The plugin still
loads from presets and sessions. Applies to web and TUI plugins alike. Documented in
`docs/plugins.md` and `packages/calm-plugin-sdk`.

### The welcome plugin (`plugins/calm-welcome`)

A static web plugin, no build step needed (plain HTML/CSS/JS like `hello-world`), with
`"addMenu": false`.

- Animated logo: `assets/brand/logo-animated-transparent.svg`, copied into the plugin.
- Title "CALM", one short instruction (for example "Press t and drag to draw a terminal.
  Right-click for more."), the closing line "Open Source for life - Long Live Tech".
- Keygen look: dark background, pixel or bitmap-style font, scrolling or glowing text,
  CRT-style effects. Fonts bundled with the plugin, no network.
- Customization: title, instruction and closing line overridable through plugin settings
  with the defaults above.

### The key bindings plugin (`plugins/calm-keys`)

A web plugin with `"addMenu": false`, listing the bindings in the "In the app" section of
`CLAUDE.md` (t, b, cmd-drag, space-drag, zoom keys, cmd-shift-h, cmd-shift-0, cmd-ctrl-f,
cmd-shift-f, alt, cmd-,).

The list should come from the app, not be hand-written, so it cannot go stale. The builder
picks the mechanism: a bridge method that returns the keymap, or a generated JSON file
written at build time from the binding definitions. If neither fits the current bridge
cleanly, a hand-written list with a test that compares it to the registered bindings is
acceptable.

## Constraints

- Native views draw above GPUI. Any confirmation or prompt is native or hides native views
  first.
- A panel's process ends only through an explicit, confirmed close.
- No credentials in presets. Browser URLs pass through the same scrub as the session.
- Plugins are trusted code; do not call them sandboxed.
- Every new setting and manifest field is validated strictly and documented.
- `.ai/status.md` and `.ai/decisions.md` get updated when this lands (layout save becomes
  wired; the welcome-as-plugin and preset decisions recorded).
- `--smoke` must still pass and should print the first-launch source (preset or empty).

## Left to the builder

- Exact menu placement of Save Preset… and Load Preset… (a new File menu is the obvious
  home).
- The native widgets for the name prompt, preset list and confirmation.
- Panel sizes and positions inside `welcome.json`.
- The visual details of the keygen styling.
- How `calm-keys` gets the binding list (see above).
- Task order and how to split this into PRs.
