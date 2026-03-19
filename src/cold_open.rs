//! Captures URLs delivered before tao's event loop callback is registered.
//!
//! On cold launch (app not running, user clicks a link), macOS delivers
//! `application:openURLs:` between `applicationDidFinishLaunching` and the
//! first iteration of `event_loop.run()`. tao's handler silently drops the
//! URL because its callback is still `None` at that point.
//!
//! We fix this by replacing tao's `application:openURLs:` on
//! `TaoAppDelegateParent` with a wrapper that buffers URLs into a global
//! `Vec`, then forwards to the original implementation (so warm-launch
//! `Event::Opened` still works). The event loop drains the buffer on its
//! first tick.
//!
//! Call `install()` once, immediately after `EventLoop::new()`.
//! Call `take()` on the first event loop iteration to get any buffered URLs.

use std::sync::{Mutex, OnceLock};

use objc2::ffi::{class_replaceMethod, objc_getClass};
use objc2::runtime::{AnyClass, AnyObject, Imp, Sel};
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
