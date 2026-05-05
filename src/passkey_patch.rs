//! Enables WebAuthn / passkey support on a WKWebView's WKPreferences.
//!
//! `wry` 0.55 builds a plain `WKWebView` whose configuration leaves WebAuthn
//! disabled — sites like GitHub then report "partial passkey support" and the
//! `navigator.credentials.get()` call fails. WebKit gates the API behind two
//! private (SPI) preference toggles:
//!
//! - `WebAuthenticationEnabled`        — experimental feature key, flipped via
//!   `[WKPreferences _setEnabled:forFeature:]` after locating the matching
//!   `_WKExperimentalFeature` in `[WKPreferences _experimentalFeatures]`.
//! - `WebAuthenticationModernEnabled`  — modern (ASAuthorization-backed)
//!   path, same mechanism. Required on macOS 13+ for the cross-device QR flow.
//!
//! Both are SPI; using them is App Store-incompatible but fine for direct
//! distribution. Without the platform-authenticator entitlement
//! (`com.apple.developer.web-browser.public-key-credential`) macOS will
//! offer the QR/cross-device flow only — Touch ID/iCloud Keychain credentials
//! remain Safari-exclusive until Apple grants the entitlement.
//!
//! # Safety
//! `prefs` must be a non-null `WKPreferences *`. All ObjC selectors used here
//! are stable WebKit SPI on macOS 11+; missing selectors are tolerated (we
//! check `respondsToSelector:` before invoking).

use objc2::runtime::AnyObject;
use objc2::{msg_send, sel};

/// Toggle WebAuthn-related WKPreferences flags on the given preferences object.
///
/// Iterates `_experimentalFeatures` and enables any feature whose `key` matches
/// our target list. Silently no-ops on macOS versions where the SPI is missing.
///
/// # Safety
/// `prefs` must be a valid `WKPreferences *` retained by the caller for the
/// duration of this call. Must be invoked on the main thread.
pub unsafe fn enable(prefs: *mut AnyObject) {
    if prefs.is_null() {
        return;
    }

    // Targets to flip. WebKit's UnifiedWebPreferences.yaml lists these as the
    // experimental feature keys covering the WebAuthn surface.
    const TARGETS: &[&str] = &["WebAuthenticationEnabled", "WebAuthenticationModernEnabled"];

    let responds: bool = msg_send![prefs, respondsToSelector: sel!(_experimentalFeatures)];
    if !responds {
        tracing::debug!("WKPreferences._experimentalFeatures unavailable; skipping passkey patch");
        return;
    }

    let features: *mut AnyObject = msg_send![prefs, _experimentalFeatures];
    if features.is_null() {
        return;
    }

    let count: usize = msg_send![features, count];
    let mut enabled = 0usize;
    for i in 0..count {
        let feature: *mut AnyObject = msg_send![features, objectAtIndex: i];
        if feature.is_null() {
            continue;
        }
        let key_ns: *mut AnyObject = msg_send![feature, key];
        if key_ns.is_null() {
            continue;
        }
        let utf8: *const i8 = msg_send![key_ns, UTF8String];
        if utf8.is_null() {
            continue;
        }
        let key = std::ffi::CStr::from_ptr(utf8).to_string_lossy();
        if TARGETS.iter().any(|t| key == *t) {
            let _: () = msg_send![prefs, _setEnabled: true, forFeature: feature];
            enabled += 1;
        }
    }

    tracing::info!(enabled, "WebAuthn experimental features toggled");
}
