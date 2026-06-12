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
<style>{css}</style>
</head>
<body>
<div class="container">
  <div class="octopus">@@OCTOPUS_BRAND@@</div>
  <h1>Octoweb</h1>
  <p class="subtitle">Press <kbd>⌘⇧P</kbd> to navigate anywhere</p>
  <div class="slots" id="slots"></div>
  <p class="hint">
    <kbd>⌘1</kbd>–<kbd>⌘0</kbd> open slots &nbsp;·&nbsp;
    <kbd>⌘⇧1</kbd>–<kbd>⌘⇧0</kbd> save current page
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
    badge.className = 'card-badge';
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
      shortcut.className = 'card-url';
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
    .replace("@@OCTOPUS_BRAND@@", crate::icons::OCTOPUS_BRAND)
    .replace("@@ICON_PLUS@@", crate::icons::PLUS)
}

const NEWTAB_CSS: &str = r#"
  * { box-sizing: border-box; margin: 0; padding: 0; }

  :root {
    --bg: #f5f5f7;
    --text-primary: rgba(0, 0, 0, 0.85);
    --text-secondary: rgba(0, 0, 0, 0.45);
    --text-dim: rgba(0, 0, 0, 0.30);
    --card-bg: rgba(255, 255, 255, 0.70);
    --card-border: rgba(0, 0, 0, 0.10);
    --card-hover: rgba(255, 255, 255, 0.90);
    --card-shadow: 0 0 0 0.5px rgba(0, 0, 0, 0.06), 0 2px 12px rgba(0, 0, 0, 0.06),
                   inset 0 1px 0 rgba(255, 255, 255, 0.6);
    --card-hover-shadow: 0 0 0 0.5px rgba(0, 0, 0, 0.06), 0 6px 24px rgba(0, 0, 0, 0.12),
                         inset 0 1px 0 rgba(255, 255, 255, 0.6);
    --kbd-bg: rgba(0, 0, 0, 0.06);
    --kbd-border: rgba(0, 0, 0, 0.10);
    --badge-bg: rgba(0, 0, 0, 0.06);
    --badge-text: rgba(0, 0, 0, 0.40);
  }

  @media (prefers-color-scheme: dark) {
    :root {
      --bg: #1c1c1e;
      --text-primary: rgba(255, 255, 255, 0.90);
      --text-secondary: rgba(255, 255, 255, 0.45);
      --text-dim: rgba(255, 255, 255, 0.25);
      --card-bg: rgba(44, 44, 48, 0.70);
      --card-border: rgba(255, 255, 255, 0.12);
      --card-hover: rgba(58, 58, 62, 0.90);
      --card-shadow: 0 0 0 0.5px rgba(255, 255, 255, 0.08), 0 2px 12px rgba(0, 0, 0, 0.20),
                     inset 0 1px 0 rgba(255, 255, 255, 0.06);
      --card-hover-shadow: 0 0 0 0.5px rgba(255, 255, 255, 0.08), 0 6px 24px rgba(0, 0, 0, 0.4),
                           inset 0 1px 0 rgba(255, 255, 255, 0.06);
      --kbd-bg: rgba(255, 255, 255, 0.08);
      --kbd-border: rgba(255, 255, 255, 0.10);
      --badge-bg: rgba(255, 255, 255, 0.08);
      --badge-text: rgba(255, 255, 255, 0.40);
    }
  }

  html, body {
    width: 100%; height: 100%;
    background: var(--bg);
    font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", "Helvetica Neue", sans-serif;
    -webkit-font-smoothing: antialiased;
    color: var(--text-primary);
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .container {
    text-align: center;
    max-width: 600px;
    padding: 40px 24px;
  }

  .octopus {
    width: 64px;
    height: 64px;
    margin: 0 auto 12px;
    animation: float 3s ease-in-out infinite;
    color: rgba(255, 99, 71, 0.82);
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .octopus svg { width: 100%; height: 100%; }
  @media (prefers-color-scheme: dark) {
    .octopus { color: rgba(255, 122, 99, 0.78); }
  }

  @keyframes float {
    0%, 100% { transform: translateY(0); }
    50% { transform: translateY(-6px); }
  }

  h1 {
    font-family: -apple-system, BlinkMacSystemFont, "SF Pro Display", "Helvetica Neue", sans-serif;
    font-size: 28px;
    font-weight: 700;
    letter-spacing: -0.03em;
    margin-bottom: 6px;
  }

  .subtitle {
    font-size: 15px;
    color: var(--text-secondary);
    margin-bottom: 32px;
  }

  kbd {
    display: inline-block;
    font-family: -apple-system, BlinkMacSystemFont, "SF Pro Text", sans-serif;
    font-size: 11px;
    font-weight: 500;
    padding: 2px 6px;
    border-radius: 4px;
    background: var(--kbd-bg);
    border: 0.5px solid var(--kbd-border);
    color: var(--text-secondary);
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
    border-radius: 14px;
    background: var(--card-bg);
    backdrop-filter: blur(12px) saturate(160%);
    -webkit-backdrop-filter: blur(12px) saturate(160%);
    box-shadow: var(--card-shadow);
    text-decoration: none;
    color: inherit;
    transition: background 0.12s ease, box-shadow 0.2s ease,
                transform 0.25s cubic-bezier(0.34, 1.56, 0.64, 1);
    min-width: 0;
  }

  .slot-card:hover {
    background: var(--card-hover);
    box-shadow: var(--card-hover-shadow);
    transform: translateY(-1px);
  }

  .slot-card:active { transform: scale(0.98); transition-duration: 0.08s; }

  .slot-card.slot-empty {
    background: transparent;
    box-shadow: none;
    border: 1px dashed var(--card-border);
    opacity: 0.5;
    transition: opacity 0.15s ease, background 0.12s ease, box-shadow 0.2s ease,
                transform 0.25s cubic-bezier(0.34, 1.56, 0.64, 1), border-color 0.12s ease;
  }

  .slot-card.slot-empty:hover {
    opacity: 0.85;
    background: var(--card-bg);
    border-color: transparent;
    box-shadow: var(--card-hover-shadow);
  }

  .empty-hint {
    color: var(--text-dim) !important;
    font-style: normal;
    font-weight: 500;
  }

  .slot-card.slot-empty .card-info { opacity: 0; transition: opacity 0.14s ease; max-width: 0; overflow: hidden; }
  .slot-card.slot-empty:hover .card-info { opacity: 1; max-width: 240px; }
  .slot-card.slot-empty:hover .card-plus { opacity: 0; max-width: 0; }

  .card-plus {
    flex-shrink: 0;
    width: 16px; height: 16px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    color: var(--text-dim);
    opacity: 0.7;
    transition: opacity 0.12s ease, max-width 0.14s ease;
    overflow: hidden;
  }
  .card-plus svg { width: 100%; height: 100%; }

  .card-badge {
    flex-shrink: 0;
    width: 20px; height: 20px;
    border-radius: 5px;
    background: var(--badge-bg);
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 11px;
    font-weight: 600;
    color: var(--badge-text);
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
    color: var(--text-dim);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    max-width: 180px;
  }

  .hint {
    font-size: 12px;
    color: var(--text-dim);
  }
"#;
