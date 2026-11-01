//! Plugin panels: discovery at startup, one WKWebView per panel, and the host side of the
//! `window.calm` bridge. Calls that block on the host live in `plugin_services`.

use crate::plugin_services::ServiceCall;
use calm_core::geometry::{Point, Rect, Size};
use calm_core::interaction::Effect;
use calm_core::preview::PanelMode;
use calm_core::resize::ResizePolicy;
use calm_core::scene::{PanelId, PanelKind, Scene};
use calm_core::viewport::NativePlacement;
use calm_native::{NativeChild, WebPanel};
use calm_plugins::{
    API_VERSION, ApiError, BridgeEvent, BridgeRequest, BridgeResponse, CALM_PRELUDE_JS, Discovery,
    ErrorCode, InstalledPlugin, discover_search, host_info_js, is_known_method, missing_scripts,
    plugin_search,
};
use futures::channel::mpsc::UnboundedSender;
use gpui::Window;
use serde_json::Value;

/// Carries raw bridge messages from a page's script handler to the canvas view.
pub type BridgeSender = UnboundedSender<(PanelId, String)>;

/// Scans the plugin folders once and logs every searched dir and every plugin found,
/// shadowed or skipped. `builtin` is the `plugins.builtin` setting. Also returns the
/// searched dirs, user dirs first, for the empty Add menu.
pub fn discover_installed(builtin: bool) -> (Discovery, Vec<String>) {
    let search = plugin_search(builtin);
    let mut searched = Vec::new();
    for (kind, dirs) in [("user", &search.user), ("builtin", &search.builtin)] {
        for dir in dirs {
            let state = if dir.is_dir() { "" } else { " (missing)" };
            eprintln!("calm: plugin dir {kind} {}{state}", dir.display());
            searched.push(dir.display().to_string());
        }
    }
    if let Some(reason) = search.builtin_disabled_by {
        eprintln!("calm: built-in plugins off ({reason})");
    }
    let discovery = discover_search(&search);
    for (index, plugin) in discovery.plugins.iter().enumerate() {
        let kind = if index < discovery.user_count() {
            "user"
        } else {
            "builtin"
        };
        let unbuilt = missing_scripts(plugin)
            .map(|missing| {
                if missing.is_empty() {
                    ""
                } else {
                    " (not built)"
                }
            })
            .unwrap_or(" (entry unreadable)");
        eprintln!(
            "calm: plugin found {kind} {} at {}{unbuilt}",
            plugin.manifest.id,
            plugin.dir.display()
        );
    }
    for shadowed in &discovery.shadowed {
        eprintln!(
            "calm: plugin {} at {} shadows the built-in at {}",
            shadowed.id,
            shadowed.kept.display(),
            shadowed.hidden.display()
        );
    }
    for err in &discovery.errors {
        eprintln!("calm: plugin skipped: {err}");
    }
    for warning in &discovery.warnings {
        eprintln!("calm: plugin warning: {warning}");
    }
    if discovery.plugins.is_empty() {
        eprintln!("calm: no plugins found");
    }
    (discovery, searched)
}

/// The status bar item for skipped plugins and plugins loaded with warnings, empty if none.
/// The pill has one text color, so the warning sign marks the item.
pub fn plugin_problems_text(errors: usize, warnings: usize) -> String {
    match (errors, warnings) {
        (0, 0) => String::new(),
        (errors, 0) => format!("⚠ plugins: {errors} errors"),
        (0, warnings) => format!("⚠ plugins: {warnings} warnings"),
        (errors, warnings) => format!("⚠ plugins: {errors} errors, {warnings} warnings"),
    }
}

/// Menu lists as `CanvasView` keeps them, in the shape `calm_core::menu` takes.
#[derive(Debug, Default, PartialEq)]
pub struct MenuEntries {
    /// (id, name) per web plugin.
    pub web: Vec<(String, String)>,
    /// (id, name) per TUI plugin.
    pub apps: Vec<(String, String)>,
    /// (id, default size) per plugin of either kind.
    pub sizes: Vec<(String, Size)>,
}

pub fn menu_entries(plugins: &[InstalledPlugin]) -> MenuEntries {
    let mut entries = MenuEntries::default();
    for plugin in plugins {
        let manifest = &plugin.manifest;
        let name = (manifest.id.clone(), manifest.name.clone());
        if manifest.tui().is_some() {
            entries.apps.push(name);
        } else {
            entries.web.push(name);
        }
        let size = Size::new(manifest.default_size.width, manifest.default_size.height);
        entries.sizes.push((manifest.id.clone(), size));
    }
    entries
}

/// A plugin page in a WKWebView, placed through the same clip and zoom views as terminals.
pub struct PluginPanel {
    // Declared before `host` so it drops first: the web view sits inside the host's views.
    web: WebPanel,
    host: NativeChild,
    logical_size: Option<Size>,
    visible: bool,
    /// Why the plugin page was not loaded. The panel shows this text instead.
    load_error: Option<String>,
}

impl PluginPanel {
    /// Creates the web view and loads the entry page, or an error page if its local
    /// scripts are missing. Messages the page posts go to `bridge` tagged with `id`.
    pub fn open(
        id: PanelId,
        plugin: &InstalledPlugin,
        bridge: BridgeSender,
        window: &mut Window,
    ) -> Result<PluginPanel, String> {
        let mut host = NativeChild::new(window).map_err(|err| err.to_string())?;
        host.set_visible(false);
        let mut web = WebPanel::new(&host).map_err(|err| err.to_string())?;
        // Before the prelude, so `calm.has` works from the first line of the page.
        web.add_user_script_at_document_start(&host_info_js(API_VERSION, served_methods()));
        web.add_user_script_at_document_start(CALM_PRELUDE_JS);
        web.on_message(Box::new(move |text| {
            if bridge.unbounded_send((id, text)).is_err() {
                eprintln!("calm: plugin panel {}: canvas gone, message dropped", id.0);
            }
        }));
        let load_error = match missing_scripts(plugin) {
            Ok(missing) if missing.is_empty() => None,
            Ok(missing) => Some(not_built_message(plugin, &missing)),
            Err(err) => Some(err),
        };
        match &load_error {
            None => {
                web.load_file(
                    &plugin.dir.join(plugin.manifest.entry().ok_or_else(|| {
                        format!("plugin {} has no HTML entry", plugin.manifest.id)
                    })?),
                    &plugin.dir,
                )
            }
            Some(message) => {
                eprintln!("calm: plugin {}: {message}", plugin.manifest.id);
                let page = error_page_url(&plugin.manifest.name, message);
                if !web.load_url(&page) {
                    return Err(format!("plugin {}: {message}", plugin.manifest.id));
                }
            }
        }
        Ok(PluginPanel {
            web,
            host,
            logical_size: None,
            visible: false,
            load_error,
        })
    }

    pub fn load_error(&self) -> Option<&str> {
        self.load_error.as_deref()
    }

