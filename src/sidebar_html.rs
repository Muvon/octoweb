/// Returns the full HTML for the ACP agent sidebar panel.
///
/// JS API (called from Rust via evaluate_script):
///   window.__setConnected()          — mark ACP session ready (green dot)
///   window.__setConnecting()         — mark ACP session connecting (orange dot)
///   window.__setError()              — mark connection error (red dot)
///   window.__appendChunk(text)       — append streaming text to current agent bubble
///   window.__setThinking(bool)       — show/hide the activity indicator
///   window.__toolStart(id,title,kind)— show a new tool call in the activity feed
///   window.__toolUpdate(id,title,st) — update a tool call status/title
///   window.__appendError(text)       — show an error message
///   window.__setAgentTag(tag)        — update the agent chip label
///
/// IPC messages sent to Rust:
///   { type: "acp_prompt", text: "..." }      — user submitted a prompt
///   { type: "sidebar_close" }                — user clicked close
///   { type: "acp_set_agent", tag: "..." }    — user changed agent tag
pub fn html() -> &'static str {
    r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<style>
  *, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }

  /* ── Tahoe Liquid Glass tokens ─────────────────────────────────────────── */
  :root {
    --glass-solid:     rgb(235, 235, 240);
    --glass-bg:        rgba(235, 235, 240, 0.72);
    --glass-border:    rgba(255, 255, 255, 0.55);
    --glass-inner:     rgba(255, 255, 255, 0.38);
    --glass-shadow:    0 8px 40px rgba(0,0,0,0.13), 0 1.5px 6px rgba(0,0,0,0.07);

    --user-bg:         rgba(0, 122, 255, 0.11);
    --user-border:     rgba(0, 122, 255, 0.22);
    --agent-bg:        rgba(255, 255, 255, 0.46);
    --agent-border:    rgba(0, 0, 0, 0.07);
    --error-bg:        rgba(255, 59, 48, 0.09);
    --error-border:    rgba(255, 59, 48, 0.22);
    --error-text:      #bf2114;

    --input-bg:        rgba(255, 255, 255, 0.60);
    --input-border:    rgba(0, 0, 0, 0.10);
    --input-focus-border: rgba(0, 122, 255, 0.55);
    --input-shadow:    inset 0 1px 3px rgba(0,0,0,0.05);

    --text-primary:    rgba(0, 0, 0, 0.86);
    --text-secondary:  rgba(0, 0, 0, 0.44);
    --text-tertiary:   rgba(0, 0, 0, 0.28);

    --accent:          #007aff;
    --accent-hover:    #0066d6;

    --dot-ok:          #28cd41;
    --dot-wait:        #ff9500;
    --dot-err:         #ff3b30;

    --divider:         rgba(0, 0, 0, 0.07);
    --scrollbar:       rgba(0, 0, 0, 0.13);

    /* Markdown content tokens */
    --md-code-bg:      rgba(0, 0, 0, 0.055);
    --md-code-border:  rgba(0, 0, 0, 0.08);
    --md-pre-bg:       rgba(0, 0, 0, 0.04);
    --md-blockquote:   rgba(0, 122, 255, 0.18);
    --md-hr:           rgba(0, 0, 0, 0.09);
    --md-table-border: rgba(0, 0, 0, 0.09);
    --md-table-head:   rgba(0, 0, 0, 0.04);
  }

  @media (prefers-color-scheme: dark) {
    :root {
      --glass-solid:     rgb(30, 30, 34);
      --glass-bg:        rgba(30, 30, 34, 0.78);
      --glass-border:    rgba(255, 255, 255, 0.10);
      --glass-inner:     rgba(255, 255, 255, 0.05);
      --glass-shadow:    0 8px 48px rgba(0,0,0,0.55), 0 1.5px 6px rgba(0,0,0,0.30);

      --user-bg:         rgba(10, 132, 255, 0.16);
      --user-border:     rgba(10, 132, 255, 0.28);
      --agent-bg:        rgba(255, 255, 255, 0.07);
      --agent-border:    rgba(255, 255, 255, 0.09);
      --error-bg:        rgba(255, 69, 58, 0.13);
      --error-border:    rgba(255, 69, 58, 0.28);
      --error-text:      rgba(255, 105, 97, 0.95);

      --input-bg:        rgba(255, 255, 255, 0.08);
      --input-border:    rgba(255, 255, 255, 0.12);
      --input-focus-border: rgba(10, 132, 255, 0.60);
      --input-shadow:    inset 0 1px 3px rgba(0,0,0,0.25);

      --text-primary:    rgba(255, 255, 255, 0.90);
      --text-secondary:  rgba(255, 255, 255, 0.44);
      --text-tertiary:   rgba(255, 255, 255, 0.26);

      --accent:          #0a84ff;
      --accent-hover:    #409cff;

      --dot-ok:          #30d158;
      --dot-wait:        #ff9f0a;
      --dot-err:         #ff453a;

      --divider:         rgba(255, 255, 255, 0.07);
      --scrollbar:       rgba(255, 255, 255, 0.13);

      --md-code-bg:      rgba(255, 255, 255, 0.08);
      --md-code-border:  rgba(255, 255, 255, 0.10);
      --md-pre-bg:       rgba(0, 0, 0, 0.28);
      --md-blockquote:   rgba(10, 132, 255, 0.22);
      --md-hr:           rgba(255, 255, 255, 0.10);
      --md-table-border: rgba(255, 255, 255, 0.10);
      --md-table-head:   rgba(255, 255, 255, 0.05);
    }
  }

  /* ── Base ───────────────────────────────────────────────────────────────── */
  html, body {
    width: 100%; height: 100%; overflow: hidden;
    font-family: -apple-system, "SF Pro Text", "Helvetica Neue", sans-serif;
    -webkit-font-smoothing: antialiased;
    font-size: 13px;
    line-height: 1.5;
    background: transparent;
    color: var(--text-primary);
  }

  /* ── Liquid Glass panel ─────────────────────────────────────────────────── */
  #sidebar {
    display: flex;
    flex-direction: column;
    height: 100vh;
    /* Solid opaque fallback — prevents see-through when WKWebView
       backdrop-filter degrades through two transparent window layers. */
    background: var(--glass-solid);
    border-left: 1px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    position: relative;
    /* Match macOS window corner radius (16pt logical = 32px physical on 2x Retina).
       Right-side corners rounded to align with the window frame.
       Measured via NSThemeFrame _cornerRadius on macOS 26 Tahoe. */
    border-radius: 0 16px 16px 0;
    overflow: hidden;
  }
  #sidebar::before {
    content: "";
    position: absolute;
    inset: 0;
    pointer-events: none;
    /* Glass layer + inner highlight on top of solid fallback */
    background: var(--glass-bg);
    backdrop-filter: blur(48px) saturate(180%);
    -webkit-backdrop-filter: blur(48px) saturate(180%);
    z-index: 0;
    border-radius: inherit;
  }
  #sidebar::after {
    content: "";
    position: absolute;
    inset: 0;
    pointer-events: none;
    background: linear-gradient(135deg, var(--glass-inner) 0%, transparent 50%);
    z-index: 0;
    border-radius: inherit;
  }
  #sidebar > * { position: relative; z-index: 1; }

  /* ── Header ─────────────────────────────────────────────────────────────── */
  #header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    height: 32px;
    padding: 0 12px 0 14px;
    border-bottom: 1px solid var(--divider);
    flex-shrink: 0;
    -webkit-app-region: drag;
  }
  #header > * { -webkit-app-region: no-drag; }

  #header-title {
    display: flex;
    align-items: center;
    gap: 7px;
  }

  #header-title .logo {
    font-size: 16px;
    line-height: 1;
    filter: drop-shadow(0 1px 2px rgba(0,0,0,0.15));
  }

  #header-title .name {
    font-size: 13px;
    font-weight: 600;
    letter-spacing: -0.015em;
    color: var(--text-primary);
  }

  #status-pill {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 2px 8px 2px 6px;
    border-radius: 20px;
    background: var(--agent-bg);
    border: 1px solid var(--agent-border);
    font-size: 10px;
    font-weight: 500;
    color: var(--text-secondary);
    letter-spacing: 0.01em;
    transition: all 0.25s ease;
  }

  .dot {
    width: 6px; height: 6px;
    border-radius: 50%;
    background: var(--dot-ok);
    box-shadow: 0 0 0 2px rgba(40, 205, 65, 0.20);
    flex-shrink: 0;
    transition: background 0.3s, box-shadow 0.3s;
  }
  .dot.connecting {
    background: var(--dot-wait);
    box-shadow: 0 0 0 2px rgba(255, 149, 0, 0.20);
    animation: pulse 1.4s ease-in-out infinite;
  }
  .dot.error {
    background: var(--dot-err);
    box-shadow: 0 0 0 2px rgba(255, 59, 48, 0.20);
  }

  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50%       { opacity: 0.25; }
  }

  #close-btn {
    width: 24px; height: 24px;
    border-radius: 50%;
    border: 1px solid var(--agent-border);
    background: var(--agent-bg);
    color: var(--text-secondary);
    cursor: pointer;
    display: flex; align-items: center; justify-content: center;
    transition: background 0.15s, color 0.15s, border-color 0.15s;
    flex-shrink: 0;
  }
  #close-btn:hover {
    background: rgba(0,0,0,0.07);
    color: var(--text-primary);
    border-color: rgba(0,0,0,0.13);
  }
  @media (prefers-color-scheme: dark) {
    #close-btn:hover {
      background: rgba(255,255,255,0.10);
      border-color: rgba(255,255,255,0.16);
    }
  }
  #close-btn:active { transform: scale(0.92); }

  /* ── Agent chip (inline in header) ─────────────────────────────────────── */
  #agent-chip {
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 2px 8px 2px 6px;
    border-radius: 20px;
    background: var(--agent-bg);
    border: 1px solid var(--agent-border);
    font-size: 10.5px;
    color: var(--text-secondary);
    cursor: pointer;
    transition: background 0.15s, border-color 0.15s, color 0.15s;
    max-width: 130px;
    overflow: hidden;
    flex-shrink: 0;
  }
  #agent-chip:hover {
    background: rgba(0,0,0,0.06);
    border-color: rgba(0,0,0,0.13);
    color: var(--text-primary);
  }
  @media (prefers-color-scheme: dark) {
    #agent-chip:hover {
      background: rgba(255,255,255,0.10);
      border-color: rgba(255,255,255,0.16);
    }
  }
  #agent-chip svg { flex-shrink: 0; opacity: 0.50; }
  #agent-chip-label {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* Edit row replaces the entire header content when active */
  #agent-edit-row {
    display: none;
    align-items: center;
    gap: 6px;
    flex: 1;
    min-width: 0;
  }
  #agent-edit-row.visible { display: flex; }

  #agent-input {
    flex: 1;
    background: var(--input-bg);
    border: 1.5px solid var(--input-focus-border);
    border-radius: 10px;
    padding: 3px 9px;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    font-size: 11px;
    color: var(--text-primary);
    outline: none;
    min-width: 0;
  }

  #agent-confirm-btn {
    width: 22px; height: 22px;
    border-radius: 50%;
    border: none;
    background: var(--accent);
    color: #fff;
    cursor: pointer;
    display: flex; align-items: center; justify-content: center;
    flex-shrink: 0;
    transition: background 0.15s, transform 0.12s;
  }
  #agent-confirm-btn:hover  { background: var(--accent-hover); }
  #agent-confirm-btn:active { transform: scale(0.88); }

  /* Hide normal header controls while edit row is open */
  #header.editing #header-title,
  #header.editing #header-right { display: none; }

  /* ── Copy button — macOS frosted pill, top-right of bubble ───────────────── */
  .msg-bubble { position: relative; }
  .msg-copy {
    position: absolute;
    top: 6px; right: 6px;
    display: flex;
    align-items: center;
    gap: 4px;
    padding: 3px 7px;
    border: none;
    border-radius: 20px;
    background: rgba(255,255,255,0.72);
    -webkit-backdrop-filter: blur(8px);
    backdrop-filter: blur(8px);
    box-shadow: 0 1px 3px rgba(0,0,0,0.12), 0 0 0 0.5px rgba(0,0,0,0.08);
    color: rgba(60,60,67,0.75);
    font-size: 10px;
    font-weight: 500;
    letter-spacing: 0.01em;
    cursor: pointer;
    opacity: 0;
    transition: opacity 0.15s, background 0.12s, color 0.12s;
    pointer-events: none;
  }
  @media (prefers-color-scheme: dark) {
    .msg-copy {
      background: rgba(58,58,60,0.82);
      box-shadow: 0 1px 3px rgba(0,0,0,0.35), 0 0 0 0.5px rgba(255,255,255,0.08);
      color: rgba(235,235,245,0.65);
    }
    .msg-copy:hover { background: rgba(72,72,74,0.92); color: rgba(235,235,245,0.9); }
  }
  .msg.agent:hover .msg-copy { opacity: 1; pointer-events: auto; }
  .msg-copy:hover { background: rgba(255,255,255,0.92); color: rgba(60,60,67,1); }
  .msg-copy.copied { color: #34c759; opacity: 1; }

  /* Show more button below collapsed bubbles */
  .msg-bubble.collapsed {
    max-height: 220px;
    overflow: hidden;
    mask-image: linear-gradient(to bottom, black 55%, transparent 100%);
    -webkit-mask-image: linear-gradient(to bottom, black 55%, transparent 100%);
  }
  .msg-show-more {
    align-self: flex-start;
    background: none;
    border: none;
    color: var(--accent);
    font-size: 11.5px;
    cursor: pointer;
    padding: 2px 4px;
    border-radius: 5px;
    transition: background 0.12s;
  }
  .msg-show-more:hover { background: rgba(0,0,0,0.06); }
  @media (prefers-color-scheme: dark) {
    .msg-show-more:hover { background: rgba(255,255,255,0.08); }
  }

  /* ── Messages ───────────────────────────────────────────────────────────── */
  #messages {
    flex: 1;
    overflow-y: auto;
    padding: 14px 12px 6px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    scroll-behavior: smooth;
    overscroll-behavior: contain;
  }

  #messages::-webkit-scrollbar { width: 3px; }
  #messages::-webkit-scrollbar-track { background: transparent; }
  #messages::-webkit-scrollbar-thumb {
    background: var(--scrollbar);
    border-radius: 2px;
  }

  .msg { display: flex; flex-direction: column; gap: 3px; }

  .msg-label {
    display: flex;
    align-items: baseline;
    gap: 6px;
    padding: 0 4px;
  }
  .msg-label .msg-who {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.06em;
    text-transform: uppercase;
    color: var(--text-tertiary);
  }
  .msg-label .msg-time {
    font-size: 9.5px;
    font-weight: 400;
    letter-spacing: 0.01em;
    text-transform: none;
    color: var(--text-tertiary);
    opacity: 0.7;
    font-variant-numeric: tabular-nums;
  }
  .msg-label .msg-tools {
    font-size: 9.5px;
    font-weight: 400;
    letter-spacing: 0.01em;
    color: var(--text-tertiary);
    opacity: 0.5;
    margin-left: 2px;
  }
  .msg.user  .msg-label { justify-content: flex-end; }
  .msg.user  .msg-label .msg-who { color: var(--accent); opacity: 0.75; }
  .msg.error .msg-label .msg-who { color: var(--dot-err); opacity: 0.85; }

  /* Bubbles — NOT glass (Tahoe: never glass on glass) */
  .msg-bubble {
    padding: 9px 12px;
    border-radius: 16px;
    line-height: 1.55;
    word-break: break-word;
    font-size: 13px;
  }

  .msg.user .msg-bubble {
    background: var(--user-bg);
    border: 1px solid var(--user-border);
    border-bottom-right-radius: 5px;
    color: var(--text-primary);
    align-self: flex-end;
    max-width: 90%;
    white-space: pre-wrap;
    transition: box-shadow 0.15s, background 0.15s;
  }
  .msg.user .msg-bubble:hover {
    box-shadow: 0 2px 8px rgba(0,122,255,0.13);
  }

  .msg.agent .msg-bubble {
    background: var(--agent-bg);
    border: 1px solid var(--agent-border);
    border-bottom-left-radius: 5px;
    color: var(--text-primary);
    align-self: flex-start;
    max-width: 100%;
    transition: box-shadow 0.15s, background 0.15s;
    /* no white-space: pre-wrap — markdown renders HTML */
  }
  .msg.agent .msg-bubble:hover {
    box-shadow: 0 2px 10px rgba(0,0,0,0.07);
  }
  @media (prefers-color-scheme: dark) {
    .msg.agent .msg-bubble:hover { box-shadow: 0 2px 10px rgba(0,0,0,0.30); }
  }

  .msg.error .msg-bubble {
    background: var(--error-bg);
    border: 1px solid var(--error-border);
    border-radius: 12px;
    color: var(--error-text);
    font-size: 12px;
    white-space: pre-wrap;
  }

  /* ── Markdown content inside agent bubbles ──────────────────────────────── */
  .msg.agent .msg-bubble p { margin: 0 0 8px; }
  .msg.agent .msg-bubble p:last-child { margin-bottom: 0; }

  .msg.agent .msg-bubble h1,
  .msg.agent .msg-bubble h2,
  .msg.agent .msg-bubble h3,
  .msg.agent .msg-bubble h4 {
    font-weight: 600;
    line-height: 1.3;
    margin: 12px 0 5px;
    color: var(--text-primary);
  }
  .msg.agent .msg-bubble h1:first-child,
  .msg.agent .msg-bubble h2:first-child,
  .msg.agent .msg-bubble h3:first-child { margin-top: 0; }
  .msg.agent .msg-bubble h1 { font-size: 15px; }
  .msg.agent .msg-bubble h2 { font-size: 14px; }
  .msg.agent .msg-bubble h3 { font-size: 13px; }
  .msg.agent .msg-bubble h4 { font-size: 12px; text-transform: uppercase; letter-spacing: 0.04em; }

  .msg.agent .msg-bubble ul,
  .msg.agent .msg-bubble ol {
    margin: 4px 0 8px 18px;
    padding: 0;
  }
  .msg.agent .msg-bubble li { margin: 2px 0; }
  .msg.agent .msg-bubble li p { margin: 0; }

  /* Inline code */
  .msg.agent .msg-bubble code {
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
    font-size: 11.5px;
    background: var(--md-code-bg);
    border: 1px solid var(--md-code-border);
    border-radius: 4px;
    padding: 1px 5px;
  }

  /* Code blocks */
  .msg.agent .msg-bubble pre {
    background: var(--md-pre-bg);
    border: 1px solid var(--md-code-border);
    border-radius: 8px;
    padding: 10px 12px;
    margin: 6px 0 8px;
    overflow-x: auto;
  }
  .msg.agent .msg-bubble pre::-webkit-scrollbar { height: 3px; }
  .msg.agent .msg-bubble pre::-webkit-scrollbar-thumb {
    background: var(--scrollbar);
    border-radius: 2px;
  }
  .msg.agent .msg-bubble pre code {
    background: none;
    border: none;
    padding: 0;
    font-size: 11.5px;
    line-height: 1.6;
    color: var(--text-primary);
  }

  /* Blockquote */
  .msg.agent .msg-bubble blockquote {
    border-left: 3px solid var(--md-blockquote);
    margin: 6px 0;
    padding: 2px 0 2px 10px;
    color: var(--text-secondary);
  }

  /* Horizontal rule */
  .msg.agent .msg-bubble hr {
    border: none;
    border-top: 1px solid var(--md-hr);
    margin: 10px 0;
  }

  /* Tables */
  .msg.agent .msg-bubble table {
    border-collapse: collapse;
    width: 100%;
    font-size: 12px;
    margin: 6px 0 8px;
  }
  .msg.agent .msg-bubble th,
  .msg.agent .msg-bubble td {
    border: 1px solid var(--md-table-border);
    padding: 5px 9px;
    text-align: left;
  }
  .msg.agent .msg-bubble th {
    background: var(--md-table-head);
    font-weight: 600;
  }

  /* Links */
  .msg.agent .msg-bubble a {
    color: var(--accent);
    text-decoration: none;
  }
  .msg.agent .msg-bubble a:hover { text-decoration: underline; }

  /* Strong / em */
  .msg.agent .msg-bubble strong { font-weight: 600; }
  .msg.agent .msg-bubble em     { font-style: italic; }

  /* ── Activity indicator ─────────────────────────────────────────────────────────────── */
  #thinking {
    display: none;
    flex-direction: column;
    gap: 0;
    padding: 4px 0;
  }
  #thinking.visible { display: flex; }

  /* Tahoe-style 3-dot bounce — shown as header while tools stream below */
  .activity-header {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 2px 4px 4px;
  }
  .activity-dots {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .activity-dots span {
    width: 5px; height: 5px;
    border-radius: 50%;
    background: var(--text-tertiary);
    animation: dot-bounce 1.3s ease-in-out infinite;
  }
  .activity-dots span:nth-child(2) { animation-delay: 0.20s; }
  .activity-dots span:nth-child(3) { animation-delay: 0.40s; }
  @keyframes dot-bounce {
    0%, 80%, 100% { transform: translateY(0);    opacity: 0.30; }
    40%           { transform: translateY(-4px); opacity: 0.90; }
  }
  .activity-elapsed {
    font-size: 10px;
    color: var(--text-tertiary);
    font-variant-numeric: tabular-nums;
    margin-left: 2px;
    opacity: 0.7;
  }

  /* Individual tool row */
  .tool-row {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 3px 4px;
    font-size: 11px;
    color: var(--text-secondary);
    animation: tool-in 0.25s ease-out;
    overflow: hidden;
  }
  @keyframes tool-in {
    from { opacity: 0; transform: translateY(4px); }
    to   { opacity: 1; transform: translateY(0); }
  }
  .tool-row.done {
    opacity: 0.45;
    transition: opacity 0.3s ease;
  }
  .tool-row.failed {
    color: var(--error-text);
    opacity: 0.7;
  }

  /* Kind icon — tiny circle with letter */
  .tool-kind {
    width: 14px; height: 14px;
    border-radius: 4px;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 8px;
    font-weight: 700;
    letter-spacing: 0;
    flex-shrink: 0;
    color: #fff;
    background: var(--text-tertiary);
  }
  .tool-kind.read    { background: #34c759; }
  .tool-kind.edit    { background: #ff9500; }
  .tool-kind.delete  { background: #ff3b30; }
  .tool-kind.search  { background: #5856d6; }
  .tool-kind.execute { background: #007aff; }
  .tool-kind.think   { background: #af52de; }
  .tool-kind.fetch   { background: #30b0c7; }

  .tool-title {
    flex: 1;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tool-time {
    flex-shrink: 0;
    font-variant-numeric: tabular-nums;
    color: var(--text-tertiary);
    font-size: 10px;
  }
  .tool-check {
    flex-shrink: 0;
    color: #34c759;
    font-size: 10px;
    line-height: 1;
  }
  .tool-fail {
    flex-shrink: 0;
    color: #ff3b30;
    font-size: 10px;
    line-height: 1;
  }

  /* ── Input area ─────────────────────────────────────────────────────────── */
  #input-area {
    flex-shrink: 0;
    padding: 10px 12px 14px;
    border-top: 1px solid var(--divider);
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  #input-row {
    display: flex;
    align-items: center;
    gap: 8px;
    background: var(--input-bg);
    border: 1.5px solid var(--input-border);
    border-radius: 18px;
    padding: 8px 8px 8px 10px;
    box-shadow: var(--input-shadow);
    transition: border-color 0.18s, box-shadow 0.18s;
  }

  #new-session-btn {
    width: 28px; height: 28px;
    border-radius: 50%;
    border: none;
    background: transparent;
    color: var(--text-tertiary);
    cursor: pointer;
    display: flex; align-items: center; justify-content: center;
    flex-shrink: 0;
    transition: background 0.15s, color 0.15s;
    padding: 0;
  }
  #new-session-btn:hover {
    background: var(--input-border);
    color: var(--text-primary);
  }
  #new-session-btn:active { transform: scale(0.88); }
  #input-row:focus-within {
    border-color: var(--input-focus-border);
    box-shadow: var(--input-shadow), 0 0 0 3px rgba(0, 122, 255, 0.11);
  }

  #prompt-input {
    flex: 1;
    background: transparent;
    border: none;
    outline: none;
    color: var(--text-primary);
    font-family: inherit;
    font-size: 13px;
    line-height: 1.5;
    resize: none;
    max-height: 120px;
    min-height: 20px;
    overflow-y: auto;
    caret-color: var(--accent);
  }
  #prompt-input::placeholder { color: var(--text-tertiary); }
  #prompt-input::-webkit-scrollbar { width: 3px; }
  #prompt-input::-webkit-scrollbar-thumb {
    background: var(--scrollbar);
    border-radius: 2px;
  }

  #attach-wrap {
    position: relative;
    flex-shrink: 0;
  }
  #attach-btn {
    width: 30px; height: 30px;
    border-radius: 50%;
    border: none;
    background: transparent;
    color: var(--text-tertiary);
    cursor: pointer;
    display: flex; align-items: center; justify-content: center;
    transition: background 0.15s, color 0.15s;
    padding: 0;
  }
  #attach-btn:hover { background: var(--input-border); color: var(--text-primary); }
  #attach-btn:active { transform: scale(0.88); }

  #attach-menu {
    display: none;
    position: absolute;
    bottom: calc(100% + 6px);
    right: 0;
    background: var(--glass-bg);
    -webkit-backdrop-filter: blur(40px) saturate(1.6);
    border: 1px solid var(--input-border);
    border-radius: 10px;
    padding: 4px;
    box-shadow: 0 4px 20px rgba(0,0,0,0.12);
    z-index: 10;
    min-width: 130px;
  }
  #attach-menu.visible { display: flex; flex-direction: column; }
  .attach-option {
    display: flex; align-items: center; gap: 8px;
    padding: 7px 10px;
    border: none; background: transparent;
    color: var(--text-primary);
    font-family: inherit; font-size: 12px;
    border-radius: 7px;
    cursor: pointer;
    white-space: nowrap;
    transition: background 0.12s;
  }
  .attach-option:hover { background: var(--input-border); }
  .attach-option svg { color: var(--text-secondary); flex-shrink: 0; }

  .doc-chip {
    display: flex; align-items: center; gap: 4px;
    padding: 4px 8px;
    background: var(--input-border);
    border-radius: 8px;
    font-size: 11px;
    color: var(--text-secondary);
    max-width: 160px;
  }
  .doc-chip span {
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  .doc-chip .rm {
    width: 14px; height: 14px; border-radius: 50%;
    background: rgba(0,0,0,0.15); color: var(--text-secondary);
    font-size: 9px; line-height: 14px; text-align: center;
    cursor: pointer; border: none; padding: 0; flex-shrink: 0;
  }

  #image-preview {
    display: none;
    gap: 6px;
    padding: 6px 8px 0;
    flex-wrap: wrap;
  }
  #image-preview.visible { display: flex; }
  .img-thumb {
    position: relative;
    width: 48px; height: 48px;
    border-radius: 8px;
    overflow: hidden;
    border: 1px solid var(--input-border);
  }
  .img-thumb img { width: 100%; height: 100%; object-fit: cover; }
  .img-thumb .rm {
    position: absolute; top: -1px; right: -1px;
    width: 16px; height: 16px; border-radius: 50%;
    background: rgba(0,0,0,0.6); color: #fff;
    font-size: 10px; line-height: 16px; text-align: center;
    cursor: pointer; border: none; padding: 0;
  }

  .msg-bubble img.chat-img {
    max-width: 100%;
    border-radius: 8px;
    margin-top: 6px;
    display: block;
  }

  #send-btn {
    width: 30px; height: 30px;
    border-radius: 50%;
    border: none;
    background: var(--accent);
    color: #fff;
    cursor: pointer;
    display: flex; align-items: center; justify-content: center;
    flex-shrink: 0;
    transition: background 0.15s, opacity 0.15s, transform 0.12s, border-radius 0.2s ease;
    opacity: 0.28;
    pointer-events: none;
  }
  #send-btn.active              { opacity: 1; pointer-events: auto; }
  #send-btn.active:hover        { background: var(--accent-hover); }
  #send-btn.active:active       { transform: scale(0.88); }
  /* Stop mode — Tahoe-style: same pill shape as input, subtle red */
  #send-btn.stop-mode {
    background: #ff3b30;
    border-radius: 50%;
    opacity: 1;
    pointer-events: auto;
    animation: stop-pulse 1.2s ease-in-out infinite;
  }
  #send-btn.stop-mode:hover     { background: #e6352b; }
  #send-btn.stop-mode:active    { transform: scale(0.88); }
  #send-btn.stop-mode .send-icon { display: none; }
  #send-btn.stop-mode .stop-icon { display: block; }
  #send-btn .stop-icon          { display: none; }
  @keyframes stop-pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.7; }
  }
  #hint {
    font-size: 10.5px;
    color: var(--text-tertiary);
    text-align: center;
    letter-spacing: 0.01em;
  }

  /* ── Message queue ───────────────────────────────────────────────────────── */
  #queue-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .queue-item {
    display: flex;
    align-items: center;
    gap: 6px;
    background: var(--agent-bg);
    border: 1px solid var(--agent-border);
    border-radius: 10px;
    padding: 5px 8px 5px 10px;
    font-size: 11.5px;
    color: var(--text-secondary);
    animation: queue-in 0.15s ease;
    transition: background 0.12s, border-color 0.12s;
  }
  .queue-item:hover {
    background: rgba(0,0,0,0.04);
    border-color: rgba(0,0,0,0.11);
  }
  @media (prefers-color-scheme: dark) {
    .queue-item:hover {
      background: rgba(255,255,255,0.07);
      border-color: rgba(255,255,255,0.13);
    }
  }
  @keyframes queue-in {
    from { opacity: 0; transform: translateY(4px); }
    to   { opacity: 1; transform: translateY(0); }
  }
  .queue-item-text {
    flex: 1;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    font-style: italic;
  }
  .queue-item-label {
    font-size: 10px;
    color: var(--text-tertiary);
    flex-shrink: 0;
  }
  .queue-remove {
    flex-shrink: 0;
    width: 16px; height: 16px;
    border: none;
    background: none;
    color: var(--text-tertiary);
    cursor: pointer;
    border-radius: 4px;
    display: flex; align-items: center; justify-content: center;
    padding: 0;
    transition: background 0.12s, color 0.12s;
  }
  .queue-remove:hover { background: rgba(0,0,0,0.07); color: var(--text-primary); }
  @media (prefers-color-scheme: dark) {
    .queue-remove:hover { background: rgba(255,255,255,0.10); }
  }
  /* Lock input when queue is full */
  #input-row.locked {
    opacity: 0.45;
    pointer-events: none;
    cursor: not-allowed;
  }
  #input-row.locked textarea {
    pointer-events: none;
    user-select: none;
    -webkit-user-select: none;
    cursor: not-allowed;
  }
