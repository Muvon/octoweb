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
///
/// IPC messages sent to Rust:
///   { type: "toggle_sidebar" }       — 🐙 button clicked
///   { type: "copy_text", text: "…" } — copy title or URL to clipboard
pub fn html() -> &'static str {
    r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<style>
  *, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }

  html, body {
    width: 100%; height: 100%;
    overflow: hidden;
    background: transparent;
    -webkit-font-smoothing: antialiased;
    font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Helvetica Neue", sans-serif;
  }

  :root {
    --text-title:  rgba(0, 0, 0, 0.82);
    --text-url:    rgba(0, 0, 0, 0.42);
    --text-host:   rgba(0, 0, 0, 0.55);
    --text-dim:    rgba(0, 0, 0, 0.28);
    --hover-bg:    rgba(0, 0, 0, 0.05);
    --copied-bg:   rgba(0, 0, 0, 0.06);
    --copied-text: rgba(0, 0, 0, 0.38);
    --lock-secure:   rgba(0, 180, 80, 0.70);
    --lock-insecure: rgba(255, 149, 0, 0.70);
    --btn-bg:    rgba(0, 0, 0, 0.04);
    --btn-hover: rgba(0, 0, 0, 0.08);
  }

  @media (prefers-color-scheme: dark) {
    :root {
      --text-title:  rgba(255, 255, 255, 0.82);
      --text-url:    rgba(255, 255, 255, 0.38);
      --text-host:   rgba(255, 255, 255, 0.52);
      --text-dim:    rgba(255, 255, 255, 0.22);
      --hover-bg:    rgba(255, 255, 255, 0.07);
      --copied-bg:   rgba(255, 255, 255, 0.10);
      --copied-text: rgba(255, 255, 255, 0.32);
      --lock-secure:   rgba(48, 209, 88, 0.75);
      --lock-insecure: rgba(255, 159, 10, 0.75);
      --btn-bg:    rgba(255, 255, 255, 0.06);
      --btn-hover: rgba(255, 255, 255, 0.12);
    }
  }

  #bar {
    position: fixed;
    top: 0; left: 0; right: 0;
    height: 100%;
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

  #text-stack {
    display: flex;
    flex-direction: row;
    align-items: center;
    min-width: 0;
    gap: 0;
  }

  /* Title — clickable to copy */
  #title-row {
    display: flex;
    align-items: center;
    gap: 3px;
    cursor: pointer;
    border-radius: 3px;
    padding: 2px 5px;
    flex-shrink: 1;
    min-width: 0;
    transition: background 0.12s ease;
  }
  #title-row:hover  { background: var(--hover-bg); }
  #title-row.copied { background: var(--copied-bg); }

  #page-title {
    font-size: 11px;
    font-weight: 500;
    color: var(--text-title);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    letter-spacing: -0.1px;
  }

  /* Separator between title and url */
  #sep {
    font-size: 10px;
    color: var(--text-dim);
    flex-shrink: 0;
    padding: 0 1px;
    opacity: 0.5;
  }

  /* URL — clickable to copy */
  #url-row {
    display: flex;
    align-items: center;
    gap: 3px;
    cursor: pointer;
    border-radius: 3px;
    padding: 2px 5px;
    flex-shrink: 2;
    min-width: 0;
    transition: background 0.12s ease;
  }
  #url-row:hover  { background: var(--hover-bg); }
  #url-row.copied { background: var(--copied-bg); }

  #lock {
    font-size: 9px;
    flex-shrink: 0;
    line-height: 1;
  }
  #lock.secure   { color: var(--lock-secure); }
  #lock.insecure { color: var(--lock-insecure); }

  #url {
    font-size: 10px;
    color: var(--text-url);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    letter-spacing: -0.1px;
  }
  #url .host { color: var(--text-host); }

  /* Shared "Copied" flash label */
  .copied-label {
    font-size: 9px;
    color: var(--copied-text);
    opacity: 0;
    transition: opacity 0.12s ease;
    flex-shrink: 0;
  }
  .copied-label.show { opacity: 1; }

  /* ── Stats section ───────────────────────────────────────────────── */
  #stats {
    display: flex;
    align-items: center;
    gap: 8px;
    font-size: 10px;
    color: var(--text-dim);
    flex-shrink: 0;
    margin-left: 12px;
    margin-right: 8px;
    letter-spacing: -0.1px;
  }
  .stat { white-space: nowrap; }
  #stats .dot { opacity: 0.5; }

  /* ── 🐙 AI toggle button + utility buttons (spotlight, close tab) ── */
  .bar-btn {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 26px;
    height: 26px;
    border-radius: 7px;
    border: none;
    background: transparent;
    cursor: pointer;
    flex-shrink: 0;
    color: var(--text-dim);
    transition: background 0.15s ease, color 0.15s ease, transform 0.12s ease;
  }
  .bar-btn:hover  { background: var(--btn-hover); color: var(--text-title); }
  .bar-btn:active { transform: scale(0.90); transition-duration: 0.06s; }

  #ai-btn {
    background: transparent;
    color: inherit;
    width: 28px;
    height: 28px;
    border-radius: 50%;
    border: none;
  }
  #ai-btn:hover { transform: scale(1.15); }
  #ai-btn:active { transform: scale(0.92); }
  .ai-icon { font-size: 15px; line-height: 1; user-select: none; }

  /* ── Unread badge dot ────────────────────────────────────────────── */
  .badge {
    position: absolute;
    top: 1px; right: 1px;
    width: 8px; height: 8px;
    border-radius: 50%;
    background: #ff3b30;
    border: 1.5px solid rgba(255,255,255,0.9);
    box-shadow: 0 1px 4px rgba(255,59,48,0.45);
    opacity: 0;
    transform: scale(0);
    transition: opacity 0.2s ease, transform 0.25s cubic-bezier(0.34,1.56,0.64,1);
    pointer-events: none;
  }
  .badge.show { opacity: 1; transform: scale(1); }
  @media (prefers-color-scheme: dark) {
    .badge { border-color: rgba(0,0,0,0.5); box-shadow: 0 1px 6px rgba(255,59,48,0.55); }
  }
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
        <span id="lock"></span>
        <span id="url"></span>
        <span class="copied-label" id="url-copied">Copied</span>
      </div>
    </div>
  </div>
  <div id="stats">
    <span id="size" class="stat"></span>
    <span class="dot" id="dot">·</span>
    <span id="time" class="stat"></span>
  </div>
  <button id="shortcuts-btn" class="bar-btn" title="Keyboard shortcuts (⌘/)">
    <svg width="12" height="12" viewBox="0 0 16 16" fill="none">
      <path d="M6.2 5.6c0-.9.7-1.6 1.8-1.6 1 0 1.8.7 1.8 1.6 0 .7-.4 1.1-1.1 1.6-.6.4-.9.7-.9 1.3v.3" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
      <circle cx="8" cy="11.5" r="0.9" fill="currentColor"/>
    </svg>
  </button>
  <button id="spotlight-btn" class="bar-btn" title="Command palette (⌘K)">
    <svg width="13" height="13" viewBox="0 0 16 16" fill="none">
      <circle cx="6.5" cy="6.5" r="4.5" stroke="currentColor" stroke-width="1.6"/>
      <line x1="10" y1="10" x2="14" y2="14" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
    </svg>
  </button>
  <button id="close-tab-btn" class="bar-btn" title="Close tab (⌘W)">
    <svg width="11" height="11" viewBox="0 0 12 12" fill="none">
      <line x1="1.5" y1="1.5" x2="10.5" y2="10.5" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
      <line x1="10.5" y1="1.5" x2="1.5" y2="10.5" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
    </svg>
  </button>
  <button id="ai-btn" title="Toggle AI sidebar (⌘⇧A)">
    <span class="ai-icon">🐙</span>
    <span id="badge" class="badge"></span>
  </button>
