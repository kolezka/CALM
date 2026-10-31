//! The Settings window: a separate native NSWindow that draws a `calm_core::settings_form`
//! with AppKit controls. Native, so it never sits under a Ghostty or WKWebView view, and
//! the color wells bring the system color picker for free.
//!
//! Every control targets one helper object. Its tag indexes the bindings list, which says
//! which field id it edits and how to read its value.

use std::cell::RefCell;
use std::path::PathBuf;

use calm_core::background::Rgb;
use calm_core::settings_form::{Control, FieldValue, Section};
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, NSObject};
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel};
use objc2_app_kit::{
    NSApplication, NSAutoresizingMaskOptions, NSBackingStoreType, NSButton, NSColor, NSColorSpace,
    NSColorWell, NSControl, NSFont, NSModalResponseOK, NSOpenPanel, NSPopUpButton, NSScrollView,
    NSSlider, NSTextField, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSInteger, NSPoint, NSRect, NSSize, NSString};

use crate::NativeError;

const WINDOW_TITLE: &str = "CALM Settings";
const WINDOW_WIDTH: f64 = 560.0;
const WINDOW_HEIGHT: f64 = 640.0;
const MARGIN: f64 = 20.0;
const ROW_HEIGHT: f64 = 30.0;
const HEADER_HEIGHT: f64 = 34.0;
const LABEL_WIDTH: f64 = 190.0;
const CONTROL_X: f64 = MARGIN + LABEL_WIDTH + 10.0;
const CONTROL_WIDTH: f64 = 230.0;
const VALUE_X: f64 = CONTROL_X + CONTROL_WIDTH + 8.0;
const VALUE_WIDTH: f64 = 70.0;

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
}

struct Binding {
    id: String,
    kind: BindingKind,
}

#[derive(Default)]
struct TargetIvars {
    callback: RefCell<Option<ChangeCallback>>,
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
            // Take the callback out while it runs, so it may rebuild the form.
            let taken = self.ivars().callback.borrow_mut().take();
            if let Some(mut callback) = taken {
                callback(id, value);
                let mut slot = self.ivars().callback.borrow_mut();
                if slot.is_none() {
                    *slot = Some(callback);
                }
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

/// The Settings window. Hidden, not destroyed, when the user closes it.
pub struct SettingsWindow {
    mtm: MainThreadMarker,
    window: Retained<NSWindow>,
    scroll: Retained<NSScrollView>,
    target: Retained<FormTarget>,
}

impl SettingsWindow {
    /// `on_change` runs on the main thread inside the AppKit action. Do not block in it.
    pub fn new(
        on_change: impl FnMut(String, FieldValue) + 'static,
    ) -> Result<SettingsWindow, NativeError> {
        let mtm = MainThreadMarker::new().ok_or(NativeError::NotMainThread)?;
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable;
        // SAFETY: standard designated initializer with a valid rect, mask and backing.
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect(0.0, 0.0, WINDOW_WIDTH, WINDOW_HEIGHT),
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // SAFETY: we keep the window retained, so AppKit must not release it on close.
        unsafe { window.setReleasedWhenClosed(false) };
        window.setTitle(&NSString::from_str(WINDOW_TITLE));
        window.setMinSize(NSSize::new(WINDOW_WIDTH, 320.0));

        let scroll = NSScrollView::initWithFrame(
            NSScrollView::alloc(mtm),
            rect(0.0, 0.0, WINDOW_WIDTH, WINDOW_HEIGHT),
        );
        scroll.setHasVerticalScroller(true);
        scroll.setAutoresizingMask(
            NSAutoresizingMaskOptions::ViewWidthSizable
                | NSAutoresizingMaskOptions::ViewHeightSizable,
        );
        window.setContentView(Some(&scroll));
        window.center();

        let target = FormTarget::new(mtm);
        *target.ivars().callback.borrow_mut() = Some(Box::new(on_change));
        Ok(SettingsWindow {
            mtm,
            window,
            scroll,
            target,
        })
    }

    /// Replaces every control with a fresh build of `sections`.
    pub fn set_form(&self, sections: &[Section]) {
        let mtm = self.mtm;
        if let Some(old) = self.scroll.documentView() {
            for view in old.subviews().iter() {
                // An active well keeps feeding the shared color panel after removal.
                if let Some(well) = view.downcast_ref::<NSColorWell>() {
                    well.deactivate();
                }
            }
        }
        let rows: usize = sections.iter().map(|s| s.fields.len()).sum();
        let height =
            MARGIN * 2.0 + sections.len() as f64 * HEADER_HEIGHT + rows as f64 * ROW_HEIGHT;
        let document = FlippedView::alloc(mtm).set_ivars(());
        // SAFETY: NSView's designated initializer on a fresh instance.
        let document: Retained<FlippedView> = unsafe {
            msg_send![super(document), initWithFrame: rect(0.0, 0.0, WINDOW_WIDTH, height)]
        };
        document.setAutoresizingMask(NSAutoresizingMaskOptions::ViewWidthSizable);

        let mut bindings = Vec::new();
        let mut y = MARGIN;
        for section in sections {
            let header = label(mtm, &section.title, rect(MARGIN, y + 8.0, 400.0, 20.0));
            header.setFont(Some(&NSFont::boldSystemFontOfSize(13.0)));
            document.addSubview(&header);
            y += HEADER_HEIGHT;
            for field in &section.fields {
                if !field.label.is_empty() {
                    let name = label(mtm, &field.label, rect(MARGIN, y + 5.0, LABEL_WIDTH, 18.0));
                    document.addSubview(&name);
                }
                if let Some((control, kind)) = self.build_control(&field.control, y, &document) {
                    control.setTag(bindings.len() as NSInteger);
                    // SAFETY: `calmChanged:` is defined on FormTarget and takes the sender.
                    // Controls hold their target weakly; SettingsWindow keeps it alive.
                    unsafe {
                        control.setTarget(Some(self.target.as_ref() as &AnyObject));
                        control.setAction(Some(sel!(calmChanged:)));
                    }
                    document.addSubview(&control);
                    bindings.push(Binding {
                        id: field.id.clone(),
                        kind,
                    });
                }
                y += ROW_HEIGHT;
            }
        }
        *self.target.ivars().bindings.borrow_mut() = bindings;
        self.scroll.setDocumentView(Some(&document));
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

    pub fn show(&self) {
        self.window.makeKeyAndOrderFront(None);
        NSApplication::sharedApplication(self.mtm).activate();
    }

    /// AppKit's identity for the window, stable while it exists.
    pub fn window_number(&self) -> isize {
        self.window.windowNumber()
    }

    pub fn hide(&self) {
        self.window.orderOut(None);
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
}

/// How many app windows carry the Settings title, for the smoke's one-window check.
pub fn settings_window_count() -> Result<usize, NativeError> {
    let mtm = MainThreadMarker::new().ok_or(NativeError::NotMainThread)?;
    let title = NSString::from_str(WINDOW_TITLE);
    Ok(NSApplication::sharedApplication(mtm)
        .windows()
        .iter()
        .filter(|window| window.title().isEqualToString(&title))
        .count())
}

/// Runs a modal NSOpenPanel for one image file. `None` when cancelled.
pub fn choose_image_file() -> Result<Option<PathBuf>, NativeError> {
    let mtm = MainThreadMarker::new().ok_or(NativeError::NotMainThread)?;
    let panel = NSOpenPanel::openPanel(mtm);
    panel.setCanChooseFiles(true);
    panel.setCanChooseDirectories(false);
    panel.setAllowsMultipleSelection(false);
    panel.setTitle(Some(&NSString::from_str("Choose a Background Image")));
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
