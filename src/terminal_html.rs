//! Terminal panel (⌘`): tabs of xterm.js terminals, each attached to a login
//! shell in `terminal.rs`. Docked above the footer; Rust owns its bounds.
//!
//! IPC out: open, input, resize, close, open_url, hide, fullscreen,
//! resize_panel, resize_panel_end, resize_panel_reset. Output comes back by
//! long poll on `octoweb-term://localhost/<id>` (200 output, 410 shell gone).
//!
//! Called from Rust:
//!   window.__termShow(fullscreen)     — panel took focus
//!   window.__termOpened(id, error)    — a shell started, or failed to
//!   window.__termNew() / __termClose() / __termCycle(step) — keymap actions
//!   window.__setShortcuts(data)       — update control titles

const XTERM_JS: &str = include_str!("../assets/lib/xterm.min.js");
const XTERM_CSS: &str = include_str!("../assets/lib/xterm.css");
const FIT_ADDON_JS: &str = include_str!("../assets/lib/xterm-addon-fit.min.js");
const WEB_LINKS_ADDON_JS: &str = include_str!("../assets/lib/xterm-addon-web-links.min.js");

/// `keybindings_json` is `Keymap::ui_json`, for the controls' shortcut titles.
pub fn html(keybindings_json: &str) -> String {
    r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<style>
/*@@THEME@@*/
/*@@XTERM_CSS@@*/
  :root { --term-bg: #f8f8fa; }
  @media (prefers-color-scheme: dark) { :root { --term-bg: #1e1e22; } }

  html, body {
    width: 100%; height: 100%;
    margin: 0;
    overflow: hidden;
    background: var(--term-bg);
    -webkit-font-smoothing: antialiased;
  }

  #panel {
    position: fixed;
    inset: 0;
    display: flex;
    flex-direction: column;
    box-shadow: inset 0 0.5px 0 var(--hairline);
  }

  #resize-handle {
    position: fixed;
    top: 0; left: 0; right: 0;
    height: 6px;
    z-index: 30;
    cursor: row-resize;
    touch-action: none;
  }
  #resize-handle::after {
    content: "";
    position: absolute;
    top: 0; left: 0; right: 0;
    height: 2px;
    background: var(--accent);
    opacity: 0;
  }
  #resize-handle:hover::after,
  #resize-handle.active::after { opacity: 1; }
  #panel.fullscreen #resize-handle { display: none; }
  body.resizing, body.resizing * { cursor: row-resize !important; }

  #bar {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    gap: 4px;
    height: 32px;
    padding: 0 10px;
    box-shadow: 0 0.5px 0 var(--hairline);
    font-family: var(--font-text);
    font-size: var(--fs-caption);
    -webkit-user-select: none; user-select: none;
  }
  #tabs {
    flex: 1 1 auto;
    display: flex;
    align-items: center;
    gap: 2px;
    min-width: 0;
    overflow: hidden;
  }

  .tab {
    display: flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
    max-width: 200px;
    height: 24px;
    padding: 0 3px 0 10px;
    border-radius: var(--r-ctl);
    color: var(--label-2);
    cursor: default;
  }
  .tab:hover { background: var(--fill-hover); }
  .tab.active { background: var(--fill-press); color: var(--label); }
  .tab .title { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  .tab .close {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 16px; height: 16px;
    padding: 0;
    border: none;
    border-radius: var(--r-capsule);
    background: transparent;
    color: var(--label-2);
    cursor: pointer;
    opacity: 0;
    transition: background var(--t-fast), opacity var(--t-fast);
  }
  .tab:hover .close, .tab.active .close { opacity: 1; }
  .tab .close:hover { background: var(--fill-hover); }

  #new-btn, #fullscreen-btn, #close-btn {
    flex-shrink: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px; height: 24px;
    padding: 0;
    border: none;
    border-radius: var(--r-ctl);
    background: transparent;
    color: var(--label-2);
    cursor: pointer;
    transition: background var(--t-fast), color var(--t-fast);
  }
  #new-btn svg, #fullscreen-btn svg, #close-btn svg { width: 14px; height: 14px; }
  #new-btn:hover, #fullscreen-btn:hover, #close-btn:hover { background: var(--fill-hover); }
  #new-btn:active, #fullscreen-btn:active, #close-btn:active { background: var(--fill-press); }
  #fullscreen-btn.active {
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 15%, transparent);
  }

  #terms { position: relative; flex: 1 1 auto; min-height: 0; }
  /* visibility, not display: hidden tabs keep a size, so they re-fit with the panel. */
  .term { position: absolute; inset: 8px 10px 6px 10px; visibility: hidden; }
  .term.active { visibility: visible; }
