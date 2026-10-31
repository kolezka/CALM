//! Turns what the user typed in a browser address bar into a URL to load, and scrubs a
//! URL before it is saved in a layout.

use serde::{Deserialize, Serialize};

/// How much of a browser URL a saved layout keeps (`session.browser_query`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrowserQueryMode {
    /// Drop the query, the fragment and any `user:password@`. They can carry tokens.
    #[default]
    StripAll,
    /// Save the URL as is. A kept URL can carry a token into the file.
    Keep,
}

impl BrowserQueryMode {
    pub const ALL: [BrowserQueryMode; 2] = [BrowserQueryMode::StripAll, BrowserQueryMode::Keep];

    pub fn label(self) -> &'static str {
        match self {
            BrowserQueryMode::StripAll => "Strip query and fragment",
            BrowserQueryMode::Keep => "Keep full URL",
        }
    }
}

/// The longest URL a layout saves. Longer ones (large `data:` pages) save as `about:blank`.
pub const MAX_SAVED_URL_BYTES: usize = 8192;

/// The URL as a layout saves it. `StripAll` keeps scheme, host, port and path and drops
/// the rest; `Keep` keeps `url` unchanged. In both modes a result over
/// `MAX_SAVED_URL_BYTES` becomes `about:blank`.
pub fn scrub_for_save(url: &str, mode: BrowserQueryMode) -> String {
    let scrubbed = match mode {
        BrowserQueryMode::Keep => url.to_string(),
        BrowserQueryMode::StripAll => strip_query(url),
    };
    if scrubbed.len() > MAX_SAVED_URL_BYTES {
        "about:blank".to_string()
    } else {
        scrubbed
    }
}

/// Schemes recognized inside free text, such as a page title.
const TEXT_URL_SCHEMES: &[&str] = &["https:", "http:", "file:", "about:", "data:"];

