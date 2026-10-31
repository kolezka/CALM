//! Snapping of panel rects to a world grid and to neighbor edges.

use crate::geometry::Rect;
use crate::interaction::ResizeEdge;

/// Default grid step, in world units.
pub const DEFAULT_SNAP_GRID: f64 = 8.0;
/// Default distance at which an edge snaps to a neighbor edge, in world units.
pub const DEFAULT_SNAP_EDGE: f64 = 8.0;
/// Grid steps offered in the Settings menu. 0.0 means off.
pub const SNAP_STEPS: &[f64] = &[0.0, 4.0, 8.0, 16.0, 32.0];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SnapConfig {
    /// Grid step in world units. `None` turns grid snapping off.
    pub grid: Option<f64>,
    /// Neighbor edge snap distance in world units.
    pub edge_distance: f64,
}

impl Default for SnapConfig {
    fn default() -> Self {
        Self {
            grid: Some(DEFAULT_SNAP_GRID),
            edge_distance: DEFAULT_SNAP_EDGE,
        }
    }
}

/// Nearest multiple of `grid`. Identity when the grid is off or not positive.
pub fn snap_value(v: f64, grid: Option<f64>) -> f64 {
    match grid {
        Some(step) if step > 0.0 => (v / step).round() * step,
        _ => v,
    }
}

/// Left and right edges of every neighbor, then top and bottom.
fn targets(others: &[Rect]) -> (Vec<f64>, Vec<f64>) {
    let xs = others
        .iter()
        .flat_map(|r| [r.origin.x, r.right()])
        .collect();
    let ys = others
        .iter()
        .flat_map(|r| [r.origin.y, r.bottom()])
        .collect();
    (xs, ys)
}

/// Smallest shift that puts one of `edges` on one of `targets`, if within `max`.
fn edge_shift(edges: &[f64], targets: &[f64], max: f64) -> Option<f64> {
    edges
        .iter()
        .flat_map(|edge| targets.iter().map(move |target| target - edge))
        .filter(|shift| shift.abs() <= max)
        .min_by(|a, b| a.abs().total_cmp(&b.abs()))
}

/// One coordinate: a nearby neighbor edge wins, otherwise the grid.
fn snap_edge(v: f64, targets: &[f64], cfg: &SnapConfig) -> f64 {
    match edge_shift(&[v], targets, cfg.edge_distance) {
        Some(shift) => v + shift,
        None => snap_value(v, cfg.grid),
    }
}

/// Shifts the whole rect; size is unchanged.
pub fn snap_move(rect: Rect, others: &[Rect], cfg: &SnapConfig) -> Rect {
    let (xs, ys) = targets(others);
    let x = match edge_shift(&[rect.origin.x, rect.right()], &xs, cfg.edge_distance) {
        Some(shift) => rect.origin.x + shift,
        None => snap_value(rect.origin.x, cfg.grid),
    };
    let y = match edge_shift(&[rect.origin.y, rect.bottom()], &ys, cfg.edge_distance) {
        Some(shift) => rect.origin.y + shift,
        None => snap_value(rect.origin.y, cfg.grid),
    };
    Rect::new(x, y, rect.size.width, rect.size.height)
}

/// Moves only the dragged right and/or bottom edge. The caller enforces `MIN_PANEL_SIZE`.
pub fn snap_resize(rect: Rect, edge: ResizeEdge, others: &[Rect], cfg: &SnapConfig) -> Rect {
    let (xs, ys) = targets(others);
    let mut out = rect;
    if matches!(edge, ResizeEdge::Right | ResizeEdge::BottomRight) {
        out.size.width = snap_edge(rect.right(), &xs, cfg) - rect.origin.x;
    }
    if matches!(edge, ResizeEdge::Bottom | ResizeEdge::BottomRight) {
        out.size.height = snap_edge(rect.bottom(), &ys, cfg) - rect.origin.y;
    }
    out
}

