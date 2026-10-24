//! Notes panel: the active workspace's markdown notes (`notes.rs`), as a
//! column on the right, left of the AI sidebar when that is open. A plain
//! textarea over a highlighted copy of its text: tasks, done tasks, headings,
//! quotes and links get colour only, so both layers keep the same metrics.
//!
//! Click a `[ ]` to tick it, ⌘↩ toggles the line's task, Enter continues a
//! list, ⌘-click opens a link. Edits save after a pause and on blur.
//!
//! Messages to Rust (`window.ipc`): `notes::Request`.
//!
//! Called from Rust:
//!   window.__notesLoad(ws, name, text, reveal) — show a workspace's notes
//!   window.__notesError(ws, name, message)     — its file could not be read
//!   window.__notesFlush()                      — save now, the panel is hiding
//!   window.__notesFocus()                      — take the caret
//!   window.__notesFullscreen(on)               — the column fills the window, or docks
//!   window.__notesView(view, focus)            — `notes` or `tasks`; focus a task line
//!   window.__notesAppend(date, entry, reveal)  — add under the day's heading, and save
//!   window.__notesDue(id, iso)                 — the date a `due` request's words name
//!   window.__notesDay()                        — the day changed: regroup the tasks
//!   window.__paneDue(n)                        — tasks due or overdue, on the Notes tab
//!   window.__setShortcuts(data)                — update control titles

pub fn html(keybindings_json: &str) -> String {
	r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<style>
/*@@THEME@@*/
  html, body {
    width: 100%; height: 100%;
    margin: 0;
    overflow: hidden;
    background: var(--canvas);
    -webkit-font-smoothing: antialiased;
  }

  #panel {
    position: fixed;
    inset: 0;
    display: flex;
    flex-direction: column;
    box-shadow: inset 0.5px 0 0 var(--hairline);
  }

  #resize-handle {
    position: fixed;
    top: 0; bottom: 0; left: 0;
    width: 6px;
    z-index: 30;
    cursor: col-resize;
    touch-action: none;
  }
  #resize-handle::after {
    content: "";
    position: absolute;
    top: 0; bottom: 0; left: 0;
    width: 2px;
    background: var(--accent);
    opacity: 0;
  }
  #resize-handle:hover::after,
  #resize-handle.active::after { opacity: 1; }
  #panel.fullscreen #resize-handle { display: none; }
  body.resizing, body.resizing * { cursor: col-resize !important; }

  /* Sized like the assistant sidebar's header beside it: a 32px row, 22px
     pills in 13px text, 24px controls. */
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
    color: var(--label-2);
    -webkit-user-select: none; user-select: none;
  }
  .seg { display: flex; gap: 2px; padding: 2px; border-radius: var(--r-capsule); background: var(--fill); }
  .tab { border: none; background: transparent; height: 18px; padding: 0 10px; border-radius: var(--r-capsule); color: var(--label-2); font: inherit; font-size: 13px; font-weight: 500; cursor: pointer; display: flex; align-items: center; gap: 5px; transition: background var(--t-fast), color var(--t-fast); }
  .tab:hover { color: var(--label); }
  .tab.active { background: var(--card); color: var(--label); box-shadow: 0 0 0 0.5px var(--hairline), 0 1px 3px rgba(0, 0, 0, 0.08); }
  .badge { min-width: 16px; padding: 0 5px; box-sizing: border-box; border-radius: var(--r-capsule); background: var(--accent); color: var(--on-accent); font-size: 10px; font-weight: 600; line-height: 16px; text-align: center; }
  .badge:empty { display: none; }
  #space { flex: 1 1 auto; min-width: 0; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; }
  #reveal-btn, #fullscreen-btn, #close-btn {
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
  #reveal-btn svg, #fullscreen-btn svg, #close-btn svg { width: 14px; height: 14px; }
  #reveal-btn:hover, #fullscreen-btn:hover, #close-btn:hover { background: var(--fill-hover); }
  #reveal-btn:active, #fullscreen-btn:active, #close-btn:active { background: var(--fill-press); }
  #fullscreen-btn.active {
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 15%, transparent);
  }

  /* Only #scroll scrolls; the textarea grows with the highlighted copy, so
     the two wrap at the same width and never drift apart. */
  #scroll { flex: 1 1 auto; min-height: 0; overflow-y: auto; }
  #wrap { position: relative; min-height: 100%; }
  #hl, #text {
    box-sizing: border-box;
    margin: 0;
    border: 0;
    padding: 10px 16px 28px;
    font-family: var(--font-text);
    font-size: 13px;
    line-height: 1.6;
    white-space: pre-wrap;
    overflow-wrap: break-word;
    tab-size: 4;
  }
  #hl { color: var(--label); pointer-events: none; }
  #text {
    position: absolute;
    inset: 0;
    width: 100%; height: 100%;
    resize: none;
    overflow: hidden;
    outline: none;
    background: transparent;
    color: transparent;
    caret-color: var(--label);
  }
  #text::placeholder { color: var(--label-3); }
  #text::selection { color: transparent; background: color-mix(in srgb, var(--accent) 28%, transparent); }
  #text.error::placeholder { color: var(--err); }
  .h { color: var(--accent); }
  .q { color: var(--label-2); }
  .box { color: var(--accent); }
  .done { color: var(--label-3); text-decoration: line-through; }
  .done .box { color: var(--label-3); }
  .link { color: var(--accent); text-decoration: underline; text-decoration-color: color-mix(in srgb, var(--accent) 40%, transparent); }
  .done .link { color: inherit; }

  /* Open tasks above the text, in the same scroll (spec 003-tasks): due ones
     by date, the rest behind one line; the head line folds the block. */
  #tasks { padding: 6px 0; box-shadow: 0 0.5px 0 var(--hairline); font-family: var(--font-text); font-size: 13px; color: var(--label); }
  #tasks[hidden] { display: none; }
  #tasks-head { display: flex; align-items: baseline; gap: 6px; width: 100%; padding: 4px 16px; border: 0; background: transparent; color: var(--label-2); font: inherit; font-size: var(--fs-caption); font-weight: 600; text-align: left; cursor: pointer; -webkit-user-select: none; user-select: none; }
  #tasks-head:hover { color: var(--label); }
  #tasks-head .chev { display: inline-block; width: 10px; transition: transform var(--t-fast); }
  #tasks.collapsed #tasks-head .chev { transform: rotate(-90deg); }
  #tasks-sum { min-width: 0; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; font-weight: 500; color: var(--label-3); }
  #tasks-sum .over { color: var(--err); }
  #tasks.collapsed #tasks-body { display: none; }
  #add { display: block; box-sizing: border-box; width: calc(100% - 32px); height: 28px; margin: 4px 16px; padding: 0 10px; border: 0; border-radius: var(--r-ctl); background: var(--fill); color: var(--label); font: inherit; outline: none; transition: box-shadow var(--t-fast); }
  #add::placeholder { color: var(--label-3); }
  #add.bad { box-shadow: 0 0 0 2px color-mix(in srgb, var(--err) 60%, transparent); }
  #list { outline: none; }
  .grp { padding: 8px 16px 2px; font-size: var(--fs-caption); font-weight: 600; color: var(--label-2); -webkit-user-select: none; user-select: none; }
  .grp.over { color: var(--err); }
  .row { display: flex; align-items: center; gap: 8px; height: 26px; padding: 0 16px; cursor: default; }
  #list:focus .row.focus { background: var(--fill-hover); }
  .rbox { flex: none; padding: 4px; margin: -4px; cursor: pointer; }
  .rbox i { display: block; position: relative; box-sizing: border-box; width: 14px; height: 14px; border: 1.5px solid var(--label-3); border-radius: 4px; }
  .row.isdone .rbox i { background: var(--accent); border-color: var(--accent); }
  .row.isdone .rbox i::after { content: ''; position: absolute; left: 3.5px; top: 0.5px; width: 3px; height: 6px; border: solid var(--on-accent); border-width: 0 1.5px 1.5px 0; transform: rotate(45deg); }
  .rtext { flex: 1 1 auto; min-width: 0; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; cursor: pointer; }
  .row.isdone .rtext { color: var(--label-3); text-decoration: line-through; }
  .row.isdone .link { color: inherit; }
  .due { flex: none; font-size: var(--fs-caption); color: var(--label-2); }
  .due.over { color: var(--err); }
  .more { display: block; margin: 4px 16px 2px; padding: 0; border: 0; background: transparent; color: var(--label-2); font: inherit; font-size: var(--fs-caption); cursor: pointer; }
  .more:hover { color: var(--label); }
