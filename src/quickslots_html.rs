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
/*@@THEME@@*/
  *, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }

  html, body {
    width: 100%; height: 100%;
    overflow: hidden;
    background: transparent;
    -webkit-font-smoothing: antialiased;
    font-family: var(--font-text);
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
    background: var(--glass);
    backdrop-filter: var(--glass-blur);
    -webkit-backdrop-filter: var(--glass-blur);
    border-top: 0.5px solid var(--hairline);
    box-shadow: var(--glass-shine);
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
    border-radius: var(--r-capsule);
    background: var(--fill);
    box-shadow: 0 0 0 0.5px var(--hairline);
    cursor: pointer;
    transition: background var(--t-fast) var(--ease), color var(--t-fast) var(--ease),
                transform var(--t-fast) var(--ease);
    max-width: 140px;
    flex-shrink: 1;
    min-width: 0;
    user-select: none;
    -webkit-user-select: none;
  }

  .slot:hover {
    background: var(--fill-hover);
  }
  .slot:active,
  .slot.current {
    background: color-mix(in srgb, var(--accent) 15%, transparent);
  }
  .slot:active { transform: scale(0.97); }

  .slot.empty {
    background: transparent;
    box-shadow: none;
    border: 1px dashed var(--hairline);
    cursor: pointer;
    transition: background var(--t-fast) var(--ease), border-color var(--t-fast) var(--ease),
                transform var(--t-fast) var(--ease);
  }

  .slot.empty:hover {
    background: var(--fill-hover);
    border-color: transparent;
    box-shadow: 0 0 0 0.5px var(--hairline);
  }

  .slot.empty:active { transform: scale(0.97); }

  .slot .badge {
    flex-shrink: 0;
    width: 17px; height: 17px;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    font-size: 9px;
    font-weight: 600;
    color: var(--label-2);
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
    color: var(--label);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
    line-height: 1;
  }

  .slot.empty .label {
    color: var(--label-3);
    font-weight: 400;
  }

  /* Empty-state plus glyph — quiet at rest, surfaces on hover */
  .slot.empty .plus {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 10px; height: 10px;
    opacity: 0.35;
    transition: opacity var(--t-fast) var(--ease);
    color: var(--label-3);
  }
  .slot.empty .plus svg { width: 100%; height: 100%; }
  .slot.empty .hint {
    font-size: 10px;
    color: var(--label-3);
    opacity: 0;
    max-width: 0;
    overflow: hidden;
    white-space: nowrap;
    transition: opacity var(--t-fast) var(--ease), max-width var(--t-fast) var(--ease);
    letter-spacing: 0.01em;
  }
  .slot.empty:hover .plus { opacity: 0; max-width: 0; width: 0; }
  .slot.empty:hover .hint { opacity: 1; max-width: 60px; }

  /* Close button — appears on hover, right side */
  .slot .close {
    position: absolute;
    right: 1px;
    top: 50%;
    transform: translateY(-50%);
    width: 22px; height: 22px;
    border-radius: var(--r-capsule);
    background: var(--fill);
    display: flex;
    align-items: center;
    justify-content: center;
    opacity: 0;
    pointer-events: none;
    transition: opacity var(--t-fast) var(--ease), background var(--t-fast) var(--ease), transform var(--t-fast) var(--ease);
    cursor: pointer;
  }

  .slot:not(.empty):hover .close {
    opacity: 1;
    pointer-events: auto;
  }

  .slot .close:hover {
    background: color-mix(in srgb, var(--err) 16%, transparent);
  }
  .slot .close:active { background: color-mix(in srgb, var(--err) 24%, transparent); transform: translateY(-50%) scale(0.94); }

  .slot .close svg {
    width: 8px; height: 8px;
    stroke: var(--label-2);
    stroke-width: 2;
    stroke-linecap: round;
  }

  .slot .close:hover svg {
    stroke: var(--err);
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
      badge.className = 'badge kbd';
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
    template
        .replace("/*@@THEME@@*/", crate::theme::CSS)
        .replace("@@ICON_PLUS@@", crate::icons::PLUS)
}
