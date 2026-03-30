//! WebView helper utilities: injected scripts, favicon cache, overlay data.

use std::collections::HashMap;

use crate::browser;

/// Single JS script injected into every tab page at document-start.
///
/// Merges all five former scripts into one IIFE so JavaScriptCore compiles
/// and injects once per page instead of five times. Shared `_ipc` helper
/// deduplicates the `window.ipc.postMessage` call site. The `load` event
/// listener for page-stats and favicon is merged into a single handler.
///
/// Covers:
///  - Page load stats (PerformanceNavigationTiming)
///  - Favicon fetch → base64 data-URI IPC
///  - SPA URL tracking (pushState / replaceState / popstate)
///  - Audio/video playback state (WeakRef, no MutationObserver)
///  - Find-in-page (CSS Custom Highlight API, zero DOM mutation)
pub const COMBINED_SCRIPT: &str = r#"
(function () {
  'use strict';

  // Shared IPC helper — one call site instead of five.
  function _ipc(obj) { window.ipc.postMessage(JSON.stringify(obj)); }

  // ── Page stats + Favicon ── single load listener combining both tasks ──────
  // Merging two former load listeners cuts event-handler overhead per page.
  window.addEventListener('load', function () {
    var loc = window.location;
    if (loc.protocol !== 'https:' && loc.protocol !== 'http:') return;

    // Page load stats via PerformanceNavigationTiming (delayed 100ms so the
    // browser finalises the navigation timing entry before we read it).
    setTimeout(function () {
      try {
        var entries = performance.getEntriesByType('navigation');
        if (!entries || !entries.length) return;
        var nav = entries[0];
        _ipc({
          type: 'page_info',
          size: nav.transferSize || nav.encodedBodySize || 0,
          time: Math.round(nav.duration || 0)
        });
      } catch (e) {}
    }, 100);

    // Favicon fetch — tries <link rel="icon"> first, falls back to /favicon.ico.
    var domain = loc.hostname;
    if (!domain) return;
    var best = null;
    var links = document.querySelectorAll('link[rel]');
    for (var i = 0; i < links.length; i++) {
      var rel = links[i].rel.toLowerCase();
      if (rel.indexOf('icon') !== -1 && links[i].href) {
        best = links[i].href;
        if (rel === 'icon' || rel === 'shortcut icon') break;
      }
    }
    var faviconUrl = best || (loc.protocol + '//' + loc.host + '/favicon.ico');
    fetch(faviconUrl, { cache: 'force-cache' })
      .then(function (r) { return r.ok ? r.blob() : Promise.reject(); })
      .then(function (blob) {
        return new Promise(function (resolve, reject) {
          var reader = new FileReader();
          reader.onload = function () { resolve(reader.result); };
          reader.onerror = reject;
          reader.readAsDataURL(blob);
        });
      })
      .then(function (dataUri) { _ipc({ type: 'favicon', domain: domain, data: dataUri }); })
      .catch(function () {});
  });

  // ── URL change tracking (SPA pushState / replaceState / popstate) ─────────
  (function () {
    function notify(url) { _ipc({ type: 'url_changed', url: url }); }
    function wrap(method) {
      var orig = history[method];
      history[method] = function () {
        var ret = orig.apply(this, arguments);
        notify(location.href);
        return ret;
      };
    }
    wrap('pushState');
    wrap('replaceState');
    // popstate fires on back/forward and explicit history.go() calls.
    window.addEventListener('popstate', function () { notify(location.href); });
  }());

  // ── Media tracking — WeakRef + capture phase, no MutationObserver ─────────
  // WeakRef avoids strong DOM references so removed elements GC naturally.
  // Capture phase catches play/pause/ended on elements that stopPropagation.
  (function () {
    var nextId = 0;
    var playing = new Map();    // id → WeakRef<Element>
    var elToId  = new WeakMap();
    var lastState = false;

    function sendState() {
      // Prune dead WeakRefs on each check.
      playing.forEach(function (ref, id) { if (!ref.deref()) playing.delete(id); });
      var nowPlaying = playing.size > 0;
      if (nowPlaying !== lastState) {
        lastState = nowPlaying;
        _ipc({ type: nowPlaying ? 'media:playing' : 'media:paused' });
      }
    }
    function isMedia(el) { var t = el && el.tagName; return t === 'VIDEO' || t === 'AUDIO'; }
    function addPlaying(el) {
      if (!elToId.has(el)) {
        var id = nextId++;
        elToId.set(el, id);
        playing.set(id, new WeakRef(el));
      }
      sendState();
    }
    function removePlaying(el) {
      if (elToId.has(el)) { playing.delete(elToId.get(el)); }
      sendState();
    }
    document.addEventListener('play',    function (e) { if (isMedia(e.target)) addPlaying(e.target); },    true);
    document.addEventListener('pause',   function (e) { if (isMedia(e.target)) removePlaying(e.target); }, true);
    document.addEventListener('ended',   function (e) { if (isMedia(e.target)) removePlaying(e.target); }, true);
    document.addEventListener('emptied', function (e) { if (isMedia(e.target)) removePlaying(e.target); }, true);
  }());

  // ── Autoplay blocking — strip autoplay + defer preload on media elements ──
  // Only targets elements with the `autoplay` attribute (explicit site-initiated
  // autoplay). Removing it and setting preload="none" prevents the browser from
  // fetching media bytes until the user interacts — saving network, CPU, and
  // GPU memory on news/media-heavy pages (typically 3-5 autoplay videos/page).
  (function () {
    function block(el) {
      if (!el || el.nodeType !== 1) return;
      if (el.tagName === 'VIDEO' || el.tagName === 'AUDIO') {
        if (el.hasAttribute('autoplay')) {
          el.removeAttribute('autoplay');
          el.setAttribute('preload', 'none');
          if (!el.paused) el.pause();
        }
      } else {
        var kids = el.querySelectorAll('video[autoplay],audio[autoplay]');
        for (var i = 0; i < kids.length; i++) {
          kids[i].removeAttribute('autoplay');
          kids[i].setAttribute('preload', 'none');
        }
      }
    }
    new MutationObserver(function (muts) {
      for (var i = 0; i < muts.length; i++) {
        var ns = muts[i].addedNodes;
        for (var j = 0; j < ns.length; j++) block(ns[j]);
      }
    }).observe(document.documentElement, { childList: true, subtree: true });
    if (document.body) block(document.body);
    else document.addEventListener('DOMContentLoaded', function () { block(document.body); }, { once: true });
  }());

  // ── Speculative preconnect — DNS+TCP+TLS warmup on link hover ─────────────
  // When the pointer hovers a cross-origin link for 150 ms, inject <link
  // rel="preconnect"> and <link rel="dns-prefetch"> for that origin. This
  // resolves DNS + establishes TCP/TLS before the user clicks — saving
  // 100-300 ms of connection setup on the subsequent navigation.
  (function () {
    var _t = null, _done = {};
    document.addEventListener('pointerover', function (e) {
      var a = e.target.closest && e.target.closest('a[href]');
      if (!a) return;
      try {
        var o = new URL(a.href, location.href).origin;
        if (o === location.origin || o === 'null' || _done[o]) return;
        clearTimeout(_t);
        _t = setTimeout(function () {
          _done[o] = 1;
          var d = document.createElement('link');
          d.rel = 'dns-prefetch'; d.href = o;
          document.head.appendChild(d);
          var c = document.createElement('link');
          c.rel = 'preconnect'; c.href = o;
          document.head.appendChild(c);
        }, 150);
      } catch (e) {}
    }, true);
    document.addEventListener('pointerout', function (e) {
      if (e.target.closest && e.target.closest('a[href]')) clearTimeout(_t);
    }, true);
  }());

  // ── Find in page — CSS Custom Highlight API, zero DOM mutation ────────────
  // Guard prevents double-init on navigate-within-document.
  if (!window.__findInPage) {
    var _ranges = [];
    var _idx    = -1;
    var _HL  = 'octoweb-find';
    var _CUR = 'octoweb-find-current';

    // Inject CSS highlight styles once per document.
    var _style = document.createElement('style');
    _style.textContent =
      '::highlight(octoweb-find){background-color:rgba(255,230,0,.35);color:inherit}' +
      '::highlight(octoweb-find-current){background-color:rgba(255,150,50,.6);color:inherit}';
    (document.head || document.documentElement).appendChild(_style);

    // Text-node cache — invalidated by MutationObserver on DOM changes.
    // Walking fresh on every search avoids stale-cache races (SPA hydration,
    // lazy images, deferred scripts).
    var _cache = null;
    var _obs   = false;

    function _attachObs() {
      if (_obs || !document.body) return;
      new MutationObserver(function () { _cache = null; })
        .observe(document.body, { childList: true, subtree: true });
      _obs = true;
    }

    function _collectNodes() {
      if (!document.body) return [];
      var nodes = [], walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT), n;
      // Numeric whatToShow is ~10x faster than a callback-based filter.
      while ((n = walker.nextNode())) {
        var tag = n.parentElement && n.parentElement.tagName;
        if (tag !== 'SCRIPT' && tag !== 'STYLE' && tag !== 'NOSCRIPT') nodes.push(n);
      }
      return nodes;
    }

    function _nodes(force) {
      if (force || !_cache) { _cache = _collectNodes(); _attachObs(); }
      return _cache;
    }

    // Deferred observer setup: body may not exist at injection time
    // (initialization script runs at document-start).
    if (document.body) { _attachObs(); }
    else { document.addEventListener('DOMContentLoaded', _attachObs, { once: true }); }

    function _clearHL(name) {
      var h = CSS.highlights.get(name);
      if (h) h.clear();   // clear ranges first so WebKit repaints
      CSS.highlights.delete(name);
    }

    function _count() {
      _ipc({ type: 'find_count', current: _ranges.length ? _idx + 1 : 0, total: _ranges.length });
    }

    function _valid(r) {
      try { return r.startContainer.isConnected !== false && r.getBoundingClientRect().height > 0; }
      catch (_) { return false; }
    }

    function _hlCur() {
      if (_idx >= 0 && _idx < _ranges.length) {
        var r = _ranges[_idx];
        _clearHL(_CUR);
        if (!_valid(r)) return;
        CSS.highlights.set(_CUR, new Highlight(r));
        var rect = r.getBoundingClientRect();
        if (rect.top < 0 || rect.bottom > window.innerHeight) {
          r.startContainer.parentElement?.scrollIntoView({ behavior: 'smooth', block: 'center' });
        }
      } else { _clearHL(_CUR); }
    }

    window.__findInPage = function (query) {
      _ranges = []; _idx = -1;
      if (!CSS.highlights) { _count(); return; }
      _clearHL(_HL); _clearHL(_CUR);
      if (!query) { _count(); return; }
      var lower = query.toLowerCase(), qLen = query.length, nodes = _nodes(true);
      for (var ni = 0; ni < nodes.length; ni++) {
        var text = nodes[ni].textContent.toLowerCase(), start = 0, pos;
        while ((pos = text.indexOf(lower, start)) !== -1) {
          var r = new Range();
          r.setStart(nodes[ni], pos);
          r.setEnd(nodes[ni], pos + qLen);
          _ranges.push(r);
          start = pos + 1;
        }
      }
      if (_ranges.length) { CSS.highlights.set(_HL, new Highlight(..._ranges)); _idx = 0; _hlCur(); }
      _count();
    };

    window.__findNext = function () {
      if (!_ranges.length) return;
      _idx = (_idx + 1) % _ranges.length; _hlCur(); _count();
    };

    window.__findPrev = function () {
      if (!_ranges.length) return;
      _idx = (_idx - 1 + _ranges.length) % _ranges.length; _hlCur(); _count();
    };

    window.__findClear = function () {
      _ranges = []; _idx = -1;
      if (CSS.highlights) { _clearHL(_HL); _clearHL(_CUR); }
    };
  }

}());
"#;

