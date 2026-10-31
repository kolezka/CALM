use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::camera::Camera;
use crate::geometry::{Point, Rect, Size};

pub const MIN_PANEL_SIZE: Size = Size::new(120.0, 80.0);
/// Height of the title strip at the top of every panel, in world units.
pub const CHROME_HEIGHT: f64 = 28.0;
pub const MINIMIZED_CHIP: Size = Size::new(200.0, 28.0);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct PanelId(pub u64);

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum PanelKind {
    Terminal {
        command: Option<String>,
        cwd: Option<PathBuf>,
    },
    Browser {
        url: String,
    },
    Plugin {
        plugin_id: String,
        settings: serde_json::Value,
    },
}

impl PanelKind {
    fn default_title(&self) -> String {
        match self {
            PanelKind::Terminal { .. } => "Terminal".to_string(),
            PanelKind::Browser { url } => url.clone(),
            PanelKind::Plugin { plugin_id, .. } => plugin_id.clone(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Panel {
    pub id: PanelId,
    pub kind: PanelKind,
    /// World coordinates, chrome strip included.
    pub rect: Rect,
    pub title: String,
    pub minimized: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Scene {
    /// Back to front: the last panel is topmost.
    pub panels: Vec<Panel>,
    pub camera: Camera,
    next_id: u64,
}

impl Default for Scene {
    fn default() -> Self {
        Self {
            panels: Vec::new(),
            camera: Camera::default(),
            next_id: 1,
        }
    }
}

impl Scene {
    /// Adds a panel on top. Ids are never reused, even after a remove.
    pub fn add(&mut self, kind: PanelKind, rect: Rect) -> PanelId {
        let id = PanelId(self.next_id);
        self.next_id += 1;
        let rect = Rect {
            origin: rect.origin,
            size: Size::new(
                rect.size.width.max(MIN_PANEL_SIZE.width),
                rect.size.height.max(MIN_PANEL_SIZE.height),
            ),
        };
        let title = kind.default_title();
        self.panels.push(Panel {
            id,
            kind,
            rect,
            title,
            minimized: false,
        });
        id
    }

    pub fn remove(&mut self, id: PanelId) -> Option<Panel> {
        let index = self.index_of(id)?;
        Some(self.panels.remove(index))
    }

    pub fn get(&self, id: PanelId) -> Option<&Panel> {
        self.panels.iter().find(|panel| panel.id == id)
    }

    pub fn get_mut(&mut self, id: PanelId) -> Option<&mut Panel> {
        self.panels.iter_mut().find(|panel| panel.id == id)
    }

    pub fn bring_to_front(&mut self, id: PanelId) {
        if let Some(index) = self.index_of(id) {
            let panel = self.panels.remove(index);
            self.panels.push(panel);
        }
    }

    /// Topmost panel under `world`. A minimized panel only hits its chip.
    pub fn hit_test(&self, world: Point) -> Option<PanelId> {
        self.panels
            .iter()
            .rev()
            .find(|panel| {
                let hit_rect = if panel.minimized {
                    chip_rect(panel)
                } else {
                    panel.rect
                };
                hit_rect.contains(world)
            })
            .map(|panel| panel.id)
    }

    /// Smallest world rect covering every panel as drawn (a minimized panel is its chip).
    /// `None` when the scene is empty.
    pub fn bounds(&self) -> Option<Rect> {
        self.panels
            .iter()
            .map(|panel| {
                if panel.minimized {
                    chip_rect(panel)
                } else {
                    panel.rect
                }
            })
            .reduce(|a, b| a.union(&b))
    }

    fn index_of(&self, id: PanelId) -> Option<usize> {
        self.panels.iter().position(|panel| panel.id == id)
    }
}

pub fn chip_rect(panel: &Panel) -> Rect {
    Rect {
        origin: panel.rect.origin,
        size: MINIMIZED_CHIP,
    }
}

pub fn content_rect(panel: &Panel) -> Rect {
    Rect::new(
        panel.rect.origin.x,
        panel.rect.origin.y + CHROME_HEIGHT,
        panel.rect.size.width,
        (panel.rect.size.height - CHROME_HEIGHT).max(0.0),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terminal() -> PanelKind {
        PanelKind::Terminal {
            command: None,
            cwd: None,
        }
    }

    #[test]
    fn add_assigns_increasing_ids_and_topmost_last() {
        let mut scene = Scene::default();
        let first = scene.add(terminal(), Rect::new(0.0, 0.0, 200.0, 200.0));
        let second = scene.add(terminal(), Rect::new(50.0, 50.0, 200.0, 200.0));
        assert!(second.0 > first.0);
        let order: Vec<PanelId> = scene.panels.iter().map(|panel| panel.id).collect();
        assert_eq!(order, vec![first, second]);
    }

    #[test]
    fn hit_test_prefers_topmost_overlap() {
        let mut scene = Scene::default();
        let back = scene.add(terminal(), Rect::new(0.0, 0.0, 200.0, 200.0));
        let front = scene.add(terminal(), Rect::new(100.0, 100.0, 200.0, 200.0));
        assert_eq!(scene.hit_test(Point::new(150.0, 150.0)), Some(front));
        assert_eq!(scene.hit_test(Point::new(50.0, 50.0)), Some(back));
        assert_eq!(scene.hit_test(Point::new(1000.0, 1000.0)), None);
    }

    #[test]
    fn bring_to_front_reorders() {
        let mut scene = Scene::default();
        let back = scene.add(terminal(), Rect::new(0.0, 0.0, 200.0, 200.0));
        let front = scene.add(terminal(), Rect::new(100.0, 100.0, 200.0, 200.0));
        scene.bring_to_front(back);
        let order: Vec<PanelId> = scene.panels.iter().map(|panel| panel.id).collect();
        assert_eq!(order, vec![front, back]);
        assert_eq!(scene.hit_test(Point::new(150.0, 150.0)), Some(back));
    }

    #[test]
    fn minimized_panel_hits_only_chip() {
        let mut scene = Scene::default();
        let id = scene.add(terminal(), Rect::new(0.0, 0.0, 400.0, 300.0));
        if let Some(panel) = scene.get_mut(id) {
            panel.minimized = true;
        }
        assert_eq!(scene.hit_test(Point::new(10.0, 10.0)), Some(id));
        assert_eq!(scene.hit_test(Point::new(199.0, 27.0)), Some(id));
        assert_eq!(scene.hit_test(Point::new(10.0, 100.0)), None);
        assert_eq!(scene.hit_test(Point::new(300.0, 10.0)), None);
        let panel = scene.get(id).unwrap();
        assert_eq!(chip_rect(panel), Rect::new(0.0, 0.0, 200.0, 28.0));
    }

    #[test]
    fn remove_returns_panel_and_shrinks() {
        let mut scene = Scene::default();
        let first = scene.add(terminal(), Rect::new(0.0, 0.0, 200.0, 200.0));
        let second = scene.add(
            PanelKind::Browser {
                url: "https://example.com".into(),
            },
            Rect::new(0.0, 0.0, 200.0, 200.0),
        );
        let removed = scene.remove(first).unwrap();
        assert_eq!(removed.id, first);
        assert_eq!(scene.panels.len(), 1);
        assert_eq!(scene.panels[0].id, second);
        assert!(scene.remove(first).is_none());
        assert!(scene.get(first).is_none());
    }

    #[test]
    fn add_enforces_min_size() {
        let mut scene = Scene::default();
        let id = scene.add(terminal(), Rect::new(5.0, 6.0, 10.0, 10.0));
        let panel = scene.get(id).unwrap();
        assert_eq!(panel.rect, Rect::new(5.0, 6.0, 120.0, 80.0));
        assert_eq!(MIN_PANEL_SIZE, Size::new(120.0, 80.0));
        assert_eq!(
            content_rect(panel),
            Rect::new(5.0, 6.0 + CHROME_HEIGHT, 120.0, 80.0 - CHROME_HEIGHT)
        );
    }

    #[test]
    fn bounds_covers_panels_and_uses_chip_when_minimized() {
        let mut scene = Scene::default();
        assert_eq!(scene.bounds(), None);
        scene.add(terminal(), Rect::new(-100.0, -50.0, 300.0, 200.0));
        let wide = scene.add(terminal(), Rect::new(400.0, 300.0, 1000.0, 500.0));
        assert_eq!(
            scene.bounds(),
            Some(Rect::new(-100.0, -50.0, 1500.0, 850.0))
        );
        scene
            .panels
            .iter_mut()
            .find(|p| p.id == wide)
            .unwrap()
            .minimized = true;
        assert_eq!(scene.bounds(), Some(Rect::new(-100.0, -50.0, 700.0, 378.0)));
    }
}
