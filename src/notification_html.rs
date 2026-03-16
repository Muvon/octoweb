/// Returns the HTML for a macOS Tahoe-style notification toast.
///
/// Slides down from the top when shown, auto-dismisses after 5 seconds.
/// Glass background, rounded corners, click opens the AI sidebar.
/// Light/dark adaptive via prefers-color-scheme.
///
/// JS API (called from Rust):
///   window.__show(preview)  — show toast with message preview text
///   window.__hide()         — dismiss immediately
///
/// IPC messages sent to Rust:
///   { type: "open_sidebar" }  — user clicked the notification
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
    left: 50%;
    transform: translateX(-50%) translateY(-120%);
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
    transform: translateX(-50%) translateY(0);
    opacity: 1;
    pointer-events: auto;
  }

  #toast.hide {
    transform: translateX(-50%) translateY(-120%);
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

  /* Hover feedback */
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
  <div class="row">
    <span class="app-icon">🐙</span>
    <div class="content">
      <div class="title">Octomind</div>
      <div class="preview" id="preview">New message from AI assistant</div>
    </div>
  </div>
</div>
<script>
(function() {
  const toast = document.getElementById('toast');
  const preview = document.getElementById('preview');
  let timer = null;

  toast.addEventListener('click', () => {
    window.ipc.postMessage(JSON.stringify({ type: 'open_sidebar' }));
    hide();
  });

  function hide() {
    clearTimeout(timer);
    timer = null;
    toast.classList.remove('show');
    toast.classList.add('hide');
  }

  window.__show = function(text) {
    clearTimeout(timer);
    preview.textContent = text || 'New message from AI assistant';
    toast.classList.remove('hide');
    // Force reflow so transition fires from the start position
    void toast.offsetWidth;
    toast.classList.add('show');
    timer = setTimeout(hide, 5000);
  };

  window.__hide = function() {
    hide();
  };
})();
</script>
</body>
</html>"#
}
