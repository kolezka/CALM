//! The canvas: paints panels from the `calm-core` scene and feeds it input.

use calm_core::chrome::{
    CORNER_RADIUS, ChromeButton, PanelIcon, TITLE_BAR_HEIGHT, button_dot, button_strokes,
    details_visible, hover_target, icon_strokes, title_bar_layout,
};
use calm_core::frame_stats::FrameStats;
use calm_core::geometry::{Point, Rect, Size};
use calm_core::interaction::{
    Button, ConfirmKey, Effect, InputHandler, Interaction, Modifiers, Tool, ZoomStep,
};
use calm_core::menu::{self, MenuCommand, MenuItem};
use calm_core::preview::{PREVIEW_REFRESH_SECS, PanelMode, PreviewImage, decide_mode};
use calm_core::scene::{Panel, PanelId, PanelKind, Scene, content_rect};
use calm_core::viewport::{NativePlacement, place_native};
use calm_native::{HostView, NativeChild, show_context_menu};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, BorderStyle, Bounds, BoxShadow, ContentMask, Context, Corners, DispatchPhase, Edges,
    Entity, FocusHandle, Focusable, InteractiveElement, IntoElement, KeyDownEvent, KeyUpEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, NoAction, ParentElement,
    PathBuilder, PinchEvent, Pixels, Render, RenderImage, ScrollDelta, ScrollWheelEvent,
    SharedString, Styled, Task, TextAlign, TextRun, WeakEntity, Window, actions, canvas, div, fill,
    outline, point, px, quad, rgb, rgba, size,
};

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::dev_menu::{ResetFps, ToggleFps};
use crate::settings_menu::{self, SettingsState};
use crate::terminal::{self, TerminalPanel};
use crate::theme;
use crate::toolbar::{TOOLBAR_HEIGHT, toolbar};

actions!(
    calm,
    [
        ZoomIn,
        ZoomOut,
        ZoomReset,
        DrawTerminal,
        DrawBrowser,
        Cancel,
        ConfirmEnter,
        ToggleFullscreen
    ]
);

pub const KEY_CONTEXT: &str = "Canvas";
const TITLE_FONT_SIZE: f64 = 12.0;
/// Below this the title is unreadable, so it is skipped.
const MIN_TITLE_FONT_PX: f64 = 6.0;
/// Width of the ring around a panel, in screen pixels at any zoom.
const BORDER_PX: f64 = 1.0;
/// Width of the `preview stale` label in the chrome strip, in world units.
const STALE_LABEL_WIDTH: f64 = 96.0;
const STALE_LABEL: &str = "preview stale";
/// Added to the refresh timer so its repaint lands just after a snapshot is due.
const PREVIEW_TIMER_MARGIN: Duration = Duration::from_millis(100);
const CONFIRM_TITLE: &str = "Close? enter / esc";

pub fn key_bindings() -> Vec<gpui::KeyBinding> {
    use gpui::KeyBinding;
    let ctx = Some(KEY_CONTEXT);
    vec![
        KeyBinding::new("cmd-=", ZoomIn, ctx),
        KeyBinding::new("cmd--", ZoomOut, ctx),
        KeyBinding::new("cmd-0", ZoomReset, ctx),
        KeyBinding::new("t", DrawTerminal, ctx),
        KeyBinding::new("b", DrawBrowser, ctx),
        KeyBinding::new("escape", Cancel, ctx),
        KeyBinding::new("enter", ConfirmEnter, ctx),
        KeyBinding::new("cmd-ctrl-f", ToggleFullscreen, ctx),
        // A focused terminal gets these keys; canvas zoom and fullscreen still apply.
        KeyBinding::new("t", NoAction, Some(terminal::KEY_CONTEXT)),
        KeyBinding::new("b", NoAction, Some(terminal::KEY_CONTEXT)),
        KeyBinding::new("escape", NoAction, Some(terminal::KEY_CONTEXT)),
        KeyBinding::new("enter", NoAction, Some(terminal::KEY_CONTEXT)),
    ]
}

pub struct CanvasView {
    pub scene: Scene,
    pub input: InputHandler,
    /// Window pixels of the canvas area below the toolbar. Set during prepaint.
    pub viewport: Rect,
    /// (id, name) per discovered plugin. Task 11 fills it.
    pub plugins: Vec<(String, String)>,
    /// (id, default panel size) per discovered plugin. Task 11 fills it.
    pub plugin_sizes: Vec<(String, Size)>,
    /// Path of the last saved layout, shown in the status strip. Task 13 sets it.
    pub saved_flash: Option<String>,
    focused: Option<PanelId>,
    /// Panel under the pointer and the chrome button under it. Shows the buttons.
    hover: Option<(PanelId, Option<ChromeButton>)>,
    /// Live terminals by panel. Dropping an entry ends its process.
    terminals: Vec<(PanelId, Entity<TerminalPanel>)>,
    /// Back-to-front terminal order the native views were last stacked in.
    native_order: Vec<PanelId>,
    focus_handle: FocusHandle,
    pub show_fps: bool,
    pub frame_stats: FrameStats,
    /// True while a native context menu is up. Terminals show previews meanwhile.
    menu_open: bool,
    /// Repaints every `PREVIEW_REFRESH_SECS` while any terminal is in `Preview`.
    preview_timer: Option<Task<()>>,
    /// Terminal input divs that a paint found over a `Preview` panel. Smoke expects 0.
    pub preview_input_targets: usize,
    pub settings: SettingsState,
}

