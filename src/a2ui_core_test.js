
// ── test harness ───────────────────────────────────────────────────────
const opened = [];
globalThis.window = { ipc: { postMessage: m => opened.push(JSON.parse(m).url) } };
let failures = 0;
function eq(actual, expected, name) {
  const a = JSON.stringify(actual), b = JSON.stringify(expected);
  if (a !== b) { failures++; console.log('FAIL ' + name + ': got ' + a + ' want ' + b); }
}
const model = {
  now: '2025-12-15T12:00:00Z',
  user: { name: 'Ada', email: 'ada@example.com' },
  cart: { total: 1234.5, items: ['apple', 'pear'] },
  items: [{ name: 'Apple', quantity: 10 }, { name: 'Banana', quantity: 5 }],
  'a/b': 'slashed',
};
const S = { root: model, local: null };

// JSON-Pointer
eq(a2uiPtrGet(model, '/user/name'), 'Ada', 'ptrGet nested');
eq(a2uiPtrGet(model, '/a~1b'), 'slashed', 'ptrGet escaped slash');
eq(a2uiPtrGet(model, '/nope/deep'), undefined, 'ptrGet missing');
eq(a2uiPtrGet(model, '/'), model, 'ptrGet root');
{
  const m = { a: { b: 1 } };
  a2uiPtrSet(m, '/a/c', 2); eq(m.a.c, 2, 'ptrSet creates leaf');
  a2uiPtrSet(m, '/x/y', 3); eq(m.x.y, 3, 'ptrSet creates parents');
  a2uiPtrDelete(m, '/a/b'); eq('b' in m.a, false, 'ptrDelete removes key');
  const l = { arr: [1, 2, 3] };
  a2uiPtrDelete(l, '/arr/1'); eq(l.arr, [1, 3], 'ptrDelete splices arrays');
  eq(a2uiPtrGet(model, '/__proto__/x'), undefined, 'ptrGet ignores __proto__');
  eq(a2uiPtrGet(model, '/constructor/name'), undefined, 'ptrGet ignores constructor');
  const p = {};
  a2uiPtrSet(p, '/__proto__/polluted', 1);
  a2uiPtrSet(p, '/a/constructor/prototype/polluted', 1);
  eq(({}).polluted, undefined, 'ptrSet leaves Object.prototype alone');
  eq(p, {}, 'ptrSet with unsafe parts leaves model unchanged');
  a2uiPtrDelete(p, '/__proto__/toString');
  eq(typeof ({}).toString, 'function', 'ptrDelete leaves Object.prototype alone');
}

// truthy / ValidationResult
eq(a2uiTruthy(true), true, 'truthy bool');
eq(a2uiTruthy({ valid: false, message: 'x' }), false, 'truthy ValidationResult false');
eq(a2uiTruthy({ valid: true }), true, 'truthy ValidationResult true');
eq(a2uiTruthy(''), false, 'truthy empty string');
// DynamicBoolean coercion table
eq(a2uiTruthy(false), false, 'truthy bool false');
eq(a2uiTruthy('true'), true, 'truthy "true"');
eq(a2uiTruthy('TrUe'), true, 'truthy "true" in any case');
eq(a2uiTruthy('FALSE'), false, 'truthy "false" in any case');
eq(a2uiTruthy('hello'), false, 'truthy other string');
eq(a2uiTruthy(0), false, 'truthy number zero');
eq(a2uiTruthy(2), true, 'truthy other number');
eq(a2uiTruthy(-1), true, 'truthy negative number');
eq(a2uiTruthy(null), false, 'truthy null');
eq(a2uiTruthy(undefined), false, 'truthy absent');
eq(a2uiTruthy({ a: 1 }), false, 'truthy other object');
eq(a2uiTruthy([1]), false, 'truthy array');
eq(a2uiTruthy({ valid: 'yes' }), false, 'truthy object whose valid is not a boolean');

