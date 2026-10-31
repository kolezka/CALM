//! The canvas: paints panels from the `calm-core` scene and feeds it input.

use calm_core::frame_stats::FrameStats;
use calm_core::geometry::{Point, Rect, Size};
use calm_core::interaction::{
    Button, CHROME_BUTTON_SIZE, ConfirmKey, Effect, InputHandler, Interaction, Modifiers, Tool,
    ZoomStep,
};
use calm_core::menu::{self, MenuCommand, MenuItem};
use calm_core::scene::{CHROME_HEIGHT, Panel, PanelId, PanelKind, Scene, chip_rect, content_rect};
use calm_core::viewport::{NativePlacement, place_native};
use calm_native::{HostView, NativeChild, show_context_menu};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, BorderStyle, Bounds, ContentMask, Context, DispatchPhase, Entity, FocusHandle, Focusable,
    InteractiveElement, IntoElement, KeyDownEvent, KeyUpEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, NoAction, ParentElement, PinchEvent, Pixels, Render, ScrollDelta,
    ScrollWheelEvent, SharedString, Styled, TextAlign, TextRun, WeakEntity, Window, actions,
    canvas, div, fill, outline, point, px, rgb, size,
};

use std::time::Instant;

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
const TITLE_FONT_SIZE: f64 = 13.0;
/// Below this the title is unreadable, so it is skipped.
const MIN_TITLE_FONT_PX: f64 = 4.0;
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
    /// Live terminals by panel. Dropping an entry ends its process.
    terminals: Vec<(PanelId, Entity<TerminalPanel>)>,
    /// Back-to-front terminal order the native views were last stacked in.
    native_order: Vec<PanelId>,
    /// Last pointer position relative to the viewport, for the draft rect.
    pointer: Point,
    focus_handle: FocusHandle,
    pub show_fps: bool,
    pub frame_stats: FrameStats,
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
            terminals: Vec::new(),
            native_order: Vec::new(),
            pointer: Point::new(0.0, 0.0),
            focus_handle: cx.focus_handle(),
            show_fps: false,
            frame_stats: FrameStats::default(),
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
        cx.spawn_in(window, async move |this, cx| {
            let Some(id) = show_context_menu(&items, at, &host) else {
                return;
            };
            this.update_in(cx, |view, window, cx| {
                view.apply_menu_choice(&id, target, window, cx)
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
        self.pointer = screen;
        let effects =
            self.input
                .mouse_down(screen, button, to_mods(&event.modifiers), &mut self.scene);
        self.apply_effects(effects, window, cx);
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        let screen = self.to_canvas(event.position);
        self.pointer = screen;
        let effects = self.input.mouse_move(screen, &mut self.scene);
        if !effects.is_empty() {
            self.apply_effects(effects, window, cx);
        }
    }

    fn mouse_up(&mut self, event: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(event.button, MouseButton::Left | MouseButton::Middle) {
            return;
        }
        let effects = self
            .input
            .mouse_up(self.to_canvas(event.position), &mut self.scene);
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
    pointer: Point,
    terminals: Vec<(PanelId, Entity<TerminalPanel>)>,
    settings: calm_core::settings::Settings,
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
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            let background = self.settings.background.rgb();
            window.paint_quad(fill(bounds, rgb(background)));
            let camera = self.scene.camera;
            if self.settings.show_grid {
                settings_menu::paint_grid(bounds, &camera, background, window);
            }
            // Panels paint inside this mask; nested masks intersect with it, so nothing
            // a panel draws can reach the toolbar.
            for panel in &self.scene.panels {
                self.paint_panel(panel, origin, window, cx);
            }
            let viewport = Rect::new(
                origin.x,
                origin.y,
                f64::from(f32::from(bounds.size.width)),
                f64::from(f32::from(bounds.size.height)),
            );
            for (id, terminal) in &self.terminals {
                let placement = self
                    .scene
                    .get(*id)
                    .and_then(|panel| terminal_placement(panel, &self.scene, viewport));
                terminal.update(cx, |terminal, _| terminal.set_placement(placement, window));
            }
            if let Interaction::Drawing { start_world } = self.state {
                let start = camera.world_to_screen(start_world);
                let draft = Rect::new(
                    start.x.min(self.pointer.x),
                    start.y.min(self.pointer.y),
                    (self.pointer.x - start.x).abs(),
                    (self.pointer.y - start.y).abs(),
                );
                window.paint_quad(outline(
                    to_bounds(draft, origin),
                    rgb(theme::DRAFT_OUTLINE),
                    BorderStyle::Dashed,
                ));
            }
        });

        this.update(cx, |view, cx| view.sync_native_order(cx)).ok();

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

    fn paint_panel(
        &self,
        panel: &Panel,
        origin: gpui::Point<f64>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let camera = self.scene.camera;
        let zoom = camera.zoom;
        let chrome_color = if self.focused == Some(panel.id) {
            theme::CHROME_FOCUSED
        } else {
            theme::CHROME
        };
        let chrome_world = if panel.minimized {
            chip_rect(panel)
        } else {
            Rect {
                origin: panel.rect.origin,
                size: calm_core::geometry::Size::new(panel.rect.size.width, CHROME_HEIGHT),
            }
        };
        let chrome = camera.world_rect_to_screen(chrome_world);
        window.paint_quad(fill(to_bounds(chrome, origin), rgb(chrome_color)));

        if !panel.minimized {
            let content = camera.world_rect_to_screen(content_rect(panel));
            window.paint_quad(fill(
                to_bounds(content, origin),
                rgb(theme::CONTENT_PLACEHOLDER),
            ));
            // Same squares as `chrome_hit`: Close rightmost, Minimize to its left.
            let margin = (CHROME_HEIGHT - CHROME_BUTTON_SIZE) / 2.0;
            let top = panel.rect.origin.y + margin;
            let close_left = panel.rect.right() - margin - CHROME_BUTTON_SIZE;
            let minimize_left = close_left - margin - CHROME_BUTTON_SIZE;
            for left in [close_left, minimize_left] {
                let square = Rect::new(left, top, CHROME_BUTTON_SIZE, CHROME_BUTTON_SIZE);
                window.paint_quad(fill(
                    to_bounds(camera.world_rect_to_screen(square), origin),
                    rgb(theme::CHROME_BUTTON),
                ));
            }
        }

        let font_px = TITLE_FONT_SIZE * zoom;
        if font_px < MIN_TITLE_FONT_PX {
            return;
        }
        // Leave room for the two buttons on the right of a full chrome strip.
        let reserved = if panel.minimized {
            0.0
        } else {
            2.0 * CHROME_HEIGHT * zoom
        };
        let padding = 8.0 * zoom;
        let text_rect = Rect::new(
            chrome.origin.x + padding,
            chrome.origin.y,
            (chrome.size.width - padding - reserved).max(0.0),
            chrome.size.height,
        );
        let title: SharedString = if self.confirming == Some(panel.id) {
            CONFIRM_TITLE.into()
        } else {
            panel.title.clone().into()
        };
        let run = TextRun {
            len: title.len(),
            font: window.text_style().font(),
            color: rgb(theme::TEXT).into(),
            background_color: None,
            underline: None,
            strikethrough: None,
        };
        let line = window
            .text_system()
            .shape_line(title, px(font_px as f32), &[run], None);
        let text_bounds = to_bounds(text_rect, origin);
        window.with_content_mask(
            Some(ContentMask {
                bounds: text_bounds,
            }),
            |window| {
                if let Err(err) = line.paint(
                    text_bounds.origin,
                    px(chrome.size.height as f32),
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                ) {
                    eprintln!("calm: title paint failed: {err:#}");
                }
            },
        );
    }
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
        let snapshot = PaintSnapshot {
            scene: self.scene.clone(),
            state: self.input.state,
            focused: self.focused,
            confirming: self.input.confirming,
            pointer: self.pointer,
            terminals: self.terminals.clone(),
            settings: self.settings.settings,
        };
        // Mouse and key targets for terminals, at last frame's placement.
        let viewport = self.viewport;
        let terminal_targets = self.terminals.iter().map(|(id, terminal)| {
            let placement = self
                .scene
                .get(*id)
                .and_then(|panel| terminal_placement(panel, &self.scene, viewport));
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
