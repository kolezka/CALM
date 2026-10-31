//! A browser panel: a native address bar with back, forward and reload above a WKWebView,
//! all inside one `NativeChild` zoom view, so the bar scales and clips with the page and
//! GPUI never draws over it.
//!
//! Unlike `WebPanel` there is no `calm` message handler: remote pages get no host access.

use std::cell::RefCell;
use std::ptr::NonNull;

use block2::RcBlock;
use calm_core::browser_url::normalize_address;
use calm_core::geometry::Size;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject, Sel};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSAutoresizingMaskOptions, NSBezelStyle, NSButton, NSControlSize, NSEvent,
    NSEventMask, NSEventModifierFlags, NSEventType, NSFont, NSTextField, NSView,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString, NSURL, NSURLRequest};
use objc2_web_kit::{
    WKNavigation, WKNavigationAction, WKNavigationDelegate, WKUIDelegate, WKWebView,
    WKWebViewConfiguration, WKWindowFeatures,
};

use crate::{NativeChild, NativeError};

/// Height of the address bar strip, in logical points.
pub const BROWSER_BAR_HEIGHT: f64 = 30.0;
const BAR_PADDING: f64 = 4.0;
const BUTTON_WIDTH: f64 = 28.0;
const CONTROL_HEIGHT: f64 = 22.0;
const FIELD_FONT_SIZE: f64 = 12.0;

/// What the page did, for the app to mirror in the panel chrome and focus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BrowserEvent {
    /// The page title after a load, possibly empty.
    TitleChanged(String),
    /// The committed URL of the main frame.
    UrlChanged(String),
    /// The user clicked inside the panel's native views.
    Clicked,
}

type EventCallback = Box<dyn FnMut(BrowserEvent)>;

struct ControlsIvars {
    web_view: Retained<WKWebView>,
    field: Retained<NSTextField>,
    callback: RefCell<Option<EventCallback>>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; MainThreadOnly keeps every call on
    // the main thread, where AppKit and WebKit deliver these messages.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "CalmBrowserControls"]
    #[ivars = ControlsIvars]
    struct Controls;

    impl Controls {
        #[unsafe(method(navigate:))]
        fn navigate(&self, _sender: Option<&AnyObject>) {
            let typed = self.ivars().field.stringValue().to_string();
            if let Some(url) = normalize_address(&typed) {
                load(&self.ivars().web_view, &url);
                focus_view(&self.ivars().web_view);
            }
        }

        #[unsafe(method(goBack:))]
        fn go_back(&self, _sender: Option<&AnyObject>) {
            // SAFETY: plain WebKit navigation on the main thread; a no-op without history.
            unsafe { self.ivars().web_view.goBack() };
        }

        #[unsafe(method(goForward:))]
        fn go_forward(&self, _sender: Option<&AnyObject>) {
            // SAFETY: as in go_back.
            unsafe { self.ivars().web_view.goForward() };
        }

        #[unsafe(method(reload:))]
        fn reload(&self, _sender: Option<&AnyObject>) {
            // SAFETY: as in go_back.
            unsafe { self.ivars().web_view.reload() };
        }
    }

    // SAFETY: NSObjectProtocol has no extra requirements.
    unsafe impl NSObjectProtocol for Controls {}

    // SAFETY: the method signatures match the protocol declarations.
    unsafe impl WKNavigationDelegate for Controls {
        #[unsafe(method(webView:didCommitNavigation:))]
        fn did_commit(&self, web_view: &WKWebView, _navigation: Option<&WKNavigation>) {
            if let Some(url) = current_url(web_view) {
                self.show_url(&url);
                self.emit(BrowserEvent::UrlChanged(url));
            }
        }

        #[unsafe(method(webView:didFinishNavigation:))]
        fn did_finish(&self, web_view: &WKWebView, _navigation: Option<&WKNavigation>) {
            // SAFETY: a plain property read on the main thread.
            let title = unsafe { web_view.title() }.map_or_else(String::new, |t| t.to_string());
            if let Some(url) = current_url(web_view) {
                self.show_url(&url);
            }
            self.emit(BrowserEvent::TitleChanged(title));
        }
    }

    // SAFETY: the method signature matches the protocol declaration.
    unsafe impl WKUIDelegate for Controls {
        /// target=_blank links and window.open load in this panel instead of a new window.
        #[unsafe(method(webView:createWebViewWithConfiguration:forNavigationAction:windowFeatures:))]
        fn create_web_view(
            &self,
            web_view: &WKWebView,
            _configuration: &WKWebViewConfiguration,
            action: &WKNavigationAction,
            _features: &WKWindowFeatures,
        ) -> *mut WKWebView {
            // SAFETY: request is always set on a navigation action; loading it on the main
            // thread is plain WebKit use.
            unsafe {
                let request = action.request();
                web_view.loadRequest(&request);
            }
            // Null means no new web view; WebKit drops the popup navigation.
            std::ptr::null_mut()
        }
    }
);

