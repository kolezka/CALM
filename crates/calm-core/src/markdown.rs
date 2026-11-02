//! Markdown panels: the text renders to one self-contained HTML page with GitHub styles
//! and highlight.js inlined, so the page needs no network except for images it links.
//!
//! Raw HTML in the source is escaped and shows as text (comrak's `escape` on, `unsafe` off) and dangerous link
//! schemes are dropped by comrak. A content security policy only lets the page's own
//! scripts run, as a second fence.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::background::Rgb;
use crate::geometry::Size;

pub const DEFAULT_MARKDOWN_SIZE: Size = Size::new(560.0, 420.0);
/// The text of a new panel, until the user writes their own in its Settings window.
pub const DEFAULT_MARKDOWN_TEXT: &str = "# Markdown\n\nOpen this panel's **Settings** to \
write your own text.\n\n- [x] GitHub flavored\n- [ ] Tables, task lists, footnotes\n";
/// The longest text a panel keeps, in bytes.
pub const MAX_MARKDOWN_BYTES: usize = 512 * 1024;

pub const FONT_SIZE_RANGE: std::ops::RangeInclusive<f64> = 8.0..=48.0;
pub const PADDING_RANGE: std::ops::RangeInclusive<f64> = 0.0..=96.0;
pub const MAX_FONT_FAMILY_CHARS: usize = 200;

const GITHUB_CSS: &str = include_str!("../assets/markdown/github-markdown-dark.css");
const HIGHLIGHT_CSS: &str = include_str!("../assets/markdown/highlight-github-dark.min.css");
const HIGHLIGHT_JS: &str = include_str!("../assets/markdown/highlight.min.js");

/// The global `markdown` section of settings: every panel's look unless it sets its own.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MarkdownSettings {
    pub background_color: Rgb,
    /// 0 to 1. Only the background fades; text stays opaque.
    pub background_opacity: f64,
    pub text_color: Rgb,
    pub link_color: Rgb,
    /// A CSS font-family list, such as `"Helvetica Neue", sans-serif`.
    pub font_family: String,
    /// Points.
    pub font_size: f64,
    /// Points around the text.
    pub padding: f64,
}

impl Default for MarkdownSettings {
    fn default() -> Self {
        MarkdownSettings {
            background_color: Rgb(0x0d1117),
            background_opacity: 1.0,
            text_color: Rgb(0xf0f6fc),
            link_color: Rgb(0x4493f8),
            font_family: "-apple-system, BlinkMacSystemFont, \"Segoe UI\", Helvetica, Arial, \
                          sans-serif"
                .to_string(),
            font_size: 15.0,
            padding: 16.0,
        }
    }
}

impl MarkdownSettings {
    pub fn validate(&self) -> Result<(), String> {
        check_opacity("markdown.background_opacity", self.background_opacity)?;
        check_font_family("markdown.font_family", &self.font_family)?;
        check_range("markdown.font_size", self.font_size, FONT_SIZE_RANGE)?;
        check_range("markdown.padding", self.padding, PADDING_RANGE)
    }

    /// The look of one panel: each field it sets and passes the checks wins, the rest
    /// come from here. A bad field from a hand-edited layout falls back instead of
    /// reaching the page's CSS.
    pub fn resolve(&self, own: Option<&PanelMarkdownStyle>) -> MarkdownSettings {
        let mut style = self.clone();
        let Some(own) = own else {
            return style;
        };
        if let Some(color) = own.background_color {
            style.background_color = color;
        }
        if let Some(value) = own
            .background_opacity
            .filter(|v| check_opacity("", *v).is_ok())
        {
            style.background_opacity = value;
        }
        if let Some(color) = own.text_color {
            style.text_color = color;
        }
        if let Some(color) = own.link_color {
            style.link_color = color;
        }
        if let Some(family) = own
            .font_family
            .as_ref()
            .filter(|family| check_font_family("", family).is_ok())
        {
            style.font_family = family.clone();
        }
        if let Some(size) = own
            .font_size
            .filter(|size| check_range("", *size, FONT_SIZE_RANGE).is_ok())
        {
            style.font_size = size;
        }
        if let Some(padding) = own
            .padding
            .filter(|padding| check_range("", *padding, PADDING_RANGE).is_ok())
        {
            style.padding = padding;
        }
        style
    }
}

/// A markdown panel's own look, saved in `overrides.markdown`. Each field is set or
/// inherited from the global `markdown` section on its own.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PanelMarkdownStyle {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background_color: Option<Rgb>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background_opacity: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_color: Option<Rgb>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link_color: Option<Rgb>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub padding: Option<f64>,
}

