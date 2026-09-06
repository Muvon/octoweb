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

/// Watchdog ceiling for action callbacks: retry window + expectation poll +
/// page-load slack. Only hit when the JS callback is discarded outright.
pub const WATCHDOG_MS: u64 = RETRY_MS + SETTLE_MS + 5000;

/// How long after an action we watch the page before summarising its effect
/// when no expectation is given. Long enough for XHR round-trips on a LAN and
/// framework re-renders, short enough that every click doesn't feel sluggish.
pub const EFFECT_MS: u64 = 450;

/// Max time to poll for a supplied `expect` condition before reporting it
/// unmet. Matches the feel of an explicit wait without a separate call.
pub const SETTLE_MS: u64 = 6000;

/// Ceiling on waiting for trusted keystrokes to reach the page. Only hit when
/// a keyup never arrives (the page swallowed it, or the character had no key
/// code); normal typing clears in well under a tenth of this.
pub const KEY_WAIT_MS: u64 = 3000;

/// Effect-capture helpers, shared by the harness and the standalone effect
/// probe. Installs `window.__octoweb_pre` with a `diff()` method.
const EFFECT_PRE_JS: &str = r#"
  function __pre() {
    try { if (window.__octoweb_pre && window.__octoweb_pre.mo) window.__octoweb_pre.mo.disconnect(); } catch (e) {}
    // Trusted keystrokes (native_input::type_text) reach the page one IPC
    // round trip at a time, and evaluateJavaScript overtakes that queue — so
    // an effect probe sent right after typing reads a half-typed field, and so
    // does the caller's next tool call. Counting keyups on a stable global
    // (capture phase, so a handler calling stopPropagation cannot hide them)
    // lets settle wait for the last one instead of racing it.
    if (!window.__octoweb_keys) {
      window.__octoweb_keys = { want: 0, got: 0 };
      document.addEventListener('keyup', function () { window.__octoweb_keys.got++; }, true);
    }
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
          // The text budget fills after ~4 entries; past that the innerText
          // read per added node is pure waste on a busy SPA route change.
          if (st.textLen > 400) continue;
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
      // Did this action throw away text the caller typed? An SPA route change
      // can remount a composer and silently discard everything in it, and the
      // only trace in `out` is an anonymous '+2100/-1800 nodes' that reads
      // exactly like "the new view rendered" — which is also true. Only warn
      // when the text is genuinely no longer anywhere in the document, so a
      // click that legitimately consumed it (a search suggestion, a submit)
      // stays quiet.
      try {
        var typed = window.__octoweb_typed;
        if (typed && out.url) {
          var body = document.body ? document.body.innerText : '';
          var live = document.body ? document.body.innerHTML : '';
          if (body.indexOf(typed.probe) === -1 && live.indexOf(typed.probe) === -1) {
            out.lost = { sel: typed.sel, len: typed.len, head: typed.head };
            delete window.__octoweb_typed;
          }
        }
      } catch (e) {}
      // Arm-once. The marker exists to catch the action that immediately
      // follows the typing; leaving it set meant a later SPA route change --
      // where the text is legitimately gone because it was already sent --
      // reported it as lost.
      try { delete window.__octoweb_typed; } catch (e) {}
      return out;
    };
    // Is an expectation currently satisfied? `exp` is {kind, value} or null.
    st.check = function (exp) {
      if (!exp) return true;
      // innerText forces a layout flush and walks the document — only the two
      // text kinds need it, and this polls every 100ms for up to maxMs.
      var v = exp.value;
      function body() { return document.body ? document.body.innerText : ''; }
      if (exp.kind === 'text') return body().indexOf(v) !== -1;
      if (exp.kind === 'text_gone') return body().indexOf(v) === -1;
      if (exp.kind === 'url') return location.href.indexOf(v) !== -1;
      if (exp.kind === 'selector') { try { return !!document.querySelector(v); } catch (e) { return false; } }
      if (exp.kind === 'selector_gone') { try { return !document.querySelector(v); } catch (e) { return false; } }
      return true;
    };
    // Settle after an action: with no expectation, wait baseMs then snapshot the
    // diff. With one, poll until it holds (met:true) or maxMs elapses (met:false)
    // — one call decides whether the action achieved its goal.
    st.settle = function (exp, baseMs, maxMs) {
      var self = this;
      return new Promise(function (res) {
        function fin(met) { res({ diff: self.diff(), met: met, expect: exp ? (exp.kind + ':' + exp.value) : null }); }
        function measure() {
          if (!exp) { setTimeout(function () { fin(true); }, baseMs); return; }
          var start = performance.now();
          (function poll() {
            if (self.check(exp)) return fin(true);
            if (performance.now() - start >= maxMs) return fin(false);
            setTimeout(poll, 100);
          })();
        }
        var k = window.__octoweb_keys, until = performance.now() + __KEY_WAIT_MS__;
        (function keys() {
          if (!k) return measure();
          if (k.got >= k.want || performance.now() >= until) { k.want = 0; return measure(); }
          setTimeout(keys, 20);
        })();
      });
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
  // Actions that WRITE something (currently browser_type) leave a verifier on
  // window.__octoweb_verify. It runs after the settle window, not immediately,
  // because that is when a controlled React input has reverted the value or a
  // remount has thrown the field away. Setting a value and reporting success is
  // an assertion; this is the observation.
  function __vfy() {
    var f = window.__octoweb_verify;
    try { delete window.__octoweb_verify; } catch (e) { window.__octoweb_verify = null; }
    if (!f) return undefined;
    try { return f(); } catch (e) { return { ok: false, err: String((e && e.message) || e) }; }
  }
  function __done(v) {
    if (!__EFFECT__ || v !== 'true') { __vfy(); return __resolve(v); }
    var p = window.__octoweb_pre;
    if (!p) return __resolve('true|' + JSON.stringify({ diff: {}, met: !__EXPECT__, val: __vfy() }));
    p.settle(__EXPECT__, __EFFECT_MS__, __SETTLE_MS__).then(function (r) {
      r.val = __vfy();
      __resolve('true|' + JSON.stringify(r));
    });
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
/// literal (or `null`); `expect_json` a `{kind,value}` literal or `null`.
fn build(
    sel_json: &str,
    gate: &str,
    stability: bool,
    occlusion: bool,
    expect_json: &str,
    act: &str,
) -> String {
    HARNESS
        .replace("__SEL__", sel_json)
        .replace("__RETRY_MS__", &RETRY_MS.to_string())
        .replace("__PRE__", EFFECT_PRE_JS)
        .replace("__EFFECT_MS__", &EFFECT_MS.to_string())
        .replace("__SETTLE_MS__", &SETTLE_MS.to_string())
        .replace("__KEY_WAIT_MS__", &KEY_WAIT_MS.to_string())
        .replace("__EXPECT__", expect_json)
        .replace("__GATE__", gate)
        .replace("__STABILITY__", if stability { "true" } else { "false" })
        .replace("__OCCLUSION__", if occlusion { "true" } else { "false" })
        .replace("__EFFECT__", "true")
        .replace("__ACT__", act)
}

/// Translate the `expect` DSL into a `{kind,value}` JSON literal (or `null`).
/// Forms: `text:…` (default when no known prefix), `gone:…`/`text_gone:…`,
/// `url:…`, `selector:…`/`css:…`, `selector_gone:…`. A value containing a
/// colon but no known prefix (e.g. a URL) is treated as plain text.
pub fn expect_json(expect: Option<&str>) -> String {
    let Some(raw) = expect.map(str::trim).filter(|s| !s.is_empty()) else {
        return "null".into();
    };
    let (kind, value) = match raw.split_once(':') {
        Some((k, v)) => match k.trim() {
            "text" => ("text", v.trim()),
            "gone" | "text_gone" => ("text_gone", v.trim()),
            "url" => ("url", v.trim()),
            "selector" | "css" => ("selector", v.trim()),
            "selector_gone" | "gone_selector" => ("selector_gone", v.trim()),
            _ => ("text", raw),
        },
        None => ("text", raw),
    };
    format!("{{\"kind\":{},\"value\":{}}}", json(kind), json(value))
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
        "if (el.tagName === 'INPUT' && el.type === 'file') {{ try {{ el.click(); }} catch (e) {{}} return __resolve('true'); }}{LOCATE_ACT}"
    );
    build(&json(selector), GATE_ENABLED, true, true, "null", &act)
}

/// Locate a hover target (no occlusion gate — hovering the overlay is fine),
/// AND dispatch synthetic enter events. Native `mouseMoved:` gives a trusted
/// hover on the *visible* tab (real CSS `:hover`), but WebKit drops it on a
/// hidden background tab — where MCP work happens. The synthetic
/// pointerover/mouseover/mouseenter cover JS hover menus there (most don't
/// gate on isTrusted); the native move that follows still upgrades a visible
/// tab to a trusted hover.
pub fn hover_locate_script(selector: &str) -> String {
    let act = r#"
    var o = { bubbles: true, cancelable: true, composed: true, view: W, clientX: x, clientY: y };
    var p = Object.assign({}, o, { pointerId: 1, pointerType: 'mouse', isPrimary: true });
    fire(el, new PointerEvent('pointerover', p));
    fire(el, new PointerEvent('pointerenter', Object.assign({}, p, { bubbles: false })));
    fire(el, new MouseEvent('mouseover', o));
    fire(el, new MouseEvent('mouseenter', Object.assign({}, o, { bubbles: false })));
    fire(el, new PointerEvent('pointermove', p));
    fire(el, new MouseEvent('mousemove', o));
    var d = (el.getAttribute('aria-label') || el.innerText || el.value || el.placeholder || '').trim().replace(/\s+/g, ' ').substring(0, 40);
    var pt = top(el, x, y);
    __resolve(JSON.stringify({ x: pt.x, y: pt.y, desc: el.tagName.toLowerCase() + (d ? ' "' + d + '"' : '') }));
"#;
    build(&json(selector), "", true, false, "null", act)
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
    build(&sel_json, "", false, false, "null", act)
}

/// The dismiss control an overlay scan located.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct DismissTarget {
    /// Present when a safe control (reject/close) was found and should be clicked.
    pub x: Option<f64>,
    pub y: Option<f64>,
    #[serde(default)]
    pub desc: String,
    /// "reject" | "close" | "accept-only" | "none".
    pub kind: String,
}

