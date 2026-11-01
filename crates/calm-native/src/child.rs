use std::cell::RefCell;
use std::ffi::c_void;
use std::ptr::NonNull;

use calm_core::geometry::{Rect, Size};
use calm_core::viewport::NativePlacement;
use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2::{
    ClassType, DefinedClass, MainThreadMarker, MainThreadOnly, Message, define_class, msg_send,
};
use objc2_app_kit::{NSView, NSWindowOrderingMode};
use objc2_core_graphics::CGMutablePath;
use objc2_foundation::{NSPoint, NSRect, NSSize};
use objc2_quartz_core::{CAShapeLayer, CATransaction};
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

/// The visible pieces in the clip view's own coordinates, or `None` when the whole
/// container is visible and no mask is needed. `flipped` picks a top-left origin (a
/// geometry flipped layer); otherwise the origin is bottom-left, as in an unflipped NSView.
pub fn mask_pieces(placement: &NativePlacement, flipped: bool) -> Option<Vec<Rect>> {
    let c = placement.container;
    if placement.visible.as_slice() == [c] {
        return None;
    }
    Some(
        placement
            .visible
            .iter()
            .map(|piece| {
                let y = if flipped {
                    piece.origin.y - c.origin.y
                } else {
                    c.bottom() - piece.bottom()
                };
                Rect::new(
                    piece.origin.x - c.origin.x,
                    y,
                    piece.size.width,
                    piece.size.height,
                )
            })
            .collect(),
    )
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
    let view = NSView::initWithFrame(NSView::alloc(mtm), zero_rect());
    view.setWantsLayer(true);
    view
}

fn zero_rect() -> NSRect {
    NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(0.0, 0.0))
}

#[derive(Default)]
struct ClipIvars {
    /// Visible pieces in the view's own (bottom-left) coordinates. `None` takes every hit.
    hit_pieces: RefCell<Option<Vec<NSRect>>>,
}

define_class!(
    // SAFETY: NSView has no subclassing requirements beyond main thread use, which
    // MainThreadOnly enforces; the only override calls super with the same argument.
    #[unsafe(super(NSView))]
    #[thread_kind = MainThreadOnly]
    #[name = "CALMNativeClipView"]
    #[ivars = ClipIvars]
    struct ClipView;

    impl ClipView {
        /// A masked area is not part of this panel: let the hit fall through to whatever
        /// is under it, usually the GPUI host drawing the panel in front.
        #[unsafe(method(hitTest:))]
        fn hit_test(&self, point: NSPoint) -> *mut NSView {
            if let Some(pieces) = self.ivars().hit_pieces.borrow().as_ref() {
                // SAFETY: main thread only; superview returns a retained view or nil.
                let superview = unsafe { self.superview() };
                // hitTest gets the point in the superview's coordinates.
                let local = self.convertPoint_fromView(point, superview.as_deref());
                let inside = pieces.iter().any(|piece| {
                    local.x >= piece.origin.x
                        && local.x < piece.origin.x + piece.size.width
                        && local.y >= piece.origin.y
                        && local.y < piece.origin.y + piece.size.height
                });
                if !inside {
                    return std::ptr::null_mut();
                }
            }
            // SAFETY: NSView implements hitTest: with this signature; the result is +0 and
            // returned as is, as AppKit expects.
            unsafe { msg_send![super(self), hitTest: point] }
        }
    }
);

impl ClipView {
    fn new(mtm: MainThreadMarker) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ClipIvars::default());
        // SAFETY: NSView's designated initializer on a freshly allocated instance with ivars
        // set.
        let view: Retained<Self> = unsafe { msg_send![super(this), initWithFrame: zero_rect()] };
        view.setWantsLayer(true);
        view
    }
}

/// True when `view` is a native panel's clip view or inside one.
pub(crate) fn is_in_native_child(view: &NSView) -> bool {
    let mut current = Some(view.retain());
    while let Some(view) = current {
        if view.isKindOfClass(ClipView::class()) {
            return true;
        }
        // SAFETY: main thread only; superview returns a retained view or nil.
        current = unsafe { view.superview() };
    }
    false
}

/// Two views per panel. `clip` sits in the window at the clipped screen rect and clips;
/// `zoom` inside it is the logical content scaled by zoom. Ghostty and WKWebView attach to
/// `zoom` at `(0, 0, logical_size)`. Parts of `clip` under a panel in front are masked out
/// of paint and hit testing, since GPUI chrome cannot draw above native views.
pub struct NativeChild {
    clip: Retained<ClipView>,
    zoom: Retained<NSView>,
    /// Mask applied last, as (clip size, pieces); `None` pieces means unmasked.
    mask: Option<(Size, Option<Vec<Rect>>)>,
    /// Nothing is visible: stay hidden even when asked to show.
    occluded: bool,
    /// What the caller asked for through `set_visible` or `apply`.
    shown: bool,
}