impl CanvasView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            scene: Scene::default(),
            input: InputHandler::default(),
            viewport: Rect::new(0.0, f64::from(TOOLBAR_HEIGHT), 0.0, 0.0),
            plugins: Vec::new(),
            plugin_sizes: Vec::new(),
            saved_flash: None,
            focused: None,
            hover: None,
            terminals: Vec::new(),
            native_order: Vec::new(),
            focus_handle: cx.focus_handle(),
            show_fps: false,
            frame_stats: FrameStats::default(),
            menu_open: false,
            preview_timer: None,
            preview_input_targets: 0,
            settings: SettingsState {
                settings: Default::default(),
                path: calm_core::settings::settings_path(),
            },
        }
    }

    pub fn set_show_fps(&mut self, show: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.show_fps = show;
        self.frame_stats.reset();
        // Rebuild the menu bar so the Show FPS check mark follows the state.
        let settings = self.settings.settings;
        cx.defer(move |cx| cx.set_menus(settings_menu::menus(show, &settings)));
        window.refresh();
        cx.notify();
    }

    fn reset_fps(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.frame_stats.reset();
        window.refresh();
        cx.notify();
    }

    pub fn apply_effects(
        &mut self,
        effects: Vec<Effect>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // CreatePanel has no id; the model emits FocusPanel(id) for it right after.
        let mut created: Option<PanelKind> = None;
        let mut queue: std::collections::VecDeque<Effect> = effects.into();
        while let Some(effect) = queue.pop_front() {
            match effect {
                Effect::CreatePanel { kind, .. } => created = Some(kind),
                Effect::FocusPanel(id) => {
                    if let Some(kind) = created.take() {
                        self.panel_created(id, &kind, window, cx);
                    }
                    self.focused = Some(id);
                    if let Some(terminal) = self.terminal(id) {
                        terminal.update(cx, |terminal, cx| terminal.focus(window, cx));
                    }
                }
                Effect::ClosePanelRequested(id) => {
                    let alive = self
                        .terminal(id)
                        .is_some_and(|terminal| terminal.read(cx).is_alive());
                    let confirm = alive && self.settings.settings.confirm_close;
                    queue.push_back(self.input.request_close(id, confirm));
                }
                Effect::ConfirmClose(_) => {
                    // Enter and Escape must reach the canvas, not the terminal.
                    self.focus_handle.focus(window, cx);
                }
                Effect::ToggleMinimize(id) => {
                    if let Some(panel) = self.scene.get_mut(id) {
                        panel.minimized = !panel.minimized;
                    }
                }
                Effect::Redraw => {}
                Effect::ShowCanvasMenu { screen, world } => {
                    let items = menu::canvas_menu(&self.plugins);
                    self.show_menu(items, screen, MenuTarget::Canvas { world }, window, cx);
                }
                Effect::ShowPanelMenu { id, screen } => {
                    let minimized = self.scene.get(id).is_some_and(|panel| panel.minimized);
                    let items = menu::panel_menu(minimized);
                    self.show_menu(items, screen, MenuTarget::Panel(id), window, cx);
                }
                Effect::ClosePanel(id) => {
                    if let Some(terminal) = self.terminal(id) {
                        terminal.update(cx, |terminal, _| terminal.release_preview(window));
                    }
                    self.terminals.retain(|(panel, _)| *panel != id);
                    self.scene.remove(id);
                    if self.focused == Some(id) {
                        self.focused = None;
                    }
                }
            }
        }
        window.refresh();
        cx.notify();
    }

    /// Pops up a native menu at the canvas point `screen` and applies the choice.
    /// The menu tracks modally, so it runs in a task instead of inside this event handler:
    /// GPUI keeps drawing while the menu is open and must not find the app borrowed.
    fn show_menu(
        &mut self,
        items: Vec<MenuItem>,
        screen: Point,
        target: MenuTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let host = match HostView::new(window) {
            Ok(host) => host,
            Err(err) => {
                eprintln!("calm: context menu unavailable: {err}");
                return;
            }
        };
        let at = Point::new(
            screen.x + self.viewport.origin.x,
            screen.y + self.viewport.origin.y,
        );
        // A menu counts as modal: terminals switch to previews while it is up.
        self.menu_open = true;
        window.refresh();
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let choice = show_context_menu(&items, at, &host);
            this.update_in(cx, |view, window, cx| {
                view.menu_open = false;
                match choice {
                    Some(id) => view.apply_menu_choice(&id, target, window, cx),
                    None => {
                        window.refresh();
                        cx.notify();
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    fn apply_menu_choice(
        &mut self,
        id: &str,
        target: MenuTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let world = match target {
            MenuTarget::Canvas { world } => world,
            MenuTarget::Panel(_) => Point::new(0.0, 0.0),
        };
        let effects = match (menu::menu_action(id, world, &self.plugin_sizes), target) {
            (Some(MenuCommand::AddPanel { kind, rect }), MenuTarget::Canvas { .. }) => {
                let id = self.scene.add(kind.clone(), rect);
                let rect = self.scene.get(id).map_or(rect, |panel| panel.rect);
                vec![Effect::CreatePanel { kind, rect }, Effect::FocusPanel(id)]
            }
            (Some(MenuCommand::Minimize), MenuTarget::Panel(id)) => {
                vec![Effect::ToggleMinimize(id), Effect::Redraw]
            }
            (Some(MenuCommand::Close), MenuTarget::Panel(id)) => {
                vec![Effect::ClosePanelRequested(id)]
            }
            _ => return,
        };
        self.apply_effects(effects, window, cx);
    }

    fn panel_created(
        &mut self,
        id: PanelId,
        kind: &PanelKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if let PanelKind::Terminal { command, cwd } = kind {
            let mut options = terminal::options(command.as_deref(), cwd.clone());
            settings_menu::apply_terminal_config(&mut options, &self.settings.settings);
            if let Err(err) = self.spawn_terminal(id, options, window, cx) {
                eprintln!("calm: terminal failed to start: {err}");
            }
        }
    }

    pub fn spawn_terminal(
        &mut self,
        id: PanelId,
        options: gpui_libghostty::TerminalOptions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let host = NativeChild::new(window).map_err(|err| err.to_string())?;
        let terminal = TerminalPanel::spawn(options, host, window, cx)?;
        self.terminals.push((id, terminal));
        Ok(())
    }

    fn terminal(&self, id: PanelId) -> Option<Entity<TerminalPanel>> {
        self.terminals
            .iter()
            .find(|(panel, _)| *panel == id)
            .map(|(_, terminal)| terminal.clone())
    }

    /// (frame count, alive) per terminal, in creation order.
    pub fn terminal_status(&self, cx: &mut App) -> Vec<(u64, bool)> {
        self.terminals
            .iter()
            .map(|(_, terminal)| {
                terminal.update(cx, |terminal, _| {
                    (terminal.frame_count(), terminal.is_alive())
                })
            })
            .collect()
    }

    /// (mode, has a snapshot) per terminal, in creation order.
    pub fn terminal_previews(&self, cx: &App) -> Vec<(PanelMode, bool)> {
        self.terminals
            .iter()
            .map(|(_, terminal)| {
                let terminal = terminal.read(cx);
                (terminal.mode(), terminal.preview().image().is_some())
            })
            .collect()
    }

    /// Snapshot attempts per terminal, in creation order.
    pub fn snapshot_counts(&self, cx: &App) -> Vec<u64> {
        self.terminals
            .iter()
            .map(|(_, terminal)| terminal.read(cx).snapshot_count())
            .collect()
    }

    /// Runs the refresh timer only while some terminal is in `Preview`.
    fn sync_preview_timer(&mut self, previewing: bool, cx: &mut Context<Self>) {
        if !previewing {
            self.preview_timer = None;
            return;
        }
        if self.preview_timer.is_some() {
            return;
        }
        self.preview_timer = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(PREVIEW_REFRESH_SECS) + PREVIEW_TIMER_MARGIN)
                    .await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        }));
    }

    /// Terminal ids in scene order, back to front.
    fn terminal_order(&self) -> Vec<PanelId> {
        self.scene
            .panels
            .iter()
            .map(|panel| panel.id)
            .filter(|id| self.terminals.iter().any(|(t, _)| t == id))
            .collect()
    }

    /// Native views stack in creation order, so restack them when the scene order changes.
    fn sync_native_order(&mut self, cx: &mut Context<Self>) {
        let order = self.terminal_order();
        if order == self.native_order {
            return;
        }
        for id in &order {
            if let Some(terminal) = self.terminal(*id) {
                terminal.update(cx, |terminal, _| terminal.bring_to_front());
            }
        }
        self.native_order = order;
    }

    /// (scene order, native stacking order) of terminal ids, both back to front.
    pub fn terminal_zorder(&self, cx: &App) -> (Vec<u64>, Vec<u64>) {
        let scene = self.terminal_order().iter().map(|id| id.0).collect();
        let mut native: Vec<(Option<usize>, u64)> = self
            .terminals
            .iter()
            .map(|(id, terminal)| (terminal.read(cx).stack_index(), id.0))
            .collect();
        native.sort();
        (scene, native.into_iter().map(|(_, id)| id).collect())
    }

    fn confirm_enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(effect) = self.input.confirm_key(ConfirmKey::Enter) {
            self.apply_effects(vec![effect], window, cx);
        }
    }

    fn to_canvas(&self, position: gpui::Point<Pixels>) -> Point {
        Point::new(
            f64::from(f32::from(position.x)) - self.viewport.origin.x,
            f64::from(f32::from(position.y)) - self.viewport.origin.y,
        )
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let button = match event.button {
            MouseButton::Left => Button::Left,
            MouseButton::Middle => Button::Middle,
            MouseButton::Right => Button::Right,
            _ => return,
        };
        // A right click on a terminal's content must not take its focus.
        if button != Button::Right {
            self.focus_handle.focus(window, cx);
        }
        let screen = self.to_canvas(event.position);
        let effects =
            self.input
                .mouse_down(screen, button, to_mods(&event.modifiers), &mut self.scene);
        self.apply_effects(effects, window, cx);
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        let screen = self.to_canvas(event.position);
        let effects = self
            .input
            .mouse_move(screen, to_mods(&event.modifiers), &mut self.scene);
        let hover = hover_target(&self.scene, self.scene.camera.screen_to_world(screen));
        let hover_changed = hover != self.hover;
        self.hover = hover;
        if !effects.is_empty() {
            self.apply_effects(effects, window, cx);
        } else if hover_changed {
            cx.notify();
        }
    }

    fn mouse_up(&mut self, event: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(event.button, MouseButton::Left | MouseButton::Middle) {
            return;
        }
        let effects = self.input.mouse_up(
            self.to_canvas(event.position),
            to_mods(&event.modifiers),
            &mut self.scene,
        );
        if !effects.is_empty() {
            self.apply_effects(effects, window, cx);
        }
    }

    fn scroll(&mut self, event: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        let delta = match event.delta {
            ScrollDelta::Pixels(_) => event.delta.pixel_delta(px(0.0)),
            ScrollDelta::Lines(_) => event.delta.pixel_delta(px(20.0)),
        };
        let screen = self.to_canvas(event.position);
        self.input.scroll(
            screen,
            f64::from(f32::from(delta.x)),
            f64::from(f32::from(delta.y)),
            to_mods(&event.modifiers),
            &mut self.scene,
        );
        window.refresh();
        cx.notify();
    }

    fn pinch(&mut self, event: &PinchEvent, window: &mut Window, cx: &mut Context<Self>) {
        let screen = self.to_canvas(event.position);
        let effects = self
            .input
            .pinch(screen, f64::from(event.delta), &mut self.scene);
        if !effects.is_empty() {
            self.apply_effects(effects, window, cx);
        }
    }

    fn key_zoom(&mut self, step: ZoomStep, window: &mut Window, cx: &mut Context<Self>) {
        let center = Point::new(
            self.viewport.size.width / 2.0,
            self.viewport.size.height / 2.0,
        );
        self.input.key_zoom(step, center, &mut self.scene);
        window.refresh();
        cx.notify();
    }

    fn set_tool(&mut self, tool: Tool, window: &mut Window, cx: &mut Context<Self>) {
        self.input.tool = tool;
        window.refresh();
        cx.notify();
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.input.confirming.is_some() {
            if let Some(effect) = self.input.confirm_key(ConfirmKey::Escape) {
                self.apply_effects(vec![effect], window, cx);
            }
            return;
        }
        self.set_tool(Tool::Select, window, cx);
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let mods = event.keystroke.modifiers;
        if event.keystroke.key == "space" && !mods.platform && !mods.control && !mods.alt {
            self.input.space_held = true;
            window.refresh();
            cx.stop_propagation();
        }
    }

    fn key_up(&mut self, event: &KeyUpEvent, window: &mut Window, _cx: &mut Context<Self>) {
        if event.keystroke.key == "space" {
            self.input.space_held = false;
            window.refresh();
        }
    }
}

