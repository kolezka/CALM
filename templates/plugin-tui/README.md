# CALM TUI app template

A TUI app is a terminal panel that runs one command, such as `slk`, `htop` or `lazygit`.
There is no HTML and no build step: the folder holds `calm-plugin.json` and, if you want one,
an icon image.

1. Copy this folder into `~/.config/calm/plugins/` and rename it.
2. In `calm-plugin.json`, set a unique `id` (lowercase letters, digits, `-`), a `name` and the
   `command` to run. Remove the fields you do not need.
3. Restart CALM. The app shows in the right-click Add menu, next to Terminal.

With `"shell": true` (the default) the command runs through your login shell, so it finds
programs on the `PATH` your shell profile sets up, even when CALM was started from Finder.

The look is yours to set: `title` (with `{name}`), `icon` (text, or a `.png` or `.svg` in this
folder), `accentColor`, `titleBarColor`, `titleColor` (`#rrggbb` or `#rrggbbaa`) and
`terminalColors` (background, foreground and 16 palette colors). Remove any of them to use the
theme. The "Customizing TUI apps" section of `docs/plugins.md` lists every rule.

Do not put tokens or passwords in `env`: the values are visible in the process list. Let the
app keep its own credentials, as most TUIs do.

The full reference is the "TUI apps" section of `docs/plugins.md` in the CALM repository.
