use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Size {
    pub width: f64,
    pub height: f64,
}

impl Size {
    pub const fn new(width: f64, height: f64) -> Self {
        Self { width, height }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub struct Rect {
    pub origin: Point,
    pub size: Size,
}

impl Rect {
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self {
            origin: Point::new(x, y),
            size: Size::new(width, height),
        }
    }

    pub fn right(&self) -> f64 {
        self.origin.x + self.size.width
    }

    pub fn bottom(&self) -> f64 {
        self.origin.y + self.size.height
    }

    /// Half-open: the left and top edges are inside, the right and bottom edges are not.
    pub fn contains(&self, point: Point) -> bool {
        point.x >= self.origin.x
            && point.x < self.right()
            && point.y >= self.origin.y
            && point.y < self.bottom()
    }

    /// Returns `None` when the overlap has no area, so touching edges do not count.
    pub fn intersect(&self, other: &Rect) -> Option<Rect> {
        let left = self.origin.x.max(other.origin.x);
        let top = self.origin.y.max(other.origin.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        if right > left && bottom > top {
            Some(Rect::new(left, top, right - left, bottom - top))
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rect_intersect_disjoint_is_none() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(20.0, 20.0, 5.0, 5.0);
        assert_eq!(a.intersect(&b), None);
        let c = Rect::new(5.0, 5.0, 10.0, 10.0);
        assert_eq!(a.intersect(&c), Some(Rect::new(5.0, 5.0, 5.0, 5.0)));
    }
}
