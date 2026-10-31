use std::cell::RefCell;
use std::path::Path;

use calm_core::geometry::Size;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString, NSURL, NSURLRequest};
use objc2_web_kit::{
    WKScriptMessage, WKScriptMessageHandler, WKUserContentController, WKUserScript,
    WKUserScriptInjectionTime, WKWebView, WKWebViewConfiguration,
};

use crate::{NativeChild, NativeError};

pub const DEFAULT_BROWSER_URL: &str = "https://ghostty.org";

/// JS posts with `window.webkit.messageHandlers.calm.postMessage(...)`.
const MESSAGE_HANDLER_NAME: &str = "calm";

type MessageCallback = Box<dyn FnMut(String)>;

#[derive(Default)]
struct HandlerIvars {
    callback: RefCell<Option<MessageCallback>>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements; the handler is only used on the
    // main thread, which MainThreadOnly enforces.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "CalmScriptMessageHandler"]
    #[ivars = HandlerIvars]
    struct MessageHandler;

    // SAFETY: NSObjectProtocol has no extra requirements.
    unsafe impl NSObjectProtocol for MessageHandler {}

    // SAFETY: the method signature matches the protocol declaration.
    unsafe impl WKScriptMessageHandler for MessageHandler {
        #[unsafe(method(userContentController:didReceiveScriptMessage:))]
        fn did_receive_script_message(
            &self,
            _controller: &WKUserContentController,
            message: &WKScriptMessage,
        ) {
            // SAFETY: body is always set on a delivered message.
            let body = unsafe { message.body() };
            let text = match body.downcast_ref::<NSString>() {
                Some(string) => string.to_string(),
                None => body_description(&body),
            };
            // Take the callback out while it runs, so it may call on_message again.
            let taken = self.ivars().callback.borrow_mut().take();
            if let Some(mut callback) = taken {
                callback(text);
                let mut slot = self.ivars().callback.borrow_mut();
                if slot.is_none() {
                    *slot = Some(callback);
                }
            }
        }
    }
);

fn body_description(body: &AnyObject) -> String {
    // SAFETY: every object reachable from a script message body is an NSObject subclass
    // (NSNumber, NSArray, NSDictionary, NSNull), which responds to description.
    let description: Retained<NSString> = unsafe { msg_send![body, description] };
    description.to_string()
}

impl MessageHandler {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(HandlerIvars::default());
        // SAFETY: init is NSObject's designated initializer and the ivars are set.
        unsafe { msg_send![super(this), init] }
    }
}

/// A WKWebView inside a `NativeChild` container.
pub struct WebPanel {
    web_view: Retained<WKWebView>,
    controller: Retained<WKUserContentController>,
    handler: Retained<MessageHandler>,
}

impl WebPanel {
    /// Adds a web view to the container. Call `set_logical_size` before showing it.
    pub fn new(host: &NativeChild) -> Result<WebPanel, NativeError> {
        let mtm = MainThreadMarker::new().ok_or(NativeError::NotMainThread)?;
        let handler = MessageHandler::new(mtm);
        // SAFETY: plain WebKit object creation and setup on the main thread.
        let (web_view, controller) = unsafe {
            let config = WKWebViewConfiguration::new(mtm);
            let controller = config.userContentController();
            controller.addScriptMessageHandler_name(
                ProtocolObject::from_ref(&*handler),
                &NSString::from_str(MESSAGE_HANDLER_NAME),
            );
            let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0));
            let web_view =
                WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), frame, &config);
            (web_view, controller)
        };
        // No autoresizing: the container frame shrinks when clipped, the page must not.
        web_view.setAutoresizingMask(objc2_app_kit::NSAutoresizingMaskOptions::ViewNotSizable);
        host.container().addSubview(&web_view);
        Ok(WebPanel {
            web_view,
            controller,
            handler,
        })
    }

    /// Lays the page out at `(0, 0, size)` in content units. Only a panel resize changes it.
    pub fn set_logical_size(&mut self, size: Size) {
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(size.width, size.height));
        self.web_view.setFrame(frame);
    }

    /// Loads `url`. An unparsable URL is ignored and returns false.
    pub fn load_url(&mut self, url: &str) -> bool {
        let Some(ns_url) = NSURL::URLWithString(&NSString::from_str(url)) else {
            return false;
        };
        let request = NSURLRequest::requestWithURL(&ns_url);
        // SAFETY: loadRequest: with a valid request, on the main thread.
        unsafe { self.web_view.loadRequest(&request) };
        true
    }

    /// Loads a local file and lets the page read files under `read_access_dir`.
    pub fn load_file(&mut self, entry: &Path, read_access_dir: &Path) {
        let entry_url = NSURL::fileURLWithPath(&NSString::from_str(&entry.to_string_lossy()));
        let dir_url = NSURL::fileURLWithPath_isDirectory(
            &NSString::from_str(&read_access_dir.to_string_lossy()),
            true,
        );
        // SAFETY: both are file URLs, on the main thread.
        unsafe {
            self.web_view
                .loadFileURL_allowingReadAccessToURL(&entry_url, &dir_url)
        };
    }

    /// Runs `script` in the page. The result is discarded.
    pub fn eval_js(&mut self, script: &str) {
        // SAFETY: a nil completion handler is allowed.
        unsafe {
            self.web_view
                .evaluateJavaScript_completionHandler(&NSString::from_str(script), None)
        };
    }

    /// Replaces the callback for messages posted to the `calm` handler.
    pub fn on_message(&mut self, callback: Box<dyn FnMut(String)>) {
        *self.handler.ivars().callback.borrow_mut() = Some(callback);
    }

    /// Injects `source` into the main frame before page scripts run, for every later load.
    pub fn add_user_script_at_document_start(&mut self, source: &str) {
        let mtm = MainThreadMarker::from(&*self.web_view);
        // SAFETY: plain WKUserScript creation on the main thread.
        unsafe {
            let script = WKUserScript::initWithSource_injectionTime_forMainFrameOnly(
                WKUserScript::alloc(mtm),
                &NSString::from_str(source),
                WKUserScriptInjectionTime::AtDocumentStart,
                true,
            );
            self.controller.addUserScript(&script);
        }
    }

    pub fn set_visible(&mut self, visible: bool) {
        self.web_view.setHidden(!visible);
    }

    pub fn is_loading(&self) -> bool {
        // SAFETY: a plain property read on the main thread.
        unsafe { self.web_view.isLoading() }
    }

    /// The web view, for tests and diagnostics.
    pub fn web_view(&self) -> &WKWebView {
        &self.web_view
    }
}

impl Drop for WebPanel {
    fn drop(&mut self) {
        // The controller retains the handler; remove it to break the cycle.
        // SAFETY: removing a handler we registered, on the main thread.
        unsafe {
            self.controller
                .removeScriptMessageHandlerForName(&NSString::from_str(MESSAGE_HANDLER_NAME))
        };
        self.web_view.removeFromSuperview();
    }
}
