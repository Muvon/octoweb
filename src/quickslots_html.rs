/// Static quick-slots footer bar — pinned at the bottom of the browser window.
///
/// Page content ends above this bar (not overlaid). Same treatment as the title bar.
/// Hover reveals ✕ to remove. Clicks open the slot URL.
///
/// IPC messages sent to Rust:
///   { type: "quickslot_open",   slot: 0-9 }
///   { type: "quickslot_remove", slot: 0-9 }
///   { type: "quickslot_save",   slot: 0-9 }  (click empty slot → save current page)
///
/// JS API called from Rust:
///   window.__updateSlots(jsonArray)  — refresh slot data
pub fn html() -> String {
    let template = r#"<!DOCTYPE html>
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
    font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Helvetica Neue", sans-serif;
  }

  :root {
    --bar-bg: rgba(255, 255, 255, 0.72);
    --bar-border: rgba(0, 0, 0, 0.06);
    --pill-bg: rgba(255, 255, 255, 0.60);
    --pill-hover: rgba(255, 255, 255, 0.90);
    --pill-border: rgba(0, 0, 0, 0.06);
    --text: rgba(0, 0, 0, 0.70);
    --text-dim: rgba(0, 0, 0, 0.35);
    --badge-bg: rgba(0, 0, 0, 0.06);
    --badge-text: rgba(0, 0, 0, 0.40);
    --close-bg: rgba(0, 0, 0, 0.08);
    --close-hover: rgba(255, 59, 48, 0.15);
    --close-color: rgba(0, 0, 0, 0.35);
    --close-hover-color: #ff3b30;
    --empty-bg: rgba(0, 0, 0, 0.03);
    --empty-border: rgba(0, 0, 0, 0.04);
  }

  @media (prefers-color-scheme: dark) {
    :root {
      --bar-bg: rgba(40, 40, 44, 0.72);
      --bar-border: rgba(255, 255, 255, 0.06);
      --pill-bg: rgba(255, 255, 255, 0.07);
      --pill-hover: rgba(255, 255, 255, 0.14);
      --pill-border: rgba(255, 255, 255, 0.06);
      --text: rgba(255, 255, 255, 0.75);
      --text-dim: rgba(255, 255, 255, 0.30);
      --badge-bg: rgba(255, 255, 255, 0.08);
      --badge-text: rgba(255, 255, 255, 0.40);
      --close-bg: rgba(255, 255, 255, 0.08);
      --close-hover: rgba(255, 69, 58, 0.20);
      --close-color: rgba(255, 255, 255, 0.35);
      --close-hover-color: #ff453a;
      --empty-bg: rgba(255, 255, 255, 0.03);
      --empty-border: rgba(255, 255, 255, 0.04);
    }
  }

  #bar {
    position: fixed;
    bottom: 0; left: 0; right: 0;
    height: 36px;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
    padding: 0 8px;
    background: var(--bar-bg);
    backdrop-filter: blur(20px) saturate(180%);
    -webkit-backdrop-filter: blur(20px) saturate(180%);
    border-top: 0.5px solid var(--bar-border);
    /* Round bottom corners to match macOS window corner radius (16pt logical).
       Prevents the glass background from painting outside the window frame. */
    border-radius: 0 0 16px 16px;
  }

  .slot {
    position: relative;
    display: flex;
    align-items: center;
    gap: 5px;
    height: 24px;
    padding: 0 8px 0 6px;
    border-radius: 12px;
    background: var(--pill-bg);
    box-shadow: 0 0 0 0.5px var(--pill-border), 0 1px 3px rgba(0, 0, 0, 0.04);
    cursor: pointer;
    transition: background 0.12s ease, box-shadow 0.15s ease,
                transform 0.2s cubic-bezier(0.34, 1.56, 0.64, 1);
    max-width: 140px;
    flex-shrink: 1;
    min-width: 0;
    user-select: none;
    -webkit-user-select: none;
  }

  .slot:hover {
    background: var(--pill-hover);
    box-shadow: 0 0 0 0.5px var(--pill-border), 0 2px 8px rgba(0, 0, 0, 0.08);
  }
  .slot:active { transform: scale(0.97); transition-duration: 0.06s; }

  .slot.empty {
    background: var(--empty-bg);
    box-shadow: none;
    border: 1px dashed var(--empty-border);
    cursor: pointer;
    transition: background 0.12s ease, border-color 0.12s ease, box-shadow 0.15s ease,
                transform 0.2s cubic-bezier(0.34, 1.56, 0.64, 1);
  }

  .slot.empty:hover {
    background: var(--pill-hover);
    border-color: transparent;
    box-shadow: 0 0 0 0.5px var(--pill-border), 0 2px 8px rgba(0, 0, 0, 0.08);
  }

  .slot.empty:active { transform: scale(0.97); }

  .slot .badge {
    flex-shrink: 0;
    width: 15px; height: 15px;
    border-radius: 5px;
    background: var(--badge-bg);
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 9px;
    font-weight: 600;
    color: var(--badge-text);
    font-variant-numeric: tabular-nums;
  }

  .slot .favicon {
    flex-shrink: 0;
    width: 12px; height: 12px;
    border-radius: 2px;
    object-fit: contain;
  }

  .slot .label {
    font-size: 11px;
    font-weight: 450;
    color: var(--text);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
    line-height: 1;
  }

  .slot.empty .label {
    color: var(--text-dim);
    font-weight: 400;
  }

  /* Empty-state plus glyph — quiet at rest, surfaces on hover */
  .slot.empty .plus {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 10px; height: 10px;
    opacity: 0.35;
    transition: opacity 0.12s ease;
    color: var(--text-dim);
  }
  .slot.empty .plus svg { width: 100%; height: 100%; }
  .slot.empty .hint {
    font-size: 10px;
    color: var(--text-dim);
    opacity: 0;
    max-width: 0;
    overflow: hidden;
    white-space: nowrap;
    transition: opacity 0.12s ease, max-width 0.16s ease;
    letter-spacing: 0.01em;
  }
  .slot.empty:hover .plus { opacity: 0; max-width: 0; width: 0; }
  .slot.empty:hover .hint { opacity: 1; max-width: 60px; }

  /* Close button — appears on hover, right side */
  .slot .close {
    position: absolute;
    right: 2px;
    top: 50%;
    transform: translateY(-50%);
    width: 14px; height: 14px;
    border-radius: 50%;
    background: var(--close-bg);
    display: flex;
    align-items: center;
    justify-content: center;
    opacity: 0;
    pointer-events: none;
    transition: opacity 0.1s ease, background 0.1s ease;
    cursor: pointer;
  }

  .slot:not(.empty):hover .close {
    opacity: 1;
    pointer-events: auto;
  }

  .slot .close:hover {
    background: var(--close-hover);
  }

  .slot .close svg {
    width: 8px; height: 8px;
    stroke: var(--close-color);
    stroke-width: 2;
    stroke-linecap: round;
  }

  .slot .close:hover svg {
    stroke: var(--close-hover-color);
  }
