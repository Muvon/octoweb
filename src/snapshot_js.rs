//! JavaScript for `browser_snapshot` — builds a compact element map with numeric refs.
//!
//! The script discovers interactive elements, assigns `@N` refs, stores a
//! `Map<string, Element>` on `window.__octoweb_refs` for subsequent tool calls
//! (click, type, hover, etc.) to resolve by ref, and returns a compact text
//! representation optimised for LLM token efficiency.
//!
//! Notes:
//! - `__octoweb_refs` (element lookup) and `__octoweb_refkeys` (a WeakMap giving
//!   each element a **stable** `@N` across snapshots) live on `window`, so a
//!   ref keeps pointing at the same element until navigation tears the context
//!   down. Stable refs are what make `diff` meaningful.
//! - Same-origin iframes and open shadow roots are scanned recursively; the
//!   resulting @refs hold direct element references, so click/type work inside
//!   them even though `document.querySelector` can't reach there.
//! - Cross-origin iframe contents and closed shadow roots are skipped.
//! - Sensitive input values (passwords, card numbers, etc.) are never returned.
//! - Header carries page state (title, h1–h3, alert/status/dialog text) and a
//!   count of present-but-hidden controls, so outcome checks ("Request sent",
//!   "Access denied") don't need a `browser_execute_js` round-trip.
//! - `within` scopes the scan to one container (a dialog, a form); `diff`
//!   returns only elements that appeared/changed/left since the last snapshot —
//!   on a busy SPA that's the difference between 8 lines and 200.

/// Build the snapshot expression. `within` is an optional CSS selector or `@ref`
/// to scope the scan; `diff` limits output to changes since the previous
/// snapshot of the same tab.
pub fn snapshot_script(within: Option<&str>, diff: bool) -> String {
    let root_expr = match within {
        Some(sel) if sel.starts_with('@') => {
            let s = serde_json::to_string(sel).unwrap_or_else(|_| "\"\"".into());
            format!("(window.__octoweb_refs && window.__octoweb_refs.get({s})) || null")
        }
        Some(sel) => {
            let s = serde_json::to_string(sel).unwrap_or_else(|_| "\"\"".into());
            format!("(function(){{ try {{ return document.querySelector({s}); }} catch(e) {{ return null; }} }})()")
        }
        None => "document".into(),
    };
    let within_desc = match within {
        Some(sel) => serde_json::to_string(sel).unwrap_or_else(|_| "\"\"".into()),
        None => "null".into(),
    };
    SNAPSHOT_TEMPLATE
        .replace("__ROOT__", &root_expr)
        .replace("__WITHIN__", &within_desc)
        .replace("__DIFF__", if diff { "true" } else { "false" })
}