</style>
</head>
<body>
<div id="panel">
  <div id="resize-handle" role="separator" aria-label="Resize notes" aria-orientation="vertical" tabindex="0"></div>
  <div id="bar">
    <div class="seg" role="tablist">
      <button class="tab active" id="tab-notes" type="button" role="tab" aria-selected="true">Notes <span class="badge" id="due"></span></button>
      <button class="tab" id="tab-mail" type="button" role="tab">Mail <span class="badge" id="unread"></span></button>
      <button class="tab" id="tab-cal" type="button" role="tab">Calendar</button>
    </div>
    <span id="space"></span>
    <button id="reveal-btn" type="button" title="Show file in Finder" aria-label="Show notes file in Finder">
      <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
        <path d="M1.5 3.5a1 1 0 0 1 1-1h3l1.5 1.5h4.5a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1h-9a1 1 0 0 1-1-1z"
              stroke="currentColor" stroke-width="1.2" stroke-linejoin="round"/>
      </svg>
    </button>
    <button id="fullscreen-btn" type="button" title="Toggle fullscreen" aria-label="Toggle notes fullscreen">
      <svg class="ic-enter" width="10" height="10" viewBox="0 0 10 10" fill="none">
        <path d="M1 4V1h3M9 4V1H6M1 6v3h3M9 6v3H6"
              stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/>
      </svg>
      <svg class="ic-exit" width="10" height="10" viewBox="0 0 10 10" fill="none" style="display:none">
        <path d="M4 1v3H1M6 1v3h3M4 9V6H1M6 9V6h3"
              stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/>
      </svg>
    </button>
    <button id="close-btn" type="button" title="Hide notes" aria-label="Hide notes">
      <svg width="8" height="8" viewBox="0 0 8 8" fill="none">
        <path d="M1 1L7 7M7 1L1 7" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
      </svg>
    </button>
  </div>
  <div id="scroll">
    <div id="tasks">
      <button id="tasks-head" type="button" aria-expanded="true"><span class="chev">▾</span><span>Tasks</span><span id="tasks-sum"></span></button>
      <div id="tasks-body">
        <input id="add" type="text" spellcheck="false" autocomplete="off" placeholder="Add a task — Pay rent >fri" aria-label="Add a task">
        <div id="list" tabindex="0" aria-label="Open tasks"></div>
      </div>
    </div>
    <div id="wrap">
      <pre id="hl" aria-hidden="true"></pre>
      <textarea id="text" spellcheck="false" autocorrect="off" autocapitalize="off" aria-label="Notes"></textarea>
    </div>
  </div>
