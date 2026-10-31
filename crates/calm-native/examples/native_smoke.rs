//! GPUI-free AppKit check for calm-native: one window, one container, one web view.
//! Prints `native_smoke:` lines and exits 0, or exits 1 with the reason.

use std::cell::RefCell;
use std::ffi::c_void;
use std::process::exit;
use std::ptr::NonNull;
use std::rc::Rc;
use std::time::{Duration, Instant};

use calm_core::camera::Camera;
use calm_core::geometry::{Point, Rect};
use calm_core::viewport::place_native;
use calm_native::{NativeChild, WebPanel};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSDate, NSPoint, NSRect, NSRunLoop, NSSize};
use raw_window_handle::{
    AppKitWindowHandle, HandleError, HasWindowHandle, RawWindowHandle, WindowHandle,
};

const WIDTH: f64 = 800.0;
const HEIGHT: f64 = 600.0;

/// Stands in for the GPUI window: an AppKit handle whose ns_view is the content view.
struct ContentViewHandle {
    ns_view: NonNull<c_void>,
}

impl HasWindowHandle for ContentViewHandle {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let raw = RawWindowHandle::AppKit(AppKitWindowHandle::new(self.ns_view));
        // SAFETY: the window, and so its content view, lives until main returns.
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

fn fail(reason: &str) -> ! {
    eprintln!("native_smoke: FAIL {reason}");
    exit(1);
}

fn fmt_rect(r: NSRect) -> String {
    format!(
        "{},{},{},{}",
        r.origin.x, r.origin.y, r.size.width, r.size.height
    )
}

/// Spins the main run loop until `done` or the timeout.
fn pump_until(timeout: Duration, mut done: impl FnMut() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if done() {
            return true;
        }
        let until = NSDate::dateWithTimeIntervalSinceNow(0.05);
        NSRunLoop::currentRunLoop().runUntilDate(&until);
    }
    done()
}

fn main() {
    let mtm = MainThreadMarker::new().unwrap_or_else(|| fail("not on the main thread"));
    let app = NSApplication::sharedApplication(mtm);
    app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);

    let content_rect = NSRect::new(NSPoint::new(100.0, 100.0), NSSize::new(WIDTH, HEIGHT));
    // SAFETY: standard NSWindow initializer on the main thread.
    let window: Retained<NSWindow> = unsafe {
        NSWindow::initWithContentRect_styleMask_backing_defer(
            mtm.alloc::<NSWindow>(),
            content_rect,
            NSWindowStyleMask::Titled | NSWindowStyleMask::Closable,
            NSBackingStoreType::Buffered,
            false,
        )
    };
    // SAFETY: we own the window through Retained, so AppKit must not release it on close.
    unsafe { window.setReleasedWhenClosed(false) };
    window.orderFrontRegardless();

    let content_view = window
        .contentView()
        .unwrap_or_else(|| fail("window has no content view"));
    let handle = ContentViewHandle {
        ns_view: NonNull::from(&*content_view).cast(),
    };

    let mut child = NativeChild::new(&handle).unwrap_or_else(|e| fail(&e.to_string()));

    // Content 400x300 at zoom 1.5 lands at screen (-60,-30), so the left and top are clipped.
    let camera = Camera {
        offset: Point::new(40.0, 20.0),
        zoom: 1.5,
    };
    let viewport = Rect::new(0.0, 0.0, WIDTH, HEIGHT);
    let content = Rect::new(0.0, 0.0, 400.0, 300.0);
    let placement =
        place_native(content, &camera, viewport).unwrap_or_else(|| fail("placement is None"));
    let scale = window.backingScaleFactor();
    child.apply(&placement, HEIGHT, scale);

    // SAFETY: ns_view is the live container NSView owned by `child`.
    let container: &objc2_app_kit::NSView =
        unsafe { &*(child.ns_view() as *const objc2_app_kit::NSView) };
    let frame = container.frame();
    let bounds = container.bounds();
    println!(
        "native_smoke: container frame={} bounds={}",
        fmt_rect(frame),
        fmt_rect(bounds)
    );
    let expected_frame = NSRect::new(NSPoint::new(0.0, 180.0), NSSize::new(540.0, 420.0));
    if frame != expected_frame {
        fail(&format!(
            "frame {} != {}",
            fmt_rect(frame),
            fmt_rect(expected_frame)
        ));
    }

    let mut web = WebPanel::new(&child).unwrap_or_else(|e| fail(&e.to_string()));
    web.set_logical_size(placement.logical_size);

    // The content point at inner_origin must sit at the container's top-left corner.
    let inner = NSPoint::new(placement.inner_origin.x, placement.inner_origin.y);
    let in_window = web.web_view().convertPoint_toView(inner, None);
    let corner = NSPoint::new(frame.origin.x, frame.origin.y + frame.size.height);
    println!(
        "native_smoke: inner_origin maps to window=({},{}) container top-left=({},{})",
        in_window.x, in_window.y, corner.x, corner.y
    );
    if (in_window.x - corner.x).abs() > 1e-6 || (in_window.y - corner.y).abs() > 1e-6 {
        fail("inner_origin is not at the container's top-left corner");
    }

    let messages: Rc<RefCell<Vec<String>>> = Rc::default();
    let sink = Rc::clone(&messages);
    web.on_message(Box::new(move |text| sink.borrow_mut().push(text)));
    web.add_user_script_at_document_start("window.__calmPrelude = 'prelude-ok';");

    if !web.load_url("about:blank") {
        fail("about:blank did not parse");
    }
    let started = web.is_loading();
    let loaded = pump_until(Duration::from_secs(10), || !web.is_loading());
    if !loaded {
        fail("web view still loading after 10 s");
    }
    println!("native_smoke: webview loaded=true (was loading after load_url: {started})");

    web.eval_js(
        "window.webkit.messageHandlers.calm.postMessage('ping:' + String(window.__calmPrelude))",
    );
    let got = pump_until(Duration::from_secs(10), || !messages.borrow().is_empty());
    if !got {
        fail("no message from the page within 10 s");
    }
    let first = messages.borrow()[0].clone();
    println!("native_smoke: bridge message={first}");
    if first != "ping:prelude-ok" {
        fail("bridge message or document-start script did not match");
    }

    drop(web);
    drop(child);
    window.close();
    exit(0);
}
