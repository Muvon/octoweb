//! HTTPS requests through `/usr/bin/curl`, for the calendar providers. As in
//! `mail_oauth`, the whole request — URL, headers, credentials and body — goes
//! to curl on stdin as a config file (`-K -`), so no token or password ever
//! appears in a process's arguments. TLS is the system's.

use std::time::Duration;

use tokio::io::AsyncWriteExt;

const CURL: &str = "/usr/bin/curl";
const MAX_REDIRECTS: usize = 5;

#[derive(Clone, Default)]
pub struct Request {
	pub method: String,
	pub url: String,
	pub headers: Vec<(String, String)>,
	/// `user:password` for Basic auth.
	pub basic: Option<String>,
	pub body: Option<String>,
	pub timeout: Option<Duration>,
}

impl std::fmt::Debug for Request {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("Request")
			.field("method", &self.method)
			.field("url", &self.url)
			.finish_non_exhaustive()
	}
}

impl Request {
	pub fn new(method: &str, url: &str) -> Self {
		Self {
			method: method.to_string(),
			url: url.to_string(),
			..Default::default()
		}
	}

	pub fn header(mut self, name: &str, value: &str) -> Self {
		self.headers.push((name.to_string(), value.to_string()));
		self
	}

	pub fn bearer(self, token: &str) -> Self {
		self.header("Authorization", &format!("Bearer {token}"))
	}

	pub fn basic(mut self, user: &str, password: &str) -> Self {
		self.basic = Some(format!("{user}:{password}"));
		self
	}

	pub fn body(mut self, content_type: &str, body: String) -> Self {
		self.body = Some(body);
		self.header("Content-Type", content_type)
	}

	pub fn json(self, value: &serde_json::Value) -> Self {
		self.body("application/json", value.to_string())
	}
}

#[derive(Debug)]
pub struct Response {
	pub status: u16,
	pub headers: Vec<(String, String)>,
	pub body: String,
}

impl Response {
	pub fn ok(&self) -> bool {
		(200..300).contains(&self.status)
	}

	pub fn header(&self, name: &str) -> Option<&str> {
		self.headers
			.iter()
			.find(|(n, _)| n.eq_ignore_ascii_case(name))
			.map(|(_, v)| v.as_str())
	}

	pub fn json(&self) -> Result<serde_json::Value, String> {
		serde_json::from_str(&self.body).map_err(|e| format!("Unexpected response ({e})"))
	}
}

/// A curl config string: quoted, with the escapes curl understands.
fn quote(s: &str) -> String {
	let mut out = String::with_capacity(s.len() + 2);
	out.push('"');
	for c in s.chars() {
		match c {
			'\\' => out.push_str("\\\\"),
			'"' => out.push_str("\\\""),
			'\n' => out.push_str("\\n"),
			'\r' => out.push_str("\\r"),
			'\t' => out.push_str("\\t"),
			c => out.push(c),
		}
	}
	out.push('"');
	out
}

fn config(req: &Request) -> String {
	let mut c = String::new();
	c.push_str(&format!("url = {}\n", quote(&req.url)));
	c.push_str(&format!("request = {}\n", quote(&req.method)));
	for (name, value) in &req.headers {
		c.push_str(&format!("header = {}\n", quote(&format!("{name}: {value}"))));
	}
	if let Some(basic) = &req.basic {
		c.push_str(&format!("user = {}\n", quote(basic)));
	}
	if let Some(body) = &req.body {
		// A leading `@` would make curl read a file.
		let body = if body.starts_with('@') {
			format!(" {body}")
		} else {
			body.clone()
		};
		c.push_str(&format!("data-binary = {}\n", quote(&body)));
	}
	c
}

/// Split curl's `--include` output into the final status, its headers and the
/// body. Interim responses (`100 Continue`) come first and are skipped.
fn parse(out: &[u8]) -> Result<Response, String> {
	let text = String::from_utf8_lossy(out);
	let mut rest: &str = &text;
	loop {
		if !rest.starts_with("HTTP/") {
			return Err("No HTTP response".into());
		}
		let (head, body) = match rest.find("\r\n\r\n") {
			Some(i) => (&rest[..i], &rest[i + 4..]),
			None => match rest.find("\n\n") {
				Some(i) => (&rest[..i], &rest[i + 2..]),
				None => (rest, ""),
			},
		};
		let mut lines = head.lines();
		let status: u16 = lines
			.next()
			.and_then(|l| l.split_whitespace().nth(1))
			.and_then(|s| s.parse().ok())
			.ok_or("Malformed HTTP status line")?;
		if (100..200).contains(&status) && body.starts_with("HTTP/") {
			rest = body;
			continue;
		}
		let headers = lines
			.filter_map(|l| l.split_once(':'))
			.map(|(n, v)| (n.trim().to_string(), v.trim().to_string()))
			.collect();
		return Ok(Response {
			status,
			headers,
			body: body.to_string(),
		});
	}
}