impl PanelMarkdownStyle {
    pub fn is_empty(&self) -> bool {
        *self == PanelMarkdownStyle::default()
    }

    pub fn validate(&self, path: &str) -> Result<(), String> {
        if let Some(value) = self.background_opacity {
            check_opacity(&format!("{path}.background_opacity"), value)?;
        }
        if let Some(family) = &self.font_family {
            check_font_family(&format!("{path}.font_family"), family)?;
        }
        if let Some(size) = self.font_size {
            check_range(&format!("{path}.font_size"), size, FONT_SIZE_RANGE)?;
        }
        if let Some(padding) = self.padding {
            check_range(&format!("{path}.padding"), padding, PADDING_RANGE)?;
        }
        Ok(())
    }
}

/// Reads `overrides.markdown` from a layout. A field with the wrong type or a value the
/// checks reject is dropped with a warning, so one bad value never fails the layout or
/// blocks a later edit. A value that is not an object is no override.
pub(crate) fn deserialize_panel_style<'de, D>(
    deserializer: D,
) -> Result<Option<PanelMarkdownStyle>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    Ok(panel_style_from_value(&value, &mut |warning| {
        eprintln!("calm: {warning}")
    }))
}

/// The valid part of a loaded `overrides.markdown`; `warn` gets one line per dropped field.
pub fn panel_style_from_value(
    value: &serde_json::Value,
    warn: &mut dyn FnMut(String),
) -> Option<PanelMarkdownStyle> {
    use serde_json::Value;
    const PATH: &str = "overrides.markdown";
    let fields = match value {
        Value::Null => return None,
        Value::Object(fields) => fields,
        other => {
            warn(format!("\"{PATH}\" is {other}, expected an object; ignored"));
            return None;
        }
    };
    let color = |path: &str, value: &Value| -> Result<Option<Rgb>, String> {
        serde_json::from_value::<Option<Rgb>>(value.clone())
            .map_err(|error| format!("\"{path}\": {error}"))
    };
    let number = |path: &str, value: &Value, range| -> Result<Option<f64>, String> {
        if value.is_null() {
            return Ok(None);
        }
        let number = value
            .as_f64()
            .ok_or_else(|| format!("\"{path}\" is {value}, expected a number"))?;
        check_range(path, number, range).map(|()| Some(number))
    };
    let mut style = PanelMarkdownStyle::default();
    for (key, value) in fields {
        let path = format!("{PATH}.{key}");
        let read = match key.as_str() {
            "background_color" => color(&path, value).map(|v| style.background_color = v),
            "text_color" => color(&path, value).map(|v| style.text_color = v),
            "link_color" => color(&path, value).map(|v| style.link_color = v),
            "background_opacity" => {
                number(&path, value, 0.0..=1.0).map(|v| style.background_opacity = v)
            }
            "font_size" => number(&path, value, FONT_SIZE_RANGE).map(|v| style.font_size = v),
            "padding" => number(&path, value, PADDING_RANGE).map(|v| style.padding = v),
            "font_family" => match value {
                Value::Null => Ok(()),
                Value::String(family) => {
                    check_font_family(&path, family).map(|()| style.font_family = Some(family.clone()))
                }
                other => Err(format!("\"{path}\" is {other}, expected a string")),
            },
            // Unknown keys are ignored, as in the other overrides.
            _ => Ok(()),
        };
        if let Err(error) = read {
            warn(format!("{error}; ignored"));
        }
    }
    (!style.is_empty()).then_some(style)
}

fn check_range(path: &str, value: f64, range: std::ops::RangeInclusive<f64>) -> Result<(), String> {
    if value.is_finite() && range.contains(&value) {
        Ok(())
    } else {
        Err(format!(
            "\"{path}\" is {value}, expected {} to {}",
            range.start(),
            range.end()
        ))
    }
}

fn check_opacity(path: &str, value: f64) -> Result<(), String> {
    check_range(path, value, 0.0..=1.0)
}