</style>
</head>
<body>
<div id="bar"></div>
<script>
(function() {
  const bar = document.getElementById('bar');
  let slots = [];

  function render() {
    bar.innerHTML = '';
    for (let i = 0; i < 10; i++) {
      const s = slots[i];
      const el = document.createElement('div');
      el.className = 'slot' + (s ? '' : ' empty');

      // Number badge: 1-9, then 0
      const badge = document.createElement('div');
      badge.className = 'badge';
      badge.textContent = (i + 1) % 10;
      el.appendChild(badge);

      if (s) {
        if (s.favicon) {
          const img = document.createElement('img');
          img.className = 'favicon';
          img.src = s.favicon;
          el.appendChild(img);
        }
        const lbl = document.createElement('span');
        lbl.className = 'label';
        // Show domain or short title
        try {
          const u = new URL(s.url);
          lbl.textContent = s.title || u.hostname.replace(/^www\./, '');
        } catch(_) {
          lbl.textContent = s.title || s.url;
        }
        lbl.title = s.title ? s.title + ' — ' + s.url : s.url;
        el.appendChild(lbl);

        // Close button
        const close = document.createElement('div');
        close.className = 'close';
        close.innerHTML = '<svg viewBox="0 0 8 8"><line x1="1" y1="1" x2="7" y2="7"/><line x1="7" y1="1" x2="1" y2="7"/></svg>';
        close.addEventListener('click', function(e) {
          e.stopPropagation();
          window.ipc.postMessage(JSON.stringify({ type: 'quickslot_remove', slot: i }));
        });
        el.appendChild(close);

        el.addEventListener('click', function() {
          window.ipc.postMessage(JSON.stringify({ type: 'quickslot_open', slot: i }));
        });
      } else {
        const plus = document.createElement('span');
        plus.className = 'plus';
        plus.innerHTML = '@@ICON_PLUS@@';
        el.appendChild(plus);

        const hint = document.createElement('span');
        hint.className = 'hint';
        hint.textContent = '⌘⇧' + ((i + 1) % 10);
        el.appendChild(hint);

        el.title = 'Save current page to slot ' + ((i + 1) % 10) + '  (⌘⇧' + ((i + 1) % 10) + ')';

        el.addEventListener('click', function() {
          window.ipc.postMessage(JSON.stringify({ type: 'quickslot_save', slot: i }));
        });
      }

      bar.appendChild(el);
    }
  }

  window.__updateSlots = function(data) {
    slots = data;
    render();
  };

  // Initial empty render
  render();
})();
</script>
</body>
</html>"#;
    template.replace("@@ICON_PLUS@@", crate::icons::PLUS)
}
