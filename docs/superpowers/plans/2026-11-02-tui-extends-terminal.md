# TUI Extends Terminal Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:executing-plans. Steps use checkbox (`- [ ]`) syntax.

**Goal:** A TUI app panel becomes a terminal with an app attached, not a separate kind. Every
terminal feature (per-panel Settings sections, title bar colors, terminal colors, presets, live
recolor, profiles) works on TUI panels. A new Add item "Run Command…" opens a terminal running a
typed command (for example `htop`), with the same exit and restart handling TUI apps have.

**Architecture:** `calm-core` drops `PanelKind::Tui`; `PanelKind::Terminal` gains
`app: Option<TerminalApp>`. One resolver stacks looks and launch for every terminal. `calm-app`
keeps one per-terminal state (`TerminalMeta`) that may own a process supervisor, so plain shells,
typed commands and apps share one spawn, refresh, recolor and exit path. `calm-native` gains a
text prompt (NSAlert with a text field).

**Branch:** `feat/tui-extends-terminal`, worktree `.worktrees/tui-extends-terminal`, from `origin/main` at `2d3f2d9`.

## Kinds of terminal

| Panel | Command from | Saved as |
|---|---|---|
| Terminal | profile (shell by default) | `type=terminal` |
| Command ("Run Command…") | typed by the user | `type=terminal`, `command: "htop"` |
| App (e.g. Top) | plugin manifest | `type=tui`, `plugin_id`, `plugin_version`, optional `profile` |

## Global constraints

- **Saved format.** `type=tui` keeps `plugin_id` and `plugin_version` and gains only an optional
  `profile` (serde default, skipped when `None`). An app panel never serializes `command`, `cwd`
  or `env`. A current-format `type=tui` file must round-trip byte-for-byte when `profile` is `None`.
  Earlier lead ruling: no rename of `type=tui`.
- **Plugin API unchanged.** `panels.create` with kind `tui`, `PanelInfo.kind == "tui"` for app
  panels, `terminal` for the rest. Events `terminalExited` and `tuiExited` keep their meaning.
- **Look precedence** (later wins): terminal defaults, profile, manifest appearance (apps only),
  panel overrides. Close-confirmation colors stay fixed (decision "TUI look comes from the manifest").
- **Launch.** Apps: command, args, cwd, shell flag and restart policy come from the manifest only.
  Env merges key by key: terminal defaults, then profile, then manifest. Command terminals: the
  typed command replaces the profile's command and args; cwd and env as for a plain terminal.
- **Exit handling.** Any terminal with a command (app or typed) has a supervisor. Apps use the
  manifest policy. Typed commands use the new setting `terminal.command_restart`
  (`"never" | "on_exit" | "prompt"`, default `"prompt"`), validated strictly. Plain shell
  terminals keep today's behavior.
- **Prompt.** Native NSAlert, title "Run Command", one text field, buttons Run and Cancel, and the
  note "The command is saved in the session file. Do not put secrets in it." Empty or blank
  input does nothing. Nothing GPUI-drawn appears over native views.
- **Title.** Templates accept `{name}` (app name, or the command's first word for typed
  commands) beside `{n}` and `{cwd}`. App default title stays the manifest's.
- All `unsafe` stays in `calm-native` with `// SAFETY:` comments. Comments short and plain, no
  em or en dashes.
- A setting change never restarts or kills a process.

## Tasks

### Task 1: core model (calm-core)

- [ ] `scene.rs`: remove `PanelKind::Tui`; add `TerminalApp { plugin_id, plugin_version }` and
  `app: Option<TerminalApp>` on `Terminal`. Serialize `Terminal { app: Some }` as `KnownKind::Tui`
  (plus optional `profile`), deserialize it back. Constructor `PanelKind::app(plugin_id, version)`
  and helpers `is_terminal()`, `app()`.
- [ ] Walk every `PanelKind::Tui` and `PanelKind::Terminal` match in `calm-core` (`rg -n
  "PanelKind::(Tui|Terminal)" crates`). For each, decide: same for all terminals, or app-aware.
  Notable: `menu.rs` (Duplicate, Restart, plugin items), `restore.rs` (missing app plugin),
  `plugin_menu.rs` (`KindName::Tui`, owning plugin id), `chrome.rs` icon, `layout.rs` tests.
