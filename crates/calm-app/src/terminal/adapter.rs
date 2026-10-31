// Adapted from gpui-libghostty 0.3.1, src/adapter.rs (the `bind_gpui!` body).
// Copyright the gpui-libghostty authors, MIT license.
// CALM changes: Ghostty parents to a calm-native container; placement comes from the canvas.

use std::sync::Arc;

use calm_core::geometry::{Point, Size};
use calm_core::viewport::{NativePlacement, unzoom_mouse_position};
use calm_native::NativeChild;
use gpui::{
    AppContext as _, ClipboardItem, Context, Entity, FocusHandle, InteractiveElement as _,
    IntoElement, KeyDownEvent, KeyUpEvent, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Render, RenderImage, ScrollDelta, ScrollWheelEvent, Styled as _, Subscription, Task, Window,
    div,
};
use gpui_libghostty::__private::{
    KeyAction, Modifiers, MouseButton, MouseState, NativeSurface, TerminalThemeState,
};
use gpui_libghostty::TerminalOptions;

pub const KEY_CONTEXT: &str = "Terminal";

/// A GPUI entity backed by Ghostty's Metal surface inside a CALM container view.
pub struct TerminalPanel {
    // Dropped before `host`: Ghostty's view is a child of the host's zoom view.
    surface: NativeSurface,
    _theme: TerminalThemeState,
    focus: FocusHandle,
    placement: Option<NativePlacement>,
    logical_size: Option<Size>,
    tick_task: Option<Task<()>>,
    visible: bool,
    window_focused: bool,
    _subscriptions: Vec<Subscription>,
    host: NativeChild,
}

impl TerminalPanel {
    /// Spawns the command with its native surface inside `host`'s zoom view.
    pub fn spawn<T: 'static>(
        options: TerminalOptions,
        host: NativeChild,
        window: &mut Window,
        cx: &mut Context<T>,
    ) -> Result<Entity<Self>, String> {
        let focus_on_spawn = options.focus_on_spawn;
        // SAFETY: `host` owns the parent view and is stored in the entity next to the
        // surface, which drops first. Both are created and dropped on the main thread.
        let (surface, theme) = unsafe {
            gpui_libghostty::__private::spawn_surface(
                options,
                &host,
                f64::from(window.scale_factor()),
            )?
        };
        let focus = cx.focus_handle();
        if focus_on_spawn {
            focus.focus(window, cx);
        }
        Ok(cx.new(|cx| {
            let subscriptions = vec![
                cx.on_focus(&focus, window, |terminal: &mut Self, window, _| {
                    terminal.sync_focus(window)
                }),
                cx.on_blur(&focus, window, |terminal: &mut Self, window, _| {
                    terminal.sync_focus(window)
                }),
                cx.observe_window_activation(window, |terminal: &mut Self, window, _| {
                    terminal.sync_focus(window)
                }),
            ];
            let mut terminal = Self {
                surface,
                _theme: theme,
                focus,
                placement: None,
                logical_size: None,
                tick_task: None,
                visible: true,
                window_focused: false,
                _subscriptions: subscriptions,
                host,
            };
            terminal.sync_focus(window);
            terminal
        }))
    }

    pub fn is_alive(&self) -> bool {
        self.surface.is_alive()
    }

    /// Frames Ghostty has drawn for this terminal.
    pub fn frame_count(&mut self) -> u64 {
        self.surface.frame_count()
    }

    /// Keeps the renderer running while hidden, so a snapshot stays current.
    #[allow(dead_code)] // Preview mode (Task 9).
    pub fn set_hidden_rendering(&mut self, rendered: bool) {
        self.surface.set_hidden_rendering(rendered);
    }

    /// Synchronous GPU readback of the last frame, at the surface's display scale.
    #[allow(dead_code)] // Preview mode (Task 9).
    pub fn snapshot(&mut self) -> Result<Arc<RenderImage>, String> {
        Ok(Arc::new(RenderImage::new([self
            .surface
            .snapshot_frame()?])))
    }

    pub fn focus<T>(&mut self, window: &mut Window, cx: &mut Context<T>) {
        self.focus.focus(window, cx);
        self.sync_focus(window);
    }

    /// Where the canvas put the terminal this frame, in window points. `None` hides it.
    pub fn set_placement(&mut self, placement: Option<NativePlacement>, window: &Window) {
        self.placement = placement;
        let Some(placement) = placement else {
            self.set_visible(false);
            return;
        };
        let scale = f64::from(window.scale_factor());
        let viewport_height = f64::from(f32::from(window.viewport_size().height));
        self.host.apply(&placement, viewport_height, scale);
        // Only a panel resize changes the grid; zoom only rescales the host.
        let size = placement.logical_size;
        if self.logical_size != Some(size) {
            self.logical_size = Some(size);
            self.surface
                .set_frame(0.0, 0.0, size.width, size.height, scale);
        }
        self.set_visible(true);
    }

    fn set_visible(&mut self, visible: bool) {
        if self.visible == visible {
            return;
        }
        self.visible = visible;
        self.host.set_visible(visible);
        self.surface.set_visible(visible);
        self.surface.set_focus(self.visible && self.window_focused);
    }

    fn sync_focus(&mut self, window: &Window) {
        self.window_focused = self.focus.is_focused(window) && window.is_window_active();
        self.surface.set_focus(self.visible && self.window_focused);
    }

    fn start_ticking(&mut self, cx: &mut Context<Self>) {
        if self.tick_task.is_some() {
            return;
        }
        self.surface.tick();
        self.service_clipboard(cx);
        let wakeup = self.surface.wakeup();
        let terminal = cx.entity().downgrade();
        self.tick_task = Some(cx.spawn(async move |_, cx| {
            loop {
                wakeup.wait().await;
                let updated = terminal.update(cx, |terminal, cx| {
                    // Ghostty draws its native view during the tick; GPUI repaints nothing.
                    terminal.surface.tick();
                    terminal.service_clipboard(cx);
                });
                if updated.is_err() {
                    break;
                }
            }
        }));
    }

    fn service_clipboard(&mut self, cx: &mut Context<Self>) {
        self.surface.service_clipboard_read(|_selection| {
            cx.read_from_clipboard()
                .and_then(|item| item.text())
                .unwrap_or_default()
        });
        while let Some(write) = self.surface.take_clipboard_write() {
            cx.write_to_clipboard(ClipboardItem::new_string(write.text));
        }
    }

    fn key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let action = if event.is_held {
            KeyAction::Repeat
        } else {
            KeyAction::Press
        };
        self.send_key(action, &event.keystroke);
        // The canvas must not see keys typed into a terminal (space would start a pan).
        cx.stop_propagation();
    }

    fn key_up(&mut self, event: &KeyUpEvent, cx: &mut Context<Self>) {
        self.send_key(KeyAction::Release, &event.keystroke);
        cx.stop_propagation();
    }

    fn send_key(&mut self, action: KeyAction, keystroke: &gpui::Keystroke) {
        self.surface.send_key(
            action,
            &keystroke.key,
            keystroke.key_char.as_deref(),
            modifiers(keystroke.modifiers),
        );
    }

    fn mouse_position(&mut self, position: gpui::Point<Pixels>, input_modifiers: gpui::Modifiers) {
        let Some(placement) = self.placement else {
            return;
        };
        let screen = Point::new(
            f64::from(f32::from(position.x)),
            f64::from(f32::from(position.y)),
        );
        let local = unzoom_mouse_position(screen, &placement);
        self.surface
            .mouse_position(local.x, local.y, modifiers(input_modifiers));
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        self.focus.focus(window, cx);
        self.sync_focus(window);
        self.mouse_position(event.position, event.modifiers);
        self.surface.mouse_button(
            MouseState::Press,
            mouse_button(event.button),
            modifiers(event.modifiers),
        );
    }

    fn mouse_up(&mut self, event: &MouseUpEvent) {
        self.mouse_position(event.position, event.modifiers);
        self.surface.mouse_button(
            MouseState::Release,
            mouse_button(event.button),
            modifiers(event.modifiers),
        );
    }

    fn scroll(&mut self, event: &ScrollWheelEvent, cx: &mut Context<Self>) {
        // Modified scrolls zoom the canvas, so they pass through.
        if event.modifiers.platform || event.modifiers.control {
            return;
        }
        self.mouse_position(event.position, event.modifiers);
        let (x, y, precision) = match event.delta {
            ScrollDelta::Pixels(delta) => (
                f64::from(f32::from(delta.x)),
                f64::from(f32::from(delta.y)),
                true,
            ),
            ScrollDelta::Lines(delta) => (f64::from(delta.x), f64::from(delta.y), false),
        };
        self.surface.mouse_scroll(x, y, precision);
        // Scrolling inside a terminal scrolls its history, not the canvas.
        cx.stop_propagation();
    }
}

