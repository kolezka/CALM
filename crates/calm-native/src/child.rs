use std::ffi::c_void;
use std::ptr::NonNull;

use calm_core::geometry::Rect;
use calm_core::viewport::NativePlacement;
use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{NSResponder, NSView};
use objc2_foundation::{NSObject, NSPoint, NSRect, NSSize};
use raw_window_handle::{
    AppKitWindowHandle, DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle,
    RawWindowHandle, WindowHandle,
};

use crate::NativeError;

define_class!(
    // SAFETY: NSView has no subclassing requirements beyond being used on the main thread,
    // which MainThreadOnly enforces. We only override isFlipped.
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "CalmContainerView"]
    struct ContainerView;

    impl ContainerView {
        // Top-left origin, so children and bounds use the same y direction as calm-core.
        // SAFETY: matches NSView's `- (BOOL)isFlipped` signature.
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }
    }
);

impl ContainerView {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(());
        let zero = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0));
        // SAFETY: initWithFrame: is NSView's designated initializer and the ivars are set.
        unsafe { msg_send![super(this), initWithFrame: zero] }
    }
}

/// Snaps a length to the device pixel grid.
fn snap(value: f64, scale_factor: f64) -> f64 {
    if scale_factor > 0.0 {
        (value * scale_factor).round() / scale_factor
    } else {
        value
    }
}

/// The container's frame in the parent's bottom-left (AppKit) coordinates, snapped to device
/// pixels. `placement.container` is top-left, in the same points as the parent view.
pub fn container_frame(
    placement: &NativePlacement,
    viewport_height_px: f64,
    scale_factor: f64,
) -> Rect {
    let c = placement.container;
    let left = snap(c.origin.x, scale_factor);
    let top = snap(c.origin.y, scale_factor);
    let right = snap(c.right(), scale_factor);
    let bottom = snap(c.bottom(), scale_factor);
    Rect::new(
        left,
        viewport_height_px - bottom,
        right - left,
        bottom - top,
    )
}

/// The container's bounds. The container is flipped, so the bounds origin is the content
/// point shown at the container's top-left corner. Snapping shifts are folded back in.
pub fn container_bounds(placement: &NativePlacement, scale_factor: f64) -> Rect {
    let c = placement.container;
    let zoom = placement.zoom;
    let left = snap(c.origin.x, scale_factor);
    let top = snap(c.origin.y, scale_factor);
    let width = snap(c.right(), scale_factor) - left;
    let height = snap(c.bottom(), scale_factor) - top;
    Rect::new(
        placement.inner_origin.x + (left - c.origin.x) / zoom,
        placement.inner_origin.y + (top - c.origin.y) / zoom,
        width / zoom,
        height / zoom,
    )
}

fn ns_rect(rect: Rect) -> NSRect {
    NSRect::new(
        NSPoint::new(rect.origin.x, rect.origin.y),
        NSSize::new(rect.size.width, rect.size.height),
    )
}

/// A plain layer-backed container view per panel. CALM moves, clips and scales it; the
/// panel's own view (Ghostty, WKWebView) sits inside at `(0, 0, logical_size)`.
pub struct NativeChild {
    container: Retained<ContainerView>,
}

impl NativeChild {
    /// Adds a hidden container to the view behind `window`'s AppKit handle.
    pub fn new(window: &impl HasWindowHandle) -> Result<NativeChild, NativeError> {
        let mtm = MainThreadMarker::new().ok_or(NativeError::NotMainThread)?;
        let handle = window
            .window_handle()
            .map_err(|err| NativeError::UnsupportedWindowHandle(err.to_string()))?;
        let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
            return Err(NativeError::UnsupportedWindowHandle(
                "not an AppKit handle".to_string(),
            ));
        };
        // SAFETY: an AppKit window handle's ns_view is a live NSView for as long as the handle
        // is borrowed, and we are on the main thread.
        let parent: &NSView = unsafe { appkit.ns_view.cast::<NSView>().as_ref() };

        let container = ContainerView::new(mtm);
        container.setWantsLayer(true);
        container.setHidden(true);
        parent.addSubview(&container);
        Ok(NativeChild { container })
    }

    /// Moves, clips and scales the container for this frame.
    pub fn apply(
        &mut self,
        placement: &NativePlacement,
        viewport_height_px: f64,
        scale_factor: f64,
    ) {
        let frame = container_frame(placement, viewport_height_px, scale_factor);
        let bounds = container_bounds(placement, scale_factor);
        self.container.setFrame(ns_rect(frame));
        self.container.setBounds(ns_rect(bounds));
        self.container.setHidden(false);
    }

    pub fn set_visible(&mut self, visible: bool) {
        self.container.setHidden(!visible);
    }

    /// The container view, used as the parent for Ghostty and WKWebView.
    pub fn ns_view(&self) -> *mut c_void {
        Retained::as_ptr(&self.container) as *mut c_void
    }

    pub(crate) fn container(&self) -> &NSView {
        &self.container
    }
}

impl HasWindowHandle for NativeChild {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let ns_view = NonNull::new(self.ns_view()).ok_or(HandleError::Unavailable)?;
        let raw = RawWindowHandle::AppKit(AppKitWindowHandle::new(ns_view));
        // SAFETY: the container is retained by self, so the view outlives the borrow.
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

impl HasDisplayHandle for NativeChild {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        Ok(DisplayHandle::appkit())
    }
}

impl Drop for NativeChild {
    fn drop(&mut self) {
        self.container.removeFromSuperview();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use calm_core::geometry::{Point, Size};

    fn placement() -> NativePlacement {
        NativePlacement {
            container: Rect::new(0.0, 36.0, 300.0, 214.0),
            inner_origin: Point::new(100.0, 86.0),
            logical_size: Size::new(400.0, 300.0),
            zoom: 2.0,
        }
    }

    #[test]
    fn container_frame_flips_y_against_viewport_height() {
        // bottom edge at 250 from the top of an 800 tall view is 550 from its bottom.
        assert_eq!(
            container_frame(&placement(), 800.0, 2.0),
            Rect::new(0.0, 550.0, 300.0, 214.0)
        );
    }

    #[test]
    fn container_bounds_starts_at_inner_origin_and_divides_by_zoom() {
        assert_eq!(
            container_bounds(&placement(), 2.0),
            Rect::new(100.0, 86.0, 150.0, 107.0)
        );
    }

    #[test]
    fn snapping_to_device_pixels_shifts_bounds_origin_to_match() {
        let mut p = placement();
        p.container = Rect::new(10.3, 36.0, 300.0, 214.0);
        p.zoom = 1.0;
        let frame = container_frame(&p, 800.0, 2.0);
        let bounds = container_bounds(&p, 2.0);
        assert_eq!(frame.origin.x, 10.5);
        assert_eq!(frame.size.width, 300.0);
        assert!((bounds.origin.x - 100.2).abs() < 1e-9);
    }
}
