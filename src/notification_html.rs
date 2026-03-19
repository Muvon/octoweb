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
pub fn html() -> &'static str {
    r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<style>
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
    border-radius: 14px;
    cursor: pointer;
    user-select: none;
    -webkit-app-region: no-drag;

    /* Glass — macOS Tahoe style */
    background: rgba(255, 255, 255, 0.88);
    backdrop-filter: blur(24px) saturate(180%);
    -webkit-backdrop-filter: blur(24px) saturate(180%);
    border: 0.5px solid rgba(0, 0, 0, 0.06);
    box-shadow:
      0 8px 32px rgba(0, 0, 0, 0.14),
      0 2px 8px rgba(0, 0, 0, 0.06);

    opacity: 0;
    transition: transform 0.35s cubic-bezier(0.34, 1.56, 0.64, 1),
                opacity 0.25s ease;
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

  @media (prefers-color-scheme: dark) {
    #toast {
      background: rgba(44, 44, 46, 0.92);
      border-color: rgba(255, 255, 255, 0.08);
      box-shadow:
        0 8px 32px rgba(0, 0, 0, 0.45),
        0 2px 8px rgba(0, 0, 0, 0.25);
    }
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
  }

  .title {
    font: 600 12px/1.3 -apple-system, BlinkMacSystemFont, sans-serif;
    color: rgba(0, 0, 0, 0.85);
    margin-bottom: 2px;
  }

  .preview {
    font: 400 12px/1.35 -apple-system, BlinkMacSystemFont, sans-serif;
    color: rgba(0, 0, 0, 0.55);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  @media (prefers-color-scheme: dark) {
    .title  { color: rgba(255, 255, 255, 0.92); }
    .preview { color: rgba(255, 255, 255, 0.50); }
  }

  /* ── Close button ─────────────────────────────────────────────────── */
  .close-btn {
    position: absolute;
    top: 6px;
    right: 6px;
    width: 20px;
    height: 20px;
    border-radius: 50%;
    background: rgba(0, 0, 0, 0.06);
    border: none;
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    opacity: 0;
    transition: opacity 0.15s ease, background 0.15s ease;
  }

  #toast:hover .close-btn {
    opacity: 1;
  }

  .close-btn:hover {
    background: rgba(0, 0, 0, 0.12);
  }

  .close-btn:active {
    background: rgba(0, 0, 0, 0.18);
  }

  @media (prefers-color-scheme: dark) {
    .close-btn {
      background: rgba(255, 255, 255, 0.08);
    }
    .close-btn:hover {
      background: rgba(255, 255, 255, 0.15);
    }
    .close-btn:active {
      background: rgba(255, 255, 255, 0.22);
    }
  }

  .close-btn svg {
    width: 10px;
    height: 10px;
    stroke: rgba(0, 0, 0, 0.5);
    stroke-width: 1.5;
    fill: none;
  }

  @media (prefers-color-scheme: dark) {
    .close-btn svg {
      stroke: rgba(255, 255, 255, 0.6);
    }
  }

  /* Hover feedback for toast */
  #toast:hover {
    background: rgba(255, 255, 255, 0.95);
  }
  @media (prefers-color-scheme: dark) {
    #toast:hover {
      background: rgba(54, 54, 56, 0.95);
    }
  }
</style>
</head>
<body>
<div id="toast">
  <button class="close-btn" title="Dismiss">
    <svg viewBox="0 0 10 10">
      <path d="M2 2L8 8M8 2L2 8" />
    </svg>
  </button>
  <div class="row" id="content">
    <span class="app-icon" id="icon">🐙</span>
    <div class="content">
      <div class="title" id="title">Octomind</div>
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
    titleEl.textContent = title || 'Octomind';
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
</html>"#
}
