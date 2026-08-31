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
pub fn html() -> String {
    r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<style>
/*@@THEME@@*/
  *, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }

  html, body {
    width: 100%; height: 100%;
    overflow: hidden;
    background: transparent;
    -webkit-font-smoothing: antialiased;
    font-family: var(--font-text);
  }

  #backdrop {
    position: fixed;
    inset: 0;
    background: color-mix(in srgb, var(--canvas) 28%, rgba(0, 0, 0, 0.52));
    display: flex;
    align-items: center;
    justify-content: center;
    -webkit-backdrop-filter: blur(8px);
    backdrop-filter: blur(8px);
    animation: fadeIn var(--t-fast) var(--ease);
  }

  @keyframes fadeIn { from { opacity: 0; } to { opacity: 1; } }
  @keyframes scaleIn { from { opacity: 0; transform: scale(0.96); } to { opacity: 1; transform: none; } }

  #panel {
    background: var(--glass-thick);
    border-radius: var(--r-panel);
    box-shadow: var(--shadow-float), var(--glass-shine);
    padding: 16px 20px;
    max-height: 88vh;
    overflow-y: auto;
    animation: scaleIn var(--t-pop) var(--spring);
  }

  #header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 14px;
  }

  #title {
    font: 600 14px/1.2 var(--font-display);
    color: var(--label);
    letter-spacing: -0.02em;
  }

  #close-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    min-width: 22px;
    height: 22px;
    border-radius: var(--r-capsule);
    border: none;
    background: transparent;
    cursor: pointer;
    color: var(--label-2);
    gap: 3px;
    padding: 0 4px;
    transition: background var(--t-fast) var(--ease), color var(--t-fast) var(--ease), transform var(--t-fast) var(--ease);
  }
  #close-btn:hover { background: var(--fill-hover); color: var(--label); }
  #close-btn:active { background: var(--fill-press); transform: scale(0.96); }

  #columns {
    display: flex;
    gap: 0;
  }

  .col {
    flex: 1;
    min-width: 0;
  }

  .col + .col {
    border-left: 0.5px solid var(--hairline);
  }

  .col-title {
    font: 600 11px/1.2 var(--font-display);
    color: var(--label-2);
    padding: 0 10px 8px;
  }

  .shortcuts {
    background: var(--glass-thin);
    box-shadow: 0 0 0 0.5px var(--hairline), var(--glass-shine);
    border-radius: var(--r-card);
    overflow: hidden;
    display: flex;
  }

  .shortcuts-col {
    flex: 1;
    min-width: 0;
  }

  .shortcuts-col + .shortcuts-col {
    border-left: 0.5px solid var(--hairline);
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
    border-top: 0.5px solid var(--hairline);
  }

  .row-label {
    font-size: 11px;
    color: var(--label);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .row-label.dim {
    color: var(--label-3);
  }

  .keys {
    display: flex;
    align-items: center;
    gap: 2px;
    flex-shrink: 0;
  }

  kbd.kbd {
    min-width: 18px;
    height: 18px;
    white-space: nowrap;
    user-select: none;
  }
</style>
</head>
<body>
<div id="backdrop">
  <div id="panel" class="glass-panel">
    <div id="header">
      <span id="title">Keyboard Shortcuts</span>
      <button id="close-btn" title="Close (Esc)">
        <svg width="10" height="10" viewBox="0 0 10 10" fill="none">
          <line x1="2" y1="2" x2="8" y2="8" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>
          <line x1="8" y1="2" x2="2" y2="8" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>
        </svg>
        <span class="kbd">esc</span>
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
          <div class="col-title">Command Palette <span id="cp-trigger" class="kbd">⌘⇧P</span></div>
          <div class="row"><span class="row-label">Remove item</span><span class="keys"><kbd class="kbd">⌘</kbd><kbd class="kbd">W</kbd></span></div>
          <div class="row"><span class="row-label">Move down / up</span><span class="keys"><kbd class="kbd">⌃</kbd><kbd class="kbd">N</kbd>/<kbd class="kbd">P</kbd></span></div>
          <div class="row"><span class="row-label">Jump to item</span><span class="keys"><kbd class="kbd">⌘</kbd><kbd class="kbd">1</kbd>–<kbd class="kbd">9</kbd></span></div>
          <div class="row"><span class="row-label">Confirm</span><span class="keys"><kbd class="kbd">↵</kbd></span></div>
          <div class="row"><span class="row-label">Force open</span><span class="keys"><kbd class="kbd">⌘</kbd><kbd class="kbd">↵</kbd></span></div>
          <div class="row"><span class="row-label">Ask AI</span><span class="keys"><kbd class="kbd">⌘</kbd><kbd class="kbd">⇧</kbd><kbd class="kbd">↵</kbd></span></div>
          <div class="row"><span class="row-label">Close</span><span class="keys"><kbd class="kbd">Esc</kbd></span></div>
          <div class="row"><span class="row-label">Start / end</span><span class="keys"><kbd class="kbd">⌃</kbd><kbd class="kbd">A</kbd>/<kbd class="kbd">E</kbd></span></div>
          <div class="row"><span class="row-label">Delete line</span><span class="keys"><kbd class="kbd">⌃</kbd><kbd class="kbd">K</kbd>/<kbd class="kbd">U</kbd></span></div>
        </div>
        <!-- Right column: AI Editor — shared-key rows aligned -->
        <div class="shortcuts-col">
          <div class="col-title">AI Editor <span id="ie-trigger" class="kbd">⌘⇧E</span></div>
          <div class="row"><span class="row-label dim">&nbsp;</span></div>
          <div class="row"><span class="row-label">History older / newer</span><span class="keys"><kbd class="kbd">⌃</kbd><kbd class="kbd">P</kbd>/<kbd class="kbd">N</kbd></span></div>
          <div class="row"><span class="row-label dim">&nbsp;</span></div>
          <div class="row"><span class="row-label">Submit</span><span class="keys"><kbd class="kbd">↵</kbd></span></div>
          <div class="row"><span class="row-label">Reverse search</span><span class="keys"><kbd class="kbd">⌃</kbd><kbd class="kbd">R</kbd></span></div>
          <div class="row"><span class="row-label">Accept completion</span><span class="keys"><kbd class="kbd">⌃</kbd><kbd class="kbd">E</kbd></span></div>
          <div class="row"><span class="row-label">Close</span><span class="keys"><kbd class="kbd">Esc</kbd></span></div>
          <div class="row"><span class="row-label">Start / end</span><span class="keys"><kbd class="kbd">⌃</kbd><kbd class="kbd">A</kbd>/<kbd class="kbd">E</kbd></span></div>
          <div class="row"><span class="row-label">Erase to start</span><span class="keys"><kbd class="kbd">⌃</kbd><kbd class="kbd">U</kbd></span></div>
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
    var kbds = keys.map(function(k) { return '<kbd class="kbd">' + esc(k) + '</kbd>'; }).join('');
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
            '<span class="keys"><kbd class="kbd">⌘</kbd><kbd class="kbd">1</kbd>–<kbd class="kbd">9</kbd></span></div>';
    html += '<div class="row"><span class="row-label">Save to slot 1–9</span>' +
            '<span class="keys"><kbd class="kbd">⌘</kbd><kbd class="kbd">⇧</kbd><kbd class="kbd">1</kbd>–<kbd class="kbd">9</kbd></span></div>';
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
</html>"#.replace("/*@@THEME@@*/", crate::theme::CSS)
}
