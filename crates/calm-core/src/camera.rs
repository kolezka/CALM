use serde::{Deserialize, Serialize};

use crate::geometry::{Point, Rect, Size};

pub const MIN_ZOOM: f64 = 0.1;
pub const MAX_ZOOM: f64 = 4.0;
/// Screen pixels left free on each side when fitting panels in view.
pub const FIT_MARGIN: f64 = 32.0;

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Camera {
    /// World point shown at the viewport origin.
    pub offset: Point,
    pub zoom: f64,
}

impl Default for Camera {
    fn default() -> Self {
        Self {
            offset: Point::new(0.0, 0.0),
            zoom: 1.0,
        }
    }
}

impl Camera {
    pub fn world_to_screen(&self, world: Point) -> Point {
        Point::new(
            (world.x - self.offset.x) * self.zoom,
            (world.y - self.offset.y) * self.zoom,
        )
    }

    pub fn screen_to_world(&self, screen: Point) -> Point {
        Point::new(
            screen.x / self.zoom + self.offset.x,
            screen.y / self.zoom + self.offset.y,
        )
    }

    pub fn world_rect_to_screen(&self, world: Rect) -> Rect {
        Rect {
            origin: self.world_to_screen(world.origin),
            size: Size::new(world.size.width * self.zoom, world.size.height * self.zoom),
        }
    }

    /// Moves the canvas content by a screen delta, so it follows the pointer.
    pub fn pan_by_screen(&mut self, dx: f64, dy: f64) {
        self.offset.x -= dx / self.zoom;
        self.offset.y -= dy / self.zoom;
    }

    pub fn zoom_by(&mut self, factor: f64, anchor_screen: Point) {
        let anchor_world = self.screen_to_world(anchor_screen);
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        self.offset = Point::new(
            anchor_world.x - anchor_screen.x / self.zoom,
            anchor_world.y - anchor_screen.y / self.zoom,
        );
    }

    /// Keeps the zoom and moves `world` to the center of a viewport of `viewport` screen size.
    pub fn center_on(&mut self, world: Point, viewport: Size) {
        self.offset = Point::new(
            world.x - viewport.width / 2.0 / self.zoom,
            world.y - viewport.height / 2.0 / self.zoom,
        );
    }

