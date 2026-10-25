const assert = require('node:assert/strict');
const vm = require('node:vm');

function specialistFixture(html, source) {
  class Element {
    constructor() {
      this.children = []; this.dataset = {}; this.className = ''; this.textContent = ''; this.innerHTML = '';
      this.open = false;
      this.classList = {
        contains: name => this.className.split(' ').includes(name),
        add: name => { if (!this.classList.contains(name)) this.className += ' ' + name; },
        toggle: (name, on = !this.classList.contains(name)) => {
          this.className = this.className.split(' ').filter(part => part !== name).join(' ');
          if (on) this.classList.add(name);
          return on;
        },
      };
    }
    get parentNode() { return this.parent || null; }
    appendChild(child) { this.children.push(child); child.parent = this; }
    remove() { this.parent.children.splice(this.parent.children.indexOf(this), 1); this.parent = null; }
    replaceChildren() { this.children = []; }
    replaceWith(child) {
      const index = this.parent.children.indexOf(this);
      this.parent.children[index] = child; child.parent = this.parent; this.parent = null;
    }
    addEventListener() {}
    querySelectorAll(selector) {
      const data = selector.match(/^\[data-([a-z-]+)\]$/);
      const key = data && data[1].replace(/-([a-z])/g, (_, c) => c.toUpperCase());
      const matches = [];
      for (const child of this.children) {
        if (data ? child.dataset[key] !== undefined : child.classList.contains(selector.slice(1))) matches.push(child);
        matches.push(...child.querySelectorAll(selector));
      }
      return matches;
    }
    querySelector(selector) { return this.querySelectorAll(selector)[0]; }
  }
  const escapeHtml = text => String(text).replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;');
  const parent = { currentAgentRaw: 'parent text', currentAgentBubble: {}, busy: true, isThinking: true,
    toolRows: { same: 'parent tool' }, toolDetails: [], msgQueue: ['next prompt'], runningTools: 1 };
  const s = { ...parent, sid: 1, container: new Element() };
  const context = { document: { createElement: () => new Element() }, window: {}, sessions: new Map([[1, s]]),
    escapeHtml, renderMd: escapeHtml,
    KIND_ICONS: {}, kindIcon: () => '', ICON_X_CIRCLE: 'fail', ICON_CHECK: 'check', fmtDuration: ms => String(ms),
  };
  function extract(start, end) {
    assert.notEqual(html.indexOf(start), -1); assert.notEqual(html.indexOf(end), -1);
    return html.slice(html.indexOf(start), html.indexOf(end, html.indexOf(start)));
  }
  vm.createContext(context);
  vm.runInContext(extract('  function toolDataObject(', '  const CHEVRON_SVG'), context);
  vm.runInContext(extract('  function buildGroupItem(', '  function tickActivity('), context);
  vm.runInContext(source, context);
  const run = { run_id: 'tap-general-abcdef', role: 'developer:<general>', status: 'running', seq: 1,
    truncated: false, error: null, complete: true, events: [{ kind: 'status', status: 'running', error: null }] };
  // The parent's `tap` step that started the run, as its steps group shows it.
  const tool = { kind: 'other', title: 'tap', toolName: 'tap', status: 'completed', duration: 5, locations: [],
    rawInput: JSON.stringify({ action: 'run', session: run.run_id, prompt: 'p' }), rawOutput: null };
  const item = context.buildGroupItem(tool);
  s.container.appendChild(item);
  const [row, detail] = item.children;
  context.window.__updateRun(1, { kind: 'snapshot', run }, []);
  context.toggleToolDetail(item, detail, tool);
  const tapRuns = vm.runInContext('tapRuns', context);
  const view = tapRuns.get(run.run_id).view;
  function delta(event, options = {}, evicted = []) {
    const update = { run_id: run.run_id, role: run.role, status: run.status, seq: run.seq + 1,
      truncated: run.truncated, event, merged: false, dropped: 0, ...options };
    context.window.__updateRun(1, { kind: 'delta', delta: update }, evicted);
  }
  return { s, parent, context, run, tool, row, detail, tapRuns, view, delta };
}

