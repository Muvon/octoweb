/// Returns the HTML for a thin top-of-screen progress bar (Safari/Chrome style).
/// Shown during page load, animated fill then fade out.
/// Light/dark adaptive via prefers-color-scheme.
pub fn html() -> &'static str {
    r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<style>
  * { box-sizing: border-box; margin: 0; padding: 0; }

  :root {
    --progress-bg: rgba(0, 122, 255, 0.15);
    --progress-fill: #007aff;
  }

  @media (prefers-color-scheme: dark) {
    :root {
      --progress-bg: rgba(10, 132, 255, 0.12);
      --progress-fill: #0a84ff;
    }
  }

  html, body {
    width: 100%;
    height: 100%;
    background: transparent;
    overflow: hidden;
  }

  #bar {
    position: fixed;
    top: 0;
    left: 0;
    width: 0%;
    height: 3px;
    background: var(--progress-fill);
    box-shadow: 0 0 8px var(--progress-fill);
    transition: width 0.15s ease-out, opacity 0.3s ease-out;
    border-radius: 16px 1px 1px 16px;
  }

  #bar.complete {
    width: 100%;
    opacity: 0;
  }
</style>
</head>
<body>
<div id="bar"></div>
<script>
(function() {
  const bar = document.getElementById('bar');

  // Auto-start on load
  window.__start = function() {
    bar.style.transition = 'none';
    bar.style.width = '0%';
    bar.style.opacity = '1';
    bar.classList.remove('complete');
    // Force reflow
    void bar.offsetWidth;
    // Animate to ~70% quickly, then slower
    bar.style.transition = 'width 0.4s cubic-bezier(0.4, 0, 0.2, 1)';
    bar.style.width = '70%';
  };

  window.__progress = function(pct) {
    bar.style.transition = 'width 0.15s ease-out';
    bar.style.width = Math.min(95, pct) + '%';
  };

  window.__finish = function() {
    bar.style.transition = 'width 0.2s ease-out, opacity 0.3s ease-out 0.1s';
    bar.style.width = '100%';
    bar.classList.add('complete');
  };

  // Start immediately on load
  window.__start();
})();
</script>
</body>
</html>"#
}
