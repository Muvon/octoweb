//! Native content blocking via WebKit's WKContentRuleList API.
//!
//! WKContentRuleList rules are compiled to a bytecode state machine that runs
//! in the **networking layer** — before any bytes are fetched — with zero
//! per-page JavaScript overhead. Unlike uBlock-style JS injections, these rules
//! cannot be bypassed by page JS and impose no renderer-process cost.
//!
//! On first launch WebKit compiles the embedded blocklist and caches the result
//! on disk (`~/Library/Caches/…`). Subsequent launches load from cache in
//! milliseconds. The rule list is applied to every tab WebView via
//! `WKUserContentController.addContentRuleList(_:)`.
//!
//! # Design
//! - `init(mtm)` — called once at startup; starts async compilation/lookup.
//! - `apply_to_webview(wv_ptr)` — called after each tab WebView is created.
//!   If the list is ready it applies immediately; otherwise the pointer is
//!   queued and applied in batch when the completion block fires.

use std::sync::{Mutex, OnceLock};

use objc2::runtime::AnyObject;
use objc2::{msg_send, rc::Retained};
use objc2_foundation::{MainThreadMarker, NSString};
use objc2_web_kit::{WKContentRuleList, WKContentRuleListStore};

/// Curated tracker/ad blocklist in WKContentRuleList JSON format,
/// embedded at compile time so there is no runtime file I/O.
const BLOCK_LIST_JSON: &str = include_str!("../assets/blocklist.json");

/// Identifier used to store/look up the compiled rule list in WebKit's cache.
/// Increment the suffix when the blocklist content changes to force recompilation.
const RULE_LIST_ID: &str = "octoweb-blocklist-v2";

/// Compiled WKContentRuleList retained pointer, stored as usize for
/// thread-agnostic storage (the object is only accessed on the main thread).
static RULE_LIST_PTR: OnceLock<usize> = OnceLock::new();

/// WebViews created before compilation finished — apply rules retroactively.
static PENDING: Mutex<Vec<usize>> = Mutex::new(Vec::new());

/// Initialise content blocking.
///
/// Tries to look up a cached compiled rule list first (fast path on all but
/// the very first launch). If not cached, triggers compilation. Both paths are
/// fully asynchronous; the completion block fires on the main thread and applies
/// rules to any WebViews that were created in the interim.
///
/// Must be called on the main thread before the event loop starts.
pub fn init(mtm: MainThreadMarker) {
    let store = match unsafe { WKContentRuleListStore::defaultStore(mtm) } {
        Some(s) => s,
        None => {
            tracing::warn!("WKContentRuleListStore unavailable — content blocking disabled");
            return;
        }
    };

    let identifier = NSString::from_str(RULE_LIST_ID);

    // Try cache first; if not found the completion handler gets a null list
    // and we fall through to compilation.
    let lookup_block = block2::RcBlock::new({
        let store = Retained::clone(&store);
        let identifier = Retained::clone(&identifier);
        move |list: *mut WKContentRuleList, _error: *mut objc2_foundation::NSError| {
            if !list.is_null() {
                // Cache hit — retain and store immediately.
                tracing::debug!("content rules: cache hit, applying immediately");
                store_and_apply(list);
            } else {
                // Cache miss — compile from embedded JSON.
                tracing::debug!("content rules: cache miss, compiling blocklist");
                compile(&store, &identifier);
            }
        }
    });

    unsafe {
        store.lookUpContentRuleListForIdentifier_completionHandler(
            Some(&identifier),
            Some(&*lookup_block),
        );
    }
}

/// Apply compiled content rules to a newly-created WebView.
///
/// If compilation has already finished, applies immediately.
/// Otherwise queues the pointer; the completion block will apply retroactively.
///
/// `wv_ptr` is the raw WKWebView pointer cast to usize (from
/// `objc2::rc::Retained::as_ptr(&wv.webview()) as usize`).
pub fn apply_to_webview(wv_ptr: usize) {
    if let Some(&list_ptr) = RULE_LIST_PTR.get() {
        unsafe { do_apply(wv_ptr, list_ptr) };
    } else {
        PENDING.lock().unwrap().push(wv_ptr);
    }
}

// ── Internal helpers ──────────────────────────────────────────────────────────

fn compile(store: &WKContentRuleListStore, identifier: &NSString) {
    let json = NSString::from_str(BLOCK_LIST_JSON);
    let compile_block = block2::RcBlock::new(
        |list: *mut WKContentRuleList, error: *mut objc2_foundation::NSError| {
            if list.is_null() {
                let desc: *mut AnyObject = if error.is_null() {
                    std::ptr::null_mut()
                } else {
                    unsafe { msg_send![error, localizedDescription] }
                };
                // A single malformed rule rejects the entire list, leaving the
                // browser with no blocking at all — never let that pass quietly.
                tracing::error!(
                    ?desc,
                    "content rules: compilation FAILED — ad/tracker blocking is DISABLED. \
                     One bad rule rejects the whole list; fix assets/blocklist.json"
                );
                return;
            }
            tracing::debug!("content rules: compilation succeeded");
            store_and_apply(list);
        },
    );

    unsafe {
        store.compileContentRuleListForIdentifier_encodedContentRuleList_completionHandler(
            Some(identifier),
            Some(&json),
            Some(&*compile_block),
        );
    }
}

/// Retain the rule list, store it globally, and apply to all queued WebViews.
fn store_and_apply(list: *mut WKContentRuleList) {
    // Retain the rule list so it outlives the completion block.
    let retained: Retained<WKContentRuleList> =
        unsafe { Retained::retain(list) }.expect("WKContentRuleList retain failed");
    let ptr = Retained::as_ptr(&retained) as usize;
    // Intentionally forget the Retained — the object lives for the app lifetime.
    std::mem::forget(retained);

    // Store once; if already set (shouldn't happen) just use existing.
    let _ = RULE_LIST_PTR.set(ptr);
    let actual_ptr = *RULE_LIST_PTR.get().unwrap();

    // Apply to all WebViews that were created before rules were ready.
    let pending: Vec<usize> = std::mem::take(&mut *PENDING.lock().unwrap());
    for wv_ptr in pending {
        unsafe { do_apply(wv_ptr, actual_ptr) };
    }
}

/// Add the content rule list to a WebView's WKUserContentController.
///
/// `webView.configuration` returns a copy, but the `userContentController`
/// inside is the live shared object — mutations via the copy affect the
/// running WebView. This is the standard post-creation injection pattern.
unsafe fn do_apply(wv_ptr: usize, list_ptr: usize) {
    if wv_ptr == 0 || list_ptr == 0 {
        return;
    }
    let wv = wv_ptr as *mut AnyObject;
    let config: *mut AnyObject = msg_send![wv, configuration];
    if config.is_null() {
        return;
    }
    let ucc: *mut AnyObject = msg_send![config, userContentController];
    if ucc.is_null() {
        return;
    }
    let list = list_ptr as *mut AnyObject;
    let _: () = msg_send![ucc, addContentRuleList: list];
    tracing::debug!(wv_ptr, "content rules applied to webview");
}
