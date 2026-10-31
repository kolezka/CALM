//! The canvas: paints panels from the `calm-core` scene and feeds it input.

use calm_core::camera::FIT_MARGIN;
use calm_core::chrome::{
    CORNER_RADIUS, ChromeButton, PanelIcon, TITLE_BAR_HEIGHT, button_dot, button_strokes,
    details_visible, hover_target, icon_strokes, title_bar_layout,
};
use calm_core::frame_stats::FrameStats;
use calm_core::geometry::{Point, Rect, Size};
use calm_core::interaction::{
    Button, ConfirmKey, Effect, InputHandler, Interaction, Modifiers, Tool, ZoomStep,
};
use calm_core::join::{Axis, JoinLayout, JoinedEdges, Side};
use calm_core::menu::{self, MenuCommand, MenuItem};
use calm_core::preview::{PREVIEW_REFRESH_SECS, PanelMode, PreviewImage, decide_mode};
use calm_core::scene::{Panel, PanelId, PanelKind, Scene, chip_rect, content_rect};
use calm_core::status_bar::{status_bar_rect, status_text};
use calm_core::viewport::{NativePlacement, place_native};
use calm_native::{BrowserEvent, HostView, NativeChild, StatusBar, show_context_menu};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, BorderStyle, Bounds, BoxShadow, ContentMask, Context, Corners, DispatchPhase, Edges,
    Entity, FocusHandle, Focusable, InteractiveElement, IntoElement, KeyDownEvent, KeyUpEvent,
    MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, NoAction, ParentElement,
    PathBuilder, PinchEvent, Pixels, Render, RenderImage, ScrollDelta, ScrollWheelEvent,
    SharedString, Styled, Task, TextAlign, TextRun, WeakEntity, Window, actions, canvas, div, fill,
    outline, point, px, quad, rgb, rgba, size,
};

use calm_plugins::{BridgeRequest, InstalledPlugin};
use futures::StreamExt as _;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::background::{self, BackgroundRenderer, BackgroundTexture};
use crate::browser_panel::{BrowserEvents, BrowserPanel};
use crate::dev_menu::{self, ResetFps, ToggleFps};
use crate::plugin_host::{self, BridgeSender, PluginPanel};
use crate::settings_menu::{self, SettingsState};
use crate::terminal::{self, TerminalEvent, TerminalPanel};
use crate::tui_host::{self, TuiRun};
use crate::theme;

actions!(
    calm,
    [
        ZoomIn,
        ZoomOut,
        ZoomReset,
        CenterOnOrigin,
        FitAll,
        DrawTerminal,
        DrawBrowser,
        Cancel,
        ConfirmEnter,
        ToggleFullscreen
    ]
);

pub const KEY_CONTEXT: &str = "Canvas";
const TITLE_FONT_SIZE: f64 = 12.0;
/// Below this the title is unreadable, so it is skipped.
const MIN_TITLE_FONT_PX: f64 = 6.0;
/// Width of the ring around a panel, in screen pixels at any zoom.
const BORDER_PX: f64 = 1.0;
/// Width of the `preview stale` label in the chrome strip, in world units.
const STALE_LABEL_WIDTH: f64 = 96.0;
const STALE_LABEL: &str = "preview stale";
/// Screen pixels left around a panel after Zoom to Fit.
const ZOOM_TO_FIT_MARGIN: f64 = 40.0;
/// Added to the refresh timer so its repaint lands just after a snapshot is due.
const PREVIEW_TIMER_MARGIN: Duration = Duration::from_millis(100);
const CONFIRM_TITLE: &str = "Close? enter / esc";

pub fn key_bindings() -> Vec<gpui::KeyBinding> {
    use gpui::KeyBinding;
    let ctx = Some(KEY_CONTEXT);
    vec![
        KeyBinding::new("cmd-=", ZoomIn, ctx),
        KeyBinding::new("cmd--", ZoomOut, ctx),
        KeyBinding::new("cmd-0", ZoomReset, ctx),
        KeyBinding::new("cmd-shift-h", CenterOnOrigin, ctx),
        KeyBinding::new("cmd-shift-0", FitAll, ctx),
        KeyBinding::new("t", DrawTerminal, ctx),
        KeyBinding::new("b", DrawBrowser, ctx),
        KeyBinding::new("escape", Cancel, ctx),
        KeyBinding::new("enter", ConfirmEnter, ctx),
        KeyBinding::new("cmd-ctrl-f", ToggleFullscreen, ctx),
        // A focused terminal gets these keys; canvas zoom and fullscreen still apply.
        KeyBinding::new("t", NoAction, Some(terminal::KEY_CONTEXT)),
        KeyBinding::new("b", NoAction, Some(terminal::KEY_CONTEXT)),
        KeyBinding::new("escape", NoAction, Some(terminal::KEY_CONTEXT)),
        KeyBinding::new("enter", NoAction, Some(terminal::KEY_CONTEXT)),
    ]
}

pub struct CanvasView {
    pub scene: Scene,
    pub input: InputHandler,
    /// Window points of the canvas area, the whole window. Set during prepaint.
    pub viewport: Rect,
    /// (id, name) per discovered web plugin, for the Add menu.
    pub plugins: Vec<(String, String)>,
    /// (id, name) per discovered TUI plugin, listed next to Terminal in the Add menu.
    pub apps: Vec<(String, String)>,
    /// (id, default panel size) per discovered plugin.
    pub plugin_sizes: Vec<(String, Size)>,
    /// Plugins found at startup. A new plugin needs an app restart to show up.
    installed_plugins: Vec<InstalledPlugin>,
    /// Plugin folders skipped at startup, shown in the status bar; stderr names each one.
    pub plugin_errors: usize,
    /// Live plugin pages by panel. Dropping an entry closes the page.
    plugin_panels: Vec<(PanelId, PluginPanel)>,
    bridge: BridgeSender,
    /// Feeds bridge messages from plugin pages into `plugin_message`.
    _bridge_task: Task<()>,
    /// Path of the last saved layout, shown in the status bar. Task 13 sets it.
    pub saved_flash: Option<String>,
    focused: Option<PanelId>,
    /// Panel under the pointer and the chrome button under it. Shows the buttons.
    hover: Option<(PanelId, Option<ChromeButton>)>,
    /// Live terminals by panel. Dropping an entry ends its process.
    terminals: Vec<(PanelId, Entity<TerminalPanel>)>,
    /// Restart state of TUI panels. Their terminal is in `terminals`.
    tui_runs: Vec<(PanelId, TuiRun)>,
    /// Live browser panels. Dropping an entry closes its page.
    browsers: Vec<(PanelId, BrowserPanel)>,
    /// Sender for page events; the receiving task starts with the first browser.
    browser_events: Option<BrowserEvents>,
    browser_listener: Option<Task<()>>,
    /// Back-to-front order of native panels (terminals, browsers and plugins) as last stacked.
    native_order: Vec<PanelId>,
    focus_handle: FocusHandle,
    pub show_fps: bool,
    pub frame_stats: FrameStats,
    /// True while a native context menu is up. Terminals show previews meanwhile.
    menu_open: bool,
    /// Repaints every `PREVIEW_REFRESH_SECS` while any terminal is in `Preview`.
    preview_timer: Option<Task<()>>,
    /// Terminal input divs that a paint found over a `Preview` panel. Smoke expects 0.
    pub preview_input_targets: usize,
    pub settings: SettingsState,
    /// Created on first `cmd-,`, then hidden and shown.
    pub settings_window: Option<calm_native::SettingsWindow>,
    pub background: BackgroundRenderer,
    /// Native bar floating over the bottom of the canvas. Created on the first paint.
    status_bar: Option<StatusBar>,
    /// Set when the status bar could not be created, so paint stops retrying.
    status_bar_error: Option<String>,
    /// Joins and visible dividers in the last render. Read by the smoke.
    pub join_counts: (usize, usize),
}

