//! OAuth sign-in for mail (spec `specs/001-mail`, *Gmail OAuth*): the user's
//! own Desktop OAuth client, PKCE, and a one-shot loopback redirect. The
//! refresh token is kept in the Keychain in the account's password slot;
//! access tokens are refreshed as they expire and used through XOAUTH2 for
//! both IMAP and SMTP.
//!
//! HTTPS calls go through `/usr/bin/curl` with the request body on stdin, so
//! no secret ever appears in a process's arguments.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use base64::Engine;
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::config::MailProvider;

/// How long the sign-in page may take before the loopback listener gives up.
const SIGN_IN_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// Refresh this long before the access token actually expires.
const EXPIRY_MARGIN_SECS: i64 = 60;
const CURL: &str = "/usr/bin/curl";

/// Where a provider signs in and hands out tokens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoints {
    pub authorize: &'static str,
    pub token: &'static str,
    pub scope: &'static str,
    /// Microsoft wants the scope again when refreshing.
    pub scope_on_refresh: bool,
}

pub fn endpoints(provider: MailProvider) -> Option<Endpoints> {
    match provider {
        MailProvider::Gmail => Some(Endpoints {
            authorize: "https://accounts.google.com/o/oauth2/v2/auth",
            token: "https://oauth2.googleapis.com/token",
            // IMAP and SMTP over XOAUTH2 need the full mail scope.
            scope: "https://mail.google.com/",
            scope_on_refresh: false,
        }),
        MailProvider::Outlook => Some(Endpoints {
            authorize: "https://login.microsoftonline.com/common/oauth2/v2.0/authorize",
            token: "https://login.microsoftonline.com/common/oauth2/v2.0/token",
            scope: "offline_access https://outlook.office.com/IMAP.AccessAsUser.All https://outlook.office.com/SMTP.Send",
            scope_on_refresh: true,
        }),
        _ => None,
    }
}

/// The Google client `build.sh` compiles in from the builder's Keychain, so
/// Gmail users sign in without making their own. Builds without it offer app
/// passwords and the user's own client instead.
pub fn bundled_google() -> Option<(&'static str, &'static str)> {
    let id = option_env!("OCTOWEB_GOOGLE_CLIENT_ID").filter(|id| !id.is_empty())?;
    Some((
        id,
        option_env!("OCTOWEB_GOOGLE_CLIENT_SECRET").unwrap_or_default(),
    ))
}

/// The client an account signs in with, and its refresh token.
#[derive(Clone, PartialEq, Eq)]
pub struct Client {
    pub endpoints: Endpoints,
    pub client_id: String,
    pub client_secret: String,
    pub refresh_token: String,
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("token_url", &self.endpoints.token)
            .field("client_id", &self.client_id)
            .finish_non_exhaustive()
    }
}

// ── PKCE (RFC 7636) ───────────────────────────────────────────────────────────

/// A code verifier of 64 unreserved characters.
pub fn code_verifier() -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~";
    (0..4)
        .flat_map(|_| crate::workspace::random_bytes16())
        .map(|b| ALPHABET[b as usize % ALPHABET.len()] as char)
        .collect()
}

/// `BASE64URL(SHA256(verifier))`, unpadded.
pub fn code_challenge(verifier: &str) -> String {
    base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(sha256(verifier.as_bytes()))
}

fn sha256(data: &[u8]) -> [u8; 32] {
    unsafe extern "C" {
        // CommonCrypto, in libSystem.
        fn CC_SHA256(data: *const u8, len: u32, md: *mut u8) -> *mut u8;
    }
    let mut out = [0u8; 32];
    // SAFETY: CC_SHA256 reads `len` bytes of `data` and writes 32 into `out`.
    unsafe { CC_SHA256(data.as_ptr(), data.len() as u32, out.as_mut_ptr()) };
    out
}

fn form(pairs: &[(&str, &str)]) -> String {
    pairs
        .iter()
        .map(|(k, v)| format!("{}={}", encode(k), encode(v)))
        .collect::<Vec<_>>()
        .join("&")
}

/// Percent-encoding for query strings and form bodies.
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                    out.push(b);
                    i += 3;
                    continue;
                }
                out.push(b'%');
            }
            b => out.push(b),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The page that asks the user to sign in.
pub fn authorize_url(
    e: &Endpoints,
    client_id: &str,
    redirect: &str,
    challenge: &str,
    state: &str,
    hint: &str,
) -> String {
    format!(
        "{}?{}",
        e.authorize,
        form(&[
            ("client_id", client_id),
            ("redirect_uri", redirect),
            ("response_type", "code"),
            ("scope", e.scope),
            ("access_type", "offline"),
            ("prompt", "consent"),
            ("code_challenge", challenge),
            ("code_challenge_method", "S256"),
            ("state", state),
            ("login_hint", hint),
        ])
    )
}

