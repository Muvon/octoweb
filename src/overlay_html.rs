/// Returns the full HTML for the CMD+K overlay window.
/// The page is injected with `window.__items` (JSON array) before being shown.
/// Each item: { title, url, kind } where kind = "tab" | "history"
///
/// Tahoe liquid glass design — light/dark adaptive via prefers-color-scheme.
pub fn html() -> &'static str {
    r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<style>
  * { box-sizing: border-box; margin: 0; padding: 0; }

  /* ── Tahoe Liquid Glass tokens ─────────────────────────────────────────── */
  :root {
    /* Glass panel — light: frosted white */
    --glass-bg:        rgba(255, 255, 255, 0.72);
    --glass-border:    rgba(0, 0, 0, 0.08);
    --glass-inner:     rgba(255, 255, 255, 0.50);
    --glass-shadow:    0 24px 80px rgba(0, 0, 0, 0.18), 0 2px 8px rgba(0, 0, 0, 0.08);

    /* Input */
    --input-bg:        rgba(255, 255, 255, 0.85);
    --input-border:    rgba(0, 0, 0, 0.06);
    --input-focus:     rgba(0, 122, 255, 0.25);

    /* Text */
    --text-primary:    rgba(0, 0, 0, 0.90);
    --text-secondary:  rgba(0, 0, 0, 0.55);
    --text-tertiary:   rgba(0, 0, 0, 0.30);

    /* Items */
    --item-hover:      rgba(0, 122, 255, 0.08);
    --item-selected:   rgba(0, 122, 255, 0.14);

    /* Accent */
    --accent:          #007aff;
    --accent-hover:    #0066d6;

    /* Section headers */
    --section-text:    rgba(0, 0, 0, 0.40);
    --section-border:  rgba(0, 0, 0, 0.06);

    /* Scrollbar */
    --scrollbar:       rgba(0, 0, 0, 0.12);
  }

  @media (prefers-color-scheme: dark) {
    :root {
      --glass-bg:        rgba(28, 28, 32, 0.82);
      --glass-border:    rgba(255, 255, 255, 0.08);
      --glass-inner:     rgba(255, 255, 255, 0.04);
      --glass-shadow:    0 24px 80px rgba(0, 0, 0, 0.55), 0 2px 8px rgba(0, 0, 0, 0.30);

      --input-bg:        rgba(255, 255, 255, 0.08);
      --input-border:    rgba(255, 255, 255, 0.10);
      --input-focus:     rgba(10, 132, 255, 0.30);

      --text-primary:    rgba(255, 255, 255, 0.92);
      --text-secondary:  rgba(255, 255, 255, 0.55);
      --text-tertiary:   rgba(255, 255, 255, 0.30);

      --item-hover:      rgba(10, 132, 255, 0.10);
      --item-selected:   rgba(10, 132, 255, 0.18);

      --accent:          #0a84ff;
      --accent-hover:    #409cff;

      --section-text:    rgba(255, 255, 255, 0.42);
      --section-border:  rgba(255, 255, 255, 0.06);

      --scrollbar:       rgba(255, 255, 255, 0.10);
    }
  }

  html, body {
    width: 100%;
    height: 100%;
    background: transparent;
    font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Helvetica Neue", sans-serif;
    -webkit-font-smoothing: antialiased;
    color: var(--text-primary);
  }

  #backdrop {
    position: fixed;
    inset: 0;
    background: radial-gradient(ellipse at top, rgba(0, 0, 0, 0.12), rgba(0, 0, 0, 0.28));
    backdrop-filter: blur(12px) saturate(180%);
    -webkit-backdrop-filter: blur(12px) saturate(180%);
    display: flex;
    align-items: flex-start;
    justify-content: center;
    padding-top: 12vh;
  }

  #modal {
    width: min(680px, calc(100vw - 32px));
    background: var(--glass-bg);
    backdrop-filter: blur(48px) saturate(200%);
    -webkit-backdrop-filter: blur(48px) saturate(200%);
    border: 1px solid var(--glass-border);
    border-radius: 14px;
    box-shadow: var(--glass-shadow), inset 0 1px 0 var(--glass-inner);
    overflow: hidden;
    transform: translateY(-12px) scale(0.97);
    opacity: 0;
    animation: reveal 180ms cubic-bezier(0.16, 1, 0.3, 1) forwards;
  }

  @keyframes reveal {
    to {
      transform: translateY(0) scale(1);
      opacity: 1;
    }
  }

  #search-row {
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 12px 14px;
    border-bottom: 1px solid var(--section-border);
  }

  #search-icon {
    width: 18px;
    height: 18px;
    color: var(--text-tertiary);
    flex-shrink: 0;
  }

  #query {
    flex: 1;
    background: transparent;
    border: none;
    outline: none;
    color: var(--text-primary);
    font-size: 16px;
    font-weight: 400;
    letter-spacing: -0.01em;
    caret-color: var(--accent);
  }

  #query::placeholder {
    color: var(--text-tertiary);
  }

  #action-badge {
    padding: 5px 11px;
    border-radius: 999px;
    border: 1px solid var(--input-border);
    background: var(--input-bg);
    font-size: 11px;
    font-weight: 500;
    color: var(--text-secondary);
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
    background: var(--scrollbar);
    border-radius: 3px;
  }

  .section-header {
    padding: 8px 10px 4px;
    font-size: 11px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--section-text);
  }

  .item {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 8px 10px;
    border-radius: 8px;
    cursor: default;
    transition: background 80ms ease;
  }

  .item.selected,
  .item:hover {
    background: var(--item-selected);
  }

  .item-icon {
    width: 18px;
    height: 18px;
    color: var(--text-secondary);
    flex-shrink: 0;
  }

  .item-favicon {
    width: 18px;
    height: 18px;
    flex-shrink: 0;
    border-radius: 4px;
    object-fit: contain;
    background: var(--glass-bg);
  }

  .item-text {
    flex: 1;
    min-width: 0;
  }

  .item-title {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-primary);
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
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
    color: var(--text-tertiary);
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
    font-size: 10px;
    font-weight: 500;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--text-secondary);
    border: 1px solid var(--input-border);
    border-radius: 999px;
    padding: 2px 7px;
  }

  .close-tab {
    width: 18px;
    height: 18px;
    border: 0;
    border-radius: 50%;
    background: transparent;
    color: var(--text-tertiary);
    font-size: 14px;
    line-height: 18px;
    cursor: pointer;
    opacity: 0;
    transition: opacity 0.1s ease, background 0.1s ease, color 0.1s ease;
  }

  .item:hover .close-tab,
  .item.selected .close-tab {
    opacity: 1;
  }

  .close-tab:hover {
    background: rgba(255, 59, 48, 0.12);
    color: #ff3b30;
  }

  #hint {
    border-top: 1px solid var(--section-border);
    padding: 10px 14px;
    text-align: center;
    font-size: 11px;
    color: var(--text-tertiary);
    letter-spacing: 0.01em;
  }

  #hint kbd {
    display: inline-block;
    padding: 2px 5px;
    margin: 0 2px;
    font-family: inherit;
    font-size: 10px;
    background: var(--input-bg);
    border: 1px solid var(--input-border);
    border-radius: 4px;
  }
