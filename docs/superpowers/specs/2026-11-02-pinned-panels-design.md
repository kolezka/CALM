# Pinned panels

Approved in chat on 2026-11-02.

## Intent

Some panels (a log tail, a notes page, a status plugin) should stay in view while the user
moves around the canvas. Pinning a panel takes it out of the canvas's pan and zoom: it
stays where it is on screen, at the size it has on screen.

## Behavior

- Every panel kind can be pinned: terminal, TUI app, browser, plugin, placeholder.
- A pinned panel keeps its screen position and screen size through pan, zoom, fit-all,
  center-origin and arrow pan. Zoom never turns it into a preview.
- Pinned panels are always above unpinned ones, in GPUI paint order and native z-order.
  Bring to Front and Send to Back work inside each tier.
- A pinned panel can still be moved, resized, minimized, focused, closed. Move and resize
  are in screen terms. It never snaps or joins to an unpinned panel; pinned panels snap
  and join among themselves.
- Unpin keeps the panel's screen origin. Its on-screen size then follows the canvas zoom
  again (the logical size never changes).
- Toggled by the rebindable action `panel.pin` ("Pin / unpin focused panel", default
  `cmd-shift-p`, the user's choice) on the focused panel, and by a Pin/Unpin item in the panel context menu.
- Saved in `session.json` and presets. Restore moves a pinned panel so its title bar
  (minimized chip, or a top band for a borderless panel) is on screen if the window is
  smaller than when it was saved, and so does a window resize.
- Fit-all (Cmd+Shift+0) and scene bounds ignore pinned panels. Zoom to Fit is hidden
  for a pinned panel.
- Plugins see `pinned: true|false` in the panel info.

## Model (lead's call)

`Panel.pin: Option<f64>`, the canvas zoom at pin time. While pinned, the panel's camera
is `Camera { offset: (0, 0), zoom: pin }` and `rect` is in that camera's space, so
`rect.origin * pin` is its screen origin. Everything per panel (hit test, chrome, resize,
placement, paint, occlusion) uses `Scene::camera_of(panel)` instead of `scene.camera`.

## Non-goals

Pinning to a screen edge or corner (anchoring), per-monitor positions, pinning groups.

## Calls made without asking

Default key `cmd-shift-p` was chosen by the user over the lead's `cmd-alt-p` (Ghostty uses
`cmd-shift-p` for its command palette; CALM's binding wins over it). Layout version goes
to 3 so an older build refuses a file it would misplace, rather than misplacing it.
