
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
}

// truthy / ValidationResult
eq(a2uiTruthy(true), true, 'truthy bool');
eq(a2uiTruthy({ valid: false, message: 'x' }), false, 'truthy ValidationResult false');
eq(a2uiTruthy({ valid: true }), true, 'truthy ValidationResult true');
eq(a2uiTruthy(''), false, 'truthy empty string');

// validation functions
eq(A2UI_FN.required({ value: '' }), false, 'required empty');
eq(A2UI_FN.required({ value: [] }), false, 'required empty array');
eq(A2UI_FN.required({ value: 0 }), true, 'required zero is present');
eq(A2UI_FN.email({ value: 'ada@example.com' }), true, 'email ok');
eq(A2UI_FN.email({ value: 'nope' }), false, 'email bad');
eq(A2UI_FN.email({ value: '' }), false, 'email empty is invalid');
eq(A2UI_FN.regex({ value: '12345', pattern: '^[0-9]{5}$' }), true, 'regex ok');
eq(A2UI_FN.regex({ value: '1234', pattern: '^[0-9]{5}$' }), false, 'regex bad');
eq(A2UI_FN.regex({ value: 'x', pattern: '[' }), false, 'regex invalid pattern');
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
eq(a2uiResolveValue({ path: '/user/name' }, S), 'Ada', 'resolve path');
eq(a2uiResolveValue({ call: 'required', args: { value: { path: '/user/name' } } }, S), true, 'resolve nested call');
eq(a2uiResolveValue({ call: 'nosuchfn', args: {} }, S), undefined, 'resolve unknown fn');
eq(a2uiResolveValue('plain', S), 'plain', 'resolve literal');
eq(a2uiPathOf({ path: '/a' }), '/a', 'pathOf absolute');
eq(a2uiPathOf({ path: 'rel' }), null, 'pathOf relative is not writable');

// collection scope
{
  const rowScope = { root: model, local: model.items[1], index: 1 };
  eq(a2uiResolveValue({ path: 'name' }, rowScope), 'Banana', 'relative path in row scope');
  eq(a2uiResolveValue({ path: '/user/name' }, rowScope), 'Ada', 'absolute path still global in row scope');
  eq(a2uiResolveValue({ call: '@index', args: { offset: 1 } }, rowScope), 2, '@index with offset');
  eq(a2uiResolveValue({ call: '@index', args: {} }, rowScope), 1, '@index bare');
  eq(a2uiInterpolate('${@index(offset: 1)}. ${name}', rowScope), '2. Banana', 'interpolate in row scope');
  const scalarScope = { root: model, local: 'apple', index: 0 };
  eq(a2uiResolveValue({ path: '/' }, scalarScope), 'apple', 'scalar row binds via "/"');
}

// openUrl: activation-gated + scheme allowlist
eq(A2UI_FN.openUrl({ url: 'https://example.com' }), undefined, 'openUrl without activation returns void');
eq(opened.length, 0, 'openUrl blocked without user activation');
a2uiWithActivation(() => A2UI_FN.openUrl({ url: 'https://example.com' }));
eq(opened, ['https://example.com'], 'openUrl opens under activation');
a2uiWithActivation(() => A2UI_FN.openUrl({ url: 'javascript:alert(1)' }));
eq(opened.length, 1, 'openUrl rejects javascript: scheme');
a2uiWithActivation(() => A2UI_FN.openUrl({ url: 'data:text/html,x' }));
eq(opened.length, 1, 'openUrl rejects data: scheme');

// toStr
eq(a2uiToStr(null), '', 'toStr null');
eq(a2uiToStr(['a', 'b']), 'a, b', 'toStr array');
eq(a2uiToStr({ text: 'hi' }), 'hi', 'toStr drills text');
eq(a2uiToStr('line\\nbreak'), 'line\\nbreak', 'toStr preserves literal backslash sequences');

// markdown
eq(a2uiRenderMarkdown('### Head').indexOf('<h3>Head</h3>') >= 0, true, 'markdown heading');
eq(a2uiRenderMarkdown('<img src=x onerror=1>').indexOf('<img') < 0, true, 'markdown escapes html');
eq(a2uiRenderMarkdown('**bold**').indexOf('<strong>bold</strong>') >= 0, true, 'markdown bold');

if (failures) { console.log(failures + ' FAILURES'); process.exit(1); }
console.log('a2ui core: all assertions passed');