impl Controls {
    fn new(
        mtm: MainThreadMarker,
        web_view: Retained<WKWebView>,
        field: Retained<NSTextField>,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ControlsIvars {
            web_view,
            field,
            callback: RefCell::new(None),
        });
        // SAFETY: init is NSObject's designated initializer and the ivars are set.
        unsafe { msg_send![super(this), init] }
    }

    /// Shows `url` unless the user is typing in the field.
    fn show_url(&self, url: &str) {
        let field = &self.ivars().field;
        if !holds_first_responder(field) {
            field.setStringValue(&NSString::from_str(shown_address(url)));
        }
    }

    fn emit(&self, event: BrowserEvent) {
        // Take the callback out while it runs, so it may replace itself.
        let taken = self.ivars().callback.borrow_mut().take();
        if let Some(mut callback) = taken {
            callback(event);
            let mut slot = self.ivars().callback.borrow_mut();
            if slot.is_none() {
                *slot = Some(callback);
            }
        }
    }
}

/// The blank page shows an empty field, so the placeholder tells the user what to type.
fn shown_address(url: &str) -> &str {
    if url == "about:blank" { "" } else { url }
}

fn load(web_view: &WKWebView, url: &str) -> bool {
    let Some(ns_url) = NSURL::URLWithString(&NSString::from_str(url)) else {
        return false;
    };
    let request = NSURLRequest::requestWithURL(&ns_url);
    // SAFETY: loadRequest: with a valid request, on the main thread.
    unsafe { web_view.loadRequest(&request) };
    true
}

fn current_url(web_view: &WKWebView) -> Option<String> {
    // SAFETY: a plain property read on the main thread.
    let url = unsafe { web_view.URL() }?;
    url.absoluteString().map(|s| s.to_string())
}

fn focus_view(view: &NSView) {
    if let Some(window) = view.window() {
        window.makeFirstResponder(Some(view));
    }
}

/// True when the window's first responder is `view` or inside it. A text field's editor
/// is a subview of the field while editing, so this covers the address bar too.
fn holds_first_responder(view: &NSView) -> bool {
    let Some(window) = view.window() else {
        return false;
    };
    let Some(responder) = window.firstResponder() else {
        return false;
    };
    responder
        .downcast_ref::<NSView>()
        .is_some_and(|responder| responder.isDescendantOf(view))
}

/// A key the browser handles itself while it holds the keyboard. GPUI sees key
/// equivalents first and would otherwise drop them, and the app has no Edit menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BrowserKey {
    Back,
    Forward,
    Reload,
    FocusAddress,
    Edit(&'static str),
}

fn browser_key(chars: &str, shift: bool) -> Option<BrowserKey> {
    Some(match (chars, shift) {
        ("[", false) => BrowserKey::Back,
        ("]", false) => BrowserKey::Forward,
        ("r", false) => BrowserKey::Reload,
        ("l", false) => BrowserKey::FocusAddress,
        ("c", false) => BrowserKey::Edit("copy:"),
        ("x", false) => BrowserKey::Edit("cut:"),
        ("v", false) => BrowserKey::Edit("paste:"),
        ("a", false) => BrowserKey::Edit("selectAll:"),
        ("z", false) => BrowserKey::Edit("undo:"),
        ("z", true) => BrowserKey::Edit("redo:"),
        _ => return None,
    })
}

/// A live browser: address bar plus web view, in a `NativeChild`'s zoom view.
pub struct BrowserView {
    controls: Retained<Controls>,
    web_view: Retained<WKWebView>,
    field: Retained<NSTextField>,
    buttons: [Retained<NSButton>; 3],
    clip: Retained<NSView>,
    monitor: Option<Retained<AnyObject>>,
}

