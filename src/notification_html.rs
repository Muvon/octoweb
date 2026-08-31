/// Returns the HTML for a macOS Tahoe-style notification toast.
///
/// Slides down from the top-right when shown, stays until user clicks or dismisses.
/// Glass background, rounded corners, click opens the AI sidebar.
/// Light/dark adaptive via prefers-color-scheme.
///
/// JS API (called from Rust):
///   window.__show(preview)  — show toast with message preview text
///   window.__hide()         — dismiss immediately
///
/// IPC messages sent to Rust:
///   { type: "open_sidebar" }  — user clicked the notification
///   { type: "dismiss_notification" }  — user clicked the X button
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
    overflow: hidden;
    background: transparent;
    -webkit-font-smoothing: antialiased;
  }

  /* ── Toast container ─────────────────────────────────────────────── */
  #toast {
    position: fixed;
    top: 8px;
    right: 8px;
    transform: translateY(-120%);
    width: 340px;
    padding: 10px 14px;
    border-radius: var(--r-card);
    cursor: pointer;
    user-select: none;

    opacity: 0;
    transition: transform var(--t-pop) var(--spring),
                opacity var(--t-pop) var(--ease),
                background var(--t-fast) var(--ease);
    pointer-events: none;
  }

  #toast.show {
    transform: translateY(0);
    opacity: 1;
    pointer-events: auto;
  }

  #toast.hide {
    transform: translateY(-120%);
    opacity: 0;
    pointer-events: none;
  }

  /* ── Layout ──────────────────────────────────────────────────────── */
  .row {
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .app-icon {
    font-size: 22px;
    line-height: 1;
    flex-shrink: 0;
  }

  .content {
    flex: 1;
    min-width: 0;
    padding-right: 38px;
  }

  .title {
    font: 600 12px/1.3 var(--font-text);
    color: var(--label);
    margin-bottom: 2px;
  }

  .preview {
    font: 400 12px/1.35 var(--font-text);
    color: var(--label-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  /* ── Close button ─────────────────────────────────────────────────── */
  .close-btn {
    position: absolute;
    top: 6px;
    right: 6px;
    min-width: 22px;
    height: 22px;
    border-radius: var(--r-capsule);
    background: var(--fill);
    border: none;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 3px;
    padding: 0 4px;
    color: var(--label-2);
    transition: background var(--t-fast) var(--ease), color var(--t-fast) var(--ease), transform var(--t-fast) var(--ease);
  }

  .close-btn:hover {
    background: var(--fill-hover);
    color: var(--label);
  }

  .close-btn:active {
    background: var(--fill-press);
    transform: scale(0.96);
  }

  .close-btn svg {
    width: 10px;
    height: 10px;
    stroke: currentColor;
    stroke-width: 1.5;
    fill: none;
  }

  /* Hover feedback for toast */
  #toast:hover {
    background: color-mix(in srgb, var(--glass-thick) 90%, var(--fill-hover));
  }
  #toast:active {
    background: color-mix(in srgb, var(--glass-thick) 86%, var(--fill-press));
  }
</style>
</head>
<body>
<div id="toast" class="glass-panel">
  <button class="close-btn" title="Dismiss (Esc)">
    <svg viewBox="0 0 10 10">
      <path d="M2 2L8 8M8 2L2 8" />
    </svg>
    <span class="kbd">esc</span>
  </button>
  <div class="row" id="content">
    <span class="app-icon" id="icon">🐙</span>
    <div class="content">
      <div class="title" id="title">Assistant</div>
      <div class="preview" id="preview">New message from AI assistant</div>
    </div>
  </div>
</div>
<script>
(function() {
  const toast = document.getElementById('toast');
  const preview = document.getElementById('preview');
  const iconEl = document.getElementById('icon');
  const titleEl = document.getElementById('title');
  const closeBtn = document.querySelector('.close-btn');
  const content = document.getElementById('content');
  let dismissTimer = null;
  let currentMode = 'acp'; // 'acp' or 'download'

  closeBtn.addEventListener('click', (e) => {
    e.stopPropagation();
    window.ipc.postMessage(JSON.stringify({ type: 'dismiss_notification' }));
    hide();
  });

  content.addEventListener('click', () => {
    if (currentMode === 'acp') {
      window.ipc.postMessage(JSON.stringify({ type: 'open_sidebar' }));
    } else {
      window.ipc.postMessage(JSON.stringify({ type: 'dismiss_notification' }));
    }
    hide();
  });

  function hide() {
    if (dismissTimer) { clearTimeout(dismissTimer); dismissTimer = null; }
    toast.classList.remove('show');
    toast.classList.add('hide');
  }

  // icon/title/autoDismissMs are optional — defaults to ACP style
  window.__show = function(text, icon, title, autoDismissMs) {
    iconEl.textContent = icon || '\uD83D\uDC19';
    titleEl.textContent = title || 'Assistant';
    preview.textContent = text || 'New message from AI assistant';
    currentMode = (icon && icon !== '\uD83D\uDC19') ? 'download' : 'acp';
    toast.classList.remove('hide');
    void toast.offsetWidth;
    toast.classList.add('show');
    if (dismissTimer) { clearTimeout(dismissTimer); dismissTimer = null; }
    if (autoDismissMs > 0) {
      dismissTimer = setTimeout(() => {
        window.ipc.postMessage(JSON.stringify({ type: 'dismiss_notification' }));
        hide();
      }, autoDismissMs);
    }
  };

  window.__hide = function() {
    hide();
  };
})();
</script>
</body>
</html>"#.replace("/*@@THEME@@*/", crate::theme::CSS)
}
