/// Titlebar address bar — child of browser_win, lives in the native titlebar zone.
///
/// Shows: favicon | title (top) / url (bottom) | stats | 🐙 AI toggle
/// Transparent background — the native macOS titlebar provides the glass effect.
/// Traffic light buttons are native NSButtons rendered ON TOP by macOS — always clickable.
///
/// JS API called from Rust:
///   window.__update(url, secure, title, sizeBytes, timeMs)  — set URL, title and stats
///   window.__setTitle(title)                                  — update title only
///   window.__clear()                                          — reset stats for new navigation
///   window.__stats(sizeBytes, timeMs)                         — update stats only
///   window.__setFavicon(dataUri)                              — update favicon
///   window.__setBadge(show)                                   — show/hide unread badge on 🐙
///   window.__sysStats(cpuPct, memMb)                          — update CPU%/RSS (null = hide)
///
/// IPC messages sent to Rust:
///   { type: "toggle_sidebar" }       — 🐙 button clicked
///   { type: "copy_text", text: "…" } — copy title or URL to clipboard
pub fn html() -> String {
    let template = r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<style>
/*@@THEME@@*/
  *, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }

  :root {
    --lock-secure: var(--ok);
    --lock-insecure: var(--warn);
  }

  html, body {
    width: 100%; height: 100%;
    overflow: hidden;
    background: transparent;
    -webkit-font-smoothing: antialiased;
    font-family: var(--font-text);
    color: var(--label);
  }

  #bar {
    position: fixed;
    top: 0; left: 0; right: 0;
    height: 32px; /* matches address_bar_h logical px — webview may grow taller during URL edit */
    display: flex;
    align-items: center;
    /* 80px left padding clears macOS traffic light buttons (zoom right edge 69pt + 8pt gap) */
    padding: 0 8px 0 80px;
    user-select: none;
    -webkit-user-select: none;
  }

  /* ── Center info block: favicon + stacked title/url ─────────────── */
  #info {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    flex: 1;
  }

  #favicon {
    width: 14px;
    height: 14px;
    border-radius: 3px;
    flex-shrink: 0;
    object-fit: contain;
    opacity: 0.85;
    display: none; /* hidden until set */
  }

  #favicon.visible { display: block; }

  /* Title and URL share the available space equally — each gets exactly 50%.
     overflow:hidden on each half ensures neither can push the other out. */
  #text-stack {
    display: flex;
    flex-direction: row;
    align-items: center;
    min-width: 0;
    flex: 1;
    overflow: hidden;
  }

  /* Title — clickable to copy, fixed 38% of text-stack (URL gets more room) */
  #title-row {
    display: flex;
    align-items: center;
    gap: 3px;
    cursor: pointer;
    min-height: 22px;
    border-radius: var(--r-ctl);
    padding: 2px 5px;
    flex: 0 0 38%;
    min-width: 0;
    overflow: hidden;
    transition: background var(--t-fast) var(--ease), transform var(--t-fast) var(--ease);
  }
  #title-row:hover  { background: var(--fill-hover); }
  #title-row:active { background: var(--fill-press); }
  #title-row.copied { background: var(--fill); }

  #page-title {
    font-size: 11px;
    font-weight: 500;
    color: var(--label);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    letter-spacing: -0.1px;
  }

  /* Separator between title and url */
  #sep {
    font-size: 10px;
    color: var(--label-3);
    flex-shrink: 0;
    padding: 0 1px;
    opacity: 0.5;
  }

  /* URL — text click copies; pencil click enters edit mode. Fixed 62% width. */
  #url-row {
    position: relative;
    display: flex;
    align-items: center;
    gap: 3px;
    min-height: 22px;
    border-radius: var(--r-ctl);
    padding: 2px 5px;
    flex: 0 0 calc(62% - 14px); /* subtract sep width so total stays 100% */
    min-width: 0;
    overflow: visible; /* let the suggestion dropdown escape this row */
    transition: background var(--t-fast) var(--ease);
  }
  #url-row.copied { background: var(--fill); }

  /* The copyable text portion is its own hover target so the pencil doesn't
     paint the whole row on hover. */
  #url-copy {
    display: flex;
    align-items: center;
    gap: 3px;
    flex: 1;
    min-width: 0;
    overflow: hidden;
    cursor: pointer;
    min-height: 22px;
    border-radius: var(--r-ctl);
    padding: 0 2px;
    transition: background var(--t-fast) var(--ease);
  }
  #url-copy:hover { background: var(--fill-hover); }
  #url-copy:active { background: var(--fill-press); }

  /* Pencil edit button — small, dim at rest, brightens on hover. */
  #url-edit-btn {
    flex-shrink: 0;
    width: 22px;
    height: 22px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: none;
    background: transparent;
    border-radius: var(--r-ctl);
    cursor: pointer;
    color: var(--label-3);
    opacity: 0.55;
    line-height: 0;
    transition: opacity var(--t-fast) var(--ease), background var(--t-fast) var(--ease),
                color var(--t-fast) var(--ease), transform var(--t-fast) var(--ease);
    padding: 0;
  }
  #url-edit-btn:hover  { opacity: 1; background: var(--fill-hover); color: var(--label); }
  #url-edit-btn:active { background: var(--fill-press); transform: scale(0.92); }
  #url-edit-btn svg { width: 10px; height: 10px; }

  /* URL input — shown only while editing; replaces the copyable URL display. */
  #url-input {
    flex: 1;
    min-width: 0;
    height: 22px;
    padding: 0 9px;
    font: inherit;
    font-size: 10.5px;
    color: var(--label);
    background: var(--fill);
    border: none;
    border-radius: var(--r-capsule);
    box-shadow: 0 0 0 0.5px var(--hairline), 0 0 0 2.5px color-mix(in srgb, var(--accent) 28%, transparent);
    outline: none;
    display: none;
    letter-spacing: -0.1px;
    caret-color: var(--accent);
    transition: background var(--t-fast) var(--ease), box-shadow var(--t-fast) var(--ease);
  }
  #url-input:hover { background: var(--fill-hover); }
  #url-input:focus { box-shadow: 0 0 0 1px var(--accent), 0 0 0 3px color-mix(in srgb, var(--accent) 24%, transparent); }
  #url-row.editing #url-input { display: block; }
  #url-row.editing #url-copy  { display: none; }
  #url-row.editing #lock      { display: none; }

  /* Suggestion dropdown — paints into the expanded address-bar webview.
     Positioned absolute against the document body (the titlebar #bar element
     is the 32px-tall strip at the top; the dropdown floats just below it). */
  #url-suggest {
    position: absolute;
    z-index: 200;
    background: var(--glass);
    -webkit-backdrop-filter: var(--glass-blur);
    backdrop-filter: var(--glass-blur);
    border-radius: var(--r-panel);
    box-shadow: var(--shadow-float), var(--glass-shine);
    overflow: hidden;
    display: none;
    max-height: 320px;
    overflow-y: auto;
    padding: 5px;
  }
  #url-suggest.show {
    display: block;
    animation: suggest-pop var(--t-pop) var(--spring);
  }
  @keyframes suggest-pop {
    from { opacity: 0; transform: translateY(-4px) scale(0.98); }
    to   { opacity: 1; transform: translateY(0) scale(1); }
  }

  .sg-item {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 6px 9px;
    min-height: 32px;
    border-radius: var(--r-ctl);
    cursor: pointer;
    min-width: 0;
    transition: background var(--t-fast) var(--ease), transform var(--t-fast) var(--ease);
  }
  .sg-item:hover  { background: var(--fill-hover); }
  .sg-item:active { background: var(--fill-press); transform: scale(0.99); }
  .sg-item.active { background: color-mix(in srgb, var(--accent) 15%, transparent); }
  .sg-item.active:active { background: color-mix(in srgb, var(--accent) 22%, transparent); }
  .sg-fav {
    flex-shrink: 0;
    width: 13px;
    height: 13px;
    border-radius: 3px;
    object-fit: contain;
    background: var(--fill);
  }
  .sg-fav-fallback {
    flex-shrink: 0;
    width: 13px;
    height: 13px;
    border-radius: 3px;
    background: var(--fill);
  }
  .sg-text { flex: 1; min-width: 0; overflow: hidden; }
  .sg-title {
    font-size: 11px;
    color: var(--label);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    letter-spacing: -0.1px;
  }
  .sg-url {
    font-size: 10px;
    color: var(--label-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    margin-top: 1px;
    letter-spacing: -0.1px;
  }
  .sg-kind {
    flex-shrink: 0;
    font-size: 9.5px;
    font-weight: 500;
    color: var(--label-3);
    padding: 1px 6px;
    border-radius: var(--r-capsule);
    background: var(--fill);
  }
  .sg-hint {
    display: flex;
    align-items: center;
    justify-content: flex-end;
    gap: 5px;
    min-height: 24px;
    padding: 4px 7px 1px;
    color: var(--label-3);
    font-size: 9.5px;
    box-shadow: 0 -0.5px 0 var(--hairline);
  }

  #lock {
    flex-shrink: 0;
    width: 10px;
    height: 10px;
    display: none;
    align-items: center;
    justify-content: center;
    line-height: 0;
  }
  #lock.visible { display: inline-flex; }
  #lock svg { width: 100%; height: 100%; }
  #lock.secure   { color: var(--lock-secure); }
  #lock.insecure { color: var(--lock-insecure); }

  /* ── Rich tooltip for icon buttons (label + shortcut) ─────────────── */
  .bar-btn[data-tip] { position: relative; }
  .bar-btn[data-tip]::after {
    content: attr(data-tip);
    position: absolute;
    top: calc(100% + 6px);
    left: 50%;
    transform: translate(-50%, -2px);
    background: var(--glass-thick);
    color: var(--label);
    font-size: 10.5px;
    font-weight: 500;
    letter-spacing: 0.01em;
    white-space: nowrap;
    padding: 4px 7px;
    border-radius: var(--r-ctl);
    box-shadow: var(--shadow-float), var(--glass-shine);
    -webkit-backdrop-filter: var(--glass-blur);
    backdrop-filter: var(--glass-blur);
    opacity: 0;
    pointer-events: none;
    transition: opacity var(--t-fast) var(--ease) 0.25s, transform var(--t-fast) var(--ease) 0.25s;
    z-index: 100;
  }
  .bar-btn[data-tip]:hover::after {
    opacity: 1;
    transform: translate(-50%, 0);
  }
  #url {
    font-size: 10px;
    color: var(--label-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    letter-spacing: -0.1px;
  }
  #url .host { color: var(--label-2); }

  /* Shared "Copied" flash label */
  .copied-label {
    font-size: 9px;
    color: var(--label-3);
    opacity: 0;
    transition: opacity var(--t-fast) var(--ease);
    flex-shrink: 0;
  }
  .copied-label.show { opacity: 1; }

  /* ── Stats section ───────────────────────────────────────────────── */
  #stats {
    display: flex;
    align-items: center;
    gap: 4px;
    font-size: 10px;
    color: var(--label-3);
    flex-shrink: 0;
    margin-left: 12px;
    margin-right: 8px;
    letter-spacing: -0.1px;
    font-variant-numeric: tabular-nums;
  }
  .stat { white-space: nowrap; }
  .stat-val {
    display: inline-block;
    width: 4.5ch;
    text-align: right;
    overflow: hidden;
  }
  /* ── System stats: CPU% and RSS memory of the active tab's WebContent process ── */
  #sys-stats {
    display: flex;
    align-items: center;
    gap: 5px;
    font-size: 10px;
    color: var(--label-3);
    flex-shrink: 0;
    margin-left: 4px;
    margin-right: 4px;
    letter-spacing: -0.1px;
    font-variant-numeric: tabular-nums;
    visibility: hidden;
    opacity: 0;
    transition: opacity var(--t-pop) var(--ease);
  }
  #sys-stats.visible {
    visibility: visible;
    opacity: 1;
  }
  .sys-chip {
    display: flex;
    align-items: center;
    gap: 2px;
    white-space: nowrap;
  }
  .sys-icon {
    width: 10px;
    height: 10px;
    line-height: 0;
    opacity: 0.7;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
  }
  .sys-icon svg { width: 100%; height: 100%; }

  /* ── 🐙 AI toggle button + utility buttons (spotlight, close tab) ── */
  .bar-btn {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    border-radius: var(--r-capsule);
    border: none;
    background: transparent;
    cursor: pointer;
    flex-shrink: 0;
    color: var(--label-3);
    transition: background var(--t-fast) var(--ease), color var(--t-fast) var(--ease),
                transform var(--t-fast) var(--spring);
  }
  .bar-btn:hover  { background: var(--fill-hover); color: var(--label); }
  .bar-btn:active { background: var(--fill-press); transform: scale(0.90); }

  #workspace-btn { position: relative; }
  .ws-icon { display: inline-flex; width: 13px; height: 13px; line-height: 0; }
  .ws-icon svg { width: 100%; height: 100%; }
  .ws-dot {
    position: absolute;
    bottom: 3px; right: 3px;
    width: 6px; height: 6px;
    border-radius: 50%;
    box-shadow: 0 0 0 1.5px var(--glass-thick);
  }

  #ai-btn {
    background: transparent;
    color: var(--label-3);
    width: 28px;
    height: 28px;
    border-radius: 50%;
    border: none;
  }
  #ai-btn:hover  { background: var(--fill-hover); color: var(--label); transform: scale(1.08); }
  #ai-btn:active { background: var(--fill-press); transform: scale(0.92); }
  .ai-icon { display: inline-flex; width: 16px; height: 16px; line-height: 0; }
  .ai-icon svg { width: 100%; height: 100%; }

  /* ── Unread badge dot ────────────────────────────────────────────── */
  .badge {
    position: absolute;
    top: 1px; right: 1px;
    width: 8px; height: 8px;
    border-radius: 50%;
    background: var(--err);
    border: 1.5px solid var(--glass-thick);
    box-shadow: 0 1px 4px color-mix(in srgb, var(--err) 45%, transparent);
    opacity: 0;
    transform: scale(0);
    transition: opacity var(--t-fast) var(--ease), transform var(--t-pop) var(--spring);
    pointer-events: none;
  }
  .badge.show { opacity: 1; transform: scale(1); }
