/// Styled new-tab page shown instead of a blank about:blank.
///
/// Displays the octopus branding, address/search field, and quick-slot cards.
///
/// IPC messages sent to Rust:
///   { type: "newtab_navigate", url: string }
///   { type: "quickslot_open", slot: 0-9 }
///   { type: "quickslot_save_url", slot: 0-9, url: string }
///   { type: "quickslot_remove", slot: 0-9 }
pub fn html(slots_json: &str) -> String {
    let keybindings_json = crate::keybindings::Keymap::load().ui_json().to_string();
    let safe_slots_json = slots_json
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026");
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>New Tab</title>
<style>/*@@THEME@@*/
{css}</style>
</head>
<body>
<main class="container">
  <div class="octopus" aria-hidden="true">@@OCTOPUS_BRAND@@</div>
  <h1>Welcome to Octoweb</h1>
  <div class="address-wrap">
    <input id="address" type="text" placeholder="Search or enter address" aria-label="Search or enter address" autocomplete="off" autocapitalize="off" spellcheck="false">
  </div>
  <p class="subtitle">Press <kbd class="kbd" id="palette-shortcut"></kbd> to navigate anywhere</p>
  <div class="slots" id="slots"></div>
  <p class="hint">
    <span><kbd class="kbd">⌘1</kbd>–<kbd class="kbd">⌘0</kbd> open slots</span>
    <span><kbd class="kbd">⌘⇧1</kbd>–<kbd class="kbd">⌘⇧0</kbd> save current page</span>
  </p>
</main>
<script>
(function() {{
  let slots = {slots_json};
  const container = document.getElementById('slots');
  const address = document.getElementById('address');
  const defaultPlaceholder = 'Search or enter address';
  let saveSlot = null;

  function ipc(message) {{
    window.ipc.postMessage(JSON.stringify(message));
  }}

  function focusAddressForSlot(slot) {{
    saveSlot = slot;
    address.value = '';
    address.placeholder = 'Enter an address to save it to slot ' + ((slot + 1) % 10);
    address.focus();
  }}

  function resetAddress() {{
    saveSlot = null;
    address.value = '';
    address.placeholder = defaultPlaceholder;
  }}

  window.__slotSaved = function() {{
    resetAddress();
    address.focus();
  }};

  address.addEventListener('keydown', function(e) {{
    if (e.key === 'Escape') {{
      e.preventDefault();
      resetAddress();
      return;
    }}
    if (e.key !== 'Enter' || e.isComposing || e.keyCode === 229) return;
    const raw = address.value.trim();
    if (!raw) return;
    e.preventDefault();
    if (saveSlot === null) {{
      ipc({{ type: 'newtab_navigate', url: raw }});
    }} else {{
      ipc({{ type: 'quickslot_save_url', slot: saveSlot, url: raw }});
    }}
  }});

  function renderSlots() {{
    container.replaceChildren();
    for (let i = 0; i < 10; i++) {{
      const s = slots[i];
      const wrap = document.createElement('div');
      wrap.className = 'slot-wrap' + (s ? '' : ' slot-empty');

      const card = document.createElement('button');
      card.type = 'button';
      card.className = 'slot-card';
      if (!s) card.setAttribute('aria-label', 'Set slot ' + ((i + 1) % 10));
      card.addEventListener('click', function() {{
        if (s) {{
          ipc({{ type: 'quickslot_open', slot: i }});
        }} else {{
          focusAddressForSlot(i);
        }}
      }});

      const badge = document.createElement('span');
      badge.className = 'card-badge kbd';
      badge.textContent = (i + 1) % 10;
      card.appendChild(badge);

      if (s) {{
        if (s.favicon) {{
          const img = document.createElement('img');
          img.className = 'card-favicon';
          img.src = s.favicon;
          img.alt = '';
          card.appendChild(img);
        }}

        const info = document.createElement('span');
        info.className = 'card-info';
        const title = document.createElement('span');
        title.className = 'card-title';
        const host = document.createElement('span');
        host.className = 'card-url';
        try {{
          const u = new URL(s.url);
          const hostname = u.hostname.replace(/^www\./, '');
          title.textContent = s.title || hostname;
          host.textContent = hostname;
        }} catch (_) {{
          title.textContent = s.title || s.url;
          host.textContent = s.url;
        }}
        card.setAttribute('aria-label', 'Open slot ' + ((i + 1) % 10) + ': ' + title.textContent);
        info.appendChild(title);
        info.appendChild(host);
        card.appendChild(info);

        const remove = document.createElement('button');
        remove.type = 'button';
        remove.className = 'slot-remove';
        remove.setAttribute('aria-label', 'Remove slot ' + ((i + 1) % 10));
        remove.textContent = '×';
        remove.addEventListener('click', function() {{
          ipc({{ type: 'quickslot_remove', slot: i }});
        }});
        wrap.appendChild(remove);
      }} else {{
        const empty = document.createElement('span');
        empty.className = 'card-title empty-label';
        empty.textContent = 'Empty';
        card.appendChild(empty);
      }}

      wrap.prepend(card);
      container.appendChild(wrap);
    }}
  }}

  window.__updateSlots = function(nextSlots) {{
    slots = Array.isArray(nextSlots) ? nextSlots : [];
    renderSlots();
  }};

  window.__setShortcuts = function(data) {{
    const actions = data && Array.isArray(data.actions) ? data.actions : [];
    const action = actions.find(function(item) {{ return item.id === 'command_palette'; }});
    document.getElementById('palette-shortcut').textContent = action && Array.isArray(action.keys)
      ? action.keys.join('')
      : '';
  }};

  document.addEventListener('visibilitychange', function() {{
    if (!document.hidden && !address.value) address.focus();
  }});

  renderSlots();
  window.__setShortcuts({keybindings_json});
  if (!document.hidden) requestAnimationFrame(function() {{ address.focus(); }});
}})();
</script>
</body>
</html>"#,
        css = NEWTAB_CSS,
        slots_json = safe_slots_json,
        keybindings_json = keybindings_json,
    )
    .replace("/*@@THEME@@*/", crate::theme::CSS)
    .replace("@@OCTOPUS_BRAND@@", crate::icons::OCTOPUS_BRAND)
}

