//! Injects WKUIDelegate methods for JavaScript alert/confirm/prompt dialogs
//! into wry's delegate class at runtime via `class_addMethod`.
//!
//! WebKit's `CompletionHandlerCallChecker` aborts the process via a CFRunLoop
//! observer if the completion handler isn't invoked before the delegate method
//! returns. We show a native NSAlert modal synchronously (its own run loop
//! spin), then call the completion handler before returning. This satisfies
//! the checker while still presenting real UI to the user.

use std::collections::HashMap;
use std::ffi::CStr;
use std::sync::{Mutex, OnceLock};

use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{msg_send, sel};

/// Dialog type surfaced to the main event loop.
#[derive(Debug, Clone)]
pub enum DialogType {
    Alert,
    Confirm,
    Prompt,
}

/// Info sent through AppEvent (Send-safe, no raw pointers).
#[derive(Debug, Clone)]
pub struct DialogInfo {
    pub tab_id: usize,
    pub dialog_type: DialogType,
    pub message: String,
}

/// Callback type: receives DialogInfo when a dialog fires.
type DialogCallback = Box<dyn Fn(DialogInfo) + Send>;

// Global registry: WKWebView pointer → (tab_id, callback)
static REGISTRY: OnceLock<Mutex<HashMap<usize, (usize, DialogCallback)>>> = OnceLock::new();

fn registry() -> &'static Mutex<HashMap<usize, (usize, DialogCallback)>> {
    REGISTRY.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Register a dialog callback for a WKWebView.
pub fn register(ptr: usize, tab_id: usize, callback: impl Fn(DialogInfo) + Send + 'static) {
    tracing::debug!(ptr, tab_id, "Registering dialog callback");
    registry()
        .lock()
        .unwrap()
        .insert(ptr, (tab_id, Box::new(callback)));
}

/// Remove the callback for a WKWebView that is being destroyed.
pub fn unregister(ptr: usize) {
    registry().lock().unwrap().remove(&ptr);
}

/// Inject UI delegate methods by getting the delegate class from a WebView instance.
pub fn inject_from_webview(webview_ptr: usize) {
    static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

    if DONE.load(std::sync::atomic::Ordering::SeqCst) {
        return;
    }

    tracing::debug!(ptr = webview_ptr, "Injecting JS dialog delegate methods");

    unsafe {
        let wv: *mut AnyObject = webview_ptr as *mut AnyObject;
        let delegate: *mut AnyObject = msg_send![wv, UIDelegate];

        if delegate.is_null() {
            tracing::debug!("UIDelegate is null — cannot inject dialog methods");
            return;
        }

        let delegate_class: *const AnyClass = msg_send![delegate, class];
        let class_name: *const i8 = msg_send![delegate_class, className];
        let name = CStr::from_ptr(class_name);
        tracing::debug!(?name, "UI delegate class for dialog injection");

        // Alert: webView:runJavaScriptAlertPanelWithMessage:initiatedByFrame:completionHandler:
        let types = c"v@:@@@@";
        objc2::ffi::class_addMethod(
            delegate_class as *mut _,
            sel!(webView:runJavaScriptAlertPanelWithMessage:initiatedByFrame:completionHandler:),
            std::mem::transmute::<
                extern "C-unwind" fn(
                    *mut AnyObject,
                    Sel,
                    *mut AnyObject,
                    *mut AnyObject,
                    *mut AnyObject,
                    *mut AnyObject,
                ),
                unsafe extern "C-unwind" fn(),
            >(handle_alert),
            types.as_ptr(),
        );

        // Confirm
        objc2::ffi::class_addMethod(
            delegate_class as *mut _,
            sel!(webView:runJavaScriptConfirmPanelWithMessage:initiatedByFrame:completionHandler:),
            std::mem::transmute::<
                extern "C-unwind" fn(
                    *mut AnyObject,
                    Sel,
                    *mut AnyObject,
                    *mut AnyObject,
                    *mut AnyObject,
                    *mut AnyObject,
                ),
                unsafe extern "C-unwind" fn(),
            >(handle_confirm),
            types.as_ptr(),
        );

        // Prompt: extra defaultText arg → "v@:@@@@@"
        let prompt_types = c"v@:@@@@@";
        objc2::ffi::class_addMethod(
            delegate_class as *mut _,
            sel!(webView:runJavaScriptTextInputPanelWithPrompt:defaultText:initiatedByFrame:completionHandler:),
            std::mem::transmute::<
                extern "C-unwind" fn(
                    *mut AnyObject,
                    Sel,
                    *mut AnyObject,
                    *mut AnyObject,
                    *mut AnyObject,
                    *mut AnyObject,
                    *mut AnyObject,
                ),
                unsafe extern "C-unwind" fn(),
            >(handle_prompt),
            prompt_types.as_ptr(),
        );

        tracing::debug!("JS dialog delegate methods injected");
        DONE.store(true, std::sync::atomic::Ordering::SeqCst);
    }
}

