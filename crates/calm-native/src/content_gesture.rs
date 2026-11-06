//! Watches mouse events over native panels (Ghostty, WKWebView) so the app can turn a
//! modifier+drag on panel content into a panel move, while a plain click still reaches the
//! native view. A middle drag over a panel always goes to the app (it pans the canvas), and
//! so does a scroll when `wants_scroll` says so. A trackpad scroll gesture keeps the owner
//! it got at its first event, momentum included. The monitor holds no policy: `wants`
//! decides which presses it takes and `on_event` decides what each held gesture becomes.

use std::cell::RefCell;
use std::ptr::NonNull;
use std::rc::Rc;

use block2::RcBlock;
use calm_core::geometry::Point;
use objc2::rc::{Retained, Weak};
use objc2::runtime::AnyObject;
use objc2::{MainThreadMarker, Message};
use objc2_app_kit::{
    NSEvent, NSEventMask, NSEventModifierFlags, NSEventPhase, NSEventType, NSView, NSWindow,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};

use crate::NativeError;
use crate::child::is_in_native_child;

/// A mouse event over a native panel, in window points with a top-left origin.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ContentEvent {
    Down(Point),
    Dragged(Point),
    Up(Point),
    /// A pointer move over a native child. It is still sent to AppKit.
    Moved(Point),
    RightDown(Point),
    MiddleDown(Point),
    MiddleDragged(Point),
    MiddleUp(Point),
    /// A scroll wheel or trackpad scroll. `precise` deltas are points, others are lines.
    /// `mods` are the keys held when the scroll happened, not when it is handled.
    Scroll {
        point: Point,
        dx: f64,
        dy: f64,
        precise: bool,
        mods: GestureMods,
    },
}

/// The button of a press the app is asked about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentButton {
    Left,
    Right,
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

type Wants = Box<dyn Fn(ContentButton, Point, GestureMods) -> bool>;
type WantsScroll = Box<dyn Fn(Point, bool) -> bool>;
type OnEvent = Box<dyn FnMut(ContentEvent) -> ContentDecision>;

struct Shared {
    window: Weak<NSWindow>,
    wants: Wants,
    wants_scroll: WantsScroll,
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
    /// Watches mouse and scroll events over `window`'s native panels. `wants` decides at
    /// a left or right press whether the gesture is taken; a middle press is always taken.
    /// `wants_scroll(point, over_native)` decides at the first event of a scroll gesture,
    /// or per event for a wheel without phases, anywhere in the window. `on_event` gets
    /// every taken event and answers what the gesture is so far.
    pub fn install(
        window: &NSWindow,
        wants: impl Fn(ContentButton, Point, GestureMods) -> bool + 'static,
        wants_scroll: impl Fn(Point, bool) -> bool + 'static,
        on_event: impl FnMut(ContentEvent) -> ContentDecision + 'static,
    ) -> Result<ContentGestureMonitor, NativeError> {
        MainThreadMarker::new().ok_or(NativeError::NotMainThread)?;
        let shared = Rc::new(Shared {
            window: Weak::new(window),
            wants: Box::new(wants),
            wants_scroll: Box::new(wants_scroll),
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
        let mask = NSEventMask::MouseMoved
            | NSEventMask::LeftMouseDown
            | NSEventMask::LeftMouseDragged
            | NSEventMask::LeftMouseUp
            | NSEventMask::RightMouseDown
            | NSEventMask::RightMouseUp
            | NSEventMask::OtherMouseDown
            | NSEventMask::OtherMouseDragged
            | NSEventMask::OtherMouseUp
            | NSEventMask::ScrollWheel;
        // SAFETY: the block returns the event or null as the API requires; the monitor is
        // removed in Drop, which releases the block and what it captured.
        let monitor =
            unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &handler) };
        Ok(ContentGestureMonitor { monitor })
    }

