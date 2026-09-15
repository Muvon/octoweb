//! Terminal panel (⌘`): tabs of xterm.js terminals, each attached to a login
//! shell in `terminal.rs`.
//!
//! IPC out: open, input, resize, close, open_url, hide. Output comes back by
//! long poll on `octoweb-term://localhost/<id>` (200 output, 410 shell gone).
//! Rust calls `window.__termShow()` when the panel takes focus and
//! `window.__termOpened(id, error)` once a shell has started or failed.

const XTERM_JS: &str = include_str!("../assets/lib/xterm.min.js");
const XTERM_CSS: &str = include_str!("../assets/lib/xterm.css");
const FIT_ADDON_JS: &str = include_str!("../assets/lib/xterm-addon-fit.min.js");
const WEB_LINKS_ADDON_JS: &str = include_str!("../assets/lib/xterm-addon-web-links.min.js");

pub fn html() -> String {
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
    background: transparent;
    overflow: hidden;
    -webkit-font-smoothing: antialiased;
  }

  #panel {
    position: fixed;
    inset: 0 0 12px 0;
    display: flex;
    flex-direction: column;
    background: var(--term-bg);
    border-radius: 0 0 var(--r-card) var(--r-card);
    box-shadow: var(--shadow-float);
    overflow: hidden;
  }

  #bar {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    gap: 2px;
    height: 30px;
    padding: 0 6px;
    border-bottom: 0.5px solid var(--hairline);
    font-family: var(--font-text);
    font-size: var(--fs-caption);
    -webkit-user-select: none; user-select: none;
  }
  #tabs { display: flex; gap: 2px; min-width: 0; overflow: hidden; }

  .tab {
    display: flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
    max-width: 200px;
    height: 22px;
    padding: 0 2px 0 10px;
    border-radius: var(--r-ctl);
    color: var(--label-2);
    cursor: default;
  }
  .tab:hover { background: var(--fill-hover); }
  .tab.active { background: var(--fill-press); color: var(--label); }
  .tab .title { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }

  button {
    flex: 0 0 auto;
    display: flex;
    align-items: center;
    justify-content: center;
    width: 18px; height: 18px;
    padding: 0;
    border: none;
    border-radius: var(--r-capsule);
    background: transparent;
    color: var(--label-2);
    font: inherit;
    font-size: 14px;
    line-height: 1;
    cursor: pointer;
  }
  button:hover { background: var(--fill-hover); color: var(--label); }
  #new { width: 22px; height: 22px; font-size: 16px; }

  #terms { position: relative; flex: 1 1 auto; min-height: 0; }
  /* visibility, not display: hidden tabs keep a size, so they re-fit with the panel. */
  .term { position: absolute; inset: 6px 4px 4px 10px; visibility: hidden; }
  .term.active { visibility: visible; }
</style>
</head>
<body>
<div id="panel">
  <div id="bar">
    <div id="tabs"></div>
    <button id="new" title="New terminal (⌘T)" aria-label="New terminal">+</button>
  </div>
  <div id="terms"></div>
</div>
<script>/*@@XTERM_JS@@*/</script>
<script>/*@@FIT_ADDON_JS@@*/</script>
<script>/*@@WEB_LINKS_ADDON_JS@@*/</script>
<script>
(function() {
  const tabsEl = document.getElementById('tabs');
  const termsEl = document.getElementById('terms');
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
    tab.innerHTML = '<span class="title">Terminal</span>' +
      '<button class="close" title="Close (⌘W)" aria-label="Close terminal">×</button>';
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
    term.attachCustomKeyEventHandler(function(e) { return shortcut(e, t); });

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

  // ⌘ shortcuts of the panel itself. Everything else, ⌘C and ⌘V included,
  // takes xterm's normal path.
  function shortcut(e, t) {
    if (e.type !== 'keydown' || !e.metaKey || e.ctrlKey || e.altKey) return true;
    const ids = Array.from(terminals.keys());
    const at = ids.indexOf(t.id);
    const digit = /^Digit[1-9]$/.test(e.code) ? ids[Number(e.code.slice(5)) - 1] : undefined;
    let handled = true;
    if (e.code === 'KeyT' && !e.shiftKey) open();
    else if (e.code === 'KeyW' && !e.shiftKey) close(t.id);
    else if (e.code === 'KeyK' && !e.shiftKey) t.term.clear();
    else if (e.code === 'BracketLeft' && e.shiftKey) activate(ids[(at + ids.length - 1) % ids.length]);
    else if (e.code === 'BracketRight' && e.shiftKey) activate(ids[(at + 1) % ids.length]);
    else if (digit && !e.shiftKey) activate(digit);
    else handled = false;
    if (handled) e.preventDefault();
    return !handled;
  }

  document.getElementById('new').addEventListener('click', open);

  window.addEventListener('resize', function() {
    terminals.forEach(function(t) { t.fit.fit(); });
  });

  dark.addEventListener('change', function() {
    terminals.forEach(function(t) { t.term.options.theme = theme(); });
  });

  window.__termShow = function() {
    if (terminals.size) activate(activeId);
    else open();
  };

  window.__termOpened = function(id, error) {
    const t = terminals.get(id);
    if (!t) return;
    if (error) t.term.write('\x1b[31m' + error + '\x1b[0m\r\n');
    else pump(t);
  };
})();
</script>
</body>
</html>"#
        .replace("/*@@THEME@@*/", crate::theme::CSS)
        .replace("/*@@XTERM_CSS@@*/", XTERM_CSS)
        .replace("/*@@XTERM_JS@@*/", XTERM_JS)
        .replace("/*@@FIT_ADDON_JS@@*/", FIT_ADDON_JS)
        .replace("/*@@WEB_LINKS_ADDON_JS@@*/", WEB_LINKS_ADDON_JS)
}
