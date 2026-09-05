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
    height: 100%;
    opacity: 0;
    background: var(--accent);
    border-radius: 0 var(--r-capsule) var(--r-capsule) 0;
    box-shadow: 0 0 4px color-mix(in srgb, var(--accent) 32%, transparent);
    transition: width var(--t-fast) var(--ease), opacity var(--t-pop) var(--ease);
  }

  #bar.running { opacity: 1; }

  #bar.complete {
    width: 100%;
    opacity: 0;
  }

  @media (prefers-reduced-motion: reduce) {
    #bar { transition: none; }
  }
</style>
</head>
<body>
<div id="bar"></div>
<script>
(function() {
  const bar = document.getElementById('bar');
  const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)');
  let current = 0;
  let trickleTimer = null;

  function stopTrickle() {
    if (trickleTimer) {
      clearInterval(trickleTimer);
      trickleTimer = null;
    }
  }

  function setProgress(next) {
    current = Math.max(current, Math.min(95, Number(next) || 0));
    bar.style.width = current + '%';
  }

  window.__stop = function() {
    stopTrickle();
    current = 0;
    bar.style.transition = 'none';
    bar.style.width = '0%';
    bar.classList.remove('running', 'complete');
  };

  window.__start = function() {
    stopTrickle();
    current = 0;
    bar.style.transition = 'none';
    bar.style.width = '0%';
    bar.classList.remove('complete');
    bar.classList.add('running');
    void bar.offsetWidth;
    current = 8;
    bar.style.width = current + '%';
    if (reducedMotion.matches) return;
    bar.style.transition = 'width 200ms var(--ease)';
    trickleTimer = setInterval(function() {
      setProgress(current + (85 - current) * 0.12);
    }, 200);
  };

  window.__progress = function(pct) {
    if (reducedMotion.matches) return;
    bar.style.transition = 'width var(--t-fast) var(--ease)';
    setProgress(pct);
  };

  window.__complete = function() {
    stopTrickle();
    current = 100;
    bar.style.transition = reducedMotion.matches
      ? 'none'
      : 'width var(--t-pop) var(--spring), opacity var(--t-pop) var(--ease) var(--t-fast)';
    bar.style.width = '100%';
    bar.classList.remove('running');
    bar.classList.add('complete');
  };

  window.__finish = window.__complete;
})();
</script>
</body>
</html>"#
        .replace("/*@@THEME@@*/", crate::theme::CSS)
}
