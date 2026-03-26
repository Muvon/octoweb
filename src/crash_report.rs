//! Crash diagnostics — black-box recorder for post-mortem analysis.
//!
//! Writes to `~/Library/Application Support/octoweb/crash.log` regardless of
//! RUST_LOG level. Covers four crash scenarios:
//!
//! 1. **Rust panic** — `set_hook` writes backtrace + app state before abort.
//! 2. **Fatal signal** (SIGSEGV/SIGBUS/SIGABRT) — signal handler writes to
//!    crash.log using only async-signal-safe syscalls (no allocations).
//! 3. **WebContent XPC termination** — logged with tab URL, PID, RSS.
//! 4. **OOM / Jetsam kill** (SIGKILL) — can't intercept, but periodic HEALTH
//!    lines show the memory trajectory leading up to death.
//!
//! A `.running` sentinel file detects unclean shutdowns on next launch.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::SystemTime;

// ── Paths ─────────────────────────────────────────────────────────────────────

fn log_dir() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from("/tmp")))
        .join("octoweb")
}

fn crash_log_path() -> PathBuf {
    log_dir().join("crash.log")
}

fn sentinel_path() -> PathBuf {
    log_dir().join(".running")
}

// ── Timestamp ─────────────────────────────────────────────────────────────────

fn iso_now() -> String {
    // Manual UTC ISO-8601 without chrono dependency.
    let dur = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    // Days since epoch → year/month/day (simplified leap-year-aware).
    let mut days = (secs / 86400) as i64;
    let day_secs = (secs % 86400) as u32;
    let h = day_secs / 3600;
    let m = (day_secs % 3600) / 60;
    let s = day_secs % 60;

    let mut year: i64 = 1970;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    let leap = is_leap(year);
    let month_days: [i64; 12] = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    let mut month = 0u32;
    for md in &month_days {
        if days < *md {
            break;
        }
        days -= md;
        month += 1;
    }
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        year,
        month + 1,
        days + 1,
        h,
        m,
        s
    )
}

fn is_leap(y: i64) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}

// ── File I/O (normal — uses std) ──────────────────────────────────────────────

fn append_line(msg: &str) {
    let path = crash_log_path();
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "[{}] {}", iso_now(), msg);
    }
}

// ── Log rotation ──────────────────────────────────────────────────────────────

const MAX_LOG_BYTES: u64 = 100 * 1024; // 100 KB

/// Rotate crash.log → crash.log.1 if it exceeds 100 KB. Call once at startup.
pub fn rotate_log() {
    let path = crash_log_path();
    if let Ok(meta) = fs::metadata(&path) {
        if meta.len() > MAX_LOG_BYTES {
            let rotated = log_dir().join("crash.log.1");
            let _ = fs::rename(&path, rotated);
        }
    }
}

// ── Startup / shutdown sentinel ───────────────────────────────────────────────

/// Log startup. Detects unclean previous shutdown via `.running` sentinel.
pub fn log_startup() {
    let _ = fs::create_dir_all(log_dir());
    let sentinel = sentinel_path();
    let unclean = sentinel.exists();
    if unclean {
        // Read last-modified time of sentinel for approximate crash time.
        let last = fs::metadata(&sentinel)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
            .map(|d| {
                let s = d.as_secs();
                // Rough timestamp — good enough for diagnostics.
                format!(
                    "~{}s ago",
                    SystemTime::now()
                        .duration_since(SystemTime::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_secs()
                        .saturating_sub(s)
                )
            })
            .unwrap_or_else(|| "unknown".into());
        append_line(&format!("STARTUP prev_shutdown=unclean (crashed {last})"));
    } else {
        append_line("STARTUP prev_shutdown=clean");
    }
    // Create sentinel — removed on clean shutdown.
    let _ = File::create(&sentinel);
}

/// Mark clean shutdown — removes sentinel, appends SHUTDOWN line.
pub fn log_clean_shutdown() {
    let _ = fs::remove_file(sentinel_path());
    append_line("SHUTDOWN clean");
}

// ── Health snapshot ───────────────────────────────────────────────────────────

pub struct HealthSnapshot<'a> {
    pub uptime_secs: u64,
    pub tab_count: usize,
    pub active_rss_mb: u64,
    pub main_rss_mb: u64,
    pub pressure: &'a str,
    pub active_url: &'a str,
}