/// What a context menu was opened on.
#[derive(Clone, Copy, Debug)]
enum MenuTarget {
    /// Empty canvas; new panels go at `world`.
    Canvas {
        world: Point,
    },
    Panel(PanelId),
}

/// What paint needs, copied out at render time so painting does not borrow the view.
struct PaintSnapshot {
    scene: Scene,
    state: Interaction,
    focused: Option<PanelId>,
    confirming: Option<PanelId>,
    hover: Option<(PanelId, Option<ChromeButton>)>,
    terminals: Vec<(PanelId, Entity<TerminalPanel>)>,
    /// Close confirmation or context menu up: native views must not cover it.
    modal_open: bool,
    /// Terminals that got an input div in this render.
    input_targets: Vec<PanelId>,
    settings: calm_core::settings::Settings,
}

/// How a terminal panel's content is drawn this paint.
enum TerminalContent {
    /// The native view covers the content rect.
    Live,
    /// A GPUI-drawn snapshot, possibly stale, or a flat rect with the title.
    Preview(PreviewImage<Arc<RenderImage>>),
}

/// The mode a terminal panel gets this frame. Render and paint both use it, so input
/// targets match what paint applies. `viewport` is the canvas area; only its size counts.
fn terminal_mode(panel: &Panel, scene: &Scene, viewport: Rect, modal_open: bool) -> PanelMode {
    let local = Rect::new(0.0, 0.0, viewport.size.width, viewport.size.height);
    let screen = scene.camera.world_rect_to_screen(content_rect(panel));
    decide_mode(
        scene.camera.zoom,
        screen,
        local,
        panel.minimized,
        modal_open,
    )
}

