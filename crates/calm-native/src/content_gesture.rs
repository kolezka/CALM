//! Watches mouse events over native panels (Ghostty, WKWebView) so the app can turn a
//! modifier+drag on panel content into a panel move, while a plain click still reaches the
//! native view. The monitor holds no policy: `wants` decides which presses it takes and
//! `on_event` decides what each held gesture becomes.

use std::cell::RefCell;
use std::ptr::NonNull;
use std::rc::Rc;

use block2::RcBlock;
use calm_core::geometry::Point;
use objc2::rc::{Retained, Weak};
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, Message};
use objc2_app_kit::{NSEvent, NSEventMask, NSEventModifierFlags, NSEventType, NSWindow};

use crate::NativeError;
use crate::child::is_in_native_child;

/// A mouse event over a native panel, in window points with a top-left origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ContentEvent {
    Down(Point),
    Dragged(Point),
    Up(Point),
    RightDown(Point),
}

/// Modifier keys held at a press.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct GestureMods {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub command: bool,
}

/// What the app made of a reported event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentDecision {
    /// Not decided yet: keep holding the press.
    Hold,
    /// The gesture moves the panel: the held press is dropped.
    Moving,
    /// A plain click: the held press and what followed go to the native view.
    Click,
}

type Wants = Box<dyn Fn(Point, GestureMods) -> bool>;
type OnEvent = Box<dyn FnMut(ContentEvent) -> ContentDecision>;

struct Shared {
    window: Weak<NSWindow>,
    wants: Wants,
    on_event: RefCell<OnEvent>,
    tracker: RefCell<Tracker<Retained<NSEvent>>>,
    /// Events this monitor is replaying; they pass through untouched.
    replaying: RefCell<Vec<Retained<NSEvent>>>,
}

/// A local mouse monitor for one window. Dropping it removes the monitor.
pub struct ContentGestureMonitor {
    monitor: Option<Retained<AnyObject>>,
}

impl ContentGestureMonitor {
    /// Watches left and right mouse events over `window`'s native panels. `wants` decides
    /// at a press whether the gesture is taken; `on_event` gets every taken event and
    /// answers what the gesture is so far.
    pub fn install(
        window: &NSWindow,
        wants: impl Fn(Point, GestureMods) -> bool + 'static,
        on_event: impl FnMut(ContentEvent) -> ContentDecision + 'static,
    ) -> Result<ContentGestureMonitor, NativeError> {
        MainThreadMarker::new().ok_or(NativeError::NotMainThread)?;
        let shared = Rc::new(Shared {
            window: Weak::new(window),
            wants: Box::new(wants),
            on_event: RefCell::new(Box::new(on_event)),
            tracker: RefCell::new(Tracker::default()),
            replaying: RefCell::new(Vec::new()),
        });
        let handler = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
            // SAFETY: AppKit passes a live event for the duration of the handler.
            let event_ref = unsafe { event.as_ref() };
            if handle_event(&shared, event_ref) {
                std::ptr::null_mut()
            } else {
                event.as_ptr()
            }
        });
        let mask = NSEventMask::LeftMouseDown
            | NSEventMask::LeftMouseDragged
            | NSEventMask::LeftMouseUp
            | NSEventMask::RightMouseDown
            | NSEventMask::RightMouseUp;
        // SAFETY: the block returns the event or null as the API requires; the monitor is
        // removed in Drop, which releases the block and what it captured.
        let monitor =
            unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &handler) };
        Ok(ContentGestureMonitor { monitor })
    }
}

impl Drop for ContentGestureMonitor {
    fn drop(&mut self) {
        if let Some(monitor) = self.monitor.take() {
            // SAFETY: the monitor came from addLocalMonitorForEventsMatchingMask:handler:.
            unsafe { NSEvent::removeMonitor(&monitor) };
        }
    }
}