    /// Live shows the web view at `placement`; Preview and Hidden hide it. The page keeps
    /// running either way, so returning to Live shows current content.
    pub fn apply_mode(
        &mut self,
        mode: PanelMode,
        placement: Option<NativePlacement>,
        window: &Window,
    ) {
        let placement = placement.filter(|_| mode == PanelMode::Live);
        let Some(placement) = placement else {
            self.set_visible(false);
            return;
        };
        if self.logical_size != Some(placement.logical_size) {
            // Only a resize gets here; zoom scales the zoom view, not the page.
            self.web.set_logical_size(placement.logical_size);
            self.logical_size = Some(placement.logical_size);
        }
        let scale = f64::from(window.scale_factor());
        let viewport_height = f64::from(f32::from(window.viewport_size().height));
        // `apply` sets the frame and unhides the clip view.
        self.host.apply(&placement, viewport_height, scale);
        self.visible = true;
    }

    fn set_visible(&mut self, visible: bool) {
        if self.visible != visible {
            self.visible = visible;
            self.host.set_visible(visible);
        }
    }

    pub fn bring_to_front(&mut self) {
        self.host.bring_to_front();
    }

    /// Gives the page the keyboard, as a terminal or browser gets it on focus.
    pub fn focus(&self) -> bool {
        self.web.focus()
    }

    pub fn stack_index(&self) -> Option<usize> {
        self.host.stack_index()
    }

    pub fn is_loading(&self) -> bool {
        self.web.is_loading()
    }

    pub fn reply(&mut self, response: &BridgeResponse) {
        self.web.eval_js(&response.to_js());
    }

    /// Fans a host event out to the page's `calm.events.on` handlers.
    pub fn emit(&mut self, event: &BridgeEvent) {
        self.web.eval_js(&event.to_js());
    }
}

fn not_built_message(plugin: &InstalledPlugin, missing: &[std::path::PathBuf]) -> String {
    let files: Vec<String> = missing.iter().map(|p| p.display().to_string()).collect();
    format!(
        "not built: {} missing. Run `bun install && bun run build` in {}",
        files.join(", "),
        plugin.dir.display()
    )
}

/// A self-contained page that states why the plugin did not load.
fn error_page_url(name: &str, message: &str) -> String {
    let html = format!(
        "<!doctype html><meta charset=\"utf-8\"><body style=\"margin:0;padding:12px;\
         font:13px -apple-system,sans-serif;background:#1e1e1e;color:#ff7b72\">\
         <b>{}</b><p style=\"white-space:pre-wrap\">{}</p></body>",
        escape_html(name),
        escape_html(message)
    );
    format!("data:text/html;charset=utf-8,{}", percent_encode(&html))
}

fn escape_html(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 3);
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || b"-_.~".contains(&byte) {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Every method this host answers with something other than `not_implemented`, here or in
/// `plugin_services`. Pages read it as `window.__calmHost.methods` through `calm.has`.
pub fn served_methods() -> &'static [&'static str] {
    &[
        "log",
        "panel.setTitle",
        "panel.getBounds",
        "panel.minimize",
        "panel.close",
        "canvas.createPanel",
        "canvas.focusPanel",
        "canvas.listPanels",
        "settings.get",
        "settings.set",
        "github.api",
        "storage.get",
        "storage.set",
        "storage.remove",
        "openExternal",
        "process.exec",
        "theme.get",
    ]
}

/// What a bridge call can see and change beyond its own panel. Built by the canvas for
/// each call; tests build it by hand.
pub struct HostContext<'a> {
    pub scene: &'a mut Scene,
    /// The calling plugin panel.
    pub caller: PanelId,
    pub focused: Option<PanelId>,
    /// Canvas size in window points, for placing a new panel at the viewport center.
    pub viewport: Size,
    /// `BackgroundSettings::base_color()` as `#rrggbb`.
    pub background: String,
    /// `panels.default_terminal_width/height` from settings.
    pub default_terminal_size: Size,
    pub plugins: &'a [InstalledPlugin],
}

/// Work a bridge call leaves for the canvas, which has the window and the native views.
#[derive(Debug, Clone, PartialEq)]
pub enum HostEffect {
    /// The panel is in the scene already; start its terminal, browser or page.
    Open { id: PanelId, kind: PanelKind },
    /// Give the panel the keyboard, as a click does.
    Focus(PanelId),
    /// The normal close path: `Effect::ClosePanelRequested`, so a live process still asks.
    Close(PanelId),
}

impl HostEffect {
    /// The canvas effects for focus and close. `Open` has its own path because its kind
    /// may not get the focus.
    pub fn canvas_effects(&self) -> Vec<Effect> {
        match self {
            HostEffect::Open { .. } => Vec::new(),
            HostEffect::Focus(id) => vec![Effect::FocusPanel(*id), Effect::Redraw],
            HostEffect::Close(id) => vec![Effect::ClosePanelRequested(*id)],
        }
    }
}

/// Where a bridge call is answered.
#[derive(Debug, PartialEq)]
pub enum Route {
    /// Answered now on the UI thread; `None` means no reply (`log`).
    Reply(Option<BridgeResponse>),
    /// Blocks on the host (`gh`, files, `open`, `exec`): run it on the background executor
    /// and answer with `service_reply`.
    Background(ServiceCall),
    /// Answered now; the canvas then applies the effects in order.
    Apply {
        reply: BridgeResponse,
        effects: Vec<HostEffect>,
    },
}

/// Routes one call from the plugin panel `host.caller`. `None` when that panel is gone.
/// Background services come first, then everything the UI thread answers. Read-only
/// methods never take mutable access to the scene, so they cause no autosave.
pub fn route(host: &mut HostContext, request: &BridgeRequest) -> Option<Route> {
    let panel = host.scene.get(host.caller)?;
    let plugin_id = match &panel.kind {
        PanelKind::Plugin { plugin_id, .. } => Some(plugin_id.as_str()),
        _ => None,
    };
    Some(
        match crate::plugin_services::parse(plugin_id, &request.method, &request.params) {
            Some(Ok(call)) => Route::Background(call),
            Some(Err(error)) => Route::Reply(Some(BridgeResponse::Err(request.id, error))),
            None => dispatch(host, request),
        },
    )
}

