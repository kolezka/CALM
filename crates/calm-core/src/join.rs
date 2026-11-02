//! Joined panels: neighbors whose facing edges sit within `JOIN_GAP` read as one block.
//! Pure geometry. Paint computes a `JoinLayout` once per frame and reads it per panel.

use std::collections::HashMap;

use crate::geometry::Rect;
use crate::scene::{Panel, PanelId};

/// Facing edges at most this far apart (and not overlapping) join, in world units.
pub const JOIN_GAP: f64 = 2.0;
/// The gap edge snapping leaves between facing edges. The divider fills it, so it never
/// covers a native view. A 0 gap would leave the divider no room over the content.
pub const JOIN_SNAP_GAP: f64 = 1.0;
/// How far a neighbor may fall short of a corner and still count as covering it.
pub const CORNER_TOLERANCE: f64 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Axis {
    /// Side by side; the divider is vertical.
    Row,
    /// Stacked; the divider is horizontal.
    Column,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    Left,
    Top,
    Right,
    Bottom,
}

impl Side {
    fn index(self) -> usize {
        match self {
            Side::Left => 0,
            Side::Top => 1,
            Side::Right => 2,
            Side::Bottom => 3,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Join {
    /// The left panel of a row, the top panel of a column.
    pub first: PanelId,
    pub second: PanelId,
    pub axis: Axis,
    pub gap: f64,
    /// The gap between the facing edges along the shared span. Zero wide when the gap is 0.
    pub divider: Rect,
}

/// Corners a joined neighbor covers. They lose their rounding.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SquareCorners {
    pub top_left: bool,
    pub top_right: bool,
    pub bottom_right: bool,
    pub bottom_left: bool,
}

/// How one panel's edges are joined.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct JoinedEdges {
    pub square: SquareCorners,
    gap: [Option<f64>; 4],
    open: [Vec<(f64, f64)>; 4],
}

impl JoinedEdges {
    /// Widest gap to a joined neighbor on `side`, `None` when that side is not joined.
    pub fn gap(&self, side: Side) -> Option<f64> {
        self.gap[side.index()]
    }

