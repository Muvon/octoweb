//! Terminal panel (⌘`): tabs of xterm.js terminals, each tab split into panes
//! (⌘D beside, ⌘⇧D below) and each pane attached to a login shell in
//! `terminal.rs`. Docked above the footer; Rust owns its bounds. Every
//! workspace has its own tabs; Rust keeps their layout, so a reloaded page or
//! the next launch rebuilds them.
//!
//! IPC out: ready, layout, open, input, resize, close, open_url, hide,
//! fullscreen, resize_panel, resize_panel_end, resize_panel_reset, focus, blur.
//! Output comes back by long poll on `octoweb-term://localhost/<id>` (200
//! output, 410 shell gone).
//!
//! Called from Rust:
//!   window.__termSpace(ws, layout, floor) — put a workspace on screen
//!   window.__termDrop(ws)             — a workspace was deleted
//!   window.__termShow(fullscreen)     — panel took focus
//!   window.__termOpened(id, error)    — a shell started, or failed to
//!   window.__termNew() / __termClose() / __termCycle(step) — keymap actions
//!   window.__setShortcuts(data)       — update control titles

const XTERM_JS: &str = include_str!("../assets/lib/xterm.min.js");
const XTERM_CSS: &str = include_str!("../assets/lib/xterm.css");
const FIT_ADDON_JS: &str = include_str!("../assets/lib/xterm-addon-fit.min.js");
const WEB_LINKS_ADDON_JS: &str = include_str!("../assets/lib/xterm-addon-web-links.min.js");
const WEBGL_ADDON_JS: &str = include_str!("../assets/lib/xterm-addon-webgl.min.js");
const UNICODE11_ADDON_JS: &str = include_str!("../assets/lib/xterm-addon-unicode11.min.js");

