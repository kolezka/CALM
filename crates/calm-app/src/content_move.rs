//! Move-modifier gestures on a native panel's content: a drag moves the panel, a click
//! still reaches the page or terminal, a right click opens the panel menu. Middle drag,
//! Space+drag (while the canvas has the keyboard) and, with `canvas.scroll_over_panels`
//! `pan`, scrolling pan the canvas over a panel too. This is the app policy behind
//! `calm_native::ContentGestureMonitor`.

use calm_core::geometry::Point;
use calm_core::gesture::{ContentGesture, GestureStep, content_gesture_target};
use calm_core::interaction::{Button, Effect, InputHandler, Modifiers};
use calm_core::scene::{PanelId, Scene};
use calm_core::settings::{ScrollOverPanels, Settings};
use calm_native::{
    ContentButton, ContentDecision, ContentEvent, ContentGestureMonitor, GestureMods,
};
use gpui::{Context, Window};

use crate::canvas_view::CanvasView;

pub fn to_modifiers(mods: GestureMods) -> Modifiers {
    Modifiers {
        shift: mods.shift,
        control: mods.control,
        alt: mods.alt,
        platform: mods.command,
    }
}

/// A window point (top-left origin) in canvas coordinates, as GPUI mouse events are.
pub fn to_canvas(window_point: Point, canvas_origin: Point) -> Point {
    Point::new(
        window_point.x - canvas_origin.x,
        window_point.y - canvas_origin.y,
    )
}

/// The monitor takes modifier gestures over content or resize handles.
pub fn wants(
    settings: &Settings,
    input: &InputHandler,
    scene: &Scene,
    screen: Point,
    mods: Modifiers,
) -> bool {
    settings.canvas.move_modifier.matches(mods)
        && (input.resize_target_at(screen, scene).is_some()
            || content_gesture_target(scene, screen).is_some())
}

/// A left press over a panel pans the canvas when Space is held and the canvas, not the
/// panel, has the keyboard (else the Space went to the panel).
pub fn space_pans(input: &InputHandler, canvas_has_keyboard: bool) -> bool {
    input.space_held && canvas_has_keyboard
}

/// The monitor takes scrolls over a panel only when they pan the canvas.
pub fn wants_scroll(settings: &Settings) -> bool {
    settings.canvas.scroll_over_panels == ScrollOverPanels::Pan
}

/// Scroll deltas in screen points, as `CanvasView::scroll` reads GPUI's: line deltas
/// count 20 points a line.
pub fn scroll_points(dx: f64, dy: f64, precise: bool) -> (f64, f64) {
    if precise {
        (dx, dy)
    } else {
        (dx * 20.0, dy * 20.0)
    }
}

fn event_modifiers(
    event: ContentEvent,
    press_mods: &mut Option<Modifiers>,
    held: Modifiers,
) -> Modifiers {
    if matches!(event, ContentEvent::Down(_)) {
        press_mods.take().unwrap_or(held)
    } else {
        held
    }
}

/// The gesture in progress, from a taken press to its release.
#[derive(Debug, Default)]
pub struct ContentMove {
    gesture: Option<(PanelId, ContentGesture)>,
    resizing: bool,
    /// The next left press is a Space pan (set by `wants` for that press).
    space_pan_armed: bool,
    /// A Space pan is in progress.
    panning: bool,
}