impl Render for TerminalPanel {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.sync_focus(window);
        self.start_ticking(cx);
        let mut element = div()
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus)
            .size_full()
            .on_key_down(cx.listener(|terminal, event, _, cx| terminal.key_down(event, cx)))
            .on_key_up(cx.listener(|terminal, event, _, cx| terminal.key_up(event, cx)))
            .on_mouse_move(cx.listener(|terminal, event: &MouseMoveEvent, _, _| {
                terminal.mouse_position(event.position, event.modifiers);
            }));
        for button in [
            gpui::MouseButton::Left,
            gpui::MouseButton::Middle,
            gpui::MouseButton::Right,
        ] {
            element = element
                .on_mouse_down(
                    button,
                    cx.listener(|terminal, event, window, cx| {
                        terminal.mouse_down(event, window, cx)
                    }),
                )
                .on_mouse_up(
                    button,
                    cx.listener(|terminal, event, _, _| terminal.mouse_up(event)),
                );
        }
        element.on_scroll_wheel(cx.listener(|terminal, event, _, cx| terminal.scroll(event, cx)))
    }
}

fn modifiers(value: gpui::Modifiers) -> Modifiers {
    let mut result = Modifiers::empty();
    if value.shift {
        result.insert(Modifiers::SHIFT);
    }
    if value.control {
        result.insert(Modifiers::CONTROL);
    }
    if value.alt {
        result.insert(Modifiers::ALT);
    }
    if value.platform {
        result.insert(Modifiers::SUPER);
    }
    result
}

fn mouse_button(value: gpui::MouseButton) -> MouseButton {
    match value {
        gpui::MouseButton::Left => MouseButton::Left,
        gpui::MouseButton::Right => MouseButton::Right,
        gpui::MouseButton::Middle => MouseButton::Middle,
        _ => MouseButton::Unknown,
    }
}
