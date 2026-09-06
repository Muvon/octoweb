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
///   window.__appendSpecialist(sid, text)          — injected specialist reply bubble
///   window.__appendImage(sid, mime, b64)          — append image to current bubble
///   window.__toolStart(sid, id, title, kind, ri, locs, startedMsAgo?) — start tool row
///   window.__toolUpdate(sid, id, title, status, ro)    — update tool row
///   window.__setThinking(sid, bool, elapsedMs?)   — show/hide activity feed
///   window.__appendError(sid, text)               — show an error bubble
///   window.__setAvailableCommands(sid, json)      — populate slash-command list
///   window.__setAgents(list)                      — agent tags for create-panel autocomplete
///   window.__setShortcuts(data)                   — update live control titles
///   window.__a2uiUpdate(sid, fileId, payload, live, ts) — render / update an A2UI surface
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
///   { type: "sidebar_resize", width }                    — apply logical width while dragging
///   { type: "sidebar_resize_end", width }                — apply and persist logical width
///   { type: "sidebar_resize_reset" }                     — restore the default width
///   { type: "a2ui_resolve",  file_id, sid, action }     — A2UI v1.0 action event → unblocks the waiting render_ui call
///   { type: "a2ui_fn_response", file_id, sid, response } — result of an agent-issued callRendererFunction
///   { type: "a2ui_open_url", url }                       — A2UI openUrl → open in a browser tab
pub fn html(max_ai_prompt_history: usize) -> String {
    let prompt_history_js = crate::prompt_history_js::prompt_history_js();
    let keybindings_json = crate::keybindings::Keymap::load().ui_json().to_string();
    r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<style>
/*@@THEME@@*/
  *, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }

  /* ── Tahoe Liquid Glass tokens ─────────────────────────────────────────── */
  :root {
    --glass-solid:     rgb(235, 235, 240);
    --glass-bg:        rgba(235, 235, 240, 0.72);
    --glass-border:    rgba(255, 255, 255, 0.55);
    --glass-inner:     rgba(255, 255, 255, 0.38);
    --glass-shadow:    0 8px 40px rgba(0,0,0,0.13), 0 1.5px 6px rgba(0,0,0,0.07);

    --user-bg:         color-mix(in srgb, var(--accent) 11%, transparent);
    --user-border:     color-mix(in srgb, var(--accent) 22%, transparent);
    --agent-bg:        rgba(255, 255, 255, 0.46);
    --agent-border:    rgba(0, 0, 0, 0.07);
    --error-bg:        rgba(255, 59, 48, 0.09);
    --error-border:    rgba(255, 59, 48, 0.22);
    --error-text:      #bf2114;

    --input-bg:        rgba(255, 255, 255, 0.60);
    --input-border:    rgba(0, 0, 0, 0.10);
    --input-focus-border: color-mix(in srgb, var(--accent) 55%, transparent);
    --input-shadow:    inset 0 1px 3px rgba(0,0,0,0.05);

    --text-primary:    var(--label);
    --text-secondary:  var(--label-2);
    --text-tertiary:   var(--label-3);

    --accent-hover:    color-mix(in srgb, var(--accent) 85%, #000);

    --dot-ok:          var(--ok);
    --dot-wait:        var(--warn);
    --dot-err:         var(--err);

    --divider:         rgba(0, 0, 0, 0.07);
    --scrollbar:       rgba(0, 0, 0, 0.13);
    --hover-bg:        rgba(0, 0, 0, 0.05);

    /* Liquid Glass material: specular rim (light catches the top edge),
       inner card highlight, floating depth shadow, springy motion. */
    --rim-hi:          rgba(255, 255, 255, 0.75);
    --rim-lo:          rgba(255, 255, 255, 0.20);
    --card-hi:         rgba(255, 255, 255, 0.50);
    --float-shadow:    0 8px 24px rgba(0, 0, 0, 0.10), 0 2px 6px rgba(0, 0, 0, 0.05);

    /* Markdown content tokens */
    --md-code-bg:      rgba(0, 0, 0, 0.055);
    --md-code-border:  rgba(0, 0, 0, 0.08);
    --md-pre-bg:       rgba(0, 0, 0, 0.04);
    --md-blockquote:   color-mix(in srgb, var(--accent) 18%, transparent);
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

      --user-bg:         color-mix(in srgb, var(--accent) 16%, transparent);
      --user-border:     color-mix(in srgb, var(--accent) 28%, transparent);
      --agent-bg:        rgba(255, 255, 255, 0.07);
      --agent-border:    rgba(255, 255, 255, 0.09);
      --error-bg:        rgba(255, 69, 58, 0.13);
      --error-border:    rgba(255, 69, 58, 0.28);
      --error-text:      rgba(255, 105, 97, 0.95);

      --input-bg:        rgba(255, 255, 255, 0.08);
      --input-border:    rgba(255, 255, 255, 0.12);
      --input-focus-border: color-mix(in srgb, var(--accent) 60%, transparent);
      --input-shadow:    inset 0 1px 3px rgba(0,0,0,0.25);

      --accent-hover:    color-mix(in srgb, var(--accent) 80%, #fff);

      --divider:         rgba(255, 255, 255, 0.07);
      --scrollbar:       rgba(255, 255, 255, 0.13);
      --hover-bg:        rgba(255, 255, 255, 0.07);

      --rim-hi:          rgba(255, 255, 255, 0.28);
      --rim-lo:          rgba(255, 255, 255, 0.06);
      --card-hi:         rgba(255, 255, 255, 0.08);
      --float-shadow:    0 8px 24px rgba(0, 0, 0, 0.45), 0 2px 6px rgba(0, 0, 0, 0.30);

      --md-code-bg:      rgba(255, 255, 255, 0.08);
      --md-code-border:  rgba(255, 255, 255, 0.10);
      --md-pre-bg:       rgba(0, 0, 0, 0.28);
      --md-blockquote:   color-mix(in srgb, var(--accent) 22%, transparent);
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

  #resize-handle {
    position: fixed;
    top: 0;
    bottom: 0;
    left: 0;
    width: 6px;
    z-index: 30;
    cursor: col-resize;
    touch-action: none;
    user-select: none;
    -webkit-user-select: none;
  }
  #resize-handle::after {
    content: "";
    position: absolute;
    top: 0;
    bottom: 0;
    left: 0;
    width: 2px;
    background: var(--accent);
    opacity: 0;
  }
  #resize-handle:hover::after,
  #resize-handle.active::after { opacity: 1; }
  #resize-handle:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }
  #sidebar.fullscreen #resize-handle { display: none; }
  body.sidebar-resizing,
  body.sidebar-resizing * {
    cursor: col-resize !important;
    user-select: none !important;
    -webkit-user-select: none !important;
  }

  /* Specular rim — 1px gradient ring that reads as refracted light on glass.
     Applied to floating elements; they must be position:relative with a
     border-radius (the ring inherits it). */
  #input-row::before,
  .tool-group.expanded::before,
  #attach-menu::before {
    content: "";
    position: absolute;
    inset: 0;
    border-radius: inherit;
    padding: 1px;
    background: linear-gradient(165deg,
      var(--rim-hi) 0%, var(--rim-lo) 38%, transparent 62%, var(--rim-lo) 100%);
    -webkit-mask: linear-gradient(#000 0 0) content-box, linear-gradient(#000 0 0);
    -webkit-mask-composite: xor;
    mask-composite: exclude;
    pointer-events: none;
    z-index: 1;
  }

  /* ── Header ─────────────────────────────────────────────────────────────── */
  #header {
    display: flex;
    align-items: center;
    gap: 4px;
    height: 32px;
    padding: 0 10px;
    box-shadow: 0 0.5px 0 var(--hairline);
    flex-shrink: 0;
    min-width: 0;
  }

  #header-logo {
    line-height: 0;
    flex-shrink: 0;
    cursor: default;
    color: var(--brand);
  }
  #header-logo svg { width: 16px; height: 16px; display: block; }

  /* ── Account bar — slim login / quota strip under the header ─────────────── */
  #account-bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 5px 10px;
    font-size: 13px;
    line-height: 1.3;
    color: var(--label);
    background: var(--fill);
    box-shadow: 0 0.5px 0 var(--hairline);
    flex-shrink: 0;
  }
  #account-bar.hidden { display: none; }
  #account-bar .account-dot {
    width: 6px; height: 6px; border-radius: 50%;
    flex-shrink: 0;
    background: var(--label-2);
  }
  #account-bar.signed-in  .account-dot { background: var(--ok); }
  #account-bar.signed-out .account-dot,
  #account-bar.over-quota .account-dot { background: var(--err); }
  #account-bar.pending    .account-dot {
    background: var(--accent);
    animation: acct-pulse 1.2s ease-in-out infinite;
  }
  @keyframes acct-pulse { 0%,100% { opacity: 0.35; } 50% { opacity: 1; } }
  #account-bar.over-quota {
    color: var(--label);
    background: color-mix(in srgb, var(--err) 10%, transparent);
  }
  #account-text {
    flex: 1; min-width: 0;
    overflow: hidden; text-overflow: ellipsis; white-space: nowrap;
  }
  #account-action {
    flex-shrink: 0;
    padding: 2px 10px;
    border-radius: 11px;
    border: none;
    background: var(--accent);
    color: var(--on-accent);
    min-height: 22px;
    font-size: 13px;
    font-weight: 500;
    cursor: pointer;
  }
  #account-action:hover { opacity: 0.88; }
  #account-action:active { opacity: 0.76; }
  #account-dismiss {
    flex-shrink: 0;
    width: 22px; height: 22px;
    display: flex; align-items: center; justify-content: center;
    border: none; background: transparent;
    color: var(--label-2);
    cursor: pointer; font-size: 13px; line-height: 1; border-radius: 6px;
  }
  #account-dismiss:hover { background: var(--fill-hover); color: var(--label); }
  #account-dismiss:active { background: var(--fill-press); }

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
    height: 22px;
    padding: 0 10px;
    border-radius: var(--r-capsule);
    background: var(--fill);
    border: none;
    font-size: 13px;
    font-weight: 500;
    color: var(--label);
    cursor: pointer;
    transition: background var(--t-fast), color var(--t-fast);
    flex-shrink: 0;
    max-width: 140px;
    min-width: 0;
    user-select: none;
    -webkit-user-select: none;
  }
  .session-tab:hover {
    background: var(--fill-hover);
    color: var(--label);
  }
  .session-tab.active {
    max-width: 240px;
    background: color-mix(in srgb, var(--accent) 15%, transparent);
    color: var(--label);
  }
  .session-tab.active:hover {
    background: color-mix(in srgb, var(--accent) 15%, transparent);
    color: var(--label);
  }

  .session-tab .session-dot {
    width: 6px; height: 6px;
    border-radius: 50%;
    background: var(--ok);
    box-shadow: none;
    flex-shrink: 0;
    transition: background 0.3s, box-shadow 0.3s;
  }
  .session-tab .session-dot.connecting {
    background: var(--warn);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--warn) 20%, transparent);
    animation: pulse 1.4s ease-in-out infinite;
  }
  .session-tab .session-dot.thinking {
    background: var(--accent);
    box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 20%, transparent);
    animation: pulse 1.4s ease-in-out infinite;
  }
  .session-tab .session-dot.error {
    background: var(--err);
    box-shadow: none;
  }

  .session-tab .session-title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    min-width: 0;
  }
  /* Inline rename input replaces the title span when editing */
  .session-tab .session-rename {
    background: var(--fill);
    border: 1px solid var(--hairline);
    border-radius: var(--r-ctl);
    padding: 1px 4px;
    font-size: 13px;
    font-weight: 500;
    color: var(--label);
    outline: none;
    width: 90px;
    min-width: 0;
    font-family: inherit;
  }
  .session-tab.active .session-rename {
    background: var(--fill);
    color: var(--label);
  }
  .session-tab .session-rename:focus-visible {
    border-color: var(--accent);
    box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 18%, transparent);
  }

  .session-tab .session-close {
    position: relative;
    width: 18px; height: 18px;
    margin: 0 2px;
    border-radius: var(--r-capsule);
    border: none;
    background: transparent;
    color: var(--label-2);
    cursor: pointer;
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    flex-shrink: 0;
    opacity: 0;
    pointer-events: none;
    transition: background 0.12s, opacity 0.12s;
  }
  .session-tab .session-close::before {
    content: "";
    position: absolute;
    inset: -2px;
  }
  .session-tab:hover .session-close,
  .session-tab:focus-within .session-close {
    opacity: 1;
    pointer-events: auto;
  }
  .session-tab .session-close:hover {
    background: var(--fill-hover);
    opacity: 1;
  }
  .session-tab .session-close:active { background: var(--fill-press); }

  /* Header controls */
  #session-add-btn, #fullscreen-btn, #close-btn {
    width: 24px; height: 24px;
    border-radius: var(--r-ctl);
    border: none;
    background: transparent;
    color: var(--label-2);
    cursor: pointer;
    display: flex; align-items: center; justify-content: center;
    flex-shrink: 0;
    padding: 0;
    transition: background var(--t-fast), color var(--t-fast);
  }
  #session-add-btn svg, #fullscreen-btn svg, #close-btn svg {
    width: 14px;
    height: 14px;
  }
  #session-add-btn:hover:not(:disabled),
  #fullscreen-btn:hover, #close-btn:hover {
    background: var(--fill-hover);
    color: var(--label-2);
  }
  #session-add-btn:active:not(:disabled),
  #fullscreen-btn:active, #close-btn:active {
    background: var(--fill-press);
  }
  #session-add-btn:focus-visible,
  #fullscreen-btn:focus-visible, #close-btn:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 1px;
  }
  #session-add-btn:disabled {
    opacity: 0.35;
    cursor: not-allowed;
  }

  /* ── Create-session inline panel (drops down from header) ───────────────── */
  #session-create-panel {
    display: none;
    position: relative;
    /* Above the later `#sidebar > *` siblings (all z-index: 1), or #messages
       paints over the tag suggestions that overhang it. */
    z-index: 20;
    align-items: flex-start;
    gap: 6px;
    padding: 7px 10px;
    background: transparent;
    box-shadow: 0 0.5px 0 var(--hairline);
    flex-shrink: 0;
  }
  #session-create-panel.visible { display: flex; }
  #session-create-panel input {
    height: 28px;
    background: var(--fill);
    border: none;
    border-radius: var(--r-ctl);
    padding: 0 8px;
    font: 400 13px/1 var(--font-text);
    color: var(--label);
    outline: none;
    box-shadow: 0 0 0 0.5px var(--hairline);
    min-width: 0;
  }
  #session-create-panel input::placeholder { color: var(--label-2); }
  #session-create-panel input:focus-visible {
    box-shadow: 0 0 0 1px var(--accent),
                0 0 0 3px color-mix(in srgb, var(--accent) 18%, transparent);
  }
  #session-create-panel #sc-title  { flex: 0 0 110px; }
  #session-create-panel .sc-agent-field {
    flex: 1 1 auto;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  #session-create-panel #sc-tag { width: 100%; }
  #sc-tag-meta {
    min-height: 14px;
    padding: 0 8px;
    color: var(--label-2);
    font-size: 11px;
    line-height: 1.25;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  #session-create-panel button {
    height: 28px;
    border: none;
    background: var(--accent);
    color: var(--on-accent);
    cursor: pointer;
    border-radius: var(--r-ctl);
    padding: 0 12px;
    font-family: var(--font-text);
    font-size: 13px;
    font-weight: 500;
    transition: opacity var(--t-fast), color var(--t-fast), background var(--t-fast);
    flex-shrink: 0;
  }
  #session-create-panel #sc-create:hover { opacity: 0.88; }
  #session-create-panel #sc-create:active { opacity: 0.76; }
  #session-create-panel button.secondary {
    background: transparent;
    color: var(--label-2);
    padding: 0 4px;
  }
  #session-create-panel button.secondary:hover {
    background: var(--fill-hover);
    color: var(--label);
  }
  #session-create-panel button.secondary:active { background: var(--fill-press); }

  /* Agent tag autocomplete — same sheet as the slash-command dropdown, but
     dropping down from the tag input instead of up from the prompt box. */
  #sc-suggest {
    display: none;
    position: absolute;
    top: 100%;
    left: 126px; right: 10px;
    margin-top: 4px;
    max-height: 240px;
    overflow-y: auto;
    background: var(--glass);
    border-radius: var(--r-card);
    box-shadow: var(--shadow-float), var(--glass-shine);
    z-index: 10;
    padding: 4px;
  }
  #sc-suggest.visible { display: block; animation: menu-pop 0.25s var(--spring); }
  #sc-suggest::-webkit-scrollbar { width: 3px; }
  #sc-suggest::-webkit-scrollbar-thumb { background: var(--label-4); border-radius: 2px; }
  #sc-suggest .cmd-name { font-family: var(--font-text); font-size: 13px; }
  #sc-suggest .cmd-desc { color: var(--label-2); font-size: 11px; }
  #sc-suggest .cmd-item.active {
    background: color-mix(in srgb, var(--accent) 15%, transparent);
    color: var(--label);
  }
  #sc-suggest .cmd-item.active .cmd-name { color: var(--label); }
  #sc-suggest .cmd-item.active .cmd-desc { color: var(--label-2); }
  #sc-suggest .cmd-item:hover:not(.active) { background: var(--fill-hover); }

  @keyframes pulse {
    0%, 100% { opacity: 1; }
    50%       { opacity: 0.25; }
  }

  #fullscreen-btn.active {
    color: var(--accent);
    background: color-mix(in srgb, var(--accent) 15%, transparent);
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
  .msg-copy:hover { background: rgba(255,255,255,0.92); color: rgba(60,60,67,1); }
  @media (prefers-color-scheme: dark) {
    .msg-copy {
      background: rgba(58,58,60,0.82);
      box-shadow: 0 1px 3px rgba(0,0,0,0.35), 0 0 0 0.5px rgba(255,255,255,0.08);
      color: rgba(235,235,245,0.65);
    }
    .msg-copy:hover { background: rgba(72,72,74,0.92); color: rgba(235,235,245,0.9); }
  }
  .msg.agent:hover .msg-copy,
  .msg.user:hover  .msg-copy { opacity: 1; pointer-events: auto; }
  /* User bubbles can be tiny ("ok") — icon-only circle so the pill never
     dwarfs the bubble. */
  .msg.user .msg-copy { padding: 5px; border-radius: 50%; }
  .msg.user .msg-copy span { display: none; }
  .msg-copy.copied { color: var(--dot-ok); opacity: 1; }

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
  .session-messages {
    display: flex;
    flex-direction: column;
    gap: 12px;
    min-height: 100%;
  }

  /* ── Chat search (⌘F) ───────────────────────────────────────────────────── */
  #chat-search {
    display: flex;
    flex-direction: column;
    border-bottom: 1px solid var(--divider);
    flex-shrink: 0;
  }
  #chat-search.hidden { display: none; }
  #cs-bar {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 6px 10px;
    color: var(--text-secondary);
  }
  #cs-input {
    flex: 1;
    min-width: 0;
    border: none;
    outline: none;
    background: transparent;
    font: inherit;
    font-size: 12.5px;
    color: var(--text-primary);
    padding: 2px 0;
  }
  #cs-count {
    font-size: 11px;
    color: var(--text-secondary);
    font-variant-numeric: tabular-nums;
    flex-shrink: 0;
  }
  #cs-bar button {
    border: none;
    background: transparent;
    color: var(--text-secondary);
    cursor: pointer;
    border-radius: 5px;
    width: 20px;
    height: 20px;
    line-height: 1;
    font-size: 12px;
    flex-shrink: 0;
  }
  #cs-bar button:hover { background: var(--hover-bg); color: var(--text-primary); }
  #cs-results {
    max-height: 220px;
    overflow-y: auto;
    overscroll-behavior: contain;
  }
  #cs-results::-webkit-scrollbar { width: 3px; }
  #cs-results::-webkit-scrollbar-thumb { background: var(--scrollbar); border-radius: 2px; }
  #cs-results:empty { display: none; }
  .cs-hit {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 6px 10px;
    cursor: pointer;
    border-top: 1px solid var(--divider);
  }
  .cs-hit:hover, .cs-hit.active { background: var(--hover-bg); }
  .cs-hit-meta {
    display: flex;
    align-items: center;
    gap: 6px;
    font-size: 10.5px;
    color: var(--text-secondary);
  }
  .cs-hit-chat {
    max-width: 40%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    color: var(--accent);
  }
  .cs-hit-snippet {
    font-size: 12px;
    color: var(--text-primary);
    line-height: 1.4;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }
  .cs-hit-snippet mark {
    background: var(--accent);
    color: #fff;
    border-radius: 2px;
    padding: 0 1px;
  }
  #cs-empty {
    padding: 10px;
    font-size: 12px;
    color: var(--text-secondary);
    text-align: center;
  }

  /* Sentinel at the top of a partially-mounted log. */
  /* `.session-messages` is a plain block, so this centers by margin rather
     than align-self. */
  .msg-more {
    display: block;
    width: fit-content;
    margin: 0 auto 10px;
    border: none;
    font: inherit;
    font-size: 11px;
    color: var(--text-secondary);
    padding: 4px 10px;
    border-radius: var(--r-capsule);
    background: var(--hover-bg);
    cursor: pointer;
  }
  .msg-more:hover { color: var(--text-primary); }

  /* Brief flash on the message a search hit jumped to. */
  @keyframes chat-hit-flash {
    0%   { background: color-mix(in srgb, var(--accent) 28%, transparent); }
    100% { background: transparent; }
  }
  .chat-hit {
    animation: chat-hit-flash 1.6s ease-out;
    border-radius: 10px;
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
  #welcome-icon { margin-bottom: 8px; line-height: 0; color: var(--brand); }
  #welcome-icon svg { width: 40px; height: 40px; }
  #welcome-title {
    font-family: -apple-system, "SF Pro Display", "Helvetica Neue", sans-serif;
    font-size: 21px;
    font-weight: 700;
    letter-spacing: -0.02em;
    color: var(--text-primary);
    margin-bottom: 2px;
  }
  #welcome-desc {
    font-size: 12px;
    line-height: 1.5;
    color: var(--text-secondary);
    max-width: 250px;
    margin-bottom: 16px;
  }
  #welcome-suggestions {
    display: flex;
    flex-direction: column;
    gap: 7px;
    width: 100%;
    max-width: 270px;
    margin-bottom: 20px;
  }
  /* Glass capsule chips — float on hover with a springy lift */
  .suggestion-btn {
    display: block;
    width: 100%;
    padding: 9px 14px;
    border: none;
    border-radius: 16px;
    background: var(--input-bg);
    box-shadow: 0 0 0 0.5px var(--input-border), inset 0 1px 0 var(--card-hi),
                0 1px 4px rgba(0,0,0,0.04);
    color: var(--text-secondary);
    font-size: 12px;
    line-height: 1.4;
    text-align: left;
    cursor: pointer;
    transition: transform 0.3s var(--spring), box-shadow 0.2s ease, color 0.15s, background 0.15s;
  }
  .suggestion-btn:hover {
    background: rgba(0, 122, 255, 0.08);
    color: var(--text-primary);
    transform: translateY(-1px);
    box-shadow: 0 0 0 0.5px rgba(0,122,255,0.30), inset 0 1px 0 var(--card-hi),
                0 4px 14px rgba(0,0,0,0.08);
  }
  .suggestion-btn:active { transform: translateY(0) scale(0.98); }
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
    border-radius: 6px;
    background: var(--md-code-bg);
    box-shadow: 0 0 0 0.5px var(--md-code-border), inset 0 1px 0 var(--card-hi);
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
    font-size: 11px;
    font-weight: 600;
    letter-spacing: -0.005em;
    color: var(--label-2);
  }
  .msg-label .msg-time {
    font-size: 11px;
    font-weight: 400;
    letter-spacing: 0.01em;
    color: var(--label-2);
    font-variant-numeric: tabular-nums;
  }
  .msg.user  .msg-label { justify-content: flex-end; }
  .msg.user .msg-label .msg-who,
  .msg.error .msg-label .msg-who,
  .msg.specialist .msg-label .msg-who { color: var(--label-2); opacity: 1; }

  /* Bubbles — solid material cards (HIG: no glass stacked on glass), depth
     comes from a specular inner top edge + soft ambient shadow. */
  .msg-bubble {
    padding: 9px 13px;
    border-radius: 18px;
    line-height: 1.55;
    word-break: break-word;
    font-size: 13px;
  }

  .msg.user .msg-bubble {
    background: var(--user-bg);
    border-bottom-right-radius: 6px;
    color: var(--text-primary);
    align-self: flex-end;
    max-width: 90%;
    white-space: pre-wrap;
    box-shadow: inset 0 0 0 1px var(--user-border), inset 0 1px 0 rgba(255,255,255,0.25);
    transition: box-shadow 0.15s, background 0.15s;
  }
  .msg.user .msg-bubble:hover {
    box-shadow: inset 0 0 0 1px var(--user-border), inset 0 1px 0 rgba(255,255,255,0.25),
                0 2px 8px rgba(0,122,255,0.13);
  }

  .msg.agent .msg-bubble {
    background: var(--agent-bg);
    border-bottom-left-radius: 6px;
    color: var(--text-primary);
    align-self: flex-start;
    max-width: 100%;
    box-shadow: inset 0 0 0 1px var(--agent-border), inset 0 1px 0 var(--card-hi),
                0 1px 4px rgba(0,0,0,0.04);
    transition: box-shadow 0.15s, background 0.15s;
    /* no white-space: pre-wrap — markdown renders HTML */
  }
  .msg.agent .msg-bubble:hover {
    box-shadow: inset 0 0 0 1px var(--agent-border), inset 0 1px 0 var(--card-hi),
                0 3px 12px rgba(0,0,0,0.08);
  }
  @media (prefers-color-scheme: dark) {
    .msg.agent .msg-bubble       { box-shadow: inset 0 0 0 1px var(--agent-border), inset 0 1px 0 var(--card-hi), 0 1px 4px rgba(0,0,0,0.20); }
    .msg.agent .msg-bubble:hover { box-shadow: inset 0 0 0 1px var(--agent-border), inset 0 1px 0 var(--card-hi), 0 3px 12px rgba(0,0,0,0.35); }
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
  .msg.agent .msg-bubble h4 { font-size: 12.5px; }

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
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, monospace;
    font-size: 10px;
    font-weight: 500;
    color: var(--text-secondary);
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
  /* A surface that is holding the agent's turn open wears an accent edge; a
     surface that is only showing you something stays quiet. The edge is the
     whole status indicator, which is why there is no header strip. */
  .msg.ui .msg-bubble { border-left-width: 2px; }
  .msg.ui.awaiting .msg-bubble { border-left-color: var(--accent); }
  .a2ui-body { padding: 12px 14px; }
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
  .a2ui-body > .a2ui-card {
    border: none;
    background: none;
    border-radius: 0;
    padding: 0;
  }
  .a2ui-col { display: flex; flex-direction: column; gap: 8px; }
  .a2ui-row { display: flex; flex-direction: row; flex-wrap: wrap; gap: 6px; align-items: center; }
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
  .a2ui-md h1, .a2ui-md h2, .a2ui-md h3, .a2ui-md h4 {
    margin: 0 0 4px;
    font-weight: 600;
    line-height: 1.3;
    color: var(--text-primary);
  }
  .a2ui-md h1 { font-size: 16px; }
  .a2ui-md h2 { font-size: 14.5px; }
  .a2ui-md h3 { font-size: 13.5px; }
  .a2ui-md h4 { font-size: 12.5px; text-transform: uppercase; letter-spacing: 0.04em; color: var(--text-secondary); }
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
    font-size: 13px;
  }
  .a2ui-label {
    font-size: 13px;
    font-weight: 500;
    color: var(--text-secondary);
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
    font-size: 13px;
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
    font-size: 13px;
    color: var(--text-primary);
  }
  .a2ui-check[hidden] { display: none; }
  .a2ui-check input { margin: 0; cursor: pointer; }
  .a2ui-slider-row { display: flex; align-items: center; gap: 8px; }
  .a2ui-slider-row input[type="range"] { flex: 1; accent-color: var(--accent); }
  .a2ui-slider-val {
    font-variant-numeric: tabular-nums;
    color: var(--text-secondary);
    font-size: 11px;
    min-width: 24px;
    text-align: right;
  }
  .a2ui-list { display: flex; gap: 6px; }
  .a2ui-list-vertical { flex-direction: column; }
  .a2ui-list-horizontal {
    flex-direction: row;
    overflow-x: auto;
    scrollbar-width: none;
    padding-bottom: 2px;
  }
  .a2ui-list-horizontal::-webkit-scrollbar { display: none; }
  .a2ui-list-horizontal > * { flex: 0 0 auto; max-width: 220px; }
  .a2ui-divider { background: var(--divider); margin: 4px 0; }
  .a2ui-divider-horizontal { height: 1px; width: 100%; }
  .a2ui-divider-vertical { width: 1px; align-self: stretch; min-height: 16px; margin: 0 4px; }
  /* No icon font ships with the sidebar, so a named catalog icon renders as a
     quiet placeholder chip rather than pretending to be a glyph. */
  .a2ui-icon {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 16px;
    padding: 0 5px;
    height: 16px;
    border-radius: 4px;
    border: 1px solid var(--divider);
    color: var(--text-tertiary);
    font-size: 9.5px;
    font-weight: 500;
    letter-spacing: 0.01em;
  }
  .a2ui-video, .a2ui-audio audio { max-width: 100%; border-radius: 8px; display: block; }
  .a2ui-audio { display: flex; flex-direction: column; gap: 4px; }
  /* Text variants (official v0.9: h1-h5, caption, body) */
  .a2ui-text-h1 { font-size: 17px; font-weight: 600; line-height: 1.2; margin: 0; }
  .a2ui-text-h2 { font-size: 15px; font-weight: 600; line-height: 1.25; margin: 0; }
  .a2ui-text-h3 { font-size: 14px; font-weight: 600; line-height: 1.3; margin: 0; }
  .a2ui-text-h4 { font-size: 13px; font-weight: 600; line-height: 1.3; margin: 0; }
  .a2ui-text-h5 { font-size: 12px; font-weight: 600; line-height: 1.35; }
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
    font-size: 13px;
    cursor: pointer;
    transition: background 0.12s, border-color 0.12s, color 0.12s;
  }
  .a2ui-chip:hover { border-color: var(--input-focus-border); }
  .a2ui-chip.on {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--on-accent);
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
    font-size: 13px;
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
    background: rgba(0,0,0,0.10);
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
    font-size: 13px;
    font-weight: 600;
    cursor: pointer;
    transition: filter 0.12s, box-shadow 0.12s, transform 0.08s;
    color: var(--on-accent);
  }
  .a2ui-btn:active:not(:disabled) { transform: translateY(0.5px); }
  .a2ui-btn .a2ui-text,
  .a2ui-btn .a2ui-md,
  .a2ui-btn .a2ui-icon,
  .a2ui-btn .a2ui-heading { color: inherit; background: none; font-size: inherit; font-weight: inherit; }
  .a2ui-btn .a2ui-md p { margin: 0; }
  .a2ui-btn:disabled { opacity: 0.4; cursor: not-allowed; }
  .a2ui-btn.primary { background: var(--accent); }
  .a2ui-btn.primary:hover:not(:disabled) { filter: brightness(1.07); box-shadow: 0 1px 6px rgba(0,122,255,0.22); }
  /* The catalog's standard control: a real Tahoe fill, not the flat grey of a
     code block, so an ordinary button never reads as a disabled one. */
  .a2ui-btn.default {
    background: var(--fill);
    border-color: var(--hairline);
    color: var(--text-primary);
  }
  .a2ui-btn.default:hover:not(:disabled) { background: var(--fill-hover); }
  .a2ui-btn.default:active:not(:disabled) { background: var(--fill-press); }
  .a2ui-btn.borderless {
    background: transparent;
    border-color: transparent;
    color: var(--accent);
    padding: 4px 6px;
  }
  .a2ui-btn.borderless:hover:not(:disabled) { text-decoration: underline; }
  .a2ui-btn.success { background: var(--dot-ok); }
  .a2ui-btn.warn    { background: var(--dot-wait); }
  .a2ui-btn.danger  { background: var(--dot-err); }
  .a2ui-btn.success:hover:not(:disabled),
  .a2ui-btn.warn:hover:not(:disabled),
  .a2ui-btn.danger:hover:not(:disabled) { filter: brightness(1.07); }
  .a2ui-check-msg {
    display: block;
    margin-top: 3px;
    font-size: 11px;
    line-height: 1.35;
    color: var(--dot-err);
  }
  .a2ui-choice-filter {
    margin-bottom: 4px;
    font-family: inherit;
    font-size: 13px;
  }
  .a2ui-icon-svg { display: inline-block; vertical-align: middle; color: var(--text-secondary); }
  /* Text renders Markdown per the v1.0 catalog, so it can't also pre-wrap. */
  .a2ui-text.a2ui-md { white-space: normal; }
  .a2ui-unknown {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 7px;
    padding: 8px 10px;
    border-radius: 8px;
    background: var(--error-bg);
    color: var(--error-text);
    font-size: 13px;
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

  /* ── Activity feed — live tool stream during a turn ─────────────────────── */
  .activity-feed {
    display: none;
    flex-direction: column;
    gap: 1px;
    padding: 4px 0 6px;
  }
  .activity-feed.visible { display: flex; }

  /* Tahoe-style header: 3-dot bounce + shimmer label + turn elapsed */
  .activity-header {
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 2px 4px 5px;
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
  .activity-label {
    font-size: 11px;
    font-weight: 500;
    background: linear-gradient(90deg,
      var(--text-tertiary) 30%, var(--text-primary) 50%, var(--text-tertiary) 70%);
    background-size: 200% 100%;
    -webkit-background-clip: text;
    background-clip: text;
    -webkit-text-fill-color: transparent;
    animation: label-shimmer 2.4s linear infinite;
  }
  @keyframes label-shimmer {
    from { background-position: 200% 0; }
    to   { background-position: -200% 0; }
  }
  .activity-elapsed {
    font-size: 10px;
    color: var(--text-tertiary);
    font-variant-numeric: tabular-nums;
    margin-left: 2px;
    opacity: 0.7;
  }

  /* Tool item = row + inline expandable detail (live feed and steps group) */
  .tool-item { display: flex; flex-direction: column; min-width: 0; }
  .tool-row {
    display: flex;
    align-items: center;
    gap: 7px;
    padding: 3px 5px;
    font-size: 11px;
    color: var(--text-secondary);
    animation: tool-in 0.25s ease-out;
    overflow: hidden;
    cursor: pointer;
    border-radius: 6px;
    transition: background 0.12s;
  }
  .tool-row:hover {
    background: var(--hover-bg);
  }
  @keyframes tool-in {
    from { opacity: 0; transform: translateY(4px); }
    to   { opacity: 1; transform: translateY(0); }
  }
  .tool-row.done {
    opacity: 0.55;
    transition: opacity 0.3s ease, background 0.12s;
  }
  .tool-row.failed {
    color: var(--error-text);
    opacity: 0.8;
  }

  /* Kind icon — tinted rounded square with stroke glyph (Tahoe) */
  .tool-kind {
    width: 16px; height: 16px;
    border-radius: 5px;
    display: flex;
    align-items: center;
    justify-content: center;
    flex-shrink: 0;
    color: #8e8e93;
    background: rgba(142, 142, 147, 0.14);
  }
  .tool-kind svg { width: 10px; height: 10px; display: block; }
  .tool-kind.read    { color: #34c759; background: rgba(52, 199, 89, 0.14); }
  .tool-kind.edit    { color: #ff9500; background: rgba(255, 149, 0, 0.14); }
  .tool-kind.delete  { color: #ff3b30; background: rgba(255, 59, 48, 0.14); }
  .tool-kind.search  { color: #5856d6; background: rgba(88, 86, 214, 0.14); }
  .tool-kind.execute { color: #007aff; background: rgba(0, 122, 255, 0.14); }
  .tool-kind.think   { color: #af52de; background: rgba(175, 82, 222, 0.14); }
  .tool-kind.fetch   { color: #30b0c7; background: rgba(48, 176, 199, 0.14); }
  .tool-kind.move    { color: #ff2d55; background: rgba(255, 45, 85, 0.14); }
  @media (prefers-color-scheme: dark) {
    .tool-kind.read    { color: #30d158; }
    .tool-kind.edit    { color: #ff9f0a; }
    .tool-kind.delete  { color: #ff453a; }
    .tool-kind.search  { color: #5e5ce6; }
    .tool-kind.execute { color: #0a84ff; }
    .tool-kind.think   { color: #bf5af2; }
    .tool-kind.fetch   { color: #40c8e0; }
    .tool-kind.move    { color: #ff375f; }
  }
  .tool-row.running .tool-kind { animation: kind-pulse 1.4s ease-in-out infinite; }
  @keyframes kind-pulse {
    0%, 100% { opacity: 1; }
    50%      { opacity: 0.4; }
  }

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
  .tool-check, .tool-fail {
    flex-shrink: 0;
    width: 11px; height: 11px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    line-height: 0;
  }
  .tool-check svg, .tool-fail svg { width: 100%; height: 100%; }
  .tool-check { color: var(--dot-ok); }
  .tool-fail  { color: var(--dot-err); }

  /* Inline expandable detail under a tool row — inset well, no hairline box */
  .tool-detail {
    display: none;
    margin: 1px 5px 5px 28px;
    padding: 8px 10px;
    background: var(--md-pre-bg);
    border-radius: 10px;
    box-shadow: inset 0 1px 2px rgba(0,0,0,0.05);
    min-width: 0;
    animation: tool-in 0.25s var(--spring);
  }
  .tool-item.expanded .tool-detail { display: block; }
  .tool-detail-title {
    font-size: 10px;
    font-weight: 600;
    color: var(--text-tertiary);
    margin: 8px 0 3px;
  }
  .tool-detail-title:first-child { margin-top: 0; }
  .tool-detail pre {
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, monospace;
    font-size: 10px;
    line-height: 1.5;
    white-space: pre-wrap;
    word-break: break-word;
    max-height: 180px;
    overflow-y: auto;
    color: var(--text-primary);
  }
  .tool-detail pre::-webkit-scrollbar { width: 3px; }
  .tool-detail pre::-webkit-scrollbar-thumb { background: var(--scrollbar); border-radius: 2px; }
  .tool-loc {
    font-family: ui-monospace, SFMono-Regular, Menlo, Monaco, monospace;
    font-size: 10px;
    color: var(--text-primary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  /* ── Steps group — quiet inline disclosure while collapsed; becomes a
     floating glass capsule only when expanded ──────────────────────────── */
  .tool-group {
    position: relative;
    align-self: flex-start;
    max-width: 100%;
    min-width: 0;
    background: transparent;
    border-radius: 14px;
    overflow: hidden;
    transition: background 0.25s ease, box-shadow 0.25s ease;
  }
  .tool-group.expanded {
    background: var(--agent-bg);
    box-shadow: 0 0 0 0.5px var(--agent-border), 0 3px 12px rgba(0,0,0,0.08);
    align-self: stretch;
    overflow: visible;
    margin-bottom: 5px;
    z-index: 1;
  }
  .tool-group-header {
    display: flex;
    align-items: center;
    gap: 6px;
    padding: 3px 10px 3px 6px;
    border-radius: 11px;
    font-size: 11px;
    color: var(--text-tertiary);
    cursor: pointer;
    user-select: none;
    -webkit-user-select: none;
    transition: background 0.12s, color 0.12s;
  }
  .tool-group-header:hover { color: var(--text-secondary); background: var(--hover-bg); }
  .tool-group.expanded .tool-group-header {
    border-radius: 0;
    padding: 5px 11px 5px 9px;
    color: var(--text-secondary);
  }
  .tool-group.expanded .tool-group-header:hover { background: var(--hover-bg); }
  .tool-group-chevron {
    display: flex;
    color: var(--text-tertiary);
    transition: transform 0.3s var(--spring);
  }
  .tool-group.expanded .tool-group-chevron { transform: rotate(90deg); }
  .tool-group-count { font-weight: 500; }
  .tool-group-fail { color: var(--dot-err); }
  .tool-group-time {
    color: var(--text-tertiary);
    font-variant-numeric: tabular-nums;
    font-size: 10px;
  }
  .tool-group-list {
    display: none;
    flex-direction: column;
    gap: 1px;
    padding: 3px 6px 6px;
    border-top: 1px solid var(--divider);
  }
  .tool-group.expanded .tool-group-list { display: flex; }

  /* ── Specialist reports — quiet native disclosure, collapsed by default ─ */
  .specialist-report {
    align-self: flex-start;
    width: 100%;
    max-width: 100%;
    min-width: 0;
  }
  .specialist-report-details {
    width: 100%;
    max-width: 100%;
  }
  .specialist-report-summary {
    display: flex;
    align-items: center;
    gap: 6px;
    min-width: 0;
    padding: 3px 10px 3px 6px;
    border-radius: 11px;
    color: var(--text-tertiary);
    cursor: pointer;
    font-size: 11px;
    list-style: none;
    user-select: none;
    -webkit-user-select: none;
    transition: background 0.12s, color 0.12s;
  }
  .specialist-report-summary::-webkit-details-marker { display: none; }
  .specialist-report-summary:hover {
    color: var(--text-secondary);
    background: var(--hover-bg);
  }
  .specialist-report-chevron {
    display: flex;
    flex-shrink: 0;
    color: var(--text-tertiary);
    transition: transform 0.3s var(--spring);
  }
  .specialist-report-details[open] .specialist-report-chevron { transform: rotate(90deg); }
  .specialist-report-who {
    flex-shrink: 0;
    color: var(--label-2);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: -0.005em;
  }
  .specialist-report-title {
    flex-shrink: 0;
    font-weight: 500;
  }
  .specialist-report-details.failed .specialist-report-status { color: var(--error-text); }
  .specialist-report-preview {
    flex: 1;
    min-width: 0;
    overflow: hidden;
    color: var(--text-tertiary);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .specialist-report-time {
    flex-shrink: 0;
    margin-left: auto;
    color: var(--label-2);
    font-size: 11px;
    font-weight: 400;
    font-variant-numeric: tabular-nums;
    letter-spacing: 0.01em;
  }
  .msg.agent .msg-bubble.specialist-report-body {
    margin-top: 3px;
    padding: 8px 11px;
    border-radius: 12px;
    font-size: 12px;
  }
  .specialist-report-input {
    margin-bottom: 8px;
    color: var(--text-secondary);
  }
  .specialist-report-input > summary {
    color: var(--text-tertiary);
    cursor: pointer;
    font-size: 10.5px;
    font-weight: 500;
  }
  .specialist-report-input-text {
    margin-top: 4px;
    padding: 6px 8px;
    border: 1px solid var(--md-code-border);
    border-radius: 8px;
    background: var(--md-pre-bg);
    color: var(--text-secondary);
    font-size: 11px;
    line-height: 1.45;
    white-space: pre-wrap;
    word-break: break-word;
  }

  /* ── Input area ─────────────────────────────────────────────────────────── */
  #input-area {
    flex-shrink: 0;
    padding: 10px 12px 14px;
    display: flex;
    flex-direction: column;
    gap: 6px;
    position: relative;
  }
  #composer-hint {
    padding: 0 10px;
    color: var(--label-2);
    font-size: 11px;
    line-height: 1.3;
    text-align: right;
  }

  /* ── Slash command dropdown — floating glass sheet with springy pop ──── */
  #cmd-dropdown {
    display: none;
    position: absolute;
    bottom: 100%;
    left: 4px; right: 4px;
    margin-bottom: 4px;
    max-height: 200px;
    overflow-y: auto;
    background: var(--glass-solid);
    border-radius: 14px;
    /* Inset rim instead of ::before ring — this element scrolls */
    box-shadow: var(--glass-shadow), inset 0 1px 0 var(--rim-hi), inset 0 0 0 0.5px var(--rim-lo);
    z-index: 10;
    padding: 4px;
  }
  #cmd-dropdown.visible { display: block; animation: menu-pop 0.25s var(--spring); }
  @keyframes menu-pop {
    from { opacity: 0; transform: translateY(5px) scale(0.97); }
    to   { opacity: 1; transform: translateY(0)   scale(1); }
  }
  #cmd-dropdown::-webkit-scrollbar { width: 3px; }
  #cmd-dropdown::-webkit-scrollbar-thumb { background: var(--scrollbar); border-radius: 2px; }

  .cmd-item {
    padding: 6px 10px;
    border-radius: 9px;
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
  .cmd-output-table { width: 100%; border-collapse: collapse; margin: 0; }
  .cmd-output-table tr { border-bottom: 1px solid var(--divider); }
  .cmd-output-table tr:last-child { border-bottom: none; }
  .msg.agent .msg-bubble .cmd-output-table td {
    border: none;
    border-bottom: 0;
  }
  .msg.agent .msg-bubble .cmd-output-table .cmd-output-key {
    padding: 3px 10px 3px 0;
  }
  .msg.agent .msg-bubble .cmd-output-table .cmd-output-val {
    padding: 3px 0;
  }
  .cmd-output-key {
    padding: 3px 10px 3px 0;
    color: var(--text-tertiary);
    font-size: 11px;
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
    min-width: 0;
  }
  .cmd-output-pre {
    margin: 0;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    font: inherit;
    color: inherit;
  }
  .cmd-records { display: flex; flex-direction: column; }
  .cmd-record {
    min-width: 0;
    padding: 5px 0;
    border-bottom: 1px solid var(--divider);
  }
  .cmd-record:last-child { border-bottom: none; }
  .cmd-record > .cmd-output-table { margin: 0; }
  .cmd-record > .cmd-output-table > tbody > tr:last-child { border-bottom: none; }
  .cmd-output-list { margin: 0; padding-left: 16px; }
  .cmd-output-list li { margin: 1px 0; font-size: 11.5px; }
  .cmd-output-single-list { display: flex; flex-direction: column; gap: 2px; }
  .cmd-output-list-label {
    font-size: 11px;
    font-weight: 600;
    color: var(--text-tertiary);
  }

  /* Section title — quiet sentence-case group header */
  .cmd-section-title {
    font-size: 11.5px;
    font-weight: 600;
    color: var(--text-secondary);
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
    font-size: 11px;
    font-weight: 600;
    color: var(--text-secondary);
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
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    margin: 2px 0;
  }
  .cmd-stat {
    display: flex;
    flex-direction: column;
    justify-content: center;
    padding: 5px 8px;
    min-width: 0;
  }
  .cmd-stat:nth-child(odd) {
    padding-left: 0;
    border-right: 1px solid var(--divider);
  }
  .cmd-stat:nth-child(even) { padding-right: 0; }
  .cmd-stat:only-child { border: none; padding-left: 0; }
  .cmd-stat-label {
    font-size: 10.5px;
    font-weight: 500;
    color: var(--text-tertiary);
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
    white-space: normal;
    overflow-wrap: anywhere;
    overflow: hidden;
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

  /* Agent runs — dense enough for the sidebar, with stable aligned columns. */
  .cmd-agent-row { padding: 6px 0; }
  .cmd-agent-row .cmd-item-head { align-items: center; }
  .cmd-agent-id {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font: 9.5px 'SF Mono', Monaco, monospace;
    color: var(--text-tertiary);
  }
  .cmd-agent-facts {
    display: flex;
    flex-wrap: wrap;
    gap: 2px 8px;
    color: var(--text-secondary);
    font-size: 10px;
    font-variant-numeric: tabular-nums;
  }
  .cmd-agent-path, .cmd-agent-action {
    min-width: 0;
    overflow-wrap: anywhere;
    font-size: 10px;
    color: var(--text-tertiary);
  }
  .cmd-agent-action { color: var(--text-secondary); }
  .cmd-agent-action::before { content: '↳ '; color: var(--text-tertiary); }

  /* Quota rows used by /usage. */
  .cmd-quotas { display: flex; flex-direction: column; gap: 7px; }
  .cmd-quota-head {
    display: grid;
    grid-template-columns: minmax(62px, 1fr) auto;
    gap: 8px;
    align-items: baseline;
    margin-bottom: 3px;
    font-size: 10.5px;
  }
  .cmd-quota-name { color: var(--text-secondary); font-weight: 600; }
  .cmd-quota-value { color: var(--text-tertiary); font-variant-numeric: tabular-nums; }
  .cmd-quota-track {
    height: 4px;
    overflow: hidden;
    border-radius: 999px;
    background: var(--divider);
  }
  .cmd-quota-fill {
    display: block;
    height: 100%;
    border-radius: inherit;
    background: var(--accent);
  }
  .cmd-quota-fill.warn { background: #ff9500; }
  .cmd-quota-fill.err { background: #ff3b30; }

  /* Badges — small, flat */
  .cmd-badge {
    display: inline-block;
    padding: 0 6px;
    border-radius: 5px;
    font-size: 10px;
    font-weight: 600;
    line-height: 15px;
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
    width: 13px; height: 13px;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    line-height: 0;
    flex-shrink: 0;
  }
  .cmd-toast-icon svg { width: 100%; height: 100%; }
  .cmd-toast.err .cmd-toast-icon { color: #ff3b30; }
  .cmd-error-text { color: #c1271d; }
  @media (prefers-color-scheme: dark) {
    .cmd-error-text { color: #ff6961; }
  }

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
    overflow-wrap: anywhere;
    vertical-align: top;
  }
  .msg.agent .msg-bubble .cmd-md th,
  .msg.agent .msg-bubble .cmd-md td {
    border: none;
    border-bottom: 1px solid var(--divider);
  }
  .cmd-md th {
    color: var(--text-secondary);
    font-weight: 600;
    font-size: 10.5px;
  }

  /* Floating glass capsule — no border, depth from shadow + specular rim */
  #input-row {
    position: relative;
    display: flex;
    align-items: center;
    gap: 8px;
    background: var(--input-bg);
    border-radius: 22px;
    padding: 9px 9px 9px 11px;
    box-shadow: 0 0 0 0.5px var(--input-border), var(--float-shadow);
    transition: box-shadow 0.25s ease, transform 0.3s var(--spring);
  }

  #new-session-btn {
    width: 26px; height: 26px;
    border-radius: var(--r-ctl);
    border: none;
    background: transparent;
    color: var(--label-2);
    cursor: pointer;
    display: flex; align-items: center; justify-content: center;
    flex-shrink: 0;
    transition: background var(--t-fast), color var(--t-fast);
    padding: 0;
  }
  #new-session-btn svg { width: 14px; height: 14px; }
  #new-session-btn:hover {
    background: var(--fill-hover);
    color: var(--label-2);
  }
  #new-session-btn:active { background: var(--fill-press); }
  #input-row:focus-within {
    transform: translateY(-1px);
    box-shadow: 0 0 0 1px var(--input-focus-border), var(--float-shadow),
                0 0 0 4px rgba(0, 122, 255, 0.12);
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
    width: 28px; height: 28px;
    border-radius: 50%;
    border: none;
    background: transparent;
    color: var(--label-2);
    cursor: pointer;
    display: flex; align-items: center; justify-content: center;
    transition: background 0.15s, color 0.15s;
    padding: 0;
  }
  #attach-btn:hover { background: var(--hover-bg); color: var(--text-primary); }
  #attach-btn:active { transform: scale(0.88); }

  #attach-menu {
    display: none;
    position: absolute;
    bottom: calc(100% + 8px);
    right: 0;
    background: var(--glass-bg);
    -webkit-backdrop-filter: blur(40px) saturate(1.6);
    backdrop-filter: blur(40px) saturate(1.6);
    border-radius: 14px;
    padding: 4px;
    box-shadow: 0 0 0 0.5px var(--input-border), var(--float-shadow);
    z-index: 10;
    min-width: 130px;
  }
  #attach-menu.visible { display: flex; flex-direction: column; animation: menu-pop 0.25s var(--spring); }
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
  .attach-option:hover { background: var(--hover-bg); }
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
    width: 22px; height: 22px; border-radius: 50%;
    background: rgba(0,0,0,0.15); color: var(--text-secondary);
    font-size: 11px; line-height: 22px; text-align: center;
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
    width: 22px; height: 22px; border-radius: 50%;
    background: rgba(0,0,0,0.6); color: var(--on-accent);
    font-size: 11px; line-height: 22px; text-align: center;
    cursor: pointer; border: none; padding: 0;
  }

  .msg-bubble img.chat-img {
    max-width: 100%;
    border-radius: 8px;
    margin-top: 6px;
    display: block;
  }

  /* Gel button: vertical gradient + specular top edge + accent glow */
  #send-btn {
    width: 28px; height: 28px;
    border-radius: 50%;
    border: none;
    background: linear-gradient(180deg, #2e9bff 0%, var(--accent) 55%, #0070e8 100%);
    box-shadow: inset 0 1px 0 rgba(255,255,255,0.45), 0 2px 8px rgba(0,122,255,0.35);
    color: var(--on-accent);
    cursor: pointer;
    display: flex; align-items: center; justify-content: center;
    flex-shrink: 0;
    transition: opacity 0.15s, transform 0.3s var(--spring), box-shadow 0.2s ease, filter 0.15s;
    opacity: 0.28;
    pointer-events: none;
  }
  #send-btn.active              { opacity: 1; pointer-events: auto; }
  #send-btn.active:hover        { filter: brightness(1.08); box-shadow: inset 0 1px 0 rgba(255,255,255,0.45), 0 3px 12px rgba(0,122,255,0.50); }
  #send-btn.active:active       { transform: scale(0.88); }
  /* Stop mode — red gel, pulsing */
  #send-btn.stop-mode {
    background: linear-gradient(180deg, #ff6259 0%, #ff3b30 55%, #e02d23 100%);
    box-shadow: inset 0 1px 0 rgba(255,255,255,0.40), 0 2px 8px rgba(255,59,48,0.40);
    border-radius: 50%;
    opacity: 1;
    pointer-events: auto;
    animation: stop-pulse 1.2s ease-in-out infinite;
  }
  #send-btn.stop-mode:hover     { filter: brightness(1.06); }
  #send-btn.stop-mode:active    { transform: scale(0.88); }
  #send-btn.stop-mode .send-icon { display: none; }
  #send-btn.stop-mode .stop-icon { display: block; }
  #send-btn .stop-icon          { display: none; }
  @keyframes stop-pulse {
    0%, 100% { opacity: 1; }
    50% { opacity: 0.7; }
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

  <div id="resize-handle" role="separator" aria-label="Resize sidebar" aria-orientation="vertical" tabindex="0"></div>

  <!-- Header — session tabs strip -->
  <div id="header">
    <span id="header-logo" title="Octopus" aria-hidden="true">/* ICON_OCTOPUS_BRAND */</span>
    <div id="session-strip" role="tablist" aria-label="ACP sessions"></div>
    <button id="session-add-btn" type="button" title="New session" aria-label="New session">
      <svg width="10" height="10" viewBox="0 0 10 10" fill="none">
        <path d="M5 1v8M1 5h8" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
      </svg>
    </button>
    <button id="fullscreen-btn" type="button" title="Toggle fullscreen" aria-label="Toggle assistant fullscreen">
      <svg class="ic-enter" width="10" height="10" viewBox="0 0 10 10" fill="none">
        <path d="M1 4V1h3M9 4V1H6M1 6v3h3M9 6v3H6"
              stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/>
      </svg>
      <svg class="ic-exit" width="10" height="10" viewBox="0 0 10 10" fill="none" style="display:none">
        <path d="M4 1v3H1M6 1v3h3M4 9V6H1M6 9V6h3"
              stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/>
      </svg>
    </button>
    <button id="close-btn" type="button" title="Close sidebar" aria-label="Close sidebar">
      <svg width="8" height="8" viewBox="0 0 8 8" fill="none">
        <path d="M1 1L7 7M7 1L1 7" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/>
      </svg>
    </button>
  </div>

  <!-- Account bar — login / quota status (populated by __setAccount) -->
  <div id="account-bar" class="hidden" role="status" aria-live="polite">
    <span class="account-dot"></span>
    <span id="account-text"></span>
    <button id="account-action" type="button" style="display:none"></button>
    <button id="account-dismiss" type="button" title="Dismiss" aria-label="Dismiss">×</button>
  </div>

  <!-- Inline create-session panel (toggled by + button) -->
  <div id="session-create-panel">
    <input id="sc-title" type="text" placeholder="Session name" aria-label="Session name" autocomplete="off" spellcheck="false" maxlength="32">
    <div class="sc-agent-field">
      <input id="sc-tag" type="text" placeholder="Agent" autocomplete="off" spellcheck="false" aria-label="Agent" aria-describedby="sc-tag-meta" role="combobox" aria-autocomplete="list" aria-controls="sc-suggest" aria-expanded="false">
      <span id="sc-tag-meta"></span>
    </div>
    <button id="sc-create" type="button">Create</button>
    <button id="sc-cancel" type="button" class="secondary">Cancel</button>
    <div id="sc-suggest" role="listbox" aria-label="Agents"></div>
  </div>

  <!-- Chat search (⌘F while the sidebar has focus) — searches every session's
       message log, including messages not currently mounted in the DOM -->
  <div id="chat-search" class="hidden">
    <div id="cs-bar">
      <svg width="12" height="12" viewBox="0 0 16 16" fill="none" aria-hidden="true">
        <circle cx="7" cy="7" r="4.5" stroke="currentColor" stroke-width="1.5"/>
        <path d="M10.5 10.5L14 14" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/>
      </svg>
      <input id="cs-input" type="text" placeholder="Search all chats" autocomplete="off" spellcheck="false" aria-label="Search conversations">
      <span id="cs-count"></span>
      <button id="cs-prev" type="button" title="Previous (⇧⏎)" aria-label="Previous match">↑</button>
      <button id="cs-next" type="button" title="Next (⏎)" aria-label="Next match">↓</button>
      <button id="cs-close" type="button" title="Close (Esc)" aria-label="Close search">×</button>
    </div>
    <div id="cs-results" role="listbox"></div>
  </div>

  <!-- Messages — host element; per-session containers mounted/swapped by JS -->
  <div id="messages"></div>

  <!-- Welcome screen — shown when a session has no messages yet -->
  <div id="welcome">
    <div id="welcome-icon" aria-hidden="true">/* ICON_OCTOPUS_BRAND */</div>
    <div id="welcome-title">How can I help?</div>
    <div id="welcome-desc">Ask questions, paste code, describe a bug, or attach a file. The assistant can browse and act on your behalf in the background.</div>
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
          placeholder="Ask Octopus"
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
    <div id="composer-hint">↵ send, ⇧↵ newline</div>
    <div id="image-preview"></div>
    <div id="queue-list"></div>
  </div>

</div>

<!-- marked.js — lightweight MD parser, served from embedded binary -->
<script src="octoweb-lib://localhost/marked.min.js"></script>
<!-- fuzzysort — fuzzy matcher for the new-session agent tag autocomplete -->
<script src="octoweb-lib://localhost/fuzzysort.min.js"></script>
<script>
  // Inline icon strings injected from src/icons.rs (Lucide stroke icons).
  const ICON_CHECK        = '/* ICON_CHECK */';
  const ICON_CHECK_CIRCLE = '/* ICON_CHECK_CIRCLE */';
  const ICON_X_CIRCLE     = '/* ICON_X_CIRCLE */';

  // Configure marked with an explicit allowlist boundary. Agent Markdown is
  // untrusted: raw HTML is displayed as text, URLs are scheme-checked, and
  // every value inserted into generated HTML is escaped first.
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
        const label = l ? '<span class="code-lang">' + escapeHtml(l) + '</span>' : '';
        return '<div class="code-block">' +
          '<div class="code-header">' + label +
            '<button class="code-copy" onclick="__copyCode(this)">' +
              _copyIcon + '<span>Copy</span>' +
            '</button>' +
          '</div>' +
          '<pre><code' + (l ? ' class="language-' + escapeAttr(l) + '"' : '') + '>' +
            escapeHtml(text) +
          '</code></pre>' +
        '</div>';
      },
      html(text) {
        return escapeHtml(text);
      },
      link(href, title, text) {
        const target = String(href == null ? '' : href).trim();
        if (!/^(https?:\/\/|mailto:)/i.test(target)) return text;
        const titleAttr = title == null ? '' : ' title="' + escapeAttr(title) + '"';
        return '<a href="' + escapeAttr(target) + '"' + titleAttr + ' rel="noopener">' + text + '</a>';
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
  const scTagMeta      = document.getElementById('sc-tag-meta');
  const scCreate       = document.getElementById('sc-create');
  const scSuggest      = document.getElementById('sc-suggest');
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
  const resizeHandle   = document.getElementById('resize-handle');

  // The sidebar moves while its left edge is dragged, so screenX is the only
  // stable pointer coordinate. Live resize messages are limited to one per
  // animation frame; the final message is persisted by Rust.
  let resizePointerId = null;
  let resizeStartScreenX = 0;
  let resizeStartWidth = Math.round(window.innerWidth);
  let resizeWidth = resizeStartWidth;
  let resizeFrame = 0;

  function queueSidebarResize(width) {
    resizeWidth = Math.max(0, Math.round(width));
    if (resizeFrame) return;
    resizeFrame = requestAnimationFrame(() => {
      resizeFrame = 0;
      window.ipc.postMessage(JSON.stringify({ type: 'sidebar_resize', width: resizeWidth }));
    });
  }

  resizeHandle.addEventListener('pointerdown', e => {
    if (!e.isPrimary || e.button !== 0) return;
    e.preventDefault();
    e.stopPropagation();
    resizePointerId = e.pointerId;
    resizeStartScreenX = e.screenX;
    resizeStartWidth = Math.round(window.innerWidth);
    resizeWidth = resizeStartWidth;
    resizeHandle.setPointerCapture(e.pointerId);
    resizeHandle.classList.add('active');
    document.body.classList.add('sidebar-resizing');
  });

  resizeHandle.addEventListener('pointermove', e => {
    if (e.pointerId !== resizePointerId) return;
    e.preventDefault();
    e.stopPropagation();
    queueSidebarResize(resizeStartWidth + resizeStartScreenX - e.screenX);
  });

  function finishSidebarResize(e) {
    if (e.pointerId !== resizePointerId) return;
    e.preventDefault();
    e.stopPropagation();
    if (e.type !== 'pointercancel') {
      resizeWidth = Math.max(0, Math.round(resizeStartWidth + resizeStartScreenX - e.screenX));
    }
    if (resizeFrame) {
      cancelAnimationFrame(resizeFrame);
      resizeFrame = 0;
    }
    if (resizeHandle.hasPointerCapture(e.pointerId)) {
      resizeHandle.releasePointerCapture(e.pointerId);
    }
    resizePointerId = null;
    resizeHandle.classList.remove('active');
    document.body.classList.remove('sidebar-resizing');
    window.ipc.postMessage(JSON.stringify({ type: 'sidebar_resize_end', width: resizeWidth }));
  }

  resizeHandle.addEventListener('pointerup', finishSidebarResize);
  resizeHandle.addEventListener('pointercancel', finishSidebarResize);
  resizeHandle.addEventListener('dblclick', e => {
    e.preventDefault();
    e.stopPropagation();
    window.ipc.postMessage(JSON.stringify({ type: 'sidebar_resize_reset' }));
  });
  resizeHandle.addEventListener('keydown', e => {
    if (e.key !== 'ArrowLeft' && e.key !== 'ArrowRight') return;
    e.preventDefault();
    e.stopPropagation();
    const step = e.shiftKey ? 64 : 16;
    const direction = e.key === 'ArrowLeft' ? 1 : -1;
    resizeWidth = Math.max(0, Math.round(window.innerWidth) + direction * step);
    window.ipc.postMessage(JSON.stringify({ type: 'sidebar_resize_end', width: resizeWidth }));
  });

  // Control tooltips follow the effective keymap rather than compiled defaults.
  let sessionCloseTitle = 'Close session';
  let fullscreenShortcut = '';
  window.__setShortcuts = function(data) {
    const actions = data && Array.isArray(data.actions) ? data.actions : [];
    const chordFor = function(id) {
      const action = actions.find(function(item) { return item.id === id; });
      return action && Array.isArray(action.keys) ? action.keys.join('') : '';
    };
    const newSessionChord = chordFor('new_session');
    const sidebarChord = chordFor('sidebar');
    fullscreenShortcut = chordFor('sidebar_fullscreen');
    document.getElementById('session-add-btn').title = 'New session' + (newSessionChord ? ' (' + newSessionChord + ')' : '');
    document.getElementById('close-btn').title = 'Close sidebar' + (sidebarChord ? ' (' + sidebarChord + ')' : '');
    const fullscreenLabel = document.getElementById('fullscreen-btn').classList.contains('active')
      ? 'Exit fullscreen'
      : 'Toggle fullscreen';
    document.getElementById('fullscreen-btn').title = fullscreenLabel + (fullscreenShortcut ? ' (' + fullscreenShortcut + ')' : '');
    const closeChord = chordFor('close_tab');
    sessionCloseTitle = 'Close session' + (closeChord ? ' (' + closeChord + ')' : '');
    document.querySelectorAll('.session-close').forEach(function(button) {
      button.title = sessionCloseTitle;
    });
  };
  window.__setShortcuts(/* KEYBINDINGS_JSON */);

  const MAX_SESSIONS = /* MAX_SESSIONS */;
  const MAX_QUEUE    = 2;
  const MAX_PROMPT_HISTORY = /* MAX_PROMPT_HISTORY */;

  // Tool-kind glyphs — mini stroke icons, tinted by .tool-kind.<kind> CSS.
  const KIND_ICONS = {
    read:    '<svg viewBox="0 0 16 16" fill="none"><path d="M4 1.5h5.5L13 5v9a1.5 1.5 0 0 1-1.5 1.5h-7A1.5 1.5 0 0 1 3 14V3A1.5 1.5 0 0 1 4.5 1.5z" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"/><path d="M9.5 1.5V5H13" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"/></svg>',
    edit:    '<svg viewBox="0 0 16 16" fill="none"><path d="M11.5 2.5a1.7 1.7 0 0 1 2.4 2.4L5 13.8l-3.2.8.8-3.2 8.9-8.9z" stroke="currentColor" stroke-width="1.5" stroke-linejoin="round"/></svg>',
    delete:  '<svg viewBox="0 0 16 16" fill="none"><path d="M2.5 4h11M5.5 4V2.8A1.3 1.3 0 0 1 6.8 1.5h2.4a1.3 1.3 0 0 1 1.3 1.3V4m1.7 0v9.2a1.3 1.3 0 0 1-1.3 1.3H5.1a1.3 1.3 0 0 1-1.3-1.3V4" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>',
    search:  '<svg viewBox="0 0 16 16" fill="none"><circle cx="7" cy="7" r="4.5" stroke="currentColor" stroke-width="1.5"/><path d="M10.5 10.5L14 14" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>',
    execute: '<svg viewBox="0 0 16 16" fill="none"><path d="M2.5 3.5l4 4-4 4M8.5 12.5H13" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>',
    think:   '<svg viewBox="0 0 16 16" fill="none"><path d="M8 1.5l1.4 3.6a2 2 0 0 0 1.1 1.1l3.6 1.4-3.6 1.4a2 2 0 0 0-1.1 1.1L8 13.7 6.6 10.1a2 2 0 0 0-1.1-1.1L1.9 7.6l3.6-1.4a2 2 0 0 0 1.1-1.1L8 1.5z" stroke="currentColor" stroke-width="1.4" stroke-linejoin="round"/></svg>',
    fetch:   '<svg viewBox="0 0 16 16" fill="none"><circle cx="8" cy="8" r="6.2" stroke="currentColor" stroke-width="1.4"/><path d="M1.8 8h12.4M8 1.8c-3.2 3.4-3.2 9 0 12.4 3.2-3.4 3.2-9 0-12.4z" stroke="currentColor" stroke-width="1.4"/></svg>',
    move:    '<svg viewBox="0 0 16 16" fill="none"><path d="M3 5.5h8.5M9 2.5l3 3-3 3M13 10.5H4.5M7 13.5l-3-3 3-3" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>',
    other:   '<svg viewBox="0 0 16 16" fill="none"><circle cx="3.5" cy="8" r="1.2" fill="currentColor"/><circle cx="8" cy="8" r="1.2" fill="currentColor"/><circle cx="12.5" cy="8" r="1.2" fill="currentColor"/></svg>'
  };
  function kindIcon(kind) { return KIND_ICONS[kind] || KIND_ICONS.other; }

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
  // Declared with the session state: history paging (mountRange callers)
  // runs before the scroll helpers below are reached during load.
  let programmaticMessagesScroll = false;
  let programmaticMessagesScrollTimer = null;
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
    thinking.className = 'activity-feed';
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
    closeBtn.title = sessionCloseTitle;
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
      // A tool call started after the last text chunk — the next chunk
      // begins a new message and must open its own bubble.
      toolSinceText: false,
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
      tapRunPrompts: new Map(),
      // count of tools still running — keeps the live feed visible when a
      // tool starts after text already began streaming.
      runningTools: 0,
      activityStart: 0,
      activityTimer: null,
      // commands
      availableCommands: [],
      // queue (per-session: each session has its own pending list)
      msgQueue: [],
      drainingQueue: false,
      submitting: false,
      // input draft (per-session, isolated)
      inputDraft: '',
      inputSelectionStart: 0,
      inputSelectionEnd: 0,
      pendingImages: [],
      pendingDocs: [],
      scrollTop: 0,
      atBottom: true,
      // ── Message log ──────────────────────────────────────────────────
      // Persisted-shaped records ({role,text,ts,tools,turn_ms,a2ui}) for the
      // whole conversation. The DOM holds only the tail of this; search runs
      // over the array, so it finds messages that were never mounted.
      log: [],
      // Index of the oldest record currently mounted. Equals log.length when
      // nothing is mounted yet.
      renderedFrom: 0,
      // Set while (re)rendering from `log` so the append helpers don't push
      // duplicate records back into it.
      replaying: false,
      // Set while a composite append (a specialist message) renders its
      // parts, so only the outer call logs a record.
      logSuppress: false,
      // Log index the next mounted wrapper should carry; null = live append,
      // which lands at log.length (the index the record is about to get).
      renderIndex: null,
      // Where mountMsg inserts. Normally null (= before `thinking`, i.e. at
      // the bottom); set to the topmost mounted message while back-filling.
      insertAnchor: null,
      // "N earlier messages" sentinel while log[0] is off-screen.
      moreRow: null,
    };
  }

  // ── Windowed history ──────────────────────────────────────────────────
  // Long conversations only mount their tail; scrolling up back-fills older
  // records in chunks. The window only ever GROWS — nothing is unmounted, so
  // A2UI blocks and tool groups keep their DOM identity and their live wiring.
  // ponytail: no eviction. A session is capped at max_acp_session_messages and
  // the user has to scroll to reach the top; if that ever gets heavy, evict
  // from the bottom of the window here rather than making the mount path
  // reversible everywhere.
  const HISTORY_WINDOW = 40;   // records mounted on first paint
  const HISTORY_CHUNK  = 30;   // records added per scroll-up

  // Every message wrapper mounts through here so it carries its log index
  // (search scrolls to it) and lands at the right end of the container.
  function mountMsg(s, wrap) {
    wrap.dataset.logIndex = (s.renderIndex != null ? s.renderIndex : s.log.length);
    s.container.insertBefore(wrap, s.insertAnchor || s.thinking);
  }

  // Record a live message in the log. No-op during replay — those records are
  // already in `log`, that's what we're rendering from — and while a caller
  // that logs the whole message itself is rendering its parts.
  function logPush(s, rec) {
    if (s.replaying || s.logSuppress) return;
    s.log.push(rec);
  }

  function firstMessageNode(s) {
    for (const n of s.container.childNodes) {
      if (n.nodeType === 1 && n.dataset && n.dataset.logIndex != null) return n;
    }
    return null;
  }

  // Mount log records [from, s.renderedFrom) above whatever is already there.
  function mountRange(s, from) {
    if (from < 0) from = 0;
    if (from >= s.renderedFrom) return;
    const prevAnchor = s.insertAnchor;
    s.insertAnchor = firstMessageNode(s);
    s.replaying = true;
    for (let i = from; i < s.renderedFrom; i++) {
      s.renderIndex = i;
      renderLogRecord(s, s.log[i]);
    }
    s.renderIndex = null;
    s.replaying = false;
    s.insertAnchor = prevAnchor;
    s.renderedFrom = from;
    syncMoreRow(s);
  }

  function syncMoreRow(s) {
    const hidden = s.renderedFrom;
    if (hidden <= 0) {
      if (s.moreRow) { s.moreRow.remove(); s.moreRow = null; }
      return;
    }
    if (!s.moreRow) {
      s.moreRow = document.createElement('button');
      s.moreRow.type = 'button';
      s.moreRow.className = 'msg-more';
      s.moreRow.addEventListener('click', () => loadOlder(s));
    }
    s.moreRow.textContent = hidden + (hidden === 1 ? ' earlier message' : ' earlier messages');
    s.container.insertBefore(s.moreRow, s.container.firstChild);
  }

  // Back-fill one chunk while pinning the reading position: the container
  // grows upward, so scrollTop has to grow by the same amount.
  function loadOlder(s) {
    if (s.renderedFrom <= 0) return;
    const isActive = s.sid === activeSid;
    const before = isActive ? messagesHost.scrollHeight : 0;
    // scroll-behavior: smooth would animate this correction into a visible
    // jump, so the adjustment runs with it off.
    const prevBehavior = messagesHost.style.scrollBehavior;
    if (isActive) messagesHost.style.scrollBehavior = 'auto';
    mountRange(s, s.renderedFrom - HISTORY_CHUNK);
    if (isActive) {
      beginProgrammaticMessagesScroll();
      messagesHost.scrollTop += messagesHost.scrollHeight - before;
      messagesHost.style.scrollBehavior = prevBehavior;
    }
  }

  // Ensure `index` is mounted, back-filling as far as needed in one go.
  function ensureMounted(s, index) {
    if (index >= s.renderedFrom) return;
    const isActive = s.sid === activeSid;
    const before = isActive ? messagesHost.scrollHeight : 0;
    const prevBehavior = messagesHost.style.scrollBehavior;
    if (isActive) messagesHost.style.scrollBehavior = 'auto';
    mountRange(s, index);
    if (isActive) {
      beginProgrammaticMessagesScroll();
      messagesHost.scrollTop += messagesHost.scrollHeight - before;
      messagesHost.style.scrollBehavior = prevBehavior;
    }
  }

  function refreshAddBtn() {
    sessionAddBtn.disabled = sessions.size >= MAX_SESSIONS;
  }

  function refreshTabActiveStates() {
    let activeTab = null;
    for (const s of sessions.values()) {
      const selected = s.sid === activeSid;
      s.tab.classList.toggle('active', selected);
      s.tab.setAttribute('aria-selected', selected ? 'true' : 'false');
      if (selected) activeTab = s.tab;
    }
    if (activeTab) requestAnimationFrame(() => {
      if (activeTab.isConnected) activeTab.scrollIntoView({ inline: 'nearest', block: 'nearest' });
    });
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
      active.scrollTop = messagesHost.scrollTop;
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
    input.selectionStart = s.inputSelectionStart ?? 0;
    input.selectionEnd = s.inputSelectionEnd ?? 0;
    _ph.resetState();
    input.style.height = 'auto';
    input.style.height = Math.min(input.scrollHeight, 120) + 'px';
    // Rebind input UI to reflect this session's state
    sendBtn.classList.toggle('stop-mode', s.busy);
    sendBtn.title = s.busy ? 'Stop' : 'Send (Return)';
    renderQueue();
    renderImagePreview();
    updateSendBtn();
    updateInputLock();
    updateWelcome();
    restoreSessionScroll(s);
    // Focus always lands in the prompt input after any session switch (manual
    // tab click, Tab/Shift+Tab cycling, or Rust-driven switch after creating
    // a new session via ⌘T). Restore caret to the saved selection range.
    input.focus();
    try {
      input.selectionStart = s.inputSelectionStart ?? input.value.length;
      input.selectionEnd   = s.inputSelectionEnd   ?? input.value.length;
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

  // ── Account bar (login / quota) ───────────────────────────────────────────
  const _acctBar = document.getElementById('account-bar');
  const _acctText = document.getElementById('account-text');
  const _acctAction = document.getElementById('account-action');
  const _acctDismiss = document.getElementById('account-dismiss');
  let _acctDismissedKind = null; // stays hidden until the account state changes kind

  function _acctSet(kind, text, actionLabel) {
    _acctBar.className = kind; // single state class; also clears 'hidden'
    _acctText.textContent = text;
    if (actionLabel) {
      _acctAction.textContent = actionLabel;
      _acctAction.style.display = '';
    } else {
      _acctAction.style.display = 'none';
    }
    // Always dismissible — a dismissed bar re-shows when the account state
    // changes, so hiding one never traps the user (and background work like a
    // pending sign-in still completes and updates the chip).
    _acctDismiss.style.display = '';
  }

  function renderAccount(a) {
    if (!a || typeof a !== 'object') return;
    let kind, text, action = null;
    if (a.signed_in) {
      if (a.over_quota) {
        kind = 'over-quota';
        text = 'Out of Octomind quota' + (a.summary ? ' · ' + a.summary : '');
      } else {
        kind = 'signed-in';
        text = a.account || 'Signed in to Octomind';
      }
    } else {
      kind = 'signed-out';
      text = 'Not signed in to Octomind';
      action = 'Sign in';
    }
    // A change of state re-shows a previously dismissed bar.
    if (_acctDismissedKind && _acctDismissedKind !== kind) _acctDismissedKind = null;
    if (_acctDismissedKind === kind) { _acctBar.className = 'hidden'; return; }
    _acctSet(kind, text, action);
  }

  _acctAction.addEventListener('click', () => {
    if (_acctBar.classList.contains('signed-out')) {
      _acctSet('pending', 'Starting sign-in…', null);
      window.ipc.postMessage(JSON.stringify({ type: 'acp_signin', session_id: activeSid || 0 }));
    }
  });
  _acctDismiss.addEventListener('click', () => {
    _acctDismissedKind = _acctBar.className.split(' ')[0] || null;
    _acctBar.className = 'hidden';
  });

  // Callable from Rust — account/quota status parsed from `/usage`.
  window.__setAccount = function(sid, json) {
    let a; try { a = JSON.parse(json); } catch (e) { return; }
    renderAccount(a);
  };
  // Callable from Rust — `/login` started; the verification tab is opening.
  window.__loginPending = function(sid, code) {
    _acctDismissedKind = null;
    _acctSet('pending', code ? ('Waiting for browser… code ' + code) : 'Waiting for browser…', null);
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
  // Agent tags Rust scanned out of the registered octomind taps: [tag, title].
  let agents = [];
  window.__setAgents = function(list) {
    agents = (list || []).map(a => ({ tag: a[0], title: a[1] || a[0] }));
  };

  const SC_SUGGEST_MAX = 8;
  let scMatches = [];
  let scIdx = 0;

  function scHideSuggest() {
    scSuggest.classList.remove('visible');
    scTag.setAttribute('aria-expanded', 'false');
    scTag.removeAttribute('aria-activedescendant');
    scMatches = [];
    scIdx = 0;
  }

  function scShowSuggest() {
    const q = scTag.value.trim();
    scMatches = q
      ? fuzzysort.go(q, agents, { keys: ['tag', 'title'], limit: SC_SUGGEST_MAX }).map(r => r.obj)
      : agents.slice(0, SC_SUGGEST_MAX);
    if (!scMatches.length) { scHideSuggest(); return; }
    scIdx = 0;
    scSuggest.innerHTML = '';
    scMatches.forEach((a, i) => {
      const div = document.createElement('div');
      div.className = 'cmd-item' + (i === scIdx ? ' active' : '');
      div.id = 'sc-agent-option-' + i;
      div.setAttribute('role', 'option');
      div.setAttribute('aria-selected', i === scIdx ? 'true' : 'false');
      const name = document.createElement('div');
      name.className = 'cmd-name';
      name.textContent = a.title;
      const desc = document.createElement('div');
      desc.className = 'cmd-desc';
      desc.textContent = a.tag;
      div.appendChild(name);
      div.appendChild(desc);
      // mousedown, not click — click fires after blur has already closed us.
      div.addEventListener('mousedown', (e) => { e.preventDefault(); scPick(i); });
      div.addEventListener('mouseenter', () => { scIdx = i; scPaintActive(); });
      scSuggest.appendChild(div);
    });
    scSuggest.classList.add('visible');
    scTag.setAttribute('aria-expanded', 'true');
    scTag.setAttribute('aria-activedescendant', 'sc-agent-option-' + scIdx);
  }

  function scPaintActive() {
    const items = scSuggest.querySelectorAll('.cmd-item');
    items.forEach((el, i) => {
      const selected = i === scIdx;
      el.classList.toggle('active', selected);
      el.setAttribute('aria-selected', selected ? 'true' : 'false');
    });
    scTag.setAttribute('aria-activedescendant', 'sc-agent-option-' + scIdx);
    if (items[scIdx]) items[scIdx].scrollIntoView({ block: 'nearest' });
  }

  function scMove(delta) {
    scIdx = Math.max(0, Math.min(scMatches.length - 1, scIdx + delta));
    scPaintActive();
  }

  function scPick(i) {
    if (!scMatches[i]) return;
    const picked = scMatches[i];
    scTag.value = picked.title;
    scTag.dataset.agentTag = picked.tag;
    scTagMeta.textContent = picked.tag;
    scHideSuggest();
    scTag.focus();
  }

  scTag.addEventListener('input', () => {
    delete scTag.dataset.agentTag;
    const typed = scTag.value.trim();
    const exact = agents.find(a => a.tag === typed || a.title.toLowerCase() === typed.toLowerCase());
    if (exact) {
      scTag.value = exact.title;
      scTag.dataset.agentTag = exact.tag;
      scTagMeta.textContent = exact.tag;
    } else {
      scTagMeta.textContent = '';
    }
    scShowSuggest();
  });
  scTag.addEventListener('focus', scShowSuggest);
  scTag.addEventListener('blur', () => setTimeout(scHideSuggest, 150));

  function openCreatePanel() {
    if (sessions.size >= MAX_SESSIONS) return;
    scPanel.classList.add('visible');
    scTitle.value = '';
    const preferred = agents.find(a => a.tag === 'octoweb:assistant')
      || { tag: 'octoweb:assistant', title: 'Assistant' };
    scTag.value = preferred.title;
    scTag.dataset.agentTag = preferred.tag;
    scTagMeta.textContent = preferred.tag;
    scTitle.focus();
  }
  window.__openCreatePanel = openCreatePanel;
  sessionAddBtn.addEventListener('click', openCreatePanel);
  function hideCreatePanel(restoreFocus) {
    scPanel.classList.remove('visible');
    scHideSuggest();
    if (restoreFocus) sessionAddBtn.focus({ preventScroll: true });
  }
  scCancel.addEventListener('click', () => hideCreatePanel(true));
  function submitCreate() {
    const title = scTitle.value.trim() || 'Session';
    const typed = scTag.value.trim();
    const known = agents.find(a => a.tag === typed || a.title === typed);
    const tag   = scTag.dataset.agentTag || (known && known.tag) || typed;
    if (!tag) { scTag.focus(); return; }
    window.ipc.postMessage(JSON.stringify({ type: 'acp_session_create', title, tag }));
    hideCreatePanel(false);
  }
  scCreate.addEventListener('click', submitCreate);
  [scTitle, scTag].forEach(el => {
    el.addEventListener('keydown', (e) => {
      // While the tag suggestions are open they own the arrows, Enter and Escape.
      if (el === scTag && scSuggest.classList.contains('visible')) {
        if (e.key === 'ArrowDown')                     { e.preventDefault(); scMove(1);  return; }
        if (e.key === 'ArrowUp')                       { e.preventDefault(); scMove(-1); return; }
        if (e.key === 'Enter' || e.key === 'Tab')      { e.preventDefault(); scPick(scIdx); return; }
        if (e.key === 'Escape')                        { e.preventDefault(); e.stopPropagation(); scHideSuggest(); return; }
      }
      if (e.key === 'Enter') { e.preventDefault(); submitCreate(); }
    });
  });
  scPanel.addEventListener('keydown', (e) => {
    if (e.key !== 'Escape') return;
    e.preventDefault();
    hideCreatePanel(true);
  });

  // ── Prompt history (shared module) ──────────────────────────────────────
  // MUST be initialized before bootstrap — switchTo() references _ph.
  /* PROMPT_HISTORY_JS */
  const ghostEl = document.getElementById('prompt-ghost');
  const _ph = createPromptHistory(input, ghostEl, 'Ask Octopus', function() {
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
    active.thinking.className = 'activity-feed';
    active.thinking.innerHTML = '';
    active.currentAgentBubble = null;
    active.currentAgentRaw = '';
    active.isThinking = false;
    active.runningTools = 0;
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
    s.thinking.className = 'activity-feed';
    s.thinking.innerHTML = '';
    s.currentAgentBubble = null;
    s.currentAgentRaw = '';
    s.availableCommands = [];
    s.log = [];
    s.renderedFrom = 0;
    s.moreRow = null; // was just removed with the rest of the container
    if (csOpen) runChatSearch();
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
  // Two fence problems marked.js can't handle on its own:
  // 1. Streaming: an opened ``` fence whose closing ``` hasn't arrived yet
  //    renders as broken HTML — we append the missing closers.
  // 2. Nesting: a ``` block whose body itself quotes markdown that contains
  //    its own ``` fences (agent showing a draft). CommonMark closes the
  //    outer block at the first inner bare ```, leaking the rest as rendered
  //    markdown. We find each block's true extent, then give it more
  //    backticks than any fence inside it so marked keeps the inner markers
  //    literal.
  function normalizeFences(text) {
    const lines = text.split('\n');
    const fences = [];  // every fence line: {line, ticks, info}
    for (let i = 0; i < lines.length; i++) {
      const m = lines[i].match(/^[ \t]*(`{3,})(.*)/);
      if (m) fences.push({ line: i, ticks: m[1].length, info: m[2].trim() });
    }
    const isMd = s => /^(markdown|md|mdx)$/i.test(s);
    // The hard case: a bare ``` inside an open block is ambiguous — it may
    // close the block, or open a bare sub-block of quoted markdown. The only
    // signal that a block quotes markdown is a lang-tagged child (```bash,
    // ```toml) or a ```markdown opener. So: a PLAIN block (no such signal)
    // closes at its first bare ``` — CommonMark, keeps sibling blocks apart.
    // A QUOTING block instead runs to its farthest balanced ``` so its inner
    // fenced blocks stay literal.
    // ponytail: "quoting" then a fenced sibling after it would get swallowed
    // by "farthest balanced" — doesn't occur in agent output (the quote block
    // is always last), so not handled. Revisit only if that shape shows up.
    const used = new Array(fences.length).fill(false);
    const pairs = [];  // {open, close} line numbers
    for (let r = 0; r < fences.length; r++) {
      if (used[r]) continue;
      const T = fences[r].ticks;
      let depth = 0, quoting = isMd(fences[r].info), closeIdx = -1;
      for (let j = r + 1; j < fences.length; j++) {
        // Shorter marker = literal content per CommonMark — ignore.
        if (fences[j].ticks < T) continue;
        if (depth === 0 && !fences[j].info) {
          closeIdx = j;
          if (!quoting) break;  // plain block: first bare closer wins
        }
        if (fences[j].info) { depth++; quoting = true; }
        else depth = depth > 0 ? depth - 1 : depth + 1;
      }
      let close;
      if (closeIdx < 0) {  // streaming: still open — append a closer
        close = lines.length;
        lines.push('`'.repeat(T));
      } else {
        close = fences[closeIdx].line;
        for (let k = r; k <= closeIdx; k++) used[k] = true;
      }
      pairs.push({ open: fences[r].line, close: close });
    }
    // Each block must out-tick every fence nested inside it so marked keeps
    // the inner markers literal.
    for (const p of pairs) {
      let maxInner = 0;
      for (const f of fences) {
        if (f.line > p.open && f.line < p.close) maxInner = Math.max(maxInner, f.ticks);
      }
      const orig = lines[p.open].match(/`{3,}/)[0].length;
      const need = Math.max(orig, maxInner + 1);
      if (need > orig) {
        const fence = '`'.repeat(need);
        lines[p.open] = lines[p.open].replace(/`{3,}/, fence);
        if (p.close < lines.length) lines[p.close] = lines[p.close].replace(/`{3,}/, fence);
      }
    }
    return lines.join('\n');
  }

  function renderMd(raw) {
    if (typeof marked === 'undefined') return escapeHtml(raw);
    return marked.parse(normalizeFences(raw));
  }

  function escapeHtml(s) {
    return String(s == null ? '' : s)
      .replace(/&/g,'&amp;')
      .replace(/</g,'&lt;')
      .replace(/>/g,'&gt;')
      .replace(/"/g,'&quot;')
      .replace(/'/g,'&#39;');
  }
  function escapeAttr(s) { return escapeHtml(s); }

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
    if (role === 'user') {
      // dataset.raw, not bubble.textContent — doc chips appended later
      // would otherwise leak their filenames into the copied text.
      wrap.dataset.raw = text;
      bubble.appendChild(makeCopyBtn(wrap));
    }
    mountMsg(s, wrap);
    logPush(s, { role: role, text: text, ts: Date.now() });
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
    var s = key.replace(/_/g, ' ');
    return s.charAt(0).toUpperCase() + s.slice(1);
  }

  function renderCommandValue(val) {
    if (val === null || val === undefined) return '<span class="null">none</span>';
    if (typeof val === 'boolean') return '<span class="bool-' + val + '">' + val + '</span>';
    if (typeof val === 'number') return '<span class="number">' + val + '</span>';
    if (typeof val === 'string') {
      var escaped = escapeHtml(val);
      return val.indexOf('\n') >= 0 ? '<pre class="cmd-output-pre">' + escaped + '</pre>' : escaped;
    }
    if (Array.isArray(val)) {
      if (val.length === 0) return '<span class="null">none</span>';
      if (val.every(function(v) { return typeof v === 'string' || typeof v === 'number'; })) {
        var ul = '<ul class="cmd-output-list">';
        for (var i = 0; i < val.length; i++) ul += '<li>' + escapeHtml(String(val[i])) + '</li>';
        return ul + '</ul>';
      }
      var records = '<div class="cmd-records">';
      for (var j = 0; j < val.length; j++) {
        records += '<div class="cmd-record">' + renderCommandValue(val[j]) + '</div>';
      }
      return records + '</div>';
    }
    if (typeof val === 'object') {
      return '<div class="cmd-output-nested">' + renderCommandTable(val) + '</div>';
    }
    return escapeHtml(String(val));
  }

  function renderCommandTable(obj) {
    if (!obj || Object.keys(obj).length === 0) return '<span class="null">none</span>';
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
  function fmtSeconds(seconds) {
    var v = Math.max(0, Number(seconds) || 0);
    if (v < 60) return Math.floor(v) + 's';
    if (v < 3600) return Math.floor(v / 60) + 'm ' + Math.floor(v % 60) + 's';
    if (v < 86400) return Math.floor(v / 3600) + 'h ' + Math.floor((v % 3600) / 60) + 'm';
    return Math.floor(v / 86400) + 'd ' + Math.floor((v % 86400) / 3600) + 'h';
  }
  function fmtAgo(seconds) {
    if (seconds === null || seconds === undefined) return '';
    return fmtSeconds(seconds) + ' ago';
  }
  function statusKind(status) {
    var s = String(status || '').toLowerCase();
    if (s === 'done' || s === 'completed' || s === 'running' || s === 'active' || s === 'ok') return 'ok';
    if (s === 'failed' || s === 'error' || s === 'dead' || s === 'unreachable') return 'err';
    if (s === 'cancelled' || s === 'canceled' || s === 'inactive' || s === 'off') return 'muted';
    return 'warn';
  }
  function statusBadge(status) {
    var label = String(status || 'unknown');
    return '<span class="cmd-badge ' + statusKind(label) + '">' + escapeHtml(label) + '</span>';
  }
  function messageBlock(message, isError) {
    var text = String(message || '');
    if (!text) return emptyState(isError ? 'Command failed' : 'No output');
    var cls = 'cmd-output-pre' + (isError ? ' cmd-error-text' : '');
    return '<pre class="' + cls + '">' + escapeHtml(text) + '</pre>';
  }
  function agentFacts(a) {
    var facts = [];
    if (a.model) facts.push(String(a.model));
    if (a.tokens_input != null || a.tokens_output != null) {
      facts.push(fmtTokens(a.tokens_input) + ' in');
      facts.push(fmtTokens(a.tokens_output) + ' out');
    }
    if (Number(a.tokens_cached) > 0) facts.push(fmtTokens(a.tokens_cached) + ' cached');
    if (a.cost != null) facts.push(fmtCost(a.cost));
    if (a.tool_calls != null) facts.push(String(a.tool_calls) + ' tools');
    return facts;
  }
  function agentRow(a, running) {
    a = a || {};
    var status = String(a.status || (running ? 'running' : 'unknown'));
    var when = running ? fmtSeconds(a.elapsed_secs) : fmtAgo(a.ago_secs);
    var html = '<div class="cmd-item-row stack cmd-agent-row"><div class="cmd-item-head">' +
      statusBadge(status) +
      '<span class="cmd-item-name">' + escapeHtml(String(a.role || 'agent')) + '</span>' +
      (when ? '<span class="cmd-item-meta">' + escapeHtml(when) + '</span>' : '') +
      '</div>';
    if (a.id) html += '<div class="cmd-agent-id">' + escapeHtml(String(a.id)) + '</div>';
    var facts = agentFacts(a);
    if (facts.length) {
      html += '<div class="cmd-agent-facts">';
      for (var i = 0; i < facts.length; i++) html += '<span>' + escapeHtml(facts[i]) + '</span>';
      html += '</div>';
    }
    if (a.workdir) html += '<div class="cmd-agent-path">' + escapeHtml(String(a.workdir)) + '</div>';
    if (a.last_action) html += '<div class="cmd-agent-action">' + escapeHtml(String(a.last_action)) + '</div>';
    return html + '</div>';
  }
  function quotaRow(label, used, cap, suffix) {
    var spent = Math.max(0, Number(used) || 0);
    var limit = Math.max(0, Number(cap) || 0);
    var pct = limit > 0 ? Math.min(100, spent / limit * 100) : 0;
    var cls = pct >= 100 ? ' err' : (pct >= 80 ? ' warn' : '');
    var value = suffix === 'GB'
      ? spent.toFixed(2) + ' / ' + limit.toFixed(2) + ' GB'
      : fmtCost(spent) + ' / ' + fmtCost(limit);
    return '<div class="cmd-quota"><div class="cmd-quota-head">' +
      '<span class="cmd-quota-name">' + escapeHtml(String(label)) + '</span>' +
      '<span class="cmd-quota-value">' + escapeHtml(value) + '</span></div>' +
      '<div class="cmd-quota-track"><span class="cmd-quota-fill' + cls + '" style="width:' + pct.toFixed(1) + '%"></span></div></div>';
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
      '<span class="cmd-toast-icon">' + (isErr ? ICON_X_CIRCLE : ICON_CHECK_CIRCLE) + '</span>' +
      '<span>' + escapeHtml(String(msg)) + '</span>' +
    '</div>';
  }
  function emptyState(msg) {
    return '<div class="cmd-empty">' + escapeHtml(msg) + '</div>';
  }

  var CMD_RENDERERS = {
    model: function(o) {
      var html = switchCard('Model', o.old_model, o.new_model, !!o.changed, 'M');
      if (o.save_error) html += '<div class="cmd-section-title">Warning</div>' + messageBlock(o.save_error, true);
      return html;
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
      if (o.save_error) html += '<div class="cmd-section-title">Warning</div>' + messageBlock(o.save_error, true);
      return html;
    },
    effort: function(o) {
      var html = switchCard('Effort', o.old_effort, o.new_effort, !!o.changed, 'E');
      if (o.save_error) html += '<div class="cmd-section-title">Warning</div>' + messageBlock(o.save_error, true);
      return html;
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
      var totalTokens = Number(o.tokens_used || 0) + Number(o.tokens_cached || 0) +
        Number(o.tokens_cache_write || 0) + Number(o.tokens_reasoning || 0);
      var stats = '<div class="cmd-stats">';
      stats += statTile('Model', o.model || '—', { cls: 'accent' });
      stats += statTile('Role', o.role || '—');
      stats += statTile('Total tokens', fmtTokens(totalTokens), { cls: 'accent' });
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
      if (o.cache_non_cached_tokens != null) meta += statTile('Non-cached', fmtTokens(o.cache_non_cached_tokens));
      meta += '</div>';
      if (o.timing && (o.timing.requests || o.timing.completed_turns)) {
        meta += '<div class="cmd-section-title">Timing</div><div class="cmd-stats">';
        meta += statTile('Requests', o.timing.requests || 0);
        meta += statTile('Avg request', fmtMs(o.timing.avg_request_time_ms));
        meta += statTile('Turns', o.timing.completed_turns || 0);
        meta += statTile('Avg turn', fmtMs(o.timing.avg_turn_time_ms));
        meta += '</div>';
      }
      var averages = [];
      if (o.avg_tokens_per_compression) averages.push(['Per compression', fmtTokens(o.avg_tokens_per_compression)]);
      if (o.avg_tokens_per_tool) averages.push(['Per tool', fmtTokens(o.avg_tokens_per_tool)]);
      if (o.avg_tokens_per_response) averages.push(['Per response', fmtTokens(o.avg_tokens_per_response)]);
      if (o.avg_input_tokens) averages.push(['Input/request', fmtTokens(o.avg_input_tokens)]);
      if (averages.length) {
        meta += '<div class="cmd-section-title">Averages</div><div class="cmd-stats">';
        for (var ai = 0; ai < averages.length; ai++) meta += statTile(averages[ai][0], averages[ai][1]);
        meta += '</div>';
      }
      var detailSections = [
        ['Compression', o.compression_stats],
        ['Agents', o.agents_stats],
        ['Supervisor', o.supervisor_stats],
        ['Learning', o.learning_stats]
      ];
      for (var ds = 0; ds < detailSections.length; ds++) {
        var detail = detailSections[ds][1];
        if (detail && typeof detail === 'object' && Object.keys(detail).length) {
          meta += '<div class="cmd-section-title">' + detailSections[ds][0] + '</div>' + renderCommandTable(detail);
        }
      }
      return stats + meta;
    },
    copy: function(o) {
      if (!o.copied) return toast('Nothing to copy', true);
      return toast('Copied ' + (o.length || 0) + ' chars to clipboard');
    },
    clear: function(o) {
      return toast(o.message || (o.success ? 'Conversation cleared' : 'Unable to clear'), !o.success);
    },
    done: function(o) {
      if (!o.done) return toast('Task was not finalized', true);
      var html = toast('Task finalized');
      var states = [
        ['Memory', o.memorized],
        ['Summary', o.summarized],
        ['Session', o.saved]
      ];
      html += '<div class="cmd-section-title">Completion</div><div class="cmd-chips">';
      for (var i = 0; i < states.length; i++) {
        if (states[i][1] == null) continue;
        html += '<span class="cmd-chip' + (states[i][1] ? ' active' : '') + '">' +
          escapeHtml(states[i][0]) + ' ' + (states[i][1] ? '✓' : '—') + '</span>';
      }
      return html + '</div>';
    },
    image: function(o) {
      if (o.error) return toast(o.error, true);
      return toast(o.image_attached ? 'Image attached: ' + (o.path || '') : 'No image attached', !o.image_attached);
    },
    video: function(o) {
      if (o.error) return toast(o.error, true);
      return toast(o.video_attached ? 'Video attached: ' + (o.path || '') : 'No video attached', !o.video_attached);
    },
    rename: function(o) {
      return toast(o.title ? 'Renamed to ' + o.title : 'Session title cleared');
    },
    schedule: function(o) {
      var d = o.data || {};
      var msg = String(d.message || '');
      if (d.is_error || d.subcommand === 'error') return toast(msg || 'Schedule command failed', true);
      if (!msg && d.subcommand === 'help') {
        return messageBlock('/schedule [list|add|remove|edit] [id] [when=...] [message=...]', false);
      }
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
      var html = '';
      if (md) {
        html = '<div class="cmd-md">' + (typeof marked !== 'undefined' ? marked.parse(md) : escapeHtml(md)) + '</div>';
      } else {
        var sessions = Array.isArray(o.sessions) ? o.sessions : [];
        html = '<div class="cmd-stats">' +
          statTile('Sessions', o.total_sessions != null ? o.total_sessions : sessions.length, { cls: 'accent' }) +
          statTile('Page', (o.page || 1) + ' / ' + (o.total_pages || 1)) + '</div>';
        if (!sessions.length) return html + emptyState('No sessions found');
        html += '<div class="cmd-items">';
        for (var si = 0; si < sessions.length; si++) {
          var se = sessions[si] || {};
          html += '<div class="cmd-item-row stack"><div class="cmd-item-head">' +
            '<span class="cmd-item-name">' + escapeHtml(String(se.name || '?')) + '</span>' +
            (se.is_current ? '<span class="cmd-badge ok">current</span>' : '') +
            '</div><div class="cmd-agent-facts">' +
            '<span>' + escapeHtml(String(se.created || '')) + '</span>' +
            '<span>' + escapeHtml(String(se.model || '')) + '</span>' +
            '<span>' + escapeHtml(fmtTokens(se.tokens)) + '</span>' +
            '<span>' + escapeHtml(fmtCost(se.cost)) + '</span></div>' +
            (se.title ? '<div class="cmd-item-desc">' + escapeHtml(String(se.title)) + '</div>' : '') + '</div>';
        }
        html += '</div>';
      }
      return html;
    },
    run: function(o) {
      var d = o.data || {};
      if (o.command_executed) {
        if (d.success === false) {
          var failed = toast(d.error || ('Command failed: ' + o.command_executed), true);
          if (Array.isArray(d.available_commands) && d.available_commands.length) {
            failed += '<div class="cmd-section-title">Available commands</div>' + renderCommandValue(d.available_commands);
          }
          return failed;
        }
        var ran = toast('Ran command: ' + o.command_executed);
        if (d.result != null) ran += '<div class="cmd-section-title">Result</div>' + renderCommandValue(d.result);
        return ran;
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
      if (d.message && (!d.servers || (Array.isArray(d.servers) && !d.servers.length))) {
        return messageBlock(d.message, d.subcommand === 'error' || d.is_error);
      }
      var html = '';

      // `/mcp list` groups tool names under server keys.
      if (d.servers && !Array.isArray(d.servers) && typeof d.servers === 'object') {
        var names = Object.keys(d.servers).sort();
        html += '<div class="cmd-stats">' +
          statTile('Servers', names.length, { cls: 'accent' }) +
          statTile('Tools', d.total_tools != null ? d.total_tools : 0) + '</div>';
        if (!names.length) return html + emptyState('No MCP servers configured');
        html += '<div class="cmd-section-title">Servers</div><div class="cmd-items">';
        for (var n = 0; n < names.length; n++) {
          var groupedTools = Array.isArray(d.servers[names[n]]) ? d.servers[names[n]] : [];
          html += '<div class="cmd-item-row stack"><div class="cmd-item-head">' +
            '<span class="cmd-item-name">' + escapeHtml(names[n]) + '</span>' +
            '<span class="cmd-item-meta">' + groupedTools.length + ' tools</span></div>';
          if (groupedTools.length) {
            html += '<div class="cmd-tools-inline">';
            for (var gt = 0; gt < groupedTools.length; gt++) {
              html += '<span class="cmd-tool-tag">' + escapeHtml(String(groupedTools[gt])) + '</span>';
            }
            html += '</div>';
          }
          html += '</div>';
        }
        return html + '</div>';
      }

      // `/mcp info|full|health` returns one structured record per server.
      var srv = Array.isArray(d.servers) ? d.servers : [];
      if (!srv.length) {
        var rest = {};
        for (var dk in d) if (dk !== 'subcommand') rest[dk] = d[dk];
        return Object.keys(rest).length ? renderCommandTable(rest) : emptyState('No MCP servers configured');
      }
      html += '<div class="cmd-stats">' +
        statTile('Servers', srv.length, { cls: 'accent' }) +
        statTile('Tools', d.total_tools != null ? d.total_tools : 0) + '</div>';
      html += '<div class="cmd-section-title">Servers</div>';
      html += '<div class="cmd-items">';
      for (var i = 0; i < srv.length; i++) {
        var s = srv[i];
        var tools = Array.isArray(s.tools) ? s.tools : [];
        var meta = [];
        if (s.connection_type) meta.push(s.connection_type);
        if (s.restart_count) meta.push(s.restart_count + ' restarts');
        if (s.consecutive_failures) meta.push(s.consecutive_failures + ' failures');
        html += '<div class="cmd-item-row stack">' +
          '<div class="cmd-item-head">' +
            '<span class="cmd-item-name">' + escapeHtml(s.name || '?') + '</span>' +
            statusBadge(s.health || 'unknown') +
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
        var empty = '<div class="cmd-empty">' + escapeHtml(o.display || 'No active plan') + '</div>';
        if (Array.isArray(o.knowledge) && o.knowledge.length) {
          empty += '<div class="cmd-section-title">Knowledge</div>' + renderCommandValue(o.knowledge);
        }
        return empty;
      }
      var planText = typeof o.display === 'string' ? o.display : '';
      var html = planText
        ? '<div class="cmd-md">' + (typeof marked !== 'undefined' ? marked.parse(planText) : escapeHtml(planText)) + '</div>'
        : renderCommandValue(o.plan);
      if (Array.isArray(o.knowledge) && o.knowledge.length) {
        html += '<div class="cmd-section-title">Knowledge</div>' + renderCommandValue(o.knowledge);
      }
      return html;
    },
    prompt: function(o) {
      var d = o.data || {};
      if (d.action === 'execute') {
        if (d.success === false) {
          var failed = toast(d.error || 'Prompt template failed', true);
          if (Array.isArray(d.available_prompts) && d.available_prompts.length) {
            failed += '<div class="cmd-section-title">Available prompts</div>' + renderCommandValue(d.available_prompts);
          }
          return failed;
        }
        var executed = toast('Started prompt: ' + (d.prompt_name || 'template'));
        if (d.prompt_content) executed += '<div class="cmd-section-title">Prompt</div>' + messageBlock(d.prompt_content, false);
        return executed;
      }
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
      if (d.subcommand === 'error') return toast(d.message || 'Skill command failed', true);
      if (d.subcommand === 'use') return toast('Enabled skill: ' + (d.name || ''));
      if (d.subcommand === 'forget') return toast('Disabled skill: ' + (d.name || ''));
      var skills = Array.isArray(d.skills) ? d.skills : [];
      var html = '<div class="cmd-stats">';
      html += statTile('Total', d.total != null ? d.total : skills.length, { cls: 'accent' });
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
    monitor: function(o) {
      var d = o.data || {};
      var failed = d.is_error || d.subcommand === 'error';
      return messageBlock(d.message || (failed ? 'Monitor command failed' : 'No running monitors'), failed);
    },
    learning: function(o) {
      var d = o.data || {};
      if (d.subcommand === 'error') return toast(d.message || 'Learning command failed', true);
      if (d.subcommand === 'list' || Array.isArray(d.lessons)) {
        var lessons = Array.isArray(d.lessons) ? d.lessons : [];
        var html = '<div class="cmd-stats">' +
          statTile('Lessons', d.total != null ? d.total : lessons.length, { cls: 'accent' }) +
          statTile('Role', d.role || '—') +
          statTile('Project', d.project || '—') +
          statTile('Page', (d.page || 1) + ' / ' + (d.total_pages || 0)) + '</div>';
        if (!lessons.length) return html + emptyState('No lessons stored');
        html += '<div class="cmd-section-title">Lessons</div><div class="cmd-items">';
        for (var i = 0; i < lessons.length; i++) {
          var l = lessons[i] || {};
          var title = l.title || l.content || ('Lesson ' + (l.index || i + 1));
          var preview = String(l.content || '');
          if (title === l.content) preview = '';
          if (preview.length > 240) preview = preview.slice(0, 240) + '…';
          html += '<div class="cmd-item-row stack"><div class="cmd-item-head">' +
            '<span class="cmd-item-name">' + escapeHtml(String(l.index || i + 1)) + '</span>' +
            (l.memory_type ? '<span class="cmd-badge info">' + escapeHtml(String(l.memory_type)) + '</span>' : '') +
            (l.scope ? '<span class="cmd-item-meta">' + escapeHtml(String(l.scope)) + '</span>' : '') +
            (l.outcome ? statusBadge(l.outcome) : '') + '</div>' +
            '<div class="cmd-item-desc">' + escapeHtml(String(title)) + '</div>' +
            (preview ? '<div class="cmd-item-desc">' + escapeHtml(preview) + '</div>' : '');
          if (Array.isArray(l.tags) && l.tags.length) {
            html += '<div class="cmd-tools-inline">';
            for (var j = 0; j < l.tags.length; j++) html += '<span class="cmd-tool-tag">' + escapeHtml(String(l.tags[j])) + '</span>';
            html += '</div>';
          }
          html += '</div>';
        }
        return html + '</div>';
      }
      if (d.subcommand === 'show') {
        var shown = '<div class="cmd-section-title">' + escapeHtml(String(d.title || d.id || 'Lesson')) + '</div>' +
          messageBlock(d.content || '', false);
        var details = {};
        for (var key in d) if (key !== 'subcommand' && key !== 'title' && key !== 'content') details[key] = d[key];
        return shown + '<div class="cmd-section-title">Details</div>' + renderCommandTable(details);
      }
      if (d.message) return messageBlock(d.message, false);
      var learningRest = {};
      for (var lk in d) if (lk !== 'subcommand') learningRest[lk] = d[lk];
      return renderCommandTable(learningRest);
    },
    agents: function(o) {
      if (o.detail) {
        var detail = o.detail;
        var html = agentRow(detail, String(detail.status) === 'running');
        if (!detail.last_action) html += emptyState('No activity yet');
        return html;
      }
      var running = Array.isArray(o.running) ? o.running : [];
      var finished = Array.isArray(o.finished) ? o.finished : [];
      var html = '<div class="cmd-stats">' +
        statTile('Total', o.total != null ? o.total : running.length + finished.length, { cls: 'accent' }) +
        statTile('Running', running.length, running.length ? { cls: 'success' } : {}) +
        statTile('Finished', finished.length) + '</div>';
      if (!running.length && !finished.length) return html + emptyState('No agents offloaded in this session');
      if (running.length) {
        html += '<div class="cmd-section-title">Running</div><div class="cmd-items">';
        for (var i = 0; i < running.length; i++) html += agentRow(running[i], true);
        html += '</div>';
      }
      if (finished.length) {
        html += '<div class="cmd-section-title">Recent</div><div class="cmd-items">';
        for (var j = 0; j < finished.length; j++) html += agentRow(finished[j], false);
        html += '</div>';
      }
      return html;
    },
    usage: function(o) {
      if (!o.signed_in) return emptyState('Sign in to view account usage');
      var html = '<div class="cmd-stats">' +
        statTile('Account', o.account || 'Signed in', { cls: 'accent' }) +
        statTile('Balance', fmtCost(o.balance_usd), { cls: 'success' }) + '</div>';
      var windows = Array.isArray(o.windows) ? o.windows : [];
      if (windows.length) {
        html += '<div class="cmd-section-title">Spend limits</div><div class="cmd-quotas">';
        for (var i = 0; i < windows.length; i++) {
          var w = windows[i] || {};
          var committed = Number(w.spent_usd || 0) + Number(w.reserved_usd || 0);
          html += quotaRow(w.label || 'Window', committed, w.cap_usd, 'USD');
        }
        html += '</div>';
      }
      if (o.storage_quota_gb != null || o.network_included_gb != null) {
        html += '<div class="cmd-section-title">Cloud resources</div><div class="cmd-quotas">';
        if (o.storage_quota_gb != null) html += quotaRow('Storage', o.storage_gb, o.storage_quota_gb, 'GB');
        if (o.network_included_gb != null) html += quotaRow('Network', o.network_used_gb, o.network_included_gb, 'GB');
        html += '</div>';
      }
      return html;
    },
    login: function(o) {
      if (o.already_signed_in) return toast('Already signed in' + (o.account ? ' as ' + o.account : ''));
      var html = toast('Sign-in started');
      if (o.user_code) html += '<div class="cmd-section-title">Verification code</div><span class="cmd-pill accent">' + escapeHtml(String(o.user_code)) + '</span>';
      if (o.verification_url) html += '<div class="cmd-section-title">Open in browser</div><div class="cmd-agent-path">' + escapeHtml(String(o.verification_url)) + '</div>';
      return html;
    },
    share: function(o) {
      var html = toast('Session shared');
      if (o.url) html += '<div class="cmd-section-title">Share URL</div><div class="cmd-agent-path">' + escapeHtml(String(o.url)) + '</div>';
      if (o.id) html += '<div class="cmd-stat-sub">ID ' + escapeHtml(String(o.id)) + '</div>';
      return html;
    },
    analyze: function(o) {
      var html = toast('Session viewer ready');
      if (o.url) html += '<div class="cmd-section-title">Local viewer</div><div class="cmd-agent-path">' + escapeHtml(String(o.url)) + '</div>';
      if (o.port != null) html += '<div class="cmd-stat-sub">Port ' + escapeHtml(String(o.port)) + '</div>';
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
          var name = e.user_request || e.tool || e.name || e.command || ('Request ' + (i + 1));
          name = String(name).replace(/\s+/g, ' ').trim();
          if (name.length > 180) name = name.slice(0, 180) + '…';
          var meta = [];
          var calls = e.tool_calls != null ? e.tool_calls : e.calls;
          if (calls != null) meta.push(calls + ' tools');
          if (e.cost != null) meta.push(typeof e.cost === 'string' ? e.cost : fmtCost(e.cost));
          if (e.task_time) meta.push('task ' + e.task_time);
          if (e.ai_time) meta.push('AI ' + e.ai_time);
          if (e.processing_time) meta.push('processing ' + e.processing_time);
          if (e.ai_time_ms != null) meta.push('AI ' + fmtMs(e.ai_time_ms));
          html += '<div class="cmd-item-row stack">' +
            '<div class="cmd-item-head"><span class="cmd-item-name">' + (i + 1) + '</span>' +
            '<span class="cmd-item-meta">' + escapeHtml(meta.join(' · ')) + '</span></div>' +
            '<div class="cmd-item-desc">' + escapeHtml(name) + '</div>';
          if (Array.isArray(e.tools_used) && e.tools_used.length) {
            html += '<div class="cmd-tools-inline">';
            for (var j = 0; j < e.tools_used.length; j++) html += '<span class="cmd-tool-tag">' + escapeHtml(String(e.tools_used[j])) + '</span>';
            html += '</div>';
          }
          html += '</div>';
        }
        html += '</div>';
      } else {
        html += emptyState('No requests recorded yet');
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
    error: function(o) {
      var html = toast(o.error || 'Command failed', true);
      if (o.context && typeof o.context === 'object' && Object.keys(o.context).length) {
        html += '<div class="cmd-section-title">Details</div>' + renderCommandTable(o.context);
      }
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

  // Called once Done arrives — stores raw, mounts the steps group above the
  // bubble, appends copy btn, collapses if tall.
  function finishAgentBubble(s, bubble, rawText, toolCount, details, turnMs) {
    const wrap = bubble.closest('.msg');
    if (!wrap) return;
    wrap.dataset.raw = rawText;
    // Before the early returns below — a tool-only turn is still a searchable
    // record, and `startAgentBubble` already stamped the wrap with this index.
    logPush(s, { role: 'agent', text: rawText, ts: Date.now(), tools: details || [], turn_ms: turnMs || 0 });

    var cmdObj = tryParseCommandJson(rawText);
    if (cmdObj) {
      bubble.innerHTML = renderCommandOutput(cmdObj);
    }

    if (toolCount > 0) {
      wrap.insertBefore(buildToolGroup(details, turnMs), bubble);
    }

    // Tool-only turn — the steps group is the record; no empty bubble.
    if (!rawText.trim() && bubble.childElementCount === 0) {
      bubble.style.display = 'none';
      return;
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

  // whoOverride labels a specialist bubble (appendSpecialistMsg); live main
  // agent bubbles omit it and are always labelled Octopus.
  function startAgentBubble(s, whoOverride) {
    if (!s) return;
    const wrap   = document.createElement('div');
    wrap.className = 'msg agent' + (whoOverride ? ' specialist' : '');
    const label  = document.createElement('div');
    label.className = 'msg-label';
    const who = document.createElement('span');
    who.className = 'msg-who';
    who.textContent = whoOverride || 'Octopus';
    const time = document.createElement('span');
    time.className = 'msg-time';
    time.textContent = fmtTime(new Date());
    label.appendChild(who);
    label.appendChild(time);
    const bubble = document.createElement('div');
    bubble.className = 'msg-bubble';
    wrap.appendChild(label);
    wrap.appendChild(bubble);
    mountMsg(s, wrap);
    if (s.sid === activeSid) scrollToBottom();
    return bubble;
  }

  // Open the live agent bubble if this turn doesn't have one yet. Shared by
  // text chunks and inline images — first real output drops the thinking
  // state but keeps the feed rows alive (a tool starting mid-stream re-shows
  // them via syncFeed). Do NOT touch `s.busy` — the agent runs until Done.
  function ensureAgentBubble(s) {
    if (s.currentAgentBubble) return;
    s.currentAgentBubble = startAgentBubble(s);
    s.currentAgentRaw = '';
    s.isThinking = false;
    syncFeed(s);
  }

  window.__appendChunk = function(sid, text) {
    const s = sessions.get(sid);
    if (!s) return;
    // Text never resumes after a tool call within one LLM message — a chunk
    // arriving after tools is a new message, not more of the previous one.
    if (s.toolSinceText && s.currentAgentBubble) {
      finishLiveTurn(s);
      s.activityStart = Date.now();
    }
    s.toolSinceText = false;
    ensureAgentBubble(s);
    s.currentAgentRaw += text;
    var cmdObj = tryParseCommandJson(s.currentAgentRaw);
    s.currentAgentBubble.innerHTML = cmdObj ? renderCommandOutput(cmdObj) : renderMd(s.currentAgentRaw);
    if (s.sid === activeSid) scrollToBottom();
  };

  // Injected message from the agent runtime — a specialist (tap-run) reply,
  // schedule or webhook payload. Text arrives as "[<source label>] <body>";
  // the bracket tag becomes the sender label (tap-run labels collapse to the
  // bare role, e.g. "doctor:blood").
  function appendSpecialistMsg(s, text) {
    // One record for the whole message, with the role Rust persists. The
    // inner render paths (report card, or a plain agent bubble) would
    // otherwise log nothing, or log it as `agent`.
    const wasSuppressed = s.logSuppress;
    s.logSuppress = true;
    try {
      appendSpecialistMsgInner(s, text);
    } finally {
      s.logSuppress = wasSuppressed;
    }
    logPush(s, { role: 'specialist', text: text, ts: Date.now() });
  }

  function appendSpecialistMsgInner(s, text) {
    let who = 'Specialist', body = text;
    const m = text.match(/^\[([^\]\n]+)\]\s*/);
    if (m) { who = m[1]; body = text.slice(m[0].length); }
    let report = body.match(/^\[Tap-run '([^'\n]+)' \(([^)\n]+)\) (completed|cancelled|failed)\]\s*/);
    if (report) {
      body = body.slice(report[0].length);
    } else {
      report = who.match(/^Tap-run '([^'\n]+)' \(([^)\n]+)\) (completed|cancelled|failed)$/);
    }
    if (report) {
      who = report[2];
    } else if (/^tap-run \S+ \(.+\)$/.test(who)) {
      // Tap-run envelope but no handback report body: tap-runs only inject
      // completion reports, so this is a main-agent reply that old builds
      // persisted under the specialist's label. Render it as Octopus.
      const bubble = startAgentBubble(s);
      if (!bubble) return who;
      bubble.innerHTML = renderMd(body);
      finishAgentBubble(s, bubble, body, 0, [], 0);
      if (s.sid === activeSid) scrollToBottom();
      return who;
    }
    if (report) {
      let preview = '';
      for (const line of body.split(/\r?\n/)) {
        if (line.trim()) { preview = line.trim(); break; }
      }
      const wrap = document.createElement('div');
      wrap.className = 'msg agent specialist specialist-report';
      const details = document.createElement('details');
      details.className = 'specialist-report-details' + (report[3] === 'failed' ? ' failed' : '');
      const summary = document.createElement('summary');
      summary.className = 'specialist-report-summary';
      summary.innerHTML =
        '<span class="specialist-report-chevron"><svg width="8" height="8" viewBox="0 0 8 8" fill="none"><path d="M2.5 1l3 3-3 3" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg></span>' +
        '<span class="specialist-report-who">' + escapeHtml(who) + '</span>' +
        '<span class="specialist-report-title">report · <span class="specialist-report-status">' + report[3] + '</span></span>' +
        (preview ? '<span class="specialist-report-preview">' + escapeHtml(preview) + '</span>' : '') +
        '<span class="specialist-report-time">' + escapeHtml(fmtTime(new Date())) + '</span>';
      const content = document.createElement('div');
      content.className = 'msg-bubble specialist-report-body';
      if (s.tapRunPrompts.has(report[1])) {
        const input = document.createElement('details');
        input.className = 'specialist-report-input';
        const inputSummary = document.createElement('summary');
        inputSummary.textContent = 'Input';
        const inputText = document.createElement('div');
        inputText.className = 'specialist-report-input-text';
        inputText.innerHTML = escapeHtml(s.tapRunPrompts.get(report[1]));
        input.appendChild(inputSummary);
        input.appendChild(inputText);
        content.appendChild(input);
      }
      const reportBody = document.createElement('div');
      reportBody.className = 'specialist-report-content';
      reportBody.innerHTML = renderMd(body);
      content.appendChild(reportBody);
      details.appendChild(summary);
      details.appendChild(content);
      wrap.appendChild(details);
      mountMsg(s, wrap);
      if (s.sid === activeSid) scrollToBottom();
      return who;
    }
    const bubble = startAgentBubble(s, who);
    if (!bubble) return who;
    bubble.innerHTML = renderMd(body);
    finishAgentBubble(s, bubble, body, 0, [], 0);
    if (s.sid === activeSid) scrollToBottom();
    return who;
  }

  window.__appendSpecialist = function(sid, text) {
    const s = sessions.get(sid);
    if (!s) return;
    // The injection itself is a turn boundary. Finalize the previous bubble
    // directly instead of faking a terminal Done: __setThinking(false) also
    // drains the user queue and can race the autonomous response that follows.
    finishLiveTurn(s);
    s.isThinking = false;
    syncFeed(s);
    // The injected message keeps its specialist label, but the autonomous
    // response it triggers belongs to the main agent and must remain Octopus.
    appendSpecialistMsg(s, text);
  };

  window.__appendImage = function(sid, mimeType, b64data) {
    const s = sessions.get(sid);
    if (!s) return;
    ensureAgentBubble(s);
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

  // Final durations: sub-10s show one decimal ("0.4s"), longer ones round.
  function fmtDuration(ms) {
    if (ms < 9950) return (Math.max(ms, 100) / 1000).toFixed(1) + 's';
    return fmtElapsed(ms);
  }

  // Feed is visible while the agent hasn't produced output yet (thinking) or
  // any tool is still running — including tools started mid-stream.
  function syncFeed(s) {
    s.thinking.classList.toggle('visible', s.isThinking || s.runningTools > 0);
  }

  function clearActivity(s) {
    if (s.activityTimer) { clearInterval(s.activityTimer); s.activityTimer = null; }
    s.thinking.innerHTML = '';
    for (const k in s.toolRows) delete s.toolRows[k];
    s.runningTools = 0;
  }

  function toolDataObject(value) {
    if (value && typeof value === 'object' && !Array.isArray(value)) return value;
    if (typeof value !== 'string') return null;
    try {
      const parsed = JSON.parse(value);
      return parsed && typeof parsed === 'object' && !Array.isArray(parsed) ? parsed : null;
    } catch (e) {
      return null;
    }
  }

  function registerTapRunPrompt(s, title, rawInput, rawOutput) {
    if (!s || title !== 'tap') return;
    const input = toolDataObject(rawInput);
    if (!input || input.action !== 'run' || typeof input.prompt !== 'string') return;
    let runId = typeof input.session === 'string' ? input.session.trim() : '';
    if (!runId) {
      const output = toolDataObject(rawOutput);
      runId = output && typeof output.id === 'string' ? output.id.trim() : '';
    }
    if (runId) s.tapRunPrompts.set(runId, input.prompt);
  }

  function formatJson(val) {
    if (val === null || val === undefined) return null;
    try { return JSON.stringify(val, null, 2); } catch(e) { return String(val); }
  }

  function renderToolDetail(t) {
    let html = '';
    if (t.locations && t.locations.length > 0) {
      html += '<div class="tool-detail-title">Locations</div>';
      for (const l of t.locations) html += '<div class="tool-loc">' + escapeHtml(l) + '</div>';
    }
    const inputJson = formatJson(t.rawInput);
    if (inputJson) html += '<div class="tool-detail-title">Input</div><pre>' + escapeHtml(inputJson) + '</pre>';
    const outputJson = formatJson(t.rawOutput);
    if (outputJson) html += '<div class="tool-detail-title">Output</div><pre>' + escapeHtml(outputJson) + '</pre>';
    return html || '<div class="tool-detail-title">No details</div>';
  }

  // Toggle the inline detail under a tool row. Re-renders on every open so
  // late-arriving output on live rows is always current.
  function toggleToolDetail(item, detail, t) {
    const open = item.classList.toggle('expanded');
    if (open) detail.innerHTML = renderToolDetail(t);
  }

  // ── Steps group — persistent, collapsed record of a turn's tool work ───
  function buildToolGroup(details, turnMs) {
    const group = document.createElement('div');
    group.className = 'tool-group';
    const fails = details.filter(t => t.status === 'failed').length;
    const header = document.createElement('div');
    header.className = 'tool-group-header';
    header.innerHTML =
      '<span class="tool-group-chevron"><svg width="8" height="8" viewBox="0 0 8 8" fill="none"><path d="M2.5 1l3 3-3 3" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg></span>' +
      '<span class="tool-group-count">' + details.length + (details.length === 1 ? ' step' : ' steps') + '</span>' +
      (fails ? '<span class="tool-group-fail">' + fails + ' failed</span>' : '') +
      (turnMs ? '<span class="tool-group-time">' + fmtDuration(turnMs) + '</span>' : '');
    const list = document.createElement('div');
    list.className = 'tool-group-list';
    header.addEventListener('click', () => {
      const open = group.classList.toggle('expanded');
      if (open && !list.childElementCount) {
        for (const t of details) list.appendChild(buildGroupItem(t));
      }
    });
    group.appendChild(header);
    group.appendChild(list);
    return group;
  }

  function buildGroupItem(t) {
    const failed = t.status === 'failed';
    const completed = t.status === 'completed';
    const item = document.createElement('div');
    item.className = 'tool-item';
    const row = document.createElement('div');
    row.className = 'tool-row ' + (failed ? 'failed' : 'done');
    row.innerHTML =
      '<span class="tool-kind ' + (KIND_ICONS[t.kind] ? t.kind : 'other') + '">' + kindIcon(t.kind) + '</span>' +
      '<span class="tool-title">' + escapeHtml(t.title) + '</span>' +
      // A tool still 'running' at turn end was interrupted — no duration, no mark.
      '<span class="tool-time">' + (completed || failed ? fmtDuration(t.duration || 0) : '—') + '</span>' +
      (failed ? '<span class="tool-fail">' + ICON_X_CIRCLE + '</span>'
              : completed ? '<span class="tool-check">' + ICON_CHECK + '</span>' : '');
    const detail = document.createElement('div');
    detail.className = 'tool-detail';
    row.addEventListener('click', () => toggleToolDetail(item, detail, t));
    item.appendChild(row);
    item.appendChild(detail);
    return item;
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

  // `startedMsAgo` is only passed when Rust rebuilds a turn already in
  // progress (workspace switch-back) so the row's timer resumes, not restarts.
  window.__toolStart = function(sid, id, title, kind, rawInput, locations, startedMsAgo) {
    const s = sessions.get(sid);
    if (!s) return;
    registerTapRunPrompt(s, title, rawInput, null);
    s.toolSinceText = true;
    if (isRenderUiTool(title)) {
      s.suppressedToolIds = s.suppressedToolIds || new Set();
      s.suppressedToolIds.add(id);
      return;
    }
    s.toolCount++;
    const d = { id, kind, title, toolName: title, status: 'running', duration: 0, rawInput, locations, rawOutput: null };
    s.toolDetails.push(d);
    const item = document.createElement('div');
    item.className = 'tool-item';
    const row = document.createElement('div');
    row.className = 'tool-row running';
    const icon = document.createElement('span');
    icon.className = 'tool-kind ' + (KIND_ICONS[kind] ? kind : 'other');
    icon.innerHTML = kindIcon(kind);
    const ttl = document.createElement('span');
    ttl.className = 'tool-title';
    ttl.textContent = title;
    const tm = document.createElement('span');
    tm.className = 'tool-time';
    tm.textContent = fmtElapsed(startedMsAgo || 0);
    row.appendChild(icon);
    row.appendChild(ttl);
    row.appendChild(tm);
    const detail = document.createElement('div');
    detail.className = 'tool-detail';
    row.addEventListener('click', () => toggleToolDetail(item, detail, d));
    item.appendChild(row);
    item.appendChild(detail);
    s.thinking.appendChild(item);
    s.toolRows[id] = { el: row, item, detail, startTime: Date.now() - (startedMsAgo || 0), timerEl: tm, finished: false, idx: s.toolDetails.length - 1 };
    s.runningTools++;
    syncFeed(s);
    if (sid === activeSid) scrollToBottom();
  };

  window.__toolUpdate = function(sid, id, title, status, rawOutput) {
    const s = sessions.get(sid);
    if (!s) return;
    if (s.suppressedToolIds && s.suppressedToolIds.has(id)) return;
    const t = s.toolRows[id];
    if (!t) return;
    const d = s.toolDetails[t.idx];
    if (title) {
      t.el.querySelector('.tool-title').textContent = title;
      d.title = title;
    }
    if (rawOutput !== undefined && rawOutput !== null) {
      d.rawOutput = rawOutput;
    }
    registerTapRunPrompt(s, d.toolName, d.rawInput, d.rawOutput);
    if ((status === 'completed' || status === 'failed') && !t.finished) {
      t.finished = true;
      s.runningTools = Math.max(0, s.runningTools - 1);
      t.el.classList.remove('running');
      d.status = status;
      d.duration = Date.now() - t.startTime;
      t.timerEl.textContent = fmtDuration(d.duration);
      const mark = document.createElement('span');
      if (status === 'completed') {
        t.el.classList.add('done');
        mark.className = 'tool-check';
        mark.innerHTML = ICON_CHECK;
      } else {
        t.el.classList.add('failed');
        mark.className = 'tool-fail';
        mark.innerHTML = ICON_X_CIRCLE;
      }
      t.el.appendChild(mark);
      syncFeed(s);
    }
    // Inline detail open on a live row — refresh so output lands as it arrives.
    if (t.item.classList.contains('expanded')) {
      t.detail.innerHTML = renderToolDetail(d);
    }
  };

  // Finalize and reset one live response without changing queue/busy state.
  // Injections use this as a causal boundary; terminal prompt events use the
  // same path before they release the queue.
  function finishLiveTurn(s) {
    const savedToolCount = s.toolCount;
    const savedToolDetails = [...s.toolDetails];
    const turnMs = s.activityStart ? Date.now() - s.activityStart : 0;
    clearActivity(s);
    // Synthesize a bubble for tool-only turns — finishAgentBubble keeps the
    // steps group as the record and hides the empty bubble itself.
    if (!s.currentAgentBubble && savedToolCount > 0) {
      s.currentAgentBubble = startAgentBubble(s);
      s.currentAgentRaw = '';
    }
    if (s.currentAgentBubble) {
      finishAgentBubble(s, s.currentAgentBubble, s.currentAgentRaw, savedToolCount, savedToolDetails, turnMs);
    }
    s.currentAgentBubble = null;
    s.currentAgentRaw = '';
    s.toolCount = 0;
    s.toolDetails = [];
    s.activityStart = 0;
  }

  // `elapsedMs` is only passed when Rust rebuilds a turn already in progress
  // (workspace switch-back): the timer resumes from the real start instead
  // of 0s, and `busy` is restored since no dispatch path set it.
  window.__setThinking = function(sid, on, elapsedMs) {
    const s = sessions.get(sid);
    if (!s) return;
    s.isThinking = on;
    if (sid === activeSid) {
      sendBtn.classList.toggle('stop-mode', on);
      sendBtn.title = on ? 'Stop' : 'Send (Return)';
    }
    if (on) {
      // A real prompt is also the terminal boundary for an autonomous inbox
      // response. Finalize its bubble before resetting live-turn state; the
      // old behavior dropped the reference and left it without copy/steps.
      finishLiveTurn(s);
      if (elapsedMs !== undefined) s.busy = true;
      s.activityStart = Date.now() - (elapsedMs || 0);
      const hdr = document.createElement('div');
      hdr.className = 'activity-header';
      hdr.innerHTML = '<span class="activity-dots"><span></span><span></span><span></span></span>' +
        '<span class="activity-label">Working…</span>' +
        '<span class="activity-elapsed">' + fmtElapsed(elapsedMs || 0) + '</span>';
      s.thinking.appendChild(hdr);
      s.activityTimer = setInterval(() => tickActivity(s), 1000);
      syncFeed(s);
      if (sid === activeSid) scrollToBottom();
    } else {
      // Terminal event for this prompt (Done/Cancelled/Error path via main.rs).
      // Clear the busy flag so queued messages can drain and new prompts go
      // straight to dispatch instead of being queued.
      s.busy = false;
      finishLiveTurn(s);
      syncFeed(s);
      // A2UI: a click optimistically locks its card into "Processing…" and
      // waits for the agent's next envelope to lift it. Turn end (Done /
      // Cancelled / Error) means no envelope is coming — unlock the cards.
      // Clearing pollFileId only drops the hint about which envelope to
      // complete; a later click still reaches Rust, which decides whether a
      // render_ui call is waiting.
      for (const block of a2uiBlocks.values()) {
        if (block.sid !== sid) continue;
        block.pollFileId = null;
        if (block.resolved) {
          block.resolved = false;
          a2uiRerender(block);
        }
      }
    }
  };

  window.__appendError = function(sid, text) {
    const s = sessions.get(sid);
    if (!s) return;
    window.__setThinking(sid, false);
    appendMessage(s, 'error', text);
  };

  // A user turn Rust synthesized — an A2UI click that had no `render_ui` call
  // waiting on it, so the choice reaches the agent as a prompt. Typed prompts
  // render their own bubble in dispatchPromptForSession; this is the only path
  // where Rust owns it. The click handler already set busy/thinking.
  window.__appendUserMsg = function(sid, text) {
    const s = sessions.get(sid);
    if (!s) return;
    appendMessage(s, 'user', text);
  };

  // Replay persisted messages on cold-start. Rust calls this once per session
  // on sidebar bootstrap with the full message log restored from disk.
  // Entries: { role: 'user'|'agent'|'specialist'|'error'|'ui', text: string, ts?: number,
  //            a2ui?: <envelope-body>, tools?: [tool records], turn_ms?: number }.
  // Agent turns rebuild their steps group from `tools`; inline images are
  // live-only and skipped.
  // Render one persisted record into `s.container`. Shared by first paint and
  // by scroll-up back-fill, so both produce identical DOM.
  function renderLogRecord(s, m) {
    const role = m && m.role;
    const text = (m && typeof m.text === 'string') ? m.text : '';
    if (role === 'user' || role === 'error') {
      appendMessage(s, role, text);
    } else if (role === 'specialist') {
      appendSpecialistMsg(s, text);
    } else if (role === 'agent') {
      const bubble = startAgentBubble(s);
      if (!bubble) return;
      const cmdObj = tryParseCommandJson(text);
      bubble.innerHTML = cmdObj ? renderCommandOutput(cmdObj) : renderMd(text);
      // Persisted snake_case tool records → live detail shape.
      const tools = Array.isArray(m.tools) ? m.tools.map(t => ({
        kind: t.kind,
        title: t.title || '',
        status: t.status,
        duration: t.duration_ms || 0,
        rawInput: t.raw_input != null ? t.raw_input : null,
        locations: Array.isArray(t.locations) ? t.locations : [],
        rawOutput: t.raw_output != null ? t.raw_output : null
      })) : [];
      for (const t of tools) registerTapRunPrompt(s, t.title, t.rawInput, t.rawOutput);
      finishAgentBubble(s, bubble, text, tools.length, tools, m.turn_ms || 0);
    } else if (role === 'ui' && m && m.a2ui) {
      // Rebuild the A2UI bubble from the persisted envelope body. Replays are
      // ghosts (nothing is blocked on them) and anchor on the persisted
      // creation ts so the bubble keeps its original chronological spot and
      // time label rather than showing "now".
      window.__a2uiUpdate(s.sid, text, m.a2ui, false, typeof m.ts === 'number' ? m.ts : 0);
    }
  }

  // Tear a session's rendered history back down to empty. `__addSession` is a
  // no-op for a session that already exists, so a second replay (sidebar
  // reload, re-entering a workspace) would otherwise stack a duplicate copy
  // of the whole conversation on top of the first.
  function resetSessionDom(s) {
    for (const entry of Array.from(a2uiBlocks)) {
      const fid = entry[0], b = entry[1];
      if (b.sid !== s.sid) continue;
      a2uiBlocks.delete(fid);
      a2uiBubbleByFile.delete(fid);
      if (b.surfaceId != null) a2uiSurfaceIndex.delete(a2uiSurfaceKey(s.sid, b.surfaceId));
    }
    while (s.container.firstChild) s.container.removeChild(s.container.firstChild);
    s.container.appendChild(s.thinking);
    s.moreRow = null;
    s.currentAgentBubble = null;
    s.currentAgentRaw = '';
  }

  window.__replayMessages = function(sid, msgs) {
    const s = sessions.get(sid);
    if (!s || !Array.isArray(msgs) || msgs.length === 0) return;
    if (firstMessageNode(s)) resetSessionDom(s);
    s.log = msgs.slice();
    // Nothing mounted yet, so the window starts empty at the end and
    // mountRange back-fills the tail.
    s.renderedFrom = s.log.length;
    mountRange(s, s.log.length - HISTORY_WINDOW);
    if (sid === activeSid) { updateWelcome(); scrollToBottom(); }
  };

  function isMessagesAtBottom() {
    return messagesHost.scrollHeight - messagesHost.scrollTop - messagesHost.clientHeight <= 24;
  }

  function finishProgrammaticMessagesScroll() {
    programmaticMessagesScroll = false;
    if (programmaticMessagesScrollTimer) clearTimeout(programmaticMessagesScrollTimer);
    programmaticMessagesScrollTimer = null;
  }

  function beginProgrammaticMessagesScroll() {
    programmaticMessagesScroll = true;
    if (programmaticMessagesScrollTimer) clearTimeout(programmaticMessagesScrollTimer);
    // `scrollend` is not available in every WKWebView version. Keep the flag
    // through the smooth animation and clear it shortly after its last event.
    programmaticMessagesScrollTimer = setTimeout(finishProgrammaticMessagesScroll, 180);
  }

  function noteProgrammaticMessagesScrollEvent() {
    if (!programmaticMessagesScroll) return;
    if (programmaticMessagesScrollTimer) clearTimeout(programmaticMessagesScrollTimer);
    programmaticMessagesScrollTimer = setTimeout(finishProgrammaticMessagesScroll, 180);
  }

  function noteUserMessagesScroll() {
    finishProgrammaticMessagesScroll();
  }

  messagesHost.addEventListener('wheel', noteUserMessagesScroll, { passive: true });
  messagesHost.addEventListener('touchstart', noteUserMessagesScroll, { passive: true });
  messagesHost.addEventListener('touchmove', noteUserMessagesScroll, { passive: true });
  messagesHost.addEventListener('pointerdown', noteUserMessagesScroll, { passive: true });
  document.addEventListener('keydown', e => {
    if (e.ctrlKey || e.metaKey || e.altKey) return;
    const target = e.target;
    if (target && ((target.matches && target.matches('input, textarea, select')) || target.isContentEditable)) return;
    if (['ArrowUp', 'ArrowDown', 'PageUp', 'PageDown', 'Home', 'End', ' '].indexOf(e.key) >= 0) noteUserMessagesScroll();
  }, true);
  messagesHost.addEventListener('scrollend', finishProgrammaticMessagesScroll, { passive: true });

  function restoreSessionScroll(s) {
    const prevBehavior = messagesHost.style.scrollBehavior;
    messagesHost.style.scrollBehavior = 'auto';
    beginProgrammaticMessagesScroll();
    messagesHost.scrollTop = s.atBottom ? messagesHost.scrollHeight : s.scrollTop;
    s.scrollTop = messagesHost.scrollTop;
    messagesHost.style.scrollBehavior = prevBehavior;
  }

  function scrollToBottom(force) {
    // Back-filling older history mounts messages above the viewport; jumping
    // to the bottom would throw the user out of what they scrolled up to read.
    if (active && active.replaying) return;
    // DOM growth changes scrollHeight before this helper runs. `atBottom`
    // records where the reader was before that growth, so streaming output
    // follows only when they were already within 24px of the bottom.
    if (!force && active && !active.atBottom) return;
    beginProgrammaticMessagesScroll();
    messagesHost.scrollTop = messagesHost.scrollHeight;
    if (active) {
      active.scrollTop = messagesHost.scrollTop;
      active.atBottom = true;
    }
  }

  // ⌃D / ⌃U / ⌃T / ⌃B from the native shell while the assistant has key.
  window.__scrollMessages = function(where) {
    // Ctrl+D / Ctrl+U are the user scrolling by hand, so they deliberately skip
    // beginProgrammaticMessagesScroll(): the scroll listener below must see
    // them and update atBottom / back-fill history exactly as a wheel would.
    if (where === 'down' || where === 'up') {
      // A wheel clears programmaticMessagesScroll via noteUserMessagesScroll;
      // this path never reaches that listener (Rust consumes the chord and
      // calls straight in), so without it the flag stays latched, atBottom is
      // never recomputed, and the next streamed chunk yanks the view back down.
      noteUserMessagesScroll();
      if (where === 'up' && active) active.atBottom = false;
      const delta = (where === 'down' ? 1 : -1) * messagesHost.clientHeight;
      messagesHost.scrollBy({ top: delta, behavior: 'smooth' });
      return;
    }
    const top = where === 'top';
    beginProgrammaticMessagesScroll();
    messagesHost.scrollTo({ top: top ? 0 : messagesHost.scrollHeight, behavior: 'smooth' });
    if (active) active.atBottom = !top;
  };

  // Scrolling to the top back-fills the next chunk of history.
  messagesHost.addEventListener('scroll', () => {
    if (!active) return;
    active.scrollTop = messagesHost.scrollTop;
    if (programmaticMessagesScroll) noteProgrammaticMessagesScrollEvent();
    else active.atBottom = isMessagesAtBottom();
    if (active.renderedFrom <= 0) return;
    if (messagesHost.scrollTop > 240) return;
    loadOlder(active);
  }, { passive: true });

  // ── Chat search (⌘F) ──────────────────────────────────────────────────
  // Runs over each session's `log`, not the DOM, so it finds messages the
  // window hasn't mounted. Jumping to a hit back-fills far enough to show it.
  const chatSearchEl = document.getElementById('chat-search');
  const csInput   = document.getElementById('cs-input');
  const csCount   = document.getElementById('cs-count');
  const csResults = document.getElementById('cs-results');
  const CS_MAX_HITS = 100;
  let csOpen = false;
  let csHits = [];
  let csIdx = -1;
  let csTimer = null;

  const CS_ROLE_LABEL = {
    user: 'You', agent: 'Octopus', error: 'Error', specialist: 'Specialist', ui: 'Surface',
  };

  // What a record contributes to the index. A2UI records carry their content
  // in components rather than `text` (which is just the call id).
  function logRecordText(m) {
    if (!m) return '';
    if (m.role === 'ui') {
      const out = [];
      const envelopes = (m.a2ui && Array.isArray(m.a2ui.messages)) ? m.a2ui.messages : [];
      for (const env of envelopes) {
        const comps = env && env.updateComponents && env.updateComponents.components;
        if (!Array.isArray(comps)) continue;
        for (const c of comps) {
          for (const k of ['text', 'label', 'placeholder', 'description']) {
            if (c && typeof c[k] === 'string') out.push(c[k]);
          }
        }
      }
      return out.join(' ');
    }
    return typeof m.text === 'string' ? m.text : '';
  }

  function runChatSearch() {
    const q = csInput.value.trim();
    csHits = [];
    csIdx = -1;
    csResults.innerHTML = '';
    if (q.length < 2) {
      csCount.textContent = '';
      return;
    }
    const needle = q.toLowerCase();
    // Current chat first — that's what ⌘F usually means — then the others in
    // tab order.
    const order = [];
    if (active) order.push(active);
    for (const s of sessions.values()) if (s !== active) order.push(s);
    outer:
    for (const s of order) {
      for (let i = 0; i < s.log.length; i++) {
        const text = logRecordText(s.log[i]);
        if (!text) continue;
        const at = text.toLowerCase().indexOf(needle);
        if (at < 0) continue;
        csHits.push({
          sid: s.sid, index: i, role: s.log[i].role, title: s.title,
          text: text, at: at, len: q.length,
        });
        if (csHits.length >= CS_MAX_HITS) break outer;
      }
    }
    renderChatHits();
  }

  function renderChatHits() {
    csResults.innerHTML = '';
    updateCsCount();
    if (!csHits.length) {
      if (csInput.value.trim().length >= 2) {
        const empty = document.createElement('div');
        empty.id = 'cs-empty';
        empty.textContent = 'No matches';
        csResults.appendChild(empty);
      }
      return;
    }
    csHits.forEach((h, i) => {
      const row = document.createElement('div');
      row.className = 'cs-hit';
      row.setAttribute('role', 'option');
      const meta = document.createElement('div');
      meta.className = 'cs-hit-meta';
      const chat = document.createElement('span');
      chat.className = 'cs-hit-chat';
      chat.textContent = h.title;
      const who = document.createElement('span');
      who.textContent = CS_ROLE_LABEL[h.role] || h.role || '';
      meta.appendChild(chat);
      meta.appendChild(who);
      row.appendChild(meta);
      row.appendChild(hitSnippet(h));
      row.addEventListener('mousedown', (e) => { e.preventDefault(); jumpToHit(i); });
      csResults.appendChild(row);
    });
  }

  // Text nodes around a <mark>, never innerHTML — the haystack is user and
  // agent content.
  function hitSnippet(h) {
    const el = document.createElement('div');
    el.className = 'cs-hit-snippet';
    const start = Math.max(0, h.at - 48);
    const stop = Math.min(h.text.length, h.at + h.len + 90);
    el.appendChild(document.createTextNode((start > 0 ? '…' : '') + h.text.slice(start, h.at)));
    const mk = document.createElement('mark');
    mk.textContent = h.text.slice(h.at, h.at + h.len);
    el.appendChild(mk);
    el.appendChild(document.createTextNode(h.text.slice(h.at + h.len, stop) + (stop < h.text.length ? '…' : '')));
    return el;
  }

  function updateCsCount() {
    if (!csHits.length) {
      csCount.textContent = csInput.value.trim().length >= 2 ? '0' : '';
      return;
    }
    csCount.textContent = (csIdx >= 0 ? (csIdx + 1) + '/' : '') + csHits.length;
  }

  function updateCsActive() {
    const rows = csResults.querySelectorAll('.cs-hit');
    rows.forEach((r, i) => r.classList.toggle('active', i === csIdx));
    if (rows[csIdx]) rows[csIdx].scrollIntoView({ block: 'nearest' });
    updateCsCount();
  }

  function jumpToHit(i) {
    if (!csHits.length) return;
    csIdx = ((i % csHits.length) + csHits.length) % csHits.length;
    const h = csHits[csIdx];
    updateCsActive();
    if (h.sid !== activeSid) {
      // Swap locally so the scroll below has a mounted container, and tell
      // Rust so its own idea of the active session follows. Its echoed
      // __switchSession is then a no-op.
      switchTo(h.sid);
      window.ipc.postMessage(JSON.stringify({ type: 'acp_session_switch', session_id: h.sid }));
    }
    const s = sessions.get(h.sid);
    if (!s) return;
    ensureMounted(s, h.index);
    scrollToLogIndex(s, h.index);
  }

  function scrollToLogIndex(s, index) {
    let target = null;
    for (const n of s.container.querySelectorAll('[data-log-index]')) {
      if (Number(n.dataset.logIndex) >= index) { target = n; break; }
    }
    if (!target) return;
    beginProgrammaticMessagesScroll();
    target.scrollIntoView({ block: 'center' });
    target.classList.remove('chat-hit');
    void target.offsetWidth; // restart the flash animation on a repeat jump
    target.classList.add('chat-hit');
    setTimeout(() => target.classList.remove('chat-hit'), 1800);
  }

  // ⌘F toggles, matching the page find bar. Pressing it while the field is
  // open but unfocused re-focuses instead of closing.
  window.__openChatSearch = function() {
    if (csOpen && document.activeElement === csInput) { closeChatSearch(); return; }
    chatSearchEl.classList.remove('hidden');
    csOpen = true;
    csInput.focus();
    csInput.select();
    if (csInput.value.trim()) runChatSearch();
  };

  function closeChatSearch() {
    chatSearchEl.classList.add('hidden');
    csOpen = false;
    csHits = [];
    csIdx = -1;
    csResults.innerHTML = '';
    csCount.textContent = '';
    input.focus();
  }

  csInput.addEventListener('input', () => {
    clearTimeout(csTimer);
    csTimer = setTimeout(runChatSearch, 120);
  });
  csInput.addEventListener('keydown', (e) => {
    if (e.key === 'Escape') { e.preventDefault(); closeChatSearch(); return; }
    if (e.key === 'Enter')  { e.preventDefault(); jumpToHit(csIdx + (e.shiftKey ? -1 : 1)); return; }
    if (e.key === 'ArrowDown') { e.preventDefault(); jumpToHit(csIdx + 1); return; }
    if (e.key === 'ArrowUp')   { e.preventDefault(); jumpToHit(csIdx - 1); return; }
  });
  document.getElementById('cs-next').addEventListener('click', () => jumpToHit(csIdx + 1));
  document.getElementById('cs-prev').addEventListener('click', () => jumpToHit(csIdx - 1));
  document.getElementById('cs-close').addEventListener('click', closeChatSearch);

  // ── Queue (per-session, max 2 pending) ─────────────────────────────────
  function renderQueue() {
    if (!active) { queueList.innerHTML = ''; return; }
    const s = active;
    queueList.innerHTML = '';
    s.msgQueue.forEach((entry, i) => {
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
      rm.setAttribute('aria-label', 'Remove queued message');
      rm.innerHTML = '<svg width="8" height="8" viewBox="0 0 8 8" fill="none"><path d="M1 1L7 7M7 1L1 7" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>';
      rm.addEventListener('click', () => {
        s.msgQueue.splice(i, 1);
        if (active === s) {
          renderQueue();
          updateInputLock();
        }
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
    if (!s || s.busy || s.submitting || s.drainingQueue || s.msgQueue.length === 0) return;
    s.drainingQueue = true;
    try {
      // Re-check inside the serialized section: a send or A2UI action may have
      // claimed the session after the delayed drain was scheduled.
      if (s.busy || s.submitting || s.msgQueue.length === 0) return;
      const next = s.msgQueue.shift();
      if (active === s) {
        renderQueue();
        updateInputLock();
      }
      if (!dispatchPromptForSession(s, next.text, next.images, next.docs)) {
        s.msgQueue.unshift(next);
        if (active === s) {
          renderQueue();
          updateInputLock();
        }
      }
    } finally {
      s.drainingQueue = false;
    }
  }

  function dispatchPromptForSession(s, text, images, docs) {
    if (!s || s.busy) return false;
    // Claim the session before touching the DOM or calling any helper. Every
    // dispatch path observes this synchronously, so only one prompt can leave
    // a session at a time.
    s.busy = true;
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
    return true;
  }

  // ── Image attachments ──────────────────────────────────────────────────
  let docLibsLoaded = false;
  let imagePickerSid = null;
  let docPickerSid = null;

  function addImageFromFile(file, session) {
    const origin = session;
    if (!origin || !file || !file.type.startsWith('image/')) return;
    const reader = new FileReader();
    reader.onload = () => {
      if (sessions.get(origin.sid) !== origin) return;
      const b64 = reader.result.split(',')[1];
      origin.pendingImages.push({ data: b64, mimeType: file.type });
      if (active === origin) {
        renderImagePreview();
        updateSendBtn();
      }
    };
    reader.readAsDataURL(file);
  }

  function renderImagePreview() {
    imagePreview.innerHTML = '';
    if (!active) {
      imagePreview.classList.remove('visible');
      return;
    }
    const s = active;
    const pendingImages = s.pendingImages;
    const pendingDocs = s.pendingDocs;
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
      rm.type = 'button';
      rm.className = 'rm';
      rm.setAttribute('aria-label', 'Remove image attachment');
      rm.textContent = '×';
      rm.onclick = () => {
        s.pendingImages.splice(i, 1);
        if (active === s) { renderImagePreview(); updateSendBtn(); }
      };
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
      rm.type = 'button';
      rm.className = 'rm';
      rm.setAttribute('aria-label', 'Remove document attachment');
      rm.textContent = '×';
      rm.onclick = () => {
        s.pendingDocs.splice(i, 1);
        if (active === s) { renderImagePreview(); updateSendBtn(); }
      };
      chip.appendChild(rm);
      imagePreview.appendChild(chip);
    }
  }

  function updateSendBtn() {
    const hasDraft = active && (input.value.trim().length > 0
      || active.pendingImages.length > 0 || active.pendingDocs.length > 0);
    sendBtn.classList.toggle('active', !!hasDraft);
  }

  attachBtn.addEventListener('click', (e) => {
    e.stopPropagation();
    attachMenu.classList.toggle('visible');
  });
  document.addEventListener('click', () => attachMenu.classList.remove('visible'));
  attachMenu.addEventListener('click', (e) => e.stopPropagation());

  document.querySelector('.attach-option[data-type="image"]').addEventListener('click', () => {
    attachMenu.classList.remove('visible');
    imagePickerSid = active ? active.sid : null;
    fileInputImage.click();
  });
  document.querySelector('.attach-option[data-type="document"]').addEventListener('click', () => {
    attachMenu.classList.remove('visible');
    docPickerSid = active ? active.sid : null;
    fileInputDoc.click();
  });

  fileInputImage.addEventListener('change', () => {
    // Never redirect a native-picker result to whichever session happens to
    // be active now. If its originating session closed while the picker was
    // open, discard the selection instead.
    const origin = imagePickerSid == null ? null : sessions.get(imagePickerSid);
    for (const f of fileInputImage.files) addImageFromFile(f, origin);
    fileInputImage.value = '';
    imagePickerSid = null;
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
    const origin = docPickerSid == null ? null : sessions.get(docPickerSid);
    if (!origin) {
      fileInputDoc.value = '';
      docPickerSid = null;
      return;
    }
    for (const f of fileInputDoc.files) {
      origin.pendingDocs.push({ file: f, name: f.name });
    }
    fileInputDoc.value = '';
    docPickerSid = null;
    if (active === origin) {
      renderImagePreview();
      updateSendBtn();
    }
  });

  input.addEventListener('paste', e => {
    const items = e.clipboardData && e.clipboardData.items;
    if (!items) return;
    for (const item of items) {
      if (item.type.startsWith('image/')) {
        e.preventDefault();
        addImageFromFile(item.getAsFile(), active);
        return;
      }
    }
  });

  // ── Send ────────────────────────────────────────────────────────────────
  async function send() {
    if (!active) return;
    const s = active;
    if (s.submitting) return;
    const text = input.value.trim();
    const images = s.pendingImages.slice();
    const docs = s.pendingDocs.slice();
    if (!text && !images.length && !docs.length) return;
    s.submitting = true;
    s.inputDraft = '';
    s.inputSelectionStart = 0;
    s.inputSelectionEnd = 0;
    input.value = '';
    input.placeholder = 'Ask Octopus';
    input.style.height = 'auto';
    s.pendingImages = [];
    s.pendingDocs = [];
    renderImagePreview();
    sendBtn.classList.remove('active');

    let docPrefix = '';
    try {
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
      if (sessions.get(s.sid) !== s) return;
      const fullText = docPrefix + text;

      // `s` is the session that owned the draft before any extraction await.
      // A session switch cannot redirect the completed prompt.
      s.submitting = false;
      if (!s.busy) {
        dispatchPromptForSession(s, fullText, images, docs);
      } else if (s.msgQueue.length < MAX_QUEUE) {
        s.msgQueue.push({ text: fullText, images, docs });
        if (active === s) {
          renderQueue();
          updateInputLock();
        }
      } else {
        // The queue can fill while document extraction is awaiting. Put the
        // entire unsent draft back on its originating session instead of
        // dropping text or attachments.
        const newerText = active === s ? input.value : s.inputDraft;
        s.inputDraft = text + (newerText ? '\n' + newerText : '');
        s.pendingImages = images.concat(s.pendingImages);
        s.pendingDocs = docs.concat(s.pendingDocs);
        if (active === s) {
          input.value = s.inputDraft;
          input.style.height = 'auto';
          input.style.height = Math.min(input.scrollHeight, 120) + 'px';
          renderImagePreview();
          updateSendBtn();
          updateInputLock();
        }
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
    } finally {
      s.submitting = false;
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
  // Tab / Shift+Tab cycle ACP sessions from anywhere in the panel — the
  // panel is one assistant, not a form. Only editing contexts keep native
  // Tab: agent-widget fields, the rename field and the create-session form
  // (their own handlers run first and preventDefault).
  function cycleSession(backwards) {
    const keys = Array.from(sessions.keys());
    if (keys.length <= 1) return false;
    const idx = keys.indexOf(activeSid);
    const nextIdx = backwards
      ? (idx <= 0 ? keys.length - 1 : idx - 1)
      : (idx >= keys.length - 1 ? 0 : idx + 1);
    window.ipc.postMessage(JSON.stringify({ type: 'acp_session_switch', session_id: keys[nextIdx] }));
    input.focus();
    return true;
  }
  document.addEventListener('keydown', e => {
    if (e.key !== 'Tab' && e.key !== 'Escape') return;
    if (e.ctrlKey || e.metaKey || e.altKey || e.defaultPrevented) return;
    if (e.target && e.target.closest && e.target.closest('.a2ui-body, #session-create-panel, .session-rename')) return;
    // Esc stops a turn in flight. Every nested Esc handler (slash-command
    // dropdown, tag suggestions, chat search, A2UI modal, rename) runs first
    // and calls preventDefault, so this only fires when nothing else claimed
    // the key. stop-mode on the send button IS "a turn is running".
    if (e.key === 'Escape') {
      // Chat search binds Escape to its input, but the panel is designed to
      // outlive that focus (⌘F reopens it, cs-next steals focus, selecting
      // text in a bubble blurs it). Closing it must outrank cancelling a turn:
      // one is recoverable, the other destroys in-flight work.
      if (csOpen) { e.preventDefault(); closeChatSearch(); return; }
      if (sendBtn.classList.contains('stop-mode')) { e.preventDefault(); stop(); }
      return;
    }
    if (cycleSession(e.shiftKey)) e.preventDefault();
  });
  input.addEventListener('keydown', e => {
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
    if (!val) { input.placeholder = 'Ask Octopus'; }
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
  window.__setThinking = function(sid, on, elapsedMs) {
    _origSetThinking(sid, on, elapsedMs);
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
    document.getElementById('sidebar').classList.toggle('fullscreen', !!on);
    fsBtn.classList.toggle('active', !!on);
    fsEnter.style.display = on ? 'none' : '';
    fsExit.style.display  = on ? '' : 'none';
    const label = on ? 'Exit fullscreen' : 'Toggle fullscreen';
    fsBtn.title = label + (fullscreenShortcut ? ' (' + fullscreenShortcut + ')' : '');
  };

  /* A2UI_CORE_JS */

  // ── A2UI v1.0 renderer ─────────────────────────────────────────────────
  // Inline interactive surfaces produced by the agent's `render_ui` tool.
  // Each envelope becomes one `.msg.ui` bubble. Button clicks IPC
  // `a2ui_resolve` back to Rust, which hands the resolution to the render_ui
  // call blocked on it. Everything below is DOM; the evaluation core it calls
  // into (JSON-Pointer, catalog functions, ValueRefs) lives in `a2ui_js.rs`.
  const a2uiBlocks = new Map();           // fileId  -> block state
  const a2uiBubbleByFile = new Map();     // fileId  -> wrapper element
  const a2uiSurfaceIndex = new Map();     // "sid:surfaceId" -> fileId of live block
  function a2uiSurfaceKey(sid, surfaceId) { return sid + ':' + surfaceId; }
  // Peek the surfaceId from any message in the envelope — every surface-scoped
  // message kind stamps it. Used to honor "same surfaceId = update existing
  // surface" even when a follow-up envelope only carries components or data
  // updates (no createSurface).
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

  function a2uiApplyMessages(block, messages) {
    for (const msg of messages || []) {
      if (msg.createSurface) {
        const s = msg.createSurface;
        if (s.surfaceId != null) block.surfaceId = s.surfaceId;
        if (s.catalogId != null) block.catalogId = s.catalogId;
        // v1.0 gates the data model behind `sendDataModel`. octoweb defaults
        // it ON: a form whose values never reach the agent is the single most
        // confusing way render_ui can misbehave. An explicit false opts out.
        if (s.sendDataModel != null) block.sendDataModel = !!s.sendDataModel;
        // v1.0 lets createSurface carry the whole UI in one message.
        if (Array.isArray(s.components)) {
          for (const c of s.components) {
            if (c && typeof c.id === 'string') block.componentsMap.set(c.id, c);
          }
        }
        if (s.dataModel != null) block.dataModel = s.dataModel;
      } else if (msg.updateComponents) {
        const arr = msg.updateComponents.components || [];
        for (const c of arr) {
          if (c && typeof c.id === 'string') block.componentsMap.set(c.id, c);
        }
      } else if (msg.updateDataModel) {
        const u = msg.updateDataModel;
        if (!u.path || u.path === '/') {
          block.dataModel = (u.value == null ? {} : u.value);
        } else if (u.value === null || u.value === undefined) {
          // v1.0: an explicit null deletes the key at `path`.
          a2uiPtrDelete(block.dataModel, u.path);
        } else {
          a2uiPtrSet(block.dataModel, u.path, u.value);
        }
      } else if (msg.deleteSurface) {
        // deleteSurface tears the surface down. The bubble is removed from the
        // DOM in a2uiRerender (flag here, act there).
        const targetSid = msg.deleteSurface.surfaceId;
        if (targetSid == null || targetSid === block.surfaceId) {
          block.componentsMap.clear();
          block.dataModel = {};
          block.deleted = true;
        }
      } else if (msg.callRendererFunction) {
        a2uiRunRendererFunction(block, msg.callRendererFunction);
      }
      // agentFunctionResponse needs no handling: octoweb never issues
      // callAgentFunction, so no renderer-side call is ever outstanding.
    }
    block.version++;
  }

  // v1.0 bidirectional functions: the agent can invoke a renderer function
  // directly instead of hanging it off a button. The response travels the same
  // channel a click does, so a blocked `render_ui` call gets its answer.
  function a2uiRunRendererFunction(block, call) {
    const fn = call && call.callFunction;
    const id = call && call.functionCallId;
    if (!fn || typeof fn.call !== 'string') return;
    const scope = { root: block.dataModel, local: null };
    let payload;
    if (!A2UI_FN[fn.call]) {
      payload = { functionCallId: id, error: { code: 'UNKNOWN_FUNCTION', message: 'No function "' + fn.call + '" in this catalog.' } };
    } else {
      try {
        // The agent asking for it is the activation: this is an explicit
        // request, not a side effect of painting a surface.
        const value = a2uiWithActivation(() => a2uiResolveValue({ call: fn.call, args: fn.args || {} }, scope));
        payload = { functionCallId: id, value: value === undefined ? null : value };
      } catch (e) {
        payload = { functionCallId: id, error: { code: 'EXECUTION_FAILED', message: String(e && e.message ? e.message : e) } };
      }
    }
    window.ipc.postMessage(JSON.stringify({
      type: 'a2ui_fn_response',
      file_id: block.pollFileId || block.fileId,
      sid: block.sid,
      response: { version: 'v1.0', rendererFunctionResponse: payload },
    }));
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

  function a2uiSendRepairPrompt(block, detail) {
    const s = sessions.get(block.sid);
    if (!s) return;
    const prompt = 'An interactive A2UI control could not be loaded. Please fix the surface and render it again. Failure: '
      + detail;
    if (!s.busy && !s.submitting) {
      dispatchPromptForSession(s, prompt, [], []);
    } else if (s.msgQueue.length < MAX_QUEUE) {
      s.msgQueue.push({ text: prompt, images: [], docs: [] });
      if (active === s) {
        renderQueue();
        updateInputLock();
      }
    } else {
      a2uiToast('The message queue is full.');
    }
  }

  function a2uiFailureControl(block, detail) {
    const wrap = document.createElement('div');
    wrap.className = 'a2ui-unknown';
    const text = document.createElement('span');
    text.textContent = "This control couldn't be loaded";
    const ask = document.createElement('button');
    ask.type = 'button';
    ask.className = 'a2ui-btn default';
    ask.textContent = 'Ask the assistant to fix it';
    ask.addEventListener('click', () => a2uiSendRepairPrompt(block, detail));
    wrap.appendChild(text);
    wrap.appendChild(ask);
    return wrap;
  }

  // Local field edits keep the existing DOM controls alive so WKWebView keeps
  // composition, selection, native slider capture and the undo stack. Only
  // nodes with derived/bound presentation register a lightweight refresh.
  function a2uiRefreshBoundOutputs(block) {
    const wrap = a2uiBubbleByFile.get(block.fileId);
    const body = wrap && wrap.querySelector('.a2ui-body');
    if (!body) return;
    for (const el of body.querySelectorAll('*')) {
      if (typeof el.__a2uiRefresh === 'function') el.__a2uiRefresh();
    }
  }

  // Tags every rendered element with the component id that produced it, so
  // `a2uiRerender` can put focus and the caret back after a rebuild. List rows
  // reuse one template id, so the row index disambiguates them. `weight` and
  // `accessibility` are catalog-wide props, applied here rather than in each
  // component branch.
  function a2uiRenderNode(block, def, scope) {
    const el = a2uiRenderNodeInner(block, def, scope);
    if (!el || el.nodeType !== 1) return el;
    if (def && typeof def.id === 'string') {
      el.dataset.a2uiId = def.id + (scope && scope.rowKey != null ? '#' + scope.rowKey : '');
    }
    el.__a2uiDef = def;
    el.__a2uiScope = scope;
    if (def && def.weight != null && isFinite(Number(def.weight))) {
      el.style.flexGrow = String(Number(def.weight));
    }
    a2uiApplyAccessibility(el, def, scope);
    return el;
  }

  // v1.0 `accessibility`: label / description / live / hidden, straight to ARIA.
  function a2uiApplyAccessibility(el, def, scope) {
    const a = def && def.accessibility;
    if (!a || typeof a !== 'object') return;
    if (a.label != null) el.setAttribute('aria-label', a2uiToStr(a2uiResolveValue(a.label, scope)));
    if (a.description != null) el.setAttribute('aria-description', a2uiToStr(a2uiResolveValue(a.description, scope)));
    if (typeof a.live === 'string' && a.live !== 'off') el.setAttribute('aria-live', a.live);
    if (a.hidden != null && a2uiTruthy(a2uiResolveValue(a.hidden, scope))) el.setAttribute('aria-hidden', 'true');
  }

  // Builds a fresh DOM tree from `def` and its children-by-id refs in
  // `block.componentsMap`. Re-rendered on every mutation — components are
  // small enough (typical surface: <20 nodes) that we don't need a smart diff.
  function a2uiRenderNodeInner(block, def, scope) {
    if (!def) return document.createComment('missing');
    const type = typeof def.component === 'string' ? def.component : '';
    const r = v => a2uiResolveValue(v, scope);
    const text = r(def.text);
    const label = r(def.label);
    const placeholder = r(def.placeholder);
    const valueRaw = r(def.value);
    const path = a2uiPathOf(def.value, scope);

    // A static id list, or a v1.0 ChildList template ({componentId, path}).
    // Returns [def, scope] pairs so template rows carry their own item scope.
    function childNodes() {
      const out = [];
      if (typeof def.child === 'string') {
        const c = block.componentsMap.get(def.child);
        if (c) out.push([c, scope]);
      }
      const ch = def.children;
      if (Array.isArray(ch)) {
        for (const id of ch) {
          if (typeof id !== 'string') continue;
          const c = block.componentsMap.get(id);
          if (c) out.push([c, scope]);
        }
      } else if (ch && typeof ch === 'object' && typeof ch.componentId === 'string') {
        const childPath = typeof ch.path === 'string' ? ch.path : '';
        const items = r({ path: childPath });
        const listModelPath = a2uiPathOf({ path: childPath }, scope);
        const tpl = block.componentsMap.get(ch.componentId);
        if (Array.isArray(items) && tpl) {
          items.forEach((item, i) => {
            const base = listModelPath === '/' ? '' : listModelPath;
            const itemModelPath = typeof listModelPath === 'string' ? base + '/' + i : null;
            out.push([tpl, {
              root: scope.root,
              local: item,
              index: i,
              rowKey: itemModelPath || i,
              modelPath: itemModelPath,
            }]);
          });
        }
      }
      return out;
    }
    function appendKids(el) {
      for (const pair of childNodes()) el.appendChild(a2uiRenderNode(block, pair[0], pair[1]));
    }
    function writeBinding(p, v) {
      if (!p) return;
      a2uiPtrSet(block.dataModel, p, v);
      block.version++;
      a2uiRefreshBoundOutputs(block);
    }
    // v1.0 CheckRule is {condition, message}; older surfaces put the function
    // call at the top level ({call, args, message}). Both gate the same way,
    // and a condition may resolve to a ValidationResult carrying its own text.
    function failedCheck() {
      const checks = Array.isArray(def.checks) ? def.checks : [];
      for (const c of checks) {
        if (!c || typeof c !== 'object') continue;
        const condition = c.condition !== undefined ? c.condition : c;
        const result = a2uiResolveValue(condition, scope);
        if (a2uiTruthy(result)) continue;
        const inline = result && typeof result === 'object' ? result.message : null;
        return String(inline || c.message || 'validation failed');
      }
      return null;
    }
    // Inputs surface their failing check inline — the v1.0 guidance is that
    // the message belongs to the check, not to a separate Text component.
    // Held back while the field is still empty so an untouched form isn't a
    // wall of red before the user has typed anything.
    function appendCheckMessage(wrap) {
      for (const child of Array.from(wrap.children)) {
        if (child.classList && child.classList.contains('a2ui-check-msg')) child.remove();
      }
      const currentValue = r(def.value);
      if (currentValue == null || currentValue === '') return;
      const msg = failedCheck();
      if (!msg) return;
      const el = document.createElement('span');
      el.className = 'a2ui-check-msg';
      el.textContent = msg;
      wrap.appendChild(el);
    }
    // CSS mapping for the catalog's align/justify enums.
    const flexMap = {
      start: 'flex-start', center: 'center', end: 'flex-end', stretch: 'stretch',
      spaceAround: 'space-around', spaceBetween: 'space-between', spaceEvenly: 'space-evenly',
    };

    if (type === 'Card') {
      const el = document.createElement('div');
      el.className = 'a2ui-card';
      appendKids(el);
      return el;
    }
    if (type === 'Column' || type === 'Row') {
      const el = document.createElement('div');
      el.className = type === 'Row' ? 'a2ui-row' : 'a2ui-col';
      // `gap` is not in the catalog but agents lean on it and it reads better
      // than nothing; the catalog's own answer is margins on the leaves.
      if (def.gap != null) el.style.gap = def.gap + 'px';
      if (typeof def.align === 'string') el.style.alignItems = flexMap[def.align] || def.align;
      if (typeof def.justify === 'string') el.style.justifyContent = flexMap[def.justify] || def.justify;
      appendKids(el);
      return el;
    }
    if (type === 'Spacer') {
      // Legacy render_ui component — never part of the catalog.
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
      // v1.0 narrows `variant` to caption|body and asks renderers to treat the
      // content as simple Markdown. v0.9's h1–h5 still map to heading tags so
      // replayed surfaces keep their shape.
      const variant = typeof def.variant === 'string' ? def.variant : 'body';
      const asHeading = ['h1', 'h2', 'h3', 'h4', 'h5'].indexOf(variant) >= 0;
      const el = document.createElement(asHeading ? variant : 'div');
      el.className = 'a2ui-text a2ui-text-' + variant + (def.muted ? ' muted' : '');
      const paint = () => {
        const raw = a2uiToStr(r(def.text));
        if (asHeading) el.textContent = raw;
        else el.innerHTML = a2uiRenderMarkdown(raw);
      };
      if (!asHeading) el.classList.add('a2ui-md');
      paint();
      el.__a2uiRefresh = paint;
      return el;
    }
    if (type === 'Heading') {
      // Legacy render_ui component — the catalog uses Text with a variant.
      const lvl = Math.min(Math.max(1, Number(def.level == null ? 2 : def.level)), 4);
      const el = document.createElement('h' + lvl);
      el.className = 'a2ui-heading';
      const paint = () => { el.textContent = a2uiToStr(r(def.text)); };
      paint();
      el.__a2uiRefresh = paint;
      return el;
    }
    if (type === 'Markdown') {
      // Legacy render_ui component — the catalog puts Markdown in Text.
      const el = document.createElement('div');
      el.className = 'a2ui-md';
      const paint = () => { el.innerHTML = a2uiRenderMarkdown(a2uiToStr(r(def.text))); };
      paint();
      el.__a2uiRefresh = paint;
      return el;
    }
    if (type === 'Image') {
      // v1.0: url, description, fit, variant. `src`/`alt` are the older
      // render_ui spellings.
      const fit = typeof def.fit === 'string' ? def.fit : null;
      const variant = typeof def.variant === 'string' ? def.variant : null;
      const img = document.createElement('img');
      img.className = 'a2ui-img' + (variant ? ' a2ui-img-' + variant : '');
      img.loading = 'lazy';
      const paint = () => {
        const url = String((r(def.url) != null ? r(def.url) : r(def.src)) || '');
        const desc = String((r(def.description) != null ? r(def.description) : r(def.alt)) || '');
        if (/^https?:\/\//i.test(url)) img.src = url;
        else img.removeAttribute('src');
        img.alt = desc;
      };
      paint();
      img.__a2uiRefresh = paint;
      if (fit) {
        const map = { contain: 'contain', cover: 'cover', fill: 'fill', none: 'none', scaleDown: 'scale-down' };
        if (map[fit]) img.style.objectFit = map[fit];
      }
      if (def.width != null) img.style.width = def.width + 'px';
      if (def.height != null) img.style.height = def.height + 'px';
      return img;
    }
    if (type === 'Icon') {
      // v1.0 `name` is either a catalog icon name or {svgPath}. We have no
      // icon font, so a named icon renders as a small labelled badge; an
      // svgPath renders for real.
      const nameRef = def.name;
      if (nameRef && typeof nameRef === 'object' && nameRef.svgPath != null) {
        const svg = document.createElementNS('http://www.w3.org/2000/svg', 'svg');
        svg.setAttribute('class', 'a2ui-icon-svg');
        svg.setAttribute('viewBox', '0 0 24 24');
        svg.setAttribute('width', '18');
        svg.setAttribute('height', '18');
        const p = document.createElementNS('http://www.w3.org/2000/svg', 'path');
        p.setAttribute('d', a2uiToStr(r(nameRef.svgPath)));
        p.setAttribute('fill', 'currentColor');
        svg.appendChild(p);
        return svg;
      }
      const span = document.createElement('span');
      span.className = 'a2ui-icon';
      const paint = () => {
        const iname = String((r(nameRef) != null ? r(nameRef) : '') || '');
        span.textContent = iname;
        span.setAttribute('aria-label', iname);
      };
      paint();
      span.__a2uiRefresh = paint;
      return span;
    }
    if (type === 'Video') {
      const url = String((r(def.url) != null ? r(def.url) : '') || '');
      const poster = String((r(def.posterUrl) != null ? r(def.posterUrl) : '') || '');
      const v = document.createElement('video');
      v.className = 'a2ui-video';
      v.controls = true;
      v.preload = 'metadata';
      if (/^https?:\/\//i.test(url)) v.src = url;
      if (/^https?:\/\//i.test(poster)) v.poster = poster;
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
      // v1.0: { child: ComponentId, variant: default|primary|borderless,
      //         action, checks }. `text`/`label`/`kind` are the older
      //         render_ui spellings and still render.
      const kind = typeof def.kind === 'string' ? def.kind : null;
      const variant = typeof def.variant === 'string' ? def.variant : null;
      // `action` is the catalog spelling. Agents routinely emit `actions`;
      // accept that legacy spelling but replace dead controls with a repair UI.
      const action = def.action || def.actions || {};
      const hasFunction = action.functionCall && typeof action.functionCall.call === 'string';
      const hasOpenUrl = typeof action.openUrl === 'string'
        || (action.openUrl && typeof action.openUrl.url === 'string');
      const hasEvent = action.event && typeof action.event.name === 'string' && action.event.name;
      const modalTriggerOnly = !hasFunction && !hasOpenUrl && !hasEvent && scope && scope.modalTrigger;
      if (!hasFunction && !hasOpenUrl && !hasEvent && !modalTriggerOnly) {
        return a2uiFailureControl(block, 'Button ' + (def.id || '(unnamed)') + ' has no supported action.');
      }
      const btn = document.createElement('button');
      // The catalog's own default is the subtle style; `primary` is the
      // surface author's way of naming the call to action.
      btn.className = 'a2ui-btn ' + (kind || variant || 'default');
      const paint = () => {
        btn.disabled = a2uiTruthy(r(def.disabled));
        if (typeof def.child !== 'string') {
          const currentText = r(def.text);
          btn.textContent = a2uiToStr(currentText != null ? currentText : (r(def.label) != null ? r(def.label) : 'Button'));
        }
      };
      if (typeof def.child === 'string') {
        const inner = block.componentsMap.get(def.child);
        if (!inner) return a2uiFailureControl(block, 'Button ' + (def.id || '(unnamed)') + ' references missing content.');
        btn.appendChild(a2uiRenderNode(block, inner, scope));
      }
      paint();
      btn.__a2uiRefresh = paint;
      btn.addEventListener('click', () => {
        if (modalTriggerOnly) return;
        if (block.resolved) return;
        const failure = failedCheck();
        if (failure) {
          a2uiToast(failure);
          return;
        }
        // A renderer-side action is `action.functionCall = {call, args}` — the
        // same shape a ValueRef uses, so resolving it runs the registry entry.
        // The click is the user activation openUrl requires.
        if (action.functionCall && typeof action.functionCall.call === 'string') {
          a2uiWithActivation(() => r(action.functionCall));
          return;
        }
        if (action.openUrl) {
          // Shorthand some agents emit instead of functionCall: `{url: string}`
          // (an object, not a bare string). Accept both shapes defensively.
          const urlValue = typeof action.openUrl === 'string'
            ? action.openUrl
            : (action.openUrl && typeof action.openUrl.url === 'string' ? action.openUrl.url : '');
          a2uiWithActivation(() => A2UI_FN.openUrl({ url: urlValue }));
          return;
        }
        const ev = action.event;
        if (!ev || !ev.name) {
          a2uiSendRepairPrompt(block, 'Button ' + (def.id || '(unnamed)') + ' has no event action.');
          return;
        }
        const context = {};
        for (const k in (ev.context || {})) context[k] = r(ev.context[k]);
        // The A2UI v1.0 renderer-to-agent `action` message.
        const actionPayload = {
          name: ev.name,
          sourceComponentId: typeof def.id === 'string' ? def.id : undefined,
          surfaceId: block.surfaceId,
          timestamp: new Date().toISOString(),
          context,
        };
        if (ev.userMessage != null) actionPayload.userMessage = a2uiToStr(r(ev.userMessage));
        if (block.sendDataModel) actionPayload.dataModel = block.dataModel;
        // Every click goes to Rust the same way. Whether a `render_ui` call is
        // still waiting on this surface is Rust's question — it holds the
        // pending map. When nothing is waiting it forwards the click to the
        // agent as a prompt instead, so a click is never silently dropped.
        // `pollFileId` is only a hint about WHICH envelope to complete.
        window.ipc.postMessage(JSON.stringify({
          type: 'a2ui_resolve',
          file_id: block.pollFileId || block.fileId,
          sid: block.sid,
          action: actionPayload,
        }));
        // Optimistic lock on the bubble.
        block.resolved = true;
        a2uiRerender(block);
        // Mark the SESSION busy too — the agent is about to process this click
        // and may take time. Without this the input box stays enabled, no
        // thinking indicator shows, and the stop button is missing. Same UX as
        // a typed prompt.
        const liveSession = sessions.get(block.sid);
        if (liveSession) {
          liveSession.busy = true;
          window.__setThinking(liveSession.sid, true);
        }
      });
      return btn;
    }
    if (type === 'TextField') {
      // v1.0: { label, value, placeholder, variant: longText|number|shortText|obscured, checks }
      // `type`/`multiline`/`rows` are the older render_ui spellings.
      const variant = typeof def.variant === 'string' ? def.variant : null;
      const isMultiline = def.multiline || variant === 'longText';
      const isNumber = def.type === 'number' || variant === 'number';
      const inputType = (() => {
        if (variant === 'obscured') return 'password';
        if (typeof def.type === 'string' && ['password', 'email', 'number', 'tel'].indexOf(def.type) >= 0) return def.type;
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
        if (placeholder != null) ta.placeholder = a2uiToStr(placeholder);
        ta.value = a2uiToStr(valueRaw);
        ta.addEventListener('input', e => path && writeBinding(path, e.currentTarget.value));
        wrap.appendChild(ta);
        wrap.__a2uiRefresh = () => {
          const next = a2uiToStr(r(def.value));
          if (document.activeElement !== ta && ta.value !== next) ta.value = next;
          appendCheckMessage(wrap);
        };
      } else {
        const inp = document.createElement('input');
        inp.type = inputType;
        if (placeholder != null) inp.placeholder = a2uiToStr(placeholder);
        inp.value = a2uiToStr(valueRaw);
        inp.addEventListener('input', e => {
          if (!path) return;
          const raw = e.currentTarget.value;
          const v = isNumber ? (raw === '' ? null : Number(raw)) : raw;
          writeBinding(path, v);
        });
        wrap.appendChild(inp);
        wrap.__a2uiRefresh = () => {
          const next = a2uiToStr(r(def.value));
          if (document.activeElement !== inp && inp.value !== next) inp.value = next;
          appendCheckMessage(wrap);
        };
      }
      appendCheckMessage(wrap);
      return wrap;
    }
    if (type === 'CheckBox') {
      const wrap = document.createElement('label');
      wrap.className = 'a2ui-check';
      const inp = document.createElement('input');
      inp.type = 'checkbox';
      inp.checked = a2uiTruthy(valueRaw);
      inp.addEventListener('change', e => path && writeBinding(path, e.currentTarget.checked));
      const sp = document.createElement('span');
      sp.textContent = a2uiToStr(label);
      wrap.appendChild(inp);
      wrap.appendChild(sp);
      wrap.__a2uiRefresh = () => {
        inp.checked = a2uiTruthy(r(def.value));
        sp.textContent = a2uiToStr(r(def.label));
      };
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
      const min = def.min != null ? Number(def.min) : 0;
      const max = def.max != null ? Number(def.max) : 100;
      inp.min = String(min);
      inp.max = String(max);
      // v1.0 `steps` is a count of divisions; `step` was the render_ui spelling
      // for the increment itself.
      if (def.steps != null && Number(def.steps) > 0) inp.step = String((max - min) / Number(def.steps));
      else if (def.step != null) inp.step = String(def.step);
      inp.value = valueRaw != null ? String(valueRaw) : String(min);
      inp.addEventListener('input', e => path && writeBinding(path, Number(e.currentTarget.value)));
      const out = document.createElement('output');
      out.className = 'a2ui-slider-val';
      out.textContent = inp.value;
      row.appendChild(inp);
      row.appendChild(out);
      wrap.appendChild(row);
      wrap.__a2uiRefresh = () => {
        const next = r(def.value);
        if (document.activeElement !== inp && next != null) inp.value = String(next);
        out.textContent = inp.value;
        appendCheckMessage(wrap);
      };
      appendCheckMessage(wrap);
      return wrap;
    }
    if (type === 'ChoicePicker') {
      // v1.0: { label, options: [{label, value}], value: string[],
      //         variant: multipleSelection|mutuallyExclusive,
      //         displayStyle: checkbox|chips, filterable }
      // `choices` and scalar options are the older render_ui spellings.
      const optionsRaw = Array.isArray(def.options) ? def.options
        : Array.isArray(def.choices) ? def.choices : [];
      const variant = typeof def.variant === 'string' ? def.variant : 'mutuallyExclusive';
      const isMulti = variant === 'multipleSelection';
      const style = typeof def.displayStyle === 'string' ? def.displayStyle : null;
      const useChips = style === 'chips';
      const opts = optionsRaw.map(c => {
        if (c != null && typeof c === 'object') return { label: r(c.label), value: r(c.value) };
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
      // v1.0 binds selections to a string array in both variants; a bare
      // scalar from an older surface still reads correctly.
      const currentSelection = () => {
        const current = r(def.value);
        return new Set(
          Array.isArray(current) ? current.map(a2uiToStr)
            : current == null || current === '' ? []
              : [a2uiToStr(current)]
        );
      };
      const selected = currentSelection();
      // Writing back keeps the array shape the catalog asks for.
      const writeSelection = next => path && writeBinding(path, Array.from(next));
      // `filterable` narrows a long list; the filter itself is view state, so
      // it lives on the block rather than in the agent's data model.
      block.filterState = block.filterState || {};
      const filterKey = typeof def.id === 'string' ? def.id : 'choice';
      const filter = String(block.filterState[filterKey] || '');
      const choiceRows = [];
      const applyFilter = value => {
        const needle = String(value || '').toLowerCase();
        for (const row of choiceRows) row.el.hidden = !!needle && row.search.indexOf(needle) < 0;
      };
      if (def.filterable) {
        const f = document.createElement('input');
        f.type = 'search';
        f.className = 'a2ui-choice-filter';
        f.placeholder = 'Filter…';
        f.value = filter;
        f.addEventListener('input', e => {
          block.filterState[filterKey] = e.currentTarget.value;
          applyFilter(e.currentTarget.value);
        });
        wrap.appendChild(f);
      }
      const visible = opts;
      // Mutually exclusive, plain style: a native <select> reads best.
      if (!isMulti && !useChips && style !== 'checkbox' && !def.filterable) {
        const sel = document.createElement('select');
        if (selected.size === 0) {
          const placeholderOpt = document.createElement('option');
          placeholderOpt.value = '';
          placeholderOpt.textContent = 'Select an option';
          placeholderOpt.disabled = true;
          placeholderOpt.selected = true;
          sel.appendChild(placeholderOpt);
        }
        for (const o of visible) {
          const opt = document.createElement('option');
          opt.value = a2uiToStr(o.value);
          opt.textContent = a2uiToStr(o.label != null ? o.label : o.value);
          if (selected.has(opt.value)) opt.selected = true;
          sel.appendChild(opt);
        }
        sel.addEventListener('change', e => {
          if (e.currentTarget.value !== '') writeSelection(new Set([e.currentTarget.value]));
        });
        wrap.appendChild(sel);
        wrap.__a2uiRefresh = () => {
          const current = currentSelection();
          const value = current.size ? Array.from(current)[0] : '';
          if (sel.value !== value) sel.value = value;
          appendCheckMessage(wrap);
        };
        appendCheckMessage(wrap);
        return wrap;
      }
      const list = document.createElement('div');
      list.className = useChips ? 'a2ui-chip-row' : 'a2ui-check-list';
      for (const o of visible) {
        const v = a2uiToStr(o.value);
        const lblTxt = a2uiToStr(o.label != null ? o.label : o.value);
        const isOn = selected.has(v);
        const toggle = on => {
          if (!isMulti) {
            writeSelection(on ? new Set([v]) : new Set());
            return;
          }
          const next = currentSelection();
          if (on) next.add(v); else next.delete(v);
          writeSelection(next);
        };
        if (useChips) {
          const chip = document.createElement('button');
          chip.type = 'button';
          chip.className = 'a2ui-chip' + (isOn ? ' on' : '');
          chip.textContent = lblTxt;
          chip.addEventListener('click', () => toggle(isMulti ? !currentSelection().has(v) : true));
          list.appendChild(chip);
          choiceRows.push({ el: chip, search: lblTxt.toLowerCase(), value: v, control: chip });
        } else {
          const item = document.createElement('label');
          item.className = 'a2ui-check';
          const inp = document.createElement('input');
          inp.type = isMulti ? 'checkbox' : 'radio';
          if (!isMulti) inp.name = (def.id || 'choice') + '_' + (block.fileId || '');
          inp.checked = isOn;
          inp.addEventListener('change', () => toggle(inp.checked));
          const sp = document.createElement('span');
          sp.textContent = lblTxt;
          item.appendChild(inp);
          item.appendChild(sp);
          list.appendChild(item);
          choiceRows.push({ el: item, search: lblTxt.toLowerCase(), value: v, control: inp });
        }
      }
      wrap.appendChild(list);
      wrap.__a2uiRefresh = () => {
        const current = currentSelection();
        for (const row of choiceRows) {
          const on = current.has(row.value);
          if (row.control.tagName === 'BUTTON') row.control.classList.toggle('on', on);
          else row.control.checked = on;
        }
        appendCheckMessage(wrap);
      };
      applyFilter(filter);
      appendCheckMessage(wrap);
      return wrap;
    }
    if (type === 'DateTimeInput') {
      // v1.0: { enableDate, enableTime, min, max, label, value }
      // `mode` is the older render_ui spelling.
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
      if (def.min != null) inp.min = a2uiToStr(r(def.min));
      if (def.max != null) inp.max = a2uiToStr(r(def.max));
      inp.addEventListener('change', e => path && writeBinding(path, e.currentTarget.value));
      wrap.appendChild(inp);
      wrap.__a2uiRefresh = () => {
        const next = a2uiToStr(r(def.value));
        if (document.activeElement !== inp && inp.value !== next) inp.value = next;
        inp.min = def.min != null ? a2uiToStr(r(def.min)) : '';
        inp.max = def.max != null ? a2uiToStr(r(def.max)) : '';
        appendCheckMessage(wrap);
      };
      appendCheckMessage(wrap);
      return wrap;
    }
    if (type === 'List') {
      const el = document.createElement('div');
      const dir = typeof def.direction === 'string' ? def.direction : 'vertical';
      el.className = 'a2ui-list a2ui-list-' + dir;
      if (typeof def.align === 'string') el.style.alignItems = flexMap[def.align] || def.align;
      appendKids(el);
      return el;
    }
    if (type === 'Tabs') {
      // v1.0: { tabs: [{ title, child }] }. `content` was the render_ui name.
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
      const childId = typeof activeTab.child === 'string' ? activeTab.child
        : typeof activeTab.content === 'string' ? activeTab.content : null;
      const childDef = childId ? block.componentsMap.get(childId) : null;
      if (childDef) pane.appendChild(a2uiRenderNode(block, childDef, scope));
      wrap.appendChild(bar);
      wrap.appendChild(pane);
      return wrap;
    }
    if (type === 'Modal') {
      // v1.0: { trigger: ComponentId, content: ComponentId }
      // We render the trigger inline; clicking it shows `content` in an
      // overlay. The visibility flag lives on the block so re-renders preserve it.
      const triggerId = typeof def.trigger === 'string' ? def.trigger : null;
      const contentId = typeof def.content === 'string' ? def.content : null;
      const wrap = document.createElement('span');
      wrap.className = 'a2ui-modal-trigger-wrap';
      block.modalState = block.modalState || {};
      block.modalNeedsFocus = block.modalNeedsFocus || {};
      const key = typeof def.id === 'string' ? def.id : 'modal';
      const triggerDef = triggerId ? block.componentsMap.get(triggerId) : null;
      if (triggerDef) {
        const triggerScope = Object.assign({}, scope, { modalTrigger: true });
        const trig = a2uiRenderNode(block, triggerDef, triggerScope);
        trig.addEventListener('click', (e) => {
          e.stopPropagation();
          block.modalState[key] = true;
          block.modalNeedsFocus[key] = true;
          a2uiRerender(block);
        }, true);
        wrap.appendChild(trig);
      } else {
        wrap.appendChild(a2uiFailureControl(block, 'Modal ' + key + ' references a missing trigger.'));
      }
      if (block.modalState[key]) {
        const contentDef = contentId ? block.componentsMap.get(contentId) : null;
        const overlay = document.createElement('div');
        overlay.className = 'a2ui-modal-overlay';
        const closeModal = () => {
          block.modalState[key] = false;
          a2uiRerender(block);
        };
        overlay.addEventListener('click', closeModal);
        overlay.addEventListener('keydown', (e) => {
          if (e.key !== 'Escape') return;
          e.preventDefault();
          e.stopPropagation();
          closeModal();
        });
        const panel = document.createElement('div');
        panel.className = 'a2ui-modal-panel';
        panel.setAttribute('role', 'dialog');
        panel.setAttribute('aria-modal', 'true');
        panel.setAttribute('aria-label', a2uiToStr(label != null ? label : (r(def.title) || 'Assistant control')));
        panel.tabIndex = -1;
        panel.addEventListener('click', (e) => e.stopPropagation());
        const close = document.createElement('button');
        close.type = 'button';
        close.className = 'a2ui-modal-close';
        close.setAttribute('aria-label', 'Close');
        close.textContent = '×';
        close.addEventListener('click', closeModal);
        panel.appendChild(close);
        if (contentDef) panel.appendChild(a2uiRenderNode(block, contentDef, scope));
        else panel.appendChild(a2uiFailureControl(block, 'Modal ' + key + ' references missing content.'));
        overlay.appendChild(panel);
        wrap.appendChild(overlay);
        if (block.modalNeedsFocus[key]) {
          block.modalNeedsFocus[key] = false;
          requestAnimationFrame(() => {
            if (!panel.isConnected) return;
            const initial = panel.querySelector(
              'input:not([disabled]), textarea:not([disabled]), select:not([disabled]), button:not(.a2ui-modal-close):not([disabled])'
            ) || close;
            initial.focus({ preventScroll: true });
          });
        }
      }
      return wrap;
    }
    return a2uiFailureControl(block, 'Unsupported component ' + (type || '(missing type)') + '.');
  }

  // Focus survival across a full surface rebuild. Anchors on the owning
  // component's id rather than the element itself, which is gone by then.
  function captureA2uiFocus(body) {
    const act = document.activeElement;
    if (!act || !body.contains(act)) return null;
    const holder = act.closest('[data-a2ui-id]');
    if (!holder) return null;
    const out = { id: holder.dataset.a2uiId, scroll: act.scrollTop || 0, start: null, end: null };
    // selectionStart throws on input types that don't support selection
    // (number, email, …), so it's guarded rather than feature-detected.
    try { out.start = act.selectionStart; out.end = act.selectionEnd; } catch (e) {}
    return out;
  }

  function restoreA2uiFocus(body, restore) {
    if (!restore) return;
    const holder = Array.from(body.querySelectorAll('[data-a2ui-id]'))
      .find(el => el.dataset.a2uiId === String(restore.id));
    const selector = 'input, textarea, select, button';
    const el = holder && (holder.matches(selector) ? holder : holder.querySelector(selector));
    if (!el) return;
    // preventScroll: refocusing must not yank the chat viewport around while
    // the user is mid-sentence.
    el.focus({ preventScroll: true });
    if (restore.scroll) el.scrollTop = restore.scroll;
    if (restore.start != null && typeof el.setSelectionRange === 'function') {
      try { el.setSelectionRange(restore.start, restore.end); } catch (e) {}
    }
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
    // The one state worth showing: this surface is holding the agent's turn
    // open until you answer it. Everything else is output, and output does not
    // need a badge. `surfaceId` is debugging detail, so it goes on the tooltip
    // rather than into a strip of chrome above every card.
    const awaiting = block.live && !block.resolved
      && Array.isArray(block.awaitEvents) && block.awaitEvents.length > 0;
    wrap.classList.toggle('awaiting', awaiting);
    if (block.surfaceId) wrap.title = block.surfaceId;
    const body = wrap.querySelector('.a2ui-body');
    if (body) {
      const rootDef = block.componentsMap.get('root');
      if (rootDef) {
        // Agent envelopes and structural actions rebuild the surface. Local
        // field edits take the incremental a2uiRefreshBoundOutputs path, so
        // this focus restoration is only for genuine structural replacement.
        const restore = captureA2uiFocus(body);
        body.innerHTML = '';
        body.appendChild(a2uiRenderNode(block, rootDef, {
          root: block.dataModel,
          local: null,
          modelPath: null,
        }));
        restoreA2uiFocus(body, restore);
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
      componentsMap: new Map(),
      dataModel: {},
      // v1.0 `sendDataModel`, defaulted on — see a2uiApplyMessages.
      sendDataModel: true,
      awaitEvents: [],
      resolved: false,
      // The fileId of the envelope a render_ui call is waiting on, used as the
      // preferred key when reporting a click. `null` after replay, after a
      // fire-and-forget update, or once the call completed — the click then
      // reports this block's own fileId and Rust resolves or forwards it.
      pollFileId: null,
      // True once a live agent produced an envelope for this block (sticky).
      // Only affects whether a terminal envelope locks the card — click
      // routing no longer depends on it.
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
    const body = document.createElement('div');
    body.className = 'a2ui-body';
    bubble.appendChild(body);
    wrap.appendChild(label);
    wrap.appendChild(bubble);
    mountMsg(s, wrap);
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
    // post-restart surfaces are NOT live — no render_ui call is waiting, so
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
    // pollFileId tracks which envelope render_ui is currently waiting on for a
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
    //   LIVE: "resolved" means the render_ui call is mid-flight after a click;
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
    // clear the hint; the next click reports the block's own fileId instead.
    if (isTerminal && block.pollFileId === fileId) {
      block.pollFileId = null;
    }
    a2uiRerender(block);
    // Mirror Rust's `upsert_a2ui_msg`: one log record per call id, updated in
    // place, so a surface is searchable and survives a window re-render.
    const sess = sessions.get(sid);
    if (sess && !sess.replaying) {
      const at = sess.log.findIndex(r => r.role === 'ui' && r.text === fileId);
      if (at >= 0) sess.log[at].a2ui = payload;
      else sess.log.push({ role: 'ui', text: fileId, ts: Date.now(), a2ui: payload });
    }
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
</html>"#.replace("/*@@THEME@@*/", crate::theme::CSS)
        .replace("/* A2UI_CORE_JS */", crate::a2ui_js::CORE)
        .replace("/* PROMPT_HISTORY_JS */", prompt_history_js)
        .replace("/* KEYBINDINGS_JSON */", &keybindings_json)
        .replace("/* MAX_SESSIONS */", &crate::MAX_SESSIONS.to_string())
        .replace(
            "/* MAX_PROMPT_HISTORY */",
            &max_ai_prompt_history.to_string(),
        )
        .replace("/* ICON_OCTOPUS_BRAND */", crate::icons::OCTOPUS_BRAND)
        .replace("/* ICON_CHECK */", crate::icons::CHECK)
        .replace("/* ICON_CHECK_CIRCLE */", crate::icons::CHECK_CIRCLE)
        .replace("/* ICON_X_CIRCLE */", crate::icons::X_CIRCLE)
}

#[cfg(test)]
mod tests {
    /// The sidebar is one long inline script assembled from several Rust
    /// consts. A syntax error anywhere in it kills the whole panel at runtime
    /// with nothing in the build to warn you, so parse it here instead.
    #[test]
    fn the_inline_script_parses() {
        let html = super::html(50);
        let open = html.find("<script>").expect("inline script");
        let start = open + "<script>".len();
        let end = start + html[start..].find("</script>").expect("script close");
        let Some(out) = crate::a2ui_js::run_node("sidebar", &html[start..end], &["--check"]) else {
            return;
        };
        assert!(
            out.status.success(),
            "sidebar script does not parse:\n{}",
            String::from_utf8_lossy(&out.stderr),
        );
    }

    /// The A2UI core is spliced in by marker; a renamed marker would silently
    /// ship a sidebar with no renderer in it.
    #[test]
    fn the_a2ui_core_is_injected() {
        let html = super::html(50);
        assert!(!html.contains("/* A2UI_CORE_JS */"), "marker not replaced");
        assert!(html.contains("function a2uiResolveValue"), "core missing");
    }
}