// validation functions
eq(A2UI_FN.required({ value: '' }), false, 'required empty');
eq(A2UI_FN.required({ value: [] }), false, 'required empty array');
eq(A2UI_FN.required({ value: 0 }), true, 'required zero is present');
eq(A2UI_FN.email({ value: 'ada@example.com' }), true, 'email ok');
eq(A2UI_FN.email({ value: 'nope' }), false, 'email bad');
eq(A2UI_FN.email({ value: '' }), false, 'email empty is invalid');
eq(A2UI_FN.regex({ value: '12345', pattern: '^[0-9]{5}$' }), true, 'regex ok');
eq(A2UI_FN.regex({ value: '1234', pattern: '^[0-9]{5}$' }), false, 'regex bad');
eq(a2uiDrainErrors(), [], 'no errors before the invalid-pattern cases');
eq(A2UI_FN.regex({ value: 'x', pattern: '[' }), false, 'regex invalid pattern');
eq(a2uiDrainErrors().map(e => e.code), ['EXPRESSION_ERROR'], 'regex invalid pattern is an EXPRESSION_ERROR');
eq(A2UI_FN.length({ value: 'abc', min: 2, max: 4 }), true, 'length in range');
eq(A2UI_FN.length({ value: 'a', min: 2 }), false, 'length too short');
eq(A2UI_FN.numeric({ value: 'zzz' }), true, 'numeric without bounds is vacuous');
eq(A2UI_FN.numeric({ value: 5, min: 1, max: 10 }), true, 'numeric in range');
eq(A2UI_FN.numeric({ value: 50, max: 10 }), false, 'numeric above max');
eq(A2UI_FN.and({ values: [true, { valid: true }] }), true, 'and over ValidationResults');
eq(A2UI_FN.and({ values: [true, false] }), false, 'and false');
eq(A2UI_FN.or({ values: [false, true] }), true, 'or true');
eq(A2UI_FN.not({ value: { valid: false } }), true, 'not ValidationResult');

// formatting
eq(A2UI_FN.formatDate({ value: '2025-12-15T12:00:00Z', format: 'EEEE, MMMM d' }), 'Monday, December 15', 'formatDate long');
eq(A2UI_FN.formatDate({ value: '2025-12-15T12:00:00Z', format: 'yyyy-MM-dd HH:mm' }), '2025-12-15 12:00', 'formatDate numeric');
eq(A2UI_FN.formatDate({ value: '2025-12-15T13:05:00Z', format: 'h:mm a' }), '1:05 PM', 'formatDate 12h');
eq(A2UI_FN.formatDate({ value: '', format: 'yyyy' }), '', 'formatDate empty');
eq(A2UI_FN.formatDate({ value: 'not a date', format: 'yyyy' }), '', 'formatDate unparseable');
eq(A2UI_FN.formatNumber({ value: 1234.5, decimals: 2 }), '1,234.50', 'formatNumber decimals');
eq(A2UI_FN.formatNumber({ value: 1234.5, grouping: false }), '1234.5', 'formatNumber no grouping');
eq(A2UI_FN.formatCurrency({ value: 1234.5, currency: 'USD' }), '$1,234.50', 'formatCurrency');
eq(A2UI_FN.pluralize({ value: 1, one: 'item', other: 'items' }), 'item', 'pluralize one');
eq(A2UI_FN.pluralize({ value: 3, one: 'item', other: 'items' }), 'items', 'pluralize other');
eq(A2UI_FN.pluralize({ value: 0, zero: 'nothing', other: 'items' }), 'nothing', 'pluralize zero');

// formatString interpolation
eq(a2uiInterpolate('Hello ${/user/name}!', S), 'Hello Ada!', 'interpolate path');
eq(a2uiInterpolate('no holes', S), 'no holes', 'interpolate literal');
eq(a2uiInterpolate('\\${/user/name}', S), '${/user/name}', 'interpolate escaped marker');
eq(a2uiInterpolate('Today is ${formatDate(value: ${/now}, format: \'EEEE, MMMM d\')}.', S),
   'Today is Monday, December 15.', 'interpolate nested function');
eq(a2uiInterpolate('${formatCurrency(value: /cart/total, currency: \'USD\')}', S), '$1,234.50', 'interpolate call with path arg');
eq(a2uiInterpolate('${/missing/key}', S), '', 'interpolate missing path');
eq(a2uiInterpolate('${unclosed', S), '${unclosed', 'interpolate unclosed renders literally');
eq(a2uiInterpolate('${formatNumber(value: 5, decimals: 1)}', S), '5.0', 'interpolate numeric literal arg');
eq(a2uiInterpolate('a${\'lit\'}b', S), 'alitb', 'interpolate string literal');

