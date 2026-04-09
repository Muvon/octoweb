//! Sensitive data sanitization for MCP responses and proactive learning.
//!
//! Strips tokens/keys from URLs, detects sensitive pages, and provides
//! field-level sensitivity checks for the snapshot tool.

/// Query parameter names that commonly carry secrets.
/// Values are replaced with `[REDACTED]` in URL sanitization.
const SENSITIVE_PARAMS: &[&str] = &[
    "token",
    "access_token",
    "refresh_token",
    "oauth_token",
    "api_key",
    "apikey",
    "apiKey",
    "key",
    "secret",
    "client_secret",
    "private_key",
    "password",
    "pwd",
    "passwd",
    "bearer",
    "auth",
    "authorization",
    "session_id",
    "sessionId",
    "sid",
    "code",  // OAuth auth code — single-use but still sensitive
    "state", // OAuth state — session identifier
];

/// URL path segments that indicate a sensitive page.
/// The learning agent skips content extraction on these pages.
const SENSITIVE_PATH_SEGMENTS: &[&str] = &[
    "/login",
    "/signin",
    "/sign-in",
    "/signup",
    "/sign-up",
    "/register",
    "/auth",
    "/oauth",
    "/callback",
    "/password",
    "/reset-password",
    "/forgot-password",
    "/2fa",
    "/mfa",
    "/otp",
    "/verify",
    "/verification",
    "/account/security",
    "/settings/security",
    "/billing",
    "/payment",
    "/checkout",
];

/// Sanitize a URL by replacing sensitive query parameter values with `[REDACTED]`.
///
/// Preserves the URL structure (path, fragment, non-sensitive params) so navigation
/// still works if the sanitized URL is used for display. The original URL is never
/// modified — a new String is returned.
///
/// Handles edge cases: invalid URLs returned as-is, fragments preserved.
pub fn sanitize_url(url: &str) -> String {
    // Fast path: no query string → nothing to sanitize
    let Some(q_pos) = url.find('?') else {
        return url.to_string();
    };

    // Split at fragment (#) if present
    let (url_no_frag, fragment) = match url.find('#') {
        Some(f_pos) => (&url[..f_pos], Some(&url[f_pos..])),
        None => (url, None),
    };

    let base = &url_no_frag[..q_pos];
    let query = &url_no_frag[q_pos + 1..];

    if query.is_empty() {
        return url.to_string();
    }

    let mut sanitized_params = Vec::new();
    for pair in query.split('&') {
        if let Some((key, _value)) = pair.split_once('=') {
            if SENSITIVE_PARAMS
                .iter()
                .any(|&s| key.eq_ignore_ascii_case(s))
            {
                sanitized_params.push(format!("{key}=[REDACTED]"));
            } else {
                sanitized_params.push(pair.to_string());
            }
        } else {
            sanitized_params.push(pair.to_string());
        }
    }

    let mut result = format!("{base}?{}", sanitized_params.join("&"));
    if let Some(frag) = fragment {
        result.push_str(frag);
    }
    result
}

/// Check whether a URL points to a sensitive page (login, auth, payment, etc.).
///
/// Used by the proactive learning agent to skip content extraction on pages
/// where the visible text is likely to contain credentials or financial data.
pub fn is_sensitive_page(url: &str) -> bool {
    let path = match url.find("://") {
        Some(pos) => {
            let after_scheme = &url[pos + 3..];
            match after_scheme.find('/') {
                Some(slash) => &after_scheme[slash..],
                None => "/",
            }
        }
        None => url,
    };

    // Strip query string and fragment for path matching
    let path_only = path.split('?').next().unwrap_or(path);
    let path_only = path_only.split('#').next().unwrap_or(path_only);
    let path_lower = path_only.to_ascii_lowercase();

    SENSITIVE_PATH_SEGMENTS
        .iter()
        .any(|seg| path_lower.contains(seg))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sanitize_url_no_query() {
        assert_eq!(
            sanitize_url("https://example.com/page"),
            "https://example.com/page"
        );
    }

    #[test]
    fn test_sanitize_url_sensitive_param() {
        assert_eq!(
            sanitize_url("https://example.com/cb?code=abc123&state=xyz"),
            "https://example.com/cb?code=[REDACTED]&state=[REDACTED]"
        );
    }

    #[test]
    fn test_sanitize_url_mixed_params() {
        assert_eq!(
            sanitize_url("https://example.com?page=1&token=secret&sort=date"),
            "https://example.com?page=1&token=[REDACTED]&sort=date"
        );
    }

    #[test]
    fn test_sanitize_url_with_fragment() {
        assert_eq!(
            sanitize_url("https://example.com?api_key=xxx#section"),
            "https://example.com?api_key=[REDACTED]#section"
        );
    }

    #[test]
    fn test_is_sensitive_page() {
        assert!(is_sensitive_page("https://example.com/login"));
        assert!(is_sensitive_page("https://bank.com/account/security"));
        assert!(is_sensitive_page("https://shop.com/checkout"));
        assert!(!is_sensitive_page("https://example.com/dashboard"));
        assert!(!is_sensitive_page("https://example.com/products"));
    }
}