/// Append a HEALTH line — called every 30 s from the event loop.
pub fn log_health(snap: &HealthSnapshot<'_>) {
    append_line(&format!(
        "HEALTH uptime={}s tabs={} active_rss={}MB main_rss={}MB pressure={} url={}",
        snap.uptime_secs,
        snap.tab_count,
        snap.active_rss_mb,
        snap.main_rss_mb,
        snap.pressure,
        snap.active_url,
    ));
}

// ── WebContent termination log ────────────────────────────────────────────────

/// Log when macOS kills a WebContent XPC process.
pub fn log_webcontent_terminated(tab_id: usize, url: &str, pid: Option<i32>, rss_mb: u64) {
    let pid_str = pid.map_or("gone".to_string(), |p| p.to_string());
    append_line(&format!(
        "TERMINATED tab={tab_id} pid={pid_str} rss={rss_mb}MB url={url}"
    ));
}

// ── Main process RSS ──────────────────────────────────────────────────────────

/// Get the main (host) process RSS in bytes. Uses same proc_pidinfo as tab_stats.
pub fn main_process_rss() -> u64 {
    // Reuse the ProcTaskInfo FFI from tab_stats — but we inline it here to avoid
    // coupling and because signal handler needs a self-contained path.
    #[repr(C)]
    struct ProcTaskInfo {
        pti_virtual_size: u64,
        pti_resident_size: u64,
        // remaining fields not needed
        _pad: [u64; 14],
    }
    const PROC_PIDTASKINFO: libc::c_int = 4;
    extern "C" {
        fn proc_pidinfo(
            pid: libc::c_int,
            flavor: libc::c_int,
            arg: u64,
            buffer: *mut libc::c_void,
            buffersize: libc::c_int,
        ) -> libc::c_int;
    }

    let mut info = std::mem::MaybeUninit::<ProcTaskInfo>::uninit();
    let ret = unsafe {
        proc_pidinfo(
            libc::getpid(),
            PROC_PIDTASKINFO,
            0,
            info.as_mut_ptr() as *mut libc::c_void,
            std::mem::size_of::<ProcTaskInfo>() as libc::c_int,
        )
    };
    if ret <= 0 {
        return 0;
    }
    unsafe { info.assume_init() }.pti_resident_size
}

// ── Panic hook ────────────────────────────────────────────────────────────────

/// Install a panic hook that writes crash info to crash.log before aborting.
/// Must be called once, early in main().
pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let thread = std::thread::current();
        let thread_name = thread.name().unwrap_or("<unnamed>");

        let payload = if let Some(s) = info.payload().downcast_ref::<&str>() {
            (*s).to_string()
        } else if let Some(s) = info.payload().downcast_ref::<String>() {
            s.clone()
        } else {
            "Box<dyn Any>".to_string()
        };

        let location = info
            .location()
            .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
            .unwrap_or_else(|| "unknown".into());

        let bt = std::backtrace::Backtrace::force_capture();
        let main_rss = main_process_rss() / (1024 * 1024);

        let msg = format!(
            "PANIC thread=\"{thread_name}\" at {location}: {payload}\n\
             |  main_rss={main_rss}MB\n\
             |  backtrace:\n{bt}"
        );

        // Write to crash.log (best-effort — we're panicking).
        append_line(&msg);

        // Also emit to stderr so it shows in terminal if running from CLI.
        eprintln!("[octoweb] PANIC at {location}: {payload}\n{bt}");
    }));
}

// ── Signal handler (async-signal-safe) ────────────────────────────────────────
//
// Signal handlers can only call async-signal-safe functions. That means:
// - No heap allocation (no String, Vec, format!, etc.)
// - No mutex locks
// - Only libc::write, libc::open, libc::close, libc::_exit, etc.
//
// We pre-compute the crash.log path and open an fd at install time, stored in a
// static. The handler writes a fixed-format message using only stack buffers.

/// Pre-opened fd for the signal handler to write to.
static SIGNAL_LOG_FD: OnceLock<libc::c_int> = OnceLock::new();