/// `code` and `state` from the redirect's request line, or the provider's
/// `error` when sign-in was refused.
pub fn redirect_params(request_line: &str) -> Result<(String, String), String> {
    let target = request_line
        .split_whitespace()
        .nth(1)
        .ok_or("Malformed redirect")?;
    let query = target.split_once('?').map(|(_, q)| q).unwrap_or_default();
    let params: HashMap<String, String> = query
        .split('&')
        .filter_map(|p| p.split_once('='))
        .map(|(k, v)| (decode(k), decode(v)))
        .collect();
    if let Some(error) = params.get("error") {
        return Err(match params.get("error_description") {
            Some(d) => format!("{error}: {d}"),
            None => error.clone(),
        });
    }
    match (params.get("code"), params.get("state")) {
        (Some(code), Some(state)) => Ok((code.clone(), state.clone())),
        _ => Err("The sign-in page returned no code".into()),
    }
}

// ── Tokens ────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
    error: Option<String>,
    error_description: Option<String>,
}

/// POST a form to `url` and parse the token JSON.
async fn post(url: &str, body: String) -> Result<TokenResponse, String> {
    let mut child = tokio::process::Command::new(CURL)
        .args([
            "--silent",
            "--show-error",
            "--max-time",
            "30",
            "--data-binary",
            "@-",
            url,
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|e| format!("curl: {e}"))?;
    let mut stdin = child.stdin.take().expect("piped stdin");
    stdin
        .write_all(body.as_bytes())
        .await
        .map_err(|e| format!("curl: {e}"))?;
    drop(stdin);
    let out = child
        .wait_with_output()
        .await
        .map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "Token request failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    serde_json::from_slice(&out.stdout).map_err(|e| {
        format!(
            "Unexpected token response ({e}): {}",
            String::from_utf8_lossy(&out.stdout)
        )
    })
}

fn token_error(r: &TokenResponse) -> String {
    match (&r.error, &r.error_description) {
        (Some(e), Some(d)) => format!("{e}: {d}"),
        (Some(e), None) => e.clone(),
        _ => "The provider returned no token".into(),
    }
}

/// Trade the sign-in code for a refresh token.
pub async fn exchange(
    e: &Endpoints,
    client_id: &str,
    client_secret: &str,
    code: &str,
    redirect: &str,
    verifier: &str,
) -> Result<String, String> {
    let mut pairs = vec![
        ("grant_type", "authorization_code"),
        ("code", code),
        ("redirect_uri", redirect),
        ("client_id", client_id),
        ("code_verifier", verifier),
    ];
    if !client_secret.is_empty() {
        pairs.push(("client_secret", client_secret));
    }
    let r = post(e.token, form(&pairs)).await?;
    r.refresh_token.clone().ok_or_else(|| {
        if r.error.is_some() {
            token_error(&r)
        } else {
            // Google omits it unless consent was given again; `prompt=consent` asks for that.
            "No refresh token came back; remove octoweb's access in your account and sign in again"
                .into()
        }
    })
}

fn token_cache() -> &'static Mutex<HashMap<String, (String, i64)>> {
    static CACHE: OnceLock<Mutex<HashMap<String, (String, i64)>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// A valid access token for `client`, refreshed when it is about to expire.
pub async fn access_token(client: &Client) -> Result<String, String> {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64);
    if let Some((token, expires)) = token_cache().lock().unwrap().get(&client.refresh_token) {
        if *expires - EXPIRY_MARGIN_SECS > now {
            return Ok(token.clone());
        }
    }
    let mut pairs = vec![
        ("grant_type", "refresh_token"),
        ("refresh_token", client.refresh_token.as_str()),
        ("client_id", client.client_id.as_str()),
    ];
    if !client.client_secret.is_empty() {
        pairs.push(("client_secret", client.client_secret.as_str()));
    }
    if client.endpoints.scope_on_refresh {
        pairs.push(("scope", client.endpoints.scope));
    }
    let r = post(client.endpoints.token, form(&pairs)).await?;
    let token = r
        .access_token
        .clone()
        .ok_or_else(|| format!("Sign in again: {}", token_error(&r)))?;
    token_cache().lock().unwrap().insert(
        client.refresh_token.clone(),
        (token.clone(), now + r.expires_in.unwrap_or(3600)),
    );
    Ok(token)
}

/// SASL XOAUTH2 for IMAP. A second challenge carries the server's error; the
/// empty answer to it ends the exchange so the error surfaces.
pub struct Xoauth2 {
    pub user: String,
    pub token: String,
    sent: bool,
}

impl Xoauth2 {
    pub fn new(user: &str, token: &str) -> Self {
        Self {
            user: user.to_string(),
            token: token.to_string(),
            sent: false,
        }
    }
}