impl ContentMove {
    /// Answers one reported event (already in canvas coordinates) and returns the effects
    /// to apply. `mods` are the keys held now, for snapping while moving.
    pub fn event(
        &mut self,
        event: ContentEvent,
        mods: Modifiers,
        input: &mut InputHandler,
        scene: &mut Scene,
    ) -> (ContentDecision, Vec<Effect>) {
        match event {
            ContentEvent::Down(screen) if std::mem::take(&mut self.space_pan_armed) => {
                // Never a click for the panel: the core pans while Space is held.
                self.resizing = false;
                self.gesture = None;
                self.panning = true;
                (
                    ContentDecision::Moving,
                    input.mouse_down(screen, Button::Left, mods, scene),
                )
            }
            ContentEvent::Down(screen) => {
                self.resizing = false;
                self.panning = false;
                if let Some(effects) = input.begin_resize_at(screen, scene) {
                    self.gesture = None;
                    self.resizing = true;
                    return (ContentDecision::Moving, effects);
                }
                match content_gesture_target(scene, screen) {
                    Some(id) => {
                        self.gesture = Some((id, ContentGesture::press(screen)));
                        (ContentDecision::Hold, Vec::new())
                    }
                    None => {
                        self.gesture = None;
                        (ContentDecision::Click, Vec::new())
                    }
                }
            }
            ContentEvent::Dragged(screen) if self.resizing || self.panning => (
                ContentDecision::Moving,
                input.mouse_move(screen, mods, scene),
            ),
            ContentEvent::Dragged(screen) => {
                let Some((id, gesture)) = self.gesture.as_mut() else {
                    return (ContentDecision::Click, Vec::new());
                };
                match gesture.moved(screen) {
                    GestureStep::StartMove { from } => {
                        let mut effects = input.begin_content_move(*id, from, scene);
                        effects.extend(input.mouse_move(screen, mods, scene));
                        (ContentDecision::Moving, effects)
                    }
                    GestureStep::Moving => (
                        ContentDecision::Moving,
                        input.mouse_move(screen, mods, scene),
                    ),
                    GestureStep::Undecided | GestureStep::Click => {
                        (ContentDecision::Hold, Vec::new())
                    }
                }
            }
            // Exactly what `release` says: Hold here would drop the click.
            ContentEvent::Up(screen) if self.resizing || self.panning => {
                self.resizing = false;
                self.panning = false;
                (
                    ContentDecision::Moving,
                    input.mouse_up_for(Button::Left, screen, mods, scene),
                )
            }
            ContentEvent::Up(screen) => match self.gesture.take() {
                Some((_, gesture)) if gesture.release() == GestureStep::Moving => (
                    ContentDecision::Moving,
                    input.mouse_up_for(Button::Left, screen, mods, scene),
                ),
                _ => (ContentDecision::Click, Vec::new()),
            },
            // The monitor ignores the answer to a right press.
            ContentEvent::RightDown(screen) => match content_gesture_target(scene, screen) {
                Some(id) => (ContentDecision::Moving, input.content_menu(id, screen)),
                None => (ContentDecision::Click, Vec::new()),
            },
            // One pointer gesture at a time: a left move, resize or Space pan keeps going.
            // The monitor still swallows these, so the native view sees no stray middle click.
            ContentEvent::MiddleDown(_)
            | ContentEvent::MiddleDragged(_)
            | ContentEvent::MiddleUp(_)
                if self.gesture.is_some() || self.resizing || self.panning =>
            {
                (ContentDecision::Moving, Vec::new())
            }
            // The monitor takes a middle gesture whole; the core pans on a middle press.
            ContentEvent::MiddleDown(screen) => (
                ContentDecision::Moving,
                input.mouse_down(screen, Button::Middle, mods, scene),
            ),
            ContentEvent::MiddleDragged(screen) => (
                ContentDecision::Moving,
                input.mouse_move(screen, mods, scene),
            ),
            // Only the owner's release ends a gesture: a canvas draw outlives a middle click.
            ContentEvent::MiddleUp(screen) => (
                ContentDecision::Moving,
                input.mouse_up_for(Button::Middle, screen, mods, scene),
            ),
            ContentEvent::Scroll {
                point,
                dx,
                dy,
                precise,
                mods: scroll_mods,
            } => {
                // The keys at the scroll, not now: Cmd may have changed since.
                let (dx, dy) = scroll_points(dx, dy, precise);
                input.scroll_anywhere(point, dx, dy, to_modifiers(scroll_mods), scene);
                (ContentDecision::Moving, vec![Effect::Redraw])
            }
        }
    }

    /// Makes the next left press a Space pan, or not.
    pub fn arm_space_pan(&mut self, armed: bool) {
        self.space_pan_armed = armed;
    }
}

