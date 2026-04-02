/// Returns the HTML for the inline AI edit modal (⌘⇧E).
/// Frosted-glass pill that floats at cursor position, auto-expands with input.
/// Features: prompt history (Ctrl+P/N), reverse search (Ctrl+R), ghost autocomplete (Ctrl+E).
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
    --ghost: rgba(0, 0, 0, 0.22);
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
      --ghost: rgba(255, 255, 255, 0.18);
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

  #ghost-wrap {
    position: relative;
    flex: 1;
    min-height: 22px;
  }

  #input {
    width: 100%;
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
    position: relative;
    z-index: 1;
  }
  #input::placeholder { color: var(--placeholder); }
  #input:disabled { opacity: 0.5; }

  #ghost {
    position: absolute;
    top: 0; left: 0; right: 0;
    min-height: 22px;
    font-size: 12.5px;
    line-height: 18px;
    letter-spacing: -0.01em;
    font-family: inherit;
    color: var(--ghost);
    pointer-events: none;
    padding: 2px 0;
    white-space: pre-wrap;
    word-wrap: break-word;
    overflow: hidden;
    z-index: 0;
  }

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
  <div id="ghost-wrap">
    <textarea id="input" rows="1" placeholder="How should I edit this?" autocomplete="off" spellcheck="false"></textarea>
    <div id="ghost" aria-hidden="true"></div>
  </div>
  <div class="controls">
    <div id="spinner"></div>
    <span id="error-msg"></span>
    <div class="sep"></div>
    <button id="hide" title="Hide (keep processing)" style="display:none">
      <svg viewBox="0 0 10 10"><polyline points="1,4 5,8 9,4"/></svg>
    </button>
    <button id="close" title="Cancel (Esc)">
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
  var hideBtn = document.getElementById('hide');
  var ghost = document.getElementById('ghost');
  var lastH = 0;

  // ── Prompt history state ────────────────────────────────────────────────
  var history = [];       // MRU-first, injected from Rust via __setHistory
  var histIdx = -1;       // -1 = fresh input, 0+ = navigating history
  var savedDraft = '';    // user text before entering history nav
  var ghostText = '';     // full autocomplete match text
  var searchMode = false; // Ctrl+R reverse-i-search active
  var searchQuery = '';   // current search query in reverse search
  var searchIdx = -1;     // start index for next Ctrl+R cycle
  var defaultPlaceholder = 'How should I edit this?';

  function ipc(msg) {
    window.ipc.postMessage(JSON.stringify(msg));
  }

  function escHtml(s) {
    return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
  }

  hideBtn.addEventListener('click', function() {
    ipc({ type: 'inline_edit_hide' });
  });

  function autoResize() {
    input.style.height = '22px';
    input.style.height = Math.min(input.scrollHeight, 120) + 'px';
    var h = bar.offsetHeight + 8;
    if (h !== lastH) {
      lastH = h;
      ipc({ type: 'inline_edit_resize', height: h });
    }
  }

  // ── Ghost text autocomplete ─────────────────────────────────────────────

  function findPrefixMatch(text) {
    if (!text) return '';
    var lower = text.toLowerCase();
    for (var i = 0; i < history.length; i++) {
      if (history[i].toLowerCase().indexOf(lower) === 0 && history[i].length > text.length) {
        return history[i];
      }
    }
    return '';
  }

  function updateGhost() {
    var val = input.value;
    // Only show ghost when typing fresh text, cursor at end, single line visible
    if (!val || histIdx >= 0 || searchMode || input.selectionStart !== val.length) {
      ghostText = '';
      ghost.innerHTML = '';
      return;
    }
    var match = findPrefixMatch(val);
    if (match) {
      ghostText = match;
      var suffix = match.substring(val.length);
      ghost.innerHTML = '<span style="visibility:hidden">' + escHtml(val) + '</span>' + escHtml(suffix);
    } else {
      ghostText = '';
      ghost.innerHTML = '';
    }
  }

  function clearGhost() {
    ghostText = '';
    ghost.innerHTML = '';
  }

  // ── Reverse incremental search (Ctrl+R) ─────────────────────────────────

  function findReverseMatch(query, startFrom) {
    if (!query) return -1;
    var lower = query.toLowerCase();
    for (var i = startFrom; i < history.length; i++) {
      if (history[i].toLowerCase().indexOf(lower) !== -1) {
        return i;
      }
    }
    return -1;
  }

  function enterSearchMode() {
    if (!searchMode) {
      savedDraft = input.value;
      searchMode = true;
      searchQuery = '';
      searchIdx = 0;
    }
    updateSearchDisplay();
  }

  function exitSearchMode(restore) {
    searchMode = false;
    input.placeholder = defaultPlaceholder;
    clearGhost();
    if (restore) {
      input.value = savedDraft;
    } else {
      // Accept: put matched entry into textarea (or keep query if no match)
      input.value = searchMatch || searchQuery;
    }
    searchMatch = '';
    searchQuery = '';
    searchIdx = -1;
    autoResize();
  }

  var searchMatch = ''; // the full matched history entry during search

  function updateSearchDisplay() {
    input.placeholder = "(reverse-i-search)`" + searchQuery + "':";
    input.value = searchQuery;
    if (!searchQuery) {
      searchMatch = '';
      ghost.innerHTML = '';
      autoResize();
      return;
    }
    var idx = findReverseMatch(searchQuery, searchIdx);
    if (idx !== -1) {
      searchIdx = idx;
      searchMatch = history[idx];
      // Show full match as ghost text — dimmed so it's visually distinct
      ghost.innerHTML = escHtml(searchMatch);
    } else {
      searchMatch = '';
      ghost.innerHTML = '';
    }
    autoResize();
  }

  function cycleSearchNext() {
    // Ctrl+R pressed again — find next older match
    if (!searchQuery) return;
    var idx = findReverseMatch(searchQuery, searchIdx + 1);
    if (idx !== -1) {
      searchIdx = idx;
      searchMatch = history[idx];
      ghost.innerHTML = escHtml(searchMatch);
      autoResize();
    }
    // If no more matches, stay on current
  }

  // ── Input event ─────────────────────────────────────────────────────────

  input.addEventListener('input', function() {
    if (searchMode) {
      // In search mode, the input event means user typed/deleted in the search query.
      // We intercept keydown for character input, so this handles edge cases.
      return;
    }
    autoResize();
    updateGhost();
  });

  // ── Keyboard handler ────────────────────────────────────────────────────

  input.addEventListener('keydown', function(e) {
    // ── Search mode key handling ──────────────────────────────────────────
    if (searchMode) {
      if (e.key === 'Escape') {
        e.preventDefault();
        exitSearchMode(true);
        return;
      }
      if (e.ctrlKey && e.key === 'r') {
        e.preventDefault();
        cycleSearchNext();
        return;
      }
      if (e.key === 'Enter') {
        // Accept match into input, exit search — don't submit yet
        e.preventDefault();
        exitSearchMode(false);
        return;
      }
      if (e.ctrlKey && e.key === 'e') {
        // Accept current search match and exit search mode
        e.preventDefault();
        exitSearchMode(false);
        return;
      }
      if (e.key === 'Backspace') {
        e.preventDefault();
        if (searchQuery.length > 0) {
          searchQuery = searchQuery.slice(0, -1);
          searchIdx = 0; // restart search from beginning
          updateSearchDisplay();
        } else {
          exitSearchMode(true);
        }
        return;
      }
      // Printable character — append to search query
      if (e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) {
        e.preventDefault();
        searchQuery += e.key;
        searchIdx = 0; // restart from top on new char
        updateSearchDisplay();
        return;
      }
      // Ignore other keys in search mode
      e.preventDefault();
      return;
    }

    // ── Normal mode key handling ──────────────────────────────────────────
    if (e.key === 'Escape') {
      e.preventDefault();
      ipc({ type: 'inline_edit_close' });
    } else if (e.key === 'Enter' && !e.shiftKey && input.value.trim()) {
      e.preventDefault();
      ipc({ type: 'inline_edit_submit', prompt: input.value.trim() });
    } else if (e.ctrlKey && e.key === 'r') {
      // Ctrl+R — enter reverse incremental search
      if (history.length === 0) return;
      e.preventDefault();
      enterSearchMode();
    } else if (e.ctrlKey && e.key === 'p') {
      // Ctrl+P — previous (older) history entry; fall through to native if no history
      if (history.length === 0) return;
      e.preventDefault();
      if (histIdx === -1) {
        savedDraft = input.value;
        histIdx = 0;
      } else if (histIdx < history.length - 1) {
        histIdx++;
      } else {
        return;
      }
      input.value = history[histIdx];
      clearGhost();
      autoResize();
    } else if (e.ctrlKey && e.key === 'n') {
      // Ctrl+N — next (newer) history entry; fall through to native if not navigating
      if (histIdx === -1) return;
      e.preventDefault();
      if (histIdx > 0) {
        histIdx--;
        input.value = history[histIdx];
      } else {
        histIdx = -1;
        input.value = savedDraft;
      }
      clearGhost();
      autoResize();
    } else if (e.ctrlKey && e.key === 'e') {
      // Ctrl+E — accept ghost text, or fall through to native end-of-line
      if (ghostText) {
        e.preventDefault();
        input.value = ghostText;
        clearGhost();
        histIdx = -1;
        autoResize();
      }
      // No ghost text: let native Ctrl+E (move to end of line) work
    } else if (e.ctrlKey && e.key === 'u') {
      // Ctrl+U — erase from cursor to start of line
      e.preventDefault();
      var pos = input.selectionStart;
      input.value = input.value.substring(pos);
      input.selectionStart = input.selectionEnd = 0;
      clearGhost();
      autoResize();
    }
  });

  document.getElementById('close').addEventListener('click', function() {
    ipc({ type: 'inline_edit_close' });
  });

  // ── API called from Rust ────────────────────────────────────────────────

  window.__focus = function() {
    lastH = 0;
    histIdx = -1;
    savedDraft = '';
    searchMode = false;
    searchQuery = '';
    searchIdx = -1;
    searchMatch = '';
    clearGhost();
    input.placeholder = defaultPlaceholder;
    input.focus();
    input.select();
  };

  window.__clear = function() {
    input.value = '';
    input.style.height = '22px';
    input.disabled = false;
    spinner.classList.remove('active');
    hideBtn.style.display = 'none';
    errorMsg.style.display = 'none';
    lastH = 0;
    histIdx = -1;
    savedDraft = '';
    searchMode = false;
    searchQuery = '';
    searchIdx = -1;
    searchMatch = '';
    clearGhost();
    input.placeholder = defaultPlaceholder;
  };

  window.__setProcessing = function(on) {
    input.disabled = on;
    spinner.classList.toggle('active', on);
    hideBtn.style.display = on ? '' : 'none';
    errorMsg.style.display = 'none';
  };

  window.__setError = function(msg) {
    spinner.classList.remove('active');
    input.disabled = false;
    errorMsg.textContent = msg;
    errorMsg.style.display = 'block';
    setTimeout(function() { errorMsg.style.display = 'none'; }, 4000);
  };

  window.__setHistory = function(h) {
    history = h || [];
    histIdx = -1;
    savedDraft = '';
    searchMode = false;
    searchQuery = '';
    searchIdx = -1;
    searchMatch = '';
    clearGhost();
  };
})();
</script>
</body>
</html>"#
}
