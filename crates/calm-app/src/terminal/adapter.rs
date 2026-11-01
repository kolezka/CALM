// Adapted from gpui-libghostty 0.3.1, src/adapter.rs (the `bind_gpui!` body).
// Copyright the gpui-libghostty authors, MIT license.
// CALM changes: Ghostty parents to a calm-native container; placement comes from the canvas.

use std::sync::Arc;
use std::time::Instant;

use calm_core::geometry::Point;
use calm_core::occlusion::region_contains;
use calm_core::preview::{PanelMode, PreviewImage, refresh_due};
use calm_core::viewport::{NativePlacement, unzoom_mouse_position};
use calm_native::NativeChild;
use gpui::{
    AppContext as _, ClipboardItem, Context, Entity, EventEmitter, FocusHandle,
    InteractiveElement as _, IntoElement, KeyDownEvent, KeyUpEvent, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Render, RenderImage, ScrollDelta, ScrollWheelEvent, Styled as _,
    Subscription, Task, Window, div,
};
use gpui_libghostty::__private::{
    KeyAction, Modifiers, MouseButton, MouseState, NativeSurface, TerminalThemeState,
};
use gpui_libghostty::{TerminalOptions, TerminalTheme};

pub const KEY_CONTEXT: &str = "Terminal";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalEvent {
    /// The process ended. Sent once per terminal.
    Exited,
    /// Enter was pressed after the process ended.
    RestartRequested,
}

/// A GPUI entity backed by Ghostty's Metal surface inside a CALM container view.
pub struct TerminalPanel {
    // Dropped before `host`: Ghostty's view is a child of the host's zoom view.
    surface: NativeSurface,
    theme: TerminalThemeState,
    focus: FocusHandle,
    placement: Option<NativePlacement>,
    tick_task: Option<Task<()>>,
    visible: bool,
    window_focused: bool,
    mode: PanelMode,
    /// Last snapshot, drawn by the canvas while in `Preview`.
    preview: PreviewImage<Arc<RenderImage>>,
    /// When the last snapshot was attempted, successful or not. Survives mode changes,
    /// so readbacks stay at most one per refresh period.
    snapshot_at: Option<Instant>,
    /// Snapshot attempts so far, for smoke.
    snapshot_count: u64,
    snapshot_error_logged: bool,
    exit_reported: bool,
    _subscriptions: Vec<Subscription>,
    host: NativeChild,
}

