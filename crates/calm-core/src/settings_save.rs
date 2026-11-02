//! When a settings change reaches disk, and which app state a change touches. Pure: the
//! caller passes the time in and does the work, so every rule is tested without a clock.

use std::time::{Duration, Instant};

use crate::settings::Settings;

/// A slider drag or color well fires a change per tick, and each durable write costs
/// milliseconds on the main thread. This holds the last change until the drag goes quiet.
#[derive(Clone, Debug, Default)]
pub struct SaveDebounce {
    last_change: Option<Instant>,
}

impl SaveDebounce {
    pub fn note_change(&mut self, now: Instant) {
        self.last_change = Some(now);
    }

    pub fn is_pending(&self) -> bool {
        self.last_change.is_some()
    }

    /// True once `delay` has passed since the last change; that change is then taken.
    pub fn take_due(&mut self, now: Instant, delay: Duration) -> bool {
        match self.last_change {
            Some(at) if now.saturating_duration_since(at) >= delay => {
                self.last_change = None;
                true
            }
            _ => false,
        }
    }

    /// True if a change waits, which is then taken: for a window close, a quit, or a
    /// change that saves at once.
    pub fn take_pending(&mut self) -> bool {
        self.last_change.take().is_some()
    }
}

/// Whether the key bindings or the menu bar read anything that changed. Rebuilding both
/// costs about a millisecond, too much for every slider tick.
pub fn keymap_affected(old: &Settings, new: &Settings) -> bool {
    old.keybindings != new.keybindings || old.grid.show != new.grid.show
}

#[cfg(test)]
mod tests {
    use super::*;

    const DELAY: Duration = Duration::from_millis(400);

    /// Runs `ticks` changes 16 ms apart, polling as the app's timers do after each one,
    /// then polls once more after the quiet time. Returns how many writes happened.
    fn drag(debounce: &mut SaveDebounce, ticks: u32) -> (u32, Instant) {
        let start = Instant::now();
        let mut writes = 0;
        let mut now = start;
        for tick in 0..ticks {
            now = start + Duration::from_millis(16 * u64::from(tick));
            debounce.note_change(now);
            if debounce.take_due(now, DELAY) {
                writes += 1;
            }
            // A timer from an earlier tick fires mid-drag.
            if debounce.take_due(now + Duration::from_millis(8), DELAY) {
                writes += 1;
            }
        }
        if debounce.take_due(now + DELAY, DELAY) {
            writes += 1;
        }
        (writes, now)
    }

    #[test]
    fn a_drag_of_many_changes_writes_once_after_it_goes_quiet() {
        let mut debounce = SaveDebounce::default();
        let (writes, _) = drag(&mut debounce, 60);
        assert_eq!(writes, 1);
        assert!(!debounce.is_pending());
    }

    #[test]
    fn nothing_is_written_before_the_quiet_time() {
        let mut debounce = SaveDebounce::default();
        let now = Instant::now();
        debounce.note_change(now);
        assert!(!debounce.take_due(now + DELAY - Duration::from_millis(1), DELAY));
        assert!(debounce.is_pending());
        assert!(debounce.take_due(now + DELAY, DELAY));
        assert!(
            !debounce.take_due(now + DELAY * 2, DELAY),
            "taken only once"
        );
    }

    #[test]
    fn a_flush_takes_the_last_change_and_leaves_nothing_for_the_timer() {
        let mut debounce = SaveDebounce::default();
        let now = Instant::now();
        assert!(!debounce.take_pending(), "nothing to flush yet");
        debounce.note_change(now);
        debounce.note_change(now + Duration::from_millis(16));
        assert!(debounce.take_pending());
        assert!(!debounce.take_due(now + DELAY * 2, DELAY));
        assert!(!debounce.take_pending());
    }

    #[test]
    fn a_zero_delay_writes_every_change() {
        let mut debounce = SaveDebounce::default();
        let now = Instant::now();
        for tick in 0..5 {
            let at = now + Duration::from_millis(16 * tick);
            debounce.note_change(at);
            assert!(debounce.take_due(at, Duration::ZERO));
        }
    }

    #[test]
    fn grid_and_background_changes_leave_the_keymap_alone() {
        let old = Settings::default();
        let mut new = old.clone();
        new.grid.opacity = 0.9;
        new.grid.spacing = 40.0;
        new.grid.color = crate::background::Rgb(0x123456);
        new.background.gradient.angle_deg = 45.0;
        new.terminal.default_profile = Some("work".into());
        assert!(!keymap_affected(&old, &new));
    }

    #[test]
    fn key_and_menu_check_mark_changes_rebuild_the_keymap() {
        let old = Settings::default();
        let mut grid = old.clone();
        grid.grid.show = !grid.grid.show;
        assert!(keymap_affected(&old, &grid));
        let mut keys = old.clone();
        keys.keybindings.insert("fit_all".into(), None);
        assert!(keymap_affected(&old, &keys));
    }
}
