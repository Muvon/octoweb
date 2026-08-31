/// Returns the HTML for the settings modal (⌘,).
/// Frosted-glass panel with form controls for all Config fields.
/// IPC messages: settings_close, settings_update.
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
    color: var(--label);
  }

  #backdrop {
    position: fixed; inset: 0;
    background: color-mix(in srgb, var(--label) 18%, transparent);
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
    -webkit-backdrop-filter: var(--glass-blur);
    backdrop-filter: var(--glass-blur);
    border-radius: var(--r-panel);
    box-shadow: var(--shadow-float), var(--glass-shine);
    padding: 16px 20px;
    width: 420px;
    max-height: 80vh;
    overflow-y: auto;
    animation: scaleIn var(--t-pop) var(--spring);
  }

  #header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 16px;
  }

  #title {
    font-family: var(--font-display);
    font-size: 15px;
    font-weight: 600;
    color: var(--label);
    letter-spacing: -0.01em;
  }

  #close-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 22px; height: 22px;
    border-radius: var(--r-capsule);
    border: none;
    background: transparent;
    cursor: pointer;
    color: var(--label-3);
    transition: background var(--t-fast) var(--ease), color var(--t-fast) var(--ease),
                transform var(--t-fast) var(--ease);
  }
  #close-btn:hover { background: var(--fill-hover); color: var(--label); }
  #close-btn:active { background: var(--fill-press); transform: scale(0.9); }

  #dismiss-hint {
    display: flex;
    align-items: center;
    gap: 5px;
    margin-left: auto;
    margin-right: 8px;
    color: var(--label-3);
    font-size: 10px;
  }

  .section {
    background: var(--fill);
    box-shadow: 0 0 0 0.5px var(--hairline);
    border-radius: var(--r-card);
    overflow: hidden;
    margin-bottom: 10px;
  }

  .section-title {
    font-family: var(--font-display);
    font-size: 13px;
    font-weight: 600;
    color: var(--label-2);
    padding: 10px 12px 6px;
  }

  code {
    font-family: var(--font-mono);
    font-size: 10.5px;
    background: var(--fill-hover);
    border: 0.5px solid var(--hairline);
    border-radius: var(--r-ctl);
    padding: 0 4px;
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
    box-shadow: 0 0.5px 0 var(--hairline) inset;
  }

  .row-label {
    font-size: 12px;
    color: var(--label);
    white-space: nowrap;
    flex-shrink: 0;
  }

  /* Stacked label + dim hint, used when a setting needs explanation */
  .row.with-hint { align-items: flex-start; padding-top: 9px; padding-bottom: 9px; }
  .row.with-hint .row-label-stack { display: flex; flex-direction: column; gap: 2px; min-width: 0; flex: 1; white-space: normal; }
  .row.with-hint .row-label { white-space: normal; }
  .row-hint {
    font-size: 11px;
    color: var(--label-2);
    line-height: 1.35;
    max-width: 240px;
  }

  .row input[type="text"],
  .row input[type="number"] {
    flex: 1;
    min-width: 0;
    height: 24px;
    padding: 0 8px;
    font-size: 12px;
    font-family: inherit;
    color: var(--label);
    background: var(--fill);
    border: none;
    border-radius: var(--r-ctl);
    box-shadow: 0 0 0 0.5px var(--hairline);
    outline: none;
    text-align: right;
    transition: background var(--t-fast) var(--ease), box-shadow var(--t-fast) var(--ease);
  }
  .row input[type="text"]:hover,
  .row input[type="number"]:hover { background: var(--fill-hover); }
  .row input[type="text"]:focus,
  .row input[type="number"]:focus {
    background: var(--fill-hover);
    box-shadow: 0 0 0 1px var(--accent), 0 0 0 3px color-mix(in srgb, var(--accent) 22%, transparent);
  }
  .row input[type="number"] { width: 70px; flex: none; }

  .row select {
    flex: none;
    max-width: 170px;
    min-height: 24px;
    padding: 2px 24px 2px 8px;
    font-size: 12px;
    font-family: inherit;
    color: var(--label);
    background-color: var(--fill);
    border: none;
    border-radius: var(--r-ctl);
    box-shadow: 0 0 0 0.5px var(--hairline);
    outline: none;
    accent-color: var(--accent);
    transition: background var(--t-fast) var(--ease), box-shadow var(--t-fast) var(--ease);
  }
  .row select:hover { background-color: var(--fill-hover); }
  .row select:active { background-color: var(--fill-press); }
  .row select:focus { box-shadow: 0 0 0 1px var(--accent), 0 0 0 3px color-mix(in srgb, var(--accent) 22%, transparent); }

  /* Toggle switch */
  .toggle {
    position: relative;
    width: 36px; height: 22px;
    background: var(--fill-press);
    border-radius: var(--r-capsule);
    cursor: pointer;
    transition: background var(--t-fast) var(--ease), transform var(--t-fast) var(--spring);
    flex-shrink: 0;
    border: none;
    padding: 0;
  }
  .toggle:hover { background: var(--fill-hover); }
  .toggle:active { background: var(--fill-press); transform: scale(0.94); }
  .toggle.on { background: var(--accent); }
  .toggle.on:hover { background: color-mix(in srgb, var(--accent) 88%, var(--label)); }
  .toggle::after {
    content: '';
    position: absolute;
    top: 2px; left: 2px;
    width: 18px; height: 18px;
    background: var(--on-accent);
    border-radius: 50%;
    box-shadow: 0 1px 3px color-mix(in srgb, var(--label) 18%, transparent);
    transition: transform var(--t-fast) var(--spring);
  }
  .toggle.on::after { transform: translateX(14px); }

  /* Tab bar */
  .tabs {
    display: flex;
    gap: 3px;
    margin-bottom: 14px;
    background: var(--fill);
    box-shadow: inset 0 0 0 0.5px var(--hairline);
    border-radius: var(--r-capsule);
    padding: 3px;
  }
  .tab {
    flex: 1;
    text-align: center;
    font-size: 12px;
    font-weight: 500;
    color: var(--label-2);
    padding: 5px 8px;
    border: none;
    min-height: 26px;
    border-radius: var(--r-capsule);
    background: transparent;
    cursor: pointer;
    transition: background var(--t-fast) var(--ease), color var(--t-fast) var(--ease),
                transform var(--t-fast) var(--ease);
  }
  .tab:hover { background: var(--fill-hover); color: var(--label); }
  .tab:active { background: var(--fill-press); transform: scale(0.98); }
  .tab.active {
    background: var(--glass-thin);
    color: var(--label);
    box-shadow: 0 0 0 0.5px var(--hairline), var(--glass-shine);
  }
  .tab-pane { display: none; }
  .tab-pane.active { display: block; }

  /* Keybindings */
  .keys { display: inline-flex; align-items: center; gap: 2px; }
  kbd.kbd {
    min-width: 17px;
    min-height: 17px;
    white-space: nowrap;
    user-select: none;
  }
  .kb-right { display: flex; align-items: center; gap: 6px; flex-shrink: 0; }
  .kb-bind {
    display: inline-flex;
    align-items: center;
    min-height: 22px;
    padding: 2px 6px;
    background: var(--fill);
    border: none;
    box-shadow: 0 0 0 0.5px var(--hairline);
    border-radius: var(--r-ctl);
    cursor: pointer;
    transition: background var(--t-fast) var(--ease), box-shadow var(--t-fast) var(--ease),
                transform var(--t-fast) var(--ease);
  }
  .kb-bind:hover { background: var(--fill-hover); }
  .kb-bind:active { background: var(--fill-press); transform: scale(0.98); }
  .kb-bind.recording {
    box-shadow: 0 0 0 1px var(--accent), 0 0 0 3px color-mix(in srgb, var(--accent) 22%, transparent);
  }
  .rec-hint { font-size: 11px; color: var(--accent); }
  .kb-reset {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 22px; height: 22px;
    border: none;
    background: transparent;
    color: var(--label-3);
    cursor: pointer;
    border-radius: var(--r-capsule);
    font-size: 13px;
    line-height: 1;
  }
  .kb-reset:hover { background: var(--fill-hover); color: var(--label); }
  .kb-reset:active { background: var(--fill-press); transform: scale(0.9); }
  .kb-error {
    display: none;
    font-size: 11px;
    color: var(--err);
    padding: 8px 2px 0;
  }
  .kb-error.show { display: block; }
  .kb-footer { display: flex; justify-content: space-between; align-items: center; padding-top: 10px; }
  .kb-note { font-size: 11px; color: var(--label-2); }
  .kb-reset-all {
    min-height: 24px;
    font-size: 11px;
    font-weight: 600;
    color: var(--on-accent);
    background: var(--accent);
    border: none;
    border-radius: var(--r-capsule);
    padding: 4px 10px;
    cursor: pointer;
    transition: filter var(--t-fast) var(--ease), transform var(--t-fast) var(--ease);
  }
  .kb-reset-all:hover { filter: brightness(1.08); }
  .kb-reset-all:active { filter: brightness(0.92); transform: scale(0.96); }
