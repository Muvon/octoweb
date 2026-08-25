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
//! 4. **Act**. Pointer and key actions do NOT dispatch synthetic events any
//!    more: the harness only *locates* the target (top-document CSS px) and
//!    Rust delivers a trusted native event (native_input.rs). Value-setting
//!    actions (type / select_option) still act in JS — they need direct model
//!    access, not trust.
//! 5. **Effect capture**. Before acting, `__pre()` records URL, title,
//!    focus, dialogs and network position and starts a MutationObserver.
//!    After the action settles, `diff()` reports what changed — the AI gets
//!    "added text: Request sent" instead of "clicked successfully".
//!
//! The scripts evaluate to a `Promise<string>` — same mechanism as
//! `readiness_js` — resolving to `'true'` (plain success), `'true|<json>'`
//! (success + effect summary), `'{"x":..}'` (located target), or a status
//! string understood by `mcp::interpret_dom_result`.
//!
//! Timers use `setTimeout`, never `requestAnimationFrame`: rAF is suspended
//! in hidden webviews, and MCP-driven tabs are background tabs by design.

/// How long an action retries before reporting its last failure reason.
pub const RETRY_MS: u64 = 2500;

/// Watchdog ceiling for action callbacks: retry window + effect window + page-load slack.
pub const WATCHDOG_MS: u64 = RETRY_MS + EFFECT_MS + 5000;

/// How long after an action we watch the page before summarising its effect.
/// Long enough for XHR round-trips on a LAN and framework re-renders, short
/// enough that every click doesn't feel sluggish.
pub const EFFECT_MS: u64 = 450;

/// Effect-capture helpers, shared by the harness and the standalone effect
/// probe. Installs `window.__octoweb_pre` with a `diff()` method.
const EFFECT_PRE_JS: &str = r#"
  function __pre() {
    try { if (window.__octoweb_pre && window.__octoweb_pre.mo) window.__octoweb_pre.mo.disconnect(); } catch (e) {}
    function dialogs() { return document.querySelectorAll('[role=dialog],[role=alertdialog],[aria-modal=true],dialog[open]'); }
    function dialogText() { var d = dialogs(); if (!d.length) return ''; return (d[d.length - 1].innerText || '').trim().replace(/\s+/g, ' ').substring(0, 160); }
    function focusDesc() {
      var a = document.activeElement;
      while (a && a.shadowRoot && a.shadowRoot.activeElement) a = a.shadowRoot.activeElement;
      if (!a || a === document.body || a === document.documentElement) return '';
      var label = a.getAttribute('aria-label') || a.placeholder || a.name || a.id || '';
      return a.tagName.toLowerCase() + (label ? ' "' + String(label).substring(0, 40) + '"' : '');
    }
    var st = { url: location.href, title: document.title, seq: (window.__octoweb_netseq || 0),
               added: 0, removed: 0, texts: [], textLen: 0, dialogs: dialogs().length, focus: focusDesc(), mo: null };
    function pushText(t) {
      t = String(t || '').trim().replace(/\s+/g, ' ');
      if (!t || st.textLen > 400) return;
      t = t.substring(0, 120);
      if (st.texts.indexOf(t) !== -1) return;
      st.texts.push(t); st.textLen += t.length;
    }
    try {
      st.mo = new MutationObserver(function (rs) {
        for (var i = 0; i < rs.length; i++) {
          var r = rs[i];
          if (r.type === 'characterData') { pushText(r.target.data); continue; }
          st.added += r.addedNodes.length; st.removed += r.removedNodes.length;
          for (var j = 0; j < r.addedNodes.length; j++) {
            var n = r.addedNodes[j];
            if (n.nodeType === 1) { if (n.tagName !== 'SCRIPT' && n.tagName !== 'STYLE') pushText(n.innerText); }
            else if (n.nodeType === 3) pushText(n.data);
          }
        }
      });
      st.mo.observe(document.documentElement, { childList: true, subtree: true, characterData: true });
    } catch (e) {}
    st.diff = function () {
      try { if (st.mo) st.mo.disconnect(); } catch (e) {}
      var out = {};
      if (location.href !== st.url) out.url = location.href.substring(0, 200);
      if (document.title !== st.title) out.title = document.title.substring(0, 80);
      var net = (window.__octoweb_net || []).concat(window.__octoweb_res || []).filter(function (e) { return e.seq > st.seq; });
      net.sort(function (a, b) { return a.seq - b.seq; });
      if (net.length) out.net = net.slice(0, 4).map(function (e) {
        return e.method + ' ' + String(e.url).substring(0, 90) + (e.status ? ' ' + e.status : '') + (e.error ? ' ERR' : '');
      });
      if (net.length > 4) out.net.push('+' + (net.length - 4) + ' more');
      if (st.added || st.removed) out.dom = '+' + st.added + '/-' + st.removed;
      if (st.texts.length) out.text = st.texts.join(' | ').substring(0, 300);
      var d = dialogs().length;
      if (d > st.dialogs) out.dialog = dialogText();
      var f = focusDesc();
      if (f !== st.focus) out.focus = f || 'none';
      return out;
    };
    Object.defineProperty(window, '__octoweb_pre', { value: st, configurable: true, writable: true });
  }
