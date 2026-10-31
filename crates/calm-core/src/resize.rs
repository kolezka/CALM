//! Who may resize a panel, within which limits, and where its resize handles are.
//!
//! Drag, hit test and the hover cursor all read this module, so they cannot disagree.
//! Native views (WKWebView) keep every click inside their rect, so each panel also has a
//! thin grab band just outside its right and bottom edges, in pixels GPUI owns.

use crate::geometry::{Point, Rect, Size};
use crate::interaction::ResizeEdge;
use crate::join::JoinLayout;
use crate::scene::{MIN_PANEL_SIZE, Panel, PanelId, PanelKind, Scene, chip_rect};

/// Grab band inside a panel's right and bottom edges, in world units. It only reaches
/// GPUI where no native view takes the click: terminals pass clicks through, web views
/// do not.
pub const RESIZE_BAND: f64 = 12.0;
/// Grab band outside a panel's right and bottom edges, in screen pixels, so it stays
/// grabbable at any zoom. GPUI owns these pixels for every panel kind.
pub const RESIZE_OUTER_PX: f64 = 8.0;

/// A plugin's resize settings, as its manifest declares them. calm-plugins validates
/// them; this type carries the checked values.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PluginResize {
    pub resizable: bool,
    pub min: Option<Size>,
    pub max: Option<Size>,
}

impl Default for PluginResize {
    fn default() -> Self {
        Self {
            resizable: true,
            min: None,
            max: None,
        }
    }
}

/// Whether a panel can be resized and the sizes a resize may reach, chrome included.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResizePolicy {
    pub resizable: bool,
    pub min: Size,
    pub max: Option<Size>,
}

impl ResizePolicy {
    /// Terminals, browsers and plugins without resize settings.
    pub const FREE: ResizePolicy = ResizePolicy {
        resizable: true,
        min: MIN_PANEL_SIZE,
        max: None,
    };
    pub const LOCKED: ResizePolicy = ResizePolicy {
        resizable: false,
        min: MIN_PANEL_SIZE,
        max: None,
    };

    /// Every kind resizes freely. Only a plugin's manifest can lock it or set limits;
    /// `plugin` is ignored for other kinds.
    pub fn derive(kind: &PanelKind, plugin: Option<&PluginResize>) -> ResizePolicy {
        let (PanelKind::Plugin { .. }, Some(spec)) = (kind, plugin) else {
            return ResizePolicy::FREE;
        };
        if !spec.resizable {
            return ResizePolicy::LOCKED;
        }
        // The manifest refuses a minimum below the scene floor; any other caller still
        // gets the floor.
        let min = spec.min.map_or(MIN_PANEL_SIZE, |min| {
            Size::new(
                min.width.max(MIN_PANEL_SIZE.width),
                min.height.max(MIN_PANEL_SIZE.height),
            )
        });
        ResizePolicy {
            resizable: true,
            min,
            max: spec.max,
        }
    }

    /// `size` held within the limits. The minimum wins if the limits cross.
    pub fn clamp(&self, size: Size) -> Size {
        let (max_width, max_height) = self.max.map_or((f64::INFINITY, f64::INFINITY), |max| {
            (max.width, max.height)
        });
        Size::new(
            size.width.min(max_width).max(self.min.width),
            size.height.min(max_height).max(self.min.height),
        )
    }
}

/// Where the panel's resize grab starts and ends: its rect plus the outer band on the
/// right and bottom. A locked panel has no outer band.
fn grab_zone(panel: &Panel, zoom: f64) -> Rect {
    let outer = if panel.resize.resizable {
        RESIZE_OUTER_PX / zoom
    } else {
        0.0
    };
    Rect::new(
        panel.rect.origin.x,
        panel.rect.origin.y,
        panel.rect.size.width + outer,
        panel.rect.size.height + outer,
    )
}

/// The panel whose resize handle is under `world`, and which edge.
///
/// Panels are tried front to back. The first one whose rect or outer band holds the
/// point decides: a locked panel, a minimized chip or a point away from the edges gives
/// `None`, so a grab never reaches a panel hidden behind another one.
///
/// The outer band never reaches into a joined neighbor: there the inner band and the
/// gap between the two panels resize, and the neighbor keeps its own clicks.
pub fn resize_target(scene: &Scene, world: Point) -> Option<(PanelId, ResizeEdge)> {
    let zoom = scene.camera.zoom;
    let joins = JoinLayout::compute(&scene.panels);
    for panel in scene.panels.iter().rev() {
        if panel.minimized {
            if chip_rect(panel).contains(world) {
                return None;
            }
            continue;
        }
        if !grab_zone(panel, zoom).contains(world) {
            continue;
        }
        if !panel.rect.contains(world) && in_joined_neighbor(scene, &joins, panel.id, world) {
            continue;
        }
        if !panel.resize.resizable {
            return None;
        }
        let near_right = world.x >= panel.rect.right() - RESIZE_BAND;
        let near_bottom = world.y >= panel.rect.bottom() - RESIZE_BAND;
        let edge = match (near_right, near_bottom) {
            (true, true) => ResizeEdge::BottomRight,
            (true, false) => ResizeEdge::Right,
            (false, true) => ResizeEdge::Bottom,
            (false, false) => return None,
        };
        return Some((panel.id, edge));
    }
    None
}

