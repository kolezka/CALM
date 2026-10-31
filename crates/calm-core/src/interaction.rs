use crate::geometry::{Point, Rect};
use crate::scene::{CHROME_HEIGHT, Panel, PanelId, PanelKind, Scene};

/// Width of the resize grab band inside a panel's right and bottom edges, in world units.
pub const RESIZE_BAND: f64 = 12.0;
/// Side of each square chrome button, in world units.
pub const CHROME_BUTTON_SIZE: f64 = 20.0;
const CHROME_BUTTON_MARGIN: f64 = (CHROME_HEIGHT - CHROME_BUTTON_SIZE) / 2.0;

#[derive(Clone, Debug, PartialEq)]
pub enum Tool {
    Select,
    DrawTerminal,
    DrawBrowser,
    DrawPlugin(String),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Modifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    /// Command on macOS.
    pub platform: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Button {
    Left,
    Middle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResizeEdge {
    Right,
    Bottom,
    BottomRight,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChromeButton {
    Close,
    Minimize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ZoomStep {
    In,
    Out,
    Reset,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Interaction {
    Idle,
    Drawing { start_world: Point },
    Dragging { id: PanelId, grab_offset_world: Point },
    Resizing { id: PanelId, edge: ResizeEdge },
    Panning { last_screen: Point },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// The panel is already in the scene; the app creates its native view.
    CreatePanel { kind: PanelKind, rect: Rect },
    FocusPanel(PanelId),
    ClosePanelRequested(PanelId),
    ToggleMinimize(PanelId),
    Redraw,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InputHandler {
    pub tool: Tool,
    pub state: Interaction,
    pub space_held: bool,
}

impl Default for InputHandler {
    fn default() -> Self {
        Self {
            tool: Tool::Select,
            state: Interaction::Idle,
            space_held: false,
        }
    }
}

impl InputHandler {
    pub fn mouse_down(
        &mut self,
        screen: Point,
        button: Button,
        _mods: Modifiers,
        scene: &mut Scene,
    ) -> Vec<Effect> {
        if button == Button::Middle || self.space_held {
            self.state = Interaction::Panning {
                last_screen: screen,
            };
            return Vec::new();
        }

        let world = scene.camera.screen_to_world(screen);
        let Some(id) = scene.hit_test(world) else {
            if self.tool != Tool::Select {
                self.state = Interaction::Drawing { start_world: world };
            }
            return Vec::new();
        };
        let Some(panel) = scene.get(id) else {
            return Vec::new();
        };

        if panel.minimized {
            return vec![Effect::ToggleMinimize(id), Effect::Redraw];
        }
        match chrome_hit(panel, world) {
            Some(ChromeButton::Close) => return vec![Effect::ClosePanelRequested(id)],
            Some(ChromeButton::Minimize) => return vec![Effect::ToggleMinimize(id), Effect::Redraw],
            None => {}
        }
        if let Some(edge) = resize_edge(panel, world) {
            self.state = Interaction::Resizing { id, edge };
            scene.bring_to_front(id);
            return vec![Effect::FocusPanel(id), Effect::Redraw];
        }
        if world.y < panel.rect.origin.y + CHROME_HEIGHT {
            self.state = Interaction::Dragging {
                id,
                grab_offset_world: Point::new(
                    world.x - panel.rect.origin.x,
                    world.y - panel.rect.origin.y,
                ),
            };
            scene.bring_to_front(id);
            return vec![Effect::FocusPanel(id), Effect::Redraw];
        }
        // Content area: the native view handles the event itself.
        vec![Effect::FocusPanel(id)]
    }

    pub fn mouse_move(&mut self, screen: Point, scene: &mut Scene) -> Vec<Effect> {
        let world = scene.camera.screen_to_world(screen);
        match self.state {
            Interaction::Idle => Vec::new(),
            // The draft rect is drawn by the app from `start_world` and the pointer.
            Interaction::Drawing { .. } => vec![Effect::Redraw],
            Interaction::Dragging {
                id,
                grab_offset_world,
            } => {
                if let Some(panel) = scene.get_mut(id) {
                    panel.rect.origin =
                        Point::new(world.x - grab_offset_world.x, world.y - grab_offset_world.y);
                }
                vec![Effect::Redraw]
            }
            Interaction::Resizing { id, edge } => {
                if let Some(panel) = scene.get_mut(id) {
                    resize_panel(panel, edge, world);
                }
                vec![Effect::Redraw]
            }
            Interaction::Panning { last_screen } => {
                scene
                    .camera
                    .pan_by_screen(screen.x - last_screen.x, screen.y - last_screen.y);
                self.state = Interaction::Panning {
                    last_screen: screen,
                };
                vec![Effect::Redraw]
            }
        }
    }

    pub fn mouse_up(&mut self, screen: Point, scene: &mut Scene) -> Vec<Effect> {
        let mut effects = self.mouse_move(screen, scene);
        if let Interaction::Drawing { start_world } = self.state {
            let end_world = scene.camera.screen_to_world(screen);
            let draft = Rect::new(
                start_world.x.min(end_world.x),
                start_world.y.min(end_world.y),
                (end_world.x - start_world.x).abs(),
                (end_world.y - start_world.y).abs(),
            );
            if let Some(kind) = self.tool_panel_kind() {
                let id = scene.add(kind.clone(), draft);
                let rect = scene.get(id).map_or(draft, |panel| panel.rect);
                effects.push(Effect::CreatePanel { kind, rect });
                effects.push(Effect::FocusPanel(id));
            }
            self.tool = Tool::Select;
        }
        self.state = Interaction::Idle;
        effects
    }

    /// Platform modifier zooms at the pointer. Otherwise scrolling pans, but only over
    /// empty canvas: over a panel the native view scrolls its own content.
    pub fn scroll(&mut self, screen: Point, dx: f64, dy: f64, mods: Modifiers, scene: &mut Scene) {
        if mods.platform {
            scene.camera.zoom_by((-dy / 300.0).exp2(), screen);
            return;
        }
        let world = scene.camera.screen_to_world(screen);
        if scene.hit_test(world).is_none() {
            scene.camera.pan_by_screen(dx, dy);
        }
    }

    pub fn key_zoom(&mut self, step: ZoomStep, anchor_screen: Point, scene: &mut Scene) {
        let factor = match step {
            ZoomStep::In => 1.1,
            ZoomStep::Out => 1.0 / 1.1,
            ZoomStep::Reset => 1.0 / scene.camera.zoom,
        };
        scene.camera.zoom_by(factor, anchor_screen);
    }

    fn tool_panel_kind(&self) -> Option<PanelKind> {
        match &self.tool {
            Tool::Select => None,
            Tool::DrawTerminal => Some(PanelKind::Terminal {
                command: None,
                cwd: None,
            }),
            Tool::DrawBrowser => Some(PanelKind::Browser {
                url: "about:blank".to_string(),
            }),
            Tool::DrawPlugin(plugin_id) => Some(PanelKind::Plugin {
                plugin_id: plugin_id.clone(),
                settings: serde_json::Value::Object(serde_json::Map::new()),
            }),
        }
    }
}

/// Close is the rightmost square of the chrome strip, Minimize sits to its left.
pub fn chrome_hit(panel: &Panel, world: Point) -> Option<ChromeButton> {
    let top = panel.rect.origin.y + CHROME_BUTTON_MARGIN;
    let close_left = panel.rect.right() - CHROME_BUTTON_MARGIN - CHROME_BUTTON_SIZE;
    let minimize_left = close_left - CHROME_BUTTON_MARGIN - CHROME_BUTTON_SIZE;
    let close = Rect::new(close_left, top, CHROME_BUTTON_SIZE, CHROME_BUTTON_SIZE);
    let minimize = Rect::new(minimize_left, top, CHROME_BUTTON_SIZE, CHROME_BUTTON_SIZE);
    if close.contains(world) {
        Some(ChromeButton::Close)
    } else if minimize.contains(world) {
        Some(ChromeButton::Minimize)
    } else {
        None
    }
}

fn resize_edge(panel: &Panel, world: Point) -> Option<ResizeEdge> {
    let near_right = world.x >= panel.rect.right() - RESIZE_BAND;
    let near_bottom = world.y >= panel.rect.bottom() - RESIZE_BAND;
    match (near_right, near_bottom) {
        (true, true) => Some(ResizeEdge::BottomRight),
        (true, false) => Some(ResizeEdge::Right),
        (false, true) => Some(ResizeEdge::Bottom),
        (false, false) => None,
    }
}

fn resize_panel(panel: &mut Panel, edge: ResizeEdge, world: Point) {
    use crate::scene::MIN_PANEL_SIZE;
    if matches!(edge, ResizeEdge::Right | ResizeEdge::BottomRight) {
        panel.rect.size.width = (world.x - panel.rect.origin.x).max(MIN_PANEL_SIZE.width);
    }
    if matches!(edge, ResizeEdge::Bottom | ResizeEdge::BottomRight) {
        panel.rect.size.height = (world.y - panel.rect.origin.y).max(MIN_PANEL_SIZE.height);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{Point, Rect};
    use crate::scene::{PanelKind, Scene};

    fn terminal() -> PanelKind {
        PanelKind::Terminal { command: None, cwd: None }
    }

    fn no_mods() -> Modifiers {
        Modifiers::default()
    }

    #[test]
    fn draw_tool_creates_terminal_panel_on_release() {
        let mut scene = Scene::default();
        let mut handler = InputHandler::default();
        handler.tool = Tool::DrawTerminal;
        handler.mouse_down(Point::new(100.0, 100.0), Button::Left, no_mods(), &mut scene);
        assert!(matches!(handler.state, Interaction::Drawing { .. }));
        handler.mouse_move(Point::new(400.0, 350.0), &mut scene);
        let effects = handler.mouse_up(Point::new(400.0, 350.0), &mut scene);

        assert_eq!(scene.panels.len(), 1);
        let panel = &scene.panels[0];
        assert_eq!(panel.rect, Rect::new(100.0, 100.0, 300.0, 250.0));
        assert!(effects.contains(&Effect::CreatePanel { kind: terminal(), rect: panel.rect }));
        assert!(effects.contains(&Effect::FocusPanel(panel.id)));
        assert_eq!(handler.tool, Tool::Select);
        assert_eq!(handler.state, Interaction::Idle);
    }

    #[test]
    fn drag_from_chrome_moves_panel_and_raises_it() {
        let mut scene = Scene::default();
        let back = scene.add(terminal(), Rect::new(0.0, 0.0, 400.0, 300.0));
        let front = scene.add(terminal(), Rect::new(500.0, 0.0, 400.0, 300.0));
        let mut handler = InputHandler::default();

        let effects = handler.mouse_down(Point::new(100.0, 10.0), Button::Left, no_mods(), &mut scene);
        assert!(effects.contains(&Effect::FocusPanel(back)));
        assert!(matches!(handler.state, Interaction::Dragging { id, .. } if id == back));
        assert_eq!(scene.panels.last().map(|panel| panel.id), Some(back));

        handler.mouse_move(Point::new(150.0, 60.0), &mut scene);
        handler.mouse_up(Point::new(150.0, 60.0), &mut scene);
        assert_eq!(scene.get(back).unwrap().rect, Rect::new(50.0, 50.0, 400.0, 300.0));
        assert_eq!(scene.get(front).unwrap().rect, Rect::new(500.0, 0.0, 400.0, 300.0));
        assert_eq!(handler.state, Interaction::Idle);
    }

    #[test]
    fn resize_from_bottom_right_changes_size_not_origin() {
        let mut scene = Scene::default();
        let id = scene.add(terminal(), Rect::new(0.0, 0.0, 400.0, 300.0));
        let mut handler = InputHandler::default();

        handler.mouse_down(Point::new(395.0, 295.0), Button::Left, no_mods(), &mut scene);
        assert_eq!(handler.state, Interaction::Resizing { id, edge: ResizeEdge::BottomRight });
        handler.mouse_move(Point::new(495.0, 345.0), &mut scene);
        handler.mouse_up(Point::new(495.0, 345.0), &mut scene);
        assert_eq!(scene.get(id).unwrap().rect, Rect::new(0.0, 0.0, 495.0, 345.0));
    }

    #[test]
    fn panning_with_space_moves_camera() {
        let mut scene = Scene::default();
        scene.add(terminal(), Rect::new(0.0, 0.0, 400.0, 300.0));
        let mut handler = InputHandler::default();
        handler.space_held = true;

        // Space+left pans even over a panel.
        handler.mouse_down(Point::new(100.0, 100.0), Button::Left, no_mods(), &mut scene);
        assert!(matches!(handler.state, Interaction::Panning { .. }));
        handler.mouse_move(Point::new(150.0, 120.0), &mut scene);
        handler.mouse_up(Point::new(150.0, 120.0), &mut scene);
        assert_eq!(scene.camera.offset, Point::new(-50.0, -20.0));
        assert_eq!(scene.panels[0].rect, Rect::new(0.0, 0.0, 400.0, 300.0));
        assert_eq!(handler.state, Interaction::Idle);
    }

    #[test]
    fn scroll_with_platform_modifier_zooms_at_pointer() {
        let mut scene = Scene::default();
        let mut handler = InputHandler::default();
        let pointer = Point::new(300.0, 200.0);
        let world_before = scene.camera.screen_to_world(pointer);
        let mods = Modifiers { platform: true, ..Modifiers::default() };

        handler.scroll(pointer, 0.0, -300.0, mods, &mut scene);
        assert!((scene.camera.zoom - 2.0).abs() < 1e-9);
        let world_after = scene.camera.screen_to_world(pointer);
        assert!((world_after.x - world_before.x).abs() < 1e-9);
        assert!((world_after.y - world_before.y).abs() < 1e-9);

        // Without the modifier, scrolling over empty canvas pans instead.
        handler.scroll(pointer, 10.0, 0.0, no_mods(), &mut scene);
        assert!((scene.camera.zoom - 2.0).abs() < 1e-9);
    }

    #[test]
    fn chrome_close_button_emits_close_requested() {
        let mut scene = Scene::default();
        let id = scene.add(terminal(), Rect::new(0.0, 0.0, 400.0, 300.0));
        let panel = scene.get(id).unwrap().clone();
        assert_eq!(chrome_hit(&panel, Point::new(386.0, 14.0)), Some(ChromeButton::Close));
        assert_eq!(chrome_hit(&panel, Point::new(362.0, 14.0)), Some(ChromeButton::Minimize));
        assert_eq!(chrome_hit(&panel, Point::new(100.0, 14.0)), None);

        let mut handler = InputHandler::default();
        let effects = handler.mouse_down(Point::new(386.0, 14.0), Button::Left, no_mods(), &mut scene);
        assert!(effects.contains(&Effect::ClosePanelRequested(id)));
        assert_eq!(handler.state, Interaction::Idle);
        assert_eq!(scene.panels.len(), 1);
    }

    #[test]
    fn content_click_focuses_without_dragging() {
        let mut scene = Scene::default();
        let id = scene.add(terminal(), Rect::new(0.0, 0.0, 400.0, 300.0));
        let mut handler = InputHandler::default();

        let effects = handler.mouse_down(Point::new(100.0, 150.0), Button::Left, no_mods(), &mut scene);
        assert_eq!(effects, vec![Effect::FocusPanel(id)]);
        assert_eq!(handler.state, Interaction::Idle);
        handler.mouse_move(Point::new(200.0, 250.0), &mut scene);
        handler.mouse_up(Point::new(200.0, 250.0), &mut scene);
        assert_eq!(scene.get(id).unwrap().rect, Rect::new(0.0, 0.0, 400.0, 300.0));
    }
}
