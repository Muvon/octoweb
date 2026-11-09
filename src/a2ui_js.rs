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
  // Keys that would reach Object.prototype or a constructor: a path naming one
  // is treated as missing.
  const A2UI_UNSAFE_KEYS = ['__proto__', 'constructor', 'prototype'];
  function a2uiPtrParts(path) {
    return path.split('/').slice(1).map(p => p.replace(/~1/g, '/').replace(/~0/g, '~'));
  }
  function a2uiPtrUnsafe(parts) {
    return parts.some(p => A2UI_UNSAFE_KEYS.includes(p));
  }
  function a2uiPtrGet(model, path) {
    if (!path || path === '/') return model;
    const parts = a2uiPtrParts(path);
    if (a2uiPtrUnsafe(parts)) return undefined;
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
    if (a2uiPtrUnsafe(parts)) return model;
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
    if (a2uiPtrUnsafe(parts)) return model;
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

  // The v1.0 DynamicBoolean coercion table. A check `condition` resolves to a
  // boolean or to a ValidationResult ({valid, message, …}); both gate an action
  // the same way. Anything the table does not name (null, other objects and
  // arrays) is false.
  function a2uiTruthy(v) {
    if (typeof v === 'boolean') return v;
    if (typeof v === 'string') return v.toLowerCase() === 'true';
    if (typeof v === 'number') return v !== 0;
    if (v && typeof v === 'object' && !Array.isArray(v) && typeof v.valid === 'boolean') {
      return v.valid;
    }
    return false;
  }

  // Evaluation problems found while resolving values (unknown function, a
  // function that threw, `@index` outside a template, a rejected regex). Pushed
  // from anywhere with no DOM involved; the renderer drains the list after each
  // paint and reports each distinct problem once.
  const A2UI_MAX_PENDING_ERRORS = 50;
  const a2uiPendingErrors = [];
  function a2uiReportError(code, message) {
    if (a2uiPendingErrors.length >= A2UI_MAX_PENDING_ERRORS) return;
    if (a2uiPendingErrors.some(e => e.code === code && e.message === message)) return;
    a2uiPendingErrors.push({ code, message });
  }
  function a2uiDrainErrors() {
    return a2uiPendingErrors.splice(0);
  }

  // One CheckRule ({condition, message}). Null when it passes, else the
  // ValidationResult it failed with: the
  // result's own `message` wins over the rule's, a plain `false` takes the
  // rule's, and `severity` defaults to "error". Only "error" blocks an action.
  const A2UI_SEVERITIES = ['error', 'warning', 'info'];
  function a2uiCheckFailure(check, scope) {
    if (!check || typeof check !== 'object') return null;
    const condition = check.condition;
    const result = a2uiResolveValue(condition, scope);
    if (a2uiTruthy(result)) return null;
    const isResult = !!result && typeof result === 'object' && !Array.isArray(result)
      && typeof result.valid === 'boolean';
    return {
      message: String((isResult && result.message) || check.message || 'validation failed'),
      severity: isResult && A2UI_SEVERITIES.indexOf(result.severity) >= 0 ? result.severity : 'error',
    };
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

  // ── regex: the A2UI dialect and its static safety check ─────────────────────────────────
  // A portable subset of ECMAScript patterns in Unicode mode with no flags.
  // The pattern is parsed here, which rejects what the dialect forbids
  // (backreferences, lookbehind, named groups, \p{}, \c, octal, inline flags),
  // then checked for catastrophic backtracking, and only then handed to the
  // engine. Anything rejected is an EXPRESSION_ERROR and the check fails.
  const A2UI_REGEX_MAX_PATTERN = 256; // code points
  const A2UI_REGEX_MAX_VALUE = 4096;  // code points
  const A2UI_REGEX_CACHE_LIMIT = 200;
  const A2UI_MAX_CODE_POINT = 0x10FFFF;

  // Code-point sets are sorted, non-overlapping [lo, hi] pairs.
  function a2uiSetNormalize(ranges) {
    const sorted = ranges.slice().sort((a, b) => a[0] - b[0]);
    const out = [];
    for (const r of sorted) {
      const last = out[out.length - 1];
      if (last && r[0] <= last[1] + 1) last[1] = Math.max(last[1], r[1]);
      else out.push([r[0], r[1]]);
    }
    return out;
  }
  function a2uiSetComplement(set) {
    const out = [];
    let next = 0;
    for (const r of set) {
      if (r[0] > next) out.push([next, r[0] - 1]);
      next = r[1] + 1;
    }
    if (next <= A2UI_MAX_CODE_POINT) out.push([next, A2UI_MAX_CODE_POINT]);
    return out;
  }
  function a2uiSetsOverlap(a, b) {
    let i = 0, j = 0;
    while (i < a.length && j < b.length) {
      if (a[i][1] < b[j][0]) i++;
      else if (b[j][1] < a[i][0]) j++;
      else return true;
    }
    return false;
  }
  // \d, \w and \b are ASCII-only; \s is the ECMAScript WhiteSpace and
  // LineTerminator set; `.` is everything but the four line terminators.
  const A2UI_SET_DIGIT = [[48, 57]];
  const A2UI_SET_WORD = [[48, 57], [65, 90], [95, 95], [97, 122]];
  const A2UI_SET_SPACE = a2uiSetNormalize([[9, 13], [32, 32], [160, 160], [0x1680, 0x1680],
    [0x2000, 0x200A], [0x2028, 0x2029], [0x202F, 0x202F], [0x205F, 0x205F], [0x3000, 0x3000], [0xFEFF, 0xFEFF]]);
  const A2UI_SET_DOT = a2uiSetComplement([[10, 10], [13, 13], [0x2028, 0x2029]]);
  const A2UI_REGEX_SYNTAX_CHARS = '^$\\.*+?()[]{}|/';

  // Pattern -> tree. Alternations hold sequences, a sequence holds quantified
  // items {atom, min, max, lazy} (max is Infinity when unbounded), and an atom
  // is a char set, a group, a lookahead or a zero-width assertion.
  function a2uiParseRegex(pattern) {
    const cps = Array.from(pattern);
    let i = 0;
    const fail = msg => { throw new Error(msg); };
    const isHex = c => c !== undefined && /^[0-9a-fA-F]$/.test(c);
    const isDigit = c => c !== undefined && /^[0-9]$/.test(c);
    const eat = c => (cps[i] === c ? (i++, true) : false);

    function hexValue(count) {
      let s = '';
      for (let k = 0; k < count; k++) {
        if (!isHex(cps[i])) fail('invalid hex escape');
        s += cps[i++];
      }
      return parseInt(s, 16);
    }
    function unicodeEscape() {
      if (eat('{')) {
        let s = '';
        while (isHex(cps[i])) s += cps[i++];
        if (!s || !eat('}')) fail('invalid \\u{...} escape');
        const v = parseInt(s, 16);
        if (v > A2UI_MAX_CODE_POINT) fail('code point out of range');
        return v;
      }
      const high = hexValue(4);
      // Two escapes that form a surrogate pair are one code point in Unicode mode.
      if (high >= 0xD800 && high <= 0xDBFF && cps[i] === '\\' && cps[i + 1] === 'u') {
        const save = i;
        i += 2;
        try {
          const low = hexValue(4);
          if (low >= 0xDC00 && low <= 0xDFFF) return (high - 0xD800) * 0x400 + (low - 0xDC00) + 0x10000;
        } catch (e) { /* not a pair: the high surrogate stands alone */ }
        i = save;
      }
      return high;
    }
    // After a backslash. Returns {cp}, {set} or {assert}.
    function escape(inClass) {
      const c = cps[i++];
      if (c === undefined) fail('the pattern ends with a backslash');
      switch (c) {
        case 'd': return { set: A2UI_SET_DIGIT };
        case 'D': return { set: a2uiSetComplement(A2UI_SET_DIGIT) };
        case 'w': return { set: A2UI_SET_WORD };
        case 'W': return { set: a2uiSetComplement(A2UI_SET_WORD) };
        case 's': return { set: A2UI_SET_SPACE };
        case 'S': return { set: a2uiSetComplement(A2UI_SET_SPACE) };
        case 't': return { cp: 9 };
        case 'n': return { cp: 10 };
        case 'v': return { cp: 11 };
        case 'f': return { cp: 12 };
        case 'r': return { cp: 13 };
        case 'b': return inClass ? { cp: 8 } : { assert: true };
        case 'B':
          if (inClass) fail('\\B is not valid in a class');
          return { assert: true };
        case '0':
          if (isDigit(cps[i])) fail('octal escapes are not allowed');
          return { cp: 0 };
        case 'x': return { cp: hexValue(2) };
        case 'u': return { cp: unicodeEscape() };
        case 'c': case 'k': case 'p': case 'P':
          fail('\\' + c + ' is not allowed');
      }
      if (isDigit(c)) fail('backreferences are not allowed');
      if (A2UI_REGEX_SYNTAX_CHARS.indexOf(c) >= 0 || (inClass && c === '-')) return { cp: c.codePointAt(0) };
      fail('unsupported escape \\' + c);
    }
    function charClass() {
      const negate = eat('^');
      const ranges = [];
      const classAtom = () => {
        const c = cps[i++];
        if (c === undefined) fail('unterminated character class');
        return c === '\\' ? escape(true) : { cp: c.codePointAt(0) };
      };
      while (cps[i] !== ']') {
        if (i >= cps.length) fail('unterminated character class');
        const low = classAtom();
        if (cps[i] === '-' && cps[i + 1] !== undefined && cps[i + 1] !== ']') {
          i++;
          const high = classAtom();
          if (low.set || high.set) fail('a class escape cannot bound a range');
          if (low.cp > high.cp) fail('class range out of order');
          ranges.push([low.cp, high.cp]);
        } else if (low.set) {
          for (const r of low.set) ranges.push(r);
        } else {
          ranges.push([low.cp, low.cp]);
        }
      }
      i++;
      const set = a2uiSetNormalize(ranges);
      return negate ? a2uiSetComplement(set) : set;
    }
    function groupBody() {
      const body = alternation();
      if (!eat(')')) fail('unclosed group');
      return body;
    }
    function atom() {
      const c = cps[i++];
      switch (c) {
        case '(':
          if (eat('?')) {
            if (eat(':')) return { t: 'group', body: groupBody() };
            if (eat('=') || eat('!')) return { t: 'look', body: groupBody() };
            if (cps[i] === '<') {
              fail(cps[i + 1] === '=' || cps[i + 1] === '!'
                ? 'lookbehind is not allowed' : 'named groups are not allowed');
            }
            fail('inline flags are not allowed');
          }
          return { t: 'group', body: groupBody() };
        case '[': return { t: 'char', set: charClass() };
        case '.': return { t: 'char', set: A2UI_SET_DOT };
        case '^': case '$': return { t: 'assert' };
        case '\\': {
          const e = escape(false);
          if (e.assert) return { t: 'assert' };
          return { t: 'char', set: e.set || [[e.cp, e.cp]] };
        }
        case '*': case '+': case '?': fail('nothing to repeat');
        case '{': case '}': case ']': fail('unexpected ' + c);
      }
      return { t: 'char', set: [[c.codePointAt(0), c.codePointAt(0)]] };
    }
    function count() {
      let s = '';
      while (isDigit(cps[i])) s += cps[i++];
      if (!s) fail('invalid quantifier');
      return parseInt(s, 10);
    }
    function item() {
      const a = atom();
      let min = 1, max = 1, quantified = true;
      if (eat('*')) { min = 0; max = Infinity; }
      else if (eat('+')) { max = Infinity; }
      else if (eat('?')) { min = 0; }
      else if (eat('{')) {
        min = max = count();
        if (eat(',')) max = cps[i] === '}' ? Infinity : count();
        if (!eat('}')) fail('invalid quantifier');
        if (max < min) fail('quantifier bounds out of order');
      } else {
        quantified = false;
      }
      let lazy = false;
      if (quantified) {
        if (a.t === 'assert' || a.t === 'look') fail('nothing to repeat');
        lazy = eat('?');
      }
      return { atom: a, min, max, lazy };
    }
    function sequence() {
      const items = [];
      while (i < cps.length && cps[i] !== '|' && cps[i] !== ')') items.push(item());
      return { t: 'seq', items };
    }
    function alternation() {
      const branches = [sequence()];
      while (eat('|')) branches.push(sequence());
      return { t: 'alt', branches };
    }

    if (cps.length > A2UI_REGEX_MAX_PATTERN) fail('the pattern is longer than 256 code points');
    const tree = alternation();
    if (i < cps.length) fail('unmatched )');
    return tree;
  }

  function a2uiRxItemNullable(it) { return it.min === 0 || a2uiRxNullable(it.atom); }
  function a2uiRxNullable(n) {
    switch (n.t) {
      case 'assert': case 'look': return true;
      case 'group': return a2uiRxNullable(n.body);
      case 'alt': return n.branches.some(a2uiRxNullable);
      case 'seq': return n.items.every(a2uiRxItemNullable);
    }
    return false;
  }
  // The code points a non-empty match of `n` can begin with.
  function a2uiRxFirst(n) {
    switch (n.t) {
      case 'char': return n.set;
      case 'group': return a2uiRxFirst(n.body);
      case 'alt': return a2uiSetNormalize([].concat(...n.branches.map(a2uiRxFirst)));
      case 'seq': {
        let out = [];
        for (const it of n.items) {
          if (it.max > 0) out = out.concat(a2uiRxFirst(it.atom));
          if (!a2uiRxItemNullable(it)) break;
        }
        return a2uiSetNormalize(out);
      }
    }
    return [];
  }
  function a2uiRxHasUnbounded(n) {
    switch (n.t) {
      case 'group': case 'look': return a2uiRxHasUnbounded(n.body);
      case 'alt': return n.branches.some(a2uiRxHasUnbounded);
      case 'seq': return n.items.some(it => it.max === Infinity || a2uiRxHasUnbounded(it.atom));
    }
    return false;
  }
  // The one item a group holds, if it holds exactly one: the group then counts
  // as that item.
  function a2uiRxSoleItem(atom) {
    if (atom.t !== 'group') return null;
    const branches = atom.body.branches;
    return branches.length === 1 && branches[0].items.length === 1 ? branches[0].items[0] : null;
  }
  // A repeated plain character set (through any single-item groups), or null.
  function a2uiRxSetItem(item) {
    let it = item;
    let unbounded = it.max === Infinity;
    for (let sole = a2uiRxSoleItem(it.atom); sole; sole = a2uiRxSoleItem(it.atom)) {
      it = sole;
      unbounded = unbounded || it.max === Infinity;
    }
    return it.atom.t === 'char' ? { set: it.atom.set, unbounded } : null;
  }
  // The first safety rule the tree breaks, as text, or null when it is safe.
  // (The length rule is applied by the parser.)
  function a2uiRxUnsafe(n) {
    switch (n.t) {
      case 'group': case 'look': return a2uiRxUnsafe(n.body);
      case 'alt':
        for (const branch of n.branches) {
          const reason = a2uiRxUnsafe(branch);
          if (reason) return reason;
        }
        return null;
      case 'seq': {
        for (const it of n.items) {
          if (it.max > 1 && a2uiRxHasUnbounded(it.atom)) {
            return 'a repeat above 1 contains an unbounded repeat';
          }
          if (it.max === Infinity) {
            let target = it.atom;
            for (let sole = a2uiRxSoleItem(target); sole && sole.min === 1 && sole.max === 1; sole = a2uiRxSoleItem(target)) {
              target = sole.atom;
            }
            if (target.t === 'group' && target.body.branches.length > 1) {
              const firsts = target.body.branches.map(a2uiRxFirst);
              for (let a = 0; a < firsts.length; a++) {
                for (let b = a + 1; b < firsts.length; b++) {
                  if (a2uiSetsOverlap(firsts[a], firsts[b])) {
                    return 'an unbounded repeat of an alternation whose branches start alike';
                  }
                }
              }
            }
          }
          const inner = a2uiRxUnsafe(it.atom);
          if (inner) return inner;
        }
        const sets = n.items.map(a2uiRxSetItem);
        for (let a = 0; a < sets.length; a++) {
          if (!sets[a] || !sets[a].unbounded) continue;
          for (let b = a + 1; b < sets.length; b++) {
            if (sets[b] && sets[b].unbounded && a2uiSetsOverlap(sets[a].set, sets[b].set)) {
              return 'two adjacent unbounded repeats match the same characters';
            }
            if (!a2uiRxItemNullable(n.items[b])) break;
          }
        }
        return null;
      }
    }
    return null;
  }

  // Parse + check + compile once per pattern; the verdict is cached because a
  // bound regex is re-evaluated on every keystroke.
  const a2uiRegexCache = new Map();
  function a2uiCompileRegex(pattern) {
    let entry = a2uiRegexCache.get(pattern);
    if (entry) return entry;
    try {
      const reason = a2uiRxUnsafe(a2uiParseRegex(pattern));
      if (reason) throw new Error('unsafe pattern (' + reason + ')');
      entry = { re: new RegExp(pattern, 'u') };
    } catch (e) {
      entry = { error: String(e && e.message ? e.message : e) };
    }
    if (a2uiRegexCache.size >= A2UI_REGEX_CACHE_LIMIT) a2uiRegexCache.clear();
    a2uiRegexCache.set(pattern, entry);
    return entry;
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
      const compiled = a2uiCompileRegex(a2uiValStr(pattern));
      if (compiled.error) {
        a2uiReportError('EXPRESSION_ERROR', 'regex pattern rejected: ' + compiled.error);
        return false;
      }
      const text = a2uiValStr(value);
      // Longer values never match, and are not an error.
      if (text.length > A2UI_REGEX_MAX_VALUE && Array.from(text).length > A2UI_REGEX_MAX_VALUE) return false;
      return compiled.re.test(text);
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
      if (!a2uiActivated) {
        a2uiReportError('EXPRESSION_ERROR', 'openUrl needs a user click and cannot run from a property or a check.');
        return undefined;
      }
      const u = a2uiValStr(url);
      // v1.0 mandates an http(s)-only allowlist: no javascript:, no data:.
      if (!/^https?:\/\//i.test(u)) return undefined;
      window.ipc.postMessage(JSON.stringify({ type: 'a2ui_open_url', url: u }));
      return undefined;
    },

    // ── System function (available in every catalog).
    '@index': ({ offset }, scope) => {
      if (!scope || scope.index == null) {
        a2uiReportError('EXPRESSION_ERROR', '@index() was used outside a list template.');
        return undefined;
      }
      return Number(scope.index) + (offset == null ? 0 : Number(offset));
    },
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
    return token ? { '@path': token } : '';
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
    return { '@call': name, args };
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

  // A leading "@" in an object key is reserved; "@@x" is the literal key "@x".
  const A2UI_RESERVED_KEY = /^@([^@]|$)/;
  function a2uiUnescapeKeys(v) {
    if (Array.isArray(v)) return v.map(a2uiUnescapeKeys);
    if (v == null || typeof v !== 'object') return v;
    const out = {};
    for (const k in v) out[k.indexOf('@@') === 0 ? k.slice(1) : k] = a2uiUnescapeKeys(v[k]);
    return out;
  }
  function a2uiResolveValue(v, scope) {
    if (v == null) return v;
    if (typeof v !== 'object') return v;
    if (Array.isArray(v)) return v.map(x => a2uiResolveValue(x, scope));
    if (typeof v['@path'] === 'string') {
      const p = v['@path'];
      // List scopes carry an absolute path as well as the initially resolved
      // item. Re-read through that path so replacing a scalar item does not
      // leave derived outputs observing the stale pre-edit value.
      const hasModelPath = scope && typeof scope.modelPath === 'string';
      const local = hasModelPath ? a2uiPtrGet(scope.root, scope.modelPath)
        : scope && scope.local;
      const hasLocal = hasModelPath || (scope && scope.local != null);
      // A path starting with "/" is absolute and `/` is always the data-model
      // root, in every scope. The spec has no way to bind a scalar list item
      // (a string in a string[]), so the empty relative path stays as that one
      // octoweb extension: it names the current item.
      if (p.charAt(0) === '/') return a2uiPtrGet(scope.root, p);
      if (!hasLocal) return undefined;
      if (p === '') return local;
      return a2uiPtrGet(local, '/' + p);
    }
    if (typeof v['@call'] === 'string') {
      const name = v['@call'];
      if (!Object.prototype.hasOwnProperty.call(A2UI_FN, name)) {
        a2uiReportError('EXPRESSION_ERROR', 'Unknown function "' + name + '".');
        return undefined;
      }
      const args = {};
      const rawArgs = v.args || {};
      for (const k in rawArgs) args[k] = a2uiResolveValue(rawArgs[k], scope);
      try {
        return A2UI_FN[name](args, scope || {});
      } catch (e) {
        // A function that ran from an action failed to execute; one that was
        // resolving a property or a check failed to evaluate.
        a2uiReportError(a2uiActivated ? 'EXECUTION_ERROR' : 'EXPRESSION_ERROR',
          'Function "' + name + '" failed: ' + String(e && e.message ? e.message : e));
        return undefined;
      }
    }
    for (const k in v) if (A2UI_RESERVED_KEY.test(k)) return undefined;
    return a2uiUnescapeKeys(v);
  }

  function a2uiPathOf(v, scope) {
    if (!v || typeof v !== 'object' || typeof v['@path'] !== 'string') return null;
    const path = v['@path'];
    // A list row carries the absolute JSON-Pointer path of its local model.
    // Mirror a2uiResolveValue's empty-path item binding, then join ordinary
    // relative bindings onto that path so edits can write back.
    if (scope && typeof scope.modelPath === 'string') {
      if (path === '') return scope.modelPath;
      if (path.charAt(0) !== '/') {
        const base = scope.modelPath === '/' ? '' : scope.modelPath.replace(/\/$/, '');
        return base + '/' + path;
      }
    }
    return path.charAt(0) === '/' ? path : null;
  }

  // Stringify a resolved value for display in a text/input slot. Avoids the
  // `String({...})` → "[object Object]" trap when the agent points a text
  // binding at an object subtree: we drill into common content fields, fall
  // back to compact JSON, and only ever pass scalars through unchanged.
  function a2uiToStr(v) {
    if (v == null) return '';
    // JSON.parse has already decoded JSON escapes. Any backslash sequences
    // still present are literal user/model data (often code or a regex) and
    // must survive rendering byte-for-byte.
    if (typeof v === 'string') return v;
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

  // Material Design Icons filled 24px paths, one entry per catalog Icon name
  // (https://www.npmjs.com/package/@material-design-icons/svg 0.14.15, Apache-2.0).
  const A2UI_ICON_PATHS = {
    accountCircle: 'M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm0 4c1.93 0 3.5 1.57 3.5 3.5S13.93 13 12 13s-3.5-1.57-3.5-3.5S10.07 6 12 6zm0 14c-2.03 0-4.43-.82-6.14-2.88a9.947 9.947 0 0 1 12.28 0C16.43 19.18 14.03 20 12 20z',
    add: 'M19 13h-6v6h-2v-6H5v-2h6V5h2v6h6v2z',
    arrowBack: 'M20 11H7.83l5.59-5.59L12 4l-8 8 8 8 1.41-1.41L7.83 13H20v-2z',
    arrowForward: 'm12 4-1.41 1.41L16.17 11H4v2h12.17l-5.58 5.59L12 20l8-8z',
    attachFile: 'M16.5 6v11.5c0 2.21-1.79 4-4 4s-4-1.79-4-4V5a2.5 2.5 0 0 1 5 0v10.5c0 .55-.45 1-1 1s-1-.45-1-1V6H10v9.5a2.5 2.5 0 0 0 5 0V5c0-2.21-1.79-4-4-4S7 2.79 7 5v12.5c0 3.04 2.46 5.5 5.5 5.5s5.5-2.46 5.5-5.5V6h-1.5z',
    calendarToday: 'M20 3h-1V1h-2v2H7V1H5v2H4c-1.1 0-2 .9-2 2v16c0 1.1.9 2 2 2h16c1.1 0 2-.9 2-2V5c0-1.1-.9-2-2-2zm0 18H4V8h16v13z',
    call: 'M20.01 15.38c-1.23 0-2.42-.2-3.53-.56a.977.977 0 0 0-1.01.24l-1.57 1.97c-2.83-1.35-5.48-3.9-6.89-6.83l1.95-1.66c.27-.28.35-.67.24-1.02-.37-1.11-.56-2.3-.56-3.53 0-.54-.45-.99-.99-.99H4.19C3.65 3 3 3.24 3 3.99 3 13.28 10.73 21 20.01 21c.71 0 .99-.63.99-1.18v-3.45c0-.54-.45-.99-.99-.99z',
    camera: 'm9.4 10.5 4.77-8.26a9.984 9.984 0 0 0-8.49 2.01l3.66 6.35.06-.1zM21.54 9c-.92-2.92-3.15-5.26-6-6.34L11.88 9h9.66zm.26 1h-7.49l.29.5 4.76 8.25A9.91 9.91 0 0 0 22 12c0-.69-.07-1.35-.2-2zM8.54 12l-3.9-6.75A9.958 9.958 0 0 0 2.2 14h7.49l-1.15-2zm-6.08 3c.92 2.92 3.15 5.26 6 6.34L12.12 15H2.46zm11.27 0-3.9 6.76a9.984 9.984 0 0 0 8.49-2.01l-3.66-6.35-.93 1.6z',
    check: 'M9 16.17 4.83 12l-1.42 1.41L9 19 21 7l-1.41-1.41z',
    close: 'M19 6.41 17.59 5 12 10.59 6.41 5 5 6.41 10.59 12 5 17.59 6.41 19 12 13.41 17.59 19 19 17.59 13.41 12z',
    delete: 'M6 19c0 1.1.9 2 2 2h8c1.1 0 2-.9 2-2V7H6v12zM19 4h-3.5l-1-1h-5l-1 1H5v2h14V4z',
    download: 'M5 20h14v-2H5v2zM19 9h-4V3H9v6H5l7 7 7-7z',
    edit: 'M3 17.25V21h3.75L17.81 9.94l-3.75-3.75L3 17.25zM20.71 7.04a.996.996 0 0 0 0-1.41l-2.34-2.34a.996.996 0 0 0-1.41 0l-1.83 1.83 3.75 3.75 1.83-1.83z',
    event: 'M17 12h-5v5h5v-5zM16 1v2H8V1H6v2H5c-1.11 0-1.99.9-1.99 2L3 19a2 2 0 0 0 2 2h14c1.1 0 2-.9 2-2V5c0-1.1-.9-2-2-2h-1V1h-2zm3 18H5V8h14v11z',
    error: 'M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm1 15h-2v-2h2v2zm0-4h-2V7h2v6z',
    fastForward: 'm4 18 8.5-6L4 6v12zm9-12v12l8.5-6L13 6z',
    favorite: 'm12 21.35-1.45-1.32C5.4 15.36 2 12.28 2 8.5 2 5.42 4.42 3 7.5 3c1.74 0 3.41.81 4.5 2.09C13.09 3.81 14.76 3 16.5 3 19.58 3 22 5.42 22 8.5c0 3.78-3.4 6.86-8.55 11.54L12 21.35z',
    favoriteOff: 'M16.5 3c-1.74 0-3.41.81-4.5 2.09C10.91 3.81 9.24 3 7.5 3 4.42 3 2 5.42 2 8.5c0 3.78 3.4 6.86 8.55 11.54L12 21.35l1.45-1.32C18.6 15.36 22 12.28 22 8.5 22 5.42 19.58 3 16.5 3zm-4.4 15.55-.1.1-.1-.1C7.14 14.24 4 11.39 4 8.5 4 6.5 5.5 5 7.5 5c1.54 0 3.04.99 3.57 2.36h1.87C13.46 5.99 14.96 5 16.5 5c2 0 3.5 1.5 3.5 3.5 0 2.89-3.14 5.74-7.9 10.05z',
    folder: 'M10 4H4c-1.1 0-1.99.9-1.99 2L2 18c0 1.1.9 2 2 2h16c1.1 0 2-.9 2-2V8c0-1.1-.9-2-2-2h-8l-2-2z',
    help: 'M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm1 17h-2v-2h2v2zm2.07-7.75-.9.92C13.45 12.9 13 13.5 13 15h-2v-.5c0-1.1.45-2.1 1.17-2.83l1.24-1.26c.37-.36.59-.86.59-1.41 0-1.1-.9-2-2-2s-2 .9-2 2H8c0-2.21 1.79-4 4-4s4 1.79 4 4c0 .88-.36 1.68-.93 2.25z',
    home: 'M10 20v-6h4v6h5v-8h3L12 3 2 12h3v8z',
    info: 'M12 2C6.48 2 2 6.48 2 12s4.48 10 10 10 10-4.48 10-10S17.52 2 12 2zm1 15h-2v-6h2v6zm0-8h-2V7h2v2z',
    locationOn: 'M12 2C8.13 2 5 5.13 5 9c0 5.25 7 13 7 13s7-7.75 7-13c0-3.87-3.13-7-7-7zm0 9.5a2.5 2.5 0 0 1 0-5 2.5 2.5 0 0 1 0 5z',
    lock: 'M18 8h-1V6c0-2.76-2.24-5-5-5S7 3.24 7 6v2H6c-1.1 0-2 .9-2 2v10c0 1.1.9 2 2 2h12c1.1 0 2-.9 2-2V10c0-1.1-.9-2-2-2zm-6 9c-1.1 0-2-.9-2-2s.9-2 2-2 2 .9 2 2-.9 2-2 2zm3.1-9H8.9V6c0-1.71 1.39-3.1 3.1-3.1 1.71 0 3.1 1.39 3.1 3.1v2z',
    lockOpen: 'M12 17c1.1 0 2-.9 2-2s-.9-2-2-2-2 .9-2 2 .9 2 2 2zm6-9h-1V6c0-2.76-2.24-5-5-5S7 3.24 7 6h1.9c0-1.71 1.39-3.1 3.1-3.1 1.71 0 3.1 1.39 3.1 3.1v2H6c-1.1 0-2 .9-2 2v10c0 1.1.9 2 2 2h12c1.1 0 2-.9 2-2V10c0-1.1-.9-2-2-2zm0 12H6V10h12v10z',
    mail: 'M20 4H4c-1.1 0-1.99.9-1.99 2L2 18c0 1.1.9 2 2 2h16c1.1 0 2-.9 2-2V6c0-1.1-.9-2-2-2zm0 4-8 5-8-5V6l8 5 8-5v2z',
    menu: 'M3 18h18v-2H3v2zm0-5h18v-2H3v2zm0-7v2h18V6H3z',
    moreVert: 'M12 8c1.1 0 2-.9 2-2s-.9-2-2-2-2 .9-2 2 .9 2 2 2zm0 2c-1.1 0-2 .9-2 2s.9 2 2 2 2-.9 2-2-.9-2-2-2zm0 6c-1.1 0-2 .9-2 2s.9 2 2 2 2-.9 2-2-.9-2-2-2z',
    moreHoriz: 'M6 10c-1.1 0-2 .9-2 2s.9 2 2 2 2-.9 2-2-.9-2-2-2zm12 0c-1.1 0-2 .9-2 2s.9 2 2 2 2-.9 2-2-.9-2-2-2zm-6 0c-1.1 0-2 .9-2 2s.9 2 2 2 2-.9 2-2-.9-2-2-2z',
    notificationsOff: 'M20 18.69 7.84 6.14 5.27 3.49 4 4.76l2.8 2.8v.01c-.52.99-.8 2.16-.8 3.42v5l-2 2v1h13.73l2 2L21 19.72l-1-1.03zM12 22c1.11 0 2-.89 2-2h-4c0 1.11.89 2 2 2zm6-7.32V11c0-3.08-1.64-5.64-4.5-6.32V4c0-.83-.67-1.5-1.5-1.5s-1.5.67-1.5 1.5v.68c-.15.03-.29.08-.42.12-.1.03-.2.07-.3.11h-.01c-.01 0-.01 0-.02.01-.23.09-.46.2-.68.31 0 0-.01 0-.01.01L18 14.68z',
    notifications: 'M12 22c1.1 0 2-.9 2-2h-4a2 2 0 0 0 2 2zm6-6v-5c0-3.07-1.64-5.64-4.5-6.32V4c0-.83-.67-1.5-1.5-1.5s-1.5.67-1.5 1.5v.68C7.63 5.36 6 7.92 6 11v5l-2 2v1h16v-1l-2-2z',
    pause: 'M6 19h4V5H6v14zm8-14v14h4V5h-4z',
    payment: 'M20 4H4c-1.11 0-1.99.89-1.99 2L2 18c0 1.11.89 2 2 2h16c1.11 0 2-.89 2-2V6c0-1.11-.89-2-2-2zm0 14H4v-6h16v6zm0-10H4V6h16v2z',
    person: 'M12 12c2.21 0 4-1.79 4-4s-1.79-4-4-4-4 1.79-4 4 1.79 4 4 4zm0 2c-2.67 0-8 1.34-8 4v2h16v-2c0-2.66-5.33-4-8-4z',
    phone: 'M6.62 10.79c1.44 2.83 3.76 5.14 6.59 6.59l2.2-2.2c.27-.27.67-.36 1.02-.24 1.12.37 2.33.57 3.57.57.55 0 1 .45 1 1V20c0 .55-.45 1-1 1-9.39 0-17-7.61-17-17 0-.55.45-1 1-1h3.5c.55 0 1 .45 1 1 0 1.25.2 2.45.57 3.57.11.35.03.74-.25 1.02l-2.2 2.2z',
    photo: 'M21 19V5c0-1.1-.9-2-2-2H5c-1.1 0-2 .9-2 2v14c0 1.1.9 2 2 2h14c1.1 0 2-.9 2-2zM8.5 13.5l2.5 3.01L14.5 12l4.5 6H5l3.5-4.5z',
    play: 'M8 5v14l11-7z',
    print: 'M19 8H5c-1.66 0-3 1.34-3 3v6h4v4h12v-4h4v-6c0-1.66-1.34-3-3-3zm-3 11H8v-5h8v5zm3-7c-.55 0-1-.45-1-1s.45-1 1-1 1 .45 1 1-.45 1-1 1zm-1-9H6v4h12V3z',
    refresh: 'M17.65 6.35A7.958 7.958 0 0 0 12 4c-4.42 0-7.99 3.58-7.99 8s3.57 8 7.99 8c3.73 0 6.84-2.55 7.73-6h-2.08A5.99 5.99 0 0 1 12 18c-3.31 0-6-2.69-6-6s2.69-6 6-6c1.66 0 3.14.69 4.22 1.78L13 11h7V4l-2.35 2.35z',
    rewind: 'M11 18V6l-8.5 6 8.5 6zm.5-6 8.5 6V6l-8.5 6z',
    search: 'M15.5 14h-.79l-.28-.27A6.471 6.471 0 0 0 16 9.5 6.5 6.5 0 1 0 9.5 16c1.61 0 3.09-.59 4.23-1.57l.27.28v.79l5 4.99L20.49 19l-4.99-5zm-6 0C7.01 14 5 11.99 5 9.5S7.01 5 9.5 5 14 7.01 14 9.5 11.99 14 9.5 14z',
    send: 'M2.01 21 23 12 2.01 3 2 10l15 2-15 2z',
    settings: 'M19.14 12.94c.04-.3.06-.61.06-.94 0-.32-.02-.64-.07-.94l2.03-1.58a.49.49 0 0 0 .12-.61l-1.92-3.32a.488.488 0 0 0-.59-.22l-2.39.96c-.5-.38-1.03-.7-1.62-.94l-.36-2.54a.484.484 0 0 0-.48-.41h-3.84c-.24 0-.43.17-.47.41l-.36 2.54c-.59.24-1.13.57-1.62.94l-2.39-.96c-.22-.08-.47 0-.59.22L2.74 8.87c-.12.21-.08.47.12.61l2.03 1.58c-.05.3-.09.63-.09.94s.02.64.07.94l-2.03 1.58a.49.49 0 0 0-.12.61l1.92 3.32c.12.22.37.29.59.22l2.39-.96c.5.38 1.03.7 1.62.94l.36 2.54c.05.24.24.41.48.41h3.84c.24 0 .44-.17.47-.41l.36-2.54c.59-.24 1.13-.56 1.62-.94l2.39.96c.22.08.47 0 .59-.22l1.92-3.32c.12-.22.07-.47-.12-.61l-2.01-1.58zM12 15.6c-1.98 0-3.6-1.62-3.6-3.6s1.62-3.6 3.6-3.6 3.6 1.62 3.6 3.6-1.62 3.6-3.6 3.6z',
    share: 'M18 16.08c-.76 0-1.44.3-1.96.77L8.91 12.7c.05-.23.09-.46.09-.7s-.04-.47-.09-.7l7.05-4.11c.54.5 1.25.81 2.04.81 1.66 0 3-1.34 3-3s-1.34-3-3-3-3 1.34-3 3c0 .24.04.47.09.7L8.04 9.81C7.5 9.31 6.79 9 6 9c-1.66 0-3 1.34-3 3s1.34 3 3 3c.79 0 1.5-.31 2.04-.81l7.12 4.16c-.05.21-.08.43-.08.65 0 1.61 1.31 2.92 2.92 2.92 1.61 0 2.92-1.31 2.92-2.92s-1.31-2.92-2.92-2.92z',
    shoppingCart: 'M7 18c-1.1 0-1.99.9-1.99 2S5.9 22 7 22s2-.9 2-2-.9-2-2-2zM1 2v2h2l3.6 7.59-1.35 2.45c-.16.28-.25.61-.25.96 0 1.1.9 2 2 2h12v-2H7.42c-.14 0-.25-.11-.25-.25l.03-.12.9-1.63h7.45c.75 0 1.41-.41 1.75-1.03l3.58-6.49A1.003 1.003 0 0 0 20 4H5.21l-.94-2H1zm16 16c-1.1 0-1.99.9-1.99 2s.89 2 1.99 2 2-.9 2-2-.9-2-2-2z',
    skipNext: 'm6 18 8.5-6L6 6v12zM16 6v12h2V6h-2z',
    skipPrevious: 'M6 6h2v12H6zm3.5 6 8.5 6V6z',
    star: 'M12 17.27 18.18 21l-1.64-7.03L22 9.24l-7.19-.61L12 2 9.19 8.63 2 9.24l5.46 4.73L5.82 21z',
    starHalf: 'm22 9.24-7.19-.62L12 2 9.19 8.63 2 9.24l5.46 4.73L5.82 21 12 17.27 18.18 21l-1.63-7.03L22 9.24zM12 15.4V6.1l1.71 4.04 4.38.38-3.32 2.88 1 4.28L12 15.4z',
    starOff: 'm22 9.24-7.19-.62L12 2 9.19 8.63 2 9.24l5.46 4.73L5.82 21 12 17.27 18.18 21l-1.63-7.03L22 9.24zM12 15.4l-3.76 2.27 1-4.28-3.32-2.88 4.38-.38L12 6.1l1.71 4.04 4.38.38-3.32 2.88 1 4.28L12 15.4z',
    stop: 'M6 6h12v12H6z',
    upload: 'M5 20h14v-2H5v2zm0-10h4v6h6v-6h4l-7-7-7 7z',
    visibility: 'M12 4.5C7 4.5 2.73 7.61 1 12c1.73 4.39 6 7.5 11 7.5s9.27-3.11 11-7.5c-1.73-4.39-6-7.5-11-7.5zM12 17c-2.76 0-5-2.24-5-5s2.24-5 5-5 5 2.24 5 5-2.24 5-5 5zm0-8c-1.66 0-3 1.34-3 3s1.34 3 3 3 3-1.34 3-3-1.34-3-3-3z',
    visibilityOff: 'M12 7c2.76 0 5 2.24 5 5 0 .65-.13 1.26-.36 1.83l2.92 2.92c1.51-1.26 2.7-2.89 3.43-4.75-1.73-4.39-6-7.5-11-7.5-1.4 0-2.74.25-3.98.7l2.16 2.16C10.74 7.13 11.35 7 12 7zM2 4.27l2.28 2.28.46.46A11.804 11.804 0 0 0 1 12c1.73 4.39 6 7.5 11 7.5 1.55 0 3.03-.3 4.38-.84l.42.42L19.73 22 21 20.73 3.27 3 2 4.27zM7.53 9.8l1.55 1.55c-.05.21-.08.43-.08.65 0 1.66 1.34 3 3 3 .22 0 .44-.03.65-.08l1.55 1.55c-.67.33-1.41.53-2.2.53-2.76 0-5-2.24-5-5 0-.79.2-1.53.53-2.2zm4.31-.78 3.15 3.15.02-.16c0-1.66-1.34-3-3-3l-.17.01z',
    volumeDown: 'M18.5 12A4.5 4.5 0 0 0 16 7.97v8.05c1.48-.73 2.5-2.25 2.5-4.02zM5 9v6h4l5 5V4L9 9H5z',
    volumeMute: 'M7 9v6h4l5 5V4l-5 5H7z',
    volumeOff: 'M16.5 12A4.5 4.5 0 0 0 14 7.97v2.21l2.45 2.45c.03-.2.05-.41.05-.63zm2.5 0c0 .94-.2 1.82-.54 2.64l1.51 1.51A8.796 8.796 0 0 0 21 12c0-4.28-2.99-7.86-7-8.77v2.06c2.89.86 5 3.54 5 6.71zM4.27 3 3 4.27 7.73 9H3v6h4l5 5v-6.73l4.25 4.25c-.67.52-1.42.93-2.25 1.18v2.06a8.99 8.99 0 0 0 3.69-1.81L19.73 21 21 19.73l-9-9L4.27 3zM12 4 9.91 6.09 12 8.18V4z',
    volumeUp: 'M3 9v6h4l5 5V4L7 9H3zm13.5 3A4.5 4.5 0 0 0 14 7.97v8.05c1.48-.73 2.5-2.25 2.5-4.02zM14 3.23v2.06c2.89.86 5 3.54 5 6.71s-2.11 5.85-5 6.71v2.06c4.01-.91 7-4.49 7-8.77s-2.99-7.86-7-8.77z',
    warning: 'M1 21h22L12 2 1 21zm12-3h-2v-2h2v2zm0-4h-2v-4h2v4z',
  };

  // Keys a component instance for view state (selected tab, open modal, filter
  // text). A template row reuses one component id, so the row's model path
  // tells its instances apart.
  function a2uiInstanceKey(def, scope) {
    const id = def && typeof def.id === 'string' ? def.id : '';
    return id + (scope && scope.rowKey != null ? '#' + scope.rowKey : '');
  }

  // A catalog enum value, or the fallback. Agent strings never reach CSS or
  // class names unless the catalog names them.
  function a2uiPick(value, allowed, fallback) {
    return allowed.indexOf(value) >= 0 ? value : fallback;
  }

  const A2UI_FLEX = {
    start: 'flex-start', center: 'center', end: 'flex-end', stretch: 'stretch',
    spaceAround: 'space-around', spaceBetween: 'space-between', spaceEvenly: 'space-evenly',
  };
  // CSS keyword for a catalog align/justify value; the catalog default when the
  // value is absent or not one of the catalog's.
  function a2uiFlex(value, fallback) {
    return Object.prototype.hasOwnProperty.call(A2UI_FLEX, value) ? A2UI_FLEX[value] : A2UI_FLEX[fallback];
  }

  function a2uiIconPath(name) {
    return Object.prototype.hasOwnProperty.call(A2UI_ICON_PATHS, name) ? A2UI_ICON_PATHS[name] : null;
  }

  // Media URL gate (guide, "Media URLs"): trim whitespace and control
  // characters, drop the tab/newline/CR a URL parser ignores, then accept only
  // http(s) written with `//` and a host. Returns '' for anything else. The
  // guide also allows data: and relative paths; the sidebar has no page base
  // and takes no inline media, so it stays http(s)-only.
  function a2uiMediaUrl(raw) {
    const url = String(raw == null ? '' : raw)
      .replace(/^[\s\x00-\x1f\x7f]+|[\s\x00-\x1f\x7f]+$/g, '')
      .replace(/[\t\n\r]/g, '');
    return /^https?:\/\/[^\s\/\\?#]/i.test(url) ? url : '';
  }

  // DateTimeInput: the model holds ISO 8601, native inputs hold local wall
  // time. date -> yyyy-MM-dd, time -> HH:mm, both -> yyyy-MM-ddTHH:mm.
  const A2UI_ISO_ZONE = /(?:Z|[+-]\d{2}:?\d{2})$/i;
  function a2uiIsoToInput(iso, enableDate, enableTime) {
    const s = String(iso == null ? '' : iso).trim();
    if (!s) return '';
    if (enableDate && !enableTime) {
      const m = /^(\d{4}-\d{2}-\d{2})/.exec(s);
      return m ? m[1] : '';
    }
    if (enableTime && !enableDate) {
      const m = /^(?:\d{4}-\d{2}-\d{2}T)?(\d{2}:\d{2})/.exec(s);
      return m ? m[1] : '';
    }
    if (/^\d{4}-\d{2}-\d{2}$/.test(s)) return s + 'T00:00';
    if (s.indexOf('T') > 0 && A2UI_ISO_ZONE.test(s)) {
      const d = new Date(s);
      if (isNaN(d.getTime())) return '';
      const p = n => (n < 10 ? '0' : '') + n;
      return d.getFullYear() + '-' + p(d.getMonth() + 1) + '-' + p(d.getDate())
        + 'T' + p(d.getHours()) + ':' + p(d.getMinutes());
    }
    const m = /^(\d{4}-\d{2}-\d{2})T(\d{2}:\d{2})/.exec(s);
    return m ? m[1] + 'T' + m[2] : '';
  }
  function a2uiInputToIso(value, enableDate, enableTime) {
    if (!value) return '';
    if (enableDate && !enableTime) return value;
    if (enableTime && !enableDate) return value.length === 5 ? value + ':00' : value;
    const d = new Date(value);
    return isNaN(d.getTime()) ? '' : d.toISOString().replace(/\.\d{3}Z$/, 'Z');
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
    // Preserve the decoded string exactly. Literal backslash-n/backslash-t
    // sequences may be meaningful source code or regular-expression input.
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
      // Keep the author's numbering: a list split by a blank line, or across
      // two Text components, would otherwise restart at 1 each time.
      const start = parseInt(m, 10);
      return '<ol' + (start > 1 ? ' start="' + start + '"' : '') + '>' + items.map(i => '<li>' + i + '</li>').join('') + '</ol>';
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
	// Two tests can check the same surface concurrently (e.g. the sidebar is
	// covered here and by sidebar_html's own gate); a per-invocation suffix
	// keeps one run's cleanup from deleting another run's script mid-flight.
	static CALL: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
	let call = CALL.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
	let dir = std::env::temp_dir().join(format!("octoweb-js-{}-{name}-{call}", std::process::id()));
	std::fs::create_dir_all(&dir).expect("temp dir");
	let path = dir.join(format!("{name}.js"));
	std::fs::write(&path, source).expect("write script");
	let out = match std::process::Command::new("node").args(flags).arg(&path).output() {
		Ok(out) => Some(out),
		Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
			// Skipping keeps `cargo test` usable on a machine without node, but
			// a skip in CI is a gate that passes without checking anything.
			assert!(
				std::env::var_os("OCTOWEB_REQUIRE_NODE").is_none(),
				"OCTOWEB_REQUIRE_NODE is set but node is not installed — \
                 the JS syntax gate would pass vacuously"
			);
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
