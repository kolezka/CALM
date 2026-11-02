//! A markdown panel's page: a WKWebView in a `NativeChild` zoom view that shows HTML
//! rendered by `calm_core::markdown`. It never navigates away: only CALM's own load and
//! jumps inside the page stay, a clicked http, https or mailto link opens in the system
//! default app, everything else is dropped.
//!
//! No `calm` message handler: the page gets no host access. The view draws no background
//! of its own, so a page background with opacity below 1 shows the canvas through it.

use std::cell::RefCell;
use std::rc::Rc;

use block2::{DynBlock, RcBlock};
use calm_core::geometry::Size;
use calm_core::markdown::{Navigation, NavigationRequest, NavigationType, navigation};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSAutoresizingMaskOptions, NSView, NSWorkspace};
use objc2_foundation::{NSError, NSNumber, NSPoint, NSRect, NSSize, NSString, NSURL};
use objc2_web_kit::{
    WKNavigationAction, WKNavigationActionPolicy, WKNavigationDelegate, WKNavigationType,
    WKWebView, WKWebViewConfiguration,
};

use crate::child::{ns_rect, zoom_bounds};
use crate::{NativeChild, NativeError};

define_class!(
    // SAFETY: NSObject has no subclassing requirements; MainThreadOnly keeps every call on
    // the main thread, where WebKit delivers navigation delegate messages.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "CalmMarkdownNavigation"]
    struct LinkPolicy;

    // SAFETY: NSObjectProtocol has no extra requirements.
    unsafe impl NSObjectProtocol for LinkPolicy {}

    // SAFETY: the method signature matches the protocol declaration.
    unsafe impl WKNavigationDelegate for LinkPolicy {
        #[unsafe(method(webView:decidePolicyForNavigationAction:decisionHandler:))]
        fn decide_policy(
            &self,
            web_view: &WKWebView,
            action: &WKNavigationAction,
            decision_handler: &DynBlock<dyn Fn(WKNavigationActionPolicy)>,
        ) {
            let url_text = |url: Option<Retained<NSURL>>| {
                url.and_then(|url| url.absoluteString())
                    .map(|url| url.to_string())
            };
            // SAFETY: request and navigationType are always set on a navigation action;
            // targetFrame is nil for a new window, which `is_some_and` handles. Called on
            // the main thread, where WebKit delivers delegate messages.
            let (url, kind, main_frame) = unsafe {
                let kind = match action.navigationType() {
                    WKNavigationType::LinkActivated => NavigationType::LinkActivated,
                    WKNavigationType::FormSubmitted | WKNavigationType::FormResubmitted => {
                        NavigationType::FormSubmitted
                    }
                    WKNavigationType::BackForward => NavigationType::BackForward,
                    WKNavigationType::Reload => NavigationType::Reload,
                    _ => NavigationType::Other,
                };
                (
                    url_text(action.request().URL()).unwrap_or_default(),
                    kind,
                    action
                        .targetFrame()
                        .is_some_and(|frame| frame.isMainFrame()),
                )
            };
            // SAFETY: a plain property read on the main thread; nil before the first load.
            let current = url_text(unsafe { web_view.URL() });
            let request = NavigationRequest {
                url: &url,
                kind,
                main_frame,
                current: current.as_deref(),
            };
            let policy = match navigation(&request) {
                Navigation::Allow => WKNavigationActionPolicy::Allow,
                Navigation::OpenExternally(url) => {
                    open_externally(&url);
                    WKNavigationActionPolicy::Cancel
                }
                Navigation::Block => WKNavigationActionPolicy::Cancel,
            };
            decision_handler.call((policy,));
        }
    }
);

impl LinkPolicy {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        // SAFETY: NSObject's designated initializer on a fresh instance.
        unsafe { msg_send![super(this), init] }
    }
}

/// Hands `url` to the default browser or mail app.
fn open_externally(url: &str) {
    let Some(ns_url) = NSURL::URLWithString(&NSString::from_str(url)) else {
        eprintln!("calm: markdown link {url:?} is not a URL");
        return;
    };
    if !NSWorkspace::sharedWorkspace().openURL(&ns_url) {
        eprintln!("calm: markdown link {url:?} did not open");
    }
}

/// A markdown page in a zoom view.
pub struct MarkdownView {
    web_view: Retained<WKWebView>,
    // The web view holds its navigation delegate weakly, so the panel keeps it.
    _policy: Retained<LinkPolicy>,
    zoom_view: Retained<NSView>,
    probe: Rc<RefCell<Option<String>>>,
}

