use serde::{Deserialize, Serialize};

use crate::geometry::{Point, Rect, Size};

pub const MIN_ZOOM: f64 = 0.1;
pub const MAX_ZOOM: f64 = 4.0;

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

    /// Centers `world` in a viewport of `viewport` pixels, as large as fits with `margin`
    /// pixels on each side. Zoom stays within the camera limits.
    pub fn fit_rect(&mut self, world: Rect, viewport: Size, margin: f64) {
        let avail_w = (viewport.width - 2.0 * margin).max(1.0);
        let avail_h = (viewport.height - 2.0 * margin).max(1.0);
        let zoom_w = avail_w / world.size.width.max(1.0);
        let zoom_h = avail_h / world.size.height.max(1.0);
        self.zoom = zoom_w.min(zoom_h).clamp(MIN_ZOOM, MAX_ZOOM);
        let center = Point::new(
            world.origin.x + world.size.width / 2.0,
            world.origin.y + world.size.height / 2.0,
        );
        self.offset = Point::new(
            center.x - viewport.width / 2.0 / self.zoom,
            center.y - viewport.height / 2.0 / self.zoom,
        );
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
    fn fit_rect_centers_and_fills_the_tighter_axis() {
        let mut camera = Camera::default();
        let world = Rect::new(100.0, 200.0, 400.0, 100.0);
        camera.fit_rect(world, Size::new(1000.0, 800.0), 50.0);
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
    fn fit_rect_clamps_zoom_for_tiny_and_huge_rects() {
        let mut camera = Camera::default();
        camera.fit_rect(
            Rect::new(0.0, 0.0, 10.0, 10.0),
            Size::new(1000.0, 800.0),
            40.0,
        );
        assert_eq!(camera.zoom, MAX_ZOOM);
        assert_close(
            camera.world_to_screen(Point::new(5.0, 5.0)),
            Point::new(500.0, 400.0),
        );
        camera.fit_rect(
            Rect::new(0.0, 0.0, 100_000.0, 100.0),
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
