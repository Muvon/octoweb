//! Builders for the injected DOM-interaction scripts used by the MCP tools
//! (click / hover / type / press_key / select_option / scroll).
//!
//! All scripts share one actionability pipeline, modelled on Playwright's
//! auto-waiting:
//!
//! 1. **Resolve** the target (`@ref` from `window.__octoweb_refs` or a CSS
//!    selector), retrying until the deadline — elements that appear late
//!    (lazy render, route transition) are found instead of erroring.
//!    `stale` refs and `invalid:` selectors fail fast: they never self-heal.
//! 2. **Stabilise** (click/hover only): wait until the bounding box stops
//!    moving between ticks. If the deadline hits while still moving
//!    (carousels never settle), act anyway — acting beats erroring.
//! 3. **Occlusion check** (click only): `elementFromPoint` at the click
//!    point must hit the element or something inside/around it, otherwise
//!    retry (overlays animating out) and finally report `occluded:<desc>`
//!    so the AI knows *what* to dismiss.
//! 4. **Act** with full pointer + mouse event sequences (`composed: true`
//!    so they cross shadow boundaries; `view`/constructors taken from the
//!    element's own window so same-origin iframe elements work).
//!
//! The scripts evaluate to a `Promise<string>` — same mechanism as
//! `readiness_js` — resolving to `'true'` or a status string understood by
//! `mcp::interpret_dom_result`.
//!
//! Timers use `setTimeout`, never `requestAnimationFrame`: rAF is suspended
//! in hidden webviews, and MCP-driven tabs are background tabs by design.

/// How long an action retries before reporting its last failure reason.
pub const RETRY_MS: u64 = 2500;

/// Watchdog ceiling for action callbacks: retry window + page-load slack.
pub const WATCHDOG_MS: u64 = RETRY_MS + 5000;

/// Shared harness. Placeholders: `__SEL__` (JSON string or `null`),
/// `__RETRY_MS__`, `__GATE__` (per-action element checks, may retry),
/// `__STABILITY__` (`true`/`false`), `__OCCLUSION__` (`true`/`false`),
/// `__ACT__` (uses `el`, `rect`, `x`, `y`; must call `__done('true')`).
const HARNESS: &str = r#"
new Promise(function(__done){
  'use strict';
  var SEL = __SEL__;
  var DEADLINE = performance.now() + __RETRY_MS__;
  var lastErr = 'missing';
  var prevRect = null;

  function resolve() {
    if (SEL === null) return { el: document.activeElement || document.body };
    if (SEL[0] === '@') {
      var m = window.__octoweb_refs;
      if (!m) return { err: 'stale' };
      var r = m.get(SEL);
      if (!r) return { err: 'stale' };
      if (!r.isConnected) return { err: 'detached' };
      return { el: r };
    }
    var el;
    try { el = document.querySelector(SEL); } catch (e) { return { err: 'invalid:' + e.message }; }
    if (!el) return { err: 'missing' };
    return { el: el };
  }

  function within(a, b) { /* b inside a, crossing shadow boundaries */
    for (var n = b; n; n = n.parentNode || n.host) if (n === a) return true;
    return false;
  }

  function describe(n) {
    if (!n || !n.tagName) return 'unknown element';
    var d = n.tagName.toLowerCase();
    if (n.id) d += '#' + n.id;
    else if (n.classList && n.classList.length) d += '.' + Array.prototype.slice.call(n.classList, 0, 3).join('.');
    return d.substring(0, 80);
  }

  function retry() {
    if (performance.now() >= DEADLINE) return __done(lastErr);
    setTimeout(attempt, 120);
  }

  function attempt() {
    var r = resolve();
    if (r.err) {
      lastErr = r.err;
      if (r.err === 'stale' || r.err.indexOf('invalid:') === 0) return __done(r.err);
      return retry();
    }
    var el = r.el;
    __GATE__
    try { el.scrollIntoView({ block: 'center', behavior: 'instant' }); } catch (e) {}
    var rect = el.getBoundingClientRect();
    if (__STABILITY__ && performance.now() < DEADLINE) {
      if (!prevRect || Math.abs(rect.left - prevRect.left) > 1 || Math.abs(rect.top - prevRect.top) > 1) {
        prevRect = rect;
        lastErr = 'detached';
        return setTimeout(attempt, 120);
      }
    }
    var x = rect.left + rect.width / 2, y = rect.top + rect.height / 2;
    if (__OCCLUSION__ && rect.width >= 2 && rect.height >= 2) {
      var root = el.getRootNode();
      var hitDoc = (root && root.elementFromPoint) ? root : el.ownerDocument;
      var hit = null;
      try { hit = hitDoc.elementFromPoint(x, y); } catch (e) {}
      if (hit && hit !== el && !within(el, hit) && !within(hit, el)) {
        lastErr = 'occluded:' + describe(hit);
        return retry();
      }
    }
    var W = el.ownerDocument.defaultView || window;
    __ACT__
  }

  attempt();
})
"#;