</div>
<script>
(function() {
  const titleEl      = document.getElementById('page-title');
  const urlEl        = document.getElementById('url');
  const lockEl       = document.getElementById('lock');
  const faviconEl    = document.getElementById('favicon');
  const sizeEl       = document.getElementById('size');
  const timeEl       = document.getElementById('time');
  const dotEl        = document.getElementById('dot');
  const titleRow     = document.getElementById('title-row');
  const urlRow       = document.getElementById('url-row');
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
    if (bytes <= 0) return '';
    if (bytes < 1024) return bytes + ' B';
    if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + ' KB';
    return (bytes / (1024 * 1024)).toFixed(1) + ' MB';
  }

  function formatTime(ms) {
    if (ms <= 0) return '';
    if (ms < 1000) return Math.round(ms) + ' ms';
    return (ms / 1000).toFixed(1) + ' s';
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

  function updateLock(url, secure) {
    if (url && url !== 'about:blank') {
      lockEl.textContent = secure ? '🔒' : '⚠️';
      lockEl.className = secure ? 'secure' : 'insecure';
    } else {
      lockEl.textContent = '';
      lockEl.className = '';
    }
  }

  function updateStats(sizeBytes, timeMs) {
    if (sizeBytes > 0) sizeEl.textContent = formatSize(sizeBytes);
    if (timeMs > 0)    timeEl.textContent = formatTime(timeMs);
    dotEl.style.display = (sizeEl.textContent && timeEl.textContent) ? '' : 'none';
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
    sizeEl.textContent = '';
    timeEl.textContent = '';
    dotEl.style.display = 'none';
  };

  window.__stats = function(sizeBytes, timeMs) {
    updateStats(sizeBytes, timeMs);
  };

  // Copy title on click
  titleRow.addEventListener('click', function() {
    if (!currentTitle) return;
    copyViaIpc(currentTitle);
    flashCopied(titleRow, titleCopied);
  });

  // Copy URL on click
  urlRow.addEventListener('click', function() {
    if (!currentUrl) return;
    copyViaIpc(currentUrl);
    flashCopied(urlRow, urlCopied);
  });

  // 🐙 AI toggle button
  document.getElementById('ai-btn').addEventListener('click', function() {
    window.ipc.postMessage(JSON.stringify({ type: 'toggle_sidebar' }));
  });

  // Spotlight / command palette
  document.getElementById('spotlight-btn').addEventListener('click', function() {
    window.ipc.postMessage(JSON.stringify({ type: 'toggle_overlay' }));
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

  // Initial state
  dotEl.style.display = 'none';
})();
</script>
</body>
</html>"#
}
