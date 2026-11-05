//! Native views draw above everything GPUI paints, so a back panel's native view would
//! cover the chrome of a panel in front of it. This works out which part of a native
//! container is not under any panel above it, so AppKit can mask the rest.

use crate::camera::Camera;
use crate::chrome::RING_PX;
use crate::geometry::{Point, Rect};
use crate::join::Join;
use crate::scene::{PanelId, PanelKind, Scene, chip_rect, content_rect};

/// The screen rect a panel covers: its body (or chip when minimized) plus the ring around
/// it. Shadows are ignored. Same space as `camera.world_rect_to_screen`.
pub fn occluder_rect(panel: &crate::scene::Panel, camera: &Camera) -> Rect {
    let body = if panel.minimized {
        chip_rect(panel)
    } else {
        panel.rect
    };
    let screen = camera.world_rect_to_screen(body);
    Rect::new(
        screen.origin.x - RING_PX,
        screen.origin.y - RING_PX,
        screen.size.width + 2.0 * RING_PX,
        screen.size.height + 2.0 * RING_PX,
    )
}

/// `rect` minus `hole`, as up to four disjoint pieces: full-width bands above and below the
/// hole, then the parts left and right of it. Pieces without area are dropped.
pub fn subtract(rect: Rect, hole: Rect) -> Vec<Rect> {
    let Some(cut) = rect.intersect(&hole) else {
        return vec![rect];
    };
    let pieces = [
        // Above the cut, full width.
        Rect::new(
            rect.origin.x,
            rect.origin.y,
            rect.size.width,
            cut.origin.y - rect.origin.y,
        ),
        // Below the cut, full width.
        Rect::new(
            rect.origin.x,
            cut.bottom(),
            rect.size.width,
            rect.bottom() - cut.bottom(),
        ),
        // Left of the cut, cut height.
        Rect::new(
            rect.origin.x,
            cut.origin.y,
            cut.origin.x - rect.origin.x,
            cut.size.height,
        ),
        // Right of the cut, cut height.
        Rect::new(
            cut.right(),
            cut.origin.y,
            rect.right() - cut.right(),
            cut.size.height,
        ),
    ];
    pieces
        .into_iter()
        .filter(|piece| piece.size.width > 0.0 && piece.size.height > 0.0)
        .collect()
}

/// What is left of `container` after removing every occluder. The pieces are disjoint and
/// lie inside `container`; an untouched container comes back as `[container]`.
pub fn visible_region(container: Rect, occluders: impl IntoIterator<Item = Rect>) -> Vec<Rect> {
    let mut visible = vec![container];
    for occluder in occluders {
        if visible.is_empty() {
            break;
        }
        visible = visible
            .into_iter()
            .flat_map(|piece| subtract(piece, occluder))
            .collect();
    }
    visible
}

/// The visible part of panel `id`'s native container, in the same screen space as
/// `container`. `offset` is added to each occluder, for a container already moved into
/// window points. Panels after `id` in `scene.panels` (in front of it) occlude, and so does
/// the divider of every join in `joins` painted after `id`: paint puts a divider on right
/// after the later of its two panels, and it fills the gap the panels' rings leave open.
/// This is the input region: a press here belongs to panel `id`.
pub fn native_visible_region(
    scene: &Scene,
    joins: &[Join],
    id: PanelId,
    container: Rect,
    offset: Point,
) -> Vec<Rect> {
    native_paint_region(scene, joins, id, container, offset, &|_| false)
}