const SNAPSHOT_TEMPLATE: &str = r#"
(function() {
  var DIFF = __DIFF__;
  var WITHIN = __WITHIN__;
  var root = __ROOT__;
  if (WITHIN !== null && !root) {
    return 'within: no element matched ' + WITHIN + ' — re-snapshot without `within`, or fix the selector.';
  }
  if (!root) root = document;

  // Stable ref registry: same element → same @N across snapshots.
  if (!window.__octoweb_refkeys) {
    Object.defineProperty(window, '__octoweb_refkeys', { value: new WeakMap(), configurable: true });
  }
  var refkeys = window.__octoweb_refkeys;
  function assignRef(el) {
    var ref = refkeys.get(el);
    if (!ref) {
      window.__octoweb_refctr = (window.__octoweb_refctr || 0) + 1;
      ref = '@' + window.__octoweb_refctr;
      refkeys.set(el, ref);
    }
    return ref;
  }

  var refs = new Map();       // ref -> element (this snapshot's actionable set)
  var seen = new WeakSet();
  var cur = {};               // ref -> line
  var order = [];             // refs in document order
  var hiddenControls = 0;

  function isVisible(el) {
    if (el.tagName === 'INPUT' && el.type === 'hidden') return true;
    var style = getComputedStyle(el);
    if (style.display === 'none') return false;
    if (style.visibility === 'hidden' || style.opacity === '0') {
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

  function record(el, role, textOverride) {
    var ref = assignRef(el);
    refs.set(ref, el);
    var text = textOverride != null ? textOverride : getText(el);
    var attrs = role === 'clickable' ? '' : getAttrs(el);
    var textPart = text ? ' "' + text.replace(/"/g, '\\"') + '"' : '';
    cur[ref] = ref + ' ' + role + textPart + attrs;
    order.push(ref);
  }

  var SEL = 'a[href],button,input,select,textarea,'
    + '[role=button],[role=link],[role=tab],[role=menuitem],[role=menuitemcheckbox],'
    + '[role=menuitemradio],[role=option],[role=checkbox],[role=radio],[role=switch],'
    + '[role=textbox],[role=combobox],[role=searchbox],[role=slider],[role=spinbutton],'
    + '[role=treeitem],[onclick],[contenteditable]:not([contenteditable=false])';

  function scan(node) {
    var elements = node.querySelectorAll(SEL);
    for (var i = 0; i < elements.length; i++) {
      var el = elements[i];
      if (seen.has(el)) continue;
      if (!isVisible(el)) continue;
      seen.add(el);
      record(el, getRole(el));
    }
    var iframes = node.querySelectorAll('iframe');
    for (var j = 0; j < iframes.length; j++) {
      try { if (iframes[j].contentDocument) scan(iframes[j].contentDocument); } catch(e) {}
    }
    var tagged = window.__octoweb_listeners;
    var all = node.querySelectorAll('*');
    for (var k = 0; k < all.length; k++) {
      var n = all[k];
      if (n.shadowRoot) scan(n.shadowRoot);
      if (tagged && tagged.has(n) && !seen.has(n) && isVisible(n) && !n.querySelector(SEL)) {
        seen.add(n);
        record(n, 'clickable');
      }
    }
  }

  // A scoped element root: include it if it is itself interactive, then descend.
  if (root.nodeType === 1 && root.matches && root.matches(SEL) && isVisible(root)) {
    seen.add(root);
    record(root, getRole(root));
  }
  scan(root);

  // Page state (always emitted, even in diff mode — it's the outcome signal).
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
    var lrole = live[l].getAttribute('role') || (live[l].tagName === 'DIALOG' ? 'dialog' : (live[l].getAttribute('aria-modal') ? 'dialog' : 'live'));
    state.push(lrole + ': "' + lt.replace(/"/g, '\\"') + '"');
    liveSeen++;
  }

  Object.defineProperty(window, '__octoweb_refs', { value: refs, configurable: true });

  // Diff against the previous snapshot of this tab, then store the new baseline.
  var prev = window.__octoweb_snaplines || {};
  var outLines, removed = [];
  if (DIFF) {
    outLines = [];
    for (var oi = 0; oi < order.length; oi++) {
      var rref = order[oi];
      if (cur[rref] !== prev[rref]) outLines.push(cur[rref]);
    }
    for (var pk in prev) { if (Object.prototype.hasOwnProperty.call(prev, pk) && !(pk in cur)) removed.push(pk); }
  } else {
    outLines = order.map(function (r) { return cur[r]; });
  }
  Object.defineProperty(window, '__octoweb_snaplines', { value: cur, configurable: true });

  var se = document.scrollingElement || document.documentElement;
  var meta = 'page: ' + location.href.substring(0, 150)
    + (document.title ? ' | title: ' + clean(document.title, 80) : '')
    + (WITHIN !== null ? ' | within: ' + WITHIN : '')
    + ' | viewport ' + Math.round(se.scrollTop) + '-' + Math.round(se.scrollTop + window.innerHeight)
    + ' of ' + Math.round(se.scrollHeight) + 'px';

  var header;
  if (DIFF) {
    header = outLines.length === 0 && removed.length === 0
      ? '(no changes since last snapshot)'
      : outLines.length + ' new/changed' + (removed.length ? ', ' + removed.length + ' removed (' + removed.join(' ') + ')' : '') + ' — refs stable across snapshots:';
  } else {
    header = order.length === 0
      ? '(no interactive elements found)'
      : order.length + ' elements (refs stable until navigation):';
  }
  if (hiddenControls) header += ' +' + hiddenControls + ' present-but-hidden controls (auto-hiding UI: browser_hover a visible element or the page centre to reveal, then re-snapshot)';

  var out = [meta];
  if (state.length) out.push(state.join('\n'));
  out.push(header);
  if (outLines.length) out.push(outLines.join('\n'));
  return out.join('\n');
})()
"#;

#[cfg(test)]
mod tests {
    use super::snapshot_script;

    #[test]
    fn root_expression_by_mode() {
        // No `within` → scans the whole document.
        assert!(snapshot_script(None, false).contains("var root = document;"));
        // CSS selector → querySelector, JSON-escaped.
        let css = snapshot_script(Some(".dialog"), false);
        assert!(css.contains("document.querySelector(\".dialog\")"));
        assert!(css.contains("var WITHIN = \".dialog\";"));
        // @ref → resolved through the live ref map.
        let refd = snapshot_script(Some("@7"), true);
        assert!(refd.contains("window.__octoweb_refs.get(\"@7\")"));
        assert!(refd.contains("var DIFF = true;"));
    }

    #[test]
    fn is_a_bare_expression() {
        for s in [
            snapshot_script(None, false),
            snapshot_script(Some("#form"), true),
        ] {
            assert!(!s.trim_end().ends_with(';'), "trailing semicolon");
            assert!(
                !s.contains("__ROOT__") && !s.contains("__DIFF__") && !s.contains("__WITHIN__")
            );
        }
    }
}
