/// Returns the HTML for the settings modal (⌘,).
/// Frosted-glass panel with form controls for all Config fields.
/// IPC messages: settings_close, settings_update.
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
    --section-bg:  rgba(255, 255, 255, 0.55);
    --heading:     rgba(0, 0, 0, 0.82);
    --text:        rgba(0, 0, 0, 0.72);
    --text-dim:    rgba(0, 0, 0, 0.36);
    --input-bg:    rgba(255, 255, 255, 0.75);
    --input-border: rgba(0, 0, 0, 0.10);
    --divider:     rgba(0, 0, 0, 0.06);
    --close-hover: rgba(0, 0, 0, 0.06);
    --toggle-off:  rgba(0, 0, 0, 0.14);
    --toggle-on:   rgba(52, 120, 247, 1);
    --toggle-knob: #fff;
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
      --input-bg:    rgba(255, 255, 255, 0.06);
      --input-border: rgba(255, 255, 255, 0.10);
      --divider:     rgba(255, 255, 255, 0.06);
      --close-hover: rgba(255, 255, 255, 0.08);
      --toggle-off:  rgba(255, 255, 255, 0.18);
      --toggle-on:   rgba(52, 120, 247, 1);
      --toggle-knob: #fff;
    }
  }

  #backdrop {
    position: fixed; inset: 0;
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
    width: 420px;
    max-height: 80vh;
    overflow-y: auto;
    animation: scaleIn 0.15s cubic-bezier(0.16, 1, 0.3, 1);
  }

  #header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 16px;
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
    width: 22px; height: 22px;
    border-radius: 5px;
    border: none;
    background: transparent;
    cursor: pointer;
    color: var(--text-dim);
    transition: background 0.1s, color 0.1s;
  }
  #close-btn:hover { background: var(--close-hover); color: var(--heading); }

  .section {
    background: var(--section-bg);
    border: 0.5px solid var(--divider);
    border-radius: 8px;
    overflow: hidden;
    margin-bottom: 10px;
  }

  .section-title {
    font-size: 10px;
    font-weight: 600;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    color: var(--text-dim);
    padding: 10px 12px 6px;
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 7px 12px;
    gap: 12px;
    min-height: 32px;
  }

  .row + .row {
    border-top: 0.5px solid var(--divider);
  }

  .row-label {
    font-size: 12px;
    color: var(--text);
    white-space: nowrap;
    flex-shrink: 0;
  }

  .row input[type="text"],
  .row input[type="number"] {
    flex: 1;
    min-width: 0;
    height: 24px;
    padding: 0 8px;
    font-size: 12px;
    font-family: inherit;
    color: var(--text);
    background: var(--input-bg);
    border: 0.5px solid var(--input-border);
    border-radius: 5px;
    outline: none;
    text-align: right;
  }
  .row input[type="text"]:focus,
  .row input[type="number"]:focus {
    border-color: rgba(52, 120, 247, 0.6);
    box-shadow: 0 0 0 2px rgba(52, 120, 247, 0.15);
  }
  .row input[type="number"] { width: 70px; flex: none; }

  /* Toggle switch */
  .toggle {
    position: relative;
    width: 34px; height: 20px;
    background: var(--toggle-off);
    border-radius: 10px;
    cursor: pointer;
    transition: background 0.2s;
    flex-shrink: 0;
    border: none;
    padding: 0;
  }
  .toggle.on { background: var(--toggle-on); }
  .toggle::after {
    content: '';
    position: absolute;
    top: 2px; left: 2px;
    width: 16px; height: 16px;
    background: var(--toggle-knob);
    border-radius: 50%;
    box-shadow: 0 1px 3px rgba(0,0,0,0.15);
    transition: transform 0.2s;
  }
  .toggle.on::after { transform: translateX(14px); }
</style>
</head>
<body>
<div id="backdrop">
  <div id="panel">
    <div id="header">
      <span id="title">Settings</span>
      <button id="close-btn" title="Close">
        <svg width="10" height="10" viewBox="0 0 10 10" fill="none">
          <line x1="2" y1="2" x2="8" y2="8" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>
          <line x1="8" y1="2" x2="2" y2="8" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>
        </svg>
      </button>
    </div>

    <div class="section">
      <div class="section-title">General</div>
      <div class="row">
        <span class="row-label">Home page</span>
        <input type="text" id="home_page" data-key="home_page">
      </div>
      <div class="row">
        <span class="row-label">Search engine</span>
        <input type="text" id="search_engine" data-key="search_engine">
      </div>
      <div class="row">
        <span class="row-label">Max history</span>
        <input type="number" id="max_history" data-key="max_history" min="100" step="100">
      </div>
    </div>

    <div class="section">
      <div class="section-title">Window</div>
      <div class="row">
        <span class="row-label">Default width</span>
        <input type="number" id="window_width" data-key="window_width" min="400" step="10">
      </div>
      <div class="row">
        <span class="row-label">Default height</span>
        <input type="number" id="window_height" data-key="window_height" min="300" step="10">
      </div>
    </div>

    <div class="section">
      <div class="section-title">AI</div>
      <div class="row">
        <span class="row-label">Auto-hide edit modal</span>
        <button class="toggle" id="ai_edit_auto_hide" data-key="ai_edit_auto_hide"></button>
      </div>
    </div>
  </div>
</div>
<script>
(function() {
  function ipc(msg) {
    window.ipc.postMessage(JSON.stringify(msg));
  }

  function close() {
    ipc({ type: 'settings_close' });
  }

  document.getElementById('backdrop').addEventListener('mousedown', function(e) {
    if (e.target === this) close();
  });
  document.getElementById('close-btn').addEventListener('click', close);
  document.addEventListener('keydown', function(e) {
    if (e.key === 'Escape') { e.preventDefault(); close(); }
  });

  // Send update on change for text/number inputs
  document.querySelectorAll('input[data-key]').forEach(function(el) {
    el.addEventListener('change', function() {
      ipc({ type: 'settings_update', key: el.dataset.key, value: el.value });
    });
  });

  // Toggle switches
  document.querySelectorAll('.toggle[data-key]').forEach(function(el) {
    el.addEventListener('click', function() {
      var on = !el.classList.contains('on');
      el.classList.toggle('on', on);
      ipc({ type: 'settings_update', key: el.dataset.key, value: on ? 'true' : 'false' });
    });
  });

  // Populate fields from Rust
  window.__setConfig = function(cfg) {
    for (var key in cfg) {
      var el = document.getElementById(key);
      if (!el) continue;
      if (el.classList.contains('toggle')) {
        el.classList.toggle('on', cfg[key] === true || cfg[key] === 'true');
      } else {
        el.value = cfg[key];
      }
    }
  };
})();
</script>
</body>
</html>"#
}
