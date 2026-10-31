use serde::{Deserialize, Serialize};
use serde_json::Value;

/// One call from plugin JS, posted as `{ id, method, params }`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BridgeRequest {
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

impl BridgeRequest {
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

/// Every request `method` the prelude can send. The single source of truth for the host
/// dispatcher, the prelude and `packages/calm-plugin-sdk/calm.d.ts` (tests keep them in sync).
pub const BRIDGE_METHODS: &[&str] = &[
    "settings.get",
    "settings.set",
    "storage.get",
    "storage.set",
    "storage.remove",
    "panel.setTitle",
    "panel.getBounds",
    "panel.minimize",
    "panel.close",
    "canvas.createPanel",
    "canvas.focusPanel",
    "canvas.listPanels",
    "process.exec",
    "github.api",
    "openExternal",
    "theme.get",
    "log",
];

pub fn is_known_method(method: &str) -> bool {
    BRIDGE_METHODS.contains(&method)
}

/// JSON text that is also safe inside a JS source string: JSON allows U+2028 and U+2029
/// raw, older JS string literals do not.
fn js_literal(value: &Value) -> String {
    value
        .to_string()
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

/// A host to plugin event, delivered to handlers registered with `calm.events.on`.
#[derive(Debug, Clone, PartialEq)]
pub struct BridgeEvent {
    pub event: String,
    pub payload: Value,
}

impl BridgeEvent {
    /// The script the host evaluates in the page to fan the event out to its handlers.
    pub fn to_js(&self) -> String {
        let event = js_literal(&Value::from(self.event.as_str()));
        let payload = js_literal(&self.payload);
        format!("window.__calm.emit({event}, {payload})")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum BridgeResponse {
    Ok(u64, Value),
    Err(u64, String),
}

impl BridgeResponse {
    /// The answer for a method the host does not serve yet. Rejects instead of hanging.
    pub fn not_implemented(id: u64, method: &str) -> Self {
        BridgeResponse::Err(id, format!("not implemented: {method}"))
    }

    /// The script the host evaluates in the page to settle the call's Promise.
    pub fn to_js(&self) -> String {
        let (id, ok, value) = match self {
            BridgeResponse::Ok(id, value) => (id, true, js_literal(value)),
            BridgeResponse::Err(id, message) => {
                (id, false, js_literal(&Value::from(message.as_str())))
            }
        };
        format!("window.__calm.resolve({id}, {ok}, {value})")
    }
}

/// Injected at document start in every plugin page. Defines `window.calm`.
pub const CALM_PRELUDE_JS: &str = r#"(function () {
  if (window.calm) return;
  var nextId = 1;
  var pending = new Map();
  var handlers = new Map();
  var EVENTS = ["focus", "blur", "resize", "visibility", "settingsChanged", "themeChanged"];

  function call(request) {
    return new Promise(function (resolve, reject) {
      var id = nextId++;
      pending.set(id, { resolve: resolve, reject: reject });
      try {
        window.webkit.messageHandlers.calm.postMessage({
          id: id,
          method: request.method,
          params: request.params === undefined ? null : request.params
        });
      } catch (err) {
        pending.delete(id);
        reject(err);
      }
    });
  }

  // Fire and forget: no pending entry, so the host never has to answer a log.
  // A failed log has nowhere better to go, so its error is dropped.
  function log(message) {
    try {
      window.webkit.messageHandlers.calm.postMessage({
        id: nextId++,
        method: "log",
        params: { message: String(message) }
      });
    } catch (err) {}
  }

  // Formatting a thrown value can throw too (a stack getter or toString that throws).
  function describe(err) {
    try { return String((err && err.stack) || err); } catch (e) {}
    try { return String(err); } catch (e) {}
    return "a value that cannot be printed";
  }

  window.__calm = Object.freeze({
    resolve: function (id, ok, value) {
      var entry = pending.get(id);
      if (!entry) return;
      pending.delete(id);
      if (ok) entry.resolve(value);
      else entry.reject(new Error(String(value)));
    },
    // Called by the host. Never throws back into it: a failing handler is logged.
    emit: function (event, payload) {
      var set = handlers.get(event);
      if (!set) return;
      Array.from(set).forEach(function (handler) {
        try {
          handler(payload);
        } catch (err) {
          log("calm.events handler for " + event + " threw: " + describe(err));
        }
      });
    }
  });

  window.calm = Object.freeze({
    settings: Object.freeze({
      get: function () { return call({ method: "settings.get", params: null }); },
      set: function (value) { return call({ method: "settings.set", params: value }); }
    }),
    storage: Object.freeze({
      get: function (key) { return call({ method: "storage.get", params: { key: String(key) } }); },
      set: function (key, value) {
        return call({ method: "storage.set", params: { key: String(key), value: value } });
      },
      remove: function (key) { return call({ method: "storage.remove", params: { key: String(key) } }); }
    }),
    panel: Object.freeze({
      setTitle: function (title) { return call({ method: "panel.setTitle", params: { title: String(title) } }); },
      getBounds: function () { return call({ method: "panel.getBounds", params: null }); },
      minimize: function () { return call({ method: "panel.minimize", params: null }); },
      close: function () { return call({ method: "panel.close", params: null }); }
    }),
    canvas: Object.freeze({
      createPanel: function (spec) { return call({ method: "canvas.createPanel", params: { spec: spec } }); },
      focusPanel: function (id) { return call({ method: "canvas.focusPanel", params: { id: id } }); },
      listPanels: function () { return call({ method: "canvas.listPanels", params: null }); }
    }),
    process: Object.freeze({
      exec: function (command, args, options) {
        var opts = options || {};
        return call({ method: "process.exec", params: {
          command: String(command),
          args: Array.prototype.map.call(args || [], String),
          cwd: opts.cwd === undefined ? null : String(opts.cwd),
          timeoutMs: opts.timeoutMs === undefined ? null : Number(opts.timeoutMs)
        } });
      }
    }),
    github: Object.freeze({
      api: function (path) { return call({ method: "github.api", params: { path: String(path) } }); }
    }),
    theme: Object.freeze({
      get: function () { return call({ method: "theme.get", params: null }); }
    }),
    events: Object.freeze({
      on: function (event, handler) {
        if (EVENTS.indexOf(event) < 0) throw new TypeError("unknown calm event: " + event);
        if (typeof handler !== "function") throw new TypeError("calm.events.on needs a function");
        var set = handlers.get(event);
        if (!set) handlers.set(event, (set = new Set()));
        var entry = function (payload) { handler(payload); };
        set.add(entry);
        return function () { set.delete(entry); };
      }
    }),
    openExternal: function (url) { return call({ method: "openExternal", params: { url: String(url) } }); },
    log: log
  });
})();
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn bridge_request_parses_from_json() {
        let request =
            BridgeRequest::from_json(r#"{"id":7,"method":"github.api","params":{"path":"user"}}"#)
                .unwrap();
        assert_eq!(
            request,
            BridgeRequest {
                id: 7,
                method: "github.api".into(),
                params: json!({ "path": "user" }),
            }
        );

        let no_params = BridgeRequest::from_json(r#"{"id":1,"method":"settings.get"}"#).unwrap();
        assert_eq!(no_params.params, Value::Null);

        assert!(BridgeRequest::from_json(r#"{"method":"log"}"#).is_err());
    }

    #[test]
    fn bridge_response_renders_resolve_call() {
        assert_eq!(
            BridgeResponse::Ok(3, json!({ "repos": ["a/b"] })).to_js(),
            r#"window.__calm.resolve(3, true, {"repos":["a/b"]})"#
        );
        assert_eq!(
            BridgeResponse::Err(4, "gh is \"not\" installed".into()).to_js(),
            r#"window.__calm.resolve(4, false, "gh is \"not\" installed")"#
        );
        // Line separators are legal in JSON strings but not in older JS string literals.
        assert_eq!(
            BridgeResponse::Ok(5, json!("a\u{2028}b")).to_js(),
            "window.__calm.resolve(5, true, \"a\\u2028b\")"
        );
    }

    /// The contract from the plugin SDK, in declaration order.
    const CONTRACT: &[&str] = &[
        "settings.get",
        "settings.set",
        "storage.get",
        "storage.set",
        "storage.remove",
        "panel.setTitle",
        "panel.getBounds",
        "panel.minimize",
        "panel.close",
        "canvas.createPanel",
        "canvas.focusPanel",
        "canvas.listPanels",
        "process.exec",
        "github.api",
        "openExternal",
        "theme.get",
        "log",
    ];

    /// Declared in the d.ts but served by the prelude itself, not by the host.
    const PRELUDE_ONLY: &[&str] = &["events.on"];

    #[test]
    fn bridge_methods_match_contract() {
        assert_eq!(BRIDGE_METHODS, CONTRACT);
    }

    /// Every `method: "<name>"` literal in the prelude, in order.
    fn prelude_method_literals() -> Vec<&'static str> {
        CALM_PRELUDE_JS
            .split("method: \"")
            .skip(1)
            .filter_map(|rest| rest.split('"').next())
            .collect()
    }

    #[test]
    fn prelude_exposes_every_bridge_method() {
        for needle in [
            "window.webkit.messageHandlers.calm.postMessage",
            "window.__calm",
        ] {
            assert!(CALM_PRELUDE_JS.contains(needle), "prelude lacks {needle}");
        }
        let literals = prelude_method_literals();
        for method in BRIDGE_METHODS {
            assert!(
                literals.contains(method),
                "prelude lacks method: \"{method}\""
            );
        }
        for literal in &literals {
            assert!(
                is_known_method(literal),
                "prelude calls {literal}, which is not in BRIDGE_METHODS"
            );
        }

        // Each property must post its own method, not just some method from the list.
        let harness = format!(
            r#"var methods = {methods};
var sent = {{}};
methods.forEach(function (method) {{
  var parts = method.split(".");
  var fn = parts.length === 1 ? window.calm[parts[0]] : (window.calm[parts[0]] || {{}})[parts[1]];
  if (typeof fn !== "function") {{ sent[method] = "not a function"; return; }}
  var before = posted.length;
  try {{ fn("x", ["y"]); }} catch (err) {{ sent[method] = "threw " + err; return; }}
  sent[method] = posted.slice(before).map(function (m) {{ return m.method; }});
}});
console.log(JSON.stringify(sent));"#,
            methods = json!(BRIDGE_METHODS)
        );
        let Some(sent) = run_prelude_under_bun("methods", &harness) else {
            return;
        };
        for method in BRIDGE_METHODS {
            assert_eq!(
                sent[*method],
                json!([method]),
                "calm.{method} posted the wrong method"
            );
        }
    }

    /// Stands in for WKWebView: records every posted message, or throws when `failPosts` is set.
    const FAKE_WEBKIT: &str = r#"var posted = [];
var failPosts = false;
globalThis.window = { webkit: { messageHandlers: { calm: { postMessage: function (message) {
  if (failPosts) throw new Error("post failed");
  posted.push(message);
} } } } };
"#;

    /// Set CALM_SKIP_BUN_TESTS=1 to explicitly skip these Bun-backed runtime tests.
    fn run_prelude_under_bun(name: &str, harness: &str) -> Option<Value> {
        use std::io::ErrorKind;
        use std::process::Command;

        if std::env::var("CALM_SKIP_BUN_TESTS").as_deref() == Ok("1") {
            return None;
        }

        let version = Command::new("bun")
            .arg("--version")
            .output()
            .unwrap_or_else(|error| {
                if error.kind() == ErrorKind::NotFound {
                    panic!("bun is required for {name}; install bun or set CALM_SKIP_BUN_TESTS=1 to explicitly skip: {error}");
                }
                panic!("failed to run bun --version for {name}: {error}");
            });
        assert!(
            version.status.success(),
            "bun --version failed for {name} ({}): {}{}",
            version.status,
            String::from_utf8_lossy(&version.stdout),
            String::from_utf8_lossy(&version.stderr)
        );

        let dir = std::env::temp_dir().join(format!("calm-prelude-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let script = dir.join("probe.js");
        std::fs::write(
            &script,
            format!("{FAKE_WEBKIT}{CALM_PRELUDE_JS}{harness}\n"),
        )
        .unwrap();
        let output = Command::new("bun")
            .arg(&script)
            .current_dir(&dir)
            .output()
            .unwrap_or_else(|error| panic!("failed to run bun for {name}: {error}"));
        std::fs::remove_dir_all(&dir).unwrap();
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success(),
            "bun failed for {name} ({}): {}\n{stdout}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        let last = stdout.lines().last().expect("bun printed nothing");
        Some(serde_json::from_str(last).unwrap())
    }

    #[test]
    fn emit_isolates_throwing_handlers() {
        let harness = r#"var ran = [];
window.calm.events.on("resize", function () { ran.push("error"); throw new Error("boom"); });
window.calm.events.on("resize", function () {
  ran.push("bad-stack");
  throw { get stack() { throw new Error("bad-stack"); } };
});
window.calm.events.on("resize", function () {
  ran.push("unprintable");
  throw { get stack() { throw new Error("s"); }, toString: function () { throw new Error("t"); } };
});
window.calm.events.on("resize", function () { ran.push("later"); });
function emit() {
  try { window.__calm.emit("resize", { width: 1, height: 2, zoom: 1 }); return null; }
  catch (err) { return "emit threw " + (err && err.message); }
}
var first = { emitError: emit(), ran: ran.splice(0) };
var logs = posted.filter(function (m) { return m.method === "log"; })
  .map(function (m) { return m.params.message; });
failPosts = true;
var second = { emitError: emit(), ran: ran.splice(0) };
console.log(JSON.stringify({ first: first, second: second, logs: logs }));"#;
        let Some(result) = run_prelude_under_bun("emit", harness) else {
            return;
        };
        let all = json!(["error", "bad-stack", "unprintable", "later"]);
        for run in ["first", "second"] {
            assert_eq!(result[run]["emitError"], Value::Null, "{run}: {result}");
            assert_eq!(result[run]["ran"], all, "{run}: {result}");
        }
        let logs = result["logs"].as_array().expect("logs array");
        assert_eq!(logs.len(), 3, "one log per throwing handler: {result}");
        assert!(logs[0].as_str().unwrap().contains("boom"), "{result}");
    }

    /// Body between the braces of `interface <name> {`, nested braces included.
    fn interface_body<'a>(src: &'a str, name: &str) -> &'a str {
        let header = format!("interface {name} {{");
        let start = src
            .find(&header)
            .unwrap_or_else(|| panic!("calm.d.ts lacks `{header}`"))
            + header.len();
        let mut depth = 1;
        for (offset, ch) in src[start..].char_indices() {
            match ch {
                '{' => depth += 1,
                '}' => {
                    depth -= 1;
                    if depth == 0 {
                        return &src[start..start + offset];
                    }
                }
                _ => {}
            }
        }
        panic!("interface {name} is not closed");
    }

    /// Members declared one per line: `(name, rest)` where rest starts with `(`, `<` or `:`.
    fn members(body: &str) -> Vec<(&str, &str)> {
        body.lines()
            .map(str::trim)
            .filter_map(|line| {
                let end = line.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))?;
                let (name, rest) = line.split_at(end);
                let first = name.chars().next()?;
                (first.is_ascii_alphabetic()
                    && matches!(rest.chars().next(), Some('(' | '<' | ':')))
                .then_some((name, rest))
            })
            .collect()
    }

    /// Every `ns.fn` and top level `fn` that the d.ts declares on `window.calm`.
    fn declared_calm_functions(src: &str) -> Vec<String> {
        let mut declared = Vec::new();
        for (name, rest) in members(interface_body(src, "Calm")) {
            if let Some(namespace_type) = rest.strip_prefix(':') {
                let namespace_type = namespace_type.trim().trim_end_matches(';').trim();
                for (function, rest) in members(interface_body(src, namespace_type)) {
                    assert!(
                        !rest.starts_with(':'),
                        "{name}.{function} is a property; declare it as a method"
                    );
                    declared.push(format!("{name}.{function}"));
                }
            } else {
                declared.push(name.to_string());
            }
        }
        declared
    }

    #[test]
    fn dts_declares_every_bridge_method() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../packages/calm-plugin-sdk/calm.d.ts");
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("read {}: {err}", path.display()));
        assert!(src.contains("interface Window {\n    calm: Calm;"));
        let declared = declared_calm_functions(&src);

        for method in BRIDGE_METHODS {
            assert!(
                declared.iter().any(|d| d == method),
                "calm.d.ts does not declare calm.{method}"
            );
        }
        for function in &declared {
            assert!(
                is_known_method(function) || PRELUDE_ONLY.contains(&function.as_str()),
                "calm.d.ts declares calm.{function} without a bridge method"
            );
        }
        for function in PRELUDE_ONLY {
            assert!(
                declared.iter().any(|d| d == function),
                "calm.d.ts lacks {function}"
            );
        }
    }

