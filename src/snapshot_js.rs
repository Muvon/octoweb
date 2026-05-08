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
//! - Cross-origin iframe contents are skipped (security boundary).
//! - Sensitive input values (passwords, card numbers, etc.) are never returned.

pub const SNAPSHOT_JS: &str = r#"
(function() {
  var refs = new Map();
  var seen = new WeakSet();
  var counter = 1;
  var lines = [];

  function isVisible(el) {
    if (el.tagName === 'INPUT' && el.type === 'hidden') return true;
    var style = getComputedStyle(el);
    if (style.display === 'none' || style.visibility === 'hidden') return false;
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

  function getText(el) {
    var text = el.getAttribute('aria-label')
      || el.getAttribute('title')
      || el.getAttribute('alt')
      || el.getAttribute('placeholder')
      || (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' ? '' : (el.innerText || ''));
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
  }

  scan(document);
  window.__octoweb_refs = refs;
  // Header tells the AI how many refs were captured and that they expire on
  // navigation — saves a follow-up clarification round-trip.
  var header = lines.length === 0
    ? '(no interactive elements found)'
    : lines.length + ' elements (refs valid until next navigation):';
  return header + '\n' + lines.join('\n');
})()
"#;