function checkSpecialist(html, source) {
  const { s, parent, context, run, tool, row, detail, tapRuns, view, delta } = specialistFixture(html, source);
  assert.equal(s.container.children.length, 1, 'a run adds no chat message of its own');
  assert.equal(row.dataset.tapRun, run.run_id);
  assert.equal(row.dataset.run, 'developer:<general> · running');
  assert.equal(row.classList.contains('run-live'), true);
  assert.equal(detail.children[0].textContent, 'Log');
  delta({ kind: 'prompt', content: '<input>' });
  delta({ kind: 'thinking', content: '<thought>' });
  delta({ kind: 'assistant', content: 'child <reply>' });
  delta({ kind: 'tool_use', tool: 'read', tool_id: 'same', params: { path: '<file>' } });
  assert.match(view.content.querySelector('.specialist-report-content').innerHTML, /child &lt;reply&gt;/);
  assert.equal(view.content.querySelector('.specialist-report-input-text').textContent, '<input>');
  assert.equal(view.content.querySelector('.tool-row').classList.contains('running'), true);
  const item = view.content.querySelector('.tool-item'); item.classList.add('expanded');
  delta({ kind: 'tool_result', tool_id: 'same', content: '<result>', success: null });
  delta({ kind: 'usage', cost: 0.1, input_tokens: 10, output_tokens: 20 });
  delta({ kind: 'usage', cost: 0.2, input_tokens: 30, output_tokens: 40 });
  assert.equal(view.content.querySelector('.tool-item').classList.contains('expanded'), true);
  assert.match(view.content.querySelector('.tool-detail').innerHTML, /&lt;result&gt;/);
  assert.doesNotMatch(view.content.querySelector('.tool-row').innerHTML, /tool-check|tool-fail/);
  assert.equal(view.content.children.filter(el => el.textContent.startsWith('$')).length, 1);
  assert.match(view.content.children.at(-1).textContent, /0.2000 · 30 input · 40 output/);
  delta({ kind: 'tool_use', tool: 'fail', tool_id: 'bad', params: {} });
  delta({ kind: 'tool_result', tool_id: 'bad', content: 'error', success: false });
  delta({ kind: 'status', status: 'failed', error: 'child failure' }, { status: 'failed' });
  context.window.__updateRun(1, { kind: 'error', run_id: run.run_id, error: 'Replay failed: unavailable' }, []);
  assert.match(view.content.querySelectorAll('.tool-row').at(-1).innerHTML, /tool-fail/);
  assert.equal(row.classList.contains('run-failed'), true);
  assert.equal(row.classList.contains('run-live'), false);
  assert.match(view.error.textContent, /Replay failed/);
  for (const [key, value] of Object.entries(parent)) assert.equal(s[key], value, 'parent ' + key + ' changed');
  // A step mounted after the run is known (a steps group opened later) badges itself.
  assert.match(context.buildGroupItem(tool).children[0].dataset.run, /failed/);
  // A replayed snapshot rebuilds an open log in place.
  const content = view.content;
  context.window.__updateRun(1, { kind: 'snapshot', run: { ...run, events: [{ kind: 'assistant', content: 'replayed' }] } }, []);
  assert.equal(tapRuns.get(run.run_id).view.content, content);
  assert.equal(content.children.length, 1);
  assert.equal(content.children[0].innerHTML, 'replayed');
  // Other steps never carry a log.
  const plain = { ...tool, title: 'read', toolName: 'read' };
  const [plainRow, plainDetail] = context.buildGroupItem(plain).children;
  context.toggleToolDetail(plainRow.parent, plainDetail, plain);
  assert.equal(plainRow.dataset.run, undefined);
  assert.equal(plainDetail.children.length, 0);
}

function checkSpecialistDeltas(html, source) {
  const { context, run, row, tapRuns, view, delta } = specialistFixture(html, source);
  delta({ kind: 'prompt', content: 'saved prompt' });
  const input = view.content.children[0]; input.open = true;
  delta({ kind: 'assistant', content: 'hello ' });
  const text = view.content.querySelector('.specialist-report-content');
  delta({ kind: 'assistant', content: 'world' }, { merged: true });
  assert.equal(view.content.querySelector('.specialist-report-content'), text);
  assert.equal(text.innerHTML, 'hello world');
  assert.equal(view.content.children[0], input); assert.equal(input.open, true);
  context.window.__updateRun(1, { kind: 'recovering', run_id: run.run_id }, []);
  assert.equal(view.content.querySelector('.specialist-report-content'), text);
  assert.equal(text.innerHTML, 'hello world'); assert.equal(run.complete, false);
  assert.match(row.dataset.run, /recovering/);

  delta({ kind: 'tool_use', tool: 'read', tool_id: 'call', params: { large: 'trim me' } });
  delta({ kind: 'tool_result', tool_id: 'call', content: 'retained result', success: true });
  const result = view.tools.get('call');
  delta({ kind: 'usage', cost: 1, input_tokens: 10, output_tokens: 20 }, { dropped: 4, truncated: true });
  assert.equal(run.events.length, 2); assert.equal(view.entries.length, 2);
  assert.equal(result.tool.rawInput, null); assert.equal(result.tool.title, 'call');
  assert.equal(view.content.querySelector('.specialist-report-content'), undefined);
  assert.match(view.trimmed.textContent, /Earlier steps trimmed/);
  assert.match(view.content.querySelector('.tool-row').innerHTML, /tool-check/);
  // Oversized chunks arrive as trim-only deltas, never as huge payloads.
  delta(null, { dropped: 2, truncated: true });
  assert.equal(run.events.length, 0); assert.equal(view.entries.length, 0);
  assert.equal(view.tools.size, 0); assert.equal(view.content.children.length, 0);
  assert.equal(view.usage, null);
  context.window.__updateRun(1, { kind: 'snapshot', run: { ...run, run_id: 'other', events: [] } }, []);
  delta({ kind: 'status', status: 'done', error: null }, { status: 'done' }, [run.run_id]);
  assert.equal(tapRuns.has(run.run_id), false);
  assert.equal(row.dataset.run, undefined, 'an evicted run leaves its step unbadged');
  assert.equal(tapRuns.has('other'), true);
  // Re-entering a workspace also drops runs evicted while its sidebar was hidden.
  context.window.__replayRuns(1, []);
  assert.equal(tapRuns.size, 0);
}