impl CanvasView {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let discovery = plugin_host::discover_installed();
        let entries = plugin_host::menu_entries(&discovery.plugins);
        let (bridge, mut messages) = futures::channel::mpsc::unbounded::<(PanelId, String)>();
        let bridge_task = cx.spawn(async move |this, cx| {
            while let Some((id, text)) = messages.next().await {
                if this
                    .update(cx, |view, cx| view.plugin_message(id, &text, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            scene: Scene::default(),
            input: InputHandler::default(),
            viewport: Rect::new(0.0, 0.0, 0.0, 0.0),
            plugins: entries.web,
            apps: entries.apps,
            plugin_sizes: entries.sizes,
            installed_plugins: discovery.plugins,
            plugin_errors: discovery.errors.len(),
            plugin_panels: Vec::new(),
            bridge,
            _bridge_task: bridge_task,
            saved_flash: None,
            focused: None,
            hover: None,
            terminals: Vec::new(),
            tui_runs: Vec::new(),
            browsers: Vec::new(),
            browser_events: None,
            browser_listener: None,
            native_order: Vec::new(),
            focus_handle: cx.focus_handle(),
            show_fps: false,
            frame_stats: FrameStats::default(),
            menu_open: false,
            preview_timer: None,
            preview_input_targets: 0,
            settings: SettingsState {
                settings: Default::default(),
                path: calm_core::settings::settings_path(),
                notice: None,
            },
            settings_window: None,
            background: BackgroundRenderer::default(),
            status_bar: None,
            status_bar_error: None,
            join_counts: (0, 0),
        }
    }

    /// Installs loaded settings at startup.
    pub fn set_settings_state(&mut self, state: SettingsState) {
        self.settings = state;
        self.apply_settings_to_input();
    }

    /// Pushes the settings the input handler reads.
    pub fn apply_settings_to_input(&mut self) {
        self.input.snap = self.settings.settings.snap.config();
    }

    pub fn set_show_fps(&mut self, show: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.show_fps = show;
        self.frame_stats.reset();
        // Rebuild the menu bar so the Show FPS check mark follows the state.
        let settings = self.settings.settings.clone();
        cx.defer(move |cx| cx.set_menus(settings_menu::menus(show, &settings)));
        self.refresh_settings_form();
        window.refresh();
        cx.notify();
    }

    fn reset_fps(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.frame_stats.reset();
        window.refresh();
        cx.notify();
    }

    pub fn apply_effects(
        &mut self,
        effects: Vec<Effect>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        // CreatePanel has no id; the model emits FocusPanel(id) for it right after.
        let mut created: Option<PanelKind> = None;
        let mut queue: std::collections::VecDeque<Effect> = effects.into();
        while let Some(effect) = queue.pop_front() {
            match effect {
                Effect::CreatePanel { kind, .. } => created = Some(kind),
                Effect::FocusPanel(id) => {
                    let new_browser = matches!(created, Some(PanelKind::Browser { .. }));
                    if let Some(kind) = created.take() {
                        self.panel_created(id, &kind, window, cx);
                    }
                    self.focused = Some(id);
                    if let Some(terminal) = self.terminal(id) {
                        terminal.update(cx, |terminal, cx| terminal.focus(window, cx));
                    }
                    if let Some((_, browser)) = self.browsers.iter_mut().find(|(b, _)| *b == id) {
                        // A new panel waits for an address; an existing one gets the keyboard.
                        if new_browser {
                            browser.focus_address();
                        } else {
                            browser.focus_page();
                        }
                    }
                }
                Effect::ClosePanelRequested(id) => {
                    // A browser's page is state too, so it confirms like a running terminal.
                    let alive = self
                        .terminal(id)
                        .is_some_and(|terminal| terminal.read(cx).is_alive())
                        || self.browser(id).is_some();
                    let confirm = alive && self.settings.settings.panels.confirm_close;
                    queue.push_back(self.input.request_close(id, confirm));
                }
                Effect::ConfirmClose(_) => {
                    // Enter and Escape must reach the canvas, not the terminal.
                    self.focus_handle.focus(window, cx);
                }
                Effect::ToggleMinimize(id) => {
                    if let Some(panel) = self.scene.get_mut(id) {
                        panel.minimized = !panel.minimized;
                    }
                }
                Effect::Redraw => {}
                Effect::ShowCanvasMenu { screen, world } => {
                    let items = menu::canvas_menu(&self.plugins, &self.apps);
                    self.show_menu(items, screen, MenuTarget::Canvas { world }, window, cx);
                }
                Effect::ShowPanelMenu { id, screen } => {
                    if let Some(panel) = self.scene.get(id) {
                        let can_restart = self
                            .tui_run(id)
                            .is_some_and(|run| run.supervisor.can_restart());
                        let items = menu::panel_menu(panel, can_restart);
                        self.show_menu(items, screen, MenuTarget::Panel(id), window, cx);
                    }
                }
                Effect::ClosePanel(id) => {
                    if let Some(terminal) = self.terminal(id) {
                        terminal.update(cx, |terminal, _| terminal.release_preview(window));
                    }
                    self.terminals.retain(|(panel, _)| *panel != id);
                    self.tui_runs.retain(|(panel, _)| *panel != id);
                    self.browsers.retain(|(panel, _)| *panel != id);
                    self.plugin_panels.retain(|(panel, _)| *panel != id);
                    self.scene.remove(id);
                    if self.focused == Some(id) {
                        self.focused = None;
                    }
                }
            }
        }
        window.refresh();
        cx.notify();
    }

    /// Pops up a native menu at the canvas point `screen` and applies the choice.
    /// The menu tracks modally, so it runs in a task instead of inside this event handler:
    /// GPUI keeps drawing while the menu is open and must not find the app borrowed.
    fn show_menu(
        &mut self,
        items: Vec<MenuItem>,
        screen: Point,
        target: MenuTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let host = match HostView::new(window) {
            Ok(host) => host,
            Err(err) => {
                eprintln!("calm: context menu unavailable: {err}");
                return;
            }
        };
        let at = Point::new(
            screen.x + self.viewport.origin.x,
            screen.y + self.viewport.origin.y,
        );
        // A menu counts as modal: terminals switch to previews while it is up.
        self.menu_open = true;
        window.refresh();
        cx.notify();
        cx.spawn_in(window, async move |this, cx| {
            let choice = show_context_menu(&items, at, &host);
            this.update_in(cx, |view, window, cx| {
                view.menu_open = false;
                match choice {
                    Some(id) => view.apply_menu_choice(&id, target, window, cx),
                    None => {
                        window.refresh();
                        cx.notify();
                    }
                }
            })
            .ok();
        })
        .detach();
    }

    fn apply_menu_choice(
        &mut self,
        id: &str,
        target: MenuTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let world = match target {
            MenuTarget::Canvas { world } => world,
            MenuTarget::Panel(_) => Point::new(0.0, 0.0),
        };
        let effects = match (menu::menu_action(id, world, &self.plugin_sizes), target) {
            (Some(MenuCommand::AddPanel { kind, mut rect }), MenuTarget::Canvas { .. }) => {
                if matches!(kind, PanelKind::Terminal { .. }) {
                    rect.size = self.settings.settings.panels.default_terminal_size();
                }
                let id = self.scene.add(kind.clone(), rect);
                let rect = self.scene.get(id).map_or(rect, |panel| panel.rect);
                vec![Effect::CreatePanel { kind, rect }, Effect::FocusPanel(id)]
            }
            (Some(MenuCommand::Restart), MenuTarget::Panel(id)) => {
                self.restart_tui(id, true, window, cx);
                vec![Effect::Redraw]
            }
            (Some(MenuCommand::Minimize), MenuTarget::Panel(id)) => {
                vec![Effect::ToggleMinimize(id), Effect::Redraw]
            }
            // Delete takes the normal close path, so a live process still asks first.
            (Some(MenuCommand::Delete), MenuTarget::Panel(id)) => {
                vec![Effect::ClosePanelRequested(id)]
            }
            (Some(MenuCommand::BringToFront), MenuTarget::Panel(id)) => {
                self.scene.bring_to_front(id);
                vec![Effect::Redraw]
            }
            (Some(MenuCommand::SendToBack), MenuTarget::Panel(id)) => {
                self.scene.send_to_back(id);
                vec![Effect::Redraw]
            }
            (Some(MenuCommand::ZoomToFit), MenuTarget::Panel(id)) => {
                let Some(rect) = self.scene.get(id).map(|panel| panel.rect) else {
                    return;
                };
                self.scene
                    .camera
                    .fit(Some(rect), self.viewport.size, ZOOM_TO_FIT_MARGIN);
                vec![Effect::Redraw]
            }
            (Some(MenuCommand::Duplicate), MenuTarget::Panel(id)) => {
                let Some(copy) = self.scene.duplicate(id) else {
                    return;
                };
                let Some(panel) = self.scene.get(copy) else {
                    return;
                };
                vec![
                    Effect::CreatePanel {
                        kind: panel.kind.clone(),
                        rect: panel.rect,
                    },
                    Effect::FocusPanel(copy),
                ]
            }
            _ => return,
        };
        self.apply_effects(effects, window, cx);
    }

    fn panel_created(
        &mut self,
        id: PanelId,
        kind: &PanelKind,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match kind {
            PanelKind::Terminal { command, cwd } => {
                let mut options = terminal::options(command.as_deref(), cwd.clone());
                settings_menu::apply_terminal_config(&mut options, &self.settings.settings);
                if let Err(err) = self.spawn_terminal(id, options, window, cx) {
                    eprintln!("calm: terminal failed to start: {err}");
                }
            }
            PanelKind::Browser { url } => {
                if let Err(err) = self.spawn_browser(id, url, window, cx) {
                    eprintln!("calm: browser failed to start: {err}");
                }
            }
            PanelKind::Plugin { plugin_id, .. } => {
                if let Err(err) = self.open_plugin_panel(id, plugin_id, window) {
                    eprintln!("calm: plugin panel failed to open: {err}");
                    if let Some(panel) = self.scene.get_mut(id) {
                        panel.title = format!("{plugin_id}: failed to open");
                    }
                }
            }
            PanelKind::Tui { plugin_id } => {
                if let Err(err) = self.open_tui_panel(id, plugin_id, window, cx) {
                    eprintln!("calm: tui panel failed to open: {err}");
                    if let Some(panel) = self.scene.get_mut(id) {
                        panel.title = format!("{plugin_id}: failed to open");
                    }
                }
            }
        }
    }

    /// Starts a TUI plugin's command in a terminal for a panel already in the scene.
    pub fn open_tui_panel(
        &mut self,
        id: PanelId,
        plugin_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let plugin = self
            .installed_plugins
            .iter()
            .find(|plugin| plugin.manifest.id == plugin_id)
            .ok_or_else(|| format!("plugin {plugin_id} is not installed"))?
            .clone();
        let run = TuiRun::new(plugin)?;
        self.tui_runs.push((id, run));
        self.launch_tui(id, window, cx)
    }

    fn tui_run(&self, id: PanelId) -> Option<&TuiRun> {
        self.tui_runs
            .iter()
            .find(|(panel, _)| *panel == id)
            .map(|(_, run)| run)
    }

    fn tui_run_mut(&mut self, id: PanelId) -> Option<&mut TuiRun> {
        self.tui_runs
            .iter_mut()
            .find(|(panel, _)| *panel == id)
            .map(|(_, run)| run)
    }

    /// Spawns the next process of a TUI panel. An old, exited terminal is dropped first.
    fn launch_tui(
        &mut self,
        id: PanelId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let run = self
            .tui_run_mut(id)
            .ok_or_else(|| format!("panel {} has no tui state", id.0))?;
        let mut options = run.next_launch(id)?;
        let title = run.title();
        settings_menu::apply_terminal_config(&mut options, &self.settings.settings);
        if let Some(old) = self.terminal(id) {
            old.update(cx, |terminal, _| terminal.release_preview(window));
            self.terminals.retain(|(panel, _)| *panel != id);
        }
        self.spawn_terminal(id, options, window, cx)?;
        let terminal = self
            .terminal(id)
            .ok_or("terminal missing right after spawn")?;
        let subscription = cx.subscribe_in(
            &terminal,
            window,
            move |view: &mut Self, _, event: &TerminalEvent, window, cx| match event {
                TerminalEvent::Exited => view.tui_exited(id, window, cx),
                TerminalEvent::RestartRequested => view.restart_tui(id, true, window, cx),
            },
        );
        if let Some(run) = self.tui_run_mut(id) {
            run.subscription = Some(subscription);
        }
        if let Some(panel) = self.scene.get_mut(id) {
            panel.title = title;
        }
        // The new native view was added on top; restack on the next paint.
        self.native_order.clear();
        Ok(())
    }

    fn tui_exited(&mut self, id: PanelId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(run) = self.tui_run_mut(id) else {
            return;
        };
        let code = tui_host::read_exit_code(&run.status_file);
        let action = run.supervisor.exited(code, run.started_at.elapsed());
        let title = run.title();
        let launches = run.launches;
        if run.supervisor.state == calm_core::tui::TuiState::NotFound {
            eprintln!(
                "calm: tui {}: command not found: {} (plugin dir {})",
                run.plugin.manifest.id,
                run.spec().command,
                run.plugin.dir.display()
            );
        }
        if let Some(panel) = self.scene.get_mut(id) {
            panel.title = title;
        }
        if let calm_core::tui::ExitAction::RestartAfter(delay) = action {
            cx.spawn_in(window, async move |this, cx| {
                cx.background_executor().timer(delay).await;
                this.update_in(cx, |view, window, cx| {
                    // Skip if the panel closed or was restarted by hand meanwhile.
                    if view.tui_run(id).is_some_and(|run| run.launches == launches) {
                        view.restart_tui(id, false, window, cx);
                    }
                })
                .ok();
            })
            .detach();
        }
        window.refresh();
        cx.notify();
    }

    /// `manual` is a user's Restart, which only works once the process has exited.
    pub fn restart_tui(&mut self, id: PanelId, manual: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(run) = self.tui_run_mut(id) else {
            return;
        };
        if manual && !run.supervisor.restart_requested() {
            return;
        }
        if let Err(err) = self.launch_tui(id, window, cx) {
            eprintln!("calm: tui panel failed to restart: {err}");
            if let Some(panel) = self.scene.get_mut(id) {
                panel.title = format!("{}: failed to restart", panel.title);
            }
        }
        if let Some(terminal) = self.terminal(id) {
            terminal.update(cx, |terminal, cx| terminal.focus(window, cx));
        }
        window.refresh();
        cx.notify();
    }

    /// Per TUI panel in creation order: chrome title, state, Restart offered, launches.
    pub fn tui_status(&self) -> Vec<(String, calm_core::tui::TuiState, bool, u64)> {
        self.tui_runs
            .iter()
            .map(|(id, run)| {
                let title = self
                    .scene
                    .get(*id)
                    .map_or_else(String::new, |panel| panel.title.clone());
                (
                    title,
                    run.supervisor.state,
                    run.supervisor.can_restart(),
                    run.launches,
                )
            })
            .collect()
    }

    /// Opens the web view for a plugin panel already in the scene. The chrome shows the
    /// plugin's name, or that it is not built.
    pub fn open_plugin_panel(
        &mut self,
        id: PanelId,
        plugin_id: &str,
        window: &mut Window,
    ) -> Result<(), String> {
        let plugin = self
            .installed_plugins
            .iter()
            .find(|plugin| plugin.manifest.id == plugin_id)
            .ok_or_else(|| format!("plugin {plugin_id} is not installed"))?;
        let panel = PluginPanel::open(id, plugin, self.bridge.clone(), window)?;
        let title = match panel.load_error() {
            Some(_) => format!("{} (not built)", plugin.manifest.name),
            None => plugin.manifest.name.clone(),
        };
        if let Some(scene_panel) = self.scene.get_mut(id) {
            scene_panel.title = title;
        }
        self.plugin_panels.push((id, panel));
        Ok(())
    }

    /// Installs plugins found outside the startup scan, e.g. the smoke's fixture.
    pub fn install_plugins(&mut self, plugins: Vec<InstalledPlugin>) {
        let entries = plugin_host::menu_entries(&plugins);
        self.plugins.extend(entries.web);
        self.apps.extend(entries.apps);
        self.plugin_sizes.extend(entries.sizes);
        self.installed_plugins.extend(plugins);
    }

    /// (loading, load error) per plugin panel, in creation order.
    pub fn plugin_status(&self) -> Vec<(bool, Option<String>)> {
        self.plugin_panels
            .iter()
            .map(|(_, panel)| (panel.is_loading(), panel.load_error().map(str::to_string)))
            .collect()
    }

    /// One message a plugin page posted. Bad JSON has no id to answer, so it is logged.
    fn plugin_message(&mut self, id: PanelId, text: &str, cx: &mut Context<Self>) {
        let request = match BridgeRequest::from_json(text) {
            Ok(request) => request,
            Err(err) => {
                eprintln!("calm: plugin panel {}: bad bridge message: {err}", id.0);
                return;
            }
        };
        let Some(panel) = self.scene.get_mut(id) else {
            return;
        };
        let Some(response) = plugin_host::dispatch(panel, &request) else {
            return;
        };
        if let Some((_, plugin)) = self.plugin_panels.iter_mut().find(|(p, _)| *p == id) {
            plugin.reply(&response);
        }
        cx.notify();
    }

    /// Applies each plugin panel's mode for this frame. Called right after paint with the
    /// modes and placements paint computed, so the chrome and the web view agree.
    fn apply_plugin_modes(
        &mut self,
        modes: &[(PanelId, PanelMode, Option<NativePlacement>)],
        window: &Window,
    ) {
        for (id, mode, placement) in modes {
            if let Some((_, plugin)) = self.plugin_panels.iter_mut().find(|(p, _)| p == id) {
                plugin.apply_mode(*mode, *placement, window);
            }
        }
    }

    /// Adds a live browser for the scene panel `id`, loading `address`.
    pub fn spawn_browser(
        &mut self,
        id: PanelId,
        address: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let events = self.browser_events(window, cx);
        let browser = BrowserPanel::open(id, address, events, window)?;
        self.browsers.push((id, browser));
        Ok(())
    }

    /// The page event sender, starting the task that applies events on first use.
    fn browser_events(&mut self, window: &mut Window, cx: &mut Context<Self>) -> BrowserEvents {
        if let Some(sender) = &self.browser_events {
            return sender.clone();
        }
        let (sender, receiver) = async_channel::unbounded();
        self.browser_listener = Some(cx.spawn_in(window, async move |this, cx| {
            while let Ok((id, event)) = receiver.recv().await {
                let applied = this.update_in(cx, |view, window, cx| {
                    view.browser_event(id, event, window, cx)
                });
                if applied.is_err() {
                    break;
                }
            }
        }));
        self.browser_events = Some(sender.clone());
        sender
    }

    fn browser_event(
        &mut self,
        id: PanelId,
        event: BrowserEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(panel) = self.scene.get_mut(id) else {
            return;
        };
        match event {
            BrowserEvent::TitleChanged(title) => {
                if !title.is_empty() {
                    panel.title = title;
                }
            }
            BrowserEvent::UrlChanged(url) => {
                panel.title = url.clone();
                panel.kind = PanelKind::Browser { url };
            }
            BrowserEvent::Clicked => {
                // The click went to the native view, so GPUI never saw it.
                self.scene.bring_to_front(id);
                self.focused = Some(id);
                self.focus_handle.focus(window, cx);
            }
        }
        window.refresh();
        cx.notify();
    }

    fn browser(&self, id: PanelId) -> Option<&BrowserPanel> {
        self.browsers
            .iter()
            .find(|(panel, _)| *panel == id)
            .map(|(_, browser)| browser)
    }

    /// (mode, loading, page URL, scene URL) per browser, in creation order.
    pub fn browser_status(&self) -> Vec<(PanelMode, bool, Option<String>, Option<String>)> {
        self.browsers
            .iter()
            .map(|(id, browser)| {
                let scene_url = self.scene.get(*id).and_then(|panel| match &panel.kind {
                    PanelKind::Browser { url } => Some(url.clone()),
                    _ => None,
                });
                (
                    browser.mode(),
                    browser.is_loading(),
                    browser.url(),
                    scene_url,
                )
            })
            .collect()
    }

    pub fn spawn_terminal(
        &mut self,
        id: PanelId,
        options: gpui_libghostty::TerminalOptions,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let host = NativeChild::new(window).map_err(|err| err.to_string())?;
        let terminal = TerminalPanel::spawn(options, host, window, cx)?;
        self.terminals.push((id, terminal));
        Ok(())
    }

    fn terminal(&self, id: PanelId) -> Option<Entity<TerminalPanel>> {
        self.terminals
            .iter()
            .find(|(panel, _)| *panel == id)
            .map(|(_, terminal)| terminal.clone())
    }

    /// (frame count, alive) per terminal, in creation order.
    pub fn terminal_status(&self, cx: &mut App) -> Vec<(u64, bool)> {
        self.terminals
            .iter()
            .map(|(_, terminal)| {
                terminal.update(cx, |terminal, _| {
                    (terminal.frame_count(), terminal.is_alive())
                })
            })
            .collect()
    }

    /// (mode, has a snapshot) per terminal, in creation order.
    pub fn terminal_previews(&self, cx: &App) -> Vec<(PanelMode, bool)> {
        self.terminals
            .iter()
            .map(|(_, terminal)| {
                let terminal = terminal.read(cx);
                (terminal.mode(), terminal.preview().image().is_some())
            })
            .collect()
    }

    /// Snapshot attempts per terminal, in creation order.
    pub fn snapshot_counts(&self, cx: &App) -> Vec<u64> {
        self.terminals
            .iter()
            .map(|(_, terminal)| terminal.read(cx).snapshot_count())
            .collect()
    }

    /// Runs the refresh timer only while some terminal is in `Preview`.
    fn sync_preview_timer(&mut self, previewing: bool, cx: &mut Context<Self>) {
        if !previewing {
            self.preview_timer = None;
            return;
        }
        if self.preview_timer.is_some() {
            return;
        }
        self.preview_timer = Some(cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_secs(PREVIEW_REFRESH_SECS) + PREVIEW_TIMER_MARGIN)
                    .await;
                if this.update(cx, |_, cx| cx.notify()).is_err() {
                    break;
                }
            }
        }));
    }

    /// Ids of panels with a native view (terminals, browsers and plugins), back to front.
    fn native_panel_order(&self) -> Vec<PanelId> {
        self.scene
            .panels
            .iter()
            .map(|panel| panel.id)
            .filter(|id| {
                self.terminals.iter().any(|(t, _)| t == id)
                    || self.browsers.iter().any(|(b, _)| b == id)
                    || self.plugin_panels.iter().any(|(p, _)| p == id)
            })
            .collect()
    }

    /// Native views stack in creation order, so restack them when the scene order changes.
    /// Terminals, browsers and plugins share one stack, so they restack together.
    fn sync_native_order(&mut self, cx: &mut Context<Self>) {
        let order = self.native_panel_order();
        if order == self.native_order {
            return;
        }
        for id in &order {
            if let Some(terminal) = self.terminal(*id) {
                terminal.update(cx, |terminal, _| terminal.bring_to_front());
            } else if let Some((_, browser)) = self.browsers.iter_mut().find(|(b, _)| b == id) {
                browser.bring_to_front();
            } else if let Some((_, plugin)) = self.plugin_panels.iter_mut().find(|(p, _)| p == id) {
                plugin.bring_to_front();
            }
        }
        self.native_order = order;
    }

    /// Updates the native status bar's text and frame and keeps it above every panel.
    /// Runs after `sync_native_order`, which can restack panels over it.
    fn sync_status_bar(&mut self, window: &Window) {
        if self.status_bar.is_none() && self.status_bar_error.is_none() {
            match StatusBar::new(window) {
                Ok(bar) => self.status_bar = Some(bar),
                Err(err) => {
                    eprintln!("calm: status bar failed to start: {err}");
                    self.status_bar_error = Some(err.to_string());
                }
            }
        }
        let saved = self
            .saved_flash
            .as_ref()
            .map(|path| format!("Saved {path}"))
            .unwrap_or_default();
        let zoom = format!("{:.0}%", self.scene.camera.zoom * 100.0);
        let fps = if self.show_fps {
            dev_menu::fps_text(&self.frame_stats)
        } else {
            String::new()
        };
        // The pill has one text color, so the warning sign marks the error item.
        let plugin_errors = if self.plugin_errors > 0 {
            format!("⚠ plugins: {} errors", self.plugin_errors)
        } else {
            String::new()
        };
        let text = status_text([
            saved.as_str(),
            plugin_errors.as_str(),
            zoom.as_str(),
            fps.as_str(),
        ]);
        let viewport = self.viewport;
        let Some(bar) = self.status_bar.as_mut() else {
            return;
        };
        let text_width = bar.set_text(&text);
        let window_height = f64::from(f32::from(window.viewport_size().height));
        bar.apply(status_bar_rect(viewport, text_width), window_height);
        bar.ensure_front();
    }

    /// (visible, topmost, text) of the status bar, or the reason it is missing.
    pub fn status_bar_state(&self) -> Result<(bool, bool, String), String> {
        match (&self.status_bar, &self.status_bar_error) {
            (Some(bar), _) => Ok((bar.is_visible(), bar.is_front(), bar.text().to_string())),
            (None, Some(err)) => Err(err.clone()),
            (None, None) => Err("not created yet".to_string()),
        }
    }

    /// (scene order, native stacking order) of all native panel ids, both back to front.
    pub fn native_zorder(&self, cx: &App) -> (Vec<u64>, Vec<u64>) {
        let scene = self.native_panel_order().iter().map(|id| id.0).collect();
        let mut native: Vec<(Option<usize>, u64)> = self
            .terminals
            .iter()
            .map(|(id, terminal)| (terminal.read(cx).stack_index(), id.0))
            .chain(
                self.browsers
                    .iter()
                    .map(|(id, browser)| (browser.stack_index(), id.0)),
            )
            .chain(
                self.plugin_panels
                    .iter()
                    .map(|(id, plugin)| (plugin.stack_index(), id.0)),
            )
            .collect();
        native.sort();
        (scene, native.into_iter().map(|(_, id)| id).collect())
    }

    /// Places or hides each browser for this paint, like `apply_terminal_modes`. A browser
    /// in preview gets a flat rect with its title: there is no page snapshot yet.
    fn apply_browser_modes(
        &mut self,
        viewport: Rect,
        modal_open: bool,
        window: &Window,
    ) -> Vec<(PanelId, TerminalContent)> {
        let mut contents = Vec::new();
        for (id, browser) in &mut self.browsers {
            let Some(panel) = self.scene.get(*id) else {
                continue;
            };
            let mode = terminal_mode(
                panel,
                &self.scene,
                viewport,
                modal_open,
                self.settings.settings.panels.preview_below_zoom,
            );
            let placement = terminal_placement(panel, &self.scene, viewport);
            browser.apply_mode(mode, placement, window);
            match browser.mode() {
                PanelMode::Live => contents.push((*id, TerminalContent::Live)),
                PanelMode::Preview => {
                    contents.push((*id, TerminalContent::Preview(PreviewImage::None)))
                }
                PanelMode::Hidden => {}
            }
        }
        contents
    }

    fn confirm_enter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(effect) = self.input.confirm_key(ConfirmKey::Enter) {
            self.apply_effects(vec![effect], window, cx);
        }
    }

    fn to_canvas(&self, position: gpui::Point<Pixels>) -> Point {
        Point::new(
            f64::from(f32::from(position.x)) - self.viewport.origin.x,
            f64::from(f32::from(position.y)) - self.viewport.origin.y,
        )
    }

    fn mouse_down(&mut self, event: &MouseDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let button = match event.button {
            MouseButton::Left => Button::Left,
            MouseButton::Middle => Button::Middle,
            MouseButton::Right => Button::Right,
            _ => return,
        };
        // A right click on a terminal's content must not take its focus.
        if button != Button::Right {
            self.focus_handle.focus(window, cx);
        }
        let screen = self.to_canvas(event.position);
        let effects =
            self.input
                .mouse_down(screen, button, to_mods(&event.modifiers), &mut self.scene);
        self.apply_effects(effects, window, cx);
    }

    fn mouse_move(&mut self, event: &MouseMoveEvent, window: &mut Window, cx: &mut Context<Self>) {
        let screen = self.to_canvas(event.position);
        let effects = self
            .input
            .mouse_move(screen, to_mods(&event.modifiers), &mut self.scene);
        let hover = hover_target(&self.scene, self.scene.camera.screen_to_world(screen));
        let hover_changed = hover != self.hover;
        self.hover = hover;
        if !effects.is_empty() {
            self.apply_effects(effects, window, cx);
        } else if hover_changed {
            cx.notify();
        }
    }

    fn mouse_up(&mut self, event: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(event.button, MouseButton::Left | MouseButton::Middle) {
            return;
        }
        let effects = self.input.mouse_up(
            self.to_canvas(event.position),
            to_mods(&event.modifiers),
            &mut self.scene,
        );
        if !effects.is_empty() {
            self.apply_effects(effects, window, cx);
        }
    }

    fn scroll(&mut self, event: &ScrollWheelEvent, window: &mut Window, cx: &mut Context<Self>) {
        let delta = match event.delta {
            ScrollDelta::Pixels(_) => event.delta.pixel_delta(px(0.0)),
            ScrollDelta::Lines(_) => event.delta.pixel_delta(px(20.0)),
        };
        let screen = self.to_canvas(event.position);
        self.input.scroll(
            screen,
            f64::from(f32::from(delta.x)),
            f64::from(f32::from(delta.y)),
            to_mods(&event.modifiers),
            &mut self.scene,
        );
        window.refresh();
        cx.notify();
    }

    fn pinch(&mut self, event: &PinchEvent, window: &mut Window, cx: &mut Context<Self>) {
        let screen = self.to_canvas(event.position);
        let effects = self
            .input
            .pinch(screen, f64::from(event.delta), &mut self.scene);
        if !effects.is_empty() {
            self.apply_effects(effects, window, cx);
        }
    }

    fn key_zoom(&mut self, step: ZoomStep, window: &mut Window, cx: &mut Context<Self>) {
        let center = Point::new(
            self.viewport.size.width / 2.0,
            self.viewport.size.height / 2.0,
        );
        self.input.key_zoom(step, center, &mut self.scene);
        window.refresh();
        cx.notify();
    }

    /// Jumps the camera: `CenterOnOrigin` keeps the zoom, `FitAll` frames every panel.
    fn frame_view(&mut self, fit_all: bool, window: &mut Window, cx: &mut Context<Self>) {
        let viewport = self.viewport.size;
        if fit_all {
            let bounds = self.scene.bounds();
            self.scene.camera.fit(bounds, viewport, FIT_MARGIN);
        } else {
            self.scene.camera.center_on(Point::new(0.0, 0.0), viewport);
        }
        window.refresh();
        cx.notify();
    }

    fn set_tool(&mut self, tool: Tool, window: &mut Window, cx: &mut Context<Self>) {
        self.input.tool = tool;
        window.refresh();
        cx.notify();
    }

    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.input.confirming.is_some() {
            if let Some(effect) = self.input.confirm_key(ConfirmKey::Escape) {
                self.apply_effects(vec![effect], window, cx);
            }
            return;
        }
        self.set_tool(Tool::Select, window, cx);
    }

    fn key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let mods = event.keystroke.modifiers;
        if event.keystroke.key == "space" && !mods.platform && !mods.control && !mods.alt {
            self.input.space_held = true;
            window.refresh();
            cx.stop_propagation();
        }
    }

    fn key_up(&mut self, event: &KeyUpEvent, window: &mut Window, _cx: &mut Context<Self>) {
        if event.keystroke.key == "space" {
            self.input.space_held = false;
            window.refresh();
        }
    }
}

