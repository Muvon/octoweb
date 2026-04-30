/// Returns the full HTML for the ACP agent sidebar panel.
///
/// Multi-session UI: a strip of session tabs lives in the header. Each tab owns its
/// own message-log container kept in a JS Map (`sessionContainers`); only the active
/// session's container is mounted in `#messages`, the rest are detached fragments.
///
/// JS API (called from Rust via `evaluate_script` — all per-session APIs take a
/// numeric `sid` matching the Rust-side `AcpSession::id`):
///   window.__addSession(sid, title, tag, status)  — mount a new session tab
///   window.__removeSession(sid)                   — unmount a session tab
///   window.__renameSession(sid, title)            — update title text
///   window.__updateSessionTag(sid, tag)           — update agent tag (for set-agent)
///   window.__switchSession(sid)                   — swap active session
///   window.__setSessionStatus(sid, st)            — 'ready'|'connecting'|'thinking'|'error'
///   window.__appendChunk(sid, text)               — append streaming MD chunk
///   window.__appendImage(sid, mime, b64)          — append image to current bubble
///   window.__toolStart(sid, id, title, kind, ri, locs) — start tool row
///   window.__toolUpdate(sid, id, title, status, ro)    — update tool row
///   window.__setThinking(sid, bool)               — show/hide activity feed
///   window.__appendError(sid, text)               — show an error bubble
///   window.__setAvailableCommands(sid, json)      — populate slash-command list
///
/// IPC messages sent to Rust (all session-scoped messages include `session_id`):
///   { type: "acp_prompt",    session_id, text, images }
///   { type: "acp_cancel",    session_id }
///   { type: "acp_set_agent", session_id, tag }
///   { type: "acp_clear_session", session_id }
///   { type: "acp_session_create", title, tag }
///   { type: "acp_session_close",  session_id }
///   { type: "acp_session_switch", session_id }
///   { type: "acp_session_rename", session_id, title }
///   { type: "sidebar_close" }
pub fn html() -> String {
    let prompt_history_js = crate::prompt_history_js::prompt_history_js();
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
    gap: 8px;
    height: 36px;
    padding: 0 8px 0 10px;
    border-bottom: 1px solid var(--divider);
    flex-shrink: 0;
    min-width: 0;
  }

  #header-logo {
    font-size: 16px;
    line-height: 1;
    filter: drop-shadow(0 1px 2px rgba(0,0,0,0.15));
    flex-shrink: 0;
    cursor: default;
  }

  /* ── Session tabs strip ─────────────────────────────────────────────────── */
  #session-strip {
    display: flex;
    align-items: center;
    gap: 4px;
    flex: 1;
    min-width: 0;
    overflow-x: auto;
    overflow-y: hidden;
    scrollbar-width: none;
  }
  #session-strip::-webkit-scrollbar { display: none; }

  .session-tab {
    display: flex;
    align-items: center;
    gap: 5px;
    padding: 3px 7px 3px 8px;
    border-radius: 14px;
    background: var(--agent-bg);
    border: 1px solid var(--agent-border);
    font-size: 11px;
    color: var(--text-secondary);
    cursor: pointer;
    transition: background 0.15s, border-color 0.15s, color 0.15s;
    flex-shrink: 0;
    max-width: 160px;
    min-width: 0;
    user-select: none;
    -webkit-user-select: none;
  }
  .session-tab:hover {
    background: rgba(0,0,0,0.06);
    border-color: rgba(0,0,0,0.13);
    color: var(--text-primary);
  }
  @media (prefers-color-scheme: dark) {
    .session-tab:hover {
      background: rgba(255,255,255,0.10);
      border-color: rgba(255,255,255,0.16);
    }
  }
  .session-tab.active {
    background: var(--accent);
    border-color: var(--accent);
    color: #fff;
  }
  .session-tab.active:hover {
    background: var(--accent-hover);
    border-color: var(--accent-hover);
    color: #fff;
  }

  .session-tab .session-dot {
    width: 6px; height: 6px;
    border-radius: 50%;
    background: var(--dot-ok);
    box-shadow: 0 0 0 2px rgba(40, 205, 65, 0.20);
    flex-shrink: 0;
    transition: background 0.3s, box-shadow 0.3s;
  }
  .session-tab .session-dot.connecting {
    background: var(--dot-wait);
    box-shadow: 0 0 0 2px rgba(255, 149, 0, 0.20);
    animation: pulse 1.4s ease-in-out infinite;
  }
  .session-tab .session-dot.thinking {
    background: var(--accent);
    box-shadow: 0 0 0 2px rgba(0, 122, 255, 0.20);
    animation: pulse 1.4s ease-in-out infinite;
  }
  .session-tab .session-dot.error {
    background: var(--dot-err);
    box-shadow: 0 0 0 2px rgba(255, 59, 48, 0.20);
  }
  .session-tab.active .session-dot {
    box-shadow: 0 0 0 2px rgba(255,255,255,0.35);
  }

  .session-tab .session-title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-weight: 500;
    min-width: 0;
  }
  /* Inline rename input replaces the title span when editing */
  .session-tab .session-rename {
    background: rgba(255,255,255,0.85);
    border: 1px solid rgba(0,0,0,0.18);
    border-radius: 6px;
    padding: 1px 4px;
    font-size: 11px;
    font-weight: 500;
    color: var(--text-primary);
    outline: none;
    width: 90px;
    min-width: 0;
    font-family: inherit;
  }
  .session-tab.active .session-rename {
    background: rgba(255,255,255,0.95);
    color: var(--text-primary);
  }

  .session-tab .session-close {
    width: 14px; height: 14px;
    border-radius: 50%;
    border: none;
    background: transparent;
    color: inherit;
    cursor: pointer;
    display: none;
    align-items: center;
    justify-content: center;
    padding: 0;
    flex-shrink: 0;
    opacity: 0.65;
    transition: background 0.12s, opacity 0.12s;
  }
  .session-tab:hover .session-close { display: flex; }
  .session-tab .session-close:hover {
    background: rgba(0,0,0,0.13);
    opacity: 1;
  }
  .session-tab.active .session-close:hover {
    background: rgba(255,255,255,0.25);
  }

  /* + button — add session */
  #session-add-btn {
    width: 22px; height: 22px;
    border-radius: 50%;
    border: 1px solid var(--agent-border);
    background: var(--agent-bg);
    color: var(--text-secondary);
    cursor: pointer;
    display: flex; align-items: center; justify-content: center;
    flex-shrink: 0;
    transition: background 0.15s, color 0.15s, border-color 0.15s;
  }
  #session-add-btn:hover:not(:disabled) {
    background: rgba(0,0,0,0.07);
    color: var(--text-primary);
    border-color: rgba(0,0,0,0.13);
  }
  @media (prefers-color-scheme: dark) {
    #session-add-btn:hover:not(:disabled) {
      background: rgba(255,255,255,0.10);
      border-color: rgba(255,255,255,0.16);
    }
  }
  #session-add-btn:disabled {
    opacity: 0.35;
    cursor: not-allowed;
  }
  #session-add-btn:active:not(:disabled) { transform: scale(0.92); }

  /* ── Create-session inline panel (drops down from header) ───────────────── */
  #session-create-panel {
    display: none;
    align-items: center;
    gap: 6px;
    padding: 6px 10px;
    border-bottom: 1px solid var(--divider);
    background: var(--glass-inner);
    flex-shrink: 0;
  }
  #session-create-panel.visible { display: flex; }
  #session-create-panel input {
    background: var(--input-bg);
    border: 1px solid var(--input-border);
    border-radius: 8px;
    padding: 4px 8px;
    font-size: 11px;
    font-family: ui-monospace, SFMono-Regular, Menlo, monospace;
    color: var(--text-primary);
    outline: none;
    min-width: 0;
  }
  #session-create-panel input:focus { border-color: var(--input-focus-border); }
  #session-create-panel #sc-title  { flex: 0 0 110px; }
  #session-create-panel #sc-tag    { flex: 1 1 auto; }
  #session-create-panel button {
    border: none;
    background: var(--accent);
    color: #fff;
    cursor: pointer;
    border-radius: 8px;
    padding: 4px 10px;
    font-size: 11px;
    font-weight: 500;
    transition: background 0.15s;
    flex-shrink: 0;
  }
  #session-create-panel button:hover { background: var(--accent-hover); }
  #session-create-panel button.secondary {
    background: transparent;
    color: var(--text-secondary);
  }
  #session-create-panel button.secondary:hover {
    background: rgba(0,0,0,0.06);
    color: var(--text-primary);
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

  /* ── Welcome screen (empty session) ─────────────────────────────────────── */
  #welcome {
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    flex: 1;
    padding: 28px 20px 20px;
    text-align: center;
    gap: 6px;
    opacity: 1;
    transition: opacity 0.25s ease;
  }
  #welcome.hidden { opacity: 0; pointer-events: none; position: absolute; }
  #welcome-icon { margin-bottom: 6px; opacity: 0.6; }
  #welcome-title {
    font-size: 17px;
    font-weight: 600;
    color: var(--text-primary);
    margin-bottom: 2px;
  }
  #welcome-desc {
    font-size: 12px;
    line-height: 1.5;
    color: var(--text-secondary);
    max-width: 250px;
    margin-bottom: 14px;
  }
  #welcome-suggestions {
    display: flex;
    flex-direction: column;
    gap: 6px;
    width: 100%;
    max-width: 260px;
    margin-bottom: 18px;
  }
  .suggestion-btn {
    display: block;
    width: 100%;
    padding: 8px 12px;
    border: 1px solid var(--input-border);
    border-radius: 8px;
    background: var(--input-bg);
    color: var(--text-secondary);
    font-size: 12px;
    line-height: 1.4;
    text-align: left;
    cursor: pointer;
    transition: border-color 0.15s, background 0.15s;
  }
  .suggestion-btn:hover {
    border-color: var(--accent);
    background: rgba(0, 122, 255, 0.06);
    color: var(--text-primary);
  }
   #welcome-shortcuts {
    display: flex;
    flex-direction: column;
    gap: 4px;
    font-size: 11px;
    color: var(--text-tertiary);
  }
  .shortcut-row {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .shortcut-row kbd {
    display: inline-block;
    min-width: 42px;
    padding: 1px 6px;
    border-radius: 4px;
    background: var(--md-code-bg);
    border: 1px solid var(--md-code-border);
    font-family: inherit;
    font-size: 10px;
    text-align: center;
    color: var(--text-secondary);
  }
  .shortcut-row span { color: var(--text-tertiary); }

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
  .msg-label .msg-sep {
    color: var(--text-tertiary);
    opacity: 0.5;
    margin: 0 2px;
  }
  .msg-label .msg-tools {
    font-size: 9.5px;
    font-weight: 400;
    letter-spacing: 0.01em;
    color: var(--text-tertiary);
    opacity: 0.5;
    cursor: pointer;
    transition: opacity 0.15s;
  }
  .msg-label .msg-tools:hover {
    opacity: 0.8;
    text-decoration: underline;
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

  /* Code block wrapper (custom renderer) */
  .msg.agent .msg-bubble .code-block {
    position: relative;
    margin: 6px 0 8px;
  }
  .msg.agent .msg-bubble .code-block .code-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    padding: 3px 6px 3px 12px;
    background: var(--md-code-border);
    border-radius: 8px 8px 0 0;
    border: 1px solid var(--md-code-border);
    border-bottom: none;
    min-height: 22px;
  }
  .code-lang {
    font-size: 10px;
    font-weight: 600;
    color: var(--text-secondary);
    text-transform: uppercase;
    letter-spacing: 0.03em;
  }
  .code-copy {
    display: flex;
    align-items: center;
    gap: 3px;
    margin-left: auto;
    padding: 2px 6px;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: var(--text-tertiary);
    font-size: 10px;
    font-weight: 500;
    cursor: pointer;
    opacity: 0;
    transition: opacity 0.15s, color 0.12s, background 0.12s;
  }
  .code-block:hover .code-copy { opacity: 1; }
  .code-copy:hover { background: var(--md-code-bg); color: var(--text-primary); }
  .code-copy.copied { color: #34c759; opacity: 1; }
  /* pre inside code-block: top corners handled by header */
  .msg.agent .msg-bubble .code-block pre {
    margin: 0;
    border-radius: 0 0 8px 8px;
    border-top: none;
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
    position: relative;
  }

  /* ── Slash command dropdown ─────────────────────────────────────────── */
  #cmd-dropdown {
    display: none;
    position: absolute;
    bottom: 100%;
    left: 4px; right: 4px;
    margin-bottom: 2px;
    max-height: 200px;
    overflow-y: auto;
    background: var(--glass-solid);
    border: 1px solid var(--glass-border);
    border-radius: 10px;
    box-shadow: var(--glass-shadow);
    z-index: 10;
    padding: 4px 0;
  }
  #cmd-dropdown.visible { display: block; }
  #cmd-dropdown::-webkit-scrollbar { width: 3px; }
  #cmd-dropdown::-webkit-scrollbar-thumb { background: var(--scrollbar); border-radius: 2px; }

  .cmd-item {
    padding: 6px 10px;
    cursor: pointer;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .cmd-item.active {
    background: var(--accent);
    color: #fff;
  }
  .cmd-item:hover:not(.active) {
    background: var(--input-border);
  }
  .cmd-name {
    font-size: 13px;
    font-weight: 600;
    letter-spacing: -0.01em;
  }
  .cmd-item.active .cmd-name { color: #fff; }
  .cmd-desc {
    font-size: 11px;
    color: var(--text-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .cmd-item.active .cmd-desc { color: rgba(255,255,255,0.75); }

  /* ── Command output card ──────────────────────────────────────────── */
  .cmd-output {
    font-size: 12px;
    line-height: 1.5;
  }
  .cmd-output-header {
    font-size: 11px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--accent);
    margin-bottom: 6px;
  }
  .cmd-output-table {
    width: 100%;
    border-collapse: collapse;
  }
  .cmd-output-table tr {
    border-bottom: 1px solid var(--divider);
  }
  .cmd-output-table tr:last-child {
    border-bottom: none;
  }
  .cmd-output-key {
    padding: 3px 8px 3px 0;
    color: var(--text-secondary);
    font-size: 11px;
    white-space: nowrap;
    vertical-align: top;
    width: 1%;
  }
  .cmd-output-val {
    padding: 3px 0;
    color: var(--text-primary);
    font-size: 12px;
    word-break: break-word;
  }
  .cmd-output-val.null { color: var(--text-tertiary); font-style: italic; }
  .cmd-output-val.bool-true { color: #34c759; }
  .cmd-output-val.bool-false { color: var(--text-tertiary); }
  .cmd-output-val.number { font-variant-numeric: tabular-nums; }
  .cmd-output-nested {
    margin: 2px 0;
    padding: 4px 0 4px 8px;
    border-left: 2px solid var(--divider);
    font-size: 11px;
  }
  .cmd-output-list {
    margin: 0;
    padding-left: 16px;
  }
  .cmd-output-list li {
    margin: 1px 0;
    font-size: 12px;
  }
  .cmd-output-single-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }
  .cmd-output-list-label {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-secondary);
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

  #prompt-ghost-wrap {
    position: relative;
    flex: 1;
    min-height: 20px;
  }
  #prompt-ghost-wrap #prompt-input {
    position: relative;
    z-index: 1;
    width: 100%;
    background: transparent;
  }
  #prompt-ghost {
    position: absolute;
    top: 0; left: 0; right: 0;
    min-height: 20px;
    font-family: inherit;
    font-size: 13px;
    line-height: 1.5;
    color: var(--text-tertiary);
    pointer-events: none;
    white-space: pre-wrap;
    word-wrap: break-word;
    overflow: hidden;
    z-index: 0;
    opacity: 0.5;
    max-height: 1.5em;
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

  /* Tool details modal */
  #tool-modal {
    position: fixed;
    inset: 0;
    background: rgba(0,0,0,0.5);
    backdrop-filter: blur(4px);
    z-index: 1000;
    display: none;
    align-items: center;
    justify-content: center;
    animation: modal-fade-in 0.15s ease;
  }
  #tool-modal.show { display: flex; }
  @keyframes modal-fade-in {
    from { opacity: 0; }
    to { opacity: 1; }
  }
  #tool-modal .modal-content {
    background: var(--glass-solid);
    border-radius: 12px;
    width: 90%;
    max-width: 700px;
    max-height: 85vh;
    display: flex;
    flex-direction: column;
    box-shadow: 0 20px 60px rgba(0,0,0,0.3);
    animation: modal-slide-up 0.2s ease;
  }
  @keyframes modal-slide-up {
    from { transform: translateY(20px); opacity: 0; }
    to { transform: translateY(0); opacity: 1; }
  }
  #tool-modal .modal-header {
    padding: 16px 20px;
    border-bottom: 1px solid var(--border);
    display: flex;
    align-items: center;
    justify-content: space-between;
    flex-shrink: 0;
  }
  #tool-modal .modal-title {
    font-size: 15px;
    font-weight: 600;
    color: var(--text-primary);
  }
  #tool-modal .modal-close {
    width: 28px; height: 28px;
    border-radius: 50%;
    border: none;
    background: transparent;
    color: var(--text-tertiary);
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 18px;
    transition: background 0.15s, color 0.15s;
  }
  #tool-modal .modal-close:hover {
    background: var(--hover-bg);
    color: var(--text-primary);
  }
  #tool-modal .modal-body {
    padding: 12px 20px 20px;
    overflow-y: auto;
    flex: 1;
  }
  #tool-modal .modal-tools-list {
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  #tool-modal .modal-tool-row {
    background: var(--hover-bg);
    border-radius: 8px;
    overflow: hidden;
  }
  #tool-modal .modal-tool-header {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 10px 12px;
    cursor: pointer;
    transition: background 0.15s;
  }
  #tool-modal .modal-tool-header:hover {
    background: rgba(0,0,0,0.04);
  }
  @media (prefers-color-scheme: dark) {
    #tool-modal .modal-tool-header:hover {
      background: rgba(255,255,255,0.06);
    }
  }
  #tool-modal .modal-tool-kind {
    width: 22px; height: 22px;
    border-radius: 5px;
    display: flex;
    align-items: center;
    justify-content: center;
    font-size: 11px;
    font-weight: 600;
    flex-shrink: 0;
  }
  #tool-modal .modal-tool-kind.read     { background: #34c759; color: #fff; }
  #tool-modal .modal-tool-kind.edit    { background: #007aff; color: #fff; }
  #tool-modal .modal-tool-kind.delete  { background: #ff3b30; color: #fff; }
  #tool-modal .modal-tool-kind.search  { background: #5856d6; color: #fff; }
  #tool-modal .modal-tool-kind.execute { background: #ff9500; color: #fff; }
  #tool-modal .modal-tool-kind.think   { background: #af52de; color: #fff; }
  #tool-modal .modal-tool-kind.fetch   { background: #00c7be; color: #fff; }
  #tool-modal .modal-tool-kind.move    { background: #ff2d55; color: #fff; }
  #tool-modal .modal-tool-kind.other   { background: #8e8e93; color: #fff; }
  #tool-modal .modal-tool-title {
    flex: 1;
    font-size: 13px;
    color: var(--text-primary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  #tool-modal .modal-tool-status {
    font-size: 11px;
    padding: 2px 6px;
    border-radius: 4px;
    flex-shrink: 0;
  }
  #tool-modal .modal-tool-status.completed {
    background: rgba(52,199,89,0.15);
    color: #34c759;
  }
  #tool-modal .modal-tool-status.failed {
    background: rgba(255,59,48,0.15);
    color: #ff3b30;
  }
  #tool-modal .modal-tool-status.running {
    background: rgba(0,122,255,0.15);
    color: #007aff;
  }
  #tool-modal .modal-tool-duration {
    font-size: 11px;
    color: var(--text-tertiary);
    flex-shrink: 0;
    min-width: 36px;
    text-align: right;
  }
  #tool-modal .modal-tool-chevron {
    color: var(--text-tertiary);
    transition: transform 0.2s;
    flex-shrink: 0;
  }
  #tool-modal .modal-tool-row.expanded .modal-tool-chevron {
    transform: rotate(90deg);
  }
  #tool-modal .modal-tool-details {
    display: none;
    padding: 0 12px 12px;
    border-top: 1px solid var(--border);
    margin-top: 0;
  }
  #tool-modal .modal-tool-row.expanded .modal-tool-details {
    display: block;
  }
  #tool-modal .detail-section {
    margin-top: 10px;
  }
  #tool-modal .detail-section-title {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-secondary);
    margin-bottom: 6px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  #tool-modal .detail-location {
    font-size: 12px;
    color: var(--text-primary);
    font-family: 'SF Mono', Monaco, monospace;
    background: rgba(0,0,0,0.04);
    padding: 4px 8px;
    border-radius: 4px;
    margin-bottom: 4px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  @media (prefers-color-scheme: dark) {
    #tool-modal .detail-location {
      background: rgba(255,255,255,0.06);
    }
  }
  #tool-modal .detail-code {
    font-size: 11px;
    font-family: 'SF Mono', Monaco, monospace;
    background: rgba(0,0,0,0.04);
    padding: 10px;
    border-radius: 6px;
    overflow-x: auto;
    white-space: pre-wrap;
    word-break: break-word;
    color: var(--text-primary);
    max-height: 200px;
    overflow-y: auto;
  }
  @media (prefers-color-scheme: dark) {
    #tool-modal .detail-code {
      background: rgba(255,255,255,0.06);
    }
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

  <!-- Header — session tabs strip -->
  <div id="header">
    <span id="header-logo" title="Octopus">🐙</span>
    <div id="session-strip" role="tablist" aria-label="ACP sessions"></div>
    <button id="session-add-btn" title="New session" aria-label="New session">
      <svg width="10" height="10" viewBox="0 0 10 10" fill="none">
        <path d="M5 1v8M1 5h8" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
      </svg>
    </button>
    <button id="close-btn" title="Close (⌘⇧A)" aria-label="Close sidebar">
      <svg width="8" height="8" viewBox="0 0 8 8" fill="none">
        <path d="M1 1L7 7M7 1L1 7" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
      </svg>
    </button>
  </div>

  <!-- Inline create-session panel (toggled by + button) -->
  <div id="session-create-panel">
    <input id="sc-title" type="text" placeholder="Title" autocomplete="off" spellcheck="false" maxlength="32">
    <input id="sc-tag"   type="text" placeholder="agent:tag (e.g. octoweb:assistant)" autocomplete="off" spellcheck="false">
    <button id="sc-create" type="button">Create</button>
    <button id="sc-cancel" type="button" class="secondary" title="Cancel">×</button>
  </div>

  <!-- Messages — host element; per-session containers mounted/swapped by JS -->
  <div id="messages"></div>

  <!-- Welcome screen — shown when a session has no messages yet -->
  <div id="welcome">
    <div id="welcome-icon">
      <svg width="32" height="32" viewBox="0 0 32 32" fill="none">
        <circle cx="16" cy="16" r="15" stroke="var(--accent)" stroke-width="1.5" fill="none" opacity="0.3"/>
        <path d="M16 10v6M16 20v1" stroke="var(--accent)" stroke-width="2" stroke-linecap="round"/>
      </svg>
    </div>
    <div id="welcome-title">How can I help?</div>
    <div id="welcome-desc">Ask questions, paste code, describe a bug, or attach a file. Can browse and act on your behalf in the background.</div>
    <div id="welcome-suggestions">
      <button class="suggestion-btn" data-prompt="Summarize the current page">Summarize the current page</button>
      <button class="suggestion-btn" data-prompt="Explain this error and suggest a fix">Explain this error and suggest a fix</button>
      <button class="suggestion-btn" data-prompt="What are the key takeaways from this article?">Key takeaways from this article</button>
      <button class="suggestion-btn" data-prompt="Open the docs for this library and find the setup instructions">Find setup instructions in the docs</button>
    </div>
    <div id="welcome-shortcuts">
      <div class="shortcut-row"><kbd>⌘⇧A</kbd> <span>Toggle this panel</span></div>
      <div class="shortcut-row"><kbd>⌘T</kbd> <span>New session</span></div>
      <div class="shortcut-row"><kbd>⌘W</kbd> <span>Close session</span></div>
      <div class="shortcut-row"><kbd>Tab</kbd> <span>Switch session</span></div>
    </div>
  </div>

  <!-- Input -->
  <div id="input-area">
    <div id="cmd-dropdown"></div>
    <div id="input-row">
      <button id="new-session-btn" title="New session" aria-label="New session">
        <svg width="13" height="13" viewBox="0 0 16 16" fill="none">
          <path d="M13.5 3.5L7 10l-2.5-.5L5 7l6.5-6.5a1.4 1.4 0 0 1 2 0v0a1.4 1.4 0 0 1 0 2z"
                stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/>
          <path d="M12 6l-2-2" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>
          <path d="M2.5 13.5h11" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>
        </svg>
      </button>
      <div id="prompt-ghost-wrap">
        <textarea
          id="prompt-input"
          rows="1"
          placeholder="Ask Octopus…"
          autocomplete="off"
          spellcheck="false"
        ></textarea>
        <div id="prompt-ghost" aria-hidden="true"></div>
      </div>
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

