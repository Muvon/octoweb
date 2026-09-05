/// Static quick-slots footer bar — pinned at the bottom of the browser window.
///
/// Page content ends above this bar (not overlaid). Same treatment as the title bar.
/// Hover or keyboard focus reveals a separate remove button. The primary button opens the URL.
///
/// IPC messages sent to Rust:
///   { type: "quickslot_open",   slot: 0-9 }
///   { type: "quickslot_remove", slot: 0-9 }
///   { type: "quickslot_save",   slot: 0-9 }  (click empty slot → save current page)
///
/// JS API called from Rust:
///   window.__updateSlots(jsonArray, activeUrl?)  — refresh slot data and current marker
///   window.__setActiveUrl(activeUrl?)            — refresh only the current marker
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
    overflow: hidden;
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
    height: 24px;
    border-radius: var(--r-capsule);
    background: var(--fill);
    box-shadow: 0 0 0 0.5px var(--hairline);
    transition: background var(--t-fast) var(--ease), color var(--t-fast) var(--ease),
                transform var(--t-fast) var(--ease);
    max-width: 180px;
    flex: 1 1 96px;
    min-width: 0;
    user-select: none;
    -webkit-user-select: none;
    overflow: hidden;
  }

  .slot:hover,
  .slot:focus-within {
    background: var(--fill-hover);
  }
  .slot:has(.slot-open:active),
  .slot.current {
    background: color-mix(in srgb, var(--accent) 15%, transparent);
  }
  .slot:has(.slot-open:active) { transform: scale(0.97); }

  button {
    font-family: var(--font-text);
  }

  .slot-open,
  .slot.empty {
    width: 100%;
    min-width: var(--ctl-min);
    height: 24px;
    border: none;
    background: transparent;
    color: var(--label);
    cursor: pointer;
  }

  .slot-open {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 0 28px 0 6px;
    border-radius: var(--r-capsule);
    overflow: hidden;
  }

  .slot.empty {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 5px;
    flex: 1 1 72px;
    max-width: 140px;
    min-width: var(--ctl-min);
    padding: 0 6px;
    border-radius: var(--r-capsule);
    background: transparent;
    box-shadow: none;
    border: 1px dashed var(--hairline);
    cursor: pointer;
    transition: background var(--t-fast) var(--ease), border-color var(--t-fast) var(--ease),
                transform var(--t-fast) var(--ease);
  }

  .slot.empty:hover,
  .slot.empty:focus-visible {
    background: var(--fill-hover);
    border-color: transparent;
    box-shadow: 0 0 0 0.5px var(--hairline);
  }

  .slot.empty:active { transform: scale(0.97); }

  .slot .badge,
  .slot.empty .badge {
    flex-shrink: 0;
    width: 17px; height: 17px;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    font-size: var(--fs-caption);
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

  .slot .label,
  .slot.empty .label {
    font-size: var(--fs-body);
    font-weight: 450;
    color: var(--label);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    min-width: 0;
    line-height: 1;
  }

  .slot.empty .label {
    color: var(--label-2);
    font-weight: 400;
    opacity: 0;
    max-width: 0;
    transition: opacity var(--t-fast) var(--ease), max-width var(--t-fast) var(--ease);
  }

  /* Empty-state plus glyph — quiet at rest, surfaces on hover */
  .slot.empty .plus {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 10px; height: 10px;
    opacity: 0.55;
    transition: opacity var(--t-fast) var(--ease);
    color: var(--label-2);
  }
  .slot.empty .plus svg { width: 100%; height: 100%; }
  .slot.empty:hover .plus,
  .slot.empty:focus-visible .plus { display: none; }
  .slot.empty:hover .label,
  .slot.empty:focus-visible .label { opacity: 1; max-width: 70px; }

  /* Close button — appears on hover, right side */
  .slot .close {
    position: absolute;
    right: 1px;
    top: 50%;
    transform: translateY(-50%);
    width: var(--ctl-min); height: var(--ctl-min);
    border-radius: var(--r-capsule);
    background: var(--fill);
    border: none;
    display: flex;
    align-items: center;
    justify-content: center;
    opacity: 0;
    pointer-events: none;
    transition: opacity var(--t-fast) var(--ease), background var(--t-fast) var(--ease), transform var(--t-fast) var(--ease);
    cursor: pointer;
  }

  .slot:hover .close,
  .slot:focus-within .close {
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

  @media (max-width: 999px) {
    #bar { justify-content: flex-start; }
    .slot,
    .slot.empty { min-width: 0; }
    .slot.empty.extra-empty { display: none; }
  }
</style>
</head>
<body>
<div id="bar"></div>
<script>
(function() {
  const bar = document.getElementById('bar');
  let slots = [];
  let activeUrl = '';

  function appendBadge(target, index) {
    const badge = document.createElement('span');
    badge.className = 'badge kbd';
    badge.textContent = (index + 1) % 10;
    target.appendChild(badge);
  }

  function render() {
    bar.replaceChildren();
    let foundEmpty = false;
    for (let i = 0; i < 10; i++) {
      const s = slots[i];

      if (s) {
        const el = document.createElement('div');
        el.className = 'slot' + (activeUrl && s.url === activeUrl ? ' current' : '');

        const open = document.createElement('button');
        open.type = 'button';
        open.className = 'slot-open';
        appendBadge(open, i);

        if (s.favicon) {
          const img = document.createElement('img');
          img.className = 'favicon';
          img.src = s.favicon;
          img.alt = '';
          open.appendChild(img);
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
        open.title = s.title ? s.title + ' — ' + s.url : s.url;
        open.appendChild(lbl);
        open.addEventListener('click', function() {
          window.ipc.postMessage(JSON.stringify({ type: 'quickslot_open', slot: i }));
        });
        el.appendChild(open);

        const close = document.createElement('button');
        close.type = 'button';
        close.className = 'close';
        close.setAttribute('aria-label', 'Remove slot ' + ((i + 1) % 10));
        close.title = 'Remove slot ' + ((i + 1) % 10);
        close.innerHTML = '<svg viewBox="0 0 8 8"><line x1="1" y1="1" x2="7" y2="7"/><line x1="7" y1="1" x2="1" y2="7"/></svg>';
        close.addEventListener('click', function() {
          window.ipc.postMessage(JSON.stringify({ type: 'quickslot_remove', slot: i }));
        });
        el.appendChild(close);
        bar.appendChild(el);
      } else {
        const el = document.createElement('button');
        el.type = 'button';
        el.className = 'slot empty' + (foundEmpty ? ' extra-empty' : '');
        foundEmpty = true;
        appendBadge(el, i);

        const plus = document.createElement('span');
        plus.className = 'plus';
        plus.innerHTML = '@@ICON_PLUS@@';
        el.appendChild(plus);

        const lbl = document.createElement('span');
        lbl.className = 'label';
        lbl.textContent = 'Save page';
        el.appendChild(lbl);

        el.title = 'Save current page (⌘⇧' + ((i + 1) % 10) + ')';

        el.addEventListener('click', function() {
          window.ipc.postMessage(JSON.stringify({ type: 'quickslot_save', slot: i }));
        });
        bar.appendChild(el);
      }
    }
  }

  window.__updateSlots = function(data, currentUrl) {
    slots = Array.isArray(data) ? data : [];
    activeUrl = typeof currentUrl === 'string' ? currentUrl : '';
    render();
  };

  window.__setActiveUrl = function(currentUrl) {
    activeUrl = typeof currentUrl === 'string' ? currentUrl : '';
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
