# See-through terminals

Status: draft for review, 2026-11-02.

## Why

The user wants a terminal panel to show what is under it: the canvas (background and
grid) or another panel. Panel Settings has no way to do that today. Picking a background
color with opacity in the macOS color picker looks like it should work, but the opacity
is dropped (see "How it fits the current code").

## Goals

1. **Ghostty background opacity shows through.** When the user's Ghostty config sets
   `background-opacity` below 1, a CALM terminal's background is see-through and its text
   stays solid. Under it the user sees the canvas, or the panel below it.
2. **Per-panel fade.** Panel Settings for a terminal gets an opacity control that fades the
   whole terminal content (background, text and cursor together). It works per panel,
   applies live, and is saved with the session.
3. **A global default for the fade** in the Terminal tab of global Settings, which a
   panel's own value overrides. Same inheritance as the #57 per-panel fields: inherited or
   set, each with a way back to inherited, covered by **Reset to global**.
4. **Color wells for terminal colors stop offering opacity** that CALM cannot apply.

The two layers stack: a terminal can have Ghostty's 50% background and a CALM fade of 80%.

## Non-goals

- **Blur.** The user said it is not required. Ghostty's own `background-blur` blurs the
  whole macOS window, not one panel. A per-panel blur would be a CALM-owned frosted layer
  and is a later step.
- **Browser and plugin panels.** Terminals only. Most web pages paint their own opaque
  background, so a see-through browser background would do nothing on most sites.
- **Per-panel background opacity with solid text.** Not possible on stock
  gpui-libghostty 0.3.1 (see constraints). The per-panel control is the fade.
- **CALM editing the user's Ghostty config.** The global background opacity is the user's
  own `background-opacity` line. CALM reads its effect, it never writes that file.

## Anti-goals (a failure even if it works)

- A panel that can be faded until it is lost. The fade has a floor.
- A setting change that restarts or kills a terminal process.
- GPUI drawing over a live native view. That rule (`CLAUDE.md`) stands.
- A control that looks like it works and does nothing, like today's opacity slider in the
  terminal color picker.

## Constraints

- **Stock gpui-libghostty 0.3.1, no patch, no fork.** Decided by the user. 0.3.1 is the
  newest on crates.io (checked 2026-11-02). Its only per-terminal input is `TerminalTheme`:
  background, foreground and 16 palette colors, RGB with no alpha (`TerminalColor`). The
  theme file it writes is fixed (`theme_config_contents`, private). The shim's
  `update_theme` would take any config path, but the surface pointer is a private field of
  `NativeSurface`. So CALM cannot send Ghostty an opacity per terminal.
- What does reach Ghostty: with `panels.terminal_user_config` on (the default), the shim
  loads the user's Ghostty config (`ghostty_config_load_default_files`, then
  `ghostty_config_load_recursive_files`) before CALM's color file. A color change through
  `TerminalThemeState::apply` takes the same path, so it keeps `background-opacity`.
  With `terminal_user_config` off, Ghostty defaults apply and there is no background
  opacity.

## Evidence from the throwaway test (2026-11-02)

A scratch build with the GPUI fill under panels removed, the user's Ghostty config copied
to a temp `XDG_CONFIG_HOME` with `background-opacity = 0.5` added, and `alphaValue` set
on each native clip view. Checked by the user, by eye:

- With `background-opacity = 0.5`, the canvas shows through a CALM terminal. **Works.**
- With a 0.7 fade on the clip view, the whole terminal fades. The user: "its good".
- A background color picked in Settings applies; its opacity is ignored.
- Where two terminals overlap, the user saw the back terminal through the front one.

Measured by the agent in smoke (window hidden, so no frames): CALM's clip view, the zoom
view and Ghostty's `IOSurfaceLayer` all report `isOpaque = false`.

Not tested: what the 2 s previews of distant terminals look like with transparency.

## How it fits the current code

- **CALM paints a solid fill under every panel's content.** In `CanvasView::paint_panel`
  (`crates/calm-app/src/canvas_view.rs`): the ring quad over the whole body filled with
  `colors.bar`, then a `theme::CONTENT_PLACEHOLDER` fill over the content rect. With these,
  a see-through terminal shows the fill, not the canvas. The test removed both for every
  panel. The real change must keep the title bar and ring looking as today and only stop
  filling under a terminal's content. CALM cannot read Ghostty's opacity through the
  crate, so the rule is: no GPUI fill under a **live** terminal's content. An opaque
  Ghostty surface covers that area anyway. The panel shadow (`canvas_view.rs`, the
  `blur_radius` shadow) may show through as darkening; the builder decides whether it
  stays under see-through content.
