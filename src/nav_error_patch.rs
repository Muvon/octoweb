//! Injects `didFailProvisionalNavigation:withError:` and `didFailNavigation:withError:`
//! into wry's `WryNavigationDelegate` class at runtime via `class_addMethod`.
//!
//! wry 0.54 does not implement these WKNavigationDelegate methods, so DNS failures,
//! timeouts, SSL errors, etc. produce a blank page with no callback. This patch adds
//! them once (guarded by `std::sync::Once`) and routes errors to per-WebView callbacks
//! stored in a global registry keyed by WKWebView pointer.
//!
//! # Usage
//! After building a tab WebView, call `register(wkwebview_ptr, callback)`.
//! On tab close, call `unregister(wkwebview_ptr)` to avoid dangling entries.

use std::collections::HashMap;
use std::ffi::{c_long, CStr};
use std::sync::{Mutex, OnceLock};

use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{msg_send, sel};

type ErrorCallback = Box<dyn Fn(String, i64) + Send>;

// Global registry: WKWebView pointer (as usize) → error callback
static REGISTRY: OnceLock<Mutex<HashMap<usize, ErrorCallback>>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<usize, ErrorCallback>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register an error callback for a WKWebView instance.
/// `ptr` is the raw WKWebView pointer cast to usize.
/// Called from the main thread after building each tab WebView.
pub fn register(ptr: usize, callback: impl Fn(String, i64) + Send + 'static) {
    #[cfg(debug_assertions)]
    eprintln!(
        "[nav_error_patch] Registering callback for webview ptr={}",
        ptr
    );
    registry().lock().unwrap().insert(ptr, Box::new(callback));
}

/// Remove the callback for a WKWebView that is being destroyed.
pub fn unregister(ptr: usize) {
    registry().lock().unwrap().remove(&ptr);
}

/// Inject error methods by getting the delegate class from a WebView instance.
/// Call this after the first WebView is created, passing the WKWebView pointer.
pub fn inject_from_webview(webview_ptr: usize) {
    static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    if DONE.load(std::sync::atomic::Ordering::SeqCst) {
        return;
    }

    #[cfg(debug_assertions)]
    eprintln!(
        "[nav_error_patch] Attempting to get delegate class from webview {:p}",
        webview_ptr as *mut ()
    );

    // Get the navigation delegate from the WKWebView
    unsafe {
        let wv: *mut AnyObject = webview_ptr as *mut AnyObject;
        let delegate: *mut AnyObject = msg_send![wv, navigationDelegate];

        if delegate.is_null() {
            #[cfg(debug_assertions)]
            eprintln!("[nav_error_patch] navigationDelegate is null");
            return;
        }

        // Get the class of the delegate
        let delegate_class: *const AnyClass = msg_send![delegate, class];
        let class_name: *const i8 = msg_send![delegate_class, className];
        let name = std::ffi::CStr::from_ptr(class_name);
        #[cfg(debug_assertions)]
        eprintln!("[nav_error_patch] Delegate class: {:?}", name);

        // Inject methods
        let types = c"v@:@@@";
        objc2::ffi::class_addMethod(
            delegate_class as *mut _,
            sel!(webView:didFailProvisionalNavigation:withError:),
            std::mem::transmute::<
                extern "C-unwind" fn(
                    *mut AnyObject,
                    Sel,
                    *mut AnyObject,
                    *mut AnyObject,
                    *mut AnyObject,
                ),
                unsafe extern "C-unwind" fn(),
            >(did_fail_provisional),
            types.as_ptr(),
        );
        objc2::ffi::class_addMethod(
            delegate_class as *mut _,
            sel!(webView:didFailNavigation:withError:),
            std::mem::transmute::<
                extern "C-unwind" fn(
                    *mut AnyObject,
                    Sel,
                    *mut AnyObject,
                    *mut AnyObject,
                    *mut AnyObject,
                ),
                unsafe extern "C-unwind" fn(),
            >(did_fail),
            types.as_ptr(),
        );

        #[cfg(debug_assertions)]
        eprintln!("[nav_error_patch] Methods injected via webview delegate");
        DONE.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

/// C callback for `webView:didFailProvisionalNavigation:withError:`.
/// Fires for DNS failures, timeouts, SSL errors — anything that fails before the
/// server responds (provisional navigation phase).
extern "C-unwind" fn did_fail_provisional(
    _delegate: *mut AnyObject,
    _sel: Sel,
    webview: *mut AnyObject,
    _navigation: *mut AnyObject,
    error: *mut AnyObject,
) {
    fire_error_callback(webview, error);
}

/// C callback for `webView:didFailNavigation:withError:`.
/// Fires for errors that occur after the server has started responding.
extern "C-unwind" fn did_fail(
    _delegate: *mut AnyObject,
    _sel: Sel,
    webview: *mut AnyObject,
    _navigation: *mut AnyObject,
    error: *mut AnyObject,
) {
    fire_error_callback(webview, error);
}

fn fire_error_callback(webview: *mut AnyObject, error: *mut AnyObject) {
    if webview.is_null() || error.is_null() {
        return;
    }

    // Extract error code (NSInteger) and URL string from NSError
    let code: c_long = unsafe { msg_send![&*error, code] };
    #[cfg(debug_assertions)]
    eprintln!(
        "[nav_error_patch] didFail callback fired! code={}, webview={:p}",
        code, webview
    );

    // Get the failing URL from NSError.userInfo[NSURLErrorFailingURLStringErrorKey]
    // Falls back to empty string — the Rust side already knows the URL from NavigateTo.
    let url_str: String = unsafe {
        let user_info: *mut AnyObject = msg_send![&*error, userInfo];
        if user_info.is_null() {
            String::new()
        } else {
            let key: *mut AnyObject = msg_send![
                objc2::class!(NSString),
                stringWithUTF8String: c"NSErrorFailingURLStringKey".as_ptr()
            ];
            let val: *mut AnyObject = msg_send![&*user_info, objectForKey: key];
            if val.is_null() {
                String::new()
            } else {
                let bytes: *const u8 = msg_send![&*val, UTF8String];
                if bytes.is_null() {
                    String::new()
                } else {
                    CStr::from_ptr(bytes.cast()).to_string_lossy().into_owned()
                }
            }
        }
    };

    let ptr = webview as usize;
    if let Ok(guard) = registry().lock() {
        if let Some(cb) = guard.get(&ptr) {
            cb(url_str, code as i64);
        }
    }
}
