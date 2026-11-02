//! Modifier press on a panel's content: a click or the start of a panel move.

use crate::geometry::Point;
use crate::interaction::CLICK_SLOP;
use crate::scene::{PanelId, Scene, content_rect};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GestureStep {
    /// Still within `CLICK_SLOP` of the press.
    Undecided,
    /// The pointer left the slop: start a panel move from the press point. Sent once.
    StartMove { from: Point },
    /// The move has started; `InputHandler::mouse_move` and `mouse_up` carry it on.
    Moving,
    /// Released within the slop.
    Click,
}

/// Tracks one press from a modifier press on content until it is a click or a move.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ContentGesture {
    start: Point,
    started: bool,
}

impl ContentGesture {
    pub fn press(screen: Point) -> Self {
        ContentGesture {
            start: screen,
            started: false,
        }
    }

    pub fn moved(&mut self, screen: Point) -> GestureStep {
        if self.started {
            return GestureStep::Moving;
        }
        if (screen.x - self.start.x).hypot(screen.y - self.start.y) > CLICK_SLOP {
            self.started = true;
            return GestureStep::StartMove { from: self.start };
        }
        GestureStep::Undecided
    }

    pub fn release(self) -> GestureStep {
        if self.started {
            GestureStep::Moving
        } else {
            GestureStep::Click
        }
    }
}

/// The panel whose content is under `screen`. Only the topmost panel at the point
/// counts, so a chip or a chrome strip in front blocks the content behind it.
pub fn content_gesture_target(scene: &Scene, screen: Point) -> Option<PanelId> {
    let world = scene.camera.screen_to_world(screen);
    let panel = scene.get(scene.hit_test(world)?)?;
    (!panel.minimized && content_rect(panel).contains(world)).then_some(panel.id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{Point, Rect};
    use crate::scene::{PanelKind, Scene};

    fn terminal() -> PanelKind {
        PanelKind::Terminal {
            command: None,
            cwd: None,
            profile: None,
            app: None,
        }
    }

    #[test]
    fn release_within_slop_is_a_click() {
        let mut gesture = ContentGesture::press(Point::new(10.0, 10.0));
        assert_eq!(
            gesture.moved(Point::new(12.0, 13.0)),
            GestureStep::Undecided
        );
        assert_eq!(gesture.release(), GestureStep::Click);
        // Exactly at the slop still counts as a click.
        let mut edge = ContentGesture::press(Point::new(0.0, 0.0));
        assert_eq!(edge.moved(Point::new(4.0, 0.0)), GestureStep::Undecided);
        assert_eq!(edge.release(), GestureStep::Click);
        assert_eq!(
            ContentGesture::press(Point::new(0.0, 0.0)).release(),
            GestureStep::Click
        );
    }

    #[test]
    fn move_past_slop_starts_once() {
        let start = Point::new(10.0, 10.0);
        let mut gesture = ContentGesture::press(start);
        assert_eq!(
            gesture.moved(Point::new(11.0, 10.0)),
            GestureStep::Undecided
        );
        assert_eq!(
            gesture.moved(Point::new(20.0, 10.0)),
            GestureStep::StartMove { from: start }
        );
        // Back inside the slop: still a move, never undecided or a click again.
        assert_eq!(gesture.moved(Point::new(10.0, 10.0)), GestureStep::Moving);
        assert_eq!(gesture.moved(Point::new(50.0, 50.0)), GestureStep::Moving);
        assert_eq!(gesture.release(), GestureStep::Moving);
    }

    #[test]
    fn no_target_on_empty_canvas_or_chip() {
        let mut scene = Scene::default();
        let back = scene.add(terminal(), Rect::new(0.0, 0.0, 400.0, 300.0));
        let front = scene.add(terminal(), Rect::new(200.0, 200.0, 400.0, 300.0));
        let chip = scene.add(terminal(), Rect::new(1000.0, 1000.0, 400.0, 300.0));
        scene.get_mut(chip).unwrap().minimized = true;
        let over = scene.add(terminal(), Rect::new(0.0, 500.0, 400.0, 300.0));
        let content_y = 500.0 + crate::scene::CHROME_HEIGHT + 50.0;

        // Positive controls: content of the back panel, and the front panel where they overlap.
        assert_eq!(
            content_gesture_target(&scene, Point::new(50.0, 100.0)),
            Some(back)
        );
        assert_eq!(
            content_gesture_target(&scene, Point::new(300.0, 290.0)),
            Some(front)
        );
        assert_eq!(
            content_gesture_target(&scene, Point::new(50.0, content_y)),
            Some(over)
        );
        // Empty canvas, a minimized chip, and the chrome strip.
        assert_eq!(
            content_gesture_target(&scene, Point::new(900.0, 50.0)),
            None
        );
        assert_eq!(
            content_gesture_target(&scene, Point::new(1005.0, 1005.0)),
            None
        );
        // A borderless chip: its content rect starts at the chip, so only `minimized` decides.
        crate::scene::set_borderless(scene.get_mut(chip).unwrap(), true);
        let chip_at = scene.get(chip).unwrap().rect.origin;
        let in_chip = Point::new(chip_at.x + 5.0, chip_at.y + 5.0);
        assert_eq!(content_gesture_target(&scene, in_chip), None);
        assert_eq!(
            content_gesture_target(&scene, Point::new(300.0, 205.0)),
            None
        );
        // A chip on top of another panel's content blocks it.
        scene.get_mut(over).unwrap().minimized = true;
        scene.get_mut(over).unwrap().rect.origin = Point::new(40.0, 90.0);
        assert_eq!(
            content_gesture_target(&scene, Point::new(50.0, 100.0)),
            None
        );
        // Borderless: the whole rect is content.
        crate::scene::set_borderless(scene.get_mut(front).unwrap(), true);
        let top = scene.get(front).unwrap().rect.origin;
        assert_eq!(
            content_gesture_target(&scene, Point::new(top.x + 100.0, top.y + 2.0)),
            Some(front)
        );
    }

    #[test]
    fn target_follows_the_camera() {
        let mut scene = Scene::default();
        let id = scene.add(terminal(), Rect::new(0.0, 0.0, 400.0, 300.0));
        scene.camera.pan_by_screen(1000.0, 0.0);
        assert_eq!(
            content_gesture_target(&scene, Point::new(50.0, 100.0)),
            None
        );
        assert_eq!(
            content_gesture_target(&scene, Point::new(1050.0, 100.0)),
            Some(id)
        );
    }
}