impl BrowserView {
    /// Adds the bar and a web view to `host`. Call `set_logical_size` before showing it.
    pub fn new(host: &NativeChild) -> Result<BrowserView, NativeError> {
        let mtm = MainThreadMarker::new().ok_or(NativeError::NotMainThread)?;
        let zero = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0));
        // SAFETY: plain WebKit object creation on the main thread.
        let web_view = unsafe {
            let config = WKWebViewConfiguration::new(mtm);
            WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), zero, &config)
        };
        web_view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewNotSizable);

        let field = NSTextField::textFieldWithString(&NSString::from_str(""), mtm);
        field.setPlaceholderString(Some(&NSString::from_str("Search or enter address")));
        field.setUsesSingleLineMode(true);
        field.setFont(Some(&NSFont::systemFontOfSize(FIELD_FONT_SIZE)));
        field.setAutoresizingMask(NSAutoresizingMaskOptions::ViewNotSizable);

        let controls = Controls::new(mtm, web_view.clone(), field.clone());
        let target: &AnyObject = &controls;
        let button = |title: &str, action: Sel| {
            // SAFETY: `controls` implements `action` and outlives the button (both are
            // owned by this BrowserView); the target is held weakly by AppKit.
            let button = unsafe {
                NSButton::buttonWithTitle_target_action(
                    &NSString::from_str(title),
                    Some(target),
                    Some(action),
                    mtm,
                )
            };
            button.setBezelStyle(NSBezelStyle::AccessoryBar);
            button.setControlSize(NSControlSize::Small);
            button.setAutoresizingMask(NSAutoresizingMaskOptions::ViewNotSizable);
            button
        };
        let buttons = [
            button("\u{2039}", sel!(goBack:)),
            button("\u{203A}", sel!(goForward:)),
            button("\u{21BB}", sel!(reload:)),
        ];
        // SAFETY: as for the buttons; Enter in the field sends `navigate:`. The delegates
        // are weak references to `controls`, which this BrowserView keeps alive.
        unsafe {
            field.setTarget(Some(target));
            field.setAction(Some(sel!(navigate:)));
            web_view.setNavigationDelegate(Some(ProtocolObject::from_ref(&*controls)));
            web_view.setUIDelegate(Some(ProtocolObject::from_ref(&*controls)));
        }

        let zoom_view = host.zoom_view();
        zoom_view.addSubview(&web_view);
        zoom_view.addSubview(&field);
        for button in &buttons {
            zoom_view.addSubview(button);
        }
        let clip = host.clip_view().clone();
        let mut view = BrowserView {
            controls,
            web_view,
            field,
            buttons,
            clip,
            monitor: None,
        };
        view.install_monitor();
        Ok(view)
    }

    /// Watches key and mouse-down events for this panel. Keys are handled only while this
    /// panel holds the keyboard; clicks are reported and always passed on.
    fn install_monitor(&mut self) {
        let controls = self.controls.clone();
        let clip = self.clip.clone();
        let handler = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
            // SAFETY: AppKit passes a live event for the duration of the handler.
            let event_ref = unsafe { event.as_ref() };
            if handle_event(&controls, &clip, event_ref) {
                std::ptr::null_mut()
            } else {
                event.as_ptr()
            }
        });
        let mask = NSEventMask::KeyDown | NSEventMask::LeftMouseDown;
        // SAFETY: the block returns the event or null as the API requires; the monitor is
        // removed in Drop, which releases the block and what it captured.
        self.monitor =
            unsafe { NSEvent::addLocalMonitorForEventsMatchingMask_handler(mask, &handler) };
    }

    /// The web view's frame size in logical points, as the last `set_logical_size` left it.
    pub fn page_size(&self) -> Size {
        let frame = self.web_view.frame();
        Size::new(frame.size.width, frame.size.height)
    }

    /// Lays out bar and page for a panel content of `size` logical points. The zoom view is
    /// unflipped, so the bar sits at the top of its bounds.
    pub fn set_logical_size(&mut self, size: Size) {
        let bar = BROWSER_BAR_HEIGHT.min(size.height);
        let page_height = (size.height - bar).max(0.0);
        self.web_view
            .setFrame(rect(0.0, 0.0, size.width, page_height));
        let control_y = page_height + (bar - CONTROL_HEIGHT).max(0.0) / 2.0;
        let mut x = BAR_PADDING;
        for button in &self.buttons {
            button.setFrame(rect(x, control_y, BUTTON_WIDTH, CONTROL_HEIGHT));
            x += BUTTON_WIDTH + BAR_PADDING / 2.0;
        }
        let field_width = (size.width - x - BAR_PADDING).max(0.0);
        self.field
            .setFrame(rect(x, control_y, field_width, CONTROL_HEIGHT));
    }

    /// Loads an address bar input (URL, bare host or search). Returns false for blank or
    /// unparsable input.
    pub fn open(&mut self, input: &str) -> bool {
        let Some(url) = normalize_address(input) else {
            return false;
        };
        self.field
            .setStringValue(&NSString::from_str(shown_address(&url)));
        load(&self.web_view, &url)
    }

    /// Replaces the callback for page events.
    pub fn on_event(&mut self, callback: Box<dyn FnMut(BrowserEvent)>) {
        *self.controls.ivars().callback.borrow_mut() = Some(callback);
    }

    /// Gives the page the keyboard.
    pub fn focus_page(&self) {
        focus_view(&self.web_view);
    }

    /// Puts the keyboard in the address bar with its text selected.
    pub fn focus_address(&self) {
        focus_view(&self.field);
        // SAFETY: a nil sender is allowed, on the main thread.
        unsafe { self.field.selectText(None) };
    }

    /// True while the page or the address bar has the keyboard.
    pub fn has_keyboard(&self) -> bool {
        holds_first_responder(&self.clip)
    }

    pub fn is_loading(&self) -> bool {
        // SAFETY: a plain property read on the main thread.
        unsafe { self.web_view.isLoading() }
    }

    pub fn url(&self) -> Option<String> {
        current_url(&self.web_view)
    }
}

