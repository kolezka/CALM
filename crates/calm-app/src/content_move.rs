//! Move-modifier gestures on a native panel's content: a drag moves the panel, a click
//! still reaches the page or terminal, a right click opens the panel menu. This is the
//! app policy behind `calm_native::ContentGestureMonitor`.

use calm_core::geometry::Point;
use calm_core::gesture::{ContentGesture, GestureStep, content_gesture_target};
use calm_core::interaction::{Effect, InputHandler, Modifiers};
use calm_core::scene::{PanelId, Scene};
use calm_core::settings::Settings;
use calm_native::{ContentDecision, ContentEvent, ContentGestureMonitor, GestureMods};
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
            ContentEvent::Down(screen) => {
                self.resizing = false;
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
            ContentEvent::Dragged(screen) if self.resizing => (
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
            ContentEvent::Up(screen) if self.resizing => {
                self.resizing = false;
                (ContentDecision::Moving, input.mouse_up(screen, mods, scene))
            }
            ContentEvent::Up(screen) => match self.gesture.take() {
                Some((_, gesture)) if gesture.release() == GestureStep::Moving => {
                    (ContentDecision::Moving, input.mouse_up(screen, mods, scene))
                }
                _ => (ContentDecision::Click, Vec::new()),
            },
            // The monitor ignores the answer to a right press.
            ContentEvent::RightDown(screen) => match content_gesture_target(scene, screen) {
                Some(id) => (ContentDecision::Moving, input.content_menu(id, screen)),
                None => (ContentDecision::Click, Vec::new()),
            },
        }
    }
}

/// `event` with its point moved into canvas coordinates.
fn in_canvas(event: ContentEvent, canvas_origin: Point) -> ContentEvent {
    match event {
        ContentEvent::Down(point) => ContentEvent::Down(to_canvas(point, canvas_origin)),
        ContentEvent::Dragged(point) => ContentEvent::Dragged(to_canvas(point, canvas_origin)),
        ContentEvent::Up(point) => ContentEvent::Up(to_canvas(point, canvas_origin)),
        ContentEvent::RightDown(point) => ContentEvent::RightDown(to_canvas(point, canvas_origin)),
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
        let installed = ContentGestureMonitor::install_for(
            window,
            move |point, mods| {
                let mut cx = wants_cx.clone();
                // Fails while GPUI is mid-update (a modal loop, say): leave the press alone.
                wants_view
                    .update_in(&mut cx, |view, _, _| {
                        view.wants_content_gesture(point, mods)
                    })
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

    fn wants_content_gesture(&mut self, point: Point, mods: GestureMods) -> bool {
        let screen = to_canvas(point, self.viewport.origin);
        let mods = to_modifiers(mods);
        // Wants and Down come from the same NSEvent in one call.
        self.content_press_mods = Some(mods);
        wants(
            &self.settings.settings,
            &self.input,
            &self.scene,
            screen,
            mods,
        )
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
