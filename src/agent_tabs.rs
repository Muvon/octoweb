//! Tabs an MCP agent is driving.
//!
//! Background tabs are `setHidden:` views, and WebKit runs a hidden page the
//! way it runs a minimised one: `document.visibilityState` is "hidden",
//! `requestAnimationFrame` never fires and timers are throttled. Pages that
//! animate their UI into place (splash screens, modal transitions, motion
//! libraries) freeze half-way, so an agent working in a background tab — the
//! MCP default — sees overlays that never leave and controls stuck at
//! opacity 0. A tab the agent is driving is therefore kept *awake*: un-hidden
//! but parked off-screen, so WebKit renders it like a visible page while the
//! user never sees it.
//!
//! Hiding is not the only way a page goes dark: WebKit also treats every page
//! of a browser window covered by other windows as hidden — the usual state
//! while the user works in another app and the agent works here. An awake tab
//! therefore also stops WebKit's window-occlusion detection
//! ([`keep_rendering`]).
//!
//! The same record exempts the tab from hibernation, which otherwise reloads
//! it — discarding typed text, scroll position and SPA state — as soon as the
//! agent pauses for a model turn under memory pressure.

use std::collections::{HashMap, HashSet};
use std::time::{Duration, Instant};

use wry::WebViewExtMacOS;

/// How long after its last MCP call a background tab keeps rendering.
pub const AWAKE: Duration = Duration::from_secs(120);

/// How long after its last MCP call a tab is exempt from hibernation. Long
/// enough to cover a slow model turn or an agent waiting on the user.
pub const PROTECT: Duration = Duration::from_secs(900);

/// Horizontal origin of a parked tab: far enough left that no window is wide
/// enough to show any of it.
pub const PARK_X: i32 = -100_000;

/// Keep `wv` rendering while its window is covered by other windows (`on`),
/// or hand it back to WebKit's normal occlusion handling. Uses the WebKit SPI
/// `_windowOcclusionDetectionEnabled` (macOS 10.13.4+); a WebKit without it
/// leaves the tab throttled, as before.
pub fn keep_rendering(wv: &wry::WebView, on: bool) {
    use objc2::{msg_send, sel};
    let wk = wv.webview();
    unsafe {
        let available: bool =
            msg_send![&*wk, respondsToSelector: sel!(_setWindowOcclusionDetectionEnabled:)];
        if available {
            let _: () = msg_send![&*wk, _setWindowOcclusionDetectionEnabled: !on];
        }
    }
}

#[derive(Default)]
pub struct AgentTabs {
    last_call: HashMap<usize, Instant>,
}

impl AgentTabs {
    /// Record an MCP call that acts on `tab_id`.
    pub fn touch(&mut self, tab_id: usize, now: Instant) {
        self.last_call.insert(tab_id, now);
    }

    pub fn forget(&mut self, tab_id: usize) {
        self.last_call.remove(&tab_id);
    }

    /// Should this tab render like a visible page?
    pub fn is_awake(&self, tab_id: usize, now: Instant) -> bool {
        self.last_call
            .get(&tab_id)
            .is_some_and(|&at| now.duration_since(at) < AWAKE)
    }

    /// Tabs hibernation must leave alone.
    pub fn protected(&self, now: Instant) -> HashSet<usize> {
        self.last_call
            .iter()
            .filter(|(_, &at)| now.duration_since(at) < PROTECT)
            .map(|(&id, _)| id)
            .collect()
    }

    /// Drop records past [`PROTECT`] and split the rest into tabs to keep
    /// awake and tabs that have gone back to sleep.
    pub fn sweep(&mut self, now: Instant) -> (Vec<usize>, Vec<usize>) {
        self.last_call
            .retain(|_, at| now.duration_since(*at) < PROTECT);
        let (awake, asleep): (Vec<_>, Vec<_>) = self
            .last_call
            .iter()
            .partition(|(_, &at)| now.duration_since(at) < AWAKE);
        (
            awake.into_iter().map(|(&id, _)| id).collect(),
            asleep.into_iter().map(|(&id, _)| id).collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touched_tab_is_awake_then_protected_then_forgotten() {
        let t0 = Instant::now();
        let mut tabs = AgentTabs::default();
        tabs.touch(7, t0);
        assert!(tabs.is_awake(7, t0));
        assert!(tabs.protected(t0).contains(&7));

        let later = t0 + AWAKE + Duration::from_secs(1);
        assert!(!tabs.is_awake(7, later));
        assert!(tabs.protected(later).contains(&7));
        assert_eq!(tabs.sweep(later), (vec![], vec![7]));

        let gone = t0 + PROTECT + Duration::from_secs(1);
        assert!(tabs.protected(gone).is_empty());
        assert_eq!(tabs.sweep(gone), (vec![], vec![]));
        assert!(!tabs.is_awake(7, t0));
    }

    #[test]
    fn a_new_call_wakes_a_sleeping_tab() {
        let t0 = Instant::now();
        let mut tabs = AgentTabs::default();
        tabs.touch(3, t0);
        let later = t0 + AWAKE + Duration::from_secs(5);
        assert!(!tabs.is_awake(3, later));
        tabs.touch(3, later);
        assert!(tabs.is_awake(3, later));
        assert_eq!(tabs.sweep(later), (vec![3], vec![]));
    }

    #[test]
    fn unknown_tabs_are_neither_awake_nor_protected() {
        let now = Instant::now();
        let mut tabs = AgentTabs::default();
        tabs.touch(1, now);
        tabs.forget(1);
        assert!(!tabs.is_awake(1, now));
        assert!(tabs.protected(now).is_empty());
    }
}
