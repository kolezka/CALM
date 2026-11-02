//! The Settings window: a separate native NSWindow that draws a `calm_core::settings_form`
//! with AppKit controls. Native, so it never sits under a Ghostty or WKWebView view, and
//! the color wells bring the system color picker for free.
//!
//! `WindowLayout::Tabs` puts each section in its own scrolling tab; `Single` stacks them in
//! one scroll view.
//!
//! Every control targets one helper object. Its tag indexes the bindings list, which says
//! which field id it edits and how to read its value.

use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::ptr::NonNull;

use block2::RcBlock;
use calm_core::background::Rgb;
use calm_core::keymap::{KeyParts, display_key, keystroke_from_parts};
use calm_core::settings_form::{Control, Field, FieldState, FieldValue, Section};
use objc2::rc::{Retained, Weak};
use objc2::runtime::{AnyObject, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSAutoresizingMaskOptions, NSBackingStoreType, NSButton, NSColor, NSColorSpace,
    NSColorWell, NSControl, NSEvent, NSEventMask, NSEventModifierFlags, NSEventType, NSFont,
    NSModalResponseOK, NSOpenPanel, NSPopUpButton, NSScrollView, NSSlider, NSTabView,
    NSTabViewItem, NSTextField, NSView, NSWindow, NSWindowDidResignKeyNotification,
    NSWindowOrderingMode, NSWindowStyleMask,
};
use objc2_foundation::{
    NSInteger, NSNotification, NSNotificationCenter, NSOperationQueue, NSPoint, NSRect, NSSize,
    NSString,
};
use raw_window_handle::HasWindowHandle;

use crate::NativeError;

const WINDOW_WIDTH: f64 = 680.0;
const WINDOW_HEIGHT: f64 = 640.0;
const MARGIN: f64 = 20.0;
const ROW_HEIGHT: f64 = 30.0;
const HEADER_HEIGHT: f64 = 34.0;
const LABEL_WIDTH: f64 = 190.0;
const CONTROL_X: f64 = MARGIN + LABEL_WIDTH + 10.0;
const CONTROL_WIDTH: f64 = 230.0;
const VALUE_X: f64 = CONTROL_X + CONTROL_WIDTH + 8.0;
const VALUE_WIDTH: f64 = 70.0;
const STATE_X: f64 = VALUE_X + VALUE_WIDTH;
const STATE_WIDTH: f64 = 76.0;
const INHERITED_LABEL: &str = "inherited";
const RESET_TITLE: &str = "Reset";
/// The recorder's title while it waits for a key.
pub const RECORDING_TITLE: &str = "Press a shortcut…";
pub const CLEAR_TITLE: &str = "Clear";
/// The recorder's title for an unbound action.
pub const UNBOUND_TITLE: &str = "None";

/// What the recorder does with one key-down.
#[derive(Debug, PartialEq, Eq)]
pub enum RecorderStep {
    Cancel,
    Ignore,
    Report(String),
}

/// Escape with no modifier cancels; no key ignores; anything else reports.
pub fn recorder_step(parts: &KeyParts) -> RecorderStep {
    let bare = !(parts.cmd || parts.ctrl || parts.alt || parts.shift || parts.function);
    if bare && parts.chars_ignoring_modifiers == "\u{1b}" {
        return RecorderStep::Cancel;
    }
    match keystroke_from_parts(parts) {
        Some(key) => RecorderStep::Report(key),
        None => RecorderStep::Ignore,
    }
}

/// The parts of one key-down, given its flags, `charactersIgnoringModifiers` and a lookup
/// of the key code's characters under given flags.
pub fn parts_from(
    flags: NSEventModifierFlags,
    chars_ignoring: &str,
    chars_with: impl Fn(NSEventModifierFlags) -> String,
    always_use_cmd_layout: bool,
) -> KeyParts {
    KeyParts {
        cmd: flags.contains(NSEventModifierFlags::Command),
        ctrl: flags.contains(NSEventModifierFlags::Control),
        alt: flags.contains(NSEventModifierFlags::Option),
        shift: flags.contains(NSEventModifierFlags::Shift),
        function: flags.contains(NSEventModifierFlags::Function),
        chars_ignoring_modifiers: chars_ignoring.to_string(),
        chars_plain: chars_with(NSEventModifierFlags::empty()),
        chars_shift: chars_with(NSEventModifierFlags::Shift),
        chars_cmd: chars_with(NSEventModifierFlags::Command),
        chars_cmd_shift: chars_with(NSEventModifierFlags::Command | NSEventModifierFlags::Shift),
        always_use_cmd_layout,
    }
}

/// A click in the Settings window while recording: on the recorder it toggles there,
/// anywhere else it cancels, so a key meant for another field is never recorded.
pub fn recorder_click_step(on_recorder: bool) -> RecorderStep {
    if on_recorder {
        RecorderStep::Ignore
    } else {
        RecorderStep::Cancel
    }
}

/// The recorder's title for a key: macOS symbols, or `UNBOUND_TITLE`.
pub fn recorder_title(value: Option<&str>) -> String {
    value.map_or_else(|| UNBOUND_TITLE.to_string(), display_key)
}