/// What a context menu was opened on.
#[derive(Clone, Copy, Debug)]
enum MenuTarget {
    /// Empty canvas; new panels go at `world`.
    Canvas {
        world: Point,
    },
    Panel(PanelId),
}

/// What paint needs, copied out at render time so painting does not borrow the view.
struct PaintSnapshot {
    scene: Scene,
    state: Interaction,
    focused: Option<PanelId>,
    confirming: Option<PanelId>,
    hover: Option<(PanelId, Option<ChromeButton>)>,
    terminals: Vec<(PanelId, Entity<TerminalPanel>)>,
    /// Panels with a plugin web view.
    plugin_ids: Vec<PanelId>,
    /// TUI panels whose manifest has an `icon`, drawn as text in place of the icon strokes.
    glyphs: Vec<(PanelId, String)>,
    /// Close confirmation or context menu up: native views must not cover it.
    modal_open: bool,
    /// Terminals that got an input div in this render.
    input_targets: Vec<PanelId>,
    settings: calm_core::settings::Settings,
    /// Rendered gradient or image, if one is ready.
    background: Option<BackgroundTexture>,
    /// Computed once per render from `scene`.
    joins: JoinLayout,
}

/// How a terminal or plugin panel's content is drawn this paint. A plugin in Preview has
/// no snapshot, so it shows the flat rect with the title.
enum TerminalContent {
    /// The native view covers the content rect.
    Live,
    /// A GPUI-drawn snapshot, possibly stale, or a flat rect with the title.
    Preview(PreviewImage<Arc<RenderImage>>),
}

