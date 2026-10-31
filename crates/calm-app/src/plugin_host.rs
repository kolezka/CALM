//! Plugin panels: discovery at startup, one WKWebView per panel, and the host side of the
//! `window.calm` bridge for the few methods served so far.

use calm_core::geometry::Size;
use calm_core::preview::PanelMode;
use calm_core::scene::{Panel, PanelId, PanelKind};
use calm_core::viewport::NativePlacement;
use calm_native::{NativeChild, WebPanel};
use calm_plugins::{
    BridgeRequest, BridgeResponse, CALM_PRELUDE_JS, Discovery, InstalledPlugin, discover_search,
    is_known_method, missing_scripts, plugin_search,
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
    if discovery.plugins.is_empty() {
        eprintln!("calm: no plugins found");
    }
    (discovery, searched)
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

    pub fn stack_index(&self) -> Option<usize> {
        self.host.stack_index()
    }

    pub fn is_loading(&self) -> bool {
        self.web.is_loading()
    }

    pub fn reply(&mut self, response: &BridgeResponse) {
        self.web.eval_js(&response.to_js());
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

/// Serves one bridge call against its panel. `None` means no reply: `log` is fire and
/// forget in the prelude. Methods the host does not serve yet reject, never hang.
pub fn dispatch(panel: &mut Panel, request: &BridgeRequest) -> Option<BridgeResponse> {
    let id = request.id;
    let params = &request.params;
    let response = match request.method.as_str() {
        "log" => {
            let message = params.get("message").and_then(Value::as_str).unwrap_or("");
            eprintln!("calm: plugin panel {}: {message}", panel.id.0);
            return None;
        }
        "panel.setTitle" => match params.get("title").and_then(Value::as_str) {
            Some(title) => {
                panel.title = title.to_string();
                BridgeResponse::Ok(id, Value::Null)
            }
            None => BridgeResponse::Err(id, "panel.setTitle: title must be a string".into()),
        },
        "settings.get" => match &panel.kind {
            PanelKind::Plugin { settings, .. } => BridgeResponse::Ok(id, settings.clone()),
            _ => BridgeResponse::Err(id, "settings.get: not a plugin panel".into()),
        },
        "settings.set" => match &mut panel.kind {
            PanelKind::Plugin { settings, .. } if params.is_object() => {
                *settings = params.clone();
                BridgeResponse::Ok(id, Value::Null)
            }
            PanelKind::Plugin { .. } => {
                BridgeResponse::Err(id, "settings.set: value must be an object".into())
            }
            _ => BridgeResponse::Err(id, "settings.set: not a plugin panel".into()),
        },
        method if is_known_method(method) => BridgeResponse::not_implemented(id, method),
        method => BridgeResponse::Err(id, format!("unknown method: {method}")),
    };
    Some(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use calm_core::geometry::Rect;
    use calm_core::scene::Scene;
    use serde_json::json;

    fn plugin_panel() -> (Scene, PanelId) {
        let mut scene = Scene::default();
        let kind = PanelKind::Plugin {
            plugin_id: "hello-world".to_string(),
            settings: json!({}),
        };
        let id = scene.add(kind, Rect::new(0.0, 0.0, 320.0, 200.0));
        (scene, id)
    }

    fn call(id: u64, method: &str, params: Value) -> BridgeRequest {
        BridgeRequest {
            id,
            method: method.to_string(),
            params,
        }
    }

    #[test]
    fn set_title_renames_the_panel() {
        let (mut scene, id) = plugin_panel();
        let panel = scene.get_mut(id).unwrap();
        let reply = dispatch(
            panel,
            &call(1, "panel.setTitle", json!({ "title": "Hello" })),
        );
        assert_eq!(reply, Some(BridgeResponse::Ok(1, Value::Null)));
        assert_eq!(panel.title, "Hello");
        let bad = dispatch(panel, &call(2, "panel.setTitle", json!({ "title": 3 })));
        assert!(matches!(bad, Some(BridgeResponse::Err(2, _))));
        assert_eq!(panel.title, "Hello");
    }

    #[test]
    fn settings_round_trip_through_the_panel_kind() {
        let (mut scene, id) = plugin_panel();
        let panel = scene.get_mut(id).unwrap();
        let value = json!({ "repos": ["owner/name"] });
        assert_eq!(
            dispatch(panel, &call(1, "settings.set", value.clone())),
            Some(BridgeResponse::Ok(1, Value::Null))
        );
        assert_eq!(
            dispatch(panel, &call(2, "settings.get", Value::Null)),
            Some(BridgeResponse::Ok(2, value.clone()))
        );
        let rejected = dispatch(panel, &call(3, "settings.set", json!([1])));
        assert!(matches!(rejected, Some(BridgeResponse::Err(3, _))));
        assert_eq!(
            dispatch(panel, &call(4, "settings.get", Value::Null)),
            Some(BridgeResponse::Ok(4, value))
        );
    }

    #[test]
    fn every_other_method_rejects_instead_of_hanging() {
        let (mut scene, id) = plugin_panel();
        let panel = scene.get_mut(id).unwrap();
        assert_eq!(
            dispatch(panel, &call(1, "log", json!({ "message": "hi" }))),
            None
        );
        let served = ["log", "panel.setTitle", "settings.get", "settings.set"];
        for (n, method) in calm_plugins::BRIDGE_METHODS.iter().enumerate() {
            if served.contains(method) {
                continue;
            }
            let n = n as u64;
            assert_eq!(
                dispatch(panel, &call(n, method, Value::Null)),
                Some(BridgeResponse::not_implemented(n, method)),
                "{method}"
            );
        }
        assert_eq!(
            dispatch(panel, &call(99, "nope", Value::Null)),
            Some(BridgeResponse::Err(99, "unknown method: nope".into()))
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
