//! The floating status bar: a rounded, translucent pill near the bottom of the window.
//!
//! It is native on purpose. Terminals and web views draw above everything GPUI paints, so a
//! GPUI bar would vanish under any panel that slides beneath it. An `NSVisualEffectView`
//! kept as the topmost sibling of the panel views stays visible and blurs what is behind it.

use calm_core::geometry::Rect;
use calm_core::status_bar::{STATUS_BAR_CORNER_RADIUS, STATUS_BAR_PADDING_X};
use objc2::MainThreadOnly;
use objc2::rc::Retained;
use objc2_app_kit::{
    NSColor, NSFont, NSFontWeightMedium, NSLineBreakMode, NSTextAlignment, NSTextField, NSView,
    NSVisualEffectBlendingMode, NSVisualEffectMaterial, NSVisualEffectState, NSVisualEffectView,
    NSWindowOrderingMode,
};
use objc2_foundation::{NSPoint, NSRect, NSSize, NSString};
use objc2_quartz_core::kCACornerCurveContinuous;
use raw_window_handle::HasWindowHandle;

use crate::NativeError;
use crate::child::{host_view, ns_rect};

const FONT_SIZE: f64 = 12.0;

pub struct StatusBar {
    effect: Retained<NSVisualEffectView>,
    label: Retained<NSTextField>,
    text: String,
}

impl StatusBar {
    /// Adds a hidden bar on top of the views behind `window`'s AppKit handle.
    pub fn new(window: &impl HasWindowHandle) -> Result<StatusBar, NativeError> {
        let (parent, mtm) = host_view(window)?;
        let zero = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0));

        let effect = NSVisualEffectView::initWithFrame(NSVisualEffectView::alloc(mtm), zero);
        // Popover follows the system light or dark appearance; WithinWindow blurs the canvas
        // and panels under the bar rather than the desktop behind the window.
        effect.setMaterial(NSVisualEffectMaterial::Popover);
        effect.setBlendingMode(NSVisualEffectBlendingMode::WithinWindow);
        effect.setState(NSVisualEffectState::Active);
        effect.setWantsLayer(true);
        if let Some(layer) = effect.layer() {
            layer.setCornerRadius(STATUS_BAR_CORNER_RADIUS);
            // SAFETY: a framework-provided constant string, valid for the process lifetime.
            layer.setCornerCurve(unsafe { kCACornerCurveContinuous });
            layer.setMasksToBounds(true);
        }
        effect.setHidden(true);

        let label = NSTextField::labelWithString(&NSString::from_str(""), mtm);
        // SAFETY: a framework-provided constant, read on the main thread.
        let weight = unsafe { NSFontWeightMedium };
        let font = NSFont::monospacedDigitSystemFontOfSize_weight(FONT_SIZE, weight);
        label.setFont(Some(&font));
        label.setTextColor(Some(&NSColor::labelColor()));
        label.setAlignment(NSTextAlignment::Center);
        label.setLineBreakMode(NSLineBreakMode::ByTruncatingMiddle);
        effect.addSubview(&label);
        parent.addSubview(&effect);
        Ok(StatusBar {
            effect,
            label,
            text: String::new(),
        })
    }

    /// Sets the text and returns its natural width in points.
    pub fn set_text(&mut self, text: &str) -> f64 {
        if self.text != text {
            self.label.setStringValue(&NSString::from_str(text));
            self.text = text.to_string();
        }
        self.label.sizeToFit();
        self.label.frame().size.width
    }

    pub fn text(&self) -> &str {
        &self.text
    }

    /// Places the bar at `rect` (top-left window points) or hides it for `None`.
    pub fn apply(&mut self, rect: Option<Rect>, viewport_height: f64) {
        let Some(rect) = rect else {
            self.effect.setHidden(true);
            return;
        };
        let flipped = Rect::new(
            rect.origin.x,
            viewport_height - rect.bottom(),
            rect.size.width,
            rect.size.height,
        );
        self.effect.setFrame(ns_rect(flipped));
        let label_height = self.label.frame().size.height;
        self.label.setFrame(ns_rect(Rect::new(
            STATUS_BAR_PADDING_X,
            ((rect.size.height - label_height) / 2.0).round(),
            (rect.size.width - 2.0 * STATUS_BAR_PADDING_X).max(0.0),
            label_height,
        )));
        self.effect.setHidden(false);
    }

    /// Keeps the bar above every panel view. Panels restack with `bring_to_front` and new
    /// ones are added on top, so this runs after each restack. Cheap when already on top.
    pub fn ensure_front(&mut self) {
        // SAFETY: main thread only (NSView is MainThreadOnly); superview returns a retained view.
        let Some(parent) = (unsafe { self.effect.superview() }) else {
            return;
        };
        if self.is_front_in(&parent) {
            return;
        }
        parent.addSubview_positioned_relativeTo(&self.effect, NSWindowOrderingMode::Above, None);
    }

    /// True when the bar is the topmost subview of its parent.
    pub fn is_front(&self) -> bool {
        // SAFETY: as in `ensure_front`.
        unsafe { self.effect.superview() }.is_some_and(|parent| self.is_front_in(&parent))
    }

    pub fn is_visible(&self) -> bool {
        !self.effect.isHidden()
    }

    fn is_front_in(&self, parent: &NSView) -> bool {
        let effect: *const NSView = Retained::as_ptr(&self.effect).cast();
        parent
            .subviews()
            .lastObject()
            .is_some_and(|top| Retained::as_ptr(&top) == effect)
    }
}

impl Drop for StatusBar {
    fn drop(&mut self) {
        self.effect.removeFromSuperview();
    }
}