/// `text` with every URL inside it passed through `scrub_for_save`, the rest kept. A URL
/// starts at a known scheme and runs to whitespace or a quote or angle bracket. `Keep`
/// returns `text` unchanged.
pub fn scrub_text_for_save(text: &str, mode: BrowserQueryMode) -> String {
    if mode == BrowserQueryMode::Keep {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = find_url_start(rest) {
        out.push_str(&rest[..start]);
        let tail = &rest[start..];
        let end = tail
            .find(|c: char| c.is_whitespace() || matches!(c, '"' | '\'' | '<' | '>'))
            .unwrap_or(tail.len());
        out.push_str(&scrub_for_save(&tail[..end], mode));
        rest = &tail[end..];
    }
    out.push_str(rest);
    out
}

/// Byte index of the first known scheme in `text` that does not continue a longer word.
fn find_url_start(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    (0..bytes.len()).find(|&index| {
        let continues_word = index > 0
            && (bytes[index - 1].is_ascii_alphanumeric()
                || matches!(bytes[index - 1], b'+' | b'-' | b'.'));
        !continues_word
            && TEXT_URL_SCHEMES.iter().any(|scheme| {
                bytes[index..]
                    .get(..scheme.len())
                    .is_some_and(|head| head.eq_ignore_ascii_case(scheme.as_bytes()))
            })
    })
}

/// Scheme, host, port and path of `url`: no query, fragment or `user:password@`.
fn strip_query(url: &str) -> String {
    let end = url.find(['?', '#']).unwrap_or(url.len());
    let url = &url[..end];
    // Userinfo sits between `://` and the first `/` of the path, ending at its last `@`.
    if let Some(start) = url.find("://").map(|index| index + 3) {
        let authority_end = url[start..]
            .find('/')
            .map_or(url.len(), |index| start + index);
        if let Some(at) = url[start..authority_end].rfind('@') {
            return format!("{}{}", &url[..start], &url[start + at + 1..]);
        }
    }
    url.to_string()
}

/// Search used when the input does not look like an address. The query is appended,
/// form-encoded.
pub const SEARCH_URL_PREFIX: &str = "https://duckduckgo.com/?q=";

/// Schemes kept as typed. Anything else that looks like `word:` is a host or a search.
const KEPT_SCHEMES: &[&str] = &["http", "https", "about", "file", "data"];

/// The URL for an address bar input, or `None` for blank input.
///
/// - A URL with a kept scheme loads as typed.
/// - `localhost` and IPv4 hosts get `http://`, since local dev servers rarely use TLS.
/// - Other dotted hosts get `https://`.
/// - Everything else becomes a search.
pub fn normalize_address(input: &str) -> Option<String> {
    let input = input.trim();
    if input.is_empty() {
        return None;
    }
    if let Some(scheme) = scheme_of(input)
        && KEPT_SCHEMES.contains(&scheme.to_ascii_lowercase().as_str())
    {
        return Some(input.to_string());
    }
    if input.chars().any(char::is_whitespace) {
        return Some(search_url(input));
    }
    let host_port = input.split(['/', '?', '#']).next().unwrap_or_default();
    let (host, port) = match host_port.rsplit_once(':') {
        Some((host, port)) => (host, Some(port)),
        None => (host_port, None),
    };
    if port.is_some_and(|port| port.is_empty() || !port.chars().all(|c| c.is_ascii_digit())) {
        return Some(search_url(input));
    }
    let host = host.to_ascii_lowercase();
    if host == "localhost" || is_ipv4(&host) {
        return Some(format!("http://{input}"));
    }
    if is_domain(&host) {
        return Some(format!("https://{input}"));
    }
    Some(search_url(input))
}

/// The search URL for `query`.
pub fn search_url(query: &str) -> String {
    format!("{SEARCH_URL_PREFIX}{}", form_encode(query.trim()))
}

/// `scheme` for input shaped like `scheme:rest`, per RFC 3986 scheme characters.
fn scheme_of(input: &str) -> Option<&str> {
    let (scheme, _) = input.split_once(':')?;
    let mut chars = scheme.chars();
    let first = chars.next()?;
    (first.is_ascii_alphabetic()
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')))
    .then_some(scheme)
}

fn is_ipv4(host: &str) -> bool {
    let parts: Vec<&str> = host.split('.').collect();
    parts.len() == 4 && parts.iter().all(|part| part.parse::<u8>().is_ok())
}

/// Dot-separated labels of letters, digits and hyphens, ending in an alphabetic TLD.
fn is_domain(host: &str) -> bool {
    let labels: Vec<&str> = host.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    let label_ok = |label: &&str| {
        !label.is_empty()
            && !label.starts_with('-')
            && !label.ends_with('-')
            && label.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
    };
    let tld = labels[labels.len() - 1];
    labels.iter().all(label_ok) && tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic())
}

/// `application/x-www-form-urlencoded` value encoding: space becomes `+`.
fn form_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scrub_for_save_table() {
        use BrowserQueryMode::{Keep, StripAll};
        let cases = [
            ("https://example.com/a/b", "https://example.com/a/b"),
            (
                "https://example.com:8443/a?x=1&code=abc",
                "https://example.com:8443/a",
            ),
            ("https://example.com/a#state=xyz", "https://example.com/a"),
            ("https://example.com/a?x=1#y", "https://example.com/a"),
            ("https://example.com?x=1", "https://example.com"),
            ("http://localhost:3000/#/route", "http://localhost:3000/"),
            ("https://user:pw@example.com/a?x", "https://example.com/a"),
            ("about:blank", "about:blank"),
            ("data:text/plain,hello", "data:text/plain,hello"),
            ("data:text/plain,hello?x=1#f", "data:text/plain,hello"),
            ("file:///tmp/a.html?q", "file:///tmp/a.html"),
        ];
        for (input, stripped) in cases {
            assert_eq!(scrub_for_save(input, StripAll), stripped, "{input}");
            assert_eq!(scrub_for_save(input, Keep), input, "{input}");
        }
    }

    fn norm(input: &str) -> String {
        normalize_address(input).expect("non-blank input")
    }

    #[test]
    fn blank_input_is_none() {
        assert_eq!(normalize_address(""), None);
        assert_eq!(normalize_address("   \t"), None);
    }

    #[test]
    fn full_urls_load_as_typed() {
        assert_eq!(norm("https://ghostty.org/docs"), "https://ghostty.org/docs");
        assert_eq!(norm("  http://example.com  "), "http://example.com");
        assert_eq!(norm("HTTPS://Example.com"), "HTTPS://Example.com");
        assert_eq!(norm("about:blank"), "about:blank");
        assert_eq!(norm("file:///tmp/a.html"), "file:///tmp/a.html");
    }

    #[test]
    fn bare_domains_get_https() {
        assert_eq!(norm("ghostty.org"), "https://ghostty.org");
        assert_eq!(
            norm("github.com/rust-lang/rust?tab=readme#top"),
            "https://github.com/rust-lang/rust?tab=readme#top"
        );
        assert_eq!(norm("example.com:8443/x"), "https://example.com:8443/x");
        assert_eq!(norm("sub.my-site.co.uk"), "https://sub.my-site.co.uk");
    }

    #[test]
    fn local_hosts_get_http() {
        assert_eq!(norm("localhost"), "http://localhost");
        assert_eq!(norm("localhost:3000/app"), "http://localhost:3000/app");
        assert_eq!(norm("127.0.0.1:8080"), "http://127.0.0.1:8080");
        assert_eq!(norm("192.168.1.10"), "http://192.168.1.10");
    }

    #[test]
    fn non_address_input_becomes_a_search() {
        assert_eq!(norm("rust gpui"), "https://duckduckgo.com/?q=rust+gpui");
        assert_eq!(norm("ghostty"), "https://duckduckgo.com/?q=ghostty");
        assert_eq!(
            norm("what is 1+1?"),
            "https://duckduckgo.com/?q=what+is+1%2B1%3F"
        );
        assert_eq!(
            norm("zażółć"),
            "https://duckduckgo.com/?q=za%C5%BC%C3%B3%C5%82%C4%87"
        );
    }

    #[test]
    fn look_alikes_are_searches_not_urls() {
        // A numeric TLD, a bad port, an unknown scheme and a single dot are not addresses.
        assert_eq!(norm("1.2"), "https://duckduckgo.com/?q=1.2");
        assert_eq!(
            norm("example.com:abc"),
            "https://duckduckgo.com/?q=example.com%3Aabc"
        );
        assert_eq!(
            norm("javascript:alert(1)"),
            "https://duckduckgo.com/?q=javascript%3Aalert%281%29"
        );
        assert_eq!(norm("."), "https://duckduckgo.com/?q=.");
        assert_eq!(norm("999.1.1.1"), "https://duckduckgo.com/?q=999.1.1.1");
    }
}
