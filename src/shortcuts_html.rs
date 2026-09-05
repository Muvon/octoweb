/// Keyboard shortcuts help overlay — compact three-column frosted glass panel.
///
/// Shown via `⌘/` or the `?` button in the address bar.
/// IPC: `{ type: "shortcuts_close" }` on Esc or backdrop click.
///
/// Each column owns an independent list so separators stop at its final row.
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
    background: rgba(0, 0, 0, 0.10);
    display: flex;
    align-items: center;
    justify-content: center;
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
    font: 600 17px/1.2 var(--font-display);
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
    align-items: flex-start;
    gap: 10px;
  }

  .shortcut-column {
    flex: 1;
    min-width: 0;
  }

  .col-title {
    font: 600 13px/1.2 var(--font-display);
    color: var(--label);
    padding: 0 6px 8px;
  }

  .shortcut-list {
    background: var(--glass-thin);
    box-shadow: 0 0 0 0.5px var(--hairline), var(--glass-shine);
    border-radius: var(--r-card);
    overflow: hidden;
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
    font-size: 13px;
    color: var(--label);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
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
    font-size: 11px;
  }
</style>
</head>
<body>
<div id="backdrop">
  <div id="panel" class="glass-panel" role="dialog" aria-modal="true" aria-labelledby="title">
    <div id="header">
      <span id="title">Keyboard shortcuts</span>
      <button id="close-btn" title="Close (Esc)" aria-label="Close keyboard shortcuts">
        <svg width="10" height="10" viewBox="0 0 10 10" fill="none">
          <line x1="2" y1="2" x2="8" y2="8" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>
          <line x1="8" y1="2" x2="2" y2="8" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>
        </svg>
        <span class="kbd">esc</span>
      </button>
    </div>
    <div id="columns">
      <!-- Each column has its own list so short columns never render blank stripes. -->
      <div class="shortcut-column">
        <div class="col-title">Global</div>
        <div class="shortcut-list" id="global-list"></div>
      </div>
      <div class="shortcut-column">
        <div class="col-title">Command palette <span id="cp-trigger" class="kbd">⌘⇧P</span></div>
        <div class="shortcut-list">
          <div class="row"><span class="row-label">Remove item</span><span class="keys"><kbd class="kbd">⌘</kbd><kbd class="kbd">W</kbd></span></div>
          <div class="row"><span class="row-label">Move down / up</span><span class="keys"><kbd class="kbd">⌃</kbd><kbd class="kbd">N</kbd>/<kbd class="kbd">P</kbd></span></div>
          <div class="row"><span class="row-label">Jump to item</span><span class="keys"><kbd class="kbd">⌘</kbd><kbd class="kbd">1</kbd>–<kbd class="kbd">9</kbd>, <kbd class="kbd">0</kbd></span></div>
          <div class="row"><span class="row-label">Confirm</span><span class="keys"><kbd class="kbd">↵</kbd></span></div>
          <div class="row"><span class="row-label">Open as URL</span><span class="keys"><kbd class="kbd">⌘</kbd><kbd class="kbd">↵</kbd></span></div>
          <div class="row"><span class="row-label">Ask AI</span><span class="keys"><kbd class="kbd">⌘</kbd><kbd class="kbd">⇧</kbd><kbd class="kbd">↵</kbd></span></div>
          <div class="row"><span class="row-label">Close</span><span class="keys"><kbd class="kbd">Esc</kbd></span></div>
          <div class="row"><span class="row-label">Start / end</span><span class="keys"><kbd class="kbd">⌃</kbd><kbd class="kbd">A</kbd>/<kbd class="kbd">E</kbd></span></div>
          <div class="row"><span class="row-label">Delete line</span><span class="keys"><kbd class="kbd">⌃</kbd><kbd class="kbd">K</kbd>/<kbd class="kbd">U</kbd></span></div>
        </div>
      </div>
      <div class="shortcut-column">
        <div class="col-title">AI editor <span id="ie-trigger" class="kbd">⌘⇧E</span></div>
        <div class="shortcut-list">
          <div class="row"><span class="row-label">History older / newer</span><span class="keys"><kbd class="kbd">⌃</kbd><kbd class="kbd">P</kbd>/<kbd class="kbd">N</kbd></span></div>
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
    if (e.key === 'Tab') {
      var focusable = Array.prototype.slice.call(document.querySelectorAll('#panel button:not([disabled]), #panel input:not([disabled])'));
      if (!focusable.length) return;
      var first = focusable[0], last = focusable[focusable.length - 1];
      if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus(); }
      else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
    }
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
    html += '<div class="row"><span class="row-label">Open slot 1–9, 0</span>' +
            '<span class="keys"><kbd class="kbd">⌘</kbd><kbd class="kbd">1</kbd>–<kbd class="kbd">9</kbd>, <kbd class="kbd">0</kbd></span></div>';
    html += '<div class="row"><span class="row-label">Save to slot 1–9, 0</span>' +
            '<span class="keys"><kbd class="kbd">⌘</kbd><kbd class="kbd">⇧</kbd><kbd class="kbd">1</kbd>–<kbd class="kbd">9</kbd>, <kbd class="kbd">0</kbd></span></div>';
    list.innerHTML = html;
    // Keep the context-column trigger badges in sync with their global chords.
    var find = function(id) {
      var a = data.actions.filter(function(x) { return x.id === id; })[0];
      return a ? a.keys.join('') : null;
    };
    var cp = find('command_palette'), ie = find('inline_edit');
    if (cp) { document.getElementById('cp-trigger').textContent = cp; }
    if (ie) { document.getElementById('ie-trigger').textContent = ie; }
    requestAnimationFrame(function() { document.getElementById('close-btn').focus(); });
  };
})();
</script>
</body>
</html>"#.replace("/*@@THEME@@*/", crate::theme::CSS)
}
