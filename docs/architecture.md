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

## Verified on

macOS 27.0.1, Apple M4 Pro, 2026-10-31.

- `cargo run -p calm-app -- --smoke`: two terminals at zoom 1.5, one clipped by the toolbar
  edge, both draw frames and stay alive.
- Driven with synthetic CGEvents and checked by screenshot: a drawn terminal shows a zsh
  prompt below its chrome strip; `echo ok` typed and run; the close button on a live shell
  shows the confirmation, Escape cancels it; cmd-= twice zooms to 121% and the grid scales
  without reflowing.
- Not yet checked (human pass pending): `htop`, mouse selection at zoom 1.0 and 1.7,
  cmd-v paste, Polish letters through the input method, panel resize reflow, overlapping
  panels.