/// The mode a terminal panel gets this frame. Render and paint both use it, so input
/// targets match what paint applies. `viewport` is the canvas area; only its size counts.
fn terminal_mode(
    panel: &Panel,
    scene: &Scene,
    viewport: Rect,
    modal_open: bool,
    preview_below_zoom: f64,
) -> PanelMode {
    let local = Rect::new(0.0, 0.0, viewport.size.width, viewport.size.height);
    let screen = scene.camera.world_rect_to_screen(content_rect(panel));
    decide_mode(
        scene.camera.zoom,
        preview_below_zoom,
        screen,
        local,
        panel.minimized,
        modal_open,
    )
}

/// A panel's native placement in window points, or `None` when minimized or off screen.
/// `viewport` is the canvas area in window points.
fn terminal_placement(panel: &Panel, scene: &Scene, viewport: Rect) -> Option<NativePlacement> {
    if panel.minimized {
        return None;
    }
    let local = Rect::new(0.0, 0.0, viewport.size.width, viewport.size.height);
    let mut placement = place_native(content_rect(panel), &scene.camera, local)?;
    placement.container.origin.x += viewport.origin.x;
    placement.container.origin.y += viewport.origin.y;
    Some(placement)
}

impl PaintSnapshot {
    fn paint_canvas(
        &self,
        bounds: Bounds<Pixels>,
        this: WeakEntity<CanvasView>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let origin = point(
            f64::from(f32::from(bounds.origin.x)),
            f64::from(f32::from(bounds.origin.y)),
        );
        let mut previewing = false;
        let mut modes_changed = false;
        let mut preview_targets = 0;
        let mut plugin_modes = Vec::new();
        window.with_content_mask(Some(ContentMask { bounds }), |window| {
            let base = self.settings.background.base_color();
            window.paint_quad(fill(bounds, rgb(base.0)));
            if let Some(texture) = &self.background {
                background::paint_background(texture, bounds, window);
            }
            let camera = self.scene.camera;
            if self.settings.grid.show {
                settings_menu::paint_grid(bounds, &camera, &self.settings.grid, window);
            }
            let viewport = Rect::new(
                origin.x,
                origin.y,
                f64::from(f32::from(bounds.size.width)),
                f64::from(f32::from(bounds.size.height)),
            );
            let (mut contents, changed) = self.apply_terminal_modes(viewport, window, cx);
            let modal_open = self.modal_open;
            if let Ok(browsers) = this.update(cx, |view, _| {
                view.apply_browser_modes(viewport, modal_open, window)
            }) {
                contents.extend(browsers);
            }
            modes_changed = changed;
            plugin_modes = self.plugin_modes(viewport);
            for (id, mode, _) in &plugin_modes {
                let content = match mode {
                    PanelMode::Live => TerminalContent::Live,
                    PanelMode::Preview => TerminalContent::Preview(PreviewImage::None),
                    PanelMode::Hidden => continue,
                };
                contents.insert(*id, content);
            }
            preview_targets = self
                .input_targets
                .iter()
                .filter(|id| matches!(contents.get(id), Some(TerminalContent::Preview(_))))
                .count();
            previewing = contents
                .values()
                .any(|content| matches!(content, TerminalContent::Preview(_)));
            // Panels paint inside this mask; nested masks intersect with it, so nothing
            // a panel draws can leave the canvas.
            let z_of: HashMap<PanelId, usize> = self
                .scene
                .panels
                .iter()
                .enumerate()
                .map(|(index, panel)| (panel.id, index))
                .collect();
            for panel in &self.scene.panels {
                // A group's shadows all go down before its first member, so its members cover
                // the shadow between them and only the outer boundary casts one.
                let group = self
                    .joins
                    .groups
                    .iter()
                    .find(|members| members.contains(&panel.id));
                match group {
                    Some(members) if members[0] == panel.id => {
                        for id in members {
                            if let Some(member) = self.scene.get(*id) {
                                self.paint_shadow(member, origin, window);
                            }
                        }
                    }
                    Some(_) => {}
                    None => self.paint_shadow(panel, origin, window),
                }
                self.paint_panel(panel, contents.get(&panel.id), origin, window, cx);
                // A divider goes on right after the later of its two panels.
                let z = z_of[&panel.id];
                for join in &self.joins.joins {
                    if z_of.get(&join.first).max(z_of.get(&join.second)) == Some(&z) {
                        self.paint_divider(join, origin, window);
                    }
                }
            }
            if let Interaction::Drawing { draft, .. } = self.state {
                // Painted from the snapped world rect so the outline matches the panel created.
                let top_left = camera.world_to_screen(draft.origin);
                let bottom_right =
                    camera.world_to_screen(Point::new(draft.right(), draft.bottom()));
                let draft = Rect::new(
                    top_left.x,
                    top_left.y,
                    bottom_right.x - top_left.x,
                    bottom_right.y - top_left.y,
                );
                window.paint_quad(outline(
                    to_bounds(draft, origin),
                    rgb(theme::DRAFT_OUTLINE),
                    BorderStyle::Dashed,
                ));
            }
        });

        this.update(cx, |view, cx| {
            view.apply_plugin_modes(&plugin_modes, window);
            view.sync_native_order(cx);
            view.sync_status_bar(window);
            view.sync_preview_timer(previewing, cx);
            view.preview_input_targets += preview_targets;
            // Render picks input targets from the mode; rebuild them if paint disagreed.
            if modes_changed {
                cx.notify();
            }
        })
        .ok();

        // Moves and releases are tracked window-wide so a drag keeps going past the canvas edge.
        let weak = this.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble {
                weak.update(cx, |view, cx| view.mouse_move(event, window, cx))
                    .ok();
            }
        });
        window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
            if phase == DispatchPhase::Bubble {
                this.update(cx, |view, cx| view.mouse_up(event, window, cx))
                    .ok();
            }
        });
    }

    /// Mode and placement per plugin panel, by the same rules as terminals.
    fn plugin_modes(&self, viewport: Rect) -> Vec<(PanelId, PanelMode, Option<NativePlacement>)> {
        self.plugin_ids
            .iter()
            .filter_map(|id| self.scene.get(*id))
            .map(|panel| {
                let mode = terminal_mode(
                    panel,
                    &self.scene,
                    viewport,
                    self.modal_open,
                    self.settings.panels.preview_below_zoom,
                );
                (
                    panel.id,
                    mode,
                    terminal_placement(panel, &self.scene, viewport),
                )
            })
            .collect()
    }

    /// Decides each terminal's mode, applies it, and returns what to draw in its content
    /// rect, plus whether any mode changed. `viewport` is the canvas area in window points.
    fn apply_terminal_modes(
        &self,
        viewport: Rect,
        window: &mut Window,
        cx: &mut App,
    ) -> (HashMap<PanelId, TerminalContent>, bool) {
        let mut contents = HashMap::new();
        let mut changed = false;
        for (id, terminal) in &self.terminals {
            let Some(panel) = self.scene.get(*id) else {
                continue;
            };
            let mode = terminal_mode(
                panel,
                &self.scene,
                viewport,
                self.modal_open,
                self.settings.panels.preview_below_zoom,
            );
            let placement = terminal_placement(panel, &self.scene, viewport);
            let content = terminal.update(cx, |terminal, _| {
                changed |= terminal.mode() != mode;
                terminal.apply_mode(mode, placement, window);
                match mode {
                    PanelMode::Preview => Some(TerminalContent::Preview(terminal.preview())),
                    PanelMode::Live => Some(TerminalContent::Live),
                    PanelMode::Hidden => None,
                }
            });
            if let Some(content) = content {
                contents.insert(*id, content);
            }
        }
        (contents, changed)
    }

    /// Drop shadow outside the panel's outline, square where a neighbor joins.
    fn paint_shadow(&self, panel: &Panel, origin: gpui::Point<f64>, window: &mut Window) {
        let zoom = self.scene.camera.zoom;
        let body = self.scene.camera.world_rect_to_screen(body_rect(panel));
        let shadow_color = if self.focused == Some(panel.id) {
            theme::SHADOW_FOCUSED
        } else {
            theme::SHADOW
        };
        window.paint_drop_shadows(
            to_bounds(body, origin),
            panel_corners(panel, self.joins.edges(panel.id), zoom, 0.0),
            &[BoxShadow {
                color: rgba(shadow_color).into(),
                offset: point(px(0.), px((4.0 * zoom).clamp(1.0, 6.0) as f32)),
                blur_radius: px((18.0 * zoom).clamp(2.0, 24.0) as f32),
                spread_radius: px(0.),
                inset: false,
            }],
        );
    }

    /// Ring pieces along the parts of joined sides that no neighbor covers.
    fn paint_open_ring(
        &self,
        body: Rect,
        edges: &JoinedEdges,
        color: gpui::Rgba,
        origin: gpui::Point<f64>,
        window: &mut Window,
    ) {
        let camera = self.scene.camera;
        let zoom = camera.zoom;
        for side in [Side::Left, Side::Top, Side::Right, Side::Bottom] {
            // Thick enough to cover the ring's reach into the gap.
            let thickness = edges
                .gap(side)
                .map_or(BORDER_PX, |gap| BORDER_PX.max(gap * zoom / 2.0));
            for &(start, end) in edges.open(side) {
                let vertical = matches!(side, Side::Left | Side::Right);
                let (low, high) = if vertical {
                    (body.origin.y, body.bottom())
                } else {
                    (body.origin.x, body.right())
                };
                // Pieces that reach a corner extend over it to meet the neighbor side's ring.
                let lead = if start <= low { BORDER_PX } else { 0.0 };
                let tail = if end >= high { BORDER_PX } else { 0.0 };
                let piece = if vertical {
                    let top = camera.world_to_screen(Point::new(body.origin.x, start)).y - lead;
                    let bottom = camera.world_to_screen(Point::new(body.origin.x, end)).y + tail;
                    let x = if side == Side::Left {
                        camera.world_to_screen(body.origin).x - thickness
                    } else {
                        camera.world_to_screen(Point::new(body.right(), 0.0)).x
                    };
                    Rect::new(x, top, thickness, bottom - top)
                } else {
                    let left = camera.world_to_screen(Point::new(start, body.origin.y)).x - lead;
                    let right = camera.world_to_screen(Point::new(end, body.origin.y)).x + tail;
                    let y = if side == Side::Top {
                        camera.world_to_screen(body.origin).y - thickness
                    } else {
                        camera.world_to_screen(Point::new(0.0, body.bottom())).y
                    };
                    Rect::new(left, y, right - left, thickness)
                };
                window.paint_quad(fill(to_bounds(piece, origin), color));
            }
        }
    }

    /// Fills the gap of a join. It sits between the two panels, so it never covers a native
    /// view. Where both title bars face each other it is at least one pixel wide, so a
    /// joined pair still shows a seam through the bar strip when the gap is 0 or tiny.
    fn paint_divider(
        &self,
        join: &calm_core::join::Join,
        origin: gpui::Point<f64>,
        window: &mut Window,
    ) {
        let camera = self.scene.camera;
        let zoom = camera.zoom;
        let involved = |id: Option<PanelId>| id == Some(join.first) || id == Some(join.second);
        let color = if involved(self.confirming) {
            theme::CONFIRM_BORDER
        } else if involved(self.focused) {
            theme::ACCENT
        } else {
            theme::PANEL_BORDER
        };
        let gap = camera.world_rect_to_screen(join.divider);
        window.paint_quad(fill(to_bounds(gap, origin), rgb(color)));
        if join.axis != Axis::Row || join.gap * zoom >= BORDER_PX {
            return;
        }
        let (Some(first), Some(second)) = (self.scene.get(join.first), self.scene.get(join.second))
        else {
            return;
        };
        // The shared span starts at the lower of the two tops; both bars run to the higher
        // top plus the bar height.
        let bars_top = join.divider.origin.y;
        let bars_bottom = (first.rect.origin.y.min(second.rect.origin.y) + TITLE_BAR_HEIGHT)
            .min(join.divider.bottom());
        if bars_bottom <= bars_top {
            return;
        }
        let top = camera.world_to_screen(Point::new(0.0, bars_top)).y;
        let bottom = camera.world_to_screen(Point::new(0.0, bars_bottom)).y;
        let center = gap.origin.x + gap.size.width / 2.0;
        let seam = Rect::new(center - BORDER_PX / 2.0, top, BORDER_PX, bottom - top);
        window.paint_quad(fill(to_bounds(seam, origin), rgb(color)));
    }

    fn paint_panel(
        &self,
        panel: &Panel,
        content: Option<&TerminalContent>,
        origin: gpui::Point<f64>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let camera = self.scene.camera;
        let zoom = camera.zoom;
        let focused = self.focused == Some(panel.id);
        let confirming = self.confirming == Some(panel.id);
        let hovered_button = match self.hover {
            Some((id, button)) if id == panel.id => Some(button),
            _ => None,
        };
        let layout = title_bar_layout(panel);
        let (bar_color, border_color, text_color) = if confirming {
            (
                theme::CONFIRM_BAR,
                theme::CONFIRM_BORDER,
                theme::CONFIRM_TEXT,
            )
        } else if focused {
            (
                theme::PANEL_BAR_FOCUSED,
                theme::ACCENT,
                theme::TITLE_TEXT_FOCUSED,
            )
        } else {
            (theme::PANEL_BAR, theme::PANEL_BORDER, theme::TITLE_TEXT)
        };

        // The ring sits outside the panel rect, so it never reaches into native content.
        // On a joined side it has no border and reaches halfway across the gap, where the
        // divider takes over; the outer ring then runs unbroken across the group.
        let edges = self.joins.edges(panel.id);
        let body = body_rect(panel);
        let body_screen = camera.world_rect_to_screen(body);
        let outward = |side: Side| match edges.and_then(|edges| edges.gap(side)) {
            Some(gap) => gap * zoom / 2.0,
            None => BORDER_PX,
        };
        let border = |side: Side| match edges.and_then(|edges| edges.gap(side)) {
            Some(_) => px(0.),
            None => px(BORDER_PX as f32),
        };
        let ring = Rect::new(
            body_screen.origin.x - outward(Side::Left),
            body_screen.origin.y - outward(Side::Top),
            body_screen.size.width + outward(Side::Left) + outward(Side::Right),
            body_screen.size.height + outward(Side::Top) + outward(Side::Bottom),
        );
        window.paint_quad(quad(
            to_bounds(ring, origin),
            panel_corners(panel, edges, zoom, BORDER_PX),
            rgb(bar_color),
            Edges {
                top: border(Side::Top),
                right: border(Side::Right),
                bottom: border(Side::Bottom),
                left: border(Side::Left),
            },
            rgb(border_color),
            BorderStyle::Solid,
        ));
        if let Some(edges) = edges {
            self.paint_open_ring(body, edges, rgb(border_color), origin, window);
        }

        if !panel.minimized {
            let content_screen = camera.world_rect_to_screen(content_rect(panel));
            let content_bounds = to_bounds(content_screen, origin);
            let divider = Rect::new(
                content_screen.origin.x,
                content_screen.origin.y - BORDER_PX,
                content_screen.size.width,
                BORDER_PX,
            );
            window.paint_quad(fill(to_bounds(divider, origin), rgb(theme::PANEL_DIVIDER)));
            window.paint_quad(fill(content_bounds, rgb(theme::CONTENT_PLACEHOLDER)));
            match content {
                Some(TerminalContent::Preview(
                    PreviewImage::Fresh(image) | PreviewImage::Stale(image),
                )) => {
                    let painted = window.paint_image(
                        content_bounds,
                        content_bounds,
                        Default::default(),
                        image.clone(),
                        0,
                        false,
                    );
                    if let Err(err) = painted {
                        eprintln!("calm: preview paint failed: {err:#}");
                    }
                }
                Some(TerminalContent::Preview(PreviewImage::None)) => {
                    // No snapshot has succeeded yet: flat rect with the title.
                    let padding = 8.0 * zoom;
                    let line = Rect::new(
                        content_screen.origin.x + padding,
                        content_screen.origin.y + padding,
                        (content_screen.size.width - 2.0 * padding).max(0.0),
                        TITLE_BAR_HEIGHT * zoom,
                    );
                    paint_text(
                        &panel.title,
                        line,
                        TextLook {
                            align: TextAlign::Left,
                            color: theme::TITLE_TEXT,
                        },
                        zoom,
                        origin,
                        window,
                        cx,
                    );
                }
                Some(TerminalContent::Live) | None => {}
            }
        }

        if !details_visible(zoom) {
            return;
        }
        let icon_color = match PanelIcon::for_kind(&panel.kind) {
            PanelIcon::Terminal => theme::ICON_TERMINAL,
            PanelIcon::Browser => theme::ICON_BROWSER,
            PanelIcon::Plugin => theme::ICON_PLUGIN,
        };
        let stroke = (1.5 * zoom).max(1.0);
        let icon_rect = camera.world_rect_to_screen(layout.icon);
        if let Some((_, glyph)) = self.glyphs.iter().find(|(id, _)| *id == panel.id) {
            paint_text(
                glyph,
                icon_rect,
                TextLook {
                    align: TextAlign::Center,
                    color: icon_color,
                },
                zoom,
                origin,
                window,
                cx,
            );
        } else {
            paint_strokes(
                &icon_strokes(PanelIcon::for_kind(&panel.kind)),
                icon_rect,
                stroke,
                icon_color,
                origin,
                window,
            );
        }

        // Buttons show while the pointer is over the panel, and during a close confirmation.
        if hovered_button.is_some() || confirming {
            let buttons = [
                (ChromeButton::Minimize, layout.minimize),
                (ChromeButton::Close, layout.close),
            ];
            for (button, hit) in buttons {
                let Some(hit) = hit else { continue };
                let dot = camera.world_rect_to_screen(button_dot(hit));
                let lit = hovered_button == Some(Some(button))
                    || (confirming && button == ChromeButton::Close);
                let (dot_color, glyph_color) = match (lit, button) {
                    (true, ChromeButton::Close) => {
                        (theme::BUTTON_CLOSE_HOVER, theme::BUTTON_GLYPH_ON_COLOR)
                    }
                    (true, ChromeButton::Minimize) => {
                        (theme::BUTTON_MINIMIZE_HOVER, theme::BUTTON_GLYPH_ON_COLOR)
                    }
                    (false, _) => (theme::BUTTON_IDLE, theme::BUTTON_GLYPH),
                };
                window.paint_quad(
                    fill(to_bounds(dot, origin), rgb(dot_color))
                        .corner_radii(Corners::all(px((dot.size.width / 2.0) as f32))),
                );
                paint_strokes(
                    &button_strokes(button),
                    dot,
                    stroke,
                    glyph_color,
                    origin,
                    window,
                );
            }
        }

        let mut middle = camera.world_rect_to_screen(layout.middle);
        let stale = !panel.minimized
            && matches!(content, Some(TerminalContent::Preview(image)) if image.is_stale());
        if stale {
            let width = (STALE_LABEL_WIDTH * zoom).min(middle.size.width);
            let label = Rect::new(
                middle.right() - width,
                middle.origin.y,
                width,
                middle.size.height,
            );
            paint_text(
                STALE_LABEL,
                label,
                TextLook {
                    align: TextAlign::Right,
                    color: theme::STALE_TEXT,
                },
                zoom,
                origin,
                window,
                cx,
            );
            middle.size.width = (middle.size.width - width - 8.0 * zoom).max(0.0);
        }
        let title = if confirming {
            CONFIRM_TITLE
        } else {
            panel.title.as_str()
        };
        paint_text(
            title,
            middle,
            TextLook {
                align: TextAlign::Left,
                color: text_color,
            },
            zoom,
            origin,
            window,
            cx,
        );
    }
}

