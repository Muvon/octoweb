//! URL resolution: turn user input into a navigable URL.
//!
//! - Already has a scheme → pass through
//! - Looks like localhost / IP / domain → prepend `https://`
//! - Otherwise → Google search

/// Turn user input into a navigable URL.
pub fn resolve_url(input: &str) -> String {
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

    format!("https://www.google.com/search?q={}", encode_uri(s))
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

/// Percent-encode a query string for use in a URL (encodes everything except unreserved chars)
fn encode_uri(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '~') {
                vec![c]
            } else {
                let mut buf = [0u8; 4];
                let bytes = c.encode_utf8(&mut buf);
                bytes
                    .bytes()
                    .flat_map(|b| format!("%{b:02X}").chars().collect::<Vec<_>>())
                    .collect()
            }
        })
        .collect()
}
