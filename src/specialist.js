  // Tap-run logs by run id. A log belongs to the `tap` step that started the
  // run: the step's row carries the run's role and status, its detail holds the
  // log, so a run never adds a message of its own to the chat.
  const tapRuns = new Map(); // run_id → { sid, run, view }

  function runFailed(run) {
    return run.status === 'failed' || run.error !== null;
  }

  function paintTapRow(row, run) {
    if (run) {
      const status = run.status + (!run.complete && run.error === null ? ' · recovering' : '');
      row.dataset.run = (run.role === null ? 'specialist' : run.role) + ' · ' + status;
    } else {
      delete row.dataset.run;
    }
    row.classList.toggle('run-failed', !!run && runFailed(run));
    row.classList.toggle('run-live', !!run && !runFailed(run) && run.status === 'running');
  }

  // Rows mount lazily (a steps group builds its list on first open), so each
  // row badges itself here and run updates repaint the rows already mounted.
  function bindTapRow(row, tool) {
    const id = tapRunId(tool.toolName || tool.title, tool.rawInput, tool.rawOutput);
    if (!id) return;
    row.dataset.tapRun = id;
    const entry = tapRuns.get(id);
    paintTapRow(row, entry ? entry.run : null);
  }

  function paintRunLog(view) {
    view.trimmed.textContent = view.run.truncated ? 'Earlier steps trimmed.' : '';
    view.error.textContent = view.run.error === null ? '' : view.run.error;
  }

  function paintRun(s, id) {
    const entry = tapRuns.get(id);
    for (const row of s.container.querySelectorAll('[data-tap-run]')) {
      if (row.dataset.tapRun === id) paintTapRow(row, entry ? entry.run : null);
    }
    if (entry && entry.view) paintRunLog(entry.view);
  }

  function runToolRow(view, entry) {
    const item = buildGroupItem(entry.tool, view.run.status === 'running');
    if (entry.node && entry.node.classList.contains('expanded')) {
      item.classList.add('expanded');
      fillToolDetail(item.querySelector('.tool-detail'), entry.tool);
    }
    if (entry.node) entry.node.replaceWith(item);
    else view.content.appendChild(item);
    entry.node = item;
  }

  function runText(entry) {
    if (entry.event.kind === 'assistant') entry.text.innerHTML = renderMd(entry.event.content);
    else entry.text.textContent = entry.event.content;
  }

  function appendRunEntry(view, event) {
    const entry = { event, node: null, result: null };
    view.entries.push(entry);
    switch (event.kind) {
      case 'prompt':
      case 'thinking': {
        entry.node = document.createElement('details');
        entry.node.className = 'specialist-report-input';
        const summary = document.createElement('summary');
        summary.textContent = event.kind === 'prompt' ? 'Input' : 'Thinking';
        entry.text = document.createElement('div');
        entry.text.className = 'specialist-report-input-text';
        entry.node.appendChild(summary);
        entry.node.appendChild(entry.text);
        runText(entry);
        break;
      }
      case 'assistant':
        entry.node = document.createElement('div');
        entry.node.className = 'specialist-report-content';
        entry.text = entry.node;
        runText(entry);
        break;
      case 'tool_use':
        entry.tool = { kind: 'other', title: event.tool, status: 'running', duration: null,
          rawInput: event.params, rawOutput: null, locations: [] };
        view.tools.set(event.tool_id, entry);
        runToolRow(view, entry);
        return;
      case 'tool_result': {
        const source = view.tools.get(event.tool_id);
        entry.tool = { kind: 'other', title: source ? source.tool.title : event.tool_id,
          status: event.success === true ? 'completed' : event.success === false ? 'failed' : 'unknown',
          duration: null, rawInput: source ? source.tool.rawInput : null, rawOutput: event.content, locations: [] };
        if (source) {
          source.result = entry;
          entry.node = source.node;
          source.node = null;
        }
        view.tools.set(event.tool_id, entry);
        runToolRow(view, entry);
        return;
      }
      case 'usage':
        if (view.usage) { view.usage.node.remove(); view.usage.node = null; }
        entry.node = document.createElement('div');
        entry.node.className = 'tool-detail-title';
        entry.node.textContent = '$' + event.cost.toFixed(4) + ' · ' + event.input_tokens + ' input · ' + event.output_tokens + ' output tokens';
        view.usage = entry;
        break;
      case 'status':
        if (event.error !== null) {
          entry.node = document.createElement('div');
          entry.node.className = 'tool-detail-title';
          entry.node.textContent = event.error;
        }
        if (event.status !== 'running') {
          for (const tool of view.tools.values()) {
            if (tool.tool.status === 'running') runToolRow(view, tool);
          }
        }
        break;
      default: throw new Error('Unknown specialist event: ' + event.kind);
    }
    if (entry.node) view.content.appendChild(entry.node);
  }

  function trimRunEntries(view, count) {
    for (const entry of view.entries.splice(0, count)) {
      if (entry.node) entry.node.remove();
      if (entry.result) {
        // A retained result must not keep a trimmed call's parameters alive.
        entry.result.tool.rawInput = null;
        entry.result.tool.title = entry.result.event.tool_id;
        runToolRow(view, entry.result);
      }
      if (view.tools.get(entry.event.tool_id) === entry) view.tools.delete(entry.event.tool_id);
      if (view.usage === entry) view.usage = null;
    }
  }

  // Full construction is reserved for opening the log or replaying a snapshot
  // into an open one; live deltas update only their own text segment or tool
  // row, preserving other DOM and disclosure state.
  function fillRunLog(entry, trimmed, content, error) {
    const view = { run: entry.run, trimmed, content, error, entries: [], tools: new Map(), usage: null };
    entry.view = view;
    entry.run.events.forEach(event => appendRunEntry(view, event));
    paintRunLog(view);
  }

  function mountTapRun(detail, tool) {
    const entry = tapRuns.get(tapRunId(tool.toolName || tool.title, tool.rawInput, tool.rawOutput));
    if (!entry) return;
    const title = document.createElement('div');
    title.className = 'tool-detail-title';
    title.textContent = 'Log';
    const trimmed = document.createElement('div');
    trimmed.className = 'tool-detail-title';
    const content = document.createElement('div');
    content.className = 'tap-run-log';
    const error = document.createElement('div');
    error.className = 'tool-detail-title';
    detail.appendChild(title); detail.appendChild(trimmed); detail.appendChild(content); detail.appendChild(error);
    fillRunLog(entry, trimmed, content, error);
  }

  function setRun(s, run) {
    const previous = tapRuns.get(run.run_id);
    const entry = { sid: s.sid, run, view: null };
    tapRuns.set(run.run_id, entry);
    if (previous && previous.view) {
      const { trimmed, content, error } = previous.view;
      content.replaceChildren();
      fillRunLog(entry, trimmed, content, error);
    }
    paintRun(s, run.run_id);
  }

  function forgetRuns(sid) {
    for (const [id, entry] of tapRuns) if (entry.sid === sid) tapRuns.delete(id);
  }

  window.__replayRuns = function(sid, runs) {
    const s = sessions.get(sid);
    if (!s) return;
    const retained = new Set(runs.map(run => run.run_id));
    for (const [id, entry] of tapRuns) {
      if (entry.sid === sid && !retained.has(id)) { tapRuns.delete(id); paintRun(s, id); }
    }
    runs.forEach(run => setRun(s, run));
  };

  window.__updateRun = function(sid, update, evicted) {
    const s = sessions.get(sid);
    if (!s) return;
    if (update.kind === 'snapshot') {
      setRun(s, update.run);
    } else {
      const id = update.kind === 'delta' ? update.delta.run_id : update.run_id;
      if (!tapRuns.has(id)) {
        setRun(s, { run_id: id, role: null, status: 'unknown', seq: 0,
          truncated: false, error: null, complete: false, events: [] });
      }
      const { run, view } = tapRuns.get(id);
      if (update.kind === 'delta') {
        const delta = update.delta;
        run.role = delta.role; run.status = delta.status; run.seq = delta.seq;
        run.truncated = delta.truncated; run.complete = true;
        if (delta.event !== null) {
          if (delta.merged) {
            run.events[run.events.length - 1].content += delta.event.content;
            if (view) runText(view.entries[view.entries.length - 1]);
          } else {
            run.events.push(delta.event);
            if (view) appendRunEntry(view, delta.event);
          }
        }
        if (view) trimRunEntries(view, delta.dropped);
        run.events.splice(0, delta.dropped);
      } else if (update.kind === 'recovering') {
        run.complete = false;
        run.error = null;
      } else if (update.kind === 'error') {
        run.error = update.error;
        run.complete = false;
      } else throw new Error('Unknown specialist update: ' + update.kind);
      paintRun(s, id);
    }
    for (const id of evicted) { tapRuns.delete(id); paintRun(s, id); }
  };