/// What a panel's chrome outlines: the chip when minimized, else the whole panel.
fn body_rect(panel: &Panel) -> Rect {
    if panel.minimized {
        chip_rect(panel)
    } else {
        panel.rect
    }
}

/// Screen radii of a panel's outline grown by `grow` pixels. Bottom corners stay square on a
/// full panel because the native view below is square; corners a neighbor covers are square.
fn panel_corners(
    panel: &Panel,
    edges: Option<&JoinedEdges>,
    zoom: f64,
    grow: f64,
) -> Corners<Pixels> {
    let radius = CORNER_RADIUS * zoom + grow;
    let bottom = if panel.minimized { radius } else { 0.0 };
    let square = edges.map(|edges| edges.square).unwrap_or_default();
    let pick = |is_square: bool, round: f64| px(if is_square { 0.0 } else { round } as f32);
    Corners {
        top_left: pick(square.top_left, radius),
        top_right: pick(square.top_right, radius),
        bottom_right: pick(square.bottom_right, bottom),
        bottom_left: pick(square.bottom_left, bottom),
    }
}

/// Strokes unit-square polylines (see `calm_core::chrome::icon_strokes`) into `area`.
fn paint_strokes(
    strokes: &[Vec<(f64, f64)>],
    area: Rect,
    width: f64,
    color: u32,
    origin: gpui::Point<f64>,
    window: &mut Window,
) {
    for polyline in strokes {
        let mut builder = PathBuilder::stroke(px(width as f32));
        for (index, (x, y)) in polyline.iter().enumerate() {
            let at = point(
                px((origin.x + area.origin.x + x * area.size.width) as f32),
                px((origin.y + area.origin.y + y * area.size.height) as f32),
            );
            if index == 0 {
                builder.move_to(at);
            } else {
                builder.line_to(at);
            }
        }
        match builder.build() {
            Ok(path) => window.paint_path(path, rgb(color)),
            Err(err) => eprintln!("calm: chrome stroke failed: {err:?}"),
        }
    }
}

