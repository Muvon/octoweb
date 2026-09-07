//! Save-for-later queue — a reading list that decays, not bookmarks.
//!
//! Items expire after `later_days` (their text stays in page memory, so the
//! palette still finds them), and opening one from the list removes it. Per
//! workspace, persisted like quick slots.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LaterItem {
    pub url: String,
    pub title: String,
    /// Unix seconds.
    pub saved_at: u64,
}

/// Every workspace's queue, keyed by workspace id.
pub type AllLater = HashMap<String, Vec<LaterItem>>;

pub fn save_all(all: &AllLater) {
    let path = later_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    match serde_json::to_string(all) {
        Ok(s) => {
            let _ = fs::write(&path, s);
        }
        Err(e) => tracing::warn!(error = %e, "Failed to serialize later queue"),
    }
}

pub fn load_all() -> AllLater {
    fs::read_to_string(later_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn to_json(items: &[LaterItem]) -> String {
    serde_json::to_string(items).unwrap_or_else(|_| "[]".into())
}

/// Save the page, or remove it if it is already queued. Returns true when saved.
pub fn toggle(items: &mut Vec<LaterItem>, url: &str, title: &str) -> bool {
    if remove(items, url) {
        return false;
    }
    items.push(LaterItem {
        url: url.to_string(),
        title: title.to_string(),
        saved_at: unix_now(),
    });
    true
}

pub fn remove(items: &mut Vec<LaterItem>, url: &str) -> bool {
    let before = items.len();
    items.retain(|i| key(&i.url) != key(url));
    items.len() != before
}

/// Drop items older than `max_days`. Zero keeps everything.
pub fn prune(items: &mut Vec<LaterItem>, max_days: usize) {
    if max_days == 0 {
        return;
    }
    let cutoff = unix_now().saturating_sub(max_days as u64 * 86_400);
    items.retain(|i| i.saved_at >= cutoff);
}

fn key(url: &str) -> &str {
    url.trim_end_matches('/')
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn later_path() -> PathBuf {
    crate::config::base_dir().join("octoweb").join("later.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggle_saves_then_removes_ignoring_trailing_slash() {
        let mut q = Vec::new();
        assert!(toggle(&mut q, "https://a.example/x/", "A"));
        assert_eq!(q.len(), 1);
        assert!(!toggle(&mut q, "https://a.example/x", "A"));
        assert!(q.is_empty());
    }

    #[test]
    fn prune_drops_expired_only() {
        let mut q = vec![
            LaterItem {
                url: "https://old".into(),
                title: String::new(),
                saved_at: 0,
            },
            LaterItem {
                url: "https://new".into(),
                title: String::new(),
                saved_at: unix_now(),
            },
        ];
        prune(&mut q, 0);
        assert_eq!(q.len(), 2, "zero keeps everything");
        prune(&mut q, 14);
        assert_eq!(q.len(), 1);
        assert_eq!(q[0].url, "https://new");
    }
}
