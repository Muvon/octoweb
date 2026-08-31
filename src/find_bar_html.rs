/// Returns the HTML for the find-in-page bar (⌘F).
/// Tahoe-style pill, positioned top-right, dark/light adaptive.
/// IPC messages: find_query, find_next, find_prev, find_close.
pub fn html() -> String {
    r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<style>
/*@@THEME@@*/
  *, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }

  html, body {
    width: 100%; height: 100%;
    background: transparent;
    overflow: hidden;
    -webkit-font-smoothing: antialiased;
    font-family: var(--font-text);
    -webkit-user-select: none; user-select: none;
  }

  #bar {
    position: fixed;
    top: 4px; right: 6px;
    display: flex;
    align-items: center;
    gap: 1px;
    height: 30px;
    padding: 0 3px 0 5px;
    border-radius: var(--r-card);
    animation: findIn var(--t-pop) var(--spring);
  }

  @keyframes findIn {
    from { opacity: 0; transform: translateY(-4px) scale(0.97); }
    to { opacity: 1; transform: none; }
  }

  #input {
    width: 116px;
    height: 24px;
    border: none;
    outline: none;
    background: var(--fill);
    border-radius: var(--r-capsule);
    font-size: 12.5px;
    letter-spacing: -0.01em;
    color: var(--label);
    font-family: inherit;
    padding: 0 9px;
    transition: background var(--t-fast) var(--ease);
  }
  #input:hover { background: var(--fill-hover); }
  #input:focus { background: var(--fill-press); }
  #input::placeholder { color: var(--label-3); }

  #count {
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    color: var(--label-2);
    white-space: nowrap;
    min-width: 32px;
    text-align: center;
    padding: 0 3px;
    letter-spacing: -0.01em;
  }

  .sep {
    width: 0.5px;
    height: 16px;
    background: var(--hairline);
    margin: 0 2px;
    flex-shrink: 0;
  }

  button {
    display: flex;
    align-items: center;
    justify-content: center;
    min-width: 24px; height: 24px;
    border: none;
    background: transparent;
    border-radius: var(--r-capsule);
    cursor: pointer;
    color: var(--label-2);
    padding: 0 4px;
    gap: 2px;
    transition: background var(--t-fast) var(--ease), color var(--t-fast) var(--ease), transform var(--t-fast) var(--ease);
  }
  button:hover { background: var(--fill-hover); color: var(--label); }
  button:active { background: var(--fill-press); transform: scale(0.96); }
  button:disabled { color: var(--label-4); cursor: default; }
  button:disabled:hover { background: transparent; }
  button:disabled .kbd { opacity: 0.45; }

  button svg {
    width: 11px; height: 11px;
    stroke: currentColor;
    fill: none;
    stroke-width: 1.8;
    stroke-linecap: round;
    stroke-linejoin: round;
  }

  #close svg {
    width: 9px; height: 9px;
    stroke-width: 2.2;
  }
</style>
</head>
<body>
<div id="bar" class="glass-panel">
  <input id="input" type="text" placeholder="Find on page" autocomplete="off" spellcheck="false">
  <span id="count"></span>
  <div class="sep"></div>
  <button id="prev" title="Previous (⇧Enter / ⌃P)" disabled>
    <svg viewBox="0 0 12 12"><polyline points="2,8 6,4 10,8"/></svg>
    <span class="kbd">⇧⏎</span>
  </button>
  <button id="next" title="Next (Enter / ⌃N)" disabled>
    <svg viewBox="0 0 12 12"><polyline points="2,4 6,8 10,4"/></svg>
    <span class="kbd">⏎</span>
  </button>
  <div class="sep"></div>
  <button id="close" title="Close (Esc)">
    <svg viewBox="0 0 10 10"><line x1="2" y1="2" x2="8" y2="8"/><line x1="8" y1="2" x2="2" y2="8"/></svg>
    <span class="kbd">esc</span>
  </button>
</div>
<script>
(function() {
  const input = document.getElementById('input');
  const count = document.getElementById('count');
  const prevBtn = document.getElementById('prev');
  const nextBtn = document.getElementById('next');

  let lastSent = '';

  function ipc(msg) {
    window.ipc.postMessage(JSON.stringify(msg));
  }

  input.addEventListener('input', function() {
    const v = input.value;
    if (v === lastSent) return;
    lastSent = v;
    ipc({ type: 'find_query', query: v });
  });


  input.addEventListener('keydown', function(e) {
    if (e.key === 'Escape') {
      e.preventDefault();
      ipc({ type: 'find_close' });
    } else if (e.key === 'Enter' && e.shiftKey) {
      e.preventDefault();
      ipc({ type: 'find_prev' });
    } else if (e.key === 'Enter') {
      e.preventDefault();
      ipc({ type: 'find_next' });
    }
  });

  prevBtn.addEventListener('click', function() { ipc({ type: 'find_prev' }); });
  nextBtn.addEventListener('click', function() { ipc({ type: 'find_next' }); });
  document.getElementById('close').addEventListener('click', function() { ipc({ type: 'find_close' }); });

  // Called from Rust to update match count display
  window.__setCount = function(current, total) {
    if (total > 0) {
      count.textContent = current + '/' + total;
      count.style.color = '';
    } else {
      count.textContent = '';
      count.style.color = '';
    }
    prevBtn.disabled = total === 0;
    nextBtn.disabled = total === 0;
  };

  // Called from Rust when find bar is shown — focus input and select all
  window.__focus = function() {
    input.focus();
    input.select();
  };

  // Called from Rust to clear state when hiding
  window.__clear = function() {
    input.value = '';
    lastSent = '';
    count.textContent = '';
    prevBtn.disabled = true;
    nextBtn.disabled = true;
  };
})();
</script>
</body>
</html>"#.replace("/*@@THEME@@*/", crate::theme::CSS)
}