/// `keybindings_json` is `Keymap::ui_json`, for the controls' shortcut titles.
/// `shell_theme_json` is `terminal::shell_theme`, or `null` to use only the
/// built-in palettes.
pub fn html(keybindings_json: &str, shell_theme_json: &str) -> String {
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
    padding: 0 3px 0 4px;
    border-radius: var(--r-ctl);
    color: var(--label-2);
    cursor: default;
  }
  .tab:hover { background: var(--fill-hover); }
  .tab.active { background: var(--fill-press); color: var(--label); }
  .tab .title { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  /* The ⌘1–⌘9 digit, numbered by position so closing a tab renumbers the rest. */
  #tabs { counter-reset: tab; }
  .tab { counter-increment: tab; }
  .tab .kbd { flex-shrink: 0; font-variant-numeric: tabular-nums; }
  .tab .kbd::before { content: counter(tab); }
  .tab:nth-child(n+10) .kbd { display: none; }
  .tab:nth-child(n+10) { padding-left: 10px; }
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
  .panes { position: absolute; inset: 0; display: flex; visibility: hidden; }
  .panes.active { visibility: visible; }
  /* One wrapper per workspace in the strip and in the pane area; only the
     workspace on screen shows. */
  #tabs > .space { display: contents; }
  #terms > .space { position: absolute; inset: 0; }
  #tabs > .space[hidden], #terms > .space[hidden] { display: none; }
  /* The split's background shows through the gap as the divider. */
  .split { display: flex; flex: 1 1 0; min-width: 0; min-height: 0; gap: 1px; background: var(--hairline); }
  .split.column { flex-direction: column; }
  .pane { position: relative; flex: 1 1 0; min-width: 0; min-height: 0; background: var(--term-bg); }
  .split .pane:not(.focused) .term { opacity: 0.6; }
  .term { position: absolute; inset: 8px 10px 6px 10px; }
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
<script>/*@@WEBGL_ADDON_JS@@*/</script>
<script>/*@@UNICODE11_ADDON_JS@@*/</script>
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
  // The user's own terminal colours ({dark, theme, options}, see
  // `terminal::shell_theme`): `theme` replaces the THEMES entry of its own
  // appearance. null keeps THEMES.
  const SHELL = /*@@SHELL_THEME_JSON@@*/;
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
  const panes = new Map(); // shell id -> { id, term, fit, el, tab, title }, every workspace's
  // Workspace id -> { ws, tabsEl, termsEl, tabs, active, pending }. Only the
  // workspace on screen shows; the shells of the others run on. `pending` is
  // the layout Rust kept, built once the panel shows it.
  const spaces = new Map();
  let space = null;        // the workspace on screen
  let nextId = 1;
  let closeTabTitle = 'Close terminal';
  let fullscreenChord = '';

  function ipc(msg) {
    window.ipc.postMessage(JSON.stringify(msg));
  }

  function theme() {
    if (SHELL && SHELL.dark === dark.matches) return SHELL.theme;
    return dark.matches ? THEMES.dark : THEMES.light;
  }

  // --term-bg paints the padding around each terminal in its background colour.
  function applyTheme() {
    const t = theme();
    document.documentElement.style.setProperty('--term-bg', t.background);
    panes.forEach(function(p) { p.term.options.theme = t; });
  }
  applyTheme();

  function openTab() {
    const tab = addTab(space);
    focusPane(newPane(tab, tab.el, null));
    showTab(tab);
  }

  // An empty tab at the end of workspace `s`'s strip.
  function addTab(s) {
    const el = document.createElement('div');
    el.className = 'panes';
    s.termsEl.appendChild(el);
    const tabEl = document.createElement('div');
    tabEl.className = 'tab';
    tabEl.setAttribute('role', 'tab');
    tabEl.innerHTML = '<span class="kbd" aria-hidden="true"></span><span class="title">Terminal</span>' +
      '<button class="close" type="button" aria-label="Close terminal">' +
      '<svg width="8" height="8" viewBox="0 0 8 8" fill="none">' +
      '<path d="M1 1L7 7M7 1L1 7" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>' +
      '</svg></button>';
    tabEl.querySelector('.close').title = closeTabTitle;
    s.tabsEl.appendChild(tabEl);
    const tab = { space: s, el: el, tabEl: tabEl, focused: null };
    s.tabs.push(tab);

    tabEl.addEventListener('mousedown', function(e) {
      if (e.target.closest('.close')) return;
      e.preventDefault();
      showTab(tab);
    });
    tabEl.querySelector('.close').addEventListener('click', function() { closeTab(tab); });
    return tab;
  }

  // A pane inserted into `parent` before `before`, on shell `id` when a kept
  // layout is rebuilt, else on a new shell.
  function newPane(tab, parent, before, id) {
    if (id === undefined) id = nextId++;
    const el = document.createElement('div');
    el.className = 'pane';
    el.dataset.pane = id;
    const host = document.createElement('div');
    host.className = 'term';
    el.appendChild(host);
    parent.insertBefore(el, before);

    const term = new Terminal(Object.assign({
      fontFamily: 'ui-monospace, "SF Mono", Menlo, monospace',
      fontSize: 13,
      cursorBlink: true,
      scrollback: 10000,
      macOptionClickForcesSelection: true,
      // `term.unicode` (Unicode 11 widths) is gated behind this flag.
      allowProposedApi: true,
      theme: theme(),
    }, SHELL && SHELL.options));
    const fit = new FitAddon.FitAddon();
    term.loadAddon(fit);
    term.loadAddon(new WebLinksAddon.WebLinksAddon(function(_event, url) {
      ipc({ type: 'open_url', url: url });
    }));
    term.open(host);
    // The DOM renderer draws block/box glyphs from the font, which leaves seams
    // between cells. WebGL synthesizes them (customGlyphs) so pixel art tiles.
    try {
      const webgl = new WebglAddon.WebglAddon();
      webgl.onContextLoss(function() { webgl.dispose(); });
      term.loadAddon(webgl);
    } catch (_e) {}
    // Default width tables are Unicode 6: emoji count as one cell and overflow.
    term.loadAddon(new Unicode11Addon.Unicode11Addon());
    term.unicode.activeVersion = '11';
    const p = { id: id, term: term, fit: fit, el: el, tab: tab, title: 'Terminal', shellTitle: '' };
    panes.set(id, p);

    el.addEventListener('focusin', function() { focusPane(p); });
    term.onTitleChange(function(title) {
      p.shellTitle = title;
      p.title = title || 'Terminal';
      if (tab.focused === p) {
        tab.tabEl.querySelector('.title').textContent = p.title;
        reportFocus();
      }
    });
    term.attachCustomKeyEventHandler(shortcut);

    // Fit first, so the shell starts at the real size.
    fit.fit();
    ipc({ type: 'open', id: id, cols: term.cols, rows: term.rows });
    term.onData(function(data) { ipc({ type: 'input', id: id, data: data }); });
    term.onResize(function(size) {
      ipc({ type: 'resize', id: id, cols: size.cols, rows: size.rows });
    });
    return p;
  }

  // The pane keys go to in its tab; the tab shows its title.
  function focusPane(p) {
    const tab = p.tab;
    if (tab.focused) tab.focused.el.classList.remove('focused');
    tab.focused = p;
    p.el.classList.add('focused');
    tab.tabEl.querySelector('.title').textContent = p.title;
    reportFocus();
    saveLayout(tab.space);
  }

  // Rust titles chrome_win after the pane with the keyboard: window trackers
  // (Timex) read the focused window's title, and the page's is browser_win's.
  function reportFocus() {
    if (space && space.active && document.hasFocus()) {
      ipc({ type: 'focus', title: space.active.focused.shellTitle });
    }
  }

  function showTab(tab) {
    const s = tab.space;
    s.active = tab;
    s.tabs.forEach(function(t) {
      t.el.classList.toggle('active', t === tab);
      t.tabEl.classList.toggle('active', t === tab);
      t.tabEl.setAttribute('aria-selected', String(t === tab));
    });
    refit(tab);
    tab.focused.term.focus();
    saveLayout(s);
  }

  function refit(tab) {
    panes.forEach(function(p) { if (p.tab === tab) p.fit.fit(); });
  }

  // A new pane beside ('row') or below ('column') the focused one. A split in
  // the other direction nests.
  function split(dir) {
    const tab = space.active;
    const target = tab.focused;
    let parent = target.el.parentElement;
    if (!parent.classList.contains(dir)) {
      // WebKit drops a moved element's focus without a blur event, which
      // would leave xterm blinking this cursor.
      target.term.blur();
      parent = document.createElement('div');
      parent.className = 'split ' + dir;
      target.el.replaceWith(parent);
      parent.appendChild(target.el);
    }
    const p = newPane(tab, parent, target.el.nextSibling);
    refit(tab);
    focusPane(p);
    p.term.focus();
  }

  function disposePane(p) {
    panes.delete(p.id);
    ipc({ type: 'close', id: p.id });
    p.term.dispose();
  }

  // A closed pane's space goes to its neighbour; the last pane takes its tab.
  function closePane(p) {
    if (!panes.has(p.id)) return;
    const tab = p.tab;
    const parent = p.el.parentElement;
    if (parent === tab.el) {
      closeTab(tab);
      return;
    }
    const neighbor = p.el.previousElementSibling || p.el.nextElementSibling;
    disposePane(p);
    p.el.remove();
    // A split down to one child gives way to it.
    if (parent.children.length === 1) parent.replaceWith(neighbor);
    if (tab.focused === p) {
      const el = neighbor.matches('.pane') ? neighbor : neighbor.querySelector('.pane');
      panes.forEach(function(q) { if (q.el === el) focusPane(q); });
    }
    refit(tab);
    // Moving the neighbour out of its split drops its focus.
    if (tab === tab.space.active) tab.focused.term.focus();
    saveLayout(tab.space);
  }

  function closeTab(tab) {
    const s = tab.space;
    const wasActive = s.active === tab;
    panes.forEach(function(p) { if (p.tab === tab) disposePane(p); });
    tab.el.remove();
    tab.tabEl.remove();
    s.tabs.splice(s.tabs.indexOf(tab), 1);
    if (wasActive) s.active = s.tabs[s.tabs.length - 1] || null;
    saveLayout(s);
    // A background workspace's tab goes when its shell exits; it shows its
    // new active tab once it is on screen again.
    if (!wasActive || s !== space) return;
    if (s.active) showTab(s.active);
    else ipc({ type: 'hide' });
  }

  // The tab and split tree Rust keeps for a workspace: { active, tabs:
  // [{ focused, root }] }, each node a { pane: id } or a
  // { split: 'row' | 'column', children: [...] }.
  function layoutOf(s) {
    function node(el) {
      if (el.classList.contains('pane')) return { pane: Number(el.dataset.pane) };
      return {
        split: el.classList.contains('row') ? 'row' : 'column',
        children: Array.prototype.map.call(el.children, node),
      };
    }
    return {
      active: s.tabs.indexOf(s.active),
      tabs: s.tabs.map(function(t) {
        return { focused: t.focused.id, root: node(t.el.firstElementChild) };
      }),
    };
  }

  function saveLayout(s) {
    // Not built yet, or mid-build: Rust's copy is the layout.
    if (s.pending) return;
    ipc({ type: 'layout', ws: s.ws, layout: layoutOf(s) });
  }

  // Rebuild a workspace's tabs and splits from the layout Rust kept. Each pane
  // reattaches to its shell, or starts one where the saved one left off.
  function build(s) {
    const layout = s.pending;
    if (!layout) return;
    layout.tabs.forEach(function(saved) {
      const tab = addTab(s);
      buildNode(tab, tab.el, saved.root);
      focusPane(panes.get(saved.focused));
    });
    s.active = s.tabs[layout.active] || null;
    s.pending = null;
  }

  function buildNode(tab, parent, node) {
    if (node.pane !== undefined) {
      newPane(tab, parent, null, node.pane);
      return;
    }
    const el = document.createElement('div');
    el.className = 'split ' + node.split;
    parent.appendChild(el);
    node.children.forEach(function(child) { buildNode(tab, el, child); });
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
    closePane(t);
  }

  // Positional keys the keymap doesn't hold: ⌘1–⌘9 pick a tab (like the
  // quickslots), ⌘D / ⌘⇧D split the focused pane beside / below, and ⌘K
  // clears. New, close and cycling come from Rust's keymap.
  function shortcut(e) {
    if (e.type !== 'keydown' || !e.metaKey || e.ctrlKey || e.altKey) return true;
    if (e.code === 'KeyD') {
      split(e.shiftKey ? 'column' : 'row');
    } else if (e.shiftKey) {
      return true;
    } else if (/^Digit[1-9]$/.test(e.code)) {
      const tab = space.tabs[Number(e.code.slice(5)) - 1];
      if (tab) showTab(tab);
    } else if (e.code === 'KeyK') {
      space.active.focused.term.clear();
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

  newBtn.addEventListener('click', openTab);
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
    panes.forEach(function(p) { p.fit.fit(); });
  });

  window.addEventListener('focus', reportFocus);
  window.addEventListener('blur', function() { ipc({ type: 'blur' }); });

  dark.addEventListener('change', applyTheme);

  // Put workspace `ws` on screen. `layout` is what Rust kept for it, used only
  // the first time this page sees `ws` and built once the panel shows; `floor`
  // is the lowest pane id free in every workspace.
  window.__termSpace = function(ws, layout, floor) {
    nextId = Math.max(nextId, floor);
    if (space) space.tabsEl.hidden = space.termsEl.hidden = true;
    space = spaces.get(ws);
    if (!space) {
      space = {
        ws: ws,
        tabsEl: document.createElement('div'),
        termsEl: document.createElement('div'),
        tabs: [],
        active: null,
        pending: layout,
      };
      space.tabsEl.className = space.termsEl.className = 'space';
      tabsEl.appendChild(space.tabsEl);
      termsEl.appendChild(space.termsEl);
      spaces.set(ws, space);
    }
    space.tabsEl.hidden = space.termsEl.hidden = false;
  };

  // A deleted workspace; Rust has hung up its shells.
  window.__termDrop = function(ws) {
    const s = spaces.get(ws);
    if (!s) return;
    panes.forEach(function(p) {
      if (p.tab.space !== s) return;
      panes.delete(p.id);
      p.term.dispose();
    });
    s.tabsEl.remove();
    s.termsEl.remove();
    spaces.delete(ws);
    if (space === s) space = null;
  };

  window.__termShow = function(fullscreen) {
    setFullscreen(!!fullscreen);
    build(space);
    if (space.active) showTab(space.active);
    else openTab();
  };

  window.__termOpened = function(id, error) {
    const p = panes.get(id);
    if (!p) return;
    if (error) p.term.write('\x1b[31m' + error + '\x1b[0m\r\n');
    else pump(p);
  };

  window.__termNew = openTab;

  window.__termClose = function() {
    if (space.active) closePane(space.active.focused);
  };

  window.__termCycle = function(step) {
    const tabs = space.tabs;
    if (tabs.length < 2) return;
    showTab(tabs[(tabs.indexOf(space.active) + step + tabs.length) % tabs.length]);
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
  // Rust answers with __termSpace: the workspace on screen and its layout.
  ipc({ type: 'ready' });
})();
</script>
</body>
</html>"#
        .replace("/*@@THEME@@*/", crate::theme::CSS)
        .replace("/*@@SHELL_THEME_JSON@@*/", shell_theme_json)
        .replace("/*@@KEYBINDINGS_JSON@@*/", keybindings_json)
        .replace("/*@@XTERM_CSS@@*/", XTERM_CSS)
        .replace("/*@@XTERM_JS@@*/", XTERM_JS)
        .replace("/*@@FIT_ADDON_JS@@*/", FIT_ADDON_JS)
        .replace("/*@@WEB_LINKS_ADDON_JS@@*/", WEB_LINKS_ADDON_JS)
        .replace("/*@@WEBGL_ADDON_JS@@*/", WEBGL_ADDON_JS)
        .replace("/*@@UNICODE11_ADDON_JS@@*/", UNICODE11_ADDON_JS)
}