</style>
</head>
<body>
<div id="bar">
  <div id="info">
    <img id="favicon" alt="">
    <div id="text-stack">
      <div id="title-row">
        <span id="page-title"></span>
        <span class="copied-label" id="title-copied">Copied</span>
      </div>
      <span id="sep">·</span>
      <div id="url-row">
        <div id="url-copy">
          <span id="lock"></span>
          <span id="url"></span>
          <span class="copied-label" id="url-copied">Copied</span>
        </div>
        <input id="url-input" type="text" spellcheck="false" autocomplete="off" autocapitalize="off">
        <button id="url-edit-btn" type="button" title="Edit URL">@@ICON_PENCIL@@</button>
      </div>
    </div>
  </div>
  <div id="stats">
    <span class="stat sys-chip" id="size-chip" title="Page transfer size"><span class="sys-icon">@@ICON_DOWNLOAD@@</span><span class="stat-val" id="size">&mdash;</span></span>
    <span class="stat sys-chip" id="time-chip" title="Page load time"><span class="sys-icon">@@ICON_CLOCK@@</span><span class="stat-val" id="time">&mdash;</span></span>
  </div>
  <div id="sys-stats">
    <span class="sys-chip" title="CPU usage of this tab's web process"><span class="sys-icon">@@ICON_ACTIVITY@@</span><span class="stat-val" id="cpu-stat"></span></span>
    <span class="sys-chip" title="Memory used by this tab's web process"><span class="sys-icon">@@ICON_CPU@@</span><span class="stat-val" id="mem-stat"></span></span>
  </div>
  <button id="workspace-btn" class="bar-btn" data-tip="Workspaces  ⌘⇧O" title="Workspaces (⌘⇧O)">
    <span class="ws-icon">@@ICON_LAYERS@@</span>
    <span class="ws-dot" id="ws-dot"></span>
  </button>
  <button id="settings-btn" class="bar-btn" data-tip="Settings  ⌘," title="Settings (⌘,)">
    <svg width="12" height="12" viewBox="0 0 16 16" fill="none">
      <path d="M6.6 1.2h2.8l.4 1.9.5.2 1.7-.9 2 2-.9 1.7.2.5 1.9.4v2.8l-1.9.4-.2.5.9 1.7-2 2-1.7-.9-.5.2-.4 1.9H6.6l-.4-1.9-.5-.2-1.7.9-2-2 .9-1.7-.2-.5L.8 9.2V6.4l1.9-.4.2-.5-.9-1.7 2-2 1.7.9.5-.2.4-1.3z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" fill="none"/>
      <circle cx="8" cy="7.8" r="2" stroke="currentColor" stroke-width="1.2" fill="none"/>
    </svg>
  </button>
  <button id="shortcuts-btn" class="bar-btn" data-tip="Shortcuts  ⌘/" title="Keyboard shortcuts (⌘/)">
    <svg width="12" height="12" viewBox="0 0 16 16" fill="none">
      <path d="M6.2 5.6c0-.9.7-1.6 1.8-1.6 1 0 1.8.7 1.8 1.6 0 .7-.4 1.1-1.1 1.6-.6.4-.9.7-.9 1.3v.3" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
      <circle cx="8" cy="11.5" r="0.9" fill="currentColor"/>
    </svg>
  </button>
  <button id="spotlight-btn" class="bar-btn" data-tip="Search  ⌘⇧P" title="Command palette (⌘⇧P)">
    <svg width="13" height="13" viewBox="0 0 16 16" fill="none">
      <circle cx="6.5" cy="6.5" r="4.5" stroke="currentColor" stroke-width="1.6"/>
      <line x1="10" y1="10" x2="14" y2="14" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
    </svg>
  </button>
  <button id="close-tab-btn" class="bar-btn" data-tip="Close tab  ⌘W" title="Close tab (⌘W)">
    <svg width="11" height="11" viewBox="0 0 12 12" fill="none">
      <line x1="1.5" y1="1.5" x2="10.5" y2="10.5" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
      <line x1="10.5" y1="1.5" x2="1.5" y2="10.5" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
    </svg>
  </button>
  <button id="ai-btn" class="bar-btn" data-tip="AI sidebar  ⌘⇧A" title="Toggle AI sidebar (⌘⇧A)">
    <span class="ai-icon">@@ICON_SPARKLES@@</span>
    <span id="badge" class="badge"></span>
  </button>
