//! Per-tab navigation generation counter.
//!
//! MCP actions evaluate JS asynchronously; when a page navigates mid-call,
//! WebKit drops the completion handler and a watchdog thread has to explain
//! what happened. Before this counter existed, every dropped callback was
//! reported as "the page navigated" — including plain timeouts on static
//! pages — which sent the AI chasing phantom navigations.
//!
//! The main loop bumps the counter on `PageLoadStarted` (full navigations) and
//! `BrowserUrlChanged` (SPA route changes). A handler records `get(tab)` when
//! it starts and compares later: changed → a navigation really happened.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

static GEN: OnceLock<Mutex<HashMap<usize, u64>>> = OnceLock::new();
/// Hard navigations only (full page loads / PageLoadStarted), NOT SPA pushState.
/// The action watchdog short-circuits to "navigated" only on these, so an
/// `expect: url:#/route` on a pushState click is still checked by the probe.
static HARD: OnceLock<Mutex<HashMap<usize, u64>>> = OnceLock::new();
/// Downloads started per tab: (count, last filename). An action whose only
/// visible effect is a download would otherwise read as "no observable change".
static DOWNLOADS: OnceLock<Mutex<HashMap<usize, (u64, String)>>> = OnceLock::new();

fn map() -> &'static Mutex<HashMap<usize, u64>> {
    GEN.get_or_init(|| Mutex::new(HashMap::new()))
}

fn hard_map() -> &'static Mutex<HashMap<usize, u64>> {
    HARD.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Record a full page navigation (PageLoadStarted). Also bumps the soft counter.
pub fn bump_hard(tab_id: usize) {
    *hard_map().lock().unwrap().entry(tab_id).or_insert(0) += 1;
    bump(tab_id);
}

/// Hard-navigation generation for `tab_id` (0 if none).
pub fn hard_get(tab_id: usize) -> u64 {
    hard_map()
        .lock()
        .unwrap()
        .get(&tab_id)
        .copied()
        .unwrap_or(0)
}

fn downloads() -> &'static Mutex<HashMap<usize, (u64, String)>> {
    DOWNLOADS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Record that a navigation in `tab_id` became a download of `filename`.
pub fn note_download(tab_id: usize, filename: &str) {
    let mut m = downloads().lock().unwrap();
    let e = m.entry(tab_id).or_insert((0, String::new()));
    e.0 += 1;
    e.1 = filename.to_string();
}

/// (count, last filename) of downloads started in `tab_id`.
pub fn download_state(tab_id: usize) -> (u64, String) {
    downloads()
        .lock()
        .unwrap()
        .get(&tab_id)
        .cloned()
        .unwrap_or((0, String::new()))
}

/// Record that `tab_id` started a navigation or changed its URL.
pub fn bump(tab_id: usize) {
    *map().lock().unwrap().entry(tab_id).or_insert(0) += 1;
}

/// Current generation for `tab_id` (0 if it never navigated).
pub fn get(tab_id: usize) -> u64 {
    map().lock().unwrap().get(&tab_id).copied().unwrap_or(0)
}

/// Drop bookkeeping for a closed tab.
pub fn forget(tab_id: usize) {
    map().lock().unwrap().remove(&tab_id);
    hard_map().lock().unwrap().remove(&tab_id);
    downloads().lock().unwrap().remove(&tab_id);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bump_and_forget() {
        let id = 987_654;
        assert_eq!(get(id), 0);
        bump(id);
        bump(id);
        assert_eq!(get(id), 2);
        forget(id);
        assert_eq!(get(id), 0);
    }

    #[test]
    fn downloads_are_counted_per_tab() {
        let id = 987_655;
        assert_eq!(download_state(id), (0, String::new()));
        note_download(id, "a.mp4");
        note_download(id, "b.jpg");
        assert_eq!(download_state(id), (2, "b.jpg".into()));
        forget(id);
        assert_eq!(download_state(id).0, 0);
    }
}
