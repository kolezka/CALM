# CALM architecture notes

## Terminals (Task 8)

Each terminal panel owns a `calm-native` `NativeChild` (clip view plus zoom view) and a
Ghostty surface parented to the zoom view at `(0, 0, logical size)`. The adapter in
`crates/calm-app/src/terminal/adapter.rs` is adapted from gpui-libghostty 0.3.1 (MIT).

- Every canvas paint computes `place_native(content_rect, camera, canvas)` per terminal,
  shifts it into window points and calls `TerminalPanel::set_placement`. `None` (minimized
  or off screen) hides the host. The process keeps running.
- Zoom only changes the host's frames. `surface.set_frame(0, 0, logical size, scale)` runs
  on every placement; the surface skips unchanged frames, so only a panel resize reflows the
  grid and a display scale change still reaches Ghostty.
- Native views stack in creation order. When the scene's back-to-front order of terminals
  changes, the canvas calls `NativeChild::bring_to_front` for each one, back to front.
- An invisible GPUI div at the terminal's clipped screen rect holds focus and receives mouse
  and key events. Mouse positions go through `unzoom_mouse_position` before Ghostty sees them.
- Keys typed in a terminal stop propagating, and `t`, `b`, `escape`, `enter` are bound to
  `NoAction` in the `Terminal` key context, so canvas shortcuts do not steal them. Zoom and
  fullscreen bindings still apply.
- Close on a live terminal asks first: the chrome strip shows `Close? enter / esc` and focus
  moves to the canvas so Enter and Escape reach it. Closing drops the entity, which frees the
  surface (ends the process) and then removes the host views.

## Preview and minimize (Task 9)

- Each paint, each terminal gets `decide_mode(zoom, content rect on screen, viewport,
  minimized, modal_open)` from `calm-core/src/preview.rs`. `modal_open` is the close
  confirmation or an open context menu.
- `Live`: hidden rendering off, native view placed as before.
- `Preview` (zoom below 50 % or a modal up): the terminal takes a snapshot when
  `refresh_due` says so (none yet, or the last attempt is at least `PREVIEW_REFRESH_SECS`
  old, no slack), then hides its native view and keeps hidden rendering on. Entering
  preview does not force a snapshot, so readbacks stay at one per period per terminal
  however often modes flip; the last image shows until the next one is due. The canvas
  paints the snapshot scaled into the content rect with GPUI. A canvas timer repaints
  every `PREVIEW_REFRESH_SECS` plus 100 ms while any terminal is in preview.
- The image is a `PreviewImage::{None, Fresh, Stale}`. A failed refresh keeps the last good
  image as `Stale`, the chrome strip shows `preview stale`, and the error logs once per
  terminal. The flat rect with the title shows only when no snapshot ever succeeded. The
  image is kept through `Live` and `Hidden` and freed from the atlas on close or replace.
- `Hidden` (minimized or off screen): native view hidden, no snapshots, no hidden rendering.
- Only `Live` terminals get the invisible input div, so clicks and scrolls on a preview
  reach the canvas. Render decides the mode from the current scene with the same
  `terminal_mode` helper paint uses, and paint notifies the view when a mode changes.
- Minimized panels draw only the 200x28 chip. A left click on the chip, the chrome minimize
  button and the context menu item all emit `Effect::ToggleMinimize`.
- The process keeps running in every mode.

## Verified on

macOS 27.0.1, Apple M4 Pro, 2026-10-31.

- `cargo run -p calm-app -- --smoke`: two terminals at zoom 1.5, one clipped by the window's
  top edge, both draw frames and stay alive.
- Driven with synthetic CGEvents and checked by screenshot: a drawn terminal shows a zsh
  prompt below its chrome strip; `echo ok` typed and run; the close button on a live shell
  shows the confirmation, Escape cancels it; cmd-= twice zooms to 121% and the grid scales
  without reflowing.
- Not yet checked (human pass pending): `htop`, mouse selection at zoom 1.0 and 1.7,
  cmd-v paste, Polish letters through the input method, panel resize reflow, overlapping
  panels.