/// The part of panel `id`'s native container that may paint, as `native_visible_region`,
/// except that a front panel for which `live_native` is true occludes only its title bar
/// strip and ring. Its own native view draws above, so whatever shows through its content
/// is the panel below.
pub fn native_paint_region(
    scene: &Scene,
    joins: &[Join],
    id: PanelId,
    container: Rect,
    offset: Point,
    live_native: &dyn Fn(PanelId) -> bool,
) -> Vec<Rect> {
    let Some(index) = scene.panels.iter().position(|panel| panel.id == id) else {
        return vec![container];
    };
    let in_front = |other: PanelId| {
        scene.panels[index + 1..]
            .iter()
            .any(|panel| panel.id == other)
    };
    let panels = scene.panels[index + 1..].iter().flat_map(|panel| {
        let camera = scene.camera_of(panel);
        let outline = occluder_rect(panel, &camera);
        if live_native(panel.id) && !panel.minimized {
            subtract(
                outline,
                camera.world_rect_to_screen(content_rect(&scene.chrome, panel)),
            )
        } else {
            vec![outline]
        }
    });
    // Both panels of a join share one camera, so either gives the divider's.
    let dividers = joins
        .iter()
        .filter(|join| in_front(join.first) || in_front(join.second))
        .map(|join| {
            let camera = scene
                .get(join.first)
                .map_or(scene.camera, |panel| scene.camera_of(panel));
            camera.world_rect_to_screen(join.divider)
        });
    let occluders = panels.chain(dividers).map(|mut rect| {
        rect.origin.x += offset.x;
        rect.origin.y += offset.y;
        rect
    });
    visible_region(container, occluders)
}

/// Whether a live panel of `kind` lets the panels behind it paint under its content. Its
/// own native view draws above them and may be see-through: a terminal with content
/// opacity, or a markdown page with a faded background.
pub fn shows_through(kind: &PanelKind) -> bool {
    kind.is_terminal() || matches!(kind, PanelKind::Markdown { .. })
}

/// Whether `point` is inside any of `pieces` (half-open, as `Rect::contains`).
pub fn region_contains(pieces: &[Rect], point: Point) -> bool {
    pieces.iter().any(|piece| piece.contains(point))
}

/// Which pointer events a partly covered native view takes. A press lands only on a visible
/// part; the rest belongs to the panel in front. Motion and the matching release follow a
/// press the view took, so a drag that starts inside keeps tracking over a covered part.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PointerGate {
    /// One bit per button index the view took a press for.
    pressed: u32,
}

impl PointerGate {
    /// Whether to deliver a press of `button` at a point that is or is not `visible`. A
    /// delivered press is remembered until its release.
    pub fn press(&mut self, button: u8, visible: bool) -> bool {
        if visible {
            self.pressed |= Self::bit(button);
        }
        visible
    }

    /// Whether to deliver a release of `button`: only when the view took its press.
    pub fn release(&mut self, button: u8) -> bool {
        let taken = self.pressed & Self::bit(button) != 0;
        self.pressed &= !Self::bit(button);
        taken
    }

    /// Whether to deliver motion. `any_held` is whether the platform reports a button down;
    /// when none is, any remembered press is dropped, since its release went elsewhere.
    pub fn motion(&mut self, visible: bool, any_held: bool) -> bool {
        if !any_held {
            self.pressed = 0;
        }
        visible || self.pressed != 0
    }

