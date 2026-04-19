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
    "card_number",
    "cc_number",
    "cvv",
    "cvc",
    "pan",
    "card_exp",
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
    "/add-card",
    "/card-details",
    "/payment-method",
    "/credit-card",
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

/// Luhn mod-10 checksum validation (ISO/IEC 7812-1).
/// Used to distinguish real card numbers from arbitrary digit sequences.
fn luhn_valid(digits: &[u8]) -> bool {
    if digits.len() < 13 {
        return false;
    }
    let mut sum = 0u32;
    for (i, &d) in digits.iter().rev().enumerate() {
        let mut n = (d - b'0') as u32;
        if i % 2 == 1 {
            n *= 2;
            if n > 9 {
                n -= 9;
            }
        }
        sum += n;
    }
    sum.is_multiple_of(10)
}

/// Check whether digit sequence has a valid IIN (Issuer Identification Number) prefix.
/// Covers Visa (4), Mastercard (51-55, 2221-2720), Amex (34/37), Discover (6011/65/644-649),
/// JCB (3528-3589), Diners (300-305/36/38), UnionPay (62).
fn valid_iin_prefix(digits: &[u8]) -> bool {
    if digits.is_empty() {
        return false;
    }
    let d0 = digits[0] - b'0';
    match d0 {
        4 => true, // Visa
        3 => {
            // Amex (34, 37), Diners (30x, 36, 38), JCB (35)
            digits.len() >= 2 && matches!(digits[1] - b'0', 0..=8)
        }
        5 => {
            // Mastercard (51-55), some Maestro (50, 56-58)
            digits.len() >= 2 && matches!(digits[1] - b'0', 0..=8)
        }
        6 => true, // Discover, Maestro, UnionPay
        2 => {
            // Mastercard 2-series (2221-2720)
            if digits.len() >= 4 {
                let prefix = (digits[0] - b'0') as u32 * 1000
                    + (digits[1] - b'0') as u32 * 100
                    + (digits[2] - b'0') as u32 * 10
                    + (digits[3] - b'0') as u32;
                (2221..=2720).contains(&prefix)
            } else {
                false
            }
        }
        _ => false,
    }
}