    #[test]
    fn event_to_js_escapes_payload() {
        let event = BridgeEvent {
            event: "settingsChanged".into(),
            payload: json!({ "note": "say \"hi\"\u{2028}</script>\u{2029}" }),
        };
        assert_eq!(
            event.to_js(),
            r#"window.__calm.emit("settingsChanged", {"note":"say \"hi\"\u2028</script>\u2029"})"#
        );

        // The event name is JSON encoded too, so it cannot break out of its string.
        let hostile = BridgeEvent {
            event: "x\");alert(1);(\"".into(),
            payload: Value::Null,
        };
        assert_eq!(
            hostile.to_js(),
            r#"window.__calm.emit("x\");alert(1);(\"", null)"#
        );
    }

    #[test]
    fn unknown_method_is_not_known() {
        for method in BRIDGE_METHODS {
            assert!(is_known_method(method), "{method} should be known");
        }
        for method in [
            "",
            "nope",
            "Settings.get",
            "settings",
            "settings.get ",
            "events.on",
        ] {
            assert!(!is_known_method(method), "{method:?} should not be known");
        }
    }

    #[test]
    fn not_implemented_rejects_with_method_name() {
        assert_eq!(
            BridgeResponse::not_implemented(9, "storage.get"),
            BridgeResponse::Err(9, "not implemented: storage.get".into())
        );
        assert_eq!(
            BridgeResponse::not_implemented(9, "storage.get").to_js(),
            r#"window.__calm.resolve(9, false, "not implemented: storage.get")"#
        );
    }
}