/// GPUI's `always_use_command_layout` (events.rs:499-505): a non-ASCII layout whose Cmd
/// layer is ASCII. Read from a key-code-0 event that is built but never posted.
fn always_use_cmd_layout() -> bool {
    let chars = |flags: NSEventModifierFlags| -> Option<String> {
        let empty = NSString::from_str("");
        let event = NSEvent::keyEventWithType_location_modifierFlags_timestamp_windowNumber_context_characters_charactersIgnoringModifiers_isARepeat_keyCode(
            NSEventType::KeyDown,
            NSPoint::new(0.0, 0.0),
            NSEventModifierFlags::empty(),
            0.0,
            0,
            None,
            &empty,
            &empty,
            false,
            0,
        )?;
        Some(event.charactersByApplyingModifiers(flags)?.to_string())
    };
    match chars(NSEventModifierFlags::empty()) {
        Some(plain) if !plain.is_ascii() => {
            chars(NSEventModifierFlags::Command).is_some_and(|cmd| cmd.is_ascii())
        }
        _ => false,
    }
}

/// How a settings window arranges its sections.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowLayout {
    /// One tab per section, each scrolling on its own.
    Tabs,
    /// Every section in one scroll view.
    Single,
}

type ChangeCallback = Box<dyn FnMut(String, FieldValue)>;

enum BindingKind {
    Checkbox,
    Slider {
        decimals: u8,
        value_label: Retained<NSTextField>,
    },
    Color,
    Choice,
    Button,
    Text,
    Reset,
    /// Click starts recording. Keeps the button so a message can replace its title.
    KeyRecorder {
        value: Option<String>,
        button: Retained<NSButton>,
    },
    KeyClear,
}

/// A recorder waiting for a key. Dropping it does not stop it; `stop_recording` does.
struct Recording {
    id: String,
    button: Retained<NSButton>,
    title: String,
    monitor: Retained<AnyObject>,
    observer: Retained<ProtocolObject<dyn NSObjectProtocol>>,
    /// Set once a key or a focus loss decided the outcome, while the stop is queued.
    finishing: Cell<bool>,
}

struct Binding {
    id: String,
    kind: BindingKind,
}

#[derive(Default)]
struct TargetIvars {
    callback: RefCell<Option<ChangeCallback>>,
    pending: RefCell<VecDeque<(String, FieldValue)>>,
    bindings: RefCell<Vec<Binding>>,
    recording: RefCell<Option<Recording>>,
    window: RefCell<Option<Weak<NSWindow>>>,
}

define_class!(
    // SAFETY: NSObject has no subclassing requirements and the class adds no Drop impl.
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "CALMSettingsTarget"]
    #[ivars = TargetIvars]
    struct FormTarget;

    impl FormTarget {
        #[unsafe(method(calmChanged:))]
        fn changed(&self, sender: &NSControl) {
            if let Ok(index) = usize::try_from(sender.tag())
                && self.is_recorder(index)
            {
                self.toggle_recording(index);
                return;
            }
            let Some((id, value)) = self.read(sender) else {
                return;
            };
            self.ivars().pending.borrow_mut().push_back((id, value));
            self.flush();
        }
    }
);

impl FormTarget {
    /// Hands every queued change to the callback.
    fn flush(&self) {
        // Take the callback out while it runs, so it may rebuild the form.
        let taken = self.ivars().callback.borrow_mut().take();
        if let Some(mut callback) = taken {
            loop {
                let change = self.ivars().pending.borrow_mut().pop_front();
                let Some((id, value)) = change else {
                    break;
                };
                callback(id, value);
            }
            *self.ivars().callback.borrow_mut() = Some(callback);
        }
    }

    fn is_recorder(&self, index: usize) -> bool {
        matches!(
            self.ivars().bindings.borrow().get(index).map(|b| &b.kind),
            Some(BindingKind::KeyRecorder { .. })
        )
    }

