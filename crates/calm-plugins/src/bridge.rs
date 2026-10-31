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

#[derive(Debug, Clone, PartialEq)]
pub enum BridgeResponse {
    Ok(u64, Value),
    Err(u64, String),
}

impl BridgeResponse {
    /// The script the host evaluates in the page to settle the call's Promise.
    pub fn to_js(&self) -> String {
        let (id, ok, value) = match self {
            BridgeResponse::Ok(id, value) => (id, true, value.to_string()),
            BridgeResponse::Err(id, message) => {
                (id, false, Value::from(message.as_str()).to_string())
            }
        };
        let value = value
            .replace('\u{2028}', "\\u2028")
            .replace('\u{2029}', "\\u2029");
        format!("window.__calm.resolve({id}, {ok}, {value})")
    }
}

/// Injected at document start in every plugin page. Defines `window.calm`.
pub const CALM_PRELUDE_JS: &str = r#"(function () {
  if (window.calm) return;
  var nextId = 1;
  var pending = new Map();

  function call(method, params) {
    return new Promise(function (resolve, reject) {
      var id = nextId++;
      pending.set(id, { resolve: resolve, reject: reject });
      try {
        window.webkit.messageHandlers.calm.postMessage({
          id: id,
          method: method,
          params: params === undefined ? null : params
        });
      } catch (err) {
        pending.delete(id);
        reject(err);
      }
    });
  }

  window.__calm = Object.freeze({
    resolve: function (id, ok, value) {
      var entry = pending.get(id);
      if (!entry) return;
      pending.delete(id);
      if (ok) entry.resolve(value);
      else entry.reject(new Error(String(value)));
    }
  });

  window.calm = Object.freeze({
    settings: Object.freeze({
      get: function () { return call("settings.get", null); },
      set: function (obj) { return call("settings.set", obj); }
    }),
    panel: Object.freeze({
      setTitle: function (title) { return call("panel.setTitle", { title: String(title) }); }
    }),
    github: Object.freeze({
      api: function (path) { return call("github.api", { path: String(path) }); }
    }),
    log: function (message) { return call("log", { message: String(message) }); },
    openExternal: function (url) { return call("openExternal", { url: String(url) }); }
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

    #[test]
    fn prelude_posts_every_bridge_method() {
        for needle in [
            "window.webkit.messageHandlers.calm.postMessage",
            "window.__calm",
            "\"settings.get\"",
            "\"settings.set\"",
            "\"panel.setTitle\"",
            "\"github.api\"",
            "\"log\"",
            "\"openExternal\"",
        ] {
            assert!(CALM_PRELUDE_JS.contains(needle), "prelude lacks {needle}");
        }
    }
}
