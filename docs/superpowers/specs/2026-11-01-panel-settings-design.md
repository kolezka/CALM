# Per-panel settings, borderless panels and Cmd+drag

Status: draft for review, 2026-11-01.

## Why

Today there is no way to change one terminal from the UI. The Settings window has one
Terminal section with the defaults for every terminal. Named profiles in
`settings.json` can override those defaults field by field, but only for new terminals,
only by editing the file, and a profile is shared by every terminal opened with it.
The panel context menu (Minimize, Zoom to
Fit, Bring to Front, Send to Back, Duplicate, Delete) has no Settings item. The user
wants to give one terminal its own colors, and to give any panel its own title and icon,
without touching the others.

The user also wants panels without a title bar ("borderless"). A panel without a bar
has nothing to grab, so this design adds a way to move and reach the menu of any panel
from its content: Cmd+drag.

## Goals

1. Every panel's context menu has a **Settings** item. It opens a native window with the
   settings of that one panel.
2. Settings of one panel change only that panel, the one the menu was opened on.
3. Global settings stay the default for every panel. A panel field the user did not set
   follows the global value, including later changes to it. A field the user set wins.
4. Per-panel fields:
   - every kind (terminal, browser, plugin, TUI): **title**, **icon**, **borderless**;
   - terminal also: everything in today's Terminal section that is about look: custom
     title bar colors (accent, title bar, title) and custom terminal colors (background,
     foreground, colors 0 to 15), with the same theme buttons (CALM Neon, Solarized Dark,
     Ghostty Default).
5. Icon can be: 1 to 4 characters of text, emoji included; a PNG or SVG file chosen
   from disk; or one of the built-in panel icons.
6. **Cmd+drag** on the content of any panel moves the panel, borderless or not.
   **Cmd+click** without movement goes to the panel unchanged (Ghostty and the browser
   open links on Cmd+click; that keeps working). **Cmd+right-click** on the content
   opens the panel menu.
7. The global Settings window is split into **tabs**, one of them **Terminal**, holding
   the global defaults for all terminals.
8. Per-panel settings persist with the session and come back after a restart.

## Non-goals

- Saved, reusable profiles made from the UI. Named profiles already exist in
  `settings.json` and stay as they are; making and assigning them from the UI is a
  possible later step, not part of this work.
- New drawn icons. The built-in list is the icons CALM already draws per kind.
- Fonts, font size and padding per terminal. gpui-libghostty 0.3.1 takes only colors per
  surface (`calm-plugins/src/tui.rs`, `PLANNED_FIELDS`).
- Per-panel command, args, cwd or env. Those decide how the process starts, not how
  the panel looks, and changing them would mean restarting the process.

## Anti-goals (would be a failure even if it works)

- A change in one panel's Settings that changes another panel.
- A panel that can end up with no way back: borderless with no way to move it, close it,
  or open its Settings again.
- Cmd+click on a link in a terminal or a page that stops opening the link.
- A setting change that restarts or kills a running terminal process. The project rule
  stands: only an explicit, confirmed close ends a process.
- GPUI drawing over a native view (Ghostty, WKWebView). Panel Settings is a native
  window, like global Settings.

## Decided by the user

- Scope of a panel change: only the panel the menu was opened on.
- Settings item for every panel kind, not only terminals.
- Browser and plugin panels: title and icon to start with.
- Icon input: typed characters (emoji) and a file from disk. Built-in list is welcome
  if it already exists.
- Borderless: a per-panel option that hides the title bar.
- Moving: hold a modifier and drag the content. Applies to every panel.
- Click and drag are told apart: a modifier click with no movement goes to the panel.
- The modifier is **Cmd** alone.
- Global Settings gets a Terminal tab with defaults for all terminals; a panel's own
  settings override them.

## Decided by the lead (the user may change these)

