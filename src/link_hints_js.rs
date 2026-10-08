//! Keyboard link following (⌘⇧F).
//!
//! Evaluated on demand in the active tab — not part of `COMBINED_SCRIPT`,
//! because nothing here is needed until the user asks for it and the DOM walk
//! must see the page as it is *now*, not as it was at document-start.
//!
//! Draws a short label next to every clickable element in the viewport; the
//! user types a label to activate it. By default ([`Mode::Once`]) the overlay
//! closes after one use, on scroll, and on any other key, and the chord
//! toggles it. With `link_hints_sticky` it stays up ([`Mode::On`]): badges
//! are re-placed after clicks, scrolling and DOM changes, step aside while a
//! text field has focus, and are restored on every page load in the tab
//! ([`Mode::Restore`]) until the chord turns them off ([`Mode::Off`]).
//!
//! Activation is a plain `el.click()`, deliberately NOT
//! `dom_actions::click_locate_script`: that wraps the whole MCP harness
//! (selector retry, actionability gate, effect capture, settle window) around
//! a click, which is right for an agent driving a page it cannot see and wrong
//! for an element the user just picked off the screen.

/// What an evaluation of [`script`] does to the overlay in the page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    /// Toggle a one-shot overlay.
    Once,
    /// Open the sticky overlay, taking focus away from a text field.
    On,
    /// Bring the sticky overlay back after a page load; a no-op when it is up.
    Restore,
    /// Close whatever overlay is open.
    Off,
}

/// The script for `mode`, ready for `evaluate_script`.
pub fn script(mode: Mode) -> String {
    let mode = match mode {
        Mode::Once => "once",
        Mode::On => "on",
        Mode::Restore => "restore",
        Mode::Off => "off",
    };
    format!("{SCRIPT}('{mode}');")
}