    /// A click on a recorder: stops it when it is the one recording, else starts it.
    fn toggle_recording(&self, index: usize) {
        let (id, button, title) = {
            let bindings = self.ivars().bindings.borrow();
            let Some(Binding {
                id,
                kind: BindingKind::KeyRecorder { value, button },
            }) = bindings.get(index)
            else {
                return;
            };
            (id.clone(), button.clone(), recorder_title(value.as_deref()))
        };
        let same = self
            .ivars()
            .recording
            .borrow()
            .as_ref()
            .map(|r| r.id.clone())
            == Some(id.clone());
        self.stop_recording();
        if same {
            return;
        }
        let Some(window) = self.ivars().window.borrow().as_ref().and_then(Weak::load) else {
            return;
        };
        button.setTitle(&NSString::from_str(RECORDING_TITLE));
        let number = window.windowNumber();
        let target = Weak::new(self);
        let handler = RcBlock::new(move |event: NonNull<NSEvent>| -> *mut NSEvent {
            // SAFETY: AppKit passes a live event for the duration of the handler.
            let event_ref = unsafe { event.as_ref() };
            let Some(target) = target.load() else {
                return event.as_ptr();
            };
            if event_ref.windowNumber() != number {
                return event.as_ptr();
            }
            if target.handle_recorded_key(event_ref) {
                std::ptr::null_mut()
            } else {
                event.as_ptr()
            }
        });
        // SAFETY: the block returns the event or null as the API requires; the monitor is
        // removed in `stop_recording`, which releases the block and what it captured.
        let monitor = unsafe {
            NSEvent::addLocalMonitorForEventsMatchingMask_handler(
                NSEventMask::KeyDown | NSEventMask::LeftMouseDown,
                &handler,
            )
        };
        let Some(monitor) = monitor else {
            button.setTitle(&NSString::from_str(&title));
            return;
        };
        let target = Weak::new(self);
        let on_resign = RcBlock::new(move |_: NonNull<NSNotification>| {
            if let Some(target) = target.load() {
                target.finish_recording(None);
            }
        });
        // SAFETY: a nil queue runs the block on the posting thread; AppKit posts window
        // notifications on the main thread, which this class is bound to. The observer is
        // removed in `stop_recording`.
        let observer = unsafe {
            NSNotificationCenter::defaultCenter().addObserverForName_object_queue_usingBlock(
                Some(NSWindowDidResignKeyNotification),
                Some(window.as_ref() as &AnyObject),
                None,
                &on_resign,
            )
        };
        *self.ivars().recording.borrow_mut() = Some(Recording {
            id,
            button,
            title,
            monitor,
            observer,
            finishing: Cell::new(false),
        });
    }

    /// The monitor's view of one key-down or click in this window. True swallows it.
    fn handle_recorded_key(&self, event: &NSEvent) -> bool {
        let on_recorder = match self.ivars().recording.borrow().as_ref() {
            None => return false,
            Some(recording) if recording.finishing.get() => {
                return event.r#type() == NSEventType::KeyDown;
            }
            Some(recording) => {
                let button = &recording.button;
                // A button on a tab that is not shown has no window, so it was not hit.
                button.window().is_some() && {
                    let point = button.convertPoint_fromView(event.locationInWindow(), None);
                    let bounds = button.bounds();
                    point.x >= bounds.origin.x
                        && point.y >= bounds.origin.y
                        && point.x <= bounds.origin.x + bounds.size.width
                        && point.y <= bounds.origin.y + bounds.size.height
                }
            }
        };
        if event.r#type() == NSEventType::LeftMouseDown {
            // Clicks always go on: a tab or field click must still work.
            if recorder_click_step(on_recorder) == RecorderStep::Cancel {
                self.finish_recording(None);
            }
            return false;
        }
        let chars_ignoring = event
            .charactersIgnoringModifiers()
            .map(|s| s.to_string())
            .unwrap_or_default();
        let parts = parts_from(
            event.modifierFlags(),
            &chars_ignoring,
            |flags| {
                event
                    .charactersByApplyingModifiers(flags)
                    .map(|s| s.to_string())
                    .unwrap_or_default()
            },
            always_use_cmd_layout(),
        );
        match recorder_step(&parts) {
            RecorderStep::Ignore => {}
            RecorderStep::Cancel => self.finish_recording(None),
            RecorderStep::Report(key) => self.finish_recording(Some(key)),
        }
        true
    }

    /// Ends recording after the current event: removing a monitor or observer from inside
    /// its own block would free the block while it runs.
    fn finish_recording(&self, key: Option<String>) {
        let id = match self.ivars().recording.borrow().as_ref() {
            Some(recording) if !recording.finishing.get() => {
                recording.finishing.set(true);
                recording.id.clone()
            }
            _ => return,
        };
        let target = Weak::new(self);
        let done = RcBlock::new(move || {
            let Some(target) = target.load() else {
                return;
            };
            target.stop_recording();
            if let Some(key) = &key {
                target
                    .ivars()
                    .pending
                    .borrow_mut()
                    .push_back((id.clone(), FieldValue::Key(Some(key.clone()))));
                target.flush();
            }
        });
        // SAFETY: the main queue runs the block on the main thread after this event.
        unsafe { NSOperationQueue::mainQueue().addOperationWithBlock(&done) };
    }

    /// Removes the monitor and observer and gives the recorder its title back.
    fn stop_recording(&self) {
        let Some(recording) = self.ivars().recording.borrow_mut().take() else {
            return;
        };
        // SAFETY: the monitor came from addLocalMonitorForEventsMatchingMask:handler:.
        unsafe { NSEvent::removeMonitor(&recording.monitor) };
        // SAFETY: the observer came from addObserverForName:object:queue:usingBlock: on
        // the default center.
        unsafe {
            NSNotificationCenter::defaultCenter()
                .removeObserver(recording.observer.as_ref() as &AnyObject)
        };
        recording
            .button
            .setTitle(&NSString::from_str(&recording.title));
    }

    fn recorder(&self, id: &str) -> Option<(Retained<NSButton>, Option<String>)> {
        self.ivars()
            .bindings
            .borrow()
            .iter()
            .find_map(|b| match &b.kind {
                BindingKind::KeyRecorder { value, button } if b.id == id => {
                    Some((button.clone(), value.clone()))
                }
                _ => None,
            })
    }
}

