/// Styled new-tab page shown instead of a blank about:blank.
///
/// Displays the octopus branding, a greeting, and quick-slot cards.
/// Slots are baked into the HTML at creation time (no dynamic JS update needed —
/// the page is only shown once per new tab).
///
/// IPC messages sent to Rust:
///   { type: "quickslot_open", slot: 0-9 }
///   { type: "quickslot_save", slot: 0-9 }  (click empty slot → save current page)
pub fn html(slots_json: &str) -> String {
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
<div class="container">
  <div class="octopus">@@OCTOPUS_BRAND@@</div>
  <h1>Welcome to Octoweb</h1>
  <p class="subtitle">Press <kbd class="kbd">⌘⇧P</kbd> to navigate anywhere</p>
  <div class="slots" id="slots"></div>
  <p class="hint">
    <kbd class="kbd">⌘1</kbd>–<kbd class="kbd">⌘0</kbd> open slots &nbsp;·&nbsp;
    <kbd class="kbd">⌘⇧1</kbd>–<kbd class="kbd">⌘⇧0</kbd> save current page
  </p>
</div>
<script>
(function() {{
  const slots = {slots_json};
  const container = document.getElementById('slots');
  for (let i = 0; i < 10; i++) {{
    const s = slots[i];
    const card = document.createElement('a');
    card.className = 'slot-card' + (s ? '' : ' slot-empty');
    card.href = '#';
    card.addEventListener('click', function(e) {{
      e.preventDefault();
      if (s) {{
        window.ipc.postMessage(JSON.stringify({{ type: 'quickslot_open', slot: i }}));
      }} else {{
        window.ipc.postMessage(JSON.stringify({{ type: 'quickslot_save', slot: i }}));
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
        card.appendChild(img);
      }}

      const info = document.createElement('div');
      info.className = 'card-info';

      const title = document.createElement('div');
      title.className = 'card-title';
      try {{
        const u = new URL(s.url);
        title.textContent = s.title || u.hostname.replace(/^www\./, '');
      }} catch(_) {{
        title.textContent = s.title || s.url;
      }}
      info.appendChild(title);

      const url = document.createElement('div');
      url.className = 'card-url';
      url.textContent = s.url;
      info.appendChild(url);

      card.appendChild(info);
    }} else {{
      const plus = document.createElement('span');
      plus.className = 'card-plus';
      plus.innerHTML = '@@ICON_PLUS@@';
      card.appendChild(plus);

      const hint = document.createElement('div');
      hint.className = 'card-info';
      const title = document.createElement('div');
      title.className = 'card-title empty-hint';
      title.textContent = 'Save current page';
      hint.appendChild(title);
      const shortcut = document.createElement('div');
      shortcut.className = 'card-url kbd';
      shortcut.textContent = '\u2318\u21e7' + ((i + 1) % 10);
      hint.appendChild(shortcut);
      card.appendChild(hint);
    }}
    container.appendChild(card);
  }}
}})();
</script>
</body>
</html>"#,
        css = NEWTAB_CSS,
        slots_json = slots_json,
    )
    .replace("/*@@THEME@@*/", crate::theme::CSS)
    .replace("@@OCTOPUS_BRAND@@", crate::icons::OCTOPUS_BRAND)
    .replace("@@ICON_PLUS@@", crate::icons::PLUS)
}