</style>
</head>
<body>
<div id="backdrop">
  <div id="modal">
    <div id="search-row">
      <svg id="search-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">
        <circle cx="11" cy="11" r="7"></circle><line x1="21" y1="21" x2="16.65" y2="16.65"></line>
      </svg>
      <input id="query" type="text" autocomplete="off" spellcheck="false" placeholder="Search tabs, history, or enter URL" />
      <div id="action-badge">↵ Open</div>
    </div>
    <div id="results"></div>
    <div id="hint"><kbd>↑↓</kbd> navigate · <kbd>Enter</kbd> confirm · <kbd>Esc</kbd> close · <kbd>⌘W</kbd> close tab</div>
  </div>
</div>

<script>
(function() {
  const queryEl = document.getElementById('query');
  const resultsEl = document.getElementById('results');
  const actionBadge = document.getElementById('action-badge');

  let items = [];
  let filtered = [];
  let sel = 0;

  const ICONS = {
    search: '<svg class="item-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="11" cy="11" r="7"></circle><line x1="21" y1="21" x2="16.65" y2="16.65"></line></svg>',
    globe: '<svg class="item-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10"></circle><line x1="2" y1="12" x2="22" y2="12"></line><path d="M12 2a15.3 15.3 0 0 1 4 10 15.3 15.3 0 0 1-4 10 15.3 15.3 0 0 1-4-10 15.3 15.3 0 0 1 4-10z"></path></svg>',
    tab: '<svg class="item-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="3" y="4" width="18" height="16" rx="2"></rect><line x1="3" y1="9" x2="21" y2="9"></line></svg>',
    history: '<svg class="item-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 3v5h5"></path><path d="M3.05 13A9 9 0 1 0 6 6.3L3 8"></path><line x1="12" y1="7" x2="12" y2="12"></line><line x1="12" y1="12" x2="15" y2="15"></line></svg>'
  };

  window.__setItems = function(data) {
    items = Array.isArray(data) ? data : [];
    queryEl.value = '';
    sel = 0;
    window.ipc.postMessage(JSON.stringify({ type: 'overlay_open' }));
    render('');
    queryEl.focus();
  };

  queryEl.addEventListener('input', () => render(queryEl.value));
  queryEl.addEventListener('keydown', onInputKeyDown);

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
      e.preventDefault();
      confirmSelection();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      closeOverlay();
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

    if (isMacCmd && e.key.toLowerCase() === 'v') {
      navigator.clipboard.readText().then(text => {
        if (!text) return;
        const start = queryEl.selectionStart;
        const end = queryEl.selectionEnd;
        const before = queryEl.value.slice(0, start);
        const after = queryEl.value.slice(end);
        queryEl.value = before + text + after;
        const pos = start + text.length;
        queryEl.setSelectionRange(pos, pos);
        render(queryEl.value);
      }).catch(() => {});
      return true;
    }

    if (!isCtrl) return false;

    const key = e.key.toLowerCase();
    if (key === 'a') {
      e.preventDefault();
      setCursor(0);
      return true;
    }
    if (key === 'e') {
      e.preventDefault();
      setCursor(queryEl.value.length);
      return true;
    }
    if (key === 'k') {
      e.preventDefault();
      const start = queryEl.selectionStart;
      queryEl.value = queryEl.value.slice(0, start);
      setCursor(start);
      render(queryEl.value);
      return true;
    }
    if (key === 'u') {
      e.preventDefault();
      const end = queryEl.selectionEnd;
      queryEl.value = queryEl.value.slice(end);
      setCursor(0);
      render(queryEl.value);
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

  function closeOverlay() {
    window.ipc.postMessage(JSON.stringify({ type: 'overlay_close' }));
    window.ipc.postMessage(JSON.stringify({ type: 'close' }));
  }

  function confirmSelection() {
    if (filtered.length === 0) {
      const q = queryEl.value.trim();
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

    navigate(item.url);
  }

  function navigate(url) {
    window.ipc.postMessage(JSON.stringify({ type: 'navigate', url }));
  }

  function searchUrl(q) {
    return 'https://www.google.com/search?q=' + encodeURIComponent(q);
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
    renderItems();
    updateBadge();
  }

  function render(rawQuery) {
    const raw = rawQuery.trim();
    const q = raw.toLowerCase();

    if (!raw) {
      filtered = items
        .filter(i => i.kind === 'tab')
        .slice()
        .sort((a, b) => (b.visit_count || 0) - (a.visit_count || 0))
        .slice(0, 12);
      sel = 0;
      renderItems();
      updateBadge();
      return;
    }

    const list = fuzzyFilter(q, items);
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
      title: 'Search Google',
      url: raw,
      query: raw,
      subtitle: raw,
      pill: 'Search'
    };

    const actions = urlLike ? [openAction, searchAction] : [searchAction, openAction];
    filtered = [...list, ...actions].slice(0, 14);
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
    if (item.kind === 'tab')     actionBadge.textContent = '↵ Switch';
    else if (item.kind === 'history') actionBadge.textContent = '↵ Open';
    else if (item.kind === 'url')    actionBadge.textContent = '↵ Open URL';
    else                             actionBadge.textContent = '↵ Search';
  }

  function renderItems() {
    resultsEl.innerHTML = '';

    if (filtered.length === 0) {
      resultsEl.innerHTML = '<div style="padding:24px;color:var(--text-tertiary);font-size:13px;text-align:center;">No matches found</div>';
      return;
    }

    // Group by kind
    const tabs = filtered.filter(i => i.kind === 'tab');
    const history = filtered.filter(i => i.kind === 'history');
    const actions = filtered.filter(i => i.kind !== 'tab' && i.kind !== 'history');

    let html = '';

    if (tabs.length > 0) {
      html += '<div class="section-header">Open Tabs</div>';
      tabs.forEach((item, i) => {
        const globalIdx = filtered.indexOf(item);
        html += renderItem(item, globalIdx);
      });
    }

    if (history.length > 0) {
      html += '<div class="section-header">History</div>';
      history.forEach((item, i) => {
        const globalIdx = filtered.indexOf(item);
        html += renderItem(item, globalIdx);
      });
    }

    if (actions.length > 0) {
      html += '<div class="section-header">Actions</div>';
      actions.forEach((item, i) => {
        const globalIdx = filtered.indexOf(item);
        html += renderItem(item, globalIdx);
      });
    }

    resultsEl.innerHTML = html;

    // Attach event listeners
    resultsEl.querySelectorAll('.item').forEach(row => {
      const idx = parseInt(row.dataset.idx, 10);
      row.addEventListener('mousedown', e => {
        if (e.target && e.target.classList && e.target.classList.contains('close-tab')) return;
        e.preventDefault();
        sel = idx;
        confirmSelection();
      });
    });

    resultsEl.querySelectorAll('.close-tab').forEach(btn => {
      btn.addEventListener('mousedown', e => {
        e.preventDefault();
        e.stopPropagation();
        const tabId = Number(btn.getAttribute('data-tab-id'));
        if (Number.isFinite(tabId) && tabId > 0) {
          window.ipc.postMessage(JSON.stringify({ type: 'close_tab', tab_id: tabId }));
        }
      });
    });
  }

  function renderItem(item, idx) {
    const hostname = cleanHost(item.url || '');
    const rawTitle = (item.title && item.title !== item.url) ? item.title : hostname;
    const kindLabel = esc(item.pill || kindLabelFor(item));
    const selected = idx === sel ? ' selected' : '';

    return '<div class="item' + selected + '" data-idx="' + idx + '">' +
      iconHtml(item) +
      '<div class="item-text">' +
        '<div class="item-title">' + esc(rawTitle) + '</div>' +
        '<div class="item-url">' + esc(hostname) + '</div>' +
      '</div>' +
      '<div class="item-meta">' +
        '<span class="kind-pill">' + kindLabel + '</span>' +
        (item.kind === 'tab' ? '<button class="close-tab" data-tab-id="' + item.tab_id + '" title="Close tab">×</button>' : '') +
      '</div>' +
    '</div>';
  }

  function iconHtml(item) {
    if (item.favicon && (item.kind === 'tab' || item.kind === 'history')) {
      const fallback = item.kind === 'tab' ? ICONS.tab : ICONS.history;
      return '<img class="item-favicon" src="' + esc(item.favicon) + '" onerror="this.outerHTML=\'' + fallback.replace(/"/g, "'") + '\'" />';
    }
    const html = item.kind === 'tab' ? ICONS.tab
               : item.kind === 'history' ? ICONS.history
               : item.kind === 'url' ? ICONS.globe
               : ICONS.search;
    return html;
  }

  function kindLabelFor(item) {
    if (item.kind === 'tab') return 'Tab';
    if (item.kind === 'history') return 'History';
    if (item.kind === 'url') return 'URL';
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
    return url.replace(/^https?:\/\//, '').replace(/\/$/, '') || url;
  }

  function fuzzyFilter(q, list) {
    const scored = [];

    for (const item of list) {
      const hay = (item.title || '') + ' ' + (item.url || '');
      const score = fuzzyScore(q, hay.toLowerCase());
      if (score <= 0) continue;
      const visits = item.visit_count || 0;
      const visitBoost = visits > 0 ? Math.round(Math.log2(visits + 1) * 80) : 0;
      const kindBoost = item.kind === 'tab' ? 1200 : item.kind === 'history' ? 200 : 0;
      scored.push({ item, score: score + kindBoost + visitBoost });
    }

    scored.sort((a, b) => b.score - a.score);
    return scored.map(entry => entry.item);
  }

  function fuzzyScore(q, hay) {
    let qi = 0;
    let score = 0;
    let prev = -10;

    for (let i = 0; i < hay.length && qi < q.length; i++) {
      if (hay[i] !== q[qi]) continue;

      score += 2;
      if (i === prev + 1) score += 3;
      if (i === 0 || hay[i - 1] === ' ' || hay[i - 1] === '/' || hay[i - 1] === '.') score += 2;
      prev = i;
      qi++;
    }

    return qi === q.length ? score : 0;
  }
})();
</script>
</body>
</html>"#
}