fn nsstring_to_string(ns: *mut AnyObject) -> String {
    if ns.is_null() {
        return String::new();
    }
    unsafe {
        let bytes: *const u8 = msg_send![&*ns, UTF8String];
        if bytes.is_null() {
            String::new()
        } else {
            CStr::from_ptr(bytes.cast()).to_string_lossy().into_owned()
        }
    }
}

/// Notify the registered callback (best-effort, no panic on poison).
fn notify(webview: *mut AnyObject, dialog_type: DialogType, message: String) {
    let ptr = webview as usize;
    if let Ok(guard) = registry().lock() {
        if let Some((tab_id, cb)) = guard.get(&ptr) {
            cb(DialogInfo {
                tab_id: *tab_id,
                dialog_type,
                message,
            });
        }
    }
}

extern "C-unwind" fn handle_alert(
    _delegate: *mut AnyObject,
    _sel: Sel,
    webview: *mut AnyObject,
    message: *mut AnyObject,
    _frame: *mut AnyObject,
    completion_handler: *mut AnyObject,
) {
    let msg = nsstring_to_string(message);
    tracing::debug!(ptr = webview as usize, %msg, "JS alert dialog");
    unsafe {
        // Show native dialog (synchronous — its own run loop spin satisfies
        // CompletionHandlerCallChecker before we call the block).
        let mtm = objc2::MainThreadMarker::new_unchecked();
        let alert = objc2_app_kit::NSAlert::new(mtm);
        alert.setMessageText(&*(message as *const objc2_foundation::NSString));
        alert.runModal();

        let block = completion_handler as *const block2::Block<dyn Fn()>;
        (*block).call(());
    }
    notify(webview, DialogType::Alert, msg);
}

extern "C-unwind" fn handle_confirm(
    _delegate: *mut AnyObject,
    _sel: Sel,
    webview: *mut AnyObject,
    message: *mut AnyObject,
    _frame: *mut AnyObject,
    completion_handler: *mut AnyObject,
) {
    let msg = nsstring_to_string(message);
    tracing::debug!(ptr = webview as usize, %msg, "JS confirm dialog");
    let accepted = unsafe {
        let mtm = objc2::MainThreadMarker::new_unchecked();
        let alert = objc2_app_kit::NSAlert::new(mtm);
        alert.setMessageText(&*(message as *const objc2_foundation::NSString));
        alert.addButtonWithTitle(objc2_foundation::ns_string!("OK"));
        alert.addButtonWithTitle(objc2_foundation::ns_string!("Cancel"));
        alert.runModal() == objc2_app_kit::NSAlertFirstButtonReturn
    };
    unsafe {
        let block = completion_handler as *const block2::Block<dyn Fn(objc2::runtime::Bool)>;
        (*block).call((if accepted {
            objc2::runtime::Bool::YES
        } else {
            objc2::runtime::Bool::NO
        },));
    }
    notify(webview, DialogType::Confirm, msg);
}

extern "C-unwind" fn handle_prompt(
    _delegate: *mut AnyObject,
    _sel: Sel,
    webview: *mut AnyObject,
    prompt: *mut AnyObject,
    default_text: *mut AnyObject,
    _frame: *mut AnyObject,
    completion_handler: *mut AnyObject,
) {
    let msg = nsstring_to_string(prompt);
    let default = nsstring_to_string(default_text);
    tracing::debug!(ptr = webview as usize, %msg, %default, "JS prompt dialog");
    let result: Option<String> = unsafe {
        use objc2_app_kit::NSTextField;
        use objc2_foundation::{NSPoint, NSRect, NSSize};
        let mtm = objc2::MainThreadMarker::new_unchecked();
        let alert = objc2_app_kit::NSAlert::new(mtm);
        alert.setMessageText(&*(prompt as *const objc2_foundation::NSString));
        alert.addButtonWithTitle(objc2_foundation::ns_string!("OK"));
        alert.addButtonWithTitle(objc2_foundation::ns_string!("Cancel"));

        // Text input field
        let frame = NSRect {
            origin: NSPoint { x: 0.0, y: 0.0 },
            size: NSSize {
                width: 300.0,
                height: 24.0,
            },
        };
        let field = NSTextField::initWithFrame(mtm.alloc::<NSTextField>(), frame);
        if !default_text.is_null() {
            field.setStringValue(&*(default_text as *const objc2_foundation::NSString));
        }
        alert.setAccessoryView(Some(&field));

        if alert.runModal() == objc2_app_kit::NSAlertFirstButtonReturn {
            Some(field.stringValue().to_string())
        } else {
            None
        }
    };
    unsafe {
        let block = completion_handler as *const block2::Block<dyn Fn(*mut AnyObject)>;
        match result {
            Some(text) => {
                let ns: *mut AnyObject = msg_send![
                    objc2::class!(NSString),
                    stringWithUTF8String: text.as_ptr() as *const i8
                ];
                (*block).call((ns,));
            }
            None => (*block).call((std::ptr::null_mut(),)),
        }
    }
    notify(webview, DialogType::Prompt, msg);
}