</div>
<div id="url-suggest" class="glass-panel"></div>
<script>
(function() {
  const titleEl      = document.getElementById('page-title');
  const urlEl        = document.getElementById('url');
  const lockEl       = document.getElementById('lock');
  const faviconEl    = document.getElementById('favicon');
  const sizeEl       = document.getElementById('size');
  const timeEl       = document.getElementById('time');
  const sizeChipEl   = document.getElementById('size-chip');
  const timeChipEl   = document.getElementById('time-chip');
  const sysStatsEl   = document.getElementById('sys-stats');
  const cpuStatEl    = document.getElementById('cpu-stat');
  const memStatEl    = document.getElementById('mem-stat');
  const titleRow     = document.getElementById('title-row');
  const urlRow       = document.getElementById('url-row');
  const urlCopy      = document.getElementById('url-copy');
  const urlInput     = document.getElementById('url-input');
  const urlEditBtn   = document.getElementById('url-edit-btn');
  const urlSuggest   = document.getElementById('url-suggest');
  const titleCopied  = document.getElementById('title-copied');
  const urlCopied    = document.getElementById('url-copied');

  let currentUrl   = '';
  let currentTitle = '';

  // Copy via IPC — navigator.clipboard unavailable in child WKWebViews
  function copyViaIpc(text) {
    window.ipc.postMessage(JSON.stringify({ type: 'copy_text', text }));
  }

  function flashCopied(row, label) {
    row.classList.add('copied');
    label.classList.add('show');
    setTimeout(function() {
      row.classList.remove('copied');
      label.classList.remove('show');
    }, 1000);
  }

  function formatSize(bytes) {
    if (bytes <= 0) return '\u2014';
    if (bytes < 1024) return bytes + 'B';
    var kb = bytes / 1024;
    if (kb < 10) return kb.toFixed(1) + 'K';
    if (kb < 1000) return Math.round(kb) + 'K';
    var mb = bytes / (1024 * 1024);
    if (mb < 10) return mb.toFixed(1) + 'M';
    if (mb < 1000) return Math.round(mb) + 'M';
    var gb = bytes / (1024 * 1024 * 1024);
    return gb.toFixed(1) + 'G';
  }

  function formatTime(ms) {
    if (ms <= 0) return '\u2014';
    if (ms < 1000) return Math.round(ms) + 'ms';
    var s = ms / 1000;
    if (s < 10) return s.toFixed(1) + 's';
    return Math.round(s) + 's';
  }

  function detailSize(bytes) {
    if (bytes <= 0) return 'Page transfer size';
    return bytes.toLocaleString() + ' bytes';
  }

  function detailTime(ms) {
    if (ms <= 0) return 'Page load time';
    return ms.toLocaleString() + ' ms';
  }

  function renderUrl(url) {
    try {
      const u = new URL(url);
      const host = u.host;
      const rest = url.slice(url.indexOf(host) + host.length);
      const scheme = u.protocol + '//';
      return '<span style="opacity:0.5">' + scheme + '</span><span class="host">' + host + '</span>' + rest;
    } catch(e) {
      return url;
    }
  }

  var ICON_LOCK   = '@@ICON_LOCK@@';
  var ICON_SHIELD = '@@ICON_SHIELD_ALERT@@';

  function updateLock(url, secure) {
    if (url && url !== 'about:blank') {
      lockEl.innerHTML = secure ? ICON_LOCK : ICON_SHIELD;
      lockEl.className = (secure ? 'secure' : 'insecure') + ' visible';
    } else {
      lockEl.innerHTML = '';
      lockEl.className = '';
    }
  }

  function updateStats(sizeBytes, timeMs) {
    sizeEl.textContent = formatSize(sizeBytes);
    timeEl.textContent = formatTime(timeMs);
    sizeChipEl.title = detailSize(sizeBytes);
    timeChipEl.title = detailTime(timeMs);
  }

  window.__update = function(url, secure, title, sizeBytes, timeMs) {
    currentUrl   = url;
    currentTitle = title || '';
    titleEl.textContent = currentTitle;
    urlEl.innerHTML = url ? renderUrl(url) : '';
    updateLock(url, secure);
    updateStats(sizeBytes, timeMs);
  };

  window.__setTitle = function(title) {
    currentTitle = title || '';
    titleEl.textContent = currentTitle;
  };

  window.__setFavicon = function(dataUri) {
    if (dataUri) {
      faviconEl.src = dataUri;
      faviconEl.classList.add('visible');
    } else {
      faviconEl.classList.remove('visible');
      faviconEl.src = '';
    }
  };

  window.__clear = function() {
    sizeEl.textContent = '\u2014';
    timeEl.textContent = '\u2014';
    sizeChipEl.title = 'Page transfer size';
    timeChipEl.title = 'Page load time';
  };

  window.__sysStats = function(cpuPct, memMb) {
    if (cpuPct === null || memMb === null) {
      sysStatsEl.classList.remove('visible');
      return;
    }
    cpuStatEl.textContent = cpuPct + '%';
    memStatEl.textContent = memMb + 'M';
    sysStatsEl.classList.add('visible');
  };

  // Poll system stats via custom protocol (avoids evaluate_script leak — wry#1489)
  // Each evaluate_script call leaks a WKWebView JS evaluation context.
  // Using fetch('octoweb-sys://stats') avoids this leak entirely.
  (function pollSysStats() {
    fetch('octoweb-sys://stats')
      .then(function(r) { return r.json(); })
      .then(function(data) {
        window.__sysStats(data.cpu_pct, data.mem_mb);
      })
      .catch(function() {
        // Silently ignore — tab may have switched or WebView not ready
      })
      .finally(function() {
        setTimeout(pollSysStats, 2000);
      });
  })();

  window.__stats = function(sizeBytes, timeMs) {
    updateStats(sizeBytes, timeMs);
  };

  // Copy title on click
  titleRow.addEventListener('click', function() {
    if (!currentTitle) return;
    copyViaIpc(currentTitle);
    flashCopied(titleRow, titleCopied);
  });

  // Copy URL when the text portion is clicked (not the pencil).
  urlCopy.addEventListener('click', function() {
    if (!currentUrl) return;
    copyViaIpc(currentUrl);
    flashCopied(urlRow, urlCopied);
  });

  // ── URL edit mode ───────────────────────────────────────────────────────
  // Pencil click → swap URL display for an input + suggestions dropdown.
  // Submit on Enter; cancel on Esc or click outside.

  var suggestions = [];       // history+tabs snapshot pushed from Rust
  var filtered    = [];       // current filtered list
  var activeIdx   = 0;
  var editing     = false;
  var focusPoll   = null;     // interval that exits edit mode when the
                              // address bar webview loses keyboard focus
                              // (clicks into sibling webviews don't fire
                              // window.blur reliably under WKWebView).

  function escHtml(s) {
    return String(s == null ? '' : s)
      .replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')
      .replace(/"/g, '&quot;').replace(/'/g, '&#39;');
  }

  // Match the query against title and URL substrings (case-insensitive).
  // Score: prefix match on URL > prefix match on host/title > substring match.
  function rankItem(item, q) {
    if (!q) return 1;
    var url = (item.url || '').toLowerCase();
    var title = (item.title || '').toLowerCase();
    var host = url.replace(/^https?:\/\//, '').split('/')[0];
    var ql = q.toLowerCase();
    if (url.startsWith(ql) || ('https://' + ql) === url) return 100;
    if (host.startsWith(ql)) return 80;
    if (title.startsWith(ql)) return 60;
    if (url.indexOf(ql) >= 0) return 40;
    if (title.indexOf(ql) >= 0) return 30;
    return 0;
  }

  function filterSuggestions(q) {
    if (!suggestions.length) return [];
    var seen = Object.create(null);
    var out = [];
    for (var i = 0; i < suggestions.length; i++) {
      var it = suggestions[i];
      if (!it.url || seen[it.url]) continue;
      var score = rankItem(it, q);
      if (score > 0) {
        seen[it.url] = 1;
        out.push({ item: it, score: score, order: i });
      }
    }
    out.sort(function(a, b) { return b.score - a.score || a.order - b.order; });
    // Copy with score attached — the Enter handler needs it to decide whether
    // the active suggestion is a strong enough match to auto-open.
    return out.slice(0, 8).map(function(x) {
      return Object.assign({}, x.item, { score: x.score });
    });
  }

  function renderSuggestions() {
    if (!filtered.length) {
      urlSuggest.classList.remove('show');
      urlSuggest.innerHTML = '';
      return;
    }
    if (activeIdx >= filtered.length) activeIdx = 0;
    var html = '';
    for (var i = 0; i < filtered.length; i++) {
      var it = filtered[i];
      var fav = it.favicon
        ? '<img class="sg-fav" src="' + escHtml(it.favicon) + '" onerror="this.replaceWith(Object.assign(document.createElement(\'span\'),{className:\'sg-fav-fallback\'}))" />'
        : '<span class="sg-fav-fallback"></span>';
      var kind = it.kind === 'tab' ? 'Tab' : 'History';
      var titleText = it.title && it.title.length ? it.title : it.url;
      html += '<div class="sg-item' + (i === activeIdx ? ' active' : '') + '" data-idx="' + i + '">'
            + fav
            + '<div class="sg-text">'
            + '<div class="sg-title">' + escHtml(titleText) + '</div>'
            + '<div class="sg-url">' + escHtml(it.url) + '</div>'
            + '</div>'
            + '<span class="sg-kind">' + kind + '</span>'
            + '</div>';
    }
    urlSuggest.innerHTML = html + '<div class="sg-hint"><span class="kbd">esc</span> dismiss</div>';
    positionSuggest();
    urlSuggest.classList.add('show');
  }

  function positionSuggest() {
    var r = urlRow.getBoundingClientRect();
    urlSuggest.style.left = Math.round(r.left) + 'px';
    urlSuggest.style.top = Math.round(r.bottom + 4) + 'px';
    urlSuggest.style.width = Math.round(r.width) + 'px';
  }

  function enterEdit() {
    if (editing) return;
    editing = true;
    urlRow.classList.add('editing');
    urlInput.value = currentUrl || '';
    activeIdx = 0;
    // Grow the address bar webview so the dropdown isn't clipped by the 32 px
    // titlebar, then ask Rust for a fresh history snapshot for autocomplete.
    window.ipc.postMessage(JSON.stringify({ type: 'url_edit_expand', expanded: true }));
    window.ipc.postMessage(JSON.stringify({ type: 'url_edit_open' }));
    // Focus + initial render on the next animation frame so the resize has
    // been applied by macOS before the dropdown paints. This is the fix for
    // "autocomplete stops working after the first use" — without it the second
    // entry would render the dropdown into a still-32 px-tall webview.
    requestAnimationFrame(function() {
      urlInput.focus();
      urlInput.select();
      filtered = filterSuggestions(urlInput.value);
      renderSuggestions();
    });
    // Poll keyboard focus so that clicking into a sibling webview (browser
    // content, sidebar, etc.) cancels the edit. WKWebView doesn't fire
    // `window.blur` reliably for cross-webview focus changes.
    //
    // We require TWO consecutive "lost focus" readings (≈300 ms) before exit:
    // single readings can be false negatives during focus transitions (e.g.
    // right after enterEdit before rAF moves focus to the input, or while the
    // user clicks the pencil / a dropdown row). Without the debounce, the
    // dropdown would flash away mid-edit.
    if (focusPoll) clearInterval(focusPoll);
    var lostFocusTicks = 0;
    focusPoll = setInterval(function() {
      if (!editing) return;
      var ok = document.hasFocus() && document.activeElement === urlInput;
      if (ok) {
        lostFocusTicks = 0;
      } else {
        lostFocusTicks++;
        if (lostFocusTicks >= 2) exitEdit();
      }
    }, 150);
  }

  function exitEdit() {
    if (!editing) return;
    editing = false;
    urlRow.classList.remove('editing');
    urlSuggest.classList.remove('show');
    urlSuggest.innerHTML = '';
    urlInput.value = '';
    urlInput.blur();
    if (focusPoll) { clearInterval(focusPoll); focusPoll = null; }
    // Tell Rust to shrink the webview back to its 32px titlebar height.
    window.ipc.postMessage(JSON.stringify({ type: 'url_edit_expand', expanded: false }));
  }

  // Exposed for ⌘E hotkey from Rust (CGEventTap → AppEvent::UrlEditRequest).
  window.__urlEditOpen = function() {
    if (editing) { exitEdit(); } else { enterEdit(); }
  };

  function submit(rawUrl) {
    var u = (rawUrl == null ? urlInput.value : rawUrl).trim();
    if (!u) return;
    exitEdit();
    window.ipc.postMessage(JSON.stringify({ type: 'navigate', url: u }));
  }

  urlEditBtn.addEventListener('click', function(e) {
    e.stopPropagation();
    enterEdit();
  });

  urlInput.addEventListener('input', function() {
    activeIdx = 0;
    filtered = filterSuggestions(urlInput.value);
    renderSuggestions();
  });

  urlInput.addEventListener('keydown', function(e) {
    if (e.key === 'Enter') {
      e.preventDefault();
      if (filtered.length && activeIdx >= 0 && activeIdx < filtered.length
          && filtered[activeIdx].url
          && urlInput.value.trim() !== ''
          && filtered[activeIdx].score >= 60) {
        submit(filtered[activeIdx].url);
      } else {
        submit();
      }
    } else if (e.key === 'Escape') {
      e.preventDefault();
      exitEdit();
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      if (filtered.length) {
        activeIdx = (activeIdx + 1) % filtered.length;
        renderSuggestions();
      }
    } else if (e.key === 'ArrowUp') {
      e.preventDefault();
      if (filtered.length) {
        activeIdx = (activeIdx - 1 + filtered.length) % filtered.length;
        renderSuggestions();
      }
    } else if (e.key === 'Tab' && filtered.length) {
      e.preventDefault();
      urlInput.value = filtered[activeIdx].url;
      activeIdx = 0;
      filtered = filterSuggestions(urlInput.value);
      renderSuggestions();
    }
  });

  urlSuggest.addEventListener('mousedown', function(e) {
    // mousedown not click — fires before the input blur kicks in.
    var item = e.target.closest('.sg-item');
    if (!item) return;
    e.preventDefault();
    var idx = parseInt(item.getAttribute('data-idx'), 10);
    if (!isNaN(idx) && filtered[idx]) submit(filtered[idx].url);
  });

  urlSuggest.addEventListener('mousemove', function(e) {
    var item = e.target.closest('.sg-item');
    if (!item) return;
    var idx = parseInt(item.getAttribute('data-idx'), 10);
    if (!isNaN(idx) && idx !== activeIdx) {
      activeIdx = idx;
      var nodes = urlSuggest.querySelectorAll('.sg-item');
      for (var i = 0; i < nodes.length; i++) {
        nodes[i].classList.toggle('active', i === activeIdx);
      }
    }
  });

  // Click outside the URL row / dropdown inside the address bar exits edit.
  document.addEventListener('mousedown', function(e) {
    if (!editing) return;
    if (urlRow.contains(e.target) || urlSuggest.contains(e.target)) return;
    exitEdit();
  });

  // If focus leaves the input (e.g. user clicks the page below the bar), exit
  // after a short delay so the suggestion mousedown can still register first.
  // The activeElement check is intentionally omitted: when the entire webview
  // loses focus, document.activeElement can stay on the input even though the
  // user has clicked elsewhere — checking it would skip the cancel.
  urlInput.addEventListener('blur', function() {
    setTimeout(function() { if (editing) exitEdit(); }, 80);
  });

  // Window-level blur — fires when the user clicks into another webview
  // (e.g. the browser content area). DOM blur on the input may not fire in
  // that case because focus moves out of this webview entirely.
  window.addEventListener('blur', function() {
    if (editing) exitEdit();
  });

  // Reposition dropdown when window resizes (titlebar width changes).
  window.addEventListener('resize', function() { if (editing) positionSuggest(); });

  // Receive history+tabs snapshot from Rust for autocomplete.
  window.__setSuggestions = function(items) {
    suggestions = Array.isArray(items) ? items.filter(function(it) {
      return it && it.url && it.kind !== 'ask' && it.kind !== 'url' && it.url.indexOf('about:') !== 0;
    }) : [];
    if (editing) {
      filtered = filterSuggestions(urlInput.value);
      renderSuggestions();
    }
  };

  // 🐙 AI toggle button
  document.getElementById('ai-btn').addEventListener('click', function() {
    window.ipc.postMessage(JSON.stringify({ type: 'toggle_sidebar' }));
  });

  // Spotlight / command palette
  document.getElementById('spotlight-btn').addEventListener('click', function() {
    window.ipc.postMessage(JSON.stringify({ type: 'toggle_overlay' }));
  });

  // Workspaces
  document.getElementById('workspace-btn').addEventListener('click', function() {
    window.ipc.postMessage(JSON.stringify({ type: 'toggle_workspaces' }));
  });

  // Active workspace color dot — pushed by Rust on switch/create/rename/delete
  // and once at startup.
  window.__setWorkspace = function(color, name) {
    var dot = document.getElementById('ws-dot');
    if (dot) dot.style.background = color;
    var btn = document.getElementById('workspace-btn');
    if (btn) btn.title = 'Workspaces (⌘⇧O) — ' + name;
  };

  // Settings
  document.getElementById('settings-btn').addEventListener('click', function() {
    window.ipc.postMessage(JSON.stringify({ type: 'toggle_settings' }));
  });

  // Keyboard shortcuts
  document.getElementById('shortcuts-btn').addEventListener('click', function() {
    window.ipc.postMessage(JSON.stringify({ type: 'toggle_shortcuts' }));
  });

  // Close current tab
  document.getElementById('close-tab-btn').addEventListener('click', function() {
    window.ipc.postMessage(JSON.stringify({ type: 'close_tab' }));
  });

  // Badge API
  window.__setBadge = function(show) {
    document.getElementById('badge').classList.toggle('show', !!show);
  };

  // Initial state — badge hidden
  document.getElementById('badge').classList.remove('show');

  // Window drag from title bar background — delegates to native macOS
  // performWindowDragWithEvent: which has a built-in drag threshold so
  // accidental clicks (e.g. when reaching past the bar to scroll content)
  // don't move the window on the slightest mouse jitter.
  document.getElementById('bar').addEventListener('mousedown', function(e) {
    if (e.button !== 0) return;
    if (e.target.closest('#ai-btn, .bar-btn, #page-title, #url, #url-edit-btn, #url-input, #url-suggest')) return;
    if (editing) return;
    window.ipc.postMessage(JSON.stringify({ type: 'begin_window_drag' }));
  });
})();
</script>
</body>
</html>"#;
    template
        .replace("/*@@THEME@@*/", crate::theme::CSS)
        .replace("@@ICON_DOWNLOAD@@", crate::icons::DOWNLOAD)
        .replace("@@ICON_CLOCK@@", crate::icons::CLOCK)
        .replace("@@ICON_ACTIVITY@@", crate::icons::ACTIVITY)
        .replace("@@ICON_CPU@@", crate::icons::CPU)
        .replace("@@ICON_SPARKLES@@", crate::icons::SPARKLES)
        .replace("@@ICON_LOCK@@", crate::icons::LOCK)
        .replace("@@ICON_SHIELD_ALERT@@", crate::icons::SHIELD_ALERT)
        .replace("@@ICON_PENCIL@@", crate::icons::PENCIL)
        .replace("@@ICON_LAYERS@@", crate::icons::LAYERS)
}