- **Inheritance per field.** Each per-panel field is either inherited or set. The
  window shows the effective value for an inherited field, marked as inherited. Each
  field has a way back to inherited, and the window has one **Reset to global** button
  for the whole panel. Only set fields are saved.
- **Resolution order for a terminal:** panel override, then the terminal's profile (if
  it was opened with one), then the global terminal defaults, then the theme. This adds
  one layer on top of `TerminalSection::resolve` (`calm-core/src/terminal_profile.rs`).
- **Live where possible.** Colors apply to the running terminal at once through
  `TerminalThemeState::apply`, which keeps the process (see `.ai/decisions.md`,
  "Terminal profiles"). Anything that cannot change on a live surface shows a note,
  "applies to new terminals". Title, icon and borderless apply at once for every kind.
- **Icon file is copied** into the CALM settings folder (`~/.config/calm`, or
  `CALM_HOME`), under a folder for panel icons, so the panel keeps its icon if the
  original moves. The saved value is the path relative to the settings folder, the
  same rule profile icons use. Allowed types: PNG and SVG, as today.
- **Built-in icons:** "Default" (the panel kind's own icon) plus the existing kind
  icons from `chrome::PanelIcon`.
- **Modifier is a setting**, default `cmd`, validated strictly, in the Canvas or Panels
  tab of global Settings. Allowed values: `cmd`, `ctrl`, `alt`, `cmd+ctrl`,
  `cmd+alt`, `ctrl+alt`. There is no `off`: a borderless panel has no other way to be
  moved or to reach its menu, so turning the gesture off would strand it. Reason for
  the setting: the customization rule in `CLAUDE.md`.
- **Restore keeps a terminal's profile.** The session stores the profile id the
  terminal was opened with, and restore uses it. A profile id that no longer exists
  resolves like no profile, with a notice, as `TerminalSection::resolve` already does
  for unknown ids. Reason: without it, inherited fields of a restored terminal change
  after a restart even when nothing was edited.
- **Drag threshold** is the existing `CLICK_SLOP` (4 points). Below it on release it
  is a click; at or past it, a move.
- **Cmd+right-click** opens the CALM panel menu, not the page's or Ghostty's own menu.
- **Global Settings tabs:** one tab per existing section (Background, Grid, Canvas and
  Snapping, Panels, Terminal, Session, Developer). Content of each tab is today's
  section, unchanged, apart from the new modifier setting.
- **Panel Settings window:** one window per panel at most. Opening Settings for a
  panel whose window is open brings it to front. Closing the panel closes its window.
  The window title names the panel (its title and kind).
- **Duplicate copies overrides.** A duplicated panel starts with the same per-panel
  settings, since the user duplicated what they see.

## How it fits the current code

What exists:

- `calm-core::scene::PanelOverrides { title, icon, accent_color }` is saved in the
  layout and session file (`calm-core/src/layout.rs`) and restored
  (`canvas_view.rs`, around line 381). Its comment says it "always wins over what the
  panel sets". It is **not read when drawing**: chrome colors and icons come from
  `CanvasView::panel_look` (`canvas_view.rs`, around line 1230). For a plain terminal
  that look comes from the resolved profile; for a TUI app from its run and manifest;
  for browser and plugin panels it is the default look. The per-panel overrides must
  lay over each of those three sources. Wiring them into the drawn chrome is part of
  this work.
- **A restored terminal loses its profile.** Restore opens every terminal with profile
  `None` (`canvas_view.rs`, around line 402), so it falls back to the current
  `default_profile`. The profile id lives only in `TerminalMeta`, not in the session.
  Since a panel's inherited fields resolve through its profile, this work saves the
  terminal's profile id in the session and restores with it (lead's call, see below).
- `.ai/decisions.md`, "Panel overrides contract": new appearance fields grow
  `PanelOverrides` through the persistence plan, no `deny_unknown_fields`, so an older
  CALM still opens a file with more fields. This design adds fields there: borderless,
  title bar colors and terminal colors. Field names are the builder's call within that
  contract and the existing snake_case style.