</style>
</head>
<body>
<div id="sidebar">

  <!-- Header -->
  <div id="header">
    <div id="header-title">
      <span class="logo">🐙</span>
      <span class="name">Octopus</span>
    </div>
    <!-- Agent chip — sits between title and right controls -->
    <div id="agent-chip" title="Click to change agent">
      <svg width="9" height="9" viewBox="0 0 10 10" fill="none">
        <path d="M1 9L9 1M6 1H9V4" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/>
      </svg>
      <span id="agent-chip-label">octoweb:assistant</span>
    </div>
    <!-- Inline edit row (hidden by default, expands to fill header) -->
    <div id="agent-edit-row">
      <input id="agent-input" type="text" value="octoweb:assistant"
             placeholder="e.g. octoweb:assistant"
             autocomplete="off" spellcheck="false">
      <button id="agent-confirm-btn" title="Apply">
        <svg width="9" height="9" viewBox="0 0 9 9" fill="none">
          <path d="M1.5 4.5L3.5 6.5L7.5 2.5" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>
        </svg>
      </button>
    </div>
    <div id="header-right" style="display:flex;align-items:center;gap:8px;">
      <div id="status-pill">
        <div class="dot connecting" id="status-dot"></div>
        <span id="status-text">connecting</span>
      </div>
      <button id="close-btn" title="Close (⌘⇧A)" aria-label="Close sidebar">
        <svg width="8" height="8" viewBox="0 0 8 8" fill="none">
          <path d="M1 1L7 7M7 1L1 7" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
        </svg>
      </button>
    </div>
  </div>

  <!-- Messages -->
  <div id="messages">
    <div id="thinking"></div>
  </div>

  <!-- Input -->
  <div id="input-area">
    <div id="input-row">
      <button id="new-session-btn" title="New session" aria-label="New session">
        <svg width="13" height="13" viewBox="0 0 16 16" fill="none">
          <path d="M13.5 3.5L7 10l-2.5-.5L5 7l6.5-6.5a1.4 1.4 0 0 1 2 0v0a1.4 1.4 0 0 1 0 2z"
                stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/>
          <path d="M12 6l-2-2" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>
          <path d="M2.5 13.5h11" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>
        </svg>
      </button>
      <textarea
        id="prompt-input"
        rows="1"
        placeholder="Ask Octopus…"
        autocomplete="off"
        spellcheck="false"
      ></textarea>
      <input type="file" id="file-input-image" accept="image/*" multiple style="display:none">
      <input type="file" id="file-input-doc" accept=".pdf,.docx,.doc" multiple style="display:none">
      <div id="attach-wrap">
        <button id="attach-btn" title="Attach" aria-label="Attach">
          <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
            <path d="M6 1v10M1 6h10" stroke="currentColor" stroke-width="1.8" stroke-linecap="round"/>
          </svg>
        </button>
        <div id="attach-menu">
          <button class="attach-option" data-type="image">
            <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
              <rect x="1.5" y="1.5" width="13" height="13" rx="2" stroke="currentColor" stroke-width="1.3"/>
              <circle cx="5.5" cy="5.5" r="1.5" stroke="currentColor" stroke-width="1.2"/>
              <path d="M1.5 11l3-3 2 2 3-4 4 5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/>
            </svg>
            Image
          </button>
          <button class="attach-option" data-type="document">
            <svg width="14" height="14" viewBox="0 0 16 16" fill="none">
              <path d="M4 1.5h5.5L13 5v9a1.5 1.5 0 0 1-1.5 1.5h-7A1.5 1.5 0 0 1 3 14V3A1.5 1.5 0 0 1 4.5 1.5z" stroke="currentColor" stroke-width="1.3" stroke-linejoin="round"/>
              <path d="M9.5 1.5V5H13" stroke="currentColor" stroke-width="1.3" stroke-linecap="round" stroke-linejoin="round"/>
              <path d="M5.5 8.5h5M5.5 11h3" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/>
            </svg>
            Document
          </button>
        </div>
      </div>
      <button id="send-btn" title="Send (Return)" aria-label="Send">
        <svg class="send-icon" width="12" height="12" viewBox="0 0 12 12" fill="none">
          <path d="M6 10V2M2 6L6 2L10 6"
                stroke="currentColor" stroke-width="1.8"
                stroke-linecap="round" stroke-linejoin="round"/>
        </svg>
        <svg class="stop-icon" width="8" height="8" viewBox="0 0 8 8" fill="none">
          <rect x="1" y="1" width="6" height="6" rx="1.5" fill="currentColor"/>
        </svg>
      </button>
    </div>
    <div id="image-preview"></div>
    <div id="queue-list"></div>
    <div id="hint">Return to send · Shift+Return for newline</div>
  </div>

