//! Checks that the local scripts an entry page loads exist, so an unbuilt plugin fails loud.

use std::fs;
use std::path::PathBuf;

use crate::manifest::InstalledPlugin;

/// Local `<script src>` files the entry references but the plugin directory lacks.
/// Typical cause: a TSX plugin whose `dist/` was never built with bun.
pub fn missing_scripts(plugin: &InstalledPlugin) -> Result<Vec<PathBuf>, String> {
    let entry = plugin.dir.join(
        plugin
            .manifest
            .entry()
            .ok_or_else(|| format!("plugin {} has no HTML entry", plugin.manifest.id))?,
    );
    let html = fs::read_to_string(&entry)
        .map_err(|err| format!("cannot read entry {}: {err}", entry.display()))?;
    let base = entry.parent().unwrap_or(&plugin.dir);
    Ok(local_script_srcs(&html)
        .into_iter()
        .map(|src| base.join(src))
        .filter(|path| !path.is_file())
        .collect())
}

/// `src` values of `<script>` tags that point at files next to the page. Remote, protocol
/// relative and data URLs are skipped. A plain scan, not a full HTML parser.
fn local_script_srcs(html: &str) -> Vec<String> {
    let lower = html.to_ascii_lowercase();
    let mut found = Vec::new();
    let mut rest = 0;
    while let Some(start) = lower[rest..].find("<script") {
        let tag_start = rest + start;
        let Some(len) = lower[tag_start..].find('>') else {
            break;
        };
        let tag_end = tag_start + len;
        if let Some(src) = attribute(&html[tag_start..tag_end], &lower[tag_start..tag_end], "src")
            && is_local(&src)
        {
            found.push(src);
        }
        rest = tag_end;
    }
    found
}

fn attribute(tag: &str, lower_tag: &str, name: &str) -> Option<String> {
    let mut search = 0;
    while let Some(at) = lower_tag[search..].find(name) {
        let start = search + at;
        search = start + name.len();
        let preceded_by_space = lower_tag[..start]
            .chars()
            .last()
            .is_some_and(char::is_whitespace);
        let after = lower_tag[search..].trim_start();
        if !preceded_by_space || !after.starts_with('=') {
            continue;
        }
        let value_start = tag.len() - after.len() + 1;
        let value = tag[value_start..].trim_start();
        let quote = value.chars().next()?;
        return if quote == '"' || quote == '\'' {
            value[1..].split(quote).next().map(str::to_string)
        } else {
            value.split_whitespace().next().map(str::to_string)
        };
    }
    None
}

fn is_local(src: &str) -> bool {
    !(src.is_empty() || src.starts_with("//") || src.starts_with("data:") || src.contains("://"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::{PluginContent, PluginManifest, Size};

    fn plugin_with(html: &str, files: &[&str]) -> (tempfile::TempDir, InstalledPlugin) {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("index.html"), html).unwrap();
        for file in files {
            let path = dir.path().join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "").unwrap();
        }
        let plugin = InstalledPlugin {
            manifest: PluginManifest {
                id: "p".to_string(),
                name: "P".to_string(),
                version: "0.1.0".to_string(),
                default_size: Size {
                    width: 100.0,
                    height: 100.0,
                },
                content: PluginContent::Web {
                    entry: "index.html".into(),
                },
                resizable: None,
                min_size: None,
                max_size: None,
            },
            dir: dir.path().to_path_buf(),
        };
        (dir, plugin)
    }

    const BUILT_PAGE: &str = r#"<html><body><div id="app"></div>
        <script src="dist/index.js"></script></body></html>"#;

    #[test]
    fn missing_scripts_reports_unbuilt_dist() {
        let (dir, plugin) = plugin_with(BUILT_PAGE, &[]);
        assert_eq!(
            missing_scripts(&plugin).unwrap(),
            vec![dir.path().join("dist/index.js")]
        );
    }

    #[test]
    fn missing_scripts_is_empty_when_built() {
        let (_dir, plugin) = plugin_with(BUILT_PAGE, &["dist/index.js"]);
        assert_eq!(missing_scripts(&plugin).unwrap(), Vec::<PathBuf>::new());
    }

    #[test]
    fn missing_scripts_ignores_inline_and_remote_scripts() {
        let page = r#"<script>calm.log("x")</script>
            <SCRIPT type="module" SRC='https://cdn.example/x.js'></SCRIPT>
            <script data-src="nope.js" src=//cdn.example/y.js></script>
            <script src="data:text/javascript,1"></script>
            <script defer src=local.js></script>"#;
        let (dir, plugin) = plugin_with(page, &[]);
        assert_eq!(
            missing_scripts(&plugin).unwrap(),
            vec![dir.path().join("local.js")]
        );
    }

    #[test]
    fn missing_scripts_fails_when_entry_unreadable() {
        let (dir, plugin) = plugin_with(BUILT_PAGE, &[]);
        fs::remove_file(dir.path().join("index.html")).unwrap();
        let err = missing_scripts(&plugin).unwrap_err();
        assert!(err.contains("cannot read entry"), "{err}");
    }
}