- Terminal look resolution: `TerminalSection::resolve` in
  `calm-core/src/terminal_profile.rs`. The panel's profile is kept beside the scene,
  not in `PanelKind` (decision "Terminal profiles").
- Settings form: `calm-core/src/settings_form.rs` (pure, tested) builds `Section`s;
  `calm-native/src/settings_window.rs` renders them in one NSWindow. A per-panel form
  should reuse the same `Section`/`Field`/`Control` model so its logic stays testable in
  `calm-core`. The window needs an "inherited" state per field and a reset control,
  which the current form model does not have.
- `calm-native::settings_window::choose_image_file` already opens a file chooser for
  images.
- Context menu: `calm-core/src/menu.rs` builds panel items; `canvas_view.rs` handles
  `MenuCommand`s. Today the panel menu opens only on the chrome strip or a minimized
  chip (`calm-core/src/interaction.rs`, around line 489).
- Mouse events on native views: `calm-native/src/browser.rs` already installs an
  `NSEvent` local monitor for the address field. A Cmd+drag gesture over any native
  view needs the same kind of monitor, owned by the canvas, that holds the mouse-down
  while the gesture is undecided.

## Behavior details

### Cmd+drag and Cmd+click

- Cmd+left-down on a panel's content starts an undecided gesture. Nothing is sent to
  the panel yet.
- Movement past `CLICK_SLOP` turns it into a move of that panel. The panel never sees
  the press. The move follows the same rules as a title bar drag: snapping, `cmd-shift`
  disables snapping, joins.
- Release before the threshold is a click: the held press and the release are both
  delivered to the panel, so Cmd+click on a link still opens it. A long Cmd+press with
  no movement reaches the panel only at release. That is the accepted cost.
- Cmd+right-down on content opens the CALM panel menu at the cursor.
- The gesture works on every panel kind, and on the title bar too (where a plain drag
  already moves).

### Borderless

- The title bar is not drawn and takes no space; the content fills the panel rect.
  Whether the stored rect keeps the bar height or not is the builder's call, as long
  as toggling borderless does not move or resize the terminal's grid unexpectedly and
  the setting survives save and restore.
- Resize edges still work.
- Focus ring and the minimized chip still show, so a borderless panel can be found
  and restored.
- Moving: Cmd+drag. Menu: Cmd+right-click. Settings is in that menu, so borderless can
  always be turned off.

### Panel Settings window

- Opened from the panel menu item **Settings**, on every kind, minimized included.
- Fields by kind as in Goals 4 and 5. Each field shows its effective value and whether
  it is inherited.
- Changes apply as they are made, like global Settings (no Save button), and mark the
  session dirty so autosave writes them.
- Validation is as strict as for global settings: title length uses the existing
  `MAX_TITLE_CHARS`, icon text 1 to 4 characters, colors `#rrggbb`.

### Global Settings tabs

- Tabs replace the single scroll. Each tab is one of today's sections.
- The Terminal tab says its values are defaults that a panel's own settings override.

## Testing expectations

- `calm-core` unit tests for: override resolution order (panel over profile over
  defaults over theme), each field inherited versus set, reset, save and restore of
  every new override field, an older file without the fields, a file with an unknown
  override field, the modifier setting's validation, and the click versus drag decision
  at the threshold.
- `--smoke` must still pass. If the smoke run can report the new state (for example a
  borderless count), add a line.
- GUI behavior (Cmd+drag over a live terminal and page, Cmd+click on a link, the panel
  Settings window, tabs) cannot be driven by agents. It goes on the by-eye list in
  `.ai/status.md` for the user to check.

## Left to the builder

- Names of new override fields and of the panel icons folder.
- How the native window shows "inherited" and the reset control.
- Whether panel Settings and global Settings share one window class.
- How the held press is replayed to Ghostty and WKWebView, as long as Cmd+click on a
  link still works in both.
- Order of the work and how it splits into PRs.