</style>
</head>
<body>
<div id="panel">
  <div id="resize-handle" role="separator" aria-label="Resize terminal" aria-orientation="horizontal" tabindex="0"></div>
  <div id="bar">
    <div id="tabs" role="tablist" aria-label="Terminals"></div>
    <button id="new-btn" type="button" title="New terminal" aria-label="New terminal">
      <svg width="10" height="10" viewBox="0 0 10 10" fill="none">
        <path d="M5 1v8M1 5h8" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
      </svg>
    </button>
    <button id="fullscreen-btn" type="button" title="Toggle fullscreen" aria-label="Toggle terminal fullscreen">
      <svg class="ic-enter" width="10" height="10" viewBox="0 0 10 10" fill="none">
        <path d="M1 4V1h3M9 4V1H6M1 6v3h3M9 6v3H6"
              stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/>
      </svg>
      <svg class="ic-exit" width="10" height="10" viewBox="0 0 10 10" fill="none" style="display:none">
        <path d="M4 1v3H1M6 1v3h3M4 9V6H1M6 9V6h3"
              stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/>
      </svg>
    </button>
    <button id="close-btn" type="button" title="Hide terminal" aria-label="Hide terminal">
      <svg width="8" height="8" viewBox="0 0 8 8" fill="none">
        <path d="M1 1L7 7M7 1L1 7" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
      </svg>
    </button>
  </div>
  <div id="terms"></div>
