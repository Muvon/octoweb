//! Smart tab hibernation driven by macOS memory pressure.
//!
//! Monitors real system memory via `host_statistics64` and scores background
//! tabs for hibernation. When memory is tight, the highest-scoring tabs have
//! their WebViews destroyed (freeing the XPC WebContent process) while tab
//! metadata (URL, title, favicon) is preserved. The tab is restored on demand
//! via the existing `pending_tabs` lazy-load mechanism.
//!
//! Zero configuration — thresholds are derived from actual system state.

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use wry::{WebView, WebViewExtMacOS};

use crate::browser::Tab;
use crate::tab_stats;

// ── Memory pressure detection (macOS host_statistics64) ──────────────────────

/// System memory pressure level.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemoryPressure {
    /// Available memory > 20% of total — no action needed.
    Normal,
    /// Available memory 10–20% — gently hibernate idle tabs.
    Warning,
    /// Available memory < 10% — aggressively hibernate.
    Critical,
}

// Mach VM statistics (vm_statistics64_data_t) — only the fields we need.
// Layout must match <mach/vm_statistics.h>.
#[repr(C)]
#[allow(non_camel_case_types)]
struct vm_statistics64 {
    free_count: u32,
    active_count: u32,
    inactive_count: u32,
    wire_count: u32,
    zero_fill_count: u64,
    reactivations: u64,
    pageins: u64,
    pageouts: u64,
    faults: u64,
    cow_faults: u64,
    lookups: u64,
    hits: u64,
    purges: u64,
    purgeable_count: u32,
    speculative_count: u32,
    decompressions: u64,
    compressions: u64,
    swapins: u64,
    swapouts: u64,
    compressor_page_count: u32,
    throttled_count: u32,
    external_page_count: u32,
    internal_page_count: u32,
    total_uncompressed_pages_in_compressor: u64,
}

extern "C" {
    fn mach_host_self() -> u32;
    fn host_statistics64(
        host: u32,
        flavor: i32,
        info: *mut vm_statistics64,
        count: *mut u32,
    ) -> i32;
    fn host_page_size(host: u32, page_size: *mut u32) -> i32;
}

const HOST_VM_INFO64: i32 = 4;

/// Query macOS for current memory pressure.
pub fn system_memory_pressure() -> MemoryPressure {
    let host = unsafe { mach_host_self() };

    let mut page_size: u32 = 0;
    let kr = unsafe { host_page_size(host, &mut page_size) };
    if kr != 0 || page_size == 0 {
        return MemoryPressure::Normal; // can't read → assume fine
    }

    let mut info = std::mem::MaybeUninit::<vm_statistics64>::uninit();
    let mut count = (std::mem::size_of::<vm_statistics64>() / std::mem::size_of::<u32>()) as u32;
    let kr = unsafe { host_statistics64(host, HOST_VM_INFO64, info.as_mut_ptr(), &mut count) };
    if kr != 0 {
        return MemoryPressure::Normal;
    }
    let info = unsafe { info.assume_init() };

    let page = page_size as u64;
    let free = info.free_count as u64 * page;
    let inactive = info.inactive_count as u64 * page;
    let purgeable = info.purgeable_count as u64 * page;
    let available = free + inactive + purgeable;

    let total = (info.free_count as u64
        + info.active_count as u64
        + info.inactive_count as u64
        + info.wire_count as u64
        + info.compressor_page_count as u64
        + info.speculative_count as u64)
        * page;

    if total == 0 {
        return MemoryPressure::Normal;
    }

    let ratio = available as f64 / total as f64;
    if ratio < 0.10 {
        MemoryPressure::Critical
    } else if ratio < 0.20 {
        MemoryPressure::Warning
    } else {
        MemoryPressure::Normal
    }
}

// ── Hibernation scoring ──────────────────────────────────────────────────────

/// Grace period — never hibernate a tab that was active less than 30 s ago.
const GRACE_SECS: f32 = 30.0;
/// Idle time at which the time weight saturates to 1.0.
const IDLE_CAP_SECS: f32 = 300.0; // 5 minutes
/// RSS at which the memory weight saturates to 1.0.
const RSS_CAP_BYTES: f32 = 200.0 * 1024.0 * 1024.0; // 200 MB

