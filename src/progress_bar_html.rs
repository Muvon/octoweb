/// Returns the HTML for a thin top-of-screen progress bar (Safari/Chrome style).
/// Shown during page load, animated fill then fade out.
/// Light/dark adaptive via prefers-color-scheme.
pub fn html() -> String {
    r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<style>
/*@@THEME@@*/
  * { box-sizing: border-box; margin: 0; padding: 0; }

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
    background: linear-gradient(90deg,
      color-mix(in srgb, var(--accent) 78%, transparent),
      var(--accent));
    border-radius: 0 var(--r-capsule) var(--r-capsule) 0;
    box-shadow: 0 0 5px color-mix(in srgb, var(--accent) 38%, transparent);
    transition: width var(--t-fast) var(--ease), opacity var(--t-pop) var(--ease);
  }

  #bar::after {
    content: '';
    position: absolute;
    top: 0;
    right: 0;
    width: 12px;
    height: 100%;
    border-radius: var(--r-capsule);
    background: color-mix(in srgb, var(--accent) 72%, white);
    box-shadow: 0 0 7px 1px color-mix(in srgb, var(--accent) 62%, transparent);
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
    bar.style.transition = 'width var(--t-pop) var(--spring)';
    bar.style.width = '70%';
  };

  window.__progress = function(pct) {
    bar.style.transition = 'width var(--t-fast) var(--ease)';
    bar.style.width = Math.min(95, pct) + '%';
  };

  window.__finish = function() {
    bar.style.transition = 'width var(--t-pop) var(--spring), opacity var(--t-pop) var(--ease) var(--t-fast)';
    bar.style.width = '100%';
    bar.classList.add('complete');
  };

  // Start immediately on load
  window.__start();
})();
</script>
</body>
</html>"#.replace("/*@@THEME@@*/", crate::theme::CSS)
}
