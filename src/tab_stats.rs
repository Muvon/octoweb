//! Per-tab WebContent process stats: RSS memory and CPU usage.
//!
//! Each WKWebView runs its content in a separate XPC process
//! (`com.apple.WebKit.WebContent`). We get that process's PID via the
//! private-but-stable `_webProcessIdentifier` selector, then read its
//! task info with `proc_pidinfo(PROC_PIDTASKINFO)` — a public macOS API
//! available without entitlements for same-user processes.
//!
//! CPU% is computed as a delta between two samples taken 2 s apart:
//!   cpu% = (Δcpu_ns / elapsed_ns) * 100
//! This matches what Activity Monitor shows for a single process.

use std::time::{Duration, Instant};

use objc2::{msg_send, runtime::AnyObject};

// ── proc_pidinfo FFI ──────────────────────────────────────────────────────────

// PROC_PIDTASKINFO returns a proc_taskinfo struct with cumulative CPU time and
// resident memory. Defined in <sys/proc_info.h>.
const PROC_PIDTASKINFO: libc::c_int = 4;

// Mirror of struct proc_taskinfo from <sys/proc_info.h>.
// All fields are u64; layout is stable across macOS versions.
#[repr(C)]
struct ProcTaskInfo {
    pti_virtual_size: u64,  // virtual memory size (bytes)
    pti_resident_size: u64, // resident memory size (bytes)
    pti_total_user: u64,    // total user-mode CPU time (nanoseconds)
    pti_total_system: u64,  // total system-mode CPU time (nanoseconds)
    pti_threads_user: u64,  // existing threads' user time
    pti_threads_system: u64,
    pti_policy: i32,
    pti_faults: i32,
    pti_pageins: i32,
    pti_cow_faults: i32,
    pti_messages_sent: i32,
    pti_messages_received: i32,
    pti_syscalls_mach: i32,
    pti_syscalls_unix: i32,
    pti_csw: i32, // context switches
    pti_threadnum: i32,
    pti_numrunning: i32,
    pti_priority: i32,
}

extern "C" {
    fn proc_pidinfo(
        pid: libc::c_int,
        flavor: libc::c_int,
        arg: u64,
        buffer: *mut libc::c_void,
        buffersize: libc::c_int,
    ) -> libc::c_int;
}

// ── Public types ──────────────────────────────────────────────────────────────

/// A single CPU-time snapshot for delta computation.
pub struct TabStatsSample {
    /// Cumulative user+system CPU nanoseconds at sample time.
    pub cpu_ns: u64,
    /// Wall-clock time of this sample.
    pub ts: Instant,
}

// ── Public functions ──────────────────────────────────────────────────────────

/// Get the PID of the WebContent XPC process backing a WKWebView.
///
/// `wkwebview_ptr` is the raw pointer to the WKWebView ObjC object,
/// obtained via `objc2::rc::Retained::as_ptr(&wv.webview()) as usize`.
///
/// Uses the private-but-stable `_webProcessIdentifier` selector.
/// Returns `None` if the process hasn't launched yet (pid == 0).
pub fn webview_pid(wkwebview_ptr: usize) -> Option<libc::pid_t> {
    if wkwebview_ptr == 0 {
        return None;
    }
    let pid: libc::pid_t = unsafe {
        let wv = wkwebview_ptr as *mut AnyObject;
        msg_send![wv, _webProcessIdentifier]
    };
    if pid > 0 {
        Some(pid)
    } else {
        None
    }
}

/// Sample a process's RSS memory (bytes) and cumulative CPU time (nanoseconds).
///
/// Returns `None` if `proc_pidinfo` fails (process gone, permission denied, etc.).
pub fn sample_pid(pid: libc::pid_t) -> Option<(u64, u64)> {
    let mut info = std::mem::MaybeUninit::<ProcTaskInfo>::uninit();
    let ret = unsafe {
        proc_pidinfo(
            pid,
            PROC_PIDTASKINFO,
            0,
            info.as_mut_ptr() as *mut libc::c_void,
            std::mem::size_of::<ProcTaskInfo>() as libc::c_int,
        )
    };
    if ret <= 0 {
        return None;
    }
    let info = unsafe { info.assume_init() };
    let rss = info.pti_resident_size;
    let cpu_ns = info.pti_total_user.saturating_add(info.pti_total_system);
    Some((rss, cpu_ns))
}

/// Compute CPU% from two samples.
///
/// Returns a value in [0.0, 100.0 * num_cores] — we cap at 100.0 for display.
pub fn compute_cpu_pct(prev: &TabStatsSample, curr_cpu_ns: u64, elapsed: Duration) -> f32 {
    let elapsed_ns = elapsed.as_nanos() as u64;
    if elapsed_ns == 0 {
        return 0.0;
    }
    let delta_cpu = curr_cpu_ns.saturating_sub(prev.cpu_ns);
    let pct = (delta_cpu as f64 / elapsed_ns as f64 * 100.0) as f32;
    pct.clamp(0.0, 100.0)
}