/// True when the event was swallowed.
fn handle_event(shared: &Shared, event: &NSEvent) -> bool {
    {
        let mut replaying = shared.replaying.borrow_mut();
        if let Some(index) = replaying
            .iter()
            .position(|replayed| std::ptr::eq(&**replayed, event))
        {
            replaying.remove(index);
            return false;
        }
    }
    let input = match event.r#type() {
        NSEventType::LeftMouseDown => Input::LeftDown,
        NSEventType::LeftMouseDragged => Input::LeftDragged,
        NSEventType::LeftMouseUp => Input::LeftUp,
        NSEventType::RightMouseDown => Input::RightDown,
        NSEventType::RightMouseUp => Input::RightUp,
        _ => return false,
    };
    let Some(window) = shared.window.load() else {
        return false;
    };
    // A nested event loop inside `on_event` (a menu, say) must not re-enter the gesture.
    let Ok(mut tracker) = shared.tracker.try_borrow_mut() else {
        return false;
    };
    let point = window_point(&window, event);
    let disposition = tracker.input(input, event.retain(), || {
        let over_native = over_native_child(&window, event);
        // `wants` is app policy: ask it only for presses over a native view.
        let wants = over_native && (shared.wants)(point, gesture_mods(event));
        !decide_passthrough(over_native, wants)
    });
    drop(tracker);
    if disposition != Disposition::Report {
        return disposition == Disposition::Swallow;
    }
    let content_event = match input {
        Input::LeftDown => ContentEvent::Down(point),
        Input::LeftDragged => ContentEvent::Dragged(point),
        Input::LeftUp => ContentEvent::Up(point),
        Input::RightDown | Input::RightUp => ContentEvent::RightDown(point),
    };
    let decision = match shared.on_event.try_borrow_mut() {
        Ok(mut on_event) => on_event(content_event),
        Err(_) => ContentDecision::Hold,
    };
    let mut replay = Vec::new();
    if let Ok(mut tracker) = shared.tracker.try_borrow_mut() {
        tracker.decide(decision, &mut |held| replay.push(held));
    }
    if let Some((first, rest)) = replay.split_first() {
        // Replacing, not appending, keeps the list bounded if a posted event never shows up.
        *shared.replaying.borrow_mut() = replay.clone();
        // The rest go to the front of the queue before the press is sent, so a view that
        // tracks the mouse in its own loop after the press still gets its up.
        for later in rest.iter().rev() {
            window.postEvent_atStart(later, true);
        }
        window.sendEvent(first);
        shared
            .replaying
            .borrow_mut()
            .retain(|replayed| !std::ptr::eq(&**replayed, &**first));
    }
    true
}

/// The event's window is this window and the view under it is part of a native panel.
fn over_native_child(window: &NSWindow, event: &NSEvent) -> bool {
    let mtm = MainThreadMarker::from(window);
    if event.window(mtm).as_deref() != Some(window) {
        return false;
    }
    let Some(content) = window.contentView() else {
        return false;
    };
    // hitTest takes a point in the superview's space, which for the content view is the
    // window's frame view, so window coordinates.
    content
        .hitTest(event.locationInWindow())
        .is_some_and(|hit| is_in_native_child(&hit))
}

fn window_point(window: &NSWindow, event: &NSEvent) -> Point {
    let location = event.locationInWindow();
    let height = window
        .contentView()
        .map_or(0.0, |content| content.frame().size.height);
    top_left_point(location.x, location.y, height)
}

fn gesture_mods(event: &NSEvent) -> GestureMods {
    let flags = event.modifierFlags();
    GestureMods {
        shift: flags.contains(NSEventModifierFlags::Shift),
        control: flags.contains(NSEventModifierFlags::Control),
        alt: flags.contains(NSEventModifierFlags::Option),
        command: flags.contains(NSEventModifierFlags::Command),
    }
}

/// True when the monitor leaves an event alone: only a wanted press over a native view
/// is taken.
pub(crate) fn decide_passthrough(over_native: bool, wants: bool) -> bool {
    !(over_native && wants)
}

