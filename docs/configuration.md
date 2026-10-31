# Configuration

CALM keeps its settings in one JSON file:

- `$CALM_HOME/settings.json` when `CALM_HOME` is set,
- else `~/.config/calm/settings.json`.

The Settings window (`cmd-,`) writes this file on every change. You can also edit it by hand
while CALM is closed. Missing fields take their default, so a file may hold only what you
change. A wrong value is an error that names the field. CALM then moves the file aside to
`settings.json.invalid-<n>` and starts with the defaults, so nothing you wrote is lost.

This page covers the `terminal` section. The other sections (`background`, `grid`, `snap`,
`panels`, `plugins`) are all editable in the Settings window.

## Terminals

The `terminal` section sets how plain terminals look and start: the ones from the `t` tool and
from Add > Terminal. TUI app panels are configured in their manifest instead; see
[Customizing TUI apps](plugins.md#customizing-tui-apps). Both use the same color rules and
the same chrome.

Fields at the top of `terminal` are the defaults. `profiles` holds named presets with the
same fields. For each field, a terminal takes the profile's value, else the default's, else
the theme's (chrome) or the built-in behavior (launch).

### Full example

```json
{
  "version": 2,
  "panels": { "terminal_user_config": true },
  "terminal": {
    "title_template": "{n}: {cwd}",
    "icon": ">_",
    "accent_color": "#48f5e7",
    "cwd": "inherit",
    "env": { "EDITOR": "nvim" },
    "default_profile": "work",
    "profiles": [
      {
        "id": "work",
        "name": "Work",
        "cwd": "~/work",
        "env": { "AWS_PROFILE": "dev" }
      },
      {
        "id": "neon",
        "name": "Neon",
        "title_template": "neon {n}",
        "icon": "icons/neon.svg",
        "accent_color": "#ff4fd8",
        "title_bar_color": "#090d1d",
        "title_color": "#e7f6ff",
        "use_ghostty_user_config": false,
        "terminal_colors": {
          "background": "#090d1d",
          "foreground": "#e7f6ff",
          "palette": [
            "#172042", "#ff4f7b", "#48f5a0", "#ffd84f", "#4f8bff", "#ff4fd8", "#48f5e7", "#c8d5e9",
            "#344a72", "#ff7b9e", "#7bffc0", "#ffe57b", "#7ab0ff", "#ff8be6", "#8bfff3", "#f2f7ff"
          ]
        }
      },
      {
        "id": "monitor",
        "name": "htop",
        "command": "htop",
        "args": ["-d", "10"],
        "cwd": "home"
      }
    ]
  }
}
```

With this file, `t` and Add > Terminal open a "Work" terminal in `~/work`, titled like
`1: ~/work`, with `EDITOR` and `AWS_PROFILE` set. Add > Terminal Profiles > Neon opens one
with the neon chrome and palette. Image icons live in the settings folder, so this one is
`~/.config/calm/icons/neon.svg`.

### Look

- `title_template`: the title bar text. Default `"Terminal"`. Two placeholders:
  - `{n}`: the terminal's number, counting plain terminals opened since CALM started, from 1.
  - `{cwd}`: the folder the terminal started in, with your home folder shown as `~`. It does
    not follow later `cd` commands yet; that needs the libghostty shim patch.

  Any other `{` or `}` is an error. It may not be blank or hold control characters, and is at
  most 120 characters.
- `icon`: text of 1 to 4 characters, or a path ending in `.png` or `.svg` relative to the
  settings folder. A path that is absolute or uses `..` is an error. A missing file, one that
  resolves through a symlink to outside the folder, or one that does not decode is logged on
  stderr once, and the panel shows the terminal icon.
- `accent_color`: the focus ring, the divider to a joined neighbor while focused, and a text
  icon. `title_bar_color` and `title_color`: the bar and its text, with or without focus.
  Each is `#rrggbb` or `#rrggbbaa`. During a close confirmation the bar always uses the
  theme's warning colors.
- `terminal_colors`: the Ghostty colors. `background`, `foreground` and all 16 `palette`
  entries are required, each `#rrggbb` with no alpha. Other keys are errors.
  Ghostty 0.3.1 takes colors only as a full set, so a partial set is refused rather than
  filled in with made-up values.
- `use_ghostty_user_config` (profiles only): read your Ghostty config for this profile. The
  default for all terminals is `panels.terminal_user_config` (Settings > Panels), so this key
  at the top of `terminal` is an error.

How the two color sources combine: `terminal_colors` is laid over the base config. With the
user config on, the base is your Ghostty config, so its fonts and keys stay and only the
colors change. With it off, the base is Ghostty's built-in defaults.

Fonts, font size and padding cannot be set per terminal yet: gpui-libghostty 0.3.1 takes only
colors per surface. They come from your Ghostty config for every terminal.

### Launch

- `command`: the program to run. Default: your login shell (`$SHELL`, else `/bin/zsh`).
  It may not be blank, start with `-`, or contain `=`.
- `args`: its arguments. Only allowed next to `command` in the same profile or in the
  defaults; the two always come from the same place.
- `cwd`: `"home"` (default), `"inherit"` (the folder the focused terminal started in, else
  home), or a path starting with `/` or `~`.
- `env`: variables to add, merged key by key with the profile winning. Keys are letters,
  digits and `_`, not starting with a digit. The Settings window shows the keys with masked
  values, and CALM never prints the values. They do reach the process's arguments through
  `/usr/bin/env`, so other programs running as you can see them in `ps`. Do not put secrets
  here that you would not put on a command line.

### Profiles

- `id`: 1 to 64 of `a-z`, `0-9`, `-` and `_`, starting with a letter or digit, unique.
- `name`: shown in the Add menu. Not blank, at most 120 characters.
- `default_profile`: the id used by `t`, Add > Terminal and plain duplicates. It must name a
  profile. Without it those use the defaults alone.

When any profile exists, the Add menu gets a Terminal Profiles submenu with one item per
profile. Duplicate keeps the source terminal's profile.

### What changes live

- Title, icon and chrome colors of open terminals follow every settings change at once.
- A changed `terminal_colors` recolors running terminals in place through libghostty's theme
  update; the process and the screen keep running.
- Removing `terminal_colors`, or changing `use_ghostty_user_config`, applies to new terminals
  only: libghostty can apply a new theme but not drop one or reload the base config.
- Launch fields (`command`, `args`, `cwd`, `env`) apply to new terminals.

### In the Settings window

Settings > Terminal edits the defaults: a switch for custom title bar colors with three color
wells, a switch for custom terminal colors with background, foreground and 16 palette wells,
Load buttons for three built-in palettes (CALM Neon, Solarized Dark, Ghostty Default), and the
default profile. The environment shows keys with masked values.

Not in the window yet: title template, icon, launch fields, and adding, duplicating,
renaming or deleting profiles. These need text input the window does not have. Edit
`settings.json` for them.

Per-terminal edits after a terminal is open (rename, change icon, pick an accent from the
right-click menu) are not built yet.
