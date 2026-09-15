//! Per-site proxies (Settings → Proxies).
//!
//! WebKit applies `proxyConfigurations` to a whole `WKWebsiteDataStore`, not
//! to one view, so a proxied tab runs on its own store — derived from its
//! workspace's store and the proxy id, i.e. one cookie jar per workspace ×
//! proxy. The proxy is picked when a tab's WebView is built and stays for the
//! tab's lifetime: every subresource, third-party request and link followed in
//! that tab goes through it.
//!
//! A tab built without that proxy (a direct tab following a link to a proxied
//! site, a redirect) would connect directly, so
//! `webView:decidePolicyForNavigationAction:decisionHandler:` is swapped — same
//! runtime patching as `download_patch` — to cancel such main-frame
//! navigations and hand the URL to the tab's reopen callback.

use std::collections::HashMap;
use std::ffi::CStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, OnceLock, RwLock};

use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{msg_send, sel};

use crate::config::{ProxyKind, ProxyRule};

/// `WKNavigationActionPolicy` — Cancel = 0, Allow = 1.
const POLICY_CANCEL: isize = 0;

type PolicyImp =
    extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, *mut AnyObject, *mut AnyObject);

/// wry's original IMP, called for every navigation we don't cancel.
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);

/// Enabled rules with a usable endpoint.
static RULES: RwLock<Vec<ProxyRule>> = RwLock::new(Vec::new());

struct TabEntry {
    /// Rule the WebView was built with.
    proxy_id: Option<[u8; 16]>,
    reopen: Box<dyn Fn(String) + Send>,
}

/// Live tab WebViews, keyed by WKWebView pointer.
static TABS: OnceLock<Mutex<HashMap<usize, TabEntry>>> = OnceLock::new();

fn tabs() -> &'static Mutex<HashMap<usize, TabEntry>> {
    TABS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Replace the active rule set. Disabled rules and endpoints wry would panic
/// on are dropped here.
pub fn set_rules(rules: &[ProxyRule]) {
    let usable: Vec<ProxyRule> = rules
        .iter()
        .filter(|r| r.enabled && valid_endpoint(r))
        .cloned()
        .collect();
    // Identified data stores and `proxyConfigurations` are macOS 14+; below
    // that wry would set the key on the default store and WebKit throws.
    let supported = usable.is_empty()
        || objc2_foundation::NSProcessInfo::processInfo()
            .operatingSystemVersion()
            .majorVersion
            >= 14;
    if !supported {
        tracing::warn!("per-site proxies need macOS 14 or later — ignored");
    }
    *RULES.write().unwrap() = if supported { usable } else { Vec::new() };
}

/// The rule a tab loading `url` should be built with.
pub fn rule_for(url: &str) -> Option<ProxyRule> {
    find(&RULES.read().unwrap(), url).cloned()
}

/// Store for a proxied, non-isolated tab: workspace store id (zeros for
/// WebKit's default store) XOR proxy id — stable across launches.
pub fn store_id(workspace_store: Option<[u8; 16]>, rule: &ProxyRule) -> [u8; 16] {
    let ws = workspace_store.unwrap_or_default();
    std::array::from_fn(|i| ws[i] ^ rule.id[i])
}

pub fn wry_config(rule: &ProxyRule) -> wry::ProxyConfig {
    let endpoint = wry::ProxyEndpoint {
        host: rule.host.clone(),
        port: rule.port.to_string(),
    };
    match rule.kind {
        ProxyKind::Socks5 => wry::ProxyConfig::Socks5(endpoint),
        ProxyKind::Http => wry::ProxyConfig::Http(endpoint),
    }
}

/// Track a tab WebView built with `proxy_id`. `reopen` receives main-frame
/// URLs that need a different proxy.
pub fn register(ptr: usize, proxy_id: Option<[u8; 16]>, reopen: impl Fn(String) + Send + 'static) {
    tabs().lock().unwrap().insert(
        ptr,
        TabEntry {
            proxy_id,
            reopen: Box::new(reopen),
        },
    );
}

pub fn unregister(ptr: usize) {
    tabs().lock().unwrap().remove(&ptr);
}

/// Install the navigation-action override on the delegate class of
/// `webview_ptr`. Idempotent; the first call wins.
pub fn inject_from_webview(webview_ptr: usize) {
    if ORIGINAL.load(Ordering::SeqCst) != 0 {
        return;
    }
    unsafe {
        let wv = webview_ptr as *mut AnyObject;
        let delegate: *mut AnyObject = msg_send![&*wv, navigationDelegate];
        if delegate.is_null() {
            tracing::debug!("navigationDelegate is null — proxy navigation policy not patched");
            return;
        }
        let class: *const AnyClass = msg_send![&*delegate, class];
        let previous = objc2::ffi::class_replaceMethod(
            class as *mut _,
            sel!(webView:decidePolicyForNavigationAction:decisionHandler:),
            std::mem::transmute::<PolicyImp, unsafe extern "C-unwind" fn()>(decide_policy),
            c"v@:@@@?".as_ptr(),
        );
        match previous {
            Some(imp) => {
                ORIGINAL.store(imp as usize, Ordering::SeqCst);
                tracing::debug!("Proxy navigation policy installed");
            }
            None => tracing::warn!(
                "decidePolicyForNavigationAction had no previous IMP — policy not patched"
            ),
        }
    }
}

extern "C-unwind" fn decide_policy(
    this: *mut AnyObject,
    sel: Sel,
    webview: *mut AnyObject,
    action: *mut AnyObject,
    handler: *mut AnyObject,
) {
    if unsafe { reopened_elsewhere(webview, action) } {
        unsafe {
            let block: &block2::Block<dyn Fn(isize)> =
                &*(handler as *const block2::Block<dyn Fn(isize)>);
            block.call((POLICY_CANCEL,));
        }
        return;
    }
    let orig = ORIGINAL.load(Ordering::SeqCst);
    if orig == 0 {
        return;
    }
    let orig: PolicyImp = unsafe { std::mem::transmute(orig) };
    orig(this, sel, webview, action, handler);
}