impl MarkdownView {
    /// Adds the web view to the container. Call `set_logical_size` before showing it.
    pub fn new(host: &NativeChild) -> Result<MarkdownView, NativeError> {
        let mtm = MainThreadMarker::new().ok_or(NativeError::NotMainThread)?;
        let policy = LinkPolicy::new(mtm);
        // SAFETY: plain WebKit object creation and setup on the main thread.
        let web_view = unsafe {
            let config = WKWebViewConfiguration::new(mtm);
            let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0));
            let web_view =
                WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), frame, &config);
            web_view.setNavigationDelegate(Some(ProtocolObject::from_ref(&*policy)));
            web_view
        };
        set_draws_background(&web_view, false);
        web_view.setAutoresizingMask(NSAutoresizingMaskOptions::ViewNotSizable);
        let zoom_view = host.zoom_view().clone();
        zoom_view.addSubview(&web_view);
        Ok(MarkdownView {
            web_view,
            _policy: policy,
            zoom_view,
            probe: Rc::new(RefCell::new(None)),
        })
    }

    /// As `WebPanel::set_logical_size`: only a resize changes it.
    pub fn set_logical_size(&mut self, size: Size) {
        let rect = ns_rect(zoom_bounds(size));
        self.zoom_view.setBounds(rect);
        self.web_view.setFrame(rect);
    }

    /// Replaces the page with `html`, in the same web view.
    pub fn load_html(&mut self, html: &str) {
        // SAFETY: a nil base URL is allowed; the page loads as about:blank.
        unsafe {
            self.web_view
                .loadHTMLString_baseURL(&NSString::from_str(html), None)
        };
    }

    pub fn set_visible(&mut self, visible: bool) {
        self.web_view.setHidden(!visible);
    }

    pub fn is_loading(&self) -> bool {
        // SAFETY: a plain property read on the main thread.
        unsafe { self.web_view.isLoading() }
    }

    /// Whether WebKit paints its own opaque background under the page. Read back
    /// through key-value coding, as it was set.
    pub fn draws_background(&self) -> Option<bool> {
        // SAFETY: WKWebView answers the `drawsBackground` key on macOS 14 with an NSNumber.
        let value: Option<Retained<AnyObject>> = unsafe {
            msg_send![&*self.web_view, valueForKey: &*NSString::from_str("drawsBackground")]
        };
        let number = value?.downcast::<NSNumber>().ok()?;
        Some(number.boolValue())
    }

    /// Runs `script` and keeps its string result for `probe_result`. A failed run
    /// stores `error: ...`.
    pub fn probe(&mut self, script: &str) {
        *self.probe.borrow_mut() = None;
        let slot = self.probe.clone();
        let block = RcBlock::new(move |result: *mut AnyObject, error: *mut NSError| {
            // SAFETY: WebKit passes either a valid result object or a valid error, each
            // alive for the duration of this call, or null.
            let text = unsafe {
                if let Some(error) = error.as_ref() {
                    format!("error: {}", error.localizedDescription())
                } else if let Some(result) = result.as_ref() {
                    match result.downcast_ref::<NSString>() {
                        Some(string) => string.to_string(),
                        None => "error: not a string".to_string(),
                    }
                } else {
                    "error: no result".to_string()
                }
            };
            *slot.borrow_mut() = Some(text);
        });
        // SAFETY: the block lives as long as WebKit retains it; called on the main thread.
        unsafe {
            self.web_view
                .evaluateJavaScript_completionHandler(&NSString::from_str(script), Some(&block))
        };
    }

    pub fn probe_result(&self) -> Option<String> {
        self.probe.borrow().clone()
    }

    pub fn focus(&self) -> bool {
        match self.web_view.window() {
            Some(window) => window.makeFirstResponder(Some(&self.web_view)),
            None => false,
        }
    }
}

/// Turns WebKit's own background on or off through key-value coding, the documented way
/// to make a WKWebView see-through on macOS.
fn set_draws_background(web_view: &WKWebView, draws: bool) {
    let value = NSNumber::new_bool(draws);
    let key = NSString::from_str("drawsBackground");
    // SAFETY: WKWebView is KVC compliant for `drawsBackground`, a BOOL; KVC unboxes the
    // NSNumber. Called on the main thread.
    let _: () = unsafe { msg_send![web_view, setValue: &*value, forKey: &*key] };
    // Also clear the color shown past the page edges when it bounces.
    // SAFETY: a plain property write on the main thread; macOS 12 and later.
    unsafe { web_view.setUnderPageBackgroundColor(Some(&objc2_app_kit::NSColor::clearColor())) };
}

impl Drop for MarkdownView {
    fn drop(&mut self) {
        // SAFETY: clearing a delegate we set, on the main thread.
        unsafe { self.web_view.setNavigationDelegate(None) };
        self.web_view.removeFromSuperview();
    }
}