/// Compute a hibernation score for a single tab.
/// Higher score = more likely to be hibernated.
/// Returns `None` if the tab is protected (should never be hibernated).
fn hibernation_score(
    tab: &Tab,
    rss_bytes: u64,
    mru_position: usize,
    mru_len: usize,
    active_id: usize,
    media_playing: &HashSet<usize>,
    now: Instant,
) -> Option<f32> {
    // Protection rules
    if tab.id == active_id {
        return None;
    }
    if tab.is_playing_audio || media_playing.contains(&tab.id) {
        return None;
    }
    if tab.url == "about:blank" {
        return None;
    }

    let idle_secs = now.duration_since(tab.last_active_at).as_secs_f32();
    if idle_secs < GRACE_SECS {
        return None;
    }

    // Time weight (70%): how long since last viewed, capped at 5 min
    let time_w = (idle_secs / IDLE_CAP_SECS).min(1.0);

    // Memory weight (20%): RSS of the WebContent process
    let mem_w = (rss_bytes as f32 / RSS_CAP_BYTES).min(1.0);

    // MRU recency weight (10%): position in MRU list (0 = most recent)
    let mru_w = if mru_len > 1 {
        mru_position as f32 / (mru_len - 1) as f32
    } else {
        0.0
    };

    Some(time_w * 0.70 + mem_w * 0.20 + mru_w * 0.10)
}

// ── Total system memory ─────────────────────────────────────────────────────

/// Detect total physical memory in bytes (via sysctl hw.memsize). Call once at startup.
pub fn total_system_memory() -> u64 {
    let mut memsize: u64 = 0;
    let mut len = std::mem::size_of::<u64>();
    let name = c"hw.memsize";
    let ret = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            &mut memsize as *mut _ as _,
            &mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if ret != 0 || memsize == 0 {
        8 * 1024 * 1024 * 1024 // fallback: assume 8 GB
    } else {
        memsize
    }
}

// ── Proactive thresholds (adaptive to system RAM) ───────────────────────────

// Base thresholds — calibrated for 8 GB machines. Scaled up via sqrt(ram_gb/8).
const BASE_FROZEN_IDLE_SECS: f64 = 600.0; // 10 minutes
const BASE_COLD_IDLE_SECS: f64 = 180.0; // 3 minutes
const BASE_COLD_TAB_THRESHOLD: f64 = 6.0;
const BASE_COLD_RSS_BYTES: f64 = 250.0 * 1024.0 * 1024.0; // 250 MB

/// Pre-computed proactive hibernation thresholds scaled to system RAM.
/// Created once at startup and reused every 60-second cycle.
#[derive(Debug, Clone, Copy)]
pub struct ProactiveConfig {
    /// Idle seconds after which a tab is "frozen" and always hibernated.
    pub frozen_idle_secs: u64,
    /// Idle seconds after which a "cold" tab is hibernated when resources are scarce.
    pub cold_idle_secs: u64,
    /// Background tab count above which cold tabs are hibernated.
    pub cold_tab_threshold: usize,
    /// RSS in bytes above which a cold tab is hibernated regardless of tab count.
    pub cold_rss_bytes: u64,
}

impl ProactiveConfig {
    /// Build config scaled to the machine's total RAM.
    ///
    /// Uses `sqrt(ram_gb / 8)` scaling — sublinear so thresholds grow gently:
    ///   8 GB → factor 1.0 (baseline)
    ///  16 GB → factor 1.41
    ///  32 GB → factor 2.0
    ///  64 GB → factor 2.83
    pub fn from_total_memory(total_bytes: u64) -> Self {
        let ram_gb = total_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
        let factor = (ram_gb / 8.0).max(1.0).sqrt();

        Self {
            frozen_idle_secs: (BASE_FROZEN_IDLE_SECS * factor).min(1800.0) as u64, // cap 30 min
            cold_idle_secs: (BASE_COLD_IDLE_SECS * factor).min(900.0) as u64,      // cap 15 min
            cold_tab_threshold: (BASE_COLD_TAB_THRESHOLD * factor).min(24.0) as usize,
            cold_rss_bytes: (BASE_COLD_RSS_BYTES * factor).min(1024.0 * 1024.0 * 1024.0) as u64, // cap 1 GB
        }
    }
}