/// True when the event was consumed.
fn handle_event(controls: &Controls, clip: &NSView, event: &NSEvent) -> bool {
    if event.r#type() == NSEventType::LeftMouseDown {
        if clicked_inside(clip, event) {
            controls.emit(BrowserEvent::Clicked);
        }
        return false;
    }
    let flags = event.modifierFlags();
    let command = flags.contains(NSEventModifierFlags::Command);
    let other = flags.intersects(NSEventModifierFlags::Control | NSEventModifierFlags::Option);
    if !command || other || !holds_first_responder(clip) {
        return false;
    }
    let Some(chars) = event.charactersIgnoringModifiers() else {
        return false;
    };
    let chars = chars.to_string().to_lowercase();
    let shift = flags.contains(NSEventModifierFlags::Shift);
    let Some(key) = browser_key(&chars, shift) else {
        return false;
    };
    let ivars = controls.ivars();
    // SAFETY: plain WebKit navigation on the main thread.
    match key {
        BrowserKey::Back => unsafe {
            ivars.web_view.goBack();
        },
        BrowserKey::Forward => unsafe {
            ivars.web_view.goForward();
        },
        BrowserKey::Reload => unsafe {
            ivars.web_view.reload();
        },
        BrowserKey::FocusAddress => {
            focus_view(&ivars.field);
            // SAFETY: as in BrowserView::focus_address.
            unsafe { ivars.field.selectText(None) };
        }
        BrowserKey::Edit(selector) => {
            let mtm = MainThreadMarker::from(clip);
            let action = Sel::register(&std::ffi::CString::new(selector).expect("no NUL"));
            // SAFETY: a nil target sends the standard edit action to the first responder,
            // which is inside this panel; responders that do not implement it ignore it.
            return unsafe {
                NSApplication::sharedApplication(mtm).sendAction_to_from(action, None, None)
            };
        }
    }
    true
}

fn clicked_inside(clip: &NSView, event: &NSEvent) -> bool {
    let mtm = MainThreadMarker::from(clip);
    let (Some(window), Some(clip_window)) = (event.window(mtm), clip.window()) else {
        return false;
    };
    if window != clip_window || clip.isHidden() {
        return false;
    }
    let Some(content) = window.contentView() else {
        return false;
    };
    // hitTest takes a point in the superview's space, which for the content view is the
    // window's frame view, so window coordinates.
    content
        .hitTest(event.locationInWindow())
        .is_some_and(|hit| hit.isDescendantOf(clip))
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
}

impl Drop for BrowserView {
    fn drop(&mut self) {
        if let Some(monitor) = self.monitor.take() {
            // SAFETY: the monitor came from addLocalMonitorForEventsMatchingMask:handler:.
            unsafe { NSEvent::removeMonitor(&monitor) };
        }
        // SAFETY: clearing delegates we set, on the main thread.
        unsafe {
            self.web_view.setNavigationDelegate(None);
            self.web_view.setUIDelegate(None);
        }
        self.web_view.removeFromSuperview();
        self.field.removeFromSuperview();
        for button in &self.buttons {
            button.removeFromSuperview();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn browser_keys_map_navigation_and_edit_commands() {
        assert_eq!(browser_key("[", false), Some(BrowserKey::Back));
        assert_eq!(browser_key("]", false), Some(BrowserKey::Forward));
        assert_eq!(browser_key("r", false), Some(BrowserKey::Reload));
        assert_eq!(browser_key("l", false), Some(BrowserKey::FocusAddress));
        assert_eq!(browser_key("v", false), Some(BrowserKey::Edit("paste:")));
        assert_eq!(browser_key("z", true), Some(BrowserKey::Edit("redo:")));
        // cmd-= and friends stay with the canvas.
        assert_eq!(browser_key("=", false), None);
        assert_eq!(browser_key("r", true), None);
    }
}
