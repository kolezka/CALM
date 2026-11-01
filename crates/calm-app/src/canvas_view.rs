//! The canvas: paints panels from the `calm-core` scene and feeds it input.

use calm_core::autosave::{
    APP_AUTOSAVE_ACTIVE, AutosaveChange, AutosaveClock, OrderedWriter, SaveQueue, WriteOutcome,
    autosave_active, autosave_allowed_after_read, autosave_change, save_error_text,
};
use calm_core::camera::FIT_MARGIN;
use calm_core::chrome::{
    CORNER_RADIUS, ChromeButton, ChromeOverrides, ChromePalette, ChromeState, PanelIcon,
    TITLE_BAR_HEIGHT, button_dot, button_strokes, chrome_colors, details_visible, divider_color,
    hover_target, icon_strokes, icon_tint, title_bar_layout,
};
use calm_core::frame_stats::FrameStats;
use calm_core::geometry::{Point, Rect, Size};
use calm_core::interaction::{
    Button, ConfirmKey, Effect, InputHandler, Interaction, Modifiers, ResizeEdge, Tool, ZoomStep,
};
use calm_core::join::{Axis, JoinLayout, JoinedEdges, Side};
use calm_core::layout::{LayoutFile, SessionWriter, load_layout_from};
use calm_core::menu::{self, MenuCommand, MenuItem};
use calm_core::preview::{PREVIEW_REFRESH_SECS, PanelMode, PreviewImage, decide_mode};
use calm_core::process_stats::{clamp_interval_ms, cpu_text, memory_text, stats_items};
use calm_core::resize::ResizePolicy;
use calm_core::restore::{
    AutosaveStart, CWD_MISSING_NOTE, RestorePlan, placeholder_reason, restored_cwd, version_note,
};
use calm_core::scene::{Panel, PanelId, PanelKind, Scene, chip_rect, content_rect};
use calm_core::status_bar::{status_bar_rect, status_text};
use calm_core::terminal_profile::{ResolvedTerminal, TerminalIcon, display_cwd, render_title};
use calm_core::viewport::{NativePlacement, place_native};
use calm_native::{
    BrowserEvent, HostView, NativeChild, ProcessSample, ProcessSampler, StatusBar,
    show_context_menu,
};
use gpui::prelude::FluentBuilder as _;
use gpui::{
    App, BorderStyle, Bounds, BoxShadow, ContentMask, Context, Corners, CursorStyle, DispatchPhase,
    Edges, Entity, FocusHandle, Focusable, InteractiveElement, IntoElement, KeyDownEvent,
    KeyUpEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, NoAction, ParentElement,
    PathBuilder, PinchEvent, Pixels, Render, RenderImage, ScrollDelta, ScrollWheelEvent,
    SharedString, Styled, Subscription, Task, TextAlign, TextRun, WeakEntity, Window, actions,
    canvas, div, fill, outline, point, px, quad, rgb, rgba, size,
};

use calm_plugins::{BridgeRequest, InstalledPlugin, PluginContent};
use futures::StreamExt as _;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use crate::background::{self, BackgroundRenderer, BackgroundTexture};
use crate::browser_panel::{BrowserEvents, BrowserPanel};
use crate::dev_menu::{self, ResetFps, ToggleFps};
use crate::grid_paint::{self, GridFrame, GridTextures};
use crate::plugin_host::{self, BridgeSender, PluginPanel, Route};
use crate::plugin_services::{self, ServiceCall};
use crate::settings_menu::{self, SettingsState};
use crate::terminal::{self, TerminalEvent, TerminalPanel};
use crate::terminal_profiles::{self, TerminalMeta};
use crate::theme;
use crate::tui_host::{self, TuiRun};

mod plugin_menus;

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

/// Autosave state for one launch. Present only once autosave has started.
struct Autosave {
    started: Instant,
    clock: AutosaveClock,
    queue: SaveQueue<LayoutFile>,
    /// Shared with the background writes and the quit path; it puts writes in order.
    writer: Arc<Mutex<OrderedWriter<SessionWriter>>>,
    timer: Option<(Duration, Task<()>)>,
    _quit: Subscription,
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
    /// Plugin dirs searched at startup, shown in the Add menu when none was found.
    plugin_search_dirs: Vec<String>,
    /// (built-in, user) plugins found at startup. The smoke prints it.
    pub plugin_counts: (usize, usize),
    /// Plugin folders skipped at startup, shown in the status bar; stderr names each one.
    pub plugin_errors: usize,
    /// Plugins loaded with a warning (unknown capability words); stderr names each one.
    pub plugin_warnings: usize,
    /// Live plugin pages by panel. Dropping an entry closes the page.
    plugin_panels: Vec<(PanelId, PluginPanel)>,
    bridge: BridgeSender,
    /// Feeds bridge messages from plugin pages into `plugin_message`.
    _bridge_task: Task<()>,
    /// What background bridge calls (`gh`, storage, `open`) run against. The smoke swaps it.
    pub plugin_services: Arc<plugin_services::Services>,
    /// Path of the last saved layout, shown in the status bar. Task 13 sets it.
    pub saved_flash: Option<String>,
    focused: Option<PanelId>,
    /// Panel under the pointer and the chrome button under it. Shows the buttons.
    hover: Option<(PanelId, Option<ChromeButton>)>,
    /// Resize edge under the pointer, or of the resize under way. Sets the cursor.
    resize_cursor: Option<ResizeEdge>,
    /// Live terminals by panel. Dropping an entry ends its process.
    terminals: Vec<(PanelId, Entity<TerminalPanel>)>,
    /// Restart state of TUI panels. Their terminal is in `terminals`.
    tui_runs: Vec<(PanelId, TuiRun)>,
    /// Plain terminals opened from the `terminal` settings. Their terminal is in `terminals`.
    terminal_meta: Vec<(PanelId, TerminalMeta)>,
    /// Profile asked for by a menu item or a duplicate, read when the panel is created.
    requested_profiles: HashMap<PanelId, String>,
    /// Plain terminals opened so far, for `{n}`.
    terminals_opened: u64,
    /// Decoded TUI image icons by absolute path, `None` when decoding failed. Decoded once,
    /// never per frame.
    tui_icons: HashMap<std::path::PathBuf, Option<Arc<RenderImage>>>,
    /// Live browser panels. Dropping an entry closes its page.
    browsers: Vec<(PanelId, BrowserPanel)>,
    /// Sender for page events; the receiving task starts with the first browser.
    browser_events: Option<BrowserEvents>,
    browser_listener: Option<Task<()>>,
    /// Back-to-front order of native panels (terminals, browsers and plugins) as last stacked.
    native_order: Vec<PanelId>,
    focus_handle: FocusHandle,
    /// The stats readout (cmd-shift-f). Which items it shows is `developer.stats`.
    pub show_stats: bool,
    pub frame_stats: FrameStats,
    /// Latest CPU and memory reading, from `stats_timer`.
    pub process_stats: Option<ProcessSample>,
    /// Successful samples since the timer last started. The smoke waits on it.
    pub process_samples: u64,
    /// Samples CPU and memory off the main thread while the readout needs them.
    stats_timer: Option<Task<()>>,
    /// (interval ms, include children) the running timer was started with.
    stats_timer_config: Option<(u64, bool)>,
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
    pub grid: GridTextures,
    /// Native bar floating over the bottom of the canvas. Created on the first paint.
    status_bar: Option<StatusBar>,
    /// Set when the status bar could not be created, so paint stops retrying.
    status_bar_error: Option<String>,
    /// Joins and visible dividers in the last render. Read by the smoke.
    pub join_counts: (usize, usize),
    /// `run` args of the plugin menu item opening a TUI panel right now.
    pending_tui_args: Option<Vec<String>>,
    autosave: Option<Autosave>,
    /// Why autosave could not start this launch, shown in the status pill.
    autosave_notice: Option<String>,
    /// Session files written this launch.
    pub autosave_writes: u64,
    /// The build gate for starting autosave from settings. `APP_AUTOSAVE_ACTIVE`, except
    /// while the smoke drives the live toggle in its own dir.
    pub autosave_gate: bool,
    /// Where the last autosave wrote, so turning it back on resumes the same file.
    autosave_path: Option<PathBuf>,
    /// Restored panels that cannot run, with the line they show. GPUI-only: no native
    /// view, and their saved kind is kept unchanged for the next save.
    placeholders: HashMap<PanelId, String>,
    /// A short note drawn after a panel's title in its chrome, never saved with it.
    title_notes: HashMap<PanelId, &'static str>,
    /// What the launch restore reported, shown in the status pill.
    restore_notice: Option<String>,
    /// The named layout this canvas came from, kept for a later save.
    pub layout_name: Option<String>,
}

