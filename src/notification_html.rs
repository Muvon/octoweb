/// Returns the HTML for a macOS Tahoe-style notification toast.
///
/// Slides down from the top-right when shown, stays until the user opens or dismisses it.
/// Glass background with separate primary and dismiss actions.
/// Light/dark adaptive via prefers-color-scheme.
///
/// JS API (called from Rust):
///   window.__show(preview, icon, title, autoDismissMs, options) — show a toast
///   window.__hide()         — dismiss immediately
///
/// IPC messages sent to Rust:
///   { type: "open_sidebar" }  — user clicked the notification
///   { type: "reveal_file", path: string }  — reveal a download in Finder
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
    top: 2px;
    right: 8px;
    transform: translateY(-120%);
    width: 340px;
    padding: 4px 8px 4px 12px;
    border-radius: var(--r-card);
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
    width: 24px;
    height: 24px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: var(--accent);
    flex-shrink: 0;
  }
  .app-icon svg { width: 24px; height: 24px; }

  .content {
    flex: 1;
    min-width: 0;
  }

  .title {
    font: 600 var(--fs-body)/1.3 var(--font-text);
    color: var(--label);
    margin-bottom: 1px;
  }

  .preview {
    font: 400 var(--fs-body)/1.35 var(--font-text);
    color: var(--label-2);
    overflow: hidden;
    text-overflow: ellipsis;
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 2;
    line-clamp: 2;
  }

  .actions {
    display: flex;
    align-items: center;
    gap: 3px;
    flex-shrink: 0;
  }

  .open-btn,
  .close-btn {
    min-width: var(--ctl-min);
    height: var(--ctl-min);
    border-radius: var(--r-capsule);
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

  .open-btn {
    min-width: 42px;
    padding: 0 9px;
    background: var(--accent);
    color: var(--on-accent);
    font: 500 var(--fs-body)/1 var(--font-text);
  }
  .open-btn[hidden] { display: none; }

  .close-btn { background: transparent; }

  .close-btn:hover {
    background: var(--fill-hover);
    color: var(--label);
  }

  .open-btn:hover { background: color-mix(in srgb, var(--accent) 88%, var(--label)); }

  .open-btn:active,
  .close-btn:active {
    transform: scale(0.96);
  }
  .close-btn:active { background: var(--fill-press); }

  .close-btn svg {
    width: 10px;
    height: 10px;
    stroke: currentColor;
    stroke-width: 1.5;
    fill: none;
  }

</style>
</head>
<body>
<div id="toast" class="glass-panel" role="status" aria-live="polite" tabindex="0">
  <div class="row">
    <span class="app-icon" id="icon">@@OCTOPUS_BRAND@@</span>
    <div class="content">
      <div class="title" id="title">Assistant</div>
      <div class="preview" id="preview">New message from AI assistant</div>
    </div>
    <div class="actions">
      <button class="open-btn" type="button">Open</button>
      <button class="close-btn" type="button" aria-label="Dismiss" title="Dismiss (Esc)">
        <svg viewBox="0 0 10 10">
          <path d="M2 2L8 8M8 2L2 8" />
        </svg>
      </button>
    </div>
  </div>
</div>
<script>
(function() {
  const toast = document.getElementById('toast');
  const preview = document.getElementById('preview');
  const iconEl = document.getElementById('icon');
  const titleEl = document.getElementById('title');
  const openBtn = document.querySelector('.open-btn');
  const closeBtn = document.querySelector('.close-btn');
  let dismissTimer = null;
  let currentMode = 'acp'; // 'acp' or 'download'
  let currentRevealPath = null;

  closeBtn.addEventListener('click', (e) => {
    e.stopPropagation();
    window.ipc.postMessage(JSON.stringify({ type: 'dismiss_notification' }));
    hide();
  });

  function activate() {
    if (currentMode === 'acp') {
      window.ipc.postMessage(JSON.stringify({ type: 'open_sidebar' }));
    } else if (currentRevealPath) {
      window.ipc.postMessage(JSON.stringify({ type: 'reveal_file', path: currentRevealPath }));
    } else {
      window.ipc.postMessage(JSON.stringify({ type: 'dismiss_notification' }));
    }
    hide();
  }

  openBtn.addEventListener('click', activate);

  toast.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') {
      e.preventDefault();
      window.ipc.postMessage(JSON.stringify({ type: 'dismiss_notification' }));
      hide();
    } else if (e.key === 'Enter' && e.target === toast) {
      e.preventDefault();
      activate();
    }
  });

  function hide() {
    if (dismissTimer) { clearTimeout(dismissTimer); dismissTimer = null; }
    toast.classList.remove('show');
    toast.classList.add('hide');
  }

  // icon/title/autoDismissMs/options are optional — defaults to ACP style.
  window.__show = function(text, icon, title, autoDismissMs, options) {
    if (!icon || icon === '\uD83D\uDC19') {
      iconEl.innerHTML = '@@OCTOPUS_BRAND@@';
    } else {
      iconEl.textContent = icon;
    }
    titleEl.textContent = title || 'Assistant';
    preview.textContent = text || 'New message from AI assistant';
    currentMode = (icon && icon !== '\uD83D\uDC19') ? 'download' : 'acp';
    currentRevealPath = options && typeof options.revealPath === 'string'
      ? options.revealPath
      : null;
    openBtn.textContent = currentMode === 'acp' ? 'Open' : 'Show in Finder';
    openBtn.hidden = currentMode !== 'acp' && !currentRevealPath;
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
        .replace("/*@@THEME@@*/", crate::theme::CSS)
        .replace("@@OCTOPUS_BRAND@@", crate::icons::OCTOPUS_BRAND)
}
