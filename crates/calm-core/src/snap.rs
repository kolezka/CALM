//! Snapping of panel rects to a world grid and to neighbor edges.

use crate::geometry::Rect;
use crate::interaction::ResizeEdge;
use crate::join::JOIN_SNAP_GAP;

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

/// Where each edge of a moving rect may land: in line with a neighbor's same-side edge, or
/// `JOIN_SNAP_GAP` off a neighbor's facing edge so the two panels join with a divider.
/// The facing edge itself is not a target, so a flush drop always leaves the divider room.
struct Targets {
    left: Vec<f64>,
    right: Vec<f64>,
    top: Vec<f64>,
    bottom: Vec<f64>,
}

fn targets(others: &[Rect]) -> Targets {
    let mut t = Targets {
        left: Vec::new(),
        right: Vec::new(),
        top: Vec::new(),
        bottom: Vec::new(),
    };
    for r in others {
        t.left.extend([r.origin.x, r.right() + JOIN_SNAP_GAP]);
        t.right.extend([r.right(), r.origin.x - JOIN_SNAP_GAP]);
        t.top.extend([r.origin.y, r.bottom() + JOIN_SNAP_GAP]);
        t.bottom.extend([r.bottom(), r.origin.y - JOIN_SNAP_GAP]);
    }
    t
}

/// Smallest shift that puts `edge` on one of `targets`, if within `max`.
fn edge_shift(edge: f64, targets: &[f64], max: f64) -> Option<f64> {
    targets
        .iter()
        .map(|target| target - edge)
        .filter(|shift| shift.abs() <= max)
        .min_by(|a, b| a.abs().total_cmp(&b.abs()))
}

/// The smaller of two optional shifts.
fn nearest(a: Option<f64>, b: Option<f64>) -> Option<f64> {
    match (a, b) {
        (Some(a), Some(b)) => Some(if b.abs() < a.abs() { b } else { a }),
        (a, None) => a,
        (None, b) => b,
    }
}

/// One coordinate: a nearby neighbor edge wins, otherwise the grid.
fn snap_edge(v: f64, targets: &[f64], cfg: &SnapConfig) -> f64 {
    match edge_shift(v, targets, cfg.edge_distance) {
        Some(shift) => v + shift,
        None => snap_value(v, cfg.grid),
    }
}

/// Shifts the whole rect; size is unchanged.
pub fn snap_move(rect: Rect, others: &[Rect], cfg: &SnapConfig) -> Rect {
    let t = targets(others);
    let max = cfg.edge_distance;
    let x = match nearest(
        edge_shift(rect.origin.x, &t.left, max),
        edge_shift(rect.right(), &t.right, max),
    ) {
        Some(shift) => rect.origin.x + shift,
        None => snap_value(rect.origin.x, cfg.grid),
    };
    let y = match nearest(
        edge_shift(rect.origin.y, &t.top, max),
        edge_shift(rect.bottom(), &t.bottom, max),
    ) {
        Some(shift) => rect.origin.y + shift,
        None => snap_value(rect.origin.y, cfg.grid),
    };
    Rect::new(x, y, rect.size.width, rect.size.height)
}

/// Moves only the dragged right and/or bottom edge. The caller enforces `MIN_PANEL_SIZE`.
pub fn snap_resize(rect: Rect, edge: ResizeEdge, others: &[Rect], cfg: &SnapConfig) -> Rect {
    let t = targets(others);
    let mut out = rect;
    if matches!(edge, ResizeEdge::Right | ResizeEdge::BottomRight) {
        out.size.width = snap_edge(rect.right(), &t.right, cfg) - rect.origin.x;
    }
    if matches!(edge, ResizeEdge::Bottom | ResizeEdge::BottomRight) {
        out.size.height = snap_edge(rect.bottom(), &t.bottom, cfg) - rect.origin.y;
    }
    out
}

/// Snaps all four edges independently, for a panel being drawn.
pub fn snap_draw(rect: Rect, others: &[Rect], cfg: &SnapConfig) -> Rect {
    let t = targets(others);
    let left = snap_edge(rect.origin.x, &t.left, cfg);
    let top = snap_edge(rect.origin.y, &t.top, cfg);
    let right = snap_edge(rect.right(), &t.right, cfg);
    let bottom = snap_edge(rect.bottom(), &t.bottom, cfg);
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
        // Left edge at 205: the grid alone would give 208; the neighbor's right edge is at
        // 200, so the panel joins it one unit off, leaving room for the divider.
        let neighbor = Rect::new(0.0, 0.0, 200.0, 100.0);
        let rect = Rect::new(205.0, 16.0, 100.0, 50.0);
        assert_eq!(
            snap_move(rect, &[neighbor], &grid8()),
            Rect::new(200.0 + JOIN_SNAP_GAP, 16.0, 100.0, 50.0)
        );
        // Right edge joins the neighbor's left edge.
        let right_neighbor = Rect::new(400.0, 0.0, 100.0, 100.0);
        let rect = Rect::new(297.0, 16.0, 100.0, 50.0);
        assert_eq!(
            snap_move(rect, &[right_neighbor], &grid8()),
            Rect::new(299.0, 16.0, 100.0, 50.0)
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
        // A neighbor's left edge at 118 wins over the grid: the right edge stops one unit
        // short of it.
        let neighbor = Rect::new(118.0, 0.0, 100.0, 300.0);
        assert_eq!(
            snap_resize(rect, ResizeEdge::BottomRight, &[neighbor], &grid8()),
            Rect::new(13.0, 21.0, 104.0, 51.0)
        );
    }

    #[test]
    fn snap_draw_snaps_both_corners() {
        let rect = Rect::new(13.0, 21.0, 100.0, 50.0);
        assert_eq!(
            snap_draw(rect, &[], &grid8()),
            Rect::new(16.0, 24.0, 96.0, 48.0)
        );
        // Neighbor right edge at 10 pulls the left edge to 11; the far corner still snaps
        // to the grid.
        let neighbor = Rect::new(0.0, 0.0, 10.0, 10.0);
        assert_eq!(
            snap_draw(rect, &[neighbor], &grid8()),
            Rect::new(11.0, 24.0, 101.0, 48.0)
        );
    }

    #[test]
    fn flush_drop_joins_instead_of_touching() {
        // Every drop within the snap distance of a facing edge lands exactly
        // JOIN_SNAP_GAP away, on each side, so the pair joins with a visible divider.
        let anchor = Rect::new(0.0, 0.0, 200.0, 200.0);
        let cases = [
            (Rect::new(203.0, 0.0, 150.0, 100.0), 201.0, 0.0),
            (Rect::new(198.0, 0.0, 150.0, 100.0), 201.0, 0.0),
            (Rect::new(-147.0, 0.0, 150.0, 100.0), -151.0, 0.0),
            (Rect::new(0.0, 205.0, 150.0, 100.0), 0.0, 201.0),
            (Rect::new(0.0, -103.0, 150.0, 100.0), 0.0, -101.0),
        ];
        for (rect, x, y) in cases {
            let snapped = snap_move(rect, &[anchor], &grid8());
            assert_eq!((snapped.origin.x, snapped.origin.y), (x, y), "{rect:?}");
            let join = crate::join::JoinLayout::compute(&{
                let mut scene = crate::scene::Scene::default();
                for r in [anchor, snapped] {
                    scene.add(
                        crate::scene::PanelKind::Terminal {
                            command: None,
                            cwd: None,
                        },
                        r,
                    );
                }
                scene.panels
            });
            assert_eq!(join.visible_dividers(), 1, "{rect:?}");
        }
    }
}