async fn send_once(req: &Request) -> Result<Response, String> {
	let timeout = req
		.timeout
		.unwrap_or(Duration::from_secs(30))
		.as_secs()
		.max(1)
		.to_string();
	let mut child = tokio::process::Command::new(CURL)
		.args([
			"--silent",
			"--show-error",
			"--include",
			"--compressed",
			"--max-time",
			&timeout,
			"--config",
			"-",
		])
		.stdin(std::process::Stdio::piped())
		.stdout(std::process::Stdio::piped())
		.stderr(std::process::Stdio::piped())
		.kill_on_drop(true)
		.spawn()
		.map_err(|e| format!("curl: {e}"))?;
	let mut stdin = child.stdin.take().expect("piped stdin");
	stdin
		.write_all(config(req).as_bytes())
		.await
		.map_err(|e| format!("curl: {e}"))?;
	drop(stdin);
	let out = child.wait_with_output().await.map_err(|e| format!("curl: {e}"))?;
	if !out.status.success() {
		let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
		return Err(if err.is_empty() {
			"The request failed".into()
		} else {
			err.trim_start_matches("curl: ").to_string()
		});
	}
	parse(&out.stdout)
}

/// Send `req`, following redirects to HTTPS targets with the same method and
/// credentials (CalDAV's `/.well-known/caldav` redirects a PROPFIND).
pub async fn send(mut req: Request) -> Result<Response, String> {
	for _ in 0..=MAX_REDIRECTS {
		let resp = send_once(&req).await?;
		if !matches!(resp.status, 301 | 302 | 303 | 307 | 308) {
			return Ok(resp);
		}
		let Some(location) = resp.header("Location") else {
			return Ok(resp);
		};
		let next = resolve(&req.url, location);
		if !next.starts_with("https://") && !is_loopback_url(&next) {
			return Err(format!("Refusing to follow a redirect to {next}"));
		}
		req.url = next;
	}
	Err("Too many redirects".into())
}

fn is_loopback_url(url: &str) -> bool {
	["http://127.0.0.1", "http://localhost", "http://[::1]"]
		.iter()
		.any(|p| url.starts_with(p))
}

/// `href` (absolute, or a path on `base`'s host) as an absolute URL.
pub fn resolve(base: &str, href: &str) -> String {
	let href = href.trim();
	if href.starts_with("http://") || href.starts_with("https://") {
		return href.to_string();
	}
	let origin = origin(base);
	if href.starts_with('/') {
		format!("{origin}{href}")
	} else {
		let dir = base.rsplit_once('/').map_or(base, |(d, _)| d);
		format!("{dir}/{href}")
	}
}

/// `https://host[:port]` of `url`.
pub fn origin(url: &str) -> String {
	let after = url.find("://").map_or(0, |i| i + 3);
	match url[after..].find('/') {
		Some(i) => url[..after + i].to_string(),
		None => url.to_string(),
	}
}

/// Percent-encoding for a path segment or query value.
pub fn encode(s: &str) -> String {
	s.bytes()
		.map(|b| match b {
			b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => (b as char).to_string(),
			_ => format!("%{b:02X}"),
		})
		.collect()
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn config_quotes_everything_and_carries_the_body() {
		let req = Request::new("POST", "https://x/y?a=1")
			.bearer("t\"ok")
			.basic("me@x", "p\\w")
			.body("application/json", "{\"a\":\"b\nc\"}".into());
		let c = config(&req);
		assert!(c.contains("url = \"https://x/y?a=1\"\n"));
		assert!(c.contains("request = \"POST\"\n"));
		assert!(c.contains("header = \"Authorization: Bearer t\\\"ok\"\n"));
		assert!(c.contains("user = \"me@x:p\\\\w\"\n"));
		assert!(c.contains("data-binary = \"{\\\"a\\\":\\\"b\\nc\\\"}\"\n"));
		assert!(config(&Request::new("PUT", "u").body("text/plain", "@file".into())).contains("\" @file\""));
	}

	#[test]
	fn responses_skip_interim_status_and_split_headers() {
		let raw = b"HTTP/1.1 100 Continue\r\n\r\nHTTP/2 207 Multi-Status\r\nETag: \"1\"\r\ncontent-type: text/xml\r\n\r\n<x/>";
		let r = parse(raw).unwrap();
		assert_eq!(r.status, 207);
		assert_eq!(r.header("etag"), Some("\"1\""));
		assert_eq!(r.body, "<x/>");
		assert!(parse(b"HTTP/1.1 204 No Content\r\n\r\n").unwrap().body.is_empty());
	}

	#[test]
	fn hrefs_resolve_against_the_request() {
		assert_eq!(resolve("https://h.x:443/a/b", "/c/"), "https://h.x:443/c/");
		assert_eq!(resolve("https://h.x/a/b", "https://o.x/d"), "https://o.x/d");
		assert_eq!(resolve("https://h.x/a/b", "c.ics"), "https://h.x/a/c.ics");
		assert_eq!(origin("https://h.x"), "https://h.x");
		assert_eq!(encode("a b@c/"), "a%20b%40c%2F");
	}
}
