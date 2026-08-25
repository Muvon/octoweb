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
    let wk = webview.webview();
    eval_async_expr_ptr(
        objc2::rc::Retained::as_ptr(&wk) as *mut AnyObject,
        expr,
        callback,
    );
}

/// [`eval_async_expr`] on a raw WKWebView pointer — for continuations that
/// run after the borrowed `wry::WebView` is out of scope (native action
/// chains: locate → trusted event → effect probe). The caller must keep the
/// view retained until the callback fires.
pub fn eval_async_expr_ptr(
    wk: *mut AnyObject,
    expr: &str,
    callback: impl Fn(Result<String, String>) + 'static,
) {
    // Parenthesised on the same line as `return` — prevents ASI from
    // turning `return\nnew Promise(...)` into `return;`.
    let body = format!("return ({expr});");

    let handler = block2::RcBlock::new(move |result: *mut AnyObject, error: *mut AnyObject| {
        if !error.is_null() {
            callback(Err(unsafe { describe_error(error) }));
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

/// Read an NSString field of an ObjC object as a Rust String (None if nil).
unsafe fn ns_string(obj: *mut AnyObject) -> Option<String> {
    if obj.is_null() {
        return None;
    }
    let bytes: *const u8 = msg_send![&*obj, UTF8String];
    if bytes.is_null() {
        return None;
    }
    Some(
        std::ffi::CStr::from_ptr(bytes.cast())
            .to_string_lossy()
            .into_owned(),
    )
}

/// Turn a WKError into the message the AI needs. `localizedDescription` is
/// always the useless "A JavaScript exception occurred"; the real exception
/// text, line and column live in `userInfo` (WKJavaScriptExceptionMessage &
/// friends). Surfacing them turns four blind retries into one fix.
unsafe fn describe_error(error: *mut AnyObject) -> String {
    let user_info: *mut AnyObject = msg_send![&*error, userInfo];
    let field = |key: &str| -> Option<*mut AnyObject> {
        if user_info.is_null() {
            return None;
        }
        let k = objc2_foundation::NSString::from_str(key);
        let v: *mut AnyObject = msg_send![&*user_info, objectForKey: &*k];
        (!v.is_null()).then_some(v)
    };
    if let Some(msg) = field("WKJavaScriptExceptionMessage").and_then(|m| ns_string(m)) {
        let line: isize = field("WKJavaScriptExceptionLineNumber")
            .map(|n| msg_send![&*n, integerValue])
            .unwrap_or(0);
        let col: isize = field("WKJavaScriptExceptionColumnNumber")
            .map(|n| msg_send![&*n, integerValue])
            .unwrap_or(0);
        return if line > 0 {
            format!("{msg} (line {line}:{col})")
        } else {
            msg
        };
    }
    let desc: *mut AnyObject = msg_send![&*error, localizedDescription];
    ns_string(desc).unwrap_or_else(|| "JS evaluation failed".to_string())
}
