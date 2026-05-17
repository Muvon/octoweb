/// Reusable prompt-history JavaScript module.
///
/// Both the inline AI edit (⌘⇧E) and the AI sidebar assistant embed this.
/// Call `createPromptHistory(inputEl, ghostEl, placeholder, onResize)` to wire
/// up Ctrl+P/N navigation, Ctrl+R reverse search, and Ctrl+E ghost completion.
pub fn prompt_history_js() -> &'static str {
    r#"
function createPromptHistory(inputEl, ghostEl, defaultPlaceholder, onResize) {
  var history = [];
  var histIdx = -1;
  var savedDraft = '';
  var ghostText = '';
  var searchMode = false;
  var searchQuery = '';
  var searchIdx = -1;
  var searchMatch = '';

  function escHtml(s) {
    return s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;');
  }

  // ── Ghost text autocomplete ───────────────────────────────────────────

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
    var val = inputEl.value;
    if (!val || histIdx >= 0 || searchMode || inputEl.selectionStart !== val.length) {
      ghostText = '';
      ghostEl.innerHTML = '';
      return;
    }
    var match = findPrefixMatch(val);
    if (match) {
      ghostText = match;
      var suffix = match.substring(val.length);
      ghostEl.innerHTML = '<span style="visibility:hidden">' + escHtml(val) + '</span>' + escHtml(suffix);
    } else {
      ghostText = '';
      ghostEl.innerHTML = '';
    }
  }

  function clearGhost() {
    ghostText = '';
    ghostEl.innerHTML = '';
  }

  // ── Reverse incremental search (Ctrl+R) ───────────────────────────────

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
      savedDraft = inputEl.value;
      searchMode = true;
      searchQuery = '';
      searchIdx = 0;
    }
    updateSearchDisplay();
  }

  function exitSearchMode(restore) {
    searchMode = false;
    inputEl.placeholder = defaultPlaceholder;
    clearGhost();
    if (restore) {
      inputEl.value = savedDraft;
    } else {
      inputEl.value = searchMatch || searchQuery;
    }
    searchMatch = '';
    searchQuery = '';
    searchIdx = -1;
    onResize();
  }

  function updateSearchDisplay() {
    inputEl.placeholder = "(reverse-i-search)`" + searchQuery + "':";
    inputEl.value = searchQuery;
    if (!searchQuery) {
      searchMatch = '';
      ghostEl.innerHTML = '';
      onResize();
      return;
    }
    var idx = findReverseMatch(searchQuery, searchIdx);
    if (idx !== -1) {
      searchIdx = idx;
      searchMatch = history[idx];
      ghostEl.innerHTML = escHtml(searchMatch);
    } else {
      searchMatch = '';
      ghostEl.innerHTML = '';
    }
    onResize();
  }

  function cycleSearchNext() {
    if (!searchQuery) return;
    var idx = findReverseMatch(searchQuery, searchIdx + 1);
    if (idx !== -1) {
      searchIdx = idx;
      searchMatch = history[idx];
      ghostEl.innerHTML = escHtml(searchMatch);
      onResize();
    }
  }

  // ── Input event ───────────────────────────────────────────────────────

  inputEl.addEventListener('input', function() {
    if (searchMode) return;
    onResize();
    updateGhost();
  });

  // ── Keyboard handler ──────────────────────────────────────────────────
  // Returns true if the event was handled (caller should NOT process it).

  function handleKeydown(e) {
    // ── Search mode ─────────────────────────────────────────────────────
    if (searchMode) {
      if (e.key === 'Escape') {
        e.preventDefault();
        exitSearchMode(true);
        return true;
      }
      if (e.ctrlKey && e.key === 'r') {
        e.preventDefault();
        cycleSearchNext();
        return true;
      }
      if (e.key === 'Enter') {
        e.preventDefault();
        exitSearchMode(false);
        return true;
      }
      if (e.ctrlKey && e.key === 'e') {
        e.preventDefault();
        exitSearchMode(false);
        return true;
      }
      if (e.key === 'Backspace') {
        e.preventDefault();
        if (searchQuery.length > 0) {
          searchQuery = searchQuery.slice(0, -1);
          searchIdx = 0;
          updateSearchDisplay();
        } else {
          exitSearchMode(true);
        }
        return true;
      }
      if (e.key.length === 1 && !e.ctrlKey && !e.metaKey && !e.altKey) {
        e.preventDefault();
        searchQuery += e.key;
        searchIdx = 0;
        updateSearchDisplay();
        return true;
      }
      e.preventDefault();
      return true;
    }

    // ── Normal mode ─────────────────────────────────────────────────────
    if (e.ctrlKey && e.key === 'r') {
      if (history.length === 0) return false;
      e.preventDefault();
      enterSearchMode();
      return true;
    }
    if (e.ctrlKey && e.key === 'p') {
      if (history.length === 0) return false;
      e.preventDefault();
      if (histIdx === -1) {
        savedDraft = inputEl.value;
        histIdx = 0;
      } else if (histIdx < history.length - 1) {
        histIdx++;
      } else {
        return true;
      }
      inputEl.value = history[histIdx];
      clearGhost();
      onResize();
      return true;
    }
    if (e.ctrlKey && e.key === 'n') {
      if (histIdx === -1) return false;
      e.preventDefault();
      if (histIdx > 0) {
        histIdx--;
        inputEl.value = history[histIdx];
      } else {
        histIdx = -1;
        inputEl.value = savedDraft;
      }
      clearGhost();
      onResize();
      return true;
    }
    if (e.ctrlKey && e.key === 'e') {
      if (ghostText) {
        e.preventDefault();
        inputEl.value = ghostText;
        clearGhost();
        histIdx = -1;
        onResize();
        return true;
      }
      return false;
    }
    if (e.ctrlKey && e.key === 'u') {
      e.preventDefault();
      var pos = inputEl.selectionStart;
      inputEl.value = inputEl.value.substring(pos);
      inputEl.selectionStart = inputEl.selectionEnd = 0;
      clearGhost();
      onResize();
      return true;
    }
    return false;
  }

  inputEl.addEventListener('keydown', function(e) {
    handleKeydown(e);
  });

  // ── Public API ────────────────────────────────────────────────────────

  return {
    setHistory: function(h) {
      history = h || [];
      histIdx = -1;
      savedDraft = '';
      searchMode = false;
      searchQuery = '';
      searchIdx = -1;
      searchMatch = '';
      inputEl.placeholder = defaultPlaceholder;
      // If the user has already typed before history finished loading
      // (common race when Rust pushes setHistory after focus), recompute
      // the ghost immediately so autocomplete becomes visible without
      // waiting for another keystroke.
      if (inputEl.value) {
        updateGhost();
      } else {
        clearGhost();
      }
    },
    getHistory: function() {
      return history.slice();
    },
    resetState: function() {
      histIdx = -1;
      savedDraft = '';
      searchMode = false;
      searchQuery = '';
      searchIdx = -1;
      searchMatch = '';
      inputEl.placeholder = defaultPlaceholder;
      clearGhost();
    },
    isInSearchMode: function() { return searchMode; }
  };
}
"#
}