/// Serves one call on the UI thread. A missing caller is answered with `not_found`;
/// `route` already returns `None` for it.
pub fn dispatch(host: &mut HostContext, request: &BridgeRequest) -> Route {
    let id = request.id;
    let params = &request.params;
    let caller = host.caller;
    let reply = |result: Result<Value, ApiError>| {
        Route::Reply(Some(match result {
            Ok(value) => BridgeResponse::Ok(id, value),
            Err(error) => BridgeResponse::Err(id, error),
        }))
    };
    let apply = |result: Result<(Value, Vec<HostEffect>), ApiError>| match result {
        Ok((value, effects)) => Route::Apply {
            reply: BridgeResponse::Ok(id, value),
            effects,
        },
        Err(error) => Route::Reply(Some(BridgeResponse::Err(id, error))),
    };
    let Some(panel) = host.scene.get(caller) else {
        return reply(Err(ApiError::not_found(caller)));
    };
    match request.method.as_str() {
        "log" => {
            let message = params.get("message").and_then(Value::as_str).unwrap_or("");
            eprintln!("calm: plugin panel {}: {message}", caller.0);
            Route::Reply(None)
        }
        "panel.setTitle" => reply(match params.get("title").and_then(Value::as_str) {
            Some(title) => {
                host.scene.set_title(caller, title.to_string());
                Ok(Value::Null)
            }
            None => Err(ApiError::invalid_params("title", "must be a string")),
        }),
        "panel.getBounds" => reply(Ok(serde_json::json!({
            "width": panel.rect.size.width,
            "height": panel.rect.size.height,
            "zoom": host.scene.camera.zoom,
        }))),
        "panel.minimize" => {
            if !panel.minimized
                && let Some(panel) = host.scene.get_mut(caller)
            {
                panel.minimized = true;
            }
            reply(Ok(Value::Null))
        }
        // Settled before the close: the page goes away with its panel.
        "panel.close" => apply(Ok((Value::Null, vec![HostEffect::Close(caller)]))),
        "canvas.listPanels" => reply(Ok(list_panels(host.scene, host.focused))),
        "canvas.focusPanel" => apply(focus_panel(host.scene, params)),
        "canvas.createPanel" => apply(create_panel(host, params)),
        "theme.get" => reply(Ok(serde_json::json!({ "background": host.background }))),
        "settings.get" => reply(match &panel.kind {
            PanelKind::Plugin { settings, .. } => Ok(settings.clone()),
            _ => Err(ApiError::wrong_kind(caller, "plugin")),
        }),
        "settings.set" => reply(match &panel.kind {
            PanelKind::Plugin { .. } if params.is_object() => {
                if let Some(PanelKind::Plugin { settings, .. }) =
                    host.scene.get_mut(caller).map(|panel| &mut panel.kind)
                {
                    *settings = params.clone();
                }
                Ok(Value::Null)
            }
            PanelKind::Plugin { .. } => Err(ApiError::invalid_params("value", "must be an object")),
            _ => Err(ApiError::wrong_kind(caller, "plugin")),
        }),
        method if is_known_method(method) => {
            Route::Reply(Some(BridgeResponse::not_implemented(id, method)))
        }
        method => Route::Reply(Some(BridgeResponse::unknown_method(id, method))),
    }
}

fn kind_word(kind: &PanelKind) -> &'static str {
    match kind {
        PanelKind::Terminal { .. } => "terminal",
        PanelKind::Browser { .. } => "browser",
        PanelKind::Plugin { .. } => "plugin",
        PanelKind::Tui { .. } => "tui",
        PanelKind::Unsupported { .. } => "unsupported",
    }
}

fn rect_json(rect: Rect) -> Value {
    serde_json::json!({
        "x": rect.origin.x,
        "y": rect.origin.y,
        "width": rect.size.width,
        "height": rect.size.height,
    })
}

fn size_json(size: Size) -> Value {
    serde_json::json!({ "width": size.width, "height": size.height })
}

/// `PanelInfo[]` back to front, every panel in the scene. The v1 fields, plus the v2
/// fields that need no host state beyond the scene and focus.
pub fn list_panels(scene: &Scene, focused: Option<PanelId>) -> Value {
    let panels = scene.panels.iter().enumerate().map(|(z, panel)| {
        let mut info = serde_json::json!({
            "id": panel.id.0,
            "kind": kind_word(&panel.kind),
            "title": panel.title,
            "rect": rect_json(panel.rect),
            "minimized": panel.minimized,
            "z": z,
            "focused": focused == Some(panel.id),
            "resize": {
                "resizable": panel.resize.resizable,
                "minSize": size_json(panel.resize.min),
                "maxSize": panel.resize.max.map_or(Value::Null, size_json),
            },
        });
        match &panel.kind {
            PanelKind::Plugin { plugin_id, .. } | PanelKind::Tui { plugin_id, .. } => {
                info["pluginId"] = Value::from(plugin_id.as_str());
            }
            PanelKind::Browser { url, .. } => info["url"] = Value::from(url.as_str()),
            PanelKind::Terminal { .. } | PanelKind::Unsupported { .. } => {}
        }
        info
    });
    Value::Array(panels.collect())
}

fn panel_id_param(params: &Value, field: &str) -> Result<PanelId, ApiError> {
    params
        .get(field)
        .and_then(Value::as_u64)
        .map(PanelId)
        .ok_or_else(|| ApiError::invalid_params(field, "must be a panel id"))
}

/// Restores a minimized panel, raises it and gives it the keyboard.
fn focus_panel(scene: &mut Scene, params: &Value) -> Result<(Value, Vec<HostEffect>), ApiError> {
    let id = panel_id_param(params, "id")?;
    let panel = scene.get(id).ok_or_else(|| ApiError::not_found(id))?;
    if panel.minimized
        && let Some(panel) = scene.get_mut(id)
    {
        panel.minimized = false;
    }
    scene.bring_to_front(id);
    Ok((Value::Null, vec![HostEffect::Focus(id)]))
}

/// Spec fields of `panels.create` that this host does not build yet.
const CREATE_LATER_FIELDS: &[&str] = &[
    "near",
    "side",
    "size",
    "gap",
    "title",
    "args",
    "env",
    "appearance",
];
/// v2 spec default for a browser panel.
const BROWSER_DEFAULT_SIZE: Size = Size::new(800.0, 600.0);

