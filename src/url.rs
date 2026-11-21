//! URL resolution: turn user input into a navigable URL.
//!
//! - Already has a scheme → pass through (an app scheme followed by spaces is a
//!   search: `site:example.com rust` uses a search operator, it names no app)
//! - Looks like localhost → prepend `http://` (local dev servers rarely speak TLS)
//! - Looks like an IP / domain → prepend `https://`
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

	if !s.chars().any(char::is_whitespace) {
		if looks_like_localhost(s) {
			return format!("http://{s}");
		}
		if looks_like_ipv4(s) || looks_like_domain(s) {
			return format!("https://{s}");
		}
	}

	search_engine.replace("{}", &encode_uri(s))
}

/// Schemes the browser loads itself; any other scheme belongs to an external app.
const NATIVE_SCHEMES: &[&str] = &[
	"http",
	"https",
	"about",
	"data",
	"blob",
	"javascript",
	"file",
	"webkit-extension",
];

/// The text before the first colon when it is a valid scheme, not a host and port.
fn scheme(s: &str) -> Option<&str> {
	let (scheme, rest) = s.split_once(':')?;
	let valid = !scheme.is_empty()
		&& scheme
			.chars()
			.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '+' | '-' | '.'));
	(valid && !is_host_port(scheme, rest)).then_some(scheme)
}

fn is_native_scheme(scheme: &str) -> bool {
	NATIVE_SCHEMES.contains(&scheme.to_ascii_lowercase().as_str())
}

/// An app link never holds a raw space, a search often does: `site:x.com rust`
/// reads as scheme `site` by the URL grammar, but it is a query. Native schemes
/// keep spaces because `javascript:` and `data:` URLs carry them.
fn is_app_link(s: &str, scheme: &str) -> bool {
	!is_native_scheme(scheme) && !s.contains(char::is_whitespace)
}

fn has_url_scheme(s: &str) -> bool {
	scheme(s).is_some_and(|scheme| is_native_scheme(scheme) || is_app_link(s, scheme))
}

/// `localhost:5173/app` and `example.com:8080` parse as a scheme by the URL
/// grammar, but a host followed by a port is what was typed. A dot or
/// `localhost` tells it apart from `tel:5551234`.
fn is_host_port(host: &str, rest: &str) -> bool {
	let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
	digits > 0
		&& matches!(rest.as_bytes().get(digits), None | Some(b'/' | b'?' | b'#'))
		&& (host == "localhost" || host.contains('.'))
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
		label.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
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
	let url = url.trim();
	scheme(url).is_some_and(|scheme| is_app_link(url, scheme))
}

/// Schemes an agent must never be able to navigate to.
///
/// `javascript:` executes in the target tab's origin — for an in-place navigate
/// that is arbitrary code in a page the user is already logged into. `data:` and
/// `blob:` let a caller paint attacker-authored content under a URL the address
/// bar then presents as the tab's real location, `file:` reads local disk, and
/// `webkit-extension:` pages hold an extension's privileged APIs. A human
/// typing these in the address bar is making their own choice; a tool call is
/// not, because the URL usually came from a page the agent just read.
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
	matches!(
		scheme.as_str(),
		"javascript" | "data" | "blob" | "file" | "webkit-extension"
	)
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
		assert!(is_agent_forbidden_scheme("webkit-extension://abc/options.html"));
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

	const SEARCH: &str = "https://search.test/?q={}";

	#[test]
	fn a_host_with_a_port_is_not_a_scheme() {
		assert_eq!(resolve_url("localhost:5180/blog", SEARCH), "http://localhost:5180/blog");
		assert_eq!(
			resolve_url("example.com:8080/x?y#z", SEARCH),
			"https://example.com:8080/x?y#z"
		);
		assert_eq!(resolve_url("192.168.1.1:8080", SEARCH), "https://192.168.1.1:8080");
	}

	#[test]
	fn localhost_gets_plain_http() {
		assert_eq!(resolve_url("localhost", SEARCH), "http://localhost");
		assert_eq!(resolve_url("localhost:3000", SEARCH), "http://localhost:3000");
		assert_eq!(resolve_url("example.com", SEARCH), "https://example.com");
	}

	#[test]
	fn real_schemes_pass_through() {
		for url in [
			"https://example.com",
			"about:blank",
			"mailto:a@b.c",
			"tel:5551234",
			"com.example.app:/oauth",
		] {
			assert_eq!(resolve_url(url, SEARCH), url);
		}
	}

	#[test]
	fn host_ports_are_not_external_schemes() {
		for url in ["localhost:3000", "127.0.0.1:8080/x", "example.com:8080?q=1"] {
			assert!(!is_external_scheme(url), "{url}");
		}
	}

	#[test]
	fn external_schemes_remain_external() {
		for url in [
			"tg://resolve?domain=x",
			"tel:5551234",
			"mailto:a@b.c",
			"com.example.app:/oauth",
		] {
			assert!(is_external_scheme(url), "{url}");
		}
	}

	#[test]
	fn extension_pages_load_in_the_browser() {
		assert!(!is_external_scheme("webkit-extension://abc/options.html"));
		assert!(is_external_scheme("tg://resolve?domain=x"));
	}

	#[test]
	fn anything_else_is_a_search() {
		assert_eq!(
			resolve_url("rust ownership", SEARCH),
			"https://search.test/?q=rust%20ownership"
		);
	}

	#[test]
	fn search_operators_with_terms_are_searches() {
		let query = "site:developers.openai.com temperature top_p";
		assert!(!is_external_scheme(query));
		assert_eq!(
			resolve_url(query, SEARCH),
			"https://search.test/?q=site%3Adevelopers.openai.com%20temperature%20top_p"
		);
	}
}
