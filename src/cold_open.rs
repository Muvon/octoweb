//! Captures URLs delivered before tao's event loop callback is registered.
//!
//! On cold launch (app not running, user clicks a link), macOS delivers
//! the URL via two mechanisms:
//!
//! 1. **Apple Event `kAEGetURL`** — delivered during `[NSApp finishLaunching]`
//!    which happens inside `EventLoop::new()`. We capture this by installing
//!    an `NSAppleEventManager` handler BEFORE the event loop is built.
//!
//! 2. **`application:openURLs:`** — delivered after `applicationDidFinishLaunching`.
//!    We hook tao's delegate method after `EventLoop::new()` to buffer these.
//!
//! Call `install_early()` BEFORE `EventLoop::new()` — captures kAEGetURL.
//! Call `install()` AFTER `EventLoop::new()` — hooks application:openURLs:.
//! Call `take()` on event loop ticks to drain buffered URLs.

use std::sync::{Mutex, OnceLock};

use objc2::ffi::{class_replaceMethod, objc_getClass};
use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
use objc2::{class, msg_send, sel};
use objc2_foundation::{NSArray, NSURL};

/// URLs buffered before the event loop callback was ready.
static COLD_URLS: OnceLock<Mutex<Vec<String>>> = OnceLock::new();

/// Original `application:openURLs:` IMP saved before replacement.
static ORIGINAL_IMP: OnceLock<
    unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, *mut NSArray<NSURL>),
> = OnceLock::new();

fn cold_urls() -> &'static Mutex<Vec<String>> {
    COLD_URLS.get_or_init(|| Mutex::new(Vec::new()))
}

/// Drain all buffered cold-launch URLs (returns them once, then empty).
pub fn take() -> Vec<String> {
    std::mem::take(&mut *cold_urls().lock().unwrap())
}

/// Install a `kAEGetURL` Apple Event handler BEFORE `EventLoop::new()`.
/// This captures the launch URL that macOS delivers during `[NSApp finishLaunching]`,
/// which happens inside tao's `EventLoop::new()` — before our delegate hook exists.
pub fn install_early() {
    unsafe {
        let manager: *mut AnyObject =
            msg_send![class!(NSAppleEventManager), sharedAppleEventManager];
        if manager.is_null() {
            return;
        }
        // kInternetEventClass = 'GURL', kAEGetURL = 'GURL'
        let event_class: u32 = u32::from_be_bytes(*b"GURL");
        let event_id: u32 = u32::from_be_bytes(*b"GURL");
        // Create an NSObject-based handler target.
        // We use a block-based approach via handleGetURLEvent:withReplyEvent:.
        let handler: *mut AnyObject = msg_send![class!(NSObject), new];
        // Register with manual selector — we inject the method on NSObject.
        let types = c"v@:@@";
        objc2::ffi::class_addMethod(
            class!(NSObject) as *const AnyClass as *mut AnyClass,
            sel!(octoweb_handleGetURL:withReply:),
            std::mem::transmute::<
                extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, *mut AnyObject),
                unsafe extern "C-unwind" fn(),
            >(handle_get_url),
            types.as_ptr(),
        );
        let _: () = msg_send![
            manager,
            setEventHandler: handler,
            andSelector: sel!(octoweb_handleGetURL:withReply:),
            forEventClass: event_class,
            andEventID: event_id
        ];
        tracing::debug!("cold_open: installed kAEGetURL handler");
    }
}

/// Apple Event handler for kAEGetURL — extracts the URL and buffers it.
extern "C-unwind" fn handle_get_url(
    _this: *mut AnyObject,
    _sel: Sel,
    event: *mut AnyObject,
    _reply: *mut AnyObject,
) {
    if event.is_null() {
        return;
    }
    unsafe {
        // keyDirectObject = '----' (0x2d2d2d2d)
        let key_direct: u32 = u32::from_be_bytes(*b"----");
        let _type_unicode: u32 = u32::from_be_bytes(*b"utxt");
        let descriptor: *mut AnyObject = msg_send![
            event,
            paramDescriptorForKeyword: key_direct
        ];
        if descriptor.is_null() {
            return;
        }
        let nsstring: *mut AnyObject = msg_send![descriptor, stringValue];
        if nsstring.is_null() {
            return;
        }
        let bytes: *const u8 = msg_send![nsstring, UTF8String];
        if bytes.is_null() {
            return;
        }
        let url = std::ffi::CStr::from_ptr(bytes.cast())
            .to_string_lossy()
            .into_owned();
        tracing::debug!(url = %url, "cold_open: captured URL from kAEGetURL");
        cold_urls().lock().unwrap().push(url);
    }
}

/// Replace tao's `application:openURLs:` with our buffering wrapper.
///
/// Must be called **after** `EventLoop::new()` — tao registers
/// `TaoAppDelegateParent` lazily during event loop construction.
pub fn install() {
    static DONE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if DONE.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }

    unsafe {
        let cls = objc_getClass(c"TaoAppDelegateParent".as_ptr()) as *mut AnyClass;
        if cls.is_null() {
            tracing::warn!("cold_open: TaoAppDelegateParent not found");
            return;
        }

        // Type encoding: void, self, _cmd, NSApplication*, NSArray<NSURL>*
        let orig = class_replaceMethod(
            cls,
            objc2::sel!(application:openURLs:),
            std::mem::transmute::<
                extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, *mut NSArray<NSURL>),
                Imp,
            >(hooked_open_urls),
            c"v@:@@".as_ptr(),
        );

        if let Some(imp) = orig {
            let _ = ORIGINAL_IMP.set(std::mem::transmute::<
                Imp,
                unsafe extern "C-unwind" fn(
                    *mut AnyObject,
                    Sel,
                    *mut AnyObject,
                    *mut NSArray<NSURL>,
                ),
            >(imp));
        }
        tracing::debug!("cold_open: installed application:openURLs: wrapper");
    }
}

/// Replacement for tao's `application:openURLs:`.
///
/// Buffers every URL, then forwards to the original IMP (if present) so
/// warm-launch `Event::Opened` still fires normally.
extern "C-unwind" fn hooked_open_urls(
    this: *mut AnyObject,
    sel: Sel,
    app: *mut AnyObject,
    urls: *mut NSArray<NSURL>,
) {
    if !urls.is_null() {
        unsafe {
            let count = (*urls).count();
            let mut buf = cold_urls().lock().unwrap();
            for i in 0..count {
                let nsurl = (*urls).objectAtIndex(i);
                if let Some(abs) = nsurl.absoluteString() {
                    buf.push(abs.to_string());
                }
            }
        }
    }

    // Forward to tao's original so Event::Opened fires on warm launch.
    if let Some(orig) = ORIGINAL_IMP.get() {
        unsafe { orig(this, sel, app, urls) };
    }
}
