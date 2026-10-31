use crate::geometry::{Point, Rect};
use crate::scene::{CHROME_HEIGHT, Panel, PanelId, PanelKind, Scene, chip_rect};
use crate::snap::{SnapConfig, snap_draw, snap_move, snap_resize};

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
    Right,
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
    Drawing {
        start_world: Point,
    },
    Dragging {
        id: PanelId,
        grab_offset_world: Point,
    },
    Resizing {
        id: PanelId,
        edge: ResizeEdge,
        /// Distance from the pointer to the right and bottom edges at press time.
        grab_offset_world: Point,
    },
    Panning {
        last_screen: Point,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub enum Effect {
    /// The panel is already in the scene; the app creates its native view.
    CreatePanel {
        kind: PanelKind,
        rect: Rect,
    },
    FocusPanel(PanelId),
    ClosePanelRequested(PanelId),
    ToggleMinimize(PanelId),
    Redraw,
    /// Ask the user before ending a live process.
    ConfirmClose(PanelId),
    /// Remove the panel and end its process.
    ClosePanel(PanelId),
    /// Right click on empty canvas. `screen` is canvas-relative, `world` is where to add.
    ShowCanvasMenu {
        screen: Point,
        world: Point,
    },
    /// Right click on a panel's chrome strip or minimized chip.
    ShowPanelMenu {
        id: PanelId,
        screen: Point,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmKey {
    Enter,
    Escape,
}

#[derive(Clone, Debug, PartialEq)]
pub struct InputHandler {
    pub tool: Tool,
    pub state: Interaction,
    pub space_held: bool,
    /// Panel waiting for the user to confirm its close.
    pub confirming: Option<PanelId>,
    pub snap: SnapConfig,
    /// Alt was held on the last move, so the panel being drawn is placed unsnapped.
    pub snap_suspended: bool,
}

impl Default for InputHandler {
    fn default() -> Self {
        Self {
            tool: Tool::Select,
            state: Interaction::Idle,
            space_held: false,
            confirming: None,
            snap: SnapConfig::default(),
            snap_suspended: false,
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
        if button == Button::Right {
            return context_menu(screen, scene);
        }
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
            Some(ChromeButton::Minimize) => {
                return vec![Effect::ToggleMinimize(id), Effect::Redraw];
            }
            None => {}
        }
        if let Some(edge) = resize_edge(panel, world) {
            self.state = Interaction::Resizing {
                id,
                edge,
                grab_offset_world: Point::new(
                    panel.rect.right() - world.x,
                    panel.rect.bottom() - world.y,
                ),
            };
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

    pub fn mouse_move(&mut self, screen: Point, mods: Modifiers, scene: &mut Scene) -> Vec<Effect> {
        let world = scene.camera.screen_to_world(screen);
        self.snap_suspended = mods.alt;
        let snap = !mods.alt;
        match self.state {
            Interaction::Idle => Vec::new(),
            // The draft rect is drawn by the app from `start_world` and the pointer.
            Interaction::Drawing { .. } => vec![Effect::Redraw],
            Interaction::Dragging {
                id,
                grab_offset_world,
            } => {
                let others = neighbor_rects(scene, Some(id));
                if let Some(panel) = scene.get_mut(id) {
                    panel.rect.origin =
                        Point::new(world.x - grab_offset_world.x, world.y - grab_offset_world.y);
                    if snap {
                        panel.rect = snap_move(panel.rect, &others, &self.snap);
                    }
                }
                vec![Effect::Redraw]
            }
            Interaction::Resizing {
                id,
                edge,
                grab_offset_world,
            } => {
                let others = neighbor_rects(scene, Some(id));
                if let Some(panel) = scene.get_mut(id) {
                    let edge_world =
                        Point::new(world.x + grab_offset_world.x, world.y + grab_offset_world.y);
                    resize_panel(panel, edge, edge_world);
                    if snap {
                        let snapped = snap_resize(panel.rect, edge, &others, &self.snap);
                        resize_panel(panel, edge, Point::new(snapped.right(), snapped.bottom()));
                    }
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

    /// Ends the current interaction. Drag and resize geometry comes only from
    /// `mouse_move`, so a press and release without movement changes nothing.
    pub fn mouse_up(&mut self, screen: Point, scene: &mut Scene) -> Vec<Effect> {
        let mut effects = Vec::new();
        if self.state != Interaction::Idle {
            effects.push(Effect::Redraw);
        }
        if let Interaction::Drawing { start_world } = self.state {
            let end_world = scene.camera.screen_to_world(screen);
            let draft = Rect::new(
                start_world.x.min(end_world.x),
                start_world.y.min(end_world.y),
                (end_world.x - start_world.x).abs(),
                (end_world.y - start_world.y).abs(),
            );
            let draft = if self.snap_suspended {
                draft
            } else {
                snap_draw(draft, &neighbor_rects(scene, None), &self.snap)
            };
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

    /// Trackpad pinch. `delta` is the fractional change, 0.1 is 10 % in.
    pub fn pinch(&mut self, screen: Point, delta: f64, scene: &mut Scene) -> Vec<Effect> {
        // A gesture must not move the canvas under a rectangle being drawn or resized.
        let busy = matches!(
            self.state,
            Interaction::Drawing { .. } | Interaction::Resizing { .. }
        );
        if busy || !delta.is_finite() || delta == 0.0 {
            return Vec::new();
        }
        scene.camera.zoom_by(1.0 + delta.clamp(-0.5, 0.5), screen);
        vec![Effect::Redraw]
    }

    pub fn key_zoom(&mut self, step: ZoomStep, anchor_screen: Point, scene: &mut Scene) {
        let factor = match step {
            ZoomStep::In => 1.1,
            ZoomStep::Out => 1.0 / 1.1,
            ZoomStep::Reset => 1.0 / scene.camera.zoom,
        };
        scene.camera.zoom_by(factor, anchor_screen);
    }

    /// A live process needs a second request or Enter before the panel closes.
    pub fn request_close(&mut self, id: PanelId, alive: bool) -> Effect {
        if !alive || self.confirming == Some(id) {
            if self.confirming == Some(id) {
                self.confirming = None;
            }
            Effect::ClosePanel(id)
        } else {
            self.confirming = Some(id);
            Effect::ConfirmClose(id)
        }
    }

    pub fn confirm_key(&mut self, key: ConfirmKey) -> Option<Effect> {
        let id = self.confirming.take()?;
        match key {
            ConfirmKey::Enter => Some(Effect::ClosePanel(id)),
            ConfirmKey::Escape => None,
        }
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

/// Rects that snapping lines up against. Minimized panels count as their chip.
fn neighbor_rects(scene: &Scene, except: Option<PanelId>) -> Vec<Rect> {
    scene
        .panels
        .iter()
        .filter(|panel| Some(panel.id) != except)
        .map(|panel| {
            if panel.minimized {
                chip_rect(panel)
            } else {
                panel.rect
            }
        })
        .collect()
}

/// Right click: canvas menu on empty canvas, panel menu on chrome or a minimized chip,
/// nothing on content, which handles its own right clicks.
fn context_menu(screen: Point, scene: &Scene) -> Vec<Effect> {
    let world = scene.camera.screen_to_world(screen);
    let Some(id) = scene.hit_test(world) else {
        return vec![Effect::ShowCanvasMenu { screen, world }];
    };
    match scene.get(id) {
        Some(panel) if panel.minimized || world.y < panel.rect.origin.y + CHROME_HEIGHT => {
            vec![Effect::ShowPanelMenu { id, screen }]
        }
        _ => Vec::new(),
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

/// Moves the grabbed edge to `edge_world`, keeping the origin fixed.
fn resize_panel(panel: &mut Panel, edge: ResizeEdge, edge_world: Point) {
    use crate::scene::MIN_PANEL_SIZE;
    if matches!(edge, ResizeEdge::Right | ResizeEdge::BottomRight) {
        panel.rect.size.width = (edge_world.x - panel.rect.origin.x).max(MIN_PANEL_SIZE.width);
    }
    if matches!(edge, ResizeEdge::Bottom | ResizeEdge::BottomRight) {
        panel.rect.size.height = (edge_world.y - panel.rect.origin.y).max(MIN_PANEL_SIZE.height);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geometry::{Point, Rect};
    use crate::scene::{PanelId, PanelKind, Scene};

    fn terminal() -> PanelKind {
        PanelKind::Terminal {
            command: None,
            cwd: None,
        }
    }

    fn no_mods() -> Modifiers {
        Modifiers::default()
    }

    /// Alt held: moves place panels exactly, without snapping.
    fn free() -> Modifiers {
        Modifiers {
            alt: true,
            ..Modifiers::default()
        }
    }

    #[test]
    fn draw_tool_creates_terminal_panel_on_release() {
        let mut scene = Scene::default();
        let mut handler = InputHandler {
            tool: Tool::DrawTerminal,
            ..InputHandler::default()
        };
        handler.mouse_down(
            Point::new(100.0, 100.0),
            Button::Left,
            no_mods(),
            &mut scene,
        );
        assert!(matches!(handler.state, Interaction::Drawing { .. }));
        handler.mouse_move(Point::new(400.0, 350.0), free(), &mut scene);
        let effects = handler.mouse_up(Point::new(400.0, 350.0), &mut scene);

        assert_eq!(scene.panels.len(), 1);
        let panel = &scene.panels[0];
        assert_eq!(panel.rect, Rect::new(100.0, 100.0, 300.0, 250.0));
        assert!(effects.contains(&Effect::CreatePanel {
            kind: terminal(),
            rect: panel.rect
        }));
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

        let effects =
            handler.mouse_down(Point::new(100.0, 10.0), Button::Left, no_mods(), &mut scene);
        assert!(effects.contains(&Effect::FocusPanel(back)));
        assert!(matches!(handler.state, Interaction::Dragging { id, .. } if id == back));
        assert_eq!(scene.panels.last().map(|panel| panel.id), Some(back));

        handler.mouse_move(Point::new(150.0, 60.0), free(), &mut scene);
        handler.mouse_up(Point::new(150.0, 60.0), &mut scene);
        assert_eq!(
            scene.get(back).unwrap().rect,
            Rect::new(50.0, 50.0, 400.0, 300.0)
        );
        assert_eq!(
            scene.get(front).unwrap().rect,
            Rect::new(500.0, 0.0, 400.0, 300.0)
        );
        assert_eq!(handler.state, Interaction::Idle);
    }

    #[test]
    fn resize_from_bottom_right_changes_size_not_origin() {
        let mut scene = Scene::default();
        let id = scene.add(terminal(), Rect::new(0.0, 0.0, 400.0, 300.0));
        let mut handler = InputHandler::default();

        handler.mouse_down(
            Point::new(395.0, 295.0),
            Button::Left,
            no_mods(),
            &mut scene,
        );
        assert!(matches!(
            handler.state,
            Interaction::Resizing { id: resizing, edge: ResizeEdge::BottomRight, .. } if resizing == id
        ));
        // Size follows the pointer delta (+100, +50), not the absolute pointer position.
        handler.mouse_move(Point::new(495.0, 345.0), free(), &mut scene);
        handler.mouse_up(Point::new(495.0, 345.0), &mut scene);
        assert_eq!(
            scene.get(id).unwrap().rect,
            Rect::new(0.0, 0.0, 500.0, 350.0)
        );
    }

    #[test]
    fn resize_click_without_drag_keeps_size() {
        let mut scene = Scene::default();
        let id = scene.add(terminal(), Rect::new(0.0, 0.0, 400.0, 300.0));
        let mut handler = InputHandler::default();

        for (x, y) in [(395.0, 295.0), (395.0, 150.0), (200.0, 295.0)] {
            handler.mouse_down(Point::new(x, y), Button::Left, no_mods(), &mut scene);
            assert!(matches!(handler.state, Interaction::Resizing { .. }));
            handler.mouse_up(Point::new(x, y), &mut scene);
            assert_eq!(
                scene.get(id).unwrap().rect,
                Rect::new(0.0, 0.0, 400.0, 300.0),
                "press and release at ({x}, {y}) must not resize"
            );
            assert_eq!(handler.state, Interaction::Idle);
        }
    }

    #[test]
    fn drag_from_chrome_keeps_grab_offset() {
        let mut scene = Scene::default();
        let id = scene.add(terminal(), Rect::new(0.0, 0.0, 400.0, 300.0));
        let mut handler = InputHandler::default();

        handler.mouse_down(Point::new(100.0, 10.0), Button::Left, no_mods(), &mut scene);
        // A one-unit move shifts the panel by one unit: no jump to the pointer.
        handler.mouse_move(Point::new(101.0, 10.0), free(), &mut scene);
        assert_eq!(
            scene.get(id).unwrap().rect,
            Rect::new(1.0, 0.0, 400.0, 300.0)
        );
        // Release only ends the drag; geometry comes from moves alone.
        handler.mouse_up(Point::new(300.0, 300.0), &mut scene);
        assert_eq!(
            scene.get(id).unwrap().rect,
            Rect::new(1.0, 0.0, 400.0, 300.0)
        );
        assert_eq!(handler.state, Interaction::Idle);
    }

    #[test]
    fn panning_with_space_moves_camera() {
        let mut scene = Scene::default();
        scene.add(terminal(), Rect::new(0.0, 0.0, 400.0, 300.0));
        let mut handler = InputHandler {
            space_held: true,
            ..InputHandler::default()
        };

        // Space+left pans even over a panel.
        handler.mouse_down(
            Point::new(100.0, 100.0),
            Button::Left,
            no_mods(),
            &mut scene,
        );
        assert!(matches!(handler.state, Interaction::Panning { .. }));
        handler.mouse_move(Point::new(150.0, 120.0), no_mods(), &mut scene);
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
        let mods = Modifiers {
            platform: true,
            ..Modifiers::default()
        };

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
        assert_eq!(
            chrome_hit(&panel, Point::new(386.0, 14.0)),
            Some(ChromeButton::Close)
        );
        assert_eq!(
            chrome_hit(&panel, Point::new(362.0, 14.0)),
            Some(ChromeButton::Minimize)
        );
        assert_eq!(chrome_hit(&panel, Point::new(100.0, 14.0)), None);

        let mut handler = InputHandler::default();
        let effects =
            handler.mouse_down(Point::new(386.0, 14.0), Button::Left, no_mods(), &mut scene);
        assert!(effects.contains(&Effect::ClosePanelRequested(id)));
        assert_eq!(handler.state, Interaction::Idle);
        assert_eq!(scene.panels.len(), 1);
    }

    #[test]
    fn content_click_focuses_without_dragging() {
        let mut scene = Scene::default();
        let id = scene.add(terminal(), Rect::new(0.0, 0.0, 400.0, 300.0));
        let mut handler = InputHandler::default();

        let effects = handler.mouse_down(
            Point::new(100.0, 150.0),
            Button::Left,
            no_mods(),
            &mut scene,
        );
        assert_eq!(effects, vec![Effect::FocusPanel(id)]);
        assert_eq!(handler.state, Interaction::Idle);
        handler.mouse_move(Point::new(200.0, 250.0), no_mods(), &mut scene);
        handler.mouse_up(Point::new(200.0, 250.0), &mut scene);
        assert_eq!(
            scene.get(id).unwrap().rect,
            Rect::new(0.0, 0.0, 400.0, 300.0)
        );
    }

    #[test]
    fn close_requires_confirmation_when_alive() {
        let mut handler = InputHandler::default();
        let id = PanelId(7);
        assert_eq!(handler.request_close(id, true), Effect::ConfirmClose(id));
        assert_eq!(handler.confirming, Some(id));
        assert_eq!(
            handler.confirm_key(ConfirmKey::Enter),
            Some(Effect::ClosePanel(id))
        );
        assert_eq!(handler.confirming, None);
        assert_eq!(handler.confirm_key(ConfirmKey::Enter), None);
    }

    #[test]
    fn close_without_live_process_is_immediate() {
        let mut handler = InputHandler::default();
        assert_eq!(
            handler.request_close(PanelId(3), false),
            Effect::ClosePanel(PanelId(3))
        );
        assert_eq!(handler.confirming, None);
    }

    #[test]
    fn second_close_within_confirmation_closes() {
        let mut handler = InputHandler::default();
        let id = PanelId(4);
        assert_eq!(handler.request_close(id, true), Effect::ConfirmClose(id));
        assert_eq!(handler.request_close(id, true), Effect::ClosePanel(id));
        assert_eq!(handler.confirming, None);

        // A close on another panel moves the confirmation instead of closing.
        handler.request_close(id, true);
        assert_eq!(
            handler.request_close(PanelId(5), true),
            Effect::ConfirmClose(PanelId(5))
        );
        assert_eq!(handler.confirming, Some(PanelId(5)));
    }

    #[test]
    fn escape_cancels_confirmation() {
        let mut handler = InputHandler::default();
        let id = PanelId(9);
        handler.request_close(id, true);
        assert_eq!(handler.confirm_key(ConfirmKey::Escape), None);
        assert_eq!(handler.confirming, None);
        assert_eq!(handler.confirm_key(ConfirmKey::Enter), None);
    }

    #[test]
    fn right_click_on_empty_canvas_emits_canvas_menu() {
        let mut scene = Scene::default();
        scene.add(terminal(), Rect::new(0.0, 0.0, 400.0, 300.0));
        scene.camera.zoom = 2.0;
        let mut handler = InputHandler::default();
        let screen = Point::new(1000.0, 100.0);
        let effects = handler.mouse_down(screen, Button::Right, no_mods(), &mut scene);
        assert_eq!(
            effects,
            vec![Effect::ShowCanvasMenu {
                screen,
                world: Point::new(500.0, 50.0),
            }]
        );
        assert_eq!(handler.state, Interaction::Idle);
        assert_eq!(scene.panels.len(), 1);
    }

    #[test]
    fn right_click_on_chrome_emits_panel_menu() {
        let mut scene = Scene::default();
        let back = scene.add(terminal(), Rect::new(0.0, 0.0, 400.0, 300.0));
        let front = scene.add(terminal(), Rect::new(500.0, 0.0, 400.0, 300.0));
        let mut handler = InputHandler::default();
        let screen = Point::new(100.0, 10.0);
        let effects = handler.mouse_down(screen, Button::Right, no_mods(), &mut scene);
        assert_eq!(effects, vec![Effect::ShowPanelMenu { id: back, screen }]);
        // No drag starts and the stacking order stays.
        assert_eq!(handler.state, Interaction::Idle);
        assert_eq!(scene.panels.last().map(|panel| panel.id), Some(front));

        // A minimized panel's chip counts as chrome and is not restored.
        scene.get_mut(front).unwrap().minimized = true;
        let chip = Point::new(510.0, 10.0);
        let effects = handler.mouse_down(chip, Button::Right, no_mods(), &mut scene);
        assert_eq!(
            effects,
            vec![Effect::ShowPanelMenu {
                id: front,
                screen: chip
            }]
        );
        assert!(scene.get(front).unwrap().minimized);
    }

    #[test]
    fn right_click_on_content_emits_nothing() {
        let mut scene = Scene::default();
        scene.add(terminal(), Rect::new(0.0, 0.0, 400.0, 300.0));
        let mut handler = InputHandler::default();
        let effects = handler.mouse_down(
            Point::new(100.0, 150.0),
            Button::Right,
            no_mods(),
            &mut scene,
        );
        assert_eq!(effects, Vec::new());
        assert_eq!(handler.state, Interaction::Idle);
    }

    #[test]
    fn pinch_in_zooms_at_center() {
        let mut scene = Scene::default();
        let mut handler = InputHandler::default();
        let center = Point::new(300.0, 200.0);
        let world_before = scene.camera.screen_to_world(center);
        let effects = handler.pinch(center, 0.1, &mut scene);
        assert_eq!(effects, vec![Effect::Redraw]);
        assert!((scene.camera.zoom - 1.1).abs() < 1e-9);
        let world_after = scene.camera.screen_to_world(center);
        assert!((world_after.x - world_before.x).abs() < 1e-9);
        assert!((world_after.y - world_before.y).abs() < 1e-9);
    }

    #[test]
    fn pinch_out_zooms_out() {
        let mut scene = Scene::default();
        let mut handler = InputHandler::default();
        handler.pinch(Point::new(300.0, 200.0), -0.1, &mut scene);
        assert!((scene.camera.zoom - 0.9).abs() < 1e-9);
    }

    #[test]
    fn pinch_clamps_large_delta() {
        let mut scene = Scene::default();
        let mut handler = InputHandler::default();
        handler.pinch(Point::new(300.0, 200.0), 5.0, &mut scene);
        assert!((scene.camera.zoom - 1.5).abs() < 1e-9);
    }

    #[test]
    fn pinch_ignores_nan_and_zero() {
        let mut scene = Scene::default();
        let mut handler = InputHandler::default();
        let before = scene.camera;
        for delta in [0.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(
                handler.pinch(Point::new(300.0, 200.0), delta, &mut scene),
                Vec::new(),
                "delta {delta}"
            );
            assert_eq!(scene.camera, before, "delta {delta}");
        }
    }

    #[test]
    fn pinch_ignored_while_drawing() {
        let mut scene = Scene::default();
        let mut handler = InputHandler {
            tool: Tool::DrawTerminal,
            ..InputHandler::default()
        };
        handler.mouse_down(
            Point::new(100.0, 100.0),
            Button::Left,
            no_mods(),
            &mut scene,
        );
        assert!(matches!(handler.state, Interaction::Drawing { .. }));
        assert_eq!(
            handler.pinch(Point::new(300.0, 200.0), 0.1, &mut scene),
            Vec::new()
        );
        assert_eq!(scene.camera.zoom, 1.0);

        // Resizing ignores it too; dragging still zooms.
        handler.state = Interaction::Resizing {
            id: PanelId(1),
            edge: ResizeEdge::BottomRight,
            grab_offset_world: Point::new(0.0, 0.0),
        };
        assert_eq!(
            handler.pinch(Point::new(300.0, 200.0), 0.1, &mut scene),
            Vec::new()
        );
        assert_eq!(scene.camera.zoom, 1.0);
        handler.state = Interaction::Dragging {
            id: PanelId(1),
            grab_offset_world: Point::new(0.0, 0.0),
        };
        assert_eq!(
            handler.pinch(Point::new(300.0, 200.0), 0.1, &mut scene),
            vec![Effect::Redraw]
        );
    }

    fn two_panels() -> (Scene, PanelId) {
        let mut scene = Scene::default();
        scene.add(terminal(), Rect::new(0.0, 0.0, 200.0, 100.0));
        let moving = scene.add(terminal(), Rect::new(300.0, 300.0, 200.0, 100.0));
        (scene, moving)
    }

    #[test]
    fn drag_with_alt_held_does_not_snap() {
        let (mut scene, moving) = two_panels();
        let mut handler = InputHandler::default();
        handler.mouse_down(
            Point::new(310.0, 310.0),
            Button::Left,
            no_mods(),
            &mut scene,
        );
        handler.mouse_move(Point::new(215.0, 411.0), free(), &mut scene);
        assert_eq!(
            scene.get(moving).unwrap().rect,
            Rect::new(205.0, 401.0, 200.0, 100.0)
        );
    }

    #[test]
    fn drag_without_alt_snaps() {
        let (mut scene, moving) = two_panels();
        let mut handler = InputHandler::default();
        handler.mouse_down(
            Point::new(310.0, 310.0),
            Button::Left,
            no_mods(),
            &mut scene,
        );
        // Left edge 205 snaps flush to the neighbor's right edge 200; top 401 to the grid.
        handler.mouse_move(Point::new(215.0, 411.0), no_mods(), &mut scene);
        assert_eq!(
            scene.get(moving).unwrap().rect,
            Rect::new(200.0, 400.0, 200.0, 100.0)
        );
    }
}
