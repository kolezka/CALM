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
use calm_core::menu::canvas_menu;
use calm_core::viewport::place_native;
use calm_native::{ContextMenu, NativeChild, WebPanel};
use objc2::MainThreadMarker;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSView, NSWindow,
    NSWindowStyleMask,
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
    let scale = window.backingScaleFactor();
    let viewport = Rect::new(0.0, 0.0, WIDTH, HEIGHT);
    let content = Rect::new(0.0, 0.0, 400.0, 300.0);
    let logical_h = content.size.height;

    // SAFETY: ns_view is the live zoom NSView owned by `child`, which outlives this borrow.
    let zoom: &NSView = unsafe { &*(child.ns_view() as *const NSView) };
    // SAFETY: a plain property read on the main thread; the clip view retains the zoom view.
    let clip = unsafe { zoom.superview() }.unwrap_or_else(|| fail("zoom view has no clip view"));

    // Stands in for gpui-libghostty: it places its view at y = NSHeight(parent.bounds) - y - h
    // without checking isFlipped. Here y = 0 and h = the logical height.
    let shim = NSView::initWithFrame(mtm.alloc::<NSView>(), NSRect::ZERO);
    zoom.addSubview(&shim);
    let shim_y = |zoom: &NSView| {
        let y = zoom.bounds().size.height - logical_h;
        shim.setFrame(NSRect::new(
            NSPoint::new(0.0, y),
            NSSize::new(content.size.width, logical_h),
        ));
        shim.frame().origin.y
    };

    // Unclipped: content lands at screen (30,30,600,450).
    let unclipped_camera = Camera {
        offset: Point::new(-20.0, -20.0),
        zoom: 1.5,
    };
    let unclipped = place_native(content, &unclipped_camera, viewport)
        .unwrap_or_else(|| fail("unclipped placement is None"));
    if unclipped.container.size.width != 600.0 || unclipped.container.size.height != 450.0 {
        fail("unclipped placement is clipped");
    }
    child.apply(&unclipped, HEIGHT, scale);
    let shim_unclipped = shim_y(zoom);

    // Content 400x300 at zoom 1.5 lands at screen (-60,-30), so the left and top are clipped.
    let camera = Camera {
        offset: Point::new(40.0, 20.0),
        zoom: 1.5,
    };
    let placement =
        place_native(content, &camera, viewport).unwrap_or_else(|| fail("placement is None"));
    child.apply(&placement, HEIGHT, scale);
    let shim_clipped = shim_y(zoom);

    let frame = clip.frame();
    println!(
        "native_smoke: clip frame={} zoom frame={} zoom bounds={}",
        fmt_rect(frame),
        fmt_rect(zoom.frame()),
        fmt_rect(zoom.bounds())
    );
    let expected_frame = NSRect::new(NSPoint::new(0.0, 180.0), NSSize::new(540.0, 420.0));
    if frame != expected_frame {
        fail(&format!(
            "clip frame {} != {}",
            fmt_rect(frame),
            fmt_rect(expected_frame)
        ));
    }

    // The zoom view is larger than the clip view here, so only clipsToBounds keeps it inside.
    let zf = zoom.frame();
    let oversized = zf.origin.x < 0.0
        || zf.origin.y < 0.0
        || zf.origin.x + zf.size.width > frame.size.width
        || zf.origin.y + zf.size.height > frame.size.height;
    if !oversized {
        fail("zoom view is not larger than the clip view, the clip check proves nothing");
    }
    let clips = clip.clipsToBounds();
    println!("native_smoke: clip={clips}");
    if !clips {
        fail("clip view does not clip to bounds");
    }

    println!("native_smoke: shim y unclipped={shim_unclipped} clipped={shim_clipped}");
    if shim_unclipped != 0.0 || shim_clipped != 0.0 {
        fail("Ghostty-style child is not at y = 0 in the zoom view");
    }
    println!("native_smoke: shim_compat=true");
    shim.removeFromSuperview();

    let mut web = WebPanel::new(&child).unwrap_or_else(|e| fail(&e.to_string()));
    web.set_logical_size(placement.logical_size);
    let web_frame = web.web_view().frame();
    if web_frame != NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(400.0, 300.0)) {
        fail(&format!("web view frame {}", fmt_rect(web_frame)));
    }

    // Logical point inner_origin (top-left based) must sit at the clip view's top-left corner.
    let inner = NSPoint::new(
        placement.inner_origin.x,
        logical_h - placement.inner_origin.y,
    );
    let in_window = zoom.convertPoint_toView(inner, None);
    let corner = NSPoint::new(frame.origin.x, frame.origin.y + frame.size.height);
    println!(
        "native_smoke: inner_origin maps to window=({},{}) clip top-left=({},{})",
        in_window.x, in_window.y, corner.x, corner.y
    );
    if (in_window.x - corner.x).abs() > 1e-6 || (in_window.y - corner.y).abs() > 1e-6 {
        fail("inner_origin is not at the clip view's top-left corner");
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

    // Built and read back, never popped up: popping up would block on user input.
    let sample = canvas_menu(
        &[("hello-world".to_string(), "Hello World".to_string())],
        &[],
        &[],
    );
    let menu = ContextMenu::build(&sample).unwrap_or_else(|e| fail(&e.to_string()));
    let (items, submenus) = menu.counts();
    println!("native_smoke: menu items={items} submenus={submenus}");
    // Add, then Terminal, Browser, a separator and Hello World inside it.
    if (items, submenus) != (5, 1) {
        fail("menu tree does not match the sample spec");
    }

    drop(web);
    drop(child);
    window.close();
    exit(0);
}
