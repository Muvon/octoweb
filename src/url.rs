//! URL resolution: turn user input into a navigable URL.
//!
//! - Already has a scheme → pass through
//! - Looks like localhost / IP / domain → prepend `https://`
//! - Otherwise → search using the configured engine

/// Turn user input into a navigable URL.
/// `search_engine` is a URL template with `{}` as the query placeholder
/// (e.g. `"https://www.google.com/search?q={}"`)
pub fn resolve_url(input: &str, search_engine: &str) -> String {
    let s = input.trim();
    if s.is_empty() {
        return "about:blank".to_string();
    }

    if has_url_scheme(s) {
        return s.to_string();
    }

    if !s.chars().any(char::is_whitespace)
        && (looks_like_localhost(s) || looks_like_ipv4(s) || looks_like_domain(s))
    {
        return format!("https://{s}");
    }

    search_engine.replace("{}", &encode_uri(s))
}

fn has_url_scheme(s: &str) -> bool {
    let Some(idx) = s.find(':') else {
        return false;
    };
    let scheme = &s[..idx];
    !scheme.is_empty()
        && scheme
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
}

fn host_part(input: &str) -> &str {
    input.split(['/', '?', '#']).next().unwrap_or(input)
}

fn looks_like_localhost(input: &str) -> bool {
    let host = host_part(input);
    if host == "localhost" {
        return true;
    }
    host.strip_prefix("localhost:")
        .map(|port| !port.is_empty() && port.chars().all(|c| c.is_ascii_digit()))
        .unwrap_or(false)
}

fn looks_like_ipv4(input: &str) -> bool {
    let host = host_part(input);
    let ip = host.split(':').next().unwrap_or(host);
    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    parts.iter().all(|p| p.parse::<u8>().is_ok())
}

fn looks_like_domain(input: &str) -> bool {
    let host = host_part(input);
    let domain = host.split(':').next().unwrap_or(host);
    if !domain.contains('.') {
        return false;
    }

    let labels: Vec<&str> = domain.split('.').collect();
    if labels.iter().any(|label| label.is_empty()) {
        return false;
    }

    labels.iter().all(|label| {
        label
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
            && !label.starts_with('-')
            && !label.ends_with('-')
    })
}

/// Returns `true` if the URL uses a scheme that should be opened by an external
/// macOS app rather than loaded inside the browser (e.g. `tg://`, `figma://`,
/// `mailto:`, `tel:`, `slack://`).
///
/// We whitelist the schemes the browser handles natively; everything else is external.
pub fn is_external_scheme(url: &str) -> bool {
    let Some(idx) = url.find(':') else {
        return false;
    };
    let scheme = &url[..idx];
    if scheme.is_empty() {
        return false;
    }
    // Must be a valid scheme (ASCII alphanumeric + '+' / '-' / '.')
    if !scheme
        .chars()
        .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'))
    {
        return false;
    }
    // Schemes the browser handles natively — everything else is external.
    !matches!(
        scheme.to_ascii_lowercase().as_str(),
        "http" | "https" | "about" | "data" | "blob" | "javascript" | "file"
    )
}

/// Schemes an agent must never be able to navigate to.
///
/// `javascript:` executes in the target tab's origin — for an in-place navigate
/// that is arbitrary code in a page the user is already logged into. `data:` and
/// `blob:` let a caller paint attacker-authored content under a URL the address
/// bar then presents as the tab's real location, and `file:` reads local disk.
/// A human typing these in the address bar is making their own choice; a tool
/// call is not, because the URL usually came from a page the agent just read.
pub fn is_agent_forbidden_scheme(url: &str) -> bool {
    // No colon means no scheme — a bare search term like "javascript" is not a
    // javascript: URL.
    if !url.contains(':') {
        return false;
    }
    // The URL parser strips ASCII whitespace and control characters before
    // reading the scheme, so `java\tscript:` is `javascript:` — strip them too
    // or the check is trivially bypassed.
    let scheme: String = url
        .chars()
        .take_while(|&c| c != ':')
        .filter(|c| !c.is_whitespace() && !c.is_control())
        .flat_map(char::to_lowercase)
        .collect();
    matches!(scheme.as_str(), "javascript" | "data" | "blob" | "file")
}

/// Percent-encode a query string for use in a URL (encodes everything except unreserved chars)
fn encode_uri(s: &str) -> String {
    use std::fmt::Write;
    let mut out = String::with_capacity(s.len() * 2);
    for &b in s.as_bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            let _ = write!(out, "%{b:02X}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_forbidden_schemes_are_refused() {
        assert!(is_agent_forbidden_scheme("javascript:alert(1)"));
        assert!(is_agent_forbidden_scheme("JavaScript:alert(1)"));
        assert!(is_agent_forbidden_scheme("  javascript:alert(1)"));
        assert!(is_agent_forbidden_scheme("data:text/html,<h1>x</h1>"));
        assert!(is_agent_forbidden_scheme("blob:https://x/y"));
        assert!(is_agent_forbidden_scheme("file:///etc/hosts"));
        // The URL parser strips these before reading the scheme, so we must too.
        assert!(is_agent_forbidden_scheme("java\tscript:alert(1)"));
        assert!(is_agent_forbidden_scheme("java\nscript:alert(1)"));
    }

    #[test]
    fn ordinary_urls_are_allowed() {
        assert!(!is_agent_forbidden_scheme("https://example.com"));
        assert!(!is_agent_forbidden_scheme("http://localhost:3434/mcp"));
        assert!(!is_agent_forbidden_scheme("about:blank"));
        assert!(!is_agent_forbidden_scheme("mailto:a@b.c"));
        assert!(!is_agent_forbidden_scheme("example.com/javascript:x"));
        assert!(!is_agent_forbidden_scheme("no-scheme-here"));
        // A bare search term is not a URL, even when it reads like a scheme.
        assert!(!is_agent_forbidden_scheme("javascript"));
        assert!(!is_agent_forbidden_scheme("data"));
    }
}