- [ ] `PanelOverrides::validate`: `terminal_colors` allowed on every terminal, apps included.
- [ ] `terminal_profile.rs`: a resolver that lays an app's manifest appearance and env between
  profile and overrides (`ResolvedTerminal` for apps). `{name}` in title rendering.
- [ ] `panel_form.rs`: `panel_effective` takes the app layer (passed in by the app; calm-core does
  not read manifests) so app panels show title bar colors, terminal colors and presets.
- [ ] Settings: `terminal.command_restart` with strict validation and a settings form control.
- [ ] Tests: round-trip of a current-format `type=tui` fixture (unchanged bytes); `type=tui` with
  `profile`; validate accepts `terminal_colors` on an app; precedence test with all four layers
  set for one color and one chrome field; env merge order; `{name}`; invalid `command_restart`.

### Task 2: one terminal path (calm-app)

- [ ] Merge `TuiRun` into the terminal state: `TerminalMeta` gains `run: Option<...>` holding the
  supervisor, status file, launch counter and, for apps, the plugin. Remove the parallel
  `tui_runs` list. Keep `tui_host.rs` only for app-specific pieces (manifest spec, status wrapper)
  or fold it in.
- [ ] One spawn path for shell, typed command and app; one refresh path
  (`refresh_terminal`) for title, chrome, icon and live recolor, used by app panels too.
  `panel_settings.rs` passes the app layer into `panel_effective`.
- [ ] Exit, restart and exited-state drawing work for typed commands with
  `terminal.command_restart`. Restart keeps the panel id (z-order fix from #40: invalidate
  `native_order` before spawn).
- [ ] `plugin_host.rs`: kind names, `panels.create` for `tui` and `terminal` with `command`,
  `PanelInfo`, events.
- [ ] Restore: a typed-command terminal reruns its command; an app panel whose plugin is gone
  behaves as today.

### Task 3: Run Command (calm-native, calm-core menu, calm-app)

- [ ] `calm-native`: `prompt_text(title, message, note, placeholder) -> Option<String>` using
  NSAlert with an NSTextField accessory, modal on the main window.
- [ ] `calm-core/menu.rs`: Add gets "Run Command…" after Terminal. Label from a constant; a
  menu test pins its position.
- [ ] `calm-app`: on the item, show the prompt (defer out of the menu callback like other native
  windows), then create `Terminal { command: Some(text), profile: None, app: None, cwd: None }`
  where Add places new panels.

### Task 4: smoke and docs

- [ ] Smoke: an app panel's Settings form has "Title bar colors" and "Terminal colors"; setting
  an app panel color recolors it live (`applied_colors` set); a typed-command terminal spawns,
  is saved with its command, and restores; TUI restart still keeps z-order. Smoke never drives the
  NSAlert; it calls the creation path directly.
- [ ] `.ai/decisions.md`: replace "A TUI app is `PanelKind::Tui`" with the new decision and its
  cost. `.ai/status.md`, `docs/plugins.md`, `docs/configuration.md` (`command_restart`, `{name}`,
  precedence), `CLAUDE.md` "In the app" (Run Command).

## Gates

While iterating: `cargo test -p <crate>`. Before handing back: `cargo clean -p calm-core -p
calm-native -p calm-plugins -p calm-app`, `cargo check --workspace --tests --examples`, `gmake
ci`, `cargo run -p calm-app -- --smoke; echo rc=$?` (rc must be 0; a flake is not a pass).

## Review focus

1. Saved files: an old `type=tui` session opens and saves unchanged; no app panel ever writes
   `command`, `cwd` or env.
2. Precedence: a manifest color beats the profile; a panel override beats the manifest; resetting
   one panel color falls back to the manifest, not to the profile.
3. App restart and typed-command restart keep panel id, z-order and Settings window.
4. Every former `PanelKind::Tui` arm has an equivalent `app()` check where behavior differs
   (menus, plugin ownership, API kind names, restore).
