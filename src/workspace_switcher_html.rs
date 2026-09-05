/// Returns the HTML for the workspace switcher popover (⌘⇧O).
/// Compact card anchored under the toolbar — not a full-screen modal like settings.
/// IPC messages: workspace_switch, workspace_jump_tab, workspace_rename, workspace_delete, workspace_create, workspace_close.
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
    position: fixed;
    inset: 0;
    background: transparent;
  }

  #panel {
    position: absolute;
    top: 50px;
    right: 12px;
    width: 300px;
    /* This popover floats in its own transparent window, so backdrop-filter has
       nothing to sample — the page behind belongs to a different WebView. The
       thin --glass fill alone left the panel see-through, and how much showed
       through varied with WebKit's compositing. --glass-thick is opaque enough
       to be deterministic. */
    background: var(--glass-thick);
    max-height: 70vh;
    overflow-y: auto;
    padding: 8px;
    animation: scaleIn var(--t-pop) var(--spring);
  }

  @keyframes scaleIn { from { opacity: 0; transform: scale(0.96) translateY(-4px); } to { opacity: 1; transform: none; } }

  #title {
    font-family: var(--font-display);
    font-size: 11px;
    font-weight: 600;
    color: var(--label-2);
    padding: 4px 8px 6px;
  }

  .ws-row {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 32px;
    padding: 0 8px;
    border-radius: var(--r-ctl);
    cursor: pointer;
    transition: background var(--t-fast) var(--ease);
  }
  .ws-row:hover { background: var(--fill-hover); }
  .ws-row:active { background: var(--fill-press); }
  .ws-row.selected { background: color-mix(in srgb, var(--accent) 15%, transparent); }

  .ws-dot {
    width: 10px; height: 10px;
    border-radius: 50%;
    flex-shrink: 0;
    box-shadow: 0 0 0 0.5px var(--hairline);
  }

  .ws-name {
    flex: 1;
    min-width: 0;
    font-size: 13px;
    color: var(--label);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .ws-name-input {
    flex: 1;
    min-width: 0;
    height: 22px;
    padding: 0 4px;
    font-size: 13px;
    font-family: inherit;
    color: var(--label);
    background: var(--fill);
    border: none;
    border-radius: var(--r-ctl);
    box-shadow: 0 0 0 1px var(--accent);
    outline: none;
  }

  .ws-count {
    font-size: 11px;
    color: var(--label-2);
    flex-shrink: 0;
  }

  .ws-kbd {
    font-size: 11px;
    font-weight: 500;
    color: var(--label-2);
    background: var(--fill);
    box-shadow: 0 0 0 0.5px var(--hairline);
    border-radius: var(--r-capsule);
    padding: 1px 5px;
    letter-spacing: 0.02em;
    flex-shrink: 0;
  }
  .ws-kbd-spacer { width: 26px; flex-shrink: 0; }

  .ws-check {
    display: inline-flex;
    width: 14px; height: 14px;
    color: var(--accent);
    flex-shrink: 0;
  }
  .ws-check svg { width: 100%; height: 100%; }
  .ws-check-spacer { width: 14px; flex-shrink: 0; }

  /* Space is reserved whether or not the row is hovered — only opacity changes,
     so hovering never reflows the row or the rows under it. */
  .ws-icon-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 22px; height: 22px;
    border: none;
    background: transparent;
    color: var(--label-2);
    cursor: pointer;
    border-radius: var(--r-ctl);
    flex-shrink: 0;
    opacity: 0;
    pointer-events: none;
    transition: opacity var(--t-fast) var(--ease), background var(--t-fast) var(--ease);
  }
  .ws-row:hover .ws-icon-btn,
  .ws-row:focus-within .ws-icon-btn,
  .ws-icon-btn:focus-visible { opacity: 1; pointer-events: auto; }
  .ws-icon-btn:hover { background: var(--fill-press); color: var(--label); }
  .ws-icon-btn svg { width: 12px; height: 12px; }
  .ws-icon-btn.ws-delete[disabled] { opacity: 0 !important; pointer-events: none !important; }

  /* Tab with audio or a live mic/camera (a call), nested under its workspace. */
  .ws-live {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 26px;
    padding: 0 8px 0 26px;
    border-radius: var(--r-ctl);
    cursor: pointer;
    font-size: 11.5px;
    color: var(--label-2);
    transition: background var(--t-fast) var(--ease);
  }
  .ws-live:hover { background: var(--fill-hover); }
  .ws-live:active { background: var(--fill-press); }
  .ws-live.selected { background: color-mix(in srgb, var(--accent) 15%, transparent); }
  .ws-live-dot {
    width: 6px; height: 6px;
    border-radius: 50%;
    background: var(--err);
    flex-shrink: 0;
    animation: livePulse 1.6s ease-in-out infinite;
  }
  @keyframes livePulse { 50% { opacity: 0.35; } }
  .ws-live-title {
    flex: 1;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .ws-live-status { flex-shrink: 0; color: var(--label-2); font-size: 11px; }

  .ws-sep {
    height: 0.5px;
    background: var(--hairline);
    margin: 6px 4px;
  }

  .ws-create {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 32px;
    padding: 0 8px;
    border-radius: var(--r-ctl);
    cursor: pointer;
    color: var(--accent);
    font-size: 13px;
    font-weight: 500;
    transition: background var(--t-fast) var(--ease);
  }
  .ws-create:hover { background: var(--fill-hover); }
  .ws-create:active { background: var(--fill-press); }
  .ws-create.selected { background: color-mix(in srgb, var(--accent) 15%, transparent); }
  .ws-create-icon { display: inline-flex; width: 14px; height: 14px; }
  .ws-create-icon svg { width: 100%; height: 100%; }
</style>
</head>
<body>
<div id="backdrop">
  <div id="panel" class="glass-panel">
    <div id="title">Workspaces</div>
    <div id="ws-list" role="listbox" aria-label="Workspaces">
      <div class="ws-create" id="ws-create-row" role="option" aria-selected="false" tabindex="-1" data-row-key="create">
        <span class="ws-create-icon">@@ICON_PLUS@@</span>
        <span>Create workspace</span>
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
    ipc({ type: 'workspace_close' });
  }

  document.getElementById('backdrop').addEventListener('mousedown', function(e) {
    if (e.target === this) close();
  });
  var lastData = [];
  var selectedKey = null;
  var pendingFocusKey = null;

  function selectableRows() {
    return Array.prototype.slice.call(document.querySelectorAll('[data-row-key]'));
  }

  function setSelection(row, focus) {
    if (!row) return;
    selectedKey = row.dataset.rowKey;
    selectableRows().forEach(function(candidate) {
      var selected = candidate === row;
      candidate.classList.toggle('selected', selected);
      candidate.setAttribute('aria-selected', selected ? 'true' : 'false');
      candidate.tabIndex = selected ? 0 : -1;
    });
    row.scrollIntoView({ block: 'nearest' });
    if (focus) row.focus();
  }

  function activateRow(row) {
    if (!row) return;
    if (row.dataset.kind === 'workspace') {
      ipc({ type: 'workspace_switch', id: row.dataset.workspaceId });
    } else if (row.dataset.kind === 'live') {
      ipc({ type: 'workspace_jump_tab', tab_id: Number(row.dataset.tabId) });
    } else if (row.dataset.kind === 'create') {
      ipc({ type: 'workspace_create' });
    }
  }

  document.addEventListener('keydown', function(e) {
    if (e.key === 'Escape') { e.preventDefault(); close(); return; }
    if (e.target.closest('input')) return;
    var rows = selectableRows();
    if (!rows.length) return;
    var current = rows.indexOf(document.activeElement.closest && document.activeElement.closest('[data-row-key]'));
    if (current < 0) current = Math.max(0, rows.findIndex(function(row) { return row.dataset.rowKey === selectedKey; }));
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault();
      var delta = e.key === 'ArrowDown' ? 1 : -1;
      setSelection(rows[(current + delta + rows.length) % rows.length], true);
    } else if (e.key === 'Enter') {
      if (e.target.closest('button')) return;
      e.preventDefault();
      activateRow(rows[current]);
    }
  });

  function renameStart(row, ws) {
    var nameEl = row.querySelector('.ws-name');
    var input = document.createElement('input');
    input.type = 'text';
    input.className = 'ws-name-input';
    input.value = ws.name;
    nameEl.replaceWith(input);
    input.focus();
    input.select();

    var finished = false;
    function finish(send) {
      if (finished) return;
      finished = true;
      var val = input.value.trim();
      pendingFocusKey = 'workspace-' + ws.id;
      if (send && val && val !== ws.name) {
        ipc({ type: 'workspace_rename', id: ws.id, name: val });
      } else {
        render(lastData, pendingFocusKey);
      }
    }
    input.addEventListener('blur', function() { finish(true); });
    input.addEventListener('keydown', function(e) {
      if (e.key === 'Enter') { e.preventDefault(); input.blur(); }
      if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); finish(false); }
    });
    input.addEventListener('click', function(e) { e.stopPropagation(); });
  }

  function wsRow(ws, idx, onlyOne) {
    var row = document.createElement('div');
    row.className = 'ws-row';
    row.setAttribute('role', 'option');
    row.setAttribute('aria-selected', 'false');
    row.tabIndex = -1;
    row.dataset.rowKey = 'workspace-' + ws.id;
    row.dataset.kind = 'workspace';
    row.dataset.workspaceId = ws.id;

    var dot = document.createElement('span');
    dot.className = 'ws-dot';
    dot.style.background = ws.color;
    row.appendChild(dot);

    var name = document.createElement('span');
    name.className = 'ws-name';
    name.textContent = ws.name;
    row.appendChild(name);

    var count = document.createElement('span');
    count.className = 'ws-count';
    count.textContent = ws.tab_count;
    count.title = ws.tab_count + (ws.tab_count === 1 ? ' tab' : ' tabs');
    row.appendChild(count);

    // ⌘1–⌘9 then ⌘0, matching the command palette's jump list. Rust re-targets
    // the digit row to workspace switching while this popover is open.
    var kbd = document.createElement('span');
    if (idx < 10) {
      kbd.className = 'ws-kbd';
      kbd.textContent = '⌘' + (idx === 9 ? '0' : idx + 1);
    } else {
      kbd.className = 'ws-kbd-spacer';
    }
    row.appendChild(kbd);

    var check = document.createElement('span');
    check.className = ws.active ? 'ws-check' : 'ws-check-spacer';
    if (ws.active) check.innerHTML = '@@ICON_CHECK@@';
    row.appendChild(check);

    var rename = document.createElement('button');
    rename.className = 'ws-icon-btn ws-rename';
    rename.title = 'Rename';
    rename.setAttribute('aria-label', 'Rename ' + ws.name);
    rename.innerHTML = '@@ICON_PENCIL@@';
    rename.addEventListener('click', function(e) {
      e.stopPropagation();
      setSelection(row, false);
      renameStart(row, ws);
    });
    rename.addEventListener('focus', function() { setSelection(row, false); });
    row.appendChild(rename);

    var del = document.createElement('button');
    del.className = 'ws-icon-btn ws-delete';
    del.title = 'Delete';
    del.setAttribute('aria-label', 'Delete ' + ws.name);
    del.innerHTML = '@@ICON_TRASH@@';
    if (onlyOne) del.disabled = true;
    del.addEventListener('click', function(e) {
      e.stopPropagation();
      setSelection(row, false);
      if (window.confirm('Delete workspace “' + ws.name + '”? This closes all its tabs.')) {
        pendingFocusKey = 'workspace-' + ws.id;
        ipc({ type: 'workspace_delete', id: ws.id });
      }
    });
    del.addEventListener('focus', function() { setSelection(row, false); });
    row.appendChild(del);

    row.addEventListener('click', function() {
      setSelection(row, false);
      activateRow(row);
    });
    row.addEventListener('focus', function() {
      setSelection(row, false);
    });

    return row;
  }

  function liveRow(t) {
    var row = document.createElement('div');
    row.className = 'ws-live';
    row.setAttribute('role', 'option');
    row.setAttribute('aria-selected', 'false');
    row.tabIndex = -1;
    row.dataset.rowKey = 'live-' + t.id;
    row.dataset.kind = 'live';
    row.dataset.tabId = t.id;
    var cameraInUse = t.media_kind === 'camera' || t.camera_in_use === true;
    var mediaStatus = cameraInUse ? 'Camera in use' : 'Playing audio';
    row.title = mediaStatus;
    var dot = document.createElement('span');
    dot.className = 'ws-live-dot';
    row.appendChild(dot);
    var title = document.createElement('span');
    title.className = 'ws-live-title';
    title.textContent = t.title;
    row.appendChild(title);
    var status = document.createElement('span');
    status.className = 'ws-live-status';
    status.textContent = mediaStatus;
    row.appendChild(status);
    row.addEventListener('click', function() {
      setSelection(row, false);
      activateRow(row);
    });
    row.addEventListener('focus', function() {
      setSelection(row, false);
    });
    return row;
  }

  function createRow() {
    var row = document.createElement('div');
    row.className = 'ws-create';
    row.id = 'ws-create-row';
    row.setAttribute('role', 'option');
    row.setAttribute('aria-selected', 'false');
    row.tabIndex = -1;
    row.dataset.rowKey = 'create';
    row.dataset.kind = 'create';
    var icon = document.createElement('span');
    icon.className = 'ws-create-icon';
    icon.innerHTML = '@@ICON_PLUS@@';
    row.appendChild(icon);
    var label = document.createElement('span');
    label.textContent = 'Create workspace';
    row.appendChild(label);
    row.addEventListener('click', function() { setSelection(row, false); activateRow(row); });
    row.addEventListener('focus', function() { setSelection(row, false); });
    return row;
  }

  function render(data, requestedFocusKey) {
    lastData = data;
    var list = document.getElementById('ws-list');
    list.innerHTML = '';
    var onlyOne = data.length <= 1;
    data.forEach(function(ws, idx) {
      list.appendChild(wsRow(ws, idx, onlyOne));
      (ws.live || []).forEach(function(t) { list.appendChild(liveRow(t)); });
    });
    var separator = document.createElement('div');
    separator.className = 'ws-sep';
    separator.setAttribute('role', 'presentation');
    list.appendChild(separator);
    list.appendChild(createRow());

    var focusKey = requestedFocusKey || pendingFocusKey;
    pendingFocusKey = null;
    var rows = selectableRows();
    var target = focusKey && rows.find(function(row) { return row.dataset.rowKey === focusKey; });
    if (!target && selectedKey) target = rows.find(function(row) { return row.dataset.rowKey === selectedKey; });
    if (!target) {
      var active = data.find(function(ws) { return ws.active; });
      if (active) target = rows.find(function(row) { return row.dataset.rowKey === 'workspace-' + active.id; });
    }
    target = target || rows[0];
    setSelection(target, Boolean(focusKey));
  }

  // Rust pushes the current workspace list on open and after every edit.
  window.__setWorkspaces = function(data) {
    if (!Array.isArray(data)) return;
    var activeRow = document.activeElement.closest && document.activeElement.closest('[data-row-key]');
    var focusKey = activeRow ? activeRow.dataset.rowKey : pendingFocusKey;
    render(data, focusKey);
  };
})();
</script>
</body>
</html>"#
        .replace("/*@@THEME@@*/", crate::theme::CSS)
        .replace("@@ICON_PLUS@@", crate::icons::PLUS)
        .replace("@@ICON_CHECK@@", crate::icons::CHECK)
        .replace("@@ICON_PENCIL@@", crate::icons::PENCIL)
        .replace("@@ICON_TRASH@@", crate::icons::TRASH)
}
