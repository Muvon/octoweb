//! Keyboard link following (⌘⇧F).
//!
//! Evaluated on demand in the active tab — not part of `COMBINED_SCRIPT`,
//! because nothing here is needed until the user asks for it and the DOM walk
//! must see the page as it is *now*, not as it was at document-start.
//!
//! Draws a short label next to every clickable element in the viewport; the
//! user types a label to activate it. Running it a second time toggles the
//! overlay off, so the same chord opens and closes.
//!
//! Activation is a plain `el.click()`, deliberately NOT
//! `dom_actions::click_locate_script`: that wraps the whole MCP harness
//! (selector retry, actionability gate, effect capture, settle window) around
//! a click, which is right for an agent driving a page it cannot see and wrong
//! for an element the user just picked off the screen.

/// Toggle the hint overlay in the page this is evaluated in.
pub const SCRIPT: &str = r#"(function () {
  var NS = '__octoweb_hints';
  if (window[NS]) { window[NS].cancel(); return; }

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

  var targets = collect();
  if (!targets.length) return;
  var codes = labelsFor(targets.length);

  // Closed shadow root: the page's own CSS and scripts cannot restyle, read,
  // or remove the overlay.
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

  for (var i = 0; i < targets.length; i++) {
    var b = document.createElement('div');
    b.className = 'h';
    b.textContent = codes[i];
    // Clamped so a badge for an element at the very edge stays on screen.
    b.style.left = Math.max(0, Math.min(targets[i].x, window.innerWidth - 28)) + 'px';
    b.style.top = Math.max(0, Math.min(targets[i].y, window.innerHeight - 16)) + 'px';
    root.appendChild(b);
    targets[i].badge = b;
  }
  document.documentElement.appendChild(host);

  var typed = '';
  var scrollAt = [window.scrollX, window.scrollY];
  function onScroll() {
    if (window.scrollX !== scrollAt[0] || window.scrollY !== scrollAt[1]) cancel();
  }
  // Every same-origin window, so the keys are caught wherever focus sits.
  var windows = [window];
  var docs = documents();
  for (var w = 1; w < docs.length; w++) {
    try { if (docs[w].defaultView) windows.push(docs[w].defaultView); } catch (e) { /* gone */ }
  }

  function cancel() {
    for (var i = 0; i < windows.length; i++) {
      try { windows[i].removeEventListener('keydown', onKey, true); } catch (e) { /* gone */ }
    }
    try { window.removeEventListener('resize', cancel, true); } catch (e) { /* gone */ }
    try { window.removeEventListener('scroll', onScroll, true); } catch (e) { /* gone */ }
    if (host.parentNode) host.parentNode.removeChild(host);
    try { delete window[NS]; } catch (e) { window[NS] = undefined; }
  }

  function activate(t, newTab) {
    var el = t.el;
    cancel();
    // Same message and same guard the target="_blank" interceptor in
    // COMBINED_SCRIPT uses — one new-tab path, not two.
    var href = el.tagName === 'A' ? el.href : null;
    if (newTab && href && !href.startsWith('javascript:')) {
      window.ipc.postMessage(JSON.stringify({ type: 'open_new_tab', url: href }));
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
    e.preventDefault();
    e.stopImmediatePropagation();
    var k = e.key;
    if (k === 'Escape') { cancel(); return; }
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
    if (!matches.length) { cancel(); return; }
    // Fixed-width codes, so a full-length match is the only match.
    if (typed.length === codes[0].length) activate(targets[matches[0]], e.shiftKey);
  }

  for (var j = 0; j < windows.length; j++) {
    try { windows[j].addEventListener('keydown', onKey, true); } catch (e) { /* gone */ }
  }
  // Badges are placed in viewport coordinates and go stale the moment the page
  // moves under them; dropping the overlay beats pointing at the wrong thing.
  // Compared against the recorded offset rather than firing on any scroll
  // event: capture-phase catches inner scrollers too, and a page with a ticker
  // or a scroll-driven animation would otherwise cancel the overlay instantly.
  window.addEventListener('resize', cancel, true);
  window.addEventListener('scroll', onScroll, true);

  window[NS] = { cancel: cancel };
})();"#;