/// Scan the page (main document, open shadow roots, same-origin iframes) for a
/// cookie/consent/newsletter overlay and the best control to get PAST it.
/// Ranks Reject/Decline highest (dismisses without granting consent), then
/// Close/×. Accept/Agree is NEVER auto-selected — granting consent is the
/// user's decision — it's only reported as `accept-only` so the agent can
/// choose. This clears the single most common blocker on real sites.
pub fn dismiss_overlay_script() -> String {
    // `return (` on the same line — DISMISS_BODY starts with a newline, so a bare
    // `return` would trip ASI into `return;` and drop the Promise.
    format!(
        "(function(){{{pre}\nreturn ({body});}})()",
        pre = EFFECT_PRE_JS.replace("__KEY_WAIT_MS__", &KEY_WAIT_MS.to_string()),
        body = DISMISS_BODY
    )
}

const DISMISS_BODY: &str = r##"
new Promise(function(__resolve){
  function frameOffset(el){
    var w = el.ownerDocument.defaultView, x = 0, y = 0;
    while (w && w !== window && w.frameElement){ var fr = w.frameElement.getBoundingClientRect(); x += fr.left; y += fr.top; try { w = w.parent; } catch(e){ break; } }
    return { x: x, y: y };
  }
  function visible(el){
    try { var s = getComputedStyle(el); if (s.display === 'none' || s.visibility === 'hidden' || s.opacity === '0') return false; var r = el.getBoundingClientRect(); return r.width > 4 && r.height > 4; } catch(e){ return false; }
  }
  function txt(el){ return ((el.getAttribute && (el.getAttribute('aria-label') || el.getAttribute('title'))) || el.innerText || el.textContent || '').trim().replace(/\s+/g, ' ').substring(0, 60); }

  var REJECT = /\b(reject|decline|refuse|only necessary|only essential|essential only|necessary only|disagree|deny|opt.?out)\b/i;
  var CLOSE = /\b(close|dismiss|no thanks|not now|skip|maybe later)\b|^[\s]*[×✕✖⨯xX✗✘]\s*$/;
  var ACCEPT = /\b(accept|agree|allow|got it|enable|consent|i understand)\b/i;
  var CTX = /cookie|consent|gdpr|ccpa|privacy|banner|modal|overlay|popup|popover|newsletter|subscribe|onetrust|cookiebot|truste|didomi|usercentrics|cmp|backdrop/i;

  var clickables = [];
  function collect(root, depth){
    if (depth > 4 || !root) return;
    var els; try { els = root.querySelectorAll('button,a[href],[role=button],[onclick],input[type=button],input[type=submit],[aria-label]'); } catch(e){ return; }
    for (var i = 0; i < els.length; i++){ if (visible(els[i])) clickables.push(els[i]); }
    var all; try { all = root.querySelectorAll('*'); } catch(e){ all = []; }
    for (var k = 0; k < all.length; k++){ if (all[k].shadowRoot) collect(all[k].shadowRoot, depth + 1); }
    var ifr; try { ifr = root.querySelectorAll('iframe'); } catch(e){ ifr = []; }
    for (var j = 0; j < ifr.length; j++){ try { if (ifr[j].contentDocument) collect(ifr[j].contentDocument, depth + 1); } catch(e){} }
  }
  collect(document, 0);

  function inOverlay(el){
    for (var n = el; n && n.nodeType === 1 && n !== document.body; n = n.parentNode || n.host){
      var role = n.getAttribute && n.getAttribute('role');
      if (role === 'dialog' || role === 'alertdialog') return true;
      if (n.getAttribute && n.getAttribute('aria-modal') === 'true') return true;
      var s; try { s = getComputedStyle(n); } catch(e){ continue; }
      if ((s.position === 'fixed' || s.position === 'sticky') && (parseInt(s.zIndex) || 0) >= 10) return true;
      var idc = ((n.id || '') + ' ' + (n.className && n.className.toString ? n.className.toString() : '')).toLowerCase();
      if (CTX.test(idc)) return true;
    }
    return false;
  }

  var best = null, bestScore = -1, acceptFallback = null;
  for (var i = 0; i < clickables.length; i++){
    var el = clickables[i], t = txt(el); if (!t) continue;
    var ov = inOverlay(el), score = -1, kind = null;
    if (REJECT.test(t)) { score = ov ? 100 : 55; kind = 'reject'; }
    else if (CLOSE.test(t)) { score = ov ? 80 : 25; kind = 'close'; }
    else if (ov && ACCEPT.test(t)) { if (!acceptFallback) acceptFallback = t; continue; }
    else continue;
    // Require overlay context unless it's an unambiguous close glyph, so we
    // never nuke a real page button that merely says "close".
    if (!ov && kind !== 'close') continue;
    if (score > bestScore){ bestScore = score; best = { el: el, t: t, kind: kind }; }
  }

  if (best){
    var r = best.el.getBoundingClientRect(), off = frameOffset(best.el);
    try { __pre(); } catch(e){}  // arm effect capture so the follow-up probe sees the overlay leave
    return __resolve(JSON.stringify({ x: r.left + r.width / 2 + off.x, y: r.top + r.height / 2 + off.y, desc: best.t, kind: best.kind }));
  }
  if (acceptFallback) return __resolve(JSON.stringify({ kind: 'accept-only', desc: acceptFallback }));
  return __resolve(JSON.stringify({ kind: 'none' }));
})
"##;

