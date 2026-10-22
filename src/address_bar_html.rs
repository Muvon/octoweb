/// Titlebar address bar — child of browser_win, lives in the native titlebar zone.
///
/// Shows: favicon | title (top) / url (bottom) | site permissions | stats | 🐙 AI toggle
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
///   window.__setSidebarOpen(open)                             — 🐙 takes the brand color while open
///   window.__sysStats(cpuPct, memMb)                          — update CPU%/RSS (null = hide)
///   window.__setShortcuts(data)                               — update live shortcut titles
///   window.__setExtensionButtons(buttons)                     — extension actions: [{id, label, badge, enabled, icon}]
///   window.__perms(state)                                     — camera/mic/location chip + prompt (null = hide)
///
/// IPC messages sent to Rust:
///   { type: "toggle_sidebar" }       — 🐙 button clicked
///   { type: "copy_text", text: "…" } — copy title or URL to clipboard
///   { type: "ext_action", id, x, y, w, h } — extension button clicked (its rect anchors the popup)
///   { type: "perm_action", action }  — permission popover: answer / set / capture / reset
///   { type: "perm_popover", height } — grow the bar by the popover's height (0 = closed)
///   { type: "perm_reload" }          — reload the page so a changed permission applies
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

  /* A short title yields its unused space to the URL. */
  #text-stack {
    display: flex;
    flex-direction: row;
    align-items: center;
    min-width: 0;
    flex: 1;
    overflow: hidden;
  }

  /* Title — clicking enters address edit; fixed 38% column so the URL column never moves. */
  #title-row {
    display: flex;
    align-items: center;
    gap: 3px;
    min-height: var(--ctl-min);
    border-radius: var(--r-ctl);
    padding: 2px 5px;
    /* Fixed split with #url-row so the address never shifts with title length. */
    flex: 0 0 38%;
    min-width: 0;
    overflow: hidden;
    transition: background var(--t-fast) var(--ease), transform var(--t-fast) var(--ease);
  }
  #title-row:hover  { background: var(--fill-hover); }
  #title-row:active { background: var(--fill-press); }

  #title-edit {
    display: flex;
    align-items: center;
    flex: 1;
    min-width: 0;
    overflow: hidden;
    padding: 0;
    border: none;
    background: transparent;
    color: inherit;
    font-family: inherit;
    text-align: left;
    cursor: pointer;
  }

  #page-title {
    font-size: var(--fs-body);
    font-weight: 500;
    color: var(--label);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    letter-spacing: -0.1px;
  }

  /* Separator between title and url */
  #sep {
    font-size: var(--fs-caption);
    color: var(--label-3);
    flex-shrink: 0;
    padding: 0 1px;
    opacity: 0.5;
  }

  /* URL — a single click enters edit mode and selects the whole address. */
  #url-row {
    position: relative;
    display: flex;
    align-items: center;
    gap: 3px;
    min-height: var(--ctl-min);
    border-radius: var(--r-ctl);
    padding: 2px 5px;
    flex: 0 0 calc(62% - 14px); /* subtract #sep width so total stays 100% */
    min-width: 0;
    overflow: visible; /* let the suggestion dropdown escape this row */
    transition: background var(--t-fast) var(--ease);
  }
  /* The displayed address is the edit affordance. */
  #url-copy {
    display: flex;
    align-items: center;
    gap: 3px;
    flex: 1;
    min-width: 0;
    overflow: hidden;
    cursor: pointer;
    border: none;
    background: transparent;
    color: inherit;
    font-family: inherit;
    text-align: left;
    min-height: var(--ctl-min);
    border-radius: var(--r-ctl);
    padding: 0 2px;
    transition: background var(--t-fast) var(--ease);
  }
  #url-copy:hover { background: var(--fill-hover); }
  #url-copy:active { background: var(--fill-press); }

  .copy-btn {
    width: 22px;
    height: 22px;
    flex: 0 0 22px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    border: none;
    border-radius: var(--r-ctl);
    background: transparent;
    color: var(--label-2);
    cursor: pointer;
    line-height: 0;
    visibility: hidden;
    transition: background var(--t-fast) var(--ease), color var(--t-fast) var(--ease),
                transform var(--t-fast) var(--ease);
  }
  #title-row:hover .copy-btn,
  #title-row:focus-within .copy-btn,
  #url-row:hover .copy-btn,
  #url-row:focus-within .copy-btn,
  .copy-btn.copied { visibility: visible; }
  .copy-btn:hover  { background: var(--fill-hover); color: var(--label); }
  .copy-btn:active { background: var(--fill-press); transform: scale(0.92); }
  .copy-btn svg { width: 12px; height: 12px; }
  .copy-btn .copy-check { display: none; }
  .copy-btn.copied { color: var(--ok); }
  .copy-btn.copied .copy-icon { display: none; }
  .copy-btn.copied .copy-check { display: inline; }
  .copy-btn.later-on { visibility: visible; color: var(--accent); }
  .copy-btn.later-on .later-fill { fill: currentColor; }

  /* URL input — shown only while editing; replaces the copyable URL display. */
  #url-input {
    flex: 1;
    min-width: 0;
    height: var(--ctl-min);
    padding: 0 9px;
    font: inherit;
    font-size: var(--fs-body);
    color: var(--label);
    background: var(--fill);
    border: none;
    border-radius: var(--r-ctl);
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
  #url-row.editing .copy-btn  { visibility: hidden; }

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
    border-radius: calc(var(--r-panel) - 5px);
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
    font-size: var(--fs-body);
    color: var(--label);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    letter-spacing: -0.1px;
  }
  .sg-url {
    font-family: var(--font-text);
    font-size: var(--fs-caption);
    color: var(--label-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    margin-top: 1px;
    letter-spacing: -0.1px;
  }
  .sg-kind {
    flex-shrink: 0;
    font-size: var(--fs-caption);
    font-weight: 500;
    color: var(--label-2);
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
    color: var(--label-2);
    font-size: var(--fs-caption);
    box-shadow: 0 -0.5px 0 var(--hairline);
  }

  /* Lock / shield — the site-info button: opens the permission popover on
     any website, whether or not the site has asked for anything yet. */
  #lock {
    flex-shrink: 0;
    width: 20px;
    height: 20px;
    margin-right: 1px;
    padding: 0;
    border: none;
    border-radius: var(--r-ctl);
    background: transparent;
    display: none;
    align-items: center;
    justify-content: center;
    line-height: 0;
    cursor: pointer;
    transition: background var(--t-fast) var(--ease), transform var(--t-fast) var(--ease);
  }
  #lock.visible { display: inline-flex; }
  #lock:hover  { background: var(--fill-hover); }
  #lock:active { background: var(--fill-press); transform: scale(0.92); }
  #lock[aria-expanded="true"] { background: var(--fill-press); }
  #lock svg { width: 12px; height: 12px; }
  #lock.secure   { color: var(--lock-secure); }
  #lock.insecure { color: var(--lock-insecure); }

  #url {
    font-size: var(--fs-body);
    font-weight: 400;
    color: var(--label-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    letter-spacing: -0.1px;
  }
  #url .host { color: var(--label); }

  .visually-hidden {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border: 0;
  }

  /* ── Combined page/system diagnostics ────────────────────────────── */
  #stats-chip {
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: var(--ctl-min);
    padding: 0 7px;
    border-radius: var(--r-capsule);
    background: var(--fill);
    font-size: var(--fs-caption);
    color: var(--label-2);
    flex-shrink: 0;
    margin-left: 8px;
    margin-right: 8px;
    letter-spacing: -0.1px;
    font-variant-numeric: tabular-nums;
  }
  #stats-chip.hidden { display: none; }
  .stat { display: inline-flex; align-items: center; gap: 2px; white-space: nowrap; }
  .stat.hidden { display: none; }
  .stat-val {
    display: inline-block;
    text-align: right;
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

  @media (max-width: 999px) {
    #stats-chip { display: none !important; }
  }

  /* ── Site permissions: chip + popover ──────────────────────────────
     Stays visible at every width: it is a privacy indicator, not a stat. */
  #perm-chip {
    display: flex;
    align-items: center;
    gap: 5px;
    min-height: var(--ctl-min);
    padding: 0 7px;
    margin-left: 8px;
    border: none;
    border-radius: var(--r-capsule);
    background: var(--fill);
    color: var(--label-2);
    font: inherit;
    cursor: pointer;
    flex-shrink: 0;
    transition: background var(--t-fast) var(--ease), color var(--t-fast) var(--ease),
                transform var(--t-fast) var(--ease);
  }
  #perm-chip.hidden { display: none; }
  /* The next meeting, from half an hour before it starts: one click joins. */
  #meet-chip {
    display: flex; align-items: center; gap: 5px; min-height: var(--ctl-min); padding: 0 9px; margin-left: 8px;
    border: none; border-radius: var(--r-capsule); font: inherit; cursor: pointer; flex-shrink: 1; min-width: 0; max-width: 280px;
    background: color-mix(in srgb, var(--accent) 14%, transparent); color: var(--accent);
    transition: background var(--t-fast) var(--ease), transform var(--t-fast) var(--ease);
  }
  #meet-chip.hidden { display: none; }
  #meet-chip:hover { background: color-mix(in srgb, var(--accent) 22%, transparent); }
  #meet-chip:active { transform: scale(0.97); }
  #meet-chip.now { background: color-mix(in srgb, var(--ok) 16%, transparent); color: var(--ok); }
  #meet-chip .mt { font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; min-width: 0; }
  #meet-chip .mw { white-space: nowrap; opacity: 0.85; font-variant-numeric: tabular-nums; }
  #perm-chip:hover  { background: var(--fill-hover); color: var(--label); }
  #perm-chip:active { transform: scale(0.95); }
  #perm-chip[aria-expanded="true"] { background: var(--fill-press); color: var(--label); }
  /* Camera or microphone on: recording red, never ambiguous. */
  #perm-chip.live { background: color-mix(in srgb, var(--err) 14%, transparent); color: var(--err); }
  #perm-chip.live:hover { background: color-mix(in srgb, var(--err) 22%, transparent); }
  /* A site is waiting for an answer. */
  #perm-chip.asking {
    background: color-mix(in srgb, var(--accent) 16%, transparent);
    color: var(--accent);
    animation: perm-ask 1.6s var(--ease) infinite;
  }
  @keyframes perm-ask { 50% { background: color-mix(in srgb, var(--accent) 30%, transparent); } }
  .perm-icon {
    width: 12px;
    height: 12px;
    line-height: 0;
    display: inline-flex;
    flex-shrink: 0;
  }
  .perm-icon svg { width: 100%; height: 100%; }
  .live-dot {
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: currentColor;
    animation: live-pulse 1.4s ease-in-out infinite;
  }
  @keyframes live-pulse { 50% { opacity: 0.3; } }

  #perm-pop {
    position: absolute;
    z-index: 210;
    width: 304px;
    padding: 12px 14px;
    display: none;
    font-size: var(--fs-body);
    color: var(--label);
  }
  #perm-pop.show {
    display: block;
    animation: suggest-pop var(--t-pop) var(--spring);
  }
  .pp-head { display: flex; align-items: center; gap: 8px; }
  .pp-head .perm-icon { width: 15px; height: 15px; color: var(--accent); }
  .pp-title { flex: 1; min-width: 0; }
  .pp-host {
    font-weight: 600;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    letter-spacing: -0.1px;
  }
  .pp-sub { font-size: var(--fs-caption); color: var(--label-2); margin-top: 1px; }
  .pp-close {
    width: 22px;
    height: 22px;
    flex: 0 0 22px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    border: none;
    border-radius: var(--r-capsule);
    background: transparent;
    color: var(--label-2);
    cursor: pointer;
    line-height: 0;
  }
  .pp-close:hover { background: var(--fill-hover); color: var(--label); }
  .pp-close svg { width: 10px; height: 10px; }
  .pp-ask { line-height: 1.4; margin: 10px 0 12px; }
  .pp-actions { display: flex; justify-content: flex-end; gap: 6px; }
  .pp-btn {
    min-height: 26px;
    padding: 0 12px;
    border: none;
    border-radius: var(--r-ctl);
    background: var(--fill);
    color: var(--label);
    font: inherit;
    cursor: pointer;
    transition: background var(--t-fast) var(--ease), transform var(--t-fast) var(--ease);
  }
  .pp-btn:hover  { background: var(--fill-hover); }
  .pp-btn:active { background: var(--fill-press); transform: scale(0.97); }
  .pp-btn.primary { background: var(--accent); color: var(--on-accent); }
  .pp-btn.primary:hover { background: color-mix(in srgb, var(--accent) 86%, #000); }
  .pp-btn.danger { color: var(--err); }
  .pp-rows { margin-top: 8px; }
  .pp-row {
    display: grid;
    grid-template-columns: 16px 1fr auto;
    align-items: center;
    gap: 6px 10px;
    padding: 8px 0;
    box-shadow: 0 -0.5px 0 var(--hairline);
  }
  .pp-row .perm-icon { width: 15px; height: 15px; color: var(--label-2); }
  .pp-row.live .perm-icon, .pp-row.live .pp-state { color: var(--err); }
  .pp-state { font-size: var(--fs-caption); color: var(--label-2); margin-top: 1px; }
  .pp-live { grid-column: 2 / -1; display: flex; gap: 6px; }
  .pp-live .pp-btn { min-height: 22px; padding: 0 10px; font-size: var(--fs-caption); }
  .seg {
    display: inline-flex;
    padding: 2px;
    border-radius: var(--r-ctl);
    background: var(--fill);
  }
  .seg button {
    border: none;
    background: transparent;
    font: inherit;
    font-size: var(--fs-caption);
    color: var(--label-2);
    padding: 3px 8px;
    border-radius: calc(var(--r-ctl) - 2px);
    cursor: pointer;
  }
  .seg button:hover { color: var(--label); }
  .seg button[aria-pressed="true"] {
    background: var(--card);
    color: var(--label);
    box-shadow: 0 0.5px 2px rgba(0, 0, 0, 0.18);
  }
  .pp-foot {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding-top: 10px;
    box-shadow: 0 -0.5px 0 var(--hairline);
  }
  .pp-link {
    border: none;
    background: none;
    padding: 2px 0;
    color: var(--label-2);
    font: inherit;
    font-size: var(--fs-caption);
    cursor: pointer;
  }
  .pp-link:hover { color: var(--label); text-decoration: underline; }
  .pp-note { font-size: var(--fs-caption); color: var(--label-2); }

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
    color: var(--label-2);
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
  /* NTF-3: the dot carries the workspace's unread Important mail. */
  .ws-dot.count {
    bottom: 0; right: 0;
    width: auto; min-width: 12px; height: 12px;
    padding: 0 3px; box-sizing: border-box;
    border-radius: 6px;
    font: 600 8.5px/12px var(--font-text);
    color: #fff; text-align: center;
  }

  #ai-btn {
    background: transparent;
    color: var(--label-2);
    width: 28px;
    height: 28px;
    border-radius: 50%;
    border: none;
  }
  #ai-btn:hover  { background: var(--fill-hover); color: var(--brand); }
  #ai-btn:active { background: var(--fill-press); transform: scale(0.92); }
  #ai-btn.active { color: var(--brand); }
  /* The octopus is pixel art on a 20-cell grid: at 20px every cell lands on
     whole device pixels, so it stays crisp. Grey with see-through eyes at
     rest; brand purple with its own eyes on hover and while the sidebar is open. */
  .ai-icon { display: inline-flex; width: 20px; height: 20px; line-height: 0; }
  .ai-icon svg { width: 100%; height: 100%; }
  #ai-btn:not(:hover):not(.active) .ai-icon path + path { fill: transparent; }

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

  /* ── Extension action buttons ─────────────────────────────────────── */
  #ext-bar { display: flex; align-items: center; gap: 2px; flex-shrink: 0; }
  #ext-bar:empty { display: none; }
  .ext-btn img { width: 16px; height: 16px; display: block; }
  .ext-btn:disabled { opacity: 0.4; cursor: default; }
  .ext-letter { font-size: var(--fs-caption); font-weight: 600; line-height: 1; }
  .ext-badge {
    position: absolute;
    bottom: 0; right: -2px;
    min-width: 12px; height: 12px;
    padding: 0 3px;
    box-sizing: border-box;
    border-radius: var(--r-capsule);
    background: var(--err);
    color: var(--on-accent);
    font-size: 8px;
    font-weight: 600;
    line-height: 12px;
    text-align: center;
    pointer-events: none;
  }
</style>
</head>
<body>
<div id="bar">
  <div id="info">
    <img id="favicon" alt="">
    <div id="text-stack">
      <div id="title-row">
        <button id="title-edit" type="button" title="Edit address"><span id="page-title"></span></button>
        <button id="title-copy-btn" class="copy-btn" type="button" aria-label="Copy title" title="Copy title">
          <svg class="copy-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2"/></svg>
          <svg class="copy-check" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polyline points="4 12 10 18 20 6"/></svg>
        </button>
      </div>
      <span id="sep">·</span>
      <div id="url-row">
        <button id="lock" type="button" aria-haspopup="dialog" aria-expanded="false" aria-controls="perm-pop" title="Site info and permissions"></button>
        <button id="url-copy" type="button" title="Edit address">
          <span id="url"></span>
        </button>
        <button id="url-copy-btn" class="copy-btn" type="button" aria-label="Copy address" title="Copy address">
          <svg class="copy-icon" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><rect x="8" y="8" width="12" height="12" rx="2"/><path d="M16 8V6a2 2 0 0 0-2-2H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h2"/></svg>
          <svg class="copy-check" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2.4" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><polyline points="4 12 10 18 20 6"/></svg>
        </button>
        <button id="later-btn" class="copy-btn" type="button" aria-label="Save for later" title="Save for later" aria-pressed="false">
          <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path class="later-fill" d="M6 3h12v18l-6-4-6 4z"/></svg>
        </button>
        <input id="url-input" type="text" role="combobox" aria-label="Address" aria-autocomplete="list" aria-expanded="false" aria-controls="url-suggest" spellcheck="false" autocomplete="off" autocapitalize="off">
      </div>
    </div>
  </div>
  <button id="perm-chip" class="hidden" type="button" aria-haspopup="dialog" aria-expanded="false" aria-controls="perm-pop"></button>
  <div id="stats-chip" class="hidden" aria-label="Page and system diagnostics">
    <span class="stat hidden" id="size-chip" title="Page transfer size"><span class="sys-icon">@@ICON_DOWNLOAD@@</span><span class="stat-val" id="size"></span></span>
    <span class="stat hidden" id="time-chip" title="Page load time"><span class="sys-icon">@@ICON_CLOCK@@</span><span class="stat-val" id="time"></span></span>
    <span class="stat hidden" id="cpu-chip" title="CPU usage of this tab's web process"><span class="sys-icon">@@ICON_ACTIVITY@@</span><span class="stat-val" id="cpu-stat"></span></span>
    <span class="stat hidden" id="mem-chip" title="Memory used by this tab's web process"><span class="sys-icon">@@ICON_CPU@@</span><span class="stat-val" id="mem-stat"></span></span>
  </div>
  <button id="meet-chip" class="hidden" type="button"><span class="mt"></span><span class="mw"></span></button>
  <div id="ext-bar" role="toolbar" aria-label="Extensions"></div>
  <button id="workspace-btn" class="bar-btn" type="button" title="Workspaces">
    <span class="ws-icon">@@ICON_LAYERS@@</span>
    <span class="ws-dot" id="ws-dot"></span>
  </button>
  <button id="settings-btn" class="bar-btn" type="button" title="Settings">
    <svg width="12" height="12" viewBox="0 0 16 16" fill="none">
      <path d="M6.6 1.2h2.8l.4 1.9.5.2 1.7-.9 2 2-.9 1.7.2.5 1.9.4v2.8l-1.9.4-.2.5.9 1.7-2 2-1.7-.9-.5.2-.4 1.9H6.6l-.4-1.9-.5-.2-1.7.9-2-2 .9-1.7-.2-.5L.8 9.2V6.4l1.9-.4.2-.5-.9-1.7 2-2 1.7.9.5-.2.4-1.3z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round" fill="none"/>
      <circle cx="8" cy="7.8" r="2" stroke="currentColor" stroke-width="1.2" fill="none"/>
    </svg>
  </button>
  <button id="shortcuts-btn" class="bar-btn" type="button" title="Shortcuts">
    <svg width="12" height="12" viewBox="0 0 16 16" fill="none">
      <path d="M6.2 5.6c0-.9.7-1.6 1.8-1.6 1 0 1.8.7 1.8 1.6 0 .7-.4 1.1-1.1 1.6-.6.4-.9.7-.9 1.3v.3" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
      <circle cx="8" cy="11.5" r="0.9" fill="currentColor"/>
    </svg>
  </button>
  <button id="spotlight-btn" class="bar-btn" type="button" title="Search">
    <svg width="13" height="13" viewBox="0 0 16 16" fill="none">
      <circle cx="6.5" cy="6.5" r="4.5" stroke="currentColor" stroke-width="1.6"/>
      <line x1="10" y1="10" x2="14" y2="14" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
    </svg>
  </button>
  <button id="close-tab-btn" class="bar-btn" type="button" title="Close tab">
    <svg width="11" height="11" viewBox="0 0 12 12" fill="none">
      <line x1="1.5" y1="1.5" x2="10.5" y2="10.5" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
      <line x1="10.5" y1="1.5" x2="1.5" y2="10.5" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
    </svg>
  </button>
  <button id="ai-btn" class="bar-btn" type="button" title="AI sidebar" aria-pressed="false">
    <span class="ai-icon">@@ICON_OCTOPUS_BRAND@@</span>
    <span id="badge" class="badge"></span>
  </button>
</div>
<div id="url-suggest" class="glass-panel" role="listbox"></div>
<div id="perm-pop" class="glass-panel" role="dialog" aria-label="Site permissions"></div>
<div id="copy-status" class="visually-hidden" aria-live="polite"></div>
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
  const statsChipEl  = document.getElementById('stats-chip');
  const cpuChipEl    = document.getElementById('cpu-chip');
  const memChipEl    = document.getElementById('mem-chip');
  const cpuStatEl    = document.getElementById('cpu-stat');
  const memStatEl    = document.getElementById('mem-stat');
  const titleRow     = document.getElementById('title-row');
  const titleEdit    = document.getElementById('title-edit');
  const titleCopyBtn = document.getElementById('title-copy-btn');
  const urlRow       = document.getElementById('url-row');
  const urlCopy      = document.getElementById('url-copy');
  const urlCopyBtn   = document.getElementById('url-copy-btn');
  const laterBtn     = document.getElementById('later-btn');
  const urlInput     = document.getElementById('url-input');
  const urlSuggest   = document.getElementById('url-suggest');
  const copyStatus   = document.getElementById('copy-status');

  let currentUrl   = '';
  let currentTitle = '';
  let laterUrls    = new Set();

  window.__setShortcuts = function(data) {
    const actions = data && Array.isArray(data.actions) ? data.actions : [];
    const titles = {
      toggle_workspaces: ['workspace-btn', 'Workspaces'],
      settings: ['settings-btn', 'Settings'],
      shortcuts: ['shortcuts-btn', 'Shortcuts'],
      command_palette: ['spotlight-btn', 'Search'],
      close_tab: ['close-tab-btn', 'Close tab'],
      sidebar: ['ai-btn', 'AI sidebar'],
      url_edit: ['url-copy', 'Edit address'],
      save_later: ['later-btn', 'Save for later']
    };
    Object.keys(titles).forEach(function(id) {
      const target = titles[id];
      const action = actions.find(function(item) { return item.id === id; });
      const chord = action && Array.isArray(action.keys) ? action.keys.join('') : '';
      document.getElementById(target[0]).title = target[1] + (chord ? ' (' + chord + ')' : '');
    });
  };

  // Copy via IPC — navigator.clipboard unavailable in child WKWebViews
  function copyViaIpc(text) {
    window.ipc.postMessage(JSON.stringify({ type: 'copy_text', text }));
  }

  function flashCopied(button, announcement) {
    if (button.copyResetTimer) clearTimeout(button.copyResetTimer);
    button.classList.add('copied');
    copyStatus.textContent = '';
    requestAnimationFrame(function() { copyStatus.textContent = announcement; });
    button.copyResetTimer = setTimeout(function() {
      button.classList.remove('copied');
      button.copyResetTimer = null;
    }, 1200);
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
    urlEl.replaceChildren();
    if (!url) return;
    try {
      const u = new URL(url);
      const scheme = document.createElement('span');
      scheme.textContent = u.protocol + (u.host ? '//' : '');
      urlEl.appendChild(scheme);
      if (u.host) {
        const host = document.createElement('span');
        host.className = 'host';
        host.textContent = u.host;
        urlEl.appendChild(host);
        urlEl.appendChild(document.createTextNode(u.pathname + u.search + u.hash));
      } else {
        urlEl.appendChild(document.createTextNode(u.href.slice(u.protocol.length)));
      }
    } catch(e) {
      urlEl.textContent = url;
    }
  }

  var ICON_LOCK   = '@@ICON_LOCK@@';
  var ICON_SHIELD = '@@ICON_SHIELD_ALERT@@';

  function updateLock(url, secure) {
    if (url && url !== 'about:blank') {
      lockEl.innerHTML = secure ? ICON_LOCK : ICON_SHIELD;
      lockEl.className = (secure ? 'secure' : 'insecure') + ' visible';
      lockEl.title = (secure ? 'Connection is secure' : 'Connection is not secure') + ' · site permissions';
      lockEl.setAttribute('aria-label', lockEl.title);
    } else {
      lockEl.innerHTML = '';
      lockEl.className = '';
    }
  }

  function updateStats(sizeBytes, timeMs) {
    const hasSize = sizeBytes > 0;
    const hasTime = timeMs > 0;
    sizeEl.textContent = hasSize ? formatSize(sizeBytes) : '';
    timeEl.textContent = hasTime ? formatTime(timeMs) : '';
    sizeChipEl.classList.toggle('hidden', !hasSize);
    timeChipEl.classList.toggle('hidden', !hasTime);
    sizeChipEl.title = detailSize(sizeBytes);
    timeChipEl.title = detailTime(timeMs);
    updateStatsChip();
  }

  function updateStatsChip() {
    const hasData = !sizeChipEl.classList.contains('hidden')
      || !timeChipEl.classList.contains('hidden')
      || !cpuChipEl.classList.contains('hidden')
      || !memChipEl.classList.contains('hidden');
    statsChipEl.classList.toggle('hidden', !hasData);
  }

  window.__update = function(url, secure, title, sizeBytes, timeMs) {
    currentUrl   = url;
    currentTitle = title || '';
    titleEl.textContent = currentTitle;
    renderUrl(url);
    updateLaterBtn();
    updateLock(url, secure);
    updateStats(sizeBytes, timeMs);
    // A navigation or reload applies any changed site permission.
    permNeedsReload = false;
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
    sizeChipEl.classList.add('hidden');
    timeChipEl.classList.add('hidden');
    sizeChipEl.title = 'Page transfer size';
    timeChipEl.title = 'Page load time';
    updateStatsChip();
  };

  window.__sysStats = function(cpuPct, memMb) {
    if (cpuPct === null || memMb === null || !Number.isFinite(Number(cpuPct))
        || !Number.isFinite(Number(memMb)) || Number(memMb) <= 0) {
      cpuChipEl.classList.add('hidden');
      memChipEl.classList.add('hidden');
      cpuStatEl.textContent = '';
      memStatEl.textContent = '';
      updateStatsChip();
      return;
    }
    cpuStatEl.textContent = cpuPct + '%';
    memStatEl.textContent = memMb + 'M';
    cpuChipEl.classList.remove('hidden');
    memChipEl.classList.remove('hidden');
    updateStatsChip();
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

  function copyFromButton(event, button, text, announcement) {
    event.stopPropagation();
    if (!text) return;
    copyViaIpc(text);
    flashCopied(button, announcement);
  }

  titleCopyBtn.addEventListener('click', function(e) {
    copyFromButton(e, titleCopyBtn, currentTitle, 'Title copied');
  });

  // Plain URL by default; ⌥-click copies a markdown link for Slack/docs.
  urlCopyBtn.addEventListener('click', function(e) {
    if (e.altKey && currentTitle) {
      copyFromButton(e, urlCopyBtn, '[' + currentTitle + '](' + currentUrl + ')', 'Markdown link copied');
    } else {
      copyFromButton(e, urlCopyBtn, currentUrl, 'Address copied');
    }
  });
  urlCopyBtn.title = 'Copy address (⌥ markdown link)';

  function normUrl(u) { return String(u || '').replace(/\/+$/, ''); }
  function updateLaterBtn() {
    const on = laterUrls.has(normUrl(currentUrl));
    laterBtn.classList.toggle('later-on', on);
    laterBtn.setAttribute('aria-pressed', on ? 'true' : 'false');
    laterBtn.title = on ? 'Remove from Later' : 'Save for later';
  }
  window.__setLaterUrls = function(items) {
    laterUrls = new Set((Array.isArray(items) ? items : []).map(function(i) { return normUrl(i.url); }));
    updateLaterBtn();
  };
  laterBtn.addEventListener('click', function(e) {
    e.stopPropagation();
    window.ipc.postMessage(JSON.stringify({ type: 'later_toggle' }));
  });

  function restoreRowFocusOnEscape(button, rowControl) {
    button.addEventListener('keydown', function(e) {
      if (e.key !== 'Escape') return;
      e.preventDefault();
      e.stopPropagation();
      rowControl.focus();
    });
  }

  restoreRowFocusOnEscape(titleCopyBtn, titleEdit);
  restoreRowFocusOnEscape(urlCopyBtn, urlCopy);

  // Both title and address clicks enter the same address editor.
  titleRow.addEventListener('click', function() {
    if (!currentUrl) return;
    enterEdit();
  });

  // Safari-style single click: edit and select the entire address.
  urlRow.addEventListener('click', function() {
    if (!currentUrl) return;
    enterEdit();
  });

  // ── URL edit mode ───────────────────────────────────────────────────────
  // Address click or ⌘E → swap URL display for an input + suggestions dropdown.
  // Submit on Enter; cancel on Esc or click outside.

  var suggestions = [];       // history+tabs snapshot pushed from Rust
  var filtered    = [];       // current filtered list
  var activeIdx   = 0;
  var explicitSelection = false;
  var editing     = false;
  var focusPoll   = null;     // interval that exits edit mode when the
                              // address bar webview loses keyboard focus
                              // (clicks into sibling webviews don't fire
                              // window.blur reliably under WKWebView).

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
    urlSuggest.replaceChildren();
    if (!filtered.length) {
      urlSuggest.classList.remove('show');
      urlInput.setAttribute('aria-expanded', 'false');
      urlInput.removeAttribute('aria-activedescendant');
      return;
    }
    if (activeIdx >= filtered.length) activeIdx = 0;
    for (var i = 0; i < filtered.length; i++) {
      var it = filtered[i];
      var kind = it.kind === 'tab' ? 'Tab' : 'History';
      var titleText = it.title && it.title.length ? it.title : it.url;
      var row = document.createElement('div');
      row.id = 'url-suggestion-' + i;
      row.className = 'sg-item';
      row.dataset.idx = i;
      row.setAttribute('role', 'option');
      row.setAttribute('aria-selected', 'false');

      var fav;
      if (it.favicon) {
        fav = document.createElement('img');
        fav.className = 'sg-fav';
        fav.alt = '';
        fav.src = it.favicon;
        fav.addEventListener('error', function(e) {
          var fallback = document.createElement('span');
          fallback.className = 'sg-fav-fallback';
          e.currentTarget.replaceWith(fallback);
        });
      } else {
        fav = document.createElement('span');
        fav.className = 'sg-fav-fallback';
      }

      var text = document.createElement('div');
      text.className = 'sg-text';
      var title = document.createElement('div');
      title.className = 'sg-title';
      title.textContent = titleText;
      var address = document.createElement('div');
      address.className = 'sg-url';
      address.textContent = it.url;
      text.append(title, address);

      var kindEl = document.createElement('span');
      kindEl.className = 'sg-kind';
      kindEl.textContent = kind;
      row.append(fav, text, kindEl);
      urlSuggest.appendChild(row);
    }
    var hint = document.createElement('div');
    hint.className = 'sg-hint';
    hint.setAttribute('role', 'presentation');
    var key = document.createElement('span');
    key.className = 'kbd';
    key.textContent = 'esc';
    hint.append(key, document.createTextNode(' Dismiss'));
    urlSuggest.appendChild(hint);
    positionSuggest();
    urlSuggest.classList.add('show');
    urlInput.setAttribute('aria-expanded', 'true');
    updateSelection(false);
  }

  function updateSelection(scroll) {
    var nodes = urlSuggest.querySelectorAll('.sg-item');
    var selected = null;
    for (var i = 0; i < nodes.length; i++) {
      var isActive = i === activeIdx;
      nodes[i].classList.toggle('active', isActive);
      nodes[i].setAttribute('aria-selected', isActive ? 'true' : 'false');
      if (isActive) selected = nodes[i];
    }
    if (selected) {
      urlInput.setAttribute('aria-activedescendant', selected.id);
      if (scroll) selected.scrollIntoView({ block: 'nearest' });
    } else {
      urlInput.removeAttribute('aria-activedescendant');
    }
  }

  function positionSuggest() {
    var r = urlRow.getBoundingClientRect();
    urlSuggest.style.left = Math.round(r.left) + 'px';
    urlSuggest.style.top = Math.round(r.bottom + 4) + 'px';
    urlSuggest.style.width = Math.round(r.width) + 'px';
  }

  function enterEdit() {
    if (editing) return;
    closePermPop();
    editing = true;
    urlRow.classList.add('editing');
    urlInput.value = currentUrl || '';
    activeIdx = 0;
    explicitSelection = false;
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
    urlSuggest.replaceChildren();
    urlInput.setAttribute('aria-expanded', 'false');
    urlInput.removeAttribute('aria-activedescendant');
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

  window.__urlEditFocus = function() {
    if (!editing) enterEdit();
    else urlInput.focus();
  };

  function submit(rawUrl) {
    var u = (rawUrl == null ? urlInput.value : rawUrl).trim();
    if (!u) return;
    exitEdit();
    window.ipc.postMessage(JSON.stringify({ type: 'navigate', url: u }));
  }

  urlInput.addEventListener('input', function() {
    activeIdx = 0;
    explicitSelection = false;
    filtered = filterSuggestions(urlInput.value);
    renderSuggestions();
  });

  urlInput.addEventListener('keydown', function(e) {
    if (e.key === 'Enter') {
      if (e.isComposing) return;
      e.preventDefault();
      if (filtered.length && activeIdx >= 0 && activeIdx < filtered.length
          && filtered[activeIdx].url
          && urlInput.value.trim() !== ''
          && (explicitSelection || filtered[activeIdx].score >= 60)) {
        submit(filtered[activeIdx].url);
      } else {
        submit();
      }
    } else if (e.key === 'Escape') {
      e.preventDefault();
      exitEdit();
    } else if (e.key === 'ArrowDown' || (e.ctrlKey && !e.metaKey && !e.altKey && (e.key === 'n' || e.key === 'N'))) {
      // Ctrl+N/P walk the suggestions while editing (the tab-switch binding
      // is suppressed by the host during URL edit).
      e.preventDefault();
      if (filtered.length) {
        activeIdx = (activeIdx + 1) % filtered.length;
        explicitSelection = true;
        updateSelection(true);
      }
    } else if (e.key === 'ArrowUp' || (e.ctrlKey && !e.metaKey && !e.altKey && (e.key === 'p' || e.key === 'P'))) {
      e.preventDefault();
      if (filtered.length) {
        activeIdx = (activeIdx - 1 + filtered.length) % filtered.length;
        explicitSelection = true;
        updateSelection(true);
      }
    } else if (e.key === 'Tab' && filtered.length) {
      e.preventDefault();
      urlInput.value = filtered[activeIdx].url;
      activeIdx = 0;
      explicitSelection = false;
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
      explicitSelection = true;
      updateSelection(false);
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

  // ── Site permissions (camera / microphone / location) ───────────────────
  // Rust pushes the active page's state via __perms (null = not a website):
  // {origin, host, items: [{kind, decision, live, refused}], prompt: {id, kinds}|null}.
  // The chip shows what the site uses, may use, or was refused; the popover
  // answers a waiting request or changes the remembered decision. The bar
  // webview grows by the popover's height while it is open (perm_popover).
  var permChip = document.getElementById('perm-chip');
  var permPop  = document.getElementById('perm-pop');
  var permState = null;
  var permOpen = false;
  var permMode = 'manage';     // 'prompt' | 'manage'
  var permShownPrompt = 0;     // id of the last request that opened the popover
  var permNeedsReload = false; // a changed decision the page has not picked up
  var permSentHeight = 0;
  var permAnchor = permChip;   // what opened the popover: the chip or the lock

  var PERM_KINDS = ['camera', 'microphone', 'location'];
  var PERM = {
    camera:     { name: 'Camera',     on: '@@ICON_VIDEO@@',   off: '@@ICON_VIDEO_OFF@@' },
    microphone: { name: 'Microphone', on: '@@ICON_MIC@@',     off: '@@ICON_MIC_OFF@@' },
    location:   { name: 'Location',   on: '@@ICON_MAP_PIN@@', off: '@@ICON_MAP_PIN_OFF@@' }
  };
  var ICON_X = '<svg viewBox="0 0 12 12" fill="none" aria-hidden="true"><line x1="2" y1="2" x2="10" y2="10" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/><line x1="10" y1="2" x2="2" y2="10" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/></svg>';

  function permSend(action) {
    window.ipc.postMessage(JSON.stringify({ type: 'perm_action', action: action }));
  }

  function permItem(kind) {
    return permState ? permState.items.find(function(i) { return i.kind === kind; }) : null;
  }

  // Something worth showing: in use, remembered, or refused on this page.
  function permRelevant(it) {
    return !!(it && (it.live || it.decision || it.refused));
  }

  function permOff(it) {
    return it.live === 'muted' || it.system_denied || (!it.live && (it.decision === 'block' || it.refused));
  }

  function permStateText(it) {
    if (it.live === 'active') return 'In use';
    if (it.live === 'muted') return 'Paused';
    if (it.system_denied) return 'Allowed, but off in macOS Settings';
    if (it.decision === 'allow') return 'Allowed';
    if (it.decision === 'block') return 'Blocked';
    if (it.refused) return 'Blocked on this page';
    return 'Ask each time';
  }

  function permKindsText(kinds) {
    var names = kinds.map(function(k) { return PERM[k].name.toLowerCase(); });
    if (names.length < 2) return names[0] || '';
    return names.slice(0, -1).join(', ') + ' and ' + names[names.length - 1];
  }

  function permIcon(kind, off) {
    var span = document.createElement('span');
    span.className = 'perm-icon';
    span.innerHTML = off ? PERM[kind].off : PERM[kind].on;
    return span;
  }

  function renderPermChip() {
    var s = permState;
    var asking = !!(s && s.prompt);
    var kinds = s ? PERM_KINDS.filter(function(k) {
      return permRelevant(permItem(k)) || (asking && s.prompt.kinds.indexOf(k) >= 0);
    }) : [];
    if (!s || !kinds.length) {
      permChip.classList.add('hidden');
      if (!s) closePermPop();
      return;
    }
    var live = kinds.some(function(k) { return permItem(k).live === 'active'; });
    permChip.replaceChildren();
    kinds.forEach(function(k) {
      var it = permItem(k);
      permChip.appendChild(permIcon(k, !(asking && s.prompt.kinds.indexOf(k) >= 0) && permOff(it)));
    });
    if (live) {
      var dot = document.createElement('span');
      dot.className = 'live-dot';
      permChip.appendChild(dot);
    }
    permChip.classList.toggle('live', live && !asking);
    permChip.classList.toggle('asking', asking);
    permChip.classList.remove('hidden');
    var label = asking
      ? s.host + ' wants to use your ' + permKindsText(s.prompt.kinds)
      : kinds.map(function(k) { return PERM[k].name + ': ' + permStateText(permItem(k)).toLowerCase(); }).join(' · ');
    permChip.title = label || 'Site permissions';
    permChip.setAttribute('aria-label', 'Site permissions — ' + (label || s.host));
  }

  function permButton(text, cls, onClick) {
    var b = document.createElement('button');
    b.type = 'button';
    b.className = cls;
    b.textContent = text;
    b.addEventListener('click', function(e) { e.stopPropagation(); onClick(); });
    return b;
  }

  function permHead(kinds, sub) {
    var head = document.createElement('div');
    head.className = 'pp-head';
    kinds.forEach(function(k) { head.appendChild(permIcon(k, false)); });
    var title = document.createElement('div');
    title.className = 'pp-title';
    var host = document.createElement('div');
    host.className = 'pp-host';
    host.textContent = permState.host;
    title.appendChild(host);
    if (sub) {
      var subEl = document.createElement('div');
      subEl.className = 'pp-sub';
      subEl.textContent = sub;
      title.appendChild(subEl);
    }
    head.appendChild(title);
    var close = document.createElement('button');
    close.type = 'button';
    close.className = 'pp-close';
    close.innerHTML = ICON_X;
    head.appendChild(close);
    return { el: head, close: close };
  }

  function renderPermPrompt() {
    var p = permState.prompt;
    var head = permHead(p.kinds, '');
    // Dismissing refuses this request only; the site may ask again.
    head.close.title = 'Not now';
    head.close.setAttribute('aria-label', 'Not now');
    head.close.addEventListener('click', function(e) {
      e.stopPropagation();
      permSend({ op: 'answer', allow: false, remember: false });
      closePermPop();
    });
    var ask = document.createElement('div');
    ask.className = 'pp-ask';
    ask.textContent = 'wants to use your ' + permKindsText(p.kinds) + '.';
    var actions = document.createElement('div');
    actions.className = 'pp-actions';
    actions.append(
      permButton('Block', 'pp-btn', function() {
        permSend({ op: 'answer', allow: false, remember: true });
        closePermPop();
      }),
      permButton('Allow', 'pp-btn primary', function() {
        permSend({ op: 'answer', allow: true, remember: true });
        closePermPop();
      })
    );
    permPop.append(head.el, ask, actions);
  }

  function renderPermManage() {
    var secure = /^https:/i.test(currentUrl);
    var head = permHead([], secure ? 'Connection is secure' : 'Connection is not secure');
    head.close.title = 'Close';
    head.close.setAttribute('aria-label', 'Close');
    head.close.addEventListener('click', function(e) { e.stopPropagation(); closePermPop(); });
    var rows = document.createElement('div');
    rows.className = 'pp-rows';
    PERM_KINDS.forEach(function(k) {
      var it = permItem(k);
      var row = document.createElement('div');
      row.className = 'pp-row' + (it.live === 'active' ? ' live' : '');
      var text = document.createElement('div');
      var name = document.createElement('div');
      name.textContent = PERM[k].name;
      var state = document.createElement('div');
      state.className = 'pp-state';
      state.textContent = permStateText(it);
      text.append(name, state);

      var seg = document.createElement('div');
      seg.className = 'seg';
      seg.setAttribute('role', 'group');
      seg.setAttribute('aria-label', PERM[k].name);
      [['Ask', null], ['Allow', 'allow'], ['Block', 'block']].forEach(function(opt) {
        var b = permButton(opt[0], '', function() {
          if ((it.decision || null) === opt[1]) return;
          permSend({ op: 'set', kind: k, decision: opt[1] });
          // Blocking live capture stops it at once; anything else takes
          // effect when the page asks again.
          if (!(opt[1] === 'block' && it.live)) permNeedsReload = true;
        });
        b.setAttribute('aria-pressed', String((it.decision || null) === opt[1]));
        seg.appendChild(b);
      });
      row.append(permIcon(k, permOff(it)), text, seg);

      if (it.live) {
        var live = document.createElement('div');
        live.className = 'pp-live';
        var paused = it.live === 'muted';
        live.append(
          permButton(paused ? 'Resume' : 'Pause', 'pp-btn', function() {
            permSend({ op: 'capture', kind: k, state: paused ? 'active' : 'muted' });
          }),
          permButton('Stop', 'pp-btn danger', function() {
            permSend({ op: 'capture', kind: k, state: 'none' });
          })
        );
        row.appendChild(live);
      }
      if (it.system_denied) {
        var sys = document.createElement('div');
        sys.className = 'pp-live';
        sys.appendChild(permButton('Open macOS Settings', 'pp-btn', function() {
          window.ipc.postMessage(JSON.stringify({ type: 'perm_system_settings' }));
          closePermPop();
        }));
        row.appendChild(sys);
      }
      rows.appendChild(row);
    });

    var foot = document.createElement('div');
    foot.className = 'pp-foot';
    var hasAny = PERM_KINDS.some(function(k) { return !!permItem(k).decision; });
    if (hasAny) {
      foot.appendChild(permButton('Reset permissions', 'pp-link', function() {
        permSend({ op: 'reset' });
        permNeedsReload = true;
      }));
    } else {
      var note = document.createElement('span');
      note.className = 'pp-note';
      note.textContent = 'The site asks before using these.';
      foot.appendChild(note);
    }
    if (permNeedsReload) {
      foot.appendChild(permButton('Reload to apply', 'pp-btn primary', function() {
        window.ipc.postMessage(JSON.stringify({ type: 'perm_reload' }));
        closePermPop();
      }));
    }
    permPop.append(head.el, rows, foot);
  }

  // Left-aligned under the lock when opened from it (or once the chip has
  // gone); otherwise right-aligned under the chip.
  function positionPermPop() {
    var w = permPop.offsetWidth;
    var r, left;
    if (permAnchor === lockEl || permChip.classList.contains('hidden')) {
      r = lockEl.getBoundingClientRect();
      left = Math.min(r.left - 6, window.innerWidth - w - 8);
    } else {
      r = permChip.getBoundingClientRect();
      left = Math.min(r.right - w, window.innerWidth - w - 8);
    }
    permPop.style.left = Math.round(Math.max(8, left)) + 'px';
    permPop.style.top = Math.round(r.bottom + 6) + 'px';
  }

  function sendPermHeight(h) {
    if (h === permSentHeight) return;
    permSentHeight = h;
    window.ipc.postMessage(JSON.stringify({ type: 'perm_popover', height: h }));
  }

  function renderPermPop() {
    if (!permOpen || !permState) return;
    if (permMode === 'prompt' && !permState.prompt) permMode = 'manage';
    permPop.replaceChildren();
    if (permMode === 'prompt') renderPermPrompt(); else renderPermManage();
    positionPermPop();
    var r = permPop.getBoundingClientRect();
    sendPermHeight(Math.ceil(r.bottom + 14) - 32);
  }

  function openPermPop(mode, anchor) {
    if (!permState) return;
    if (editing) exitEdit();
    permMode = mode;
    permAnchor = anchor || permChip;
    permOpen = true;
    permPop.classList.add('show');
    permAnchor.setAttribute('aria-expanded', 'true');
    renderPermPop();
  }

  function closePermPop() {
    if (!permOpen) return;
    permOpen = false;
    permPop.classList.remove('show');
    permPop.replaceChildren();
    permChip.setAttribute('aria-expanded', 'false');
    lockEl.setAttribute('aria-expanded', 'false');
    sendPermHeight(0);
    renderPermChip();
  }

  window.__perms = function(state) {
    var prevOrigin = permState && permState.origin;
    permState = state && state.origin ? state : null;
    if (permOpen && permState && permState.origin !== prevOrigin) closePermPop();
    renderPermChip();
    if (!permState) return;
    var p = permState.prompt;
    // A new request opens the popover — without taking keyboard focus from
    // the page; one that arrives mid URL-edit waits on the pulsing chip.
    if (p && p.id !== permShownPrompt && !editing) {
      permShownPrompt = p.id;
      openPermPop('prompt');
    } else if (permOpen) {
      if (permMode === 'prompt' && !p) closePermPop(); else renderPermPop();
    }
  };

  // A second click on the same anchor closes; the other anchor re-opens.
  function togglePermPop(anchor) {
    if (permOpen && permAnchor === anchor) { closePermPop(); return; }
    closePermPop();
    openPermPop(permState && permState.prompt ? 'prompt' : 'manage', anchor);
    var first = permPop.querySelector('.pp-btn.primary, .seg button, .pp-btn');
    if (first) first.focus();
  }

  permChip.addEventListener('click', function(e) {
    e.stopPropagation();
    togglePermPop(permChip);
  });

  // The lock sits inside the address row, whose click starts URL editing.
  lockEl.addEventListener('click', function(e) {
    e.stopPropagation();
    togglePermPop(lockEl);
  });

  // Clicks in the grown, transparent part of the bar close the popover; an
  // unanswered request stays on the pulsing chip.
  document.addEventListener('mousedown', function(e) {
    if (!permOpen) return;
    if (permPop.contains(e.target) || permChip.contains(e.target) || lockEl.contains(e.target)) return;
    closePermPop();
  });

  document.addEventListener('keydown', function(e) {
    if (permOpen && e.key === 'Escape') {
      e.preventDefault();
      closePermPop();
      permAnchor.focus();
    }
  });

  window.addEventListener('resize', function() { if (permOpen) positionPermPop(); });

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
  };

  // The next meeting (spec 002-calendar NXT-3): its countdown, and Join.
  var meetChip = document.getElementById('meet-chip');
  var meeting = null;
  function renderMeeting() {
    if (!meeting) { meetChip.classList.add('hidden'); return; }
    var now = Date.now() / 1000;
    if (now >= meeting.end) { meeting = null; meetChip.classList.add('hidden'); return; }
    var mins = Math.ceil((meeting.start - now) / 60);
    var started = now >= meeting.start;
    var when = started ? 'now' : mins <= 1 ? 'in 1 min' : mins < 60 ? 'in ' + mins + ' min' : 'in ' + Math.floor(mins / 60) + ' h ' + (mins % 60) + ' min';
    meetChip.querySelector('.mt').textContent = meeting.title;
    meetChip.querySelector('.mw').textContent = (meeting.join ? (started ? '· Join now' : '· ' + when + ' · Join') : '· ' + when);
    meetChip.classList.toggle('now', started);
    meetChip.title = meeting.join ? 'Join ' + meeting.title + ' (' + meeting.join + ')' : meeting.title;
    meetChip.classList.remove('hidden');
  }
  window.__setMeeting = function(m) { meeting = m; renderMeeting(); };
  setInterval(renderMeeting, 15000);
  meetChip.addEventListener('click', function() {
    window.ipc.postMessage(JSON.stringify({ type: meeting && meeting.join ? 'join_meeting' : 'show_calendar' }));
  });

  // Unread Important mail in this workspace, on the dot.
  window.__setMailUnread = function(n) {
    var dot = document.getElementById('ws-dot');
    if (!dot) return;
    dot.textContent = n > 0 ? (n > 99 ? '99+' : String(n)) : '';
    dot.classList.toggle('count', n > 0);
    document.getElementById('workspace-btn').title = n > 0 ? 'Workspaces · ' + n + ' unread' : 'Workspaces';
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

  // Extension action buttons — pushed by Rust when an action, or the tab it
  // stands for, changes. Labels and badges come from extensions: text only.
  var extBar = document.getElementById('ext-bar');
  window.__setExtensionButtons = function(buttons) {
    extBar.textContent = '';
    buttons.forEach(function(b) {
      var btn = document.createElement('button');
      btn.className = 'bar-btn ext-btn';
      btn.type = 'button';
      btn.title = b.label;
      btn.setAttribute('aria-label', b.label);
      btn.disabled = !b.enabled;
      btn.dataset.id = b.id;
      if (b.icon) {
        var img = document.createElement('img');
        img.src = b.icon;
        img.alt = '';
        btn.appendChild(img);
      } else {
        var letter = document.createElement('span');
        letter.className = 'ext-letter';
        letter.textContent = (b.label || '?').charAt(0).toUpperCase();
        btn.appendChild(letter);
      }
      if (b.badge) {
        var badge = document.createElement('span');
        badge.className = 'ext-badge';
        badge.textContent = b.badge;
        btn.appendChild(badge);
      }
      extBar.appendChild(btn);
    });
  };
  extBar.addEventListener('click', function(e) {
    var btn = e.target.closest('.ext-btn');
    if (!btn || btn.disabled) return;
    var r = btn.getBoundingClientRect();
    window.ipc.postMessage(JSON.stringify({
      type: 'ext_action', id: btn.dataset.id, x: r.left, y: r.top, w: r.width, h: r.height
    }));
  });

  // Badge API
  window.__setSidebarOpen = function(open) {
    const button = document.getElementById('ai-btn');
    button.classList.toggle('active', !!open);
    button.setAttribute('aria-pressed', String(!!open));
  };

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
    if (e.target.closest('#ai-btn, .bar-btn, #title-row, #url-row, #url-input, #url-suggest, #perm-chip, #perm-pop, #meet-chip')) return;
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
		.replace("@@ICON_OCTOPUS_BRAND@@", crate::icons::OCTOPUS_BRAND)
		.replace("@@ICON_LOCK@@", crate::icons::LOCK)
		.replace("@@ICON_SHIELD_ALERT@@", crate::icons::SHIELD_ALERT)
		.replace("@@ICON_LAYERS@@", crate::icons::LAYERS)
		.replace("@@ICON_VIDEO@@", crate::icons::VIDEO)
		.replace("@@ICON_VIDEO_OFF@@", crate::icons::VIDEO_OFF)
		.replace("@@ICON_MIC@@", crate::icons::MIC)
		.replace("@@ICON_MIC_OFF@@", crate::icons::MIC_OFF)
		.replace("@@ICON_MAP_PIN@@", crate::icons::MAP_PIN)
		.replace("@@ICON_MAP_PIN_OFF@@", crate::icons::MAP_PIN_OFF)
}
