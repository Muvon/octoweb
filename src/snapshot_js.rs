//! JavaScript for `browser_snapshot` — builds a compact element map with numeric refs.
//!
//! The script discovers all interactive elements, assigns `@N` refs, stores a
//! `Map<string, Element>` on `window.__octoweb_refs` for subsequent tool calls
//! (click, type, hover, etc.) to resolve by ref, and returns a compact text
//! representation optimised for LLM token efficiency.
//!
//! Notes:
//! - `__octoweb_refs` lives on `window` so it survives until the next navigation.
//!   Refs become invalid after navigation, full reload, or DOM tear-down.
//! - Same-origin iframes and open shadow roots are scanned recursively; the
//!   resulting @refs hold direct element references, so click/type work inside
//!   them even though `document.querySelector` can't reach there.
//! - Cross-origin iframe contents and closed shadow roots are skipped.
//! - Sensitive input values (passwords, card numbers, etc.) are never returned.
//! - Header carries page state (title, h1–h3, alert/status/dialog text) and a
//!   count of present-but-hidden controls, so outcome checks ("Request sent",
//!   "Access denied") don't need a `browser_execute_js` round-trip.

pub const SNAPSHOT_JS: &str = r#"
(function() {
  var refs = new Map();
  var seen = new WeakSet();
  var counter = 1;
  var lines = [];

  var hiddenControls = 0;
  function isVisible(el) {
    if (el.tagName === 'INPUT' && el.type === 'hidden') return true;
    var style = getComputedStyle(el);
    if (style.display === 'none') return false;
    if (style.visibility === 'hidden' || style.opacity === '0') {
      // Present but invisible: auto-hiding toolbars (Drive, video players)
      // collapse in a pointer-less background tab. Counted for the header
      // so the AI knows a hover would reveal more, not that the page is empty.
      var r0 = el.getBoundingClientRect();
      if (r0.width > 0 && r0.height > 0) hiddenControls++;
      return false;
    }
    if (el.offsetParent === null && style.position !== 'fixed' && style.position !== 'sticky') return false;
    var rect = el.getBoundingClientRect();
    return rect.width > 0 || rect.height > 0;
  }

  function getRole(el) {
    var role = el.getAttribute('role');
    if (role) return role;
    var tag = el.tagName.toLowerCase();
    if (tag === 'a') return 'link';
    if (tag === 'button') return 'button';
    if (tag === 'select') return 'combobox';
    if (tag === 'textarea') return 'textbox';
    if (tag === 'input') {
      var t = el.type;
      if (t === 'checkbox') return 'checkbox';
      if (t === 'radio') return 'radio';
      if (t === 'submit' || t === 'button' || t === 'reset' || t === 'image') return 'button';
      if (t === 'hidden') return 'hidden';
      if (t === 'range') return 'slider';
      if (t === 'file') return 'file';
      return 'textbox';
    }
    if (el.isContentEditable) return 'textbox';
    if (el.getAttribute('onclick')) return 'button';
    return tag;
  }

  function labelledBy(el) {
    var ids = el.getAttribute('aria-labelledby');
    if (!ids) return '';
    var doc = el.ownerDocument, out = [];
    ids.split(/\s+/).forEach(function (id) { var n = doc.getElementById(id); if (n) out.push(n.innerText || n.textContent || ''); });
    return out.join(' ');
  }
  // Form controls: the human-visible name is usually a <label for>, a wrapping
  // <label>, or aria-labelledby — none of which live on the element itself.
  // Without this a radio group renders as "val=1 / val=2 / val=3".
  function controlLabel(el) {
    var labels = el.labels;
    if (labels && labels.length) return labels[0].innerText || labels[0].textContent || '';
    var wrap = el.closest && el.closest('label');
    if (wrap) return wrap.innerText || wrap.textContent || '';
    return '';
  }
  function getText(el) {
    var tag = el.tagName;
    var isControl = tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT';
    var text = el.getAttribute('aria-label')
      || labelledBy(el)
      || (isControl ? controlLabel(el) : '')
      || el.getAttribute('title')
      || el.getAttribute('alt')
      || el.getAttribute('placeholder')
      || (isControl ? '' : (el.innerText || ''));
    return (text || '').trim().replace(/\s+/g, ' ').substring(0, 80);
  }

  var SENSITIVE_NAMES = /password|passwd|pwd|secret|token|csrf|xsrf|api_key|apikey|auth_token|access_token|refresh_token|session|nonce|ssn|credit.?card|cc.?number|card.?number|card.?no|card.?#|cc.?num|acct.?num|cvv|cvc|csc|cvn|security.?code|verification|card.?identification|pin|otp|expir|exp.?date|exp.?month|exp.?year|ccmonth|cardmonth|card.?holder|name.?on.?card|cc.?name|cc.?full.?name/i;
  var SENSITIVE_AC = /^(cc-|new-password|current-password)/;

  function isSensitiveInput(el) {
    if (el.type === 'password' || el.type === 'hidden') return true;
    if (SENSITIVE_NAMES.test(el.name || '')) return true;
    if (SENSITIVE_NAMES.test(el.id || '')) return true;
    var ac = el.getAttribute('autocomplete') || '';
    if (SENSITIVE_AC.test(ac)) return true;
    return false;
  }

  function getAttrs(el) {
    var parts = [];
    var tag = el.tagName.toLowerCase();
    if (tag === 'a' && el.href) {
      try { parts.push('href=' + new URL(el.href).pathname.substring(0, 60)); }
      catch(e) { parts.push('href=' + el.getAttribute('href')); }
    }
    if (tag === 'input') {
      if (el.type && el.type !== 'text') parts.push('type=' + el.type);
      if (el.placeholder) parts.push('placeholder=' + el.placeholder.substring(0, 40));
      if (el.type === 'checkbox' || el.type === 'radio') parts.push(el.checked ? 'checked' : 'unchecked');
      if (el.value && !isSensitiveInput(el)) parts.push('val=' + el.value.substring(0, 40));
    }
    if (tag === 'textarea' && el.value && !isSensitiveInput(el)) parts.push('val=' + el.value.substring(0, 40));
    if (tag === 'select') {
      var opts = Array.from(el.options).slice(0, 10);
      var optStr = opts.map(function(o) {
        return (o.selected ? '*' : '') + o.text.trim().substring(0, 30) + '=' + o.value;
      }).join('|');
      if (el.options.length > 10) optStr += '|...';
      if (optStr) parts.push('options=[' + optStr + ']');
    }
    if (el.disabled) parts.push('disabled');
    if (el.required) parts.push('required');
    if (el.readOnly) parts.push('readonly');
    if (el.name) parts.push('name=' + el.name);
    if (el.isContentEditable && tag !== 'input' && tag !== 'textarea') parts.push('contenteditable');
    return parts.length ? ' ' + parts.join(' ') : '';
  }

  function scan(doc) {
    var selector = 'a[href],button,input,select,textarea,'
      + '[role=button],[role=link],[role=tab],[role=menuitem],[role=menuitemcheckbox],'
      + '[role=menuitemradio],[role=option],[role=checkbox],[role=radio],[role=switch],'
      + '[role=textbox],[role=combobox],[role=searchbox],[role=slider],[role=spinbutton],'
      + '[role=treeitem],[onclick],[contenteditable=true]';
    var elements = doc.querySelectorAll(selector);
    for (var i = 0; i < elements.length; i++) {
      var el = elements[i];
      // Dedup across nested matches and recursive iframe scans.
      // (Bug fix: original code used `refs.has(el)` but `refs` keys are ref
      // strings, not elements — the check never triggered.)
      if (seen.has(el)) continue;
      if (!isVisible(el)) continue;
      seen.add(el);
      var ref = '@' + counter++;
      refs.set(ref, el);
      var role = getRole(el);
      var text = getText(el);
      var attrs = getAttrs(el);
      var textPart = text ? ' "' + text.replace(/"/g, '\\"') + '"' : '';
      lines.push(ref + ' ' + role + textPart + attrs);
    }
    var iframes = doc.querySelectorAll('iframe');
    for (var j = 0; j < iframes.length; j++) {
      try {
        // Cross-origin frames throw on contentDocument access — caught and skipped.
        if (iframes[j].contentDocument) scan(iframes[j].contentDocument);
      } catch(e) {}
    }
    // Single full walk doing two jobs:
    //  1. Pierce open shadow roots — web-component UIs (Lit, Polymer, LWC)
    //     render everything inside shadowRoot, invisible to querySelectorAll.
    //  2. Surface listener-only clickables — <div>s whose sole interactivity
    //     is an addEventListener('click'), tagged at document-start by
    //     COMBINED_SCRIPT. Skipped when they contain a real interactive
    //     element (event-delegation roots like React containers).
    var tagged = window.__octoweb_listeners;
    var all = doc.querySelectorAll('*');
    for (var k = 0; k < all.length; k++) {
      var node = all[k];
      if (node.shadowRoot) scan(node.shadowRoot);
      if (tagged && tagged.has(node) && !seen.has(node) && isVisible(node) && !node.querySelector(selector)) {
        seen.add(node);
        var cRef = '@' + counter++;
        refs.set(cRef, node);
        var cText = getText(node);
        lines.push(cRef + ' clickable' + (cText ? ' "' + cText.replace(/"/g, '\\"') + '"' : ''));
      }
    }
  }

  scan(document);

  // Page state the AI otherwise has to fetch with execute_js: what the page
  // is saying (headings), and anything it is shouting (alerts, status
  // regions, open dialogs) — where "Request sent" / "Access denied" live.
  function clean(t, n) { return (t || '').trim().replace(/\s+/g, ' ').substring(0, n); }
  function visibleText(el, n) {
    var st = getComputedStyle(el);
    if (st.display === 'none' || st.visibility === 'hidden') return '';
    return clean(el.innerText, n);
  }
  var state = [];
  var heads = [], hs = document.querySelectorAll('h1,h2,h3');
  for (var h = 0; h < hs.length && heads.length < 6; h++) {
    var ht = visibleText(hs[h], 80);
    if (ht) heads.push(hs[h].tagName.toLowerCase() + ' "' + ht.replace(/"/g, '\\"') + '"');
  }
  if (heads.length) state.push(heads.join(' · '));
  var live = document.querySelectorAll('[role=alert],[role=status],[aria-live=polite],[aria-live=assertive],[role=dialog],[role=alertdialog],[aria-modal=true],dialog[open]');
  var liveSeen = 0;
  for (var l = 0; l < live.length && liveSeen < 4; l++) {
    var lt = visibleText(live[l], 160);
    if (!lt) continue;
    var role = live[l].getAttribute('role') || (live[l].tagName === 'DIALOG' ? 'dialog' : (live[l].getAttribute('aria-modal') ? 'dialog' : 'live'));
    state.push(role + ': "' + lt.replace(/"/g, '\\"') + '"');
    liveSeen++;
  }

  // Non-enumerable so pages iterating `window` can't fingerprint it.
  Object.defineProperty(window, '__octoweb_refs', { value: refs, configurable: true });
  // Header tells the AI how many refs were captured, that they expire on
  // navigation, and how much of the page is below the fold — saves
  // follow-up clarification round-trips.
  var se = document.scrollingElement || document.documentElement;
  var meta = 'page: ' + location.href.substring(0, 150)
    + (document.title ? ' | title: ' + clean(document.title, 80) : '')
    + ' | viewport ' + Math.round(se.scrollTop) + '-' + Math.round(se.scrollTop + window.innerHeight)
    + ' of ' + Math.round(se.scrollHeight) + 'px';
  var header = lines.length === 0
    ? '(no interactive elements found)'
    : lines.length + ' elements (refs valid until next navigation):';
  if (hiddenControls) header += ' +' + hiddenControls + ' present-but-hidden controls (auto-hiding UI: browser_hover a visible element or the page centre to reveal, then re-snapshot)';
  var out = [meta];
  if (state.length) out.push(state.join('\n'));
  out.push(header);
  if (lines.length) out.push(lines.join('\n'));
  return out.join('\n');
})()
"#;