impl CanvasView {
    /// `builtin_plugins` is the `plugins.builtin` setting, read once here.
    pub fn new(builtin_plugins: bool, cx: &mut Context<Self>) -> Self {
        let (discovery, plugin_search_dirs) = plugin_host::discover_installed(builtin_plugins);
        let plugin_counts = (discovery.builtin_count, discovery.user_count());
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
            plugin_search_dirs,
            plugin_counts,
            plugin_errors: discovery.errors.len(),
            plugin_warnings: discovery.warnings.len(),
            plugin_panels: Vec::new(),
            bridge,
            _bridge_task: bridge_task,
            plugin_services: Arc::new(plugin_services::Services::default()),
            saved_flash: None,
            focused: None,
            hover: None,
            resize_cursor: None,
            terminals: Vec::new(),
            tui_runs: Vec::new(),
            terminal_meta: Vec::new(),
            requested_profiles: HashMap::new(),
            terminals_opened: 0,
            tui_icons: HashMap::new(),
            browsers: Vec::new(),
            browser_events: None,
            browser_listener: None,
            native_order: Vec::new(),
            focus_handle: cx.focus_handle(),
            show_stats: false,
            frame_stats: FrameStats::default(),
            process_stats: None,
            process_samples: 0,
            stats_timer: None,
            stats_timer_config: None,
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
            grid: GridTextures::default(),
            status_bar: None,
            status_bar_error: None,
            join_counts: (0, 0),
            pending_tui_args: None,
            autosave: None,
            autosave_notice: None,
            autosave_writes: 0,
            autosave_gate: APP_AUTOSAVE_ACTIVE,
            autosave_path: None,
            placeholders: HashMap::new(),
            title_notes: HashMap::new(),
            restore_notice: None,
            layout_name: None,
        }
    }

    /// Carries out a launch plan whose quarantine is done: builds its panels, then
    /// starts autosave to `session` on the result, so an untouched start never writes.
    pub fn apply_restore_plan(
        &mut self,
        plan: RestorePlan,
        session: PathBuf,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let mut notices = plan.notices;
        if let Some(layout) = plan.layout {
            notices.extend(self.restore_layout(layout, window, cx));
        }
        self.layout_name = plan.layout_name;
        for notice in &notices {
            eprintln!("calm: {notice}");
        }
        self.restore_notice = match notices.len() {
            0 => None,
            1 => notices.pop(),
            n => Some(format!("{} (+{} more on stderr)", notices[0], n - 1)),
        };
        self.autosave_gate = plan.autosave.launch_gate(self.autosave_gate);
        match plan.autosave {
            AutosaveStart::On => {
                if autosave_active(self.autosave_gate, self.settings.settings.session.autosave)
                    && let Err(err) = self.start_autosave(session, cx)
                {
                    eprintln!("calm: {err}");
                }
            }
            AutosaveStart::Off(reason) => self.autosave_notice = Some(reason),
        }
        window.refresh();
        cx.notify();
    }

    fn plugin_installed(&self, plugin_id: &str, tui: bool) -> bool {
        self.installed_plugins.iter().any(|plugin| {
            plugin.manifest.id == plugin_id
                && matches!(plugin.manifest.content, PluginContent::Tui(_)) == tui
        })
    }

    /// Creates `layout`'s panels back to front with fresh ids, through the same open paths
    /// as new panels (so current resize limits apply), then sets the camera. Returns the
    /// notices for the pill.
    fn restore_layout(
        &mut self,
        layout: LayoutFile,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<String> {
        let mut notices = Vec::new();
        for saved in layout.panels {
            let id = self.scene.add(saved.kind.clone(), saved.rect);
            let saved_title = saved.title.clone();
            if let Some(panel) = self.scene.get_mut(id) {
                panel.title = saved.title;
                panel.minimized = saved.minimized;
                panel.overrides = saved.overrides;
            }
            let reason = placeholder_reason(
                &saved.kind,
                |plugin_id| self.plugin_installed(plugin_id, false),
                |plugin_id| self.plugin_installed(plugin_id, true),
            );
            if let Some(reason) = reason {
                notices.push(reason.clone());
                self.placeholders.insert(id, reason);
                continue;
            }
            match &saved.kind {
                PanelKind::Terminal { command, cwd } => {
                    let home = terminal_profiles::home_dir();
                    let (cwd, notice) =
                        restored_cwd(cwd.as_deref(), std::path::Path::is_dir, &home);
                    if notice.is_some() {
                        self.title_notes.insert(id, CWD_MISSING_NOTE);
                    }
                    notices.extend(notice);
                    let opened =
                        self.open_plain_terminal(id, None, command.as_deref(), cwd, window, cx);
                    if let Err(err) = opened {
                        notices.push(format!("a restored terminal failed to start: {err}"));
                    }
                }
                PanelKind::Browser { url, .. } => {
                    let url = calm_core::browser_url::restored_url(url).unwrap_or_else(|notice| {
                        notices.push(notice);
                        calm_core::browser_url::RESTORE_FALLBACK_URL.to_string()
                    });
                    if let Err(err) = self.spawn_browser(id, &url, window, cx) {
                        notices.push(format!("a restored browser failed to start: {err}"));
                    }
                }
                kind => {
                    if let PanelKind::Plugin {
                        plugin_id,
                        plugin_version,
                        ..
                    } = kind
                        && let Some(note) = version_note(
                            plugin_id,
                            plugin_version.as_deref(),
                            self.installed_version(plugin_id).as_deref(),
                        )
                    {
                        eprintln!("calm: {note}");
                    }
                    self.panel_created(id, kind, window, cx);
                }
            }
            // The open paths set a default title; the saved one stays until the panel sets
            // its own. A panel that failed to open keeps its failure title.
            if self.panel_running(id) && !saved_title.is_empty() {
                self.scene.set_title(id, saved_title);
            }
        }
        // Last, so placement runs once with the saved zoom.
        self.scene.camera = layout.camera;
        notices
    }

    /// Whether `id` has a live terminal, TUI app, browser or plugin view.
    fn panel_running(&self, id: PanelId) -> bool {
        self.terminal(id).is_some()
            || self.tui_run(id).is_some()
            || self.browser(id).is_some()
            || self.plugin_panels.iter().any(|(panel, _)| *panel == id)
    }

    /// The note drawn after `id`'s title, if it has one.
    pub fn title_note(&self, id: PanelId) -> Option<&'static str> {
        self.title_notes.get(&id).copied()
    }

    /// The line a placeholder panel shows, if `id` is one.
    pub fn placeholder(&self, id: PanelId) -> Option<&str> {
        self.placeholders.get(&id).map(String::as_str)
    }

    /// Starts writing the scene to `path`. The scene as it is now counts as saved, so an
    /// untouched canvas never replaces the file. A file that cannot be read or parsed
    /// turns autosave off for this launch.
    pub fn start_autosave(&mut self, path: PathBuf, cx: &mut Context<Self>) -> Result<(), String> {
        let dir = path.parent().map(PathBuf::from).unwrap_or_default();
        let name = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default()
            .to_string();
        if let Err(err) = autosave_allowed_after_read(&load_layout_from(&dir, &name)) {
            self.autosave_notice = Some(err.clone());
            cx.notify();
            return Err(err);
        }
        let session = &self.settings.settings.session;
        let writer = SessionWriter::new(path.clone(), session.backup_count, session.browser_query);
        let debounce = Duration::from_millis(session.debounce_ms);
        self.autosave = Some(Autosave {
            started: Instant::now(),
            clock: AutosaveClock::new(debounce, self.scene.sync_revision()),
            queue: SaveQueue::default(),
            writer: Arc::new(Mutex::new(OrderedWriter::new(writer))),
            timer: None,
            _quit: cx.on_app_quit(|view, _| {
                view.save_on_quit();
                async {}
            }),
        });
        self.autosave_notice = None;
        self.autosave_path = Some(path);
        Ok(())
    }

    /// Whether autosave is writing this launch.
    pub fn autosave_running(&self) -> bool {
        self.autosave.is_some()
    }

    /// Ends autosave without a last write. The smoke uses it before removing its dir.
    pub fn stop_autosave(&mut self) {
        self.autosave = None;
    }

    /// Applies a change to the session settings: off stops the timer and the quit write, on
    /// starts through the build gate, and a running save takes the new debounce on its next
    /// tick, the browser query mode on its next write and the backup count at rotation.
    pub fn reconcile_autosave(
        &mut self,
        old: &calm_core::settings::SessionSettings,
        cx: &mut Context<Self>,
    ) {
        let new = &self.settings.settings.session;
        match autosave_change(self.autosave_gate, self.autosave.is_some(), old, new) {
            AutosaveChange::Keep => {}
            AutosaveChange::Stop => self.stop_autosave(),
            AutosaveChange::Start => {
                let path = self
                    .autosave_path
                    .clone()
                    .unwrap_or_else(calm_core::layout::session_path);
                if let Err(err) = self.start_autosave(path, cx) {
                    eprintln!("calm: {err}");
                }
            }
            AutosaveChange::Update => {
                let debounce = Duration::from_millis(new.debounce_ms);
                let (backup_count, browser_query) = (new.backup_count, new.browser_query);
                if let Some(autosave) = self.autosave.as_mut() {
                    autosave.clock.set_debounce(debounce);
                    autosave
                        .writer
                        .lock()
                        .unwrap_or_else(PoisonError::into_inner)
                        .inner_mut()
                        .set_options(backup_count, browser_query);
                }
                self.autosave_tick(cx);
            }
        }
    }

    /// Checks the scene revision, starts a save when one is due and sets the timer for the
    /// next one. Runs on every render and from its own timer.
    pub fn autosave_tick(&mut self, cx: &mut Context<Self>) {
        let revision = self.scene.sync_revision();
        let Some(autosave) = self.autosave.as_mut() else {
            return;
        };
        let now = autosave.started.elapsed();
        autosave.clock.observe(revision, now);
        if let Some(due) = autosave.clock.due(now) {
            autosave.clock.submitted(due);
            if let Some(job) = autosave.queue.submit(due, self.scene.to_layout()) {
                self.spawn_autosave_write(job, cx);
            }
        }
        self.schedule_autosave_timer(cx);
    }

    fn schedule_autosave_timer(&mut self, cx: &mut Context<Self>) {
        let Some(autosave) = self.autosave.as_mut() else {
            return;
        };
        let Some(deadline) = autosave.clock.next_deadline() else {
            autosave.timer = None;
            return;
        };
        if autosave
            .timer
            .as_ref()
            .is_some_and(|(at, _)| *at == deadline)
        {
            return;
        }
        let delay = deadline.saturating_sub(autosave.started.elapsed());
        let task = cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let _ = this.update(cx, |view, cx| {
                if let Some(autosave) = view.autosave.as_mut() {
                    autosave.timer = None;
                }
                view.autosave_tick(cx);
            });
        });
        autosave.timer = Some((deadline, task));
    }

    /// Writes one snapshot on the background executor, then acknowledges it on the UI thread.
    fn spawn_autosave_write(
        &mut self,
        (generation, layout): (u64, LayoutFile),
        cx: &mut Context<Self>,
    ) {
        let Some(autosave) = self.autosave.as_ref() else {
            return;
        };
        let writer = autosave.writer.clone();
        cx.spawn(async move |this, cx| {
            let outcome = cx
                .background_executor()
                .spawn(async move { write_session(&writer, generation, &layout) })
                .await;
            let _ = this.update(cx, |view, cx| view.autosave_finished(outcome, cx));
        })
        .detach();
    }

    fn autosave_finished(&mut self, outcome: WriteOutcome, cx: &mut Context<Self>) {
        self.note_autosave_outcome(&outcome);
        let Some(autosave) = self.autosave.as_mut() else {
            return;
        };
        if let Some(next) = autosave.queue.finish(&outcome) {
            self.spawn_autosave_write(next, cx);
        }
        cx.notify();
    }

    fn note_autosave_outcome(&mut self, outcome: &WriteOutcome) {
        match outcome {
            WriteOutcome::Written(_) => self.autosave_writes += 1,
            WriteOutcome::Skipped { .. } => {}
            WriteOutcome::Failed { message, .. } => {
                eprintln!("calm: {}", save_error_text(message));
            }
        }
    }

    /// The quit hook: writes the latest scene now. Waits for a write in flight, and the
    /// ordered writer drops that older write if it runs after this one.
    pub fn save_on_quit(&mut self) {
        let revision = self.scene.sync_revision();
        let Some(autosave) = self.autosave.as_mut() else {
            return;
        };
        let queued = autosave.queue.take_queued().is_some();
        // The live revision, not the clock's: a change since the last tick must be saved.
        let pending =
            autosave.clock.changed_since_submit(revision) || autosave.queue.in_flight().is_some();
        if !queued && !pending {
            return;
        }
        autosave.clock.submitted(revision);
        let outcome = write_session(&autosave.writer, revision, &self.scene.to_layout());
        autosave.queue.record(&outcome);
        self.note_autosave_outcome(&outcome);
    }

    /// The installed manifest version of `plugin_id`, if that plugin is installed.
    fn installed_version(&self, plugin_id: &str) -> Option<String> {
        installed_version(&self.installed_plugins, plugin_id)
    }

    /// Adds a panel through the one path new panels take, so plugin and TUI kinds carry
    /// the installed manifest version.
    fn add_new_panel(&mut self, kind: PanelKind, rect: Rect) -> (PanelId, PanelKind) {
        let kind = kind.with_installed_version(|id| self.installed_version(id));
        (self.scene.add(kind.clone(), rect), kind)
    }

    /// Installs loaded settings at startup.
    pub fn set_settings_state(&mut self, state: SettingsState) {
        self.settings = state;
        self.apply_settings_to_input();
    }

    /// Pushes the settings the input handler reads.
    pub fn apply_settings_to_input(&mut self) {
        self.input.snap = self.settings.settings.snap.config();
        self.input.click_empty_clears_focus =
            self.settings.settings.canvas.click_empty_clears_focus;
    }

    pub fn set_show_stats(&mut self, show: bool, window: &mut Window, cx: &mut Context<Self>) {
        self.show_stats = show;
        self.frame_stats.reset();
        self.sync_stats_timer(cx);
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
                Effect::ClearFocus => {
                    self.focused = None;
                    // GPUI focus alone leaves a web view or Ghostty surface holding the keys.
                    if !calm_native::focus_host_view(window) {
                        eprintln!("focus: the canvas could not take the first responder");
                    }
                    self.focus_handle.focus(window, cx);
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
                    let items = self.canvas_menu();
                    self.show_menu(items, screen, MenuTarget::Canvas { world }, window, cx);
                }
                Effect::ShowPanelMenu { id, screen } => {
                    if let Some(items) = self.panel_menu(id) {
                        self.show_menu(items, screen, MenuTarget::Panel(id), window, cx);
                    }
                }
                Effect::ClosePanel(id) => {
                    if let Some(terminal) = self.terminal(id) {
                        terminal.update(cx, |terminal, _| terminal.release_preview(window));
                    }
                    self.terminals.retain(|(panel, _)| *panel != id);
                    self.tui_runs.retain(|(panel, _)| *panel != id);
                    self.terminal_meta.retain(|(panel, _)| *panel != id);
                    self.browsers.retain(|(panel, _)| *panel != id);
                    self.plugin_panels.retain(|(panel, _)| *panel != id);
                    self.placeholders.remove(&id);
                    self.title_notes.remove(&id);
                    self.scene.remove(id);
                    if self.focused == Some(id) {
                        self.focused = None;
                    }
                }
            }
        }
        // AppKit must see scene changes even when an occluded window does not paint.
        self.sync_native_order(cx);
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

    /// Runs the item the native menu returned. The smoke calls this with an id from the
    /// built menu, the same path a real click takes after the NSMenu closes.
    pub(crate) fn apply_menu_choice(
        &mut self,
        id: &str,
        target: MenuTarget,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.apply_plugin_choice(id, target, window, cx) {
            window.refresh();
            cx.notify();
            return;
        }
        let world = match target {
            MenuTarget::Canvas { world } => world,
            MenuTarget::Panel(_) => Point::new(0.0, 0.0),
        };
        let profile = menu::terminal_profile_choice(id).map(str::to_string);
        let effects = match (menu::menu_action(id, world, &self.plugin_sizes), target) {
            (Some(MenuCommand::AddPanel { kind, mut rect }), MenuTarget::Canvas { .. }) => {
                if matches!(kind, PanelKind::Terminal { .. }) {
                    rect.size = self.settings.settings.panels.default_terminal_size();
                }
                let (id, kind) = self.add_new_panel(kind, rect);
                if let Some(profile) = profile {
                    self.requested_profiles.insert(id, profile);
                }
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
                let installed =
                    |plugin_id: &str| installed_version(&self.installed_plugins, plugin_id);
                let Some(copy) = self.scene.duplicate(id, installed) else {
                    return;
                };
                if let Some(profile) = self.terminal_meta(id).and_then(|meta| meta.profile.clone())
                {
                    self.requested_profiles.insert(copy, profile);
                }
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
                let profile = self.requested_profiles.remove(&id);
                let opened = self.open_plain_terminal(
                    id,
                    profile,
                    command.as_deref(),
                    cwd.clone(),
                    window,
                    cx,
                );
                if let Err(err) = opened {
                    eprintln!("calm: terminal failed to start: {err}");
                }
            }
            PanelKind::Browser { url, .. } => {
                if let Err(err) = self.spawn_browser(id, url, window, cx) {
                    eprintln!("calm: browser failed to start: {err}");
                }
            }
            PanelKind::Plugin { plugin_id, .. } => {
                if let Err(err) = self.open_plugin_panel(id, plugin_id, window, cx) {
                    eprintln!("calm: plugin panel failed to open: {err}");
                    if let Some(panel) = self.scene.get_mut(id) {
                        panel.title = format!("{plugin_id}: failed to open");
                    }
                }
            }
            PanelKind::Tui { plugin_id, .. } => {
                if let Err(err) = self.open_tui_panel(id, plugin_id, window, cx) {
                    eprintln!("calm: tui panel failed to open: {err}");
                    if let Some(panel) = self.scene.get_mut(id) {
                        panel.title = format!("{plugin_id}: failed to open");
                    }
                }
            }
            // No native view: drawn by GPUI as a placeholder, so it never covers a live one.
            PanelKind::Unsupported { raw } => {
                let reason = placeholder_reason(kind, |_| true, |_| true)
                    .unwrap_or_else(|| format!("Unknown panel type {}", raw.type_name()));
                self.placeholders.insert(id, reason);
            }
        }
    }

    /// Starts a plain terminal for a panel already in the scene, from `profile` (else the
    /// default profile) over the `terminal` defaults. A command or cwd stored on the panel
    /// wins over the profile's.
    pub fn open_plain_terminal(
        &mut self,
        id: PanelId,
        profile: Option<String>,
        command: Option<&str>,
        cwd: Option<std::path::PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let resolved = self.resolve_terminal(profile.as_deref());
        if let Some(asked) = &profile
            && resolved.profile.as_ref() != Some(asked)
        {
            eprintln!("calm: terminal profile {asked:?} not found, using the defaults");
        }
        let home = terminal_profiles::home_dir();
        let inherited = self
            .focused
            .and_then(|focused| self.terminal_meta(focused))
            .map(|meta| meta.cwd.clone());
        let start =
            terminal_profiles::terminal_start(cwd, &resolved.cwd, inherited.as_deref(), &home);
        let options = terminal_profiles::options(&resolved, command, start.clone());
        let configuration = terminal_profiles::configuration_name(&options.configuration);
        self.spawn_terminal(id, options, window, cx)?;
        self.terminals_opened += 1;
        self.terminal_meta.push((
            id,
            TerminalMeta {
                profile,
                index: self.terminals_opened,
                cwd: start,
                applied_colors: resolved.colors,
                configuration,
            },
        ));
        self.refresh_terminal_chrome(id, &resolved);
        Ok(())
    }

    pub fn terminal_meta(&self, id: PanelId) -> Option<&TerminalMeta> {
        self.terminal_meta
            .iter()
            .find(|(panel, _)| *panel == id)
            .map(|(_, meta)| meta)
    }

    fn resolve_terminal(&self, profile: Option<&str>) -> ResolvedTerminal {
        let settings = &self.settings.settings;
        settings
            .terminal
            .resolve(profile, settings.panels.terminal_user_config)
    }

    fn settings_dir(&self) -> std::path::PathBuf {
        self.settings
            .path
            .parent()
            .map_or_else(std::path::PathBuf::new, std::path::Path::to_path_buf)
    }

    /// Sets the title and decodes an image icon once. Decoding and path problems are logged
    /// once per path; the panel then shows the terminal icon.
    fn refresh_terminal_chrome(&mut self, id: PanelId, resolved: &ResolvedTerminal) {
        let Some(meta) = self.terminal_meta(id) else {
            return;
        };
        let cwd = display_cwd(&meta.cwd, &terminal_profiles::home_dir());
        let title = render_title(&resolved.title_template, meta.index, &cwd);
        self.scene.set_title(id, title);
        if let Some(TerminalIcon::Image(relative)) = &resolved.icon {
            let dir = self.settings_dir();
            self.tui_icons.entry(dir.join(relative)).or_insert_with(|| {
                terminal_profiles::icon_file(&dir, relative)
                    .and_then(|path| crate::icon_image::load(&path))
                    .inspect_err(|err| eprintln!("calm: {err}"))
                    .ok()
            });
        }
    }

    /// After a settings change: every plain terminal's title and chrome follow at once, and
    /// a changed palette recolors the running surface. Removing a palette, or switching the
    /// Ghostty user config, needs a new terminal: libghostty only takes a new theme.
    pub fn terminal_settings_changed(&mut self, cx: &mut Context<Self>) {
        let ids: Vec<PanelId> = self.terminal_meta.iter().map(|(id, _)| *id).collect();
        for id in ids {
            let Some(meta) = self.terminal_meta(id) else {
                continue;
            };
            let applied = meta.applied_colors;
            let resolved = self.resolve_terminal(meta.profile.clone().as_deref());
            self.refresh_terminal_chrome(id, &resolved);
            let Some(colors) = resolved.colors.filter(|colors| Some(*colors) != applied) else {
                continue;
            };
            let Some(terminal) = self.terminal(id) else {
                continue;
            };
            let theme = terminal_profiles::theme(&colors);
            match terminal.update(cx, |terminal, _| terminal.apply_theme(&theme)) {
                Ok(()) => {
                    if let Some((_, meta)) = self.terminal_meta.iter_mut().find(|(p, _)| *p == id) {
                        meta.applied_colors = Some(colors);
                    }
                }
                Err(err) => eprintln!("calm: terminal recolor failed: {err}"),
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
        let mut run = TuiRun::new(plugin)?;
        run.extra_args = self.pending_tui_args.take().unwrap_or_default();
        if let Some(path) = run.icon_path() {
            self.tui_icons.entry(path).or_insert_with_key(|path| {
                crate::icon_image::load(path)
                    .inspect_err(|err| eprintln!("calm: tui icon: {err}"))
                    .ok()
            });
        }
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
        if let Some(run) = self.tui_run(id) {
            run.apply_terminal_colors(&mut options);
        }
        if let Some(old) = self.terminal(id) {
            old.update(cx, |terminal, _| terminal.release_preview(window));
            self.terminals.retain(|(panel, _)| *panel != id);
        }
        // A restart replaces the native view without changing its panel id.
        self.native_order.clear();
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
        self.scene.set_title(id, title);
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
        self.scene.set_title(id, title);
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
    pub fn restart_tui(
        &mut self,
        id: PanelId,
        manual: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
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

    /// The chrome look paint uses for a panel. Read by the smoke.
    pub fn panel_look(&self, id: PanelId) -> PanelLook {
        if let Some(meta) = self.terminal_meta(id) {
            let resolved = self.resolve_terminal(meta.profile.as_deref());
            let icon = match resolved.icon {
                Some(TerminalIcon::Text(text)) => Some(IconLook::Text(text)),
                Some(TerminalIcon::Image(relative)) => self
                    .tui_icons
                    .get(&self.settings_dir().join(relative))
                    .cloned()
                    .flatten()
                    .map(IconLook::Image),
                None => None,
            };
            return PanelLook {
                overrides: resolved.overrides,
                icon,
            };
        }
        let Some(run) = self.tui_run(id) else {
            return PanelLook::default();
        };
        let icon = match (run.glyph(), run.icon_path()) {
            (Some(text), _) => Some(IconLook::Text(text.to_string())),
            (None, Some(path)) => self
                .tui_icons
                .get(&path)
                .cloned()
                .flatten()
                .map(IconLook::Image),
            (None, None) => None,
        };
        PanelLook {
            overrides: run.chrome_overrides(),
            icon,
        }
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
        cx: &mut Context<Self>,
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
        let resize = self.scene.get(id).map(|scene_panel| {
            ResizePolicy::derive(&scene_panel.kind, Some(&plugin.manifest.resize()))
        });
        if let Some(scene_panel) = self.scene.get_mut(id) {
            scene_panel.title = title;
        }
        if let Some(policy) = resize {
            self.scene.set_resize_policy(id, policy);
        }
        self.plugin_panels.push((id, panel));
        self.sync_native_order(cx);
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
        let Some(route) = plugin_host::route_in_scene(&mut self.scene, id, &request) else {
            return;
        };
        let response = match route {
            Route::Reply(Some(response)) => response,
            Route::Reply(None) => return,
            Route::Background(call) => return self.serve_in_background(id, request.id, call, cx),
        };
        self.reply_to_plugin(id, &response);
        cx.notify();
    }

    fn reply_to_plugin(&mut self, id: PanelId, response: &calm_plugins::BridgeResponse) {
        if let Some((_, plugin)) = self.plugin_panels.iter_mut().find(|(p, _)| *p == id) {
            plugin.reply(response);
        }
    }

    /// Runs a blocking bridge call on the background executor and answers from a fresh
    /// task, never from inside the message callback. A panel closed meanwhile gets nothing.
    fn serve_in_background(
        &mut self,
        id: PanelId,
        request_id: u64,
        call: ServiceCall,
        cx: &mut Context<Self>,
    ) {
        let services = self.plugin_services.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { services.run(call) })
                .await;
            let response = plugin_host::service_reply(request_id, result);
            // Err means the canvas is gone, so there is no page left to answer.
            let _ = this.update(cx, |view, cx| {
                view.reply_to_plugin(id, &response);
                cx.notify();
            });
        })
        .detach();
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
        self.sync_native_order(cx);
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
                // Navigation keeps the saved page zoom.
                let page_zoom = match &panel.kind {
                    PanelKind::Browser { page_zoom, .. } => *page_zoom,
                    _ => None,
                };
                panel.kind = PanelKind::Browser { url, page_zoom };
            }
            BrowserEvent::Clicked => {
                // The click went to the native view, so GPUI never saw it.
                self.scene.bring_to_front(id);
                self.sync_native_order(cx);
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
                    PanelKind::Browser { url, .. } => Some(url.clone()),
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
        self.sync_native_order(cx);
        Ok(())
    }

    pub(crate) fn terminal(&self, id: PanelId) -> Option<Entity<TerminalPanel>> {
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

    /// Starts, restarts or stops CPU and memory sampling to match the readout and its
    /// settings. Sampling runs on the background executor, never in paint.
    pub fn sync_stats_timer(&mut self, cx: &mut Context<Self>) {
        let stats = &self.settings.settings.developer.stats;
        if !(self.show_stats && (stats.show_cpu || stats.show_memory)) {
            self.stats_timer = None;
            self.stats_timer_config = None;
            self.process_stats = None;
            self.process_samples = 0;
            return;
        }
        let config = (clamp_interval_ms(stats.interval_ms), stats.include_children);
        if self.stats_timer.is_some() && self.stats_timer_config == Some(config) {
            return;
        }
        let (interval_ms, include_children) = config;
        self.stats_timer_config = Some(config);
        self.process_stats = None;
        self.process_samples = 0;
        self.stats_timer = Some(cx.spawn(async move |this, cx| {
            let mut sampler = ProcessSampler::new();
            let mut reported = false;
            loop {
                let (back, result) = cx
                    .background_executor()
                    .spawn(async move {
                        let result = sampler.sample(include_children);
                        (sampler, result)
                    })
                    .await;
                sampler = back;
                let sample = match result {
                    Ok(sample) => Some(sample),
                    Err(err) => {
                        if !reported {
                            eprintln!("calm: process stats unavailable: {err}");
                            reported = true;
                        }
                        None
                    }
                };
                let updated = this.update(cx, |view, cx| {
                    if sample.is_some() {
                        view.process_samples += 1;
                    }
                    view.process_stats = sample;
                    cx.notify();
                });
                if updated.is_err() {
                    break;
                }
                cx.background_executor()
                    .timer(Duration::from_millis(interval_ms))
                    .await;
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
        if let Some(bar) = self.status_bar.as_mut() {
            bar.ensure_front();
        }
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
        let stats = &self.settings.settings.developer.stats;
        let shown = |item: bool| self.show_stats && item;
        let fps = shown(stats.show_fps).then(|| dev_menu::fps_text(&self.frame_stats));
        let cpu = shown(stats.show_cpu)
            .then(|| cpu_text(self.process_stats.and_then(|sample| sample.cpu_percent)));
        let memory = shown(stats.show_memory).then(|| match self.process_stats {
            Some(sample) => memory_text(sample.memory_bytes, stats.memory_unit),
            None => "-- MB".to_string(),
        });
        let [fps, cpu, memory] = stats_items(fps, cpu, memory);
        // The pill has one text color, so the warning sign marks the plugin item.
        let plugin_errors =
            plugin_host::plugin_problems_text(self.plugin_errors, self.plugin_warnings);
        let session_error = self
            .autosave
            .as_ref()
            .and_then(|autosave| autosave.queue.error())
            .map(save_error_text)
            .or_else(|| self.autosave_notice.clone())
            .or_else(|| self.restore_notice.clone())
            .unwrap_or_default();
        let text = status_text([
            session_error.as_str(),
            saved.as_str(),
            plugin_errors.as_str(),
            zoom.as_str(),
            fps.as_str(),
            cpu.as_str(),
            memory.as_str(),
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
        let cursor_changed = self.update_resize_cursor(screen);
        if !effects.is_empty() {
            self.apply_effects(effects, window, cx);
        } else if hover_changed || cursor_changed {
            cx.notify();
        }
    }

    fn mouse_up(&mut self, event: &MouseUpEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !matches!(event.button, MouseButton::Left | MouseButton::Middle) {
            return;
        }
        let screen = self.to_canvas(event.position);
        let effects = self
            .input
            .mouse_up(screen, to_mods(&event.modifiers), &mut self.scene);
        let cursor_changed = self.update_resize_cursor(screen);
        if !effects.is_empty() {
            self.apply_effects(effects, window, cx);
        } else if cursor_changed {
            cx.notify();
        }
    }

    /// The resize cursor follows the same handle lookup as a press. True if it changed.
    fn update_resize_cursor(&mut self, screen: Point) -> bool {
        let cursor = self.input.hover_resize(screen, &self.scene);
        let changed = cursor != self.resize_cursor;
        self.resize_cursor = cursor;
        changed
    }

    /// Drives a press, a move by `delta` canvas pixels and a release through the real
    /// mouse handlers, starting at `from_world`. Alt is held so snapping stays out of the
    /// numbers. Only the smoke uses it.
    pub fn smoke_drag(
        &mut self,
        from_world: Point,
        delta: Point,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let start = self.scene.camera.world_to_screen(from_world);
        let position = |x: f64, y: f64| {
            point(
                px((x + self.viewport.origin.x) as f32),
                px((y + self.viewport.origin.y) as f32),
            )
        };
        let modifiers = gpui::Modifiers {
            alt: true,
            ..gpui::Modifiers::default()
        };
        let from = position(start.x, start.y);
        let to = position(start.x + delta.x, start.y + delta.y);
        let down = MouseDownEvent {
            button: MouseButton::Left,
            position: from,
            modifiers,
            click_count: 1,
            first_mouse: false,
        };
        self.mouse_down(&down, window, cx);
        let moved = MouseMoveEvent {
            position: to,
            pressed_button: Some(MouseButton::Left),
            modifiers,
        };
        self.mouse_move(&moved, window, cx);
        let up = MouseUpEvent {
            button: MouseButton::Left,
            position: to,
            modifiers,
            click_count: 1,
        };
        self.mouse_up(&up, window, cx);
    }

    /// The resize edge a hover at `world` shows a cursor for.
    pub fn resize_cursor_at(&self, world: Point) -> Option<ResizeEdge> {
        let screen = self.scene.camera.world_to_screen(world);
        self.input.hover_resize(screen, &self.scene)
    }

    /// (panel, web view frame size) per browser, in creation order.
    pub fn browser_page_sizes(&self) -> Vec<(PanelId, Size)> {
        self.browsers
            .iter()
            .map(|(id, browser)| (*id, browser.page_size()))
            .collect()
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

    /// Escape that reached the canvas. A native view with the keyboard keeps it, and its
    /// panel keeps focus: Escape belongs to the program or page in there.
    fn cancel(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let native_keyboard = calm_native::host_view_is_first_responder(window) != Some(true);
        let effects = self.input.escape(native_keyboard);
        self.apply_effects(effects, window, cx);
    }

    /// A world point on screen over no panel, chrome or resize band. Only the smoke uses it.
    pub fn smoke_empty_point(&self) -> Option<Point> {
        let size = self.viewport.size;
        let steps = |len: f64| (1..(len / 20.0) as usize).map(|i| i as f64 * 20.0);
        steps(size.height)
            .flat_map(|y| steps(size.width).map(move |x| Point::new(x, y)))
            .map(|screen| self.scene.camera.screen_to_world(screen))
            .find(|world| {
                self.scene.hit_test(*world).is_none() && self.resize_cursor_at(*world).is_none()
            })
    }

    pub fn focused(&self) -> Option<PanelId> {
        self.focused
    }

    /// Presses and releases `button` at `world` through the real mouse handlers, without
    /// moving. Only the smoke uses it.
    pub fn smoke_click(
        &mut self,
        world: Point,
        button: MouseButton,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let screen = self.scene.camera.world_to_screen(world);
        let position = point(
            px((screen.x + self.viewport.origin.x) as f32),
            px((screen.y + self.viewport.origin.y) as f32),
        );
        let modifiers = gpui::Modifiers::default();
        let down = MouseDownEvent {
            button,
            position,
            modifiers,
            click_count: 1,
            first_mouse: false,
        };
        self.mouse_down(&down, window, cx);
        let up = MouseUpEvent {
            button,
            position,
            modifiers,
            click_count: 1,
        };
        self.mouse_up(&up, window, cx);
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
pub(crate) enum MenuTarget {
    /// Empty canvas; new panels go at `world`.
    Canvas {
        world: Point,
    },
    Panel(PanelId),
}

/// What paint needs, copied out at render time so painting does not borrow the view.
/// A panel's own chrome colors and icon, over the theme.
#[derive(Clone, Debug, Default)]
pub struct PanelLook {
    pub overrides: ChromeOverrides,
    pub icon: Option<IconLook>,
}

#[derive(Clone, Debug)]
pub enum IconLook {
    Text(String),
    Image(Arc<RenderImage>),
}

/// A theme 0xRRGGBB color as 0xRRGGBBAA.
const fn opaque(rgb: u32) -> u32 {
    (rgb << 8) | 0xff
}

/// Theme chrome colors as 0xRRGGBBAA.
pub(crate) fn chrome_palette() -> ChromePalette {
    ChromePalette {
        bar: opaque(theme::PANEL_BAR),
        bar_focused: opaque(theme::PANEL_BAR_FOCUSED),
        border: opaque(theme::PANEL_BORDER),
        accent: opaque(theme::ACCENT),
        title: opaque(theme::TITLE_TEXT),
        title_focused: opaque(theme::TITLE_TEXT_FOCUSED),
        confirm_bar: opaque(theme::CONFIRM_BAR),
        confirm_border: opaque(theme::CONFIRM_BORDER),
        confirm_title: opaque(theme::CONFIRM_TEXT),
    }
}

struct PaintSnapshot {
    scene: Scene,
    state: Interaction,
    focused: Option<PanelId>,
    confirming: Option<PanelId>,
    hover: Option<(PanelId, Option<ChromeButton>)>,
    terminals: Vec<(PanelId, Entity<TerminalPanel>)>,
    /// Panels with a plugin web view.
    plugin_ids: Vec<PanelId>,
    /// Panels with custom chrome colors or icon. Others use the theme.
    looks: HashMap<PanelId, PanelLook>,
    /// Close confirmation or context menu up: native views must not cover it.
    modal_open: bool,
    /// Terminals that got an input div in this render.
    input_targets: Vec<PanelId>,
    settings: calm_core::settings::Settings,
    /// Rendered gradient or image, if one is ready.
    background: Option<BackgroundTexture>,
    grid: GridFrame,
    /// Computed once per render from `scene`.
    joins: JoinLayout,
    /// Restored panels that cannot run, with their line.
    placeholders: HashMap<PanelId, String>,
    title_notes: HashMap<PanelId, &'static str>,
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
            grid_paint::paint_grid(&self.grid, bounds, window);
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

    fn overrides(&self, id: PanelId) -> &ChromeOverrides {
        const THEME: ChromeOverrides = ChromeOverrides {
            accent: None,
            title_bar: None,
            title: None,
        };
        self.looks.get(&id).map_or(&THEME, |look| &look.overrides)
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
        let color = rgba(divider_color(
            [join.first, join.second].map(|id| {
                (
                    ChromeState::of(self.focused == Some(id), self.confirming == Some(id)),
                    self.overrides(id),
                )
            }),
            &chrome_palette(),
        ));
        let gap = camera.world_rect_to_screen(join.divider);
        window.paint_quad(fill(to_bounds(gap, origin), color));
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
        window.paint_quad(fill(to_bounds(seam, origin), color));
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
        let look = self.looks.get(&panel.id);
        let overrides = self.overrides(panel.id);
        let colors = chrome_colors(
            ChromeState::of(focused, confirming),
            overrides,
            &chrome_palette(),
        );

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
            rgba(colors.bar),
            Edges {
                top: border(Side::Top),
                right: border(Side::Right),
                bottom: border(Side::Bottom),
                left: border(Side::Left),
            },
            rgba(colors.border),
            BorderStyle::Solid,
        ));
        if let Some(edges) = edges {
            self.paint_open_ring(body, edges, rgba(colors.border), origin, window);
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
                            color: opaque(theme::TITLE_TEXT),
                        },
                        zoom,
                        origin,
                        window,
                        cx,
                    );
                }
                Some(TerminalContent::Live) | None => {
                    if let Some(reason) = self.placeholders.get(&panel.id) {
                        let padding = 8.0 * zoom;
                        let line = Rect::new(
                            content_screen.origin.x + padding,
                            content_screen.origin.y + padding,
                            (content_screen.size.width - 2.0 * padding).max(0.0),
                            TITLE_BAR_HEIGHT * zoom,
                        );
                        let look = TextLook {
                            align: TextAlign::Left,
                            color: opaque(theme::TITLE_TEXT),
                        };
                        paint_text(reason, line, look, zoom, origin, window, cx);
                    }
                }
            }
        }

        if !details_visible(zoom) {
            return;
        }
        let kind_color = match PanelIcon::for_kind(&panel.kind) {
            PanelIcon::Terminal => theme::ICON_TERMINAL,
            PanelIcon::Browser => theme::ICON_BROWSER,
            PanelIcon::Plugin => theme::ICON_PLUGIN,
        };
        let icon_color = icon_tint(overrides, opaque(kind_color));
        let stroke = (1.5 * zoom).max(1.0);
        let icon_rect = camera.world_rect_to_screen(layout.icon);
        if let Some(IconLook::Image(image)) = look.and_then(|look| look.icon.as_ref()) {
            let bounds = to_bounds(icon_rect, origin);
            if let Err(err) =
                window.paint_image(bounds, bounds, Default::default(), image.clone(), 0, false)
            {
                eprintln!("calm: icon paint failed: {err:#}");
            }
        } else if let Some(IconLook::Text(glyph)) = look.and_then(|look| look.icon.as_ref()) {
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
                    opaque(glyph_color),
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
                    color: opaque(theme::STALE_TEXT),
                },
                zoom,
                origin,
                window,
                cx,
            );
            middle.size.width = (middle.size.width - width - 8.0 * zoom).max(0.0);
        }
        let noted;
        let title = match self.title_notes.get(&panel.id) {
            _ if confirming => CONFIRM_TITLE,
            Some(note) => {
                noted = format!("{} ({note})", panel.title);
                noted.as_str()
            }
            None => panel.title.as_str(),
        };
        paint_text(
            title,
            middle,
            TextLook {
                align: TextAlign::Left,
                color: colors.title,
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
            Ok(path) => window.paint_path(path, rgba(color)),
            Err(err) => eprintln!("calm: chrome stroke failed: {err:?}"),
        }
    }
}

/// Alignment and color of one `paint_text` line.
struct TextLook {
    align: TextAlign,
    /// 0xRRGGBBAA.
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
        color: rgba(look.color).into(),
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
        self.autosave_tick(cx);
        if self.show_stats && self.settings.settings.developer.stats.show_fps {
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
        let grid = self.grid.prepare(
            &self.settings.settings.grid,
            &self.scene.camera,
            canvas_size,
            window,
        );
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
            looks: self
                .tui_runs
                .iter()
                .map(|(id, _)| (*id, self.panel_look(*id)))
                .collect(),
            modal_open: self.input.confirming.is_some() || self.menu_open,
            input_targets: Vec::new(),
            settings: self.settings.settings.clone(),
            background,
            grid,
            joins: JoinLayout::compute(&self.scene.panels),
            placeholders: self.placeholders.clone(),
            title_notes: self.title_notes.clone(),
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
                view.set_show_stats(!view.show_stats, window, cx)
            }))
            .on_action(cx.listener(|view, _: &ResetFps, window, cx| view.reset_fps(window, cx)))
            .map(|root| settings_menu::bind(root, cx))
            .child(
                div()
                    .flex_1()
                    .w_full()
                    .relative()
                    .when_some(self.resize_cursor, |canvas, edge| {
                        canvas.cursor(match edge {
                            ResizeEdge::Right => CursorStyle::ResizeLeftRight,
                            ResizeEdge::Bottom => CursorStyle::ResizeUpDown,
                            ResizeEdge::BottomRight => CursorStyle::ResizeUpLeftDownRight,
                        })
                    })
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

/// Runs one ordered session write. A poisoned lock still writes: the writer holds no
/// half-done state between calls.
fn write_session(
    writer: &Mutex<OrderedWriter<SessionWriter>>,
    generation: u64,
    layout: &LayoutFile,
) -> WriteOutcome {
    writer
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .write(generation, |session| {
            session.save(layout).map_err(|err| err.to_string())
        })
}

/// The manifest version of the installed plugin `plugin_id`.
fn installed_version(plugins: &[InstalledPlugin], plugin_id: &str) -> Option<String> {
    plugins
        .iter()
        .find(|plugin| plugin.manifest.id == plugin_id)
        .map(|plugin| plugin.manifest.version.clone())
}