/// A panel's native placement in window points, or `None` when minimized or off screen.
/// `viewport` is the canvas area in window points.
fn terminal_placement(panel: &Panel, scene: &Scene, viewport: Rect) -> Option<NativePlacement> {
    if panel.minimized {
        return None;
    }
    let local = Rect::new(0.0, 0.0, viewport.size.width, viewport.size.height);
    let mut placement = place_native(content_rect(panel), &scene.camera, local)?;
    placement.container.origin.x += viewport.origin.x;
    placement.container.origin.y += viewport.origin.y;
    Some(placement)
}

impl PaintSnapshot {
    fn paint_canvas(
        &self,
        bounds: Bounds<Pixels>,
        this: WeakEntity<CanvasView>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let origin = point(
            f64::from(f32::from(bounds.origin.x)),
            f64::from(f32::from(bounds.origin.y)),
        );
        let mut previewing = false;
        let mut modes_changed = false;
        let mut preview_targets = 0;
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            let background = self.settings.background.rgb();
            window.paint_quad(fill(bounds, rgb(background)));
            let camera = self.scene.camera;
            if self.settings.show_grid {
                settings_menu::paint_grid(bounds, &camera, background, window);
            }
            let viewport = Rect::new(
                origin.x,
                origin.y,
                f64::from(f32::from(bounds.size.width)),
                f64::from(f32::from(bounds.size.height)),
            );
            let (contents, changed) = self.apply_terminal_modes(viewport, window, cx);
            modes_changed = changed;
            preview_targets = self
                .input_targets
                .iter()
                .filter(|id| matches!(contents.get(id), Some(TerminalContent::Preview(_))))
                .count();
            previewing = contents
                .values()
                .any(|content| matches!(content, TerminalContent::Preview(_)));
            // Panels paint inside this mask; nested masks intersect with it, so nothing
            // a panel draws can reach the toolbar.
            for panel in &self.scene.panels {
                self.paint_panel(panel, contents.get(&panel.id), origin, window, cx);
            }
            if let Interaction::Drawing { draft, .. } = self.state {
                // Painted from the snapped world rect so the outline matches the panel created.
                let top_left = camera.world_to_screen(draft.origin);
                let bottom_right =
                    camera.world_to_screen(Point::new(draft.right(), draft.bottom()));
                let draft = Rect::new(
                    top_left.x,
                    top_left.y,
                    bottom_right.x - top_left.x,
                    bottom_right.y - top_left.y,
                );
                window.paint_quad(outline(
                    to_bounds(draft, origin),
                    rgb(theme::DRAFT_OUTLINE),
                    BorderStyle::Dashed,
                ));
            }
        });

        this.update(cx, |view, cx| {
            view.sync_native_order(cx);
            view.sync_preview_timer(previewing, cx);
            view.preview_input_targets += preview_targets;
            // Render picks input targets from the mode; rebuild them if paint disagreed.
            if modes_changed {
                cx.notify();
            }
        })
        .ok();

        // Moves and releases are tracked window-wide so a drag keeps going past the canvas edge.
        let weak = this.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble {
                weak.update(cx, |view, cx| view.mouse_move(event, window, cx))
                    .ok();
            }
        });
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble {
                this.update(cx, |view, cx| view.mouse_up(event, window, cx))
                    .ok();
            }
        });
    }

    /// Decides each terminal's mode, applies it, and returns what to draw in its content
    /// rect, plus whether any mode changed. `viewport` is the canvas area in window points.
    fn apply_terminal_modes(
        &self,
        viewport: Rect,
        window: &mut Window,
        cx: &mut App,
    ) -> (HashMap<PanelId, TerminalContent>, bool) {
        let mut contents = HashMap::new();
        let mut changed = false;
        for (id, terminal) in &self.terminals {
            let Some(panel) = self.scene.get(*id) else {
                continue;
            };
            let mode = terminal_mode(panel, &self.scene, viewport, self.modal_open);
            let placement = terminal_placement(panel, &self.scene, viewport);
            let content = terminal.update(cx, |terminal, _| {
                changed |= terminal.mode() != mode;
                terminal.apply_mode(mode, placement, window);
                match mode {
                    PanelMode::Preview => Some(TerminalContent::Preview(terminal.preview())),
                    PanelMode::Live => Some(TerminalContent::Live),
                    PanelMode::Hidden => None,
                }
            });
            if let Some(content) = content {
                contents.insert(*id, content);
            }
        }
        (contents, changed)
    }

    fn paint_panel(
        &self,
        panel: &Panel,
        content: Option<&TerminalContent>,
        origin: gpui::Point<f64>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let camera = self.scene.camera;
        let zoom = camera.zoom;
        let focused = self.focused == Some(panel.id);
        let confirming = self.confirming == Some(panel.id);
        let hovered_button = match self.hover {
            Some((id, button)) if id == panel.id => Some(button),
            _ => None,
        };
        let layout = title_bar_layout(panel);
        let (bar_color, border_color, text_color) = if confirming {
            (
                theme::CONFIRM_BAR,
                theme::CONFIRM_BORDER,
                theme::CONFIRM_TEXT,
            )
        } else if focused {
            (
                theme::PANEL_BAR_FOCUSED,
                theme::ACCENT,
                theme::TITLE_TEXT_FOCUSED,
            )
        } else {
            (theme::PANEL_BAR, theme::PANEL_BORDER, theme::TITLE_TEXT)
        };

        // Shadow and ring sit outside the panel rect, so neither reaches into native content.
        // Bottom corners stay square on a full panel: the native view below is square.
        let body = camera.world_rect_to_screen(if panel.minimized {
            layout.bar
        } else {
            panel.rect
        });
        let radius = CORNER_RADIUS * zoom;
        let bottom_radius = if panel.minimized { radius } else { 0.0 };
        let shadow_color = if focused {
            theme::SHADOW_FOCUSED
        } else {
            theme::SHADOW
        };
        window.paint_drop_shadows(
            to_bounds(body, origin),
            corners(radius, bottom_radius),
            &[BoxShadow {
                color: rgba(shadow_color).into(),
                offset: point(px(0.), px((4.0 * zoom).clamp(1.0, 6.0) as f32)),
                blur_radius: px((18.0 * zoom).clamp(2.0, 24.0) as f32),
                spread_radius: px(0.),
                inset: false,
            }],
        );
        let ring = Rect::new(
            body.origin.x - BORDER_PX,
            body.origin.y - BORDER_PX,
            body.size.width + 2.0 * BORDER_PX,
            body.size.height + 2.0 * BORDER_PX,
        );
        let ring_bottom = if panel.minimized {
            bottom_radius + BORDER_PX
        } else {
            0.0
        };
        window.paint_quad(quad(
            to_bounds(ring, origin),
            corners(radius + BORDER_PX, ring_bottom),
            rgb(bar_color),
            Edges::all(px(BORDER_PX as f32)),
            rgb(border_color),
            BorderStyle::Solid,
        ));

        if !panel.minimized {
            let content_screen = camera.world_rect_to_screen(content_rect(panel));
            let content_bounds = to_bounds(content_screen, origin);
            let divider = Rect::new(
                content_screen.origin.x,
                content_screen.origin.y - BORDER_PX,
                content_screen.size.width,
                BORDER_PX,
            );
            window.paint_quad(fill(to_bounds(divider, origin), rgb(theme::PANEL_DIVIDER)));
            window.paint_quad(fill(content_bounds, rgb(theme::CONTENT_PLACEHOLDER)));
            match content {
                Some(TerminalContent::Preview(
                    PreviewImage::Fresh(image) | PreviewImage::Stale(image),
                )) => {
                    let painted = window.paint_image(
                        content_bounds,
                        content_bounds,
                        Default::default(),
                        image.clone(),
                        0,
                        false,
                    );
                    if let Err(err) = painted {
                        eprintln!("calm: preview paint failed: {err:#}");
                    }
                }
                Some(TerminalContent::Preview(PreviewImage::None)) => {
                    // No snapshot has succeeded yet: flat rect with the title.
                    let padding = 8.0 * zoom;
                    let line = Rect::new(
                        content_screen.origin.x + padding,
                        content_screen.origin.y + padding,
                        (content_screen.size.width - 2.0 * padding).max(0.0),
                        TITLE_BAR_HEIGHT * zoom,
                    );
                    paint_text(
                        &panel.title,
                        line,
                        TextLook {
                            align: TextAlign::Left,
                            color: theme::TITLE_TEXT,
                        },
                        zoom,
                        origin,
                        window,
                        cx,
                    );
                }
                Some(TerminalContent::Live) | None => {}
            }
        }

        if !details_visible(zoom) {
            return;
        }
        let icon_color = match PanelIcon::for_kind(&panel.kind) {
            PanelIcon::Terminal => theme::ICON_TERMINAL,
            PanelIcon::Browser => theme::ICON_BROWSER,
            PanelIcon::Plugin => theme::ICON_PLUGIN,
        };
        let stroke = (1.5 * zoom).max(1.0);
        paint_strokes(
            &icon_strokes(PanelIcon::for_kind(&panel.kind)),
            camera.world_rect_to_screen(layout.icon),
            stroke,
            icon_color,
            origin,
            window,
        );

        // Buttons show while the pointer is over the panel, and during a close confirmation.
        if hovered_button.is_some() || confirming {
            let buttons = [
                (ChromeButton::Minimize, layout.minimize),
                (ChromeButton::Close, layout.close),
            ];
            for (button, hit) in buttons {
                let Some(hit) = hit else { continue };
                let dot = camera.world_rect_to_screen(button_dot(hit));
                let lit = hovered_button == Some(Some(button))
                    || (confirming && button == ChromeButton::Close);
                let (dot_color, glyph_color) = match (lit, button) {
                    (true, ChromeButton::Close) => {
                        (theme::BUTTON_CLOSE_HOVER, theme::BUTTON_GLYPH_ON_COLOR)
                    }
                    (true, ChromeButton::Minimize) => {
                        (theme::BUTTON_MINIMIZE_HOVER, theme::BUTTON_GLYPH_ON_COLOR)
                    }
                    (false, _) => (theme::BUTTON_IDLE, theme::BUTTON_GLYPH),
                };
                window.paint_quad(
                    fill(to_bounds(dot, origin), rgb(dot_color))
                        .corner_radii(Corners::all(px((dot.size.width / 2.0) as f32))),
                );
                paint_strokes(
                    &button_strokes(button),
                    dot,
                    stroke,
                    glyph_color,
                    origin,
                    window,
                );
            }
        }

        let mut middle = camera.world_rect_to_screen(layout.middle);
        let stale = !panel.minimized
            && matches!(content, Some(TerminalContent::Preview(image)) if image.is_stale());
        if stale {
            let width = (STALE_LABEL_WIDTH * zoom).min(middle.size.width);
            let label = Rect::new(
                middle.right() - width,
                middle.origin.y,
                width,
                middle.size.height,
            );
            paint_text(
                STALE_LABEL,
                label,
                TextLook {
                    align: TextAlign::Right,
                    color: theme::STALE_TEXT,
                },
                zoom,
                origin,
                window,
                cx,
            );
            middle.size.width = (middle.size.width - width - 8.0 * zoom).max(0.0);
        }
        let title = if confirming {
            CONFIRM_TITLE
        } else {
            panel.title.as_str()
        };
        paint_text(
            title,
            middle,
            TextLook {
                align: TextAlign::Left,
                color: text_color,
            },
            zoom,
            origin,
            window,
            cx,
        );
    }
}