impl EventEmitter<TerminalEvent> for TerminalPanel {}

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
                theme,
                focus,
                placement: None,
                tick_task: None,
                visible: true,
                window_focused: false,
                mode: PanelMode::Live,
                preview: PreviewImage::None,
                snapshot_at: None,
                snapshot_count: 0,
                snapshot_error_logged: false,
                exit_reported: false,
                _subscriptions: subscriptions,
                host,
            };
            terminal.sync_focus(window);
            terminal
        }))
    }

    /// Recolors the running surface through libghostty's theme update. The process and
    /// the screen contents stay; whether the user config is read was fixed at spawn.
    pub fn apply_theme(&mut self, theme: &TerminalTheme) -> Result<(), String> {
        self.theme.apply(&mut self.surface, theme)
    }

    pub fn is_alive(&self) -> bool {
        self.surface.is_alive()
    }

    /// Frames Ghostty has drawn for this terminal.
    pub fn frame_count(&mut self) -> u64 {
        self.surface.frame_count()
    }

    pub fn mode(&self) -> PanelMode {
        self.mode
    }

    /// The image to draw while in `Preview`. `PreviewImage::None` means draw a flat rect.
    pub fn preview(&self) -> PreviewImage<Arc<RenderImage>> {
        self.preview.clone()
    }

    pub fn snapshot_count(&self) -> u64 {
        self.snapshot_count
    }

    /// Applies this paint's mode. `placement` is only used in `Live`.
    /// The process keeps running in every mode.
    pub fn apply_mode(
        &mut self,
        mode: PanelMode,
        placement: Option<NativePlacement>,
        window: &mut Window,
    ) {
        let entering = mode != self.mode;
        self.mode = mode;
        match mode {
            PanelMode::Live => {
                if entering {
                    self.surface.set_hidden_rendering(false);
                }
                self.set_placement(placement, window);
            }
            PanelMode::Preview => {
                if entering {
                    // Keep rendering once hidden, so later snapshots are current.
                    self.surface.set_hidden_rendering(true);
                }
                // Not forced on entry: flipping modes must not beat the refresh limit.
                // Until the next one is due, the last image (at most one period old) shows.
                if refresh_due(self.snapshot_at, Instant::now()) {
                    // Taken before hiding, so entering shows the last live frame.
                    self.take_snapshot(window);
                }
                self.set_placement(None, window);
            }
            PanelMode::Hidden => {
                if entering {
                    self.surface.set_hidden_rendering(false);
                }
                self.set_placement(None, window);
            }
        }
    }

    fn take_snapshot(&mut self, window: &mut Window) {
        self.snapshot_at = Some(Instant::now());
        self.snapshot_count += 1;
        match self.surface.snapshot_frame() {
            Ok(frame) => {
                let image = Arc::new(RenderImage::new([frame]));
                if let Some(old) = self.preview.refreshed(image) {
                    window.drop_image(old).ok();
                }
            }
            Err(err) => {
                // Not fatal: the canvas keeps the last image marked stale, or a flat rect.
                self.preview.refresh_failed();
                if !self.snapshot_error_logged {
                    eprintln!("calm: terminal snapshot failed: {err}");
                    self.snapshot_error_logged = true;
                }
            }
        }
    }

    /// Drops the snapshot and frees its texture in the window's atlas.
    pub fn release_preview(&mut self, window: &mut Window) {
        if let Some(image) = self.preview.take() {
            window.drop_image(image).ok();
        }
    }

    pub fn focus<T>(&mut self, window: &mut Window, cx: &mut Context<T>) {
        self.focus.focus(window, cx);
        self.sync_focus(window);
    }

    /// Where the canvas put the terminal this frame, in window points. `None` hides it.
    fn set_placement(&mut self, placement: Option<NativePlacement>, window: &Window) {
        self.placement = placement.clone();
        let Some(placement) = placement else {
            self.set_visible(false);
            return;
        };
        let scale = f64::from(window.scale_factor());
        let viewport_height = f64::from(f32::from(window.viewport_size().height));
        self.host.apply(&placement, viewport_height, scale);
        // Zoom never reaches here as a size change, so the grid only reflows on resize.
        // The surface dedupes unchanged frames and picks up display scale changes.
        let size = placement.logical_size;
        self.surface
            .set_frame(0.0, 0.0, size.width, size.height, scale);
        self.set_visible(true);
    }

    pub fn bring_to_front(&mut self) {
        self.host.bring_to_front();
    }

    pub fn stack_index(&self) -> Option<usize> {
        self.host.stack_index()
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
        self.check_exit(cx);
        let wakeup = self.surface.wakeup();
        let terminal = cx.entity().downgrade();
        self.tick_task = Some(cx.spawn(async move |_, cx| {
            loop {
                wakeup.wait().await;
                let updated = terminal.update(cx, |terminal, cx| {
                    // Ghostty draws its native view during the tick; GPUI repaints nothing.
                    terminal.surface.tick();
                    terminal.service_clipboard(cx);
                    terminal.check_exit(cx);
                });
                if updated.is_err() {
                    break;
                }
            }
        }));
    }

    // Ghostty wakes the app when the child exits, so the tick right after sees it.
    fn check_exit(&mut self, cx: &mut Context<Self>) {
        if !self.exit_reported && !self.surface.is_alive() {
            self.exit_reported = true;
            cx.emit(TerminalEvent::Exited);
        }
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
        // Only once the process is gone, so Enter never leaves a live program.
        if self.exit_reported && event.keystroke.key == "enter" && !event.is_held {
            cx.emit(TerminalEvent::RestartRequested);
            cx.stop_propagation();
            return;
        }
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
        let Some(placement) = &self.placement else {
            return;
        };
        let screen = Point::new(
            f64::from(f32::from(position.x)),
            f64::from(f32::from(position.y)),
        );
        let local = unzoom_mouse_position(screen, placement);
        self.surface
            .mouse_position(local.x, local.y, modifiers(input_modifiers));
    }

    /// Whether `position` is on a visible part of the terminal. A part under a panel in
    /// front belongs to that panel, so presses and scrolls there go on to the canvas.
    fn takes_pointer(&self, position: gpui::Point<Pixels>) -> bool {
        self.placement.as_ref().is_some_and(|placement| {
            region_contains(
                &placement.visible,
                Point::new(
                    f64::from(f32::from(position.x)),
                    f64::from(f32::from(position.y)),
                ),
            )
        })
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.takes_pointer(event.position) {
            return;
        }
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
        if !self.takes_pointer(event.position) {
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
