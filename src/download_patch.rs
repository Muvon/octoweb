//! Honour `Content-Disposition: attachment` for navigation responses.
//!
//! wry's `webView:decidePolicyForNavigationResponse:decisionHandler:` only
//! asks WebKit "can you display this MIME type?" — so an mp4/jpeg/pdf served
//! as an attachment (every Google Drive download, most "Export" buttons) is
//! rendered inline instead of saved, and the user's "Download" click looks
//! like nothing happened. This swaps in an implementation that returns
//! `WKNavigationResponsePolicyDownload` for attachments (main frame *and*
//! subframes — Drive downloads through a hidden iframe) and defers to wry's
//! original implementation for everything else. Once WebKit converts the
//! navigation, wry's download delegate takes over (destination in
//! `~/Downloads`, started/completed callbacks in main.rs).
//!
//! Same runtime-patching approach as `nav_error_patch`: resolve the delegate
//! class from a live WKWebView, `class_replaceMethod` once, keep the previous
//! IMP for the fallback.

use std::ffi::CStr;
use std::sync::atomic::{AtomicUsize, Ordering};

use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{msg_send, sel};

/// `WKNavigationResponsePolicy` — Cancel = 0, Allow = 1, Download = 2.
const POLICY_DOWNLOAD: isize = 2;

type PolicyImp =
    extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, *mut AnyObject, *mut AnyObject);

/// wry's original IMP, called for non-attachment responses. 0 until injected.
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);

/// Install the override on the navigation delegate class of `webview_ptr`.
/// Idempotent; the first call wins.
pub fn inject_from_webview(webview_ptr: usize) {
    if ORIGINAL.load(Ordering::SeqCst) != 0 {
        return;
    }
    unsafe {
        let wv = webview_ptr as *mut AnyObject;
        let delegate: *mut AnyObject = msg_send![&*wv, navigationDelegate];
        if delegate.is_null() {
            tracing::debug!("navigationDelegate is null — download policy not patched");
            return;
        }
        let class: *const AnyClass = msg_send![&*delegate, class];
        let previous = objc2::ffi::class_replaceMethod(
            class as *mut _,
            sel!(webView:decidePolicyForNavigationResponse:decisionHandler:),
            std::mem::transmute::<PolicyImp, unsafe extern "C-unwind" fn()>(decide_policy),
            c"v@:@@@?".as_ptr(),
        );
        match previous {
            Some(imp) => {
                ORIGINAL.store(imp as usize, Ordering::SeqCst);
                tracing::debug!("Attachment download policy installed");
            }
            None => tracing::warn!(
                "decidePolicyForNavigationResponse had no previous IMP — policy not patched"
            ),
        }
    }
}

extern "C-unwind" fn decide_policy(
    this: *mut AnyObject,
    sel: Sel,
    webview: *mut AnyObject,
    response: *mut AnyObject,
    handler: *mut AnyObject,
) {
    let attachment = unsafe { response_is_attachment(response) };
    if attachment {
        tracing::debug!("Navigation response is an attachment — downloading");
        unsafe {
            let block: &block2::Block<dyn Fn(isize)> =
                &*(handler as *const block2::Block<dyn Fn(isize)>);
            block.call((POLICY_DOWNLOAD,));
        }
        return;
    }
    let orig = ORIGINAL.load(Ordering::SeqCst);
    if orig == 0 {
        return;
    }
    let orig: PolicyImp = unsafe { std::mem::transmute(orig) };
    orig(this, sel, webview, response, handler);
}

/// `Content-Disposition` of a WKNavigationResponse's HTTP response, if any.
unsafe fn response_is_attachment(nav_response: *mut AnyObject) -> bool {
    if nav_response.is_null() {
        return false;
    }
    let response: *mut AnyObject = msg_send![&*nav_response, response];
    if response.is_null() {
        return false;
    }
    let is_http: bool = msg_send![&*response, isKindOfClass: objc2::class!(NSHTTPURLResponse)];
    if !is_http {
        return false;
    }
    let field = objc2_foundation::NSString::from_str("Content-Disposition");
    let value: *mut AnyObject = msg_send![&*response, valueForHTTPHeaderField: &*field];
    if value.is_null() {
        return false;
    }
    let bytes: *const u8 = msg_send![&*value, UTF8String];
    if bytes.is_null() {
        return false;
    }
    is_attachment(&CStr::from_ptr(bytes.cast()).to_string_lossy())
}

/// `attachment` disposition (RFC 6266), tolerant of casing and whitespace.
/// `inline` and bare `filename=` headers are not downloads.
pub fn is_attachment(content_disposition: &str) -> bool {
    content_disposition
        .trim_start()
        .split(';')
        .next()
        .map(|t| t.trim().eq_ignore_ascii_case("attachment"))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::is_attachment;

    #[test]
    fn attachment_detection() {
        assert!(is_attachment("attachment"));
        assert!(is_attachment("attachment; filename=\"MVI_5662.mp4\""));
        assert!(is_attachment("  Attachment ;filename*=UTF-8''x.jpg"));
        assert!(!is_attachment("inline; filename=\"x.pdf\""));
        assert!(!is_attachment("filename=\"x.pdf\""));
        assert!(!is_attachment(""));
    }
}