<!-- Tool details modal -->
<div id="tool-modal">
  <div class="modal-content">
    <div class="modal-header">
      <span class="modal-title">Tool Details</span>
      <button class="modal-close" aria-label="Close">×</button>
    </div>
    <div class="modal-body">
      <div class="modal-tools-list"></div>
    </div>
  </div>
</div>


<!-- marked.js — lightweight MD parser, served from embedded binary -->
<script src="octoweb-lib://localhost/marked.min.js"></script>
<script>
  // Configure marked: safe defaults, no mangling
  if (typeof marked !== 'undefined') {
    marked.setOptions({ breaks: true, gfm: true });
    const _copyIcon =
      '<svg width="10" height="10" viewBox="0 0 12 12" fill="none" style="flex-shrink:0">' +
        '<rect x="4" y="4" width="7" height="7" rx="1.5" stroke="currentColor" stroke-width="1.4"/>' +
        '<path d="M3 8H2a1 1 0 0 1-1-1V2a1 1 0 0 1 1-1h5a1 1 0 0 1 1 1v1" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>' +
      '</svg>';
    marked.use({ renderer: {
      code(text, lang) {
        const l = (lang || '').split(/\s+/)[0];
        const label = l ? '<span class="code-lang">' + l + '</span>' : '';
        return '<div class="code-block">' +
          '<div class="code-header">' + label +
            '<button class="code-copy" onclick="__copyCode(this)">' +
              _copyIcon + '<span>Copy</span>' +
            '</button>' +
          '</div>' +
          '<pre><code' + (l ? ' class="language-' + l + '"' : '') + '>' +
            text +
          '</code></pre>' +
        '</div>';
      }
    }});
  }

  // ── DOM refs ────────────────────────────────────────────────────────────
  const messagesHost   = document.getElementById('messages');
  const welcomeEl      = document.getElementById('welcome');
  const sessionStrip   = document.getElementById('session-strip');
  const sessionAddBtn  = document.getElementById('session-add-btn');
  const scPanel        = document.getElementById('session-create-panel');
  const scTitle        = document.getElementById('sc-title');
  const scTag          = document.getElementById('sc-tag');
  const scCreate       = document.getElementById('sc-create');
  const scCancel       = document.getElementById('sc-cancel');
  const input          = document.getElementById('prompt-input');
  const sendBtn        = document.getElementById('send-btn');
  const newSessBtn     = document.getElementById('new-session-btn');
  const cmdDropdown    = document.getElementById('cmd-dropdown');
  const queueList      = document.getElementById('queue-list');
  const inputRow       = document.getElementById('input-row');
  const imagePreview   = document.getElementById('image-preview');
  const fileInputImage = document.getElementById('file-input-image');
  const fileInputDoc   = document.getElementById('file-input-doc');
  const attachBtn      = document.getElementById('attach-btn');
  const attachMenu     = document.getElementById('attach-menu');

  const MAX_SESSIONS = 4;
  const MAX_QUEUE    = 2;
  const MAX_PROMPT_HISTORY = 50;

  const kindLabel = { read:'R', edit:'E', delete:'D', search:'S', execute:'X', think:'T', fetch:'F', move:'M', other:'·' };

  // ── Welcome screen toggle ─────────────────────────────────────────────
  function updateWelcome() {
    if (!active) return;
    const hasMessages = active.container.querySelector('.msg');
    welcomeEl.classList.toggle('hidden', !!hasMessages);
  }

  // Suggestion buttons pre-fill the input
  welcomeEl.querySelectorAll('.suggestion-btn').forEach(btn => {
    btn.addEventListener('click', () => {
      input.value = btn.dataset.prompt;
      input.focus();
      input.style.height = 'auto';
      input.style.height = Math.min(input.scrollHeight, 120) + 'px';
    });
  });

  // ── Per-session state ──────────────────────────────────────────────────
  // Map<sid, Session>. Each Session owns its DOM container, thinking element,
  // and all chat/tool/queue state. Only the active session's container is
  // mounted in #messagesHost; others are detached (kept alive in JS memory).
  const sessions = new Map();
  let activeSid = null;
  // `active` is a live proxy that always points to the current session's
  // state. Updated on every switchTo(). All chat helpers read/write through it.
  let active = null;
  // Global prompt-history list, populated by Rust via __setHistory when the
  // sidebar opens. This is the single source of truth shared across ALL
  // sessions — Ctrl+P / Ctrl+N walks one MRU list regardless of which
  // session is active. Per-session state only owns the input draft + selection.
  let globalPromptHistory = [];

  function makeSession(sid, title, tag, status) {
    const container = document.createElement('div');
    container.className = 'session-messages';
    container.dataset.sid = sid;
    const thinking = document.createElement('div');
    thinking.id = 'thinking-' + sid;
    thinking.className = '';
    container.appendChild(thinking);

    const tab = document.createElement('div');
    tab.className = 'session-tab';
    tab.dataset.sid = sid;
    tab.setAttribute('role', 'tab');
    const dot = document.createElement('span');
    dot.className = 'session-dot ' + (status || 'connecting');
    const titleEl = document.createElement('span');
    titleEl.className = 'session-title';
    titleEl.textContent = title;
    titleEl.title = tag;
    const closeBtn = document.createElement('button');
    closeBtn.className = 'session-close';
    closeBtn.title = 'Close session';
    closeBtn.setAttribute('aria-label', 'Close session');
    closeBtn.innerHTML = '<svg width="7" height="7" viewBox="0 0 8 8" fill="none"><path d="M1 1L7 7M7 1L1 7" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/></svg>';
    tab.appendChild(dot);
    tab.appendChild(titleEl);
    tab.appendChild(closeBtn);
    sessionStrip.appendChild(tab);

    tab.addEventListener('click', (e) => {
      if (e.target === closeBtn || closeBtn.contains(e.target)) return;
      if (tab.querySelector('.session-rename')) return; // editing
      if (sid !== activeSid) {
        window.ipc.postMessage(JSON.stringify({ type: 'acp_session_switch', session_id: sid }));
      }
    });
    tab.addEventListener('dblclick', (e) => {
      e.preventDefault();
      startRename(sid);
    });
    closeBtn.addEventListener('click', (e) => {
      e.stopPropagation();
      if (sessions.size <= 1) return;
      window.ipc.postMessage(JSON.stringify({ type: 'acp_session_close', session_id: sid }));
    });

    return {
      sid,
      title,
      tag,
      status: status || 'connecting',
      container,
      thinking,
      tab,
      tabDot: dot,
      tabTitle: titleEl,
      // chat state
      currentAgentBubble: null,
      currentAgentRaw: '',
      isThinking: false,
      // tool tracking
      toolCount: 0,
      toolDetails: [],
      toolRows: {},
      activityStart: 0,
      activityTimer: null,
      // commands
      availableCommands: [],
      // queue (per-session: each session has its own pending list)
      msgQueue: [],
      // input draft (per-session, isolated)
      inputDraft: '',
      inputSelectionStart: 0,
      inputSelectionEnd: 0,
    };
  }

  function refreshAddBtn() {
    sessionAddBtn.disabled = sessions.size >= MAX_SESSIONS;
  }

  function refreshTabActiveStates() {
    for (const s of sessions.values()) {
      s.tab.classList.toggle('active', s.sid === activeSid);
    }
  }

  function applyStatus(s, st) {
    s.status = st;
    s.tabDot.className = 'session-dot ' + (st === 'ready' ? '' : st);
    s.tabDot.className = s.tabDot.className.trim();
  }

  function switchTo(sid) {
    const s = sessions.get(sid);
    if (!s) return;
    if (activeSid === sid && active === s) {
      refreshTabActiveStates();
      return;
    }
    // Save current session's input state before switching
    if (active) {
      active.inputDraft = input.value;
      active.inputSelectionStart = input.selectionStart;
      active.inputSelectionEnd = input.selectionEnd;
    }
    // Detach old container (its DOM stays in memory, attached to its session object)
    if (active && active.container.parentNode === messagesHost) {
      messagesHost.removeChild(active.container);
    }
    activeSid = sid;
    active = s;
    messagesHost.appendChild(s.container);
    refreshTabActiveStates();
    // Restore new session's input state (history is global — no swap needed)
    input.value = s.inputDraft || '';
    input.selectionStart = s.inputSelectionStart || 0;
    input.selectionEnd = s.inputSelectionEnd || 0;
    _ph.resetState();
    input.style.height = 'auto';
    input.style.height = Math.min(input.scrollHeight, 120) + 'px';
    // Rebind input UI to reflect this session's state
    sendBtn.classList.toggle('stop-mode', s.isThinking);
    sendBtn.title = s.isThinking ? 'Stop' : 'Send (Return)';
    renderQueue();
    updateInputLock();
    updateWelcome();
    scrollToBottom();
  }

  // Rust-driven session lifecycle
  window.__addSession = function(sid, title, tag, status) {
    if (sessions.has(sid)) return;
    const s = makeSession(sid, title, tag, status);
    sessions.set(sid, s);
    refreshAddBtn();
  };
  window.__removeSession = function(sid) {
    const s = sessions.get(sid);
    if (!s) return;
    if (s.activityTimer) clearInterval(s.activityTimer);
    if (s.container.parentNode === messagesHost) messagesHost.removeChild(s.container);
    if (s.tab.parentNode === sessionStrip) sessionStrip.removeChild(s.tab);
    sessions.delete(sid);
    refreshAddBtn();
  };
  window.__renameSession = function(sid, title) {
    const s = sessions.get(sid);
    if (!s) return;
    s.title = title;
    s.tabTitle.textContent = title;
  };
  window.__updateSessionTag = function(sid, tag) {
    const s = sessions.get(sid);
    if (!s) return;
    s.tag = tag;
    s.tabTitle.title = tag;
  };
  window.__switchSession = switchTo;
  window.__setSessionStatus = function(sid, st) {
    const s = sessions.get(sid);
    if (!s) return;
    applyStatus(s, st);
  };

  // ── Inline rename ───────────────────────────────────────────────────────
  function startRename(sid) {
    const s = sessions.get(sid);
    if (!s) return;
    const titleEl = s.tabTitle;
    if (s.tab.querySelector('.session-rename')) return;
    const inp = document.createElement('input');
    inp.className = 'session-rename';
    inp.type = 'text';
    inp.value = s.title;
    inp.maxLength = 32;
    s.tab.replaceChild(inp, titleEl);
    inp.focus();
    inp.select();
    const commit = (save) => {
      const v = inp.value.trim();
      s.tab.replaceChild(s.tabTitle, inp);
      if (save && v && v !== s.title) {
        window.ipc.postMessage(JSON.stringify({ type: 'acp_session_rename', session_id: sid, title: v }));
      }
    };
    inp.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') { e.preventDefault(); commit(true); }
      else if (e.key === 'Escape') { e.preventDefault(); commit(false); }
    });
    inp.addEventListener('blur', () => commit(true));
  }

  // ── Session create panel (header + button) ──────────────────────────────
  function openCreatePanel() {
    if (sessions.size >= MAX_SESSIONS) return;
    scPanel.classList.add('visible');
    scTitle.value = '';
    scTag.value = 'octoweb:assistant';
    scTitle.focus();
  }
  window.__openCreatePanel = openCreatePanel;
  sessionAddBtn.addEventListener('click', openCreatePanel);
  function hideCreatePanel() { scPanel.classList.remove('visible'); }
  scCancel.addEventListener('click', hideCreatePanel);
  function submitCreate() {
    const title = scTitle.value.trim() || 'Session';
    const tag   = scTag.value.trim();
    if (!tag) { scTag.focus(); return; }
    window.ipc.postMessage(JSON.stringify({ type: 'acp_session_create', title, tag }));
    hideCreatePanel();
  }
  scCreate.addEventListener('click', submitCreate);
  [scTitle, scTag].forEach(el => {
    el.addEventListener('keydown', (e) => {
      if (e.key === 'Enter') { e.preventDefault(); submitCreate(); }
      else if (e.key === 'Escape') { e.preventDefault(); hideCreatePanel(); }
    });
  });

  // ── Prompt history (shared module) ──────────────────────────────────────
  // MUST be initialized before bootstrap — switchTo() references _ph.
  /* PROMPT_HISTORY_JS */
  const ghostEl = document.getElementById('prompt-ghost');
  const _ph = createPromptHistory(input, ghostEl, 'Ask Octopus\u2026', function() {
    input.style.height = 'auto';
    input.style.height = Math.min(input.scrollHeight, 120) + 'px';
    updateSendBtn();
  });
  // Global prompt history is shared by all sessions. Rust calls this on
  // sidebar open with the persisted MRU list, and any time it wants to push
  // a refresh. Ctrl+P / Ctrl+N navigate this single list from any session.
  window.__setHistory = function(arr) {
    globalPromptHistory = Array.isArray(arr) ? arr.slice() : [];
    _ph.setHistory(globalPromptHistory);
  };

  // ── Bootstrap default session (sid=1, matches main.rs) ──────────────────
  // The main.rs spawns a default session (id=1, "Assistant", "octoweb:assistant")
  // before the sidebar HTML loads. We register it here so events arriving with
  // sid=1 find a target.
  (function initDefault() {
    window.__addSession(1, 'Assistant', 'octoweb:assistant', 'connecting');
    switchTo(1);
  })();

  // ── Clear current session (toolbar pencil button) ───────────────────────
  newSessBtn.addEventListener('click', () => {
    if (!active) return;
    // Wipe local DOM optimistically; Rust will restart the agent.
    while (active.container.firstChild) {
      active.container.removeChild(active.container.firstChild);
    }
    active.container.appendChild(active.thinking);
    active.thinking.className = '';
    active.thinking.innerHTML = '';
    active.currentAgentBubble = null;
    active.currentAgentRaw = '';
    active.isThinking = false;
    active.toolCount = 0;
    active.toolDetails = [];
    active.toolRows = {};
    if (active.activityTimer) { clearInterval(active.activityTimer); active.activityTimer = null; }
    active.inputDraft = '';
    active.inputSelectionStart = 0;
    active.inputSelectionEnd = 0;
    sendBtn.className = '';
    input.value = '';
    input.style.height = 'auto';
    _ph.resetState();
    window.ipc.postMessage(JSON.stringify({ type: 'acp_clear_session', session_id: active.sid }));
  });

  // Callable from Rust to clear messages for a specific session
  window.__clearMessages = function(sid) {
    const s = sid ? sessions.get(sid) : active;
    if (!s) return;
    while (s.container.firstChild) s.container.removeChild(s.container.firstChild);
    s.container.appendChild(s.thinking);
    s.thinking.className = '';
    s.thinking.innerHTML = '';
    s.currentAgentBubble = null;
    s.currentAgentRaw = '';
    s.availableCommands = [];
    s.inputDraft = '';
    s.inputSelectionStart = 0;
    s.inputSelectionEnd = 0;
    if (s === active) {
      hideCmdDropdown();
      input.value = '';
      input.style.height = 'auto';
      _ph.resetState();
    }
  };

  // ── Slash commands (per-session) ────────────────────────────────────────
  var cmdActiveIdx = 0;
  var cmdFiltered = [];
  var cmdVisible = false;

  window.__setAvailableCommands = function(sid, json) {
    const s = sessions.get(sid);
    if (!s) return;
    try { s.availableCommands = JSON.parse(json); } catch(e) { s.availableCommands = []; }
  };

  function showCmdDropdown(filter) {
    if (!active) return;
    var lower = filter.toLowerCase();
    cmdFiltered = active.availableCommands.filter(function(c) {
      return c.name.toLowerCase().indexOf(lower) === 0;
    });
    if (cmdFiltered.length === 0) { hideCmdDropdown(); return; }
    cmdActiveIdx = 0;
    renderCmdDropdown();
    cmdDropdown.classList.add('visible');
    cmdVisible = true;
  }

  function hideCmdDropdown() {
    cmdDropdown.classList.remove('visible');
    cmdVisible = false;
    cmdFiltered = [];
    cmdActiveIdx = 0;
  }

  function renderCmdDropdown() {
    cmdDropdown.innerHTML = '';
    for (var i = 0; i < cmdFiltered.length; i++) {
      var c = cmdFiltered[i];
      var div = document.createElement('div');
      div.className = 'cmd-item' + (i === cmdActiveIdx ? ' active' : '');
      var nameEl = document.createElement('div');
      nameEl.className = 'cmd-name';
      nameEl.textContent = '/' + c.name;
      var descEl = document.createElement('div');
      descEl.className = 'cmd-desc';
      descEl.textContent = c.description;
      div.appendChild(nameEl);
      div.appendChild(descEl);
      (function(idx) {
        div.addEventListener('mousedown', function(e) {
          e.preventDefault();
          selectCmd(idx);
        });
        div.addEventListener('mouseenter', function() {
          cmdActiveIdx = idx;
          updateCmdActive();
        });
      })(i);
      cmdDropdown.appendChild(div);
    }
  }

  function updateCmdActive() {
    var items = cmdDropdown.querySelectorAll('.cmd-item');
    for (var i = 0; i < items.length; i++) {
      items[i].classList.toggle('active', i === cmdActiveIdx);
    }
    if (items[cmdActiveIdx]) items[cmdActiveIdx].scrollIntoView({ block: 'nearest' });
  }

  function selectCmd(idx) {
    var cmd = cmdFiltered[idx];
    if (!cmd) return;
    input.value = '/' + cmd.name + ' ';
    if (cmd.hint) { input.placeholder = cmd.hint; }
    hideCmdDropdown();
    input.focus();
    input.style.height = 'auto';
    input.style.height = Math.min(input.scrollHeight, 120) + 'px';
    updateSendBtn();
  }

  // Capture-phase keydown: intercept arrow/enter/tab/escape when dropdown is visible
  input.addEventListener('keydown', function(e) {
    if (!cmdVisible) return;
    if (e.key === 'ArrowDown') {
      e.preventDefault(); e.stopPropagation();
      if (cmdActiveIdx < cmdFiltered.length - 1) cmdActiveIdx++;
      updateCmdActive();
      return;
    }
    if (e.key === 'ArrowUp') {
      e.preventDefault(); e.stopPropagation();
      if (cmdActiveIdx > 0) cmdActiveIdx--;
      updateCmdActive();
      return;
    }
    if (e.key === 'Enter' || e.key === 'Tab') {
      e.preventDefault(); e.stopPropagation();
      selectCmd(cmdActiveIdx);
      return;
    }
    if (e.key === 'Escape') {
      e.preventDefault(); e.stopPropagation();
      hideCmdDropdown();
      return;
    }
  }, true);

  input.addEventListener('blur', function() {
    setTimeout(hideCmdDropdown, 150);
  });

  // ── Markdown render helper ─────────────────────────────────────────────
  function renderMd(raw) {
    if (typeof marked === 'undefined') return escapeHtml(raw);
    return marked.parse(raw);
  }

  function escapeHtml(s) {
    return s.replace(/&/g,'&amp;').replace(/</g,'&lt;').replace(/>/g,'&gt;');
  }

  // Per-code-block copy — called from inline onclick in custom renderer
  window.__copyCode = function(btn) {
    const block = btn.closest('.code-block');
    if (!block) return;
    const code = block.querySelector('code');
    if (!code) return;
    window.ipc.postMessage(JSON.stringify({ type: 'copy_text', text: code.textContent }));
    btn.classList.add('copied');
    const span = btn.querySelector('span');
    const oldLabel = span.textContent;
    span.textContent = 'Copied';
    setTimeout(() => { btn.classList.remove('copied'); span.textContent = oldLabel; }, 1800);
  };

  // ── Messages ────────────────────────────────────────────────────────────
  function fmtTime(d) {
    const h = d.getHours();
    const m = d.getMinutes();
    const ampm = h >= 12 ? 'PM' : 'AM';
    const h12 = h % 12 || 12;
    return h12 + ':' + String(m).padStart(2, '0') + ' ' + ampm;
  }

  function appendMessage(s, role, text) {
    if (!s) return;
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
    s.container.insertBefore(wrap, s.thinking);
    if (s.sid === activeSid) { updateWelcome(); scrollToBottom(); }
    return bubble;
  }

  // Copy button — macOS frosted pill, top-right of bubble, shows on hover
  function makeCopyBtn(wrap) {
    const btn = document.createElement('button');
    btn.className = 'msg-copy';
    btn.title = 'Copy';
    btn.innerHTML =
      '<svg width="10" height="10" viewBox="0 0 12 12" fill="none" style="flex-shrink:0">' +
        '<rect x="4" y="4" width="7" height="7" rx="1.5" stroke="currentColor" stroke-width="1.4"/>' +
        '<path d="M3 8H2a1 1 0 0 1-1-1V2a1 1 0 0 1 1-1h5a1 1 0 0 1 1 1v1" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/>' +
      '</svg>' +
      '<span>Copy</span>';
    btn.addEventListener('click', () => {
      const raw = wrap.dataset.raw || wrap.querySelector('.msg-bubble')?.textContent || '';
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

  // ── Command output renderer ──────────────────────────────────────────
  function tryParseCommandJson(raw) {
    var trimmed = raw.trim();
    if (trimmed.charAt(0) !== '{') return null;
    try {
      var obj = JSON.parse(trimmed);
      if (obj && typeof obj === 'object' && obj.command_type) return obj;
    } catch(e) {}
    return null;
  }

  function formatLabel(key) {
    return key.replace(/_/g, ' ');
  }

  function renderCommandValue(val) {
    if (val === null || val === undefined) return '<span class="null">none</span>';
    if (typeof val === 'boolean') return '<span class="bool-' + val + '">' + val + '</span>';
    if (typeof val === 'number') return '<span class="number">' + val + '</span>';
    if (typeof val === 'string') return escapeHtml(val);
    if (Array.isArray(val)) {
      if (val.length === 0) return '<span class="null">none</span>';
      if (val.every(function(v) { return typeof v === 'string' || typeof v === 'number'; })) {
        var ul = '<ul class="cmd-output-list">';
        for (var i = 0; i < val.length; i++) ul += '<li>' + escapeHtml(String(val[i])) + '</li>';
        return ul + '</ul>';
      }
      return '<div class="cmd-output-nested">' + escapeHtml(JSON.stringify(val, null, 2)) + '</div>';
    }
    if (typeof val === 'object') {
      return '<div class="cmd-output-nested">' + renderCommandTable(val) + '</div>';
    }
    return escapeHtml(String(val));
  }

  function renderCommandTable(obj) {
    var html = '<table class="cmd-output-table">';
    for (var key in obj) {
      if (!obj.hasOwnProperty(key)) continue;
      html += '<tr><td class="cmd-output-key">' + escapeHtml(formatLabel(key)) + '</td>';
      html += '<td class="cmd-output-val">' + renderCommandValue(obj[key]) + '</td></tr>';
    }
    return html + '</table>';
  }

  function renderCommandOutput(obj) {
    var cmdType = obj.command_type;
    var rest = {};
    for (var key in obj) {
      if (key === 'command_type') continue;
      rest[key] = obj[key];
    }
    var keys = Object.keys(rest);
    var body;
    if (keys.length === 1 && Array.isArray(rest[keys[0]])) {
      var k = keys[0];
      var arr = rest[k];
      body = '<div class="cmd-output-single-list">' +
        '<div class="cmd-output-list-label">' + escapeHtml(formatLabel(k)) + ':</div>' +
        renderCommandValue(arr) +
        '</div>';
    } else {
      body = renderCommandTable(rest);
    }
    return '<div class="cmd-output">' +
      '<div class="cmd-output-header">/' + escapeHtml(cmdType) + '</div>' +
      body +
      '</div>';
  }

  // Called once Done arrives — stores raw, appends copy btn, collapses if tall
  function finishAgentBubble(s, bubble, rawText, toolCount, details) {
    const wrap = bubble.closest('.msg');
    if (!wrap) return;
    wrap.dataset.raw = rawText;

    var cmdObj = tryParseCommandJson(rawText);
    if (cmdObj) {
      bubble.innerHTML = renderCommandOutput(cmdObj);
    }

    if (toolCount > 0) {
      const sepEl = wrap.querySelector('.msg-sep');
      if (sepEl) sepEl.style.display = 'inline';
      const toolsEl = wrap.querySelector('.msg-tools');
      if (toolsEl) {
        toolsEl.textContent = toolCount + ' tools';
        toolsEl.style.display = 'inline';
        toolsEl.style.cursor = 'pointer';
        s.messageToolDetails.set(wrap, details);
        toolsEl.addEventListener('click', (e) => {
          e.stopPropagation();
          const d = s.messageToolDetails.get(wrap);
          if (d) showToolModal(d);
        });
      }
    }

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

  function startAgentBubble(s) {
    if (!s) return;
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
    const sep = document.createElement('span');
    sep.className = 'msg-sep';
    sep.textContent = '·';
    sep.style.display = 'none';
    const tools = document.createElement('span');
    tools.className = 'msg-tools';
    tools.style.display = 'none';
    label.appendChild(who);
    label.appendChild(time);
    label.appendChild(sep);
    label.appendChild(tools);
    const bubble = document.createElement('div');
    bubble.className = 'msg-bubble';
    wrap.appendChild(label);
    wrap.appendChild(bubble);
    s.container.insertBefore(wrap, s.thinking);
    if (s.sid === activeSid) scrollToBottom();
    return bubble;
  }

  window.__appendChunk = function(sid, text) {
    const s = sessions.get(sid);
    if (!s) return;
    if (!s.currentAgentBubble) {
      s.currentAgentBubble = startAgentBubble(s);
      s.currentAgentRaw = '';
      s.isThinking = false;
      s.thinking.className = '';
      clearActivity(s);
    }
    s.currentAgentRaw += text;
    var cmdObj = tryParseCommandJson(s.currentAgentRaw);
    s.currentAgentBubble.innerHTML = cmdObj ? renderCommandOutput(cmdObj) : renderMd(s.currentAgentRaw);
    if (s.sid === activeSid) scrollToBottom();
  };

  window.__appendImage = function(sid, mimeType, b64data) {
    const s = sessions.get(sid);
    if (!s) return;
    if (!s.currentAgentBubble) {
      s.currentAgentBubble = startAgentBubble(s);
      s.currentAgentRaw = '';
      s.isThinking = false;
      s.thinking.className = '';
      clearActivity(s);
    }
    const img = document.createElement('img');
    img.className = 'chat-img';
    img.src = 'data:' + mimeType + ';base64,' + b64data;
    s.currentAgentBubble.appendChild(img);
    if (s.sid === activeSid) scrollToBottom();
  };

  // ── Activity feed state (per-session) ──────────────────────────────────
  function fmtElapsed(ms) {
    const s = Math.floor(ms / 1000);
    if (s < 60) return s + 's';
    return Math.floor(s / 60) + 'm ' + (s % 60) + 's';
  }

  function clearActivity(s) {
    if (s.activityTimer) { clearInterval(s.activityTimer); s.activityTimer = null; }
    s.thinking.innerHTML = '';
    for (const k in s.toolRows) delete s.toolRows[k];
  }

  function tickActivity(s) {
    const hdr = s.thinking.querySelector('.activity-elapsed');
    if (hdr) hdr.textContent = fmtElapsed(Date.now() - s.activityStart);
    for (const id in s.toolRows) {
      const t = s.toolRows[id];
      if (t.timerEl && !t.finished) {
        t.timerEl.textContent = fmtElapsed(Date.now() - t.startTime);
      }
    }
  }

  window.__toolStart = function(sid, id, title, kind, rawInput, locations) {
    const s = sessions.get(sid);
    if (!s) return;
    s.toolCount++;
    s.toolDetails.push({ id, kind, title, status: 'running', duration: 0, rawInput, locations, rawOutput: null });
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
    s.thinking.appendChild(row);
    s.toolRows[id] = { el: row, startTime: Date.now(), timerEl: tm, finished: false, idx: s.toolDetails.length - 1 };
    if (sid === activeSid) scrollToBottom();
  };

  window.__toolUpdate = function(sid, id, title, status, rawOutput) {
    const s = sessions.get(sid);
    if (!s) return;
    const t = s.toolRows[id];
    if (!t) return;
    if (title) {
      t.el.querySelector('.tool-title').textContent = title;
      s.toolDetails[t.idx].title = title;
    }
    if (rawOutput !== undefined && rawOutput !== null) {
      s.toolDetails[t.idx].rawOutput = rawOutput;
    }
    if (status === 'completed') {
      t.finished = true;
      t.el.classList.add('done');
      const duration = Date.now() - t.startTime;
      t.timerEl.textContent = fmtElapsed(duration);
      s.toolDetails[t.idx].status = 'completed';
      s.toolDetails[t.idx].duration = duration;
      const check = document.createElement('span');
      check.className = 'tool-check';
      check.textContent = '✓';
      t.timerEl.replaceWith(check);
    } else if (status === 'failed') {
      t.finished = true;
      t.el.classList.add('failed');
      s.toolDetails[t.idx].status = 'failed';
      s.toolDetails[t.idx].duration = Date.now() - t.startTime;
      const fail = document.createElement('span');
      fail.className = 'tool-fail';
      fail.textContent = '✗';
      t.timerEl.replaceWith(fail);
    }
  };

  window.__setThinking = function(sid, on) {
    const s = sessions.get(sid);
    if (!s) return;
    s.isThinking = on;
    s.thinking.className = on ? 'visible' : '';
    if (sid === activeSid) {
      sendBtn.classList.toggle('stop-mode', on);
      sendBtn.title = on ? 'Stop' : 'Send (Return)';
    }
    if (on) {
      s.currentAgentBubble = null;
      s.currentAgentRaw = '';
      s.toolCount = 0;
      s.toolDetails = [];
      clearActivity(s);
      s.activityStart = Date.now();
      const hdr = document.createElement('div');
      hdr.className = 'activity-header';
      hdr.innerHTML = '<span class="activity-dots"><span></span><span></span><span></span></span><span class="activity-elapsed">0s</span>';
      s.thinking.appendChild(hdr);
      s.activityTimer = setInterval(() => tickActivity(s), 1000);
      if (sid === activeSid) scrollToBottom();
    } else {
      const savedToolCount = s.toolCount;
      const savedToolDetails = [...s.toolDetails];
      clearActivity(s);
      if (s.currentAgentBubble) {
        finishAgentBubble(s, s.currentAgentBubble, s.currentAgentRaw, savedToolCount, savedToolDetails);
        s.currentAgentBubble = null;
        s.currentAgentRaw = '';
      }
    }
  };

  window.__appendError = function(sid, text) {
    const s = sessions.get(sid);
    if (!s) return;
    window.__setThinking(sid, false);
    appendMessage(s, 'error', text);
  };

  function scrollToBottom() {
    messagesHost.scrollTop = messagesHost.scrollHeight;
  }

  // ── Queue (per-session, max 2 pending) ─────────────────────────────────
  function renderQueue() {
    if (!active) { queueList.innerHTML = ''; return; }
    queueList.innerHTML = '';
    active.msgQueue.forEach((entry, i) => {
      const item = document.createElement('div');
      item.className = 'queue-item';
      const lbl = document.createElement('span');
      lbl.className = 'queue-item-label';
      lbl.textContent = '#' + (i + 1);
      const txt = document.createElement('span');
      txt.className = 'queue-item-text';
      txt.textContent = entry.text + (entry.images.length ? ' [' + entry.images.length + ' img]' : '');
      const rm = document.createElement('button');
      rm.className = 'queue-remove';
      rm.title = 'Remove';
      rm.innerHTML = '<svg width="8" height="8" viewBox="0 0 8 8" fill="none"><path d="M1 1L7 7M7 1L1 7" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>';
      rm.addEventListener('click', () => {
        active.msgQueue.splice(i, 1);
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
    const lock = active && active.msgQueue.length >= MAX_QUEUE;
    inputRow.classList.toggle('locked', lock);
    input.disabled = lock;
  }

  function drainQueueForSession(s) {
    if (!s || s.msgQueue.length === 0) return;
    const next = s.msgQueue.shift();
    renderQueue();
    updateInputLock();
    dispatchPromptForSession(s, next.text, next.images, next.docs);
  }

  function drainQueue() {
    drainQueueForSession(active);
  }

  function dispatchPromptForSession(s, text, images, docs) {
    if (!s) return;
    const displayText = text.replace(/<doc filename="[^"]*">[\s\S]*?<\/doc>\s*/g, '').trim();
    const bubble = appendMessage(s, 'user', displayText || '(document attached)');
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
    if (images && images.length) {
      for (const img of images) {
        const el = document.createElement('img');
        el.className = 'chat-img';
        el.src = 'data:' + img.mimeType + ';base64,' + img.data;
        bubble.appendChild(el);
      }
    }
    window.__setThinking(s.sid, true);
    window.ipc.postMessage(JSON.stringify({ type: 'acp_prompt', session_id: s.sid, text, images: images || [] }));
  }

  function dispatchPrompt(text, images, docs) {
    if (!active) return;
    dispatchPromptForSession(active, text, images, docs);
  }

  // ── Image attachments ──────────────────────────────────────────────────
  let pendingImages = []; // [{data: base64, mimeType: string}]
  let pendingDocs = [];   // [{file: File, name: string}]
  let docLibsLoaded = false;

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
    if (!pendingImages.length && !pendingDocs.length) {
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
      rm.onclick = () => { pendingDocs.splice(i, 1); renderImagePreview(); updateSendBtn(); };
      chip.appendChild(rm);
      imagePreview.appendChild(chip);
    }
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
  async function ensureDocLibs() {
    if (docLibsLoaded) return;
    async function loadScript(url) {
      return new Promise((resolve, reject) => {
        const s = document.createElement('script');
        s.src = url;
        s.onload = resolve;
        s.onerror = reject;
        document.head.appendChild(s);
      });
    }
    await loadScript('octoweb-lib://localhost/pdf.min.js');
    const workerSrc = await fetch('octoweb-lib://localhost/pdf.worker.min.js').then(r => r.text());
    const workerBlob = new Blob([workerSrc], { type: 'application/javascript' });
    pdfjsLib.GlobalWorkerOptions.workerSrc = URL.createObjectURL(workerBlob);
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

  fileInputDoc.addEventListener('change', () => {
    for (const f of fileInputDoc.files) {
      pendingDocs.push({ file: f, name: f.name });
    }
    fileInputDoc.value = '';
    renderImagePreview();
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
    if (!active) return;
    const text = input.value.trim();
    const images = pendingImages.slice();
    const docs = pendingDocs.slice();
    if (!text && !images.length && !docs.length) return;
    input.value = '';
    input.placeholder = 'Ask Octopus\u2026';
    input.style.height = 'auto';
    pendingImages = [];
    pendingDocs = [];
    renderImagePreview();
    sendBtn.classList.remove('active');

    let docPrefix = '';
    if (docs.length) {
      for (const doc of docs) {
        try {
          const extracted = await extractDocText(doc.file);
          if (extracted) {
            docPrefix += '<doc filename="' + doc.name + '"\u003e\n' + extracted + '\n</doc>\n\n';
          } else {
            docPrefix += '<doc filename="' + doc.name + '"\u003e\n[Document was empty or could not be parsed]\n</doc>\n\n';
          }
        } catch (e) {
          const msg = e && (e.message || e.toString()) || 'unknown error';
          console.error('Doc extraction failed:', e);
          docPrefix += '<doc filename="' + doc.name + '"\u003e\n[Failed to extract: ' + msg + ']\n</doc>\n\n';
        }
      }
    }
    const fullText = docPrefix + text;

    if (!active.isThinking) {
      dispatchPrompt(fullText, images, docs);
    } else if (active.msgQueue.length < MAX_QUEUE) {
      active.msgQueue.push({ text: fullText, images, docs });
      renderQueue();
      updateInputLock();
    }

    // Save raw prompt text to GLOBAL history (MRU, dedup) so Ctrl+P/N walks
    // it from any session and persistence (driven by Rust) sees every prompt.
    if (text) {
      const pos = globalPromptHistory.indexOf(text);
      if (pos !== -1) globalPromptHistory.splice(pos, 1);
      globalPromptHistory.unshift(text);
      if (globalPromptHistory.length > MAX_PROMPT_HISTORY) globalPromptHistory.pop();
      _ph.setHistory(globalPromptHistory);
    }
  }

  function stop() {
    if (!active) return;
    window.ipc.postMessage(JSON.stringify({ type: 'acp_cancel', session_id: active.sid }));
    window.__setThinking(active.sid, false);
  }

  sendBtn.addEventListener('click', () => {
    if (sendBtn.classList.contains('stop-mode')) {
      stop();
    } else {
      send();
    }
  });
  input.addEventListener('keydown', e => {
    if (e.key === 'Enter' && !e.shiftKey && !_ph.isInSearchMode()) { e.preventDefault(); send(); _ph.resetState(); return; }
  });
  input.addEventListener('input', () => {
    input.style.height = 'auto';
    input.style.height = Math.min(input.scrollHeight, 120) + 'px';
    updateSendBtn();
    var val = input.value;
    if (!val) { input.placeholder = 'Ask Octopus\u2026'; }
    if (val.charAt(0) === '/' && active && active.availableCommands.length > 0) {
      var spaceIdx = val.indexOf(' ');
      if (spaceIdx === -1) {
        showCmdDropdown(val.substring(1));
      } else {
        hideCmdDropdown();
      }
    } else {
      hideCmdDropdown();
    }
  });

  // Hook into __setThinking to drain queue when agent becomes free
  const _origSetThinking = window.__setThinking;
  window.__setThinking = function(sid, on) {
    _origSetThinking(sid, on);
    if (!on) {
      const s = sessions.get(sid);
      if (s) setTimeout(() => drainQueueForSession(s), 80);
    }
  };

  // ── Close sidebar ──────────────────────────────────────────────────────
  document.getElementById('close-btn').addEventListener('click', () => {
    window.ipc.postMessage(JSON.stringify({ type: 'sidebar_close' }));
  });

  // ── Tool Details Modal ───────────────────────────────────────────────────
  const toolModal = document.getElementById('tool-modal');
  const toolModalBody = toolModal.querySelector('.modal-body');
  const toolModalClose = toolModal.querySelector('.modal-close');

  function showToolModal(details) {
    const list = toolModal.querySelector('.modal-tools-list');
    list.innerHTML = '';
    for (const t of details) {
      const row = document.createElement('div');
      row.className = 'modal-tool-row';
      row.dataset.id = t.id;
      row.innerHTML = `
        <div class="modal-tool-header">
          <span class="modal-tool-kind ${t.kind}">${kindLabel[t.kind] || '·'}</span>
          <span class="modal-tool-title">${escapeHtml(t.title)}</span>
          <span class="modal-tool-status ${t.status}">${t.status}</span>
          <span class="modal-tool-duration">${t.duration ? fmtElapsed(t.duration) : '-'}</span>
          <span class="modal-tool-chevron">▶</span>
        </div>
        <div class="modal-tool-details">
          ${buildToolDetails(t)}
        </div>
      `;
      row.querySelector('.modal-tool-header').addEventListener('click', () => {
        row.classList.toggle('expanded');
      });
      list.appendChild(row);
    }
    toolModal.classList.add('show');
  }

  function formatJson(val) {
    if (val === null || val === undefined) return null;
    try { return JSON.stringify(val, null, 2); } catch(e) { return String(val); }
  }

  function buildToolDetails(t) {
    let html = '';
    if (t.locations && t.locations.length > 0) {
      html += '<div class="detail-section"><div class="detail-section-title">Locations</div>';
      for (const l of t.locations) {
        html += `<div class="detail-location">${escapeHtml(l)}</div>`;
      }
      html += '</div>';
    }
    const inputJson = formatJson(t.rawInput);
    if (inputJson) {
      html += '<div class="detail-section"><div class="detail-section-title">Input</div><pre class="detail-code">' + escapeHtml(inputJson) + '</pre></div>';
    }
    const outputJson = formatJson(t.rawOutput);
    if (outputJson) {
      html += '<div class="detail-section"><div class="detail-section-title">Output</div><pre class="detail-code">' + escapeHtml(outputJson) + '</pre></div>';
    }
    return html;
  }

  function hideToolModal() {
    toolModal.classList.remove('show');
  }

  toolModalClose.addEventListener('click', hideToolModal);
  toolModal.addEventListener('click', e => {
    if (e.target === toolModal) hideToolModal();
  });
  document.addEventListener('keydown', e => {
    if (e.key === 'Escape' && toolModal.classList.contains('show')) hideToolModal();
  });

  // Tab / Shift+Tab cycle ACP sessions globally, regardless of which element
  // has focus. Capture phase + skip when modifier keys are present so the
  // shortcut doesn't collide with browser focus traversal in modals or
  // ⌘⇧⇥ system shortcuts. Always returns focus to the prompt input so users
  // can keep typing without an extra click.
  document.addEventListener('keydown', e => {
    if (e.key !== 'Tab' || e.ctrlKey || e.metaKey || e.altKey) return;
    // Don't hijack Tab when the create-session panel is open (its inputs
    // need normal Tab navigation between title/tag fields).
    if (scPanel.classList.contains('visible')) return;
    const keys = Array.from(sessions.keys());
    if (keys.length <= 1) {
      e.preventDefault();
      input.focus();
      return;
    }
    e.preventDefault();
    const idx = keys.indexOf(activeSid);
    const nextIdx = e.shiftKey
      ? (idx <= 0 ? keys.length - 1 : idx - 1)
      : (idx >= keys.length - 1 ? 0 : idx + 1);
    const nextSid = keys[nextIdx];
    window.ipc.postMessage(JSON.stringify({ type: 'acp_session_switch', session_id: nextSid }));
    input.focus();
  }, true);

  // ── Inject prompt (from Rust "Ask AI") ───────────────────────────────
  window.__injectPrompt = function(text) {
    input.value = text;
    input.style.height = 'auto';
    sendBtn.classList.toggle('active', true);
    send();
  };

</script>
</body>
</html>"#.replace("/* PROMPT_HISTORY_JS */", prompt_history_js)
}