    /// Zooms and centers so `bounds` fits with `margin` screen pixels on each side.
    /// Zoom stays within the limits, so a huge `bounds` can still overflow at `MIN_ZOOM`.
    /// `None` (no panels) keeps the zoom and centers on the world origin.
    pub fn fit(&mut self, bounds: Option<Rect>, viewport: Size, margin: f64) {
        let Some(bounds) = bounds else {
            self.center_on(Point::new(0.0, 0.0), viewport);
            return;
        };
        let available = Size::new(
            viewport.width - 2.0 * margin,
            viewport.height - 2.0 * margin,
        );
        // Before the first layout the viewport is empty; keep the zoom rather than divide by it.
        if available.width > 0.0 && available.height > 0.0 {
            let fit_axis = |space: f64, extent: f64| {
                if extent > 0.0 {
                    space / extent
                } else {
                    f64::INFINITY
                }
            };
            self.zoom = fit_axis(available.width, bounds.size.width)
                .min(fit_axis(available.height, bounds.size.height))
                .clamp(MIN_ZOOM, MAX_ZOOM);
        }
        let center = Point::new(
            bounds.origin.x + bounds.size.width / 2.0,
            bounds.origin.y + bounds.size.height / 2.0,
        );
        self.center_on(center, viewport);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::Point;

    fn assert_close(a: Point, b: Point) {
        assert!(
            (a.x - b.x).abs() < 1e-9 && (a.y - b.y).abs() < 1e-9,
            "{a:?} != {b:?}"
        );
    }

    #[test]
    fn world_to_screen_roundtrip() {
        let camera = Camera {
            offset: Point::new(100.0, 50.0),
            zoom: 2.0,
        };
        let world = Point::new(120.0, 70.0);
        let screen = camera.world_to_screen(world);
        assert_close(screen, Point::new(40.0, 40.0));
        assert_close(camera.screen_to_world(screen), world);
    }

    #[test]
    fn zoom_by_keeps_anchor_fixed() {
        let mut camera = Camera {
            offset: Point::new(30.0, -20.0),
            zoom: 1.2,
        };
        let anchor = Point::new(300.0, 200.0);
        let before = camera.screen_to_world(anchor);
        camera.zoom_by(1.5, anchor);
        assert!((camera.zoom - 1.8).abs() < 1e-9);
        assert_close(camera.screen_to_world(anchor), before);
    }

    #[test]
    fn fit_one_rect_centers_and_fills_the_tighter_axis() {
        let mut camera = Camera::default();
        let world = Rect::new(100.0, 200.0, 400.0, 100.0);
        camera.fit(Some(world), Size::new(1000.0, 800.0), 50.0);
        // Width limits: (1000 - 100) / 400 = 2.25.
        assert!((camera.zoom - 2.25).abs() < 1e-9);
        assert_close(
            camera.world_to_screen(Point::new(300.0, 250.0)),
            Point::new(500.0, 400.0),
        );
        let screen = camera.world_rect_to_screen(world);
        assert!((screen.origin.x - 50.0).abs() < 1e-9);
        assert!(screen.origin.y > 50.0);
    }

    #[test]
    fn fit_one_rect_clamps_zoom_for_tiny_and_huge_rects() {
        let mut camera = Camera::default();
        camera.fit(
            Some(Rect::new(0.0, 0.0, 10.0, 10.0)),
            Size::new(1000.0, 800.0),
            40.0,
        );
        assert_eq!(camera.zoom, MAX_ZOOM);
        assert_close(
            camera.world_to_screen(Point::new(5.0, 5.0)),
            Point::new(500.0, 400.0),
        );
        camera.fit(
            Some(Rect::new(0.0, 0.0, 100_000.0, 100.0)),
            Size::new(1000.0, 800.0),
            40.0,
        );
        assert_eq!(camera.zoom, MIN_ZOOM);
    }

    #[test]
    fn zoom_clamps_to_bounds() {
        let mut camera = Camera::default();
        camera.zoom_by(100.0, Point::new(0.0, 0.0));
        assert_eq!(camera.zoom, MAX_ZOOM);
        camera.zoom_by(0.001, Point::new(0.0, 0.0));
        assert_eq!(camera.zoom, MIN_ZOOM);
    }

    const VIEWPORT: Size = Size::new(1200.0, 800.0);

    fn viewport_center() -> Point {
        Point::new(VIEWPORT.width / 2.0, VIEWPORT.height / 2.0)
    }

    #[test]
    fn center_on_origin_lands_at_viewport_center_at_any_zoom() {
        for zoom in [MIN_ZOOM, 0.37, 1.0, 2.5, MAX_ZOOM] {
            let mut camera = Camera {
                offset: Point::new(-913.0, 4410.5),
                zoom,
            };
            camera.center_on(Point::new(0.0, 0.0), VIEWPORT);
            assert_eq!(camera.zoom, zoom);
            assert_close(
                camera.world_to_screen(Point::new(0.0, 0.0)),
                viewport_center(),
            );
        }
    }

    fn assert_inside_margin(camera: &Camera, world: Rect) {
        let screen = camera.world_rect_to_screen(world);
        let eps = 1e-9;
        assert!(
            screen.origin.x >= FIT_MARGIN - eps
                && screen.origin.y >= FIT_MARGIN - eps
                && screen.right() <= VIEWPORT.width - FIT_MARGIN + eps
                && screen.bottom() <= VIEWPORT.height - FIT_MARGIN + eps,
            "{world:?} maps to {screen:?}, outside the margin"
        );
    }

    #[test]
    fn fit_contains_every_panel_rect() {
        let rects = [
            Rect::new(-500.0, -300.0, 640.0, 400.0),
            Rect::new(900.0, 200.0, 800.0, 600.0),
            Rect::new(100.0, 1500.0, 200.0, 120.0),
        ];
        let bounds = rects.iter().copied().reduce(|a, b| a.union(&b));
        let mut camera = Camera {
            offset: Point::new(7000.0, -3000.0),
            zoom: 3.0,
        };
        camera.fit(bounds, VIEWPORT, FIT_MARGIN);
        for rect in rects {
            assert_inside_margin(&camera, rect);
        }
        // Tight: the limiting axis (height here) fills the space between the margins.
        let screen = camera.world_rect_to_screen(bounds.unwrap());
        assert!((screen.size.height - (VIEWPORT.height - 2.0 * FIT_MARGIN)).abs() < 1e-9);
    }

    #[test]
    fn fit_with_no_panels_keeps_zoom_and_centers_origin() {
        let mut camera = Camera {
            offset: Point::new(250.0, -80.0),
            zoom: 1.7,
        };
        camera.fit(None, VIEWPORT, FIT_MARGIN);
        assert_eq!(camera.zoom, 1.7);
        assert_close(
            camera.world_to_screen(Point::new(0.0, 0.0)),
            viewport_center(),
        );
    }

    #[test]
    fn fit_clamps_zoom_to_limits_and_still_centers() {
        let tiny = Rect::new(40.0, 40.0, 10.0, 10.0);
        let mut camera = Camera::default();
        camera.fit(Some(tiny), VIEWPORT, FIT_MARGIN);
        assert_eq!(camera.zoom, MAX_ZOOM);
        assert_close(
            camera.world_to_screen(Point::new(45.0, 45.0)),
            viewport_center(),
        );

        let huge = Rect::new(-1e6, -1e6, 2e6, 2e6);
        camera.fit(Some(huge), VIEWPORT, FIT_MARGIN);
        assert_eq!(camera.zoom, MIN_ZOOM);
        assert_close(
            camera.world_to_screen(Point::new(0.0, 0.0)),
            viewport_center(),
        );
    }

    #[test]
    fn fit_before_layout_keeps_zoom() {
        let mut camera = Camera::default();
        camera.fit(
            Some(Rect::new(0.0, 0.0, 100.0, 100.0)),
            Size::new(0.0, 0.0),
            FIT_MARGIN,
        );
        assert_eq!(camera.zoom, 1.0);
        assert!(camera.offset.x.is_finite() && camera.offset.y.is_finite());
    }

    #[test]
    fn pan_moves_offset_by_unzoomed_delta() {
        let mut camera = Camera {
            offset: Point::new(0.0, 0.0),
            zoom: 2.0,
        };
        camera.pan_by_screen(10.0, 0.0);
        assert_close(camera.offset, Point::new(-5.0, 0.0));
    }
}