"#;

/// Shared harness. Placeholders: `__SEL__` (JSON string or `null`),
/// `__RETRY_MS__`, `__GATE__` (per-action element checks, may retry),
/// `__STABILITY__` (`true`/`false`), `__OCCLUSION__` (`true`/`false`),
/// `__EFFECT__` (`true`: wrap `__done('true')` with an effect diff),
/// `__ACT__` (uses `el`, `rect`, `x`, `y`, `top`; must call `__done(...)`).
const HARNESS: &str = r#"
new Promise(function(__resolve){
  'use strict';
  var SEL = __SEL__;
  var DEADLINE = performance.now() + __RETRY_MS__;
  var lastErr = 'missing';
  var prevRect = null;
  __PRE__
  function __done(v) {
    if (!__EFFECT__ || v !== 'true') return __resolve(v);
    setTimeout(function () {
      var d = {}; try { d = window.__octoweb_pre ? window.__octoweb_pre.diff() : {}; } catch (e) {}
      __resolve('true|' + JSON.stringify(d));
    }, __EFFECT_MS__);
  }

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

  // Dispatch a synthetic event, isolating the PAGE's own handlers. Those run
  // synchronously inside this Promise executor; if one throws, WebKit surfaces
  // the exception and rejects the promise — so an action we DID deliver gets
  // reported as failed, and the AI retries into a double-fire. A real browser
  // reports a throwing listener and keeps dispatching, so we do the same.
  function fire(t, ev) { try { return t.dispatchEvent(ev); } catch (e) { return true; } }

  function describe(n) {
    if (!n || !n.tagName) return 'unknown element';
    var d = n.tagName.toLowerCase();
    if (n.id) d += '#' + n.id;
    else if (n.classList && n.classList.length) d += '.' + Array.prototype.slice.call(n.classList, 0, 3).join('.');
    return d.substring(0, 80);
  }

  // Element centre in TOP-document CSS px: rects inside same-origin iframes
  // are relative to the frame's viewport, so add each frame's offset. Native
  // input is delivered to the top-level webview and needs top coordinates.
  function top(el, x, y) {
    var w = el.ownerDocument.defaultView;
    while (w && w !== window && w.frameElement) {
      var fr = w.frameElement.getBoundingClientRect();
      x += fr.left + w.frameElement.clientLeft;
      y += fr.top + w.frameElement.clientTop;
      w = w.parent;
    }
    return { x: x, y: y };
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
    if (__EFFECT__) __pre();
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
        .replace("__PRE__", EFFECT_PRE_JS)
        .replace("__EFFECT_MS__", &EFFECT_MS.to_string())
        .replace("__GATE__", gate)
        .replace("__STABILITY__", if stability { "true" } else { "false" })
        .replace("__OCCLUSION__", if occlusion { "true" } else { "false" })
        .replace("__EFFECT__", "true")
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

/// Located target: top-document CSS px centre plus a short description for
/// the effect summary ("clicked button 'Request access' → …").
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct Target {
    pub x: f64,
    pub y: f64,
    #[serde(default)]
    pub desc: String,
}

/// Parse a harness result: `Ok(Some(target))` when the script located the
/// element, `Ok(None)` when it finished the action itself (`'true'`, used for
/// file inputs), `Err(status)` for the failure statuses.
pub fn parse_target(val: &str) -> Result<Option<Target>, String> {
    let inner: String = serde_json::from_str(val).unwrap_or_else(|_| val.to_string());
    let s = inner.trim();
    if s == "true" {
        return Ok(None);
    }
    if s.starts_with('{') {
        return serde_json::from_str::<Target>(s)
            .map(Some)
            .map_err(|e| format!("bad target payload: {e}"));
    }
    Err(s.to_string())
}

const LOCATE_ACT: &str = r#"
    var d = (el.getAttribute('aria-label') || el.innerText || el.value || el.placeholder || '').trim().replace(/\s+/g, ' ').substring(0, 40);
    var p = top(el, x, y);
    __resolve(JSON.stringify({ x: p.x, y: p.y, desc: el.tagName.toLowerCase() + (d ? ' "' + d + '"' : '') }));
"#;

/// Locate a click target. File inputs are activated in JS (the native click()
/// path opens the armed chooser); everything else is returned as coordinates
/// for a trusted native click.
pub fn click_locate_script(selector: &str) -> String {
    let act = format!(
        "if (el.tagName === 'INPUT' && el.type === 'file') {{ try {{ el.click(); }} catch (e) {{}} return __done('true'); }}{LOCATE_ACT}"
    );
    build(&json(selector), GATE_ENABLED, true, true, &act)
}

/// Locate a hover target (no occlusion gate — hovering the overlay is fine).
pub fn hover_locate_script(selector: &str) -> String {
    build(&json(selector), "", true, false, LOCATE_ACT)
}

/// Focus the key target (or keep the current focus when `selector` is None)
/// and arm effect capture; the key itself is delivered natively.
pub fn key_focus_script(selector: Option<&str>) -> String {
    let act = r#"
    if (SEL !== null && el.focus) try { el.focus(); } catch (e) {}
    var d = (el.getAttribute('aria-label') || el.placeholder || el.name || el.innerText || '').trim().replace(/\s+/g, ' ').substring(0, 40);
    var p = top(el, x, y);
    __resolve(JSON.stringify({ x: p.x, y: p.y, desc: el.tagName.toLowerCase() + (d ? ' "' + d + '"' : '') }));
"#;
    let sel_json = match selector {
        Some(s) => json(s),
        None => "null".into(),
    };
    build(&sel_json, "", false, false, act)
}

/// Standalone effect probe: after a native action, wait `EFFECT_MS` in-page
/// and report `window.__octoweb_pre.diff()` (empty object if none armed).
pub fn effect_script() -> String {
    format!(
        "new Promise(function(r){{ setTimeout(function(){{ var d = {{}}; try {{ d = window.__octoweb_pre ? window.__octoweb_pre.diff() : {{}}; }} catch (e) {{}} r(JSON.stringify(d)); }}, {EFFECT_MS}); }})"
    )
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
    fire(el, new IE('input', { bubbles: true, composed: true, inputType: 'insertReplacementText', data: TXT }));
    fire(el, new Event('change', { bubbles: true }));
    __done('true');
"#
    .replace("__TXT__", &json(text));
    build(&json(selector), GATE_TYPE, false, false, &act)
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
    fire(el, new Event('input', { bubbles: true }));
    fire(el, new Event('change', { bubbles: true }));
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
    __resolve('true');
"#
    .replace("__DIR__", &json(direction))
    .replace(
        "__PX__",
        &pixels.map(|p| p.to_string()).unwrap_or_else(|| "null".into()),
    );
    build(&json(selector), "", false, false, &act)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_target_variants() {
        assert_eq!(parse_target("\"true\""), Ok(None));
        let t = parse_target(
            "\"{\\\"x\\\":10.5,\\\"y\\\":20,\\\"desc\\\":\\\"button \\\\\\\"Go\\\\\\\"\\\"}\"",
        )
        .unwrap()
        .unwrap();
        assert_eq!((t.x, t.y), (10.5, 20.0));
        assert_eq!(t.desc, "button \"Go\"");
        assert_eq!(
            parse_target("\"occluded:div#overlay\""),
            Err("occluded:div#overlay".into())
        );
        assert_eq!(parse_target("\"stale\""), Err("stale".into()));
    }

    #[test]
    fn scripts_are_bare_expressions() {
        // async_eval wraps every script as `return (<expr>);` — a trailing
        // semicolon or leftover placeholder would be a SyntaxError at runtime.
        for s in [
            click_locate_script("@1"),
            hover_locate_script("#a"),
            key_focus_script(None),
            key_focus_script(Some("@2")),
            type_script("#t", "he\"llo"),
            select_option_script("#s", "v"),
            scroll_script("#c", "down", Some(10)),
            effect_script(),
        ] {
            assert!(!s.trim_end().ends_with(';'), "trailing semicolon in {s}");
            assert!(
                !s.contains("__ACT__") && !s.contains("__PRE__") && !s.contains("__EFFECT__"),
                "unreplaced placeholder"
            );
        }
    }

    #[test]
    fn selector_is_json_escaped() {
        let s = click_locate_script("a[href=\"x\"]");
        assert!(s.contains("var SEL = \"a[href=\\\"x\\\"]\";"));
        let k = key_focus_script(None);
        assert!(k.contains("var SEL = null;"));
    }
}