fn corners(top: f64, bottom: f64) -> Corners<Pixels> {
    Corners {
        top_left: px(top as f32),
        top_right: px(top as f32),
        bottom_right: px(bottom as f32),
        bottom_left: px(bottom as f32),
    }
}

/// Strokes unit-square polylines (see `calm_core::chrome::icon_strokes`) into `area`.
fn paint_strokes(
    strokes: &[Vec<(f64, f64)>],
    area: Rect,
    width: f64,
    color: u32,
    origin: gpui::Point<f64>,
    window: &mut Window,
) {
    for polyline in strokes {
        let mut builder = PathBuilder::stroke(px(width as f32));
        for (index, (x, y)) in polyline.iter().enumerate() {
            let at = point(
                px((origin.x + area.origin.x + x * area.size.width) as f32),
                px((origin.y + area.origin.y + y * area.size.height) as f32),
            );
            if index == 0 {
                builder.move_to(at);
            } else {
                builder.line_to(at);
            }
        }
        match builder.build() {
            Ok(path) => window.paint_path(path, rgb(color)),
            Err(err) => eprintln!("calm: chrome stroke failed: {err:?}"),
        }
    }
}

/// Alignment and color of one `paint_text` line.
struct TextLook {
    align: TextAlign,
    color: u32,
}