define_class!(
    // SAFETY: NSView has no subclassing requirements; this only flips the coordinates so
    // rows lay out top down.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "CALMSettingsFlippedView"]
    struct FlippedView;

    impl FlippedView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

impl FormTarget {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(TargetIvars::default());
        // SAFETY: plain NSObject init on a freshly allocated instance with ivars set.
        unsafe { msg_send![super(this), init] }
    }

    fn read(&self, sender: &NSControl) -> Option<(String, FieldValue)> {
        let bindings = self.ivars().bindings.borrow();
        let binding = bindings.get(usize::try_from(sender.tag()).ok()?)?;
        let value = match &binding.kind {
            BindingKind::Checkbox => {
                let button = sender.downcast_ref::<NSButton>()?;
                FieldValue::Bool(button.state() != 0)
            }
            BindingKind::Slider {
                decimals,
                value_label,
            } => {
                let value = sender.doubleValue();
                value_label.setStringValue(&NSString::from_str(&format_value(value, *decimals)));
                FieldValue::Number(value)
            }
            BindingKind::Color => {
                let well = sender.downcast_ref::<NSColorWell>()?;
                FieldValue::Color(rgb_from_ns(&well.color())?)
            }
            BindingKind::Choice => {
                let popup = sender.downcast_ref::<NSPopUpButton>()?;
                FieldValue::Index(usize::try_from(popup.indexOfSelectedItem()).ok()?)
            }
            BindingKind::Button => FieldValue::Pressed,
            BindingKind::Text => FieldValue::Text(sender.stringValue().to_string()),
            BindingKind::Reset => FieldValue::Reset,
            // A click on a recorder starts recording in `changed`; it reports no value.
            BindingKind::KeyRecorder { .. } => return None,
            BindingKind::KeyClear => FieldValue::Key(None),
        };
        Some((binding.id.clone(), value))
    }
}

pub fn format_value(value: f64, decimals: u8) -> String {
    format!("{value:.*}", usize::from(decimals))
}

fn rgb_from_ns(color: &NSColor) -> Option<Rgb> {
    let srgb = color.colorUsingColorSpace(&NSColorSpace::sRGBColorSpace())?;
    Some(Rgb::from_unit(
        srgb.redComponent(),
        srgb.greenComponent(),
        srgb.blueComponent(),
    ))
}

fn ns_from_rgb(color: Rgb) -> Retained<NSColor> {
    let [r, g, b] = color.to_unit();
    NSColor::colorWithSRGBRed_green_blue_alpha(r, g, b, 1.0)
}

fn rect(x: f64, y: f64, width: f64, height: f64) -> NSRect {
    NSRect::new(NSPoint::new(x, y), NSSize::new(width, height))
}

fn label(mtm: MainThreadMarker, text: &str, frame: NSRect) -> Retained<NSTextField> {
    let field = NSTextField::labelWithString(&NSString::from_str(text), mtm);
    field.setFrame(frame);
    field
}

enum Body {
    Single(Retained<NSScrollView>),
    Tabs(Retained<NSTabView>),
}

fn make_scroll(mtm: MainThreadMarker, frame: NSRect) -> Retained<NSScrollView> {
    let scroll = NSScrollView::initWithFrame(NSScrollView::alloc(mtm), frame);
    scroll.setHasVerticalScroller(true);
    scroll.setAutoresizingMask(
        NSAutoresizingMaskOptions::ViewWidthSizable | NSAutoresizingMaskOptions::ViewHeightSizable,
    );
    scroll
}

/// A settings window. Hidden, not destroyed, when the user closes it.
pub struct SettingsWindow {
    mtm: MainThreadMarker,
    window: Retained<NSWindow>,
    body: Body,
    target: Retained<FormTarget>,
}

impl SettingsWindow {
    /// `on_change` runs on the main thread inside the AppKit action. Do not block in it.
    pub fn new(
        title: &str,
        layout: WindowLayout,
        on_change: impl FnMut(String, FieldValue) + 'static,
    ) -> Result<SettingsWindow, NativeError> {
        let mtm = MainThreadMarker::new().ok_or(NativeError::NotMainThread)?;
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable;
        let frame = rect(0.0, 0.0, WINDOW_WIDTH, WINDOW_HEIGHT);
        // SAFETY: standard designated initializer with a valid rect, mask and backing.
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                frame,
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // SAFETY: we keep the window retained, so AppKit must not release it on close.
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(&NSString::from_str(title));
        window.setMinSize(NSSize::new(WINDOW_WIDTH, 320.0));

        let body = match layout {
            WindowLayout::Single => {
                let scroll = make_scroll(mtm, frame);
                window.setContentView(Some(&scroll));
                Body::Single(scroll)
            }
            WindowLayout::Tabs => {
                let tabs = NSTabView::initWithFrame(NSTabView::alloc(mtm), frame);
                tabs.setAutoresizingMask(
                    NSAutoresizingMaskOptions::ViewWidthSizable
                        | NSAutoresizingMaskOptions::ViewHeightSizable,
                );
                window.setContentView(Some(&tabs));
                Body::Tabs(tabs)
            }
        };
        window.center();

        let target = FormTarget::new(mtm);
        *target.ivars().callback.borrow_mut() = Some(Box::new(on_change));
        *target.ivars().window.borrow_mut() = Some(Weak::from_retained(&window));
        Ok(SettingsWindow {
            mtm,
            window,
            body,
            target,
        })
    }