</div>

<!-- marked.js — lightweight MD parser, served from embedded binary -->
<script src="octoweb-lib://localhost/marked.min.js"></script>
<script>
  // Configure marked: safe defaults, no mangling
  if (typeof marked !== 'undefined') {
    marked.setOptions({ breaks: true, gfm: true });
  }

  const messages  = document.getElementById('messages');
  const thinking  = document.getElementById('thinking');
  const input     = document.getElementById('prompt-input');
  const sendBtn   = document.getElementById('send-btn');
  const newSessBtn = document.getElementById('new-session-btn');
  const dot       = document.getElementById('status-dot');
  const statusTxt = document.getElementById('status-text');

  let currentAgentBubble = null;
  let currentAgentRaw    = '';   // accumulate raw MD chunks
  let isThinking = false;

  // ── New session ─────────────────────────────────────────────────────────
  newSessBtn.addEventListener('click', () => {
    // Clear all messages but keep the thinking div reference intact
    while (messages.firstChild) {
      if (messages.firstChild === thinking) break;
      messages.removeChild(messages.firstChild);
    }
    // Remove any nodes after thinking too
    while (thinking.nextSibling) {
      messages.removeChild(thinking.nextSibling);
    }
    thinking.className = '';
    thinking.innerHTML = '';
    currentAgentBubble = null;
    currentAgentRaw = '';
    isThinking = false;
    sendBtn.className = '';
    input.value = '';
    input.style.height = 'auto';
    window.ipc.postMessage(JSON.stringify({ type: 'acp_new_session' }));
  });

  // Callable from Rust to clear messages (e.g. on agent restart)
  window.__clearMessages = function() {
    while (messages.firstChild) {
      if (messages.firstChild === thinking) break;
      messages.removeChild(messages.firstChild);
    }
    while (thinking.nextSibling) {
      messages.removeChild(thinking.nextSibling);
    }
    thinking.className = '';
    thinking.innerHTML = '';
    currentAgentBubble = null;
    currentAgentRaw = '';
  };

  // ── Status ──────────────────────────────────────────────────────────────
  window.__setConnected = function() {
    dot.className = 'dot';
    statusTxt.textContent = 'ready';
  };
  window.__setConnecting = function() {
    dot.className = 'dot connecting';
    statusTxt.textContent = 'connecting';
  };
  window.__setError = function() {
    dot.className = 'dot error';
    statusTxt.textContent = 'error';
  };

  // ── Agent chip ──────────────────────────────────────────────────────────
  const agentChip      = document.getElementById('agent-chip');
  const agentEditRow   = document.getElementById('agent-edit-row');
  const agentInput     = document.getElementById('agent-input');
  const agentChipLabel = document.getElementById('agent-chip-label');
  const agentConfirm   = document.getElementById('agent-confirm-btn');

  const header = document.getElementById('header');

  function showChip() {
    header.classList.remove('editing');
    agentEditRow.classList.remove('visible');
  }
  function showEdit() {
    header.classList.add('editing');
    agentEditRow.classList.add('visible');
    agentInput.focus();
    agentInput.select();
  }

  agentChip.addEventListener('click', showEdit);

  function applyAgent() {
    const tag = agentInput.value.trim();
    if (!tag) { showChip(); return; }
    agentChipLabel.textContent = tag;
    showChip();
    window.ipc.postMessage(JSON.stringify({ type: 'acp_set_agent', tag }));
    window.__setConnecting();
  }

  agentConfirm.addEventListener('click', applyAgent);
  agentInput.addEventListener('keydown', e => {
    if (e.key === 'Enter') { e.preventDefault(); applyAgent(); }
    if (e.key === 'Escape') { showChip(); }
  });

  // Called from Rust (overlay "Ask AI") to programmatically submit a prompt
  window.__injectPrompt = function(text) {
    input.value = text;
    input.style.height = 'auto';
    sendBtn.classList.toggle('active', true);
    send();
  };

  // Called from Rust after AcpRestart to sync the chip label
  window.__setAgentTag = function(tag) {
    agentInput.value = tag;
    agentChipLabel.textContent = tag;
  };

  // ── Markdown render helper ───────────────────────────────────────────────
  function renderMd(raw) {
    if (typeof marked === 'undefined') return escapeHtml(raw);
    return marked.parse(raw);
  }

  function escapeHtml(s) {
    return s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');
  }

  // ── Messages ────────────────────────────────────────────────────────────
  function fmtTime(d) {
    const h = d.getHours();
    const m = d.getMinutes();
    const ampm = h >= 12 ? 'PM' : 'AM';
    const h12 = h % 12 || 12;
    return h12 + ':' + String(m).padStart(2, '0') + ' ' + ampm;
  }

  function appendMessage(role, text) {
    const wrap   = document.createElement('div');
    wrap.className = 'msg ' + role;
    const label  = document.createElement('div');
    label.className = 'msg-label';
    const who = document.createElement('span');
    who.className = 'msg-who';
    who.textContent = role === 'user' ? 'You' : role === 'error' ? 'Error' : 'Octopus';
    const time = document.createElement('span');
    time.className = 'msg-time';
    time.textContent = fmtTime(new Date());
    label.appendChild(who);
    label.appendChild(time);
    const bubble = document.createElement('div');
    bubble.className = 'msg-bubble';
    bubble.textContent = text;
    wrap.appendChild(label);
    wrap.appendChild(bubble);
    messages.insertBefore(wrap, thinking);
    scrollToBottom();
    return bubble;
  }

  // Copy button — macOS frosted pill, top-right of bubble, shows on hover
  function makeCopyBtn(wrap) {
    const btn = document.createElement('button');
    btn.className = 'msg-copy';
    btn.title = 'Copy';
    // SF-style: small copy icon + label
    btn.innerHTML =
      '<svg width="10" height="10" viewBox="0 0 12 12" fill="none" style="flex-shrink:0">' +
        '<rect x="4" y="4" width="7" height="7" rx="1.5" stroke="currentColor" stroke-width="1.4"/>' +
        '<path d="M3 8H2a1 1 0 0 1-1-1V2a1 1 0 0 1 1-1h5a1 1 0 0 1 1 1v1" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>' +
      '</svg>' +
      '<span>Copy</span>';
    btn.addEventListener('click', () => {
      const raw = wrap.dataset.raw || wrap.querySelector('.msg-bubble')?.textContent || '';
      // navigator.clipboard requires secure context — unavailable in
      // child WKWebView loaded via with_html().  Fall back to IPC so
      // Rust writes to NSPasteboard directly.
      window.ipc.postMessage(JSON.stringify({ type: 'copy_text', text: raw }));
      btn.classList.add('copied');
      btn.innerHTML =
        '<svg width="10" height="10" viewBox="0 0 12 12" fill="none" style="flex-shrink:0">' +
          '<path d="M2 6l3 3 5-5" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" stroke-linejoin="round"/>' +
        '</svg>' +
        '<span>Copied</span>';
      setTimeout(() => {
        btn.classList.remove('copied');
        btn.innerHTML =
          '<svg width="10" height="10" viewBox="0 0 12 12" fill="none" style="flex-shrink:0">' +
            '<rect x="4" y="4" width="7" height="7" rx="1.5" stroke="currentColor" stroke-width="1.4"/>' +
            '<path d="M3 8H2a1 1 0 0 1-1-1V2a1 1 0 0 1 1-1h5a1 1 0 0 1 1 1v1" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>' +
          '</svg>' +
          '<span>Copy</span>';
      }, 1800);
    });
    return btn;
  }

  // Called once Done arrives — stores raw, appends copy btn inside bubble, collapses if tall
  function finishAgentBubble(bubble, rawText, toolCount) {
    const wrap = bubble.closest('.msg');
    wrap.dataset.raw = rawText;

    // Show tool count in header if any tools were used
    if (toolCount > 0) {
      const toolsEl = wrap.querySelector('.msg-tools');
      if (toolsEl) {
        toolsEl.textContent = '· ' + toolCount + ' tools';
        toolsEl.style.display = 'inline';
      }
    }

    // Copy button sits inside the bubble, bottom-right corner
    bubble.appendChild(makeCopyBtn(wrap));

    requestAnimationFrame(() => {
      if (bubble.scrollHeight > 240) {
        bubble.classList.add('collapsed');
        const btn = document.createElement('button');
        btn.className = 'msg-show-more';
        btn.textContent = 'Show more';
        btn.addEventListener('click', () => {
          bubble.classList.remove('collapsed');
          btn.remove();
        });
        wrap.appendChild(btn);
      }
    });
  }

  function startAgentBubble() {
    const wrap   = document.createElement('div');
    wrap.className = 'msg agent';
    const label  = document.createElement('div');
    label.className = 'msg-label';
    const who = document.createElement('span');
    who.className = 'msg-who';
    who.textContent = 'Octopus';
    const time = document.createElement('span');
    time.className = 'msg-time';
    time.textContent = fmtTime(new Date());
    // Placeholder for tool count (populated on finish)
    const tools = document.createElement('span');
    tools.className = 'msg-tools';
    tools.style.display = 'none';
    label.appendChild(who);
    label.appendChild(time);
    label.appendChild(tools);
    const bubble = document.createElement('div');
    bubble.className = 'msg-bubble';
    wrap.appendChild(label);
    wrap.appendChild(bubble);
    messages.insertBefore(wrap, thinking);
    return bubble;
  }

  // Chunks arrive as raw markdown — accumulate and re-render on each chunk.
  // Re-rendering is cheap for typical response sizes and gives correct output
  // (partial markdown like unclosed ``` would render wrong if we appended HTML).
  window.__appendChunk = function(text) {
    if (!currentAgentBubble) {
      currentAgentBubble = startAgentBubble();
      currentAgentRaw    = '';
      // Hide activity feed without triggering finishAgentBubble
      isThinking = false;
      thinking.className = '';
      clearActivity();
    }
    currentAgentRaw += text;
    currentAgentBubble.innerHTML = renderMd(currentAgentRaw);
    scrollToBottom();
  };

  window.__appendImage = function(mimeType, b64data) {
    if (!currentAgentBubble) {
      currentAgentBubble = startAgentBubble();
      currentAgentRaw    = '';
      isThinking = false;
      thinking.className = '';
      clearActivity();
    }
    const img = document.createElement('img');
    img.className = 'chat-img';
    img.src = 'data:' + mimeType + ';base64,' + b64data;
    currentAgentBubble.appendChild(img);
    scrollToBottom();
  };

  // ── Activity feed state ──────────────────────────────────────────────────────────────
  let activityStart = 0;       // Date.now() when thinking began
  let activityTimer = null;    // setInterval id for elapsed display
  const toolRows = {};         // id → { el, startTime, timerEl }
  let toolCount = 0;           // total tools used in current response

  const kindLabel = { read:'R', edit:'E', delete:'D', search:'S', execute:'X', think:'T', fetch:'F', move:'M', other:'·' };

  function fmtElapsed(ms) {
    const s = Math.floor(ms / 1000);
    if (s < 60) return s + 's';
    return Math.floor(s / 60) + 'm ' + (s % 60) + 's';
  }

  function clearActivity() {
    if (activityTimer) { clearInterval(activityTimer); activityTimer = null; }
    thinking.innerHTML = '';
    for (const k in toolRows) delete toolRows[k];
  }

  function tickActivity() {
    const hdr = thinking.querySelector('.activity-elapsed');
    if (hdr) hdr.textContent = fmtElapsed(Date.now() - activityStart);
    // Update per-tool timers for in-progress tools
    for (const id in toolRows) {
      const t = toolRows[id];
      if (t.timerEl && !t.finished) {
        t.timerEl.textContent = fmtElapsed(Date.now() - t.startTime);
      }
    }
  }

  window.__toolStart = function(id, title, kind) {
    toolCount++;
    // Create row
    const row = document.createElement('div');
    row.className = 'tool-row';

    const icon = document.createElement('span');
    icon.className = 'tool-kind ' + kind;
    icon.textContent = kindLabel[kind] || '·';

    const ttl = document.createElement('span');
    ttl.className = 'tool-title';
    ttl.textContent = title;

    const tm = document.createElement('span');
    tm.className = 'tool-time';
    tm.textContent = '0s';

    row.appendChild(icon);
    row.appendChild(ttl);
    row.appendChild(tm);
    thinking.appendChild(row);

    toolRows[id] = { el: row, startTime: Date.now(), timerEl: tm, finished: false };
    scrollToBottom();
  };

  window.__toolUpdate = function(id, title, status) {
    const t = toolRows[id];
    if (!t) return;
    if (title) t.el.querySelector('.tool-title').textContent = title;
    if (status === 'completed') {
      t.finished = true;
      t.el.classList.add('done');
      t.timerEl.textContent = fmtElapsed(Date.now() - t.startTime);
      // Replace timer with checkmark
      const check = document.createElement('span');
      check.className = 'tool-check';
      check.textContent = '✓';
      t.timerEl.replaceWith(check);
    } else if (status === 'failed') {
      t.finished = true;
      t.el.classList.add('failed');
      const fail = document.createElement('span');
      fail.className = 'tool-fail';
      fail.textContent = '✗';
      t.timerEl.replaceWith(fail);
    }
  };

  window.__setThinking = function(on) {
    isThinking = on;
    thinking.className = on ? 'visible' : '';
    // Toggle send button between send and stop modes
    sendBtn.classList.toggle('stop-mode', on);
    sendBtn.title = on ? 'Stop' : 'Send (Return)';
    if (on) {
      currentAgentBubble = null;
      currentAgentRaw = '';
      toolCount = 0;
      clearActivity();
      activityStart = Date.now();
      // Header with 3-dot bounce + elapsed
      const hdr = document.createElement('div');
      hdr.className = 'activity-header';
      hdr.innerHTML = '<span class="activity-dots"><span></span><span></span><span></span></span><span class="activity-elapsed">0s</span>';
      thinking.appendChild(hdr);
      activityTimer = setInterval(tickActivity, 1000);
      scrollToBottom();
    } else {
      // Save tool count before clearing
      const savedToolCount = toolCount;
      clearActivity();
      if (currentAgentBubble) {
        // Done — finalize: store raw for copy, add copy btn, collapse if tall
        finishAgentBubble(currentAgentBubble, currentAgentRaw, savedToolCount);
        currentAgentBubble = null;
        currentAgentRaw = '';
      }
    }
  };

  window.__appendError = function(text) {
    window.__setThinking(false);
    appendMessage('error', text);
  };

  function scrollToBottom() { messages.scrollTop = messages.scrollHeight; }

  // ── Queue (max 2 pending messages) ──────────────────────────────────────
  const MAX_QUEUE = 2;
  const msgQueue  = [];   // pending texts waiting to be sent
  const queueList = document.getElementById('queue-list');
  const inputRow  = document.getElementById('input-row');

  function renderQueue() {
    queueList.innerHTML = '';
    msgQueue.forEach((entry, i) => {
      const item = document.createElement('div');
      item.className = 'queue-item';

      const lbl = document.createElement('span');
      lbl.className = 'queue-item-label';
      lbl.textContent = `#${i + 1}`;

      const txt = document.createElement('span');
      txt.className = 'queue-item-text';
      txt.textContent = entry.text + (entry.images.length ? ` [${entry.images.length} img]` : '');

      const rm = document.createElement('button');
      rm.className = 'queue-remove';
      rm.title = 'Remove';
      rm.innerHTML = '<svg width="8" height="8" viewBox="0 0 8 8" fill="none"><path d="M1 1L7 7M7 1L1 7" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>';
      rm.addEventListener('click', () => {
        msgQueue.splice(i, 1);
        renderQueue();
        updateInputLock();
      });

      item.appendChild(lbl);
      item.appendChild(txt);
      item.appendChild(rm);
      queueList.appendChild(item);
    });
  }

  function updateInputLock() {
    const lock = msgQueue.length >= MAX_QUEUE;
    inputRow.classList.toggle('locked', lock);
    input.disabled = lock;
  }

  // Called when agent finishes — drain queue one item at a time
  function drainQueue() {
    if (msgQueue.length === 0) return;
    const next = msgQueue.shift();
    renderQueue();
    updateInputLock();
    dispatchPrompt(next.text, next.images, next.docs);
  }

  function dispatchPrompt(text, images, docs) {
    // Show user's typed text (without the <doc> prefix) in the bubble
    const displayText = text.replace(/<doc filename="[^"]*">[\s\S]*?<\/doc>\s*/g, '').trim();
    const bubble = appendMessage('user', displayText || '(document attached)');
    // Show doc chips in user bubble
    if (docs && docs.length) {
      for (const doc of docs) {
        const chip = document.createElement('div');
        chip.className = 'doc-chip';
        chip.style.marginTop = '4px';
        chip.innerHTML = '<svg width="10" height="10" viewBox="0 0 16 16" fill="none"><path d="M4 1.5h5.5L13 5v9a1.5 1.5 0 0 1-1.5 1.5h-7A1.5 1.5 0 0 1 3 14V3A1.5 1.5 0 0 1 4.5 1.5z" stroke="currentColor" stroke-width="1.3"/></svg>';
        const span = document.createElement('span');
        span.textContent = doc.name;
        chip.appendChild(span);
        bubble.appendChild(chip);
      }
    }
    // Show attached images in user bubble
    if (images && images.length) {
      for (const img of images) {
        const el = document.createElement('img');
        el.className = 'chat-img';
        el.src = 'data:' + img.mimeType + ';base64,' + img.data;
        bubble.appendChild(el);
      }
    }
    window.__setThinking(true);
    window.ipc.postMessage(JSON.stringify({ type: 'acp_prompt', text, images: images || [] }));
  }

  // ── Image attachments ──────────────────────────────────────────────────
  let pendingImages = []; // [{data: base64, mimeType: string}]
  const imagePreview = document.getElementById('image-preview');
  const fileInputImage = document.getElementById('file-input-image');
  const fileInputDoc = document.getElementById('file-input-doc');
  const attachBtn = document.getElementById('attach-btn');
  const attachMenu = document.getElementById('attach-menu');

  function addImageFromFile(file) {
    if (!file || !file.type.startsWith('image/')) return;
    const reader = new FileReader();
    reader.onload = () => {
      const b64 = reader.result.split(',')[1];
      pendingImages.push({ data: b64, mimeType: file.type });
      renderImagePreview();
      updateSendBtn();
    };
    reader.readAsDataURL(file);
  }

  function renderImagePreview() {
    imagePreview.innerHTML = '';
    if (!pendingImages.length) {
      imagePreview.classList.remove('visible');
      return;
    }
    imagePreview.classList.add('visible');
    pendingImages.forEach((img, i) => {
      const thumb = document.createElement('div');
      thumb.className = 'img-thumb';
      const el = document.createElement('img');
      el.src = 'data:' + img.mimeType + ';base64,' + img.data;
      thumb.appendChild(el);
      const rm = document.createElement('button');
      rm.className = 'rm';
      rm.textContent = '×';
      rm.onclick = () => { pendingImages.splice(i, 1); renderImagePreview(); updateSendBtn(); };
      thumb.appendChild(rm);
      imagePreview.appendChild(thumb);
    });
  }

  function updateSendBtn() {
    sendBtn.classList.toggle('active', input.value.trim().length > 0 || pendingImages.length > 0 || pendingDocs.length > 0);
  }

  attachBtn.addEventListener('click', (e) => {
    e.stopPropagation();
    attachMenu.classList.toggle('visible');
  });
  document.addEventListener('click', () => attachMenu.classList.remove('visible'));
  attachMenu.addEventListener('click', (e) => e.stopPropagation());

  document.querySelector('.attach-option[data-type="image"]').addEventListener('click', () => {
    attachMenu.classList.remove('visible');
    fileInputImage.click();
  });
  document.querySelector('.attach-option[data-type="document"]').addEventListener('click', () => {
    attachMenu.classList.remove('visible');
    fileInputDoc.click();
  });

  fileInputImage.addEventListener('change', () => {
    for (const f of fileInputImage.files) addImageFromFile(f);
    fileInputImage.value = '';
  });
  // ── Document attachments ──────────────────────────────────────────────
  let pendingDocs = []; // [{file: File, name: string}]
  let docLibsLoaded = false;

  async function ensureDocLibs() {
    if (docLibsLoaded) return;
    // Load scripts sequentially — each sets a global (pdfjsLib, mammoth)
    async function loadScript(url) {
      return new Promise((resolve, reject) => {
        const s = document.createElement('script');
        s.src = url;
        s.onload = resolve;
        s.onerror = reject;
        document.head.appendChild(s);
      });
    }
    // pdf.js v3 UMD — sets globalThis.pdfjsLib
    await loadScript('octoweb-lib://localhost/pdf.min.js');
    // Worker: blob URL so pdf.js can load it without network access
    const workerSrc = await fetch('octoweb-lib://localhost/pdf.worker.min.js').then(r => r.text());
    const workerBlob = new Blob([workerSrc], { type: 'application/javascript' });
    pdfjsLib.GlobalWorkerOptions.workerSrc = URL.createObjectURL(workerBlob);
    // mammoth (DOCX) — sets window.mammoth
    await loadScript('octoweb-lib://localhost/mammoth.browser.min.js');
    docLibsLoaded = true;
  }

  async function extractDocText(file) {
    await ensureDocLibs();
    const buf = await file.arrayBuffer();
    const name = file.name.toLowerCase();
    if (name.endsWith('.pdf')) {
      const pdf = await window.pdfjsLib.getDocument({ data: new Uint8Array(buf) }).promise;
      let text = '';
      for (let i = 1; i <= pdf.numPages; i++) {
        const page = await pdf.getPage(i);
        const content = await page.getTextContent();
        text += content.items.map(item => item.str).join(' ') + '\n';
      }
      return text.trim();
    } else {
      const result = await mammoth.extractRawText({ arrayBuffer: buf });
      return result.value.trim();
    }
  }

  function renderPreview() {
    renderImagePreview();
    // Append doc chips
    for (let i = 0; i < pendingDocs.length; i++) {
      const chip = document.createElement('div');
      chip.className = 'doc-chip';
      chip.innerHTML = '<svg width="10" height="10" viewBox="0 0 16 16" fill="none"><path d="M4 1.5h5.5L13 5v9a1.5 1.5 0 0 1-1.5 1.5h-7A1.5 1.5 0 0 1 3 14V3A1.5 1.5 0 0 1 4.5 1.5z" stroke="currentColor" stroke-width="1.3"/></svg>';
      const span = document.createElement('span');
      span.textContent = pendingDocs[i].name;
      chip.appendChild(span);
      const rm = document.createElement('button');
      rm.className = 'rm';
      rm.textContent = '×';
      rm.onclick = () => { pendingDocs.splice(i, 1); renderPreview(); updateSendBtn(); };
      chip.appendChild(rm);
      imagePreview.appendChild(chip);
    }
    imagePreview.classList.toggle('visible', pendingImages.length > 0 || pendingDocs.length > 0);
  }

  fileInputDoc.addEventListener('change', () => {
    for (const f of fileInputDoc.files) {
      pendingDocs.push({ file: f, name: f.name });
    }
    fileInputDoc.value = '';
    renderPreview();
    updateSendBtn();
  });

  input.addEventListener('paste', e => {
    const items = e.clipboardData && e.clipboardData.items;
    if (!items) return;
    for (const item of items) {
      if (item.type.startsWith('image/')) {
        e.preventDefault();
        addImageFromFile(item.getAsFile());
        return;
      }
    }
  });

  // ── Send ────────────────────────────────────────────────────────────────
  async function send() {
    const text = input.value.trim();
    const images = pendingImages.slice();
    const docs = pendingDocs.slice();
    if (!text && !images.length && !docs.length) return;
    input.value = '';
    input.style.height = 'auto';
    pendingImages = [];
    pendingDocs = [];
    renderPreview();
    sendBtn.classList.remove('active');

    // Extract doc text (lazy-loads libs on first use)
    let docPrefix = '';
    if (docs.length) {
      for (const doc of docs) {
        try {
          const extracted = await extractDocText(doc.file);
          if (extracted) {
            docPrefix += '<doc filename="' + doc.name + '">\n' + extracted + '\n</doc>\n\n';
          } else {
            docPrefix += '<doc filename="' + doc.name + '">\n[Document was empty or could not be parsed]\n</doc>\n\n';
          }
        } catch (e) {
          const msg = e && (e.message || e.toString()) || 'unknown error';
          console.error('Doc extraction failed:', e);
          docPrefix += '<doc filename="' + doc.name + '">\n[Failed to extract: ' + msg + ']\n</doc>\n\n';
        }
      }
    }
    const fullText = docPrefix + text;

    if (!isThinking) {
      dispatchPrompt(fullText, images, docs);
    } else if (msgQueue.length < MAX_QUEUE) {
      msgQueue.push({ text: fullText, images, docs });
      renderQueue();
      updateInputLock();
    }
  }

  // Stop button — cancel current prompt
  function stop() {
    window.ipc.postMessage(JSON.stringify({ type: 'acp_cancel' }));
    // Clear activity feed immediately
    window.__setThinking(false);
  }

  // Click handler: send or stop depending on mode
  sendBtn.addEventListener('click', () => {
    if (sendBtn.classList.contains('stop-mode')) {
      stop();
    } else {
      send();
    }
  });
  input.addEventListener('keydown', e => {
    if (e.key === 'Enter' && !e.shiftKey) { e.preventDefault(); send(); }
  });
  input.addEventListener('input', () => {
    input.style.height = 'auto';
    input.style.height = Math.min(input.scrollHeight, 120) + 'px';
    updateSendBtn();
  });

  // Hook into __setThinking to drain queue when agent becomes free
  const _origSetThinking = window.__setThinking;
  window.__setThinking = function(on) {
    _origSetThinking(on);
    if (!on) {
      // Small delay so the bubble finishes rendering before next prompt
      setTimeout(drainQueue, 80);
    }
  };

  // ── Close ────────────────────────────────────────────────────────────────
  document.getElementById('close-btn').addEventListener('click', () => {
    window.ipc.postMessage(JSON.stringify({ type: 'sidebar_close' }));
  });
</script>
</body>
</html>"#
}