/// `event` with its point moved into canvas coordinates.
fn in_canvas(event: ContentEvent, canvas_origin: Point) -> ContentEvent {
    match event {
        ContentEvent::Down(point) => ContentEvent::Down(to_canvas(point, canvas_origin)),
        ContentEvent::Dragged(point) => ContentEvent::Dragged(to_canvas(point, canvas_origin)),
        ContentEvent::Up(point) => ContentEvent::Up(to_canvas(point, canvas_origin)),
        ContentEvent::RightDown(point) => ContentEvent::RightDown(to_canvas(point, canvas_origin)),
        ContentEvent::MiddleDown(point) => {
            ContentEvent::MiddleDown(to_canvas(point, canvas_origin))
        }
        ContentEvent::MiddleDragged(point) => {
            ContentEvent::MiddleDragged(to_canvas(point, canvas_origin))
        }
        ContentEvent::MiddleUp(point) => ContentEvent::MiddleUp(to_canvas(point, canvas_origin)),
        ContentEvent::Scroll {
            point,
            dx,
            dy,
            precise,
            mods,
        } => ContentEvent::Scroll {
            point: to_canvas(point, canvas_origin),
            dx,
            dy,
            precise,
            mods,
        },
    }
}

impl CanvasView {
    /// Installs the content gesture monitor on the canvas window once.
    pub(crate) fn ensure_content_monitor(&mut self, window: &Window, cx: &mut Context<Self>) {
        if self.content_monitor.is_some() || self.content_monitor_error.is_some() {
            return;
        }
        let view = cx.entity().downgrade();
        let async_cx = window.to_async(cx);
        let wants_view = view.clone();
        let wants_cx = async_cx.clone();
        let scroll_view = view.clone();
        let scroll_cx = async_cx.clone();
        let installed = ContentGestureMonitor::install_for(
            window,
            move |button, point, mods| {
                let mut cx = wants_cx.clone();
                // Fails while GPUI is mid-update (a modal loop, say): leave the press alone.
                wants_view
                    .update_in(&mut cx, |view, window, _| {
                        view.wants_content_gesture(button, point, mods, window)
                    })
                    .unwrap_or(false)
            },
            // Asked per scroll, so a settings change applies at once.
            move |_| {
                let mut cx = scroll_cx.clone();
                scroll_view
                    .update_in(&mut cx, |view, _, _| wants_scroll(&view.settings.settings))
                    .unwrap_or(false)
            },
            move |event| {
                let mut cx = async_cx.clone();
                // If the view cannot be reached, the native view gets the events back.
                view.update_in(&mut cx, |view, window, cx| {
                    view.content_event(event, window, cx)
                })
                .unwrap_or(ContentDecision::Click)
            },
        );
        match installed {
            Ok(monitor) => self.content_monitor = Some(monitor),
            Err(err) => {
                eprintln!("calm: content gestures unavailable: {err}");
                self.content_monitor_error = Some(err.to_string());
            }
        }
    }

    fn wants_content_gesture(
        &mut self,
        button: ContentButton,
        point: Point,
        mods: GestureMods,
        window: &Window,
    ) -> bool {
        let screen = to_canvas(point, self.viewport.origin);
        let mods = to_modifiers(mods);
        // Wants and Down come from the same NSEvent in one call.
        self.content_press_mods = Some(mods);
        self.content_move.arm_space_pan(false);
        // The move modifier wins over Space, as in `InputHandler::mouse_down`.
        if wants(
            &self.settings.settings,
            &self.input,
            &self.scene,
            screen,
            mods,
        ) {
            return true;
        }
        // Both are needed: a focused terminal takes GPUI focus but leaves the GPUI host view
        // as first responder, while a focused page takes first responder.
        let canvas_has_keyboard = self.focus_handle.is_focused(window)
            && calm_native::host_view_is_first_responder(window) == Some(true);
        if button == ContentButton::Left && space_pans(&self.input, canvas_has_keyboard) {
            self.content_move.arm_space_pan(true);
            return true;
        }
        false
    }