// ValueRef resolution
eq(a2uiResolveValue({ '@path': '/user/name' }, S), 'Ada', 'resolve path');
eq(a2uiResolveValue({ '@call': 'required', args: { value: { '@path': '/user/name' } } }, S), true, 'resolve nested call');
eq(a2uiResolveValue({ '@call': 'nosuchfn', args: {} }, S), undefined, 'resolve unknown fn');
eq(a2uiDrainErrors().map(e => e.code), ['EXPRESSION_ERROR'], 'unknown fn is an EXPRESSION_ERROR');
eq(a2uiResolveValue({ '@call': 'constructor', args: {} }, S), undefined, 'inherited object keys are not functions');
eq(a2uiDrainErrors().length, 1, 'inherited key reported as unknown');
eq(a2uiResolveValue('plain', S), 'plain', 'resolve literal');
eq(a2uiPathOf({ '@path': '/a' }), '/a', 'pathOf absolute');
eq(a2uiPathOf({ '@path': 'rel' }), null, 'pathOf relative is not writable');
eq(a2uiPathOf({ path: '/a' }), null, 'pathOf plain path is not a binding');
eq(a2uiResolveValue({ path: '/user/name' }, S), { path: '/user/name' }, 'plain {path} stays literal');
eq(a2uiResolveValue({ call: 'required', args: {} }, S), { call: 'required', args: {} }, 'plain {call} stays literal');
eq(a2uiResolveValue({ '@@type': 'x', n: { '@@a': 1 } }, S), { '@type': 'x', n: { '@a': 1 } }, '@@ key unescapes recursively');
eq(a2uiResolveValue({ '@bogus': 1 }, S), undefined, 'unknown @ key resolves to undefined');
eq(a2uiResolveValue({ '@': 1 }, S), undefined, 'bare @ key resolves to undefined');

// collection scope
{
  const rowScope = { root: model, local: model.items[1], index: 1 };
  eq(a2uiResolveValue({ '@path': 'name' }, rowScope), 'Banana', 'relative path in row scope');
  eq(a2uiResolveValue({ '@path': '/user/name' }, rowScope), 'Ada', 'absolute path still global in row scope');
  eq(a2uiResolveValue({ '@call': '@index', args: { offset: 1 } }, rowScope), 2, '@index with offset');
  eq(a2uiResolveValue({ '@call': '@index', args: {} }, rowScope), 1, '@index bare');
  eq(a2uiInterpolate('${@index(offset: 1)}. ${name}', rowScope), '2. Banana', 'interpolate in row scope');
  const scalarScope = { root: model, local: 'apple', index: 0 };
  eq(a2uiResolveValue({ '@path': '' }, scalarScope), 'apple', 'scalar row binds via the empty relative path');
  eq(a2uiResolveValue({ '@path': '/' }, scalarScope), model, '"/" is the data-model root inside a row');
  eq(a2uiResolveValue({ '@path': '.' }, scalarScope), undefined, '"." has no meaning');
  eq(a2uiResolveValue({ '@path': './name' }, rowScope), undefined, '"./x" has no meaning');
  eq(a2uiPathOf({ '@path': '' }, { root: model, local: 'apple', modelPath: '/cart/items/0' }), '/cart/items/0', 'pathOf empty path is the item');
  eq(a2uiPathOf({ '@path': '/' }, { root: model, local: 'apple', modelPath: '/cart/items/0' }), '/', 'pathOf "/" is the root');
  eq(a2uiPathOf({ '@path': 'name' }, { root: model, local: {}, modelPath: '/items/1' }), '/items/1/name', 'pathOf relative joins the item path');
  eq(a2uiDrainErrors().length, 0, 'path cases report no errors');

  // @index outside a template is an error, not 0.
  eq(a2uiResolveValue({ '@call': '@index', args: {} }, S), undefined, '@index outside a template');
  eq(a2uiDrainErrors().map(e => e.code), ['EXPRESSION_ERROR'], '@index outside a template is an EXPRESSION_ERROR');
}

