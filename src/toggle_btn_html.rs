/// Returns the HTML for the floating AI toggle button.
///
/// A small pill-shaped button pinned to the top-right of the browser content area.
/// Adapts to macOS light/dark mode. Sends IPC to toggle the sidebar.
///
/// IPC messages sent to Rust:
///   { type: "toggle_sidebar" }  — user clicked the button
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

  /* ── Button ─────────────────────────────────────────────────────────── */
  #btn {
    position: fixed;
    inset: 0;
    display: flex;
    align-items: center;
    justify-content: center;
    cursor: pointer;
    border: none;
    background: none;
    padding: 0;
    -webkit-app-region: no-drag;
  }

  .pill {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 36px;
    height: 36px;
    border-radius: 50%;
    /* Glass pill — adapts to light/dark */
    background: rgba(255, 255, 255, 0.82);
    backdrop-filter: blur(20px) saturate(180%);
    -webkit-backdrop-filter: blur(20px) saturate(180%);
    border: 1px solid rgba(0, 0, 0, 0.08);
    box-shadow:
      0 2px 12px rgba(0, 0, 0, 0.12),
      0 1px 3px rgba(0, 0, 0, 0.08);
    transition: transform 0.18s cubic-bezier(0.34, 1.56, 0.64, 1),
                box-shadow 0.15s ease,
                background 0.15s ease;
  }

  @media (prefers-color-scheme: dark) {
    .pill {
      background: rgba(58, 58, 60, 0.88);
      border-color: rgba(255, 255, 255, 0.10);
      box-shadow:
        0 2px 16px rgba(0, 0, 0, 0.40),
        0 1px 4px rgba(0, 0, 0, 0.25);
    }
  }

  #btn:hover .pill {
    transform: scale(1.10);
    box-shadow:
      0 4px 20px rgba(0, 0, 0, 0.18),
      0 1px 4px rgba(0, 0, 0, 0.10);
  }

  @media (prefers-color-scheme: dark) {
    #btn:hover .pill {
      box-shadow:
        0 4px 24px rgba(0, 0, 0, 0.55),
        0 1px 6px rgba(0, 0, 0, 0.30);
    }
  }

  #btn:active .pill {
    transform: scale(0.94);
    transition-duration: 0.08s;
  }

  /* Octopus emoji — crisp at all sizes */
  .icon {
    font-size: 18px;
    line-height: 1;
    user-select: none;
    /* Slight drop shadow so it pops on both light and dark */
    filter: drop-shadow(0 1px 2px rgba(0,0,0,0.18));
  }

  /* ── Unread badge dot ──────────────────────────────────────────────── */
  .badge {
    position: absolute;
    top: 2px;
    right: 2px;
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: #ff3b30;
    border: 1.5px solid rgba(255,255,255,0.9);
    box-shadow: 0 1px 4px rgba(255,59,48,0.45);
    opacity: 0;
    transform: scale(0);
    transition: opacity 0.2s ease, transform 0.25s cubic-bezier(0.34,1.56,0.64,1);
    pointer-events: none;
  }
  .badge.show {
    opacity: 1;
    transform: scale(1);
  }
  @media (prefers-color-scheme: dark) {
    .badge {
      border-color: rgba(0,0,0,0.5);
      box-shadow: 0 1px 6px rgba(255,59,48,0.55);
    }
  }
</style>
</head>
<body>
<button id="btn" title="Toggle octomind (?)">
  <div class="pill">
    <span class="icon">🐙</span>
    <span id="badge" class="badge"></span>
  </div>
</button>
<script>
  document.getElementById('btn').addEventListener('click', () => {
    window.ipc.postMessage(JSON.stringify({ type: 'toggle_sidebar' }));
  });
  // Called from Rust to show/hide the unread badge dot
  window.__setBadge = function(show) {
    document.getElementById('badge').classList.toggle('show', !!show);
  };
</script>
</body>
</html>"#
}