/// Snaps all four edges independently, for a panel being drawn.
pub fn snap_draw(rect: Rect, others: &[Rect], cfg: &SnapConfig) -> Rect {
    let (xs, ys) = targets(others);
    let left = snap_edge(rect.origin.x, &xs, cfg);
    let top = snap_edge(rect.origin.y, &ys, cfg);
    let right = snap_edge(rect.right(), &xs, cfg);
    let bottom = snap_edge(rect.bottom(), &ys, cfg);
    Rect::new(left, top, right - left, bottom - top)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn grid8() -> SnapConfig {
        SnapConfig::default()
    }

    #[test]
    fn snap_value_rounds_to_nearest_multiple() {
        assert_eq!(snap_value(13.0, Some(8.0)), 16.0);
        assert_eq!(snap_value(11.0, Some(8.0)), 8.0);
        assert_eq!(snap_value(21.0, Some(8.0)), 24.0);
        assert_eq!(snap_value(-5.0, Some(8.0)), -8.0);
        assert_eq!(snap_value(17.0, Some(4.0)), 16.0);
    }

    #[test]
    fn snap_value_off_is_identity() {
        assert_eq!(snap_value(13.3, None), 13.3);
        assert_eq!(snap_value(13.3, Some(0.0)), 13.3);
    }

    #[test]
    fn snap_move_snaps_origin_to_grid() {
        let rect = Rect::new(13.0, 21.0, 100.0, 50.0);
        assert_eq!(
            snap_move(rect, &[], &grid8()),
            Rect::new(16.0, 24.0, 100.0, 50.0)
        );
    }

    #[test]
    fn snap_move_edge_beats_grid() {
        // Left edge at 205: the grid alone would give 208, the neighbor's right edge is at 200.
        let neighbor = Rect::new(0.0, 0.0, 200.0, 100.0);
        let rect = Rect::new(205.0, 16.0, 100.0, 50.0);
        assert_eq!(
            snap_move(rect, &[neighbor], &grid8()),
            Rect::new(200.0, 16.0, 100.0, 50.0)
        );
        // Right edge lines up with the neighbor's left edge.
        let right_neighbor = Rect::new(400.0, 0.0, 100.0, 100.0);
        let rect = Rect::new(297.0, 16.0, 100.0, 50.0);
        assert_eq!(
            snap_move(rect, &[right_neighbor], &grid8()),
            Rect::new(300.0, 16.0, 100.0, 50.0)
        );
    }

    #[test]
    fn snap_move_ignores_far_edges() {
        let neighbor = Rect::new(0.0, 0.0, 200.0, 100.0);
        let rect = Rect::new(250.0, 16.0, 100.0, 50.0);
        assert_eq!(
            snap_move(rect, &[neighbor], &grid8()),
            Rect::new(248.0, 16.0, 100.0, 50.0)
        );
        let edge_only = SnapConfig {
            grid: None,
            ..grid8()
        };
        assert_eq!(snap_move(rect, &[neighbor], &edge_only), rect);
    }

    #[test]
    fn snap_resize_moves_only_dragged_edge() {
        let rect = Rect::new(13.0, 21.0, 100.0, 50.0);
        // Right edge 113 snaps to 112; origin and height stay.
        assert_eq!(
            snap_resize(rect, ResizeEdge::Right, &[], &grid8()),
            Rect::new(13.0, 21.0, 99.0, 50.0)
        );
        // Bottom edge 71 snaps to 72; origin and width stay.
        assert_eq!(
            snap_resize(rect, ResizeEdge::Bottom, &[], &grid8()),
            Rect::new(13.0, 21.0, 100.0, 51.0)
        );
        // A neighbor's left edge at 118 wins over the grid.
        let neighbor = Rect::new(118.0, 0.0, 100.0, 300.0);
        assert_eq!(
            snap_resize(rect, ResizeEdge::BottomRight, &[neighbor], &grid8()),
            Rect::new(13.0, 21.0, 105.0, 51.0)
        );
    }

    #[test]
    fn snap_draw_snaps_both_corners() {
        let rect = Rect::new(13.0, 21.0, 100.0, 50.0);
        assert_eq!(
            snap_draw(rect, &[], &grid8()),
            Rect::new(16.0, 24.0, 96.0, 48.0)
        );
        // Neighbor right edge at 10 pulls the left edge; the far corner still snaps to the grid.
        let neighbor = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert_eq!(
            snap_draw(rect, &[neighbor], &grid8()),
            Rect::new(10.0, 24.0, 102.0, 48.0)
        );
    }
}