    /// Replaces every control with a fresh build of `sections`. In tabs mode the selected
    /// tab stays selected by title, else the first tab is selected.
    pub fn set_form(&self, sections: &[Section]) {
        self.target.stop_recording();
        // Commit an edit in progress while the old bindings still match its tag.
        self.window.makeFirstResponder(None);
        for document in self.documents() {
            for view in document.subviews().iter() {
                // An active well keeps feeding the shared color panel after removal.
                if let Some(well) = view.downcast_ref::<NSColorWell>() {
                    well.deactivate();
                }
            }
        }
        let mut bindings = Vec::new();
        match &self.body {
            Body::Single(scroll) => {
                let document = self.build_document(sections, true, &mut bindings);
                *self.target.ivars().bindings.borrow_mut() = bindings;
                scroll.setDocumentView(Some(&document));
            }
            Body::Tabs(tabs) => {
                let selected = self.selected_tab();
                let mut items = Vec::new();
                for section in sections {
                    let document =
                        self.build_document(std::slice::from_ref(section), false, &mut bindings);
                    let scroll = make_scroll(self.mtm, tabs.contentRect());
                    scroll.setDocumentView(Some(&document));
                    let item = NSTabViewItem::new();
                    item.setLabel(&NSString::from_str(&section.title));
                    item.setView(Some(&scroll));
                    items.push(item);
                }
                *self.target.ivars().bindings.borrow_mut() = bindings;
                for old in tabs.tabViewItems().iter() {
                    tabs.removeTabViewItem(&old);
                }
                for item in &items {
                    tabs.addTabViewItem(item);
                }
                let index = selected
                    .and_then(|title| sections.iter().position(|s| s.title == title))
                    .unwrap_or(0);
                if !items.is_empty() {
                    tabs.selectTabViewItemAtIndex(index as NSInteger);
                }
            }
        }
    }

    /// The document views now shown, one per tab in tabs mode.
    fn documents(&self) -> Vec<Retained<NSView>> {
        match &self.body {
            Body::Single(scroll) => scroll.documentView().into_iter().collect(),
            Body::Tabs(tabs) => tabs
                .tabViewItems()
                .iter()
                .filter_map(|item| item.view(self.mtm))
                .filter_map(|view| {
                    view.downcast_ref::<NSScrollView>()
                        .and_then(|scroll| scroll.documentView())
                })
                .collect(),
        }
    }

    fn build_document(
        &self,
        sections: &[Section],
        headers: bool,
        bindings: &mut Vec<Binding>,
    ) -> Retained<FlippedView> {
        let mtm = self.mtm;
        let rows: usize = sections.iter().map(|s| s.fields.len()).sum();
        let header_rows = if headers { sections.len() } else { 0 };
        let height = MARGIN * 2.0 + header_rows as f64 * HEADER_HEIGHT + rows as f64 * ROW_HEIGHT;
        let document = FlippedView::alloc(mtm).set_ivars(());
        // SAFETY: NSView's designated initializer on a fresh instance.
        let document: Retained<FlippedView> = unsafe {
            msg_send![super(document), initWithFrame: rect(0.0, 0.0, WINDOW_WIDTH, height)]
        };
        document.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);