/// Install signal handlers for SIGSEGV, SIGBUS, SIGABRT.
/// Must be called once, early in main().
pub fn install_signal_handlers() {
    // Pre-open crash.log fd for the signal handler.
    let path = crash_log_path();
    let _ = fs::create_dir_all(log_dir());
    let c_path = std::ffi::CString::new(path.to_string_lossy().as_bytes()).unwrap();
    let fd = unsafe {
        libc::open(
            c_path.as_ptr(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_APPEND,
            0o644,
        )
    };
    if fd < 0 {
        tracing::error!("crash_report: failed to open crash.log for signal handler");
        return;
    }
    let _ = SIGNAL_LOG_FD.set(fd);

    // Register signal handlers.
    for sig in [libc::SIGSEGV, libc::SIGBUS, libc::SIGABRT] {
        unsafe {
            let mut sa: libc::sigaction = std::mem::zeroed();
            sa.sa_sigaction = signal_handler as *const () as usize;
            sa.sa_flags = libc::SA_SIGINFO | libc::SA_RESETHAND; // one-shot: re-raise kills us
            libc::sigemptyset(&mut sa.sa_mask);
            libc::sigaction(sig, &sa, std::ptr::null_mut());
        }
    }
}

/// Async-signal-safe signal handler. Writes a crash line and re-raises.
extern "C" fn signal_handler(
    sig: libc::c_int,
    _info: *mut libc::siginfo_t,
    _ctx: *mut libc::c_void,
) {
    let fd = match SIGNAL_LOG_FD.get() {
        Some(&fd) => fd,
        None => {
            unsafe { libc::_exit(128 + sig) };
        }
    };

    // Build message on the stack — no allocations.
    let sig_name: &[u8] = match sig {
        libc::SIGSEGV => b"SIGSEGV",
        libc::SIGBUS => b"SIGBUS",
        libc::SIGABRT => b"SIGABRT",
        _ => b"UNKNOWN",
    };

    // Write: "[timestamp] SIGNAL <name> (signal <num>)\n"
    // We can't call iso_now() (allocates), so write a simpler marker.
    let mut buf = [0u8; 256];
    let mut pos = 0usize;

    // Prefix
    let prefix = b"[signal] FATAL_SIGNAL ";
    let n = prefix.len().min(buf.len() - pos);
    buf[pos..pos + n].copy_from_slice(&prefix[..n]);
    pos += n;

    // Signal name
    let n = sig_name.len().min(buf.len() - pos);
    buf[pos..pos + n].copy_from_slice(&sig_name[..n]);
    pos += n;

    // " (sig="
    let mid = b" (sig=";
    let n = mid.len().min(buf.len() - pos);
    buf[pos..pos + n].copy_from_slice(&mid[..n]);
    pos += n;

    // Signal number as decimal
    pos += write_int_to_buf(&mut buf[pos..], sig as u64);

    // ")\n"
    if pos + 2 <= buf.len() {
        buf[pos] = b')';
        buf[pos + 1] = b'\n';
        pos += 2;
    }

    unsafe {
        libc::write(fd, buf.as_ptr() as *const libc::c_void, pos);
        libc::fsync(fd);
    }

    // Re-raise to get default behavior (core dump / termination).
    // SA_RESETHAND already restored the default handler.
    unsafe {
        libc::raise(sig);
    }
}

/// Write a u64 as decimal into a byte buffer. Returns number of bytes written.
/// Async-signal-safe (no allocations).
fn write_int_to_buf(buf: &mut [u8], mut val: u64) -> usize {
    if buf.is_empty() {
        return 0;
    }
    if val == 0 {
        buf[0] = b'0';
        return 1;
    }
    // Write digits in reverse, then flip.
    let mut tmp = [0u8; 20]; // max u64 digits
    let mut i = 0;
    while val > 0 && i < tmp.len() {
        tmp[i] = b'0' + (val % 10) as u8;
        val /= 10;
        i += 1;
    }
    let len = i.min(buf.len());
    for j in 0..len {
        buf[j] = tmp[i - 1 - j];
    }
    len
}
