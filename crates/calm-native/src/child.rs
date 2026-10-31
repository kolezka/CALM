use std::ffi::c_void;
use std::ptr::NonNull;

use calm_core::geometry::{Rect, Size};
use calm_core::viewport::NativePlacement;
use objc2::rc::Retained;
use objc2::{MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{NSView, NSWindowOrderingMode};
use objc2_foundation::{NSPoint, NSRect, NSSize};
use raw_window_handle::{
    AppKitWindowHandle, DisplayHandle, HandleError, HasDisplayHandle, HasWindowHandle,
    RawWindowHandle, WindowHandle,
};

use crate::NativeError;

/// The clip view's frame in the parent's bottom-left (AppKit) coordinates.
/// `placement.container` is top-left, in the parent's points.
pub fn clip_frame(placement: &NativePlacement, viewport_height_px: f64) -> Rect {
    let c = placement.container;
    Rect::new(
        c.origin.x,
        viewport_height_px - c.bottom(),
        c.size.width,
        c.size.height,
    )
}

/// The zoom view's frame inside the clip view. It puts logical point `inner_origin`
/// (top-left based) at the clip view's top-left corner. Neither view is flipped.
pub fn zoom_frame(placement: &NativePlacement) -> Rect {
    let zoom = placement.zoom;
    let logical = placement.logical_size;
    Rect::new(
        -placement.inner_origin.x * zoom,
        placement.container.size.height - logical.height * zoom + placement.inner_origin.y * zoom,
        logical.width * zoom,
        logical.height * zoom,
    )
}

/// The zoom view's bounds are always the logical size, so AppKit scales by frame / bounds
/// and a child filling `(0, 0, logical_size)` never sees the clip.
pub fn zoom_bounds(logical_size: Size) -> Rect {
    Rect::new(0.0, 0.0, logical_size.width, logical_size.height)
}

pub(crate) fn ns_rect(rect: Rect) -> NSRect {
    NSRect::new(
        NSPoint::new(rect.origin.x, rect.origin.y),
        NSSize::new(rect.size.width, rect.size.height),
    )
}

/// The view behind `window`'s AppKit handle, which native panels and the status bar join.
pub(crate) fn host_view(
    window: &impl HasWindowHandle,
) -> Result<(Retained<NSView>, MainThreadMarker), NativeError> {
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
    Ok((parent.retain(), mtm))
}

fn plain_layer_view(mtm: MainThreadMarker) -> Retained<NSView> {
    let zero = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0));
    let view = NSView::initWithFrame(NSView::alloc(mtm), zero);
    view.setWantsLayer(true);
    view
}

/// Two plain views per panel. `clip` sits in the window at the clipped screen rect and clips;
/// `zoom` inside it is the logical content scaled by zoom. Ghostty and WKWebView attach to
/// `zoom` at `(0, 0, logical_size)`.
pub struct NativeChild {
    clip: Retained<NSView>,
    zoom: Retained<NSView>,
}

impl NativeChild {
    /// Adds a hidden clip view to the view behind `window`'s AppKit handle.
    pub fn new(window: &impl HasWindowHandle) -> Result<NativeChild, NativeError> {
        let (parent, mtm) = host_view(window)?;
        let clip = plain_layer_view(mtm);
        // Off by default on macOS 14+, so an oversized child would paint past the clip rect.
        clip.setClipsToBounds(true);
        clip.setHidden(true);
        let zoom = plain_layer_view(mtm);
        clip.addSubview(&zoom);
        parent.addSubview(&clip);
        Ok(NativeChild { clip, zoom })
    }

    /// Moves, clips and scales the views for this frame. All values are logical points;
    /// `_scale_factor` is kept for the call signature.
    pub fn apply(
        &mut self,
        placement: &NativePlacement,
        viewport_height_px: f64,
        _scale_factor: f64,
    ) {
        self.clip
            .setFrame(ns_rect(clip_frame(placement, viewport_height_px)));
        // Frame first: setting the frame after custom bounds would rescale the bounds.
        self.zoom.setFrame(ns_rect(zoom_frame(placement)));
        self.zoom
            .setBounds(ns_rect(zoom_bounds(placement.logical_size)));
        self.clip.setHidden(false);
    }

    pub fn set_visible(&mut self, visible: bool) {
        self.clip.setHidden(!visible);
    }

