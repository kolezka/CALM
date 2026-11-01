use serde::{Deserialize, Serialize};

use crate::geometry::{Point, Rect, Size};
use crate::resize::resize_target;
use crate::scene::{Panel, PanelId, PanelKind, Scene, chip_rect, chrome_height};
use crate::snap::{SnapConfig, snap_draw, snap_move, snap_resize};

pub use crate::resize::RESIZE_BAND;
/// Side of each square chrome button hit target, in world units.
pub const CHROME_BUTTON_SIZE: f64 = crate::chrome::BUTTON_HIT_SIZE;
pub use crate::chrome::{ChromeButton, chrome_hit};
/// A press that moves less than this, in screen points, before release is a click.
pub const CLICK_SLOP: f64 = 4.0;

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

/// Keys held to move a panel by dragging its content (`canvas.move_modifier`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum MoveModifier {
    #[default]
    #[serde(rename = "cmd")]
    Cmd,
    #[serde(rename = "ctrl")]
    Ctrl,
    #[serde(rename = "alt")]
    Alt,
    #[serde(rename = "cmd+ctrl")]
    CmdCtrl,
    #[serde(rename = "cmd+alt")]
    CmdAlt,
    #[serde(rename = "ctrl+alt")]
    CtrlAlt,
}

impl MoveModifier {
    pub const ALL: [MoveModifier; 6] = [
        MoveModifier::Cmd,
        MoveModifier::Ctrl,
        MoveModifier::Alt,
        MoveModifier::CmdCtrl,
        MoveModifier::CmdAlt,
        MoveModifier::CtrlAlt,
    ];

    /// macOS names: alt is Option.
    pub fn label(self) -> &'static str {
        match self {
            MoveModifier::Cmd => "Cmd",
            MoveModifier::Ctrl => "Ctrl",
            MoveModifier::Alt => "Option",
            MoveModifier::CmdCtrl => "Cmd+Ctrl",
            MoveModifier::CmdAlt => "Cmd+Option",
            MoveModifier::CtrlAlt => "Ctrl+Option",
        }
    }

    /// Shift is ignored. Command, control and alt must be exactly this set.
    pub fn matches(self, mods: Modifiers) -> bool {
        let (platform, control, alt) = match self {
            MoveModifier::Cmd => (true, false, false),
            MoveModifier::Ctrl => (false, true, false),
            MoveModifier::Alt => (false, false, true),
            MoveModifier::CmdCtrl => (true, true, false),
            MoveModifier::CmdAlt => (true, false, true),
            MoveModifier::CtrlAlt => (false, true, true),
        };
        mods.platform == platform && mods.control == control && mods.alt == alt
    }
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
        /// The rect a release here would create, already snapped. The app paints it.
        draft: Rect,
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
    /// Left press on empty canvas with the select tool. A release within `CLICK_SLOP`
    /// is a click on nothing.
    Pressing {
        start_screen: Point,
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
    /// No panel is focused and the canvas takes the keyboard.
    ClearFocus,
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
    /// A click on empty canvas, or Escape, drops panel focus.
    pub click_empty_clears_focus: bool,
}

