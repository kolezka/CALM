//! The Settings window: a separate native NSWindow that draws a `calm_core::settings_form`
//! with AppKit controls. Native, so it never sits under a Ghostty or WKWebView view, and
//! the color wells bring the system color picker for free.
//!
//! `WindowLayout::Tabs` puts each section in its own scrolling tab; `Single` stacks them in
//! one scroll view.
//!
//! Every control targets one helper object. Its tag indexes the bindings list, which says
//! which field id it edits and how to read its value.

use std::cell::RefCell;
use std::collections::VecDeque;
use std::path::PathBuf;

use calm_core::background::Rgb;
use calm_core::settings_form::{Control, Field, FieldState, FieldValue, Section};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSAutoresizingMaskOptions, NSBackingStoreType, NSButton, NSColor, NSColorSpace,
    NSColorWell, NSControl, NSFont, NSModalResponseOK, NSOpenPanel, NSPopUpButton, NSScrollView,
    NSSlider, NSTabView, NSTabViewItem, NSTextField, NSView, NSWindow, NSWindowOrderingMode,
    NSWindowStyleMask,
};
use objc2_foundation::{NSInteger, NSPoint, NSRect, NSSize, NSString};
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
            let Some((id, value)) = self.read(sender) else {
                return;
            };
            self.ivars().pending.borrow_mut().push_back((id, value));
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
    }
);

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
        if let Some((control, kind)) = self.build_control(&field.control, y, document) {
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
        self.detach_from_parent();
        self.window.orderOut(None);
    }

    /// Closes the window for good, for a window whose subject is gone.
    pub fn close(&self) {
        self.window.makeFirstResponder(None);
        self.detach_from_parent();
        self.window.close();
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
        if let Body::Tabs(tabs) = &self.body
            && let Some(index) = self.tab_titles().iter().position(|t| t == title)
        {
            tabs.selectTabViewItemAtIndex(index as NSInteger);
        }
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
}
