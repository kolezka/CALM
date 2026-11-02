//! Skips `-[NSCursor set]` calls that repeat the cursor already showing. With a customized
//! system pointer every set renders a new cursor image (about 4 ms each), and AppKit sets the
//! canvas cursor again on every display cycle that changes tracking regions. The decision is
//! `calm_core::cursor_guard`; this module swaps the method in this process only and feeds it.

use std::cell::Cell;
use std::ptr::NonNull;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering};
use std::time::Instant;

use block2::RcBlock;
use calm_core::cursor_guard::CursorSetGuard;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Imp, Sel};
use objc2::{ClassType, MainThreadMarker, sel};
use objc2_app_kit::{NSApplication, NSCursor, NSEvent, NSEventMask, NSEventType, NSWindow};

type SetFn = unsafe extern "C-unwind" fn(*mut AnyObject, Sel);

static ENABLED: AtomicBool = AtomicBool::new(false);
static ORIGINAL: OnceLock<SetFn> = OnceLock::new();
static CALLS: AtomicU64 = AtomicU64::new(0);
static SKIPPED: AtomicU64 = AtomicU64::new(0);
static SET_NANOS: AtomicU64 = AtomicU64::new(0);
static MONITORS: AtomicU8 = AtomicU8::new(MONITORS_NOT_INSTALLED);
const MONITORS_NOT_INSTALLED: u8 = 0;
const MONITORS_ACTIVE: u8 = 1;
const MONITORS_FAILED: u8 = 2;

thread_local! {
    static GUARD: Cell<CursorSetGuard> = Cell::new(CursorSetGuard::default());
}

/// Counters since launch: set calls seen, calls skipped, and time spent in real sets.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct CursorSetCounts {
    pub calls: u64,
    pub skipped: u64,
    pub set_nanos: u64,
}

pub fn counts() -> CursorSetCounts {
    CursorSetCounts {
        calls: CALLS.load(Ordering::Relaxed),
        skipped: SKIPPED.load(Ordering::Relaxed),
        set_nanos: SET_NANOS.load(Ordering::Relaxed),
    }
}

/// Whether the event monitors the guard relies on are registered: "active", "failed" (the
/// guard then never skips) or "not installed".
pub fn monitors_status() -> &'static str {
    match MONITORS.load(Ordering::Relaxed) {
        MONITORS_ACTIVE => "active",
        MONITORS_FAILED => "failed",
        _ => "not installed",
    }
}

/// Turns skipping on or off. The first call with `true` installs the guard; once installed
/// it stays and only passes calls through while off, so turning it off needs no restart.
pub fn set_enabled(enabled: bool) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    if enabled {
        install(mtm);
    }
    ENABLED.store(enabled, Ordering::Relaxed);
    invalidate();
}

/// Installs the guard without enabling it, so the zoom bench can count calls either way.
pub fn install(mtm: MainThreadMarker) {
    install_on_main(mtm);
}

/// `install` for callers without a marker; does nothing off the main thread.
pub fn install_here() {
    if let Some(mtm) = MainThreadMarker::new() {
        install_on_main(mtm);
    }
}

fn install_on_main(mtm: MainThreadMarker) {
    if ORIGINAL.get().is_some() {
        return;
    }
    let Some(method) = NSCursor::class().instance_method(sel!(set)) else {
        return;
    };
    // SAFETY: `-[NSCursor set]` takes no arguments and returns void, so its implementation
    // has the `SetFn` signature.
    let original: SetFn = unsafe { std::mem::transmute::<Imp, SetFn>(method.implementation()) };
    // The original is stored before the swap, so the guard never runs without it.
    let _ = ORIGINAL.set(original);
    // SAFETY: `guarded_set` has the same signature as the method it replaces, and it calls
    // the original for every set it does not skip. Only this process's NSCursor changes.
    unsafe {
        method.set_implementation(std::mem::transmute::<SetFn, Imp>(guarded_set));
    }
    let active = install_monitors(mtm);
    if !active {
        eprintln!(
            "calm: cursor guard off: an event monitor failed to register, so every cursor set goes through"
        );
    }
    MONITORS.store(
        if active {
            MONITORS_ACTIVE
        } else {
            MONITORS_FAILED
        },
        Ordering::Relaxed,
    );
    GUARD.with(|guard| {
        let mut state = guard.get();
        state.set_monitors_active(active);
        guard.set(state);
    });
}

fn invalidate() {
    GUARD.with(|guard| {
        let mut state = guard.get();
        state.invalidate();
        guard.set(state);
    });
}

/// True when the pointer is over one of this app's windows.
fn pointer_inside(mtm: MainThreadMarker) -> bool {
    let number =
        NSWindow::windowNumberAtPoint_belowWindowWithWindowNumber(NSEvent::mouseLocation(), 0, mtm);
    NSApplication::sharedApplication(mtm)
        .windowWithWindowNumber(number)
        .is_some()
}

unsafe extern "C-unwind" fn guarded_set(this: *mut AnyObject, cmd: Sel) {
    CALLS.fetch_add(1, Ordering::Relaxed);
    let inside = MainThreadMarker::new().map(pointer_inside);
    if let Some(inside) = inside {
        let current = NSCursor::currentCursor();
        let is_current = std::ptr::eq(Retained::as_ptr(&current).cast::<AnyObject>(), this);
        let enabled = ENABLED.load(Ordering::Relaxed);
        if GUARD.with(|guard| guard.get().should_skip(enabled, is_current, inside)) {
            SKIPPED.fetch_add(1, Ordering::Relaxed);
            return;
        }
    }
    let Some(original) = ORIGINAL.get() else {
        return;
    };
    let start = Instant::now();
    // SAFETY: the receiver and selector are the ones AppKit passed for this set.
    unsafe { original(this, cmd) };
    SET_NANOS.fetch_add(start.elapsed().as_nanos() as u64, Ordering::Relaxed);
    GUARD.with(|guard| {
        let mut state = guard.get();
        state.did_set(inside == Some(true));
        guard.set(state);
    });
}

/// Mouse events in other apps and app activation changes mean another process may have
/// drawn its own cursor, so the next set must reach AppKit. False when either monitor failed.
fn install_monitors(_mtm: MainThreadMarker) -> bool {
    let global = RcBlock::new(|_event: NonNull<NSEvent>| invalidate());
    let global_mask = NSEventMask::MouseMoved
        | NSEventMask::LeftMouseDragged
        | NSEventMask::RightMouseDragged
        | NSEventMask::LeftMouseDown
        | NSEventMask::RightMouseDown
        | NSEventMask::ScrollWheel;
    let local = RcBlock::new(|event: NonNull<NSEvent>| -> *mut NSEvent {
        // SAFETY: AppKit passes a live event for the duration of the handler.
        if unsafe { event.as_ref() }.r#type() == NSEventType::AppKitDefined {
            invalidate();
        }
        event.as_ptr()
    });
    // SAFETY: the handlers match the documented block types. Both monitors live for the rest
    // of the process, so they are leaked on purpose with their blocks.
    let (global, local) = unsafe {
        (
            NSEvent::addGlobalMonitorForEventsMatchingMask_handler(global_mask, &global),
            NSEvent::addLocalMonitorForEventsMatchingMask_handler(
                NSEventMask::AppKitDefined,
                &local,
            ),
        )
    };
    let active = global.is_some() && local.is_some();
    std::mem::forget(global);
    std::mem::forget(local);
    active
}