    fn content_event(
        &mut self,
        event: ContentEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> ContentDecision {
        let event = in_canvas(event, self.viewport.origin);
        let held = to_modifiers(calm_native::held_mods());
        let mods = event_modifiers(event, &mut self.content_press_mods, held);
        let (decision, effects) =
            self.content_move
                .event(event, mods, &mut self.input, &mut self.scene);
        if !effects.is_empty() {
            self.apply_effects(effects, window, cx);
        }
        decision
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use calm_core::geometry::Rect;
    use calm_core::interaction::MoveModifier;
    use calm_core::scene::{PanelKind, content_rect};

    const CMD: Modifiers = Modifiers {
        shift: false,
        control: false,
        alt: false,
        platform: true,
    };
    /// Cmd and Shift keep snapping out of the numbers.
    const CMD_SHIFT: Modifiers = Modifiers {
        shift: true,
        control: false,
        alt: false,
        platform: true,
    };

    fn scene_with_terminal() -> (Scene, PanelId) {
        let mut scene = Scene::default();
        let id = scene.add(
            PanelKind::Terminal {
                command: None,
                cwd: None,
                profile: None,
                app: None,
            },
            Rect::new(100.0, 100.0, 400.0, 300.0),
        );
        (scene, id)
    }

    fn content_point(scene: &Scene, id: PanelId) -> Point {
        let content = content_rect(scene.get(id).unwrap());
        let world = Point::new(
            content.origin.x + content.size.width / 2.0,
            content.origin.y + content.size.height / 2.0,
        );
        scene.camera.world_to_screen(world)
    }

    #[test]
    fn down_uses_press_modifiers_and_drag_uses_held_modifiers() {
        let pressed = Modifiers { shift: true, ..CMD };
        let mut stored = Some(pressed);
        assert_eq!(
            event_modifiers(ContentEvent::Down(Point::new(0.0, 0.0)), &mut stored, CMD),
            pressed
        );
        assert_eq!(stored, None);
        assert_eq!(
            event_modifiers(
                ContentEvent::Dragged(Point::new(1.0, 0.0)),
                &mut stored,
                CMD
            ),
            CMD
        );
    }

    #[test]
    fn window_points_shift_by_the_canvas_origin() {
        assert_eq!(
            to_canvas(Point::new(300.0, 340.0), Point::new(0.0, 40.0)),
            Point::new(300.0, 300.0)
        );
    }

    #[test]
    fn command_maps_to_platform() {
        let mods = to_modifiers(GestureMods {
            command: true,
            ..GestureMods::default()
        });
        assert_eq!(mods, CMD);
    }

    #[test]
    fn wants_the_move_modifier_over_content_only() {
        let (scene, id) = scene_with_terminal();
        let mut settings = Settings::default();
        let on_content = content_point(&scene, id);
        assert!(wants(
            &settings,
            &InputHandler::default(),
            &scene,
            on_content,
            CMD
        ));
        assert!(!wants(
            &settings,
            &InputHandler::default(),
            &scene,
            on_content,
            Modifiers::default()
        ));
        assert!(wants(
            &settings,
            &InputHandler::default(),
            &scene,
            on_content,
            CMD_SHIFT
        ));
        // The title bar is chrome, not content.
        let panel = scene.get(id).unwrap();
        let chrome = scene.camera.world_to_screen(Point::new(
            panel.rect.origin.x + 50.0,
            panel.rect.origin.y + 2.0,
        ));
        assert!(!wants(
            &settings,
            &InputHandler::default(),
            &scene,
            chrome,
            CMD
        ));
        assert!(!wants(
            &settings,
            &InputHandler::default(),
            &scene,
            Point::new(5.0, 5.0),
            CMD
        ));
        settings.canvas.move_modifier = MoveModifier::Ctrl;
        assert!(!wants(
            &settings,
            &InputHandler::default(),
            &scene,
            on_content,
            CMD_SHIFT
        ));
        assert!(!wants(
            &settings,
            &InputHandler::default(),
            &scene,
            on_content,
            CMD
        ));
        settings.canvas.move_modifier = MoveModifier::Cmd;
        assert!(wants(
            &settings,
            &InputHandler::default(),
            &scene,
            on_content,
            CMD_SHIFT
        ));
        assert!(wants(
            &settings,
            &InputHandler::default(),
            &scene,
            Point::new(499.0, 250.0),
            CMD_SHIFT
        ));
    }

    #[test]
    fn cmd_shift_inner_band_starts_unsnapped_resize() {
        let (mut scene, id) = scene_with_terminal();
        let mut input = InputHandler::default();
        let mut gesture = ContentMove::default();
        let at = Point::new(499.0, 250.0);
        let mods = Modifiers { shift: true, ..CMD };
        let settings = Settings::default();
        assert!(wants(&settings, &InputHandler::default(), &scene, at, mods));
        let (down, _) = gesture.event(ContentEvent::Down(at), mods, &mut input, &mut scene);
        assert_eq!(down, ContentDecision::Moving);
        let to = Point::new(512.0, 250.0);
        let (drag, _) = gesture.event(ContentEvent::Dragged(to), mods, &mut input, &mut scene);
        assert_eq!(drag, ContentDecision::Moving);
        assert_eq!(scene.get(id).unwrap().rect.size.width, 413.0);
        let (up, _) = gesture.event(ContentEvent::Up(to), mods, &mut input, &mut scene);
        assert_eq!(up, ContentDecision::Moving);
    }

    #[test]
    fn cmd_shift_middle_moves_without_snapping() {
        let (mut scene, id) = scene_with_terminal();
        let mut input = InputHandler::default();
        let mut gesture = ContentMove::default();
        let at = content_point(&scene, id);
        let mods = Modifiers { shift: true, ..CMD };
        assert_eq!(
            gesture
                .event(ContentEvent::Down(at), mods, &mut input, &mut scene)
                .0,
            ContentDecision::Hold
        );
        let to = Point::new(at.x + 13.0, at.y + 5.0);
        assert_eq!(
            gesture
                .event(ContentEvent::Dragged(to), mods, &mut input, &mut scene)
                .0,
            ContentDecision::Moving
        );
        assert_eq!(scene.get(id).unwrap().rect.origin, Point::new(113.0, 105.0));
    }

    #[test]
    fn unmodified_inner_band_is_not_captured() {
        let (scene, _) = scene_with_terminal();
        assert!(!wants(
            &Settings::default(),
            &InputHandler::default(),
            &scene,
            Point::new(499.0, 250.0),
            Modifiers::default()
        ));
    }

    #[test]
    fn cmd_inner_band_resizes_with_snapping() {
        let (mut scene, id) = scene_with_terminal();
        let mut input = InputHandler::default();
        let mut gesture = ContentMove::default();
        let at = Point::new(499.0, 250.0);
        assert!(wants(&Settings::default(), &input, &scene, at, CMD));
        assert_eq!(
            gesture
                .event(ContentEvent::Down(at), CMD, &mut input, &mut scene)
                .0,
            ContentDecision::Moving
        );
        let to = Point::new(512.0, 250.0);
        assert_eq!(
            gesture
                .event(ContentEvent::Dragged(to), CMD, &mut input, &mut scene)
                .0,
            ContentDecision::Moving
        );
        assert_eq!(scene.get(id).unwrap().rect.size.width, 412.0);
        assert_eq!(
            gesture
                .event(ContentEvent::Up(to), CMD, &mut input, &mut scene)
                .0,
            ContentDecision::Moving
        );
    }

    #[test]
    fn cmd_middle_content_moves_with_snapping() {
        let (mut scene, id) = scene_with_terminal();
        let mut input = InputHandler::default();
        let mut gesture = ContentMove::default();
        let at = content_point(&scene, id);
        assert_eq!(
            gesture
                .event(ContentEvent::Down(at), CMD, &mut input, &mut scene)
                .0,
            ContentDecision::Hold
        );
        let to = Point::new(at.x + 13.0, at.y + 5.0);
        assert_eq!(
            gesture
                .event(ContentEvent::Dragged(to), CMD, &mut input, &mut scene)
                .0,
            ContentDecision::Moving
        );
        assert_eq!(scene.get(id).unwrap().rect.origin, Point::new(112.0, 104.0));
    }

    #[test]
    fn a_press_released_in_place_is_a_click() {
        let (mut scene, id) = scene_with_terminal();
        let mut input = InputHandler::default();
        let mut gesture = ContentMove::default();
        let at = content_point(&scene, id);
        let before = scene.get(id).unwrap().rect;
        let (down, _) = gesture.event(ContentEvent::Down(at), CMD, &mut input, &mut scene);
        assert_eq!(down, ContentDecision::Hold);
        let nudge = Point::new(at.x + 1.0, at.y + 1.0);
        let (dragged, _) = gesture.event(ContentEvent::Dragged(nudge), CMD, &mut input, &mut scene);
        assert_eq!(dragged, ContentDecision::Hold);
        let (up, effects) = gesture.event(ContentEvent::Up(nudge), CMD, &mut input, &mut scene);
        assert_eq!(up, ContentDecision::Click, "an up is never Hold");
        assert!(effects.is_empty(), "{effects:?}");
        assert_eq!(scene.get(id).unwrap().rect, before);
    }

    #[test]
    fn a_drag_past_the_slop_moves_the_panel() {
        let (mut scene, id) = scene_with_terminal();
        let mut input = InputHandler::default();
        let mut gesture = ContentMove::default();
        let at = content_point(&scene, id);
        let before = scene.get(id).unwrap().rect;
        gesture.event(ContentEvent::Down(at), CMD_SHIFT, &mut input, &mut scene);
        let far = Point::new(at.x + 60.0, at.y + 20.0);
        let (decision, effects) = gesture.event(
            ContentEvent::Dragged(far),
            CMD_SHIFT,
            &mut input,
            &mut scene,
        );
        assert_eq!(decision, ContentDecision::Moving);
        assert!(effects.contains(&Effect::FocusPanel(id)), "{effects:?}");
        let farther = Point::new(at.x + 80.0, at.y + 30.0);
        let (decision, _) = gesture.event(
            ContentEvent::Dragged(farther),
            CMD_SHIFT,
            &mut input,
            &mut scene,
        );
        assert_eq!(decision, ContentDecision::Moving);
        let (up, _) = gesture.event(ContentEvent::Up(farther), CMD_SHIFT, &mut input, &mut scene);
        assert_eq!(up, ContentDecision::Moving);
        let after = scene.get(id).unwrap().rect;
        assert_eq!(
            after.origin,
            Point::new(before.origin.x + 80.0, before.origin.y + 30.0)
        );
        assert_eq!(after.size, before.size);
        // The handler is idle again: a later plain move changes nothing.
        let (decision, _) = gesture.event(
            ContentEvent::Dragged(Point::new(at.x + 200.0, at.y)),
            CMD_SHIFT,
            &mut input,
            &mut scene,
        );
        assert_eq!(decision, ContentDecision::Click);
        assert_eq!(scene.get(id).unwrap().rect, after);
    }

    #[test]
    fn a_right_press_on_content_opens_the_panel_menu() {
        let (mut scene, id) = scene_with_terminal();
        let mut input = InputHandler::default();
        let mut gesture = ContentMove::default();
        let at = content_point(&scene, id);
        let (_, effects) = gesture.event(ContentEvent::RightDown(at), CMD, &mut input, &mut scene);
        assert_eq!(effects, vec![Effect::ShowPanelMenu { id, screen: at }]);
    }

    #[test]
    fn space_pans_only_while_the_canvas_has_the_keyboard() {
        let mut input = InputHandler::default();
        assert!(!space_pans(&input, true));
        input.space_held = true;
        assert!(space_pans(&input, true));
        assert!(!space_pans(&input, false));
    }

    #[test]
    fn scroll_is_wanted_only_when_set_to_pan() {
        let mut settings = Settings::default();
        assert!(!wants_scroll(&settings));
        settings.canvas.scroll_over_panels = ScrollOverPanels::Pan;
        assert!(wants_scroll(&settings));
    }

    #[test]
    fn line_scrolls_count_twenty_points_a_line() {
        assert_eq!(scroll_points(1.5, -2.0, true), (1.5, -2.0));
        assert_eq!(scroll_points(1.0, -2.0, false), (20.0, -40.0));
    }

    #[test]
    fn a_space_press_on_content_pans_and_never_moves_the_panel() {
        let (mut scene, id) = scene_with_terminal();
        let mut input = InputHandler {
            space_held: true,
            ..InputHandler::default()
        };
        let mut gesture = ContentMove::default();
        let at = content_point(&scene, id);
        let before = scene.get(id).unwrap().rect;
        gesture.arm_space_pan(true);
        let none = Modifiers::default();
        let (down, _) = gesture.event(ContentEvent::Down(at), none, &mut input, &mut scene);
        assert_eq!(
            down,
            ContentDecision::Moving,
            "a Space press is never a click"
        );
        let to = Point::new(at.x + 30.0, at.y + 10.0);
        let (drag, _) = gesture.event(ContentEvent::Dragged(to), none, &mut input, &mut scene);
        assert_eq!(drag, ContentDecision::Moving);
        let (up, _) = gesture.event(ContentEvent::Up(to), none, &mut input, &mut scene);
        assert_eq!(up, ContentDecision::Moving);
        assert_eq!(scene.camera.offset, Point::new(-30.0, -10.0));
        assert_eq!(scene.get(id).unwrap().rect, before);
        // Armed for one press only: the next plain press is a click again.
        let (down, _) = gesture.event(ContentEvent::Down(at), CMD, &mut input, &mut scene);
        assert_eq!(down, ContentDecision::Hold);
    }

    #[test]
    fn a_middle_drag_on_content_pans_the_canvas() {
        let (mut scene, id) = scene_with_terminal();
        let mut input = InputHandler::default();
        let mut gesture = ContentMove::default();
        let at = content_point(&scene, id);
        let before = scene.get(id).unwrap().rect;
        let none = Modifiers::default();
        let to = Point::new(at.x + 25.0, at.y - 15.0);
        for event in [
            ContentEvent::MiddleDown(at),
            ContentEvent::MiddleDragged(to),
            ContentEvent::MiddleUp(to),
        ] {
            let (decision, _) = gesture.event(event, none, &mut input, &mut scene);
            assert_eq!(decision, ContentDecision::Moving, "{event:?}");
        }
        assert_eq!(scene.camera.offset, Point::new(-25.0, 15.0));
        assert_eq!(scene.get(id).unwrap().rect, before);
        // The pan is over: a later move changes nothing.
        let (_, effects) = gesture.event(
            ContentEvent::MiddleDragged(Point::new(at.x + 90.0, at.y)),
            none,
            &mut input,
            &mut scene,
        );
        assert!(effects.is_empty(), "{effects:?}");
        assert_eq!(scene.camera.offset, Point::new(-25.0, 15.0));
    }

    #[test]
    fn a_scroll_on_content_pans_and_cmd_scroll_zooms() {
        let (mut scene, id) = scene_with_terminal();
        let mut input = InputHandler::default();
        let mut gesture = ContentMove::default();
        let at = content_point(&scene, id);
        let scroll = |dx, dy, precise, command| ContentEvent::Scroll {
            point: at,
            dx,
            dy,
            precise,
            mods: GestureMods {
                command,
                ..GestureMods::default()
            },
        };
        let none = Modifiers::default();
        let (decision, effects) =
            gesture.event(scroll(0.0, 1.0, false, false), none, &mut input, &mut scene);
        assert_eq!(decision, ContentDecision::Moving);
        assert_eq!(effects, vec![Effect::Redraw]);
        let panned = scene.camera.offset;
        assert_ne!(panned, Point::new(0.0, 0.0));
        gesture.event(scroll(0.0, -300.0, true, true), CMD, &mut input, &mut scene);
        assert!(
            (scene.camera.zoom - 2.0).abs() < 1e-9,
            "{}",
            scene.camera.zoom
        );
    }

    #[test]
    fn a_scroll_reads_its_own_modifiers_not_the_keys_held_later() {
        let (mut scene, id) = scene_with_terminal();
        let mut input = InputHandler::default();
        let mut gesture = ContentMove::default();
        let at = content_point(&scene, id);
        let scroll = |command| ContentEvent::Scroll {
            point: at,
            dx: 0.0,
            dy: -300.0,
            precise: true,
            mods: GestureMods {
                command,
                ..GestureMods::default()
            },
        };
        // Cmd was down at the scroll and is up by the time it is handled: a zoom.
        let none = Modifiers::default();
        gesture.event(scroll(true), none, &mut input, &mut scene);
        assert!(
            (scene.camera.zoom - 2.0).abs() < 1e-9,
            "{}",
            scene.camera.zoom
        );
        let zoomed = scene.camera.offset;
        // Cmd was up at the scroll and is down now: a pan, the zoom stays.
        gesture.event(scroll(false), CMD, &mut input, &mut scene);
        assert!(
            (scene.camera.zoom - 2.0).abs() < 1e-9,
            "{}",
            scene.camera.zoom
        );
        assert_ne!(scene.camera.offset, zoomed);
    }

    #[test]
    fn a_middle_click_over_content_leaves_a_canvas_draw_alone() {
        use calm_core::interaction::{Interaction, Tool};
        let (mut scene, id) = scene_with_terminal();
        let mut input = InputHandler {
            tool: Tool::DrawTerminal,
            ..InputHandler::default()
        };
        let none = Modifiers::default();
        let start = Point::new(700.0, 600.0);
        let end = Point::new(900.0, 800.0);
        input.mouse_down(start, Button::Left, none, &mut scene);
        input.mouse_move(end, CMD_SHIFT, &mut scene);
        assert!(matches!(input.state, Interaction::Drawing { .. }));
        let mut gesture = ContentMove::default();
        let at = content_point(&scene, id);
        for event in [
            ContentEvent::MiddleDown(at),
            ContentEvent::MiddleDragged(at),
            ContentEvent::MiddleUp(at),
        ] {
            let (_, effects) = gesture.event(event, none, &mut input, &mut scene);
            assert!(
                !effects
                    .iter()
                    .any(|effect| matches!(effect, Effect::CreatePanel { .. })),
                "{event:?} committed the draw: {effects:?}"
            );
        }
        assert!(
            matches!(input.state, Interaction::Drawing { .. }),
            "{:?}",
            input.state
        );
        let up = input.mouse_up_for(Button::Left, end, CMD_SHIFT, &mut scene);
        assert!(
            up.iter()
                .any(|effect| matches!(effect, Effect::CreatePanel { .. })),
            "{up:?}"
        );
    }

    #[test]
    fn a_middle_gesture_leaves_a_cmd_move_in_progress_alone() {
        let (mut scene, id) = scene_with_terminal();
        let mut input = InputHandler::default();
        let mut gesture = ContentMove::default();
        let at = content_point(&scene, id);
        let before = scene.get(id).unwrap().rect;
        gesture.event(ContentEvent::Down(at), CMD_SHIFT, &mut input, &mut scene);
        let far = Point::new(at.x + 60.0, at.y + 20.0);
        let (decision, _) = gesture.event(
            ContentEvent::Dragged(far),
            CMD_SHIFT,
            &mut input,
            &mut scene,
        );
        assert_eq!(decision, ContentDecision::Moving);
        let elsewhere = Point::new(at.x - 50.0, at.y + 70.0);
        for event in [
            ContentEvent::MiddleDown(far),
            ContentEvent::MiddleDragged(elsewhere),
            ContentEvent::MiddleUp(elsewhere),
        ] {
            let (decision, effects) = gesture.event(event, CMD_SHIFT, &mut input, &mut scene);
            // Still swallowed, so the native view never sees a stray middle click.
            assert_eq!(decision, ContentDecision::Moving, "{event:?}");
            assert!(effects.is_empty(), "{event:?}: {effects:?}");
        }
        let farther = Point::new(at.x + 80.0, at.y + 30.0);
        gesture.event(
            ContentEvent::Dragged(farther),
            CMD_SHIFT,
            &mut input,
            &mut scene,
        );
        let (up, _) = gesture.event(ContentEvent::Up(farther), CMD_SHIFT, &mut input, &mut scene);
        assert_eq!(up, ContentDecision::Moving);
        assert_eq!(
            scene.get(id).unwrap().rect.origin,
            Point::new(before.origin.x + 80.0, before.origin.y + 30.0)
        );
        assert_eq!(scene.camera.offset, Point::new(0.0, 0.0));
    }

    #[test]
    fn a_press_off_content_goes_to_the_native_view() {
        let (mut scene, _) = scene_with_terminal();
        let mut input = InputHandler::default();
        let mut gesture = ContentMove::default();
        let (down, effects) = gesture.event(
            ContentEvent::Down(Point::new(5.0, 5.0)),
            CMD,
            &mut input,
            &mut scene,
        );
        assert_eq!(down, ContentDecision::Click);
        assert!(effects.is_empty());
    }
}
