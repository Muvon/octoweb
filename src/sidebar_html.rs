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
///   window.__a2uiUpdate(sid, fileId, payload)     — render / update an A2UI surface
///   window.__a2uiResolved(sid, fileId, payload)   — surface was resolved (gray it out)
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
///   { type: "a2ui_resolve",  file_id, action } — A2UI Button event → unblocks the bash tool
///   { type: "a2ui_open_url", url }              — A2UI Button.openUrl → open in a browser tab
pub fn html(max_ai_prompt_history: usize) -> String {
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

  #close-btn, #fullscreen-btn {
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
  #close-btn:hover, #fullscreen-btn:hover {
    background: rgba(0,0,0,0.07);
    color: var(--text-primary);
    border-color: rgba(0,0,0,0.13);
  }
  @media (prefers-color-scheme: dark) {
    #close-btn:hover, #fullscreen-btn:hover {
      background: rgba(255,255,255,0.10);
      border-color: rgba(255,255,255,0.16);
    }
  }
  #close-btn:active, #fullscreen-btn:active { transform: scale(0.92); }
  #fullscreen-btn.active {
    color: var(--accent);
    border-color: rgba(0, 122, 255, 0.30);
    background: rgba(0, 122, 255, 0.10);
  }

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

  /* ── A2UI surface bubble ──────────────────────────────────────────────── */
  /* Inline UI surfaces rendered from `render_ui` envelopes. Per A2UI v0.9 the
     surface persists across events — a button click only unblocks the agent's
     current `render_ui` call; the bubble stays alive until the agent emits
     `deleteSurface`. While the agent is processing the event we briefly lock
     interaction and show a "processing" overlay; the next envelope clears it. */
  .msg.ui .msg-bubble {
    background: var(--agent-bg);
    border: 1px solid var(--agent-border);
    border-radius: 14px;
    padding: 0;
    overflow: hidden;
    align-self: stretch;
    max-width: 100%;
    transition: opacity 0.2s, filter 0.2s;
  }
  /* In-flight: agent has the event, hasn't pushed an update yet. Dim slightly,
     suspend pointer events, show a non-modal "Processing…" pill. Lifts as soon
     as the next envelope for this surface arrives. */
  .msg.ui.resolved .msg-bubble { opacity: 0.96; }
  .msg.ui.resolved .a2ui-body { pointer-events: none; position: relative; }
  .msg.ui.resolved .a2ui-body::after {
    content: "Processing…";
    position: absolute;
    top: 10px;
    right: 12px;
    padding: 3px 10px;
    background: rgba(0, 0, 0, 0.55);
    color: #fff;
    border-radius: 999px;
    font-size: 11px;
    font-weight: 500;
    letter-spacing: 0.02em;
    pointer-events: none;
    z-index: 5;
    animation: a2ui-pulse 1.4s ease-in-out infinite;
  }
  @media (prefers-color-scheme: light) {
    .msg.ui.resolved .a2ui-body::after { background: rgba(0, 0, 0, 0.7); }
  }
  @keyframes a2ui-pulse {
    0%, 100% { opacity: 0.85; }
    50% { opacity: 0.55; }
  }
  .msg.ui.resolved .a2ui-btn,
  .msg.ui.resolved .a2ui-chip,
  .msg.ui.resolved .a2ui-tab,
  .msg.ui.resolved .a2ui-modal-close { opacity: 0.7; }
  .msg.ui.resolved .a2ui-field input,
  .msg.ui.resolved .a2ui-field textarea,
  .msg.ui.resolved .a2ui-field select { opacity: 0.85; }
  .a2ui-head {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 10px;
    border-bottom: 1px solid var(--divider);
    background: linear-gradient(to bottom, rgba(0,0,0,0.015), transparent);
    font-size: 10px;
    color: var(--text-tertiary);
  }
  @media (prefers-color-scheme: dark) {
    .a2ui-head { background: linear-gradient(to bottom, rgba(255,255,255,0.02), transparent); }
  }
  .a2ui-head .kind-tag {
    font-weight: 600;
    letter-spacing: 0.08em;
    text-transform: uppercase;
    padding: 1px 6px;
    border-radius: 4px;
    background: var(--a2ui-primary, var(--accent));
    color: white;
    font-size: 9px;
  }
  .a2ui-head .mono {
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
    opacity: 0.6;
  }
  .a2ui-body { padding: 10px 12px; }
  .a2ui-resolved-note {
    padding: 5px 10px;
    border-top: 1px solid var(--divider);
    font-size: 10.5px;
    color: var(--text-secondary);
    background: rgba(0,0,0,0.025);
  }
  @media (prefers-color-scheme: dark) {
    .a2ui-resolved-note { background: rgba(255,255,255,0.04); }
  }
  .a2ui-card {
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 9px 10px;
    border-radius: 10px;
    border: 1px solid var(--md-table-border);
    background: rgba(0,0,0,0.018);
  }
  @media (prefers-color-scheme: dark) {
    .a2ui-card { background: rgba(255,255,255,0.025); }
  }
  .a2ui-col { display: flex; flex-direction: column; }
  .a2ui-row { display: flex; flex-direction: row; flex-wrap: wrap; }
  .a2ui-spacer { display: block; min-height: 6px; }
  .a2ui-divider { border: none; border-top: 1px solid var(--divider); margin: 4px 0; }
  .a2ui-text { font-size: 13px; line-height: 1.55; color: var(--text-primary); white-space: pre-wrap; }
  .a2ui-text.muted { color: var(--text-secondary); font-size: 12px; }
  .a2ui-heading { font-weight: 600; line-height: 1.3; margin: 0; color: var(--text-primary); }
  h1.a2ui-heading { font-size: 15px; }
  h2.a2ui-heading { font-size: 14px; }
  h3.a2ui-heading { font-size: 13px; }
  h4.a2ui-heading { font-size: 12px; text-transform: uppercase; letter-spacing: 0.04em; }
  .a2ui-md { font-size: 13px; line-height: 1.55; color: var(--text-primary); }
  .a2ui-md p { margin: 0 0 6px; }
  .a2ui-md p:last-child { margin-bottom: 0; }
  .a2ui-md code,
  .a2ui-md-code {
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
    font-size: 11.5px;
    background: var(--md-code-bg);
    border: 1px solid var(--md-code-border);
    border-radius: 4px;
    padding: 1px 5px;
  }
  .a2ui-md pre,
  .a2ui-md-pre {
    background: var(--md-pre-bg);
    border: 1px solid var(--md-code-border);
    border-radius: 8px;
    padding: 8px 10px;
    margin: 5px 0;
    overflow-x: auto;
    font-size: 11.5px;
  }
  .a2ui-md a { color: var(--accent); text-decoration: none; }
  .a2ui-md ul, .a2ui-md ol { margin: 4px 0 6px 18px; padding: 0; }
  .a2ui-md li { margin: 2px 0; }
  .a2ui-md blockquote,
  .a2ui-md-quote {
    border-left: 3px solid var(--md-blockquote);
    margin: 6px 0;
    padding: 3px 0 3px 10px;
    color: var(--text-secondary);
  }
  .a2ui-md a:hover { text-decoration: underline; }
  .a2ui-img { max-width: 100%; border-radius: 6px; display: block; }
  .a2ui-field {
    display: flex;
    flex-direction: column;
    gap: 3px;
    font-size: 12px;
  }
  .a2ui-label {
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--text-tertiary);
  }
  .a2ui-field input[type="text"],
  .a2ui-field input[type="email"],
  .a2ui-field input[type="password"],
  .a2ui-field input[type="number"],
  .a2ui-field input[type="tel"],
  .a2ui-field input[type="date"],
  .a2ui-field input[type="datetime-local"],
  .a2ui-field input[type="time"],
  .a2ui-field textarea,
  .a2ui-field select {
    padding: 6px 9px;
    border-radius: 7px;
    border: 1px solid var(--input-border);
    background: var(--input-bg);
    color: var(--text-primary);
    font-family: inherit;
    font-size: 12.5px;
    outline: none;
    box-shadow: var(--input-shadow);
    transition: border-color 0.12s, box-shadow 0.12s;
  }
  .a2ui-field textarea { resize: vertical; min-height: 60px; }
  .a2ui-field input:focus,
  .a2ui-field textarea:focus,
  .a2ui-field select:focus {
    border-color: var(--input-focus-border);
    box-shadow: 0 0 0 2.5px rgba(0,122,255,0.13);
  }
  .a2ui-check {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    cursor: pointer;
    font-size: 12.5px;
    color: var(--text-primary);
  }
  .a2ui-check input { margin: 0; cursor: pointer; }
  .a2ui-slider-row { display: flex; align-items: center; gap: 8px; }
  .a2ui-slider-row input[type="range"] { flex: 1; accent-color: var(--a2ui-primary, var(--accent)); }
  .a2ui-slider-val {
    font-variant-numeric: tabular-nums;
    color: var(--text-secondary);
    font-size: 11px;
    min-width: 24px;
    text-align: right;
  }
  .a2ui-list { display: flex; gap: 6px; }
  .a2ui-list-vertical { flex-direction: column; }
  .a2ui-list-horizontal { flex-direction: row; flex-wrap: wrap; }
  .a2ui-divider { background: var(--divider); margin: 4px 0; }
  .a2ui-divider-horizontal { height: 1px; width: 100%; }
  .a2ui-divider-vertical { width: 1px; align-self: stretch; min-height: 16px; margin: 0 4px; }
  .a2ui-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 18px;
    padding: 0 6px;
    height: 18px;
    border-radius: 4px;
    background: var(--md-code-bg);
    color: var(--text-secondary);
    font-size: 10px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }
  .a2ui-video, .a2ui-audio audio { max-width: 100%; border-radius: 8px; display: block; }
  .a2ui-audio { display: flex; flex-direction: column; gap: 4px; }
  /* Text variants (official v0.9: h1-h5, caption, body) */
  .a2ui-text-h1 { font-size: 17px; font-weight: 600; line-height: 1.2; margin: 0; }
  .a2ui-text-h2 { font-size: 15px; font-weight: 600; line-height: 1.25; margin: 0; }
  .a2ui-text-h3 { font-size: 14px; font-weight: 600; line-height: 1.3; margin: 0; }
  .a2ui-text-h4 { font-size: 13px; font-weight: 600; line-height: 1.3; margin: 0; }
  .a2ui-text-h5 { font-size: 12px; font-weight: 600; line-height: 1.35; text-transform: uppercase; letter-spacing: 0.04em; }
  .a2ui-text-caption { font-size: 11px; color: var(--text-secondary); line-height: 1.4; }
  .a2ui-text-body { font-size: 13px; line-height: 1.55; color: var(--text-primary); }
  /* Image variants */
  .a2ui-img-icon { width: 16px; height: 16px; }
  .a2ui-img-avatar { width: 36px; height: 36px; border-radius: 50%; object-fit: cover; }
  .a2ui-img-smallFeature  { max-height: 80px;  width: auto; }
  .a2ui-img-mediumFeature { max-height: 160px; width: auto; }
  .a2ui-img-largeFeature  { max-height: 240px; width: auto; }
  .a2ui-img-header { width: 100%; max-height: 180px; object-fit: cover; }
  /* Choice variants */
  .a2ui-choice .a2ui-check-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 6px 8px;
    border-radius: 8px;
    border: 1px solid var(--input-border);
    background: var(--input-bg);
  }
  .a2ui-chip-row { display: flex; flex-wrap: wrap; gap: 6px; }
  .a2ui-chip {
    padding: 4px 10px;
    border-radius: 999px;
    border: 1px solid var(--input-border);
    background: var(--input-bg);
    color: var(--text-primary);
    font-size: 11.5px;
    cursor: pointer;
    transition: background 0.12s, border-color 0.12s, color 0.12s;
  }
  .a2ui-chip:hover { border-color: var(--input-focus-border); }
  .a2ui-chip.on {
    background: var(--a2ui-primary, var(--accent));
    border-color: var(--a2ui-primary, var(--accent));
    color: white;
  }
  /* Tabs */
  .a2ui-tabs { display: flex; flex-direction: column; gap: 6px; }
  .a2ui-tabs-bar {
    display: flex;
    gap: 2px;
    padding: 2px;
    border-radius: 8px;
    background: var(--md-code-bg);
    border: 1px solid var(--md-code-border);
  }
  .a2ui-tab {
    flex: 1;
    padding: 5px 10px;
    border: none;
    background: transparent;
    color: var(--text-secondary);
    font-family: inherit;
    font-size: 11.5px;
    font-weight: 600;
    border-radius: 6px;
    cursor: pointer;
    transition: background 0.12s, color 0.12s;
  }
  .a2ui-tab:hover { color: var(--text-primary); }
  .a2ui-tab.active {
    background: var(--glass-solid);
    color: var(--text-primary);
    box-shadow: 0 1px 3px rgba(0,0,0,0.06);
  }
  @media (prefers-color-scheme: dark) {
    .a2ui-tab.active { box-shadow: 0 1px 3px rgba(0,0,0,0.35); }
  }
  .a2ui-tabs-pane { padding: 4px 2px; }
  /* Modal */
  .a2ui-modal-trigger-wrap { display: inline-flex; }
  .a2ui-modal-overlay {
    position: fixed;
    inset: 0;
    background: rgba(0,0,0,0.42);
    z-index: 9998;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 24px;
    animation: a2ui-modal-fade 0.18s ease-out;
  }
  @keyframes a2ui-modal-fade { from { opacity: 0; } to { opacity: 1; } }
  .a2ui-modal-panel {
    position: relative;
    max-width: 560px;
    width: 100%;
    max-height: 80vh;
    overflow: auto;
    border-radius: 14px;
    background: var(--glass-solid);
    border: 1px solid var(--glass-border);
    box-shadow: var(--glass-shadow);
    padding: 16px 16px 14px;
  }
  .a2ui-modal-close {
    position: absolute;
    top: 6px;
    right: 8px;
    width: 24px;
    height: 24px;
    border: none;
    border-radius: 50%;
    background: transparent;
    color: var(--text-tertiary);
    font-size: 18px;
    line-height: 1;
    cursor: pointer;
  }
  .a2ui-modal-close:hover { background: var(--md-code-bg); color: var(--text-primary); }
  .a2ui-btn {
    padding: 6px 13px;
    border-radius: 8px;
    border: 1px solid transparent;
    font-family: inherit;
    font-size: 12.5px;
    font-weight: 600;
    cursor: pointer;
    transition: filter 0.12s, box-shadow 0.12s, transform 0.08s;
    color: white;
  }
  .a2ui-btn:active:not(:disabled) { transform: translateY(0.5px); }
  .a2ui-btn:disabled { opacity: 0.4; cursor: not-allowed; }
  .a2ui-btn.primary { background: var(--a2ui-primary, var(--accent)); }
  .a2ui-btn.primary:hover:not(:disabled) { filter: brightness(1.07); box-shadow: 0 1px 6px rgba(0,122,255,0.22); }
  .a2ui-btn.success { background: var(--dot-ok); }
  .a2ui-btn.warn    { background: var(--dot-wait); }
  .a2ui-btn.danger  { background: var(--dot-err); }
  .a2ui-btn.success:hover:not(:disabled),
  .a2ui-btn.warn:hover:not(:disabled),
  .a2ui-btn.danger:hover:not(:disabled) { filter: brightness(1.07); }
  .a2ui-unknown {
    padding: 4px 8px;
    border-radius: 5px;
    background: var(--error-bg);
    color: var(--error-text);
    font-size: 11px;
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace;
  }
  .a2ui-toast {
    position: fixed;
    left: 50%;
    bottom: 24px;
    transform: translateX(-50%);
    padding: 7px 13px;
    border-radius: 8px;
    background: rgba(0,0,0,0.82);
    color: white;
    font-size: 12px;
    z-index: 9999;
    box-shadow: 0 4px 16px rgba(0,0,0,0.32);
    pointer-events: none;
    opacity: 0;
    transition: opacity 0.18s;
  }
  .a2ui-toast.show { opacity: 1; }

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

  /* ── Command output (Tahoe — minimal chrome, hairline dividers) ─── */
  .cmd-output {
    font-size: 11.5px;
    line-height: 1.45;
    color: var(--text-primary);
  }
  /* Subtle slash-tag, not a banner */
  .cmd-output-header {
    display: inline-block;
    font-family: 'SF Mono', Monaco, monospace;
    font-size: 10px;
    font-weight: 600;
    color: var(--text-tertiary);
    margin-bottom: 6px;
    letter-spacing: 0;
    text-transform: none;
  }
  /* Generic fallback table — flat, no borders */
  .cmd-output-table { width: 100%; border-collapse: collapse; }
  .cmd-output-table tr { border-bottom: 1px solid var(--divider); }
  .cmd-output-table tr:last-child { border-bottom: none; }
  .cmd-output-key {
    padding: 3px 10px 3px 0;
    color: var(--text-tertiary);
    font-size: 10.5px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    white-space: nowrap;
    vertical-align: top;
    width: 1%;
  }
  .cmd-output-val {
    padding: 3px 0;
    color: var(--text-primary);
    font-size: 11.5px;
    word-break: break-word;
  }
  .cmd-output-val.null { color: var(--text-tertiary); font-style: italic; }
  .cmd-output-val.bool-true { color: #34c759; }
  .cmd-output-val.bool-false { color: var(--text-tertiary); }
  .cmd-output-val.number { font-variant-numeric: tabular-nums; }
  .cmd-output-nested {
    margin: 2px 0;
    padding: 2px 0 2px 8px;
    border-left: 1px solid var(--divider);
    font-size: 11px;
  }
  .cmd-output-list { margin: 0; padding-left: 16px; }
  .cmd-output-list li { margin: 1px 0; font-size: 11.5px; }
  .cmd-output-single-list { display: flex; flex-direction: column; gap: 2px; }
  .cmd-output-list-label {
    font-size: 10px;
    font-weight: 600;
    color: var(--text-tertiary);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }

  /* Section title — small caps hairline label */
  .cmd-section-title {
    font-size: 9.5px;
    font-weight: 700;
    color: var(--text-tertiary);
    text-transform: uppercase;
    letter-spacing: 0.07em;
    margin: 10px 0 4px;
  }
  .cmd-section-title:first-child { margin-top: 0; }

  /* Switch — single inline line, no card */
  .cmd-switch {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
    font-size: 11.5px;
  }
  .cmd-switch-label {
    font-size: 9.5px;
    font-weight: 700;
    color: var(--text-tertiary);
    text-transform: uppercase;
    letter-spacing: 0.07em;
    margin-right: 2px;
  }
  .cmd-arrow {
    color: var(--text-tertiary);
    font-size: 11px;
  }
  .cmd-current {
    font-size: 10px;
    color: var(--text-tertiary);
    margin-left: 2px;
    font-style: italic;
  }

  /* Pills — mono token highlights */
  .cmd-pill {
    display: inline-block;
    padding: 1px 6px;
    border-radius: 4px;
    background: var(--divider);
    color: var(--text-primary);
    font-size: 11px;
    font-weight: 500;
    font-family: 'SF Mono', Monaco, monospace;
  }
  .cmd-pill.accent {
    background: rgba(0, 122, 255, 0.12);
    color: var(--accent);
    font-weight: 600;
  }
  .cmd-pill.muted {
    background: transparent;
    color: var(--text-tertiary);
    text-decoration: line-through;
    text-decoration-color: var(--text-tertiary);
  }

  /* Stats — inline strip with vertical hairline separators */
  .cmd-stats {
    display: flex;
    flex-wrap: wrap;
    gap: 0;
    align-items: stretch;
    margin: 2px 0;
  }
  .cmd-stat {
    display: flex;
    flex-direction: column;
    padding: 2px 12px;
    border-right: 1px solid var(--divider);
    min-width: 0;
  }
  .cmd-stat:first-child { padding-left: 0; }
  .cmd-stat:last-child { border-right: none; padding-right: 0; }
  .cmd-stat-label {
    font-size: 9px;
    font-weight: 700;
    color: var(--text-tertiary);
    text-transform: uppercase;
    letter-spacing: 0.07em;
    line-height: 1.2;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .cmd-stat-val {
    font-size: 12.5px;
    font-weight: 600;
    color: var(--text-primary);
    font-variant-numeric: tabular-nums;
    line-height: 1.3;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .cmd-stat.accent .cmd-stat-val { color: var(--accent); }
  .cmd-stat.success .cmd-stat-val { color: #34c759; }
  .cmd-stat-sub {
    font-size: 9.5px;
    color: var(--text-tertiary);
    line-height: 1.2;
  }

  /* Chips — flat, mono, tight */
  .cmd-chips {
    display: flex;
    flex-wrap: wrap;
    gap: 3px;
  }
  .cmd-chip {
    display: inline-block;
    padding: 1px 6px;
    border-radius: 4px;
    background: var(--divider);
    color: var(--text-secondary);
    font-size: 10.5px;
    font-family: 'SF Mono', Monaco, monospace;
    border: none;
  }
  .cmd-chip.active {
    background: rgba(0, 122, 255, 0.14);
    color: var(--accent);
    font-weight: 600;
  }

  /* Items — hairline rows, no card per item */
  .cmd-items {
    display: flex;
    flex-direction: column;
  }
  .cmd-item-row {
    display: flex;
    align-items: baseline;
    gap: 8px;
    padding: 4px 0;
    border-bottom: 1px solid var(--divider);
    font-size: 11.5px;
  }
  .cmd-item-row:last-child { border-bottom: none; }
  .cmd-item-row.stack {
    flex-direction: column;
    align-items: stretch;
    gap: 2px;
  }
  .cmd-item-head {
    display: flex;
    align-items: baseline;
    gap: 6px;
    flex-wrap: wrap;
  }
  .cmd-item-name {
    font-size: 11.5px;
    font-weight: 600;
    color: var(--text-primary);
    font-family: 'SF Mono', Monaco, monospace;
    flex-shrink: 0;
  }
  .cmd-item-row:not(.stack) .cmd-item-name {
    min-width: 90px;
  }
  .cmd-item-desc {
    font-size: 11px;
    color: var(--text-secondary);
    line-height: 1.45;
    flex: 1;
  }
  .cmd-item-meta {
    font-size: 9.5px;
    color: var(--text-tertiary);
  }

  /* Badges — small, flat */
  .cmd-badge {
    display: inline-block;
    padding: 0 5px;
    border-radius: 3px;
    font-size: 9.5px;
    font-weight: 700;
    text-transform: uppercase;
    letter-spacing: 0.05em;
    line-height: 14px;
    vertical-align: middle;
  }
  .cmd-badge.ok      { background: rgba(52, 199, 89, 0.16);  color: #1f8f3a; }
  .cmd-badge.warn    { background: rgba(255, 149, 0, 0.16);  color: #b65b00; }
  .cmd-badge.err     { background: rgba(255, 59, 48, 0.16);  color: #c1271d; }
  .cmd-badge.info    { background: rgba(0, 122, 255, 0.14);  color: var(--accent); }
  .cmd-badge.muted   { background: var(--divider);           color: var(--text-tertiary); }
  @media (prefers-color-scheme: dark) {
    .cmd-badge.ok    { color: #69db7c; }
    .cmd-badge.warn  { color: #ffb454; }
    .cmd-badge.err   { color: #ff6961; }
  }

  /* Toast — inline glyph + text, no panel */
  .cmd-toast {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font-size: 11.5px;
    color: var(--text-primary);
  }
  .cmd-toast-icon {
    color: #34c759;
    font-weight: 700;
    font-size: 13px;
    line-height: 1;
  }
  .cmd-toast.err .cmd-toast-icon { color: #ff3b30; }

  /* Empty state — quiet hairline note */
  .cmd-empty {
    padding: 4px 0;
    color: var(--text-tertiary);
    font-size: 11px;
    font-style: italic;
  }

  /* Tool tags inline (mcp servers) */
  .cmd-tools-inline {
    display: flex;
    flex-wrap: wrap;
    gap: 3px;
  }
  .cmd-tool-tag {
    font-size: 9.5px;
    padding: 0 5px;
    border-radius: 3px;
    background: var(--divider);
    color: var(--text-tertiary);
    font-family: 'SF Mono', Monaco, monospace;
    line-height: 14px;
  }

  /* Markdown passthrough — tighter typography */
  .cmd-md {
    font-size: 11.5px;
    line-height: 1.5;
  }
  .cmd-md p { margin: 4px 0; }
  .cmd-md h1, .cmd-md h2, .cmd-md h3 {
    font-size: 12px;
    margin: 8px 0 4px;
    font-weight: 600;
  }
  .cmd-md table {
    width: 100%;
    border-collapse: collapse;
    font-size: 11px;
    margin: 4px 0;
  }
  .cmd-md th, .cmd-md td {
    padding: 3px 6px;
    border-bottom: 1px solid var(--divider);
    text-align: left;
  }
  .cmd-md th {
    color: var(--text-tertiary);
    font-weight: 700;
    font-size: 9.5px;
    text-transform: uppercase;
    letter-spacing: 0.05em;
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
    <button id="fullscreen-btn" title="Toggle fullscreen (⌘⇧Return)" aria-label="Toggle assistant fullscreen">
      <svg class="ic-enter" width="10" height="10" viewBox="0 0 10 10" fill="none">
        <path d="M1 4V1h3M9 4V1H6M1 6v3h3M9 6v3H6"
              stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/>
      </svg>
      <svg class="ic-exit" width="10" height="10" viewBox="0 0 10 10" fill="none" style="display:none">
        <path d="M4 1v3H1M6 1v3h3M4 9V6H1M6 9V6h3"
              stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/>
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
      <div class="shortcut-row"><kbd>⌘⇧↩</kbd> <span>Fullscreen assistant</span></div>
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

  const MAX_SESSIONS = /* MAX_SESSIONS */;
  const MAX_QUEUE    = 2;
  const MAX_PROMPT_HISTORY = /* MAX_PROMPT_HISTORY */;

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
      // `busy` = agent is processing a prompt right now (set on dispatch,
      // cleared on Done/Cancelled/Error). Drives queue logic and the stop
      // button. NOT the same as `isThinking` (the activity spinner UI),
      // which chunks may hide while the agent is still busy streaming.
      busy: false,
      isThinking: false,
      // tool tracking
      toolCount: 0,
      toolDetails: [],
      toolRows: {},
      // per-message tool details (keyed by message wrap element) — drives
      // the tool details modal opened by clicking the "N tools" label.
      messageToolDetails: new WeakMap(),
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
    sendBtn.classList.toggle('stop-mode', s.busy);
    sendBtn.title = s.busy ? 'Stop' : 'Send (Return)';
    renderQueue();
    updateInputLock();
    updateWelcome();
    scrollToBottom();
    // Focus always lands in the prompt input after any session switch (manual
    // tab click, Tab/Shift+Tab cycling, or Rust-driven switch after creating
    // a new session via ⌘T). Restore caret to the saved selection range.
    input.focus();
    try {
      input.selectionStart = s.inputSelectionStart || input.value.length;
      input.selectionEnd   = s.inputSelectionEnd   || input.value.length;
    } catch (_) {}
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
  // sid=1 find a target. If Rust restored persisted sessions from disk, it
  // will push __addSession / __renameSession / __replayMessages on
  // `sidebar_ready` to override this default.
  (function initDefault() {
    window.__addSession(1, 'Assistant', 'octoweb:assistant', 'connecting');
    switchTo(1);
  })();

  // Tell Rust the JS layer is initialized. Rust uses this signal to push
  // persisted-session restore (additional sessions, renamed titles, replayed
  // messages, last-active session). Fires once per sidebar webview lifetime
  // (the webview survives toggle visibility — JS state is not re-run).
  window.ipc.postMessage(JSON.stringify({ type: 'sidebar_ready' }));

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
    // Clearing the session restarts the agent on Rust side — drop busy and any
    // queued prompts so we don't dispatch stale messages into the new run.
    active.busy = false;
    active.msgQueue = [];
    renderQueue();
    updateInputLock();
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
  // During streaming, the accumulated text may contain an unclosed fenced
  // code block (``` opened but closing ``` not yet received). marked.js
  // renders that as broken HTML — the fence line becomes visible plain text
  // and the content leaks out unstyled. We detect this and close the fence
  // before parsing so the partial block renders correctly at every chunk.
  function closeUnclosedFences(text) {
    // Count ``` fence openings vs closings. A fence line starts with optional
    // whitespace then 3+ backticks. We track open/close pairs; if an odd
    // number of fence markers exist the last one is unclosed.
    const lines = text.split('\n');
    let inFence = false;
    let fenceLang = '';
    for (const line of lines) {
      const m = line.match(/^[ \t]*(`{3,})(.*)/);
      if (m) {
        if (!inFence) {
          inFence = true;
          fenceLang = m[2].trim();
        } else {
          inFence = false;
          fenceLang = '';
        }
      }
    }
    // If we ended inside a fence, append a closing marker so marked sees
    // a complete block. The trailing newline is required by marked's lexer.
    return inFence ? text + '\n```' : text;
  }

  function renderMd(raw) {
    if (typeof marked === 'undefined') return escapeHtml(raw);
    return marked.parse(closeUnclosedFences(raw));
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

  // ── Per-command beautiful renderers ─────────────────────────────────
  // Each renderer returns inner HTML for the body of the .cmd-output card.
  // Schemas were captured by probing `octomind acp assistant:general`.
  // Unknown command_type falls back to the generic key/value table.
  function fmtTokens(n) {
    if (n == null) return '0';
    n = Number(n);
    if (n >= 1e9) return (n / 1e9).toFixed(2) + 'B';
    if (n >= 1e6) return (n / 1e6).toFixed(2) + 'M';
    if (n >= 1e3) return (n / 1e3).toFixed(1) + 'K';
    return String(n);
  }
  function fmtCost(n) {
    var v = Number(n) || 0;
    if (v === 0) return '$0';
    if (v < 0.01) return '$' + v.toFixed(5);
    return '$' + v.toFixed(4);
  }
  function fmtMs(ms) {
    var v = Number(ms) || 0;
    if (v < 1000) return v + 'ms';
    if (v < 60000) return (v / 1000).toFixed(1) + 's';
    if (v < 3600000) return Math.floor(v / 60000) + 'm ' + Math.floor((v % 60000) / 1000) + 's';
    return Math.floor(v / 3600000) + 'h ' + Math.floor((v % 3600000) / 60000) + 'm';
  }
  function statTile(label, val, opts) {
    opts = opts || {};
    var cls = 'cmd-stat' + (opts.cls ? ' ' + opts.cls : '');
    var sub = opts.sub ? '<div class="cmd-stat-sub">' + escapeHtml(opts.sub) + '</div>' : '';
    return '<div class="' + cls + '">' +
      '<div class="cmd-stat-label">' + escapeHtml(label) + '</div>' +
      '<div class="cmd-stat-val">' + escapeHtml(String(val)) + '</div>' + sub +
      '</div>';
  }
  function switchCard(label, oldVal, newVal, changed, _icon) {
    var flow;
    if (changed && oldVal && newVal && oldVal !== newVal) {
      flow = '<span class="cmd-pill muted">' + escapeHtml(String(oldVal)) + '</span>' +
             '<span class="cmd-arrow">→</span>' +
             '<span class="cmd-pill accent">' + escapeHtml(String(newVal)) + '</span>';
    } else {
      var cur = newVal || oldVal || '—';
      flow = '<span class="cmd-pill accent">' + escapeHtml(String(cur)) + '</span>' +
             (changed ? '' : '<span class="cmd-current">unchanged</span>');
    }
    return '<div class="cmd-switch">' +
      '<span class="cmd-switch-label">' + escapeHtml(label) + '</span>' + flow +
    '</div>';
  }
  function toast(msg, isErr) {
    return '<div class="cmd-toast' + (isErr ? ' err' : '') + '">' +
      '<span class="cmd-toast-icon">' + (isErr ? '✕' : '✓') + '</span>' +
      '<span>' + escapeHtml(String(msg)) + '</span>' +
    '</div>';
  }
  function emptyState(msg) {
    return '<div class="cmd-empty">' + escapeHtml(msg) + '</div>';
  }

  var CMD_RENDERERS = {
    model: function(o) {
      return switchCard('Model', o.old_model, o.new_model, !!o.changed, 'M');
    },
    role: function(o) {
      var html = switchCard('Role', o.old_role, o.new_role || o.current_role, !!o.changed, 'R');
      if (Array.isArray(o.available_roles) && o.available_roles.length) {
        html += '<div class="cmd-section-title">Available roles</div>';
        html += '<div class="cmd-chips">';
        for (var i = 0; i < o.available_roles.length; i++) {
          var r = o.available_roles[i];
          var active = (r === (o.current_role || o.new_role));
          html += '<span class="cmd-chip' + (active ? ' active' : '') + '">' + escapeHtml(r) + '</span>';
        }
        html += '</div>';
      }
      return html;
    },
    effort: function(o) {
      return switchCard('Effort', o.old_effort, o.new_effort, !!o.changed, 'E');
    },
    loglevel: function(o) {
      var html = switchCard('Log level', o.old_level, o.new_level || o.current_level, !!o.changed, 'L');
      if (Array.isArray(o.available_levels) && o.available_levels.length) {
        html += '<div class="cmd-section-title">Available levels</div>';
        html += '<div class="cmd-chips">';
        for (var i = 0; i < o.available_levels.length; i++) {
          var lv = o.available_levels[i];
          var active = (lv === (o.current_level || o.new_level));
          html += '<span class="cmd-chip' + (active ? ' active' : '') + '">' + escapeHtml(lv) + '</span>';
        }
        html += '</div>';
      }
      return html;
    },
    info: function(o) {
      var stats = '<div class="cmd-stats">';
      stats += statTile('Model', o.model || '—', { cls: 'accent' });
      stats += statTile('Role', o.role || '—');
      stats += statTile('Total tokens', fmtTokens(o.tokens_used), { cls: 'accent' });
      stats += statTile('Cost', fmtCost(o.total_cost), { cls: 'success' });
      stats += statTile('Input', fmtTokens(o.tokens_input));
      stats += statTile('Output', fmtTokens(o.tokens_output));
      stats += statTile('Cached', fmtTokens(o.tokens_cached));
      stats += statTile('Cache write', fmtTokens(o.tokens_cache_write));
      if (o.tokens_reasoning) stats += statTile('Reasoning', fmtTokens(o.tokens_reasoning));
      if (o.tokens_per_second) stats += statTile('Tokens/sec', Number(o.tokens_per_second).toFixed(1));
      if (o.cache_savings) stats += statTile('Cache savings', fmtCost(o.cache_savings));
      stats += '</div>';
      var meta = '<div class="cmd-section-title">Session</div>';
      meta += '<div class="cmd-stats">';
      meta += statTile('Name', o.session_name || '—');
      if (o.cache_markers_system != null) meta += statTile('Sys markers', String(o.cache_markers_system));
      if (o.cache_markers_tool != null) meta += statTile('Tool markers', String(o.cache_markers_tool));
      if (o.cache_markers_content != null) meta += statTile('Content markers', String(o.cache_markers_content));
      meta += '</div>';
      return stats + meta;
    },
    copy: function(o) {
      if (!o.copied) return toast('Nothing to copy', true);
      return toast('Copied ' + (o.length || 0) + ' chars to clipboard');
    },
    schedule: function(o) {
      var d = o.data || {};
      var msg = String(d.message || '');
      if (d.is_error) return toast(msg, true);
      var lines = msg.split('\n').map(escapeHtml).join('<br>');
      return '<div class="cmd-toast"><span class="cmd-toast-icon" style="color:var(--accent)">⏱</span><span>' + lines + '</span></div>';
    },
    help: function(o) {
      var arr = Array.isArray(o.commands) ? o.commands : [];
      if (!arr.length) return emptyState('No commands available');
      var html = '<div class="cmd-section-title">' + arr.length + ' commands</div>';
      html += '<div class="cmd-chips">';
      for (var i = 0; i < arr.length; i++) {
        html += '<span class="cmd-chip">' + escapeHtml(arr[i]) + '</span>';
      }
      html += '</div>';
      return html;
    },
    list: function(o) {
      var md = String(o.plain_text || '');
      var html = '<div class="cmd-md">' + (typeof marked !== 'undefined' ? marked.parse(md) : escapeHtml(md)) + '</div>';
      if (o.page) html += '<div class="cmd-stat-sub">Page ' + escapeHtml(String(o.page)) + '</div>';
      return html;
    },
    run: function(o) {
      var d = o.data || {};
      if (o.command_executed) {
        return toast('Ran command: ' + o.command_executed);
      }
      var cmds = Array.isArray(d.commands) ? d.commands : [];
      var html = '<div class="cmd-section-title">' + escapeHtml(d.message || 'Available') + '</div>';
      if (!cmds.length) return html + emptyState('No commands defined');
      html += '<div class="cmd-chips">';
      for (var i = 0; i < cmds.length; i++) {
        html += '<span class="cmd-chip">' + escapeHtml(cmds[i]) + '</span>';
      }
      html += '</div>';
      return html;
    },
    workflow: function(o) {
      var d = o.data || {};
      if (o.workflow_executed) return toast('Ran workflow: ' + o.workflow_executed);
      var wfs = Array.isArray(d.workflows) ? d.workflows : [];
      var html = '<div class="cmd-section-title">' + escapeHtml(d.message || 'Workflows') + '</div>';
      if (!wfs.length) return html + emptyState('No workflows defined');
      html += '<div class="cmd-items">';
      for (var i = 0; i < wfs.length; i++) {
        var w = wfs[i];
        var name = Array.isArray(w) ? w[0] : (w.name || '');
        var desc = Array.isArray(w) ? (w[1] || '') : (w.description || '');
        html += '<div class="cmd-item-row">' +
          '<span class="cmd-item-name">' + escapeHtml(name) + '</span>' +
          '<span class="cmd-item-desc">' + escapeHtml(desc) + '</span>' +
        '</div>';
      }
      html += '</div>';
      return html;
    },
    mcp: function(o) {
      var d = o.data || {};
      var srv = Array.isArray(d.servers) ? d.servers : [];
      if (!srv.length) return emptyState('No MCP servers configured');
      var html = '<div class="cmd-section-title">' + srv.length + ' servers</div>';
      html += '<div class="cmd-items">';
      for (var i = 0; i < srv.length; i++) {
        var s = srv[i];
        var hKind = (s.health === 'running' ? 'ok' : (s.health === 'failed' || s.health === 'error' ? 'err' : 'warn'));
        var tools = Array.isArray(s.tools) ? s.tools : [];
        var meta = [];
        if (s.connection_type) meta.push(s.connection_type);
        if (s.restart_count) meta.push(s.restart_count + ' restarts');
        if (s.consecutive_failures) meta.push(s.consecutive_failures + ' failures');
        html += '<div class="cmd-item-row stack">' +
          '<div class="cmd-item-head">' +
            '<span class="cmd-item-name">' + escapeHtml(s.name || '?') + '</span>' +
            '<span class="cmd-badge ' + hKind + '">' + escapeHtml(s.health || 'unknown') + '</span>' +
            (meta.length ? '<span class="cmd-item-meta">' + escapeHtml(meta.join(' · ')) + '</span>' : '') +
          '</div>';
        if (tools.length) {
          html += '<div class="cmd-tools-inline">';
          for (var j = 0; j < tools.length; j++) {
            html += '<span class="cmd-tool-tag">' + escapeHtml(String(tools[j])) + '</span>';
          }
          html += '</div>';
        }
        html += '</div>';
      }
      html += '</div>';
      return html;
    },
    plan: function(o) {
      if (!o.has_plan) {
        return '<div class="cmd-empty">' + escapeHtml(o.display || 'No active plan') + '</div>';
      }
      var planText = '';
      if (typeof o.plan === 'string') planText = o.plan;
      else if (o.plan) planText = JSON.stringify(o.plan, null, 2);
      return '<div class="cmd-md">' + (typeof marked !== 'undefined' ? marked.parse(planText) : escapeHtml(planText)) + '</div>';
    },
    prompt: function(o) {
      var d = o.data || {};
      var prompts = Array.isArray(d.prompts) ? d.prompts : [];
      if (!prompts.length) return emptyState('No prompts available');
      var html = '<div class="cmd-section-title">' + prompts.length + ' prompt templates</div>';
      html += '<div class="cmd-items">';
      for (var i = 0; i < prompts.length; i++) {
        var p = prompts[i];
        html += '<div class="cmd-item-row">' +
          '<span class="cmd-item-name">' + escapeHtml(p.name || '') + '</span>' +
          '<span class="cmd-item-desc">' + escapeHtml(p.description || '') + '</span>' +
        '</div>';
      }
      html += '</div>';
      return html;
    },
    skill: function(o) {
      var d = o.data || {};
      var skills = Array.isArray(d.skills) ? d.skills : [];
      var html = '<div class="cmd-stats">';
      html += statTile('Total', skills.length, { cls: 'accent' });
      if (d.active_count != null) html += statTile('Active', d.active_count, { cls: 'success' });
      if (d.page) html += statTile('Page', d.page);
      if (d.pattern) html += statTile('Filter', d.pattern);
      html += '</div>';
      if (!skills.length) return html + emptyState('No skills');
      html += '<div class="cmd-section-title">Skills</div>';
      html += '<div class="cmd-items">';
      for (var i = 0; i < skills.length; i++) {
        var s = skills[i];
        var doms = Array.isArray(s.domains) && s.domains.length ? s.domains.join(', ') : '';
        html += '<div class="cmd-item-row stack">' +
          '<div class="cmd-item-head">' +
            '<span class="cmd-item-name">' + escapeHtml(s.name || '') + '</span>' +
            '<span class="cmd-badge ' + (s.active ? 'ok' : 'muted') + '">' + (s.active ? 'on' : 'off') + '</span>' +
            (doms ? '<span class="cmd-item-meta">' + escapeHtml(doms) + '</span>' : '') +
          '</div>' +
          '<div class="cmd-item-desc">' + escapeHtml(s.description || '') + '</div>' +
        '</div>';
      }
      html += '</div>';
      return html;
    },
    report: function(o) {
      var t = o.totals || {};
      var html = '<div class="cmd-stats">';
      html += statTile('Tool calls', t.total_tool_calls != null ? t.total_tool_calls : 0, { cls: 'accent' });
      html += statTile('Total cost', fmtCost(t.total_cost), { cls: 'success' });
      html += statTile('AI time', fmtMs(t.total_ai_time_ms));
      html += statTile('Processing', fmtMs(t.total_processing_time_ms));
      html += statTile('Task time', fmtMs(t.total_task_time_ms));
      html += '</div>';
      var entries = Array.isArray(o.entries) ? o.entries : [];
      if (entries.length) {
        html += '<div class="cmd-section-title">' + entries.length + ' entries</div>';
        html += '<div class="cmd-items">';
        for (var i = 0; i < entries.length; i++) {
          var e = entries[i];
          var name = e.tool || e.name || e.command || ('entry ' + (i + 1));
          var meta = [];
          if (e.calls != null) meta.push(e.calls + ' calls');
          if (e.cost != null) meta.push(fmtCost(e.cost));
          if (e.ai_time_ms != null) meta.push(fmtMs(e.ai_time_ms));
          html += '<div class="cmd-item-row">' +
            '<span class="cmd-item-name">' + escapeHtml(String(name)) + '</span>' +
            '<span class="cmd-item-desc">' + escapeHtml(meta.join(' · ')) + '</span>' +
          '</div>';
        }
        html += '</div>';
      }
      return html;
    },
    context: function(o) {
      var msgs = Array.isArray(o.filtered_messages) ? o.filtered_messages : [];
      var html = '<div class="cmd-stats">';
      html += statTile('Messages', msgs.length, { cls: 'accent' });
      if (o.filter) html += statTile('Filter', o.filter);
      html += '</div>';
      if (!msgs.length) return html + emptyState('No messages in context');
      html += '<div class="cmd-section-title">Context</div>';
      html += '<div class="cmd-items">';
      for (var i = 0; i < Math.min(msgs.length, 50); i++) {
        var m = msgs[i];
        var role = m.role || 'message';
        var content = String(m.content || '');
        var preview = content.length > 220 ? content.slice(0, 220) + '…' : content;
        html += '<div class="cmd-item-row stack">' +
          '<div class="cmd-item-head"><span class="cmd-badge info">' + escapeHtml(role) + '</span></div>' +
          '<div class="cmd-item-desc" style="white-space:pre-wrap">' + escapeHtml(preview) + '</div>' +
        '</div>';
      }
      if (msgs.length > 50) {
        html += '<div class="cmd-stat-sub" style="margin-top:4px">… and ' + (msgs.length - 50) + ' more</div>';
      }
      html += '</div>';
      return html;
    },
  };

  function renderCommandOutput(obj) {
    var cmdType = obj.command_type;
    var header = '<div class="cmd-output-header">/' + escapeHtml(cmdType) + '</div>';
    var renderer = CMD_RENDERERS[cmdType];
    if (renderer) {
      try {
        return '<div class="cmd-output">' + header + renderer(obj) + '</div>';
      } catch (e) {
        // Fall through to generic on renderer error.
      }
    }
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
    return '<div class="cmd-output">' + header + body + '</div>';
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
      // Hide the activity spinner UI now that real output is streaming.
      // Do NOT touch `s.busy` — the agent is still running until Done.
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
      // Same rationale as __appendChunk — spinner hide only, busy stays true.
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

  // `render_ui` is rendered as a full A2UI bubble inline (window.__a2uiUpdate);
  // a tool row alongside it would be redundant chrome, so we drop those.
  function isRenderUiTool(title) {
    return title === 'render_ui' || (typeof title === 'string' && title.indexOf('render_ui') === 0);
  }

  window.__toolStart = function(sid, id, title, kind, rawInput, locations) {
    const s = sessions.get(sid);
    if (!s) return;
    if (isRenderUiTool(title)) {
      s.suppressedToolIds = s.suppressedToolIds || new Set();
      s.suppressedToolIds.add(id);
      return;
    }
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
    if (s.suppressedToolIds && s.suppressedToolIds.has(id)) return;
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
      // Terminal event for this prompt (Done/Cancelled/Error path via main.rs).
      // Clear the busy flag so queued messages can drain and new prompts go
      // straight to dispatch instead of being queued.
      s.busy = false;
      const savedToolCount = s.toolCount;
      const savedToolDetails = [...s.toolDetails];
      clearActivity(s);
      // Synthesize an empty bubble for tool-only turns — otherwise the work
      // disappears with the activity feed and there's no record of it.
      if (!s.currentAgentBubble && savedToolCount > 0) {
        s.currentAgentBubble = startAgentBubble(s);
        s.currentAgentRaw = '';
      }
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

  // Replay persisted messages on cold-start. Rust calls this once per session
  // on sidebar bootstrap with the full message log restored from disk.
  // Entries: { role: 'user'|'agent'|'error'|'ui', text: string, ts?: number,
  //            a2ui?: <envelope-body> }. Skips toolDetails/images — those
  // aren't persisted (live-only).
  window.__replayMessages = function(sid, msgs) {
    const s = sessions.get(sid);
    if (!s || !Array.isArray(msgs) || msgs.length === 0) return;
    for (let i = 0; i < msgs.length; i++) {
      const m = msgs[i];
      const role = m && m.role;
      const text = (m && typeof m.text === 'string') ? m.text : '';
      if (role === 'user' || role === 'error') {
        appendMessage(s, role, text);
      } else if (role === 'agent') {
        const bubble = startAgentBubble(s);
        if (!bubble) continue;
        const cmdObj = tryParseCommandJson(text);
        bubble.innerHTML = cmdObj ? renderCommandOutput(cmdObj) : renderMd(text);
        finishAgentBubble(s, bubble, text, 0, []);
      } else if (role === 'ui' && m && m.a2ui) {
        // Rebuild the A2UI bubble from the persisted envelope body. Replays
        // are ghosts (no live poll) and anchor on the persisted creation ts
        // so the bubble lands in its original chronological spot and shows
        // its original time, not "now".
        window.__a2uiUpdate(sid, text, m.a2ui, false, typeof m.ts === 'number' ? m.ts : 0);
      }
    }
    if (sid === activeSid) { updateWelcome(); scrollToBottom(); }
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
    // Mark the session busy BEFORE flipping the spinner. send() reads `busy`
    // to decide queue-vs-dispatch, and the stop button mirrors `busy`.
    s.busy = true;
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

    if (!active.busy) {
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
    // Don't optimistically clear busy/spinner here — the agent will emit
    // Cancelled which routes through __setThinking(sid, false) and clears
    // busy at the right moment. Clearing early causes a race where the
    // queue drains and a new prompt dispatches BEFORE the cancel lands,
    // then the late Cancelled event clears busy again while the new prompt
    // is still running → parallel dispatch on next send().
  }

  sendBtn.addEventListener('click', () => {
    if (sendBtn.classList.contains('stop-mode')) {
      stop();
    } else {
      send();
    }
  });
  input.addEventListener('keydown', e => {
    // Tab / Shift+Tab cycle ACP sessions. Handled here at the textarea level
    // (not just at document capture) because WKWebView's native Tab focus
    // traversal in <textarea> can race past a document-level capture listener
    // and move focus to other page elements before our handler runs.
    if (e.key === 'Tab' && !e.ctrlKey && !e.metaKey && !e.altKey) {
      if (scPanel.classList.contains('visible')) return;
      e.preventDefault();
      const keys = Array.from(sessions.keys());
      if (keys.length <= 1) { input.focus(); return; }
      const idx = keys.indexOf(activeSid);
      const nextIdx = e.shiftKey
        ? (idx <= 0 ? keys.length - 1 : idx - 1)
        : (idx >= keys.length - 1 ? 0 : idx + 1);
      const nextSid = keys[nextIdx];
      window.ipc.postMessage(JSON.stringify({ type: 'acp_session_switch', session_id: nextSid }));
      input.focus();
      return;
    }
    if (_ph.isInSearchMode()) return;
    // Ctrl+J → insert newline at caret. execCommand('insertText') is the same
    // path WKWebView uses for Shift+Enter — preserves focus, caret, undo,
    // scroll, and fires the 'input' event for auto-resize.
    if (e.ctrlKey && !e.metaKey && !e.altKey && !e.shiftKey && (e.key === 'j' || e.key === 'J' || e.code === 'KeyJ')) {
      e.preventDefault();
      document.execCommand('insertText', false, '\n');
      // Browsers under-measure a trailing "\n" in scrollHeight, so the caret
      // line ends up clipped after auto-resize. Pin the textarea scroll to
      // the caret so the new empty line stays visible — Shift+Enter gets
      // this for free via the native text view path.
      input.scrollTop = input.scrollHeight;
      return;
    }
    // Plain Enter sends. Shift+Enter falls through → native newline.
    if (e.key === 'Enter' && !e.shiftKey && !e.ctrlKey && !e.metaKey && !e.altKey) {
      e.preventDefault();
      send();
      _ph.resetState();
    }
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

  // ── Toggle assistant fullscreen ───────────────────────────────────────
  // Rust owns the actual bounds; this just round-trips the request and
  // swaps the icon glyph when the IPC echoes back via window.__setSidebarFullscreen.
  const fsBtn = document.getElementById('fullscreen-btn');
  const fsEnter = fsBtn.querySelector('.ic-enter');
  const fsExit  = fsBtn.querySelector('.ic-exit');
  fsBtn.addEventListener('click', () => {
    window.ipc.postMessage(JSON.stringify({ type: 'sidebar_fullscreen_toggle' }));
  });
  window.__setSidebarFullscreen = function(on) {
    fsBtn.classList.toggle('active', !!on);
    fsEnter.style.display = on ? 'none' : '';
    fsExit.style.display  = on ? '' : 'none';
    fsBtn.title = on ? 'Exit fullscreen (⌘⇧Return)' : 'Toggle fullscreen (⌘⇧Return)';
  };

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

  // Tab / Shift+Tab fallback for when focus is NOT in the prompt input
  // (e.g. user clicked a session tab header). The textarea's own keydown
  // handler covers the common case; this one catches everything else.
  // Capture phase so we win against any other listener.
  document.addEventListener('keydown', e => {
    if (e.key !== 'Tab' || e.ctrlKey || e.metaKey || e.altKey) return;
    if (e.defaultPrevented) return;
    if (document.activeElement === input) return; // textarea handler already ran
    if (scPanel.classList.contains('visible')) return;
    e.preventDefault();
    const keys = Array.from(sessions.keys());
    if (keys.length <= 1) { input.focus(); return; }
    const idx = keys.indexOf(activeSid);
    const nextIdx = e.shiftKey
      ? (idx <= 0 ? keys.length - 1 : idx - 1)
      : (idx >= keys.length - 1 ? 0 : idx + 1);
    const nextSid = keys[nextIdx];
    window.ipc.postMessage(JSON.stringify({ type: 'acp_session_switch', session_id: nextSid }));
    input.focus();
  }, true);

  // ── A2UI v0.9 renderer ─────────────────────────────────────────────────
  // Inline interactive surfaces produced by the agent's `render_ui` tool.
  // Each envelope file (`~/.local/share/a2ui/<id>.json`) becomes one
  // `.msg.ui` bubble. Button clicks IPC `a2ui_resolve` back to Rust, which
  // writes the resolution into the file and unblocks the tool's bash poll.
  const a2uiBlocks = new Map();           // fileId  -> block state
  const a2uiBubbleByFile = new Map();     // fileId  -> wrapper element
  const a2uiSurfaceIndex = new Map();     // "sid:surfaceId" -> fileId of live block
  function a2uiSurfaceKey(sid, surfaceId) { return sid + ':' + surfaceId; }
  // Peek the surfaceId from any message in the envelope — A2UI v0.9 stamps
  // `surfaceId` on every message kind (createSurface, updateComponents,
  // updateDataModel, deleteSurface). Used to honor "same surfaceId = update
  // existing surface" even when a follow-up envelope only carries components
  // or data updates (no createSurface).
  function a2uiSniffSurfaceId(payload) {
    const msgs = payload && payload.messages;
    if (!Array.isArray(msgs)) return null;
    for (const m of msgs) {
      if (!m || typeof m !== 'object') continue;
      for (const k of ['createSurface', 'updateComponents', 'updateDataModel', 'deleteSurface']) {
        const inner = m[k];
        if (inner && typeof inner.surfaceId === 'string') return inner.surfaceId;
      }
    }
    return null;
  }

  // JSON-Pointer (RFC 6901)
  function a2uiPtrParts(path) {
    return path.split('/').slice(1).map(p => p.replace(/~1/g, '/').replace(/~0/g, '~'));
  }
  function a2uiPtrGet(model, path) {
    if (!path || path === '/') return model;
    const parts = a2uiPtrParts(path);
    let cur = model;
    for (const p of parts) {
      if (cur == null || typeof cur !== 'object') return undefined;
      cur = cur[p];
    }
    return cur;
  }
  function a2uiPtrSet(model, path, value) {
    if (!path || path === '/') return value;
    const parts = a2uiPtrParts(path);
    let cur = model;
    for (let i = 0; i < parts.length - 1; i++) {
      const k = parts[i];
      if (cur[k] == null || typeof cur[k] !== 'object') cur[k] = {};
      cur = cur[k];
    }
    cur[parts[parts.length - 1]] = value;
    return model;
  }

  // Function registry — every call entry the agent can put in a ValueRef.
  const A2UI_FN = {
    required: ({ value }) => value != null && value !== '' && !(Array.isArray(value) && value.length === 0),
    email: ({ value }) => value == null || value === '' || /^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(String(value)),
    numeric: ({ value }) => value == null || value === '' || !isNaN(Number(value)),
    regex: ({ value, pattern }) => {
      if (value == null || value === '') return true;
      try { return new RegExp(String(pattern)).test(String(value)); } catch (e) { return false; }
    },
    length: ({ value, min, max }) => {
      const len = String(value == null ? '' : value).length;
      if (min != null && len < Number(min)) return false;
      if (max != null && len > Number(max)) return false;
      return true;
    },
    range: ({ value, min, max }) => {
      const n = Number(value);
      if (isNaN(n)) return false;
      if (min != null && n < Number(min)) return false;
      if (max != null && n > Number(max)) return false;
      return true;
    },
    and: ({ values }) => Array.isArray(values) && values.every(v => !!v),
    or:  ({ values }) => Array.isArray(values) && values.some(v => !!v),
    not: ({ value }) => !value,
    eq:  ({ a, b }) => a === b || String(a) === String(b),
    neq: ({ a, b }) => !(a === b || String(a) === String(b)),
    formatString: ({ template, args }) => String(template == null ? '' : template).replace(/\{(\w+)\}/g, (_, k) => {
      const m = args || {};
      return m[k] != null ? String(m[k]) : '';
    }),
    formatDate: ({ value, locale }) => {
      if (value == null || value === '') return '';
      const d = new Date(value);
      if (isNaN(d.getTime())) return String(value);
      try { return d.toLocaleDateString(typeof locale === 'string' ? locale : undefined); }
      catch (e) { return d.toISOString().slice(0, 10); }
    },
    formatNumber: ({ value, decimals, locale }) => {
      const n = Number(value);
      if (isNaN(n)) return '';
      try {
        return new Intl.NumberFormat(
          typeof locale === 'string' ? locale : undefined,
          decimals != null ? { minimumFractionDigits: Number(decimals), maximumFractionDigits: Number(decimals) } : {}
        ).format(n);
      } catch (e) {
        return decimals != null ? n.toFixed(Number(decimals)) : String(n);
      }
    },
    formatCurrency: ({ value, currency, locale }) => {
      const n = Number(value);
      if (isNaN(n)) return '';
      try {
        return new Intl.NumberFormat(typeof locale === 'string' ? locale : undefined, {
          style: 'currency',
          currency: typeof currency === 'string' ? currency : 'USD',
        }).format(n);
      } catch (e) { return String(n); }
    },
    // openUrl routes through Rust so we can open the URL as a new browser tab
    // instead of trying to open a window from the sidebar webview.
    openUrl: ({ url }) => {
      const u = String(url == null ? '' : url);
      if (!/^(https?:\/\/|mailto:)/i.test(u)) return false;
      window.ipc.postMessage(JSON.stringify({ type: 'a2ui_open_url', url: u }));
      return true;
    },
  };

  function a2uiResolveValue(v, scope) {
    if (v == null) return v;
    if (typeof v !== 'object') return v;
    if (Array.isArray(v)) return v.map(x => a2uiResolveValue(x, scope));
    if (typeof v.path === 'string') {
      const p = v.path;
      // Inside a List iteration scope, treat "/", "." and "" as "the current
      // item" — that's the natural way to bind a scalar item (e.g. a string
      // in a string[]) into a Text/Image. Without this, agents that write
      // `{path: "/"}` on a list template end up dumping the whole root model
      // into every row.
      if (scope.local != null && (p === '' || p === '/' || p === '.')) {
        return scope.local;
      }
      if (p.charAt(0) === '/') return a2uiPtrGet(scope.root, p);
      if (scope.local != null) return a2uiPtrGet(scope.local, '/' + p);
      return undefined;
    }
    if (typeof v.call === 'string') {
      const fn = A2UI_FN[v.call];
      if (!fn) return undefined;
      const args = {};
      const rawArgs = v.args || {};
      for (const k in rawArgs) args[k] = a2uiResolveValue(rawArgs[k], scope);
      try { return fn(args); } catch (e) { return undefined; }
    }
    return v;
  }

  function a2uiPathOf(v) {
    if (v && typeof v === 'object' && typeof v.path === 'string' && v.path.charAt(0) === '/') {
      return v.path;
    }
    return null;
  }

  // Stringify a resolved value for display in a text/input slot. Avoids the
  // `String({...})` → "[object Object]" trap when the agent points a text
  // binding at an object subtree: we drill into common content fields, fall
  // back to compact JSON, and only ever pass scalars through unchanged.
  function a2uiToStr(v) {
    if (v == null) return '';
    if (typeof v === 'string') {
      // Defensive: some models over-escape and put the literal 2-char "\n"
      // (backslash + n) instead of a real newline. Same for \r\n and \t.
      // Idempotent — if the string already has real newlines, no-op.
      if (v.indexOf('\\') !== -1) {
        return v.replace(/\\r\\n/g, '\n').replace(/\\n/g, '\n').replace(/\\t/g, '\t');
      }
      return v;
    }
    if (typeof v === 'number' || typeof v === 'boolean') return String(v);
    if (Array.isArray(v)) return v.map(a2uiToStr).join(', ');
    if (typeof v === 'object') {
      // Try common text-bearing keys before serializing.
      const keys = ['text', 'label', 'title', 'name', 'value', 'content'];
      for (const k of keys) {
        if (typeof v[k] === 'string') return v[k];
      }
      try { return JSON.stringify(v); } catch (e) { return ''; }
    }
    return String(v);
  }

  // Minimal safe Markdown — escape-then-allow-list. Sufficient for Markdown
  // component content; we don't want to expose unrestricted innerHTML here.
  function a2uiEscapeHtml(s) {
    return String(s == null ? '' : s).replace(/[&<>"']/g, c =>
      c === '&' ? '&amp;' :
      c === '<' ? '&lt;' :
      c === '>' ? '&gt;' :
      c === '"' ? '&quot;' : '&#39;');
  }
  function a2uiRenderMarkdown(src) {
    // Some models over-escape newlines/tabs when writing JSON, sending the
    // literal 2-char sequence "\n" (backslash + n) where they meant a real
    // newline. JSON.parse decodes those to literal backslash-n in the JS
    // string, which leaks into rendered prose / code blocks / blockquotes.
    // Convert defensively before markdown parsing.
    if (typeof src === 'string' && src.indexOf('\\') !== -1) {
      src = src.replace(/\\r\\n/g, '\n').replace(/\\n/g, '\n').replace(/\\t/g, '\t');
    }
    let s = a2uiEscapeHtml(src);
    // Use a placeholder that can't collide with prose ("CB0" did, as you
    // saw at end-of-input where the space-bounded marker matcher failed).
    //   are control chars escapeHtml leaves alone and that
    // never appear in normal text.
    const blocks = [];
    s = s.replace(/```([a-zA-Z0-9_-]*)\n([\s\S]*?)```/g, (_, lang, code) => {
      const idx = blocks.push('<pre class="a2ui-md-pre" data-lang="' + lang + '"><code>' + code + '</code></pre>') - 1;
      return 'CB' + idx + '';
    });
    s = s.replace(/`([^`\n]+?)`/g, '<code class="a2ui-md-code">$1</code>');
    s = s.replace(/^####\s+(.+)$/gm, '<h4>$1</h4>');
    s = s.replace(/^###\s+(.+)$/gm, '<h3>$1</h3>');
    s = s.replace(/^##\s+(.+)$/gm, '<h2>$1</h2>');
    s = s.replace(/^#\s+(.+)$/gm, '<h1>$1</h1>');
    // Blockquote — leading ">", optionally multiple lines.
    s = s.replace(/(?:^&gt;\s?.*(?:\n|$))+/gm, m => {
      const inner = m.split('\n').map(l => l.replace(/^&gt;\s?/, '')).join('<br>').replace(/(<br>)+$/, '');
      return '<blockquote class="a2ui-md-quote">' + inner + '</blockquote>';
    });
    // Bold: ** ** and __ __
    s = s.replace(/\*\*([^*\n]+?)\*\*/g, '<strong>$1</strong>');
    s = s.replace(/__([^_\n]+?)__/g, '<strong>$1</strong>');
    // Italic: * * (not **) and _ _ (not __)
    s = s.replace(/(^|[^*\w])\*([^*\n]+?)\*(?!\*)/g, '$1<em>$2</em>');
    s = s.replace(/(^|[^_\w])_([^_\n]+?)_(?!_)/g, '$1<em>$2</em>');
    s = s.replace(/\[([^\]]+)\]\((https?:\/\/[A-Za-z0-9._~:/?#\[\]@!$&'()*+,;=%-]+|mailto:[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,})\)/g,
      '<a href="$2" target="_blank" rel="noopener noreferrer">$1</a>');
    s = s.replace(/(?:^- .+(?:\n|$))+/gm, m => {
      const items = m.trim().split('\n').map(l => l.replace(/^-\s+/, ''));
      return '<ul>' + items.map(i => '<li>' + i + '</li>').join('') + '</ul>';
    });
    s = s.replace(/(?:^\d+\.\s.+(?:\n|$))+/gm, m => {
      const items = m.trim().split('\n').map(l => l.replace(/^\d+\.\s+/, ''));
      return '<ol>' + items.map(i => '<li>' + i + '</li>').join('') + '</ol>';
    });
    s = s.split(/\n{2,}/).map(p => {
      const t = p.trim();
      if (!t) return '';
      if (/^<(h\d|ul|ol|pre|p|blockquote)\b/.test(t)) return t;
      return '<p>' + t.replace(/\n/g, '<br>') + '</p>';
    }).join('');
    // Restore code-fence blocks (uses unambiguous control-char markers).
    s = s.replace(/CB(\d+)/g, (_, idx) => blocks[Number(idx)] || '');
    return s;
  }

  function a2uiApplyMessages(block, messages) {
    for (const msg of messages || []) {
      if (msg.createSurface) {
        const s = msg.createSurface;
        if (s.surfaceId != null) block.surfaceId = s.surfaceId;
        if (s.catalogId != null) block.catalogId = s.catalogId;
        if (s.theme != null) block.theme = s.theme;
      } else if (msg.updateComponents) {
        const arr = msg.updateComponents.components || [];
        for (const c of arr) {
          if (c && typeof c.id === 'string') block.componentsMap.set(c.id, c);
        }
      } else if (msg.updateDataModel) {
        const u = msg.updateDataModel;
        if (!u.path || u.path === '/') {
          block.dataModel = (u.value == null ? {} : u.value);
        } else if (u.value === undefined) {
          try { a2uiPtrSet(block.dataModel, u.path, undefined); } catch (e) {}
        } else {
          a2uiPtrSet(block.dataModel, u.path, u.value);
        }
      } else if (msg.deleteSurface) {
        // Per A2UI v0.9: deleteSurface tears the surface down. Bubble is
        // removed from DOM in a2uiRerender (flag here, act there).
        const targetSid = msg.deleteSurface.surfaceId;
        if (targetSid == null || targetSid === block.surfaceId) {
          block.componentsMap.clear();
          block.dataModel = {};
          block.deleted = true;
        }
      }
    }
    block.version++;
  }

  function a2uiToast(text) {
    const el = document.createElement('div');
    el.className = 'a2ui-toast';
    el.textContent = text;
    document.body.appendChild(el);
    requestAnimationFrame(() => el.classList.add('show'));
    setTimeout(() => {
      el.classList.remove('show');
      setTimeout(() => el.remove(), 250);
    }, 2400);
  }

  // Builds a fresh DOM tree from `def` and its children-by-id refs in
  // `block.componentsMap`. Re-rendered on every mutation — components are
  // small enough (typical surface: <20 nodes) that we don't need a smart diff.
  function a2uiRenderNode(block, def, scope) {
    if (!def) return document.createComment('missing');
    const type = typeof def.component === 'string' ? def.component : '';
    const r = v => a2uiResolveValue(v, scope);
    const text = r(def.text);
    const label = r(def.label);
    const placeholder = r(def.placeholder);
    const valueRaw = r(def.value);
    const path = a2uiPathOf(def.value);

    function children() {
      const out = [];
      if (typeof def.child === 'string') {
        const c = block.componentsMap.get(def.child);
        if (c) out.push(c);
      }
      if (Array.isArray(def.children)) {
        for (const id of def.children) {
          if (typeof id === 'string') {
            const c = block.componentsMap.get(id);
            if (c) out.push(c);
          }
        }
      }
      return out;
    }
    function appendKids(el) {
      for (const c of children()) el.appendChild(a2uiRenderNode(block, c, scope));
    }
    function writeBinding(p, v) {
      a2uiPtrSet(block.dataModel, p, v);
      block.version++;
      a2uiRerender(block);
    }

    if (type === 'Card') {
      const el = document.createElement('div');
      el.className = 'a2ui-card';
      appendKids(el);
      return el;
    }
    if (type === 'Column' || type === 'Row') {
      const el = document.createElement('div');
      el.className = type === 'Row' ? 'a2ui-row' : 'a2ui-col';
      if (def.gap != null) el.style.gap = def.gap + 'px';
      // v0.9 enum → CSS: start|center|end|stretch (plus justify's spaceX variants).
      const flexMap = {
        start: 'flex-start', center: 'center', end: 'flex-end', stretch: 'stretch',
        spaceAround: 'space-around', spaceBetween: 'space-between', spaceEvenly: 'space-evenly',
      };
      if (typeof def.align === 'string') {
        el.style.alignItems = flexMap[def.align] || def.align;
      }
      if (typeof def.justify === 'string') {
        el.style.justifyContent = flexMap[def.justify] || def.justify;
      }
      appendKids(el);
      return el;
    }
    if (type === 'Spacer') {
      // Custom (not in official v0.9 catalog) — kept for render_ui description compat.
      const el = document.createElement('div');
      el.className = 'a2ui-spacer';
      if (def.size != null) el.style.minHeight = def.size + 'px';
      return el;
    }
    if (type === 'Divider') {
      const axis = typeof def.axis === 'string' ? def.axis : 'horizontal';
      const el = document.createElement('div');
      el.className = 'a2ui-divider a2ui-divider-' + axis;
      return el;
    }
    if (type === 'Text') {
      // Official v0.9 supports `variant: h1|h2|h3|h4|h5|caption|body`. We map
      // h1-h4 to actual heading tags so semantics carry; h5 + caption render
      // as styled divs.
      const variant = typeof def.variant === 'string' ? def.variant : 'body';
      let el;
      if (variant === 'h1') el = document.createElement('h1');
      else if (variant === 'h2') el = document.createElement('h2');
      else if (variant === 'h3') el = document.createElement('h3');
      else if (variant === 'h4') el = document.createElement('h4');
      else el = document.createElement('div');
      el.className = 'a2ui-text a2ui-text-' + variant + (def.muted ? ' muted' : '');
      el.textContent = a2uiToStr(text);
      return el;
    }
    if (type === 'Heading') {
      // render_ui-only extension — official v0.9 uses Text variants instead.
      const lvl = Math.min(Math.max(1, Number(def.level == null ? 2 : def.level)), 4);
      const el = document.createElement('h' + lvl);
      el.className = 'a2ui-heading';
      el.textContent = a2uiToStr(text);
      return el;
    }
    if (type === 'Markdown') {
      // render_ui extension — official v0.9 puts simple Markdown directly in Text.
      const el = document.createElement('div');
      el.className = 'a2ui-md';
      el.innerHTML = a2uiRenderMarkdown(a2uiToStr(text));
      return el;
    }
    if (type === 'Image') {
      // Official v0.9 fields: url, description, fit, variant.
      // render_ui description fields: src, alt, width, height.
      const url = String((r(def.url) != null ? r(def.url) : r(def.src)) || '');
      const desc = String((r(def.description) != null ? r(def.description) : r(def.alt)) || '');
      const fit = typeof def.fit === 'string' ? def.fit : null;
      const variant = typeof def.variant === 'string' ? def.variant : null;
      const img = document.createElement('img');
      img.className = 'a2ui-img' + (variant ? ' a2ui-img-' + variant : '');
      img.loading = 'lazy';
      if (/^https?:\/\//i.test(url)) img.src = url;
      img.alt = desc;
      if (fit) {
        const map = { contain:'contain', cover:'cover', fill:'fill', none:'none', scaleDown:'scale-down' };
        if (map[fit]) img.style.objectFit = map[fit];
      }
      if (def.width  != null) img.style.width  = def.width  + 'px';
      if (def.height != null) img.style.height = def.height + 'px';
      return img;
    }
    if (type === 'Icon') {
      // v0.9: { name: string }. We render as a small badge with the name —
      // upgrading to a real icon font is out of scope.
      const span = document.createElement('span');
      span.className = 'a2ui-icon';
      const iname = String((r(def.name) != null ? r(def.name) : '') || '');
      span.textContent = iname;
      span.setAttribute('aria-label', iname);
      return span;
    }
    if (type === 'Video') {
      const url = String((r(def.url) != null ? r(def.url) : '') || '');
      const v = document.createElement('video');
      v.className = 'a2ui-video';
      v.controls = true;
      v.preload = 'metadata';
      if (/^https?:\/\//i.test(url)) v.src = url;
      return v;
    }
    if (type === 'AudioPlayer') {
      const url = String((r(def.url) != null ? r(def.url) : '') || '');
      const desc = String((r(def.description) != null ? r(def.description) : '') || '');
      const wrap = document.createElement('div');
      wrap.className = 'a2ui-audio';
      if (desc) {
        const lbl = document.createElement('span');
        lbl.className = 'a2ui-label';
        lbl.textContent = desc;
        wrap.appendChild(lbl);
      }
      const a = document.createElement('audio');
      a.controls = true;
      a.preload = 'metadata';
      if (/^https?:\/\//i.test(url)) a.src = url;
      wrap.appendChild(a);
      return wrap;
    }
    if (type === 'Button') {
      // Official v0.9: { child: ComponentId, variant: default|primary|borderless, action }
      // render_ui description: { text, label, kind: primary|danger|warn|success, disabled, checks, action }
      const kind = typeof def.kind === 'string' ? def.kind : null;
      const variant = typeof def.variant === 'string' ? def.variant : null;
      const cls = kind || (variant ? (variant === 'default' ? 'primary' : variant) : 'primary');
      const btn = document.createElement('button');
      btn.className = 'a2ui-btn ' + cls;
      if (def.disabled) btn.disabled = true;
      // If `child` is provided (v0.9), render the inner component for richer labels.
      if (typeof def.child === 'string') {
        const inner = block.componentsMap.get(def.child);
        if (inner) btn.appendChild(a2uiRenderNode(block, inner, scope));
        else btn.textContent = '';
      } else {
        btn.textContent = a2uiToStr(text != null ? text : (def.label != null ? def.label : 'Button'));
      }
      btn.addEventListener('click', () => {
        if (block.resolved) return;
        const checks = Array.isArray(def.checks) ? def.checks : [];
        for (const c of checks) {
          const ok = r(c);
          if (!ok) {
            const msg = c && typeof c === 'object' && c.message != null ? String(c.message) : 'validation failed';
            a2uiToast(msg);
            return;
          }
        }
        const action = def.action || {};
        if (action.openUrl) {
          // A2UI v0.9: Button.action.openUrl is `{ url: string }` (an object,
          // not a bare string). Older renderer assumed a string and lost the
          // URL. Accept both shapes defensively.
          const urlValue = typeof action.openUrl === 'string'
            ? action.openUrl
            : (action.openUrl && typeof action.openUrl.url === 'string' ? action.openUrl.url : '');
          if (/^(https?:|mailto:)/i.test(urlValue)) {
            window.ipc.postMessage(JSON.stringify({ type: 'a2ui_open_url', url: urlValue }));
          }
          return;
        }
        const ev = action.event;
        if (!ev || !ev.name) return;
        const context = {};
        for (const k in (ev.context || {})) context[k] = r(ev.context[k]);
        const actionPayload = {
          name: ev.name,
          sourceComponentId: typeof def.id === 'string' ? def.id : undefined,
          surfaceId: block.surfaceId,
          context,
          dataModel: block.dataModel,
        };
        if (block.pollFileId) {
          // An envelope's bash poll is still waiting for a click. Send the
          // resolution to THAT file (not necessarily the latest envelope we
          // received — fire-and-forget updates don't carry a poll).
          window.ipc.postMessage(JSON.stringify({
            type: 'a2ui_resolve',
            file_id: block.pollFileId,
            action: actionPayload,
          }));
          // Optimistic lock on the bubble.
          block.resolved = true;
          a2uiRerender(block);
          // Mark the SESSION busy too — the agent is about to process this
          // click and may take time. Without this, the input box stays
          // enabled, no thinking indicator shows, and the stop button is
          // missing. Same UX as a typed prompt.
          const liveSession = sessions.get(block.sid);
          if (liveSession) {
            liveSession.busy = true;
            window.__setThinking(liveSession.sid, true);
          }
        } else {
          // No active poll — either the surface is replayed from history,
          // OR the agent sent a fire-and-forget update without a new
          // await_events envelope. Deliver the event by simulating a user
          // prompt through the normal dispatch path so the user sees a
          // user bubble + thinking indicator (same as typing a message).
          const s = sessions.get(block.sid);
          if (s) {
            const author = (context && (context.author || context.handle)) || '';
            const surfaceLabel = block.surfaceId || '(unknown)';
            const headline = author
              ? `Clicked ${ev.name} on ${author}`
              : `Clicked ${ev.name}`;
            const promptText =
              headline + '\n\n' +
              '[A2UI event — out-of-band click on prior surface]\n' +
              'surfaceId: ' + surfaceLabel + '\n' +
              'event: ' + ev.name + '\n' +
              'sourceComponentId: ' + (typeof def.id === 'string' ? def.id : '?') + '\n' +
              'context: ' + JSON.stringify(context) + '\n\n' +
              'Do the work this event implies right now (post the tweet, save the draft, advance the wizard…). Then call render_ui again with the SAME surfaceId AND await_events so the next click resolves normally.';
            dispatchPromptForSession(s, promptText, [], []);
          }
        }
      });
      return btn;
    }
    if (type === 'TextField') {
      // Official v0.9: { label, value, variant: longText|number|shortText|obscured, validationRegexp }
      // render_ui description: { label, placeholder, type: text|email|password|number|tel, multiline, rows, value }
      const variant = typeof def.variant === 'string' ? def.variant : null;
      const isMultiline = def.multiline || variant === 'longText';
      const isNumber = def.type === 'number' || variant === 'number';
      const inputType = (() => {
        if (variant === 'obscured') return 'password';
        if (typeof def.type === 'string' && ['password','email','number','tel'].indexOf(def.type) >= 0) return def.type;
        if (isNumber) return 'number';
        return 'text';
      })();
      const wrap = document.createElement('label');
      wrap.className = 'a2ui-field';
      if (label != null && label !== '') {
        const lbl = document.createElement('span');
        lbl.className = 'a2ui-label';
        lbl.textContent = a2uiToStr(label);
        wrap.appendChild(lbl);
      }
      if (isMultiline) {
        const ta = document.createElement('textarea');
        if (def.rows != null) ta.rows = Number(def.rows);
        if (placeholder != null) ta.placeholder = String(placeholder);
        ta.value = a2uiToStr(valueRaw);
        if (typeof def.validationRegexp === 'string') ta.pattern = def.validationRegexp;
        ta.addEventListener('input', e => path && writeBinding(path, e.currentTarget.value));
        wrap.appendChild(ta);
      } else {
        const inp = document.createElement('input');
        inp.type = inputType;
        if (placeholder != null) inp.placeholder = String(placeholder);
        inp.value = a2uiToStr(valueRaw);
        if (typeof def.validationRegexp === 'string') inp.pattern = def.validationRegexp;
        inp.addEventListener('input', e => {
          if (!path) return;
          const raw = e.currentTarget.value;
          const v = isNumber ? (raw === '' ? null : Number(raw)) : raw;
          writeBinding(path, v);
        });
        wrap.appendChild(inp);
      }
      return wrap;
    }
    if (type === 'CheckBox') {
      const wrap = document.createElement('label');
      wrap.className = 'a2ui-check';
      const inp = document.createElement('input');
      inp.type = 'checkbox';
      inp.checked = !!valueRaw;
      inp.addEventListener('change', e => path && writeBinding(path, e.currentTarget.checked));
      const sp = document.createElement('span');
      sp.textContent = a2uiToStr(label);
      wrap.appendChild(inp);
      wrap.appendChild(sp);
      return wrap;
    }
    if (type === 'Slider') {
      const wrap = document.createElement('label');
      wrap.className = 'a2ui-field a2ui-slider';
      if (label != null && label !== '') {
        const lbl = document.createElement('span');
        lbl.className = 'a2ui-label';
        lbl.textContent = a2uiToStr(label);
        wrap.appendChild(lbl);
      }
      const row = document.createElement('div');
      row.className = 'a2ui-slider-row';
      const inp = document.createElement('input');
      inp.type = 'range';
      if (def.min  != null) inp.min  = String(def.min);
      if (def.max  != null) inp.max  = String(def.max);
      if (def.step != null) inp.step = String(def.step);
      inp.value = valueRaw != null ? String(valueRaw) : (def.min != null ? String(def.min) : '0');
      inp.addEventListener('input', e => path && writeBinding(path, Number(e.currentTarget.value)));
      const out = document.createElement('output');
      out.className = 'a2ui-slider-val';
      out.textContent = inp.value;
      row.appendChild(inp);
      row.appendChild(out);
      wrap.appendChild(row);
      return wrap;
    }
    if (type === 'ChoicePicker') {
      // Official v0.9: { options: array, variant: multipleSelection|mutuallyExclusive,
      //   displayStyle: checkbox|chips, filterable }
      // render_ui description: { choices: [scalar | {label,value}] }
      const optionsRaw = Array.isArray(def.options) ? def.options
                       : Array.isArray(def.choices) ? def.choices : [];
      const variant = typeof def.variant === 'string' ? def.variant : 'mutuallyExclusive';
      const isMulti = variant === 'multipleSelection';
      const style = typeof def.displayStyle === 'string' ? def.displayStyle : null;
      const useChips = style === 'chips';
      // Normalize {label, value} or scalar entries.
      const opts = optionsRaw.map(c => {
        if (c != null && typeof c === 'object') {
          return { label: r(c.label), value: r(c.value) };
        }
        return { label: String(c), value: String(c) };
      });
      const wrap = document.createElement('div');
      wrap.className = 'a2ui-field a2ui-choice ' + (useChips ? 'as-chips' : 'as-list');
      if (label != null && label !== '') {
        const lbl = document.createElement('span');
        lbl.className = 'a2ui-label';
        lbl.textContent = a2uiToStr(label);
        wrap.appendChild(lbl);
      }
      const selected = (() => {
        if (isMulti) {
          if (Array.isArray(valueRaw)) return new Set(valueRaw.map(String));
          if (valueRaw == null) return new Set();
          return new Set([String(valueRaw)]);
        }
        return valueRaw == null ? null : String(valueRaw);
      })();
      // For mutually-exclusive non-chip: use a native <select>.
      if (!isMulti && !useChips && style !== 'checkbox') {
        const sel = document.createElement('select');
        for (const o of opts) {
          const opt = document.createElement('option');
          opt.value = a2uiToStr(o.value);
          opt.textContent = a2uiToStr(o.label != null ? o.label : o.value);
          if (selected != null && String(selected) === opt.value) opt.selected = true;
          sel.appendChild(opt);
        }
        sel.addEventListener('change', e => path && writeBinding(path, e.currentTarget.value));
        wrap.appendChild(sel);
        return wrap;
      }
      // Chips or checkbox list (either multi or single).
      const list = document.createElement('div');
      list.className = useChips ? 'a2ui-chip-row' : 'a2ui-check-list';
      for (const o of opts) {
        const v = a2uiToStr(o.value);
        const lblTxt = a2uiToStr(o.label != null ? o.label : o.value);
        const isOn = isMulti
          ? selected.has(v)
          : (selected != null && selected === v);
        if (useChips) {
          const chip = document.createElement('button');
          chip.type = 'button';
          chip.className = 'a2ui-chip' + (isOn ? ' on' : '');
          chip.textContent = lblTxt;
          chip.addEventListener('click', () => {
            if (!path) return;
            if (isMulti) {
              const next = new Set(selected);
              if (next.has(v)) next.delete(v); else next.add(v);
              writeBinding(path, Array.from(next));
            } else {
              writeBinding(path, v);
            }
          });
          list.appendChild(chip);
        } else {
          const item = document.createElement('label');
          item.className = 'a2ui-check';
          const inp = document.createElement('input');
          inp.type = isMulti ? 'checkbox' : 'radio';
          if (!isMulti) inp.name = (def.id || 'choice') + '_' + (block.fileId || '');
          inp.checked = isOn;
          inp.addEventListener('change', () => {
            if (!path) return;
            if (isMulti) {
              const next = new Set(selected);
              if (inp.checked) next.add(v); else next.delete(v);
              writeBinding(path, Array.from(next));
            } else if (inp.checked) {
              writeBinding(path, v);
            }
          });
          const sp = document.createElement('span');
          sp.textContent = lblTxt;
          item.appendChild(inp);
          item.appendChild(sp);
          list.appendChild(item);
        }
      }
      wrap.appendChild(list);
      return wrap;
    }
    if (type === 'DateTimeInput') {
      // Official v0.9: { enableDate, enableTime, min, max, label, value }
      // render_ui description: { mode: date|datetime|time, min, max, label, value }
      const enableDate = def.enableDate != null ? !!def.enableDate : (def.mode === 'date' || def.mode === 'datetime' || def.mode == null);
      const enableTime = def.enableTime != null ? !!def.enableTime : (def.mode === 'time' || def.mode === 'datetime');
      const inputType = enableDate && enableTime ? 'datetime-local'
                      : enableTime ? 'time'
                      : 'date';
      const wrap = document.createElement('label');
      wrap.className = 'a2ui-field';
      if (label != null && label !== '') {
        const lbl = document.createElement('span');
        lbl.className = 'a2ui-label';
        lbl.textContent = a2uiToStr(label);
        wrap.appendChild(lbl);
      }
      const inp = document.createElement('input');
      inp.type = inputType;
      inp.value = a2uiToStr(valueRaw);
      if (def.min != null) inp.min = String(def.min);
      if (def.max != null) inp.max = String(def.max);
      inp.addEventListener('change', e => path && writeBinding(path, e.currentTarget.value));
      wrap.appendChild(inp);
      return wrap;
    }
    if (type === 'List') {
      // Official v0.9 + render_ui description both use children:{path, componentId}.
      // Plus direction (vertical|horizontal) and align from official spec.
      const el = document.createElement('div');
      const dir = typeof def.direction === 'string' ? def.direction : 'vertical';
      el.className = 'a2ui-list a2ui-list-' + dir;
      if (typeof def.align === 'string') el.style.alignItems = def.align;
      const ch = def.children;
      if (ch && typeof ch === 'object' && !Array.isArray(ch) && typeof ch.path === 'string' && typeof ch.componentId === 'string') {
        const items = r({ path: ch.path });
        const tpl = block.componentsMap.get(ch.componentId);
        if (Array.isArray(items) && tpl) {
          for (const item of items) {
            el.appendChild(a2uiRenderNode(block, tpl, { root: scope.root, local: item }));
          }
          return el;
        }
      }
      appendKids(el);
      return el;
    }
    if (type === 'Tabs') {
      // v0.9: { tabs: [{ title, content (componentId) }, ...] }
      const tabs = Array.isArray(def.tabs) ? def.tabs : [];
      if (!tabs.length) return document.createComment('empty tabs');
      const wrap = document.createElement('div');
      wrap.className = 'a2ui-tabs';
      const bar = document.createElement('div');
      bar.className = 'a2ui-tabs-bar';
      const pane = document.createElement('div');
      pane.className = 'a2ui-tabs-pane';
      // Active tab is tracked per def.id in the block — survives re-renders.
      block.tabState = block.tabState || {};
      const key = typeof def.id === 'string' ? def.id : 'tabs';
      let active = block.tabState[key] != null ? block.tabState[key] : 0;
      if (active >= tabs.length) active = 0;
      tabs.forEach((t, i) => {
        const titleResolved = r(t && t.title);
        const btn = document.createElement('button');
        btn.type = 'button';
        btn.className = 'a2ui-tab' + (i === active ? ' active' : '');
        btn.textContent = a2uiToStr(titleResolved != null ? titleResolved : ('Tab ' + (i + 1)));
        btn.addEventListener('click', () => {
          block.tabState[key] = i;
          a2uiRerender(block);
        });
        bar.appendChild(btn);
      });
      const activeTab = tabs[active] || {};
      const contentId = typeof activeTab.content === 'string' ? activeTab.content : null;
      const contentDef = contentId ? block.componentsMap.get(contentId) : null;
      if (contentDef) pane.appendChild(a2uiRenderNode(block, contentDef, scope));
      wrap.appendChild(bar);
      wrap.appendChild(pane);
      return wrap;
    }
    if (type === 'Modal') {
      // v0.9: { trigger: ComponentId, content: ComponentId }
      // We render the trigger inline; clicking it shows `content` in an
      // overlay. The visibility flag lives on the block so re-renders preserve it.
      const triggerId = typeof def.trigger === 'string' ? def.trigger : null;
      const contentId = typeof def.content === 'string' ? def.content : null;
      const wrap = document.createElement('span');
      wrap.className = 'a2ui-modal-trigger-wrap';
      block.modalState = block.modalState || {};
      const key = typeof def.id === 'string' ? def.id : 'modal';
      const triggerDef = triggerId ? block.componentsMap.get(triggerId) : null;
      if (triggerDef) {
        const trig = a2uiRenderNode(block, triggerDef, scope);
        trig.addEventListener('click', (e) => {
          e.stopPropagation();
          block.modalState[key] = true;
          a2uiRerender(block);
        }, true);
        wrap.appendChild(trig);
      }
      if (block.modalState[key] && contentId) {
        const contentDef = block.componentsMap.get(contentId);
        const overlay = document.createElement('div');
        overlay.className = 'a2ui-modal-overlay';
        overlay.addEventListener('click', () => {
          block.modalState[key] = false;
          a2uiRerender(block);
        });
        const panel = document.createElement('div');
        panel.className = 'a2ui-modal-panel';
        panel.addEventListener('click', (e) => e.stopPropagation());
        const close = document.createElement('button');
        close.type = 'button';
        close.className = 'a2ui-modal-close';
        close.setAttribute('aria-label', 'Close');
        close.textContent = '×';
        close.addEventListener('click', () => {
          block.modalState[key] = false;
          a2uiRerender(block);
        });
        panel.appendChild(close);
        if (contentDef) panel.appendChild(a2uiRenderNode(block, contentDef, scope));
        overlay.appendChild(panel);
        wrap.appendChild(overlay);
      }
      return wrap;
    }
    const unk = document.createElement('div');
    unk.className = 'a2ui-unknown';
    unk.textContent = '[unsupported component: ' + (type || '?') + ']';
    return unk;
  }

  function a2uiRerender(block) {
    const wrap = a2uiBubbleByFile.get(block.fileId);
    if (!wrap) return;
    // deleteSurface → remove the bubble entirely and drop from all indices.
    if (block.deleted) {
      wrap.remove();
      a2uiBubbleByFile.delete(block.fileId);
      a2uiBlocks.delete(block.fileId);
      if (block.surfaceId != null && block.sid != null) {
        a2uiSurfaceIndex.delete(a2uiSurfaceKey(block.sid, block.surfaceId));
      }
      const s = sessions.get(block.sid);
      if (s && s.sid === activeSid) { updateWelcome(); scrollToBottom(); }
      return;
    }
    wrap.classList.toggle('resolved', !!block.resolved);
    if (block.theme && typeof block.theme === 'object' && typeof block.theme.primaryColor === 'string') {
      wrap.style.setProperty('--a2ui-primary', block.theme.primaryColor);
    }
    const head = wrap.querySelector('.a2ui-head');
    if (head) {
      head.innerHTML = '';
      const tag = document.createElement('span');
      tag.className = 'kind-tag';
      tag.textContent = 'ui';
      head.appendChild(tag);
      const agentName = block.theme && typeof block.theme.agentDisplayName === 'string' ? block.theme.agentDisplayName : null;
      if (agentName) {
        const sp = document.createElement('span');
        sp.textContent = agentName;
        head.appendChild(sp);
      }
      if (block.surfaceId) {
        const sp = document.createElement('span');
        sp.className = 'mono';
        sp.textContent = block.surfaceId;
        head.appendChild(sp);
      }
    }
    const body = wrap.querySelector('.a2ui-body');
    if (body) {
      const rootDef = block.componentsMap.get('root');
      if (rootDef) {
        body.innerHTML = '';
        body.appendChild(a2uiRenderNode(block, rootDef, { root: block.dataModel, local: null }));
      } else if (body.childNodes.length === 0) {
        // Only render the empty placeholder if this bubble has never had a
        // root. Once we've shown something useful we KEEP it on screen even
        // if a later envelope wipes components without replacing them —
        // otherwise the surface looks "stuck" between updates.
        const empty = document.createElement('div');
        empty.className = 'a2ui-text muted';
        empty.textContent = 'Loading…';
        body.appendChild(empty);
      }
      // else: keep last good render visible.
    }
    // No resolved-state DOM mutation: the surface is only transiently locked
    // ("Processing…" overlay via CSS, pointer-events suppressed). The next
    // envelope from the agent lifts the lock; deleteSurface tears it down.
    const stale = wrap.querySelector('.a2ui-resolved-note');
    if (stale) stale.remove();
  }

  function a2uiEnsureBlock(sid, fileId, ts) {
    let block = a2uiBlocks.get(fileId);
    if (block) return block;
    const s = sessions.get(sid);
    if (!s) return null;
    // ts (ms since epoch) is the bubble's chronological anchor — used for
    // the time label. Caller passes the persisted creation time on replay;
    // live emissions pass null/undefined and we use current time.
    const displayDate = (typeof ts === 'number' && ts > 0) ? new Date(ts) : new Date();
    block = {
      fileId,
      sid,
      surfaceId: null,
      catalogId: null,
      theme: {},
      componentsMap: new Map(),
      dataModel: {},
      awaitEvents: [],
      resolved: false,
      // The fileId of the envelope whose bash poll loop is currently waiting
      // for a click. `null` if no such envelope (after-restart replay, after
      // the agent sent a fire-and-forget update without a new poll, or after
      // the prior poll exhausted). Clicks go via the live `a2ui_resolve` path
      // when this is set; otherwise they go via the ghost `a2ui_replay_event`
      // path (synthetic prompt to the agent).
      pollFileId: null,
      // True if the most-recent envelope came from a running agent in this
      // session. Affects only toast text; click routing uses pollFileId.
      live: false,
      resolutionLabel: null,
      version: 0,
    };
    a2uiBlocks.set(fileId, block);

    const wrap = document.createElement('div');
    wrap.className = 'msg ui a2ui-block';
    const label = document.createElement('div');
    label.className = 'msg-label';
    const who = document.createElement('span');
    who.className = 'msg-who';
    who.textContent = 'UI';
    const time = document.createElement('span');
    time.className = 'msg-time';
    time.textContent = fmtTime(displayDate);
    label.appendChild(who);
    label.appendChild(time);
    const bubble = document.createElement('div');
    bubble.className = 'msg-bubble';
    const head = document.createElement('header');
    head.className = 'a2ui-head';
    const body = document.createElement('div');
    body.className = 'a2ui-body';
    bubble.appendChild(head);
    bubble.appendChild(body);
    wrap.appendChild(label);
    wrap.appendChild(bubble);
    s.container.insertBefore(wrap, s.thinking);
    a2uiBubbleByFile.set(fileId, wrap);
    if (s.sid === activeSid) { updateWelcome(); scrollToBottom(); }
    return block;
  }

  window.__a2uiUpdate = function(sid, fileId, payload, live, ts) {
    // A2UI v0.9 surface lifecycle:
    //   - Each render_ui call writes a new envelope (new fileId) but may carry
    //     the same `surfaceId`. We key the DOM bubble by surfaceId so updates
    //     and re-renders land on the same bubble.
    //   - A button click resolves THIS envelope's poll; the surface stays alive.
    //   - The next envelope for the same surfaceId clears the in-flight lock.
    //   - deleteSurface tears the bubble down.
    //
    // `live` is the 4th arg: true only when the envelope's parent octomind
    // subprocess is still running this session. Replayed / cross-session /
    // post-restart surfaces are NOT live — their bash poll loop is dead so
    // clicks have nowhere to land. We render them anyway (history is useful)
    // but suppress clicks with a toast.
    const isLive = live === true;
    const newSurfaceId = a2uiSniffSurfaceId(payload);
    // Guard: vacuous envelopes (no surfaceId, no messages, no live block for
    // this fileId) would create an orphan "Loading…" bubble. The agent emits
    // these by mistake (empty probe calls). Drop them on the floor.
    const hasMessages = Array.isArray(payload && payload.messages) && payload.messages.length > 0;
    if (!a2uiBlocks.has(fileId) && newSurfaceId == null && !hasMessages) {
      return;
    }
    let block = a2uiBlocks.get(fileId);
    if (!block && newSurfaceId != null) {
      const key = a2uiSurfaceKey(sid, newSurfaceId);
      const existingFile = a2uiSurfaceIndex.get(key);
      if (existingFile && a2uiBlocks.has(existingFile)) {
        block = a2uiBlocks.get(existingFile);
        // Rebind: same DOM bubble, new envelope identity.
        a2uiBlocks.delete(existingFile);
        a2uiBlocks.set(fileId, block);
        const wrap = a2uiBubbleByFile.get(existingFile);
        if (wrap) {
          a2uiBubbleByFile.delete(existingFile);
          a2uiBubbleByFile.set(fileId, wrap);
        }
        block.fileId = fileId;
      }
    }
    if (!block) {
      block = a2uiEnsureBlock(sid, fileId, ts);
      if (!block) return;
    }
    if (Array.isArray(payload.await_events)) block.awaitEvents = payload.await_events;
    // `live` is sticky-on: once we've seen a live envelope for this block
    // (or a same-surfaceId block we just rebound from), the surface is
    // wired into a running agent and clicks can resolve normally. Replays
    // and ghosts never flip it on.
    if (isLive) block.live = true;
    // pollFileId tracks which envelope's bash is currently waiting for a
    // click. Set when an envelope arrives with non-empty await_events and
    // pending status. Cleared below if THIS envelope is the active poll AND
    // it just transitioned to a terminal status.
    const awaiting = Array.isArray(payload.await_events) && payload.await_events.length > 0;
    const incomingStatusEarly = payload && payload.status;
    const isPendingEarly = !incomingStatusEarly || incomingStatusEarly === 'pending';
    if (awaiting && isPendingEarly && isLive) {
      block.pollFileId = fileId;
    }
    // Pin surfaceId on the block BEFORE applying messages — a deleteSurface
    // that lands on a fresh (never-seen-createSurface) block must still match
    // its target by surfaceId. Same goes for updateComponents/DataModel that
    // arrive in isolated envelopes.
    if (newSurfaceId != null) {
      block.surfaceId = newSurfaceId;
      a2uiSurfaceIndex.set(a2uiSurfaceKey(sid, newSurfaceId), fileId);
    }
    // Resolved-state logic differs for live vs ghost:
    //   LIVE: "resolved" means the bash poll is mid-flight after a click;
    //         we lock the bubble into "Processing…" until the next envelope.
    //   GHOST: "resolved" is the frozen end-state of a prior session — there
    //          IS no next envelope coming. Locking ghosts blocks the
    //          replay-event click path. So we never lock ghosts.
    const incomingStatus = payload && payload.status;
    const isTerminal = incomingStatus === 'resolved'
                    || incomingStatus === 'expired'
                    || incomingStatus === 'cancelled';
    if (!isTerminal) {
      block.resolved = false;
    }
    a2uiApplyMessages(block, payload.messages || []);
    if (isTerminal && block.live) {
      block.resolved = true;
    } else if (isTerminal) {
      // Ghost surface arriving with terminal status — keep clickable.
      block.resolved = false;
    }
    // If THIS envelope's poll just exhausted (transitioned to terminal),
    // clear it so subsequent clicks fall through to the ghost path until
    // the agent emits a new envelope with await_events.
    if (isTerminal && block.pollFileId === fileId) {
      block.pollFileId = null;
    }
    a2uiRerender(block);
  };

  // Legacy hook — kept for any callers still posting a separate "resolved"
  // event. Equivalent to flipping the in-flight lock on and clearing the
  // active poll for this fileId.
  window.__a2uiResolved = function(sid, fileId, payload) {
    const block = a2uiBlocks.get(fileId);
    if (!block) return;
    if (block.live) block.resolved = true;
    if (block.pollFileId === fileId) block.pollFileId = null;
    a2uiRerender(block);
  };

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
        .replace("/* MAX_SESSIONS */", &crate::MAX_SESSIONS.to_string())
        .replace(
            "/* MAX_PROMPT_HISTORY */",
            &max_ai_prompt_history.to_string(),
        )
}