/// Build the JSON array of items for the overlay palette.
///
/// Tabs come first (always shown). History is already deduplicated by URL in
/// memory (update_url upserts), so we just iterate newest-first, skip open-tab
/// URLs, and cap at 200 entries. visit_count is stored on each HistoryEntry.
///
/// Favicons come from the local cache (base64 data-URIs) — no external requests.
pub fn build_items_json(
    tabs: &[browser::Tab],
    history: &[browser::HistoryEntry],
    favicons: &HashMap<String, String>,
) -> String {
    let mut items: Vec<serde_json::Value> = Vec::new();

    // ── Tabs ──────────────────────────────────────────────────────────────────
    let open_urls: std::collections::HashSet<&str> =
        tabs.iter().map(|t| t.url.trim_end_matches('/')).collect();

    for tab in tabs {
        // Look up this tab's visit count from history (it's stored there)
        let visits = history
            .iter()
            .find(|e| e.url.trim_end_matches('/') == tab.url.trim_end_matches('/'))
            .map(|e| e.visit_count)
            .unwrap_or(0);
        items.push(serde_json::json!({
            "kind": "tab",
            "tab_id": tab.id,
            "title": tab.title,
            "url": tab.url,
            "favicon": cached_favicon(&tab.url, favicons),
            "visit_count": visits,
            "visited_at": 0u64,  // tabs are live — recency handled in JS as "now"
        }));
    }

    // ── History ───────────────────────────────────────────────────────────────
    // Newest-first (iter().rev()), skip URLs already shown as tabs, cap at 200.
    // Skip blank/internal pages — they're noise in the palette.
    let mut history_count = 0;
    for entry in history.iter().rev() {
        if history_count >= 200 {
            break;
        }
        if entry.url.is_empty() || entry.url == "about:blank" {
            continue;
        }
        if open_urls.contains(entry.url.trim_end_matches('/')) {
            continue;
        }
        items.push(serde_json::json!({
            "kind": "history",
            "title": entry.title,
            "url": entry.url,
            "favicon": cached_favicon(&entry.url, favicons),
            "visit_count": entry.visit_count,
            "visited_at": entry.visited_at,
        }));
        history_count += 1;
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