/// Main-frame navigation of a tracked tab to a site whose proxy is not the one
/// the tab was built with: hand the URL to the tab's reopen callback and
/// return true so this navigation is cancelled.
unsafe fn reopened_elsewhere(webview: *mut AnyObject, action: *mut AnyObject) -> bool {
    if action.is_null() {
        return false;
    }
    // nil targetFrame = new-window request, handled by wry's window callbacks.
    let frame: *mut AnyObject = msg_send![&*action, targetFrame];
    if frame.is_null() {
        return false;
    }
    let main_frame: bool = msg_send![&*frame, isMainFrame];
    if !main_frame {
        return false;
    }
    let request: *mut AnyObject = msg_send![&*action, request];
    if request.is_null() {
        return false;
    }
    let ns_url: *mut AnyObject = msg_send![&*request, URL];
    if ns_url.is_null() {
        return false;
    }
    let abs: *mut AnyObject = msg_send![&*ns_url, absoluteString];
    if abs.is_null() {
        return false;
    }
    let bytes: *const u8 = msg_send![&*abs, UTF8String];
    if bytes.is_null() {
        return false;
    }
    let url = CStr::from_ptr(bytes.cast()).to_string_lossy().into_owned();
    let tabs = tabs().lock().unwrap();
    let Some(tab) = tabs.get(&(webview as usize)) else {
        return false;
    };
    let Some(rule) = rule_for(&url) else {
        return false;
    };
    if tab.proxy_id == Some(rule.id) {
        return false;
    }
    tracing::debug!(%url, "navigation needs another proxy — reopening in a proxied tab");
    (tab.reopen)(url);
    true
}

fn find<'a>(rules: &'a [ProxyRule], url: &str) -> Option<&'a ProxyRule> {
    let host = url_host(url)?;
    rules
        .iter()
        .find(|r| r.sites.iter().any(|site| site_matches(&host, site)))
}

/// Lowercased host of an http(s) URL; `None` for about:, data:, etc.
fn url_host(url: &str) -> Option<String> {
    let (scheme, rest) = url.split_once("://")?;
    (scheme.eq_ignore_ascii_case("http") || scheme.eq_ignore_ascii_case("https"))
        .then(|| bare_host(rest))
}

/// `user@Host.com:8080/path?q` → `host.com`; `[::1]:80` → `::1`.
fn bare_host(s: &str) -> String {
    let s = s.split(['/', '?', '#']).next().unwrap_or(s);
    let s = s.rsplit_once('@').map_or(s, |(_, host)| host);
    let s = match s.strip_prefix('[') {
        Some(v6) => v6.split(']').next().unwrap_or(v6),
        None => s.split(':').next().unwrap_or(s),
    };
    s.to_ascii_lowercase()
}

/// Sites are loose on purpose: `example.com`, `www.example.com` and a pasted
/// `https://www.example.com/path` all mean example.com plus its subdomains.
fn site_matches(host: &str, site: &str) -> bool {
    let site = site.trim();
    let site = bare_host(site.split_once("://").map_or(site, |(_, rest)| rest));
    let site = site.strip_prefix("www.").unwrap_or(&site);
    !site.is_empty()
        && (host == site
            || host
                .strip_suffix(site)
                .is_some_and(|sub| sub.ends_with('.')))
}

/// Host name or IP literal — mirrors the settings UI check. wry unwraps the
/// endpoint it builds from these, so nothing else may reach it.
fn valid_endpoint(rule: &ProxyRule) -> bool {
    rule.port != 0
        && !rule.host.is_empty()
        && rule
            .host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ':'))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rule(id: u8, host: &str, sites: &[&str]) -> ProxyRule {
        ProxyRule {
            id: [id; 16],
            enabled: true,
            kind: ProxyKind::Socks5,
            host: host.to_string(),
            port: 1080,
            sites: sites.iter().map(|s| s.to_string()).collect(),
        }
    }

    #[test]
    fn sites_match_host_and_subdomains() {
        let rules = [
            rule(1, "127.0.0.1", &["https://www.Example.com/some/path"]),
            rule(2, "127.0.0.1", &["", "  other.org  "]),
        ];
        let matched = |url: &str| find(&rules, url).map(|r| r.id[0]);
        assert_eq!(matched("https://example.com/"), Some(1));
        assert_eq!(matched("http://www.example.com:8080/x?y#z"), Some(1));
        assert_eq!(matched("https://a.b.EXAMPLE.com"), Some(1));
        assert_eq!(matched("https://user@other.org/"), Some(2));
        assert_eq!(matched("https://notexample.com"), None);
        assert_eq!(matched("https://example.com.evil.net"), None);
        assert_eq!(matched("https://example.com@evil.net/"), None);
        assert_eq!(matched("about:blank"), None);
        assert_eq!(matched("ftp://example.com"), None);
    }

    #[test]
    fn endpoints_are_validated() {
        assert!(valid_endpoint(&rule(1, "proxy.local", &[])));
        assert!(valid_endpoint(&rule(1, "::1", &[])));
        assert!(!valid_endpoint(&rule(1, "", &[])));
        assert!(!valid_endpoint(&rule(1, "proxy host", &[])));
        assert!(!valid_endpoint(&rule(1, "a\0b", &[])));
        let mut zero_port = rule(1, "127.0.0.1", &[]);
        zero_port.port = 0;
        assert!(!valid_endpoint(&zero_port));
    }
}
