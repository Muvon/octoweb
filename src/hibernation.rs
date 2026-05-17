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

/// Single point on the RAM/threshold curve. Interpolated between in
/// `ProactiveConfig::modern`.
#[derive(Debug, Clone, Copy)]
struct ThresholdPoint {
    ram_gb: f64,
    frozen_idle_secs: f64,
    cold_idle_secs: f64,
    cold_tab_threshold: f64,
    cold_rss_mb: f64,
}

// Modern-laptop friendly curve. 8 GB matches the historical "tight" baseline;
// 16 GB and up are deliberately looser so users with modern Macs don't see
// tabs reloading after a coffee break. Interpolated linearly between points.
const MODERN_CURVE: &[ThresholdPoint] = &[
    ThresholdPoint {
        ram_gb: 8.0,
        frozen_idle_secs: 600.0,
        cold_idle_secs: 180.0,
        cold_tab_threshold: 6.0,
        cold_rss_mb: 250.0,
    },
    ThresholdPoint {
        ram_gb: 16.0,
        frozen_idle_secs: 1200.0,
        cold_idle_secs: 480.0,
        cold_tab_threshold: 12.0,
        cold_rss_mb: 500.0,
    },
    ThresholdPoint {
        ram_gb: 32.0,
        frozen_idle_secs: 1800.0,
        cold_idle_secs: 720.0,
        cold_tab_threshold: 18.0,
        cold_rss_mb: 800.0,
    },
    ThresholdPoint {
        ram_gb: 64.0,
        frozen_idle_secs: 2700.0,
        cold_idle_secs: 1200.0,
        cold_tab_threshold: 30.0,
        cold_rss_mb: 1500.0,
    },
];

// Legacy aggressive curve — original sqrt(ram_gb/8) scaling. Kept verbatim so
// users on tight machines (or anyone who wants the old behavior) can opt in
// via Config::aggressive_hibernation.
const AGG_BASE_FROZEN_IDLE_SECS: f64 = 600.0;
const AGG_BASE_COLD_IDLE_SECS: f64 = 180.0;
const AGG_BASE_COLD_TAB_THRESHOLD: f64 = 6.0;
const AGG_BASE_COLD_RSS_BYTES: f64 = 250.0 * 1024.0 * 1024.0;

// Runaway guard: any tab whose WebContent process exceeds this RSS while idle
// for at least `RUNAWAY_IDLE_SECS` gets hibernated regardless of pressure or
// thresholds. Catches single misbehaving pages eating the whole machine.
const RUNAWAY_RSS_BYTES: u64 = 1_500 * 1024 * 1024;
const RUNAWAY_IDLE_SECS: u64 = 60;

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
    /// RSS in bytes above which a single tab is hibernated unconditionally
    /// after being idle for `RUNAWAY_IDLE_SECS`. Acts as an emergency brake
    /// on runaway WebContent processes.
    pub runaway_rss_bytes: u64,
    /// Idle threshold (seconds) for the runaway guard.
    pub runaway_idle_secs: u64,
}

impl ProactiveConfig {
    /// Modern-laptop friendly: linear interpolation across MODERN_CURVE.
    /// Tabs survive longer on 16 GB+ machines than the old sqrt scaling.
    pub fn modern(total_bytes: u64) -> Self {
        let ram_gb = total_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
        let (frozen, cold, tabs, rss_mb) = interpolate_curve(ram_gb, MODERN_CURVE);
        Self {
            frozen_idle_secs: frozen as u64,
            cold_idle_secs: cold as u64,
            cold_tab_threshold: tabs as usize,
            cold_rss_bytes: (rss_mb * 1024.0 * 1024.0) as u64,
            runaway_rss_bytes: RUNAWAY_RSS_BYTES,
            runaway_idle_secs: RUNAWAY_IDLE_SECS,
        }
    }