impl NativeChild {
    /// Adds a hidden clip view to the view behind `window`'s AppKit handle.
    pub fn new(window: &impl HasWindowHandle) -> Result<NativeChild, NativeError> {
        let (parent, mtm) = host_view(window)?;
        let clip = ClipView::new(mtm);
        // Off by default on macOS 14+, so an oversized child would paint past the clip rect.
        clip.setClipsToBounds(true);
        clip.setHidden(true);
        let zoom = plain_layer_view(mtm);
        clip.addSubview(&zoom);
        parent.addSubview(&clip);
        Ok(NativeChild {
            clip,
            zoom,
            mask: None,
            occluded: false,
            shown: false,
        })
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
        self.occluded = placement.visible.is_empty();
        if !self.occluded {
            self.apply_mask(placement);
        }
        self.shown = true;
        self.clip.setHidden(self.occluded);
    }

    /// Masks paint to the visible pieces and makes hit testing match. Skips work when
    /// nothing changed, and removes the mask when the whole container is visible.
    fn apply_mask(&mut self, placement: &NativePlacement) {
        let Some(layer) = self.clip.layer() else {
            return;
        };
        let pieces = mask_pieces(placement, layer.isGeometryFlipped());
        let key = (placement.container.size, pieces);
        if self.mask.as_ref() == Some(&key) {
            return;
        }
        let (_, pieces) = &key;
        // No implicit animation, or the mask trails the content during pan and zoom.
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        match pieces {
            None => {
                // SAFETY: removing the mask of a layer we own, on the main thread.
                unsafe { layer.setMask(None) };
            }
            Some(pieces) => {
                // One path, not one layer per piece: separate layers antialias their shared
                // edges at fractional points and leave a see-through hairline.
                let path = CGMutablePath::new();
                for piece in pieces {
                    // SAFETY: a null transform is allowed; the path is ours and alive.
                    unsafe {
                        CGMutablePath::add_rect(Some(&path), std::ptr::null(), ns_rect(*piece))
                    };
                }
                let mask = CAShapeLayer::new();
                mask.setFrame(layer.bounds());
                // The default fill is opaque black with the non-zero rule, which is what the
                // mask needs: pieces are disjoint, so the union is every piece.
                mask.setPath(Some(&path));
                // SAFETY: a fresh layer that is not in any tree, set as the mask of a layer
                // we own, on the main thread.
                unsafe { layer.setMask(Some(&mask)) };
            }
        }
        CATransaction::commit();
        // Hit testing works in view coordinates, bottom-left for this unflipped view.
        let hit =
            mask_pieces(placement, false).map(|pieces| pieces.into_iter().map(ns_rect).collect());
        *self.clip.ivars().hit_pieces.borrow_mut() = hit;
        self.mask = Some(key);
    }

    pub fn set_visible(&mut self, visible: bool) {
        self.shown = visible;
        self.clip.setHidden(!visible || self.occluded);
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
        let clip = Retained::as_ptr(&self.clip).cast::<NSView>();
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

    pub(crate) fn clip_view(&self) -> Retained<NSView> {
        Retained::into_super(self.clip.clone())
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

/// Makes the window's host view, the GPUI canvas, the first responder, so no Ghostty
/// surface, web view or address field keeps the keyboard. False if AppKit refused or the
/// handle is not an AppKit view in a window.
pub fn focus_host_view(window: &impl HasWindowHandle) -> bool {
    let Ok((host, _)) = host_view(window) else {
        return false;
    };
    host.window()
        .is_some_and(|ns_window| ns_window.makeFirstResponder(Some(&host)))
}

/// Whether the window's first responder is the host view itself, not a native panel view
/// inside it. `None` when the handle is not an AppKit view in a window.
pub fn host_view_is_first_responder(window: &impl HasWindowHandle) -> Option<bool> {
    let (host, _) = host_view(window).ok()?;
    let responder = host.window()?.firstResponder();
    let host_ptr = Retained::as_ptr(&host).cast::<c_void>();
    Some(
        responder
            .is_some_and(|responder| Retained::as_ptr(&responder).cast::<c_void>() == host_ptr),
    )
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
            visible: vec![Rect::new(0.0, 36.0, 300.0, 214.0)],
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
            visible: vec![Rect::new(10.0, 20.0, 600.0, 450.0)],
        };
        assert_eq!(zoom_frame(&p), Rect::new(0.0, 0.0, 600.0, 450.0));
    }

    #[test]
    fn whole_container_visible_needs_no_mask() {
        assert_eq!(mask_pieces(&placement(), false), None);
    }

    #[test]
    fn mask_pieces_are_clip_local_in_both_origins() {
        let mut p = placement();
        // Container (0,36,300,214); the top 28 points are covered.
        p.visible = vec![Rect::new(0.0, 64.0, 300.0, 186.0)];
        // Bottom-left: the piece starts at the clip's bottom and stops 28 below its top.
        assert_eq!(
            mask_pieces(&p, false),
            Some(vec![Rect::new(0.0, 0.0, 300.0, 186.0)])
        );
        assert_eq!(
            mask_pieces(&p, true),
            Some(vec![Rect::new(0.0, 28.0, 300.0, 186.0)])
        );
        p.visible.clear();
        assert_eq!(mask_pieces(&p, false), Some(Vec::new()));
    }

    #[test]
    fn zoom_bounds_are_the_logical_size() {
        assert_eq!(
            zoom_bounds(Size::new(400.0, 300.0)),
            Rect::new(0.0, 0.0, 400.0, 300.0)
        );
    }
}
