/// Keyboard shortcuts help overlay — compact 3-column frosted glass panel.
///
/// Shown via `⌘/` or the `?` button in the address bar.
/// IPC: `{ type: "shortcuts_close" }` on Esc or backdrop click.
///
/// ## Layout rules
///
/// Three columns inside a single `.shortcuts` flex box: **Global** (left),
/// **Command Palette** (middle), and **AI Editor** (right).
/// All columns are `flex: 1` — always equal width.
///
/// **Row alignment rule:**
/// - If the same key binding has an action in BOTH columns, place it on the
///   SAME row index in both `.shortcuts-col` lists so they visually align.
/// - If a key only exists in one column, add it after the shared rows —
///   columns may have different lengths, no placeholder rows needed.
///
/// **Ordering convention:**
/// 1. Shared-key rows first (both columns filled on the same row).
/// 2. Column-specific rows after — each column lists its own extras.
///
/// Currently shared: `⌘W` (row 1), `⌃N/P` (row 2), `⌘1–9` (row 3).
/// When adding a new shortcut that exists in both contexts, insert it in the
/// shared block at the top and keep both column lists in sync by row position.
///
/// **Compactness rule:** related pairs share a row with `/` separator
/// (e.g. "Scroll ↕" for ⌃D/⌃U). Keeps the panel tight.
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
    --backdrop:    rgba(0, 0, 0, 0.25);
    --panel-bg:    rgba(246, 246, 246, 0.92);
    --panel-border: rgba(0, 0, 0, 0.08);
    --section-bg:  rgba(255, 255, 255, 0.65);
    --heading:     rgba(0, 0, 0, 0.82);
    --text:        rgba(0, 0, 0, 0.72);
    --text-dim:    rgba(0, 0, 0, 0.36);
    --kbd-bg:      rgba(255, 255, 255, 0.85);
    --kbd-border:  rgba(0, 0, 0, 0.12);
    --kbd-shadow:  rgba(0, 0, 0, 0.06);
    --divider:     rgba(0, 0, 0, 0.06);
    --close-hover: rgba(0, 0, 0, 0.06);
  }

  @media (prefers-color-scheme: dark) {
    :root {
      --backdrop:    rgba(0, 0, 0, 0.45);
      --panel-bg:    rgba(40, 40, 40, 0.92);
      --panel-border: rgba(255, 255, 255, 0.08);
      --section-bg:  rgba(255, 255, 255, 0.04);
      --heading:     rgba(255, 255, 255, 0.88);
      --text:        rgba(255, 255, 255, 0.72);
      --text-dim:    rgba(255, 255, 255, 0.30);
      --kbd-bg:      rgba(255, 255, 255, 0.08);
      --kbd-border:  rgba(255, 255, 255, 0.10);
      --kbd-shadow:  rgba(0, 0, 0, 0.25);
      --divider:     rgba(255, 255, 255, 0.06);
      --close-hover: rgba(255, 255, 255, 0.08);
    }
  }

  #backdrop {
    position: fixed;
    inset: 0;
    background: var(--backdrop);
    display: flex;
    align-items: center;
    justify-content: center;
    -webkit-backdrop-filter: blur(8px);
    backdrop-filter: blur(8px);
    animation: fadeIn 0.12s ease;
  }

  @keyframes fadeIn { from { opacity: 0; } to { opacity: 1; } }
  @keyframes scaleIn { from { opacity: 0; transform: scale(0.96); } to { opacity: 1; transform: none; } }

  #panel {
    background: var(--panel-bg);
    border: 0.5px solid var(--panel-border);
    border-radius: 12px;
    box-shadow: 0 24px 80px rgba(0,0,0,0.18), 0 2px 12px rgba(0,0,0,0.08);
    padding: 16px 20px;
    max-height: 88vh;
    overflow-y: auto;
    animation: scaleIn 0.15s cubic-bezier(0.16, 1, 0.3, 1);
  }

  #header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 14px;
  }

  #title {
    font-size: 13px;
    font-weight: 600;
    color: var(--heading);
    letter-spacing: -0.01em;
  }

  #close-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 22px;
    height: 22px;
    border-radius: 5px;
    border: none;
    background: transparent;
    cursor: pointer;
    color: var(--text-dim);
    transition: background 0.1s, color 0.1s;
  }
  #close-btn:hover { background: var(--close-hover); color: var(--heading); }

  #columns {
    display: flex;
    gap: 0;
  }

  .col {
    flex: 1;
    min-width: 0;
  }

  .col + .col {
    border-left: 0.5px solid var(--divider);
  }

  .col-title {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--text-dim);
    padding: 0 10px 8px;
  }

  .shortcuts {
    background: var(--section-bg);
    border: 0.5px solid var(--divider);
    border-radius: 8px;
    overflow: hidden;
    display: flex;
  }

  .shortcuts-col {
    flex: 1;
    min-width: 0;
  }

  .shortcuts-col + .shortcuts-col {
    border-left: 0.5px solid var(--divider);
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 4px 10px;
    gap: 12px;
    min-height: 26px;
  }

  .row + .row {
    border-top: 0.5px solid var(--divider);
  }

  .row-label {
    font-size: 11px;
    color: var(--text);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .row-label.dim {
    color: var(--text-dim);
  }

  .keys {
    display: flex;
    align-items: center;
    gap: 2px;
    flex-shrink: 0;
  }

  kbd {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 18px;
    height: 18px;
    padding: 0 4px;
    font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", sans-serif;
    font-size: 10px;
    font-weight: 500;
    color: var(--text);
    background: var(--kbd-bg);
    border: 0.5px solid var(--kbd-border);
    border-radius: 4px;
    box-shadow: 0 1px 0 var(--kbd-shadow);
    white-space: nowrap;
    user-select: none;
  }
</style>
</head>
<body>
<div id="backdrop">
  <div id="panel">
    <div id="header">
      <span id="title">Keyboard Shortcuts</span>
      <button id="close-btn" title="Close">
        <svg width="10" height="10" viewBox="0 0 10 10" fill="none">
          <line x1="2" y1="2" x2="8" y2="8" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>
          <line x1="8" y1="2" x2="2" y2="8" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>
        </svg>
      </button>
    </div>
    <div id="columns">
      <div class="shortcuts">
        <!-- Left column: Global — populated live from the keymap via __setShortcuts -->
        <div class="shortcuts-col">
          <div class="col-title">Global</div>
          <div id="global-list"></div>
        </div>
        <!-- Middle column: Command Palette — shared-key rows aligned to left column -->
        <div class="shortcuts-col">
          <div class="col-title">Command Palette <span id="cp-trigger" style="opacity:0.5">⌘⇧P</span></div>
          <div class="row"><span class="row-label">Remove item</span><span class="keys"><kbd>⌘</kbd><kbd>W</kbd></span></div>
          <div class="row"><span class="row-label">Move down / up</span><span class="keys"><kbd>⌃</kbd><kbd>N</kbd>/<kbd>P</kbd></span></div>
          <div class="row"><span class="row-label">Jump to item</span><span class="keys"><kbd>⌘</kbd><kbd>1</kbd>–<kbd>9</kbd></span></div>
          <div class="row"><span class="row-label">Confirm</span><span class="keys"><kbd>↵</kbd></span></div>
          <div class="row"><span class="row-label">Force open</span><span class="keys"><kbd>⌘</kbd><kbd>↵</kbd></span></div>
          <div class="row"><span class="row-label">Ask AI</span><span class="keys"><kbd>⌘</kbd><kbd>⇧</kbd><kbd>↵</kbd></span></div>
          <div class="row"><span class="row-label">Close</span><span class="keys"><kbd>Esc</kbd></span></div>
          <div class="row"><span class="row-label">Start / end</span><span class="keys"><kbd>⌃</kbd><kbd>A</kbd>/<kbd>E</kbd></span></div>
          <div class="row"><span class="row-label">Delete line</span><span class="keys"><kbd>⌃</kbd><kbd>K</kbd>/<kbd>U</kbd></span></div>
        </div>
        <!-- Right column: AI Editor — shared-key rows aligned -->
        <div class="shortcuts-col">
          <div class="col-title">AI Editor <span id="ie-trigger" style="opacity:0.5">⌘⇧E</span></div>
          <div class="row"><span class="row-label dim">&nbsp;</span></div>
          <div class="row"><span class="row-label">History older / newer</span><span class="keys"><kbd>⌃</kbd><kbd>P</kbd>/<kbd>N</kbd></span></div>
          <div class="row"><span class="row-label dim">&nbsp;</span></div>
          <div class="row"><span class="row-label">Submit</span><span class="keys"><kbd>↵</kbd></span></div>
          <div class="row"><span class="row-label">Reverse search</span><span class="keys"><kbd>⌃</kbd><kbd>R</kbd></span></div>
          <div class="row"><span class="row-label">Accept completion</span><span class="keys"><kbd>⌃</kbd><kbd>E</kbd></span></div>
          <div class="row"><span class="row-label">Close</span><span class="keys"><kbd>Esc</kbd></span></div>
          <div class="row"><span class="row-label">Start / end</span><span class="keys"><kbd>⌃</kbd><kbd>A</kbd>/<kbd>E</kbd></span></div>
          <div class="row"><span class="row-label">Erase to start</span><span class="keys"><kbd>⌃</kbd><kbd>U</kbd></span></div>
        </div>
      </div>
    </div>
  </div>
</div>
<script>
(function() {
  function close() {
    window.ipc.postMessage(JSON.stringify({ type: 'shortcuts_close' }));
  }
  document.getElementById('backdrop').addEventListener('mousedown', function(e) {
    if (e.target === this) close();
  });
  document.getElementById('close-btn').addEventListener('click', close);
  document.addEventListener('keydown', function(e) {
    if (e.key === 'Escape') { e.preventDefault(); close(); }
  });

  function esc(s) {
    return String(s).replace(/[&<>]/g, function(c) {
      return { '&': '&amp;', '<': '&lt;', '>': '&gt;' }[c];
    });
  }
  function row(label, keys) {
    var kbds = keys.map(function(k) { return '<kbd>' + esc(k) + '</kbd>'; }).join('');
    return '<div class="row"><span class="row-label">' + esc(label) +
           '</span><span class="keys">' + kbds + '</span></div>';
  }

  // Render the Global column from live keybindings. The quickslot digit family
  // is fixed (not remappable) so it's appended as a static pair.
  window.__setShortcuts = function(data) {
    var list = document.getElementById('global-list');
    if (!list || !data || !data.actions) return;
    var html = data.actions.map(function(a) { return row(a.label, a.keys); }).join('');
    html += '<div class="row"><span class="row-label">Open slot 1–9</span>' +
            '<span class="keys"><kbd>⌘</kbd><kbd>1</kbd>–<kbd>9</kbd></span></div>';
    html += '<div class="row"><span class="row-label">Save to slot 1–9</span>' +
            '<span class="keys"><kbd>⌘</kbd><kbd>⇧</kbd><kbd>1</kbd>–<kbd>9</kbd></span></div>';
    list.innerHTML = html;
    // Keep the context-column trigger badges in sync with their global chords.
    var find = function(id) {
      var a = data.actions.filter(function(x) { return x.id === id; })[0];
      return a ? a.keys.join('') : null;
    };
    var cp = find('command_palette'), ie = find('inline_edit');
    if (cp) { document.getElementById('cp-trigger').textContent = cp; }
    if (ie) { document.getElementById('ie-trigger').textContent = ie; }
  };
})();
</script>
</body>
</html>"#
}