impl async_imap::Authenticator for Xoauth2 {
    type Response = String;

    fn process(&mut self, _challenge: &[u8]) -> String {
        if std::mem::replace(&mut self.sent, true) {
            return String::new();
        }
        format!("user={}\x01auth=Bearer {}\x01\x01", self.user, self.token)
    }
}

// ── Sign-in ───────────────────────────────────────────────────────────────────

/// Run the sign-in: listen on a loopback port, hand `open` the page to show,
/// take the redirect, and return the refresh token.
pub async fn sign_in(
    e: &Endpoints,
    client_id: &str,
    client_secret: &str,
    hint: &str,
    open: impl FnOnce(String),
) -> Result<String, String> {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .map_err(|e| format!("Could not listen for the sign-in: {e}"))?;
    let port = listener.local_addr().map_err(|e| e.to_string())?.port();
    let redirect = format!("http://127.0.0.1:{port}");
    let verifier = code_verifier();
    let state = crate::mail::hex(&crate::workspace::random_bytes16());
    open(authorize_url(
        e,
        client_id,
        &redirect,
        &code_challenge(&verifier),
        &state,
        hint,
    ));
    let (code, returned_state) = tokio::time::timeout(SIGN_IN_TIMEOUT, async {
        loop {
            let (mut stream, _) = listener.accept().await.map_err(|e| e.to_string())?;
            let mut buf = vec![0u8; 8192];
            let n = stream.read(&mut buf).await.map_err(|e| e.to_string())?;
            let request = String::from_utf8_lossy(&buf[..n]);
            let line = request.lines().next().unwrap_or_default();
            // Browsers also ask for /favicon.ico; only the redirect counts.
            if !line.contains("code=") && !line.contains("error=") {
                let _ = stream.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n").await;
                continue;
            }
            let result = redirect_params(line);
            let page = match &result {
                Ok(_) => "Signed in. You can close this tab and return to octoweb.",
                Err(_) => "Sign-in did not complete. Return to octoweb to see why.",
            };
            let body = format!(
                "<!DOCTYPE html><meta charset=utf-8><title>octoweb</title><body style=\"font:15px -apple-system,sans-serif;padding:48px\">{page}</body>"
            );
            let _ = stream
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    )
                    .as_bytes(),
                )
                .await;
            return result;
        }
    })
    .await
    .map_err(|_| "Sign-in timed out".to_string())??;
    if returned_state != state {
        return Err("The sign-in response did not match this request".into());
    }
    exchange(e, client_id, client_secret, &code, &redirect, &verifier).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pkce_matches_the_rfc_example() {
        // RFC 7636, Appendix B.
        assert_eq!(
            code_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
            "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
        );
        let v = code_verifier();
        assert_eq!(v.len(), 64);
        assert!(v
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b)));
    }

    #[test]
    fn redirects_yield_code_and_state_or_the_error() {
        assert_eq!(
            redirect_params("GET /?state=abc&code=4%2F0AX-y&scope=x HTTP/1.1").unwrap(),
            ("4/0AX-y".to_string(), "abc".to_string())
        );
        assert_eq!(
            redirect_params("GET /?error=access_denied&error_description=User+said+no HTTP/1.1")
                .unwrap_err(),
            "access_denied: User said no"
        );
        assert!(redirect_params("GET /favicon.ico HTTP/1.1").is_err());
    }

    #[test]
    fn authorize_urls_carry_pkce_and_offline_access() {
        let e = endpoints(MailProvider::Gmail).unwrap();
        let url = authorize_url(
            &e,
            "id.apps",
            "http://127.0.0.1:5000",
            "chal",
            "st",
            "me@gmail.com",
        );
        assert!(url.starts_with("https://accounts.google.com/o/oauth2/v2/auth?client_id=id.apps&"));
        assert!(url.contains("redirect_uri=http%3A%2F%2F127.0.0.1%3A5000"));
        assert!(url.contains("scope=https%3A%2F%2Fmail.google.com%2F"));
        assert!(url.contains("code_challenge=chal&code_challenge_method=S256"));
        assert!(url.contains("access_type=offline&prompt=consent"));
        assert!(url.contains("login_hint=me%40gmail.com"));
        assert!(endpoints(MailProvider::Icloud).is_none());
    }

    #[test]
    fn xoauth2_answers_once_then_ends() {
        use async_imap::Authenticator;
        let mut a = Xoauth2::new("me@gmail.com", "tok");
        assert_eq!(
            a.process(b""),
            "user=me@gmail.com\x01auth=Bearer tok\x01\x01"
        );
        assert_eq!(a.process(b"{\"status\":\"400\"}"), "");
    }
}