/// Letters, digits, spaces, commas, quotes, dots, hyphens and underscores only, so the
/// value can never leave its CSS declaration.
pub fn check_font_family(path: &str, family: &str) -> Result<(), String> {
    if family.trim().is_empty() {
        return Err(format!("\"{path}\" must not be empty"));
    }
    let count = family.chars().count();
    if count > MAX_FONT_FAMILY_CHARS {
        return Err(format!(
            "\"{path}\" has {count} characters, at most {MAX_FONT_FAMILY_CHARS}"
        ));
    }
    let allowed =
        |c: char| c.is_alphanumeric() || matches!(c, ' ' | ',' | '"' | '\'' | '.' | '-' | '_');
    if let Some(bad) = family.chars().find(|c| !allowed(*c)) {
        return Err(format!(
            "\"{path}\" has {bad:?}; use letters, digits, spaces, commas, quotes, dots, \
             hyphens and underscores"
        ));
    }
    Ok(())
}

/// The text a panel may keep: at most `MAX_MARKDOWN_BYTES`.
pub fn check_text(text: &str) -> Result<(), String> {
    if text.len() > MAX_MARKDOWN_BYTES {
        return Err(format!(
            "markdown text is {} bytes, at most {MAX_MARKDOWN_BYTES}",
            text.len()
        ));
    }
    Ok(())
}

fn options() -> comrak::Options<'static> {
    let mut options = comrak::Options::default();
    let extension = &mut options.extension;
    extension.strikethrough = true;
    extension.table = true;
    extension.autolink = true;
    extension.tasklist = true;
    extension.footnotes = true;
    extension.alerts = true;
    // Raw HTML in the source shows as text: never passed through, never silently omitted.
    options.render.r#unsafe = false;
    options.render.escape = true;
    options.render.github_pre_lang = false;
    options
}

/// The HTML for `source`, without the page around it.
pub fn render_body(source: &str) -> String {
    comrak::markdown_to_html(source, &options())
}

fn css_color(rgb: Rgb) -> String {
    format!("#{:06x}", rgb.0 & 0xff_ffff)
}

/// The CSS that applies `style` over the GitHub sheet.
pub fn style_css(style: &MarkdownSettings) -> String {
    let [r, g, b] = style.background_color.channels();
    let alpha = style.background_opacity.clamp(0.0, 1.0);
    format!(
        "html{{background:transparent;margin:0}}\
         body{{margin:0;min-height:100vh;background-color:rgba({r},{g},{b},{alpha})}}\
         .markdown-body{{box-sizing:border-box;min-height:100vh;background:transparent;\
         color:{text};font-family:{family};font-size:{size}px;padding:{padding}px}}\
         .markdown-body a{{color:{link}}}\
         .markdown-body pre code.hljs{{background:transparent;padding:0}}",
        text = css_color(style.text_color),
        link = css_color(style.link_color),
        family = style.font_family,
        size = style.font_size,
        padding = style.padding,
    )
}

/// A nonce only the page's own script tags carry. Taken from the body, which cannot
/// contain its own hash, and with raw HTML off it cannot contain a tag at all.
fn script_nonce(body: &str) -> String {
    let digest = Sha256::digest(body.as_bytes());
    digest[..16]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The whole page for a panel: `source` rendered in `style`.
pub fn page_html(source: &str, style: &MarkdownSettings) -> String {
    let body = render_body(source);
    let nonce = script_nonce(&body);
    format!(
        "<!doctype html><html><head><meta charset=\"utf-8\">\
         <meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; \
         img-src https: http: data:; style-src 'unsafe-inline'; font-src data:; \
         script-src 'nonce-{nonce}'\">\
         <style>{GITHUB_CSS}</style><style>{HIGHLIGHT_CSS}</style><style>{css}</style>\
         </head><body><article class=\"markdown-body\">{body}</article>\
         <script nonce=\"{nonce}\">{HIGHLIGHT_JS}</script>\
         <script nonce=\"{nonce}\">hljs.highlightAll();</script></body></html>",
        css = style_css(style),
    )
}

/// What a markdown panel does with a navigation its page starts.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Navigation {
    /// The page itself, or a jump inside it (footnotes).
    Allow,
    /// A clicked link the system browser or mail app opens. The page stays.
    OpenExternally(String),
    /// Anything else: the page never navigates away.
    Block,
}

/// The schemes a clicked link may open outside CALM.
pub fn opens_externally(url: &str) -> bool {
    let Some((scheme, rest)) = url.split_once(':') else {
        return false;
    };
    match scheme.to_ascii_lowercase().as_str() {
        "http" | "https" => rest.starts_with("//") && rest.len() > 2,
        "mailto" => !rest.is_empty(),
        _ => false,
    }
}

/// How WebKit says a navigation started.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavigationType {
    LinkActivated,
    FormSubmitted,
    BackForward,
    Reload,
    /// Anything else, such as the host's own `loadHTMLString`.
    Other,
}

