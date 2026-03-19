//! WebView helper utilities: injected scripts, favicon cache, overlay data.

use std::collections::HashMap;

use crate::browser;

/// JS injected into every tab page — collects page load stats after load.
/// Reads PerformanceNavigationTiming for transferSize and duration,
/// then posts IPC with `{ type: "page_info", size, time }`.
pub const PAGE_INFO_SCRIPT: &str = r#"
(function() {
  'use strict';
  window.addEventListener('load', function() {
    // Delay slightly so the browser finalises the navigation timing entry.
    setTimeout(function() {
      var loc = window.location;
      if (loc.protocol !== 'https:' && loc.protocol !== 'http:') return;
      try {
        var entries = performance.getEntriesByType('navigation');
        if (!entries || !entries.length) return;
        var nav = entries[0];
        var size = nav.transferSize || nav.encodedBodySize || 0;
        var time = Math.round(nav.duration || 0);
        window.ipc.postMessage(JSON.stringify({ type: 'page_info', size: size, time: time }));
      } catch(e) {}
    }, 100);
  });
})();
"#;

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

  // Track playing media via WeakRef — no strong references to DOM elements,
  // so removed elements are GC'd naturally without a MutationObserver.
  // This avoids the heavy subtree MutationObserver that fires on every DOM
  // mutation during WebRTC calls (Google Meet churns DOM heavily).
  var nextId = 0;
  var playing = new Map();  // id → WeakRef<Element>
  var lastState = false;

  function sendState() {
    // Prune dead WeakRefs on each state check
    playing.forEach(function(ref, id) {
      if (!ref.deref()) playing.delete(id);
    });
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

  // Map element → id for O(1) removal on pause/ended
  var elToId = new WeakMap();

  function addPlaying(el) {
    if (!elToId.has(el)) {
      var id = nextId++;
      elToId.set(el, id);
      playing.set(id, new WeakRef(el));
    }
    sendState();
  }

  function removePlaying(el) {
    if (elToId.has(el)) {
      var id = elToId.get(el);
      playing.delete(id);
    }
    sendState();
  }

  // Capture phase catches events before they can be stopPropagation'd.
  // Media events (play/pause/ended) don't bubble, but they DO get captured.
  document.addEventListener('play', function(e) {
    if (isMedia(e.target)) addPlaying(e.target);
  }, true);

  document.addEventListener('pause', function(e) {
    if (isMedia(e.target)) removePlaying(e.target);
  }, true);

  document.addEventListener('ended', function(e) {
    if (isMedia(e.target)) removePlaying(e.target);
  }, true);

  // Emptied/error — media element reset or failed
  document.addEventListener('emptied', function(e) {
    if (isMedia(e.target)) removePlaying(e.target);
  }, true);
})();
"#;