/// AppKit window point (bottom-left origin) to the canvas's top-left origin.
pub(crate) fn top_left_point(x: f64, y: f64, content_height: f64) -> Point {
    Point::new(x, content_height - y)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Input {
    LeftDown,
    LeftDragged,
    LeftUp,
    RightDown,
    RightUp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Disposition {
    /// Not ours: AppKit handles it as usual.
    Pass,
    /// Ours but not reported.
    Swallow,
    /// Ours: report it, then call `Tracker::decide` with the answer.
    Report,
}

#[derive(Debug, Default)]
enum Phase {
    #[default]
    Idle,
    /// A wanted left press, possibly still held for replay.
    Left,
}

/// The policy free state machine behind the monitor, generic over the event so tests can
/// use plain values. Replay goes through a sink so the caller can deliver events after
/// releasing any borrow on the tracker.
#[derive(Debug)]
pub(crate) struct Tracker<E> {
    phase: Phase,
    swallow_right_up: bool,
    /// The swallowed left press, until the gesture is a move or a click.
    held: Option<E>,
    /// The event being reported, until `decide`.
    current: Option<(Input, E)>,
}

impl<E> Default for Tracker<E> {
    fn default() -> Self {
        Tracker {
            phase: Phase::Idle,
            swallow_right_up: false,
            held: None,
            current: None,
        }
    }
}

impl<E: Clone> Tracker<E> {
    /// Classifies one event. `candidate` (over a native view and wanted) is asked only for
    /// presses that could start a gesture.
    pub(crate) fn input(
        &mut self,
        input: Input,
        event: E,
        candidate: impl FnOnce() -> bool,
    ) -> Disposition {
        self.current = None;
        let disposition = match (&self.phase, input) {
            (_, Input::LeftDown) => {
                // A new press resets a stale gesture for the same button.
                self.phase = Phase::Idle;
                self.held = None;
                if !candidate() {
                    return Disposition::Pass;
                }
                self.phase = Phase::Left;
                self.held = Some(event.clone());
                Disposition::Report
            }
            (_, Input::RightDown) => {
                self.swallow_right_up = candidate();
                if !self.swallow_right_up {
                    return Disposition::Pass;
                }
                Disposition::Report
            }
            (Phase::Left, Input::LeftDragged | Input::LeftUp) => Disposition::Report,
            (_, Input::RightUp) if self.swallow_right_up => {
                self.swallow_right_up = false;
                return Disposition::Swallow;
            }
            _ => return Disposition::Pass,
        };
        self.current = Some((input, event));
        disposition
    }

    /// Applies the app's answer to the event `input` last reported.
    pub(crate) fn decide(&mut self, decision: ContentDecision, sink: &mut impl FnMut(E)) {
        let Some((input, event)) = self.current.take() else {
            return;
        };
        if input == Input::RightDown {
            return;
        }
        match decision {
            ContentDecision::Hold => {}
            ContentDecision::Moving => self.held = None,
            ContentDecision::Click => {
                // After a move the press is gone and there is nothing to click.
                if let Some(held) = self.held.take() {
                    // The press first, then the event that decided it, unless that is the
                    // press. Later events of this gesture pass through.
                    sink(held);
                    if input != Input::LeftDown {
                        sink(event);
                    }
                    self.phase = Phase::Idle;
                    return;
                }
            }
        }
        if input == Input::LeftUp {
            // No click: the gesture ends without the native view seeing any of it.
            self.held = None;
            self.phase = Phase::Idle;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decide_passthrough_takes_only_wanted_presses_over_native_views() {
        assert!(decide_passthrough(false, false));
        assert!(decide_passthrough(false, true));
        assert!(decide_passthrough(true, false));
        assert!(!decide_passthrough(true, true));
    }

    #[test]
    fn replays_down_then_up_on_click() {
        let mut tracker = Tracker::default();
        let mut sink = Vec::new();
        assert_eq!(
            tracker.input(Input::LeftDown, "down", || true),
            Disposition::Report
        );
        tracker.decide(ContentDecision::Hold, &mut |e| sink.push(e));
        assert!(sink.is_empty());
        assert_eq!(
            tracker.input(Input::LeftUp, "up", || unreachable!()),
            Disposition::Report
        );
        tracker.decide(ContentDecision::Click, &mut |e| sink.push(e));
        assert_eq!(sink, ["down", "up"]);
        // The gesture is over: the next drag is not ours.
        assert_eq!(
            tracker.input(Input::LeftDragged, "drag", || unreachable!()),
            Disposition::Pass
        );
    }

    #[test]
    fn right_press_during_left_gesture_keeps_the_held_click() {
        let mut tracker = Tracker::default();
        let mut sink = Vec::new();
        assert_eq!(
            tracker.input(Input::LeftDown, "down", || true),
            Disposition::Report
        );
        tracker.decide(ContentDecision::Hold, &mut |e| sink.push(e));
        assert_eq!(
            tracker.input(Input::RightDown, "rdown", || false),
            Disposition::Pass
        );
        assert_eq!(
            tracker.input(Input::LeftUp, "up", || unreachable!()),
            Disposition::Report
        );
        tracker.decide(ContentDecision::Click, &mut |e| sink.push(e));
        assert_eq!(sink, ["down", "up"]);
    }

    #[test]
    fn left_press_after_captured_right_still_swallows_right_up() {
        let mut tracker = Tracker::<&str>::default();
        assert_eq!(
            tracker.input(Input::RightDown, "rdown", || true),
            Disposition::Report
        );
        tracker.decide(ContentDecision::Hold, &mut |_| panic!("no replay"));
        assert_eq!(
            tracker.input(Input::LeftDown, "down", || false),
            Disposition::Pass
        );
        assert_eq!(
            tracker.input(Input::RightUp, "rup", || unreachable!()),
            Disposition::Swallow
        );
    }

    #[test]
    fn drops_held_down_on_move() {
        let mut tracker = Tracker::default();
        let mut sink = Vec::new();
        tracker.input(Input::LeftDown, "down", || true);
        tracker.decide(ContentDecision::Hold, &mut |e| sink.push(e));
        assert_eq!(
            tracker.input(Input::LeftDragged, "drag", || unreachable!()),
            Disposition::Report
        );
        tracker.decide(ContentDecision::Moving, &mut |e| sink.push(e));
        assert_eq!(
            tracker.input(Input::LeftDragged, "drag2", || unreachable!()),
            Disposition::Report
        );
        tracker.decide(ContentDecision::Moving, &mut |e| sink.push(e));
        assert_eq!(
            tracker.input(Input::LeftUp, "up", || unreachable!()),
            Disposition::Report
        );
        // Even a late Click cannot resurrect the dropped press.
        tracker.decide(ContentDecision::Click, &mut |e| sink.push(e));
        assert!(sink.is_empty(), "replayed {sink:?}");
        assert_eq!(
            tracker.input(Input::LeftUp, "up2", || unreachable!()),
            Disposition::Pass
        );
    }

    #[test]
    fn unwanted_presses_and_their_followers_pass_through() {
        let mut tracker = Tracker::<&str>::default();
        assert_eq!(
            tracker.input(Input::LeftDown, "down", || false),
            Disposition::Pass
        );
        assert_eq!(
            tracker.input(Input::LeftDragged, "drag", || unreachable!()),
            Disposition::Pass
        );
        assert_eq!(
            tracker.input(Input::LeftUp, "up", || unreachable!()),
            Disposition::Pass
        );
        assert_eq!(
            tracker.input(Input::RightDown, "rdown", || false),
            Disposition::Pass
        );
        assert_eq!(
            tracker.input(Input::RightUp, "rup", || unreachable!()),
            Disposition::Pass
        );
    }

    #[test]
    fn wanted_right_down_and_its_up_are_swallowed() {
        let mut tracker = Tracker::<&str>::default();
        assert_eq!(
            tracker.input(Input::RightDown, "rdown", || true),
            Disposition::Report
        );
        tracker.decide(ContentDecision::Hold, &mut |_| panic!("no replay"));
        assert_eq!(
            tracker.input(Input::RightUp, "rup", || unreachable!()),
            Disposition::Swallow
        );
        assert_eq!(
            tracker.input(Input::RightUp, "rup2", || unreachable!()),
            Disposition::Pass
        );
    }

    #[test]
    fn click_before_up_replays_the_press_and_releases_the_gesture() {
        let mut tracker = Tracker::default();
        let mut sink = Vec::new();
        tracker.input(Input::LeftDown, "down", || true);
        tracker.decide(ContentDecision::Hold, &mut |e| sink.push(e));
        tracker.input(Input::LeftDragged, "drag", || unreachable!());
        tracker.decide(ContentDecision::Click, &mut |e| sink.push(e));
        assert_eq!(sink, ["down", "drag"]);
        assert_eq!(
            tracker.input(Input::LeftUp, "up", || unreachable!()),
            Disposition::Pass
        );
    }

    #[test]
    fn a_new_press_resets_a_stale_gesture() {
        let mut tracker = Tracker::default();
        // A right up that never reached the monitor leaves the gesture open.
        tracker.input(Input::RightDown, "rdown", || true);
        tracker.decide(ContentDecision::Hold, &mut |_| panic!("no replay"));
        assert_eq!(
            tracker.input(Input::RightDown, "rdown2", || true),
            Disposition::Report
        );
        tracker.decide(ContentDecision::Hold, &mut |_| panic!("no replay"));
        // Same for a left press whose up was lost: the old press is never replayed.
        let mut sink = Vec::new();
        tracker.input(Input::LeftDown, "down", || true);
        tracker.decide(ContentDecision::Hold, &mut |e| sink.push(e));
        assert_eq!(
            tracker.input(Input::LeftDown, "down2", || false),
            Disposition::Pass
        );
        assert_eq!(
            tracker.input(Input::LeftUp, "up", || unreachable!()),
            Disposition::Pass
        );
        assert!(sink.is_empty(), "replayed {sink:?}");
    }

    #[test]
    fn window_points_flip_to_a_top_left_origin() {
        assert_eq!(top_left_point(10.0, 30.0, 400.0), Point::new(10.0, 370.0));
        assert_eq!(top_left_point(0.0, 400.0, 400.0), Point::new(0.0, 0.0));
    }
}
