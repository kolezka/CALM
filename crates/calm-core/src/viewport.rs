use crate::camera::Camera;
use crate::geometry::{Point, Rect, Size};

/// Where to put a native view's container so AppKit clips, offsets and scales it.
#[derive(Clone, Debug, PartialEq)]
pub struct NativePlacement {
    /// Screen pixels, already clipped to the viewport.
    pub container: Rect,
    /// Offset of the content inside the container, in unzoomed content units.
    pub inner_origin: Point,
    /// The content's own size. Zoom never changes it, so the terminal grid stays put.
    pub logical_size: Size,
    pub zoom: f64,
    /// The parts of `container` not under a panel in front, disjoint, same space as
    /// `container`. `place_native` sets `[container]`; see `occlusion`.
    pub visible: Vec<Rect>,
}

/// Returns `None` when no part of the content is inside the viewport.
pub fn place_native(
    content_world: Rect,
    camera: &Camera,
    viewport: Rect,
) -> Option<NativePlacement> {
    let screen = camera.world_rect_to_screen(content_world);
    let container = screen.intersect(&viewport)?;
    Some(NativePlacement {
        container,
        inner_origin: Point::new(
            (container.origin.x - screen.origin.x) / camera.zoom,
            (container.origin.y - screen.origin.y) / camera.zoom,
        ),
        logical_size: content_world.size,
        zoom: camera.zoom,
        visible: vec![container],
    })
}

/// Maps a screen point to the content's unzoomed coordinates.
pub fn unzoom_mouse_position(screen: Point, placement: &NativePlacement) -> Point {
    Point::new(
        (screen.x - placement.container.origin.x) / placement.zoom + placement.inner_origin.x,
        (screen.y - placement.container.origin.y) / placement.zoom + placement.inner_origin.y,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::Camera;
    use crate::geometry::{Point, Rect, Size};

    const CONTENT: Rect = Rect::new(0.0, 0.0, 400.0, 300.0);
    const VIEWPORT: Rect = Rect::new(0.0, 36.0, 1280.0, 764.0);

    fn first_placement() -> NativePlacement {
        let camera = Camera {
            offset: Point::new(100.0, 50.0),
            zoom: 1.0,
        };
        place_native(CONTENT, &camera, VIEWPORT).unwrap()
    }

    fn second_placement() -> NativePlacement {
        let camera = Camera {
            offset: Point::new(0.0, -18.0),
            zoom: 2.0,
        };
        place_native(CONTENT, &camera, VIEWPORT).unwrap()
    }

    #[test]
    fn place_native_clips_to_viewport_and_offsets_inner() {
        let placement = first_placement();
        assert_eq!(placement.container, Rect::new(0.0, 36.0, 300.0, 214.0));
        assert_eq!(placement.inner_origin, Point::new(100.0, 86.0));
        assert_eq!(placement.logical_size, Size::new(400.0, 300.0));
        assert_eq!(placement.zoom, 1.0);
        assert_eq!(placement.visible, vec![placement.container]);
    }

    #[test]
    fn place_native_scales_logical_size_by_zoom() {
        let placement = second_placement();
        assert_eq!(placement.container, Rect::new(0.0, 36.0, 800.0, 600.0));
        assert_eq!(placement.inner_origin, Point::new(0.0, 0.0));
        assert_eq!(placement.logical_size, Size::new(400.0, 300.0));
        assert_eq!(placement.zoom, 2.0);
    }

    #[test]
    fn place_native_fully_offscreen_is_none() {
        let camera = Camera {
            offset: Point::new(5000.0, 5000.0),
            zoom: 1.0,
        };
        assert_eq!(place_native(CONTENT, &camera, VIEWPORT), None);
    }

    #[test]
    fn unzoom_mouse_position_divides_by_zoom_and_adds_inner_origin() {
        assert_eq!(
            unzoom_mouse_position(Point::new(50.0, 86.0), &first_placement()),
            Point::new(150.0, 136.0)
        );
        assert_eq!(
            unzoom_mouse_position(Point::new(200.0, 236.0), &second_placement()),
            Point::new(100.0, 100.0)
        );
    }
}