/// Alignment and color of one `paint_text` line.
struct TextLook {
    align: TextAlign,
    color: u32,
}

/// One line of text, aligned and clipped to `rect` (canvas pixels). Skipped when too small.
fn paint_text(
    text: &str,
    rect: Rect,
    look: TextLook,
    zoom: f64,
    origin: gpui::Point<f64>,
    window: &mut Window,
    cx: &mut App,
) {
    let font_px = TITLE_FONT_SIZE * zoom;
    if font_px < MIN_TITLE_FONT_PX || text.is_empty() {
        return;
    }
    let title: SharedString = text.to_string().into();
    let run = TextRun {
        len: title.len(),
        font: window.text_style().font(),
        color: rgb(look.color).into(),
        background_color: None,
        underline: None,
        strikethrough: None,
    };
    let line = window
        .text_system()
        .shape_line(title, px(font_px as f32), &[run], None);
    let text_bounds = to_bounds(rect, origin);
    window.with_content_mask(
        Some(ContentMask {
            bounds: text_bounds,
        }),
        |window| {
            if let Err(err) = line.paint(
                text_bounds.origin,
                px(rect.size.height as f32),
                look.align,
                Some(text_bounds.size.width),
                window,
                cx,
            ) {
                eprintln!("calm: title paint failed: {err:#}");
            }
        },
    );
}

