# Pinned Panels Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Any panel can be pinned so it keeps its screen position and size while the canvas pans and zooms.

**Architecture:** `Panel.pin: Option<f64>` holds the zoom at pin time. `Scene::camera_of(panel)` returns `scene.camera` for an unpinned panel and `Camera { offset: (0,0), zoom: pin }` for a pinned one. Every per-panel computation that used `scene.camera` switches to `camera_of`; hit testing becomes screen-based because one screen point maps to a different world point per panel. Pinned panels sit after all unpinned ones in `scene.panels`, so paint order, native z-order and occlusion follow without a separate sort.

**Tech Stack:** Rust, GPUI, AppKit via `calm-native`, serde.

**Spec:** `docs/superpowers/specs/2026-11-02-pinned-panels-design.md`

## Global Constraints

- Worktree: `.worktrees/pinned-panels`, branch `feat/pinned-panels`. Never edit the main checkout.
- Action id `panel.pin`, label `Pin / unpin focused panel`, group `Panels`, scope `Canvas`, default `cmd-shift-p` (user's choice), `in_menu: false`. It must also reach CALM when a terminal has focus, like `cmd-shift-backspace` does.
- Menu ids: `panel.pin` with label `Pin` when unpinned, `Unpin` when pinned, placed after `panel.send_to_back` in the same group.
- `LAYOUT_VERSION` becomes 3; versions 1 to 3 load. Field `pin` is omitted from JSON when `None`.
- The zoom view's bounds always equal the panel's logical size: pin and unpin never change `rect.size`.
- Native z-order follows scene order (`.ai/decisions.md:49`): the pinned tier is enforced inside `scene.panels`.
- Pinned and unpinned panels never snap to or join each other.
- Iterate with `cargo test -p <crate>`. Before the PR: `cargo clean -p calm-core -p calm-app`, `cargo check --workspace --tests --examples`, `gmake ci`, `cargo run -p calm-app -- --smoke; echo rc=$?`.
- Comments short and plain, no long dashes (global style rule).

## Review Focus

1. Pin while zoomed far out (zoom 0.1) then unpin at zoom 4: the panel must stay visible at the same screen origin and not become a preview while pinned. Test in Task 1 (`unpin_after_zoom_change_keeps_screen_origin`) and Task 5 smoke.
2. Window shrinks after a pinned panel was placed near the right edge: the panel must be pulled back so at least its title bar is on screen. Test in Task 1 (`clamp_pinned_pulls_offscreen_panel_back`).
3. A pinned panel overlapping a live unpinned terminal: the pinned panel's GPUI chrome must not be drawn under the terminal's native view (occlusion with each panel's camera). Test in Task 2 (`native_paint_region_uses_pin_camera`).
4. Dragging an unpinned panel next to a pinned one must not snap or join to it. Test in Task 2 (`pinned_panels_are_not_snap_neighbors_of_unpinned`).
5. A session saved with a pinned panel and loaded by the same build round-trips pin and tier order; a v2 file still loads. Test in Task 3.

---

### Task 1: Core model (scene)

**Files:**
- Modify: `crates/calm-core/src/scene.rs` (Panel struct ~357, `add_with_overrides` ~477, `bring_to_front` 543, `send_to_back` 553, `duplicate` 566, `hit_test` 585, `bounds` 602, tests)
- Modify: Panel literals in tests: `crates/calm-core/src/menu.rs:481`, `crates/calm-core/src/plugin_menu.rs:318`

**Interfaces:**
- Produces:
  - `Panel.pin: Option<f64>` (new field, `None` on add).
  - `Panel::is_pinned(&self) -> bool`.
  - `Scene::camera_of(&self, panel: &Panel) -> Camera`.
  - `Scene::set_pinned(&mut self, id: PanelId, pinned: bool) -> bool` (true if it changed). Pin: `pin = Some(camera.zoom)`, `rect.origin = camera.world_to_screen(origin) / zoom` component-wise. Unpin: `rect.origin = camera.screen_to_world(pin_camera.world_to_screen(origin))`, `pin = None`. Moves the panel to the top of its new tier. Bumps the revision.
  - `Scene::hit_test_screen(&self, screen: Point) -> Option<PanelId>`: topmost panel whose hit rect (chip when minimized) contains `camera_of(panel).screen_to_world(screen)`. Keep `hit_test(world)` only if a caller still needs it; otherwise remove it.
  - `Scene::bounds()` skips pinned panels.
  - `Scene::clamp_pinned(&mut self, viewport: Size) -> bool`: moves each pinned panel so its screen rect keeps at least its title strip (or chip) fully inside `viewport`; returns true if any moved.
  - Tier invariant: every pinned panel comes after every unpinned one. `add` inserts new (unpinned) panels before the first pinned one. `bring_to_front`/`send_to_back` stay inside the panel's tier. `duplicate` copies `pin` and inserts at the top of the copy's tier.

- [ ] **Step 1: Write failing tests in `scene.rs`**: `pin_keeps_screen_rect_through_pan_and_zoom` (pin at zoom 2 offset (100,50), then pan and zoom the camera, assert `camera_of(p).world_rect_to_screen(p.rect)` unchanged), `unpin_after_zoom_change_keeps_screen_origin`, `pin_and_unpin_never_change_size`, `pinned_panels_stay_above_unpinned` (add, pin, add again, bring_to_front unpinned, send_to_back pinned: assert tier order each time), `hit_test_screen_prefers_pinned_over_unpinned_under_it`, `bounds_ignore_pinned`, `clamp_pinned_pulls_offscreen_panel_back`, `duplicate_of_pinned_is_pinned_on_top`.
- [ ] **Step 2: Run** `cargo test -p calm-core scene::` and see them fail to compile or fail.
- [ ] **Step 3: Implement** the interface above and fix the Panel literals.
- [ ] **Step 4: Run** `cargo test -p calm-core`: all pass.
- [ ] **Step 5: Commit** `feat(core): pinned panel model and tier order`.

### Task 2: Per-panel camera in interaction, resize, chrome, joins, snapping, occlusion

**Files:**
- Modify: `crates/calm-core/src/interaction.rs` (277-391 mouse_down/resize/content move/mouse_move, 625 hover_resize, 688 neighbor_rects, 706 context_menu)
- Modify: `crates/calm-core/src/resize.rs:120-133`, `gesture.rs:56-59`, `chrome.rs:175 hover_target`, `join.rs:95` callers, `occlusion.rs:123-136`

**Interfaces:**
- Consumes: Task 1 `camera_of`, `hit_test_screen`, `is_pinned`.
- Produces: `chrome::hover_target(scene, screen: Point)` takes a screen point (was world). `gesture::content_gesture_target` takes a screen point. `JoinLayout::compute` is fed only panels of one tier (pass the tier of the panel being resized or dragged; the app-side callers at `canvas_view.rs:2180` and `3571` compute both tiers and merge).

- [ ] **Step 1: Failing tests**: in `interaction.rs`, `drag_pinned_panel_moves_in_screen_space` (pinned at zoom 2, drag 40 px, screen origin moves 40 px whatever `scene.camera.zoom` is), `resize_pinned_panel_grab_zone_uses_pin_zoom`, `pinned_panels_are_not_snap_neighbors_of_unpinned`; in `occlusion.rs`, `native_paint_region_uses_pin_camera`; in `chrome.rs`, `hover_target_on_pinned_title_bar`.
- [ ] **Step 2: Run** `cargo test -p calm-core` and see the new tests fail.
- [ ] **Step 3: Implement.** Convert the pointer with the target panel's camera at press and on every move of a drag or resize. Filter snap and join neighbors to the dragged panel's tier.
- [ ] **Step 4: Run** `cargo test -p calm-core`: all pass, no existing test changed its expected values.
- [ ] **Step 5: Commit** `feat(core): per-panel camera for input, joins and occlusion`.

### Task 3: Persistence

**Files:**
- Modify: `crates/calm-core/src/layout.rs` (18 `LAYOUT_VERSION`, 38 `LayoutPanel`, 103 version check, 126 `to_layout`, 151 `from_layout`, 497 `validate_layout`)

**Interfaces:**
- Consumes: `Panel.pin`, `Scene::set_pinned` not needed (copy the field directly, then restore tier order with one stable partition).
- Produces: `LayoutPanel.pin: Option<f64>` with `#[serde(default, skip_serializing_if = "Option::is_none")]`; `LAYOUT_VERSION = 3`.

- [ ] **Step 1: Failing tests**: `pinned_panel_round_trips_with_tier_order`, `v2_layout_still_loads_unpinned`, `layout_rejects_non_finite_or_out_of_range_pin` (pin must be finite and inside the camera's MIN_ZOOM..=MAX_ZOOM), `unpinned_layout_has_no_pin_key` (keep the key-set assertion at 666 passing).
- [ ] **Step 2: Run** `cargo test -p calm-core layout::`: new tests fail.
- [ ] **Step 3: Implement.**
- [ ] **Step 4: Run** `cargo test -p calm-core`: pass. Check `presets/welcome.json` still loads (`rg -n '"version"' presets/`).
- [ ] **Step 5: Commit** `feat(core): save pinned panels in layouts (v3)`.

### Task 4: Action, menu, plugin info

**Files:**
- Modify: `crates/calm-core/src/keymap.rs` (ACTIONS, after `panel.settings` ~124), `crates/calm-core/src/menu.rs` (consts 20-35, `MenuCommand` 59, `panel_menu` 182, `menu_action` 216, tests 501-527), `crates/calm-core/src/plugin_menu.rs` (`panel_info` 236, `when` filter 74), `packages/calm-plugin-sdk/calm.d.ts`, `docs/plugins.md`

**Interfaces:**
- Produces: `menu::PANEL_PIN = "panel.pin"`, `MenuCommand::TogglePin`. `panel_menu` adds `Pin`/`Unpin` after Send to Back and drops Zoom to Fit for a pinned panel. Panel info JSON gains `"pinned": bool`; `contributes.menus` `when.pinned: bool` optional filter, same shape as `when.minimized`.

- [ ] **Step 1: Failing tests**: update `panel_menu_for_terminal_lists_all_actions_with_delete_last` to the new list; add `panel_menu_pinned_says_unpin_and_drops_zoom_to_fit`, `menu_action_maps_panel_pin`, `panel_pin_action_has_free_default_key` (assert the default does not collide, reuse the keymap's existing conflict check), `command_payload_reports_pinned`, `when_pinned_filters_items`.
- [ ] **Step 2: Run** `cargo test -p calm-core`: fail.
- [ ] **Step 3: Implement.** Add `pinned?: boolean`-style fields to `calm.d.ts` matching the existing `minimized` ones.
- [ ] **Step 4: Run** `cargo test -p calm-core -p calm-plugins` and the SDK type check from `gmake ci` (`rg -n tsc Makefile` for the exact target): pass.
- [ ] **Step 5: Commit** `feat: panel.pin action, Pin/Unpin menu item, pinned in plugin info`.

### Task 5: App wiring and smoke

**Files:**
- Modify: `crates/calm-app/src/canvas_view.rs` (actions! list 80-100; `.on_action` near 3671; `apply_menu_choice` 957 near 1011-1025; `restore_layout` 470; `frame_view` 2423; `terminal_mode` 2672-2683; `terminal_placement` 2725; paint 2957, 2986, 3045, 3088-3343; join callers 2180, 3571; hover 2250; window resize handler), `crates/calm-app/src/keymap.rs:22-60`, `crates/calm-app/src/plugin_host.rs:425` (getBounds zoom), `crates/calm-app/src/smoke_runner.rs:28-82`, `crates/calm-app/src/main.rs` (smoke step body near 409-430)

**Interfaces:**
- Consumes: everything above. New GPUI action `TogglePinPanel` mapped from `"panel.pin"`, acting on `view.focused`.

- [ ] **Step 1: Add the smoke step `pin`** after `resize`: draw a terminal, pin it, record its native container rect, pan by (200, 120) and zoom to 0.5 then 3, assert the container rect is unchanged within 0.5 pt and the pinned id is last in `native_zorder`; unpin and assert the screen origin is unchanged; print `pin ok rect=… zorder=…`.
- [ ] **Step 2: Run** `cargo run -p calm-app -- --smoke; echo rc=$?`: the `pin` step fails (rc != 0).
- [ ] **Step 3: Implement**: use `scene.camera_of(panel)` in mode, placement and paint; leave draft drawing, grid and status bar on `scene.camera`. Call `scene.clamp_pinned(viewport)` after restore and on window resize. Hide nothing else.
- [ ] **Step 4: Run** `cargo test -p calm-app` and the smoke: rc=0, `pin ok` printed, no other step regressed. Run a control smoke on `origin/main` if any known flake shows.
- [ ] **Step 5: Commit** `feat(app): wire pinned panels, smoke pin step`.

### Task 6: Docs and knowledge

**Files:**
- Modify: `README.md` (menu items ~84, key tables ~161, ~179), `CLAUDE.md` ("In the app"), `.ai/status.md` (merged list, unverified list: pin feel by hand), `.ai/decisions.md` (pin model, tier invariant, layout v3, default key `cmd-shift-p` chosen by the user, shadows Ghostty's palette key), `docs/architecture.md` if it describes hit testing.

- [ ] **Step 1: Write the docs** in the style of the surrounding text. No long dashes.
- [ ] **Step 2: Run** `gmake ci`: pass. Run `repo-intel build; echo rc=$?`: rc=0.
- [ ] **Step 3: Commit** `docs: pinned panels`.
