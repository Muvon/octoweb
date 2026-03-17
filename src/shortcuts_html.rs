/// Keyboard shortcuts help overlay — compact 2-column frosted glass panel.
///
/// Shown via `⌘/` or the `?` button in the address bar.
/// Two columns: Global (left) and Command Palette (right).
/// IPC: `{ type: "shortcuts_close" }` on Esc or backdrop click.
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
    gap: 24px;
  }

  .col {
    min-width: 0;
  }

  .col-title {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--text-dim);
    margin-bottom: 8px;
  }

  .shortcuts {
    background: var(--section-bg);
    border: 0.5px solid var(--divider);
    border-radius: 8px;
    overflow: hidden;
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 5px 10px;
    gap: 12px;
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
      <!-- Global -->
      <div class="col">
        <div class="col-title">Global</div>
        <div class="shortcuts">
          <div class="row"><span class="row-label">Command palette</span><span class="keys"><kbd>⌘</kbd><kbd>K</kbd></span></div>
          <div class="row"><span class="row-label">Close tab</span><span class="keys"><kbd>⌘</kbd><kbd>W</kbd></span></div>
          <div class="row"><span class="row-label">Reload</span><span class="keys"><kbd>⌘</kbd><kbd>R</kbd></span></div>
          <div class="row"><span class="row-label">Quit</span><span class="keys"><kbd>⌘</kbd><kbd>Q</kbd></span></div>
          <div class="row"><span class="row-label">AI sidebar</span><span class="keys"><kbd>⌘</kbd><kbd>⇧</kbd><kbd>A</kbd></span></div>
          <div class="row"><span class="row-label">DevTools</span><span class="keys"><kbd>⌘</kbd><kbd>⇧</kbd><kbd>I</kbd></span></div>
          <div class="row"><span class="row-label">Shortcuts</span><span class="keys"><kbd>⌘</kbd><kbd>/</kbd></span></div>
          <div class="row"><span class="row-label">Next tab</span><span class="keys"><kbd>⌃</kbd><kbd>N</kbd></span></div>
          <div class="row"><span class="row-label">Prev tab</span><span class="keys"><kbd>⌃</kbd><kbd>P</kbd></span></div>
          <div class="row"><span class="row-label">Open slot 1–9</span><span class="keys"><kbd>⌘</kbd><kbd>1</kbd>–<kbd>9</kbd></span></div>
          <div class="row"><span class="row-label">Save to slot</span><span class="keys"><kbd>⌘</kbd><kbd>⇧</kbd><kbd>1</kbd>–<kbd>9</kbd></span></div>
        </div>
      </div>
      <!-- Command Palette -->
      <div class="col">
        <div class="col-title">Command Palette <span style="opacity:0.5">⌘K</span></div>
        <div class="shortcuts">
          <div class="row"><span class="row-label">Navigate</span><span class="keys"><kbd>↑</kbd><kbd>↓</kbd></span></div>
          <div class="row"><span class="row-label">Confirm</span><span class="keys"><kbd>↵</kbd></span></div>
          <div class="row"><span class="row-label">Force open</span><span class="keys"><kbd>⌘</kbd><kbd>↵</kbd></span></div>
          <div class="row"><span class="row-label">Ask AI</span><span class="keys"><kbd>⌘</kbd><kbd>⇧</kbd><kbd>↵</kbd></span></div>
          <div class="row"><span class="row-label">Jump 1–9</span><span class="keys"><kbd>⌘</kbd><kbd>1</kbd>–<kbd>9</kbd></span></div>
          <div class="row"><span class="row-label">Close</span><span class="keys"><kbd>Esc</kbd></span></div>
          <div class="row"><span class="row-label">Close tab</span><span class="keys"><kbd>⌘</kbd><kbd>W</kbd></span></div>
          <div class="row"><span class="row-label">Start / end</span><span class="keys"><kbd>⌃</kbd><kbd>A</kbd>/<kbd>E</kbd></span></div>
          <div class="row"><span class="row-label">Delete line</span><span class="keys"><kbd>⌃</kbd><kbd>K</kbd>/<kbd>U</kbd></span></div>
          <div class="row"><span class="row-label">Paste</span><span class="keys"><kbd>⌘</kbd><kbd>V</kbd></span></div>
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
})();
</script>
</body>
</html>"#
}
