/// Returns the HTML for the workspace switcher popover (⌘⇧O).
/// Compact card anchored under the toolbar — not a full-screen modal like settings.
/// IPC messages: workspace_switch, workspace_rename, workspace_delete, workspace_create, workspace_close.
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
    width: 260px;
    max-height: 70vh;
    overflow-y: auto;
    padding: 8px;
    animation: scaleIn var(--t-pop) var(--spring);
  }

  @keyframes scaleIn { from { opacity: 0; transform: scale(0.96) translateY(-4px); } to { opacity: 1; transform: none; } }

  #title {
    font-family: var(--font-display);
    font-size: 12px;
    font-weight: 600;
    color: var(--label-2);
    padding: 4px 8px 6px;
    text-transform: uppercase;
    letter-spacing: 0.02em;
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

  .ws-dot {
    width: 10px; height: 10px;
    border-radius: 50%;
    flex-shrink: 0;
    box-shadow: 0 0 0 0.5px var(--hairline);
  }

  .ws-name {
    flex: 1;
    min-width: 0;
    font-size: 12.5px;
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
    font-size: 12.5px;
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
    color: var(--label-3);
    flex-shrink: 0;
  }

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
    width: 20px; height: 20px;
    border: none;
    background: transparent;
    color: var(--label-3);
    cursor: pointer;
    border-radius: var(--r-ctl);
    flex-shrink: 0;
    opacity: 0;
    pointer-events: none;
    transition: opacity var(--t-fast) var(--ease), background var(--t-fast) var(--ease);
  }
  .ws-row:hover .ws-icon-btn { opacity: 1; pointer-events: auto; }
  .ws-icon-btn:hover { background: var(--fill-press); color: var(--label); }
  .ws-icon-btn svg { width: 12px; height: 12px; }
  .ws-icon-btn.ws-delete[disabled] { opacity: 0 !important; pointer-events: none !important; }

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
    font-size: 12.5px;
    font-weight: 500;
    transition: background var(--t-fast) var(--ease);
  }
  .ws-create:hover { background: var(--fill-hover); }
  .ws-create:active { background: var(--fill-press); }
  .ws-create-icon { display: inline-flex; width: 14px; height: 14px; }
  .ws-create-icon svg { width: 100%; height: 100%; }
</style>
</head>
<body>
<div id="backdrop">
  <div id="panel" class="glass-panel">
    <div id="title">Workspaces</div>
    <div id="ws-list"></div>
    <div class="ws-sep"></div>
    <div class="ws-create" id="ws-create-row">
      <span class="ws-create-icon">@@ICON_PLUS@@</span>
      <span>New Workspace</span>
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
  document.addEventListener('keydown', function(e) {
    if (e.key === 'Escape') { e.preventDefault(); close(); }
  });

  document.getElementById('ws-create-row').addEventListener('click', function() {
    ipc({ type: 'workspace_create' });
  });

  var lastData = [];

  function renameStart(row, ws) {
    var nameEl = row.querySelector('.ws-name');
    var input = document.createElement('input');
    input.type = 'text';
    input.className = 'ws-name-input';
    input.value = ws.name;
    nameEl.replaceWith(input);
    input.focus();
    input.select();

    function commit() {
      var val = input.value.trim();
      if (val && val !== ws.name) {
        ipc({ type: 'workspace_rename', id: ws.id, name: val });
      } else {
        render(lastData);
      }
    }
    input.addEventListener('blur', commit);
    input.addEventListener('keydown', function(e) {
      if (e.key === 'Enter') { e.preventDefault(); input.blur(); }
      if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); render(lastData); }
    });
    input.addEventListener('click', function(e) { e.stopPropagation(); });
  }

  function wsRow(ws, onlyOne) {
    var row = document.createElement('div');
    row.className = 'ws-row';

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
    row.appendChild(count);

    var check = document.createElement('span');
    check.className = ws.active ? 'ws-check' : 'ws-check-spacer';
    if (ws.active) check.innerHTML = '@@ICON_CHECK@@';
    row.appendChild(check);

    var rename = document.createElement('button');
    rename.className = 'ws-icon-btn ws-rename';
    rename.title = 'Rename';
    rename.innerHTML = '@@ICON_PENCIL@@';
    rename.addEventListener('click', function(e) {
      e.stopPropagation();
      renameStart(row, ws);
    });
    row.appendChild(rename);

    var del = document.createElement('button');
    del.className = 'ws-icon-btn ws-delete';
    del.title = 'Delete';
    del.innerHTML = '@@ICON_TRASH@@';
    if (onlyOne) del.disabled = true;
    del.addEventListener('click', function(e) {
      e.stopPropagation();
      if (window.confirm('Delete workspace “' + ws.name + '”? This closes all its tabs.')) {
        ipc({ type: 'workspace_delete', id: ws.id });
      }
    });
    row.appendChild(del);

    row.addEventListener('click', function() {
      ipc({ type: 'workspace_switch', id: ws.id });
    });

    return row;
  }

  function render(data) {
    lastData = data;
    var list = document.getElementById('ws-list');
    list.innerHTML = '';
    var onlyOne = data.length <= 1;
    data.forEach(function(ws) {
      list.appendChild(wsRow(ws, onlyOne));
    });
  }

  // Rust pushes the current workspace list on open and after every edit.
  window.__setWorkspaces = function(data) {
    if (!Array.isArray(data)) return;
    render(data);
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