impl Default for InputHandler {
    fn default() -> Self {
        Self {
            tool: Tool::Select,
            state: Interaction::Idle,
            space_held: false,
            confirming: None,
            snap: SnapConfig::default(),
            click_empty_clears_focus: true,
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
            let mut effects = context_menu(screen, scene);
            if matches!(effects.first(), Some(Effect::ShowCanvasMenu { .. })) {
                effects.splice(0..0, self.clear_focus());
            }
            return effects;
        }
        if button == Button::Middle || self.space_held {
            self.state = Interaction::Panning {
                last_screen: screen,
            };
            return Vec::new();
        }

        let world = scene.camera.screen_to_world(screen);
        let hit = scene.hit_test(world);
        if let Some(panel) = hit.and_then(|id| scene.get(id)) {
            let id = panel.id;
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
        }
        if let Some((id, edge)) = self.resize_handle(scene, world, hit)
            && let Some(panel) = scene.get(id)
        {
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
        let Some(panel) = hit.and_then(|id| scene.get(id)) else {
            if self.tool == Tool::Select {
                self.state = Interaction::Pressing {
                    start_screen: screen,
                };
            } else {
                self.state = Interaction::Drawing {
                    start_world: world,
                    draft: Rect::new(world.x, world.y, 0.0, 0.0),
                };
            }
            // Drawing starts on nothing, so it drops focus at once; the new panel takes it.
            return if self.tool == Tool::Select {
                Vec::new()
            } else {
                self.clear_focus()
            };
        };
        let id = panel.id;
        if world.y < panel.rect.origin.y + chrome_height(panel) {
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

    /// A move-modifier drag on content: the same state and effects as a title bar press.
    /// Later `mouse_move` and `mouse_up` move and drop the panel.
    pub fn begin_content_move(
        &mut self,
        id: PanelId,
        screen: Point,
        scene: &mut Scene,
    ) -> Vec<Effect> {
        let world = scene.camera.screen_to_world(screen);
        let Some(panel) = scene.get(id) else {
            return Vec::new();
        };
        self.state = Interaction::Dragging {
            id,
            grab_offset_world: Point::new(
                world.x - panel.rect.origin.x,
                world.y - panel.rect.origin.y,
            ),
        };
        scene.bring_to_front(id);
        vec![Effect::FocusPanel(id), Effect::Redraw]
    }

    /// A move-modifier right click on content opens the panel menu.
    pub fn content_menu(&self, id: PanelId, screen: Point) -> Vec<Effect> {
        vec![Effect::ShowPanelMenu { id, screen }]
    }

    pub fn mouse_move(&mut self, screen: Point, mods: Modifiers, scene: &mut Scene) -> Vec<Effect> {
        let world = scene.camera.screen_to_world(screen);
        let snap = !mods.alt;
        match self.state {
            Interaction::Idle => Vec::new(),
            Interaction::Pressing { start_screen } => {
                if beyond_slop(start_screen, screen) {
                    self.state = Interaction::Idle;
                }
                Vec::new()
            }
            Interaction::Drawing { start_world, .. } => {
                self.state = Interaction::Drawing {
                    start_world,
                    draft: self.draft_rect(start_world, world, mods, scene),
                };
                vec![Effect::Redraw]
            }
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
    pub fn mouse_up(&mut self, screen: Point, mods: Modifiers, scene: &mut Scene) -> Vec<Effect> {
        let mut effects = Vec::new();
        if let Interaction::Pressing { start_screen } = self.state {
            self.state = Interaction::Idle;
            if beyond_slop(start_screen, screen) {
                return effects;
            }
            return self.clear_focus();
        }
        if self.state != Interaction::Idle {
            effects.push(Effect::Redraw);
        }
        if let Interaction::Drawing { start_world, .. } = self.state {
            // Recomputed from the release itself: alt may have changed since the last move.
            let end_world = scene.camera.screen_to_world(screen);
            let draft = self.draft_rect(start_world, end_world, mods, scene);
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

    /// Escape answers a pending close first. Otherwise it drops the draw tool and the panel
    /// focus, but only while no native view holds the keyboard: those get Escape themselves.
    pub fn escape(&mut self, native_keyboard: bool) -> Vec<Effect> {
        if self.confirming.is_some() {
            return self.confirm_key(ConfirmKey::Escape).into_iter().collect();
        }
        self.tool = Tool::Select;
        let effects = if native_keyboard {
            Vec::new()
        } else {
            self.clear_focus()
        };
        if effects.is_empty() {
            vec![Effect::Redraw]
        } else {
            effects
        }
    }

    fn clear_focus(&self) -> Vec<Effect> {
        if self.click_empty_clears_focus {
            vec![Effect::ClearFocus, Effect::Redraw]
        } else {
            Vec::new()
        }
    }

    pub fn confirm_key(&mut self, key: ConfirmKey) -> Option<Effect> {
        let id = self.confirming.take()?;
        match key {
            ConfirmKey::Enter => Some(Effect::ClosePanel(id)),
            ConfirmKey::Escape => None,
        }
    }

    /// The resize edge the pointer is over, for the hover cursor. While a resize runs it
    /// is that resize's edge, wherever the pointer is.
    pub fn hover_resize(&self, screen: Point, scene: &Scene) -> Option<ResizeEdge> {
        match self.state {
            Interaction::Resizing { edge, .. } => Some(edge),
            Interaction::Idle if !self.space_held => {
                let world = scene.camera.screen_to_world(screen);
                self.resize_handle(scene, world, scene.hit_test(world))
                    .map(|(_, edge)| edge)
            }
            _ => None,
        }
    }

    /// A press on empty canvas with a draw tool draws, even next to a panel's edge.
    fn resize_handle(
        &self,
        scene: &Scene,
        world: Point,
        hit: Option<PanelId>,
    ) -> Option<(PanelId, ResizeEdge)> {
        if hit.is_none() && self.tool != Tool::Select {
            return None;
        }
        resize_target(scene, world)
    }

    /// Rect spanned by a draw from `start` to `end`, snapped unless alt is held.
    fn draft_rect(&self, start: Point, end: Point, mods: Modifiers, scene: &Scene) -> Rect {
        let raw = Rect::new(
            start.x.min(end.x),
            start.y.min(end.y),
            (end.x - start.x).abs(),
            (end.y - start.y).abs(),
        );
        if mods.alt {
            raw
        } else {
            snap_draw(raw, &neighbor_rects(scene, None), &self.snap)
        }
    }

    fn tool_panel_kind(&self) -> Option<PanelKind> {
        match &self.tool {
            Tool::Select => None,
            Tool::DrawTerminal => Some(PanelKind::Terminal {
                command: None,
                cwd: None,
                profile: None,
            }),
            Tool::DrawBrowser => Some(PanelKind::Browser {
                url: "about:blank".to_string(),
                page_zoom: None,
            }),
            Tool::DrawPlugin(plugin_id) => Some(PanelKind::Plugin {
                plugin_id: plugin_id.clone(),
                plugin_version: None,
                settings: serde_json::Value::Object(serde_json::Map::new()),
            }),
        }
    }
}

fn beyond_slop(start: Point, end: Point) -> bool {
    (end.x - start.x).hypot(end.y - start.y) > CLICK_SLOP
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
        Some(panel) if panel.minimized || world.y < panel.rect.origin.y + chrome_height(panel) => {
            vec![Effect::ShowPanelMenu { id, screen }]
        }
        _ => Vec::new(),
    }
}

/// Moves the grabbed edge to `edge_world`, keeping the origin fixed, within the panel's
/// resize limits. A locked panel does not change.
fn resize_panel(panel: &mut Panel, edge: ResizeEdge, edge_world: Point) {
    if !panel.resize.resizable {
        return;
    }
    let mut size: Size = panel.rect.size;
    if matches!(edge, ResizeEdge::Right | ResizeEdge::BottomRight) {
        size.width = edge_world.x - panel.rect.origin.x;
    }
    if matches!(edge, ResizeEdge::Bottom | ResizeEdge::BottomRight) {
        size.height = edge_world.y - panel.rect.origin.y;
    }
    panel.rect.size = panel.resize.clamp(size);
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
            profile: None,
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
        let effects = handler.mouse_up(Point::new(400.0, 350.0), free(), &mut scene);

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
        handler.mouse_up(Point::new(150.0, 60.0), no_mods(), &mut scene);
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

    /// A live WKWebView keeps every click inside its rect, so GPUI only sees the mouse
    /// outside the content rect. For every kind and zoom, a press a few screen pixels
    /// outside the right edge, bottom edge and corner must start a resize that drags.
    #[test]
    fn resize_grab_works_outside_the_native_content_rect_for_every_kind() {
        let kinds = [
            terminal(),
            PanelKind::Browser {
                url: "https://example.com".into(),
                page_zoom: None,
            },
            PanelKind::Plugin {
                plugin_id: "hello-world".into(),
                plugin_version: None,
                settings: serde_json::json!({}),
            },
        ];
        for kind in kinds {
            for zoom in [0.5, 1.0, 2.0] {
                for edge in [
                    ResizeEdge::Right,
                    ResizeEdge::Bottom,
                    ResizeEdge::BottomRight,
                ] {
                    let mut scene = Scene::default();
                    scene.camera.zoom = zoom;
                    // Browser default size.
                    let id = scene.add(kind.clone(), Rect::new(100.0, 100.0, 800.0, 600.0));
                    let content = crate::scene::content_rect(scene.get(id).unwrap());
                    let outside = 3.0 / zoom;
                    let world = match edge {
                        ResizeEdge::Right => Point::new(content.right() + outside, 400.0),
                        ResizeEdge::Bottom => Point::new(500.0, content.bottom() + outside),
                        ResizeEdge::BottomRight => {
                            Point::new(content.right() + outside, content.bottom() + outside)
                        }
                    };
                    assert!(!content.contains(world));
                    let press = scene.camera.world_to_screen(world);
                    let mut handler = InputHandler::default();
                    handler.mouse_down(press, Button::Left, no_mods(), &mut scene);
                    assert!(
                        matches!(
                            handler.state,
                            Interaction::Resizing { id: got, edge: got_edge, .. }
                                if got == id && got_edge == edge
                        ),
                        "{kind:?} zoom {zoom} {edge:?}: {:?}",
                        handler.state
                    );
                    let release = Point::new(press.x + 40.0, press.y + 40.0);
                    handler.mouse_move(release, free(), &mut scene);
                    handler.mouse_up(release, free(), &mut scene);
                    let size = scene.get(id).unwrap().rect.size;
                    let grown = 40.0 / zoom;
                    let want_w = if edge == ResizeEdge::Bottom {
                        800.0
                    } else {
                        800.0 + grown
                    };
                    let want_h = if edge == ResizeEdge::Right {
                        600.0
                    } else {
                        600.0 + grown
                    };
                    assert_eq!(
                        (size.width, size.height),
                        (want_w, want_h),
                        "{kind:?} {edge:?}"
                    );
                }
            }
        }
    }

    fn plugin_kind() -> PanelKind {
        PanelKind::Plugin {
            plugin_id: "fixture".into(),
            plugin_version: None,
            settings: serde_json::json!({}),
        }
    }

    /// Presses at `press`, drags by `delta` screen pixels and releases.
    fn drag(scene: &mut Scene, press: Point, delta: Point, mods: Modifiers) -> InputHandler {
        let mut handler = InputHandler::default();
        handler.mouse_down(press, Button::Left, no_mods(), scene);
        let release = Point::new(press.x + delta.x, press.y + delta.y);
        handler.mouse_move(release, mods, scene);
        handler.mouse_up(release, mods, scene);
        handler
    }

    #[test]
    fn locked_panel_ignores_edge_drags_inside_and_outside() {
        use crate::resize::ResizePolicy;
        let mut scene = Scene::default();
        let id = scene.add(plugin_kind(), Rect::new(0.0, 0.0, 400.0, 300.0));
        scene.set_resize_policy(id, ResizePolicy::LOCKED);
        let before = scene.get(id).unwrap().rect;
        for press in [
            Point::new(395.0, 150.0),
            Point::new(403.0, 150.0),
            Point::new(200.0, 295.0),
            Point::new(395.0, 295.0),
            Point::new(403.0, 303.0),
        ] {
            let handler = drag(&mut scene, press, Point::new(80.0, 60.0), free());
            assert_eq!(scene.get(id).unwrap().rect, before, "press {press:?}");
            assert_eq!(handler.hover_resize(press, &scene), None, "press {press:?}");
        }
    }

    #[test]
    fn resize_drag_stays_within_min_and_max_with_and_without_snapping() {
        use crate::resize::{PluginResize, ResizePolicy};
        let spec = PluginResize {
            resizable: true,
            min: Some(Size::new(200.0, 160.0)),
            max: Some(Size::new(496.0, 400.0)),
        };
        // Grid multiples, so a snapped drag lands on the limits too.
        for mods in [free(), no_mods()] {
            let mut scene = Scene::default();
            let id = scene.add(plugin_kind(), Rect::new(0.0, 0.0, 400.0, 300.0));
            scene.set_resize_policy(id, ResizePolicy::derive(&plugin_kind(), Some(&spec)));
            drag(
                &mut scene,
                Point::new(403.0, 303.0),
                Point::new(900.0, 900.0),
                mods,
            );
            assert_eq!(scene.get(id).unwrap().rect.size, Size::new(496.0, 400.0));
            drag(
                &mut scene,
                Point::new(499.0, 403.0),
                Point::new(-900.0, -900.0),
                mods,
            );
            assert_eq!(scene.get(id).unwrap().rect.size, Size::new(200.0, 160.0));
        }
    }

    #[test]
    fn set_resize_policy_moves_a_free_panel_into_its_limits() {
        use crate::resize::ResizePolicy;
        let mut scene = Scene::default();
        let id = scene.add(plugin_kind(), Rect::new(0.0, 0.0, 900.0, 100.0));
        let policy = ResizePolicy {
            resizable: true,
            min: Size::new(200.0, 150.0),
            max: Some(Size::new(500.0, 400.0)),
        };
        scene.set_resize_policy(id, policy);
        assert_eq!(scene.get(id).unwrap().rect.size, Size::new(500.0, 150.0));
    }

    #[test]
    fn hover_reports_the_edge_and_holds_it_during_a_resize() {
        let mut scene = Scene::default();
        scene.add(
            PanelKind::Browser {
                url: "https://example.com".into(),
                page_zoom: None,
            },
            Rect::new(0.0, 0.0, 800.0, 600.0),
        );
        let mut handler = InputHandler::default();
        let corner = Point::new(803.0, 603.0);
        assert_eq!(
            handler.hover_resize(corner, &scene),
            Some(ResizeEdge::BottomRight)
        );
        assert_eq!(handler.hover_resize(Point::new(400.0, 300.0), &scene), None);
        handler.mouse_down(corner, Button::Left, no_mods(), &mut scene);
        // Far from the panel, the cursor still shows the resize under way.
        assert_eq!(
            handler.hover_resize(Point::new(5.0, 5.0), &scene),
            Some(ResizeEdge::BottomRight)
        );
    }

    #[test]
    fn draw_tool_next_to_an_edge_draws_instead_of_resizing() {
        let mut scene = Scene::default();
        scene.add(terminal(), Rect::new(0.0, 0.0, 400.0, 300.0));
        let mut handler = InputHandler {
            tool: Tool::DrawTerminal,
            ..InputHandler::default()
        };
        handler.mouse_down(
            Point::new(403.0, 150.0),
            Button::Left,
            no_mods(),
            &mut scene,
        );
        assert!(matches!(handler.state, Interaction::Drawing { .. }));
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
        handler.mouse_up(Point::new(495.0, 345.0), no_mods(), &mut scene);
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
            handler.mouse_up(Point::new(x, y), no_mods(), &mut scene);
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
        handler.mouse_up(Point::new(300.0, 300.0), no_mods(), &mut scene);
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
        handler.mouse_up(Point::new(150.0, 120.0), no_mods(), &mut scene);
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

    fn press_release(
        handler: &mut InputHandler,
        scene: &mut Scene,
        button: Button,
        from: Point,
        to: Point,
    ) -> Vec<Effect> {
        let mut effects = handler.mouse_down(from, button, no_mods(), scene);
        effects.extend(handler.mouse_move(to, no_mods(), scene));
        effects.extend(handler.mouse_up(to, no_mods(), scene));
        effects
    }

    /// One terminal at (0, 0, 400, 300); (600, 500) is empty canvas.
    fn focus_scene() -> (Scene, PanelId) {
        let mut scene = Scene::default();
        let id = scene.add(terminal(), Rect::new(0.0, 0.0, 400.0, 300.0));
        (scene, id)
    }

    #[test]
    fn empty_canvas_click_clears_focus() {
        let (mut scene, _) = focus_scene();
        let mut handler = InputHandler::default();
        let at = Point::new(600.0, 500.0);
        let down = handler.mouse_down(at, Button::Left, no_mods(), &mut scene);
        assert!(
            !down.contains(&Effect::ClearFocus),
            "a press alone is not a click"
        );
        // Jitter inside the slop is still a click.
        let wiggle = Point::new(at.x + CLICK_SLOP / 2.0, at.y);
        handler.mouse_move(wiggle, no_mods(), &mut scene);
        let up = handler.mouse_up(wiggle, no_mods(), &mut scene);
        assert!(up.contains(&Effect::ClearFocus), "{up:?}");
        assert_eq!(handler.state, Interaction::Idle);
    }

    #[test]
    fn empty_canvas_right_click_clears_focus_before_menu() {
        let (mut scene, _) = focus_scene();
        let mut handler = InputHandler::default();
        let at = Point::new(600.0, 500.0);
        let effects = handler.mouse_down(at, Button::Right, no_mods(), &mut scene);
        assert_eq!(effects.first(), Some(&Effect::ClearFocus), "{effects:?}");
        assert!(matches!(
            effects.last(),
            Some(Effect::ShowCanvasMenu { .. })
        ));
    }

    #[test]
    fn pan_gestures_on_empty_canvas_keep_focus() {
        let (mut scene, _) = focus_scene();
        let mut handler = InputHandler::default();
        let from = Point::new(600.0, 500.0);
        let far = Point::new(700.0, 560.0);
        let cases = [
            // Left drag beyond the slop is not a click.
            press_release(&mut handler, &mut scene, Button::Left, from, far),
            // Middle drag and a middle click both pan.
            press_release(&mut handler, &mut scene, Button::Middle, from, far),
            press_release(&mut handler, &mut scene, Button::Middle, from, from),
        ];
        handler.space_held = true;
        let space = press_release(&mut handler, &mut scene, Button::Left, from, far);
        let space_click = press_release(&mut handler, &mut scene, Button::Left, from, from);
        handler.space_held = false;
        let mods = no_mods();
        handler.scroll(from, 30.0, 20.0, mods, &mut scene);
        let pinch = handler.pinch(from, 0.1, &mut scene);
        for effects in cases.iter().chain([&space, &space_click, &pinch]) {
            assert!(!effects.contains(&Effect::ClearFocus), "{effects:?}");
        }
    }

    #[test]
    fn panel_chrome_and_content_clicks_focus_the_panel() {
        let (mut scene, id) = focus_scene();
        let mut handler = InputHandler::default();
        for at in [Point::new(100.0, 10.0), Point::new(100.0, 150.0)] {
            let effects = press_release(&mut handler, &mut scene, Button::Left, at, at);
            assert!(effects.contains(&Effect::FocusPanel(id)), "{effects:?}");
            assert!(!effects.contains(&Effect::ClearFocus), "{effects:?}");
        }
    }

    #[test]
    fn draw_tool_clears_focus_then_focuses_the_new_panel() {
        let (mut scene, _) = focus_scene();
        let mut handler = InputHandler {
            tool: Tool::DrawTerminal,
            ..InputHandler::default()
        };
        let from = Point::new(600.0, 500.0);
        let down = handler.mouse_down(from, Button::Left, no_mods(), &mut scene);
        assert_eq!(down.first(), Some(&Effect::ClearFocus), "{down:?}");
        assert!(matches!(handler.state, Interaction::Drawing { .. }));
        handler.mouse_move(Point::new(900.0, 800.0), free(), &mut scene);
        let up = handler.mouse_up(Point::new(900.0, 800.0), free(), &mut scene);
        let new_id = scene.panels.last().unwrap().id;
        assert!(up.contains(&Effect::FocusPanel(new_id)), "{up:?}");
        assert_eq!(scene.panels.len(), 2);
    }

    #[test]
    fn setting_off_keeps_focus_on_empty_clicks() {
        let (mut scene, _) = focus_scene();
        let mut handler = InputHandler {
            click_empty_clears_focus: false,
            ..InputHandler::default()
        };
        let at = Point::new(600.0, 500.0);
        let left = press_release(&mut handler, &mut scene, Button::Left, at, at);
        let right = handler.mouse_down(at, Button::Right, no_mods(), &mut scene);
        let escape = handler.escape(false);
        for effects in [&left, &right, &escape] {
            assert!(!effects.contains(&Effect::ClearFocus), "{effects:?}");
        }
        assert!(matches!(right.as_slice(), [Effect::ShowCanvasMenu { .. }]));
    }

    #[test]
    fn escape_clears_focus_only_without_native_keyboard() {
        let mut handler = InputHandler {
            tool: Tool::DrawBrowser,
            ..InputHandler::default()
        };
        assert_eq!(handler.escape(true), vec![Effect::Redraw]);
        assert_eq!(handler.tool, Tool::Select);
        assert!(handler.escape(false).contains(&Effect::ClearFocus));
        // A pending close takes Escape first and keeps focus.
        handler.confirming = Some(PanelId(1));
        assert!(handler.escape(false).is_empty());
        assert_eq!(handler.confirming, None);
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
        handler.mouse_up(Point::new(200.0, 250.0), no_mods(), &mut scene);
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
            vec![
                Effect::ClearFocus,
                Effect::Redraw,
                Effect::ShowCanvasMenu {
                    screen,
                    world: Point::new(500.0, 50.0),
                }
            ]
        );
        assert_eq!(handler.state, Interaction::Idle);
        assert_eq!(scene.panels.len(), 1);
    }

    #[test]
    fn borderless_panel_has_no_chrome_hit_and_no_bar_menu() {
        let mut scene = Scene::default();
        let id = scene.add(terminal(), Rect::new(0.0, 0.0, 400.0, 300.0));
        crate::scene::set_borderless(scene.get_mut(id).unwrap(), true);
        let panel = scene.get(id).unwrap();
        // Inside the panel, within the strip where a bar would be, over where Close sits.
        let top = panel.rect.origin.y;
        let close_spot = Point::new(panel.rect.right() - 14.0, top + 14.0);
        assert!(close_spot.y < top + crate::scene::CHROME_HEIGHT);
        assert_eq!(scene.hit_test(close_spot), Some(id));
        assert_eq!(crate::chrome::chrome_hit(panel, close_spot), None);
        assert_eq!(
            crate::chrome::chrome_hit(panel, Point::new(panel.rect.right() - 38.0, top + 14.0)),
            None
        );

        let mut handler = InputHandler::default();
        let screen = scene.camera.world_to_screen(Point::new(100.0, top + 10.0));
        let effects = handler.mouse_down(screen, Button::Right, no_mods(), &mut scene);
        assert!(
            !effects
                .iter()
                .any(|effect| matches!(effect, Effect::ShowPanelMenu { .. })),
            "{effects:?}"
        );
        // A left press there is content, not a drag.
        let effects = handler.mouse_down(screen, Button::Left, no_mods(), &mut scene);
        assert_eq!(effects, vec![Effect::FocusPanel(id)]);
        assert_eq!(handler.state, Interaction::Idle);
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
    fn left_click_on_chip_restores() {
        let mut scene = Scene::default();
        let id = scene.add(terminal(), Rect::new(0.0, 0.0, 400.0, 300.0));
        scene.get_mut(id).unwrap().minimized = true;
        let mut handler = InputHandler::default();

        // Below the chip is empty canvas while minimized.
        let below = handler.mouse_down(
            Point::new(100.0, 100.0),
            Button::Left,
            no_mods(),
            &mut scene,
        );
        assert!(below.is_empty());
        handler.mouse_up(Point::new(100.0, 100.0), no_mods(), &mut scene);

        let effects =
            handler.mouse_down(Point::new(100.0, 10.0), Button::Left, no_mods(), &mut scene);
        assert_eq!(effects, vec![Effect::ToggleMinimize(id), Effect::Redraw]);
        assert_eq!(handler.state, Interaction::Idle);
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
        // Left edge 205 joins the neighbor's right edge 200 one unit off; top 401 to the grid.
        handler.mouse_move(Point::new(215.0, 411.0), no_mods(), &mut scene);
        assert_eq!(
            scene.get(moving).unwrap().rect,
            Rect::new(201.0, 400.0, 200.0, 100.0)
        );
    }

    #[test]
    fn cmd_matches_with_shift_but_not_with_ctrl() {
        let mods = |platform, control, alt, shift| Modifiers {
            shift,
            control,
            alt,
            platform,
        };
        assert!(MoveModifier::Cmd.matches(mods(true, false, false, false)));
        assert!(MoveModifier::Cmd.matches(mods(true, false, false, true)));
        assert!(!MoveModifier::Cmd.matches(mods(true, true, false, false)));
        assert!(!MoveModifier::Cmd.matches(mods(true, false, true, false)));
        assert!(!MoveModifier::Cmd.matches(mods(false, false, false, true)));
        assert!(!MoveModifier::Cmd.matches(Modifiers::default()));
        // Every value against every set of the three keys: exactly one set matches.
        for value in MoveModifier::ALL {
            let mut hits = Vec::new();
            for bits in 0..8u8 {
                let (p, c, a) = (bits & 1 != 0, bits & 2 != 0, bits & 4 != 0);
                for shift in [false, true] {
                    if value.matches(mods(p, c, a, shift)) {
                        hits.push((p, c, a, shift));
                    }
                }
            }
            assert_eq!(hits.len(), 2, "{value:?}: {hits:?}");
            let keys = |h: (bool, bool, bool, bool)| (h.0, h.1, h.2);
            assert_eq!(keys(hits[0]), keys(hits[1]), "{value:?}");
        }
        assert!(MoveModifier::CtrlAlt.matches(mods(false, true, true, false)));
        assert!(!MoveModifier::CtrlAlt.matches(mods(true, true, true, false)));
        assert_eq!(MoveModifier::default(), MoveModifier::Cmd);
    }

    #[test]
    fn begin_content_move_then_mouse_move_moves_the_panel() {
        // Snapping on: same result as a title bar drag (drag_without_alt_snaps).
        let (mut scene, moving) = two_panels();
        let back = scene.panels[0].id;
        scene.bring_to_front(back);
        let mut handler = InputHandler::default();
        let press = Point::new(310.0, 360.0);
        let effects = handler.begin_content_move(moving, press, &mut scene);
        assert_eq!(effects, vec![Effect::FocusPanel(moving), Effect::Redraw]);
        assert_eq!(scene.panels.last().unwrap().id, moving);
        assert!(matches!(handler.state, Interaction::Dragging { id, .. } if id == moving));
        handler.mouse_move(Point::new(215.0, 461.0), no_mods(), &mut scene);
        assert_eq!(
            scene.get(moving).unwrap().rect,
            Rect::new(201.0, 400.0, 200.0, 100.0)
        );
        let up = handler.mouse_up(Point::new(215.0, 461.0), no_mods(), &mut scene);
        assert_eq!(up, vec![Effect::Redraw]);
        assert_eq!(handler.state, Interaction::Idle);

        // Alt held: no snapping.
        let (mut scene, moving) = two_panels();
        let mut handler = InputHandler::default();
        handler.begin_content_move(moving, press, &mut scene);
        handler.mouse_move(Point::new(215.0, 461.0), free(), &mut scene);
        assert_eq!(
            scene.get(moving).unwrap().rect,
            Rect::new(205.0, 401.0, 200.0, 100.0)
        );

        // An unknown id changes nothing.
        let mut handler = InputHandler::default();
        assert_eq!(
            handler.begin_content_move(PanelId(999), press, &mut scene),
            Vec::new()
        );
        assert_eq!(handler.state, Interaction::Idle);
    }

    #[test]
    fn content_menu_shows_the_panel_menu() {
        let handler = InputHandler::default();
        let screen = Point::new(5.0, 6.0);
        assert_eq!(
            handler.content_menu(PanelId(3), screen),
            vec![Effect::ShowPanelMenu {
                id: PanelId(3),
                screen
            }]
        );
    }

    fn drawing_terminal() -> InputHandler {
        InputHandler {
            tool: Tool::DrawTerminal,
            ..InputHandler::default()
        }
    }

    #[test]
    fn drawing_draft_is_snapped_while_moving() {
        let mut scene = Scene::default();
        let mut handler = drawing_terminal();
        handler.mouse_down(Point::new(13.0, 21.0), Button::Left, no_mods(), &mut scene);
        handler.mouse_move(Point::new(213.0, 121.0), no_mods(), &mut scene);
        let Interaction::Drawing { draft, .. } = handler.state else {
            panic!("expected Drawing, got {:?}", handler.state);
        };
        assert_eq!(draft, Rect::new(16.0, 24.0, 200.0, 96.0));
        // Alt shows the free rect.
        handler.mouse_move(Point::new(213.0, 121.0), free(), &mut scene);
        let Interaction::Drawing { draft, .. } = handler.state else {
            panic!("expected Drawing, got {:?}", handler.state);
        };
        assert_eq!(draft, Rect::new(13.0, 21.0, 200.0, 100.0));
    }

    #[test]
    fn release_with_alt_does_not_snap_even_without_move() {
        let mut scene = Scene::default();
        let mut handler = drawing_terminal();
        handler.mouse_down(Point::new(13.0, 21.0), Button::Left, no_mods(), &mut scene);
        handler.mouse_move(Point::new(213.0, 121.0), no_mods(), &mut scene);
        // Alt pressed after the last move, then released in place.
        handler.mouse_up(Point::new(213.0, 121.0), free(), &mut scene);
        assert_eq!(scene.panels[0].rect, Rect::new(13.0, 21.0, 200.0, 100.0));
    }

    #[test]
    fn release_without_alt_snaps_after_alt_move() {
        let mut scene = Scene::default();
        let mut handler = drawing_terminal();
        handler.mouse_down(Point::new(13.0, 21.0), Button::Left, no_mods(), &mut scene);
        handler.mouse_move(Point::new(213.0, 121.0), free(), &mut scene);
        handler.mouse_up(Point::new(213.0, 121.0), no_mods(), &mut scene);
        assert_eq!(scene.panels[0].rect, Rect::new(16.0, 24.0, 200.0, 96.0));
    }

    #[test]
    fn shared_edge_resize_moves_only_the_left_panel() {
        let mut scene = Scene::default();
        let left = scene.add(terminal(), Rect::new(0.0, 0.0, 200.0, 100.0));
        let right = scene.add(terminal(), Rect::new(201.0, 0.0, 200.0, 100.0));
        scene.bring_to_front(left);
        let mut handler = InputHandler::default();

        // A press in the 1 unit gap grabs the left panel's right edge.
        handler.mouse_down(Point::new(200.5, 50.0), Button::Left, no_mods(), &mut scene);
        assert!(matches!(
            handler.state,
            Interaction::Resizing { id, edge: ResizeEdge::Right, .. } if id == left
        ));
        handler.mouse_move(Point::new(150.5, 50.0), free(), &mut scene);
        handler.mouse_up(Point::new(150.5, 50.0), free(), &mut scene);
        assert_eq!(
            scene.get(left).unwrap().rect,
            Rect::new(0.0, 0.0, 150.0, 100.0)
        );
        assert_eq!(
            scene.get(right).unwrap().rect,
            Rect::new(201.0, 0.0, 200.0, 100.0)
        );
    }

    #[test]
    fn press_on_joined_neighbor_title_bar_does_not_resize() {
        let mut scene = Scene::default();
        let left = scene.add(terminal(), Rect::new(0.0, 0.0, 200.0, 100.0));
        let right = scene.add(terminal(), Rect::new(201.0, 0.0, 200.0, 100.0));
        scene.bring_to_front(left);
        let mut handler = InputHandler::default();

        // Inside the left panel's 8 px outer band, but on the right panel's title bar.
        let press = Point::new(204.0, 5.0);
        assert_eq!(handler.hover_resize(press, &scene), None);
        handler.mouse_down(press, Button::Left, no_mods(), &mut scene);
        assert!(
            !matches!(handler.state, Interaction::Resizing { .. }),
            "got {:?}",
            handler.state
        );
        handler.mouse_up(press, no_mods(), &mut scene);
        assert_eq!(
            scene.get(left).unwrap().rect,
            Rect::new(0.0, 0.0, 200.0, 100.0)
        );
        assert_eq!(
            scene.get(right).unwrap().rect,
            Rect::new(201.0, 0.0, 200.0, 100.0)
        );
    }
}