// openUrl: activation-gated + scheme allowlist
eq(A2UI_FN.openUrl({ url: 'https://example.com' }), undefined, 'openUrl without activation returns void');
eq(a2uiDrainErrors().map(e => e.code), ['EXPRESSION_ERROR'], 'openUrl from a passive binding is an EXPRESSION_ERROR');
eq(opened.length, 0, 'openUrl blocked without user activation');
a2uiWithActivation(() => A2UI_FN.openUrl({ url: 'https://example.com' }));
eq(opened, ['https://example.com'], 'openUrl opens under activation');
a2uiWithActivation(() => A2UI_FN.openUrl({ url: 'javascript:alert(1)' }));
eq(opened.length, 1, 'openUrl rejects javascript: scheme');
a2uiWithActivation(() => A2UI_FN.openUrl({ url: 'data:text/html,x' }));
eq(opened.length, 1, 'openUrl rejects data: scheme');

// a function that throws: EXECUTION_ERROR from an action, EXPRESSION_ERROR from a binding
A2UI_FN.boom = () => { throw new Error('boom'); };
eq(a2uiResolveValue({ '@call': 'boom', args: {} }, S), undefined, 'throwing fn resolves to undefined');
eq(a2uiDrainErrors().map(e => e.code), ['EXPRESSION_ERROR'], 'throwing fn in a binding');
a2uiWithActivation(() => a2uiResolveValue({ '@call': 'boom', args: {} }, S));
eq(a2uiDrainErrors().map(e => e.code), ['EXECUTION_ERROR'], 'throwing fn in an action');
delete A2UI_FN.boom;

// the error list: distinct entries only, drained empty
a2uiReportError('EXPRESSION_ERROR', 'same');
a2uiReportError('EXPRESSION_ERROR', 'same');
a2uiReportError('EXECUTION_ERROR', 'same');
eq(a2uiDrainErrors().length, 2, 'error list keeps distinct entries');
eq(a2uiDrainErrors().length, 0, 'draining empties the list');

// regex dialect: accepted
const rx = (pattern, value) => A2UI_FN.regex({ pattern, value });
eq(rx('^[a-z]+@[a-z]+\\.com$', 'ada@example.com'), true, 'regex spec example 1');
eq(rx('^(?=.*\\d).{8,}$', 'abcdefg1'), true, 'regex spec example 2 (lookahead)');
eq(rx('^(?=.*\\d).{8,}$', 'abcdefgh'), false, 'regex lookahead fails');
eq(rx('\\w+\\s+\\w+', 'hello world'), true, 'regex spec example 3');
eq(rx('^(a|b)*c$', 'ababc'), true, 'regex bounded alternation repeat');
eq(rx('colou?r', 'my color'), true, 'regex search semantics');
eq(rx('^\\u{1F600}$', '\u{1F600}'), true, 'regex \\u{...} escape');
eq(rx('^\\x41\\u0042$', 'AB'), true, 'regex \\x and \\u escapes');
eq(rx('^a.c$', 'a\nc'), false, '"." excludes line terminators');
eq(rx('^abc$', 'abc\n'), false, '"$" does not match before a trailing newline');
eq(rx('^\\d+$', '٣'), false, '\\d is ASCII-only');
eq(rx('^[^\\d]+$', 'abc'), true, 'regex negated class');
eq(rx('^(?:ab){2,3}$', 'ababab'), true, 'regex counted group repeat');
eq(rx('^a{2}b*?$', 'aab'), true, 'regex lazy quantifier');
eq(rx('^a$', 'A'), false, 'regex is case-sensitive');
eq(rx('^a$', 'x'.repeat(5000)), false, 'regex value over 4096 code points does not match');
eq(a2uiDrainErrors().length, 0, 'accepted patterns report no errors');

