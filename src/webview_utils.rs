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
/// Uses MutationObserver to catch dynamically added media elements.
pub const MEDIA_TRACK_SCRIPT: &str = r#"
(function() {
  'use strict';

  // Count of currently playing media elements
  var playingCount = 0;

  function sendState() {
    var msg = playingCount > 0 ? 'media:playing' : 'media:paused';
    window.ipc.postMessage(JSON.stringify({ type: msg }));
  }

  function setupMediaListeners(el) {
    el.addEventListener('play', function() {
      playingCount++;
      sendState();
    });
    el.addEventListener('pause', function() {
      if (playingCount > 0) playingCount--;
      sendState();
    });
    el.addEventListener('ended', function() {
      if (playingCount > 0) playingCount--;
      sendState();
    });
    // If already playing (e.g., autoplay), count it
    if (!el.paused && !el.ended) {
      playingCount++;
      sendState();
    }
  }

  // Setup existing audio/video elements
  document.addEventListener('DOMContentLoaded', function() {
    var medias = document.querySelectorAll('audio, video');
    for (var i = 0; i < medias.length; i++) {
      setupMediaListeners(medias[i]);
    }
  });

  // Watch for dynamically added media elements
  var observer = new MutationObserver(function(mutations) {
    mutations.forEach(function(mutation) {
      mutation.addedNodes.forEach(function(node) {
        if (node.nodeType === 1) { // Element node
          if (node.tagName === 'AUDIO' || node.tagName === 'VIDEO') {
            setupMediaListeners(node);
          }
          // Also check children
          var children = node.querySelectorAll ? node.querySelectorAll('audio, video') : [];
          for (var i = 0; i < children.length; i++) {
            setupMediaListeners(children[i]);
          }
        }
      });
    });
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

/// Look up a cached favicon data-URI by domain extracted from the URL.
pub fn cached_favicon(url: &str, favicons: &HashMap<String, String>) -> Option<String> {
    let after_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    let host = after_scheme.split('/').next().filter(|h| !h.is_empty())?;
    favicons.get(host).cloned()
}