/// Sanitize free text by redacting sequences that look like credit card PANs.
///
/// Detection uses the same approach as Zendesk's credit_card_sanitizer:
/// 1. Find digit sequences (allowing space/dash separators) of 13–19 digits
/// 2. Validate IIN prefix (known card brand ranges)
/// 3. Validate Luhn checksum
///
/// Only sequences passing both checks are redacted — this keeps false positives
/// near zero (phone numbers, timestamps, IDs won't match).
pub fn sanitize_text(text: &str) -> String {
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut result = String::with_capacity(len);
    let mut i = 0;

    while i < len {
        if !bytes[i].is_ascii_digit() {
            // Non-digit: copy full UTF-8 character
            let ch = text[i..].chars().next().unwrap();
            result.push(ch);
            i += ch.len_utf8();
            continue;
        }

        // Found a digit — collect the first contiguous digit group
        let span_start = i;
        let mut j = i;
        while j < len && bytes[j].is_ascii_digit() {
            j += 1;
        }
        let first_group_len = j - span_start;

        if first_group_len > 19 {
            // Too many contiguous digits — not a card, emit as-is
            result.push_str(&text[span_start..j]);
            i = j;
        } else if first_group_len >= 13 {
            // Standalone group in card-length range — check directly
            let digits: Vec<u8> = bytes[span_start..j].to_vec();
            if valid_iin_prefix(&digits) && luhn_valid(&digits) {
                result.push_str("[CARD REDACTED]");
            } else {
                result.push_str(&text[span_start..j]);
            }
            i = j;
        } else {
            // < 13 contiguous digits — try assembling with separator-connected groups
            let mut digits: Vec<u8> = bytes[span_start..j].to_vec();
            let mut span_end = j;

            while digits.len() < 19 {
                if span_end < len
                    && (bytes[span_end] == b' ' || bytes[span_end] == b'-')
                    && span_end + 1 < len
                    && bytes[span_end + 1].is_ascii_digit()
                {
                    span_end += 1; // skip separator
                    while span_end < len && bytes[span_end].is_ascii_digit() && digits.len() < 19 {
                        digits.push(bytes[span_end]);
                        span_end += 1;
                    }
                } else {
                    break;
                }
            }

            if digits.len() >= 13
                && digits.len() <= 19
                && valid_iin_prefix(&digits)
                && luhn_valid(&digits)
            {
                result.push_str("[CARD REDACTED]");
            } else {
                result.push_str(&text[span_start..span_end]);
            }
            i = span_end;
        }
    }
    result
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
        assert!(is_sensitive_page("https://shop.com/add-card"));
        assert!(is_sensitive_page("https://shop.com/payment-method"));
        assert!(!is_sensitive_page("https://example.com/dashboard"));
        assert!(!is_sensitive_page("https://example.com/products"));
    }

    // ── Luhn checksum ──────────────────────────────────────────────

    #[test]
    fn test_luhn_valid_known_cards() {
        // Visa test number
        assert!(luhn_valid(b"4111111111111111"));
        // Mastercard test number
        assert!(luhn_valid(b"5500000000000004"));
        // Amex test number
        assert!(luhn_valid(b"378282246310005"));
        // Discover test number
        assert!(luhn_valid(b"6011111111111117"));
    }

    #[test]
    fn test_luhn_invalid() {
        assert!(!luhn_valid(b"4111111111111112")); // wrong check digit
        assert!(!luhn_valid(b"1234567890123")); // random 13 digits
    }

    // ── PAN text sanitization ──────────────────────────────────────

    #[test]
    fn test_sanitize_text_pan_no_separators() {
        let text = "Card: 4111111111111111 thanks";
        assert_eq!(sanitize_text(text), "Card: [CARD REDACTED] thanks");
    }

    #[test]
    fn test_sanitize_text_pan_with_spaces() {
        let text = "Card 4111 1111 1111 1111 ok";
        assert_eq!(sanitize_text(text), "Card [CARD REDACTED] ok");
    }

    #[test]
    fn test_sanitize_text_pan_with_dashes() {
        let text = "Card 4111-1111-1111-1111 ok";
        assert_eq!(sanitize_text(text), "Card [CARD REDACTED] ok");
    }

    #[test]
    fn test_sanitize_text_amex() {
        let text = "Amex 3782 822463 10005 end";
        assert_eq!(sanitize_text(text), "Amex [CARD REDACTED] end");
    }

    #[test]
    fn test_sanitize_text_no_false_positive_phone() {
        // Phone numbers: too short (10-11 digits) and wrong IIN
        let text = "Call +1-555-123-4567 now";
        assert_eq!(sanitize_text(text), "Call +1-555-123-4567 now");
    }

    #[test]
    fn test_sanitize_text_no_false_positive_timestamp() {
        let text = "ID: 20260410143022 ref";
        // 14 digits starting with 2 — won't match IIN 2221-2720 range
        assert_eq!(sanitize_text(text), "ID: 20260410143022 ref");
    }

    #[test]
    fn test_sanitize_text_no_false_positive_short() {
        let text = "Order 123456 confirmed";
        assert_eq!(sanitize_text(text), "Order 123456 confirmed");
    }

    #[test]
    fn test_sanitize_text_fails_luhn_not_redacted() {
        // 16 digits starting with 4 but fails Luhn
        let text = "Number 4111111111111112 here";
        assert_eq!(sanitize_text(text), "Number 4111111111111112 here");
    }

    #[test]
    fn test_sanitize_text_preserves_normal_text() {
        let text = "Hello world, no cards here.";
        assert_eq!(sanitize_text(text), text);
    }

    #[test]
    fn test_sanitize_text_multiple_pans() {
        let text = "Cards: 4111111111111111 and 5500000000000004";
        assert_eq!(
            sanitize_text(text),
            "Cards: [CARD REDACTED] and [CARD REDACTED]"
        );
    }

    #[test]
    fn test_sanitize_text_pan_in_prose() {
        let text = "Your payment with card 4111111111111111 was processed successfully.";
        assert_eq!(
            sanitize_text(text),
            "Your payment with card [CARD REDACTED] was processed successfully."
        );
    }
}