const NEWTAB_CSS: &str = r#"  * { box-sizing: border-box; margin: 0; padding: 0; }

  html, body {
    width: 100%;
    min-height: 100%;
    background: var(--canvas);
    font-family: var(--font-text);
    -webkit-font-smoothing: antialiased;
    color: var(--label);
  }

  body { display: flex; overflow-y: auto; padding: 48px 24px; }

  .container {
    width: min(100%, 600px);
    margin: auto;
    text-align: center;
    animation: newTabIn var(--t-pop) var(--spring);
  }
  @keyframes newTabIn {
    from { opacity: 0; transform: translateY(8px); }
    to { opacity: 1; transform: none; }
  }

  .octopus {
    width: 40px;
    height: 40px;
    margin: 0 auto 12px;
    color: color-mix(in srgb, var(--err) 65%, var(--warn));
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .octopus svg { width: 100%; height: 100%; }

  h1 {
    font-family: var(--font-display);
    font-size: 22px;
    font-weight: 600;
    letter-spacing: -0.025em;
    margin-bottom: 16px;
  }

  .address-wrap { max-width: 480px; margin: 0 auto; }

  #address {
    width: 100%;
    height: 44px;
    padding: 0 16px;
    border: 0;
    border-radius: var(--r-capsule);
    background: var(--glass-thin);
    box-shadow: 0 0 0 0.5px var(--hairline), var(--glass-shine);
    color: var(--label);
    font: 15px var(--font-text);
  }
  #address::placeholder { color: var(--label-2); }
  #address:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }

  .subtitle { margin: 8px 0 28px; font-size: 11px; color: var(--label-2); }

  kbd.kbd, .card-badge { font-size: 11px; }
  kbd.kbd { padding: 2px 6px; }

  .slots {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
    margin-bottom: 20px;
  }

  .slot-wrap { position: relative; min-width: 0; }

  .slot-card {
    width: 100%;
    min-height: 44px;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 40px 10px 14px;
    border: 0;
    border-radius: var(--r-card);
    background: var(--glass-thin);
    backdrop-filter: var(--glass-blur);
    -webkit-backdrop-filter: var(--glass-blur);
    box-shadow: 0 0 0 0.5px var(--hairline), var(--glass-shine);
    color: inherit;
    font-family: inherit;
    cursor: pointer;
    min-width: 0;
    transition: background var(--t-fast) var(--ease);
  }
  .slot-card:hover { background: color-mix(in srgb, var(--glass-thick) 88%, var(--fill-hover)); }
  .slot-card:active { background: color-mix(in srgb, var(--glass-thick) 84%, var(--fill-press)); }
  .slot-card:focus-visible,
  .slot-remove:focus-visible { outline: 2px solid var(--accent); outline-offset: 2px; }

  .slot-empty .slot-card {
    padding-right: 14px;
    background: transparent;
    box-shadow: none;
    border: 1px dashed var(--hairline);
  }
  .slot-empty .slot-card:hover { background: var(--glass-thin); }

  .card-badge {
    flex-shrink: 0;
    width: 20px;
    height: 20px;
    border-radius: 5px;
    background: var(--fill);
    display: flex;
    align-items: center;
    justify-content: center;
    font-weight: 600;
    color: var(--label-2);
    font-variant-numeric: tabular-nums;
  }

  .card-favicon {
    flex-shrink: 0;
    width: 16px;
    height: 16px;
    border-radius: 3px;
    object-fit: contain;
  }

  .card-info { min-width: 0; display: flex; flex-direction: column; text-align: left; }

  .card-title {
    display: block;
    font-size: 13px;
    font-weight: 500;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .empty-label { color: var(--label-2); }

  .card-url {
    display: block;
    max-width: 180px;
    font-size: 11px;
    color: var(--label-2);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .slot-remove {
    position: absolute;
    z-index: 1;
    top: 50%;
    right: 9px;
    width: 22px;
    height: 22px;
    transform: translateY(-50%);
    border: 0;
    border-radius: var(--r-capsule);
    background: transparent;
    color: var(--label-2);
    font: 17px/22px var(--font-text);
    cursor: pointer;
    opacity: 0;
    transition: opacity var(--t-fast) var(--ease), background var(--t-fast) var(--ease);
  }
  .slot-wrap:hover .slot-remove,
  .slot-wrap:focus-within .slot-remove { opacity: 1; }
  .slot-remove:hover { background: var(--fill-hover); color: var(--label); }

  .hint {
    display: flex;
    justify-content: center;
    gap: 16px;
    flex-wrap: wrap;
    font-size: 11px;
    color: var(--label-2);
  }

  @media (prefers-reduced-motion: reduce) { .container { animation: none; } }
"#;