    fn bit(button: u8) -> u32 {
        1 << (button % 32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::ChromeMetrics;
    use crate::scene::MINIMIZED_CHIP;

    const CONTAINER: Rect = Rect::new(0.0, 0.0, 100.0, 100.0);

    fn area(pieces: &[Rect]) -> f64 {
        pieces.iter().map(|r| r.size.width * r.size.height).sum()
    }

    fn assert_disjoint_inside(pieces: &[Rect], container: Rect) {
        for (i, a) in pieces.iter().enumerate() {
            assert!(
                a.size.width > 0.0 && a.size.height > 0.0,
                "empty piece {a:?}"
            );
            assert_eq!(
                a.intersect(&container),
                Some(*a),
                "{a:?} leaves {container:?}"
            );
            for b in &pieces[i + 1..] {
                assert_eq!(a.intersect(b), None, "{a:?} overlaps {b:?}");
            }
        }
    }

    /// No piece may overlap an occluder, and every sample point outside all occluders must
    /// be covered. Checks the region against its definition on a fine grid.
    fn assert_exact(pieces: &[Rect], container: Rect, occluders: &[Rect]) {
        let step = 0.5;
        let mut y = container.origin.y + step / 2.0;
        while y < container.bottom() {
            let mut x = container.origin.x + step / 2.0;
            while x < container.right() {
                let p = Point::new(x, y);
                let hidden = occluders.iter().any(|o| o.contains(p));
                assert_eq!(region_contains(pieces, p), !hidden, "at {p:?}");
                x += step;
            }
            y += step;
        }
    }

    #[test]
    fn no_occluders_is_the_container() {
        assert_eq!(visible_region(CONTAINER, []), vec![CONTAINER]);
    }

    #[test]
    fn occluder_outside_leaves_the_container() {
        let far = Rect::new(200.0, 200.0, 10.0, 10.0);
        assert_eq!(visible_region(CONTAINER, [far]), vec![CONTAINER]);
        // Touching edges do not count.
        let touching = Rect::new(100.0, 0.0, 10.0, 100.0);
        assert_eq!(visible_region(CONTAINER, [touching]), vec![CONTAINER]);
    }

    #[test]
    fn full_cover_is_empty() {
        let cover = Rect::new(-5.0, -5.0, 200.0, 200.0);
        assert_eq!(visible_region(CONTAINER, [cover]), Vec::<Rect>::new());
    }

    #[test]
    fn corner_overlap_leaves_two_pieces() {
        let corner = Rect::new(60.0, 60.0, 100.0, 100.0);
        let pieces = visible_region(CONTAINER, [corner]);
        assert_eq!(pieces.len(), 2);
        assert_disjoint_inside(&pieces, CONTAINER);
        assert_eq!(area(&pieces), 100.0 * 100.0 - 40.0 * 40.0);
        assert_exact(&pieces, CONTAINER, &[corner]);
    }

    #[test]
    fn edge_strip_leaves_one_piece() {
        // A title bar sized strip across the top.
        let strip = Rect::new(-10.0, -10.0, 300.0, 38.0);
        let pieces = visible_region(CONTAINER, [strip]);
        assert_eq!(pieces, vec![Rect::new(0.0, 28.0, 100.0, 72.0)]);
    }

    #[test]
    fn hole_in_the_middle_leaves_four_pieces() {
        let hole = Rect::new(25.0, 25.0, 50.0, 50.0);
        let pieces = visible_region(CONTAINER, [hole]);
        assert_eq!(pieces.len(), 4);
        assert_disjoint_inside(&pieces, CONTAINER);
        assert_eq!(area(&pieces), 100.0 * 100.0 - 50.0 * 50.0);
        assert_exact(&pieces, CONTAINER, &[hole]);
    }

    #[test]
    fn overlapping_occluders_subtract_their_union_once() {
        let a = Rect::new(10.0, 10.0, 50.0, 50.0);
        let b = Rect::new(40.0, 40.0, 50.0, 50.0);
        let pieces = visible_region(CONTAINER, [a, b]);
        assert_disjoint_inside(&pieces, CONTAINER);
        // Union is 2500 + 2500 - 400 (the 20x20 overlap).
        assert_eq!(area(&pieces), 100.0 * 100.0 - (2500.0 + 2500.0 - 400.0));
        assert_exact(&pieces, CONTAINER, &[a, b]);
    }

    #[test]
    fn many_occluders_stay_exact() {
        let occluders = [
            Rect::new(5.0, 5.0, 30.0, 70.0),
            Rect::new(20.0, 50.0, 70.0, 20.0),
            Rect::new(60.0, -10.0, 15.0, 120.0),
            Rect::new(0.0, 90.0, 100.0, 5.0),
        ];
        let pieces = visible_region(CONTAINER, occluders);
        assert_disjoint_inside(&pieces, CONTAINER);
        assert_exact(&pieces, CONTAINER, &occluders);
    }

    fn terminal() -> PanelKind {
        PanelKind::Terminal {
            command: None,
            cwd: None,
            profile: None,
            app: None,
        }
    }

    fn scene_with_two() -> (Scene, PanelId, PanelId) {
        let mut scene = Scene::default();
        let back = scene.add(terminal(), Rect::new(0.0, 0.0, 400.0, 300.0));
        let front = scene.add(terminal(), Rect::new(200.0, 100.0, 400.0, 300.0));
        (scene, back, front)
    }

    #[test]
    fn panel_above_occludes_with_its_ring() {
        let (scene, back, _) = scene_with_two();
        let container = Rect::new(0.0, 28.0, 400.0, 272.0);
        let pieces = native_visible_region(&scene, &[], back, container, Point::new(0.0, 0.0));
        let front_screen = Rect::new(199.0, 99.0, 402.0, 302.0);
        assert_disjoint_inside(&pieces, container);
        assert_exact(&pieces, container, &[front_screen]);
        // The ring pixel left of the front panel is masked too.
        assert!(!region_contains(&pieces, Point::new(199.5, 150.0)));
        assert!(region_contains(&pieces, Point::new(198.5, 150.0)));
    }

    #[test]
    fn panel_below_does_not_occlude() {
        let (scene, _, front) = scene_with_two();
        let container = Rect::new(200.0, 128.0, 400.0, 272.0);
        assert_eq!(
            native_visible_region(&scene, &[], front, container, Point::new(0.0, 0.0)),
            vec![container]
        );
    }

    #[test]
    fn offset_moves_occluders_into_window_points() {
        let (scene, back, _) = scene_with_two();
        let offset = Point::new(10.0, 36.0);
        let container = Rect::new(10.0, 64.0, 400.0, 272.0);
        let pieces = native_visible_region(&scene, &[], back, container, offset);
        let front_window = Rect::new(209.0, 135.0, 402.0, 302.0);
        assert_exact(&pieces, container, &[front_window]);
    }

    #[test]
    fn camera_zoom_scales_the_occluder() {
        let (mut scene, back, _) = scene_with_two();
        scene.camera.zoom = 2.0;
        let container = Rect::new(0.0, 56.0, 800.0, 544.0);
        let pieces = native_visible_region(&scene, &[], back, container, Point::new(0.0, 0.0));
        // Ring stays 1 screen pixel at any zoom.
        assert_exact(&pieces, container, &[Rect::new(399.0, 199.0, 802.0, 602.0)]);
    }

    #[test]
    fn minimized_panel_above_occludes_its_chip_only() {
        let (mut scene, back, front) = scene_with_two();
        scene.get_mut(front).unwrap().minimized = true;
        let container = Rect::new(0.0, 28.0, 400.0, 272.0);
        let pieces = native_visible_region(&scene, &[], back, container, Point::new(0.0, 0.0));
        let chip = Rect::new(
            199.0,
            99.0,
            MINIMIZED_CHIP.width + 2.0,
            MINIMIZED_CHIP.height + 2.0,
        );
        assert_exact(&pieces, container, &[chip]);
        // Below the chip, where the full panel would have been, stays visible.
        assert!(region_contains(&pieces, Point::new(300.0, 250.0)));
    }

    /// A rear panel with a joined pair in front, 1 world unit apart, at zoom 4. Returns the
    /// scene, the joins, the rear id and the rear container on screen.
    fn rear_behind_joined_pair() -> (Scene, Vec<Join>, PanelId, Rect) {
        let mut scene = Scene::default();
        let rear = scene.add(terminal(), Rect::new(0.0, 0.0, 1000.0, 600.0));
        scene.add(terminal(), Rect::new(100.0, 100.0, 200.0, 200.0));
        scene.add(terminal(), Rect::new(301.0, 100.0, 200.0, 200.0));
        scene.camera.zoom = 4.0;
        let joins = crate::join::JoinLayout::compute(&scene.panels).joins;
        assert_eq!(joins.len(), 1);
        let container = scene
            .camera
            .world_rect_to_screen(crate::scene::content_rect(
                &ChromeMetrics::default(),
                scene.get(rear).unwrap(),
            ));
        (scene, joins, rear, container)
    }

    #[test]
    fn divider_of_a_joined_pair_in_front_occludes() {
        let (scene, joins, rear, container) = rear_behind_joined_pair();
        let pieces = native_visible_region(&scene, &joins, rear, container, Point::new(0.0, 0.0));
        // The gap is 1200..1204 on screen; the rings cover 1200..1201 and 1203..1204.
        let gap_center = Point::new(1202.0, 800.0);
        assert!(!region_contains(&pieces, gap_center), "gap center exposed");
        let occluders = [
            Rect::new(399.0, 399.0, 802.0, 802.0),
            Rect::new(1203.0, 399.0, 802.0, 802.0),
            Rect::new(1200.0, 400.0, 4.0, 800.0),
        ];
        assert_disjoint_inside(&pieces, container);
        assert_exact(&pieces, container, &occluders);
    }

    #[test]
    fn divider_offset_moves_with_the_panels() {
        let (scene, joins, rear, mut container) = rear_behind_joined_pair();
        let offset = Point::new(10.0, 36.0);
        container.origin.x += offset.x;
        container.origin.y += offset.y;
        let pieces = native_visible_region(&scene, &joins, rear, container, offset);
        assert!(!region_contains(&pieces, Point::new(1212.0, 836.0)));
    }

    #[test]
    fn join_behind_the_target_does_not_occlude() {
        let mut scene = Scene::default();
        scene.add(terminal(), Rect::new(100.0, 100.0, 200.0, 200.0));
        scene.add(terminal(), Rect::new(301.0, 100.0, 200.0, 200.0));
        // In front of both, covering their gap.
        let target = scene.add(terminal(), Rect::new(250.0, 50.0, 200.0, 300.0));
        scene.camera.zoom = 4.0;
        let joins = crate::join::JoinLayout::compute(&scene.panels).joins;
        assert_eq!(joins.len(), 1);
        let container = scene
            .camera
            .world_rect_to_screen(crate::scene::content_rect(
                &ChromeMetrics::default(),
                scene.get(target).unwrap(),
            ));
        assert_eq!(
            native_visible_region(&scene, &joins, target, container, Point::new(0.0, 0.0)),
            vec![container]
        );
    }

    #[test]
    fn own_join_does_not_cut_into_own_content() {
        let mut scene = Scene::default();
        let left = scene.add(terminal(), Rect::new(0.0, 0.0, 300.0, 200.0));
        let right = scene.add(terminal(), Rect::new(301.0, 0.0, 300.0, 200.0));
        let below = scene.add(terminal(), Rect::new(0.0, 201.0, 300.0, 200.0));
        scene.camera.zoom = 4.0;
        let joins = crate::join::JoinLayout::compute(&scene.panels).joins;
        assert_eq!(joins.len(), 2);
        let content = |id| {
            scene
                .camera
                .world_rect_to_screen(crate::scene::content_rect(
                    &ChromeMetrics::default(),
                    scene.get(id).unwrap(),
                ))
        };
        // Both of the left panel's dividers are painted above it, yet sit outside it.
        for id in [left, right, below] {
            let container = content(id);
            assert_eq!(
                native_visible_region(&scene, &joins, id, container, Point::new(0.0, 0.0)),
                vec![container],
                "panel {id:?}"
            );
        }
    }

    /// Back 0..200 and front 100..300 on both axes, overlapping; content below a 28 bar.
    fn overlapping_pair() -> (Scene, PanelId, PanelId, Rect) {
        let mut scene = Scene::default();
        let back = scene.add(terminal(), Rect::new(0.0, 0.0, 200.0, 200.0));
        let front = scene.add(terminal(), Rect::new(100.0, 100.0, 200.0, 200.0));
        let container = scene
            .camera
            .world_rect_to_screen(crate::scene::content_rect(
                &ChromeMetrics::default(),
                scene.get(back).unwrap(),
            ));
        (scene, back, front, container)
    }

    const ORIGIN: Point = Point::new(0.0, 0.0);
    const IN_FRONT_CONTENT: Point = Point::new(150.0, 150.0);
    const IN_FRONT_BAR: Point = Point::new(150.0, 110.0);
    const IN_FRONT_RING: Point = Point::new(99.5, 150.0);

    #[test]
    fn paint_region_keeps_a_back_panel_under_a_live_front_terminal_content() {
        let (scene, back, front, container) = overlapping_pair();
        let live = move |id: PanelId| id == front;
        let pieces = native_paint_region(&scene, &[], back, container, ORIGIN, &live);
        assert_disjoint_inside(&pieces, container);
        assert!(
            region_contains(&pieces, IN_FRONT_CONTENT),
            "content cut out"
        );
        assert!(!region_contains(&pieces, IN_FRONT_BAR), "title bar exposed");
        assert!(!region_contains(&pieces, IN_FRONT_RING), "ring exposed");
        let front_panel = scene.get(front).unwrap();
        let outline = occluder_rect(front_panel, &scene.camera);
        let content = crate::scene::content_rect(&ChromeMetrics::default(), front_panel);
        assert_exact(&pieces, container, &subtract(outline, content));
    }

    #[test]
    fn paint_region_still_cuts_under_a_preview_or_minimized_front() {
        let (mut scene, back, front, container) = overlapping_pair();
        let visible = native_visible_region(&scene, &[], back, container, ORIGIN);
        let not_live = |_: PanelId| false;
        assert_eq!(
            native_paint_region(&scene, &[], back, container, ORIGIN, &not_live),
            visible
        );
        // A minimized front is a GPUI chip even if the caller calls it live.
        scene.get_mut(front).unwrap().minimized = true;
        let live = move |id: PanelId| id == front;
        assert_eq!(
            native_paint_region(&scene, &[], back, container, ORIGIN, &live),
            native_visible_region(&scene, &[], back, container, ORIGIN)
        );
    }

    #[test]
    fn a_live_markdown_panel_in_front_lets_the_terminal_behind_paint_but_not_take_clicks() {
        let (mut scene, back, front, container) = overlapping_pair();
        scene.get_mut(front).unwrap().kind = PanelKind::Markdown {
            text: "# Note".into(),
        };
        // As the app builds it: live panels whose kind shows through.
        let live = |id: PanelId| {
            scene
                .get(id)
                .is_some_and(|panel| shows_through(&panel.kind))
        };
        let paint = native_paint_region(&scene, &[], back, container, ORIGIN, &live);
        assert_disjoint_inside(&paint, container);
        assert!(
            region_contains(&paint, IN_FRONT_CONTENT),
            "the terminal shows through the markdown page"
        );
        assert!(!region_contains(&paint, IN_FRONT_BAR), "title bar exposed");
        assert!(!region_contains(&paint, IN_FRONT_RING), "ring exposed");
        let input = native_visible_region(&scene, &[], back, container, ORIGIN);
        assert!(
            !region_contains(&input, IN_FRONT_CONTENT),
            "clicks on the markdown page go to it"
        );
        // Pages that paint their own opaque content still cut.
        let browser = PanelKind::Browser {
            url: "https://example.com".into(),
            page_zoom: None,
        };
        assert!(!shows_through(&browser));
        assert!(shows_through(&terminal()));
    }

    #[test]
    fn input_region_is_unchanged_by_live_front_panels() {
        let (scene, back, front, container) = overlapping_pair();
        let visible = native_visible_region(&scene, &[], back, container, ORIGIN);
        let outline = occluder_rect(scene.get(front).unwrap(), &scene.camera);
        assert_eq!(visible, visible_region(container, [outline]));
        assert!(!region_contains(&visible, IN_FRONT_CONTENT));
    }

    #[test]
    fn paint_region_keeps_dividers_in_front_of_a_live_pair() {
        let (scene, joins, rear, container) = rear_behind_joined_pair();
        let live = |_: PanelId| true;
        let pieces = native_paint_region(&scene, &joins, rear, container, ORIGIN, &live);
        assert!(
            !region_contains(&pieces, Point::new(1202.0, 800.0)),
            "gap exposed"
        );
        // Inside the left pair panel's content, the rear shows through.
        assert!(region_contains(&pieces, Point::new(800.0, 800.0)));
    }

    #[test]
    fn gate_drops_presses_on_a_covered_part() {
        let mut gate = PointerGate::default();
        assert!(!gate.press(0, false));
        // Motion over the panel in front, with the button still down, stays out.
        assert!(!gate.motion(false, true));
        // So does the release of the press the view never took.
        assert!(!gate.release(0));
    }

    #[test]
    fn gate_keeps_a_drag_that_started_inside() {
        let mut gate = PointerGate::default();
        assert!(gate.press(0, true));
        assert!(
            gate.motion(false, true),
            "drag over a covered part keeps tracking"
        );
        assert!(gate.release(0), "release outside ends the drag");
        assert!(!gate.motion(false, false));
        assert!(!gate.release(0), "a second release is not delivered");
    }

    #[test]
    fn gate_tracks_buttons_separately() {
        let mut gate = PointerGate::default();
        assert!(gate.press(0, true));
        assert!(!gate.press(2, false));
        assert!(!gate.release(2));
        assert!(gate.motion(false, true));
        assert!(gate.release(0));
        assert!(!gate.motion(false, true));
    }

    #[test]
    fn gate_passes_motion_over_a_visible_part() {
        let mut gate = PointerGate::default();
        assert!(gate.motion(true, false));
        assert!(gate.motion(true, true));
    }

    #[test]
    fn gate_forgets_a_press_whose_release_went_elsewhere() {
        let mut gate = PointerGate::default();
        assert!(gate.press(0, true));
        // The button came up outside the view; the next motion reports nothing held.
        assert!(!gate.motion(false, false));
        assert!(!gate.motion(false, true));
        assert!(!gate.release(0));
    }

    #[test]
    fn unknown_panel_is_unmasked() {
        let (scene, _, _) = scene_with_two();
        assert_eq!(
            native_visible_region(&scene, &[], PanelId(999), CONTAINER, Point::new(0.0, 0.0)),
            vec![CONTAINER]
        );
    }

    #[test]
    fn native_paint_region_uses_pin_camera() {
        let mut scene = Scene::default();
        let back = scene.add(terminal(), Rect::new(0.0, 0.0, 1000.0, 1000.0));
        scene.camera.zoom = 2.0;
        let front = scene.add(terminal(), Rect::new(50.0, 50.0, 100.0, 100.0));
        assert!(scene.set_pinned(front, true));
        scene.camera.zoom = 0.5;
        let container = scene
            .camera
            .world_rect_to_screen(crate::scene::content_rect(
                &ChromeMetrics::default(),
                scene.get(back).unwrap(),
            ));
        let front_panel = scene.get(front).unwrap();
        let outline = occluder_rect(front_panel, &scene.camera_of(front_panel));
        let not_live = |_: PanelId| false;
        assert_eq!(
            native_paint_region(&scene, &[], back, container, ORIGIN, &not_live),
            visible_region(container, [outline])
        );
        // A live pinned front keeps its content hole in its own camera.
        let live = move |id: PanelId| id == front;
        let content =
            scene
                .camera_of(front_panel)
                .world_rect_to_screen(crate::scene::content_rect(
                    &ChromeMetrics::default(),
                    front_panel,
                ));
        let pieces = native_paint_region(&scene, &[], back, container, ORIGIN, &live);
        assert_exact(&pieces, container, &subtract(outline, content));
    }
}
