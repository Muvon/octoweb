use std::time::{SystemTime, UNIX_EPOCH};

/// A single browser tab (metadata only — the WebView is owned separately)
#[derive(Debug, Clone)]
pub struct Tab {
    pub id: usize,
    pub title: String,
    pub url: String,
    pub is_playing_audio: bool,
}

/// A history entry
pub struct HistoryEntry {
    pub title: String,
    pub url: String,
    pub visited_at: u64,
}

/// Manages tab metadata and browsing history.
/// WebViews are owned separately in main.rs as HashMap<usize, WebView>.
pub struct TabManager {
    tabs: Vec<Tab>,
    active_id: Option<usize>,
    history: Vec<HistoryEntry>,
    max_history: usize,
    next_id: usize,
}

impl TabManager {
    pub fn new(max_history: usize) -> Self {
        Self {
            tabs: Vec::new(),
            active_id: None,
            history: Vec::new(),
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
        });
        self.active_id = Some(id);
        id
    }

    /// Close a tab by id. Switches to the previous tab if it was active.
    pub fn close(&mut self, id: usize) {
        if let Some(pos) = self.tabs.iter().position(|t| t.id == id) {
            self.tabs.remove(pos);
            if self.active_id == Some(id) {
                self.active_id = self.tabs.get(pos.saturating_sub(1)).map(|t| t.id);
            }
        }
    }

    /// Switch active tab
    pub fn switch(&mut self, id: usize) {
        if self.tabs.iter().any(|t| t.id == id) {
            self.active_id = Some(id);
        }
    }

    /// Update the title of a tab; backfills the most recent history entry for its URL.
    pub fn update_title(&mut self, id: usize, title: String) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
            tab.title = title.clone();
        } else {
            return;
        }
        let url = self.tabs.iter().find(|t| t.id == id).map(|t| t.url.clone());
        if let Some(url) = url {
            if let Some(entry) = self.history.iter_mut().rev().find(|e| e.url == url) {
                entry.title = title;
            }
        }
    }

    /// Update the URL of a tab on navigation — pushes a history entry immediately.
    pub fn update_url(&mut self, id: usize, url: String) {
        if let Some(tab) = self.tabs.iter_mut().find(|t| t.id == id) {
            if tab.url == url {
                return; // same URL re-fired (iframe, redirect) — skip
            }
            self.history.push(HistoryEntry {
                title: tab.title.clone(),
                url: url.clone(),
                visited_at: unix_now(),
            });
            if self.history.len() > self.max_history {
                self.history.remove(0);
            }
            tab.url = url;
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

    /// Remove all history entries matching the given URL.
    pub fn remove_history(&mut self, url: &str) {
        let normalized = url.trim_end_matches('/');
        self.history
            .retain(|e| e.url.trim_end_matches('/') != normalized);
    }

    pub fn history(&self) -> &[HistoryEntry] {
        &self.history
    }

    /// How many times this URL appears in history (used for ranking in the overlay).
    pub fn visit_count(&self, url: &str) -> u32 {
        let url = url.trim_end_matches('/');
        self.history
            .iter()
            .filter(|e| e.url.trim_end_matches('/') == url)
            .count() as u32
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