    /// Parts of a joined side no neighbor covers, as (start, end) along the side. These keep
    /// their ring. Empty for a side that is not joined or is fully covered.
    pub fn open(&self, side: Side) -> &[(f64, f64)] {
        &self.open[side.index()]
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct JoinLayout {
    pub joins: Vec<Join>,
    /// Groups of two or more joined panels, each in scene order (back to front).
    pub groups: Vec<Vec<PanelId>>,
    edges: HashMap<PanelId, JoinedEdges>,
}

impl JoinLayout {
    /// Minimized panels never join. O(n^2) over the panels: n stays in the tens.
    pub fn compute(panels: &[Panel]) -> Self {
        let rects: Vec<(PanelId, Rect)> = panels
            .iter()
            .filter(|panel| !panel.minimized)
            .map(|panel| (panel.id, panel.rect))
            .collect();
        let mut joins = Vec::new();
        let mut links = Vec::new();
        for (i, a) in rects.iter().enumerate() {
            for (j, b) in rects.iter().enumerate().skip(i + 1) {
                if let Some(join) = join_between(*a, *b) {
                    joins.push(join);
                    links.push((i, j));
                }
            }
        }

        let mut edges: HashMap<PanelId, JoinedEdges> = HashMap::new();
        let mut covered: HashMap<(PanelId, usize), Vec<(f64, f64)>> = HashMap::new();
        let rect_of = |id: PanelId| {
            rects
                .iter()
                .find(|(other, _)| *other == id)
                .map(|(_, rect)| *rect)
                .expect("join refers to a listed panel")
        };
        for join in &joins {
            let (a, b) = (rect_of(join.first), rect_of(join.second));
            let (first_side, second_side, span) = match join.axis {
                Axis::Row => (
                    Side::Right,
                    Side::Left,
                    (join.divider.origin.y, join.divider.bottom()),
                ),
                Axis::Column => (
                    Side::Bottom,
                    Side::Top,
                    (join.divider.origin.x, join.divider.right()),
                ),
            };
            for (id, side) in [(join.first, first_side), (join.second, second_side)] {
                let entry = edges.entry(id).or_default();
                let slot = &mut entry.gap[side.index()];
                *slot = Some(slot.map_or(join.gap, |gap| gap.max(join.gap)));
                covered.entry((id, side.index())).or_default().push(span);
            }
            let tol = CORNER_TOLERANCE;
            match join.axis {
                Axis::Row => {
                    let left = &mut edges.get_mut(&join.first).expect("inserted").square;
                    left.top_right |= b.origin.y <= a.origin.y + tol;
                    left.bottom_right |= b.bottom() >= a.bottom() - tol;
                    let right = &mut edges.get_mut(&join.second).expect("inserted").square;
                    right.top_left |= a.origin.y <= b.origin.y + tol;
                    right.bottom_left |= a.bottom() >= b.bottom() - tol;
                }
                Axis::Column => {
                    let top = &mut edges.get_mut(&join.first).expect("inserted").square;
                    top.bottom_left |= b.origin.x <= a.origin.x + tol;
                    top.bottom_right |= b.right() >= a.right() - tol;
                    let bottom = &mut edges.get_mut(&join.second).expect("inserted").square;
                    bottom.top_left |= a.origin.x <= b.origin.x + tol;
                    bottom.top_right |= a.right() >= b.right() - tol;
                }
            }
        }
        for ((id, side), spans) in covered {
            let rect = rect_of(id);
            let extent = if side == Side::Left.index() || side == Side::Right.index() {
                (rect.origin.y, rect.bottom())
            } else {
                (rect.origin.x, rect.right())
            };
            edges.get_mut(&id).expect("inserted").open[side] = uncovered(extent, spans);
        }

        JoinLayout {
            joins,
            groups: groups(&rects, &links),
            edges,
        }
    }

    pub fn edges(&self, id: PanelId) -> Option<&JoinedEdges> {
        self.edges.get(&id)
    }

    /// Joins with room for a visible divider in the gap.
    pub fn visible_dividers(&self) -> usize {
        self.joins.iter().filter(|join| join.gap > 0.0).count()
    }
}

fn join_between(a: (PanelId, Rect), b: (PanelId, Rect)) -> Option<Join> {
    if a.1.intersect(&b.1).is_some() {
        return None;
    }
    row(a, b)
        .or_else(|| row(b, a))
        .or_else(|| column(a, b))
        .or_else(|| column(b, a))
}

fn row(left: (PanelId, Rect), right: (PanelId, Rect)) -> Option<Join> {
    let (l, r) = (left.1, right.1);
    let gap = r.origin.x - l.right();
    let top = l.origin.y.max(r.origin.y);
    let bottom = l.bottom().min(r.bottom());
    ((0.0..=JOIN_GAP).contains(&gap) && bottom > top).then(|| Join {
        first: left.0,
        second: right.0,
        axis: Axis::Row,
        gap,
        divider: Rect::new(l.right(), top, gap, bottom - top),
    })
}

fn column(upper: (PanelId, Rect), lower: (PanelId, Rect)) -> Option<Join> {
    let (u, d) = (upper.1, lower.1);
    let gap = d.origin.y - u.bottom();
    let left = u.origin.x.max(d.origin.x);
    let right = u.right().min(d.right());
    ((0.0..=JOIN_GAP).contains(&gap) && right > left).then(|| Join {
        first: upper.0,
        second: lower.0,
        axis: Axis::Column,
        gap,
        divider: Rect::new(left, u.bottom(), right - left, gap),
    })
}

/// The parts of `extent` that none of `spans` cover.
fn uncovered(extent: (f64, f64), mut spans: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut open = Vec::new();
    let mut cursor = extent.0;
    for (start, end) in spans {
        if start > cursor {
            open.push((cursor, start.min(extent.1)));
        }
        cursor = cursor.max(end);
    }
    if cursor < extent.1 {
        open.push((cursor, extent.1));
    }
    open
}

/// Connected components of the join graph with two or more members.
fn groups(rects: &[(PanelId, Rect)], links: &[(usize, usize)]) -> Vec<Vec<PanelId>> {
    let mut parent: Vec<usize> = (0..rects.len()).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for &(a, b) in links {
        let (ra, rb) = (root(&mut parent, a), root(&mut parent, b));
        parent[ra] = rb;
    }
    let mut by_root: Vec<(usize, Vec<PanelId>)> = Vec::new();
    for (i, (id, _)) in rects.iter().enumerate() {
        let r = root(&mut parent, i);
        match by_root.iter_mut().find(|(other, _)| *other == r) {
            Some((_, members)) => members.push(*id),
            None => by_root.push((r, vec![*id])),
        }
    }
    by_root
        .into_iter()
        .map(|(_, members)| members)
        .filter(|members| members.len() > 1)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{PanelKind, Scene};

    fn scene(rects: &[Rect]) -> (Scene, Vec<PanelId>) {
        let mut scene = Scene::default();
        let ids = rects
            .iter()
            .map(|rect| {
                scene.add(
                    PanelKind::Terminal {
                        command: None,
                        cwd: None,
                        profile: None,
                        app: None,
                    },
                    *rect,
                )
            })
            .collect();
        (scene, ids)
    }

    fn layout(rects: &[Rect]) -> (JoinLayout, Vec<PanelId>) {
        let (scene, ids) = scene(rects);
        (JoinLayout::compute(&scene.panels), ids)
    }

    #[test]
    fn side_by_side_joins_with_a_divider_in_the_gap() {
        let (join, ids) = layout(&[
            Rect::new(0.0, 0.0, 300.0, 200.0),
            Rect::new(301.0, 0.0, 300.0, 200.0),
        ]);
        assert_eq!(
            join.joins,
            vec![Join {
                first: ids[0],
                second: ids[1],
                axis: Axis::Row,
                gap: 1.0,
                divider: Rect::new(300.0, 0.0, 1.0, 200.0),
            }]
        );
        assert_eq!(join.visible_dividers(), 1);
        let left = join.edges(ids[0]).unwrap();
        assert_eq!(
            left.square,
            SquareCorners {
                top_right: true,
                bottom_right: true,
                ..Default::default()
            }
        );
        assert_eq!(left.gap(Side::Right), Some(1.0));
        assert_eq!(left.gap(Side::Left), None);
        assert!(left.open(Side::Right).is_empty());
        let right = join.edges(ids[1]).unwrap();
        assert!(right.square.top_left && right.square.bottom_left);
        assert!(!right.square.top_right);
        assert_eq!(join.groups, vec![vec![ids[0], ids[1]]]);
    }

    #[test]
    fn stacked_panels_join_top_to_bottom() {
        let (join, ids) = layout(&[
            Rect::new(0.0, 0.0, 300.0, 200.0),
            Rect::new(0.0, 201.0, 300.0, 200.0),
        ]);
        assert_eq!(join.joins.len(), 1);
        assert_eq!(join.joins[0].axis, Axis::Column);
        assert_eq!(join.joins[0].divider, Rect::new(0.0, 200.0, 300.0, 1.0));
        let lower = join.edges(ids[1]).unwrap();
        assert!(lower.square.top_left && lower.square.top_right);
        assert_eq!(lower.gap(Side::Top), Some(1.0));
        let upper = join.edges(ids[0]).unwrap();
        assert!(!upper.square.top_left && !upper.square.top_right);
    }

    #[test]
    fn partial_overlap_joins_only_the_shared_segment() {
        // The right panel starts 50 lower and is shorter: it covers neither of the left
        // panel's right corners.
        let (join, ids) = layout(&[
            Rect::new(0.0, 0.0, 300.0, 200.0),
            Rect::new(301.0, 50.0, 300.0, 100.0),
        ]);
        assert_eq!(join.joins[0].divider, Rect::new(300.0, 50.0, 1.0, 100.0));
        let left = join.edges(ids[0]).unwrap();
        assert_eq!(left.square, SquareCorners::default());
        assert_eq!(left.open(Side::Right), &[(0.0, 50.0), (150.0, 200.0)]);
        // The left panel covers both of the right panel's left corners.
        let right = join.edges(ids[1]).unwrap();
        assert!(right.square.top_left && right.square.bottom_left);
        assert!(right.open(Side::Left).is_empty());
    }

    #[test]
    fn gap_just_inside_joins_and_just_outside_does_not() {
        let (inside, _) = layout(&[
            Rect::new(0.0, 0.0, 300.0, 200.0),
            Rect::new(300.0 + JOIN_GAP, 0.0, 300.0, 200.0),
        ]);
        assert_eq!(inside.joins.len(), 1);
        let (outside, ids) = layout(&[
            Rect::new(0.0, 0.0, 300.0, 200.0),
            Rect::new(300.0 + JOIN_GAP + 0.01, 0.0, 300.0, 200.0),
        ]);
        assert!(outside.joins.is_empty());
        assert!(outside.groups.is_empty());
        assert_eq!(outside.edges(ids[0]), None);
    }

    #[test]
    fn touching_edges_join_without_a_visible_divider() {
        let (join, _) = layout(&[
            Rect::new(0.0, 0.0, 300.0, 200.0),
            Rect::new(300.0, 0.0, 300.0, 200.0),
        ]);
        assert_eq!(join.joins.len(), 1);
        assert_eq!(join.visible_dividers(), 0);
    }

    #[test]
    fn grid_of_four_squares_the_inner_corners_only() {
        let (join, ids) = layout(&[
            Rect::new(0.0, 0.0, 300.0, 200.0),
            Rect::new(301.0, 0.0, 300.0, 200.0),
            Rect::new(0.0, 201.0, 300.0, 200.0),
            Rect::new(301.0, 201.0, 300.0, 200.0),
        ]);
        // Two rows and two columns; diagonal neighbors do not join.
        assert_eq!(join.joins.len(), 4);
        assert_eq!(join.groups.len(), 1);
        assert_eq!(join.groups[0].len(), 4);
        let top_left = join.edges(ids[0]).unwrap();
        assert!(!top_left.square.top_left);
        assert!(top_left.square.top_right);
        let bottom_right = join.edges(ids[3]).unwrap();
        assert!(bottom_right.square.top_left && bottom_right.square.top_right);
        assert!(!bottom_right.square.bottom_right);
        assert!(bottom_right.gap(Side::Top).is_some() && bottom_right.gap(Side::Left).is_some());
    }

    #[test]
    fn three_in_a_row_form_one_group_with_two_dividers() {
        let (join, ids) = layout(&[
            Rect::new(0.0, 0.0, 200.0, 200.0),
            Rect::new(201.0, 0.0, 200.0, 200.0),
            Rect::new(402.0, 0.0, 200.0, 200.0),
        ]);
        assert_eq!(join.visible_dividers(), 2);
        assert_eq!(join.groups, vec![ids.clone()]);
        let middle = join.edges(ids[1]).unwrap();
        assert!(middle.square.top_left && middle.square.top_right);
        let first = join.edges(ids[0]).unwrap();
        assert!(!first.square.top_left);
    }

    #[test]
    fn minimized_panel_never_joins() {
        let (mut scene, ids) = scene(&[
            Rect::new(0.0, 0.0, 300.0, 200.0),
            Rect::new(301.0, 0.0, 300.0, 200.0),
        ]);
        scene.get_mut(ids[1]).unwrap().minimized = true;
        let join = JoinLayout::compute(&scene.panels);
        assert!(join.joins.is_empty());
        assert_eq!(join.edges(ids[0]), None);
    }

    #[test]
    fn overlapping_and_diagonal_panels_do_not_join() {
        let (overlap, _) = layout(&[
            Rect::new(0.0, 0.0, 300.0, 200.0),
            Rect::new(299.0, 0.0, 300.0, 200.0),
        ]);
        assert!(overlap.joins.is_empty());
        // Corner to corner: no shared span.
        let (diagonal, _) = layout(&[
            Rect::new(0.0, 0.0, 300.0, 200.0),
            Rect::new(301.0, 201.0, 300.0, 200.0),
        ]);
        assert!(diagonal.joins.is_empty());
    }

    #[test]
    fn uncovered_merges_overlapping_spans() {
        assert_eq!(
            uncovered((0.0, 100.0), vec![(60.0, 80.0), (10.0, 30.0), (20.0, 40.0)]),
            vec![(0.0, 10.0), (40.0, 60.0), (80.0, 100.0)]
        );
        assert!(uncovered((0.0, 100.0), vec![(-5.0, 105.0)]).is_empty());
    }
}
