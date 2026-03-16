//! WebView helper utilities: injected scripts, favicon cache, overlay data.

use std::collections::HashMap;

use crate::browser;

/// JS injected into every tab page at document-start.
/// Finds the best favicon (link[rel~=icon] → /favicon.ico), fetches it,
/// converts to a base64 data-URI, and posts IPC once per page load.
/// The Rust side deduplicates by domain so disk writes are rare.
pub const FAVICON_FETCH_SCRIPT: &str = r#"
(function() {
  'use strict';

  // Favicon fetch — runs on every page load
  window.addEventListener('load', function() {
    var loc = window.location;
    if (loc.protocol !== 'https:' && loc.protocol !== 'http:') return;
    var domain = loc.hostname;
    if (!domain) return;

    // Find best <link rel="icon"> href
    var best = null;
    var links = document.querySelectorAll('link[rel]');
    for (var i = 0; i < links.length; i++) {
      var rel = links[i].rel.toLowerCase();
      if (rel.indexOf('icon') !== -1 && links[i].href) {
        best = links[i].href;
        if (rel === 'icon' || rel === 'shortcut icon') break;
      }
    }
    var url = best || (loc.protocol + '//' + loc.host + '/favicon.ico');

    fetch(url, { cache: 'force-cache' })
      .then(function(r) { return r.ok ? r.blob() : Promise.reject(); })
      .then(function(blob) {
        return new Promise(function(resolve, reject) {
          var reader = new FileReader();
          reader.onload = function() { resolve(reader.result); };
          reader.onerror = reject;
          reader.readAsDataURL(blob);
        });
      })
      .then(function(dataUri) {
        window.ipc.postMessage(JSON.stringify({ type: 'favicon', domain: domain, data: dataUri }));
      })
      .catch(function() {});
  });
})();
"#;

/// Tracks audio/video playback state and notifies Rust via IPC.
/// Uses document-level capture-phase listeners to catch all media events,
/// including dynamically added elements (YouTube, SPAs, etc.).
pub const MEDIA_TRACK_SCRIPT: &str = r#"
(function() {
  'use strict';

  // Track which media elements are currently playing
  var playing = new Set();
  var lastState = false;

  function sendState() {
    var nowPlaying = playing.size > 0;
    if (nowPlaying !== lastState) {
      lastState = nowPlaying;
      var msg = nowPlaying ? 'media:playing' : 'media:paused';
      window.ipc.postMessage(JSON.stringify({ type: msg }));
    }
  }

  function isMedia(el) {
    var tag = el && el.tagName;
    return tag === 'VIDEO' || tag === 'AUDIO';
  }

  // Capture phase catches events before they can be stopPropagation'd.
  // Media events (play/pause/ended) don't bubble, but they DO get captured.
  document.addEventListener('play', function(e) {
    if (isMedia(e.target)) { playing.add(e.target); sendState(); }
  }, true);

  document.addEventListener('pause', function(e) {
    if (isMedia(e.target)) { playing.delete(e.target); sendState(); }
  }, true);

  document.addEventListener('ended', function(e) {
    if (isMedia(e.target)) { playing.delete(e.target); sendState(); }
  }, true);

  // Emptied/error — media element reset or failed
  document.addEventListener('emptied', function(e) {
    if (isMedia(e.target)) { playing.delete(e.target); sendState(); }
  }, true);

  // Clean up removed elements so the Set doesn't hold stale refs
  var observer = new MutationObserver(function(mutations) {
    var changed = false;
    mutations.forEach(function(mutation) {
      mutation.removedNodes.forEach(function(node) {
        if (node.nodeType !== 1) return;
        if (isMedia(node) && playing.delete(node)) changed = true;
        var children = node.querySelectorAll ? node.querySelectorAll('audio, video') : [];
        for (var i = 0; i < children.length; i++) {
          if (playing.delete(children[i])) changed = true;
        }
      });
    });
    if (changed) sendState();
  });
  observer.observe(document.documentElement || document, { childList: true, subtree: true });
})();
"#;

/// Serialize open tabs + recent history into a JSON array for the overlay JS.
/// Favicons come from the local cache (base64 data-URIs) — no external requests.
pub fn build_items_json(
    tabs: &[browser::Tab],
    history: &[browser::HistoryEntry],
    tm: &browser::TabManager,
    favicons: &HashMap<String, String>,
) -> String {
    let mut items: Vec<serde_json::Value> = Vec::new();
    for tab in tabs {
        let visits = tm.visit_count(&tab.url);
        items.push(serde_json::json!({
            "kind": "tab",
            "tab_id": tab.id,
            "title": tab.title,
            "url": tab.url,
            "favicon": cached_favicon(&tab.url, favicons),
            "visit_count": visits,
        }));
    }
    let open_urls: std::collections::HashSet<&str> =
        tabs.iter().map(|t| t.url.trim_end_matches('/')).collect();
    for entry in history.iter().rev().take(200) {
        if !open_urls.contains(entry.url.trim_end_matches('/')) {
            let visits = tm.visit_count(&entry.url);
            items.push(serde_json::json!({
                "kind": "history",
                "title": entry.title,
                "url": entry.url,
                "favicon": cached_favicon(&entry.url, favicons),
                "visit_count": visits,
                "visited_at": entry.visited_at,
            }));
        }
    }
    serde_json::to_string(&items).unwrap_or_else(|_| "[]".into())
}

/// Escape a string for safe embedding in a JS template literal (backtick string).
/// Handles: backslash, backtick, and `${` (template interpolation).
pub fn escape_js_template(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + s.len() / 8);
    for ch in s.chars() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '`' => out.push_str("\\`"),
            '$' => out.push_str("\\$"),
            _ => out.push(ch),
        }
    }
    out
}

/// Look up a cached favicon data-URI by domain extracted from the URL.
pub fn cached_favicon(url: &str, favicons: &HashMap<String, String>) -> Option<String> {
    let after_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let host = after_scheme.split('/').next().filter(|h| !h.is_empty())?;
    favicons.get(host).cloned()
}
