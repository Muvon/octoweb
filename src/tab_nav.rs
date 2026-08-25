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

fn map() -> &'static Mutex<HashMap<usize, u64>> {
    GEN.get_or_init(|| Mutex::new(HashMap::new()))
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
}
