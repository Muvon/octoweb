//! True async JS evaluation for WKWebView — awaits Promises.
//!
//! wry's `evaluate_script_with_callback` uses `evaluateJavaScript:`, which
//! hands the *Promise object* to the completion handler (unserialisable →
//! empty result) instead of awaiting it. WebKit's `callAsyncJavaScript:`
//! wraps the script in an async function and resolves a returned promise
//! before invoking the completion handler — exactly what the actionability
//! harness (dom_actions.rs), `browser_wait`, and the navigate readiness
//! probe need.
//!
//! Runs in the page content world (`WKContentWorld.pageWorld`) because the
//! scripts touch page-world state (`__octoweb_refs`, the observability
//! buffers, page JS itself).

use objc2::msg_send;
use objc2::runtime::AnyObject;
use wry::WebViewExtMacOS;

/// Evaluate `expr` (a JS *expression*; promises are awaited) in the tab's
/// main frame. The callback receives the JSON-encoded result — same
/// encoding wry produces (strings arrive quoted) — or an error message.
/// Must be called on the main thread (WebView is main-thread-only).
pub fn eval_async_expr(
    webview: &wry::WebView,
    expr: &str,
    callback: impl Fn(Result<String, String>) + 'static,
) {
    // Parenthesised on the same line as `return` — prevents ASI from
    // turning `return\nnew Promise(...)` into `return;`.
    let body = format!("return ({expr});");

    let handler = block2::RcBlock::new(move |result: *mut AnyObject, error: *mut AnyObject| {
        if !error.is_null() {
            let desc = unsafe {
                let d: *mut AnyObject = msg_send![&*error, localizedDescription];
                if d.is_null() {
                    "JS evaluation failed".to_string()
                } else {
                    let bytes: *const u8 = msg_send![&*d, UTF8String];
                    if bytes.is_null() {
                        "JS evaluation failed".to_string()
                    } else {
                        std::ffi::CStr::from_ptr(bytes.cast())
                            .to_string_lossy()
                            .into_owned()
                    }
                }
            };
            callback(Err(desc));
            return;
        }
        if result.is_null() {
            // Script resolved to undefined/null.
            callback(Ok(String::new()));
            return;
        }
        // JSON-encode the fragment the way wry does, so downstream parsing
        // (quoted strings, JSON payloads) stays identical across both paths.
        let json = unsafe {
            let data: *mut AnyObject = msg_send![
                objc2::class!(NSJSONSerialization),
                dataWithJSONObject: &*result,
                options: 4usize, // NSJSONWritingFragmentsAllowed
                error: std::ptr::null_mut::<*mut AnyObject>()
            ];
            if data.is_null() {
                None
            } else {
                let len: usize = msg_send![&*data, length];
                let bytes: *const u8 = msg_send![&*data, bytes];
                Some(String::from_utf8_lossy(std::slice::from_raw_parts(bytes, len)).into_owned())
            }
        };
        match json {
            Some(s) => callback(Ok(s)),
            None => callback(Err("result not JSON-serialisable".into())),
        }
    });

    unsafe {
        let wk = webview.webview();
        let ns_body = objc2_foundation::NSString::from_str(&body);
        let world: *mut AnyObject = msg_send![objc2::class!(WKContentWorld), pageWorld];
        let nil_args: *const AnyObject = std::ptr::null();
        let nil_frame: *const AnyObject = std::ptr::null();
        let _: () = msg_send![
            &*wk,
            callAsyncJavaScript: &*ns_body,
            arguments: nil_args,
            inFrame: nil_frame,
            inContentWorld: world,
            completionHandler: &*handler
        ];
    }
}