const SCRIPT: &str = r#"(function (mode) {
  var NS = '__octoweb_hints';
  var sticky = mode === 'on' || mode === 'restore';
  var live = window[NS];
  if (live) {
    // A sticky overlay is refreshed in place; every other combination closes
    // what is open, and ⌘⇧F without sticky hints is a toggle.
    if (live.sticky && sticky) { live.wake(mode === 'on'); return; }
    live.cancel();
    if (!sticky) return;
  }
  if (mode === 'off') return;

  // Home row only — every label is typed without moving the hands.
  var ALPHABET = 'asdfghjkl';
  var SELECTOR = [
    'a[href]', 'button', 'input:not([type=hidden])', 'select', 'textarea',
    'summary', 'label[for]',
    '[role=button]', '[role=link]', '[role=menuitem]', '[role=menuitemcheckbox]',
    '[role=menuitemradio]', '[role=tab]', '[role=checkbox]', '[role=radio]',
    '[role=switch]', '[role=option]', '[role=combobox]', '[role=searchbox]',
    '[role=textbox]',
    '[onclick]', '[contenteditable=""]', '[contenteditable=true]', '[tabindex]'
  ].join(',');
  // Guards a pathological page from freezing the UI mid-walk.
  var MAX_TARGETS = 500;
  // Sticky badges are re-placed once scrolling and DOM changes go quiet for
  // SETTLE_MS; a page that never goes quiet still gets them every MAX_STALE_MS.
  var SETTLE_MS = 120;
  var MAX_STALE_MS = 500;
  // Inputs that take a click, not typing — hints stay up while they have focus.
  var NOT_TEXT = ['button', 'submit', 'reset', 'checkbox', 'radio', 'image', 'file', 'range', 'color'];
  // Every same-origin document, top frame first. Cross-origin frames throw on
  // contentDocument and are skipped — unreachable, same limit the snapshot
  // tool reports.
  function documents() {
    var out = [document];
    var frames = document.querySelectorAll('iframe,frame');
    for (var i = 0; i < frames.length; i++) {
      try {
        var d = frames[i].contentDocument;
        if (d && d.body) out.push(d);
      } catch (e) { /* cross-origin */ }
    }
    return out;
  }

  // Viewport rect of `el` in TOP-frame coordinates: an element inside an
  // iframe reports coordinates relative to that iframe, so its badge would
  // land in the wrong place without the frame's own offset added.
  function frameOffset(doc) {
    if (doc === document) return { x: 0, y: 0 };
    try {
      var r = doc.defaultView.frameElement.getBoundingClientRect();
      return { x: r.left, y: r.top };
    } catch (e) { return null; }
  }

  function visible(el, doc, off) {
    var r = el.getBoundingClientRect();
    if (r.width < 2 || r.height < 2) return null;
    // Clip against the element's own view first. An element scrolled out of a
    // short iframe still has a frame-local rect inside the top viewport once
    // the frame offset is added, so without this it gets a badge painted over
    // whatever is really at those coordinates. No-op for the top document.
    var fw = doc.defaultView;
    if (fw !== window) {
      if (r.left >= fw.innerWidth || r.top >= fw.innerHeight) return null;
      if (r.left + r.width <= 0 || r.top + r.height <= 0) return null;
    }
    var x = r.left + off.x, y = r.top + off.y;
    if (x + r.width <= 0 || y + r.height <= 0) return null;
    if (x >= window.innerWidth || y >= window.innerHeight) return null;
    var cs;
    try { cs = doc.defaultView.getComputedStyle(el); } catch (e) { return null; }
    if (!cs || cs.visibility === 'hidden' || cs.opacity === '0') return null;
    if (el.disabled) return null;
    if (el.getAttribute && el.getAttribute('aria-hidden') === 'true') return null;
    // Occlusion: whatever is actually painted at the element's centre must be
    // the element or something inside it. Skips targets under a cookie banner
    // or a modal backdrop, which are the ones a click would not reach anyway.
    var cx = Math.min(Math.max(r.left + r.width / 2, 1), (doc.defaultView.innerWidth || 1) - 1);
    var cy = Math.min(Math.max(r.top + r.height / 2, 1), (doc.defaultView.innerHeight || 1) - 1);
    var hit;
    try { hit = doc.elementFromPoint(cx, cy); } catch (e) { hit = null; }
    if (hit && hit !== el && !el.contains(hit) && !hit.contains(el)) return null;
    return { x: x, y: y };
  }

  function collect() {
    var out = [];
    var docs = documents();
    for (var d = 0; d < docs.length && out.length < MAX_TARGETS; d++) {
      var doc = docs[d];
      var off = frameOffset(doc);
      if (!off) continue;
      var els;
      try { els = doc.querySelectorAll(SELECTOR); } catch (e) { continue; }
      for (var i = 0; i < els.length && out.length < MAX_TARGETS; i++) {
        var el = els[i];
        // tabindex="-1" is "focusable by script", not "clickable by the user".
        if (el.hasAttribute('tabindex') && el.getAttribute('tabindex') === '-1' &&
            !el.matches('a[href],button,input,select,textarea,summary,[role],[onclick]')) continue;
        var pos = visible(el, doc, off);
        if (pos) out.push({ el: el, x: pos.x, y: pos.y });
      }
    }
    return out;
  }

  // Fixed-width codes drawn from ALPHABET, so no label is a prefix of another:
  // 9 targets or fewer get one character, up to 81 get two, and so on.
  function labelsFor(n) {
    var width = 1;
    while (Math.pow(ALPHABET.length, width) < n) width++;
    var out = [];
    for (var i = 0; i < n; i++) {
      var s = '', x = i;
      for (var j = 0; j < width; j++) {
        s = ALPHABET.charAt(x % ALPHABET.length) + s;
        x = Math.floor(x / ALPHABET.length);
      }
      out.push(s);
    }
    return out;
  }

  // Deepest focused element, through open shadow roots and same-origin frames.
  function focused() {
    var el = document.activeElement;
    for (;;) {
      if (el && el.shadowRoot && el.shadowRoot.activeElement) { el = el.shadowRoot.activeElement; continue; }
      if (el && (el.tagName === 'IFRAME' || el.tagName === 'FRAME')) {
        var inner = null;
        try { inner = el.contentDocument && el.contentDocument.activeElement; } catch (e) { /* cross-origin */ }
        if (inner) { el = inner; continue; }
      }
      return el;
    }
  }

  function editable(el) {
    if (!el || el.nodeType !== 1) return false;
    if (el.isContentEditable) return true;
    if (el.tagName === 'TEXTAREA' || el.tagName === 'SELECT') return true;
    return el.tagName === 'INPUT' && NOT_TEXT.indexOf((el.type || '').toLowerCase()) === -1;
  }

  function blurField() {
    var f = focused();
    if (editable(f)) f.blur();
  }

  var first = sticky ? [] : collect();
  if (!first.length && !sticky) return;

  // Closed shadow root: the page's own CSS and scripts cannot restyle, read,
  // or remove the overlay. Mutations inside it are also invisible to the
  // page-wide MutationObserver below, so redrawing never re-triggers it.
  var host = document.createElement('div');
  host.style.cssText = 'all:initial;position:fixed;inset:0;z-index:2147483647;pointer-events:none';
  var root = host.attachShadow({ mode: 'closed' });
  var style = document.createElement('style');
  style.textContent =
    '.h{position:fixed;font:bold 11px/1 ui-monospace,Menlo,monospace;color:#1a1400;' +
    'background:linear-gradient(#ffe066,#ffc400);border:1px solid #b38600;border-radius:3px;' +
    'padding:2px 3px;text-transform:uppercase;letter-spacing:.5px;' +
    'box-shadow:0 1px 3px rgba(0,0,0,.4);white-space:nowrap}' +
    '.h b{color:#b38600;font-weight:bold}' +
    '.h.off{display:none}';
  root.appendChild(style);
  var layer = document.createElement('div');
  root.appendChild(layer);

  var targets = [];
  var codes = [];
  var typed = '';

  function draw(next) {
    layer.textContent = '';
    targets = next;
    codes = labelsFor(targets.length);
    typed = '';
    for (var i = 0; i < targets.length; i++) {
      var b = document.createElement('div');
      b.className = 'h';
      b.textContent = codes[i];
      // Clamped so a badge for an element at the very edge stays on screen.
      b.style.left = Math.max(0, Math.min(targets[i].x, window.innerWidth - 28)) + 'px';
      b.style.top = Math.max(0, Math.min(targets[i].y, window.innerHeight - 16)) + 'px';
      layer.appendChild(b);
      targets[i].badge = b;
    }
  }

  // Unchanged targets keep their labels, and a half-typed label survives.
  function same(next) {
    if (next.length !== targets.length) return false;
    for (var i = 0; i < next.length; i++) {
      if (next[i].el !== targets[i].el || next[i].x !== targets[i].x || next[i].y !== targets[i].y) return false;
    }
    return true;
  }

  // Badges hide while what they point at moves, and while a text field owns
  // the keyboard.
  var moving = false;
  function sync() {
    layer.style.display = moving || editable(focused()) ? 'none' : '';
  }
  // focusout fires before the next element takes focus.
  function onFocus() { setTimeout(sync, 0); }

  var timer = 0;
  var staleSince = 0;
  function schedule() {
    var now = Date.now();
    if (!timer) staleSince = now;
    clearTimeout(timer);
    timer = setTimeout(refresh, Math.max(0, Math.min(SETTLE_MS, staleSince + MAX_STALE_MS - now)));
  }

  function refresh() {
    clearTimeout(timer);
    timer = 0;
    if (document.hidden) return; // visibilitychange refreshes on return
    if (!host.isConnected) document.documentElement.appendChild(host);
    listen();
    var next = collect();
    if (!same(next)) draw(next);
    moving = false;
    sync();
  }

  var scrollAt = [window.scrollX, window.scrollY];
  function onScroll(e) {
    if (sticky) {
      // Only scrolls that carry a target hide the badges: a ticker scrolling
      // on its own elsewhere on the page would otherwise blank them for good.
      var s = e.target;
      for (var i = 0; i < targets.length && s && s.contains; i++) {
        if (s.contains(targets[i].el)) { moving = true; sync(); break; }
      }
      schedule();
      return;
    }
    if (window.scrollX !== scrollAt[0] || window.scrollY !== scrollAt[1]) cancel();
  }
  function onResize() {
    if (!sticky) { cancel(); return; }
    moving = true;
    sync();
    schedule();
  }
  function onVisible() { if (!document.hidden) refresh(); }
  var observer = sticky ? new MutationObserver(schedule) : null;

  // Every same-origin window, so the keys are caught wherever focus sits.
  // Re-run on refresh: frames come and go while a sticky overlay is up.
  var windows = [];
  function listen() {
    var docs = documents();
    for (var i = 0; i < docs.length; i++) {
      var w = docs[i].defaultView;
      if (!w || windows.indexOf(w) !== -1) continue;
      windows.push(w);
      try {
        w.addEventListener('keydown', onKey, true);
        if (sticky) {
          w.addEventListener('focusin', onFocus, true);
          w.addEventListener('focusout', onFocus, true);
        }
      } catch (e) { /* gone */ }
    }
  }

  function cancel() {
    for (var i = 0; i < windows.length; i++) {
      try {
        windows[i].removeEventListener('keydown', onKey, true);
        windows[i].removeEventListener('focusin', onFocus, true);
        windows[i].removeEventListener('focusout', onFocus, true);
      } catch (e) { /* gone */ }
    }
    try { window.removeEventListener('resize', onResize, true); } catch (e) { /* gone */ }
    try { window.removeEventListener('scroll', onScroll, true); } catch (e) { /* gone */ }
    try { window.removeEventListener('pagehide', cancel); } catch (e) { /* gone */ }
    document.removeEventListener('visibilitychange', onVisible);
    document.removeEventListener('DOMContentLoaded', start);
    if (observer) observer.disconnect();
    clearTimeout(timer);
    if (host.parentNode) host.parentNode.removeChild(host);
    try { delete window[NS]; } catch (e) { window[NS] = undefined; }
  }

  function activate(t, newTab) {
    var el = t.el;
    if (sticky) {
      typed = '';
      repaint();
    } else {
      cancel();
    }
    // Same message and same guard the target="_blank" interceptor in
    // COMBINED_SCRIPT uses — one new-tab path, not two.
    var href = el.tagName === 'A' ? el.href : null;
    if (newTab && href && !href.startsWith('javascript:')) {
      window.webkit.messageHandlers.ipc.postMessage(JSON.stringify({ type: 'open_new_tab', url: href }));
      return;
    }
    try { el.focus({ preventScroll: true }); } catch (e) { /* not focusable */ }
    // A real click, not a synthetic event sequence: the element is on screen
    // and was chosen by hand, so the native path is both correct and instant.
    try { el.click(); } catch (e) { /* removed mid-keystroke */ }
  }

  function repaint() {
    var matches = [];
    for (var i = 0; i < targets.length; i++) {
      var hit = codes[i].indexOf(typed) === 0;
      targets[i].badge.classList.toggle('off', !hit);
      if (hit) {
        targets[i].badge.innerHTML = '';
        var lead = document.createElement('b');
        lead.textContent = typed;
        targets[i].badge.appendChild(lead);
        targets[i].badge.appendChild(document.createTextNode(codes[i].slice(typed.length)));
        matches.push(i);
      }
    }
    return matches;
  }

  function onKey(e) {
    if (e.metaKey || e.altKey || e.ctrlKey) return; // let real shortcuts through
    var k = e.key;
    if (sticky) {
      // A text field keeps its keys; Escape hands them back to the hints.
      var f = focused();
      if (editable(f)) {
        if (k === 'Escape') {
          e.preventDefault();
          e.stopImmediatePropagation();
          f.blur();
        }
        return;
      }
      // Keys the hints have no use for stay the page's: Space and arrows
      // scroll, Escape closes the page's own popups.
      if (!targets.length) return;
      if (k === 'Escape' || k === 'Backspace') {
        if (!typed) return;
      } else if (!k || k.length !== 1 || ALPHABET.indexOf(k.toLowerCase()) === -1) {
        return;
      }
    }
    e.preventDefault();
    e.stopImmediatePropagation();
    if (k === 'Escape') {
      if (sticky) { typed = ''; repaint(); } else cancel();
      return;
    }
    if (k === 'Backspace') {
      typed = typed.slice(0, -1);
      repaint();
      return;
    }
    if (!k || k.length !== 1) return;
    var ch = k.toLowerCase();
    if (ALPHABET.indexOf(ch) === -1) { cancel(); return; }
    typed += ch;
    var matches = repaint();
    if (!matches.length) {
      if (sticky) { typed = ''; repaint(); } else cancel();
      return;
    }
    // Fixed-width codes, so a full-length match is the only match.
    if (typed.length === codes[0].length) activate(targets[matches[0]], e.shiftKey);
  }

  var started = false;
  function start() {
    started = true;
    if (mode === 'on') blurField();
    document.documentElement.appendChild(host);
    listen();
    // Badges are placed in viewport coordinates and go stale the moment the
    // page moves under them: a one-shot overlay is dropped rather than left
    // pointing at the wrong thing, a sticky one is re-placed. The one-shot
    // check compares against the recorded offset rather than firing on any
    // scroll event: capture-phase catches inner scrollers too, and a page
    // with a ticker or a scroll-driven animation would otherwise cancel the
    // overlay instantly.
    window.addEventListener('resize', onResize, true);
    window.addEventListener('scroll', onScroll, true);
    // A page put in the back/forward cache must not come back with an
    // overlay the browser no longer knows is up.
    window.addEventListener('pagehide', cancel);
    if (sticky) {
      document.addEventListener('visibilitychange', onVisible);
      observer.observe(document, {
        childList: true, subtree: true, attributes: true,
        attributeFilter: ['class', 'style', 'hidden', 'disabled', 'aria-hidden', 'open']
      });
      refresh();
    } else {
      draw(first);
    }
  }

  window[NS] = {
    sticky: sticky,
    cancel: cancel,
    wake: function (blur) {
      if (!started) return;
      if (blur) blurField();
      refresh();
    }
  };
  // Restored as soon as a new page commits, before its body exists.
  if (sticky && document.readyState === 'loading') {
    document.addEventListener('DOMContentLoaded', start, { once: true });
  } else {
    start();
  }
})"#;
