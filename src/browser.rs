use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// A single browser tab (metadata only — the WebView is owned separately)
#[derive(Debug, Clone)]
pub struct Tab {
    pub id: usize,
    pub title: String,
    pub url: String,
    pub is_playing_audio: bool,
    pub page_bytes: u64,
    pub page_time_ms: u64,
    /// When this tab was last the active (visible) tab.
    pub last_active_at: Instant,
}

/// A history entry — serialized to history.json for cross-session persistence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub title: String,
    pub url: String,
    pub visited_at: u64,
    /// How many times this URL has been visited (across all sessions).
    #[serde(default = "default_visit_count")]
    pub visit_count: u32,
}

fn default_visit_count() -> u32 {
    1
}

/// Manages tab metadata and browsing history.
/// WebViews are owned separately in main.rs as HashMap<usize, WebView>.
pub struct TabManager {
    tabs: Vec<Tab>,
    active_id: Option<usize>,
    history: VecDeque<HistoryEntry>,
    max_history: usize,
    next_id: usize,
}

impl TabManager {
    pub fn new(max_history: usize) -> Self {
        Self {
            tabs: Vec::new(),
            active_id: None,
            history: VecDeque::new(),
            max_history,
            next_id: 1,
        }
    }

    /// Register a new tab with the given URL, returns its id.
    pub fn open(&mut self, url: String) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.tabs.push(Tab {
            id,
            title: String::new(),
            url,
            is_playing_audio: false,
            page_bytes: 0,
            page_time_ms: 0,
            last_active_at: Instant::now(),
        });
        self.active_id = Some(id);
        id
    }
    /// Register a new tab with a pre-filled title (used for session restore).
    pub fn open_with_title(&mut self, url: String, title: String) -> usize {
        let id = self.next_id;
        self.next_id += 1;
        self.tabs.push(Tab {
            id,
            title,
            url,
            is_playing_audio: false,
            page_bytes: 0,
            page_time_ms: 0,
            last_active_at: Instant::now(),
        });
        self.active_id = Some(id);
        id
    }

    /// Close a tab by id. Clears active_id if the closed tab was active
    /// (caller picks the next tab via MRU).
    pub fn close(&mut self, id: usize) {
        if let Some(pos) = self.tabs.iter().position(|t| t.id == id) {
            self.tabs.remove(pos);
            if self.active_id == Some(id) {
                self.active_id = None;
            }
        }
    }

    /// Switch active tab
    pub fn switch(&mut self, id: usize) {
        if self.tabs.iter().any(|t| t.id == id) {
            // Stamp the outgoing tab so hibernation knows when it was last viewed
            if let Some(old_id) = self.active_id {
                if let Some(old_tab) = self.tabs.iter_mut().find(|t| t.id == old_id) {
                    old_tab.last_active_at = Instant::now();
                }
            }
            self.active_id = Some(id);
        }
    }

    /// Update the title of a tab; backfills the most recent history entry for its URL.
    pub fn update_title(&mut self, id: usize, title: String) {
        let url = if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
            tab.title = title.clone();
            tab.url.clone()
        } else {
            return;
        };
        if let Some(entry) = self.history.iter_mut().rev().find(|e| e.url == url) {
            entry.title = title;
        }
    }

    /// Update the URL of a tab on navigation — upserts a history entry.
    /// If the URL already exists in history, increments its visit count and
    /// updates visited_at. Otherwise pushes a new entry. This keeps history
    /// deduplicated at write time so the deque never fills with duplicates.
    pub fn update_url(&mut self, id: usize, url: String) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
            if tab.url == url {
                return; // same URL re-fired (iframe, redirect) — skip
            }
            let now = unix_now();
            let key = url.trim_end_matches('/');
            if let Some(entry) = self
                .history
                .iter_mut()
                .find(|e| e.url.trim_end_matches('/') == key)
            {
                // URL already in history — update in place, move to back for recency
                entry.visited_at = now;
                entry.visit_count += 1;
                // Move to back so newest-first iteration works correctly
                let updated = entry.clone();
                self.history.retain(|e| e.url.trim_end_matches('/') != key);
                self.history.push_back(updated);
            } else {
                self.history.push_back(HistoryEntry {
                    title: tab.title.clone(),
                    url: url.clone(),
                    visited_at: now,
                    visit_count: 1,
                });
                if self.history.len() > self.max_history {
                    self.history.pop_front();
                }
            }
            tab.url = url;
            tab.title = String::new();
            tab.page_bytes = 0;
            tab.page_time_ms = 0;
        }
    }

    pub fn active_id(&self) -> Option<usize> {
        self.active_id
    }

    pub fn active_tab(&self) -> Option<&Tab> {
        self.active_id
            .and_then(|id| self.tabs.iter().find(|t| t.id == id))
    }

    pub fn tabs(&self) -> &[Tab] {
        &self.tabs
    }

    /// Set the audio playback state for a tab.
    pub fn set_playing_audio(&mut self, id: usize, playing: bool) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
            tab.is_playing_audio = playing;
        }
    }

    /// Cache page load stats (transfer size + load time) for a tab.
    pub fn set_page_info(&mut self, id: usize, bytes: u64, time_ms: u64) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
            tab.page_bytes = bytes;
            tab.page_time_ms = time_ms;
        }
    }

    /// Remove all history entries matching the given URL.
    pub fn remove_history(&mut self, url: &str) {
        let normalized = url.trim_end_matches('/');
        self.history
            .retain(|e| e.url.trim_end_matches('/') != normalized);
    }

    /// Seed history from persisted data (called once at startup, before the event loop).
    /// Persisted entries are the older baseline. Any live entries already in the deque
    /// (shouldn't exist at startup, but defensive) stay and are merged by URL.
    /// Duplicate URLs: live entry wins on visited_at, counts are summed.
    pub fn seed_history(&mut self, mut entries: Vec<HistoryEntry>) {
        // Merge any live entries (defensive — normally empty at startup)
        for live in self.history.drain(..) {
            let key = live.url.trim_end_matches('/');
            if let Some(existing) = entries
                .iter_mut()
                .find(|e| e.url.trim_end_matches('/') == key)
            {
                // Live entry is newer — keep its visited_at, sum counts
                existing.visited_at = existing.visited_at.max(live.visited_at);
                existing.visit_count += live.visit_count;
            } else {
                entries.push(live);
            }
        }
        // Sort oldest-first so deque front = oldest, back = newest
        entries.sort_by_key(|e| e.visited_at);
        for e in entries {
            self.history.push_back(e);
        }
        // Trim to cap — oldest entries fall off the front
        while self.history.len() > self.max_history {
            self.history.pop_front();
        }
    }

    /// Ensure history is contiguous in memory. Call before `history()` when
    /// you also need other immutable borrows (e.g. `tabs()`).
    pub fn ensure_contiguous(&mut self) {
        self.history.make_contiguous();
    }

    pub fn history(&self) -> &[HistoryEntry] {
        // After ensure_contiguous (or when deque hasn't wrapped), first slice is everything
        let (a, b) = self.history.as_slices();
        debug_assert!(b.is_empty(), "call ensure_contiguous() first");
        a
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