/// One line of text, aligned and clipped to `rect` (canvas pixels). Skipped when too small.
fn paint_text(
    text: &str,
    rect: Rect,
    look: TextLook,
    zoom: f64,
    origin: gpui::Point<f64>,
    window: &mut Window,
    cx: &mut App,
) {
    let font_px = TITLE_FONT_SIZE * zoom;
    if font_px < MIN_TITLE_FONT_PX || text.is_empty() {
        return;
    }
    let title: SharedString = text.to_string().into();
    let run = TextRun {
        len: title.len(),
        font: window.text_style().font(),
        color: rgb(look.color).into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window
        .text_system()
        .shape_line(title, px(font_px as f32), &[run], None);
    let text_bounds = to_bounds(rect, origin);
    window.with_content_mask(
        Some(ContentMask {
            bounds: text_bounds,
        }),
        |window| {
            if let Err(err) = line.paint(
                text_bounds.origin,
                px(rect.size.height as f32),
                look.align,
                Some(text_bounds.size.width),
                window,
                cx,
            ) {
                eprintln!("calm: title paint failed: {err:#}");
            }
        },
    );
}

impl Focusable for CanvasView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for CanvasView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.show_fps {
            // Each render records a frame and asks for the next one, so the readout
            // measures the frame rate the window can sustain. Off means no extra frames.
            self.frame_stats.record(Instant::now());
            window.request_animation_frame();
        }
        let entity = cx.entity().downgrade();
        let paint_entity = entity.clone();
        let mut snapshot = PaintSnapshot {
            scene: self.scene.clone(),
            state: self.input.state,
            focused: self.focused,
            confirming: self.input.confirming,
            hover: self.hover,
            terminals: self.terminals.clone(),
            modal_open: self.input.confirming.is_some() || self.menu_open,
            input_targets: Vec::new(),
            settings: self.settings.settings,
        };
        // Mouse and key targets for terminals, at last frame's viewport.
        let viewport = self.viewport;
        // Only live terminals take input; a preview's clicks and scrolls go to the canvas.
        // The mode comes from the current scene, not last paint, so a target never
        // outlives the switch to `Preview`.
        let placements: Vec<Option<NativePlacement>> = self
            .terminals
            .iter()
            .map(|(id, _)| {
                self.scene
                    .get(*id)
                    .filter(|panel| {
                        terminal_mode(panel, &self.scene, viewport, snapshot.modal_open)
                            == PanelMode::Live
                    })
                    .and_then(|panel| terminal_placement(panel, &self.scene, viewport))
            })
            .collect();
        snapshot.input_targets = self
            .terminals
            .iter()
            .zip(&placements)
            .filter(|(_, placement)| placement.is_some())
            .map(|((id, _), _)| *id)
            .collect();
        let terminal_targets =
            self.terminals
                .iter()
                .zip(placements)
                .map(|((_, terminal), placement)| {
                    let rect = placement.map_or(Rect::new(0.0, 0.0, 0.0, 0.0), |p| p.container);
                    div()
                        .absolute()
                        .left(px((rect.origin.x - viewport.origin.x) as f32))
                        .top(px((rect.origin.y - viewport.origin.y) as f32))
                        .w(px(rect.size.width as f32))
                        .h(px(rect.size.height as f32))
                        .child(terminal.clone())
                });
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(self.settings.settings.background.rgb()))
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::key_down))
            .on_key_up(cx.listener(Self::key_up))
            .on_pinch(cx.listener(Self::pinch))
            .on_action(
                cx.listener(|view, _: &ZoomIn, window, cx| view.key_zoom(ZoomStep::In, window, cx)),
            )
            .on_action(
                cx.listener(|view, _: &ZoomOut, window, cx| {
                    view.key_zoom(ZoomStep::Out, window, cx)
                }),
            )
            .on_action(cx.listener(|view, _: &ZoomReset, window, cx| {
                view.key_zoom(ZoomStep::Reset, window, cx)
            }))
            .on_action(cx.listener(|view, _: &DrawTerminal, window, cx| {
                view.set_tool(Tool::DrawTerminal, window, cx)
            }))
            .on_action(cx.listener(|view, _: &DrawBrowser, window, cx| {
                view.set_tool(Tool::DrawBrowser, window, cx)
            }))
            .on_action(cx.listener(|view, _: &Cancel, window, cx| view.cancel(window, cx)))
            .on_action(
                cx.listener(|view, _: &ConfirmEnter, window, cx| view.confirm_enter(window, cx)),
            )
            .on_action(|_: &ToggleFullscreen, window, _| window.toggle_fullscreen())
            .on_action(cx.listener(|view, _: &ToggleFps, window, cx| {
                view.set_show_fps(!view.show_fps, window, cx)
            }))
            .on_action(cx.listener(|view, _: &ResetFps, window, cx| view.reset_fps(window, cx)))
            .map(|root| settings_menu::bind(root, cx))
            .child(toolbar(self))
            .child(
                div()
                    .flex_1()
                    .w_full()
                    .relative()
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
                    .on_mouse_down(MouseButton::Middle, cx.listener(Self::mouse_down))
                    .on_mouse_down(MouseButton::Right, cx.listener(Self::mouse_down))
                    .on_scroll_wheel(cx.listener(Self::scroll))
                    .child(
                        canvas(
                            move |bounds, _window, cx| {
                                let viewport = Rect::new(
                                    f64::from(f32::from(bounds.origin.x)),
                                    f64::from(f32::from(bounds.origin.y)),
                                    f64::from(f32::from(bounds.size.width)),
                                    f64::from(f32::from(bounds.size.height)),
                                );
                                entity.update(cx, |view, _| view.viewport = viewport).ok();
                            },
                            move |bounds, (), window, cx| {
                                snapshot.paint_canvas(bounds, paint_entity, window, cx);
                            },
                        )
                        .size_full(),
                    )
                    .children(terminal_targets),
            )
    }
}

fn to_mods(mods: &gpui::Modifiers) -> Modifiers {
    Modifiers {
        shift: mods.shift,
        control: mods.control,
        alt: mods.alt,
        platform: mods.platform,
    }
}

fn to_bounds(rect: Rect, origin: gpui::Point<f64>) -> Bounds<Pixels> {
    Bounds {
        origin: point(
            px((rect.origin.x + origin.x) as f32),
            px((rect.origin.y + origin.y) as f32),
        ),
        size: size(px(rect.size.width as f32), px(rect.size.height as f32)),
    }
}