/// Parse the overlay scan result.
pub fn parse_dismiss(val: &str) -> Result<DismissTarget, String> {
    let inner: String = serde_json::from_str(val).unwrap_or_else(|_| val.to_string());
    serde_json::from_str::<DismissTarget>(inner.trim())
        .map_err(|e| format!("bad dismiss payload: {e}"))
}

/// Standalone effect+expectation probe for the native-input path (click / hover
/// / press_key). Reads `window.__octoweb_pre` (armed by the locate phase) and
/// resolves to `{diff, met, expect}` — the same payload shape the harness emits
/// after `true|`. With no expectation it waits `EFFECT_MS`; with one it polls
/// up to `SETTLE_MS` for it to hold.
pub fn effect_script(expect: Option<&str>) -> String {
    let exp = expect_json(expect);
    format!(
        "new Promise(function(r){{ var p = window.__octoweb_pre;          if (!p) return r(JSON.stringify({{ diff: {{}}, met: {no_exp} }}));          p.settle({exp}, {EFFECT_MS}, {SETTLE_MS}).then(function(x){{ r(JSON.stringify(x)); }}); }})",
        no_exp = if exp == "null" { "true" } else { "false" },
    )
}

pub fn type_script(selector: &str, text: &str, expect: Option<&str>, keys_only: bool) -> String {
    // Two paths, both replace (don't append) per the tool contract:
    //
    //  - contenteditable → a fallback chain, because no single primitive works
    //    across editors AND background tabs:
    //      1. Synthetic `paste` (DataTransfer text/plain). Model-backed editors
    //         (Lexical, DraftJS, ProseMirror, Medium) keep their own model and
    //         have paste handlers that route through it, so the model updates
    //         and any submit button gated on it enables. Focus-independent, so
    //         it works on background tabs too.
    //      2. If the paste didn't land (plain contenteditable has no paste
    //         handler), WebKit's editing commands — one block per line, so
    //         paragraphs survive instead of collapsing into one.
    //      3. If neither landed, resolve `'keys'`: the target is focused and
    //         cleared, and Rust types the text with trusted native keystrokes
    //         (native_input::type_text) — the one input every editor accepts.
    //    "Landed" is judged by the editor's own DOM, not a literal substring:
    //    rich editors split lines into blocks (textContent loses the `\n`),
    //    swap spaces for nbsp and re-typeset quotes, so a literal match on the
    //    pasted text fails on exactly the editors the paste path exists for —
    //    and a false "failed" would double-insert via the next step.
    //    Existing content is cleared first with a range scoped to the target:
    //    `execCommand('selectAll')` would widen it to the whole editing host
    //    and wipe sibling blocks (a title above the body paragraph).
    //
    //  - <input>/<textarea> → set value via the prototype setter (bypasses
    //    React's controlled-input cache) and fire input+change.
    //
    // `keys_only` skips the paste/command attempts: clear, then `'keys'`.
    //
    // Value setter must come from the element's own interface AND window
    // (iframe elements have their own constructors); WebKit brand-checks
    // prototype setters, so HTMLInputElement's setter throws on <textarea>.
    let act = r#"
    var TXT = __TXT__, KEYS_ONLY = __KEYS_ONLY__;
    // One keyup per character Rust is about to send — `\r` is the only one it
    // drops. Absolute target, not a reset, so a stray keyup cannot uncount it.
    function armKeys() {
      var k = window.__octoweb_keys;
      if (!k) return;
      // A keyup for a field inside an iframe lands in that frame's document,
      // where the top-level listener never sees it.
      var d = el.ownerDocument;
      if (d !== document && !d.__octoweb_keys_bound) {
        d.__octoweb_keys_bound = true;
        d.addEventListener('keyup', function () { k.got++; }, true);
      }
      k.want = k.got + String(TXT).replace(/\r/g, '').length;
    }
    try { el.focus(); } catch (e) {}
    if (el.isContentEditable) {
      var doc = el.ownerDocument;
      var host = el;
      while (host.parentElement && host.parentElement.isContentEditable) host = host.parentElement;
      function selAll() {
        try {
          var root = el.getRootNode();
          var s = (root.getSelection ? root : doc).getSelection();
          var rg = doc.createRange(); rg.selectNodeContents(el);
          s.removeAllRanges(); s.addRange(rg);
        } catch (e) {}
      }
      // Letters and digits only: blind to block splitting, nbsp, smart quotes.
      function norm(s) { return String(s || '').replace(/[^\p{L}\p{N}]+/gu, ''); }
      var orig = norm(host.textContent);
      selAll();
      try { doc.execCommand('delete', false, null); } catch (e) {}
      if (KEYS_ONLY) { armKeys(); return __resolve('keys'); }
      var want = norm(TXT), probe = want.substring(0, 24);
      if (TXT === '') return __done('true');
      var before = norm(host.textContent).length, mutated = 0, mo = null;
      try {
        mo = new MutationObserver(function (rs) { mutated += rs.length; });
        mo.observe(host, { childList: true, subtree: true, characterData: true });
      } catch (e) {}
      function landed() {
        var now = norm(host.textContent);
        if (probe) return now.indexOf(probe) !== -1 || (mutated > 0 && now !== orig && now.length > before);
        return mutated > 0;
      }
      function finish(ok) {
        try { if (mo) mo.disconnect(); } catch (e) {}
        if (!ok) { armKeys(); return __resolve('keys'); }
        // landed() only proves the first 24 characters arrived. Check the whole
        // string here: a rich editor that truncated at its character limit, or
        // dropped the tail, is the difference between a posted draft and a
        // silently mangled one.
        window.__octoweb_verify = function () {
          var live = host.isConnected;
          var now = live ? norm(host.textContent) : '';
          return {
            ok: live && (want === '' || now.indexOf(want) !== -1),
            connected: live,
            got: (live ? String(host.textContent || '') : '').substring(0, 60),
            len: now.length,
            want: want.length
          };
        };
        window.__octoweb_typed = (function () {
          var t = String(TXT || '');
          if (t.length < 40) { try { delete window.__octoweb_typed; } catch (e) {} return undefined; }
          return { sel: String(SEL), len: t.length, probe: t.substring(0, 24), head: t.substring(0, 60) };
        })();
        return __done('true');
      }
      try {
        var dt = new DataTransfer(); dt.setData('text/plain', TXT);
        el.dispatchEvent(new ClipboardEvent('paste', { bubbles: true, cancelable: true, clipboardData: dt }));
      } catch (e) {}
      var tries = 0;
      (function check() {
        if (landed()) return finish(true);
        if (tries++ < 8) return setTimeout(check, 40);
        selAll();
        try { doc.execCommand('delete', false, null); } catch (e) {}
        before = norm(host.textContent).length; mutated = 0;
        var lines = TXT.replace(/\r\n?/g, '\n').split('\n');
        for (var i = 0; i < lines.length; i++) {
          try {
            if (i > 0) doc.execCommand('insertParagraph', false, null);
            if (lines[i]) doc.execCommand('insertText', false, lines[i]);
          } catch (e) {}
        }
        setTimeout(function () { finish(landed()); }, 40);
      })();
      return;
    }
    var setter;
    if (el instanceof W.HTMLTextAreaElement) setter = Object.getOwnPropertyDescriptor(W.HTMLTextAreaElement.prototype, 'value').set;
    else if (el instanceof W.HTMLInputElement) setter = Object.getOwnPropertyDescriptor(W.HTMLInputElement.prototype, 'value').set;
    var IE = W.InputEvent || Event;
    if (KEYS_ONLY) {
      if (setter) setter.call(el, ''); else el.value = '';
      fire(el, new IE('input', { bubbles: true, composed: true, inputType: 'deleteContentBackward' }));
      armKeys();
      return __resolve('keys');
    }
    if (setter) setter.call(el, TXT); else el.value = TXT;
    fire(el, new IE('input', { bubbles: true, composed: true, inputType: 'insertReplacementText', data: TXT }));
    fire(el, new Event('change', { bubbles: true }));
    window.__octoweb_verify = function () {
      var live = el.isConnected;
      var now = live ? String(el.value == null ? '' : el.value) : '';
      return {
        ok: live && now === TXT,
        connected: live,
        got: now.substring(0, 60),
        len: now.length,
        want: String(TXT).length
      };
    };
    window.__octoweb_typed = (function () {
      var t = String(TXT || '');
      if (t.length < 40) { try { delete window.__octoweb_typed; } catch (e) {} return undefined; }
      return { sel: String(SEL), len: t.length, probe: t.substring(0, 24), head: t.substring(0, 60) };
    })();
    __done('true');
"#
    .replace("__TXT__", &json(text))
    .replace("__KEYS_ONLY__", if keys_only { "true" } else { "false" });
    build(
        &json(selector),
        GATE_TYPE,
        false,
        false,
        &expect_json(expect),
        &act,
    )
}