</div>
<script>
(function() {
  const text = document.getElementById('text');
  const hl = document.getElementById('hl');
  const scroll = document.getElementById('scroll');
  const spaceEl = document.getElementById('space');
  const handle = document.getElementById('resize-handle');
  const closeBtn = document.getElementById('close-btn');
  const revealBtn = document.getElementById('reveal-btn');
  const fullscreenBtn = document.getElementById('fullscreen-btn');
  const panel = document.getElementById('panel');
  const SAVE_DELAY_MS = 400;
  // A task line: indent and bullet, then the box.
  const TASK = /^(\s*[-*] )\[([ xX])\]/;
  // A list or quote line Enter continues: indent, then the marker.
  const LIST = /^(\s*)([-*] (?:\[[ xX]\] )?|> )/;
  // Markdown link, <autolink> or bare URL; only http(s) and links to mail
  // conversations (`octoweb-mail://account/thread`) ever open.
  const LINK = /\[[^\]\n]*\]\(((?:https?|octoweb-mail):\/\/[^)\s]+)\)|<(https?:\/\/[^>\s]+)>|(https?:\/\/[^\s<>()\]]+)/g;
  const MAIL_LINK = /octoweb-mail:\/\/([0-9a-f]+)\//;
  let mailColors = {};

  let ws = null;     // workspace whose notes are in the editor; null while unreadable
  let saved = '';    // what Rust last stored for it
  let saveTimer = 0;
  let placeholder = '';
  let fullscreenChord = '';

  function ipc(msg) {
    window.ipc.postMessage(JSON.stringify(msg));
  }

  function esc(s) {
    return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
  }

  function inline(s) {
    let out = '';
    let last = 0;
    s.replace(LINK, function(m) {
      const at = arguments[arguments.length - 2];
      // A mail conversation's link takes its account's colour (TRI-9).
      const mail = MAIL_LINK.exec(m);
      const style = mail && mailColors[mail[1]] ? ' style="color:' + esc(mailColors[mail[1]]) + '"' : '';
      out += esc(s.slice(last, at)) + '<span class="link"' + style + '>' + esc(m) + '</span>';
      last = at + m.length;
      return m;
    });
    return out + esc(s.slice(last));
  }

  function line(l) {
    if (/^#{1,6} /.test(l)) return '<span class="h">' + esc(l) + '</span>';
    const task = TASK.exec(l);
    if (task) {
      const cls = task[2] === ' ' ? 'task' : 'done';
      const box = task[1].length;
      return '<span class="' + cls + '">' + esc(task[1]) + '<span class="box">' +
        esc(l.slice(box, box + 3)) + '</span>' + inline(l.slice(box + 3)) + '</span>';
    }
    if (/^\s*>/.test(l)) return '<span class="q">' + inline(l) + '</span>';
    return inline(l);
  }

  // The trailing newline gives a final empty line its height.
  function render() {
    hl.innerHTML = text.value.split('\n').map(line).join('\n') + '\n';
  }

  function save() {
    clearTimeout(saveTimer);
    saveTimer = 0;
    if (ws === null || text.value === saved) return;
    saved = text.value;
    ipc({ type: 'save', ws: ws, text: saved });
  }

  // The line holding the caret: its start offset and text.
  function caretLine() {
    const v = text.value;
    const pos = text.selectionStart;
    const start = v.lastIndexOf('\n', pos - 1) + 1;
    let end = v.indexOf('\n', pos);
    if (end < 0) end = v.length;
    return { start: start, text: v.slice(start, end), col: pos - start };
  }

  // Through execCommand, so ⌘Z undoes it like typing.
  function replaceRange(from, to, s) {
    text.focus();
    text.setSelectionRange(from, to);
    document.execCommand('insertText', false, s);
  }

  function toggleTask(l) {
    const task = TASK.exec(l.text);
    const pos = text.selectionStart;
    if (task) {
      const at = l.start + task[1].length + 1;
      const done = task[2] === ' ';
      replaceRange(at, at + 1, done ? 'x' : ' ');
      text.setSelectionRange(pos, pos);
      // TRI-10: ticking a task made from an email closes the email too.
      const mail = /octoweb-mail:\/\/[^)\s]+/.exec(l.text);
      if (mail) ipc({ type: 'task', url: mail[0], done: done });
      return;
    }
    // A plain line becomes a task, keeping its indent and dropping a bullet.
    const lead = /^(\s*)(?:[-*] )?/.exec(l.text);
    replaceRange(l.start, l.start + lead[0].length, lead[1] + '- [ ] ');
    const end = l.start + lead[1].length + 6 + (l.text.length - lead[0].length);
    text.setSelectionRange(end, end);
  }

  function urlAt(l) {
    LINK.lastIndex = 0;
    let m;
    while ((m = LINK.exec(l.text))) {
      if (l.col >= m.index && l.col <= m.index + m[0].length) return m[1] || m[2] || m[3];
    }
    return null;
  }

  text.addEventListener('input', function() {
    render();
    clearTimeout(saveTimer);
    saveTimer = setTimeout(save, SAVE_DELAY_MS);
  });
  text.addEventListener('blur', save);

  text.addEventListener('click', function(e) {
    if (text.selectionStart !== text.selectionEnd) return;
    const l = caretLine();
    if (e.metaKey) {
      const url = urlAt(l);
      if (url) ipc({ type: 'open_url', url: url });
      return;
    }
    const task = TASK.exec(l.text);
    if (task && l.col >= task[1].length && l.col <= task[1].length + 3) toggleTask(l);
  });

  text.addEventListener('keydown', function(e) {
    if (e.isComposing) return;
    if (e.key === 'Escape') {
      e.preventDefault();
      ipc({ type: 'hide' });
      return;
    }
    // Tab and ⇧Tab move through the pane's tabs, as Tab cycles the
    // assistant's sessions: the pane is tabs, not a form.
    if (e.key === 'Tab' && !e.metaKey && !e.ctrlKey && !e.altKey) {
      e.preventDefault();
      ipc({ type: 'tab', tab: e.shiftKey ? 'calendar' : 'mail' });
      return;
    }
    if (e.key !== 'Enter' || e.shiftKey || e.altKey || e.ctrlKey) return;
    const l = caretLine();
    if (e.metaKey) {
      e.preventDefault();
      toggleTask(l);
      return;
    }
    const list = LIST.exec(l.text);
    if (!list || text.selectionStart !== text.selectionEnd) return;
    e.preventDefault();
    if (l.text.trim() === list[0].trim()) {
      // Enter on an empty item ends the list.
      replaceRange(l.start, l.start + l.text.length, '');
      return;
    }
    document.execCommand('insertText', false, '\n' + list[1] + list[2].replace(/\[[xX]\]/, '[ ]'));
  });

  function setFullscreen(on) {
    panel.classList.toggle('fullscreen', on);
    fullscreenBtn.classList.toggle('active', on);
    fullscreenBtn.querySelector('.ic-enter').style.display = on ? 'none' : '';
    fullscreenBtn.querySelector('.ic-exit').style.display = on ? '' : 'none';
    const label = on ? 'Exit fullscreen' : 'Toggle fullscreen';
    fullscreenBtn.title = label + (fullscreenChord ? ' (' + fullscreenChord + ')' : '');
  }

  closeBtn.addEventListener('click', function() { ipc({ type: 'hide' }); });
  revealBtn.addEventListener('click', function() { ipc({ type: 'reveal' }); });
  fullscreenBtn.addEventListener('click', function() { ipc({ type: 'fullscreen' }); });

  // The column's left edge moves while it's dragged, so screenX is the stable
  // coordinate. Live resizes go out at most once per frame; Rust persists the
  // final width.
  let dragPointer = null;
  let dragStartX = 0;
  let dragStartWidth = 0;
  let dragWidth = 0;
  let dragFrame = 0;

  function queueResize(width) {
    dragWidth = Math.max(0, Math.round(width));
    if (dragFrame) return;
    dragFrame = requestAnimationFrame(function() {
      dragFrame = 0;
      ipc({ type: 'resize', width: dragWidth });
    });
  }

  handle.addEventListener('pointerdown', function(e) {
    if (!e.isPrimary || e.button !== 0) return;
    e.preventDefault();
    dragPointer = e.pointerId;
    dragStartX = e.screenX;
    dragStartWidth = Math.round(window.innerWidth);
    dragWidth = dragStartWidth;
    handle.setPointerCapture(e.pointerId);
    handle.classList.add('active');
    document.body.classList.add('resizing');
  });

  handle.addEventListener('pointermove', function(e) {
    if (e.pointerId !== dragPointer) return;
    e.preventDefault();
    queueResize(dragStartWidth + dragStartX - e.screenX);
  });

  function finishResize(e) {
    if (e.pointerId !== dragPointer) return;
    e.preventDefault();
    if (e.type !== 'pointercancel') {
      dragWidth = Math.max(0, Math.round(dragStartWidth + dragStartX - e.screenX));
    }
    if (dragFrame) {
      cancelAnimationFrame(dragFrame);
      dragFrame = 0;
    }
    if (handle.hasPointerCapture(e.pointerId)) handle.releasePointerCapture(e.pointerId);
    dragPointer = null;
    handle.classList.remove('active');
    document.body.classList.remove('resizing');
    ipc({ type: 'resize_end', width: dragWidth });
  }

  handle.addEventListener('pointerup', finishResize);
  handle.addEventListener('pointercancel', finishResize);
  handle.addEventListener('dblclick', function(e) {
    e.preventDefault();
    ipc({ type: 'resize_reset' });
  });
  handle.addEventListener('keydown', function(e) {
    if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
    e.preventDefault();
    const step = (e.shiftKey ? 64 : 16) * (e.key === 'ArrowLeft' ? 1 : -1);
    ipc({ type: 'resize_end', width: Math.round(window.innerWidth) + step });
  });

  window.addEventListener('focus', function() { ipc({ type: 'focus' }); });
  window.addEventListener('blur', function() { ipc({ type: 'blur' }); });

  window.__notesLoad = function(wsId, name, value, reveal) {
    const same = wsId === ws;
    if (!same) save();
    ws = wsId;
    spaceEl.textContent = name;
    text.readOnly = false;
    text.classList.remove('error');
    text.placeholder = placeholder;
    if (value !== text.value) {
      keep = new Set();
      const pos = Math.min(text.selectionStart, value.length);
      text.value = value;
      if (same) text.setSelectionRange(pos, pos);
    }
    saved = value;
    render();
    renderTasks();
    if (reveal) {
      text.setSelectionRange(value.length, value.length);
      scroll.scrollTop = scroll.scrollHeight;
    } else if (!same) {
      scroll.scrollTop = 0;
    }
  };

  window.__notesError = function(wsId, name, message) {
    save();
    ws = null;
    spaceEl.textContent = name;
    text.value = '';
    text.readOnly = true;
    text.classList.add('error');
    text.placeholder = message;
    render();
    renderTasks();
  };

  // The pane is hiding: save, and drop the ticked tasks kept on show.
  window.__notesFlush = function() {
    save();
    keep = new Set();
    renderTasks();
  };

  document.getElementById('tab-mail').addEventListener('click', function() { ipc({ type: 'tab', tab: 'mail' }); });
  document.getElementById('tab-cal').addEventListener('click', function() { ipc({ type: 'tab', tab: 'calendar' }); });
  // Tab from anywhere else in the pane too (the text's own handler runs first).
  document.addEventListener('keydown', function(e) {
    if (e.key !== 'Tab' || e.defaultPrevented || e.metaKey || e.ctrlKey || e.altKey) return;
    e.preventDefault();
    ipc({ type: 'tab', tab: e.shiftKey ? 'calendar' : 'mail' });
  });

  window.__paneUnread = function(n) {
    document.getElementById('unread').textContent = n ? String(n) : '';
  };

  // Account key -> colour, for links to mail conversations.
  window.__notesMailAccounts = function(colors) {
    mailColors = colors || {};
    render();
    renderTasks();
  };

  window.__notesFocus = function() {
    if (area !== 'tasks') {
      text.focus();
    } else if (!tasksEl.contains(document.activeElement)) {
      expand();
      addEl.focus();
    }
  };

  // ---- Open tasks above the text (spec 003-tasks): the `- [ ]` lines of the
  // buffer, due ones grouped by date, the rest behind one line; the head line
  // folds the block and says what is in it.
  const tasksEl = document.getElementById('tasks');
  const headEl = document.getElementById('tasks-head');
  const sumEl = document.getElementById('tasks-sum');
  const addEl = document.getElementById('add');
  const listEl = document.getElementById('list');
  // The due field, and the `>words` the page asks Rust to turn into a date.
  const DUE = /📅\s*(\d{4}-\d{2}-\d{2})/;
  const TRAIL_DUE = /(^|\s)>([^>\n]+)$/;
  const FOLD_KEY = 'octoweb.tasks.collapsed';
  let area = 'notes';     // where the keyboard is: the text, or the tasks
  let collapsed = false;
  try { collapsed = localStorage.getItem(FOLD_KEY) === '1'; } catch (e) { /* no storage here */ }
  let showUndated = false;
  let keep = new Set();   // line indexes ticked here, kept listed until the pane hides
  let shown = [];         // tasks in the order the list shows them
  let focusLine = -1;
  let captureChord = '';
  let dueSeq = 0;
  const dueWait = {};
  let adding = false;
  let curLine = null;     // the caret's line in the text: start offset and text
  let queued = false;

  function p2(n) { return (n < 10 ? '0' : '') + n; }

  function isoToday() {
    const d = new Date();
    return d.getFullYear() + '-' + p2(d.getMonth() + 1) + '-' + p2(d.getDate());
  }

  function dayNum(iso) {
    const a = iso.split('-');
    return Math.round(Date.UTC(+a[0], +a[1] - 1, +a[2]) / 864e5);
  }

  function dueLabel(iso, today) {
    const n = dayNum(iso) - dayNum(today);
    if (n === 0) return 'Today';
    if (n === 1) return 'Tomorrow';
    if (n === -1) return 'Yesterday';
    const a = iso.split('-');
    const d = new Date(+a[0], +a[1] - 1, +a[2]);
    if (n > 1 && n <= 6) return d.toLocaleDateString('en-US', { weekday: 'short' });
    const opts = { month: 'short', day: 'numeric' };
    if (a[0] !== today.slice(0, 4)) opts.year = 'numeric';
    return d.toLocaleDateString('en-US', opts);
  }

  // Same grammar as `notes::tasks`: the due date is the first 📅 date in the line.
  function parseTasks(v) {
    const out = [];
    v.split('\n').forEach(function(l, i) {
      const t = TASK.exec(l);
      if (!t) return;
      const rest = l.slice(t[0].length);
      const d = DUE.exec(rest);
      out.push({
        idx: i,
        line: l,
        done: t[2] !== ' ',
        due: d ? d[1] : null,
        text: rest.replace(DUE, ' ').replace(/\s+/g, ' ').trim()
      });
    });
    return out;
  }

  function attr(s) { return esc(s).replace(/"/g, '&quot;'); }

  // Links show their label only; a bare URL shows as is.
  function rowInline(s) {
    let out = '';
    let last = 0;
    LINK.lastIndex = 0;
    let m;
    while ((m = LINK.exec(s))) {
      const url = m[1] || m[2] || m[3];
      const label = m[0].charAt(0) === '[' ? m[0].slice(1, m[0].indexOf(']')) : m[0].charAt(0) === '<' ? url : m[0];
      const mail = MAIL_LINK.exec(url);
      const style = mail && mailColors[mail[1]] ? ' style="color:' + attr(mailColors[mail[1]]) + '"' : '';
      out += esc(s.slice(last, m.index)) + '<span class="link" data-url="' + attr(url) + '"' + style + '>' + esc(label) + '</span>';
      last = m.index + m[0].length;
    }
    return out + esc(s.slice(last));
  }

  function rowHtml(t, today, over) {
    const lab = t.due ? dueLabel(t.due, today) : '';
    return '<div class="row' + (t.done ? ' isdone' : '') + (t.idx === focusLine ? ' focus' : '') + '" data-i="' + t.idx + '">' +
      '<span class="rbox"><i></i></span><span class="rtext">' + rowInline(t.text) + '</span>' +
      (lab ? '<span class="due' + (over ? ' over' : '') + '">' + esc(lab) + '</span>' : '') + '</div>';
  }

  function renderTasks() {
    tasksEl.hidden = ws === null;   // an unreadable file shows its error in the text
    if (ws === null) {
      shown = [];
      listEl.innerHTML = '';
      return;
    }
    const today = isoToday();
    const open = parseTasks(text.value).filter(function(t) { return !t.done || keep.has(t.idx); });
    const byDue = function(a, b) { return a.due < b.due ? -1 : a.due > b.due ? 1 : a.idx - b.idx; };
    const over = open.filter(function(t) { return t.due && t.due < today; }).sort(byDue);
    const now = open.filter(function(t) { return t.due === today; }).sort(byDue);
    const later = open.filter(function(t) { return t.due && t.due > today; }).sort(byDue);
    const undated = open.filter(function(t) { return !t.due; });
    const sum = [];
    if (over.length) sum.push('<span class="over">' + over.length + ' overdue</span>');
    if (now.length) sum.push(now.length + ' today');
    if (later.length) sum.push(later.length + ' upcoming');
    if (undated.length) sum.push(undated.length + ' without a date');
    sumEl.innerHTML = sum.length ? '· ' + sum.join(' · ') : '';
    const groups = [['Overdue', over, true], ['Today', now, false], ['Upcoming', later, false]];
    if (showUndated) groups.push(['No date', undated, false]);
    shown = [];
    groups.forEach(function(g) { shown = shown.concat(g[1]); });
    if (!shown.some(function(t) { return t.idx === focusLine; })) focusLine = shown.length ? shown[0].idx : -1;
    let h = '';
    groups.forEach(function(g) {
      if (!g[1].length) return;
      h += '<div class="grp' + (g[2] ? ' over' : '') + '">' + g[0] + '</div>';
      g[1].forEach(function(t) { h += rowHtml(t, today, g[2] && !t.done); });
    });
    if (undated.length) {
      h += '<button class="more" type="button" data-act="undated">' + (showUndated ? 'Hide tasks without a date' :
        undated.length + (undated.length === 1 ? ' task' : ' tasks') + ' without a date') + '</button>';
    }
    listEl.innerHTML = h;
  }

  // Typing re-reads the tasks once per frame, not once per key.
  function queueTasks() {
    if (queued) return;
    queued = true;
    requestAnimationFrame(function() {
      queued = false;
      renderTasks();
    });
  }
  text.addEventListener('input', queueTasks);

  function setCollapsed(on) {
    collapsed = on;
    tasksEl.classList.toggle('collapsed', on);
    headEl.setAttribute('aria-expanded', on ? 'false' : 'true');
    try { localStorage.setItem(FOLD_KEY, on ? '1' : '0'); } catch (e) { /* kept for the session */ }
  }
  function expand() { if (collapsed) setCollapsed(false); }
  headEl.addEventListener('click', function() { setCollapsed(!collapsed); });
  setCollapsed(collapsed);

  // Rust's ⌥⌘T and ⌘⇧J toggle by where the keyboard is.
  function setArea(a) {
    if (a === area) return;
    area = a;
    ipc({ type: 'view', view: a });
  }
  addEl.addEventListener('focus', function() { setArea('tasks'); });
  listEl.addEventListener('focus', function() { setArea('tasks'); });
  text.addEventListener('focus', function() { setArea('notes'); });

  function revealFocus() {
    const row = listEl.querySelector('.row.focus');
    if (row) row.scrollIntoView({ block: 'nearest' });
  }

  // The buffer with `entry` under today's heading, the caret where it was.
  function appendLine(date, entry, reveal) {
    const a = text.selectionStart;
    const b = text.selectionEnd;
    text.value = appendEntry(text.value, date, entry);
    if (reveal) text.setSelectionRange(text.value.length, text.value.length);
    else text.setSelectionRange(a, b);
    render();
    save();
    renderTasks();
  }

  // Flip the box in the buffer line; saved at once, not after the typing pause.
  function tickLine(idx) {
    const lines = text.value.split('\n');
    const l = lines[idx];
    const t = l === undefined ? null : TASK.exec(l);
    if (!t) return;
    const done = t[2] === ' ';
    const a = text.selectionStart;
    const b = text.selectionEnd;
    lines[idx] = l.slice(0, t[1].length + 1) + (done ? 'x' : ' ') + l.slice(t[1].length + 2);
    text.value = lines.join('\n');
    text.setSelectionRange(a, b);
    keep.add(idx);
    render();
    save();
    const mail = /octoweb-mail:\/\/[^)\s]+/.exec(l);
    if (mail) ipc({ type: 'task', url: mail[0], done: done });
    renderTasks();
  }

  // Put the caret after the line's box and bring it into view.
  function openLine(idx) {
    const lines = text.value.split('\n');
    if (idx < 0 || idx >= lines.length) return;
    let at = 0;
    for (let i = 0; i < idx; i++) at += lines[i].length + 1;
    const t = TASK.exec(lines[idx]);
    const pos = at + (t ? t[0].length : 0);
    text.focus();
    text.setSelectionRange(pos, pos);
    const walker = document.createTreeWalker(hl, NodeFilter.SHOW_TEXT);
    let off = pos;
    let node;
    while ((node = walker.nextNode())) {
      if (off <= node.length) break;
      off -= node.length;
    }
    if (!node) return;
    const range = document.createRange();
    range.setStart(node, off);
    range.setEnd(node, Math.min(off + 1, node.length));
    const r = range.getBoundingClientRect();
    const box = scroll.getBoundingClientRect();
    const top = r.top - box.top + scroll.scrollTop;
    const bottom = top + (r.height || 21);
    const margin = 24;
    if (top < scroll.scrollTop + margin || bottom > scroll.scrollTop + scroll.clientHeight - margin) {
      scroll.scrollTop = Math.max(0, top - scroll.clientHeight / 3);
    }
  }

  // Rust: `tasks` puts the keyboard in the task block (on `focus`, a task
  // line, when given), `notes` in the text.
  window.__notesView = function(v, focus) {
    if (v !== 'tasks') {
      text.focus();
      return;
    }
    expand();
    const hit = typeof focus === 'string' && parseTasks(text.value).find(function(t) { return t.line === focus; });
    if (hit) {
      focusLine = hit.idx;
      if (!hit.due) showUndated = true;
      renderTasks();
      listEl.focus();
      revealFocus();
      return;
    }
    renderTasks();
    scroll.scrollTop = 0;
    addEl.focus();
  };

  window.__notesDay = renderTasks;

  window.__paneDue = function(n) {
    document.getElementById('due').textContent = n ? String(n) : '';
  };

  // ---- Dates: ask Rust what the words after `>` start with; the rest stays text.
  function askDue(phrase, done) {
    const id = ++dueSeq;
    const timer = setTimeout(function() {
      delete dueWait[id];
      done(null, '');
    }, 5000);
    dueWait[id] = function(iso, rest) {
      clearTimeout(timer);
      done(iso, rest);
    };
    ipc({ type: 'due', id: id, phrase: phrase });
  }

  window.__notesDue = function(id, iso, rest) {
    const cb = dueWait[id];
    if (!cb) return;
    delete dueWait[id];
    cb(iso || null, rest || '');
  };

  // Port of `notes::append`: the entry goes under the day's heading.
  function appendEntry(value, date, entry) {
    const heading = '## ' + date;
    let out = value.trimEnd();
    const lines = out.split('\n');
    let last = null;
    for (let i = lines.length - 1; i >= 0; i--) {
      if (lines[i].indexOf('## ') === 0) { last = lines[i]; break; }
    }
    if (last === null || last.trimEnd() !== heading) {
      if (out) out += '\n\n';
      out += heading;
    }
    const lastLine = out.split('\n').pop();
    const sep = lastLine.indexOf('- ') === 0 && entry.indexOf('- ') === 0 ? '\n' : '\n\n';
    return out + sep + entry + '\n';
  }

  // Rust's appends while the panel shows the notes (a capture, mail's To
  // notes, an agent's task) go into the buffer, which may hold unsaved
  // typing, rather than into the file under it.
  window.__notesAppend = function(date, entry, reveal) {
    if (ws === null) return;
    appendLine(date, entry, reveal);
    if (reveal && area === 'notes') scroll.scrollTop = scroll.scrollHeight;
  };

  function flashAdd() {
    addEl.classList.add('bad');
    setTimeout(function() { addEl.classList.remove('bad'); }, 1200);
  }

  // `Pay rent >fri`, `>fri pay rent` and `Pay >fri rent` all date the task;
  // words after `>` that are not a date stay as typed.
  function addTask() {
    if (adding || ws === null) return;
    const raw = addEl.value.replace(/[\[\]]/g, '').trim();
    if (!raw) return;
    const m = TRAIL_DUE.exec(raw);
    const lead = m ? raw.slice(0, m.index).trim() : raw;
    const finish = function(iso, rest) {
      adding = false;
      const body = iso ? (lead + ' ' + rest).trim() : raw;
      if (!body) {
        flashAdd();
        return;
      }
      appendLine(isoToday(), '- [ ] ' + body + (iso ? ' 📅 ' + iso : ''), false);
      addEl.value = '';
      addEl.focus();
    };
    if (m) {
      adding = true;
      askDue(m[2].trim(), finish);
    } else {
      finish(null, '');
    }
  }

  addEl.addEventListener('keydown', function(e) {
    if (e.isComposing) return;
    if (e.key === 'Enter') {
      e.preventDefault();
      addTask();
    } else if (e.key === 'Escape') {
      e.preventDefault();
      if (addEl.value) addEl.value = '';
      else ipc({ type: 'hide' });
    } else if (e.key === 'ArrowDown') {
      e.preventDefault();
      if (shown.length) {
        listEl.focus();
        revealFocus();
      } else {
        text.focus();
      }
    }
  });

  function move(d) {
    let i = shown.findIndex(function(t) { return t.idx === focusLine; });
    if (d < 0 && i <= 0) {
      addEl.focus();
      return;
    }
    if (!shown.length) return;
    i = Math.max(0, Math.min(shown.length - 1, i + d));
    focusLine = shown[i].idx;
    renderTasks();
    revealFocus();
  }

  listEl.addEventListener('keydown', function(e) {
    if (e.isComposing || e.metaKey || e.ctrlKey || e.altKey) return;
    const k = e.key;
    if (k === 'ArrowDown' || k === 'j') move(1);
    else if (k === 'ArrowUp' || k === 'k') move(-1);
    else if (k === 'x' || k === ' ') { if (focusLine >= 0) tickLine(focusLine); }
    else if (k === 'Enter') { if (focusLine >= 0) openLine(focusLine); }
    else if (k === 'n') addEl.focus();
    else if (k === 'Escape') ipc({ type: 'hide' });
    else return;
    e.preventDefault();
  });

  listEl.addEventListener('click', function(e) {
    const t = e.target;
    if (t.closest('[data-act="undated"]')) {
      showUndated = !showUndated;
      renderTasks();
      return;
    }
    const row = t.closest('.row');
    if (!row) return;
    const idx = +row.getAttribute('data-i');
    focusLine = idx;
    if (t.closest('.rbox')) {
      tickLine(idx);
      return;
    }
    const link = t.closest('.link');
    if (link && e.metaKey) {
      ipc({ type: 'open_url', url: link.getAttribute('data-url') });
      return;
    }
    openLine(idx);
  });

  // ---- `>words` at the end of a task line become a 📅 date once the caret leaves it.
  function trackCaret() {
    if (document.activeElement !== text) return;
    const l = caretLine();
    if (curLine && curLine.start !== l.start) leaveLine();
    curLine = { start: l.start, text: l.text };
  }

  function leaveLine() {
    const old = curLine;
    curLine = null;
    if (!old || ws === null) return;
    const v = text.value;
    let end = v.indexOf('\n', old.start);
    if (end < 0) end = v.length;
    checkDue(old.start, v.slice(old.start, end));
  }

  function checkDue(start, l) {
    if (!TASK.test(l)) return;
    const m = TRAIL_DUE.exec(l);
    const words = m ? m[2].trim() : '';
    if (!words) return;
    askDue(words, function(iso, rest) {
      if (iso) applyDue(start, l, m.index, (rest ? ' ' + rest : '') + ' 📅 ' + iso);
    });
  }

  // Replace the line's `>words` (from `tailAt`) with `s`, if the line is unchanged.
  function applyDue(start, l, tailAt, s) {
    const v = text.value;
    let end = v.indexOf('\n', start);
    if (end < 0) end = v.length;
    let at = -1;
    if (v.slice(start, end) === l) {
      at = start;
    } else {
      let off = 0;
      const lines = v.split('\n');
      for (let i = 0; i < lines.length; i++) {
        if (lines[i] === l) { at = off; break; }
        off += lines[i].length + 1;
      }
    }
    if (at < 0) return;
    const from = at + l.slice(0, tailAt).trimEnd().length;
    const to = at + l.length;
    const focused = document.activeElement === text;
    if (focused && caretLine().start === at) return;
    const shift = s.length - (to - from);
    const a = text.selectionStart;
    const b = text.selectionEnd;
    const map = function(p) { return p > to ? p + shift : p; };
    if (focused) {
      replaceRange(from, to, s);
    } else {
      text.value = v.slice(0, from) + s + v.slice(to);
      render();
      clearTimeout(saveTimer);
      saveTimer = setTimeout(save, SAVE_DELAY_MS);
    }
    text.setSelectionRange(map(a), map(b));
    renderTasks();
  }

  document.addEventListener('selectionchange', trackCaret);
  text.addEventListener('keyup', trackCaret);
  text.addEventListener('click', trackCaret);
  text.addEventListener('input', trackCaret);
  text.addEventListener('blur', leaveLine);

  window.__notesFullscreen = setFullscreen;

  window.__setShortcuts = function(data) {
    const actions = data && Array.isArray(data.actions) ? data.actions : [];
    const chordFor = function(id) {
      const action = actions.find(function(item) { return item.id === id; });
      return action && Array.isArray(action.keys) ? action.keys.join('') : '';
    };
    const toggle = chordFor('notes');
    closeBtn.title = 'Hide notes' + (toggle ? ' (' + toggle + ')' : '');
    fullscreenChord = chordFor('sidebar_fullscreen');
    setFullscreen(panel.classList.contains('fullscreen'));
    const capture = chordFor('note_capture');
    captureChord = capture;
    const tasksChord = chordFor('tasks');
    addEl.title = 'Add a task' + (tasksChord ? ' (' + tasksChord + ')' : '');
    placeholder = 'Notes and - [ ] tasks for this workspace, kept until you delete them.\n' +
      'Click [ ] or ⌘↩ to tick a task, ⌘-click to open a link.\n' +
      'End a task with >fri and it becomes a 📅 date.' +
      (capture ? '\n' + capture + ' on a page adds it as a task, or quotes the selection.' : '');
    if (!text.classList.contains('error')) text.placeholder = placeholder;
  };
  window.__setShortcuts(/*@@KEYBINDINGS_JSON@@*/);
})();
</script>
</body>
</html>"#
        .replace("/*@@THEME@@*/", crate::theme::CSS)
        .replace("/*@@KEYBINDINGS_JSON@@*/", keybindings_json)
}
