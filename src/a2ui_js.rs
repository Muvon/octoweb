//! A2UI v1.0 evaluation core, injected into the AI sidebar's script.
//!
//! Split out of `sidebar_html` because it is the one part of the renderer
//! with no DOM in it: JSON-Pointer access, the basic-catalog function
//! registry, ValueRef resolution and the `formatString` interpolation
//! grammar. That makes it runnable — and therefore testable — under node.
//!
//! Spec: <https://a2ui.org/specification/v1.0-a2ui/>

/// Injected into `sidebar_html::HTML` at the `/* A2UI_CORE_JS */` marker.
pub const CORE: &str = r##"
  // ── A2UI v1.0 evaluation core ──────────────────────────────────────────
  // Pure logic behind the sidebar's A2UI renderer: JSON-Pointer access, the
  // basic-catalog function registry, ValueRef resolution and the
  // `formatString` interpolation grammar. Touches no DOM, so the tests at the
  // bottom of a2ui_js.rs can run it under node.

  // JSON-Pointer (RFC 6901)
  function a2uiPtrParts(path) {
    return path.split('/').slice(1).map(p => p.replace(/~1/g, '/').replace(/~0/g, '~'));
  }
  function a2uiPtrGet(model, path) {
    if (!path || path === '/') return model;
    const parts = a2uiPtrParts(path);
    let cur = model;
    for (const p of parts) {
      if (cur == null || typeof cur !== 'object') return undefined;
      cur = cur[p];
    }
    return cur;
  }
  function a2uiPtrSet(model, path, value) {
    if (!path || path === '/') return value;
    const parts = a2uiPtrParts(path);
    let cur = model;
    for (let i = 0; i < parts.length - 1; i++) {
      const k = parts[i];
      if (cur[k] == null || typeof cur[k] !== 'object') cur[k] = {};
      cur = cur[k];
    }
    cur[parts[parts.length - 1]] = value;
    return model;
  }
  // v1.0 `updateDataModel`: an explicit null value deletes the key at `path`.
  function a2uiPtrDelete(model, path) {
    if (!path || path === '/') return {};
    const parts = a2uiPtrParts(path);
    let cur = model;
    for (let i = 0; i < parts.length - 1; i++) {
      if (cur == null || typeof cur !== 'object') return model;
      cur = cur[parts[i]];
    }
    if (cur == null || typeof cur !== 'object') return model;
    const last = parts[parts.length - 1];
    if (Array.isArray(cur)) cur.splice(Number(last), 1);
    else delete cur[last];
    return model;
  }

  // A check `condition` resolves to a boolean or to a v1.0 ValidationResult
  // ({valid, message, …}); both gate an action the same way.
  function a2uiTruthy(v) {
    if (v && typeof v === 'object' && !Array.isArray(v) && typeof v.valid === 'boolean') {
      return v.valid;
    }
    return !!v;
  }

  // `openUrl` is a side effect, and the v1.0 catalog requires genuine user
  // activation for it: resolving a ValueRef while painting a surface must
  // never open a tab. Only a click handler wraps itself in this.
  let a2uiActivated = false;
  function a2uiWithActivation(fn) {
    a2uiActivated = true;
    try { return fn(); } finally { a2uiActivated = false; }
  }

  // Plain stringification for validation/formatting args — unlike a2uiToStr
  // it never drills into objects looking for something prettier to show.
  function a2uiValStr(v) {
    if (v == null) return '';
    if (typeof v === 'object') {
      try { return JSON.stringify(v); } catch (e) { return ''; }
    }
    return String(v);
  }

  const A2UI_MONTHS_LONG = ['January', 'February', 'March', 'April', 'May', 'June',
    'July', 'August', 'September', 'October', 'November', 'December'];
  const A2UI_MONTHS_SHORT = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun',
    'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
  const A2UI_DAYS_LONG = ['Sunday', 'Monday', 'Tuesday', 'Wednesday', 'Thursday', 'Friday', 'Saturday'];
  const A2UI_DAYS_SHORT = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
  const A2UI_DATE_TOKENS = /yyyy|yy|MMMM|MMM|MM|M|EEEE|E|dd|d|HH|H|hh|h|mm|ss|a/g;

  // Unicode TR35 subset used by the catalog's `formatDate`. An ISO input that
  // carries a zone ("Z" or ±hh:mm) is read in UTC so the rendered date matches
  // the instant the agent wrote; a zoneless one is read as local wall time.
  function a2uiFormatDate(value, format) {
    const raw = a2uiValStr(value);
    if (!raw) return '';
    const d = new Date(raw);
    if (isNaN(d.getTime())) return '';
    const fmt = format == null || format === '' ? 'yyyy-MM-dd' : String(format);
    if (fmt === 'ISO') return d.toISOString();
    const zoned = /(?:Z|[+-]\d{2}:?\d{2})$/i.test(raw);
    const year   = zoned ? d.getUTCFullYear() : d.getFullYear();
    const month  = (zoned ? d.getUTCMonth() : d.getMonth()) + 1;
    const day    = zoned ? d.getUTCDate() : d.getDate();
    const dow    = zoned ? d.getUTCDay() : d.getDay();
    const hour   = zoned ? d.getUTCHours() : d.getHours();
    const minute = zoned ? d.getUTCMinutes() : d.getMinutes();
    const second = zoned ? d.getUTCSeconds() : d.getSeconds();
    const pad = n => (n < 10 ? '0' + n : String(n));
    return fmt.replace(A2UI_DATE_TOKENS, tok => {
      switch (tok) {
        case 'yyyy': return String(year);
        case 'yy':   return String(year).slice(-2);
        case 'MMMM': return A2UI_MONTHS_LONG[month - 1];
        case 'MMM':  return A2UI_MONTHS_SHORT[month - 1];
        case 'MM':   return pad(month);
        case 'M':    return String(month);
        case 'EEEE': return A2UI_DAYS_LONG[dow];
        case 'E':    return A2UI_DAYS_SHORT[dow];
        case 'dd':   return pad(day);
        case 'd':    return String(day);
        case 'HH':   return pad(hour);
        case 'H':    return String(hour);
        case 'hh':   return pad(hour % 12 || 12);
        case 'h':    return String(hour % 12 || 12);
        case 'mm':   return pad(minute);
        case 'ss':   return pad(second);
        case 'a':    return hour < 12 ? 'AM' : 'PM';
        default:     return tok;
      }
    });
  }

  // Every function the A2UI v1.0 basic catalog defines. Each takes the
  // resolved `args` object plus the surrounding data scope.
  const A2UI_FN = {
    // ── Validation. The catalog types these `validationResult`; the
    // reference renderer returns a plain boolean and a2uiTruthy takes either.
    required: ({ value }) =>
      value != null && value !== '' && !(Array.isArray(value) && value.length === 0),
    email: ({ value }) => /^[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}$/.test(a2uiValStr(value)),
    regex: ({ value, pattern }) => {
      try { return new RegExp(a2uiValStr(pattern)).test(a2uiValStr(value)); }
      catch (e) { return false; }
    },
    length: ({ value, min, max }) => {
      const len = a2uiValStr(value).length;
      return (min == null || len >= Number(min)) && (max == null || len <= Number(max));
    },
    // Range check, not a "looks like a number" check — with neither bound
    // there is nothing to violate, which is what the reference does too.
    numeric: ({ value, min, max }) => {
      if (min == null && max == null) return true;
      const n = Number(value);
      if (!isFinite(n)) return false;
      return (min == null || n >= Number(min)) && (max == null || n <= Number(max));
    },

    // ── Logic
    and: ({ values }) => Array.isArray(values) && values.every(a2uiTruthy),
    or:  ({ values }) => Array.isArray(values) && values.some(a2uiTruthy),
    not: ({ value }) => !a2uiTruthy(value),

    // ── Formatting
    formatString: ({ value }, scope) => a2uiInterpolate(a2uiValStr(value), scope),
    formatDate: ({ value, format }) => a2uiFormatDate(value, format),
    formatNumber: ({ value, decimals, grouping }) => {
      const n = Number(value);
      if (!isFinite(n)) return '';
      const opts = { useGrouping: grouping == null ? true : !!grouping };
      if (decimals != null) {
        opts.minimumFractionDigits = Number(decimals);
        opts.maximumFractionDigits = Number(decimals);
      }
      try { return new Intl.NumberFormat(undefined, opts).format(n); }
      catch (e) { return decimals != null ? n.toFixed(Number(decimals)) : String(n); }
    },
    formatCurrency: ({ value, currency, decimals, grouping }) => {
      const n = Number(value);
      if (!isFinite(n)) return '';
      const code = currency == null || currency === '' ? 'USD' : String(currency).toUpperCase();
      const digits = decimals == null ? 2 : Number(decimals);
      try {
        return new Intl.NumberFormat(undefined, {
          style: 'currency',
          currency: code,
          useGrouping: grouping == null ? true : !!grouping,
          minimumFractionDigits: digits,
          maximumFractionDigits: digits,
        }).format(n);
      } catch (e) { return code + ' ' + n.toFixed(digits); }
    },
    pluralize: (args) => {
      const n = Number(args.value);
      if (!isFinite(n)) return a2uiValStr(args.other);
      // Exact 0/1/2 win over the CLDR category so "no items"/"a pair" read
      // naturally in locales that lump them into `other`.
      let cat = null;
      if (n === 0 && args.zero != null) cat = 'zero';
      else if (n === 1 && args.one != null) cat = 'one';
      else if (n === 2 && args.two != null) cat = 'two';
      if (cat == null) {
        try { cat = new Intl.PluralRules(undefined).select(n); }
        catch (e) { cat = 'other'; }
      }
      const picked = args[cat] != null ? args[cat] : args.other;
      return a2uiValStr(picked);
    },

    // ── Side effects. openUrl routes through Rust so the URL opens as a real
    // browser tab instead of a popup from the sidebar webview.
    openUrl: ({ url }) => {
      if (!a2uiActivated) return undefined;
      const u = a2uiValStr(url);
      // v1.0 mandates an http(s)-only allowlist: no javascript:, no data:.
      if (!/^https?:\/\//i.test(u)) return undefined;
      window.ipc.postMessage(JSON.stringify({ type: 'a2ui_open_url', url: u }));
      return undefined;
    },

    // ── System function (available in every catalog).
    '@index': ({ offset }, scope) =>
      (scope && scope.index != null ? Number(scope.index) : 0) + (offset == null ? 0 : Number(offset)),
  };

  // ── formatString interpolation ─────────────────────────────────────────
  // Grammar: literal text with `${expression}` holes, where an expression is
  // a data path (`/absolute` or `relative`), a function call
  // (`fn(name: value, …)`) or a literal. `\${` is an escaped marker. Parsing
  // yields ordinary ValueRef nodes, so resolution reuses a2uiResolveValue.
  const A2UI_MAX_EXPR_DEPTH = 10;

  // Index of the `}` closing a `${` that opened at `from`. Quoted spans are
  // skipped so a brace inside a string literal doesn't unbalance the scan.
  function a2uiCloseBrace(input, from) {
    let depth = 1;
    let i = from;
    while (i < input.length && depth > 0) {
      const c = input.charAt(i++);
      if (c === '{') depth++;
      else if (c === '}') depth--;
      else if (c === "'" || c === '"') {
        while (i < input.length) {
          const d = input.charAt(i++);
          if (d === '\\') i++;
          else if (d === c) break;
        }
      }
    }
    if (depth > 0) throw new Error('unclosed interpolation');
    return i - 1;
  }

  function a2uiParseTemplate(input, depth) {
    if (!input || input.indexOf('${') < 0) return input ? [input] : [];
    if (depth > A2UI_MAX_EXPR_DEPTH) throw new Error('expression too deep');
    const parts = [];
    let i = 0;
    while (i < input.length) {
      if (input.charAt(i) === '\\' && input.substr(i + 1, 2) === '${') {
        parts.push('${');
        i += 3;
        continue;
      }
      if (input.substr(i, 2) === '${') {
        const end = a2uiCloseBrace(input, i + 2);
        parts.push(a2uiParseExpr(input.slice(i + 2, end), depth + 1));
        i = end + 1;
        continue;
      }
      let j = i;
      while (j < input.length
        && input.substr(j, 2) !== '${'
        && !(input.charAt(j) === '\\' && input.substr(j + 1, 2) === '${')) j++;
      parts.push(input.slice(i, j));
      i = j;
    }
    return parts.filter(p => p !== '' && p != null);
  }

  function a2uiSkipWs(st) {
    while (st.i < st.s.length && /\s/.test(st.s.charAt(st.i))) st.i++;
  }
  function a2uiParseExpr(expr, depth) {
    const s = String(expr == null ? '' : expr).trim();
    if (s === '') return '';
    if (depth > A2UI_MAX_EXPR_DEPTH) throw new Error('expression too deep');
    const st = { s, i: 0 };
    const node = a2uiParseExprAt(st, depth);
    a2uiSkipWs(st);
    if (st.i < st.s.length) throw new Error('trailing characters in expression');
    return node;
  }
  function a2uiParseExprAt(st, depth) {
    a2uiSkipWs(st);
    if (st.i >= st.s.length) return '';
    if (st.s.substr(st.i, 2) === '${') {
      const end = a2uiCloseBrace(st.s, st.i + 2);
      const inner = st.s.slice(st.i + 2, end);
      st.i = end + 1;
      return a2uiParseExpr(inner, depth + 1);
    }
    const c = st.s.charAt(st.i);
    if (c === "'" || c === '"') return a2uiParseStrLit(st);
    if (/[0-9]/.test(c) || (c === '-' && /[0-9]/.test(st.s.charAt(st.i + 1)))) return a2uiParseNumLit(st);
    if (a2uiKeyword(st, 'true')) return true;
    if (a2uiKeyword(st, 'false')) return false;
    if (a2uiKeyword(st, 'null')) return '';
    const token = a2uiParsePathTok(st);
    a2uiSkipWs(st);
    if (st.s.charAt(st.i) === '(') return a2uiParseCall(st, token, depth);
    return token ? { path: token } : '';
  }
  function a2uiKeyword(st, kw) {
    if (st.s.substr(st.i, kw.length) !== kw) return false;
    const next = st.s.charAt(st.i + kw.length);
    if (next && /[A-Za-z0-9_]/.test(next)) return false;
    st.i += kw.length;
    return true;
  }
  function a2uiParsePathTok(st) {
    const start = st.i;
    while (st.i < st.s.length && /[A-Za-z0-9/._@-]/.test(st.s.charAt(st.i))) st.i++;
    return st.s.slice(start, st.i);
  }
  function a2uiParseStrLit(st) {
    const q = st.s.charAt(st.i++);
    let out = '';
    while (st.i < st.s.length) {
      const c = st.s.charAt(st.i++);
      if (c === '\\') {
        const n = st.s.charAt(st.i++);
        out += n === 'n' ? '\n' : n === 't' ? '\t' : n === 'r' ? '\r' : n;
      } else if (c === q) {
        break;
      } else {
        out += c;
      }
    }
    return out;
  }
  function a2uiParseNumLit(st) {
    const start = st.i;
    if (st.s.charAt(st.i) === '-') st.i++;
    while (st.i < st.s.length && /[0-9.]/.test(st.s.charAt(st.i))) st.i++;
    return Number(st.s.slice(start, st.i));
  }
  function a2uiParseCall(st, name, depth) {
    st.i++; // consume "("
    const args = {};
    a2uiSkipWs(st);
    while (st.i < st.s.length && st.s.charAt(st.i) !== ')') {
      const start = st.i;
      while (st.i < st.s.length && /[A-Za-z0-9_]/.test(st.s.charAt(st.i))) st.i++;
      const argName = st.s.slice(start, st.i);
      a2uiSkipWs(st);
      if (st.s.charAt(st.i) !== ':') throw new Error('expected ":" after argument "' + argName + '"');
      st.i++;
      args[argName] = a2uiParseExprAt(st, depth + 1);
      a2uiSkipWs(st);
      if (st.s.charAt(st.i) === ',') { st.i++; a2uiSkipWs(st); }
    }
    if (st.s.charAt(st.i) !== ')') throw new Error('unclosed call to "' + name + '"');
    st.i++;
    return { call: name, args };
  }
  // A template the parser rejects renders as its own literal text — a broken
  // expression should look wrong, not blank out the surrounding sentence.
  function a2uiInterpolate(src, scope) {
    const raw = String(src == null ? '' : src);
    let parts;
    try { parts = a2uiParseTemplate(raw, 0); } catch (e) { return raw; }
    return parts.map(p => {
      const v = (p && typeof p === 'object') ? a2uiResolveValue(p, scope) : p;
      return v == null ? '' : a2uiToStr(v);
    }).join('');
  }

  function a2uiResolveValue(v, scope) {
    if (v == null) return v;
    if (typeof v !== 'object') return v;
    if (Array.isArray(v)) return v.map(x => a2uiResolveValue(x, scope));
    if (typeof v.path === 'string') {
      const p = v.path;
      // Inside a List iteration scope, treat "/", "." and "" as "the current
      // item" — that's the natural way to bind a scalar item (e.g. a string
      // in a string[]) into a Text/Image. Without this, agents that write
      // `{path: "/"}` on a list template end up dumping the whole root model
      // into every row.
      if (scope.local != null && (p === '' || p === '/' || p === '.')) {
        return scope.local;
      }
      if (p.charAt(0) === '/') return a2uiPtrGet(scope.root, p);
      if (scope.local != null) return a2uiPtrGet(scope.local, '/' + p);
      return undefined;
    }
    if (typeof v.call === 'string') {
      const fn = A2UI_FN[v.call];
      if (!fn) return undefined;
      const args = {};
      const rawArgs = v.args || {};
      for (const k in rawArgs) args[k] = a2uiResolveValue(rawArgs[k], scope);
      try { return fn(args, scope || {}); } catch (e) { return undefined; }
    }
    return v;
  }

  function a2uiPathOf(v) {
    if (v && typeof v === 'object' && typeof v.path === 'string' && v.path.charAt(0) === '/') {
      return v.path;
    }
    return null;
  }

  // Stringify a resolved value for display in a text/input slot. Avoids the
  // `String({...})` → "[object Object]" trap when the agent points a text
  // binding at an object subtree: we drill into common content fields, fall
  // back to compact JSON, and only ever pass scalars through unchanged.
  function a2uiToStr(v) {
    if (v == null) return '';
    if (typeof v === 'string') {
      // Defensive: some models over-escape and put the literal 2-char "\n"
      // (backslash + n) instead of a real newline. Same for \r\n and \t.
      // Idempotent — if the string already has real newlines, no-op.
      if (v.indexOf('\\') !== -1) {
        return v.replace(/\\r\\n/g, '\n').replace(/\\n/g, '\n').replace(/\\t/g, '\t');
      }
      return v;
    }
    if (typeof v === 'number' || typeof v === 'boolean') return String(v);
    if (Array.isArray(v)) return v.map(a2uiToStr).join(', ');
    if (typeof v === 'object') {
      // Try common text-bearing keys before serializing.
      const keys = ['text', 'label', 'title', 'name', 'value', 'content'];
      for (const k of keys) {
        if (typeof v[k] === 'string') return v[k];
      }
      try { return JSON.stringify(v); } catch (e) { return ''; }
    }
    return String(v);
  }

  // Minimal safe Markdown — escape-then-allow-list. Sufficient for Markdown
  // component content; we don't want to expose unrestricted innerHTML here.
  function a2uiEscapeHtml(s) {
    return String(s == null ? '' : s).replace(/[&<>"']/g, c =>
      c === '&' ? '&amp;' :
      c === '<' ? '&lt;' :
      c === '>' ? '&gt;' :
      c === '"' ? '&quot;' : '&#39;');
  }
  function a2uiRenderMarkdown(src) {
    // Some models over-escape newlines/tabs when writing JSON, sending the
    // literal 2-char sequence "\n" (backslash + n) where they meant a real
    // newline. JSON.parse decodes those to literal backslash-n in the JS
    // string, which leaks into rendered prose / code blocks / blockquotes.
    // Convert defensively before markdown parsing.
    if (typeof src === 'string' && src.indexOf('\\') !== -1) {
      src = src.replace(/\\r\\n/g, '\n').replace(/\\n/g, '\n').replace(/\\t/g, '\t');
    }
    let s = a2uiEscapeHtml(src);
    // Use a placeholder that can't collide with prose ("CB0" did, as you
    // saw at end-of-input where the space-bounded marker matcher failed).
    //   are control chars escapeHtml leaves alone and that
    // never appear in normal text.
    const blocks = [];
    s = s.replace(/```([a-zA-Z0-9_-]*)\n([\s\S]*?)```/g, (_, lang, code) => {
      const idx = blocks.push('<pre class="a2ui-md-pre" data-lang="' + lang + '"><code>' + code + '</code></pre>') - 1;
      return 'CB' + idx + '';
    });
    s = s.replace(/`([^`\n]+?)`/g, '<code class="a2ui-md-code">$1</code>');
    s = s.replace(/^####\s+(.+)$/gm, '<h4>$1</h4>');
    s = s.replace(/^###\s+(.+)$/gm, '<h3>$1</h3>');
    s = s.replace(/^##\s+(.+)$/gm, '<h2>$1</h2>');
    s = s.replace(/^#\s+(.+)$/gm, '<h1>$1</h1>');
    // Blockquote — leading ">", optionally multiple lines.
    s = s.replace(/(?:^&gt;\s?.*(?:\n|$))+/gm, m => {
      const inner = m.split('\n').map(l => l.replace(/^&gt;\s?/, '')).join('<br>').replace(/(<br>)+$/, '');
      return '<blockquote class="a2ui-md-quote">' + inner + '</blockquote>';
    });
    // Bold: ** ** and __ __
    s = s.replace(/\*\*([^*\n]+?)\*\*/g, '<strong>$1</strong>');
    s = s.replace(/__([^_\n]+?)__/g, '<strong>$1</strong>');
    // Italic: * * (not **) and _ _ (not __)
    s = s.replace(/(^|[^*\w])\*([^*\n]+?)\*(?!\*)/g, '$1<em>$2</em>');
    s = s.replace(/(^|[^_\w])_([^_\n]+?)_(?!_)/g, '$1<em>$2</em>');
    s = s.replace(/\[([^\]]+)\]\((https?:\/\/[A-Za-z0-9._~:/?#\[\]@!$&'()*+,;=%-]+|mailto:[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,})\)/g,
      // No target="_blank": the sidebar has no window opener, so a blank
      // target is a dead click. A same-frame navigation is caught by Rust's
      // navigation handler and opened as a real browser tab instead.
      '<a href="$2" rel="noopener noreferrer">$1</a>');
    s = s.replace(/(?:^- .+(?:\n|$))+/gm, m => {
      const items = m.trim().split('\n').map(l => l.replace(/^-\s+/, ''));
      return '<ul>' + items.map(i => '<li>' + i + '</li>').join('') + '</ul>';
    });
    s = s.replace(/(?:^\d+\.\s.+(?:\n|$))+/gm, m => {
      const items = m.trim().split('\n').map(l => l.replace(/^\d+\.\s+/, ''));
      return '<ol>' + items.map(i => '<li>' + i + '</li>').join('') + '</ol>';
    });
    s = s.split(/\n{2,}/).map(p => {
      const t = p.trim();
      if (!t) return '';
      if (/^<(h\d|ul|ol|pre|p|blockquote)\b/.test(t)) return t;
      return '<p>' + t.replace(/\n/g, '<br>') + '</p>';
    }).join('');
    // Restore code-fence blocks (uses unambiguous control-char markers).
    s = s.replace(/CB(\d+)/g, (_, idx) => blocks[Number(idx)] || '');
    return s;
  }
"##;

/// Feed `source` to node and hand back what it said. Node is not a build
/// dependency of octoweb, so `None` means "not installed" and the caller
/// skips; CI runners have it.
#[cfg(test)]
pub(crate) fn run_node(name: &str, source: &str, flags: &[&str]) -> Option<std::process::Output> {
    let dir = std::env::temp_dir().join(format!("octoweb-js-{}-{name}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join(format!("{name}.js"));
    std::fs::write(&path, source).expect("write script");
    let out = match std::process::Command::new("node")
        .args(flags)
        .arg(&path)
        .output()
    {
        Ok(out) => Some(out),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("skipping {name}: node is not installed");
            None
        }
        Err(e) => panic!("failed to run node: {e}"),
    };
    let _ = std::fs::remove_dir_all(&dir);
    out
}

#[cfg(test)]
mod tests {
    /// Runs `CORE` under node with an assertion harness covering every catalog
    /// function, the interpolation grammar and the binding rules.
    #[test]
    fn core_js_behaves_per_spec() {
        let harness = include_str!("a2ui_core_test.js");
        let source = format!("{}\n{harness}", super::CORE);
        let Some(out) = super::run_node("a2ui_core", &source, &[]) else {
            return;
        };
        assert!(
            out.status.success(),
            "a2ui core assertions failed:\n{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr),
        );
    }
}