    /// `install` on the AppKit window behind a window handle, such as a GPUI window.
    pub fn install_for(
        window: &impl HasWindowHandle,
        wants: impl Fn(ContentButton, Point, GestureMods) -> bool + 'static,
        wants_scroll: impl Fn(Point, bool) -> bool + 'static,
        on_event: impl FnMut(ContentEvent) -> ContentDecision + 'static,
    ) -> Result<ContentGestureMonitor, NativeError> {
        let ns_window = ns_window(window)?;
        ContentGestureMonitor::install(&ns_window, wants, wants_scroll, on_event)
    }
}

/// The NSWindow behind an AppKit window handle, retained.
pub(crate) fn ns_window(window: &impl HasWindowHandle) -> Result<Retained<NSWindow>, NativeError> {
    MainThreadMarker::new().ok_or(NativeError::NotMainThread)?;
    let handle = window
        .window_handle()
        .map_err(|err| NativeError::UnsupportedWindowHandle(err.to_string()))?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return Err(NativeError::UnsupportedWindowHandle(
            "not an AppKit handle".to_string(),
        ));
    };
    // SAFETY: an AppKit window handle's ns_view is a live NSView while the handle is
    // borrowed, and we are on the main thread (checked above).
    let view: &NSView = unsafe { appkit.ns_view.cast::<NSView>().as_ref() };
    view.window().ok_or_else(|| {
        NativeError::UnsupportedWindowHandle("the view is not in a window".to_string())
    })
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
    if event.r#type() == NSEventType::MouseMoved {
        if let Some(window) = shared.window.load()
            && over_native_child(&window, event)
            && let Ok(mut on_event) = shared.on_event.try_borrow_mut()
        {
            on_event(ContentEvent::Moved(window_point(&window, event)));
        }
        return false;
    }
    let input = match event.r#type() {
        NSEventType::LeftMouseDown => Input::LeftDown,
        NSEventType::LeftMouseDragged => Input::LeftDragged,
        NSEventType::LeftMouseUp => Input::LeftUp,
        NSEventType::RightMouseDown => Input::RightDown,
        NSEventType::RightMouseUp => Input::RightUp,
        // Button 2 is the middle button; other extra buttons stay with AppKit.
        NSEventType::OtherMouseDown if event.buttonNumber() == MIDDLE_BUTTON => Input::MiddleDown,
        NSEventType::OtherMouseDragged if event.buttonNumber() == MIDDLE_BUTTON => {
            Input::MiddleDragged
        }
        NSEventType::OtherMouseUp if event.buttonNumber() == MIDDLE_BUTTON => Input::MiddleUp,
        NSEventType::ScrollWheel => {
            Input::Scroll(scroll_stage(event.phase(), event.momentumPhase()))
        }
        _ => return false,
    };
    let Some(window) = shared.window.load() else {
        return false;
    };
    // The monitor sees every window of the app; a scroll in another one is not ours.
    if matches!(input, Input::Scroll(_)) && !is_this_window(&window, event) {
        return false;
    }
    // A nested event loop inside `on_event` (a menu, say) must not re-enter the gesture.
    let Ok(mut tracker) = shared.tracker.try_borrow_mut() else {
        return false;
    };
    let point = window_point(&window, event);
    let disposition = tracker.input(input, event.retain(), || {
        let over_native = over_native_child(&window, event);
        // A scroll is asked anywhere in the window: a gesture that starts on empty canvas
        // stays with the canvas when a panel slides under the pointer.
        if matches!(input, Input::Scroll(_)) {
            return (shared.wants_scroll)(point, over_native);
        }
        // `wants` is app policy: ask it only for presses over a native view.
        let wants = over_native
            && match input {
                Input::LeftDown => (shared.wants)(ContentButton::Left, point, gesture_mods(event)),
                Input::RightDown => {
                    (shared.wants)(ContentButton::Right, point, gesture_mods(event))
                }
                _ => true,
            };
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
        Input::MiddleDown => ContentEvent::MiddleDown(point),
        Input::MiddleDragged => ContentEvent::MiddleDragged(point),
        Input::MiddleUp => ContentEvent::MiddleUp(point),
        Input::Scroll(_) => ContentEvent::Scroll {
            point,
            dx: event.scrollingDeltaX(),
            dy: event.scrollingDeltaY(),
            precise: event.hasPreciseScrollingDeltas(),
            mods: gesture_mods(event),
        },
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

/// `buttonNumber` of the middle mouse button.
const MIDDLE_BUTTON: isize = 2;

fn is_this_window(window: &NSWindow, event: &NSEvent) -> bool {
    let mtm = MainThreadMarker::from(window);
    event.window(mtm).as_deref() == Some(window)
}

/// The event's window is this window and the view under it is part of a native panel.
fn over_native_child(window: &NSWindow, event: &NSEvent) -> bool {
    if !is_this_window(window, event) {
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
    mods_from_flags(event.modifierFlags())
}

/// The modifier keys held right now. Drag events reach `on_event` without their keys.
pub fn held_mods() -> GestureMods {
    mods_from_flags(NSEvent::modifierFlags_class())
}

fn mods_from_flags(flags: NSEventModifierFlags) -> GestureMods {
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
    MiddleDown,
    MiddleDragged,
    MiddleUp,
    Scroll(ScrollStage),
}

/// Where a scroll event sits in a trackpad gesture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ScrollStage {
    /// No gesture phases: a mouse wheel.
    Wheel,
    /// The first event of a gesture.
    Began,
    /// A later event, momentum included.
    Continued,
    /// The last event of a gesture. It still carries a delta.
    Ended,
}

/// The stage of a scroll from its `phase` and `momentumPhase`. Both are empty for a mouse
/// wheel. A finger lift (`phase` Ended) does not end the gesture: AppKit sends it alone and
/// momentum follows with an empty `phase`, so the owner holds until the momentum ends. A
/// gesture without momentum leaves its owner behind, and the next Began replaces it.
pub(crate) fn scroll_stage(phase: NSEventPhase, momentum: NSEventPhase) -> ScrollStage {
    let done = NSEventPhase::Ended | NSEventPhase::Cancelled;
    if phase.is_empty() && momentum.is_empty() {
        ScrollStage::Wheel
    } else if phase.intersects(NSEventPhase::Began | NSEventPhase::MayBegin) {
        ScrollStage::Began
    } else if momentum.intersects(done) || phase.intersects(NSEventPhase::Cancelled) {
        ScrollStage::Ended
    } else {
        ScrollStage::Continued
    }
}

/// Who a scroll gesture belongs to, fixed at its first event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScrollOwner {
    /// Reported to the app, which pans or zooms the canvas.
    Canvas,
    /// Left to AppKit, which sends it to the view under the pointer.
    Panel,
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
    /// A middle press over a native view is ours until its up. Kept apart from `phase` so
    /// a middle drag leaves a held left press alone.
    middle: bool,
    /// The owner of the scroll gesture in progress. Kept apart from the buttons.
    scroll_owner: Option<ScrollOwner>,
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
            middle: false,
            scroll_owner: None,
            held: None,
            current: None,
        }
    }
}

