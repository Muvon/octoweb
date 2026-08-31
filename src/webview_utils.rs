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

  // ── URL change tracking (SPA pushState / replaceState / popstate / hash) ──
  // Covers all SPA routing strategies:
  //  1. History API (pushState/replaceState) — patched on prototype so frameworks
  //     that cache History.prototype.pushState at import time still hit our hook.
  //  2. popstate — back/forward and history.go().
  //  3. hashchange — hash-based routers (e.g. Vue hash mode, legacy SPAs).
  //  4. BFCache restore — pageshow with persisted=true.
  // Deduplicates via _lastUrl so rapid replaceState calls don't spam IPC.
  (function () {
    var _lastUrl = location.href;
    function notify() {
      var url = location.href;
      if (url !== _lastUrl) {
        _lastUrl = url;
        _ipc({ type: 'url_changed', url: url });
      }
    }
    // Patch on History.prototype — survives frameworks that snapshot the method
    // reference at module init (React Router, Vue Router, etc.).
    var proto = History.prototype;
    function wrap(method) {
      var orig = proto[method];
      proto[method] = function () {
        var ret = orig.apply(this, arguments);
        notify();
        return ret;
      };
    }
    wrap('pushState');
    wrap('replaceState');
    // popstate fires on back/forward and explicit history.go() calls.
    window.addEventListener('popstate', function () { notify(); });
    // hashchange — hash-based routers that don't use pushState.
    window.addEventListener('hashchange', function () { notify(); });
    // BFCache restore: page was frozen in memory and restored on back/forward.
    // `pageshow` with `persisted=true` fires instead of load/DOMContentLoaded.
    // Notify Rust of the URL so the address bar updates, without triggering a
    // full page load cycle (didCommitNavigation/didFinish don't fire for BFCache).
    window.addEventListener('pageshow', function (e) {
      if (e.persisted) { notify(); }
    });
  }());

  // ── SPA title tracking — MutationObserver + setter intercept ──────────────
  // WKWebView's document_title_changed callback doesn't fire for SPA title
  // changes (pushState/replaceState). Two complementary mechanisms cover all
  // frameworks:
  //  1. MutationObserver on <title> — catches innerHTML/textContent mutations
  //     (Next.js <Head>, Remix, static SPAs).
  //  2. document.title setter intercept — catches direct property assignment
  //     (React useEffect, Vue watch, Angular Title service).
  // Deduplicates via _lastTitle so only actual changes fire IPC.
  (function () {
    var _lastTitle = document.title || '';

    function onTitleChange(title) {
      if (title && title !== _lastTitle) {
        _lastTitle = title;
        _ipc({ type: 'title_changed', title: title });
      }
    }

    // 1. MutationObserver — watches <title> element for child/text mutations.
    //    Deferred: <title> may not exist at injection time (document-start).
    function observeTitle() {
      var el = document.querySelector('title');
      if (!el) return;
      new MutationObserver(function () {
        onTitleChange(document.title);
      }).observe(el, { childList: true, characterData: true, subtree: true });
    }

    if (document.querySelector('title')) { observeTitle(); }
    else {
      // SPAs that create <title> dynamically — watch <head> for it to appear.
      var headObs = new MutationObserver(function () {
        if (document.querySelector('title')) {
          headObs.disconnect();
          observeTitle();
        }
      });
      var head = document.head || document.documentElement;
      headObs.observe(head, { childList: true, subtree: true });
    }

    // 2. Setter intercept — catches `document.title = "..."` assignments.
    //    The native setter is preserved so the browser's <title> element still
    //    updates normally; we just get notified as a side effect.
    var titleDesc = Object.getOwnPropertyDescriptor(Document.prototype, 'title')
                 || Object.getOwnPropertyDescriptor(HTMLDocument.prototype, 'title');
    if (titleDesc && titleDesc.set) {
      var origSet = titleDesc.set;
      Object.defineProperty(document, 'title', {
        get: function () { return titleDesc.get ? titleDesc.get.call(this) : _lastTitle; },
        set: function (v) { origSet.call(this, v); onTitleChange(v); },
        configurable: true
      });
    }
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

  // ── Inline AI edit: selection capture & replacement ──────────────────
  window.__inlineEditCapture = function() {
    var el = document.activeElement;
    var sel = window.getSelection();
    var text = '';
    var rect = null;

    if (el && (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA')) {
      text = el.value.substring(el.selectionStart, el.selectionEnd);
      window.__octoweb_edit = { type: 'input', element: el, start: el.selectionStart, end: el.selectionEnd };
      rect = el.getBoundingClientRect();
    } else if (sel && sel.rangeCount > 0 && sel.toString().length > 0) {
      // Only treat as a real selection if there's actually selected text.
      // A collapsed range (caret with no selection) is not useful here — fall
      // through to the "no context" branch below so the modal opens at a sane
      // default position rather than at <body>'s bottom edge.
      text = sel.toString();
      var range = sel.getRangeAt(0);
      window.__octoweb_edit = { type: 'range', range: range.cloneRange() };
      rect = range.getBoundingClientRect();
    }

    // No focused editable + no real selection: clear any stale edit anchor and
    // signal "no position" to the host so it can pick a default. We send NaN
    // (serializes as null in JSON) so Rust can detect the missing position.
    if (!rect || (rect.width === 0 && rect.height === 0)) {
      window.__octoweb_edit = null;
      _ipc({ type: 'inline_edit_ready', text: '', x: null, y: null });
      return;
    }

    var x = rect.left;
    var y = rect.bottom + 4;
    _ipc({ type: 'inline_edit_ready', text: text, x: x, y: y });
  };

  window.__inlineEditReplace = function(newText) {
    var edit = window.__octoweb_edit;
    if (!edit) return;

    if (edit.type === 'input') {
      var el = edit.element;
      el.value = el.value.substring(0, edit.start) + newText + el.value.substring(edit.end);
      el.selectionStart = el.selectionEnd = edit.start + newText.length;
      el.dispatchEvent(new Event('input', { bubbles: true }));
    } else if (edit.type === 'range') {
      edit.range.deleteContents();
      edit.range.insertNode(document.createTextNode(newText));
    }

    window.__octoweb_edit = null;
  };

  // ── MCP observability: console + network ring buffers, listener tagging ──
  // Read by browser_console_messages / browser_network_requests and by the
  // snapshot script. All globals are non-enumerable so pages iterating
  // `window` don't trip over them. Buffers cap at 200 entries (FIFO).
  (function () {
    // Every network entry gets a monotonic `seq` (shared across both rings) so
    // the action effect probe can ask "what fired after my click?" even after
    // the ring has rotated.
    var netSeq = 0;
    function ring(seq) {
      var b = [];
      b.push2 = function (e) { if (seq) { e.seq = ++netSeq; window.__octoweb_netseq = netSeq; } b.push(e); if (b.length > 200) b.shift(); };
      return b;
    }
    var con = ring(false), net = ring(true), res = ring(true);
    Object.defineProperty(window, '__octoweb_console', { value: con, configurable: true });
    Object.defineProperty(window, '__octoweb_net', { value: net, configurable: true });
    // Resource-timing ring: beacons, images, scripts, iframes, form posts —
    // everything the fetch/XHR wrappers can't see. Kept separate so a page
    // with 300 images doesn't rotate the fetch/XHR entries out.
    Object.defineProperty(window, '__octoweb_res', { value: res, configurable: true });
    window.__octoweb_netseq = 0;

    // Console wrap — keeps original behavior, mirrors a truncated text line.
    ['log', 'info', 'warn', 'error', 'debug'].forEach(function (level) {
      var orig = console[level];
      console[level] = function () {
        try {
          var parts = [];
          for (var i = 0; i < arguments.length; i++) {
            var a = arguments[i];
            if (typeof a === 'string') { parts.push(a); }
            else { try { parts.push(JSON.stringify(a)); } catch (e) { parts.push(String(a)); } }
          }
          con.push2({ level: level, text: parts.join(' ').substring(0, 500), ts: Date.now() });
        } catch (e) {}
        return orig.apply(console, arguments);
      };
    });
    window.addEventListener('error', function (e) {
      con.push2({ level: 'error', text: ('Uncaught: ' + e.message + ' @' + (e.filename || '') + ':' + (e.lineno || 0)).substring(0, 500), ts: Date.now() });
    });
    window.addEventListener('unhandledrejection', function (e) {
      var r; try { r = String((e.reason && e.reason.message) || e.reason); } catch (err) { r = '?'; }
      con.push2({ level: 'error', text: ('Unhandled rejection: ' + r).substring(0, 500), ts: Date.now() });
    });

    // Response bodies let the AI read the API behind a UI without scraping the
    // DOM. Cap at 20 KB, texty content-types only (json/text/xml/urlencoded) —
    // never buffer images or downloads. Body is read from a clone so the page's
    // own consumer is unaffected.
    var BODY_CAP = 20000;
    function textyType(ct) { return /json|text|xml|javascript|urlencoded|graphql/i.test(ct || ''); }

    // fetch wrap
    var origFetch = window.fetch;
    if (origFetch) window.fetch = function (input, init) {
      var url = ''; try { url = (typeof input === 'string') ? input : ((input && input.url) || String(input)); } catch (e) {}
      var method = (init && init.method) || (input && input.method) || 'GET';
      var t0 = performance.now();
      var entry = null;
      function rec(status, error) {
        entry = { method: method, url: String(url).substring(0, 300), status: status, type: 'fetch',
                  ms: Math.round(performance.now() - t0), ts: Date.now(), error: error };
        net.push2(entry);
      }
      return origFetch.apply(this, arguments).then(
        function (res) {
          rec(res.status);
          try {
            var ct = res.headers && res.headers.get ? res.headers.get('content-type') : '';
            if (entry && textyType(ct)) {
              res.clone().text().then(function (t) { if (entry) entry.body = String(t).substring(0, BODY_CAP); }).catch(function () {});
            }
          } catch (e) {}
          return res;
        },
        function (err) { rec(0, String(err).substring(0, 200)); throw err; }
      );
    };

    // Resource timing — no status code from this API (responseStatus only on
    // newer WebKit), but it proves *whether* a beacon / image / script fired.
    try {
      var po = new PerformanceObserver(function (l) {
        var es = l.getEntries();
        for (var i = 0; i < es.length; i++) {
          var e = es[i], it = e.initiatorType || 'other';
          if (it === 'fetch' || it === 'xmlhttprequest') continue;
          res.push2({ method: it === 'beacon' ? 'POST' : 'GET', url: String(e.name).substring(0, 300),
                      status: e.responseStatus || 0, type: it, ms: Math.round(e.duration || 0), ts: Date.now() });
        }
      });
      po.observe({ type: 'resource', buffered: true });
    } catch (e) {}

    // XHR wrap
    var XO = XMLHttpRequest.prototype.open, XS = XMLHttpRequest.prototype.send;
    XMLHttpRequest.prototype.open = function (m, u) { this.__ow_req = [String(m), String(u)]; return XO.apply(this, arguments); };
    XMLHttpRequest.prototype.send = function () {
      var x = this, t0 = performance.now();
      x.addEventListener('loadend', function () {
        var i = x.__ow_req || ['?', '?'];
        var entry = { method: i[0], url: i[1].substring(0, 300), status: x.status, type: 'xhr',
                      ms: Math.round(performance.now() - t0), ts: Date.now() };
        try {
          var ct = x.getResponseHeader && x.getResponseHeader('content-type');
          if (textyType(ct) && (x.responseType === '' || x.responseType === 'text')) {
            entry.body = String(x.responseText || '').substring(0, BODY_CAP);
          }
        } catch (e) {}
        net.push2(entry);
      });
      return XS.apply(this, arguments);
    };

    // Click-listener tagging: lets browser_snapshot surface <div>-buttons whose
    // only interactivity is an addEventListener — invisible to role/tag scans.
    var tagged = new WeakSet();
    Object.defineProperty(window, '__octoweb_listeners', { value: tagged, configurable: true });
    var origAdd = EventTarget.prototype.addEventListener;
    EventTarget.prototype.addEventListener = function (type) {
      try {
        if ((type === 'click' || type === 'pointerdown' || type === 'mousedown') && this && this.nodeType === 1) tagged.add(this);
      } catch (e) {}
      return origAdd.apply(this, arguments);
    };
  }());

  // ── target=_blank link interceptor ────────────────────────────────────────
  // Clicks on <a target="_blank"> are caught here and routed through IPC to
  // open in a new browser tab. This prevents them from reaching the native
  // createWebViewWithConfiguration handler (which creates a popup window).
  // Genuine window.open() calls (OAuth, sign-up flows) are NOT affected —
  // they bypass this listener and go through the native handler where
  // window.opener is preserved.
  document.addEventListener('click', function (e) {
    var el = e.target;
    while (el && el.tagName !== 'A') el = el.parentElement;
    if (!el) return;
    var t = (el.getAttribute('target') || '').toLowerCase();
    if (t !== '_blank') return;
    var href = el.href;
    if (!href || href.startsWith('javascript:')) return;
    e.preventDefault();
    e.stopImmediatePropagation();
    _ipc({ type: 'open_new_tab', url: href });
  }, true);

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
    hibernated: &std::collections::HashSet<usize>,
) -> String {
    let mut items: Vec<serde_json::Value> = Vec::new();

    // ── Tabs ──────────────────────────────────────────────────────────────────
    let open_urls: std::collections::HashSet<&str> =
        tabs.iter().map(|t| t.url.trim_end_matches('/')).collect();
    let visits_by_url: HashMap<&str, u32> = history
        .iter()
        .map(|e| (e.url.trim_end_matches('/'), e.visit_count))
        .collect();

    for tab in tabs {
        // Look up this tab's visit count from history (it's stored there)
        let visits = visits_by_url
            .get(tab.url.trim_end_matches('/'))
            .copied()
            .unwrap_or(0);
        items.push(serde_json::json!({
            "kind": "tab",
            "tab_id": tab.id,
            "title": tab.title,
            "url": tab.url,
            "favicon": cached_favicon(&tab.url, favicons),
            "visit_count": visits,
            "visited_at": 0u64,  // tabs are live — recency handled in JS as "now"
            "hibernated": hibernated.contains(&tab.id),
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

/// Wrap a JS *expression* so its string result is well-formed UTF-16 before it
/// leaves the page.
///
/// wry's `evaluate_script_with_callback` completion handler does
/// `NSJSONSerialization::dataWithJSONObject(...).unwrap()` (wry 0.55
/// `wkwebview/mod.rs:742`). `NSJSONSerialization` returns NSError 3852 ("data
/// couldn't be written because of an error in the content of the data") for a
/// string containing a *lone UTF-16 surrogate* — routine in `innerText` /
/// element text on emoji-heavy pages. wry unwraps that Err and panics on the
/// main thread *inside an Objective-C block*, which cannot unwind → `abort()`
/// (whole-app crash). `String.prototype.toWellFormed()` (WebKit, Safari 17+)
/// replaces lone surrogates with U+FFFD while preserving valid pairs, so the
/// serializer never fails. Guarded so a non-string result or an older engine
/// passes through unchanged.
pub fn well_formed_js(expr: &str) -> String {
    format!("(function(){{var v=({expr});return (typeof v==='string'&&v.toWellFormed)?v.toWellFormed():v;}})()")
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
