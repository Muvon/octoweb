/// Returns the HTML for the find-in-page bar (⌘F).
/// Tahoe-style pill, positioned top-right, dark/light adaptive.
/// IPC messages: find_query, find_next, find_prev, find_close.
pub fn html() -> &'static str {
    r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<style>
  *, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }

  :root {
    --bg: rgba(244, 244, 244, 0.82);
    --border: rgba(0, 0, 0, 0.08);
    --text: rgba(0, 0, 0, 0.82);
    --placeholder: rgba(0, 0, 0, 0.32);
    --count: rgba(0, 0, 0, 0.42);
    --count-none: rgba(255, 59, 48, 0.8);
    --btn-hover: rgba(0, 0, 0, 0.06);
    --btn-active: rgba(0, 0, 0, 0.10);
    --icon: rgba(0, 0, 0, 0.50);
    --icon-disabled: rgba(0, 0, 0, 0.15);
    --sep: rgba(0, 0, 0, 0.08);
    --shadow: 0 1px 4px rgba(0, 0, 0, 0.06), 0 0 0 0.5px rgba(0, 0, 0, 0.05);
  }

  @media (prefers-color-scheme: dark) {
    :root {
      --bg: rgba(44, 44, 46, 0.82);
      --border: rgba(255, 255, 255, 0.08);
      --text: rgba(255, 255, 255, 0.82);
      --placeholder: rgba(255, 255, 255, 0.28);
      --count: rgba(255, 255, 255, 0.38);
      --count-none: rgba(255, 69, 58, 0.85);
      --btn-hover: rgba(255, 255, 255, 0.08);
      --btn-active: rgba(255, 255, 255, 0.14);
      --icon: rgba(255, 255, 255, 0.50);
      --icon-disabled: rgba(255, 255, 255, 0.15);
      --sep: rgba(255, 255, 255, 0.08);
      --shadow: 0 1px 4px rgba(0, 0, 0, 0.2), 0 0 0 0.5px rgba(255, 255, 255, 0.06);
    }
  }

  html, body {
    width: 100%; height: 100%;
    background: transparent;
    overflow: hidden;
    -webkit-font-smoothing: antialiased;
    font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Helvetica Neue", sans-serif;
    -webkit-user-select: none; user-select: none;
  }

  #bar {
    position: fixed;
    top: 4px; right: 6px;
    display: flex;
    align-items: center;
    gap: 1px;
    height: 30px;
    padding: 0 3px 0 10px;
    background: var(--bg);
    backdrop-filter: saturate(180%) blur(20px);
    -webkit-backdrop-filter: saturate(180%) blur(20px);
    border: 0.5px solid var(--border);
    border-radius: 10px;
    box-shadow: var(--shadow);
  }

  #input {
    width: 164px;
    height: 22px;
    border: none;
    outline: none;
    background: transparent;
    font-size: 12.5px;
    letter-spacing: -0.01em;
    color: var(--text);
    font-family: inherit;
  }
  #input::placeholder { color: var(--placeholder); }

  #count {
    font-size: 11px;
    font-variant-numeric: tabular-nums;
    color: var(--count);
    white-space: nowrap;
    min-width: 38px;
    text-align: center;
    padding: 0 3px;
    letter-spacing: -0.01em;
  }

  .sep {
    width: 0.5px;
    height: 16px;
    background: var(--sep);
    margin: 0 2px;
    flex-shrink: 0;
  }

  button {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 24px; height: 24px;
    border: none;
    background: transparent;
    border-radius: 6px;
    cursor: pointer;
    color: var(--icon);
    padding: 0;
    transition: background 0.1s;
  }
  button:hover { background: var(--btn-hover); }
  button:active { background: var(--btn-active); }
  button:disabled { color: var(--icon-disabled); cursor: default; }
  button:disabled:hover { background: transparent; }

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
<div id="bar">
  <input id="input" type="text" placeholder="Find on page" autocomplete="off" spellcheck="false">
  <span id="count"></span>
  <div class="sep"></div>
  <button id="prev" title="Previous (⇧Enter / ⌃P)" disabled>
    <svg viewBox="0 0 12 12"><polyline points="2,8 6,4 10,8"/></svg>
  </button>
  <button id="next" title="Next (Enter / ⌃N)" disabled>
    <svg viewBox="0 0 12 12"><polyline points="2,4 6,8 10,4"/></svg>
  </button>
  <div class="sep"></div>
  <button id="close" title="Close (Esc)">
    <svg viewBox="0 0 10 10"><line x1="2" y1="2" x2="8" y2="8"/><line x1="8" y1="2" x2="2" y2="8"/></svg>
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
</html>"#
}
