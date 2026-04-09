//! Injects WKUIDelegate methods for JavaScript alert/confirm/prompt dialogs
//! into wry's delegate class at runtime via `class_addMethod`.
//!
//! Without this, WKWebView silently dismisses JS dialogs (no callback fires).
//! This patch captures them and routes to the main event loop so MCP can
//! respond (accept/dismiss/provide text).
//!
//! # Usage
//! After building a tab WebView, call `register(wkwebview_ptr, tab_id, callback)`.
//! On tab close, call `unregister(wkwebview_ptr)`.

use std::collections::HashMap;
use std::ffi::CStr;
use std::sync::{Mutex, OnceLock};

use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{msg_send, sel};

/// Dialog type surfaced to MCP.
#[derive(Debug, Clone)]
pub enum DialogType {
    Alert,
    Confirm,
    Prompt { default_text: String },
}

/// A pending dialog whose completion handler has not yet been called.
/// Stored in PENDING_DIALOGS, resolved by `resolve()`.
struct PendingCompletion {
    dialog_type: DialogType,
    /// Retained ObjC block pointer — must be called exactly once, then released.
    completion: *mut AnyObject,
}

// SAFETY: We only access completions from the main thread (via event loop).
unsafe impl Send for PendingCompletion {}

/// Info sent through AppEvent (all Send-safe, no raw pointers).
#[derive(Debug, Clone)]
pub struct DialogInfo {
    pub tab_id: usize,
    pub dialog_type: DialogType,
    pub message: String,
    pub dialog_id: u64,
}

// Global: dialog_id → PendingCompletion
static PENDING_DIALOGS: OnceLock<Mutex<HashMap<u64, PendingCompletion>>> = OnceLock::new();
static DIALOG_COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

fn pending_dialogs() -> &'static Mutex<HashMap<u64, PendingCompletion>> {
    PENDING_DIALOGS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Resolve a pending dialog by its ID.
/// - accept=true: OK/Yes for alert/confirm, submit text for prompt
/// - accept=false: Cancel/No
pub fn resolve(dialog_id: u64, accept: bool, text: Option<&str>) -> Option<DialogInfo> {
    let pending = pending_dialogs().lock().unwrap().remove(&dialog_id)?;
    if pending.completion.is_null() {
        return None;
    }
    unsafe {
        match &pending.dialog_type {
            DialogType::Alert => {
                // completionHandler: ^(void)
                let block = pending.completion as *const block2::Block<dyn Fn()>;
                (*block).call(());
            }
            DialogType::Confirm => {
                // completionHandler: ^(BOOL)
                // ObjC BOOL is i8 on arm64. Cast to block that takes no args and use msg_send.
                // Simplest: call the block via objc2 msg_send (blocks are ObjC objects).
                let val: i8 = if accept { 1 } else { 0 };
                let _: () = msg_send![&*pending.completion, invoke: val];
            }
            DialogType::Prompt { .. } => {
                // completionHandler: ^(NSString*)
                if accept {
                    let text = text.unwrap_or("");
                    let ns_string: *mut AnyObject = msg_send![
                        objc2::class!(NSString),
                        stringWithUTF8String: text.as_ptr() as *const i8
                    ];
                    let _: () = msg_send![&*pending.completion, invoke: ns_string];
                } else {
                    let nil: *mut AnyObject = std::ptr::null_mut();
                    let _: () = msg_send![&*pending.completion, invoke: nil];
                }
            }
        }
        let _: () = msg_send![&*pending.completion, release];
    }
    None // info already consumed
}

/// Auto-dismiss a stale dialog (timeout).
pub fn dismiss(dialog_id: u64) {
    resolve(dialog_id, false, None);
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

fn fire_dialog(
    webview: *mut AnyObject,
    dialog_type: DialogType,
    message: String,
    completion_handler: *mut AnyObject,
) {
    let ptr = webview as usize;
    // Retain the completion handler so it survives past this callback
    let retained: *mut AnyObject = unsafe { msg_send![&*completion_handler, retain] };

    let dialog_id = DIALOG_COUNTER.fetch_add(1, std::sync::atomic::Ordering::SeqCst);

    if let Ok(guard) = registry().lock() {
        if let Some((tab_id, cb)) = guard.get(&ptr) {
            // Store completion handler in pending map
            pending_dialogs().lock().unwrap().insert(
                dialog_id,
                PendingCompletion {
                    dialog_type: dialog_type.clone(),
                    completion: retained,
                },
            );
            // Notify main event loop (no raw pointers in DialogInfo)
            cb(DialogInfo {
                tab_id: *tab_id,
                dialog_type,
                message,
                dialog_id,
            });
            return;
        }
    }
    // No callback registered — auto-dismiss immediately
    unsafe {
        match dialog_type {
            DialogType::Alert => {
                let block = retained as *const block2::Block<dyn Fn()>;
                (*block).call(());
            }
            DialogType::Confirm => {
                let val: i8 = 0; // reject
                let _: () = msg_send![&*retained, invoke: val];
            }
            DialogType::Prompt { .. } => {
                let nil: *mut AnyObject = std::ptr::null_mut();
                let _: () = msg_send![&*retained, invoke: nil];
            }
        }
        let _: () = msg_send![&*retained, release];
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
    fire_dialog(webview, DialogType::Alert, msg, completion_handler);
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
    fire_dialog(webview, DialogType::Confirm, msg, completion_handler);
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
    fire_dialog(
        webview,
        DialogType::Prompt {
            default_text: default,
        },
        msg,
        completion_handler,
    );
}