impl Focusable for CanvasView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl Render for CanvasView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if self.show_fps {
            // Each render records a frame and asks for the next one, so the readout
            // measures the frame rate the window can sustain. Off means no extra frames.
            self.frame_stats.record(Instant::now());
            window.request_animation_frame();
        }
        self.sync_background(window, cx);
        let canvas_size = Size::new(self.viewport.size.width, self.viewport.size.height);
        let background = self
            .background
            .texture(&self.settings.settings.background, canvas_size);
        let entity = cx.entity().downgrade();
        let paint_entity = entity.clone();
        let mut snapshot = PaintSnapshot {
            scene: self.scene.clone(),
            state: self.input.state,
            focused: self.focused,
            confirming: self.input.confirming,
            hover: self.hover,
            terminals: self.terminals.clone(),
            plugin_ids: self.plugin_panels.iter().map(|(id, _)| *id).collect(),
            glyphs: self
                .tui_runs
                .iter()
                .filter_map(|(id, run)| run.glyph().map(|glyph| (*id, glyph.to_string())))
                .collect(),
            modal_open: self.input.confirming.is_some() || self.menu_open,
            input_targets: Vec::new(),
            settings: self.settings.settings.clone(),
            background,
            joins: JoinLayout::compute(&self.scene.panels),
        };
        self.join_counts = (
            snapshot.joins.joins.len(),
            snapshot.joins.visible_dividers(),
        );
        // Mouse and key targets for terminals, at last frame's viewport.
        let viewport = self.viewport;
        // Only live terminals take input; a preview's clicks and scrolls go to the canvas.
        // The mode comes from the current scene, not last paint, so a target never
        // outlives the switch to `Preview`.
        let placements: Vec<Option<NativePlacement>> = self
            .terminals
            .iter()
            .map(|(id, _)| {
                self.scene
                    .get(*id)
                    .filter(|panel| {
                        terminal_mode(
                            panel,
                            &self.scene,
                            viewport,
                            snapshot.modal_open,
                            snapshot.settings.panels.preview_below_zoom,
                        ) == PanelMode::Live
                    })
                    .and_then(|panel| terminal_placement(panel, &self.scene, viewport))
            })
            .collect();
        snapshot.input_targets = self
            .terminals
            .iter()
            .zip(&placements)
            .filter(|(_, placement)| placement.is_some())
            .map(|((id, _), _)| *id)
            .collect();
        let terminal_targets =
            self.terminals
                .iter()
                .zip(placements)
                .map(|((_, terminal), placement)| {
                    let rect = placement.map_or(Rect::new(0.0, 0.0, 0.0, 0.0), |p| p.container);
                    div()
                        .absolute()
                        .left(px((rect.origin.x - viewport.origin.x) as f32))
                        .top(px((rect.origin.y - viewport.origin.y) as f32))
                        .w(px(rect.size.width as f32))
                        .h(px(rect.size.height as f32))
                        .child(terminal.clone())
                });
        div()
            .size_full()
            .flex()
            .flex_col()
            .bg(rgb(self.settings.settings.background.base_color().0))
            .key_context(KEY_CONTEXT)
            .track_focus(&self.focus_handle)
            .on_key_down(cx.listener(Self::key_down))
            .on_key_up(cx.listener(Self::key_up))
            .on_pinch(cx.listener(Self::pinch))
            .on_action(
                cx.listener(|view, _: &ZoomIn, window, cx| view.key_zoom(ZoomStep::In, window, cx)),
            )
            .on_action(
                cx.listener(|view, _: &ZoomOut, window, cx| {
                    view.key_zoom(ZoomStep::Out, window, cx)
                }),
            )
            .on_action(cx.listener(|view, _: &ZoomReset, window, cx| {
                view.key_zoom(ZoomStep::Reset, window, cx)
            }))
            .on_action(cx.listener(|view, _: &CenterOnOrigin, window, cx| {
                view.frame_view(false, window, cx)
            }))
            .on_action(
                cx.listener(|view, _: &FitAll, window, cx| view.frame_view(true, window, cx)),
            )
            .on_action(cx.listener(|view, _: &DrawTerminal, window, cx| {
                view.set_tool(Tool::DrawTerminal, window, cx)
            }))
            .on_action(cx.listener(|view, _: &DrawBrowser, window, cx| {
                view.set_tool(Tool::DrawBrowser, window, cx)
            }))
            .on_action(cx.listener(|view, _: &Cancel, window, cx| view.cancel(window, cx)))
            .on_action(
                cx.listener(|view, _: &ConfirmEnter, window, cx| view.confirm_enter(window, cx)),
            )
            .on_action(|_: &ToggleFullscreen, window, _| window.toggle_fullscreen())
            .on_action(cx.listener(|view, _: &ToggleFps, window, cx| {
                view.set_show_fps(!view.show_fps, window, cx)
            }))
            .on_action(cx.listener(|view, _: &ResetFps, window, cx| view.reset_fps(window, cx)))
            .map(|root| settings_menu::bind(root, cx))
            .child(
                div()
                    .flex_1()
                    .w_full()
                    .relative()
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::mouse_down))
                    .on_mouse_down(MouseButton::Middle, cx.listener(Self::mouse_down))
                    .on_mouse_down(MouseButton::Right, cx.listener(Self::mouse_down))
                    .on_scroll_wheel(cx.listener(Self::scroll))
                    .child(
                        canvas(
                            move |bounds, _window, cx| {
                                let viewport = Rect::new(
                                    f64::from(f32::from(bounds.origin.x)),
                                    f64::from(f32::from(bounds.origin.y)),
                                    f64::from(f32::from(bounds.size.width)),
                                    f64::from(f32::from(bounds.size.height)),
                                );
                                entity.update(cx, |view, _| view.viewport = viewport).ok();
                            },
                            move |bounds, (), window, cx| {
                                snapshot.paint_canvas(bounds, paint_entity, window, cx);
                            },
                        )
                        .size_full(),
                    )
                    .children(terminal_targets),
            )
    }
}

fn to_mods(mods: &gpui::Modifiers) -> Modifiers {
    Modifiers {
        shift: mods.shift,
        control: mods.control,
        alt: mods.alt,
        platform: mods.platform,
    }
}

fn to_bounds(rect: Rect, origin: gpui::Point<f64>) -> Bounds<Pixels> {
    Bounds {
        origin: point(
            px((rect.origin.x + origin.x) as f32),
            px((rect.origin.y + origin.y) as f32),
        ),
        size: size(px(rect.size.width as f32), px(rect.size.height as f32)),
    }
}