/// Adds a terminal, browser, plugin or TUI panel from a `PanelSpec`. The size goes through
/// the new panel's resize policy, as a drag does. The reply is the new id.
fn create_panel(
    host: &mut HostContext,
    params: &Value,
) -> Result<(Value, Vec<HostEffect>), ApiError> {
    let spec = params
        .get("spec")
        .and_then(Value::as_object)
        .ok_or_else(|| ApiError::invalid_params("spec", "must be an object"))?;
    let kind_word = spec
        .get("kind")
        .and_then(Value::as_str)
        .ok_or_else(|| ApiError::invalid_params("spec.kind", "must be a string"))?;
    let allowed: &[&str] = match kind_word {
        "terminal" => &["kind", "command", "cwd", "rect", "focus"],
        "browser" => &["kind", "url", "rect", "focus"],
        "plugin" => &["kind", "pluginId", "settings", "rect", "focus"],
        "tui" => &["kind", "pluginId", "rect", "focus"],
        other => {
            return Err(ApiError::invalid_params(
                "spec.kind",
                &format!("must be terminal, browser, plugin or tui, got {other:?}"),
            ));
        }
    };
    for key in spec.keys() {
        if CREATE_LATER_FIELDS.contains(&key.as_str()) {
            return Err(ApiError::new(
                ErrorCode::Unavailable,
                format!("spec.{key}: not built in this host yet"),
            ));
        }
        if !allowed.contains(&key.as_str()) {
            return Err(ApiError::invalid_params(
                &format!("spec.{key}"),
                &format!("is not a field of a {kind_word} panel"),
            ));
        }
    }
    let optional_string = |field: &str| match spec.get(field) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(text)) => Ok(Some(text.clone())),
        Some(_) => Err(ApiError::invalid_params(
            &format!("spec.{field}"),
            "must be a string",
        )),
    };
    let installed = |plugin_id: &str, want_tui: bool| {
        host.plugins
            .iter()
            .find(|plugin| {
                plugin.manifest.id == plugin_id && plugin.manifest.tui().is_some() == want_tui
            })
            .ok_or_else(|| {
                let what = if want_tui { "TUI app" } else { "web plugin" };
                ApiError::new(
                    ErrorCode::NotInstalled,
                    format!("spec.pluginId: no installed {what} {plugin_id:?}"),
                )
            })
    };
    let manifest_size = |plugin: &InstalledPlugin| {
        let size = plugin.manifest.default_size;
        Size::new(size.width, size.height)
    };
    let (kind, default_size, plugin) = match kind_word {
        "terminal" => {
            let command = optional_string("command")?;
            let cwd = optional_string("cwd")?.map(std::path::PathBuf::from);
            if let Some(cwd) = &cwd
                && !(cwd.is_absolute() && cwd.is_dir())
            {
                return Err(ApiError::invalid_params(
                    "spec.cwd",
                    "must be an absolute path to a directory",
                ));
            }
            let kind = PanelKind::Terminal { command, cwd };
            (kind, host.default_terminal_size, None)
        }
        "browser" => {
            let url = optional_string("url")?
                .and_then(|url| calm_core::browser_url::normalize_address(&url))
                .ok_or_else(|| {
                    ApiError::invalid_params("spec.url", "must be a non-empty address")
                })?;
            let kind = PanelKind::Browser {
                url,
                page_zoom: None,
            };
            (kind, BROWSER_DEFAULT_SIZE, None)
        }
        "plugin" => {
            let plugin_id = optional_string("pluginId")?
                .ok_or_else(|| ApiError::invalid_params("spec.pluginId", "is required"))?;
            let plugin = installed(&plugin_id, false)?;
            let settings = match spec.get("settings") {
                None | Some(Value::Null) => Value::Object(serde_json::Map::new()),
                Some(value @ Value::Object(_)) => value.clone(),
                Some(_) => {
                    return Err(ApiError::invalid_params(
                        "spec.settings",
                        "must be an object",
                    ));
                }
            };
            let kind = PanelKind::Plugin {
                plugin_id,
                plugin_version: Some(plugin.manifest.version.clone()),
                settings,
            };
            (kind, manifest_size(plugin), Some(plugin))
        }
        _ => {
            let plugin_id = optional_string("pluginId")?
                .ok_or_else(|| ApiError::invalid_params("spec.pluginId", "is required"))?;
            let plugin = installed(&plugin_id, true)?;
            let kind = PanelKind::Tui {
                plugin_id,
                plugin_version: Some(plugin.manifest.version.clone()),
            };
            (kind, manifest_size(plugin), Some(plugin))
        }
    };
    let focus = match spec.get("focus") {
        None | Some(Value::Null) => true,
        Some(Value::Bool(focus)) => *focus,
        Some(_) => return Err(ApiError::invalid_params("spec.focus", "must be a boolean")),
    };
    let rect = match spec.get("rect") {
        None | Some(Value::Null) => {
            let center = host.scene.camera.screen_to_world(Point::new(
                host.viewport.width / 2.0,
                host.viewport.height / 2.0,
            ));
            Rect {
                origin: Point::new(
                    center.x - default_size.width / 2.0,
                    center.y - default_size.height / 2.0,
                ),
                size: default_size,
            }
        }
        Some(value) => rect_param(value, "spec.rect")?,
    };
    let policy = ResizePolicy::derive(&kind, plugin.map(|p| p.manifest.resize()).as_ref());
    // A locked plugin keeps its manifest size; every other size is held within the limits.
    let size = if policy.resizable {
        policy.clamp(rect.size)
    } else if rect.size == default_size {
        default_size
    } else {
        return Err(ApiError::invalid_params(
            "spec.rect",
            &format!(
                "plugin {} is not resizable, its size is {}x{}",
                plugin.map_or("", |p| p.manifest.id.as_str()),
                default_size.width,
                default_size.height
            ),
        ));
    };
    let id = host.scene.add(
        kind.clone(),
        Rect {
            origin: rect.origin,
            size,
        },
    );
    host.scene.set_resize_policy(id, policy);
    let mut effects = vec![HostEffect::Open { id, kind }];
    if focus {
        effects.push(HostEffect::Focus(id));
    }
    Ok((Value::from(id.0), effects))
}

fn rect_param(value: &Value, field: &str) -> Result<Rect, ApiError> {
    let number = |key: &str| {
        value
            .get(key)
            .and_then(Value::as_f64)
            .filter(|n| n.is_finite())
            .ok_or_else(|| ApiError::invalid_params(&format!("{field}.{key}"), "must be a number"))
    };
    let rect = Rect::new(
        number("x")?,
        number("y")?,
        number("width")?,
        number("height")?,
    );
    if rect.size.width <= 0.0 || rect.size.height <= 0.0 {
        return Err(ApiError::invalid_params(
            field,
            "width and height must be above 0",
        ));
    }
    Ok(rect)
}