const NEWTAB_CSS: &str = r#"  * { box-sizing: border-box; margin: 0; padding: 0; }

  html, body {
    width: 100%; height: 100%;
    background: var(--canvas);
    font-family: var(--font-text);
    -webkit-font-smoothing: antialiased;
    color: var(--label);
  }

  /* Overflow-safe centering: margin:auto centers when the content fits but
     top-aligns and scrolls when it's taller than the window — align-items:
     center would push the container's top out of reach (page appears stuck
     at the bottom). */
  body { display: flex; overflow-y: auto; }

  .container {
    margin: auto;
    text-align: center;
    max-width: 600px;
    padding: 40px 24px;
    animation: newTabIn var(--t-pop) var(--spring);
  }
  @keyframes newTabIn {
    from { opacity: 0; transform: translateY(8px) scale(0.985); }
    to { opacity: 1; transform: none; }
  }

  .octopus {
    width: 64px;
    height: 64px;
    margin: 0 auto 12px;
    animation: float 3s var(--ease) infinite;
    color: color-mix(in srgb, var(--err) 65%, var(--warn));
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .octopus svg { width: 100%; height: 100%; }
  @keyframes float {
    0%, 100% { transform: translateY(0); }
    50% { transform: translateY(-6px); }
  }

  h1 {
    font-family: var(--font-display);
    font-size: 32px;
    font-weight: 700;
    letter-spacing: -0.04em;
    margin-bottom: 6px;
  }

  .subtitle {
    font-size: 15px;
    color: var(--label-2);
    margin-bottom: 32px;
  }

  kbd.kbd {
    font-size: 11px;
    padding: 2px 6px;
  }

  .slots {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 8px;
    margin-bottom: 24px;
  }

  .slot-card {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 10px 14px;
    min-height: 44px;
    border-radius: var(--r-card);
    background: var(--glass-thin);
    backdrop-filter: var(--glass-blur);
    -webkit-backdrop-filter: var(--glass-blur);
    box-shadow: 0 0 0 0.5px var(--hairline), var(--glass-shine);
    text-decoration: none;
    color: inherit;
    transition: background var(--t-fast) var(--ease), box-shadow var(--t-fast) var(--ease),
                transform var(--t-fast) var(--ease);
    min-width: 0;
  }

  .slot-card:hover {
    background: color-mix(in srgb, var(--glass-thick) 88%, var(--fill-hover));
    box-shadow: 0 0 0 0.5px var(--hairline), var(--glass-shine),
                0 8px 24px color-mix(in srgb, black 14%, transparent);
    transform: translateY(-1px);
  }

  .slot-card:active { background: color-mix(in srgb, var(--glass-thick) 84%, var(--fill-press)); transform: scale(0.98); }

  .slot-card.slot-empty {
    background: transparent;
    box-shadow: none;
    border: 1px dashed var(--hairline);
    opacity: 0.5;
    transition: opacity var(--t-fast) var(--ease), background var(--t-fast) var(--ease),
                transform var(--t-fast) var(--ease), border-color var(--t-fast) var(--ease);
  }

  .slot-card.slot-empty:hover {
    opacity: 0.85;
    background: var(--glass-thin);
    border-color: transparent;
    box-shadow: 0 0 0 0.5px var(--hairline), var(--glass-shine);
  }

  .empty-hint {
    color: var(--label-3) !important;
    font-style: normal;
    font-weight: 500;
  }

  .slot-card.slot-empty .card-info { opacity: 0; transition: opacity var(--t-fast) var(--ease); max-width: 0; overflow: hidden; }
  .slot-card.slot-empty:hover .card-info { opacity: 1; max-width: 240px; }
  .slot-card.slot-empty:hover .card-plus { opacity: 0; max-width: 0; }

  .card-plus {
    flex-shrink: 0;
    width: 16px; height: 16px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: var(--label-3);
    opacity: 0.7;
    transition: opacity var(--t-fast) var(--ease), max-width var(--t-fast) var(--ease);
    overflow: hidden;
  }
  .card-plus svg { width: 100%; height: 100%; }

  .card-badge {
    flex-shrink: 0;
    width: 20px; height: 20px;
    border-radius: 5px;
    background: var(--fill);
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 11px;
    font-weight: 600;
    color: var(--label-2);
    font-variant-numeric: tabular-nums;
  }

  .card-favicon {
    flex-shrink: 0;
    width: 16px; height: 16px;
    border-radius: 3px;
    object-fit: contain;
  }

  .card-info {
    min-width: 0;
    text-align: left;
  }

  .card-title {
    font-size: 13px;
    font-weight: 500;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .card-url {
    font-size: 11px;
    color: var(--label-3);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 180px;
  }

  .hint {
    font-size: 12px;
    color: var(--label-3);
  }
"#;