/// Build a script from the harness. `sel_json` must already be a JSON string
/// literal (or `null`).
fn build(sel_json: &str, gate: &str, stability: bool, occlusion: bool, act: &str) -> String {
    HARNESS
        .replace("__SEL__", sel_json)
        .replace("__RETRY_MS__", &RETRY_MS.to_string())
        .replace("__GATE__", gate)
        .replace("__STABILITY__", if stability { "true" } else { "false" })
        .replace("__OCCLUSION__", if occlusion { "true" } else { "false" })
        .replace("__ACT__", act)
}

fn json(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".into())
}

const GATE_ENABLED: &str = "if (el.disabled) { lastErr = 'disabled'; return retry(); }";
// Typing target must be a real text field. Without this check, typing into a
// non-editable element (e.g. a placeholder/label node overlaying the real
// editor) silently "succeeds" and inserts nothing — the false success that
// makes callers conclude the editor is broken.
const GATE_TYPE: &str = "if (!(el.isContentEditable || el.tagName === 'INPUT' || el.tagName === 'TEXTAREA')) { lastErr = 'noteditable'; return retry(); } if (el.disabled || el.readOnly) { lastErr = 'disabled'; return retry(); }";

pub fn click_script(selector: &str) -> String {
    // File inputs need the native click() activation path to open the chooser
    // (answered by the armed upload handler in dialog_patch).
    let act = r#"
    var o = { bubbles: true, cancelable: true, composed: true, view: W, clientX: x, clientY: y };
    var p = Object.assign({}, o, { pointerId: 1, pointerType: 'mouse', isPrimary: true });
    if (el.tagName === 'INPUT' && el.type === 'file') { el.click(); return __done('true'); }
    el.dispatchEvent(new PointerEvent('pointerdown', p));
    el.dispatchEvent(new MouseEvent('mousedown', o));
    if (el.focus) try { el.focus(); } catch (e) {}
    el.dispatchEvent(new PointerEvent('pointerup', p));
    el.dispatchEvent(new MouseEvent('mouseup', o));
    el.dispatchEvent(new MouseEvent('click', o));
    __done('true');
"#;
    build(&json(selector), GATE_ENABLED, true, true, act)
}

pub fn hover_script(selector: &str) -> String {
    let act = r#"
    var o = { bubbles: true, cancelable: true, composed: true, view: W, clientX: x, clientY: y };
    var p = Object.assign({}, o, { pointerId: 1, pointerType: 'mouse', isPrimary: true });
    el.dispatchEvent(new PointerEvent('pointerover', p));
    el.dispatchEvent(new PointerEvent('pointerenter', Object.assign({}, p, { bubbles: false })));
    el.dispatchEvent(new MouseEvent('mouseenter', Object.assign({}, o, { bubbles: false })));
    el.dispatchEvent(new MouseEvent('mouseover', o));
    el.dispatchEvent(new PointerEvent('pointermove', p));
    el.dispatchEvent(new MouseEvent('mousemove', o));
    __done('true');
"#;
    build(&json(selector), "", true, false, act)
}