/// Proactive hibernation — runs on a 60 s timer regardless of memory pressure.
///
/// Two tiers (thresholds scaled to system RAM via `ProactiveConfig`):
/// - **Frozen** (idle > threshold): always hibernate, no conditions.
/// - **Cold** (idle > threshold): hibernate if too many background tabs OR this
///   tab's WebContent process exceeds the RSS threshold.
///
/// Protected tabs (active, audio-playing, about:blank, pending-swap) are skipped.
pub fn pick_proactive_victims(
    tabs: &[Tab],
    tab_webviews: &HashMap<usize, WebView>,
    pending_tabs: &HashMap<usize, String>,
    pending_swap: Option<(usize, usize)>,
    active_id: usize,
    media_playing: &HashSet<usize>,
    config: &ProactiveConfig,
) -> Vec<usize> {
    let now = Instant::now();
    // How many background WebViews exist (exclude the active one).
    let background_count = tab_webviews.len().saturating_sub(1);

    let (swap_old, swap_new) = match pending_swap {
        Some((old, new)) => (Some(old), Some(new)),
        None => (None, None),
    };

    tabs.iter()
        .filter(|t| tab_webviews.contains_key(&t.id) && !pending_tabs.contains_key(&t.id))
        .filter(|t| swap_old != Some(t.id) && swap_new != Some(t.id))
        .filter(|t| {
            if t.id == active_id {
                return false;
            }
            if t.is_playing_audio || media_playing.contains(&t.id) {
                return false;
            }
            if t.url == "about:blank" {
                return false;
            }

            let idle = now.duration_since(t.last_active_at).as_secs();

            // Frozen: idle > threshold — always hibernate.
            if idle > config.frozen_idle_secs {
                return true;
            }

            // Cold: idle > threshold — hibernate if resources are scarce.
            if idle > config.cold_idle_secs {
                if background_count > config.cold_tab_threshold {
                    return true;
                }
                // Sample this tab's RSS; hibernate if heavy.
                let rss = tab_webviews
                    .get(&t.id)
                    .and_then(|wv| {
                        let ptr = objc2::rc::Retained::as_ptr(&wv.webview()) as usize;
                        let pid = tab_stats::webview_pid(ptr)?;
                        let (rss, _) = tab_stats::sample_pid(pid)?;
                        Some(rss)
                    })
                    .unwrap_or(0);
                if rss > config.cold_rss_bytes {
                    return true;
                }
            }

            false
        })
        .map(|t| t.id)
        .collect()
}

// ── Victim selection ─────────────────────────────────────────────────────────

/// Pick tabs to hibernate based on current memory pressure.
///
/// Returns tab IDs sorted by score (highest first), up to the limit
/// dictated by pressure level. Only considers tabs that have a live WebView
/// (not already pending/hibernated).
///
/// `pending_swap` contains (old_visible_id, new_loading_id) for tabs mid-transition.
/// Both tabs are protected from hibernation until the swap completes.
#[allow(clippy::too_many_arguments)]
pub fn pick_victims(
    tabs: &[Tab],
    tab_webviews: &HashMap<usize, WebView>,
    pending_tabs: &HashMap<usize, String>,
    pending_swap: Option<(usize, usize)>,
    mru: &[usize],
    active_id: usize,
    media_playing: &HashSet<usize>,
    pressure: MemoryPressure,
) -> Vec<usize> {
    let max_victims = match pressure {
        MemoryPressure::Normal => return Vec::new(),
        MemoryPressure::Warning => 1,
        MemoryPressure::Critical => 3,
    };

    let now = Instant::now();
    let mru_len = mru.len();

    // Tabs involved in a pending swap are mid-transition — don't hibernate
    let (swap_old, swap_new) = match pending_swap {
        Some((old, new)) => (Some(old), Some(new)),
        None => (None, None),
    };

    let mut scored: Vec<(usize, f32)> = tabs
        .iter()
        .filter(|t| {
            // Only consider tabs with a live WebView (not already hibernated/pending)
            tab_webviews.contains_key(&t.id) && !pending_tabs.contains_key(&t.id)
        })
        .filter(|t| {
            // Exclude tabs involved in pending_swap
            swap_old != Some(t.id) && swap_new != Some(t.id)
        })
        .filter_map(|t| {
            let mru_pos = mru.iter().position(|&id| id == t.id).unwrap_or(mru_len);

            // Get RSS from the WebContent process; skip if we can't read it
            let rss = tab_webviews
                .get(&t.id)
                .and_then(|wv| {
                    let wv_ptr = objc2::rc::Retained::as_ptr(&wv.webview()) as usize;
                    let pid = tab_stats::webview_pid(wv_ptr)?;
                    let (rss, _cpu_ns) = tab_stats::sample_pid(pid)?;
                    Some(rss)
                })
                .unwrap_or(0);

            let score = hibernation_score(t, rss, mru_pos, mru_len, active_id, media_playing, now)?;
            Some((t.id, score))
        })
        .collect();

    // Sort by score descending — highest score = best candidate
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(max_victims);
    scored.into_iter().map(|(id, _)| id).collect()
}