</style>
</head>
<body>
<div id="backdrop">
  <div id="panel" class="glass-panel">
    <div id="header">
      <span id="title">Settings</span>
      <span id="dismiss-hint"><span class="kbd">esc</span> close</span>
      <button id="close-btn" title="Close">
        <svg width="10" height="10" viewBox="0 0 10 10" fill="none">
          <line x1="2" y1="2" x2="8" y2="8" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>
          <line x1="8" y1="2" x2="2" y2="8" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>
        </svg>
      </button>
    </div>

    <div class="tabs">
      <button class="tab active" data-pane="tab-general">General</button>
      <button class="tab" data-pane="tab-keybindings">Keybindings</button>
    </div>

    <div id="tab-general" class="tab-pane active">
    <div class="section">
      <div class="section-title">General</div>
      <div class="row with-hint">
        <div class="row-label-stack">
          <span class="row-label">Home page</span>
          <span class="row-hint">Loads on launch when there's no previous session to restore.</span>
        </div>
        <input type="text" id="home_page" data-key="home_page">
      </div>
      <div class="row with-hint">
        <div class="row-label-stack">
          <span class="row-label">Search engine</span>
          <span class="row-hint">Used when you type a search instead of a URL.</span>
        </div>
        <select id="search_engine_select">
          <option value="https://www.google.com/search?q={}">Google</option>
          <option value="https://duckduckgo.com/?q={}">DuckDuckGo</option>
          <option value="https://www.bing.com/search?q={}">Bing</option>
          <option value="https://search.brave.com/search?q={}">Brave</option>
          <option value="https://www.ecosia.org/search?q={}">Ecosia</option>
          <option value="custom">Custom…</option>
        </select>
      </div>
      <div class="row with-hint" id="search_engine_custom_row" style="display:none">
        <div class="row-label-stack">
          <span class="row-label">Custom search URL</span>
          <span class="row-hint"><code>{}</code> is replaced with your search terms.</span>
        </div>
        <input type="text" id="search_engine" data-key="search_engine" placeholder="https://example.com/search?q={}">
      </div>
      <div class="row with-hint">
        <div class="row-label-stack">
          <span class="row-label">Max history</span>
          <span class="row-hint">Total entries kept across sessions.</span>
        </div>
        <input type="number" id="max_history" data-key="max_history" min="100" step="100">
      </div>
    </div>

    <div class="section">
      <div class="section-title">Window</div>
      <div class="row with-hint">
        <div class="row-label-stack">
          <span class="row-label">Default width</span>
          <span class="row-hint">Initial window width in pixels.</span>
        </div>
        <input type="number" id="window_width" data-key="window_width" min="400" step="10">
      </div>
      <div class="row with-hint">
        <div class="row-label-stack">
          <span class="row-label">Default height</span>
          <span class="row-hint">Initial window height in pixels.</span>
        </div>
        <input type="number" id="window_height" data-key="window_height" min="300" step="10">
      </div>
    </div>

    <div class="section">
      <div class="section-title">Memory</div>
      <div class="row with-hint">
        <div class="row-label-stack">
          <span class="row-label">Aggressive hibernation</span>
          <span class="row-hint">Reclaim tab memory sooner. Leave off on modern Macs — tabs survive longer.</span>
        </div>
        <button class="toggle" id="aggressive_hibernation" data-key="aggressive_hibernation"></button>
      </div>
      <div class="row with-hint">
        <div class="row-label-stack">
          <span class="row-label">Max open tabs</span>
          <span class="row-hint">Least-recently-used tabs above this are closed automatically; pages stay in history. 0 disables.</span>
        </div>
        <input type="number" id="max_tabs" data-key="max_tabs" min="0" step="50">
      </div>
    </div>

    <div class="section">
      <div class="section-title">AI</div>
      <div class="row with-hint">
        <div class="row-label-stack">
          <span class="row-label">Auto-hide edit modal</span>
          <span class="row-hint">Hide the ⌘⇧E modal after submit; show a loading cursor instead.</span>
        </div>
        <button class="toggle" id="ai_edit_auto_hide" data-key="ai_edit_auto_hide"></button>
      </div>
      <div class="row with-hint">
        <div class="row-label-stack">
          <span class="row-label">Editor prompt history size</span>
          <span class="row-hint">Recall via ⌃P / ⌃N inside the ⌘⇧E modal.</span>
        </div>
        <input type="number" id="max_prompt_history" data-key="max_prompt_history" min="10" step="10">
      </div>
      <div class="row with-hint">
        <div class="row-label-stack">
          <span class="row-label">Assistant prompt history size</span>
          <span class="row-hint">Sidebar (⌘⇧A) prompt recall depth.</span>
        </div>
        <input type="number" id="max_ai_prompt_history" data-key="max_ai_prompt_history" min="10" step="10">
      </div>
      <div class="row with-hint">
        <div class="row-label-stack">
          <span class="row-label">Proactive learning</span>
          <span class="row-hint">Background agent reads recent browsing and memorizes patterns.</span>
        </div>
        <button class="toggle" id="proactive_learning" data-key="proactive_learning"></button>
      </div>
      <div class="row with-hint">
        <div class="row-label-stack">
          <span class="row-label">Learning interval (min)</span>
          <span class="row-hint">How often the learning agent runs.</span>
        </div>
        <input type="number" id="learning_interval_min" data-key="learning_interval_min" min="5" step="5">
      </div>
    </div>
    </div><!-- /tab-general -->

    <div id="tab-keybindings" class="tab-pane">
      <div id="kb-groups"></div>
      <div id="kb-error" class="kb-error"></div>
      <div class="kb-footer">
        <span class="kb-note">Click a shortcut, then press the new combination.</span>
        <button class="kb-reset-all" id="kb-reset-all">Reset all</button>
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
    cancelRecord();
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
    syncSearchEngine();
  };

  // ── Search engine presets ─────────────────────────────────────────────
  // The select holds full URL templates; "custom" reveals the raw URL input.
  var seSelect    = document.getElementById('search_engine_select');
  var seCustomRow = document.getElementById('search_engine_custom_row');
  var seInput     = document.getElementById('search_engine');

  function syncSearchEngine() {
    var tpl = seInput.value;
    var preset = Array.prototype.some.call(seSelect.options, function(o) {
      return o.value === tpl;
    });
    seSelect.value = preset ? tpl : 'custom';
    seCustomRow.style.display = preset ? 'none' : '';
  }

  seSelect.addEventListener('change', function() {
    if (seSelect.value === 'custom') {
      seCustomRow.style.display = '';
      seInput.focus();
      return;
    }
    seCustomRow.style.display = 'none';
    seInput.value = seSelect.value;
    ipc({ type: 'settings_update', key: 'search_engine', value: seSelect.value });
  });

  // ── Tabs ──────────────────────────────────────────────────────────────
  document.querySelectorAll('.tab').forEach(function(t) {
    t.addEventListener('click', function() {
      document.querySelectorAll('.tab').forEach(function(x) { x.classList.remove('active'); });
      document.querySelectorAll('.tab-pane').forEach(function(x) { x.classList.remove('active'); });
      t.classList.add('active');
      document.getElementById(t.dataset.pane).classList.add('active');
      cancelRecord();
    });
  });

  // ── Keybindings ───────────────────────────────────────────────────────
  function esc(s) {
    return String(s).replace(/[&<>"]/g, function(c) {
      return { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c];
    });
  }

  // JS KeyboardEvent.code → the physical key token the Rust keymap understands.
  function codeToToken(code) {
    if (/^Key[A-Z]$/.test(code)) return code.slice(3).toLowerCase();
    if (/^Digit[0-9]$/.test(code)) return code.slice(5);
    var m = {
      Minus: 'minus', Equal: 'equal', BracketLeft: 'bracketleft', BracketRight: 'bracketright',
      Backslash: 'backslash', Semicolon: 'semicolon', Quote: 'quote', Comma: 'comma',
      Period: 'period', Slash: 'slash', Backquote: 'backquote', Enter: 'return',
      Space: 'space', Tab: 'tab', Escape: 'escape',
      ArrowLeft: 'left', ArrowRight: 'right', ArrowUp: 'up', ArrowDown: 'down'
    };
    return m[code] || null;
  }

  var lastData = null;     // most recent bindings snapshot, for cancel re-render
  var recordingId = null;  // action id currently capturing a chord
  var recordingBtn = null;

  function cancelRecord() {
    if (recordingId) {
      ipc({ type: 'keybind_capture', on: false });
      if (lastData) render(lastData);
    }
  }

  function startRecord(id, btn) {
    if (recordingBtn && recordingBtn !== btn) recordingBtn.classList.remove('recording');
    recordingId = id;
    recordingBtn = btn;
    btn.classList.add('recording');
    btn.querySelector('.keys').innerHTML = '<span class="rec-hint">Press keys…</span>';
    // Tell the host to stop firing global shortcuts so we can capture the chord.
    ipc({ type: 'keybind_capture', on: true });
  }

  function kbRow(a) {
    var keys = a.keys.map(function(k) { return '<kbd class="kbd">' + esc(k) + '</kbd>'; }).join('');
    var reset = a.is_default ? '' :
      '<button class="kb-reset" data-reset="' + esc(a.id) + '" title="Reset to default">↺</button>';
    return '<div class="row">' +
      '<span class="row-label">' + esc(a.label) + '</span>' +
      '<span class="kb-right">' + reset +
        '<button class="kb-bind" data-bind="' + esc(a.id) + '"><span class="keys">' + keys + '</span></button>' +
      '</span></div>';
  }

  function render(data) {
    lastData = data;
    recordingId = null;
    recordingBtn = null;
    var groups = {}, order = [];
    data.actions.forEach(function(a) {
      if (!groups[a.group]) { groups[a.group] = []; order.push(a.group); }
      groups[a.group].push(a);
    });
    document.getElementById('kb-groups').innerHTML = order.map(function(g) {
      return '<div class="section"><div class="section-title">' + esc(g) + '</div>' +
        groups[g].map(kbRow).join('') + '</div>';
    }).join('');
    var err = document.getElementById('kb-error');
    if (data.error) { err.textContent = data.error; err.classList.add('show'); }
    else { err.textContent = ''; err.classList.remove('show'); }
  }

  // Rust pushes the full binding set on open and after every edit.
  window.__setKeybindings = function(data) {
    if (!data || !data.actions) return;
    render(data);
  };

  // Delegate clicks: start recording on a binding, reset on the ↺ button.
  document.getElementById('kb-groups').addEventListener('click', function(e) {
    var reset = e.target.closest('[data-reset]');
    if (reset) {
      cancelRecord();
      ipc({ type: 'keybind_reset', action: reset.dataset.reset });
      return;
    }
    var bind = e.target.closest('[data-bind]');
    if (bind) {
      var id = bind.dataset.bind;
      if (recordingId) cancelRecord(); // restores the previously recording row
      var fresh = document.querySelector('.kb-bind[data-bind="' + id + '"]');
      startRecord(id, fresh || bind);
    }
  });

  document.getElementById('kb-reset-all').addEventListener('click', function() {
    ipc({ type: 'keybind_reset_all' });
  });

  // Capture phase so a recording keystroke is intercepted before the modal's
  // Esc-to-close handler (which lives on the bubble phase) ever sees it.
  document.addEventListener('keydown', function(e) {
    if (!recordingId) return;
    e.preventDefault();
    e.stopPropagation();
    // Wait through bare modifier presses for the real key.
    if (['Meta', 'Shift', 'Control', 'Alt'].indexOf(e.key) >= 0) return;
    // Esc with no modifiers cancels the capture.
    if (e.code === 'Escape' && !e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey) {
      cancelRecord();
      return;
    }
    var token = codeToToken(e.code);
    if (!token) return; // unsupported physical key — keep waiting
    var mods = [];
    if (e.metaKey) mods.push('cmd');
    if (e.ctrlKey) mods.push('ctrl');
    if (e.altKey) mods.push('opt');
    if (e.shiftKey) mods.push('shift');
    var id = recordingId;
    recordingId = null;
    recordingBtn = null;
    ipc({ type: 'keybind_record', action: id, chord: mods.concat([token]).join('+') });
    ipc({ type: 'keybind_capture', on: false });
  }, true);
})();
</script>
</body>
</html>"#
    .replace("/*@@THEME@@*/", crate::theme::CSS)
}