impl<E: Clone> Tracker<E> {
    /// Classifies one event. `candidate` (over a native view and wanted, or for a scroll
    /// just wanted) is asked only for events that could start a gesture.
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
            (_, Input::MiddleDown) => {
                // No hold and no replay: a middle press over a panel is never a click.
                self.middle = candidate();
                if !self.middle {
                    return Disposition::Pass;
                }
                Disposition::Report
            }
            (_, Input::MiddleDragged) if self.middle => Disposition::Report,
            (_, Input::MiddleUp) if self.middle => {
                self.middle = false;
                Disposition::Report
            }
            (_, Input::Scroll(stage)) => {
                let owner = match stage {
                    ScrollStage::Continued => self.scroll_owner,
                    ScrollStage::Ended => self.scroll_owner.take(),
                    ScrollStage::Began | ScrollStage::Wheel => None,
                };
                // No owner: a wheel, or a gesture that began before the monitor did or
                // after a cancel. A new gesture always picks its owner afresh.
                let ours = match owner {
                    Some(owner) => owner == ScrollOwner::Canvas,
                    None => candidate(),
                };
                if stage == ScrollStage::Began {
                    self.scroll_owner = Some(if ours {
                        ScrollOwner::Canvas
                    } else {
                        ScrollOwner::Panel
                    });
                }
                if !ours {
                    return Disposition::Pass;
                }
                Disposition::Report
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
        // Only a left gesture holds a press: the answer to anything else changes nothing.
        if !matches!(input, Input::LeftDown | Input::LeftDragged | Input::LeftUp) {
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
    fn middle_press_over_native_is_taken_with_its_drags_and_up() {
        let mut tracker = Tracker::<&str>::default();
        assert_eq!(
            tracker.input(Input::MiddleDown, "mdown", || true),
            Disposition::Report
        );
        tracker.decide(ContentDecision::Moving, &mut |_| panic!("no replay"));
        for _ in 0..2 {
            assert_eq!(
                tracker.input(Input::MiddleDragged, "mdrag", || unreachable!()),
                Disposition::Report
            );
            tracker.decide(ContentDecision::Moving, &mut |_| panic!("no replay"));
        }
        assert_eq!(
            tracker.input(Input::MiddleUp, "mup", || unreachable!()),
            Disposition::Report
        );
        tracker.decide(ContentDecision::Moving, &mut |_| panic!("no replay"));
        // The gesture is over: a stray drag or up is not ours.
        assert_eq!(
            tracker.input(Input::MiddleDragged, "mdrag2", || unreachable!()),
            Disposition::Pass
        );
        assert_eq!(
            tracker.input(Input::MiddleUp, "mup2", || unreachable!()),
            Disposition::Pass
        );
    }

    #[test]
    fn middle_press_off_native_passes_with_its_drags_and_up() {
        let mut tracker = Tracker::<&str>::default();
        assert_eq!(
            tracker.input(Input::MiddleDown, "mdown", || false),
            Disposition::Pass
        );
        assert_eq!(
            tracker.input(Input::MiddleDragged, "mdrag", || unreachable!()),
            Disposition::Pass
        );
        assert_eq!(
            tracker.input(Input::MiddleUp, "mup", || unreachable!()),
            Disposition::Pass
        );
    }

    #[test]
    fn a_wheel_scroll_is_asked_per_event() {
        let mut tracker = Tracker::<&str>::default();
        let wheel = Input::Scroll(ScrollStage::Wheel);
        assert_eq!(tracker.input(wheel, "scroll", || false), Disposition::Pass);
        assert_eq!(
            tracker.input(wheel, "scroll2", || true),
            Disposition::Report
        );
        tracker.decide(ContentDecision::Click, &mut |_| panic!("no replay"));
        // Asked per event: wanted once does not mean wanted next time.
        assert_eq!(tracker.input(wheel, "scroll3", || false), Disposition::Pass);
    }

    #[test]
    fn a_canvas_scroll_gesture_stays_with_the_canvas_until_it_ends() {
        let mut tracker = Tracker::<&str>::default();
        let mut no_replay = |_| panic!("no replay");
        assert_eq!(
            tracker.input(Input::Scroll(ScrollStage::Began), "began", || true),
            Disposition::Report
        );
        tracker.decide(ContentDecision::Moving, &mut no_replay);
        // A panel under the pointer now would say no; it is never asked.
        for _ in 0..3 {
            assert_eq!(
                tracker.input(Input::Scroll(ScrollStage::Continued), "changed", || {
                    panic!("asked mid gesture")
                }),
                Disposition::Report
            );
            tracker.decide(ContentDecision::Moving, &mut no_replay);
        }
        assert_eq!(
            tracker.input(Input::Scroll(ScrollStage::Ended), "ended", || {
                panic!("asked at the end")
            }),
            Disposition::Report
        );
        tracker.decide(ContentDecision::Moving, &mut no_replay);
        // The gesture is over: a stray event is decided on its own, like a wheel.
        assert_eq!(
            tracker.input(Input::Scroll(ScrollStage::Continued), "stray", || false),
            Disposition::Pass
        );
    }

    /// The user's case: a flick that starts on empty canvas while a panel slides under the
    /// pointer. AppKit's real phases, mapped and fed through the tracker, keep every event
    /// with the canvas, the finger lift and the momentum included.
    #[test]
    fn a_flick_from_empty_canvas_keeps_panning_through_the_finger_lift_and_momentum() {
        let none = NSEventPhase::None;
        let flick = [
            (NSEventPhase::MayBegin, none),
            (NSEventPhase::Began, none),
            (NSEventPhase::Changed, none),
            (NSEventPhase::Changed, none),
            (NSEventPhase::Ended, none),
            (none, NSEventPhase::Began),
            (none, NSEventPhase::Changed),
            (none, NSEventPhase::Ended),
        ];
        let mut tracker = Tracker::<&str>::default();
        let mut no_replay = |_| panic!("no replay");
        let mut asked = 0;
        for (index, (phase, momentum)) in flick.iter().enumerate() {
            let stage = scroll_stage(*phase, *momentum);
            // Only the gesture starts ask; from the first Changed on, a panel sits under
            // the pointer and would answer false.
            let disposition = tracker.input(Input::Scroll(stage), "event", || {
                asked += 1;
                index < 2
            });
            assert_eq!(disposition, Disposition::Report, "event {index} {stage:?}");
            tracker.decide(ContentDecision::Moving, &mut no_replay);
        }
        assert_eq!(asked, 2, "MayBegin and Began each start the gesture");
        // Over: the next event is on its own again.
        assert_eq!(
            tracker.input(Input::Scroll(ScrollStage::Continued), "stray", || false),
            Disposition::Pass
        );
    }

    #[test]
    fn a_panel_scroll_gesture_stays_with_the_panel_until_it_ends() {
        let mut tracker = Tracker::<&str>::default();
        assert_eq!(
            tracker.input(Input::Scroll(ScrollStage::Began), "began", || false),
            Disposition::Pass
        );
        assert_eq!(
            tracker.input(Input::Scroll(ScrollStage::Continued), "changed", || {
                panic!("asked mid gesture")
            }),
            Disposition::Pass
        );
        assert_eq!(
            tracker.input(Input::Scroll(ScrollStage::Ended), "ended", || {
                panic!("asked at the end")
            }),
            Disposition::Pass
        );
        // A new gesture asks again and may go to the canvas.
        assert_eq!(
            tracker.input(Input::Scroll(ScrollStage::Began), "began2", || true),
            Disposition::Report
        );
    }

    #[test]
    fn middle_gesture_leaves_a_held_left_press_alone() {
        let mut tracker = Tracker::default();
        let mut sink = Vec::new();
        tracker.input(Input::LeftDown, "down", || true);
        tracker.decide(ContentDecision::Hold, &mut |e| sink.push(e));
        tracker.input(Input::MiddleDown, "mdown", || true);
        tracker.decide(ContentDecision::Click, &mut |e| sink.push(e));
        tracker.input(Input::MiddleUp, "mup", || unreachable!());
        tracker.decide(ContentDecision::Click, &mut |e| sink.push(e));
        tracker.input(Input::Scroll(ScrollStage::Wheel), "scroll", || true);
        tracker.decide(ContentDecision::Click, &mut |e| sink.push(e));
        assert!(sink.is_empty(), "replayed {sink:?}");
        // The left press is still held and still becomes a click.
        assert_eq!(
            tracker.input(Input::LeftUp, "up", || unreachable!()),
            Disposition::Report
        );
        tracker.decide(ContentDecision::Click, &mut |e| sink.push(e));
        assert_eq!(sink, ["down", "up"]);
    }

    #[test]
    fn a_canvas_scroll_gesture_leaves_a_held_left_press_alone() {
        let mut tracker = Tracker::default();
        let mut sink = Vec::new();
        tracker.input(Input::LeftDown, "down", || true);
        tracker.decide(ContentDecision::Hold, &mut |e| sink.push(e));
        for (stage, event) in [
            (ScrollStage::Began, "began"),
            (ScrollStage::Continued, "changed"),
            (ScrollStage::Ended, "ended"),
        ] {
            assert_eq!(
                tracker.input(Input::Scroll(stage), event, || true),
                Disposition::Report
            );
            tracker.decide(ContentDecision::Click, &mut |e| sink.push(e));
        }
        assert!(sink.is_empty(), "replayed {sink:?}");
        // The left press is still held and still becomes a click.
        assert_eq!(
            tracker.input(Input::LeftUp, "up", || unreachable!()),
            Disposition::Report
        );
        tracker.decide(ContentDecision::Click, &mut |e| sink.push(e));
        assert_eq!(sink, ["down", "up"]);
    }

    #[test]
    fn scroll_stages_follow_the_gesture_and_momentum_phases() {
        let none = NSEventPhase::None;
        let cases = [
            (none, none, ScrollStage::Wheel),
            (NSEventPhase::MayBegin, none, ScrollStage::Began),
            (NSEventPhase::Began, none, ScrollStage::Began),
            (NSEventPhase::Changed, none, ScrollStage::Continued),
            (NSEventPhase::Stationary, none, ScrollStage::Continued),
            // Fingers lifted: AppKit sends this alone, then momentum with an empty phase,
            // so the gesture is not over yet.
            (NSEventPhase::Ended, none, ScrollStage::Continued),
            (NSEventPhase::Cancelled, none, ScrollStage::Ended),
            (none, NSEventPhase::Began, ScrollStage::Continued),
            (none, NSEventPhase::Changed, ScrollStage::Continued),
            (none, NSEventPhase::Ended, ScrollStage::Ended),
            (none, NSEventPhase::Cancelled, ScrollStage::Ended),
            // Fingers lifted with momentum to follow: the gesture goes on.
            (
                NSEventPhase::Ended,
                NSEventPhase::Began,
                ScrollStage::Continued,
            ),
        ];
        for (phase, momentum, stage) in cases {
            assert_eq!(
                scroll_stage(phase, momentum),
                stage,
                "{phase:?} {momentum:?}"
            );
        }
        // The sequence AppKit really sends for a flick: the gesture ends only with the
        // momentum, so the owner set at Began holds through the finger lift.
        let flick = [
            (NSEventPhase::MayBegin, none),
            (NSEventPhase::Began, none),
            (NSEventPhase::Changed, none),
            (NSEventPhase::Ended, none),
            (none, NSEventPhase::Began),
            (none, NSEventPhase::Changed),
            (none, NSEventPhase::Ended),
        ];
        let stages: Vec<ScrollStage> = flick
            .iter()
            .map(|(phase, momentum)| scroll_stage(*phase, *momentum))
            .collect();
        assert_eq!(
            stages,
            [
                ScrollStage::Began,
                ScrollStage::Began,
                ScrollStage::Continued,
                ScrollStage::Continued,
                ScrollStage::Continued,
                ScrollStage::Continued,
                ScrollStage::Ended,
            ]
        );
    }

    #[test]
    fn window_points_flip_to_a_top_left_origin() {
        assert_eq!(top_left_point(10.0, 30.0, 400.0), Point::new(10.0, 370.0));
        assert_eq!(top_left_point(0.0, 400.0, 400.0), Point::new(0.0, 0.0));
    }
}
