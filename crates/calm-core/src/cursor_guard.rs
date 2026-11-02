//! When a cursor set may be skipped. With a customized system pointer (Accessibility pointer
//! colors or size) every `-[NSCursor set]` renders a new cursor image, about 4 ms on an M4 Pro,
//! even when the cursor is already showing. AppKit sets the canvas cursor again on every
//! display cycle that changes tracking regions, so a pan or zoom paid that on every frame.
//!
//! A set is skipped only when it repeats AppKit's current cursor and nothing outside this
//! process can have changed the visible cursor since the last real set. The native side calls
//! `invalidate` when that may have happened: mouse events in another app, app activation
//! changes, or the pointer leaving the window.

/// State of the guard, one per process.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CursorSetGuard {
    /// The last real set is still what the screen shows.
    trusted: bool,
}

impl CursorSetGuard {
    /// True when a set can be skipped. `enabled` is the setting, `is_current` says the cursor
    /// being set is AppKit's current cursor, `pointer_inside` that the pointer is over one of
    /// this app's windows.
    pub fn should_skip(&self, enabled: bool, is_current: bool, pointer_inside: bool) -> bool {
        enabled && is_current && pointer_inside && self.trusted
    }

    /// Records a set that reached AppKit. Only a set made while the pointer is over this app's
    /// windows is trusted: outside them another process may draw its own cursor.
    pub fn did_set(&mut self, pointer_inside: bool) {
        self.trusted = pointer_inside;
    }

    /// Something outside this process may have changed the cursor; the next set goes through.
    pub fn invalidate(&mut self) {
        self.trusted = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn trusted() -> CursorSetGuard {
        let mut guard = CursorSetGuard::default();
        guard.did_set(true);
        guard
    }

    #[test]
    fn repeating_the_current_cursor_after_a_real_set_is_skipped() {
        let guard = trusted();
        assert!(guard.should_skip(true, true, true));
        assert!(guard.should_skip(true, true, true));
    }

    #[test]
    fn a_different_cursor_always_goes_through() {
        assert!(!trusted().should_skip(true, false, true));
    }

    #[test]
    fn the_first_set_goes_through() {
        assert!(!CursorSetGuard::default().should_skip(true, true, true));
    }

    #[test]
    fn after_invalidate_one_set_goes_through_then_repeats_skip() {
        let mut guard = trusted();
        guard.invalidate();
        assert!(!guard.should_skip(true, true, true));
        guard.did_set(true);
        assert!(guard.should_skip(true, true, true));
    }

    #[test]
    fn leaving_the_window_and_coming_back_sets_the_cursor_again() {
        let mut guard = trusted();
        // Outside the window nothing is skipped, and a set made there is not trusted.
        assert!(!guard.should_skip(true, true, false));
        guard.did_set(false);
        // Back inside, the first set goes through even if AppKit thinks it is current.
        assert!(!guard.should_skip(true, true, true));
        guard.did_set(true);
        assert!(guard.should_skip(true, true, true));
    }

    #[test]
    fn disabled_never_skips() {
        assert!(!trusted().should_skip(false, true, true));
    }
}