        let mut y = MARGIN;
        for section in sections {
            if headers {
                let header = label(mtm, &section.title, rect(MARGIN, y + 8.0, 400.0, 20.0));
                header.setFont(Some(&NSFont::boldSystemFontOfSize(13.0)));
                document.addSubview(&header);
                y += HEADER_HEIGHT;
            }
            for field in &section.fields {
                self.build_row(field, y, &document, bindings);
                y += ROW_HEIGHT;
            }
        }
        document
    }

    fn build_row(&self, field: &Field, y: f64, document: &NSView, bindings: &mut Vec<Binding>) {
        let mtm = self.mtm;
        if !field.label.is_empty() {
            let name = label(mtm, &field.label, rect(MARGIN, y + 5.0, LABEL_WIDTH, 18.0));
            document.addSubview(&name);
        }
        if let Control::Key { value, .. } = &field.control {
            self.build_key_row(&field.id, value.as_deref(), y, document, bindings);
        } else if let Some((control, kind)) = self.build_control(&field.control, y, document) {
            self.bind(&control, &field.id, kind, document, bindings);
        }
        match field.state {
            FieldState::Plain => {}
            FieldState::Inherited => {
                let note = label(
                    mtm,
                    INHERITED_LABEL,
                    rect(STATE_X, y + 6.0, STATE_WIDTH, 16.0),
                );
                note.setFont(Some(&NSFont::systemFontOfSize(
                    NSFont::smallSystemFontSize(),
                )));
                note.setTextColor(Some(&NSColor::secondaryLabelColor()));
                document.addSubview(&note);
            }
            FieldState::Set => {
                // SAFETY: nil target and action are set right after by `bind`.
                let button = unsafe {
                    NSButton::buttonWithTitle_target_action(
                        &NSString::from_str(RESET_TITLE),
                        None,
                        None,
                        mtm,
                    )
                };
                button.setFrame(rect(STATE_X, y + 2.0, STATE_WIDTH, 24.0));
                let control: Retained<NSControl> = Retained::into_super(button);
                self.bind(&control, &field.id, BindingKind::Reset, document, bindings);
            }
        }
    }

    /// A recorder showing the key, and Clear, which unbinds it.
    fn build_key_row(
        &self,
        id: &str,
        value: Option<&str>,
        y: f64,
        document: &NSView,
        bindings: &mut Vec<Binding>,
    ) {
        let mtm = self.mtm;
        // SAFETY: nil target and action are set right after by `bind`.
        let recorder = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str(&recorder_title(value)),
                None,
                None,
                mtm,
            )
        };
        recorder.setFrame(rect(CONTROL_X, y + 2.0, CONTROL_WIDTH, 24.0));
        let kind = BindingKind::KeyRecorder {
            value: value.map(str::to_string),
            button: recorder.clone(),
        };
        let control: Retained<NSControl> = Retained::into_super(recorder);
        self.bind(&control, id, kind, document, bindings);
        // SAFETY: nil target and action are set right after by `bind`.
        let clear = unsafe {
            NSButton::buttonWithTitle_target_action(
                &NSString::from_str(CLEAR_TITLE),
                None,
                None,
                mtm,
            )
        };
        clear.setFrame(rect(VALUE_X, y + 2.0, VALUE_WIDTH, 24.0));
        clear.setEnabled(value.is_some());
        let control: Retained<NSControl> = Retained::into_super(clear);
        self.bind(&control, id, BindingKind::KeyClear, document, bindings);
    }

    fn bind(
        &self,
        control: &NSControl,
        id: &str,
        kind: BindingKind,
        document: &NSView,
        bindings: &mut Vec<Binding>,
    ) {
        control.setTag(bindings.len() as NSInteger);
        // SAFETY: `calmChanged:` is defined on FormTarget and takes the sender.
        // Controls hold their target weakly; SettingsWindow keeps it alive.
        unsafe {
            control.setTarget(Some(self.target.as_ref() as &AnyObject));
            control.setAction(Some(sel!(calmChanged:)));
        }
        document.addSubview(control);
        bindings.push(Binding {
            id: id.to_string(),
            kind,
        });
    }

    fn build_control(
        &self,
        control: &Control,
        y: f64,
        document: &NSView,
    ) -> Option<(Retained<NSControl>, BindingKind)> {
        let mtm = self.mtm;
        let frame = rect(CONTROL_X, y + 2.0, CONTROL_WIDTH, 24.0);
        Some(match control {
            Control::Checkbox(on) => {
                // SAFETY: nil target and action are set right after by the caller.
                let button = unsafe {
                    NSButton::checkboxWithTitle_target_action(
                        &NSString::from_str(""),
                        None,
                        None,
                        mtm,
                    )
                };
                button.setFrame(frame);
                button.setState(isize::from(*on));
                (Retained::into_super(button), BindingKind::Checkbox)
            }
            Control::Slider {
                min,
                max,
                value,
                decimals,
            } => {
                // SAFETY: nil target and action are set right after by the caller.
                let slider = unsafe {
                    NSSlider::sliderWithValue_minValue_maxValue_target_action(
                        *value, *min, *max, None, None, mtm,
                    )
                };
                slider.setFrame(frame);
                slider.setContinuous(true);
                let value_label = label(
                    mtm,
                    &format_value(*value, *decimals),
                    rect(VALUE_X, y + 5.0, VALUE_WIDTH, 18.0),
                );
                document.addSubview(&value_label);
                (
                    Retained::into_super(slider),
                    BindingKind::Slider {
                        decimals: *decimals,
                        value_label,
                    },
                )
            }
            Control::Color(color) => {
                let well = NSColorWell::initWithFrame(
                    NSColorWell::alloc(mtm),
                    rect(CONTROL_X, y + 1.0, 60.0, 26.0),
                );
                well.setColor(&ns_from_rgb(*color));
                (Retained::into_super(well), BindingKind::Color)
            }
            Control::Choice { options, selected } => {
                let popup =
                    NSPopUpButton::initWithFrame_pullsDown(NSPopUpButton::alloc(mtm), frame, false);
                for option in options {
                    popup.addItemWithTitle(&NSString::from_str(option));
                }
                popup.selectItemAtIndex(*selected as NSInteger);
                (
                    Retained::into_super(Retained::into_super(popup)),
                    BindingKind::Choice,
                )
            }
            Control::Button(title) => {
                // SAFETY: nil target and action are set right after by the caller.
                let button = unsafe {
                    NSButton::buttonWithTitle_target_action(
                        &NSString::from_str(title),
                        None,
                        None,
                        mtm,
                    )
                };
                button.setFrame(frame);
                (Retained::into_super(button), BindingKind::Button)
            }
            Control::Text { value, placeholder } => {
                let field = NSTextField::textFieldWithString(&NSString::from_str(value), mtm);
                field.setFrame(frame);
                field.setPlaceholderString(Some(&NSString::from_str(placeholder)));
                // Report on Return and on focus loss, not on every keystroke.
                if let Some(cell) = field.cell() {
                    cell.setSendsActionOnEndEditing(true);
                }
                (Retained::into_super(field), BindingKind::Text)
            }
            Control::Note(text) => {
                let note = NSTextField::wrappingLabelWithString(&NSString::from_str(text), mtm);
                note.setFrame(rect(CONTROL_X, y + 5.0, CONTROL_WIDTH + VALUE_WIDTH, 20.0));
                note.setSelectable(true);
                note.setTextColor(Some(&NSColor::secondaryLabelColor()));
                note.setToolTip(Some(&NSString::from_str(text)));
                document.addSubview(&note);
                return None;
            }
            // Built by `build_key_row`, which binds two controls.
            Control::Key { .. } => return None,
        })
    }

    /// Shows the window as a child of `parent`, so it stays above the canvas until hidden.
    /// Without a parent it still opens, but the canvas can cover it.
    pub fn show_above(&self, parent: &impl HasWindowHandle) {
        match crate::content_gesture::ns_window(parent) {
            Ok(parent) => {
                if self.window.parentWindow().as_deref() != Some(&*parent) {
                    self.detach_from_parent();
                    // SAFETY: both windows are live and retained, and we are on the main
                    // thread (`self.mtm`). A child window joins its parent's Space too.
                    unsafe {
                        parent.addChildWindow_ordered(&self.window, NSWindowOrderingMode::Above)
                    };
                }
            }
            Err(err) => eprintln!("calm: settings window has no parent: {err}"),
        }
        self.window.makeKeyAndOrderFront(None);
        NSApplication::sharedApplication(self.mtm).activate();
    }

    /// True while AppKit keeps this window above `parent`, for the smoke.
    pub fn is_child_of(&self, parent: &impl HasWindowHandle) -> bool {
        let Ok(parent) = crate::content_gesture::ns_window(parent) else {
            return false;
        };
        self.window.parentWindow().as_deref() == Some(&*parent)
    }

    /// Closes as the title bar button does, for the smoke.
    pub fn perform_close(&self) {
        self.window.performClose(None);
    }

    /// True while the window has any parent, for the smoke.
    pub fn has_parent(&self) -> bool {
        self.window.parentWindow().is_some()
    }

    /// A hidden child would come back whenever its parent is ordered front.
    fn detach_from_parent(&self) {
        if let Some(parent) = self.window.parentWindow() {
            parent.removeChildWindow(&self.window);
        }
    }

    /// AppKit's identity for the window, stable while it exists.
    pub fn window_number(&self) -> isize {
        self.window.windowNumber()
    }

    pub fn hide(&self) {
        self.target.stop_recording();
        self.detach_from_parent();
        self.window.orderOut(None);
    }

    /// Closes the window for good, for a window whose subject is gone.
    pub fn close(&self) {
        self.target.stop_recording();
        self.window.makeFirstResponder(None);
        self.detach_from_parent();
        self.window.close();
    }

    /// Starts recording for the key row `id`, as a click on its recorder does.
    pub fn start_recording(&self, id: &str) -> bool {
        let index = self
            .target
            .ivars()
            .bindings
            .borrow()
            .iter()
            .position(|b| b.id == id && matches!(b.kind, BindingKind::KeyRecorder { .. }));
        let Some(index) = index else {
            return false;
        };
        if self.recording_field().as_deref() != Some(id) {
            self.target.toggle_recording(index);
        }
        self.recording_field().as_deref() == Some(id)
    }

    /// The key row now recording, if any.
    pub fn recording_field(&self) -> Option<String> {
        self.target
            .ivars()
            .recording
            .borrow()
            .as_ref()
            .map(|r| r.id.clone())
    }

    /// Shows `text` on the key row's recorder in place of its key.
    pub fn show_field_message(&self, id: &str, text: &str) {
        if let Some((button, _)) = self.target.recorder(id) {
            button.setTitle(&NSString::from_str(text));
        }
    }

    /// Puts the key back on the recorder after `show_field_message`.
    pub fn clear_field_message(&self, id: &str) {
        if self.recording_field().as_deref() == Some(id) {
            return;
        }
        if let Some((button, value)) = self.target.recorder(id) {
            button.setTitle(&NSString::from_str(&recorder_title(value.as_deref())));
        }
    }

    pub fn set_title(&self, title: &str) {
        self.window.setTitle(&NSString::from_str(title));
    }

    pub fn is_visible(&self) -> bool {
        self.window.isVisible()
    }

    /// True when AppKit placed the window on a display, so it is not lost off screen.
    pub fn is_on_screen(&self) -> bool {
        self.window.screen().is_some()
    }

    /// Controls wired to the target, for the smoke.
    pub fn bound_controls(&self) -> usize {
        self.target.ivars().bindings.borrow().len()
    }

    /// Tab labels in order. Empty for the single layout.
    pub fn tab_titles(&self) -> Vec<String> {
        match &self.body {
            Body::Single(_) => Vec::new(),
            Body::Tabs(tabs) => tabs
                .tabViewItems()
                .iter()
                .map(|item| item.label().to_string())
                .collect(),
        }
    }

    /// The selected tab's label. `None` for the single layout or with no tabs.
    pub fn selected_tab(&self) -> Option<String> {
        match &self.body {
            Body::Single(_) => None,
            Body::Tabs(tabs) => tabs
                .selectedTabViewItem()
                .map(|item| item.label().to_string()),
        }
    }

    /// Selects the tab with this label. Does nothing when there is none.
    pub fn select_tab(&self, title: &str) {
        // A recorder on a tab that goes away must not keep taking keys.
        self.target.stop_recording();
        if let Body::Tabs(tabs) = &self.body
            && let Some(index) = self.tab_titles().iter().position(|t| t == title)
        {
            tabs.selectTabViewItemAtIndex(index as NSInteger);
        }
    }
}