/// Whether `world` lies inside a panel joined to `id`.
fn in_joined_neighbor(scene: &Scene, joins: &JoinLayout, id: PanelId, world: Point) -> bool {
    joins
        .joins
        .iter()
        .filter_map(|join| match (join.first == id, join.second == id) {
            (true, _) => Some(join.second),
            (_, true) => Some(join.first),
            _ => None,
        })
        .filter_map(|other| scene.get(other))
        .any(|other| other.rect.contains(world))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::content_rect;

    fn terminal() -> PanelKind {
        PanelKind::Terminal {
            command: None,
            cwd: None,
        }
    }

    fn browser() -> PanelKind {
        PanelKind::Browser {
            url: "https://example.com".into(),
            page_zoom: None,
        }
    }

    fn plugin() -> PanelKind {
        PanelKind::Plugin {
            plugin_id: "fixture".into(),
            plugin_version: None,
            settings: serde_json::json!({}),
        }
    }

    fn locked() -> PluginResize {
        PluginResize {
            resizable: false,
            ..PluginResize::default()
        }
    }

    #[test]
    fn every_kind_is_resizable_by_default() {
        for kind in [terminal(), browser(), plugin()] {
            assert_eq!(ResizePolicy::derive(&kind, None), ResizePolicy::FREE);
            assert_eq!(
                ResizePolicy::derive(&kind, Some(&PluginResize::default())),
                ResizePolicy::FREE
            );
        }
    }

    #[test]
    fn plugin_manifest_locks_or_limits_only_plugins() {
        assert_eq!(
            ResizePolicy::derive(&plugin(), Some(&locked())),
            ResizePolicy::LOCKED
        );
        // A plugin's settings never apply to a terminal or browser.
        assert_eq!(
            ResizePolicy::derive(&browser(), Some(&locked())),
            ResizePolicy::FREE
        );
        let limits = PluginResize {
            resizable: true,
            min: Some(Size::new(200.0, 150.0)),
            max: Some(Size::new(600.0, 400.0)),
        };
        assert_eq!(
            ResizePolicy::derive(&plugin(), Some(&limits)),
            ResizePolicy {
                resizable: true,
                min: Size::new(200.0, 150.0),
                max: Some(Size::new(600.0, 400.0)),
            }
        );
    }

    #[test]
    fn derive_keeps_the_scene_floor() {
        let tiny = PluginResize {
            resizable: true,
            min: Some(Size::new(10.0, 500.0)),
            max: None,
        };
        let policy = ResizePolicy::derive(&plugin(), Some(&tiny));
        assert_eq!(policy.min, Size::new(MIN_PANEL_SIZE.width, 500.0));
    }

    #[test]
    fn clamp_holds_size_between_min_and_max() {
        let policy = ResizePolicy {
            resizable: true,
            min: Size::new(200.0, 150.0),
            max: Some(Size::new(600.0, 400.0)),
        };
        assert_eq!(
            policy.clamp(Size::new(50.0, 900.0)),
            Size::new(200.0, 400.0)
        );
        assert_eq!(
            policy.clamp(Size::new(300.0, 300.0)),
            Size::new(300.0, 300.0)
        );
        assert_eq!(
            ResizePolicy::FREE.clamp(Size::new(5000.0, 1.0)),
            Size::new(5000.0, MIN_PANEL_SIZE.height)
        );
    }

    /// Edge, corner and interior points for a browser sized panel, for every kind, both in
    /// the inner band and in the outer band GPUI owns.
    #[test]
    fn hit_bands_cover_edges_and_corner_for_every_kind() {
        for kind in [terminal(), browser(), plugin()] {
            for zoom in [0.25, 1.0, 3.0] {
                let mut scene = Scene::default();
                scene.camera.zoom = zoom;
                let id = scene.add(kind.clone(), Rect::new(0.0, 0.0, 800.0, 600.0));
                let outer = RESIZE_OUTER_PX / zoom;
                let cases = [
                    (Point::new(795.0, 300.0), Some(ResizeEdge::Right)),
                    (
                        Point::new(800.0 + outer * 0.5, 300.0),
                        Some(ResizeEdge::Right),
                    ),
                    (Point::new(400.0, 595.0), Some(ResizeEdge::Bottom)),
                    (
                        Point::new(400.0, 600.0 + outer * 0.5),
                        Some(ResizeEdge::Bottom),
                    ),
                    (Point::new(795.0, 595.0), Some(ResizeEdge::BottomRight)),
                    (
                        Point::new(800.0 + outer * 0.5, 600.0 + outer * 0.5),
                        Some(ResizeEdge::BottomRight),
                    ),
                    (Point::new(400.0, 300.0), None),
                    (Point::new(800.0 + outer * 1.5, 300.0), None),
                    (Point::new(400.0, 600.0 + outer * 1.5), None),
                ];
                for (world, want) in cases {
                    let got = resize_target(&scene, world);
                    assert_eq!(
                        got,
                        want.map(|edge| (id, edge)),
                        "{kind:?} zoom {zoom} {world:?}"
                    );
                }
            }
        }
    }

    /// The bug: a browser's web view fills its content rect, so the handle must exist
    /// outside it, where GPUI receives the mouse.
    #[test]
    fn browser_has_a_handle_outside_its_web_view() {
        let mut scene = Scene::default();
        let id = scene.add(browser(), Rect::new(100.0, 100.0, 800.0, 600.0));
        let content = content_rect(scene.get(id).unwrap());
        let outside = Point::new(content.right() + 2.0, content.bottom() + 2.0);
        assert!(!content.contains(outside));
        assert_eq!(
            resize_target(&scene, outside),
            Some((id, ResizeEdge::BottomRight))
        );
    }

    #[test]
    fn locked_panel_has_no_handle_and_does_not_block_with_its_band() {
        let mut scene = Scene::default();
        let back = scene.add(terminal(), Rect::new(0.0, 0.0, 400.0, 300.0));
        let front = scene.add(plugin(), Rect::new(0.0, 0.0, 380.0, 280.0));
        scene.set_resize_policy(front, ResizePolicy::LOCKED);
        // Inside the locked panel's own edge band: nothing.
        assert_eq!(resize_target(&scene, Point::new(375.0, 100.0)), None);
        // Just right of it, where a free panel's outer band would be: the panel behind.
        assert_eq!(
            resize_target(&scene, Point::new(395.0, 100.0)),
            Some((back, ResizeEdge::Right))
        );
    }

    #[test]
    fn front_panel_shadows_the_handles_behind_it() {
        let mut scene = Scene::default();
        let back = scene.add(terminal(), Rect::new(0.0, 0.0, 400.0, 300.0));
        let front = scene.add(terminal(), Rect::new(300.0, 100.0, 400.0, 300.0));
        // The back panel's right band lies inside the front panel's interior.
        assert_eq!(resize_target(&scene, Point::new(398.0, 200.0)), None);
        assert_eq!(
            resize_target(&scene, Point::new(398.0, 50.0)),
            Some((back, ResizeEdge::Right))
        );
        assert_eq!(
            resize_target(&scene, Point::new(702.0, 200.0)),
            Some((front, ResizeEdge::Right))
        );
    }

    #[test]
    fn minimized_chip_has_no_handle() {
        let mut scene = Scene::default();
        let id = scene.add(terminal(), Rect::new(0.0, 0.0, 400.0, 300.0));
        scene.get_mut(id).unwrap().minimized = true;
        assert_eq!(resize_target(&scene, Point::new(195.0, 20.0)), None);
        assert_eq!(resize_target(&scene, Point::new(398.0, 200.0)), None);
    }

    /// A on the left, B one join gap to its right. `a_front` puts A in front of B.
    fn joined_pair(a_front: bool) -> (Scene, PanelId, PanelId) {
        let mut scene = Scene::default();
        let a = scene.add(terminal(), Rect::new(0.0, 0.0, 200.0, 100.0));
        let b = scene.add(browser(), Rect::new(201.0, 0.0, 200.0, 100.0));
        if a_front {
            scene.bring_to_front(a);
        }
        assert_eq!(JoinLayout::compute(&scene.panels).joins.len(), 1);
        (scene, a, b)
    }

    #[test]
    fn joined_edge_band_does_not_reach_into_the_neighbor() {
        for zoom in [0.5, 1.0, 2.0] {
            for a_front in [true, false] {
                let (mut scene, _, _) = joined_pair(a_front);
                scene.camera.zoom = zoom;
                // B's title bar and content right of the shared edge, inside A's 8 px band.
                for x in [201.0, 203.0, 201.0 + RESIZE_OUTER_PX / zoom - 0.1] {
                    for y in [5.0, 50.0] {
                        assert_eq!(
                            resize_target(&scene, Point::new(x, y)),
                            None,
                            "zoom {zoom} a_front {a_front} at ({x}, {y})"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn joined_gap_and_inner_band_resize_the_left_panel() {
        for a_front in [true, false] {
            let (scene, a, _) = joined_pair(a_front);
            for x in [195.0, 200.0, 200.5] {
                assert_eq!(
                    resize_target(&scene, Point::new(x, 50.0)),
                    Some((a, ResizeEdge::Right)),
                    "a_front {a_front} at x {x}"
                );
            }
        }
    }

    #[test]
    fn unjoined_edges_of_a_joined_pair_keep_the_outer_band() {
        let (scene, a, b) = joined_pair(true);
        let outer = RESIZE_OUTER_PX - 1.0;
        assert_eq!(
            resize_target(&scene, Point::new(401.0 + outer, 50.0)),
            Some((b, ResizeEdge::Right))
        );
        assert_eq!(
            resize_target(&scene, Point::new(50.0, 100.0 + outer)),
            Some((a, ResizeEdge::Bottom))
        );
        assert_eq!(
            resize_target(&scene, Point::new(300.0, 100.0 + outer)),
            Some((b, ResizeEdge::Bottom))
        );
    }
}
