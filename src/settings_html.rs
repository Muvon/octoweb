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

  @media (prefers-reduced-motion: reduce) {
    *, *::before, *::after { animation: none !important; transition: none !important; }
  }

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
    --toggle-on:   #007aff;
    --toggle-knob: #fff;
    --accent:      #007aff;
    --focus-ring:  rgba(0, 122, 255, 0.18);
    --focus-border: rgba(0, 122, 255, 0.6);
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
      --toggle-on:   #0a84ff;
      --toggle-knob: #fff;
      --accent:      #0a84ff;
      --focus-ring:  rgba(10, 132, 255, 0.22);
      --focus-border: rgba(10, 132, 255, 0.65);
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
    border-radius: 16px;
    box-shadow: 0 0 0 0.5px var(--panel-border),
                0 24px 80px rgba(0,0,0,0.18), 0 2px 12px rgba(0,0,0,0.08),
                inset 0 1px 0 rgba(255,255,255,0.5);
    padding: 16px 20px;
    width: 420px;
    max-height: 80vh;
    overflow-y: auto;
    animation: scaleIn 0.25s cubic-bezier(0.34, 1.56, 0.64, 1);
  }
  @media (prefers-color-scheme: dark) {
    #panel {
      box-shadow: 0 0 0 0.5px var(--panel-border),
                  0 24px 80px rgba(0,0,0,0.4), 0 2px 12px rgba(0,0,0,0.2),
                  inset 0 1px 0 rgba(255,255,255,0.07);
    }
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
    box-shadow: 0 0 0 0.5px var(--divider);
    border-radius: 10px;
    overflow: hidden;
    margin-bottom: 10px;
  }

  .section-title {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-dim);
    padding: 10px 12px 6px;
  }

  code {
    font-family: ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace;
    font-size: 10.5px;
    background: var(--input-bg);
    border: 0.5px solid var(--input-border);
    border-radius: 3px;
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
    border-top: 0.5px solid var(--divider);
  }

  .row-label {
    font-size: 12px;
    color: var(--text);
    white-space: nowrap;
    flex-shrink: 0;
  }

  /* Stacked label + dim hint, used when a setting needs explanation */
  .row.with-hint { align-items: flex-start; padding-top: 9px; padding-bottom: 9px; }
  .row.with-hint .row-label-stack { display: flex; flex-direction: column; gap: 2px; min-width: 0; flex: 1; white-space: normal; }
  .row.with-hint .row-label { white-space: normal; }
  .row-hint {
    font-size: 11px;
    color: var(--text-dim);
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
    color: var(--text);
    background: var(--input-bg);
    border: none;
    border-radius: 7px;
    box-shadow: 0 0 0 0.5px var(--input-border);
    outline: none;
    text-align: right;
    transition: box-shadow 0.12s ease;
  }
  .row input[type="text"]:focus,
  .row input[type="number"]:focus {
    box-shadow: 0 0 0 0.5px var(--focus-border), 0 0 0 2.5px var(--focus-ring);
  }
  .row input[type="number"] { width: 70px; flex: none; }

  .row select {
    flex: none;
    max-width: 170px;
    font-size: 12px;
    font-family: inherit;
    color: var(--text);
    accent-color: var(--accent);
  }

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

  /* Tab bar */
  .tabs {
    display: flex;
    gap: 3px;
    margin-bottom: 14px;
    background: var(--section-bg);
    box-shadow: inset 0 0 0 0.5px var(--divider);
    border-radius: 10px;
    padding: 3px;
  }
  .tab {
    flex: 1;
    text-align: center;
    font-size: 12px;
    font-weight: 500;
    color: var(--text-dim);
    padding: 5px 8px;
    border: none;
    border-radius: 8px;
    background: transparent;
    cursor: pointer;
    transition: background 0.12s, color 0.12s;
  }
  .tab.active {
    background: var(--input-bg);
    color: var(--heading);
    box-shadow: 0 0 0 0.5px var(--divider), 0 1px 3px rgba(0, 0, 0, 0.08);
  }
  .tab-pane { display: none; }
  .tab-pane.active { display: block; }

  /* Keybindings */
  .keys { display: inline-flex; align-items: center; gap: 2px; }
  kbd {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 16px; height: 16px;
    padding: 0 3px;
    font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", sans-serif;
    font-size: 10px;
    font-weight: 500;
    color: var(--text);
    background: var(--input-bg);
    border: 0.5px solid var(--input-border);
    border-radius: 4px;
    white-space: nowrap;
    user-select: none;
  }
  .kb-right { display: flex; align-items: center; gap: 6px; flex-shrink: 0; }
  .kb-bind {
    display: inline-flex;
    align-items: center;
    min-height: 22px;
    padding: 2px 6px;
    background: transparent;
    border: 0.5px solid var(--input-border);
    border-radius: 6px;
    cursor: pointer;
    transition: border-color 0.12s, box-shadow 0.12s;
  }
  .kb-bind:hover { border-color: var(--focus-border); }
  .kb-bind.recording {
    border-color: var(--accent);
    box-shadow: 0 0 0 2px var(--focus-ring);
  }
  .rec-hint { font-size: 11px; color: var(--toggle-on); }
  .kb-reset {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 20px; height: 20px;
    border: none;
    background: transparent;
    color: var(--text-dim);
    cursor: pointer;
    border-radius: 5px;
    font-size: 13px;
    line-height: 1;
  }
  .kb-reset:hover { background: var(--close-hover); color: var(--heading); }
  .kb-error {
    display: none;
    font-size: 11px;
    color: #e5484d;
    padding: 8px 2px 0;
  }
  .kb-error.show { display: block; }
  .kb-footer { display: flex; justify-content: space-between; align-items: center; padding-top: 10px; }
  .kb-note { font-size: 11px; color: var(--text-dim); }
  .kb-reset-all {
    font-size: 11px;
    color: var(--text-dim);
    background: transparent;
    border: 0.5px solid var(--input-border);
    border-radius: 6px;
    padding: 4px 10px;
    cursor: pointer;
  }
  .kb-reset-all:hover { color: var(--heading); border-color: var(--text-dim); }
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
    var keys = a.keys.map(function(k) { return '<kbd>' + esc(k) + '</kbd>'; }).join('');
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
}
