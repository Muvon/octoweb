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
</style>
</head>
<body>
<div id="panel">
  <div id="resize-handle" role="separator" aria-label="Resize notes" aria-orientation="vertical" tabindex="0"></div>
  <div id="bar">
    <div class="seg" role="tablist">
      <button class="tab active" id="tab-notes" type="button" role="tab" aria-selected="true">Notes</button>
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
      const pos = Math.min(text.selectionStart, value.length);
      text.value = value;
      if (same) text.setSelectionRange(pos, pos);
    }
    saved = value;
    render();
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
  };

  window.__notesFlush = save;

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
  };

  window.__notesFocus = function() {
    text.focus();
  };

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
    placeholder = 'Notes and - [ ] tasks for this workspace, kept until you delete them.\n' +
      'Click [ ] or ⌘↩ to tick a task, ⌘-click to open a link.' +
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