</div>
<script>/*@@XTERM_JS@@*/</script>
<script>/*@@FIT_ADDON_JS@@*/</script>
<script>/*@@WEB_LINKS_ADDON_JS@@*/</script>
<script>
(function() {
  const panel = document.getElementById('panel');
  const tabsEl = document.getElementById('tabs');
  const termsEl = document.getElementById('terms');
  const handle = document.getElementById('resize-handle');
  const newBtn = document.getElementById('new-btn');
  const fullscreenBtn = document.getElementById('fullscreen-btn');
  const closeBtn = document.getElementById('close-btn');
  const dark = matchMedia('(prefers-color-scheme: dark)');
  const THEMES = {
    light: {
      background: '#f8f8fa', foreground: '#1d1d1f', cursor: '#1d1d1f', cursorAccent: '#f8f8fa',
      selectionBackground: '#add6ff',
      black: '#000000', red: '#cd3131', green: '#00bc00', yellow: '#949800',
      blue: '#0451a5', magenta: '#bc05bc', cyan: '#0598bc', white: '#555555',
      brightBlack: '#666666', brightRed: '#cd3131', brightGreen: '#14ce14', brightYellow: '#b5ba00',
      brightBlue: '#0451a5', brightMagenta: '#bc05bc', brightCyan: '#0598bc', brightWhite: '#a5a5a5',
    },
    dark: {
      background: '#1e1e22', foreground: '#e5e5e5', cursor: '#e5e5e5', cursorAccent: '#1e1e22',
      selectionBackground: '#264f78',
      black: '#000000', red: '#cd3131', green: '#0dbc79', yellow: '#e5e510',
      blue: '#2472c8', magenta: '#bc3fbc', cyan: '#11a8cd', white: '#e5e5e5',
      brightBlack: '#666666', brightRed: '#f14c4c', brightGreen: '#23d18b', brightYellow: '#f5f543',
      brightBlue: '#3b8eea', brightMagenta: '#d670d6', brightCyan: '#29b8db', brightWhite: '#e5e5e5',
    },
  };
  const terminals = new Map(); // id -> { id, term, fit, el, tab }
  let nextId = 1;
  let activeId = 0;
  let closeTabTitle = 'Close terminal';
  let fullscreenChord = '';

  function ipc(msg) {
    window.ipc.postMessage(JSON.stringify(msg));
  }

  function theme() {
    return dark.matches ? THEMES.dark : THEMES.light;
  }

  function open() {
    const id = nextId++;
    const el = document.createElement('div');
    el.className = 'term';
    termsEl.appendChild(el);
    const tab = document.createElement('div');
    tab.className = 'tab';
    tab.setAttribute('role', 'tab');
    tab.innerHTML = '<span class="title">Terminal</span>' +
      '<button class="close" type="button" aria-label="Close terminal">' +
      '<svg width="8" height="8" viewBox="0 0 8 8" fill="none">' +
      '<path d="M1 1L7 7M7 1L1 7" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>' +
      '</svg></button>';
    tab.querySelector('.close').title = closeTabTitle;
    tabsEl.appendChild(tab);

    const term = new Terminal({
      fontFamily: 'ui-monospace, "SF Mono", Menlo, monospace',
      fontSize: 13,
      cursorBlink: true,
      scrollback: 10000,
      macOptionClickForcesSelection: true,
      theme: theme(),
    });
    const fit = new FitAddon.FitAddon();
    term.loadAddon(fit);
    term.loadAddon(new WebLinksAddon.WebLinksAddon(function(_event, url) {
      ipc({ type: 'open_url', url: url });
    }));
    term.open(el);
    const t = { id: id, term: term, fit: fit, el: el, tab: tab };
    terminals.set(id, t);

    tab.addEventListener('mousedown', function(e) {
      if (e.target.closest('.close')) return;
      e.preventDefault();
      activate(id);
    });
    tab.querySelector('.close').addEventListener('click', function() { close(id); });
    term.onTitleChange(function(title) {
      tab.querySelector('.title').textContent = title || 'Terminal';
    });
    term.attachCustomKeyEventHandler(shortcut);

    // Fit first, so the shell starts at the real size.
    activate(id);
    ipc({ type: 'open', id: id, cols: term.cols, rows: term.rows });
    term.onData(function(data) { ipc({ type: 'input', id: id, data: data }); });
    term.onResize(function(size) {
      ipc({ type: 'resize', id: id, cols: size.cols, rows: size.rows });
    });
  }

  function activate(id) {
    activeId = id;
    terminals.forEach(function(t) {
      t.el.classList.toggle('active', t.id === id);
      t.tab.classList.toggle('active', t.id === id);
      t.tab.setAttribute('aria-selected', String(t.id === id));
    });
    const t = terminals.get(id);
    t.fit.fit();
    t.term.focus();
  }

  function close(id) {
    const t = terminals.get(id);
    if (!t) return;
    terminals.delete(id);
    ipc({ type: 'close', id: id });
    t.term.dispose();
    t.el.remove();
    t.tab.remove();
    if (activeId !== id) return;
    const last = Array.from(terminals.keys()).pop();
    if (last) {
      activate(last);
    } else {
      activeId = 0;
      ipc({ type: 'hide' });
    }
  }

  async function pump(t) {
    for (;;) {
      let response;
      try {
        response = await fetch('octoweb-term://localhost/' + t.id, { method: 'POST', cache: 'no-store' });
      } catch (e) {
        break;
      }
      if (response.status !== 200) break;
      const bytes = new Uint8Array(await response.arrayBuffer());
      // Read again only once xterm has parsed this: a flood backs up into the shell.
      await new Promise(function(resolve) { t.term.write(bytes, resolve); });
    }
    close(t.id);
  }

  // Positional keys the keymap doesn't hold: ⌘1–⌘9 pick a tab (like the
  // quickslots) and ⌘K clears. New, close and cycling come from Rust's keymap.
  function shortcut(e) {
    if (e.type !== 'keydown' || !e.metaKey || e.ctrlKey || e.altKey || e.shiftKey) return true;
    if (/^Digit[1-9]$/.test(e.code)) {
      const id = Array.from(terminals.keys())[Number(e.code.slice(5)) - 1];
      if (id) activate(id);
    } else if (e.code === 'KeyK') {
      terminals.get(activeId).term.clear();
    } else {
      return true;
    }
    e.preventDefault();
    return false;
  }

  function setFullscreen(on) {
    panel.classList.toggle('fullscreen', on);
    fullscreenBtn.classList.toggle('active', on);
    fullscreenBtn.querySelector('.ic-enter').style.display = on ? 'none' : '';
    fullscreenBtn.querySelector('.ic-exit').style.display = on ? '' : 'none';
    const label = on ? 'Exit fullscreen' : 'Toggle fullscreen';
    fullscreenBtn.title = label + (fullscreenChord ? ' (' + fullscreenChord + ')' : '');
  }

  newBtn.addEventListener('click', open);
  fullscreenBtn.addEventListener('click', function() { ipc({ type: 'fullscreen' }); });
  closeBtn.addEventListener('click', function() { ipc({ type: 'hide' }); });

  // The panel's top edge moves while it's dragged, so screenY is the stable
  // coordinate. Live resizes go out at most once per frame; Rust persists the
  // final height.
  let dragPointer = null;
  let dragStartY = 0;
  let dragStartHeight = 0;
  let dragHeight = 0;
  let dragFrame = 0;

  function queueResize(height) {
    dragHeight = Math.max(0, Math.round(height));
    if (dragFrame) return;
    dragFrame = requestAnimationFrame(function() {
      dragFrame = 0;
      ipc({ type: 'resize_panel', height: dragHeight });
    });
  }

  handle.addEventListener('pointerdown', function(e) {
    if (!e.isPrimary || e.button !== 0) return;
    e.preventDefault();
    dragPointer = e.pointerId;
    dragStartY = e.screenY;
    dragStartHeight = Math.round(window.innerHeight);
    dragHeight = dragStartHeight;
    handle.setPointerCapture(e.pointerId);
    handle.classList.add('active');
    document.body.classList.add('resizing');
  });

  handle.addEventListener('pointermove', function(e) {
    if (e.pointerId !== dragPointer) return;
    e.preventDefault();
    queueResize(dragStartHeight + dragStartY - e.screenY);
  });

  function finishResize(e) {
    if (e.pointerId !== dragPointer) return;
    e.preventDefault();
    if (e.type !== 'pointercancel') {
      dragHeight = Math.max(0, Math.round(dragStartHeight + dragStartY - e.screenY));
    }
    if (dragFrame) {
      cancelAnimationFrame(dragFrame);
      dragFrame = 0;
    }
    if (handle.hasPointerCapture(e.pointerId)) handle.releasePointerCapture(e.pointerId);
    dragPointer = null;
    handle.classList.remove('active');
    document.body.classList.remove('resizing');
    ipc({ type: 'resize_panel_end', height: dragHeight });
  }

  handle.addEventListener('pointerup', finishResize);
  handle.addEventListener('pointercancel', finishResize);
  handle.addEventListener('dblclick', function(e) {
    e.preventDefault();
    ipc({ type: 'resize_panel_reset' });
  });
  handle.addEventListener('keydown', function(e) {
    if (e.key !== 'ArrowUp' && e.key !== 'ArrowDown') return;
    e.preventDefault();
    const step = (e.shiftKey ? 64 : 16) * (e.key === 'ArrowUp' ? 1 : -1);
    ipc({ type: 'resize_panel_end', height: Math.round(window.innerHeight) + step });
  });

  window.addEventListener('resize', function() {
    terminals.forEach(function(t) { t.fit.fit(); });
  });

  dark.addEventListener('change', function() {
    terminals.forEach(function(t) { t.term.options.theme = theme(); });
  });

  window.__termShow = function(fullscreen) {
    setFullscreen(!!fullscreen);
    if (terminals.size) activate(activeId);
    else open();
  };

  window.__termOpened = function(id, error) {
    const t = terminals.get(id);
    if (!t) return;
    if (error) t.term.write('\x1b[31m' + error + '\x1b[0m\r\n');
    else pump(t);
  };

  window.__termNew = open;

  window.__termClose = function() {
    if (activeId) close(activeId);
  };

  window.__termCycle = function(step) {
    const ids = Array.from(terminals.keys());
    if (ids.length < 2) return;
    activate(ids[(ids.indexOf(activeId) + step + ids.length) % ids.length]);
  };

  // Control titles follow the effective keymap rather than compiled defaults.
  window.__setShortcuts = function(data) {
    const actions = data && Array.isArray(data.actions) ? data.actions : [];
    const chordFor = function(id) {
      const action = actions.find(function(item) { return item.id === id; });
      return action && Array.isArray(action.keys) ? action.keys.join('') : '';
    };
    const titled = function(label, id) {
      const chord = chordFor(id);
      return chord ? label + ' (' + chord + ')' : label;
    };
    newBtn.title = titled('New terminal', 'new_session');
    closeBtn.title = titled('Hide terminal', 'terminal');
    closeTabTitle = titled('Close terminal', 'close_tab');
    document.querySelectorAll('.tab .close').forEach(function(button) {
      button.title = closeTabTitle;
    });
    fullscreenChord = chordFor('sidebar_fullscreen');
    setFullscreen(panel.classList.contains('fullscreen'));
  };
  window.__setShortcuts(/*@@KEYBINDINGS_JSON@@*/);
})();
</script>
</body>
</html>"#
        .replace("/*@@THEME@@*/", crate::theme::CSS)
        .replace("/*@@KEYBINDINGS_JSON@@*/", keybindings_json)
        .replace("/*@@XTERM_CSS@@*/", XTERM_CSS)
        .replace("/*@@XTERM_JS@@*/", XTERM_JS)
        .replace("/*@@FIT_ADDON_JS@@*/", FIT_ADDON_JS)
        .replace("/*@@WEB_LINKS_ADDON_JS@@*/", WEB_LINKS_ADDON_JS)
}