// regex dialect: not allowed
for (const [pattern, why] of [
  ['(\\w)\\1', 'backreference'],
  ['\\k<n>', 'named backreference'],
  ['(?<=a)b', 'lookbehind'],
  ['(?<!a)b', 'negative lookbehind'],
  ['(?<name>a)', 'named group'],
  ['\\p{L}', 'property escape'],
  ['\\P{L}', 'negated property escape'],
  ['\\cA', 'control escape'],
  ['\\01', 'octal escape'],
  ['(?i)a', 'inline flag'],
  ['a**', 'doubled quantifier'],
  ['*a', 'nothing to repeat'],
  ['(a', 'unclosed group'],
  ['a)', 'unmatched paren'],
  ['[a', 'unclosed class'],
  ['[z-a]', 'reversed range'],
  ['\\q', 'unknown identity escape'],
  ['a{', 'dangling brace'],
  ['(?=a)*', 'quantified lookahead'],
  ['x'.repeat(257), 'longer than 256 code points'],
]) {
  eq(rx(pattern, 'x'), false, 'regex rejects ' + why);
  eq(a2uiDrainErrors().map(e => e.code), ['EXPRESSION_ERROR'], 'regex ' + why + ' is an EXPRESSION_ERROR');
}
eq(rx('x'.repeat(256), 'x'.repeat(256)), true, 'regex of exactly 256 code points is fine');
eq(a2uiDrainErrors().length, 0, '256-code-point pattern reports no error');

// regex safety check
for (const [pattern, why] of [
  ['(a+)+$', 'rule 2: repeat above 1 over an unbounded repeat'],
  ['(a*){2}', 'rule 2: counted repeat over an unbounded repeat'],
  ['(a|ab)*c', 'rule 3: alternation branches start alike'],
  ['(?:x|\\w)+', 'rule 3: class overlaps a literal branch'],
  ['\\d+\\s*\\d+', 'rule 4: overlapping repeats around an empty-capable item'],
  ['a*a*', 'rule 4: adjacent overlapping repeats'],
  ['(a+)b*(a+)', 'rule 4: groups of one repeated atom count as that atom'],
]) {
  eq(rx(pattern, 'aaaa'), false, 'regex unsafe: ' + why);
  eq(a2uiDrainErrors().map(e => e.code), ['EXPRESSION_ERROR'], 'regex unsafe is an EXPRESSION_ERROR: ' + why);
}
for (const [pattern, why] of [
  ['(a+)?', 'upper bound 1'],
  ['(a|b)*', 'disjoint branches'],
  ['a+b+', 'disjoint adjacent repeats'],
  ['\\d+\\.\\d+', 'a separator between repeats'],
  ['(\\d{1,3}){1,3}', 'counted bounds are bounded'],
]) {
  rx(pattern, 'aaaa');
  eq(a2uiDrainErrors().length, 0, 'regex safe: ' + why);
}

// check results: ValidationResult message wins, severity defaults to error
eq(a2uiCheckFailure({ condition: true, message: 'm' }, S), null, 'check passes on true');
eq(a2uiCheckFailure({ condition: false, message: 'rule' }, S), { message: 'rule', severity: 'error' }, 'false takes the rule message');
eq(a2uiCheckFailure({ condition: { '@path': '/vr' }, message: 'rule' }, { root: { vr: { valid: false, message: 'own', severity: 'warning' } }, local: null }),
  { message: 'own', severity: 'warning' }, 'ValidationResult message and severity win');
eq(a2uiCheckFailure({ condition: { '@path': '/vr' }, message: 'rule' }, { root: { vr: { valid: false } }, local: null }),
  { message: 'rule', severity: 'error' }, 'missing message falls back to the rule, severity to error');
eq(a2uiCheckFailure({ condition: { '@path': '/vr' } }, { root: { vr: { valid: true, severity: 'error' } }, local: null }), null, 'valid result passes');
eq(a2uiCheckFailure({ condition: { '@path': '/vr' }, message: 'r' }, { root: { vr: { valid: false, severity: 'info' } }, local: null }).severity,
  'info', 'info severity is kept');
eq(a2uiCheckFailure({ condition: { '@path': '/vr' }, message: 'r' }, { root: { vr: { valid: false, severity: 'bogus' } }, local: null }).severity,
  'error', 'unknown severity is an error');
eq(a2uiCheckFailure({ '@call': 'required', args: { value: 'x' }, message: 'old form' }, S), { message: 'old form', severity: 'error' }, 'a top-level call is not a condition');
eq(a2uiDrainErrors().length, 0, 'check cases report no errors');

