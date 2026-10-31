# CALM Prototype Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A macOS app with an infinite pan/zoom canvas hosting live Ghostty terminals, a WebKit browser panel and HTML/JS plugins, with saved layouts, preview mode and a measured 5/10/20-terminal benchmark.

**Architecture:** Rust + GPUI (`gpui-pre` crates.io snapshot) draws the canvas, panel chrome and previews. Terminals and web content are native AppKit child views (Ghostty via `gpui-libghostty`, WKWebView via `objc2`) parented to a per-panel container `NSView` that CALM positions, clips and scales each frame; GPUI paints beneath them, so nothing GPUI draws may overlap a live native rect. All geometry, interaction and layout logic lives in a GPUI-free `calm-core` crate and is unit tested; the GPUI and AppKit layers are thin and verified by a `--smoke` run plus manual checks.

**Tech Stack:** Rust 1.95+ (edition 2024), `gpui-pre =0.3.4` (bound as `gpui`), `gpui-libghostty =0.3.1` (Zig 0.16 build), `objc2 0.6` + `objc2-app-kit 0.3` + `objc2-web-kit 0.3` + `objc2-foundation 0.3` + `block2 0.6`, `raw-window-handle 0.6`, `serde`/`serde_json`, `gh` CLI for GitHub, `bun` for the TSX plugin template.

**Spec:** `docs/superpowers/specs/2026-10-31-calm-design.md`

## Global Constraints

