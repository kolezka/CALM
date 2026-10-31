//! The canvas: paints panels from the `calm-core` scene and feeds it input.

use calm_core::geometry::{Point, Rect};
use calm_core::interaction::{
    Button, CHROME_BUTTON_SIZE, ConfirmKey, Effect, InputHandler, Interaction, Modifiers, Tool,
    ZoomStep,
};
use calm_core::scene::{CHROME_HEIGHT, Panel, PanelId, PanelKind, Scene, chip_rect, content_rect};
use gpui::{
    App, BorderStyle, Bounds, ContentMask, Context, DispatchPhase, FocusHandle, Focusable,
    InteractiveElement, IntoElement, KeyDownEvent, KeyUpEvent, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Render, ScrollDelta, ScrollWheelEvent,
    SharedString, Styled, TextAlign, TextRun, WeakEntity, Window, actions, canvas, div, fill,
    outline, point, px, rgb, size,
};

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
        ToggleFullscreen
    ]
);

pub const KEY_CONTEXT: &str = "Canvas";
const TITLE_FONT_SIZE: f64 = 13.0;
/// Below this the title is unreadable, so it is skipped.
const MIN_TITLE_FONT_PX: f64 = 4.0;

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
        KeyBinding::new("cmd-ctrl-f", ToggleFullscreen, ctx),
    ]
}

pub struct CanvasView {
    pub scene: Scene,
    pub input: InputHandler,
    /// Window pixels of the canvas area below the toolbar. Set during prepaint.
    pub viewport: Rect,
    /// (id, name) per discovered plugin. Task 11 fills it.
    pub plugins: Vec<(String, String)>,
    focused: Option<PanelId>,
    /// Last pointer position relative to the viewport, for the draft rect.
    pointer: Point,
    focus_handle: FocusHandle,
}

impl CanvasView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            scene: Scene::default(),
            input: InputHandler::default(),
            viewport: Rect::new(0.0, f64::from(TOOLBAR_HEIGHT), 0.0, 0.0),
            plugins: Vec::new(),
            focused: None,
            pointer: Point::new(0.0, 0.0),
            focus_handle: cx.focus_handle(),
        }
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
                        self.panel_created(id, &kind);
                    }
                    self.focused = Some(id);
                }
                Effect::ClosePanelRequested(id) => {
                    // Placeholders run no process yet, so nothing needs confirming.
                    queue.push_back(self.input.request_close(id, false));
                }
                Effect::ToggleMinimize(id) => {
                    if let Some(panel) = self.scene.get_mut(id) {
                        panel.minimized = !panel.minimized;
                    }
                }
                Effect::ConfirmClose(_) | Effect::Redraw => {}
                Effect::ClosePanel(id) => {
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

    /// Hook for native view creation (Task 8). Placeholders need nothing.
    fn panel_created(&mut self, _id: PanelId, _kind: &PanelKind) {}

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
            _ => return,
        };
        self.focus_handle.focus(window, cx);
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

/// What paint needs, copied out at render time so painting does not borrow the view.
struct PaintSnapshot {
    scene: Scene,
    state: Interaction,
    focused: Option<PanelId>,
    pointer: Point,
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
            window.paint_quad(fill(bounds, rgb(theme::BACKGROUND)));
            let camera = self.scene.camera;
            for panel in &self.scene.panels {
                self.paint_panel(panel, origin, window, cx);
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
        let title: SharedString = panel.title.clone().into();
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
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let entity = cx.entity().downgrade();
        let paint_entity = entity.clone();
        let snapshot = PaintSnapshot {
            scene: self.scene.clone(),
            state: self.input.state,
            focused: self.focused,
            pointer: self.pointer,
        };
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(theme::BACKGROUND))
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::key_down))
            .on_key_up(cx.listener(Self::key_up))
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
            .on_action(|_: &ToggleFullscreen, window, _| window.toggle_fullscreen())
            .child(toolbar(self, cx))
            .child(
                div()
                    .flex_1()
                    .w_full()
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
                    .on_mouse_down(MouseButton::Middle, cx.listener(Self::mouse_down))
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
                    ),
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