pub fn select_option_script(selector: &str, value: &str, expect: Option<&str>) -> String {
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
    build(
        &json(selector),
        GATE_ENABLED,
        false,
        false,
        &expect_json(expect),
        &act,
    )
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
    // Resolves bare 'true': `interpret_dom_result` treats anything else as a
    // failure status, so a position readback would have to travel as a
    // 'true|<effect>' payload through `format_effect`, which is a diff format,
    // not free text. Element-scoped scrolls therefore report no position.
    __resolve('true');
"#
    .replace("__DIR__", &json(direction))
    .replace(
        "__PX__",
        &pixels.map(|p| p.to_string()).unwrap_or_else(|| "null".into()),
    );
    build(&json(selector), "", false, false, "null", &act)
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
            type_script("#t", "he\"llo", None, false),
            type_script("#t", "x", Some("text:Saved"), false),
            type_script("#t", "a\nb", None, true),
            select_option_script("#s", "v", None),
            scroll_script("#c", "down", Some(10)),
            effect_script(None),
            effect_script(Some("url:/next")),
            dismiss_overlay_script(),
        ] {
            assert!(!s.trim_end().ends_with(';'), "trailing semicolon in {s}");
            // Placeholders are `__UPPER__`; every runtime name in these scripts
            // (__resolve, __done, __vfy, __octoweb_pre) is lowercase after the
            // underscores. Scanning beats a list nobody remembers to extend.
            assert!(
                !s.match_indices("__")
                    .any(|(i, _)| s[i + 2..].starts_with(|c: char| c.is_ascii_uppercase())),
                "unreplaced placeholder in {s}"
            );
        }
    }

    #[test]
    fn expect_dsl_maps_kinds() {
        assert_eq!(expect_json(None), "null");
        assert_eq!(expect_json(Some("")), "null");
        assert_eq!(
            expect_json(Some("Request sent")),
            "{\"kind\":\"text\",\"value\":\"Request sent\"}"
        );
        assert_eq!(
            expect_json(Some("text:hi")),
            "{\"kind\":\"text\",\"value\":\"hi\"}"
        );
        assert_eq!(
            expect_json(Some("gone:Loading")),
            "{\"kind\":\"text_gone\",\"value\":\"Loading\"}"
        );
        assert_eq!(
            expect_json(Some("url:/dashboard")),
            "{\"kind\":\"url\",\"value\":\"/dashboard\"}"
        );
        assert_eq!(
            expect_json(Some("selector:.ok")),
            "{\"kind\":\"selector\",\"value\":\".ok\"}"
        );
        assert_eq!(
            expect_json(Some("selector_gone:.spinner")),
            "{\"kind\":\"selector_gone\",\"value\":\".spinner\"}"
        );
        // Unknown prefix (a URL value) → treated as plain text, kept whole.
        assert_eq!(
            expect_json(Some("https://x.test/a")),
            "{\"kind\":\"text\",\"value\":\"https://x.test/a\"}"
        );
    }

    #[test]
    fn parse_dismiss_variants() {
        let r = parse_dismiss("\"{\\\"x\\\":12,\\\"y\\\":34,\\\"desc\\\":\\\"Reject all\\\",\\\"kind\\\":\\\"reject\\\"}\"").unwrap();
        assert_eq!(
            (r.x, r.y, r.kind.as_str()),
            (Some(12.0), Some(34.0), "reject")
        );
        let n = parse_dismiss("\"{\\\"kind\\\":\\\"none\\\"}\"").unwrap();
        assert_eq!(n.kind, "none");
        assert!(n.x.is_none());
        let a =
            parse_dismiss("\"{\\\"kind\\\":\\\"accept-only\\\",\\\"desc\\\":\\\"Accept all\\\"}\"")
                .unwrap();
        assert_eq!(
            (a.kind.as_str(), a.desc.as_str()),
            ("accept-only", "Accept all")
        );
    }

    #[test]
    fn selector_is_json_escaped() {
        let s = click_locate_script("a[href=\"x\"]");
        assert!(s.contains("var SEL = \"a[href=\\\"x\\\"]\";"));
        let k = key_focus_script(None);
        assert!(k.contains("var SEL = null;"));
    }
}