    /// Moves the clip view above its siblings, so this panel draws over other native panels.
    pub fn bring_to_front(&mut self) {
        // SAFETY: main thread only (NSView is MainThreadOnly); superview returns a retained view.
        let Some(parent) = (unsafe { self.clip.superview() }) else {
            return;
        };
        parent.addSubview_positioned_relativeTo(&self.clip, NSWindowOrderingMode::Above, None);
    }

    /// Position of the clip view among its superview's subviews, back to front.
    pub fn stack_index(&self) -> Option<usize> {
        // SAFETY: as in `bring_to_front`.
        let parent = unsafe { self.clip.superview() }?;
        let clip = Retained::as_ptr(&self.clip);
        parent
            .subviews()
            .iter()
            .position(|view| Retained::as_ptr(&view) == clip)
    }

    /// The zoom view, used as the parent for Ghostty and WKWebView.
    pub fn ns_view(&self) -> *mut c_void {
        Retained::as_ptr(&self.zoom) as *mut c_void
    }

    pub(crate) fn zoom_view(&self) -> &Retained<NSView> {
        &self.zoom
    }

    pub(crate) fn clip_view(&self) -> &Retained<NSView> {
        &self.clip
    }
}

/// Whether AppKit reports any part of `window` as visible on screen, or `None` when the
/// handle is not an AppKit view in a window. GPUI stops drawing an occluded window.
pub fn window_visible(window: &impl HasWindowHandle) -> Option<bool> {
    MainThreadMarker::new()?;
    let handle = window.window_handle().ok()?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return None;
    };
    // SAFETY: as in `NativeChild::new`: a live NSView while the handle is borrowed, read on
    // the main thread.
    let view: &NSView = unsafe { appkit.ns_view.cast::<NSView>().as_ref() };
    let state = view.window()?.occlusionState();
    Some(state.contains(objc2_app_kit::NSWindowOcclusionState::Visible))
}

impl HasWindowHandle for NativeChild {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let ns_view = NonNull::new(self.ns_view()).ok_or(HandleError::Unavailable)?;
        let raw = RawWindowHandle::AppKit(AppKitWindowHandle::new(ns_view));
        // SAFETY: the zoom view is retained by self, so it outlives the borrow.
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
        self.clip.removeFromSuperview();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use calm_core::geometry::Point;

    fn placement() -> NativePlacement {
        NativePlacement {
            container: Rect::new(0.0, 36.0, 300.0, 214.0),
            inner_origin: Point::new(100.0, 86.0),
            logical_size: Size::new(400.0, 300.0),
            zoom: 2.0,
        }
    }

    /// Where a top-left based logical point lands in the clip view's (unflipped) coordinates.
    fn logical_to_clip(p: &NativePlacement, logical: Point) -> Point {
        let frame = zoom_frame(p);
        let unflipped_y = p.logical_size.height - logical.y;
        Point::new(
            frame.origin.x + logical.x * p.zoom,
            frame.origin.y + unflipped_y * p.zoom,
        )
    }

    #[test]
    fn clip_frame_flips_y_against_viewport_height() {
        // bottom edge at 250 from the top of an 800 tall view is 550 from its bottom.
        assert_eq!(
            clip_frame(&placement(), 800.0),
            Rect::new(0.0, 550.0, 300.0, 214.0)
        );
    }

    #[test]
    fn zoom_frame_scales_logical_size_and_offsets_by_inner_origin() {
        // x: -100 * 2; y: 214 - 300 * 2 + 86 * 2.
        assert_eq!(
            zoom_frame(&placement()),
            Rect::new(-200.0, -214.0, 800.0, 600.0)
        );
    }

    #[test]
    fn inner_origin_lands_at_clip_top_left() {
        let p = placement();
        assert_eq!(
            logical_to_clip(&p, p.inner_origin),
            Point::new(0.0, p.container.size.height)
        );
    }

    #[test]
    fn unclipped_zoom_frame_fills_the_clip_view() {
        let p = NativePlacement {
            container: Rect::new(10.0, 20.0, 600.0, 450.0),
            inner_origin: Point::new(0.0, 0.0),
            logical_size: Size::new(400.0, 300.0),
            zoom: 1.5,
        };
        assert_eq!(zoom_frame(&p), Rect::new(0.0, 0.0, 600.0, 450.0));
    }

    #[test]
    fn zoom_bounds_are_the_logical_size() {
        assert_eq!(
            zoom_bounds(Size::new(400.0, 300.0)),
            Rect::new(0.0, 0.0, 400.0, 300.0)
        );
    }
}