- **Occlusion.** `calm_core::occlusion::native_visible_region` removes every panel in
  front from a back panel's visible region, so by the code the back terminal should be cut
  out under the front one. The user saw it through anyway. **The builder must find out
  why** before relying on it. Wanted behavior: behind a see-through or faded terminal, the
  panel below shows its live content in the content area, and the front panel's title bar
  and ring are still never covered by a native view behind it. If the mask is right and
  the user's view came from a stale mask, the occluder for a see-through terminal becomes
  its title bar and ring only, not its content.
- **Per-panel fields.** `calm_core::scene::PanelOverrides` (saved in layout and session,
  no `deny_unknown_fields`, per the "Panel overrides contract" decision) gets one new
  terminal-only field for the fade. Global default in the terminal section of
  `Settings`. Form rows in `calm-core::panel_form` and `settings_form`, rendered by
  `calm-native::settings_window`. Validation is strict, like other numbers.
- **Applying the fade.** `NSView::setAlphaValue` on the panel's `NativeChild` clip view,
  which is how the test did it. It is live, keeps the process, and does not touch GPUI
  chrome, so the title bar stays solid.
- **Color wells.** `rgb_from_ns` (`settings_window.rs`, `BindingKind::Color`) keeps RGB
  only. Terminal colors are `#rrggbb` by design ("Ghostty colors have no alpha",
  `terminal_profile.rs`). Terminal color wells should not show the opacity slider.
  The same `rgb_from_ns` serves every CALM color well, chrome included, so no well honors
  alpha today. Plan-time call: hide the opacity slider app-wide
  (`NSColor::setIgnoresAlpha`), and bring it back per well when chrome alpha is wired.

## Decided by the user

- See-through means the canvas or the panel below shows through.
- Terminals only.
- Stay on stock gpui-libghostty, no patch.
- Both layers: Ghostty `background-opacity` from the user's config, plus a CALM per-panel
  fade that fades text too.
- Blur is not required.
- The fade as tested at 0.7 looks good.

## Decided by the lead (the user may change these)

- **Fade range 20% to 100%, default 100%.** The floor keeps a panel findable. Stored as
  a number `0.2..=1.0`.
- **The fade covers terminal content only.** The title bar, ring and minimized chip stay
  solid, so the panel is always easy to grab and find.
- **The fade follows #57 inheritance**: global default in the Terminal tab, per-panel
  override with reset, only set values saved, live apply, autosave marks dirty.
- **TUI app panels count as terminals.** They are Ghostty surfaces, so they get the fade
  and the no-fill rule too.
- **Duplicate copies the fade**, as #57 does for other overrides.
- **Previews should look like the live panel**: a faded terminal's preview is drawn at the
  same opacity. If the snapshot does not keep Ghostty's alpha, the preview shows the
  background solid, and that is accepted and written in `.ai/status.md`.
- **The Terminal tab says** that background opacity comes from the user's Ghostty config
  (`background-opacity`) and only applies with the Ghostty user config on. One line, no
  control, since CALM cannot set it.

## Related finding, out of scope

Chrome colors (accent, title bar, title) accept `#rrggbbaa` in the data model, but the same
`rgb_from_ns` drops the picker's alpha for them too. This is not part of this work. It goes
in `.ai/status.md` as a known gap.

## Testing expectations

- `calm-core` unit tests: the fade field's validation (range, NaN, wrong type), inherited
  versus set, reset, save and restore, an older file without the field, a fade override
  on a non-terminal panel rejected, and the occluder change if there is one.
- `--smoke` still passes on a visible window. Add a line with the applied fade per
  terminal if smoke can read it back from the clip view (`alphaValue`).
- By eye, by the user (agents do not drive the GUI): canvas through a terminal with
  Ghostty `background-opacity = 0.5`; a back terminal through a front one; the fade at
  20%, 70% and 100%; title bar solid; preview of a faded terminal; no CALM color well shows
  an opacity slider.

## Left to the builder

- The fade field's name, within the snake_case override style.
- How preview images get drawn at an opacity in GPUI.
- What shows in a live terminal's content rect before Ghostty's first frame, now that the
  fill is gone there.
- The cause of the occlusion mismatch, and the fix.
- Order of the work and how it splits into PRs.
