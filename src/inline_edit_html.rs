/// Returns the HTML for the inline AI edit modal (⌘⇧E).
/// Frosted-glass pill that floats at cursor position, auto-expands with input.
/// IPC messages: inline_edit_submit, inline_edit_close, inline_edit_resize.
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
    --btn-hover: rgba(0, 0, 0, 0.06);
    --btn-active: rgba(0, 0, 0, 0.10);
    --icon: rgba(0, 0, 0, 0.50);
    --sep: rgba(0, 0, 0, 0.08);
    --shadow: 0 2px 12px rgba(0, 0, 0, 0.10), 0 0 0 0.5px rgba(0, 0, 0, 0.06);
    --spinner: rgba(0, 0, 0, 0.35);
    --error: rgba(255, 59, 48, 0.8);
  }

  @media (prefers-color-scheme: dark) {
    :root {
      --bg: rgba(44, 44, 46, 0.82);
      --border: rgba(255, 255, 255, 0.08);
      --text: rgba(255, 255, 255, 0.82);
      --placeholder: rgba(255, 255, 255, 0.28);
      --btn-hover: rgba(255, 255, 255, 0.08);
      --btn-active: rgba(255, 255, 255, 0.14);
      --icon: rgba(255, 255, 255, 0.50);
      --sep: rgba(255, 255, 255, 0.08);
      --shadow: 0 2px 12px rgba(0, 0, 0, 0.25), 0 0 0 0.5px rgba(255, 255, 255, 0.06);
      --spinner: rgba(255, 255, 255, 0.35);
      --error: rgba(255, 69, 58, 0.85);
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
    top: 4px;
    left: 4px;
    right: 4px;
    display: flex;
    align-items: flex-start;
    gap: 1px;
    min-height: 30px;
    padding: 4px 3px 4px 10px;
    background: var(--bg);
    backdrop-filter: saturate(180%) blur(20px);
    -webkit-backdrop-filter: saturate(180%) blur(20px);
    border: 0.5px solid var(--border);
    border-radius: 10px;
    box-shadow: var(--shadow);
  }

  #input {
    flex: 1;
    min-height: 22px;
    max-height: 120px;
    border: none;
    outline: none;
    background: transparent;
    font-size: 12.5px;
    line-height: 18px;
    letter-spacing: -0.01em;
    color: var(--text);
    font-family: inherit;
    resize: none;
    overflow-y: auto;
    padding: 2px 0;
  }
  #input::placeholder { color: var(--placeholder); }
  #input:disabled { opacity: 0.5; }

  .controls {
    display: flex;
    align-items: center;
    gap: 1px;
    flex-shrink: 0;
    padding-top: 1px;
  }

  .sep {
    width: 0.5px;
    height: 16px;
    background: var(--sep);
    margin: 0 2px;
    flex-shrink: 0;
  }

  #spinner {
    display: none;
    width: 14px;
    height: 14px;
    border: 1.5px solid transparent;
    border-top-color: var(--spinner);
    border-radius: 50%;
    animation: spin 0.6s linear infinite;
    flex-shrink: 0;
    margin: 0 4px;
  }
  #spinner.active { display: block; }

  @keyframes spin { to { transform: rotate(360deg); } }

  #error-msg {
    display: none;
    font-size: 11px;
    color: var(--error);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 200px;
    padding: 0 4px;
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

  button svg {
    width: 9px; height: 9px;
    stroke: currentColor;
    fill: none;
    stroke-width: 2.2;
    stroke-linecap: round;
    stroke-linejoin: round;
  }
</style>
</head>
<body>
<div id="bar">
  <textarea id="input" rows="1" placeholder="How should I edit this?" autocomplete="off" spellcheck="false"></textarea>
  <div class="controls">
    <div id="spinner"></div>
    <span id="error-msg"></span>
    <div class="sep"></div>
    <button id="close" title="Close (Esc)">
      <svg viewBox="0 0 10 10"><line x1="2" y1="2" x2="8" y2="8"/><line x1="8" y1="2" x2="2" y2="8"/></svg>
    </button>
  </div>
</div>
<script>
(function() {
  var input = document.getElementById('input');
  var spinner = document.getElementById('spinner');
  var errorMsg = document.getElementById('error-msg');
  var bar = document.getElementById('bar');
  var lastH = 0;

  function ipc(msg) {
    window.ipc.postMessage(JSON.stringify(msg));
  }

  function autoResize() {
    input.style.height = '22px';
    input.style.height = Math.min(input.scrollHeight, 120) + 'px';
    // Notify Rust to resize the WebView when bar height changes
    var h = bar.offsetHeight + 8; // 8 = top padding
    if (h !== lastH) {
      lastH = h;
      ipc({ type: 'inline_edit_resize', height: h });
    }
  }

  input.addEventListener('input', autoResize);

  input.addEventListener('keydown', function(e) {
    if (e.key === 'Escape') {
      e.preventDefault();
      ipc({ type: 'inline_edit_close' });
    } else if (e.key === 'Enter' && !e.shiftKey && input.value.trim()) {
      e.preventDefault();
      ipc({ type: 'inline_edit_submit', prompt: input.value.trim() });
    }
  });

  document.getElementById('close').addEventListener('click', function() {
    ipc({ type: 'inline_edit_close' });
  });

  window.__focus = function() {
    lastH = 0;
    input.focus();
    input.select();
  };

  window.__clear = function() {
    input.value = '';
    input.style.height = '22px';
    input.disabled = false;
    spinner.classList.remove('active');
    errorMsg.style.display = 'none';
    lastH = 0;
  };

  window.__setProcessing = function(on) {
    input.disabled = on;
    spinner.classList.toggle('active', on);
    errorMsg.style.display = 'none';
  };

  window.__setError = function(msg) {
    spinner.classList.remove('active');
    input.disabled = false;
    errorMsg.textContent = msg;
    errorMsg.style.display = 'block';
    setTimeout(function() { errorMsg.style.display = 'none'; }, 4000);
  };
})();
</script>
</body>
</html>"#
}