pub fn type_script(selector: &str, text: &str) -> String {
    // Two paths, both replace (don't append) per the tool contract:
    //
    //  - contenteditable → a fallback chain, because no single primitive works
    //    across editors AND background tabs:
    //      1. Synthetic `paste` (DataTransfer text/plain). Model-backed editors
    //         (Lexical, DraftJS, ProseMirror) keep their own model and have
    //         paste handlers that route through it, so the model updates and any
    //         submit button gated on it enables. This is focus-independent — it
    //         works even on background tabs, where some sites refuse
    //         programmatic focus and `execCommand` (which needs document focus)
    //         silently inserts nothing or, when focus is held, double-inserts
    //         under such editors.
    //      2. If paste didn't land (plain contenteditable has no paste handler),
    //         fall back to `execCommand('insertText')` — WebKit's editing
    //         command, which plain editors honour.
    //    Existing content is cleared first (selectAll+delete) for replace
    //    semantics. We poll briefly between the two so a synchronous framework
    //    paste is detected before the execCommand fallback fires (no double).
    //
    //  - <input>/<textarea> → set value via the prototype setter (bypasses
    //    React's controlled-input cache) and fire input+change.
    //
    // Value setter must come from the element's own interface AND window
    // (iframe elements have their own constructors); WebKit brand-checks
    // prototype setters, so HTMLInputElement's setter throws on <textarea>.
    let act = r#"
    var TXT = __TXT__;
    try { el.focus(); } catch (e) {}
    if (el.isContentEditable) {
      var doc = el.ownerDocument, root = el.getRootNode();
      function selAll() {
        try {
          var s = (root.getSelection ? root : doc).getSelection();
          var rg = doc.createRange(); rg.selectNodeContents(el);
          s.removeAllRanges(); s.addRange(rg);
        } catch (e) {}
      }
      selAll();
      try { doc.execCommand('selectAll', false, null); doc.execCommand('delete', false, null); } catch (e) {}
      try {
        var dt = new DataTransfer(); dt.setData('text/plain', TXT);
        el.dispatchEvent(new ClipboardEvent('paste', { bubbles: true, cancelable: true, clipboardData: dt }));
      } catch (e) {}
      var tries = 0;
      (function check() {
        if (el.textContent.indexOf(TXT) !== -1) return __done('true');
        if (tries++ < 3) return setTimeout(check, 30);
        selAll();
        try { doc.execCommand('insertText', false, TXT); } catch (e) {}
        return __done(el.textContent.indexOf(TXT) !== -1 ? 'true' : 'typefailed');
      })();
      return;
    }
    var setter;
    if (el instanceof W.HTMLTextAreaElement) setter = Object.getOwnPropertyDescriptor(W.HTMLTextAreaElement.prototype, 'value').set;
    else if (el instanceof W.HTMLInputElement) setter = Object.getOwnPropertyDescriptor(W.HTMLInputElement.prototype, 'value').set;
    if (setter) setter.call(el, TXT); else el.value = TXT;
    var IE = W.InputEvent || Event;
    el.dispatchEvent(new IE('input', { bubbles: true, composed: true, inputType: 'insertReplacementText', data: TXT }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
    __done('true');
"#
    .replace("__TXT__", &json(text));
    build(&json(selector), GATE_TYPE, false, false, &act)
}

pub fn press_key_script(selector: Option<&str>, key: &str, modifiers: &[String]) -> String {
    let has = |m: &str| modifiers.iter().any(|x| x == m).to_string();
    // Synthetic key events never trigger native default actions, so Enter on
    // an <input> wouldn't submit its form. Emulate that default:
    //  - keydown not prevented → submit immediately (native behavior).
    //  - keydown prevented → the page claimed it, but some sites preventDefault
    //    and then ignore untrusted events (DuckDuckGo). Wait 250 ms; if the page
    //    visibly reacted (URL changed, network fired, form re-rendered) trust
    //    it, otherwise force the submit.
    let act = r#"
    if (el.focus) try { el.focus(); } catch (e) {}
    var KEY = __KEY__;
    var opts = { key: KEY, bubbles: true, cancelable: true, composed: true,
                 shiftKey: __S__, ctrlKey: __C__, altKey: __A__, metaKey: __M__ };
    var proceed = el.dispatchEvent(new W.KeyboardEvent('keydown', opts));
    if (KEY.length === 1) el.dispatchEvent(new W.KeyboardEvent('keypress', opts));
    el.dispatchEvent(new W.KeyboardEvent('keyup', opts));
    if (KEY === 'Enter' && el.tagName === 'INPUT' && el.form) {
      var f = el.form;
      if (proceed) {
        if (f.requestSubmit) f.requestSubmit(); else f.submit();
      } else {
        var href = W.location.href;
        var net = (W === window) ? (window.__octoweb_net || []) : [];
        var n0 = net.length;
        setTimeout(function () {
          try {
            if (W.location.href !== href) return;
            if (net.length > n0) return;
            if (!f.isConnected) return;
            if (f.requestSubmit) f.requestSubmit(); else f.submit();
          } catch (e) {}
        }, 250);
      }
    }
    __done('true');
"#
    .replace("__KEY__", &json(key))
    .replace("__S__", &has("shift"))
    .replace("__C__", &has("ctrl"))
    .replace("__A__", &has("alt"))
    .replace("__M__", &has("meta"));
    let sel_json = match selector {
        Some(s) => json(s),
        None => "null".into(),
    };
    build(&sel_json, "", false, false, &act)
}

pub fn select_option_script(selector: &str, value: &str) -> String {
    // Matches by option value first, visible label second.
    let act = r#"
    if (el.tagName !== 'SELECT') return __done('not-select');
    var VAL = __VAL__;
    var opt = Array.prototype.find.call(el.options, function (o) { return o.value === VAL; })
           || Array.prototype.find.call(el.options, function (o) { return o.text.trim() === VAL; });
    if (!opt) return __done('no-such-option');
    el.value = opt.value;
    el.dispatchEvent(new Event('input', { bubbles: true }));
    el.dispatchEvent(new Event('change', { bubbles: true }));
    __done('true');
"#
    .replace("__VAL__", &json(value));
    build(&json(selector), GATE_ENABLED, false, false, &act)
}

/// Scroll the nearest scrollable container of `selector` (or the element
/// itself when it scrolls). Used when `browser_scroll` gets a selector;
/// window scrolling stays on the cheap non-callback path in main.rs.
pub fn scroll_script(selector: &str, direction: &str, pixels: Option<i32>) -> String {
    let act = r#"
    var DIR = __DIR__, PX = __PX__;
    function container(n) {
      for (var c = n; c && c !== n.ownerDocument.body && c !== n.ownerDocument.documentElement; c = c.parentElement) {
        var s; try { s = W.getComputedStyle(c); } catch (e) { break; }
        if (/(auto|scroll|overlay)/.test(s.overflowY + s.overflowX)
            && (c.scrollHeight > c.clientHeight + 1 || c.scrollWidth > c.clientWidth + 1)) return c;
      }
      return n.ownerDocument.scrollingElement || n.ownerDocument.documentElement;
    }
    var c = container(el);
    var amt = PX !== null ? PX : Math.max(c.clientHeight - 100, 50);
    if (DIR === 'top') c.scrollTop = 0;
    else if (DIR === 'bottom') c.scrollTop = c.scrollHeight;
    else if (DIR === 'up') c.scrollTop -= amt;
    else c.scrollTop += amt;
    __done('true');
"#
    .replace("__DIR__", &json(direction))
    .replace(
        "__PX__",
        &pixels.map(|p| p.to_string()).unwrap_or_else(|| "null".into()),
    );
    build(&json(selector), "", false, false, &act)
}