/// The reply for a finished background call.
pub fn service_reply(id: u64, result: Result<Value, ApiError>) -> BridgeResponse {
    match result {
        Ok(value) => BridgeResponse::Ok(id, value),
        Err(error) => BridgeResponse::Err(id, error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use calm_core::interaction::InputHandler;
    use serde_json::json;

    fn plugin_panel() -> (Scene, PanelId) {
        let mut scene = Scene::default();
        let kind = PanelKind::Plugin {
            plugin_id: "hello-world".to_string(),
            plugin_version: None,
            settings: json!({}),
        };
        let id = scene.add(kind, Rect::new(0.0, 0.0, 320.0, 200.0));
        (scene, id)
    }

    fn manifest(json: &str) -> InstalledPlugin {
        InstalledPlugin {
            manifest: calm_plugins::PluginManifest::from_json(json).unwrap(),
            dir: "/tmp/plugin".into(),
        }
    }

    /// A web plugin, a locked web plugin and a TUI app.
    fn plugins() -> Vec<InstalledPlugin> {
        vec![
            manifest(
                r#"{"id":"hello-world","name":"Hello World","version":"0.2.0","entry":"index.html",
                    "defaultSize":{"width":320,"height":200},"maxSize":{"width":400,"height":300}}"#,
            ),
            manifest(
                r#"{"id":"locked","name":"Locked","version":"1.0.0","entry":"index.html",
                    "defaultSize":{"width":300,"height":160},"resizable":false}"#,
            ),
            manifest(
                r#"{"id":"top","name":"Top","version":"0.1.0","kind":"tui",
                    "defaultSize":{"width":640,"height":400},"command":"top"}"#,
            ),
        ]
    }

    fn host<'a>(
        scene: &'a mut Scene,
        caller: PanelId,
        plugins: &'a [InstalledPlugin],
    ) -> HostContext<'a> {
        HostContext {
            scene,
            caller,
            focused: None,
            viewport: Size::new(1000.0, 800.0),
            background: "#102030".to_string(),
            default_terminal_size: Size::new(640.0, 400.0),
            plugins,
        }
    }

    fn call(id: u64, method: &str, params: Value) -> BridgeRequest {
        BridgeRequest {
            id,
            method: method.to_string(),
            params,
        }
    }

    /// Routes `request` from `caller` with the test plugins installed.
    fn run(scene: &mut Scene, caller: PanelId, request: &BridgeRequest) -> Option<Route> {
        let plugins = plugins();
        route(&mut host(scene, caller, &plugins), request)
    }

    fn ok(id: u64, value: Value) -> Option<Route> {
        Some(Route::Reply(Some(BridgeResponse::Ok(id, value))))
    }

    fn err(id: u64, error: ApiError) -> Option<Route> {
        Some(Route::Reply(Some(BridgeResponse::Err(id, error))))
    }

    fn error_code(route: Option<Route>) -> (ErrorCode, String) {
        match route {
            Some(Route::Reply(Some(BridgeResponse::Err(_, error)))) => (error.code, error.message),
            other => panic!("expected an error reply, got {other:?}"),
        }
    }

    /// Methods that may change the scene. Every other one must leave its revision alone.
    const SCENE_WRITERS: &[&str] = &[
        "panel.setTitle",
        "panel.minimize",
        "settings.set",
        "canvas.createPanel",
        "canvas.focusPanel",
    ];

    /// Good params for every method, so the revision test runs the real path, not a reject.
    fn good_params(method: &str, other: PanelId) -> Value {
        match method {
            "log" => json!({ "message": "hi" }),
            "panel.setTitle" => json!({ "title": "x" }),
            "settings.set" => json!({ "a": 1 }),
            "storage.get" | "storage.remove" => json!({ "key": "k" }),
            "storage.set" => json!({ "key": "k", "value": 1 }),
            "github.api" => json!({ "path": "user" }),
            "openExternal" => json!({ "url": "https://example.com" }),
            "process.exec" => json!({ "command": "/bin/echo" }),
            "canvas.focusPanel" => json!({ "id": other.0 }),
            "canvas.createPanel" => json!({ "spec": { "kind": "terminal" } }),
            _ => Value::Null,
        }
    }

    #[test]
    fn read_only_bridge_calls_leave_the_scene_revision_alone() {
        let (mut scene, id) = plugin_panel();
        let other = scene.add(
            PanelKind::Terminal {
                command: None,
                cwd: None,
            },
            Rect::new(400.0, 0.0, 320.0, 200.0),
        );
        // Behind the caller, so focusing it has something to raise.
        scene.send_to_back(other);
        for (n, method) in calm_plugins::BRIDGE_METHODS.iter().enumerate() {
            let before = scene.sync_revision();
            let request = call(n as u64, method, good_params(method, other));
            let routed = run(&mut scene, id, &request);
            assert!(routed.is_some(), "{method}");
            let moved = scene.sync_revision() != before;
            if SCENE_WRITERS.contains(method) {
                // Positive control: the writers really change something here.
                assert!(moved, "{method} should change the scene: {routed:?}");
            } else {
                assert!(!moved, "{method} bumped the revision: {routed:?}");
            }
        }
        assert_eq!(
            run(&mut scene, PanelId(99), &call(1, "theme.get", Value::Null)),
            None
        );
    }

    #[test]
    fn set_title_renames_the_panel() {
        let (mut scene, id) = plugin_panel();
        let reply = run(
            &mut scene,
            id,
            &call(1, "panel.setTitle", json!({ "title": "Hello" })),
        );
        assert_eq!(reply, ok(1, Value::Null));
        assert_eq!(scene.get(id).unwrap().title, "Hello");
        let bad = run(
            &mut scene,
            id,
            &call(2, "panel.setTitle", json!({ "title": 3 })),
        );
        assert_eq!(
            bad,
            err(2, ApiError::invalid_params("title", "must be a string"))
        );
        assert_eq!(scene.get(id).unwrap().title, "Hello");
    }

    #[test]
    fn settings_round_trip_through_the_panel_kind() {
        let (mut scene, id) = plugin_panel();
        let value = json!({ "repos": ["owner/name"] });
        assert_eq!(
            run(&mut scene, id, &call(1, "settings.set", value.clone())),
            ok(1, Value::Null)
        );
        assert_eq!(
            run(&mut scene, id, &call(2, "settings.get", Value::Null)),
            ok(2, value.clone())
        );
        assert_eq!(
            run(&mut scene, id, &call(3, "settings.set", json!([1]))),
            err(3, ApiError::invalid_params("value", "must be an object"))
        );
        assert_eq!(
            run(&mut scene, id, &call(4, "settings.get", Value::Null)),
            ok(4, value)
        );
    }

    #[test]
    fn settings_on_a_non_plugin_panel_is_wrong_kind() {
        let mut scene = Scene::default();
        let id = scene.add(
            PanelKind::Terminal {
                command: None,
                cwd: None,
            },
            Rect::new(0.0, 0.0, 320.0, 200.0),
        );
        for method in ["settings.get", "settings.set"] {
            assert_eq!(
                run(&mut scene, id, &call(1, method, json!({}))),
                err(1, ApiError::wrong_kind(id, "plugin")),
                "{method}"
            );
        }
    }

    #[test]
    fn every_bridge_method_is_served_and_unknown_ones_reject() {
        let (mut scene, id) = plugin_panel();
        assert_eq!(
            run(&mut scene, id, &call(1, "log", json!({ "message": "hi" }))),
            Some(Route::Reply(None))
        );
        for (n, method) in calm_plugins::BRIDGE_METHODS.iter().enumerate() {
            let routed = run(&mut scene, id, &call(n as u64, method, Value::Null));
            assert!(
                !matches!(
                    &routed,
                    Some(Route::Reply(Some(BridgeResponse::Err(_, e)))) if e.code == ErrorCode::NotImplemented
                ),
                "{method} is still not implemented"
            );
        }
        assert_eq!(
            run(&mut scene, id, &call(99, "nope", Value::Null)),
            err(
                99,
                ApiError::new(ErrorCode::UnknownMethod, "unknown method: nope")
            )
        );
    }

    // `calm.has` reads this list, so it must match what `route` really answers.
    #[test]
    fn served_methods_are_bridge_methods_and_are_served() {
        let (mut scene, id) = plugin_panel();
        for method in served_methods() {
            assert!(calm_plugins::is_known_method(method), "{method}");
        }
        for (n, method) in calm_plugins::BRIDGE_METHODS.iter().enumerate() {
            let not_served = match run(&mut scene, id, &call(n as u64, method, Value::Null)) {
                Some(Route::Reply(Some(BridgeResponse::Err(_, error)))) => {
                    error.code == ErrorCode::NotImplemented
                }
                _ => false,
            };
            assert_eq!(
                served_methods().contains(method),
                !not_served,
                "{method}: served_methods disagrees with route"
            );
        }
    }

    #[test]
    fn get_bounds_reports_size_and_zoom() {
        let (mut scene, id) = plugin_panel();
        scene.camera.zoom = 1.5;
        assert_eq!(
            run(&mut scene, id, &call(1, "panel.getBounds", Value::Null)),
            ok(1, json!({ "width": 320.0, "height": 200.0, "zoom": 1.5 }))
        );
        // A bad param: the caller is gone.
        assert_eq!(
            run(
                &mut scene,
                PanelId(42),
                &call(2, "panel.getBounds", Value::Null)
            ),
            None
        );
    }

    #[test]
    fn minimize_keeps_the_panel_and_repeats_harmlessly() {
        let (mut scene, id) = plugin_panel();
        for n in 1..=2 {
            assert_eq!(
                run(&mut scene, id, &call(n, "panel.minimize", Value::Null)),
                ok(n, Value::Null)
            );
            let panel = scene.get(id).expect("the page keeps running");
            assert!(panel.minimized, "call {n}");
        }
        let before = scene.sync_revision();
        run(
            &mut scene,
            id,
            &call(3, "panel.minimize", json!({ "x": 1 })),
        );
        assert_eq!(
            scene.sync_revision(),
            before,
            "a second minimize changed nothing"
        );
    }

    #[test]
    fn theme_get_answers_the_base_color() {
        let (mut scene, id) = plugin_panel();
        assert_eq!(
            run(&mut scene, id, &call(1, "theme.get", Value::Null)),
            ok(1, json!({ "background": "#102030" }))
        );
        assert_eq!(
            run(
                &mut scene,
                PanelId(7),
                &call(2, "theme.get", json!({ "junk": true }))
            ),
            None
        );
    }

    #[test]
    fn list_panels_covers_every_panel_back_to_front() {
        let (mut scene, plugin) = plugin_panel();
        let browser = scene.add(
            PanelKind::Browser {
                url: "https://example.com/".into(),
                page_zoom: None,
            },
            Rect::new(400.0, 10.0, 500.0, 300.0),
        );
        let tui = scene.add(
            PanelKind::Tui {
                plugin_id: "top".into(),
                plugin_version: None,
            },
            Rect::new(0.0, 400.0, 640.0, 400.0),
        );
        scene.get_mut(tui).unwrap().minimized = true;
        let plugins = plugins();
        let mut context = host(&mut scene, plugin, &plugins);
        context.focused = Some(browser);
        let Some(Route::Reply(Some(BridgeResponse::Ok(1, list)))) =
            route(&mut context, &call(1, "canvas.listPanels", Value::Null))
        else {
            panic!("listPanels did not answer");
        };
        assert_eq!(
            list[0],
            json!({
                "id": plugin.0, "kind": "plugin", "title": "hello-world",
                "rect": { "x": 0.0, "y": 0.0, "width": 320.0, "height": 200.0 },
                "minimized": false, "z": 0, "focused": false, "pluginId": "hello-world",
                "resize": { "resizable": true, "minSize": { "width": 120.0, "height": 80.0 }, "maxSize": null },
            })
        );
        assert_eq!(list[1]["url"], json!("https://example.com/"));
        assert_eq!(list[1]["focused"], json!(true));
        assert_eq!(list[1]["z"], json!(1));
        assert_eq!(list[2]["kind"], json!("tui"));
        assert_eq!(list[2]["minimized"], json!(true));
        assert_eq!(list.as_array().unwrap().len(), 3);
        // A bad param: the caller is gone.
        assert_eq!(
            run(
                &mut scene,
                PanelId(99),
                &call(2, "canvas.listPanels", Value::Null)
            ),
            None
        );
    }

    #[test]
    fn focus_panel_restores_raises_and_focuses() {
        let (mut scene, caller) = plugin_panel();
        let target = scene.add(
            PanelKind::Terminal {
                command: None,
                cwd: None,
            },
            Rect::new(400.0, 0.0, 320.0, 200.0),
        );
        scene.get_mut(target).unwrap().minimized = true;
        scene.send_to_back(target);
        assert_eq!(
            run(
                &mut scene,
                caller,
                &call(1, "canvas.focusPanel", json!({ "id": target.0 }))
            ),
            Some(Route::Apply {
                reply: BridgeResponse::Ok(1, Value::Null),
                effects: vec![HostEffect::Focus(target)],
            })
        );
        assert!(!scene.get(target).unwrap().minimized);
        assert_eq!(scene.panels.last().unwrap().id, target);

        assert_eq!(
            run(
                &mut scene,
                caller,
                &call(2, "canvas.focusPanel", json!({ "id": 99 }))
            ),
            err(2, ApiError::not_found(PanelId(99)))
        );
        assert_eq!(
            run(
                &mut scene,
                caller,
                &call(3, "canvas.focusPanel", json!({ "id": "1" }))
            ),
            err(3, ApiError::invalid_params("id", "must be a panel id"))
        );
    }

    fn create(scene: &mut Scene, caller: PanelId, spec: Value) -> Option<Route> {
        run(
            scene,
            caller,
            &call(1, "canvas.createPanel", json!({ "spec": spec })),
        )
    }

    /// The new panel's id and effects from a successful create.
    fn created(route: Option<Route>) -> (PanelId, Vec<HostEffect>) {
        match route {
            Some(Route::Apply {
                reply: BridgeResponse::Ok(_, id),
                effects,
            }) => (PanelId(id.as_u64().expect("id")), effects),
            other => panic!("expected a created panel, got {other:?}"),
        }
    }

    #[test]
    fn create_panel_adds_each_kind_and_opens_it() {
        let (mut scene, caller) = plugin_panel();
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().to_str().unwrap().to_string();
        let (terminal, effects) = created(create(
            &mut scene,
            caller,
            json!({ "kind": "terminal", "command": "htop", "cwd": cwd,
                    "rect": { "x": 10, "y": 20, "width": 500, "height": 300 } }),
        ));
        let kind = PanelKind::Terminal {
            command: Some("htop".into()),
            cwd: Some(cwd.into()),
        };
        assert_eq!(
            effects,
            vec![
                HostEffect::Open {
                    id: terminal,
                    kind: kind.clone()
                },
                HostEffect::Focus(terminal)
            ]
        );
        let panel = scene.get(terminal).unwrap();
        assert_eq!(
            (panel.kind.clone(), panel.rect),
            (kind, Rect::new(10.0, 20.0, 500.0, 300.0))
        );

        // No rect: the default size at the viewport center, which is world (500, 400) here.
        let (browser, effects) = created(create(
            &mut scene,
            caller,
            json!({ "kind": "browser", "url": "example.com", "focus": false }),
        ));
        assert_eq!(effects.len(), 1, "focus false opens without focusing");
        let panel = scene.get(browser).unwrap();
        assert_eq!(panel.rect, Rect::new(100.0, 100.0, 800.0, 600.0));
        assert!(
            matches!(&panel.kind, PanelKind::Browser { url, .. } if url == "https://example.com")
        );

        let (plugin, _) = created(create(
            &mut scene,
            caller,
            json!({ "kind": "plugin", "pluginId": "hello-world", "settings": { "a": 1 } }),
        ));
        assert_eq!(
            scene.get(plugin).unwrap().kind,
            PanelKind::Plugin {
                plugin_id: "hello-world".into(),
                plugin_version: Some("0.2.0".into()),
                settings: json!({ "a": 1 }),
            }
        );
        let (tui, _) = created(create(
            &mut scene,
            caller,
            json!({ "kind": "tui", "pluginId": "top" }),
        ));
        assert_eq!(scene.get(tui).unwrap().rect.size, Size::new(640.0, 400.0));
    }

    #[test]
    fn create_panel_sizes_go_through_the_resize_policy() {
        let (mut scene, caller) = plugin_panel();
        let big = json!({ "x": 0, "y": 0, "width": 2000, "height": 2000 });
        let (limited, _) = created(create(
            &mut scene,
            caller,
            json!({ "kind": "plugin", "pluginId": "hello-world", "rect": big }),
        ));
        assert_eq!(
            scene.get(limited).unwrap().rect.size,
            Size::new(400.0, 300.0)
        );
        let tiny = json!({ "x": 0, "y": 0, "width": 5, "height": 5 });
        let (terminal, _) = created(create(
            &mut scene,
            caller,
            json!({ "kind": "terminal", "rect": tiny }),
        ));
        assert_eq!(
            scene.get(terminal).unwrap().rect.size,
            Size::new(120.0, 80.0)
        );

        let (locked, _) = created(create(
            &mut scene,
            caller,
            json!({ "kind": "plugin", "pluginId": "locked" }),
        ));
        let panel = scene.get(locked).unwrap();
        assert_eq!(panel.rect.size, Size::new(300.0, 160.0));
        assert!(!panel.resize.resizable);
        let count = scene.panels.len();
        let (code, message) = error_code(create(
            &mut scene,
            caller,
            json!({ "kind": "plugin", "pluginId": "locked", "rect": big }),
        ));
        assert_eq!(code, ErrorCode::InvalidParams);
        assert!(message.contains("locked is not resizable"), "{message}");
        assert_eq!(scene.panels.len(), count, "a rejected create added a panel");
    }

    #[test]
    fn create_panel_rejects_bad_specs_without_adding() {
        let (mut scene, caller) = plugin_panel();
        let cases = [
            (json!(null), ErrorCode::InvalidParams, "spec:"),
            (
                json!({ "kind": "window" }),
                ErrorCode::InvalidParams,
                "spec.kind",
            ),
            (
                json!({ "kind": "browser" }),
                ErrorCode::InvalidParams,
                "spec.url",
            ),
            (
                json!({ "kind": "browser", "url": "  " }),
                ErrorCode::InvalidParams,
                "spec.url",
            ),
            (
                json!({ "kind": "terminal", "cwd": "rel" }),
                ErrorCode::InvalidParams,
                "spec.cwd",
            ),
            (
                json!({ "kind": "terminal", "cmd": "ls" }),
                ErrorCode::InvalidParams,
                "spec.cmd",
            ),
            (
                json!({ "kind": "terminal", "focus": 1 }),
                ErrorCode::InvalidParams,
                "spec.focus",
            ),
            (
                json!({ "kind": "plugin" }),
                ErrorCode::InvalidParams,
                "spec.pluginId",
            ),
            (
                json!({ "kind": "plugin", "pluginId": "top" }),
                ErrorCode::NotInstalled,
                "spec.pluginId",
            ),
            (
                json!({ "kind": "tui", "pluginId": "hello-world" }),
                ErrorCode::NotInstalled,
                "spec.pluginId",
            ),
            (
                json!({ "kind": "plugin", "pluginId": "hello-world", "settings": [1] }),
                ErrorCode::InvalidParams,
                "spec.settings",
            ),
            (
                json!({ "kind": "terminal", "rect": { "x": 0, "y": 0, "width": 0, "height": 10 } }),
                ErrorCode::InvalidParams,
                "spec.rect",
            ),
            (
                json!({ "kind": "terminal", "rect": { "x": "0", "y": 0, "width": 10, "height": 10 } }),
                ErrorCode::InvalidParams,
                "spec.rect.x",
            ),
            (
                json!({ "kind": "terminal", "near": 1 }),
                ErrorCode::Unavailable,
                "spec.near",
            ),
            (
                json!({ "kind": "terminal", "env": {} }),
                ErrorCode::Unavailable,
                "spec.env",
            ),
        ];
        let count = scene.panels.len();
        for (spec, code, field) in cases {
            let (got, message) = error_code(create(&mut scene, caller, spec.clone()));
            assert_eq!(got, code, "{spec}: {message}");
            assert!(message.starts_with(field), "{spec}: {message}");
        }
        assert_eq!(scene.panels.len(), count, "a rejected create added a panel");
    }

    // The invariant: a plugin never removes a panel itself. Its close is a request on the
    // normal path, so a live terminal stays until the user confirms with Enter.
    #[test]
    fn plugin_close_only_requests_the_normal_close_path() {
        let (mut scene, caller) = plugin_panel();
        let routed = run(&mut scene, caller, &call(1, "panel.close", Value::Null));
        let Some(Route::Apply { reply, effects }) = routed else {
            panic!("panel.close did not route to an effect: {routed:?}");
        };
        assert_eq!(reply, BridgeResponse::Ok(1, Value::Null));
        assert!(
            scene.get(caller).is_some(),
            "dispatch removed the panel itself"
        );
        let canvas: Vec<Effect> = effects
            .iter()
            .flat_map(HostEffect::canvas_effects)
            .collect();
        assert_eq!(canvas, vec![Effect::ClosePanelRequested(caller)]);

        // What the canvas does with that request for a panel with a live process.
        let mut input = InputHandler::default();
        let alive = true;
        assert_eq!(
            input.request_close(caller, alive),
            Effect::ConfirmClose(caller)
        );
        assert_eq!(input.confirming, Some(caller));
        assert_eq!(
            input.confirm_key(calm_core::interaction::ConfirmKey::Enter),
            Some(Effect::ClosePanel(caller))
        );
        // A bad param: the caller is gone.
        assert_eq!(
            run(
                &mut scene,
                PanelId(77),
                &call(2, "panel.close", Value::Null)
            ),
            None
        );
    }

    #[test]
    fn process_exec_is_a_background_call() {
        let (mut scene, id) = plugin_panel();
        let routed = run(
            &mut scene,
            id,
            &call(
                1,
                "process.exec",
                json!({ "command": "/bin/echo", "args": ["hi"] }),
            ),
        );
        assert!(
            matches!(&routed, Some(Route::Background(ServiceCall::ProcessExec(spec))) if spec.args == ["hi"]),
            "{routed:?}"
        );
        let (code, _) = error_code(run(
            &mut scene,
            id,
            &call(2, "process.exec", json!({ "args": [] })),
        ));
        assert_eq!(code, ErrorCode::InvalidParams);
    }

    /// What the canvas does with a call, run inline: route, then the background service.
    fn serve(
        scene: &mut Scene,
        id: PanelId,
        request: &BridgeRequest,
        services: &crate::plugin_services::Services,
    ) -> Option<BridgeResponse> {
        match run(scene, id, request)? {
            Route::Reply(reply) => reply,
            Route::Apply { reply, .. } => Some(reply),
            Route::Background(call) => Some(service_reply(request.id, services.run(call))),
        }
    }

    #[test]
    fn process_exec_answers_through_the_bridge() {
        let dir = tempfile::tempdir().unwrap();
        let services = gh_services(dir.path(), "exit 1");
        let (mut scene, id) = plugin_panel();
        let request = call(
            4,
            "process.exec",
            json!({ "command": "/bin/echo", "args": ["hi"], "cwd": null, "timeoutMs": null }),
        );
        assert_eq!(
            serve(&mut scene, id, &request, &services),
            Some(BridgeResponse::Ok(
                4,
                json!({ "code": 0, "stdout": "hi\n", "stderr": "" })
            ))
        );
    }

    fn gh_services(dir: &std::path::Path, gh_body: &str) -> crate::plugin_services::Services {
        crate::plugin_services::Services {
            gh: calm_plugins::GhClient::new(fake_program(dir, "gh", gh_body)),
            data_dir: dir.join("plugin-data"),
            opener: dir.join("no-open"),
        }
    }

    use crate::plugin_services::tests::{FAKE_GH_LOCK, fake_program};

    const PULLS: &str = "repos/inkitt/flash/pulls?state=open&per_page=30";

    // Regression: github.api answered "not implemented: github.api" and the GitHub PRs
    // panel showed that instead of pull requests.
    #[test]
    fn github_api_runs_gh_and_answers_with_its_json() {
        let _guard = FAKE_GH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let services = gh_services(
            dir.path(),
            &format!(
                r#"if [ "$1" = api ] && [ "$2" = "{PULLS}" ]; then
  echo '[{{"number":7,"title":"Fix login","draft":false}}]'
else
  echo "unexpected args: $*" >&2; exit 1
fi"#
            ),
        );
        let (mut scene, id) = plugin_panel();
        assert_eq!(
            serve(
                &mut scene,
                id,
                &call(5, "github.api", json!({ "path": PULLS })),
                &services
            ),
            Some(BridgeResponse::Ok(
                5,
                json!([{ "number": 7, "title": "Fix login", "draft": false }])
            ))
        );
    }

    #[test]
    fn github_api_errors_say_what_to_do() {
        let _guard = FAKE_GH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let (mut scene, id) = plugin_panel();
        let scene = &mut scene;
        let request = call(6, "github.api", json!({ "path": PULLS }));
        let error = |services: &crate::plugin_services::Services, scene: &mut Scene| match serve(
            scene, id, &request, services,
        ) {
            Some(BridgeResponse::Err(6, error)) => error,
            other => panic!("expected an error reply, got {other:?}"),
        };

        let mut missing = gh_services(dir.path(), "exit 0");
        missing.gh = calm_plugins::GhClient::new(dir.path().join("no-such-gh"));
        let not_installed = error(&missing, scene);
        assert_eq!(not_installed.code, ErrorCode::GhNotInstalled);
        assert!(not_installed.message.starts_with("gh is not installed"));

        let logged_out = gh_services(
            dir.path(),
            r#"echo "To get started with GitHub CLI, please run:  gh auth login" >&2; exit 4"#,
        );
        let logged_out = error(&logged_out, scene);
        assert_eq!(logged_out.code, ErrorCode::GhNotLoggedIn);
        assert!(
            logged_out
                .message
                .starts_with("gh is not logged in, run gh auth login")
        );

        let not_found = gh_services(dir.path(), r#"echo "gh: Not Found (HTTP 404)" >&2; exit 1"#);
        assert_eq!(
            error(&not_found, scene),
            ApiError::new(
                ErrorCode::GhFailed,
                "gh api failed: gh: Not Found (HTTP 404)"
            )
        );

        let bad_path = call(7, "github.api", json!({ "path": "--paginate" }));
        assert!(matches!(
            serve(scene, id, &bad_path, &not_found),
            Some(BridgeResponse::Err(7, e))
                if e.code == ErrorCode::GhInvalidPath && e.message.starts_with("invalid gh api path")
        ));
    }

    #[test]
    fn github_api_error_reply_never_carries_the_token() {
        let _guard = FAKE_GH_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let dir = tempfile::tempdir().unwrap();
        let token = "fake-token-for-tests";
        let mut services = gh_services(
            dir.path(),
            r#"echo "gh: Bad credentials for $GH_TOKEN (HTTP 401)" >&2; exit 1"#,
        );
        services.gh.token = calm_plugins::GhToken::new(token);
        let (mut scene, id) = plugin_panel();

        let reply = serve(
            &mut scene,
            id,
            &call(8, "github.api", json!({ "path": "user" })),
            &services,
        );

        let Some(BridgeResponse::Err(8, error)) = &reply else {
            panic!("expected an error reply, got {reply:?}");
        };
        // GhClient reads "Bad credentials" as not logged in.
        assert_eq!(error.code, ErrorCode::GhNotLoggedIn);
        let message = &error.message;
        // Positive control: the fake printed the token, and the reply has the marker instead.
        assert!(message.contains("<redacted>"), "{message}");
        assert!(!message.contains(token));
        assert!(!reply.unwrap().to_js().contains(token));
        assert!(!format!("{services:?}").contains(token));
    }

    #[test]
    fn storage_is_served_for_the_panel_plugin() {
        let dir = tempfile::tempdir().unwrap();
        let services = gh_services(dir.path(), "exit 1");
        let (mut scene, id) = plugin_panel();
        let set = call(1, "storage.set", json!({ "key": "k", "value": [1, 2] }));
        assert_eq!(
            serve(&mut scene, id, &set, &services),
            Some(BridgeResponse::Ok(1, Value::Null))
        );
        let get = call(2, "storage.get", json!({ "key": "k" }));
        assert_eq!(
            serve(&mut scene, id, &get, &services),
            Some(BridgeResponse::Ok(2, json!([1, 2])))
        );
        assert!(dir.path().join("plugin-data/hello-world.json").exists());
    }

    #[test]
    fn plugin_problems_count_errors_and_warnings() {
        assert_eq!(plugin_problems_text(0, 0), "");
        assert_eq!(plugin_problems_text(2, 0), "⚠ plugins: 2 errors");
        assert_eq!(plugin_problems_text(0, 1), "⚠ plugins: 1 warnings");
        assert_eq!(
            plugin_problems_text(1, 3),
            "⚠ plugins: 1 errors, 3 warnings"
        );
    }

    #[test]
    fn error_page_is_an_escaped_data_url() {
        let url = error_page_url("A<b>", "x & y");
        assert!(url.starts_with("data:text/html;charset=utf-8,"));
        let body = url.split_once(',').unwrap().1;
        assert!(
            body.bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-_.~%".contains(&b))
        );
        assert!(body.contains("A%26lt%3Bb%26gt%3B"), "{body}");
        assert!(body.contains("x%20%26amp%3B%20y"), "{body}");
    }

    #[test]
    fn menu_entries_use_manifest_name_and_size() {
        let manifest = calm_plugins::PluginManifest::from_json(
            r#"{"id":"hello-world","name":"Hello World","version":"0.1.0",
                "entry":"index.html","defaultSize":{"width":320,"height":200}}"#,
        )
        .unwrap();
        let plugins = vec![InstalledPlugin {
            manifest,
            dir: "/tmp/hello-world".into(),
        }];
        let entries = menu_entries(&plugins);
        assert_eq!(
            entries.web,
            vec![("hello-world".into(), "Hello World".into())]
        );
        assert_eq!(entries.apps, vec![]);
        assert_eq!(
            entries.sizes,
            vec![("hello-world".into(), Size::new(320.0, 200.0))]
        );
    }
}
