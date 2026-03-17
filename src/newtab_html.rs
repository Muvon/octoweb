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
  <div class="octopus">🐙</div>
  <h1>Octoweb</h1>
  <p class="subtitle">Press <kbd>⌘K</kbd> to navigate anywhere</p>
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
      const hint = document.createElement('div');
      hint.className = 'card-info';
      const title = document.createElement('div');
      title.className = 'card-title empty-hint';
      title.textContent = 'Save page here';
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
}

const NEWTAB_CSS: &str = r#"
  * { box-sizing: border-box; margin: 0; padding: 0; }

  :root {
    --bg: #f5f5f7;
    --text-primary: rgba(0, 0, 0, 0.85);
    --text-secondary: rgba(0, 0, 0, 0.45);
    --text-dim: rgba(0, 0, 0, 0.30);
    --card-bg: rgba(255, 255, 255, 0.70);
    --card-border: rgba(0, 0, 0, 0.06);
    --card-hover: rgba(255, 255, 255, 0.90);
    --card-shadow: 0 2px 12px rgba(0, 0, 0, 0.06);
    --card-hover-shadow: 0 4px 20px rgba(0, 0, 0, 0.10);
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
      --card-border: rgba(255, 255, 255, 0.06);
      --card-hover: rgba(58, 58, 62, 0.90);
      --card-shadow: 0 2px 12px rgba(0, 0, 0, 0.20);
      --card-hover-shadow: 0 4px 20px rgba(0, 0, 0, 0.35);
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
    font-size: 56px;
    line-height: 1;
    margin-bottom: 12px;
    animation: float 3s ease-in-out infinite;
  }

  @keyframes float {
    0%, 100% { transform: translateY(0); }
    50% { transform: translateY(-6px); }
  }

  h1 {
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
    border-radius: 10px;
    background: var(--card-bg);
    backdrop-filter: blur(12px) saturate(160%);
    -webkit-backdrop-filter: blur(12px) saturate(160%);
    border: 0.5px solid var(--card-border);
    box-shadow: var(--card-shadow);
    text-decoration: none;
    color: inherit;
    transition: background 0.12s ease, box-shadow 0.15s ease, transform 0.1s ease;
    min-width: 0;
  }

  .slot-card:hover {
    background: var(--card-hover);
    box-shadow: var(--card-hover-shadow);
  }

  .slot-card:active { transform: scale(0.98); }

  .slot-card.slot-empty {
    border-style: dashed;
    opacity: 0.5;
    transition: opacity 0.15s ease, background 0.12s ease, box-shadow 0.15s ease, transform 0.1s ease, border-color 0.12s ease;
  }

  .slot-card.slot-empty:hover {
    opacity: 0.8;
    border-style: solid;
  }

  .empty-hint {
    color: var(--text-dim) !important;
    font-style: italic;
  }

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
