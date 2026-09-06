/// Returns the full HTML for the CMD+SHIFT+P overlay window.
/// The page is injected with `window.__items` (JSON array) before being shown.
/// Each item: { title, url, kind } where kind = "tab" | "history"
///
/// Tahoe liquid glass design — light/dark adaptive via the shared theme tokens.
pub fn html() -> String {
    r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<style>
/*@@THEME@@*/
  * { box-sizing: border-box; margin: 0; padding: 0; }

  html, body {
    width: 100%;
    height: 100%;
    background: transparent;
    font-family: var(--font-text);
    -webkit-font-smoothing: antialiased;
    color: var(--label);
  }

  #backdrop {
    position: fixed;
    inset: 0;
    background: transparent;
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding-top: 12vh;
  }

  #modal {
    width: min(680px, calc(100vw - 32px));
    background: var(--glass);
    backdrop-filter: var(--glass-blur);
    -webkit-backdrop-filter: var(--glass-blur);
    border-radius: var(--r-panel);
    box-shadow: var(--shadow-float), var(--glass-shine);
    overflow: hidden;
    animation: reveal var(--t-pop) var(--spring);
  }

  @keyframes reveal {
    from { transform: translateY(-12px) scale(0.97); opacity: 0; }
    to { transform: translateY(0) scale(1); opacity: 1; }
  }

  #search-row {
    display: flex;
    align-items: center;
    gap: 11px;
    min-height: 48px;
    margin: 12px 12px 6px;
    padding: 8px 12px;
    background: var(--fill);
    border-radius: var(--r-capsule);
    box-shadow: inset 0 0 0 0.5px var(--hairline);
    transition: background var(--t-fast) var(--ease), box-shadow var(--t-fast) var(--ease);
  }
  #search-row:hover { background: var(--fill-hover); }
  #search-row:focus-within { box-shadow: inset 0 0 0 1px var(--accent), 0 0 0 3px color-mix(in srgb, var(--accent) 22%, transparent); }

  #search-icon {
    width: 18px;
    height: 18px;
    color: var(--label-3);
    flex-shrink: 0;
  }

  #query {
    flex: 1;
    background: transparent;
    border: none;
    outline: none;
    color: var(--label);
    font-size: 15px;
    font-weight: 400;
    letter-spacing: -0.01em;
    caret-color: var(--accent);
  }

  #query::placeholder {
    color: var(--label-3);
  }

  #action-badge {
    padding: 5px 11px;
    min-height: 24px;
    border-radius: var(--r-capsule);
    background: var(--fill-hover);
    box-shadow: 0 0 0 0.5px var(--hairline);
    font-size: 11px;
    font-weight: 500;
    color: var(--label-2);
    letter-spacing: 0.01em;
    white-space: nowrap;
  }

  #results {
    max-height: min(420px, 56vh);
    overflow-y: auto;
    padding: 6px;
  }

  #results::-webkit-scrollbar {
    width: 6px;
  }

  #results::-webkit-scrollbar-track {
    background: transparent;
  }

  #results::-webkit-scrollbar-thumb {
    background: var(--label-4);
    border-radius: var(--r-capsule);
  }

  .item {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 40px;
    padding: 8px 10px;
    border-radius: calc(var(--r-panel) - 6px);
    cursor: default;
    transition: background var(--t-fast) var(--ease), transform var(--t-fast) var(--ease);
  }

  .item:hover { background: var(--fill-hover); }
  .item:active { background: var(--fill-press); transform: scale(0.995); }
  .item.selected {
    background: color-mix(in srgb, var(--accent) 15%, transparent);
  }
  .item.selected:active { background: color-mix(in srgb, var(--accent) 22%, transparent); }

  .item-icon {
    width: 18px;
    height: 18px;
    color: var(--label-2);
    flex-shrink: 0;
  }

  .item-favicon {
    width: 18px;
    height: 18px;
    flex-shrink: 0;
    border-radius: 4px;
    object-fit: contain;
    background: var(--glass-thin);
  }

  /* Hibernated-tab indicator — small dim dot on the favicon corner */
  .icon-slot { position: relative; width: 18px; height: 18px; flex-shrink: 0; display: inline-block; }
  .icon-slot .item-favicon, .icon-slot > svg { display: block; }
  .icon-slot.hibernated::after {
    content: '';
    position: absolute;
    right: -1px;
    bottom: -1px;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--warn);
    box-shadow: 0 0 0 1.5px var(--glass-thick);
  }

  .item-text {
    flex: 1;
    min-width: 0;
  }

  .item-title {
    font-size: 13px;
    font-weight: 500;
    color: var(--label);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .item-title .match {
    color: var(--accent);
    font-weight: 600;
  }

  .item-url {
    margin-top: 1px;
    font-size: 11px;
    font-family: var(--font-text);
    color: var(--label-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .item-meta {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .kind-pill {
    font-size: 11px;
    font-weight: 500;
    color: var(--label-2);
    background: var(--fill);
    box-shadow: 0 0 0 0.5px var(--hairline);
    border-radius: var(--r-capsule);
    padding: 2px 7px;
  }

  .close-btn {
    width: 22px;
    height: 22px;
    border: 0;
    border-radius: 50%;
    background: transparent;
    color: var(--label-2);
    font-size: 14px;
    line-height: 22px;
    cursor: pointer;
    opacity: 0;
    transition: opacity var(--t-fast) var(--ease), background var(--t-fast) var(--ease),
                color var(--t-fast) var(--ease), transform var(--t-fast) var(--ease);
  }

  .item.selected .close-btn {
    opacity: 1;
  }
  .close-btn:focus-visible { opacity: 1; }

  .close-btn:hover {
    background: color-mix(in srgb, var(--err) 14%, transparent);
    color: var(--err);
  }
  .close-btn:active { background: color-mix(in srgb, var(--err) 22%, transparent); transform: scale(0.9); }

  .shortcut-badge {
    font-size: 11px;
    font-weight: 500;
    font-family: inherit;
    color: var(--label-2);
    background: var(--fill);
    box-shadow: 0 0 0 0.5px var(--hairline);
    border-radius: var(--r-capsule);
    padding: 1px 5px;
    letter-spacing: 0.02em;
  }

  #hint {
    box-shadow: 0 -0.5px 0 var(--hairline);
    padding: 10px 14px;
    text-align: center;
    font-size: 11px;
    color: var(--label-2);
    letter-spacing: 0.01em;
  }

  #hint .kbd {
    margin: 0 2px;
  }
</style>
</head>
<body>
<div id="backdrop">
  <div id="modal" class="glass-panel">
    <div id="search-row">
      <svg id="search-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <circle cx="11" cy="11" r="7"></circle><line x1="21" y1="21" x2="16.65" y2="16.65"></line>
      </svg>
      <input id="query" type="text" autocomplete="off" spellcheck="false" placeholder="Search tabs, history, or enter URL" role="combobox" aria-expanded="true" aria-controls="results" aria-activedescendant="result-0" />
      <div id="action-badge">↵ Open</div>
    </div>
    <div id="results" role="listbox"></div>
    <div id="hint"><span class="kbd">↑↓</span> navigate&nbsp;&nbsp;&nbsp;<span class="kbd">↵</span> open&nbsp;&nbsp;&nbsp;<span class="kbd">esc</span> dismiss</div>
  </div>
</div>

<script src="octoweb-lib://localhost/fuzzysort.min.js"></script>
<script>
(function() {
  const queryEl = document.getElementById('query');
  const resultsEl = document.getElementById('results');
  const actionBadge = document.getElementById('action-badge');

  let items = [];
  let filtered = [];
  let sel = 0;
  let searchEngine = 'https://www.google.com/search?q={}';

  // Rust pushes the configured engine template on overlay open.
  window.__setSearchEngine = function(tpl) {
    if (typeof tpl === 'string' && tpl.indexOf('{}') >= 0) searchEngine = tpl;
  };

  function engineName() {
    try {
      const host = new URL(searchEngine.replace('{}', 'q')).hostname.replace(/^www\./, '');
      const known = {
        'google.com': 'Google', 'duckduckgo.com': 'DuckDuckGo', 'bing.com': 'Bing',
        'search.brave.com': 'Brave', 'ecosia.org': 'Ecosia', 'startpage.com': 'Startpage',
        'kagi.com': 'Kagi', 'yandex.com': 'Yandex'
      };
      return known[host] || host;
    } catch (e) {
      return 'the Web';
    }
  }
  let userQuery = ''; // tracks what the user actually typed (vs autofill)
  let pointerActive = false; // suppresses mouse until real movement after overlay open / keyboard nav
  let lastPointerX = 0;
  let lastPointerY = 0;
  const ICONS = {
    search: '<svg class="item-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="7"></circle><line x1="21" y1="21" x2="16.65" y2="16.65"></line></svg>',
    globe: '<svg class="item-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"></circle><line x1="2" y1="12" x2="22" y2="12"></line><path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"></path></svg>',
    tab: '<svg class="item-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"></rect><line x1="3" y1="9" x2="21" y2="9"></line></svg>',
    history: '<svg class="item-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 3v5h5"></path><path d="M3.05 13A9 9 0 1 0 6 6.3L3 8"></path><line x1="12" y1="7" x2="12" y2="12"></line><line x1="12" y1="12" x2="15" y2="15"></line></svg>',
    ai: '<svg class="item-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M12 2l2.09 6.26L20 10l-5.91 1.74L12 18l-2.09-6.26L4 10l5.91-1.74L12 2z"></path><path d="M18 14l1.18 3.54L22.72 19l-3.54 1.18L18 24l-1.18-3.54L13.28 19l3.54-1.18L18 14z"></path></svg>'
  };

  window.__setItems = function(data) {
    items = Array.isArray(data) ? data : [];
    queryEl.value = '';
    userQuery = '';
    sel = 0;
    pointerActive = false;
    window.ipc.postMessage(JSON.stringify({ type: 'overlay_open' }));
    render('');
    queryEl.focus();
  };

  // Refresh items in-place (after tab close / history remove) — keeps current query and selection.
  // Does NOT call render() to avoid the sel=0 reset; rebuilds filtered directly then re-renders.
  // Uses userQuery (what user typed) not queryEl.value (which may be autofilled URL).
  window.__refreshItems = function(data) {
    items = Array.isArray(data) ? data : [];
    const q = userQuery.trim().toLowerCase();
    if (!q) {
      // Empty query: same sort logic as render()
      const candidates = items.filter(i => i.kind === 'tab' || i.kind === 'history');
      candidates.sort((a, b) => {
        const recencyA = a.kind === 'tab' ? Date.now() / 1000 : (a.visited_at || 0);
        const recencyB = b.kind === 'tab' ? Date.now() / 1000 : (b.visited_at || 0);
        const freqA = (a.visit_count || 0) + (a.kind === 'tab' ? 5 : 0);
        const freqB = (b.visit_count || 0) + (b.kind === 'tab' ? 5 : 0);
        const ageDiffSecs = recencyB - recencyA;
        if (Math.abs(ageDiffSecs) > 86400) return ageDiffSecs > 0 ? 1 : -1;
        return freqB - freqA;
      });
      filtered = candidates.slice(0, 12);
    } else {
      // Non-empty query: re-run fuzzy search, keep action items
      const list = fuzzyRrf(q, items);
      const raw = userQuery.trim();
      const urlLike = isLikelyUrl(raw);
      const openAction = { kind: 'url', title: 'Open URL', url: toNavigableUrl(raw), subtitle: toNavigableUrl(raw), pill: 'URL' };
      const searchAction = { kind: 'search', title: 'Search ' + engineName(), url: raw, query: raw, subtitle: raw, pill: 'Search' };
      const askAction = { kind: 'ask', title: 'Ask AI', url: '', query: raw, subtitle: raw, pill: 'AI' };
      const actions = urlLike ? [openAction, searchAction, askAction] : [searchAction, openAction, askAction];
      filtered = [...list.slice(0, 14 - actions.length), ...actions];
    }
    // Clamp selection to new list size, keep position if possible
    sel = Math.min(sel, Math.max(filtered.length - 1, 0));
    renderItems();
    updateBadge();
  };

  queryEl.addEventListener('input', () => updateQuery(queryEl.value));
  queryEl.addEventListener('keydown', onInputKeyDown);

  // Escape remains available when focus moves from the combobox to a row action.
  document.addEventListener('keydown', e => {
    if (e.key === 'Escape') {
      e.preventDefault();
      closeOverlay();
    }
  });

  document.getElementById('backdrop').addEventListener('mousedown', e => {
    if (e.target === e.currentTarget) closeOverlay();
  });

  function onInputKeyDown(e) {
    if (handleEditingHotkeys(e)) return;

    if (e.key === 'ArrowDown') {
      e.preventDefault();
      move(1);
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      move(-1);
    } else if (e.key === 'Enter') {
      if (e.isComposing) return;
      e.preventDefault();
      if (e.metaKey && e.shiftKey) {
        // ⌘⇧Enter → Ask AI
        askAI();
      } else if (e.metaKey) {
        // ⌘Enter → force navigate (URL → open, else → search)
        forceNavigate();
      } else {
        confirmSelection();
      }
    } else if (e.key === 'Home') {
      e.preventDefault();
      setCursor(0);
    } else if (e.key === 'End') {
      e.preventDefault();
      setCursor(queryEl.value.length);
    }
  }

  function handleEditingHotkeys(e) {
    const isMacCmd = e.metaKey && !e.ctrlKey && !e.altKey;
    const isCtrl = e.ctrlKey && !e.metaKey && !e.altKey;

    // ⌘1-⌘9, ⌘0 → jump to item at index 1-9, 10 (skip 0 — Enter handles it)
    if (isMacCmd && /^[0-9]$/.test(e.key)) {
      e.preventDefault();
      var idx = e.key === '0' ? 10 : parseInt(e.key, 10);
      if (idx < filtered.length) {
        var target = filtered[idx];
        if (target.kind === 'tab' || target.kind === 'history') {
          sel = idx;
          confirmSelection();
        }
      }
      return true;
    }

    if (isMacCmd && e.key.toLowerCase() === 'w') {
      e.preventDefault();
      if (filtered.length > 0 && sel >= 0 && sel < filtered.length) {
        const item = filtered[sel];
        if (item.kind === 'tab') {
          window.ipc.postMessage(JSON.stringify({ type: 'close_tab', tab_id: item.tab_id }));
        } else if (item.kind === 'history') {
          window.ipc.postMessage(JSON.stringify({ type: 'remove_history', url: item.url }));
        }
      }
      return true;
    }

    // Cmd+V is intentionally NOT handled here — the Edit menu's paste: action
    // already inserts the clipboard at the caret and fires an `input` event,
    // which our `input` listener catches to refresh results. A custom handler
    // here would race with the native paste and double the clipboard text
    // (e.g. paste "L" → "LL"), because keydown.preventDefault() does not
    // suppress key-equivalent menu items in WKWebView.

    if (!isCtrl) return false;

    const key = e.key.toLowerCase();
    if (key === 'a') {
      e.preventDefault();
      setCursor(0);
      return true;
    }
    if (key === 'e') {
      e.preventDefault();
      // Fill the input with the highlighted item's URL so it can be edited;
      // if it's already there (or nothing fillable), act as end-of-line.
      const hit = filtered[sel];
      const url = hit && (hit.kind === 'tab' || hit.kind === 'history') ? hit.url : '';
      if (url && url !== queryEl.value) {
        updateQuery(url, url.length);
      } else {
        setCursor(queryEl.value.length);
      }
      return true;
    }
    if (key === 'k') {
      e.preventDefault();
      const start = queryEl.selectionStart;
      updateQuery(queryEl.value.slice(0, start), start);
      return true;
    }
    if (key === 'u') {
      e.preventDefault();
      const end = queryEl.selectionEnd;
      updateQuery(queryEl.value.slice(end), 0);
      return true;
    }

    if (key === 'p') {
      e.preventDefault();
      move(-1);
      return true;
    }
    if (key === 'n') {
      e.preventDefault();
      move(1);
      return true;
    }

    return false;
  }

  function setCursor(pos) {
    queryEl.setSelectionRange(pos, pos);
  }

  function updateQuery(value, caret) {
    queryEl.value = value;
    userQuery = value;
    render(value);
    if (typeof caret === 'number') setCursor(Math.min(caret, value.length));
  }

  function closeOverlay() {
    window.ipc.postMessage(JSON.stringify({ type: 'overlay_close' }));
    window.ipc.postMessage(JSON.stringify({ type: 'close' }));
  }

  function confirmSelection() {
    if (filtered.length === 0) {
      const q = userQuery.trim();
      if (!q) {
        closeOverlay();
        return;
      }
      if (isLikelyUrl(q)) {
        navigate(toNavigableUrl(q));
      } else {
        navigate(searchUrl(q));
      }
      return;
    }

    const item = filtered[sel];
    if (item.kind === 'tab') {
      window.ipc.postMessage(JSON.stringify({ type: 'switch_tab', tab_id: item.tab_id }));
      return;
    }

    if (item.kind === 'search') {
      navigate(searchUrl(item.query));
      return;
    }

    if (item.kind === 'ask') {
      askAI();
      return;
    }

    navigate(item.url);
  }

  function forceNavigate() {
    const q = userQuery.trim();
    if (!q) return;
    if (isLikelyUrl(q)) {
      navigate(toNavigableUrl(q));
    } else {
      navigate(searchUrl(q));
    }
  }

  function askAI() {
    const q = userQuery.trim();
    if (!q) return;
    window.ipc.postMessage(JSON.stringify({ type: 'ask_ai', text: q }));
  }

  function navigate(url) {
    window.ipc.postMessage(JSON.stringify({ type: 'navigate', url }));
  }

  function searchUrl(q) {
    return searchEngine.replace('{}', encodeURIComponent(q));
  }

  function toNavigableUrl(raw) {
    const q = raw.trim();
    return hasScheme(q) ? q : 'https://' + q;
  }

  function hasScheme(value) {
    return /^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(value);
  }

  function isLikelyUrl(value) {
    const s = value.trim();
    if (!s || /\s/.test(s)) return false;
    if (hasScheme(s)) return true;

    const host = s.split(/[/?#]/)[0];
    if (host === 'localhost' || /^localhost:\d+$/.test(host)) return true;
    if (/^\d{1,3}(\.\d{1,3}){3}(:\d+)?$/.test(host)) return true;
    if (/^[a-z0-9-]+(\.[a-z0-9-]+)+(:\d+)?$/i.test(host)) return true;

    return false;
  }

  function move(dir) {
    if (filtered.length === 0) return;
    sel = (sel + dir + filtered.length) % filtered.length;
    pointerActive = false;
    updateSelection();
    updateBadge();
  }

  function render(rawQuery) {
    const raw = rawQuery.trim();
    const q = raw.toLowerCase();

    if (!raw) {
      // Empty query: show tabs + recent history combined, sorted by recency then frequency.
      // Tabs get a small boost since they're already open and immediately actionable.
      const candidates = items.filter(i => i.kind === 'tab' || i.kind === 'history');
      candidates.sort((a, b) => {
        const recencyA = a.kind === 'tab' ? Date.now() / 1000 : (a.visited_at || 0);
        const recencyB = b.kind === 'tab' ? Date.now() / 1000 : (b.visited_at || 0);
        const freqA = (a.visit_count || 0) + (a.kind === 'tab' ? 5 : 0);
        const freqB = (b.visit_count || 0) + (b.kind === 'tab' ? 5 : 0);
        // Primary: recency (last 24h treated as equally fresh, then decay)
        const ageDiffSecs = recencyB - recencyA;
        if (Math.abs(ageDiffSecs) > 86400) return ageDiffSecs > 0 ? 1 : -1;
        // Secondary: frequency within the same recency band
        return freqB - freqA;
      });
      filtered = candidates.slice(0, 12);
      sel = 0;
      renderItems();
      updateBadge();
      return;
    }

    const list = fuzzyRrf(q, items);
    const urlLike = isLikelyUrl(raw);
    const openAction = {
      kind: 'url',
      title: 'Open URL',
      url: toNavigableUrl(raw),
      subtitle: toNavigableUrl(raw),
      pill: 'URL'
    };
    const searchAction = {
      kind: 'search',
      title: 'Search ' + engineName(),
      url: raw,
      query: raw,
      subtitle: raw,
      pill: 'Search'
    };
    const askAction = {
      kind: 'ask',
      title: 'Ask AI',
      url: '',
      query: raw,
      subtitle: raw,
      pill: 'AI'
    };

    const actions = urlLike ? [openAction, searchAction, askAction] : [searchAction, openAction, askAction];
    filtered = [...list.slice(0, 14 - actions.length), ...actions];
    sel = 0;
    renderItems();
    updateBadge();
  }

  function updateBadge() {
    if (filtered.length === 0) {
      actionBadge.textContent = '↵ Search';
      return;
    }
    const item = filtered[sel];
    if (item.kind === 'tab')          actionBadge.textContent = '↵ Switch';
    else if (item.kind === 'history') actionBadge.textContent = '↵ Open';
    else if (item.kind === 'url')     actionBadge.textContent = '↵ Open URL';
    else if (item.kind === 'ask')     actionBadge.textContent = '↵ Ask AI';
    else                              actionBadge.textContent = '↵ Search';
  }

  function renderItems() {
    resultsEl.innerHTML = '';

    if (filtered.length === 0) {
      resultsEl.innerHTML = '<div style="padding:24px;color:var(--label-2);font-size:13px;text-align:center;">No matches found</div>';
      queryEl.removeAttribute('aria-activedescendant');
      return;
    }

    // Flat render in score order — kind shown via the pill badge on each item.
    let html = '';
    filtered.forEach((item, idx) => { html += renderItem(item, idx); });
    resultsEl.innerHTML = html;

    // Attach event listeners
    resultsEl.querySelectorAll('.item').forEach(row => {
      const idx = parseInt(row.dataset.idx, 10);
      row.addEventListener('pointermove', (e) => {
        if (!pointerActive) {
          // First pointermove after open/keyboard nav — calibrate position, don't select
          lastPointerX = e.clientX;
          lastPointerY = e.clientY;
          pointerActive = true;
          return;
        }
        if (e.clientX === lastPointerX && e.clientY === lastPointerY) return;
        lastPointerX = e.clientX;
        lastPointerY = e.clientY;
        if (sel === idx) return;
        sel = idx;
        // Update highlight only — do not rewrite input on mouse hover
        updateSelection();
        updateBadge();
      });
      row.addEventListener('mousedown', e => {
        if (e.target && e.target.classList && e.target.classList.contains('close-btn')) return;
        e.preventDefault();
        sel = idx;
        confirmSelection();
      });
    });

    resultsEl.querySelectorAll('.close-btn').forEach(btn => {
      btn.addEventListener('click', e => {
        e.stopPropagation();
        const tabId = Number(btn.getAttribute('data-tab-id'));
        if (Number.isFinite(tabId) && tabId > 0) {
          window.ipc.postMessage(JSON.stringify({ type: 'close_tab', tab_id: tabId }));
          return;
        }
        const histUrl = btn.getAttribute('data-history-url');
        if (histUrl) {
          window.ipc.postMessage(JSON.stringify({ type: 'remove_history', url: histUrl }));
        }
      });
    });

    updateSelection();
  }

  function updateSelection(scroll) {
    const rows = resultsEl.querySelectorAll('.item');
    rows.forEach((row, idx) => {
      const selected = idx === sel;
      row.classList.toggle('selected', selected);
      row.setAttribute('aria-selected', selected ? 'true' : 'false');
    });
    const selectedRow = rows[sel];
    if (!selectedRow) {
      queryEl.removeAttribute('aria-activedescendant');
      return;
    }
    queryEl.setAttribute('aria-activedescendant', selectedRow.id);
    if (scroll !== false) selectedRow.scrollIntoView({ block: 'nearest' });
  }

  function renderItem(item, idx) {
    const hostname = cleanHost(item.url || '');
    const rawTitle = (item.title && item.title !== item.url) ? item.title : hostname;
    const kindLabel = esc(item.pill || kindLabelFor(item));
    const selected = idx === sel ? ' selected' : '';
    const isJumpable = item.kind === 'tab' || item.kind === 'history';
    const shortcutNum = isJumpable ? (idx >= 1 && idx <= 9 ? String(idx) : idx === 10 ? '0' : '') : '';
    const actionShortcut = item.kind === 'ask' ? '⌘⇧↵' : (item.kind === 'search' || item.kind === 'url') ? '⌘↵' : '';
    const shortcutHtml = shortcutNum ? '<span class="shortcut-badge">⌘' + shortcutNum + '</span>'
                       : actionShortcut ? '<span class="shortcut-badge">' + actionShortcut + '</span>'
                       : '';
    const canClose = isJumpable;
    const closeAttr = item.kind === 'tab'
      ? 'data-tab-id="' + item.tab_id + '"'
      : 'data-history-url="' + esc(item.url) + '"';
    const closeHtml = canClose
      ? '<button class="close-btn" ' + closeAttr + ' title="Close tab" aria-label="Close tab">×</button>'
      : '';

    return '<div class="item' + selected + '" id="result-' + idx + '" role="option" aria-selected="' + (idx === sel ? 'true' : 'false') + '" data-idx="' + idx + '">' +
      iconHtml(item) +
      '<div class="item-text">' +
        '<div class="item-title">' + esc(rawTitle) + '</div>' +
        (hostname ? '<div class="item-url">' + esc(hostname) + '</div>' : '') +
      '</div>' +
      '<div class="item-meta">' +
        shortcutHtml +
        '<span class="kind-pill">' + kindLabel + '</span>' +
        closeHtml +
      '</div>' +
    '</div>';
  }

  function iconHtml(item) {
    var inner;
    if (item.favicon && (item.kind === 'tab' || item.kind === 'history')) {
      const fallback = item.kind === 'tab' ? ICONS.tab : ICONS.history;
      inner = '<img class="item-favicon" src="' + esc(item.favicon) + '" onerror="this.outerHTML=\'' + fallback.replace(/"/g, "'") + '\'" />';
    } else {
      inner = item.kind === 'tab' ? ICONS.tab
            : item.kind === 'history' ? ICONS.history
            : item.kind === 'url' ? ICONS.globe
            : item.kind === 'ask' ? ICONS.ai
            : ICONS.search;
    }
    var cls = 'icon-slot' + (item.hibernated ? ' hibernated' : '');
    return '<span class="' + cls + '" title="' + (item.hibernated ? 'Hibernated, click to wake' : '') + '">' + inner + '</span>';
  }

  function kindLabelFor(item) {
    if (item.kind === 'tab') return 'Tab';
    if (item.kind === 'history') return 'History';
    if (item.kind === 'url') return 'URL';
    if (item.kind === 'ask') return 'AI';
    return 'Search';
  }

  function esc(s) {
    return String(s)
      .replace(/&/g, '&amp;')
      .replace(/</g, '&lt;')
      .replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;');
  }

  function cleanHost(url) {
    if (!url) return '';
    if (url.startsWith('file://')) {
      // Show just the filename, not the full path
      return url.split('/').pop() || url.replace('file://', '');
    }
    return url.replace(/^https?:\/\//, '').replace(/\/$/, '') || url;
  }

  // Reciprocal Rank Fusion search over tabs + history.
  //
  // Three independent rank lists are fused:
  //   1. Fuzzy match rank  — fuzzysort score over title + url (relevance to query)
  //   2. Recency rank      — by visited_at descending (how recently visited)
  //   3. Frequency rank    — by visit_count descending (how often visited)
  //
  // RRF formula: score = Σ 1/(k + rank_i), k=60 (standard constant dampens top-rank dominance).
  // Tabs get +0.05 on the final score — a mild nudge since they're already open,
  // not a multiplier that buries history regardless of relevance.
  //
  // Result: a URL visited 50 times last week beats a tab opened 5 minutes ago
  // for a query that matches it well. Relevance + habit + recency all matter.
  function fuzzyRrf(q, list) {
    const K = 60;

    // ── Rank 1: fuzzy match ───────────────────────────────────────────────────
    const fuzzyResults = fuzzysort.go(q, list, { keys: ['title', 'url'], limit: 100, threshold: 0.3 });
    if (fuzzyResults.length === 0) return [];

    // Map item → fuzzy rank (0-indexed, lower = better match)
    const fuzzyRank = new Map();
    fuzzyResults.forEach((r, i) => fuzzyRank.set(r.obj, i));

    // Only score items that passed the fuzzy filter
    const candidates = fuzzyResults.map(r => r.obj);

    // ── Rank 2: recency ───────────────────────────────────────────────────────
    // Tabs have no visited_at — treat them as "just now" so they're not penalised.
    const nowSecs = Date.now() / 1000;
    const byRecency = candidates
      .slice()
      .sort((a, b) => {
        const ta = a.kind === 'tab' ? nowSecs : (a.visited_at || 0);
        const tb = b.kind === 'tab' ? nowSecs : (b.visited_at || 0);
        return tb - ta;
      });
    const recencyRank = new Map(byRecency.map((item, i) => [item, i]));

    // ── Rank 3: frequency ─────────────────────────────────────────────────────
    const byFreq = candidates
      .slice()
      .sort((a, b) => (b.visit_count || 0) - (a.visit_count || 0));
    const freqRank = new Map(byFreq.map((item, i) => [item, i]));

    // ── Fuse ──────────────────────────────────────────────────────────────────
    const qLower = q.toLowerCase();
    const scored = candidates.map(item => {
      const rFuzzy   = fuzzyRank.get(item)   ?? candidates.length;
      const rRecency = recencyRank.get(item) ?? candidates.length;
      const rFreq    = freqRank.get(item)    ?? candidates.length;
      const rrf = 1 / (K + rFuzzy) + 1 / (K + rRecency) + 1 / (K + rFreq);
      // Mild open-tab nudge — not a kind-based hard separation
      const tabBoost = item.kind === 'tab' ? 0.05 : 0;
      // Exact domain match: query == hostname (e.g. "x.com" typed → x.com/* floats above
      // airwallex.com/* even if airwallex has higher visit_count/recency).
      const host = cleanHost(item.url || '');
      const exactBoost = host === qLower ? 1.0 : host.startsWith(qLower + '/') ? 0.5 : 0;
      return { item, score: rrf + tabBoost + exactBoost };
    });

    scored.sort((a, b) => b.score - a.score);
    return scored.map(s => s.item);
  }
})();
</script>
</body>
</html>"#
    .replace("/*@@THEME@@*/", crate::theme::CSS)
}