/// Serialize open tabs + recent history into a JSON array for the overlay JS.
/// Favicons come from the local cache (base64 data-URIs) — no external requests.
pub fn build_items_json(
    tabs: &[browser::Tab],
    history: &[browser::HistoryEntry],
    visit_counts: &std::collections::HashMap<String, u32>,
    favicons: &HashMap<String, String>,
) -> String {
    let mut items: Vec<serde_json::Value> = Vec::new();
    for tab in tabs {
        let key = tab.url.trim_end_matches('/');
        let visits = visit_counts.get(key).copied().unwrap_or(0);
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
            let key = entry.url.trim_end_matches('/');
            let visits = visit_counts.get(key).copied().unwrap_or(0);
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
/// Handles: backslash, backtick, `${` (template interpolation), and `</` (script injection).
pub fn escape_js_template(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + s.len() / 8);
    for (i, ch) in s.char_indices() {
        match ch {
            '\\' => out.push_str("\\\\"),
            '`' => out.push_str("\\`"),
            // Escape ${ to prevent template literal interpolation
            '$' if s.as_bytes().get(i + 1) == Some(&b'{') => out.push_str("\\$"),
            // Break up </script> to prevent early HTML parser termination
            '<' if s.as_bytes().get(i + 1) == Some(&b'/') => out.push_str("<\\/"),
            _ => out.push(ch),
        }
    }
    out
}

/// Find-in-page script using CSS Custom Highlight API.
/// Injected into tab WebViews. Exposes:
/// - `window.__findInPage(query)` — highlight all matches, scroll to first
/// - `window.__findNext()` / `window.__findPrev()` — cycle through matches
/// - `window.__findClear()` — remove all highlights
///
/// Posts IPC `{ type: "find_count", current, total }` after each operation.
pub const FIND_IN_PAGE_SCRIPT: &str = r#"
(function() {
  if (window.__findInPage) return;

  let ranges = [];
  let currentIdx = -1;
  const HIGHLIGHT_NAME = 'octoweb-find';
  const CURRENT_NAME = 'octoweb-find-current';

  // Inject highlight styles once.
  const s = document.createElement('style');
  s.textContent = `
    ::highlight(octoweb-find) { background-color: rgba(255, 230, 0, 0.35); color: inherit; }
    ::highlight(octoweb-find-current) { background-color: rgba(255, 150, 50, 0.6); color: inherit; }
  `;
  (document.head || document.documentElement).appendChild(s);

  // --- Text node cache ---
  // Walk DOM once per search; invalidate on mutation so next search rebuilds.
  // TreeWalker with numeric whatToShow (not a filter function) is ~10x faster —
  // the browser applies it natively without JS callback overhead.
  let textNodeCache = null;

  function getTextNodes() {
    if (textNodeCache) return textNodeCache;
    const root = document.body;
    if (!root) return [];
    const nodes = [];
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    let node;
    while ((node = walker.nextNode())) {
      const tag = node.parentElement && node.parentElement.tagName;
      if (tag !== 'SCRIPT' && tag !== 'STYLE' && tag !== 'NOSCRIPT') nodes.push(node);
    }
    textNodeCache = nodes;
    return nodes;
  }

  // Invalidate cache when DOM changes (e.g. SPA navigation, lazy content).
  // Guard against document.body being null at injection time.
  if (document.body) {
    new MutationObserver(function() { textNodeCache = null; })
      .observe(document.body, { childList: true, subtree: true });
  }

  // --- Highlight helpers ---

  // Clear a named highlight: empty its Range Set first so WebKit repaints,
  // then remove from registry. delete() alone doesn't always trigger repaint.
  function clearHighlight(name) {
    const h = CSS.highlights.get(name);
    if (h) h.clear();
    CSS.highlights.delete(name);
  }

  function postCount() {
    window.ipc.postMessage(JSON.stringify({
      type: 'find_count',
      current: ranges.length > 0 ? currentIdx + 1 : 0,
      total: ranges.length
    }));
  }

  function highlightCurrent() {
    if (currentIdx >= 0 && currentIdx < ranges.length) {
      // Clear first so WebKit repaints the old position before setting the new one.
      clearHighlight(CURRENT_NAME);
      CSS.highlights.set(CURRENT_NAME, new Highlight(ranges[currentIdx]));
      const rect = ranges[currentIdx].getBoundingClientRect();
      if (rect.top < 0 || rect.bottom > window.innerHeight) {
        ranges[currentIdx].startContainer.parentElement
          ?.scrollIntoView({ behavior: 'smooth', block: 'center' });
      }
    } else {
      clearHighlight(CURRENT_NAME);
    }
  }

  // --- Public API ---

  window.__findInPage = function(query) {
    ranges = [];
    currentIdx = -1;
    if (!CSS.highlights) { postCount(); return; }
    clearHighlight(HIGHLIGHT_NAME);
    clearHighlight(CURRENT_NAME);

    if (!query) { postCount(); return; }

    const lower = query.toLowerCase();
    const qLen  = query.length;
    const nodes = getTextNodes();

    for (let ni = 0; ni < nodes.length; ni++) {
      const text = nodes[ni].textContent.toLowerCase();
      let start = 0;
      let idx;
      while ((idx = text.indexOf(lower, start)) !== -1) {
        const r = new Range();
        r.setStart(nodes[ni], idx);
        r.setEnd(nodes[ni], idx + qLen);
        ranges.push(r);
        start = idx + 1;
      }
    }

    if (ranges.length > 0) {
      CSS.highlights.set(HIGHLIGHT_NAME, new Highlight(...ranges));
      currentIdx = 0;
      highlightCurrent();
    }
    postCount();
  };

  window.__findNext = function() {
    if (ranges.length === 0) return;
    currentIdx = (currentIdx + 1) % ranges.length;
    highlightCurrent();
    postCount();
  };

  window.__findPrev = function() {
    if (ranges.length === 0) return;
    currentIdx = (currentIdx - 1 + ranges.length) % ranges.length;
    highlightCurrent();
    postCount();
  };

  window.__findClear = function() {
    ranges = [];
    currentIdx = -1;
    if (CSS.highlights) {
      clearHighlight(HIGHLIGHT_NAME);
      clearHighlight(CURRENT_NAME);
    }
  };
})();
"#;

/// Look up a cached favicon data-URI by domain extracted from the URL.
pub fn cached_favicon<'a>(url: &str, favicons: &'a HashMap<String, String>) -> Option<&'a str> {
    let domain = extract_domain(url)?;
    favicons.get(domain).map(|s| s.as_str())
}

/// Extract the domain (host) from a URL, e.g. "https://example.com/path" → "example.com".
pub fn extract_domain(url: &str) -> Option<&str> {
    let after_scheme = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))?;
    after_scheme.split('/').next().filter(|h| !h.is_empty())
}