impl Drop for SettingsWindow {
    fn drop(&mut self) {
        // The monitor and observer must not outlive the window they watch.
        self.target.stop_recording();
    }
}

/// How many app windows carry `title`, for the smoke's one-window check.
pub fn settings_window_count(title: &str) -> Result<usize, NativeError> {
    let mtm = MainThreadMarker::new().ok_or(NativeError::NotMainThread)?;
    let title = NSString::from_str(title);
    Ok(NSApplication::sharedApplication(mtm)
        .windows()
        .iter()
        .filter(|window| window.title().isEqualToString(&title))
        .count())
}

/// Runs a modal NSOpenPanel for one image file. `None` when cancelled.
pub fn choose_image_file(title: &str) -> Result<Option<PathBuf>, NativeError> {
    let mtm = MainThreadMarker::new().ok_or(NativeError::NotMainThread)?;
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseFiles(true);
    panel.setCanChooseDirectories(false);
    panel.setAllowsMultipleSelection(false);
    panel.setTitle(Some(&NSString::from_str(title)));
    if panel.runModal() != NSModalResponseOK {
        return Ok(None);
    }
    Ok(panel
        .URLs()
        .firstObject()
        .and_then(|url| url.to_file_path()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slider_values_format_with_decimals() {
        assert_eq!(format_value(0.456, 2), "0.46");
        assert_eq!(format_value(64.4, 0), "64");
        assert_eq!(format_value(1.0, 1), "1.0");
    }

    use calm_core::keymap::{KeyParts, keystroke_from_parts};

    #[test]
    fn escape_alone_cancels_and_escape_with_cmd_records() {
        let escape = KeyParts {
            chars_ignoring_modifiers: "\u{1b}".into(),
            ..KeyParts::default()
        };
        assert_eq!(recorder_step(&escape), RecorderStep::Cancel);
        let cmd_escape = KeyParts {
            cmd: true,
            ..escape.clone()
        };
        assert_eq!(
            recorder_step(&cmd_escape),
            RecorderStep::Report("cmd-escape".into())
        );
        assert_eq!(recorder_step(&KeyParts::default()), RecorderStep::Ignore);
    }

    #[test]
    fn parts_from_reads_each_flag_and_each_layer() {
        use objc2_app_kit::NSEventModifierFlags as F;
        let chars = |f: F| {
            match (f.contains(F::Command), f.contains(F::Shift)) {
                (false, false) => "0",
                (false, true) => ")",
                (true, false) => "0",
                (true, true) => ")",
            }
            .to_string()
        };
        let p = parts_from(F::Command | F::Shift, "0", chars, false);
        assert!(p.cmd && p.shift && !p.ctrl && !p.alt && !p.function);
        assert_eq!(
            (
                p.chars_plain.as_str(),
                p.chars_shift.as_str(),
                p.chars_cmd.as_str(),
                p.chars_cmd_shift.as_str()
            ),
            ("0", ")", "0", ")")
        );
        assert_eq!(keystroke_from_parts(&p).as_deref(), Some("cmd-)"));
        let p = parts_from(
            F::Control | F::Option | F::Function,
            "a",
            |_| "a".into(),
            false,
        );
        assert!(p.ctrl && p.alt && p.function && !p.cmd);
    }

    #[test]
    fn recorder_shows_symbols_or_none() {
        assert_eq!(recorder_title(Some("cmd-shift-backspace")), "⇧⌘⌫");
        assert_eq!(recorder_title(None), UNBOUND_TITLE);
    }

    #[test]
    fn a_click_off_the_recorder_cancels_recording() {
        assert_eq!(recorder_click_step(false), RecorderStep::Cancel);
        assert_eq!(recorder_click_step(true), RecorderStep::Ignore);
    }
}