// catalog enum mapping: only catalog values reach CSS
eq(a2uiFlex('spaceBetween', 'start'), 'space-between', 'flex maps a catalog value');
eq(a2uiFlex(undefined, 'start'), 'flex-start', 'flex absent takes the catalog default');
eq(a2uiFlex('red; x: y', 'stretch'), 'stretch', 'flex rejects a raw value');
eq(a2uiFlex('constructor', 'stretch'), 'stretch', 'flex rejects an inherited key');
eq(a2uiPick('header', ['icon', 'header'], 'icon'), 'header', 'pick keeps a catalog value');
eq(a2uiPick('x" onerror="', ['icon', 'header'], 'icon'), 'icon', 'pick drops a raw value');
eq(a2uiInstanceKey({ id: 't' }, null), 't', 'instance key outside a template');
eq(a2uiInstanceKey({ id: 't' }, { rowKey: '/rows/1' }), 't#/rows/1', 'instance key per template row');

// media URLs
eq(a2uiMediaUrl('  https://x.test/a.png\n'), 'https://x.test/a.png', 'media url is trimmed');
eq(a2uiMediaUrl('ht\ttps://x.test/a'), 'https://x.test/a', 'media url drops tabs inside the scheme');
eq(a2uiMediaUrl('https:x.test'), '', 'media url needs //');
eq(a2uiMediaUrl('https:///path'), '', 'media url needs a host');
eq(a2uiMediaUrl('javascript:alert(1)'), '', 'media url rejects javascript:');
eq(a2uiMediaUrl('//x.test/a'), '', 'media url rejects a protocol-relative url');
eq(a2uiMediaUrl('   '), '', 'media url rejects an empty result');

// icons: every catalog name has a path
eq(Object.keys(A2UI_ICON_PATHS).length, 59, 'icon path per catalog name');
eq(Object.values(A2UI_ICON_PATHS).every(d => /^[Mm]/.test(d)), true, 'icon paths are path data');
eq(a2uiIconPath('accountCircle') !== null, true, 'icon lookup finds a catalog name');
eq(a2uiIconPath('constructor'), null, 'icon lookup ignores inherited keys');

// DateTimeInput: ISO 8601 both ways
eq(a2uiIsoToInput('2025-12-15', true, false), '2025-12-15', 'iso date to input');
eq(a2uiIsoToInput('2025-12-15T09:30:00Z', true, false), '2025-12-15', 'iso datetime read as date');
eq(a2uiIsoToInput('09:30:00', false, true), '09:30', 'iso time to input');
eq(a2uiIsoToInput('2025-12-15T09:30:00', true, true), '2025-12-15T09:30', 'zoneless iso datetime to input');
eq(a2uiIsoToInput('2025-12-15', true, true), '2025-12-15T00:00', 'date-only iso in a datetime input');
eq(a2uiIsoToInput('nope', true, true), '', 'unparseable iso is empty');
eq(a2uiInputToIso('2025-12-15', true, false), '2025-12-15', 'input date to iso');
eq(a2uiInputToIso('09:30', false, true), '09:30:00', 'input time to iso');
eq(a2uiInputToIso('', true, true), '', 'empty input to iso');
{
  const local = a2uiIsoToInput('2025-12-15T14:30:00Z', true, true);
  eq(a2uiInputToIso(local, true, true), '2025-12-15T14:30:00Z', 'datetime round-trips through local time');
}

// toStr
eq(a2uiToStr(null), '', 'toStr null');
eq(a2uiToStr(['a', 'b']), 'a, b', 'toStr array');
eq(a2uiToStr({ text: 'hi' }), 'hi', 'toStr drills text');
eq(a2uiToStr('line\\nbreak'), 'line\\nbreak', 'toStr preserves literal backslash sequences');

// markdown
eq(a2uiRenderMarkdown('### Head').indexOf('<h3>Head</h3>') >= 0, true, 'markdown heading');
eq(a2uiRenderMarkdown('<img src=x onerror=1>').indexOf('<img') < 0, true, 'markdown escapes html');
eq(a2uiRenderMarkdown('**bold**').indexOf('<strong>bold</strong>') >= 0, true, 'markdown bold');
eq(a2uiRenderMarkdown('1. a\n\n2. b').indexOf('<ol start="2"><li>b</li></ol>') >= 0, true, 'markdown loose list keeps numbering');

if (failures) { console.log(failures + ' FAILURES'); process.exit(1); }
console.log('a2ui core: all assertions passed');