/// A navigation the page asks for.
#[derive(Clone, Copy, Debug)]
pub struct NavigationRequest<'a> {
    pub url: &'a str,
    pub kind: NavigationType,
    /// False for a subframe, or for a new window (no target frame).
    pub main_frame: bool,
    /// The page's URL now; `None` before the first load.
    pub current: Option<&'a str>,
}

/// Decides a navigation the page asks for.
pub fn navigation(request: &NavigationRequest) -> Navigation {
    let url = request.url;
    if request.main_frame && same_document_jump(url, request.current) {
        return Navigation::Allow;
    }
    // The host's `loadHTMLString` with no base URL. A page cannot start one itself: raw
    // HTML is escaped and only CALM's own scripts run.
    if request.main_frame
        && request.kind == NavigationType::Other
        && url.eq_ignore_ascii_case("about:blank")
    {
        return Navigation::Allow;
    }
    if request.kind == NavigationType::LinkActivated && opens_externally(url) {
        return Navigation::OpenExternally(url.to_string());
    }
    Navigation::Block
}

/// A jump to a fragment of the page already shown, such as a footnote.
fn same_document_jump(url: &str, current: Option<&str>) -> bool {
    let (Some(current), Some((base, _))) = (current, url.split_once('#')) else {
        return false;
    };
    let current_base = current.split_once('#').map_or(current, |(base, _)| base);
    base == current_base
}