- macOS first; arm64 Macs; minimum macOS 14 (builder's call, matches the Ghostty embedding examples checked). No Linux/Windows claims.
- Camera zoom never changes a terminal's logical grid; only panel resize does.
- A panel's process keeps running through pan, zoom, preview, minimize and viewport culling. Only an explicit close, confirmed when the process is alive, ends it.
- No credentials in layouts, plugin manifests, docs, logs or project state. GitHub access goes through the host calling `gh`; plugin JS never sees a token.
- Plugins are trusted, explicitly installed code. Docs must not call them sandboxed.
- No performance number is reported without a recorded measurement (hardware, workload, method, budget).
- Comments and docs: plain, short, no long dashes (global style rule).
- Code changes happen in a git worktree, never in the main checkout (Task 0 commits docs only on `main`).

## Review Focus

1. A panel dragged or zoomed so it partly leaves the viewport or slides under the toolbar: the native view must be clipped to the canvas viewport, not drawn over the toolbar. Test pinned in Task 7 (`clip_to_viewport`).
2. Zooming with the pointer over a terminal: the terminal must receive mouse positions in its own unzoomed coordinates. Test pinned in Task 8 (`unzoom_mouse_position`).
3. Loading a layout file with an unknown version or malformed JSON must fail loudly with the file path, not open an empty canvas. Test pinned in Task 5.
4. Quitting with live terminals: processes end with the app (spec allows it) but a user clicking a panel's close button with a live process must be asked first. Test pinned in Task 8 (`close_requires_confirmation_when_alive`).
5. A plugin directory with a broken manifest must be reported and skipped without hiding the other plugins. Test pinned in Task 11.

---

### Task 0: Repository bootstrap (docs only, on `main`)

**Files:**
- Create: `.gitignore`, `README.md`, `CLAUDE.md`, `docs/dev-setup.md`
- Existing: `docs/superpowers/specs/2026-10-31-calm-design.md`, `docs/superpowers/plans/2026-10-31-calm-prototype.md`

- [ ] **Step 1: Init repo**

Run: `cd /Users/me/Development/CALM && git init -b main && git status --short`
Expected: two untracked `docs/` files.

- [ ] **Step 2: Write `.gitignore`**

Entries: `target/`, `graphify-out/`, `.codegraph/`, `node_modules/`, `plugins/*/dist/`, `templates/*/dist/`, `.DS_Store`, `*.log`, `bench-results/`.

- [ ] **Step 3: Write `CLAUDE.md`** (project contract, about 40 lines)

Must state: workspace layout (crates below), `cargo test -p calm-core` while iterating and `cargo test --workspace` once before a PR, `cargo run -p calm-app -- --smoke` as the GUI gate (a green build is not a running window), `ZIG` must point at Zig 0.16, native views cover GPUI paint so no GPUI overlay may sit on a live native rect, `repo-intel build` after structural changes with its exit code checked, spec and plan paths, and the no-credentials rule.

- [ ] **Step 4: Write `README.md` and `docs/dev-setup.md`**

README: one paragraph on what CALM is, link to spec, plan, dev-setup. dev-setup: prerequisites (`rustup update stable`, `brew install zig@0.16`, Xcode CLT, `gh auth login`, `bun`), `.cargo/config.toml` `ZIG` override, `cargo run -p calm-app`, `--smoke`, `--bench`.

- [ ] **Step 5: Commit**

```bash
git add .gitignore README.md CLAUDE.md docs
git commit -m "docs: add CALM design, prototype plan and project contract"
```

---

### Task 1: Toolchain and workspace skeleton

**Files:**
- Create: `rust-toolchain.toml`, `Cargo.toml`, `.cargo/config.toml`, `crates/calm-core/Cargo.toml`, `crates/calm-core/src/lib.rs`, `crates/calm-app/Cargo.toml`, `crates/calm-app/src/main.rs`

**Interfaces:**
- Produces: workspace members `calm-core`, `calm-app`; `calm-app` binary flags `--smoke` and `--fullscreen`.

- [ ] **Step 1: Create worktree** via superpowers:using-git-worktrees, branch `feat/canvas-prototype`. All later tasks run there.

- [ ] **Step 2: Toolchain**

Run: `rustup update stable && rustc --version && brew install zig@0.16 && /opt/homebrew/opt/zig@0.16/bin/zig version`
Expected: rustc ≥ 1.95.0; zig prints `0.16.x`. Write `rust-toolchain.toml` with `channel = "stable"` and `.cargo/config.toml`:

```toml
[env]
ZIG = { value = "/opt/homebrew/opt/zig@0.16/bin/zig", force = false }
```

- [ ] **Step 3: Workspace `Cargo.toml`**

`members = ["crates/*"]`, `resolver = "2"`, `[workspace.package] edition = "2024", rust-version = "1.95"`, `[workspace.dependencies]`: `gpui = { package = "gpui-pre", version = "=0.3.4" }`, `gpui-libghostty = "=0.3.1"`, `serde = { version = "1", features = ["derive"] }`, `serde_json = "1"`, `raw-window-handle = "0.6"`, `objc2 = "0.6"`, `objc2-foundation = "0.3"`, `objc2-app-kit = "0.3"`, `objc2-web-kit = "0.3"`, `block2 = "0.6"`.

- [ ] **Step 4: `calm-core` lib with one placeholder test; `calm-app` GPUI hello window**

`main.rs`: parse args (`std::env::args`, no clap); `Application::new().run` opens a window with `WindowBounds::Fullscreen` when `--fullscreen`, else `Windowed` 1280x800 centered; root view renders a dark `div()` with the text "CALM". With `--smoke`: after the first `window.on_next_frame`, print `smoke: viewport=<w>x<h> scale=<s>` to stdout and `cx.quit()`.

- [ ] **Step 5: Verify**

Run: `cargo build --workspace 2>&1 | tail -3; echo rc=$?` then `cargo run -p calm-app -- --smoke; echo rc=$?`
Expected: build rc=0; smoke prints a viewport line with nonzero size and exits rc=0 within 10 s. If `gpui-pre` fails to build, record the error verbatim in `docs/dev-setup.md` and stop; do not switch GPUI source without asking.

- [ ] **Step 6: Commit** `build: add cargo workspace with calm-core and calm-app skeleton`

---

### Task 2: Camera math (`calm-core`)

**Files:**
- Create: `crates/calm-core/src/geometry.rs`, `crates/calm-core/src/camera.rs`
- Test: inline `#[cfg(test)]` modules

**Interfaces:**
- Produces:
  - `geometry.rs`: `Point { x: f64, y: f64 }`, `Size { width: f64, height: f64 }`, `Rect { origin: Point, size: Size }` with `contains(Point) -> bool`, `intersect(&Rect) -> Option<Rect>`, `right()`, `bottom()`. All `Copy + PartialEq + Debug + Serialize + Deserialize`.
  - `camera.rs`: `Camera { offset: Point /* world point at viewport origin */, zoom: f64 }`, `MIN_ZOOM: f64 = 0.1`, `MAX_ZOOM: f64 = 4.0`, `Camera::world_to_screen(Point) -> Point`, `screen_to_world(Point) -> Point`, `world_rect_to_screen(Rect) -> Rect`, `pan_by_screen(dx, dy)`, `zoom_by(factor, anchor_screen: Point)` (clamps zoom; the world point under `anchor_screen` stays fixed), `Default` = offset (0,0), zoom 1.0.

- [ ] **Step 1: Write failing tests**

`world_to_screen_roundtrip` (zoom 2.0, offset (100, 50): world (120, 70) -> screen (40, 40) -> back), `zoom_by_keeps_anchor_fixed` (anchor (300, 200), factor 1.5: `screen_to_world(anchor)` unchanged before and after), `zoom_clamps_to_bounds` (factor 100 -> `MAX_ZOOM`; factor 0.001 -> `MIN_ZOOM`), `pan_moves_offset_by_unzoomed_delta` (zoom 2, pan (10, 0) -> offset.x decreases by 5), `rect_intersect_disjoint_is_none`.

- [ ] **Step 2: Run** `cargo test -p calm-core` Expected: FAIL (unresolved items).
- [ ] **Step 3: Implement** the signatures above.
- [ ] **Step 4: Run** `cargo test -p calm-core` Expected: PASS.
- [ ] **Step 5: Commit** `feat(core): add geometry and camera math`

---

### Task 3: Scene and panels (`calm-core`)

**Files:**
- Create: `crates/calm-core/src/scene.rs`

**Interfaces:**
- Produces: `PanelId(u64)`; `PanelKind::{ Terminal { command: Option<String>, cwd: Option<PathBuf> }, Browser { url: String }, Plugin { plugin_id: String, settings: serde_json::Value } }`; `Panel { id, kind, rect: Rect /* world */, title: String, minimized: bool }`; `Scene { panels: Vec<Panel> /* back to front */, camera: Camera }` with `add(kind, rect) -> PanelId`, `remove(id) -> Option<Panel>`, `get(id)`, `get_mut(id)`, `bring_to_front(id)`, `hit_test(world: Point) -> Option<PanelId>` (topmost; a minimized panel hits only its chip rect), `chip_rect(&Panel) -> Rect` (origin = panel origin, size `MINIMIZED_CHIP = 200x28`), `MIN_PANEL_SIZE = Size { 120, 80 }`, `CHROME_HEIGHT = 28.0` (world units), `content_rect(&Panel) -> Rect` (rect minus chrome strip at top).

- [ ] **Step 1: Write failing tests**

`add_assigns_increasing_ids_and_topmost_last`, `hit_test_prefers_topmost_overlap`, `bring_to_front_reorders`, `minimized_panel_hits_only_chip`, `remove_returns_panel_and_shrinks`, `add_enforces_min_size` (rect 10x10 becomes 120x80).

- [ ] **Step 2: Run** Expected: FAIL. **Step 3: Implement.** **Step 4: Run** Expected: PASS.
- [ ] **Step 5: Commit** `feat(core): add scene with panels, z-order and hit testing`

---

### Task 4: Interaction state machine (`calm-core`)

**Files:**
- Create: `crates/calm-core/src/interaction.rs`

**Interfaces:**
- Consumes: `Scene`, `Camera`, `Rect`.
- Produces: `Tool::{ Select, DrawTerminal, DrawBrowser, DrawPlugin(String) }`; `Modifiers { shift, control, alt, platform: bool }`; `Button::{Left, Middle}`; `ResizeEdge::{Right, Bottom, BottomRight}` (12 world-unit grab band inside the rect); `Interaction` (enum state: `Idle`, `Drawing { start_world }`, `Dragging { id, grab_offset_world }`, `Resizing { id, edge }`, `Panning { last_screen }`); `Effect::{ CreatePanel { kind, rect }, FocusPanel(PanelId), ClosePanelRequested(PanelId), ToggleMinimize(PanelId), Redraw }`; `InputHandler { tool, state, space_held: bool }` with `mouse_down(screen, button, mods, &mut Scene) -> Vec<Effect>`, `mouse_move(screen, &mut Scene) -> Vec<Effect>`, `mouse_up(screen, &mut Scene) -> Vec<Effect>`, `scroll(screen, dx, dy, mods, &mut Scene)` (platform modifier held: `camera.zoom_by((-dy / 300.0).exp2(), screen)`; else over empty canvas: pan), `key_zoom(step: ZoomStep::{In, Out, Reset}, anchor_screen, &mut Scene)` (In = factor 1.1, Out = 1/1.1, Reset = zoom 1.0 at same anchor), `chrome_hit(&Panel, world) -> Option<ChromeButton::{Close, Minimize}>` (two 20x20 squares at the right end of the chrome strip).

Rules: Left down on empty canvas with a Draw tool starts `Drawing`; up creates the panel (min size enforced), tool returns to `Select`, emits `CreatePanel` and `FocusPanel`. Left down on a panel's chrome starts `Dragging` and `bring_to_front`; on a resize edge starts `Resizing`; on the content area emits `FocusPanel` only (events go to the native view). Middle button or Space+left anywhere starts `Panning`. Mouse up ends any state.

- [ ] **Step 1: Write failing tests**

`draw_tool_creates_terminal_panel_on_release`, `drag_from_chrome_moves_panel_and_raises_it`, `resize_from_bottom_right_changes_size_not_origin`, `panning_with_space_moves_camera`, `scroll_with_platform_modifier_zooms_at_pointer`, `chrome_close_button_emits_close_requested`, `content_click_focuses_without_dragging`.

- [ ] **Step 2: Run** Expected: FAIL. **Step 3: Implement.** **Step 4: Run** Expected: PASS.
- [ ] **Step 5: Commit** `feat(core): add canvas interaction state machine`

---

### Task 5: Layout files (`calm-core`)

**Files:**
- Create: `crates/calm-core/src/layout.rs`

**Interfaces:**
- Produces: `LayoutFile { version: u32 /* = 1 */, camera: Camera, panels: Vec<LayoutPanel> }`, `LayoutPanel { kind: PanelKind, rect: Rect, title: String, minimized: bool }`; `Scene::to_layout(&self) -> LayoutFile`; `Scene::from_layout(LayoutFile) -> Scene` (fresh ids, same order); `layouts_dir() -> PathBuf` (`$CALM_HOME/layouts` else `~/.config/calm/layouts`); `save_layout(name: &str, &LayoutFile) -> Result<PathBuf, LayoutError>` (creates dir, writes pretty JSON atomically via temp file + rename); `load_layout(name) -> Result<LayoutFile, LayoutError>`; `LayoutError::{ NotFound(PathBuf), UnsupportedVersion { path, found }, Malformed { path, message }, Io { path, source } }` with `Display` naming the path.

- [ ] **Step 1: Write failing tests** (use `tempfile::tempdir()` and `CALM_HOME`)

`roundtrip_preserves_panels_camera_and_order`, `terminal_panel_has_no_session_state_fields` (serialized JSON for a terminal panel contains only `kind`, `rect`, `title`, `minimized`), `load_rejects_unknown_version_with_path`, `load_reports_malformed_json_with_path`, `load_missing_is_not_found`, `plugin_settings_roundtrip_verbatim` (a `Plugin` panel's `settings` value survives save and load unchanged; `docs/plugins.md` tells authors settings are stored in plain text, so no secrets belong there).

- [ ] **Step 2: Run** Expected: FAIL. **Step 3: Implement.** **Step 4: Run** Expected: PASS.
- [ ] **Step 5: Commit** `feat(core): add layout file save and load`

---

### Task 6: GPUI canvas with placeholder panels (`calm-app`)

**Files:**
- Create: `crates/calm-app/src/canvas_view.rs`, `crates/calm-app/src/toolbar.rs`, `crates/calm-app/src/theme.rs`
- Modify: `crates/calm-app/src/main.rs`

**Interfaces:**
- Consumes: `Scene`, `InputHandler`, `Effect`, `Camera`.
- Produces: `CanvasView` (GPUI `Render` entity) holding `scene: Scene`, `input: InputHandler`, `viewport: Rect` (screen px, below toolbar), `fn apply_effects(&mut self, Vec<Effect>, &mut Window, &mut Context<Self>)`; `TOOLBAR_HEIGHT: f32 = 36.0`; `theme.rs` colors (background 0x151515, chrome 0x2a2a2a, chrome focused 0x3c6e9f, text 0xe6e6e6).

- [ ] **Step 1: Render the canvas**

`CanvasView::render`: a full-size `div()` with the toolbar strip on top and a `canvas()` element below. The canvas prepaint records `viewport` bounds; paint draws each panel in z-order at `camera.world_rect_to_screen(rect)`: a filled chrome strip (`CHROME_HEIGHT * zoom` tall) with the title text scaled by zoom and the close/minimize squares, and a flat content rect (placeholder until Task 8). Minimized panels draw the chip only. Draw the in-progress `Drawing` rectangle as an outline.

- [ ] **Step 2: Wire input**

`on_mouse_down/up/move` for Left and Middle, `on_scroll_wheel`, key bindings: `cmd-=` / `cmd--` / `cmd-0` → `key_zoom`, `space` down/up → `input.space_held`, `t` → `Tool::DrawTerminal`, `b` → `Tool::DrawBrowser`, `escape` → `Tool::Select`, `cmd-ctrl-f` → `window.toggle_fullscreen()`. Convert GPUI `Point<Pixels>` to `calm_core::Point` by subtracting the canvas origin. Call `window.refresh()` after effects. Single window in v1: closing it quits the app (`cx.on_window_closed`, quit when no windows remain).

- [ ] **Step 3: Toolbar**

Buttons: `Terminal`, `Browser`, one per discovered plugin (Task 11 fills the list; empty now), `Save`, zoom percentage label. Clicking sets the tool.

- [ ] **Step 4: Verify** `cargo run -p calm-app` then manually: draw two panels with `t`, drag one by its chrome, resize from the corner, pan with Space+drag and Middle-drag, zoom with `cmd+scroll` around the pointer and with `cmd-=`; note any mismatch. `cargo run -p calm-app -- --smoke; echo rc=$?` still rc=0.

- [ ] **Step 5: Commit** `feat(app): render canvas, toolbar and placeholder panels with pan and zoom`

---

### Task 6b: Developer menu with an FPS overlay (`calm-app`)

Added 2026-10-31 at the user's request. Runs after Task 8 merges, because it touches the same app files.

**Files:**
- Create: `crates/calm-core/src/frame_stats.rs`, `crates/calm-app/src/dev_menu.rs`
- Modify: `crates/calm-app/src/main.rs`, `crates/calm-app/src/canvas_view.rs`, `crates/calm-app/src/toolbar.rs`

**Interfaces:**
- Produces (`calm-core/frame_stats.rs`, pure, tested): `FrameStats::new(window: Duration)` (default window 1 s), `fn record(&mut self, now: Instant)`, `fn fps(&self) -> Option<f64>` (frames in the trailing window divided by the window span, `None` until two frames), `fn p95_ms(&self) -> Option<f64>`, `fn reset(&mut self)`. Task 14 reuses it for the benchmark.
- Produces (`calm-app`): a macOS menu bar menu `Developer` (via `cx.set_menus`) with the items `Show FPS` (checkbox state, `cmd-shift-f`), `Reset FPS Stats`; a `ToggleFps` / `ResetFps` GPUI action pair; `CanvasView.show_fps: bool` and `frame_stats: FrameStats`. When `show_fps` is on, the canvas requests a frame every tick (`window.request_animation_frame()` in the `on_next_frame` loop) and draws `FPS 60.0  p95 16.7ms` at the right end of the toolbar strip in `theme` text color; when off, no extra frames are requested and nothing is drawn. The overlay lives in the toolbar strip only, never over the canvas viewport (native views cover it).

- [ ] **Step 1: Write failing tests** (`calm-core`): `fps_is_none_before_two_frames`, `fps_counts_frames_in_trailing_window` (61 frames 16 ms apart over 0.96 s gives fps within 0.5 of 62.5), `old_frames_fall_out_of_window`, `p95_of_mixed_intervals`, `reset_clears`.
- [ ] **Step 2: Run** `cargo test -p calm-core` Expected: FAIL. **Step 3: Implement.** **Step 4: Run** Expected: PASS.
- [ ] **Step 5: Implement the menu and overlay** in `calm-app`. Extend `--smoke`: after the two panels render, toggle `show_fps` on programmatically, wait for 10 more frames, print `smoke: fps=<value>` (value > 0) and keep exit 0 only if the value is present.
- [ ] **Step 6: Verify** `cargo run -p calm-app -- --smoke; echo rc=$?` rc=0 with the fps line; `cargo clippy --workspace --all-targets -- -D warnings; echo rc=$?`; `cargo fmt --all -- --check; echo rc=$?`. Manual: open the app, pick Developer > Show FPS, confirm the readout appears in the toolbar and updates while panning; pick it again to hide it.
- [ ] **Step 7: Commit** `feat(app): add Developer menu with FPS overlay`

---

### Task 6c: Right-click context menu replaces the element toolbar (`calm-core`, `calm-native`, `calm-app`)

Added 2026-10-31 at the user's request. Runs after Task 8 merges (same app files as 6b; implement 6b and 6c in one dispatch).

**Files:**
- Create: `crates/calm-core/src/menu.rs`, `crates/calm-native/src/menu.rs`
- Modify: `crates/calm-core/src/interaction.rs`, `crates/calm-app/src/canvas_view.rs`, `crates/calm-app/src/toolbar.rs`, `crates/calm-native/examples/native_smoke.rs`

**Interfaces:**
- Produces (`calm-core/menu.rs`, pure, tested): `MenuItem::{ Action { id: String, label: String }, Submenu { label: String, items: Vec<MenuItem> }, Separator }`; `fn canvas_menu(plugins: &[(String, String)]) -> Vec<MenuItem>` = `Submenu "Add"` with `Action { id: "add.terminal", label: "Terminal" }`, `Action { id: "add.browser", label: "Browser" }`, then a `Separator` and one `Action { id: "add.plugin.<plugin_id>", label: <name> }` per plugin (no separator when there are no plugins); `fn panel_menu(minimized: bool) -> Vec<MenuItem>` = `Action "panel.minimize"` labelled `Minimize` or `Restore`, `Separator`, `Action "panel.close"` labelled `Close`; `fn menu_action(id: &str, world: Point, plugins: &[(String, String)]) -> Option<MenuCommand>` mapping ids to `MenuCommand::{ AddPanel { kind: PanelKind, rect: Rect }, Minimize, Close }`, where the new rect's origin is the right-click world point and the size is `DEFAULT_TERMINAL_SIZE = 640x400`, `DEFAULT_BROWSER_SIZE = 800x600`, or the plugin's default size passed in by the caller (extend the signature with `plugin_default_sizes: &[(String, Size)]` if simpler).
- Produces (`calm-native/menu.rs`): `fn show_context_menu(items: &[MenuItem], screen_point: Point, host_view: &NativeView /* the GPUI window content view */) -> Option<String>` using `NSMenu` and `popUpMenuPositioningItem:atLocation:inView:`; nested `Submenu` items become `NSMenu` submenus; returns the chosen action id or `None` when dismissed. Native menus render above native views, which is why the menu is not GPUI-painted.
- Interaction (`calm-core/interaction.rs`): right mouse button (`Button::Right`) down on empty canvas emits `Effect::ShowCanvasMenu { screen, world }`; right down on a panel's chrome emits `Effect::ShowPanelMenu { id, screen }`; right down on a panel's content rect emits nothing (the content owns its right clicks). The app runs `show_context_menu`, maps the id with `menu_action`, and applies: `AddPanel` creates the panel at the click point with the default size and focuses it (no drag needed); `Minimize` toggles; `Close` goes through `request_close`.
- Toolbar: remove the element buttons (`Terminal`, `Browser`, plugin buttons); keep a slim status strip with the zoom percentage, the `Saved <path>` flash and the FPS readout from Task 6b. Keyboard draw tools (`t`, `b`, `escape`) stay as a secondary path.

- [ ] **Step 1: Write failing tests** (`calm-core`): `canvas_menu_lists_add_submenu_with_terminal_browser_and_plugins`, `canvas_menu_without_plugins_has_no_trailing_separator`, `menu_action_add_terminal_places_default_rect_at_click`, `menu_action_add_plugin_uses_plugin_default_size`, `panel_menu_labels_restore_when_minimized`, `right_click_on_empty_canvas_emits_canvas_menu`, `right_click_on_chrome_emits_panel_menu`, `right_click_on_content_emits_nothing`.
- [ ] **Step 2: Run** `cargo test -p calm-core` Expected: FAIL. **Step 3: Implement.** **Step 4: Run** Expected: PASS.
- [ ] **Step 5: Implement `calm-native/menu.rs`** and extend `native_smoke` with a non-interactive check that builds the `NSMenu` tree from a sample spec and prints `native_smoke: menu items=<n> submenus=<m>` (do not pop it up in the smoke).
- [ ] **Step 6: Wire the app**, remove the element toolbar buttons, keep the status strip.
- [ ] **Step 7: Verify** `cargo run -p calm-app -- --smoke; echo rc=$?`, `cargo run -p calm-native --example native_smoke; echo rc=$?`, clippy and fmt once each. Manual: right-click empty canvas, Add > Terminal creates a shell at the click point; Add > Browser; Add > Hello World once plugins are wired; right-click a chrome strip shows Minimize/Close; right-click inside a terminal does not open the CALM menu.
- [ ] **Step 8: Commit** `feat(app): right-click context menu with Add submenu replaces the element toolbar`

---

### Task 7: Native child host (`calm-native`)

**Files:**
- Create: `crates/calm-native/Cargo.toml` (deps: `objc2`, `objc2-foundation`, `objc2-app-kit`, `raw-window-handle`, `calm-core`), `crates/calm-native/src/lib.rs`, `crates/calm-native/src/child.rs`
- Create: `crates/calm-core/src/viewport.rs`

**Interfaces:**
- Produces (`calm-core/viewport.rs`, pure, tested): `NativePlacement { container: Rect /* screen px, clipped */, inner_origin: Point /* unzoomed, offset of content inside container */, logical_size: Size, zoom: f64 }`; `fn place_native(content_world: Rect, camera: &Camera, viewport: Rect) -> Option<NativePlacement>` (None when the clipped rect is empty); `fn unzoom_mouse_position(screen: Point, placement: &NativePlacement) -> Point` (position in the content's unzoomed coordinates).
- Produces (`calm-native/child.rs`, macOS): `NativeChild::new(window: &impl HasWindowHandle) -> Result<NativeChild, NativeError>` (creates a plain, layer-backed `NSView` container, `wantsLayer = true`, added to the GPUI window's content view, hidden); `fn apply(&mut self, placement: &NativePlacement, viewport_height_px: f64, scale_factor: f64)` (sets container `frame` to the flipped clipped rect, sets container `bounds` to `(-inner_origin.x, -inner_origin.y, container.w / zoom, container.h / zoom)` so AppKit scales and offsets the children, and `setHidden:false`); `fn set_visible(bool)`; `fn ns_view(&self) -> *mut c_void` (the container, used as parent for Ghostty and WKWebView); `impl HasWindowHandle + HasDisplayHandle for NativeChild` returning `AppKitWindowHandle { ns_view: container }` and `AppKitDisplayHandle`; `Drop` removes the view from its superview.

- [ ] **Step 1: Write failing tests** (`calm-core`)

`place_native_clips_to_viewport_and_offsets_inner` (content world (0,0,400,300), zoom 1, camera offset (100,50), viewport (0,36,1280,764): screen rect is (-100,-50,400,300), so container = (0,36,300,214), inner_origin = (100,86), logical_size = 400x300), `place_native_scales_logical_size_by_zoom` (same content, zoom 2, camera offset (0,-18): container = (0,36,800,600), inner_origin = (0,0), logical_size = 400x300), `place_native_fully_offscreen_is_none` (camera offset (5000,5000)), `unzoom_mouse_position_divides_by_zoom_and_adds_inner_origin` (first placement: screen (50,86) -> (150,136); second placement: screen (200,236) -> (100,100)).

- [ ] **Step 2: Run** Expected: FAIL. **Step 3: Implement** `viewport.rs`. **Step 4: Run** Expected: PASS.
- [ ] **Step 5: Implement `calm-native`** with `objc2` (`unsafe` confined to this crate, each block with a SAFETY comment). Add a `--smoke` sub-check in `calm-app`: create one `NativeChild`, apply a placement, read back `frame` and print `smoke: native_child frame=<x,y,w,h>`.
- [ ] **Step 6: Verify** `cargo run -p calm-app -- --smoke; echo rc=$?` Expected: the native_child line shows the expected flipped frame; rc=0.
- [ ] **Step 7: Commit** `feat(native): add AppKit container view with clipped, zoomed placement`

---

### Task 8: Live Ghostty terminals

**Files:**
- Create: `crates/calm-app/src/terminal/adapter.rs` (copy of `gpui-libghostty` 0.3.1 `adapter.rs`, MIT, with attribution header), `crates/calm-app/src/terminal/mod.rs`
- Modify: `crates/calm-app/Cargo.toml` (add `gpui-libghostty`, `calm-native`), `canvas_view.rs`, `main.rs`

**Interfaces:**
- Consumes: `gpui_libghostty::__private::{spawn_surface, NativeSurface, ...}`, `TerminalOptions`, `NativeChild`, `NativePlacement`, `unzoom_mouse_position`.
- Produces: `TerminalPanel` (GPUI entity): `spawn(options: TerminalOptions, host: NativeChild, window, cx) -> Result<Entity<Self>, String>` (passes `&host` as the `HasWindowHandle` parent so Ghostty's view becomes a child of the container), `set_placement(Option<NativePlacement>, window)` (applies to the host, calls `surface.set_frame(0, 0, logical_size.w, logical_size.h, scale)`; `None` hides), `is_alive()`, `snapshot()`, `set_hidden_rendering(bool)`, `focus()`; mouse handlers map positions through `unzoom_mouse_position` before `surface.mouse_position`. Default options: command `$SHELL` or `/bin/zsh`, cwd `$HOME`, `configuration: TerminalConfiguration::UserDefault`, `quiet_login: true`, `clipboard_approval: Some(Arc::new(|_| true))`.
- Produces (`calm-core/interaction.rs` addition): `InputHandler.confirming: Option<PanelId>`; `fn request_close(&mut self, id: PanelId, alive: bool) -> Effect` (returns `Effect::ClosePanel(id)` when not alive or when `confirming == Some(id)`, else sets `confirming` and returns `Effect::ConfirmClose(id)`); `fn confirm_key(&mut self, key: ConfirmKey::{ Enter, Escape }) -> Option<Effect>` (Enter closes the confirming panel, Escape clears). The app maps `Effect::ClosePanelRequested(id)` from Task 4 to `request_close(id, terminal.is_alive())`. While confirming, the chrome strip shows `Close? enter / esc` in place of the title.

- [ ] **Step 1: Write failing tests** (`calm-core`): `close_requires_confirmation_when_alive`, `close_without_live_process_is_immediate`, `second_close_within_confirmation_closes`, `escape_cancels_confirmation`.
- [ ] **Step 2: Run** Expected: FAIL. **Step 3: Implement** the core part. **Step 4: Run** Expected: PASS.
- [ ] **Step 5: Implement `TerminalPanel`** and wire `Effect::CreatePanel { kind: Terminal }` to spawn one with a new `NativeChild`; every paint calls `set_placement(place_native(content_rect, camera, viewport))` for each terminal; `Drop` of the panel drops the entity (process ends).
- [ ] **Step 6: Extend `--smoke`**: spawn two terminals at different rects and zoom 1.5, wait up to 10 s for `frame_count() > 0` on both, print `smoke: terminals=2 frames=[a,b] alive=[true,true]`, exit 0; exit 1 with the reason otherwise.
- [ ] **Step 7: Verify** `cargo run -p calm-app -- --smoke; echo rc=$?` Expected: frames both > 0, rc=0. Then manually: type in each terminal, run `htop`, select text with the mouse at zoom 1.0 and 1.7 (pointer lands on the right cell), `cmd-v` paste, type Polish letters via the input method, resize a panel (grid reflows) and zoom (grid does not reflow), close with a live `sleep 100` (confirmation appears). Record results in `docs/architecture.md` under "Verified on".
- [ ] **Step 8: Commit** `feat(app): host live Ghostty terminals on the canvas`

---

### Task 9: Preview mode and minimize

**Files:**
- Create: `crates/calm-core/src/preview.rs`
- Modify: `canvas_view.rs`, `terminal/mod.rs`

**Interfaces:**
- Produces: `PanelMode::{ Live, Preview, Hidden }`; `PREVIEW_ZOOM_THRESHOLD: f64 = 0.5`; `PREVIEW_REFRESH_SECS: u64 = 2`; `fn decide_mode(zoom: f64, screen_rect: Rect, viewport: Rect, minimized: bool, modal_open: bool) -> PanelMode` (minimized or no intersection → `Hidden`; `modal_open` or `zoom < PREVIEW_ZOOM_THRESHOLD` → `Preview`; else `Live`).
- App behavior: entering `Preview` takes `snapshot()`, hides the native child, draws the image scaled into the content rect; while in `Preview` and intersecting the viewport, `set_hidden_rendering(true)` and refresh the snapshot every `PREVIEW_REFRESH_SECS`; entering `Live` sets `set_hidden_rendering(false)` and shows the child. `Hidden` shows the chip (if minimized) or nothing; the process keeps running in all modes.

- [ ] **Step 1: Write failing tests**: `below_threshold_is_preview`, `offscreen_is_hidden_even_when_live_zoom`, `minimized_is_hidden`, `modal_forces_preview`, `visible_at_threshold_is_live`.
- [ ] **Step 2: Run** Expected: FAIL. **Step 3: Implement.** **Step 4: Run** Expected: PASS.
- [ ] **Step 5: Wire** into `CanvasView` paint and the minimize chrome button (`Effect::ToggleMinimize`; clicking a chip restores).
- [ ] **Step 6: Verify** manually: run `yes` in a terminal, zoom out below 50 %: snapshot appears and updates about every 2 s; zoom back: live view returns with the process still running (output continued). Minimize, wait, restore: same. Record in `docs/architecture.md`.
- [ ] **Step 7: Commit** `feat(app): preview mode for distant panels and minimize chips`

---

### Task 10: Browser panel (WKWebView)

**Files:**
- Create: `crates/calm-native/src/webview.rs`
- Modify: `crates/calm-native/Cargo.toml` (add `objc2-web-kit`, `block2`), `canvas_view.rs`, `toolbar.rs`

**Interfaces:**
- Produces: `WebPanel::new(host: &NativeChild) -> Result<WebPanel, NativeError>` (a `WKWebView` filling the container, autoresizing), `load_url(&str)`, `load_file(entry: &Path, read_access_dir: &Path)`, `eval_js(&str)`, `on_message(Box<dyn FnMut(String)>)` (a `WKScriptMessageHandler` subclass via `define_class!` registered as `calm`), `set_visible(bool)`; `DEFAULT_BROWSER_URL: &str = "https://ghostty.org"`.
- App: `Effect::CreatePanel { kind: Browser }` creates a `NativeChild` + `WebPanel`, loads the url; placement/zoom through the same `apply`; `PanelMode::Preview` hides the view and draws a flat rect with the title (no web snapshot in v1).

- [ ] **Step 1: Implement `webview.rs`**.
- [ ] **Step 2: Extend `--smoke`**: create one browser panel with `load_url("about:blank")`, poll `isLoading == false` up to 10 s, print `smoke: browser loaded=true`.
- [ ] **Step 3: Verify** `cargo run -p calm-app -- --smoke; echo rc=$?` rc=0; manually: a browser panel next to a terminal, overlap them, drag by chrome, zoom (page scales with the container), click links and type in a form, pan the canvas while the pointer is over the page (scroll goes to the page; Space+drag pans). Record limits found in `docs/architecture.md`.
- [ ] **Step 4: Commit** `feat(native): add WKWebView browser panels`

---

### Task 11: Plugin discovery and JS bridge, Hello World

**Files:**
- Create: `crates/calm-plugins/Cargo.toml` (deps `serde`, `serde_json`, `thiserror`), `crates/calm-plugins/src/lib.rs`, `crates/calm-plugins/src/manifest.rs`, `crates/calm-plugins/src/bridge.rs`, `crates/calm-app/src/plugin_panel.rs`, `plugins/hello-world/calm-plugin.json`, `plugins/hello-world/index.html`, `docs/plugins.md`

**Interfaces:**
- Produces (`manifest.rs`): `PluginManifest { id: String /* [a-z0-9-]+ */, name, version, entry: PathBuf /* relative */, default_size: Size }`, `InstalledPlugin { manifest, dir: PathBuf }`, `fn plugin_dirs() -> Vec<PathBuf>` (`$CALM_PLUGIN_PATH` entries, then `$CALM_HOME/plugins` else `~/.config/calm/plugins`), `fn discover(dirs: &[PathBuf]) -> Discovery { plugins: Vec<InstalledPlugin>, errors: Vec<PluginError> }`, `PluginError::{ MissingManifest(PathBuf), Invalid { path, message }, DuplicateId { id, first, second } }`.
- Produces (`bridge.rs`): `BridgeRequest { id: u64, method: String, params: serde_json::Value }` (`serde` from the JSON the webview posts), `BridgeResponse::{ Ok(id, Value), Err(id, String) }` with `fn to_js(&self) -> String` producing `window.__calm.resolve(<id>, <ok>, <json>)`, and `const CALM_PRELUDE_JS: &str` (the injected `window.calm` client: `calm.settings.get()`, `calm.settings.set(obj)`, `calm.panel.setTitle(s)`, `calm.github.api(path)`, `calm.log(msg)`; each returns a Promise keyed by id, posts via `window.webkit.messageHandlers.calm.postMessage`).
- App (`plugin_panel.rs`): a `WebPanel` loaded with `load_file(dir/entry, dir)`, prelude injected as a `WKUserScript` at document start, `on_message` parses `BridgeRequest` and dispatches: `settings.*` reads/writes the panel's `PanelKind::Plugin.settings`, `panel.setTitle`, `log` → stderr, `github.api` → Task 12. Toolbar lists discovered plugins; discovery errors print to stderr and show as a red toolbar label `plugins: N errors`.

- [ ] **Step 1: Write failing tests** (`calm-plugins`): `discover_reads_valid_manifest`, `discover_skips_broken_manifest_and_reports_path`, `discover_reports_duplicate_ids`, `manifest_rejects_entry_outside_plugin_dir` (`../x.html`), `bridge_request_parses_from_json`, `bridge_response_renders_resolve_call`.
- [ ] **Step 2: Run** Expected: FAIL. **Step 3: Implement.** **Step 4: Run** Expected: PASS.
- [ ] **Step 5: Hello World plugin**: manifest `{ "id": "hello-world", "name": "Hello World", "version": "0.1.0", "entry": "index.html", "defaultSize": { "width": 320, "height": 200 } }`; `index.html` shows "Hello, world" and calls `calm.panel.setTitle("Hello")`.
- [ ] **Step 6: Write `docs/plugins.md`**: install location, manifest fields, the `window.calm` API, dev loop with `CALM_PLUGIN_PATH=./plugins`, the trusted-code statement.
- [ ] **Step 7: Verify** `CALM_PLUGIN_PATH=./plugins cargo run -p calm-app`: the toolbar shows "Hello World"; draw one; the panel title becomes "Hello" and the text renders. Add a broken plugin dir in a temp `CALM_PLUGIN_PATH` entry and confirm the other plugin still loads and stderr names the broken path.
- [ ] **Step 8: Commit** `feat(plugins): discover HTML/JS plugins and bridge them to the host`

---

### Task 12: GitHub pull-request plugin and TSX template

**Files:**
- Create: `crates/calm-plugins/src/github.rs`, `plugins/github-prs/calm-plugin.json`, `plugins/github-prs/src/index.tsx`, `plugins/github-prs/package.json`, `plugins/github-prs/index.html`, `templates/plugin-tsx/*`
- Modify: `plugin_panel.rs` (dispatch `github.api`), `docs/plugins.md`

**Interfaces:**
- Produces (`github.rs`): `GhClient { program: PathBuf /* default "gh" */ }`, `fn api(&self, path: &str) -> Result<serde_json::Value, GhError>` (runs `gh api <path>` with `--paginate` off, captures stdout/stderr; `GhError::{ NotInstalled, NotAuthenticated(String), Rejected(String), InvalidPath(String) }`; `path` must match `^[A-Za-z0-9_./?=&%-]+$` and must not start with `-`). The token never passes through CALM: `gh` holds it.
- Plugin UI: settings `{ "repos": ["owner/name"] }` editable in the panel (text input + add/remove), lists open PRs per repo (`repos/{owner}/{repo}/pulls?state=open&per_page=30`): number, title, author, draft flag, link opens in the system browser through `calm.openExternal(url)` (add to the prelude and dispatch via `open`). States: loading, empty ("No open pull requests"), error (shows `GhError` text, including "gh is not installed" and "run gh auth login").
- Build: `plugins/github-prs/package.json` with `"build": "bun build src/index.tsx --outdir dist --target browser"`, `index.html` loading `dist/index.js`; `templates/plugin-tsx` is the same shape with a hello component. Preact is the JSX runtime (builder's call: small, TSX-compatible).

- [ ] **Step 1: Write failing tests** (`calm-plugins`, with a fake `gh` script on a temp PATH): `api_runs_gh_and_parses_json`, `api_reports_not_installed`, `api_reports_not_authenticated_from_stderr`, `api_rejects_paths_with_flags_or_spaces`.
- [ ] **Step 2: Run** Expected: FAIL. **Step 3: Implement.** **Step 4: Run** Expected: PASS.
- [ ] **Step 5: Build the plugin and template**; `cd plugins/github-prs && bun install && bun run build; echo rc=$?` Expected rc=0 and `dist/index.js` exists (dist is gitignored; docs say to build).
- [ ] **Step 6: Verify** in the app against a repo the account can read (public first, then a private one of the user's choice): PRs listed, error state when `gh` is replaced by a stub that exits 4. Confirm with `rg -n 'ghp_|github_pat_' -g '!target' .; echo rc=$?` that rc=1 (no token strings in the tree).
- [ ] **Step 7: Commit** `feat(plugins): add GitHub pull-request plugin and TSX template`

---

### Task 13: Save and load layouts in the app

**Files:**
- Modify: `main.rs`, `canvas_view.rs`, `toolbar.rs`

**Interfaces:**
- Consumes: `save_layout`, `load_layout`, `Scene::to_layout`, `Scene::from_layout`.
- App: `--layout <name>` loads that layout at start (missing → empty canvas with the name remembered; other errors → print the `LayoutError` and exit 2); `cmd-s` and the `Save` button write the current scene to the current name (default `default`) and flash `Saved <path>` in the toolbar for 2 s. Loading creates fresh terminals (new shells), browsers at their saved url, plugins with their saved settings.

- [ ] **Step 1: Wire** the flag and shortcut.
- [ ] **Step 2: Verify**: arrange two terminals, a browser and the GitHub plugin; `cmd-s`; quit; `cargo run -p calm-app -- --layout default`: same positions and zoom, new shells, browser at its url, plugin settings kept. `jq . ~/.config/calm/layouts/default.json` shows no command history and no tokens. Start with `--layout nope` → empty canvas; corrupt a copy and load it → error names the path, exit 2.
- [ ] **Step 3: Commit** `feat(app): save and load named layouts`

---

### Task 14: Benchmark mode and results

**Files:**
- Create: `crates/calm-app/src/bench.rs`, `docs/benchmarks.md`

**Interfaces:**
- App flags: `--bench <n>` (terminals in a grid, each running `sh -c 'while :; do cat /usr/share/dict/words; done'`), `--bench-idle` (same grid, idle shells), `--bench-seconds <s>` (default 30), `--bench-zoom <z>` (default 1.0; 0.4 exercises preview mode). The run records frame intervals via a `window.on_next_frame` loop, samples `ps -o %cpu=,rss= -p <pid>` once per second, then prints one JSON line `{ "n", "mode", "zoom", "seconds", "frames", "frame_ms": { "p50", "p95", "p99", "max" }, "over_16_7ms", "cpu_pct": { "avg", "max" }, "rss_mb": { "avg", "max" } }` and exits 0.
- Budget (recorded in the doc before running): p95 frame interval ≤ 16.7 ms at zoom 1.0 for idle terminals, ≤ 33 ms with 20 output-producing terminals; anything else is reported as a miss, not tuned away silently.

- [ ] **Step 1: Implement `bench.rs`** (pure stats helpers `percentile(&mut Vec<f64>, p)` tested in `calm-core`).
- [ ] **Step 2: Run** the matrix on the development Mac with the display refresh rate noted: `for n in 5 10 20; do cargo run --release -p calm-app -- --bench $n; cargo run --release -p calm-app -- --bench $n --bench-idle; done; cargo run --release -p calm-app -- --bench 20 --bench-zoom 0.4` each followed by `echo rc=$?`.
- [ ] **Step 3: Write `docs/benchmarks.md`**: hardware (`sysctl -n machdep.cpu.brand_string`, memory, macOS version, display refresh), method, budget, the JSON lines, and a plain verdict per row (met / missed). No row without a recorded run.
- [ ] **Step 4: Commit** `perf: add benchmark mode and record 5/10/20 terminal results`

---

### Task 15: Documentation, code graph and branch finish

**Files:**
- Create: `docs/architecture.md`
- Modify: `CLAUDE.md`, `README.md`, `docs/dev-setup.md`

- [ ] **Step 1: Write `docs/architecture.md`**: crate map, the native-child model (GPUI paints under native views; container clip and zoom; mouse unzoom), preview mode, layout format v1, plugin bridge, known limits (blurry upscaled terminals above zoom 1, web panels have no snapshot preview, modal UI hides natives, single window), and the "Verified on" log from Tasks 8 to 10.
- [ ] **Step 2: Update `CLAUDE.md`** with the final crate list, test commands and the GUI gate.
- [ ] **Step 3: Code graph**: `repo-intel config --show` then `repo-intel build; echo rc=$?`. Expected rc=0; if it prints unparsed files, list them in the commit message. A nonzero exit is reported, not worked around.
- [ ] **Step 4: Full verification**: `cargo fmt --all -- --check; echo rc=$?`, `cargo clippy --workspace --all-targets -- -D warnings; echo rc=$?`, `cargo test --workspace; echo rc=$?`, `cargo run -p calm-app -- --smoke; echo rc=$?`. All rc=0.
- [ ] **Step 5: Commit** `docs: add architecture notes and refresh project contract`, then use superpowers:finishing-a-development-branch to merge or open a PR from `feat/canvas-prototype`.