    /// Aggressive (legacy): sqrt(ram_gb/8) scaling. For users on tight RAM or
    /// who want quicker reclamation.
    pub fn aggressive(total_bytes: u64) -> Self {
        let ram_gb = total_bytes as f64 / (1024.0 * 1024.0 * 1024.0);
        let factor = (ram_gb / 8.0).max(1.0).sqrt();
        Self {
            frozen_idle_secs: (AGG_BASE_FROZEN_IDLE_SECS * factor).min(1800.0) as u64,
            cold_idle_secs: (AGG_BASE_COLD_IDLE_SECS * factor).min(900.0) as u64,
            cold_tab_threshold: (AGG_BASE_COLD_TAB_THRESHOLD * factor).min(24.0) as usize,
            cold_rss_bytes: (AGG_BASE_COLD_RSS_BYTES * factor).min(1024.0 * 1024.0 * 1024.0) as u64,
            runaway_rss_bytes: RUNAWAY_RSS_BYTES,
            runaway_idle_secs: RUNAWAY_IDLE_SECS,
        }
    }

    /// Pick the right curve based on the user's `aggressive_hibernation` setting.
    pub fn from_total_memory(total_bytes: u64, aggressive: bool) -> Self {
        if aggressive {
            Self::aggressive(total_bytes)
        } else {
            Self::modern(total_bytes)
        }
    }
}

/// Linear interpolation across the RAM/threshold curve. Clamps below the
/// first point and above the last point (no extrapolation).
fn interpolate_curve(ram_gb: f64, curve: &[ThresholdPoint]) -> (f64, f64, f64, f64) {
    debug_assert!(!curve.is_empty());
    if ram_gb <= curve[0].ram_gb {
        let p = curve[0];
        return (
            p.frozen_idle_secs,
            p.cold_idle_secs,
            p.cold_tab_threshold,
            p.cold_rss_mb,
        );
    }
    for window in curve.windows(2) {
        let lo = window[0];
        let hi = window[1];
        if ram_gb <= hi.ram_gb {
            let t = (ram_gb - lo.ram_gb) / (hi.ram_gb - lo.ram_gb);
            return (
                lo.frozen_idle_secs + t * (hi.frozen_idle_secs - lo.frozen_idle_secs),
                lo.cold_idle_secs + t * (hi.cold_idle_secs - lo.cold_idle_secs),
                lo.cold_tab_threshold + t * (hi.cold_tab_threshold - lo.cold_tab_threshold),
                lo.cold_rss_mb + t * (hi.cold_rss_mb - lo.cold_rss_mb),
            );
        }
    }
    let p = curve[curve.len() - 1];
    (
        p.frozen_idle_secs,
        p.cold_idle_secs,
        p.cold_tab_threshold,
        p.cold_rss_mb,
    )
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

            // Runaway guard: a single tab eating an absurd amount of RAM gets
            // hibernated as soon as it's been idle for a minute, regardless
            // of pressure or thresholds.
            if idle > config.runaway_idle_secs {
                let rss = sample_tab_rss(t.id, tab_webviews);
                if rss > config.runaway_rss_bytes {
                    return true;
                }
            }

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
                let rss = sample_tab_rss(t.id, tab_webviews);
                if rss > config.cold_rss_bytes {
                    return true;
                }
            }

            false
        })
        .map(|t| t.id)
        .collect()
}

/// Shared RSS sampling — used by both proactive and reactive paths.
fn sample_tab_rss(tab_id: usize, tab_webviews: &HashMap<usize, WebView>) -> u64 {
    tab_webviews
        .get(&tab_id)
        .and_then(|wv| {
            let ptr = objc2::rc::Retained::as_ptr(&wv.webview()) as usize;
            let pid = tab_stats::webview_pid(ptr)?;
            let (rss, _) = tab_stats::sample_pid(pid)?;
            Some(rss)
        })
        .unwrap_or(0)
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
            let rss = sample_tab_rss(t.id, tab_webviews);
            let score = hibernation_score(t, rss, mru_pos, mru_len, active_id, media_playing, now)?;
            Some((t.id, score))
        })
        .collect();

    // Sort by score descending — highest score = best candidate
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(max_victims);
    scored.into_iter().map(|(id, _)| id).collect()
}