/// JS that counts what rendered, for the smoke: `"h1=N table=N tasks=N code=N"`.
pub const PROBE_JS: &str = "(() => { const n = s => document.querySelectorAll(s).length; \
    return `h1=${n('h1')} table=${n('table')} tasks=${n('input[type=checkbox]')} \
    code=${n('code.hljs')}`; })()";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gfm_extensions_render() {
        let html = render_body(
            "# Title\n\n| a | b |\n|---|---|\n| 1 | 2 |\n\n- [x] done\n- [ ] todo\n\n\
             ~~gone~~ www.example.com\n\nNote[^1].\n\n[^1]: The note.\n\n\
             > [!NOTE]\n> Heads up.\n",
        );
        assert!(html.contains("<h1>Title</h1>"), "{html}");
        assert!(html.contains("<table>"), "{html}");
        assert!(html.contains("type=\"checkbox\""), "{html}");
        assert!(html.contains("<del>gone</del>"), "{html}");
        assert!(html.contains("href=\"http://www.example.com\""), "{html}");
        assert!(html.contains("footnote"), "{html}");
        assert!(html.contains("markdown-alert"), "{html}");
    }

    #[test]
    fn raw_html_is_escaped_not_passed_through() {
        let html = render_body(
            "<div>Important note</div>\n\n<script>alert(1)</script>\n\n\
             hi <img src=x onerror=alert(1)> <b>bold</b>\n",
        );
        assert!(!html.contains("<script"), "{html}");
        assert!(!html.contains("<img src=x"), "{html}");
        assert!(!html.contains("<b>"), "{html}");
        assert!(!html.contains("raw HTML omitted"), "{html}");
        // The text shows as written.
        assert!(
            html.contains("&lt;div&gt;Important note&lt;/div&gt;"),
            "{html}"
        );
        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"), "{html}");
        assert!(html.contains("&lt;b&gt;bold&lt;/b&gt;"), "{html}");
    }

    #[test]
    fn dangerous_link_schemes_are_dropped() {
        let html = render_body(
            "[a](javascript:alert(1)) [b](vbscript:x) [c](data:text/html,<b>x</b>) \
             [d](https://ok.example) [e](file:///etc/passwd)\n",
        );
        assert!(!html.contains("javascript:"), "{html}");
        assert!(!html.contains("vbscript:"), "{html}");
        assert!(!html.contains("data:text/html"), "{html}");
        assert!(!html.contains("file:"), "{html}");
        assert!(html.contains("href=\"https://ok.example\""), "{html}");
    }

    #[test]
    fn images_keep_their_https_source() {
        let html = render_body("![logo](https://example.com/logo.png)\n");
        assert!(
            html.contains("src=\"https://example.com/logo.png\""),
            "{html}"
        );
    }

    #[test]
    fn code_blocks_carry_the_language_class_for_highlight_js() {
        let html = render_body("```rust\nfn main() {}\n```\n");
        assert!(html.contains("class=\"language-rust\""), "{html}");
    }

    #[test]
    fn page_inlines_assets_and_only_its_own_scripts_run() {
        let page = page_html("# Hi\n", &MarkdownSettings::default());
        assert!(page.contains("hljs.highlightAll()"));
        assert!(page.contains(".markdown-body"));
        assert!(page.contains("Content-Security-Policy"));
        assert!(!page.contains("src=\"http"), "no remote script or style");
        let nonce = script_nonce(&render_body("# Hi\n"));
        assert!(page.contains(&format!("script-src 'nonce-{nonce}'")));
        assert_eq!(
            page.matches(&format!("<script nonce=\"{nonce}\">")).count(),
            2
        );
        // highlight.js names `<script` in its own grammar, so count outside it.
        assert_eq!(page.replace(HIGHLIGHT_JS, "").matches("<script").count(), 2);
    }

    #[test]
    fn vendored_assets_keep_their_license_headers() {
        assert!(GITHUB_CSS.starts_with("/*! github-markdown-css"));
        assert!(HIGHLIGHT_CSS.contains("BSD-3-Clause"));
        assert!(HIGHLIGHT_JS.contains("License: BSD-3-Clause"));
        assert!(!HIGHLIGHT_JS.contains("</script"));
    }

    #[test]
    fn style_css_uses_every_field_and_fades_only_the_background() {
        let style = MarkdownSettings {
            background_color: Rgb(0x102030),
            background_opacity: 0.25,
            text_color: Rgb(0xaabbcc),
            link_color: Rgb(0x00ff00),
            font_family: "Menlo, monospace".into(),
            font_size: 13.0,
            padding: 4.0,
        };
        let css = style_css(&style);
        assert!(css.contains("rgba(16,32,48,0.25)"), "{css}");
        assert!(css.contains("color:#aabbcc"), "{css}");
        assert!(css.contains("a{color:#00ff00}"), "{css}");
        assert!(css.contains("font-family:Menlo, monospace"), "{css}");
        assert!(css.contains("font-size:13px"), "{css}");
        assert!(css.contains("padding:4px"), "{css}");
        assert!(!css.contains("opacity:"), "text never fades: {css}");
    }

    #[test]
    fn resolve_takes_valid_panel_fields_and_falls_back_on_bad_ones() {
        let global = MarkdownSettings::default();
        assert_eq!(global.resolve(None), global);
        let own = PanelMarkdownStyle {
            background_opacity: Some(0.0),
            text_color: Some(Rgb(0x111111)),
            font_size: Some(99.0),
            font_family: Some("x}</style><script>".into()),
            ..Default::default()
        };
        let style = global.resolve(Some(&own));
        assert_eq!(style.background_opacity, 0.0);
        assert_eq!(style.text_color, Rgb(0x111111));
        assert_eq!(style.font_size, global.font_size);
        assert_eq!(style.font_family, global.font_family);
        assert_eq!(style.link_color, global.link_color);
    }

    #[test]
    fn validation_names_the_field() {
        assert!(MarkdownSettings::default().validate().is_ok());
        let bad = |change: fn(&mut MarkdownSettings)| {
            let mut settings = MarkdownSettings::default();
            change(&mut settings);
            settings.validate().unwrap_err()
        };
        assert!(bad(|s| s.background_opacity = 1.5).contains("markdown.background_opacity"));
        assert!(bad(|s| s.background_opacity = f64::NAN).contains("background_opacity"));
        assert!(bad(|s| s.font_size = 2.0).contains("markdown.font_size"));
        assert!(bad(|s| s.padding = -1.0).contains("markdown.padding"));
        assert!(bad(|s| s.font_family = " ".into()).contains("markdown.font_family"));
        assert!(bad(|s| s.font_family = "a;b".into()).contains("';'"));
        assert!(bad(|s| s.font_family = "a{".into()).contains("font_family"));
        let own = PanelMarkdownStyle {
            padding: Some(1000.0),
            ..Default::default()
        };
        assert!(
            own.validate("overrides.markdown")
                .unwrap_err()
                .contains("overrides.markdown.padding")
        );
        assert!(check_text(&"a".repeat(MAX_MARKDOWN_BYTES)).is_ok());
        assert!(check_text(&"a".repeat(MAX_MARKDOWN_BYTES + 1)).is_err());
    }

    #[test]
    fn a_loaded_style_warns_once_per_dropped_field() {
        let mut warnings = Vec::new();
        let value = serde_json::json!({
            "text_color": "#nothex",
            "font_size": 99,
            "padding": "bad",
            "link_color": "#00ff00",
            "future_field": 1
        });
        let style = panel_style_from_value(&value, &mut |w| warnings.push(w));
        assert_eq!(
            style,
            Some(PanelMarkdownStyle {
                link_color: Some(Rgb(0x00ff00)),
                ..Default::default()
            })
        );
        assert_eq!(warnings.len(), 3, "{warnings:?}");
        for field in ["text_color", "font_size", "padding"] {
            assert!(
                warnings
                    .iter()
                    .any(|w| w.contains(&format!("overrides.markdown.{field}"))),
                "{field}: {warnings:?}"
            );
        }
        warnings.clear();
        assert_eq!(panel_style_from_value(&serde_json::json!(5), &mut |w| warnings.push(w)), None);
        assert_eq!(warnings.len(), 1, "{warnings:?}");
        warnings.clear();
        assert_eq!(
            panel_style_from_value(&serde_json::Value::Null, &mut |w| warnings.push(w)),
            None
        );
        assert!(warnings.is_empty());
    }

    #[test]
    fn settings_parse_partial_and_reject_unknown_fields() {
        let parsed: MarkdownSettings =
            serde_json::from_str(r##"{"text_color": "#123456", "font_size": 20}"##).unwrap();
        assert_eq!(parsed.text_color, Rgb(0x123456));
        assert_eq!(parsed.font_size, 20.0);
        assert_eq!(parsed.padding, MarkdownSettings::default().padding);
        assert!(serde_json::from_str::<MarkdownSettings>(r#"{"colour": 1}"#).is_err());
        let own: PanelMarkdownStyle =
            serde_json::from_str(r##"{"link_color": "#00ff00"}"##).unwrap();
        assert_eq!(
            serde_json::to_value(&own).unwrap(),
            serde_json::json!({"link_color": "#00ff00"})
        );
    }

    fn request<'a>(
        url: &'a str,
        kind: NavigationType,
        main_frame: bool,
        current: Option<&'a str>,
    ) -> Navigation {
        navigation(&NavigationRequest {
            url,
            kind,
            main_frame,
            current,
        })
    }

    #[test]
    fn only_the_host_load_and_jumps_inside_the_page_stay() {
        use NavigationType::*;
        let page = Some("about:blank");
        // The host's loadHTMLString, first or again after an edit.
        assert_eq!(request("about:blank", Other, true, None), Navigation::Allow);
        assert_eq!(request("about:blank", Other, true, page), Navigation::Allow);
        // A footnote jump and the jump back.
        assert_eq!(
            request("about:blank#fn-1", LinkActivated, true, page),
            Navigation::Allow
        );
        assert_eq!(
            request("about:blank#fnref-1", LinkActivated, true, Some("about:blank#fn-1")),
            Navigation::Allow
        );
        // A clicked about: link would replace the page.
        assert_eq!(
            request("about:blank", LinkActivated, true, page),
            Navigation::Block
        );
        assert_eq!(
            request("about:srcdoc", LinkActivated, true, page),
            Navigation::Block
        );
        assert_eq!(request("about:srcdoc", Other, true, page), Navigation::Block);
        assert_eq!(request("about:blank", Other, false, page), Navigation::Block);
        assert_eq!(
            request("about:blank#x", LinkActivated, false, page),
            Navigation::Block,
            "a subframe never loads"
        );
        assert_eq!(request("about:blank", Reload, true, page), Navigation::Block);
        assert_eq!(
            request("about:blank", FormSubmitted, true, page),
            Navigation::Block
        );
    }

    #[test]
    fn links_open_outside_and_the_page_never_navigates_away() {
        use NavigationType::*;
        let page = Some("about:blank");
        for url in [
            "https://example.com/x",
            "http://a.b",
            "mailto:me@example.com",
        ] {
            assert_eq!(
                request(url, LinkActivated, true, page),
                Navigation::OpenExternally(url.to_string()),
                "{url}"
            );
            assert_eq!(request(url, Other, true, page), Navigation::Block, "{url}");
            assert_eq!(request(url, BackForward, true, page), Navigation::Block);
        }
        for url in [
            "javascript:alert(1)",
            "file:///etc/passwd",
            "data:text/html,x",
            "calm://x",
            "https:",
            "mailto:",
            "ftp://x",
            "relative.md",
            "about:blank#x",
        ] {
            assert_eq!(
                request(url, LinkActivated, true, Some("https://other.example/")),
                Navigation::Block,
                "{url}"
            );
        }
        assert!(opens_externally("HTTPS://EXAMPLE.COM"));
    }
}
