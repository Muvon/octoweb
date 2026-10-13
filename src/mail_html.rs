//! Mail tab of the right-hand pane (spec `specs/001-mail`): the workspace's
//! inbox, a thread, and the composer. Docked it shows one of them at a time;
//! fullscreen puts the list beside the thread.
//!
//! HTML mail renders in a sandboxed iframe without scripts, under a CSP that
//! blocks every remote load until the user allows images (READ-2). Links open
//! in browser tabs through Rust (READ-3).
//!
//! Messages to Rust (`window.ipc`): `mail::Pane` and `mail::PageRequest`.
//!
//! Called from Rust, on `window.__mail`:
//!   setup({ws, accounts, ai})      — workspace on screen and its accounts
//!   status(account, state, message)
//!   inbox / search / thread / acted / queued / cancelled / sendError / images
//!   changed()                      — mail changed: refresh what is shown
//!   open(account, thread, fromNotes), openDraft(draft), show({title, items})
//!   files(key, files), unread(n), fullscreen(on), focus()
//!   window.__setShortcuts(data)

pub fn html(keybindings_json: &str) -> String {
    r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<style>
/*@@THEME@@*/
  html, body {
    width: 100%; height: 100%;
    margin: 0;
    overflow: hidden;
    background: var(--canvas);
    color: var(--label);
    font-family: var(--font-text);
    font-size: 13px;
    -webkit-font-smoothing: antialiased;
  }
  button { font: inherit; color: inherit; }
  #panel { position: fixed; inset: 0; display: flex; flex-direction: column; box-shadow: inset 0.5px 0 0 var(--hairline); }

  #resize-handle { position: fixed; top: 0; bottom: 0; left: 0; width: 6px; z-index: 30; cursor: col-resize; touch-action: none; }
  #resize-handle::after { content: ""; position: absolute; top: 0; bottom: 0; left: 0; width: 2px; background: var(--accent); opacity: 0; }
  #resize-handle:hover::after, #resize-handle.active::after { opacity: 1; }
  #panel.fullscreen #resize-handle { display: none; }
  body.resizing, body.resizing * { cursor: col-resize !important; }

  #bar {
    flex: 0 0 auto; display: flex; align-items: center; gap: 4px;
    height: 40px; padding: 0 10px;
    box-shadow: 0 0.5px 0 var(--hairline);
    font-size: var(--fs-caption); color: var(--label-2);
    -webkit-user-select: none; user-select: none;
  }
  .seg { display: flex; gap: 2px; padding: 2px; border-radius: var(--r-capsule); background: var(--fill); }
  .tab { border: none; background: transparent; padding: 3px 11px; border-radius: var(--r-capsule); color: var(--label-2); font-weight: 500; cursor: pointer; display: flex; align-items: center; gap: 5px; transition: background var(--t-fast), color var(--t-fast); }
  .tab:hover { color: var(--label); }
  .tab.active { background: var(--card); color: var(--label); box-shadow: 0 0 0 0.5px var(--hairline), 0 1px 3px rgba(0, 0, 0, 0.08); }
  .badge { min-width: 16px; padding: 0 5px; box-sizing: border-box; border-radius: var(--r-capsule); background: var(--accent); color: var(--on-accent); font-size: 10px; font-weight: 600; line-height: 16px; text-align: center; }
  .badge:empty { display: none; }
  #bar .spacer { flex: 1; }
  .icon-btn {
    flex-shrink: 0; display: flex; align-items: center; justify-content: center;
    width: 28px; height: 28px; padding: 0; border: none; border-radius: var(--r-capsule);
    background: transparent; color: var(--label-2); cursor: pointer;
    transition: background var(--t-fast), color var(--t-fast);
  }
  .icon-btn svg { width: 15px; height: 15px; }
  .icon-btn:hover { background: var(--fill-hover); color: var(--label); }
  .icon-btn:active { background: var(--fill-press); }
  .icon-btn.active { color: var(--accent); background: color-mix(in srgb, var(--accent) 15%, transparent); }
  .icon-btn[hidden] { display: none; }

  #banner { flex: 0 0 auto; margin: 8px 10px 0; padding: 7px 12px; border-radius: var(--r-card); font-size: 12px; background: color-mix(in srgb, var(--warn) 14%, transparent); color: var(--label); }
  #banner:empty { display: none; }
  #banner div + div { margin-top: 2px; }
  #banner b { font-weight: 600; }

  #body { flex: 1 1 auto; min-height: 0; display: flex; }
  #list-pane { flex: 1 1 auto; min-width: 0; display: flex; flex-direction: column; }
  #detail-pane { flex: 1 1 auto; min-width: 0; overflow-y: auto; display: none; }
  #panel.detail #list-pane { display: none; }
  #panel.detail #detail-pane { display: block; }
  #panel.fullscreen #list-pane { display: flex; flex: 0 0 380px; box-shadow: 0.5px 0 0 var(--hairline); }
  #panel.fullscreen #detail-pane { display: block; }

  #searchbox { flex: 0 0 auto; padding: 10px 10px 6px; display: flex; gap: 6px; align-items: center; }
  #search {
    flex: 1; min-width: 0; height: 30px; padding: 0 12px 0 30px; border-radius: var(--r-capsule);
    border: none; outline: none; color: var(--label); font: inherit;
    background: var(--fill) url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='14' height='14' fill='none' stroke='%238e8e93' stroke-width='1.5' stroke-linecap='round'%3E%3Ccircle cx='6' cy='6' r='4.25'/%3E%3Cpath d='M9.2 9.2L12.5 12.5'/%3E%3C/svg%3E") no-repeat 11px center;
    transition: box-shadow var(--t-fast);
  }
  #search::placeholder { color: var(--label-3); }
  #search:focus { box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 40%, transparent); }
  #list { flex: 1 1 auto; overflow-y: auto; padding-bottom: 24px; outline: none; }

  .section { padding: 14px 18px 5px; font-size: 12px; font-weight: 600; color: var(--label-2); display: flex; gap: 6px; align-items: center; }
  .section .more { margin-left: auto; font-weight: 400; }
  .row {
    position: relative; display: flex; gap: 9px; padding: 9px 12px 9px 12px; cursor: default;
    border-radius: 12px; margin: 1px 8px; transition: background var(--t-fast);
  }
  .row:hover { background: var(--fill-hover); }
  .row.focused { background: var(--fill-press); }
  .row.selected { background: color-mix(in srgb, var(--accent) 18%, transparent); }
  .row.selected.focused { background: color-mix(in srgb, var(--accent) 26%, transparent); }
  #panel.fullscreen .row.open { background: color-mix(in srgb, var(--accent) 12%, transparent); }
  .dot { flex: 0 0 auto; width: 8px; height: 8px; border-radius: 50%; margin-top: 5px; }
  .row.selected .dot { box-shadow: 0 0 0 2px var(--canvas), 0 0 0 3.5px var(--accent); }
  .row .main { flex: 1; min-width: 0; }
  .line1 { display: flex; gap: 6px; align-items: baseline; }
  .from { flex: 1; min-width: 0; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; color: var(--label-2); }
  .row.unread .from { color: var(--label); font-weight: 600; }
  .row.unread .subject { font-weight: 600; }
  .row.unread .dot { box-shadow: 0 0 0 2.5px color-mix(in srgb, var(--accent) 30%, transparent); }
  .count { color: var(--label-3); font-size: 11px; }
  .time { flex-shrink: 0; color: var(--label-3); font-size: 11px; font-variant-numeric: tabular-nums; }
  .subject { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; margin-top: 1px; }
  .preview { color: var(--label-3); overflow: hidden; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; font-size: 12px; line-height: 1.38; margin-top: 2px; }
  .preview .ai { color: var(--accent); }
  .marks { display: flex; gap: 4px; margin-top: 4px; flex-wrap: wrap; }
  .mark { font-size: 10.5px; padding: 0 7px; border-radius: var(--r-capsule); background: var(--fill); color: var(--label-2); line-height: 17px; }
  .mark.accent { background: color-mix(in srgb, var(--accent) 16%, transparent); color: var(--accent); }
  .row .gate { display: flex; gap: 4px; align-self: center; }
  .pill { border: none; border-radius: var(--r-capsule); padding: 3px 11px; background: var(--fill); cursor: pointer; font-size: 11.5px; transition: background var(--t-fast); }
  .pill:hover { background: var(--fill-hover); }
  .pill.primary { background: var(--accent); color: var(--on-accent); font-weight: 500; }
  .group .count { margin-left: 2px; }
  .chev { color: var(--label-3); transition: transform var(--t-fast); width: 8px; text-align: center; }
  .group.open .chev { transform: rotate(90deg); }
  .empty { padding: 48px 24px; text-align: center; color: var(--label-3); line-height: 1.6; }
  .empty .big { font-size: var(--fs-title); font-weight: 600; color: var(--label-2); margin-bottom: 4px; }

  #selbar {
    flex: 0 0 auto; display: none; align-items: center; gap: 2px; margin: 4px 10px 10px; padding: 4px 4px 4px 14px;
    border-radius: var(--r-capsule); background: var(--glass); box-shadow: var(--shadow-float), var(--glass-shine);
  }
  #selbar.open { display: flex; }
  #sel-count { flex: 1; font-size: 12px; font-weight: 600; white-space: nowrap; }

  /* Thread */
  .thread-head {
    position: sticky; top: 0; z-index: 2; padding: 10px 12px 8px; box-shadow: 0 0.5px 0 var(--hairline);
    background: color-mix(in srgb, var(--canvas) 78%, transparent);
    -webkit-backdrop-filter: var(--glass-blur); backdrop-filter: var(--glass-blur);
  }
  .thread-title { display: flex; gap: 6px; align-items: flex-start; }
  .thread-title .subject { white-space: normal; font-family: var(--font-display); font-size: var(--fs-sheet); font-weight: 700; line-height: 1.25; flex: 1; padding-top: 3px; margin: 0; }
  .toolbar { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 8px; align-items: center; }
  .tgroup { display: flex; padding: 2px; border-radius: var(--r-capsule); background: var(--fill); }
  .tgroup .icon-btn { width: 30px; height: 26px; }
  .tgroup .icon-btn:hover { background: var(--card); box-shadow: 0 0 0 0.5px var(--hairline); }
  .msg { margin: 10px 12px; border-radius: var(--r-card); background: var(--card); box-shadow: 0 0 0 0.5px var(--hairline), 0 1px 3px rgba(0, 0, 0, 0.05); overflow: hidden; }
  .msg.collapsed { cursor: pointer; transition: background var(--t-fast); }
  .msg.collapsed:hover { background: color-mix(in srgb, var(--card) 70%, var(--fill-hover)); }
  .msg-head { display: flex; gap: 10px; padding: 11px 14px 4px; align-items: center; }
  .msg.collapsed .msg-head { padding-bottom: 11px; }
  .avatar { flex: 0 0 auto; width: 28px; height: 28px; border-radius: 50%; display: flex; align-items: center; justify-content: center; color: #fff; font-size: 11px; font-weight: 600; letter-spacing: .02em; }
  .msg-head .who { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .msg-head .who b { font-weight: 600; }
  .msg-head .addr { color: var(--label-3); font-size: 12px; }
  .msg.collapsed .msg-head .snip { color: var(--label-3); }
  .msg-to { padding: 0 14px 6px 52px; margin-top: -4px; color: var(--label-3); font-size: 12px; }
  .msg-body { padding: 6px 14px 14px; }
  .msg-text { white-space: pre-wrap; overflow-wrap: anywhere; line-height: 1.5; }
  .msg-text .quote { color: var(--label-3); display: none; }
  .msg-text.quotes .quote { display: inline; }
  .msg-text a, .linkish { color: var(--accent); }
  iframe.mail { width: 100%; border: none; display: block; background: #fff; border-radius: 10px; min-height: 40px; }
  .images-bar { margin: 2px 14px 8px; padding: 5px 6px 5px 12px; border-radius: var(--r-capsule); background: var(--fill); font-size: 12px; color: var(--label-2); display: flex; gap: 6px; align-items: center; flex-wrap: wrap; }
  .images-bar span { flex: 1; }
  .images-bar .pill { background: var(--card); box-shadow: 0 0 0 0.5px var(--hairline); }
  .attachments { display: flex; flex-wrap: wrap; gap: 8px; padding: 0 14px 14px; }
  .att { display: flex; gap: 10px; align-items: center; max-width: 100%; box-sizing: border-box; border: none; border-radius: 12px; padding: 6px 12px 6px 6px; background: var(--fill); cursor: pointer; text-align: left; transition: background var(--t-fast); }
  .att:hover { background: var(--fill-hover); }
  .att .nm, .att .size { display: block; }
  .att .nm { font-size: 12.5px; max-width: 220px; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .att .size { color: var(--label-3); font-size: 11px; }
  .ft { flex: 0 0 auto; width: 30px; height: 30px; border-radius: 8px; display: flex; align-items: center; justify-content: center; color: #fff; font-size: 8.5px; font-weight: 700; letter-spacing: .03em; background: #8e8e93; }
  .ft.pdf { background: #ff3b30; }
  .ft.img { background: #34c759; }
  .ft.doc { background: #0a84ff; }
  .ft.sheet { background: #30a46c; }
  .ft.slide { background: #ff9500; }
  .ft.txt { background: #636366; }
  .quote-toggle { margin: 0 14px 12px; }
  .summary { margin: 10px 12px 0; padding: 9px 12px; border-radius: var(--r-card); background: color-mix(in srgb, var(--accent) 10%, transparent); font-size: 12.5px; line-height: 1.45; }

  /* Composer */
  .composer { margin: 10px 12px 16px; border-radius: var(--r-card); box-shadow: 0 0 0 0.5px var(--hairline), var(--shadow-float); background: var(--card); overflow: hidden; }
  .composer.standalone { margin-top: 12px; }
  .composer .title { padding: 10px 14px 2px; font-size: 12px; font-weight: 600; color: var(--label-2); display: flex; gap: 6px; }
  .composer .title .draft { color: var(--accent); font-weight: 400; }
  .field { display: flex; align-items: center; gap: 8px; padding: 0 14px; box-shadow: 0 0.5px 0 var(--hairline); min-height: 34px; }
  .field label { width: 52px; color: var(--label-3); font-size: 12px; flex-shrink: 0; }
  .field input, .field select { flex: 1; min-width: 0; border: none; outline: none; background: transparent; color: var(--label); font: inherit; height: 32px; }
  .field .toggle-cc { color: var(--label-3); font-size: 12px; cursor: pointer; background: none; border: none; }
  .composer textarea {
    display: block; width: 100%; box-sizing: border-box; min-height: 160px; resize: vertical;
    border: none; outline: none; background: transparent; color: var(--label);
    font: 13px/1.55 var(--font-text); padding: 12px 14px;
  }
  .composer.dragging { box-shadow: 0 0 0 2px var(--accent); }
  .files { display: flex; flex-wrap: wrap; gap: 6px; padding: 0 14px 8px; }
  .files:empty { display: none; }
  .file { background: var(--fill); border-radius: var(--r-capsule); padding: 3px 10px; font-size: 12px; display: flex; gap: 6px; align-items: center; }
  .file button { border: none; background: none; cursor: pointer; color: var(--label-3); padding: 0; }
  .ai-row { display: flex; gap: 6px; padding: 8px 14px; box-shadow: 0 -0.5px 0 var(--hairline); }
  .ai-row input { flex: 1; min-width: 0; border: none; outline: none; background: var(--fill); border-radius: var(--r-capsule); height: 28px; padding: 0 12px; font: inherit; color: var(--label); }
  .actions { display: flex; gap: 6px; padding: 10px 14px; align-items: center; box-shadow: 0 -0.5px 0 var(--hairline); flex-wrap: wrap; }
  .actions .spacer { flex: 1; }
  .btn { border: none; border-radius: var(--r-capsule); padding: 5px 13px; background: var(--fill); cursor: pointer; font-size: 12.5px; transition: background var(--t-fast); }
  .btn:hover { background: var(--fill-hover); }
  .btn.primary { background: var(--accent); color: var(--on-accent); font-weight: 600; }
  .btn.primary:hover { filter: brightness(1.06); }
  .btn.set { color: var(--accent); }
  .error { color: var(--err); padding: 6px 14px; font-size: 12px; }
  .error:empty { display: none; }
  .kbd { color: var(--label-3); font-size: 11px; margin-left: 4px; }
  .btn.primary .kbd { color: var(--on-accent); opacity: .75; }

  /* Popover, undo bar, help */
  #popover { position: fixed; z-index: 50; min-width: 220px; background: var(--glass-thick); -webkit-backdrop-filter: var(--glass-blur); backdrop-filter: var(--glass-blur); border-radius: var(--r-card); box-shadow: var(--shadow-float), var(--glass-shine); padding: 5px; display: none; }
  #popover.open { display: block; }
  #popover .opt { display: flex; gap: 10px; align-items: center; width: 100%; border: none; background: none; text-align: left; padding: 6px 9px; border-radius: 9px; cursor: pointer; }
  #popover .opt:hover, #popover .opt.focused { background: var(--fill-hover); }
  #popover .opt .when { margin-left: auto; color: var(--label-3); font-size: 11.5px; }
  #popover .opt .n { color: var(--label-3); font-size: 11px; width: 10px; }
  #popover .custom { display: flex; gap: 6px; padding: 6px 6px 3px; }
  #popover .custom input { flex: 1; border: none; border-radius: var(--r-capsule); background: var(--fill); color: var(--label); font: inherit; padding: 3px 10px; }
  #popover .title { padding: 4px 9px 6px; font-size: 12px; color: var(--label-2); font-weight: 600; }
  #undo { position: fixed; left: 50%; bottom: 14px; transform: translateX(-50%); z-index: 40; background: var(--label); color: var(--canvas); border-radius: var(--r-capsule); padding: 6px 6px 6px 16px; font-size: 12.5px; display: none; align-items: center; gap: 10px; box-shadow: var(--shadow-float); white-space: nowrap; }
  body.selecting #undo { bottom: 62px; }
  #undo.open { display: flex; }
  #undo button { border: none; border-radius: var(--r-capsule); padding: 4px 12px; background: color-mix(in srgb, var(--canvas) 22%, transparent); color: var(--canvas); cursor: pointer; font-weight: 600; }
  #help { position: fixed; inset: 46px 14px auto 14px; z-index: 45; background: var(--glass-thick); -webkit-backdrop-filter: var(--glass-blur); backdrop-filter: var(--glass-blur); border-radius: var(--r-card); box-shadow: var(--shadow-float), var(--glass-shine); padding: 14px 18px; display: none; columns: 2; column-gap: 24px; font-size: 12px; }
  #help.open { display: block; }
  #help div { display: flex; gap: 8px; padding: 2px 0; break-inside: avoid; }
  #help kbd { min-width: 34px; color: var(--label-2); font: 11px var(--font-mono); }

  /* Attachment preview (READ-4) */
  #preview { position: fixed; inset: 0; z-index: 60; display: none; align-items: center; justify-content: center; padding: 14px; background: rgba(0, 0, 0, 0.28); -webkit-backdrop-filter: blur(6px); backdrop-filter: blur(6px); }
  #preview.open { display: flex; animation: pv-fade var(--t-pop) var(--ease); }
  .pv-card { width: 100%; height: 100%; max-width: 1000px; display: flex; flex-direction: column; border-radius: var(--r-panel); background: var(--glass); box-shadow: var(--shadow-float), var(--glass-shine); overflow: hidden; animation: pv-pop var(--t-pop) var(--spring); }
  .pv-head { flex: 0 0 auto; display: flex; align-items: center; gap: 10px; padding: 10px 10px 10px 12px; }
  .pv-title { flex: 1; min-width: 0; }
  #pv-name { font-weight: 600; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  #pv-meta { color: var(--label-3); font-size: 11px; }
  #pv-body { flex: 1 1 auto; min-height: 0; display: flex; align-items: center; justify-content: center; margin: 0 10px 10px; border-radius: var(--r-card); background: var(--fill); overflow: auto; }
  #pv-body img { max-width: 100%; max-height: 100%; object-fit: contain; }
  #pv-body iframe { align-self: stretch; width: 100%; border: none; background: #fff; }
  #pv-body pre { align-self: stretch; width: 100%; box-sizing: border-box; margin: 0; padding: 14px; font: 12px/1.5 var(--font-mono); white-space: pre-wrap; overflow-wrap: anywhere; }
  .pv-empty { text-align: center; color: var(--label-2); line-height: 1.6; padding: 24px; }
  .pv-empty .ft { width: 56px; height: 56px; border-radius: 14px; font-size: 13px; margin: 0 auto 12px; }
  .pv-empty .btn { margin-top: 12px; }
  @keyframes pv-fade { from { opacity: 0; } }
  @keyframes pv-pop { from { opacity: 0; transform: scale(0.96); } }
</style>
</head>
<body>
<div id="panel">
  <div id="resize-handle" role="separator" aria-label="Resize pane" aria-orientation="vertical" tabindex="0"></div>
  <div id="bar">
    <div class="seg" role="tablist">
      <button class="tab" id="tab-notes" type="button" role="tab">Notes</button>
      <button class="tab active" id="tab-mail" type="button" role="tab" aria-selected="true">Mail <span class="badge" id="unread"></span></button>
    </div>
    <span class="spacer"></span>
    <button class="icon-btn" id="brief-btn" type="button" title="Daily brief (AI)" aria-label="Daily brief" hidden>
      <svg viewBox="0 0 14 14" fill="none"><path d="M7 1.5l1.3 3.2 3.2 1.3-3.2 1.3L7 10.5 5.7 7.3 2.5 6l3.2-1.3z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round"/></svg>
    </button>
    <button class="icon-btn" id="compose-btn" type="button" title="Compose (c)" aria-label="Compose">
      <svg viewBox="0 0 14 14" fill="none"><path d="M9.5 2.5l2 2L5 11H3V9z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round"/></svg>
    </button>
    <button class="icon-btn" id="help-btn" type="button" title="Keyboard (?)" aria-label="Keyboard shortcuts">
      <svg viewBox="0 0 14 14" fill="none"><rect x="1.5" y="3.5" width="11" height="7" rx="1.5" stroke="currentColor" stroke-width="1.1"/><path d="M4 6h.01M6 6h.01M8 6h.01M10 6h.01M4.5 8.2h5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/></svg>
    </button>
    <button class="icon-btn" id="fullscreen-btn" type="button" title="Toggle fullscreen" aria-label="Toggle mail fullscreen">
      <svg class="ic-enter" viewBox="0 0 10 10" fill="none"><path d="M1 4V1h3M9 4V1H6M1 6v3h3M9 6v3H6" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/></svg>
      <svg class="ic-exit" viewBox="0 0 10 10" fill="none" style="display:none"><path d="M4 1v3H1M6 1v3h3M4 9V6H1M6 9V6h3" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/></svg>
    </button>
    <button class="icon-btn" id="close-btn" type="button" title="Hide" aria-label="Hide mail">
      <svg viewBox="0 0 8 8" fill="none" style="width:8px;height:8px"><path d="M1 1L7 7M7 1L1 7" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/></svg>
    </button>
  </div>
  <div id="banner"></div>
  <div id="body">
    <div id="list-pane">
      <div id="searchbox"><input id="search" type="search" placeholder="Search mail  ( / )" spellcheck="false" autocomplete="off"></div>
      <div id="list" tabindex="0" role="listbox" aria-label="Conversations" aria-multiselectable="true"></div>
      <div id="selbar" role="toolbar" aria-label="Selected conversations"></div>
    </div>
    <div id="detail-pane"></div>
  </div>
</div>
<div id="popover" role="menu"></div>
<div id="undo"><span id="undo-text"></span><button id="undo-btn" type="button">Undo</button></div>
<div id="help"></div>
<div id="preview" role="dialog" aria-modal="true" aria-labelledby="pv-name">
  <div class="pv-card">
    <div class="pv-head">
      <span class="ft" id="pv-ft"></span>
      <div class="pv-title"><div id="pv-name"></div><div id="pv-meta"></div></div>
      <button class="icon-btn" id="pv-prev" type="button" title="Previous (←)" aria-label="Previous attachment"><svg viewBox="0 0 14 14" fill="none"><path d="M8.5 3L4.5 7l4 4" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/></svg></button>
      <button class="icon-btn" id="pv-next" type="button" title="Next (→)" aria-label="Next attachment"><svg viewBox="0 0 14 14" fill="none"><path d="M5.5 3l4 4-4 4" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/></svg></button>
      <button class="btn" id="pv-save" type="button" title="Save to Downloads">Download</button>
      <button class="icon-btn" id="pv-close" type="button" title="Close (Esc)" aria-label="Close preview"><svg viewBox="0 0 8 8" fill="none" style="width:9px;height:9px"><path d="M1 1L7 7M7 1L1 7" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg></button>
    </div>
    <div id="pv-body"></div>
  </div>
</div>
<script>
(function() {
  'use strict';
  const $ = function(id) { return document.getElementById(id); };
  const panel = $('panel'), list = $('list'), detail = $('detail-pane'), search = $('search');
  const popover = $('popover'), undoBar = $('undo'), help = $('help');
  const DOT_COLORS = ['#0a84ff', '#ff9f0a', '#30d158', '#bf5af2', '#ff375f', '#64d2ff'];
  const SAVE_DRAFT_MS = 800;
  const UNDO_BAR_MS = 6000;

  const S = {
    ws: null, accounts: [], ai: false, blockImages: false,
    view: null, scheduled: [], drafts: [],
    mode: 'inbox', query: '', hits: [], showTitle: '', showItems: [],
    rows: [], focus: 0, selected: new Set(), expanded: new Set(),
    thread: null, fromNotes: false, composer: null, composerError: '',
    undo: null, sending: new Map(), fullscreen: false, chords: {},
  };

  function ipc(msg) { window.ipc.postMessage(JSON.stringify(msg)); }
  function esc(s) { return String(s == null ? '' : s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;'); }
  function key(item) { return item.account + '\u0000' + item.thread; }
  function account(k) { return S.accounts.find(function(a) { return a.key === k; }); }
  function color(k) {
    const i = S.accounts.findIndex(function(a) { return a.key === k; });
    const a = S.accounts[i];
    return (a && a.color) || DOT_COLORS[Math.max(0, i) % DOT_COLORS.length];
  }
  function now() { return Math.floor(Date.now() / 1000); }

  function fmtTime(ts, long) {
    const d = new Date(ts * 1000), n = new Date();
    const time = d.toLocaleTimeString([], { hour: '2-digit', minute: '2-digit' });
    if (d.toDateString() === n.toDateString()) return long ? 'Today ' + time : time;
    const y = new Date(n); y.setDate(n.getDate() - 1);
    if (d.toDateString() === y.toDateString()) return long ? 'Yesterday ' + time : 'Yesterday';
    const opts = { day: 'numeric', month: 'short' };
    if (d.getFullYear() !== n.getFullYear()) opts.year = 'numeric';
    if (long) { opts.weekday = 'short'; return d.toLocaleDateString([], opts) + ' ' + time; }
    return d.toLocaleDateString([], opts);
  }
  function fmtSize(n) {
    if (n < 1024) return n + ' B';
    if (n < 1024 * 1024) return Math.round(n / 1024) + ' KB';
    return (n / 1024 / 1024).toFixed(1) + ' MB';
  }

  // ── Times for snooze, send later and remind (TRI-4) ─────────────────────────
  function at(days, hour) {
    const d = new Date(); d.setDate(d.getDate() + days); d.setHours(hour, 0, 0, 0);
    return Math.floor(d.getTime() / 1000);
  }
  function laterOptions() {
    const n = new Date(), out = [];
    if (n.getHours() < 20) out.push({ label: 'Later today', at: now() + 3 * 3600 });
    if (n.getHours() < 17) out.push({ label: 'This evening', at: at(0, 18) });
    out.push({ label: 'Tomorrow morning', at: at(1, 8) });
    const dow = n.getDay();
    if (dow >= 1 && dow <= 4) out.push({ label: 'This weekend', at: at(6 - dow, 9) });
    out.push({ label: 'Next week', at: at(((8 - dow) % 7) || 7, 8) });
    return out;
  }
  function remindOptions() {
    return [
      { label: 'In 1 day', after: 86400 },
      { label: 'In 3 days', after: 3 * 86400 },
      { label: 'In 1 week', after: 7 * 86400 },
      { label: 'Don’t remind', after: null },
    ];
  }

  // ── Popover ─────────────────────────────────────────────────────────────────
  let popoverPick = null;
  function openPopover(anchor, title, options, pick, custom) {
    popoverPick = pick;
    let html = '<div class="title">' + esc(title) + '</div>';
    options.forEach(function(o, i) {
      html += '<button class="opt" type="button" data-i="' + i + '"><span class="n">' + (i + 1) + '</span>' +
        esc(o.label) + (o.at ? '<span class="when">' + esc(fmtTime(o.at, true)) + '</span>' : '') + '</button>';
    });
    if (custom) {
      html += '<div class="custom"><input type="datetime-local" id="custom-at"><button class="pill primary" type="button" id="custom-go">Set</button></div>';
    }
    popover.innerHTML = html;
    popover.classList.add('open');
    const r = anchor ? anchor.getBoundingClientRect() : { left: 20, bottom: 80 };
    const w = popover.offsetWidth, h = popover.offsetHeight;
    popover.style.left = Math.max(8, Math.min(r.left, innerWidth - w - 8)) + 'px';
    popover.style.top = Math.max(8, Math.min(r.bottom + 4, innerHeight - h - 8)) + 'px';
    popover.querySelectorAll('.opt').forEach(function(b) {
      b.addEventListener('click', function() { closePopover(); pick(options[Number(b.dataset.i)]); });
    });
    if (custom) {
      $('custom-go').addEventListener('click', function() {
        const v = $('custom-at').value;
        if (!v) return;
        const ts = Math.floor(new Date(v).getTime() / 1000);
        if (ts <= now()) return;
        closePopover();
        pick({ label: 'Custom', at: ts });
      });
    }
    popover._options = options;
  }
  function closePopover() { popover.classList.remove('open'); popoverPick = null; }
  document.addEventListener('mousedown', function(e) {
    if (popover.classList.contains('open') && !popover.contains(e.target)) closePopover();
    if (help.classList.contains('open') && !help.contains(e.target)) help.classList.remove('open');
  });

  // ── Undo bar (TRI-1, WRT-4) ─────────────────────────────────────────────────
  let undoTimer = 0, undoAction = null;
  function showUndo(text, action, ms) {
    $('undo-text').textContent = text;
    undoAction = action;
    $('undo-btn').style.display = action ? '' : 'none';
    undoBar.classList.add('open');
    clearTimeout(undoTimer);
    undoTimer = setTimeout(hideUndo, ms || UNDO_BAR_MS);
  }
  function hideUndo() { undoBar.classList.remove('open'); undoAction = null; }
  $('undo-btn').addEventListener('click', function() { const a = undoAction; hideUndo(); if (a) a(); });

  // ── Triage ──────────────────────────────────────────────────────────────────
  const INVERSE = { done: 'restore', snooze: 'unsnooze', pin: 'unpin', mute: 'unmute', block: 'unblock', to_notes: 'undo_notes', mark_unread: 'mark_read', mark_read: 'mark_unread', unpin: 'pin', accept: 'unaccept', important: 'unclass', other: 'unclass' };
  const LABEL = { done: 'Done', snooze: 'Snoozed', pin: 'Pinned', unpin: 'Unpinned', mute: 'Muted', block: 'Sender blocked', to_notes: 'Added to notes', mark_unread: 'Marked unread', mark_read: 'Marked read', accept: 'Sender accepted', important: 'Moved to Important', other: 'Moved to Other' };
  const REMOVES = { done: 1, snooze: 1, mute: 1, block: 1, to_notes: 1 };

  function act(verb, items, until) {
    if (!items.length) return;
    ipc({ type: 'act', verb: verb, items: items, until: until || null });
    const inverse = INVERSE[verb];
    S.undo = inverse ? { verb: inverse, items: items } : null;
    const n = items.length > 1 ? items.length + ' conversations' : '';
    const label = (LABEL[verb] || verb) + (n ? ' · ' + n : '') + (verb === 'snooze' && until ? ' until ' + fmtTime(until, true) : '');
    showUndo(label, inverse ? undo : null);
    if (REMOVES[verb]) {
      // TRI-3: the next conversation takes focus.
      const keys = new Set(items.map(key));
      const thread = S.thread && keys.has(key(S.thread));
      S.view && dropRows(keys);
      S.selected.clear();
      if (thread) closeThread(true);
      render();
    }
  }
  function undo() {
    const u = S.undo;
    S.undo = null;
    hideUndo();
    if (u) ipc({ type: 'act', verb: u.verb, items: u.items, until: null });
  }
  function dropRows(keys) {
    const v = S.view;
    const keep = function(t) { return !keys.has(key(t)); };
    v.important = v.important.filter(keep);
    v.new_senders = v.new_senders.filter(keep);
    v.other.forEach(function(g) { g.threads = g.threads.filter(keep); });
    v.other = v.other.filter(function(g) { return g.threads.length; });
    S.hits = S.hits.filter(keep);
    S.showItems = S.showItems.filter(keep);
  }
  // Selected rows, else the focused one, else the open thread.
  function targets() {
    if (S.selected.size) return Array.from(S.selected).map(function(k) { const p = k.split('\u0000'); return { account: p[0], thread: p[1] }; });
    if (S.thread && panel.classList.contains('detail') && !S.fullscreen) return [{ account: S.thread.account, thread: S.thread.thread }];
    const r = S.rows[S.focus];
    if (r && r.thread) return [{ account: r.thread.account, thread: r.thread.thread }];
    if (S.thread) return [{ account: S.thread.account, thread: S.thread.thread }];
    return [];
  }
  function snooze(anchor) {
    const items = targets();
    if (!items.length) return;
    openPopover(anchor, 'Snooze until', laterOptions(), function(o) { act('snooze', items, o.at); }, true);
  }

  // ── List (INB) ──────────────────────────────────────────────────────────────
  function rowHtml(t, i, extra) {
    const marks = [];
    if (t.pinned) marks.push('<span class="mark accent">Pinned</span>');
    if (t.returned === 'snooze') marks.push('<span class="mark accent">Back from snooze</span>');
    if (t.returned === 'reminder') marks.push('<span class="mark accent">No reply yet</span>');
    if (t.remind_at) marks.push('<span class="mark">Reminder ' + esc(fmtTime(t.remind_at)) + '</span>');
    if (t.attachment) marks.push('<span class="mark">Attachment</span>');
    const preview = t.summary ? '<span class="ai">✦</span> ' + esc(t.summary) : esc(t.preview);
    const open = S.thread && S.thread.account === t.account && S.thread.thread === t.thread;
    const cls = 'row' + (t.unread ? ' unread' : '') + (S.selected.has(key(t)) ? ' selected' : '') + (i === S.focus ? ' focused' : '') + (open ? ' open' : '');
    return '<div class="' + cls + '" data-i="' + i + '" role="option">' +
      '<span class="dot" style="background:' + esc(color(t.account)) + '"></span>' +
      '<div class="main"><div class="line1"><span class="from">' + esc(t.from_name || t.from_addr) + '</span>' +
      (t.count > 1 ? '<span class="count">' + t.count + '</span>' : '') +
      '<span class="time">' + esc(fmtTime(t.key || t.date)) + '</span></div>' +
      '<div class="subject">' + esc(t.subject || '(no subject)') + '</div>' +
      (preview ? '<div class="preview">' + preview + '</div>' : '') +
      (marks.length ? '<div class="marks">' + marks.join('') + '</div>' : '') +
      '</div>' + (extra || '') + '</div>';
  }

  function render() {
    S.rows = [];
    let html = '';
    const push = function(r) { S.rows.push(r); return S.rows.length - 1; };
    if (!S.accounts.length) {
      list.innerHTML = '<div class="empty"><div class="big">No mail accounts in this workspace</div>' +
        'Add Gmail, iCloud, Private Email or any IMAP account.<br><br>' +
        '<button class="pill primary" type="button" id="add-account">Add account</button></div>';
      $('add-account').addEventListener('click', function() { ipc({ type: 'settings' }); });
      return;
    }
    if (S.mode === 'search' || S.mode === 'show') {
      const items = S.mode === 'search' ? S.hits : S.showItems;
      html += '<div class="section">' + esc(S.mode === 'search' ? 'Results' : S.showTitle) +
        '<span class="more" style="display:flex;gap:4px">' +
        (S.mode === 'search' && S.ai ? '<button class="pill" type="button" id="ask-ai" title="The assistant turns your words into searches">\u2726 Ask AI</button>' : '') +
        (S.mode === 'show' && items.length ? '<button class="pill" type="button" id="done-all" title="Done for every conversation here (E)">Done all</button>' : '') +
        '<button class="pill" type="button" id="back-inbox">Inbox</button></span></div>';
      items.forEach(function(t) { html += rowHtml(t, push({ thread: t })); });
      if (!items.length) html += '<div class="empty">Nothing found.</div>';
    } else if (S.view) {
      const v = S.view;
      if (S.drafts.length) {
        html += '<div class="section">Drafts</div>';
        S.drafts.forEach(function(d) {
          const i = push({ draft: d });
          html += '<div class="row' + (i === S.focus ? ' focused' : '') + '" data-i="' + i + '"><span class="dot" style="background:' + esc(color(d.account)) + '"></span>' +
            '<div class="main"><div class="line1"><span class="from">To: ' + esc(d.to || '—') + '</span></div><div class="subject">' + esc(d.subject || '(no subject)') + '</div></div></div>';
        });
      }
      if (S.scheduled.length) {
        html += '<div class="section">Sending later</div>';
        S.scheduled.forEach(function(s) {
          const i = push({ scheduled: s });
          html += '<div class="row' + (i === S.focus ? ' focused' : '') + '" data-i="' + i + '"><span class="dot" style="background:' + esc(color(s.account)) + '"></span>' +
            '<div class="main"><div class="subject">Scheduled for ' + esc(fmtTime(s.at, true)) + '</div></div>' +
            '<div class="gate"><button class="pill" type="button" data-cancel="' + s.id + '">Cancel</button></div></div>';
        });
      }
      if (v.new_senders.length) {
        html += '<div class="section">New senders</div>';
        v.new_senders.forEach(function(t) {
          const i = push({ thread: t });
          html += rowHtml(t, i, '<div class="gate"><button class="pill primary" type="button" data-gate="accept" data-i="' + i + '">Accept</button>' +
            '<button class="pill" type="button" data-gate="block" data-i="' + i + '">Block</button></div>');
        });
      }
      html += '<div class="section">Important</div>';
      v.important.forEach(function(t) { html += rowHtml(t, push({ thread: t })); });
      if (!v.important.length) {
        html += '<div class="empty"><div class="big">Inbox zero</div>' +
          (v.next_snooze ? 'Next snoozed conversation returns ' + esc(fmtTime(v.next_snooze, true)) + '.' : 'Nothing needs you right now.') + '</div>';
      }
      if (v.other.length) {
        const unread = v.other.reduce(function(n, g) { return n + g.unread; }, 0);
        html += '<div class="section">Other' + (unread ? ' · ' + unread + ' unread' : '') + '</div>';
        v.other.forEach(function(g) {
          const open = S.expanded.has(g.addr);
          const i = push({ group: g });
          html += '<div class="row group' + (open ? ' open' : '') + (g.unread ? ' unread' : '') + (i === S.focus ? ' focused' : '') + '" data-i="' + i + '">' +
            '<span class="chev">›</span><div class="main"><div class="line1"><span class="from">' + esc(g.name || g.addr) + '</span>' +
            '<span class="count">' + g.threads.length + '</span><span class="time">' + esc(fmtTime(g.threads[0].key)) + '</span></div>' +
            '<div class="preview">' + esc(g.threads[0].subject) + '</div></div></div>';
          if (open) g.threads.forEach(function(t) { html += rowHtml(t, push({ thread: t })); });
        });
      }
    } else {
      html = '<div class="empty">Loading…</div>';
    }
    S.focus = Math.max(0, Math.min(S.focus, S.rows.length - 1));
    list.innerHTML = html;
    const back = $('back-inbox');
    if (back) back.addEventListener('click', backToInbox);
    const ask = $('ask-ai');
    if (ask) ask.addEventListener('click', askSearch);
    const all = $('done-all');
    if (all) all.addEventListener('click', doneAll);
    const focused = list.querySelector('.row.focused');
    if (focused) focused.scrollIntoView({ block: 'nearest' });
    renderSelbar();
  }

  list.addEventListener('click', function(e) {
    const cancel = e.target.closest('[data-cancel]');
    if (cancel) { ipc({ type: 'cancel_send', id: Number(cancel.dataset.cancel) }); return; }
    const gate = e.target.closest('[data-gate]');
    if (gate) {
      const r = S.rows[Number(gate.dataset.i)];
      act(gate.dataset.gate === 'accept' ? 'accept' : 'block', [{ account: r.thread.account, thread: r.thread.thread }]);
      return;
    }
    const row = e.target.closest('.row');
    if (!row) return;
    const i = Number(row.dataset.i);
    if (e.metaKey || e.shiftKey) { S.focus = i; toggleSelect(); return; }
    S.focus = i;
    openFocused();
  });

  function toggleSelect() {
    const r = S.rows[S.focus];
    if (!r || !r.thread) return;
    const k = key(r.thread);
    if (S.selected.has(k)) S.selected.delete(k); else S.selected.add(k);
    render();
  }
  function moveFocus(step) {
    if (!S.rows.length) return;
    S.focus = Math.max(0, Math.min(S.rows.length - 1, S.focus + step));
    render();
    const r = S.rows[S.focus];
    if (S.fullscreen && r && r.thread) openThread(r.thread.account, r.thread.thread, false);
  }
  function openFocused() {
    const r = S.rows[S.focus];
    if (!r) return;
    if (r.group) {
      if (S.expanded.has(r.group.addr)) S.expanded.delete(r.group.addr); else S.expanded.add(r.group.addr);
      render();
    } else if (r.thread) {
      openThread(r.thread.account, r.thread.thread, false);
    } else if (r.draft) {
      openComposer(r.draft);
    }
  }
  function backToInbox() {
    S.mode = 'inbox'; S.query = ''; search.value = ''; S.focus = 0;
    ipc({ type: 'inbox' });
    render();
  }

  // ── Thread (READ) ───────────────────────────────────────────────────────────
  function openThread(acc, thread, fromNotes) {
    S.fromNotes = !!fromNotes;
    S.thread = { account: acc, thread: thread, loading: true };
    S.composer = null;
    panel.classList.add('detail');
    detail.innerHTML = '<div class="empty">Loading…</div>';
    ipc({ type: 'thread', account: acc, thread: thread });
  }
  function closeThread(keepFocus) {
    S.thread = null; S.composer = null;
    panel.classList.remove('detail');
    detail.innerHTML = '';
    if (S.fromNotes && !keepFocus) { S.fromNotes = false; ipc({ type: 'back_to_notes' }); return; }
    S.fromNotes = false;
    list.focus();
  }

  const REMOTE = /(?:src|background|srcset)\s*=\s*["']?\s*https?:|url\(\s*["']?\s*https?:/i;

  function frameDoc(html, remote) {
    const csp = "default-src 'none'; style-src 'unsafe-inline'; font-src data:; img-src data:" + (remote ? ' https: http:' : '');
    return '<!DOCTYPE html><html><head><meta charset="utf-8">' +
      '<meta http-equiv="Content-Security-Policy" content="' + csp + '">' +
      '<style>html{margin:0;padding:0;background:#fff}body{margin:0;padding:10px 12px;display:flow-root;background:#fff;color:#1d1d1f;font:13px/1.5 -apple-system,Helvetica,sans-serif;overflow-wrap:anywhere}' +
      'img{max-width:100%;height:auto}table{max-width:100%}' +
      '.gmail_quote,blockquote[type=cite],#divRplyFwdMsg,#appendonsend~*{display:none}' +
      'body.quotes .gmail_quote,body.quotes blockquote[type=cite],body.quotes #divRplyFwdMsg,body.quotes #appendonsend~*{display:block}</style></head><body>' +
      html + '</body></html>';
  }

  function textHtml(text) {
    // Quoted lines (and the "On … wrote:" above them) fold; links work.
    const lines = String(text || '').replace(/\r/g, '').split('\n');
    let out = '', inQuote = false, sig = false;
    lines.forEach(function(line, i) {
      const quoted = /^\s*>/.test(line) || (/wrote:\s*$/.test(line) && /^\s*>/.test(lines[i + 1] || ''));
      if (/^-- ?$/.test(line)) sig = true;
      if (quoted && !inQuote) { out += '<span class="quote">'; inQuote = true; }
      if (!quoted && inQuote) { out += '</span>'; inQuote = false; }
      const html = esc(line).replace(/(https?:\/\/[^\s<>"]+)/g, '<a href="$1" title="$1">$1</a>');
      out += (sig ? '<span style="color:var(--label-3)">' + html + '</span>' : html) + '\n';
    });
    if (inQuote) out += '</span>';
    return out;
  }

  function hasQuote(m) {
    return m.html ? /gmail_quote|blockquote[^>]*type=["']?cite|divRplyFwdMsg|appendonsend/i.test(m.html) : /^\s*>/m.test(m.text || '');
  }

  // Remote images load unless the user blocks them (READ-2), then per
  // message or per sender.
  function imagesOn(t, m, i) {
    return !S.blockImages || (t.images || []).indexOf(m.from.addr) >= 0 || (t.shown && t.shown.has(i));
  }
  function initials(a) {
    const words = (a.name || a.addr || '?').replace(/[^\p{L}\p{N}\s]/gu, ' ').trim().split(/\s+/);
    const first = words[0] || '?', last = words.length > 1 ? words[words.length - 1] : '';
    return (Array.from(first)[0] + (last ? Array.from(last)[0] : '')).toUpperCase();
  }
  function avatar(a) {
    let h = 0;
    for (const c of a.addr || '') h = (h * 31 + c.charCodeAt(0)) >>> 0;
    return '<span class="avatar" aria-hidden="true" style="background:' + DOT_COLORS[h % DOT_COLORS.length] + '">' + esc(initials(a)) + '</span>';
  }

  const I = {
    back: '<svg viewBox="0 0 14 14" fill="none"><path d="M8.5 3L4.5 7l4 4" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/></svg>',
    done: '<svg viewBox="0 0 14 14" fill="none"><path d="M3 7.5l2.5 2.5L11 4.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>',
    snooze: '<svg viewBox="0 0 14 14" fill="none"><circle cx="7" cy="7.5" r="4.5" stroke="currentColor" stroke-width="1.2"/><path d="M7 5v2.7l1.8 1" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/></svg>',
    pin: '<svg viewBox="0 0 14 14" fill="none"><path d="M5 2h4l-.6 3.5L10.5 8h-7L5.6 5.5z" stroke="currentColor" stroke-width="1.1" stroke-linejoin="round"/><path d="M7 8v4" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/></svg>',
    mute: '<svg viewBox="0 0 14 14" fill="none"><path d="M3 5.5h2l3-2.5v8L5 8.5H3z" stroke="currentColor" stroke-width="1.1" stroke-linejoin="round"/><path d="M10 5.5l2.5 3M12.5 5.5L10 8.5" stroke="currentColor" stroke-width="1.1" stroke-linecap="round"/></svg>',
    block: '<svg viewBox="0 0 14 14" fill="none"><circle cx="7" cy="7" r="4.6" stroke="currentColor" stroke-width="1.2"/><path d="M3.8 10.2l6.4-6.4" stroke="currentColor" stroke-width="1.2"/></svg>',
    notes: '<svg viewBox="0 0 14 14" fill="none"><rect x="2.5" y="2" width="9" height="10" rx="1.5" stroke="currentColor" stroke-width="1.1"/><path d="M4.5 5.5l1 1 2-2M8.5 6h1M4.5 9h5" stroke="currentColor" stroke-width="1.1" stroke-linecap="round"/></svg>',
    unread: '<svg viewBox="0 0 14 14" fill="none"><rect x="2" y="3.5" width="10" height="7" rx="1.2" stroke="currentColor" stroke-width="1.1"/><path d="M2.5 4l4.5 3.3L11.5 4" stroke="currentColor" stroke-width="1.1"/></svg>',
    reply: '<svg viewBox="0 0 14 14" fill="none"><path d="M5.5 4L2.5 7l3 3" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/><path d="M3 7h5a3.5 3.5 0 013.5 3.5V11" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/></svg>',
    all: '<svg viewBox="0 0 14 14" fill="none"><path d="M6.5 4L3.5 7l3 3M4 4L1 7l3 3" stroke="currentColor" stroke-width="1.1" stroke-linecap="round" stroke-linejoin="round"/><path d="M4 7h4.5A3.5 3.5 0 0112 10.5V11" stroke="currentColor" stroke-width="1.1" stroke-linecap="round"/></svg>',
    forward: '<svg viewBox="0 0 14 14" fill="none"><path d="M8.5 4l3 3-3 3" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/><path d="M11 7H6a3.5 3.5 0 00-3.5 3.5V11" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/></svg>',
    spark: '<svg viewBox="0 0 14 14" fill="none"><path d="M7 1.5l1.3 3.2 3.2 1.3-3.2 1.3L7 10.5 5.7 7.3 2.5 6l3.2-1.3z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round"/></svg>',
    tasks: '<svg viewBox="0 0 14 14" fill="none"><path d="M2.5 4l1 1 2-2M2.5 9l1 1 2-2M7.5 4.5h4M7.5 9.5h4" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/></svg>',
    read: '<svg viewBox="0 0 14 14" fill="none"><path d="M2 6.2L7 3l5 3.2V11a.8.8 0 01-.8.8H2.8A.8.8 0 012 11z" stroke="currentColor" stroke-width="1.1" stroke-linejoin="round"/><path d="M2.4 6.5L7 9.3l4.6-2.8" stroke="currentColor" stroke-width="1.1"/></svg>',
    close: '<svg viewBox="0 0 14 14" fill="none"><path d="M4 4l6 6M10 4l-6 6" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/></svg>',
  };
  function iconBtn(attr, id, label, keyHint, svg) {
    return '<button class="icon-btn" type="button" ' + attr + '="' + id + '" title="' + esc(label + (keyHint ? ' (' + keyHint + ')' : '')) + '" aria-label="' + esc(label) + '">' + svg + '</button>';
  }

  function renderThread() {
    const t = S.thread;
    if (!t || !t.messages) return;
    const row = t.row || {};
    const ai = S.ai;
    const btn = function(id, label, keyHint, svg) { return iconBtn('data-act', id, label, keyHint, svg); };
    const group = function(inner) { return '<div class="tgroup">' + inner + '</div>'; };
    let html = '<div class="thread-head"><div class="thread-title">' + btn('back', 'Back', 'Esc', I.back) +
      '<h1 class="subject">' + esc(t.messages[0].subject || '(no subject)') + '</h1></div>' +
      '<div class="toolbar">' +
      group(btn('done', 'Done', 'e', I.done) + btn('snooze', 'Snooze', 'h', I.snooze) +
        btn(row.pinned ? 'unpin' : 'pin', row.pinned ? 'Unpin' : 'Pin', 'p', I.pin) + btn('to_notes', 'To notes', 't', I.notes)) +
      group(btn('mute', 'Mute', 'm', I.mute) + btn('block', 'Block sender', 'b', I.block) + btn('mark_unread', 'Mark unread', 'u', I.unread)) +
      group(btn('reply', 'Reply', 'r', I.reply) + btn('all', 'Reply all', 'a', I.all) + btn('forward', 'Forward', 'f', I.forward)) +
      (ai ? group(btn('summarise', 'Summarise with AI', 's', I.spark) + btn('extract', 'Extract tasks with AI', '', I.tasks)) : '') +
      (row.class === 'other' ? '<button class="pill" type="button" data-act="important">Move sender to Important</button>' : '') +
      (row.class === 'important' ? '<button class="pill" type="button" data-act="other" title="Later mail from this sender goes to Other">Move sender to Other</button>' : '') +
      '</div></div>';
    if (row.summary) html += '<div class="summary"><span style="color:var(--accent)">✦</span> ' + esc(row.summary) + '</div>';
    const last = t.messages.length - 1;
    t.messages.forEach(function(m, i) {
      const open = i === last || !m.seen || t.open && t.open.has(i);
      const who = m.from.name ? '<b>' + esc(m.from.name) + '</b> <span class="addr">' + esc(m.from.addr) + '</span>' : '<b>' + esc(m.from.addr) + '</b>';
      if (!open) {
        const snip = (m.text || '').replace(/\s+/g, ' ').slice(0, 120);
        html += '<div class="msg collapsed" data-open="' + i + '"><div class="msg-head">' + avatar(m.from) + '<span class="who">' + who +
          ' <span class="snip">— ' + esc(snip) + '</span></span><span class="time">' + esc(fmtTime(m.date)) + '</span></div></div>';
        return;
      }
      const recipients = m.to.concat(m.cc).map(function(a) { return a.addr === t.me ? 'me' : (a.name || a.addr); }).join(', ');
      html += '<div class="msg" data-i="' + i + '"><div class="msg-head">' + avatar(m.from) + '<span class="who">' + who + '</span><span class="time">' + esc(fmtTime(m.date, true)) + '</span></div>' +
        (recipients ? '<div class="msg-to">to ' + esc(recipients) + '</div>' : '');
      if (m.html && REMOTE.test(m.html) && !imagesOn(t, m, i)) {
        html += '<div class="images-bar"><span>Remote images are hidden.</span><button class="pill" type="button" data-images="once" data-i="' + i + '">Show</button>' +
          '<button class="pill" type="button" data-images="always" data-i="' + i + '">Always from ' + esc(m.from.name || m.from.addr) + '</button></div>';
      }
      html += '<div class="msg-body">' + (m.html ? '<iframe class="mail" sandbox="allow-same-origin" data-i="' + i + '" title="Message"></iframe>' :
        '<div class="msg-text">' + textHtml(m.text || (m.body_loaded ? '' : 'Loading…')) + '</div>') + '</div>';
      if (hasQuote(m)) html += '<button class="pill quote-toggle" type="button" data-quotes="' + i + '">Show quoted text</button>';
      if (m.attachments && m.attachments.length) {
        html += '<div class="attachments">' + m.attachments.map(function(a, j) {
          const ft = fileType(a);
          return '<button class="att" type="button" data-att="' + i + ':' + j + '" title="Preview ' + esc(a.name) + '"><span class="ft ' + ft.cls + '">' + esc(ft.label) + '</span>' +
            '<span><span class="nm">' + esc(a.name) + '</span><span class="size">' + fmtSize(a.size) + '</span></span></button>';
        }).join('') + '</div>';
      }
      html += '</div>';
    });
    html += '<div id="reply-slot"></div>';
    detail.innerHTML = html;
    detail.querySelectorAll('iframe.mail').forEach(function(f) {
      const i = Number(f.dataset.i), m = t.messages[i];
      f.addEventListener('load', function() { wireFrame(f); });
      f.srcdoc = frameDoc(m.html, imagesOn(t, m, i));
    });
    if (S.composer) renderComposer();
  }

  function wireFrame(f) {
    const doc = f.contentDocument;
    if (!doc) return;
    // The body's own height: the document's is at least the frame's, so it
    // would never shrink below the iframe default.
    const fit = function() { f.style.height = Math.ceil(doc.body.getBoundingClientRect().height) + 'px'; };
    fit();
    doc.addEventListener('load', fit, true);
    setTimeout(fit, 300);
    // READ-3: the full address shows on hover before clicking.
    doc.querySelectorAll('a[href]').forEach(function(a) { a.title = a.getAttribute('href'); });
    doc.addEventListener('click', function(e) {
      const a = e.target.closest && e.target.closest('a[href]');
      if (!a) return;
      e.preventDefault();
      openLink(a.getAttribute('href'));
    });
    doc.addEventListener('keydown', onKey);
    f._fit = fit;
  }

  function openLink(href) {
    if (!href) return;
    if (/^mailto:/i.test(href)) {
      const to = decodeURIComponent(href.slice(7).split('?')[0]);
      compose({ to: to });
      return;
    }
    if (/^https?:\/\//i.test(href)) ipc({ type: 'open_url', url: href });
  }

  detail.addEventListener('click', function(e) {
    const a = e.target.closest('a[href]');
    if (a) { e.preventDefault(); openLink(a.getAttribute('href')); return; }
    const t = S.thread;
    const actBtn = e.target.closest('[data-act]');
    if (actBtn) { threadAction(actBtn.dataset.act, actBtn); return; }
    const collapsed = e.target.closest('.msg.collapsed');
    if (collapsed) { t.open = t.open || new Set(); t.open.add(Number(collapsed.dataset.open)); renderThread(); return; }
    const images = e.target.closest('[data-images]');
    if (images) {
      const i = Number(images.dataset.i), m = t.messages[i];
      t.shown = t.shown || new Set(); t.shown.add(i);
      if (images.dataset.images === 'always') {
        t.images = (t.images || []).concat([m.from.addr]);
        ipc({ type: 'images', account: t.account, thread: t.thread, sender: m.from.addr, always: true });
      }
      renderThread();
      return;
    }
    const q = e.target.closest('[data-quotes]');
    if (q) {
      const i = Number(q.dataset.quotes);
      const frame = detail.querySelector('iframe.mail[data-i="' + i + '"]');
      if (frame && frame.contentDocument) { frame.contentDocument.body.classList.toggle('quotes'); frame._fit && frame._fit(); }
      const text = detail.querySelector('.msg[data-i="' + i + '"] .msg-text');
      if (text) text.classList.toggle('quotes');
      q.textContent = q.textContent === 'Show quoted text' ? 'Hide quoted text' : 'Show quoted text';
      return;
    }
    const att = e.target.closest('[data-att]');
    if (att) {
      const p = att.dataset.att.split(':');
      openPreview(Number(p[0]), Number(p[1]));
    }
  });

  // ── Attachment preview (READ-4) ─────────────────────────────────────────────
  const FILE_KINDS = [
    { cls: 'pdf', exts: ['pdf'], view: 'pdf' },
    { cls: 'img', exts: ['png', 'jpg', 'jpeg', 'gif', 'webp', 'heic', 'bmp', 'svg'], view: 'image' },
    { cls: 'txt', exts: ['txt', 'csv', 'log', 'md', 'json', 'ics', 'vcf', 'xml'], view: 'text' },
    { cls: 'doc', exts: ['doc', 'docx', 'pages', 'rtf', 'odt'] },
    { cls: 'sheet', exts: ['xls', 'xlsx', 'numbers', 'ods'] },
    { cls: 'slide', exts: ['ppt', 'pptx', 'key', 'odp'] },
  ];
  const IMAGE_MIME = { jpg: 'image/jpeg', svg: 'image/svg+xml' };
  function fileType(a) {
    const mime = (a.mime || '').toLowerCase();
    const ext = ((a.name || '').toLowerCase().match(/\.([a-z0-9]{1,7})$/) || [])[1] || '';
    const kind = FILE_KINDS.find(function(k) { return k.exts.indexOf(ext) >= 0; }) ||
      (mime === 'application/pdf' ? FILE_KINDS[0] : /^image\//.test(mime) ? FILE_KINDS[1] : /^text\//.test(mime) ? FILE_KINDS[2] : null);
    const label = (ext || (kind ? kind.cls : 'file')).toUpperCase().slice(0, 4);
    if (!kind) return { cls: '', label: label, view: null };
    const type = kind.view === 'pdf' ? 'application/pdf' : kind.view === 'text' ? 'text/plain' :
      /^image\//.test(mime) ? mime : IMAGE_MIME[ext] || 'image/' + ext;
    return { cls: kind.cls, label: label, view: kind.view || null, mime: type };
  }

  const preview = $('preview'), pvBody = $('pv-body');
  // Contents fetched while this preview is open, by "message:index".
  const PV = { m: null, j: 0, url: '', cache: new Map(), back: null };
  function openPreview(mi, j) {
    const m = S.thread && S.thread.messages[mi];
    if (!m || !m.attachments || !m.attachments[j]) return;
    PV.m = m; PV.j = j; PV.back = document.activeElement;
    preview.classList.add('open');
    renderPreview();
    $('pv-close').focus();
  }
  function closePreview() {
    if (!PV.m) return;
    preview.classList.remove('open');
    dropPreviewUrl();
    pvBody.innerHTML = '';
    PV.m = null; PV.cache.clear();
    if (PV.back && PV.back.isConnected) PV.back.focus(); else list.focus();
  }
  function dropPreviewUrl() {
    if (PV.url) { URL.revokeObjectURL(PV.url); PV.url = ''; }
  }
  function stepPreview(step) {
    const n = PV.m.attachments.length;
    if (n < 2) return;
    PV.j = (PV.j + step + n) % n;
    renderPreview();
  }
  function previewNote(ft, text) {
    return '<div class="pv-empty"><span class="ft ' + ft.cls + '">' + esc(ft.label) + '</span>' + esc(text) +
      '<br><button class="btn primary" type="button" data-pv-save>Download</button></div>';
  }
  function renderPreview() {
    const m = PV.m, a = m.attachments[PV.j], ft = fileType(a), n = m.attachments.length;
    $('pv-ft').className = 'ft ' + ft.cls;
    $('pv-ft').textContent = ft.label;
    $('pv-name').textContent = a.name;
    $('pv-meta').textContent = fmtSize(a.size) + (n > 1 ? ' · ' + (PV.j + 1) + ' of ' + n : '');
    $('pv-prev').hidden = $('pv-next').hidden = n < 2;
    dropPreviewUrl();
    if (!ft.view) { pvBody.innerHTML = previewNote(ft, 'No preview for this kind of file.'); return; }
    const k = m.id + ':' + PV.j;
    if (PV.cache.has(k)) { showPreview(ft, PV.cache.get(k)); return; }
    pvBody.innerHTML = '<div class="pv-empty">Loading…</div>';
    ipc({ type: 'preview', message: m.id, index: PV.j });
  }
  function showPreview(ft, b64) {
    const bin = atob(b64), bytes = new Uint8Array(bin.length);
    for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
    if (ft.view === 'text') { pvBody.innerHTML = '<pre>' + esc(new TextDecoder().decode(bytes)) + '</pre>'; return; }
    PV.url = URL.createObjectURL(new Blob([bytes], { type: ft.mime }));
    pvBody.innerHTML = ft.view === 'image' ? '<img alt="">' : '<iframe title="Preview"></iframe>';
    pvBody.firstChild.src = PV.url;
  }
  function saveAttachment() {
    ipc({ type: 'attachment', message: PV.m.id, index: PV.j });
    showUndo('Saving to Downloads…', null, 2500);
  }
  $('pv-close').addEventListener('click', closePreview);
  $('pv-prev').addEventListener('click', function() { stepPreview(-1); });
  $('pv-next').addEventListener('click', function() { stepPreview(1); });
  $('pv-save').addEventListener('click', saveAttachment);
  preview.addEventListener('click', function(e) {
    if (e.target === preview) closePreview();
    else if (e.target.closest('[data-pv-save]')) saveAttachment();
  });

  // ── Selection (⌘A, x, ⌘-click) ───────────────────────────────────────────────
  const selbar = $('selbar');
  selbar.innerHTML = '<span id="sel-count"></span>' +
    iconBtn('data-sel', 'done', 'Done', 'e', I.done) + iconBtn('data-sel', 'snooze', 'Snooze', 'h', I.snooze) +
    iconBtn('data-sel', 'mark_read', 'Mark read', '', I.read) + iconBtn('data-sel', 'pin', 'Pin', 'p', I.pin) +
    iconBtn('data-sel', 'to_notes', 'To notes', 't', I.notes) + iconBtn('data-sel', 'clear', 'Clear selection', 'Esc', I.close);
  selbar.addEventListener('click', function(e) {
    const b = e.target.closest('[data-sel]');
    if (!b) return;
    const verb = b.dataset.sel;
    if (verb === 'clear') { S.selected.clear(); render(); }
    else if (verb === 'snooze') snooze(b);
    else act(verb, targets());
    list.focus();
  });
  function renderSelbar() {
    const n = S.selected.size;
    selbar.classList.toggle('open', n > 0);
    document.body.classList.toggle('selecting', n > 0);
    $('sel-count').textContent = n + ' selected';
  }
  function selectAll() {
    S.rows.forEach(function(r) { if (r.thread) S.selected.add(key(r.thread)); });
    render();
  }

  function threadAction(id, anchor) {
    const t = S.thread;
    if (!t) return;
    const item = [{ account: t.account, thread: t.thread }];
    switch (id) {
      case 'back': closeThread(); break;
      case 'snooze': snooze(anchor); break;
      case 'reply': case 'all': case 'forward': reply(id); break;
      case 'summarise': ipc({ type: 'ai', kind: 'summarise', item: item[0], text: '' }); showUndo('Asked the assistant', null, 2500); break;
      case 'extract': ipc({ type: 'ai', kind: 'extract', item: item[0], text: '' }); showUndo('Asked the assistant', null, 2500); break;
      default: act(id, item);
    }
  }

  // ── Composer (WRT) ──────────────────────────────────────────────────────────
  function newKey() { return 'new:' + Date.now().toString(36) + Math.random().toString(36).slice(2, 8); }
  function defaultAccount() { return (S.thread && S.thread.account) || (S.accounts[0] && S.accounts[0].key) || ''; }
  function compose(fields) {
    openComposer(Object.assign({ key: newKey(), account: defaultAccount(), mode: 'new', thread: null, message: null, to: '', cc: '', bcc: '', subject: '', body: '', attachments: [] }, fields || {}));
  }
  function reply(mode) {
    const t = S.thread;
    if (!t || !t.messages) return;
    const me = t.me;
    const theirs = t.messages.slice().reverse().find(function(m) { return m.from.addr !== me; }) || t.messages[t.messages.length - 1];
    const subject = theirs.subject || '';
    const d = { key: 'reply:' + t.account + ':' + t.thread, account: t.account, mode: mode, thread: t.thread, message: theirs.id, to: '', cc: '', bcc: '', body: '', attachments: [] };
    if (mode === 'forward') {
      d.key = newKey();
      d.subject = /^fwd?:/i.test(subject) ? subject : 'Fwd: ' + subject;
    } else {
      d.subject = /^re:/i.test(subject) ? subject : 'Re: ' + subject;
      const to = theirs.from.addr === me ? theirs.to.map(function(a) { return a.addr; }) : [(theirs.reply_to || theirs.from).addr];
      d.to = to.join(', ');
      if (mode === 'all') {
        const seen = new Set(to.concat([me]));
        d.cc = theirs.to.concat(theirs.cc).map(function(a) { return a.addr; }).filter(function(a) { if (seen.has(a)) return false; seen.add(a); return true; }).join(', ');
      }
    }
    if (t.draft && mode !== 'forward' && t.draft.key === d.key) Object.assign(d, t.draft, { mode: mode === 'all' ? 'all' : t.draft.mode });
    openComposer(d);
  }
  function openComposer(d) {
    S.composer = d;
    S.composerError = '';
    if (d.thread && (!S.thread || S.thread.thread !== d.thread)) {
      openThread(d.account, d.thread, false);
      S.composer = d;
      return;
    }
    if (!d.thread) { S.thread = null; panel.classList.add('detail'); detail.innerHTML = ''; }
    renderComposer();
  }
  function renderComposer() {
    const d = S.composer;
    if (!d) return;
    const slot = d.thread ? $('reply-slot') : detail;
    if (!slot) return;
    const title = { new: 'New message', reply: 'Reply', all: 'Reply all', forward: 'Forward' }[d.mode] || 'Message';
    const from = S.accounts.map(function(a) {
      return '<option value="' + esc(a.key) + '"' + (a.key === d.account ? ' selected' : '') + '>' + esc((a.name ? a.name + ' <' + a.address + '>' : a.address)) + '</option>';
    }).join('');
    const showCc = d.cc || d.bcc || d._cc;
    slot.innerHTML = '<div class="composer' + (d.thread ? '' : ' standalone') + '" id="composer">' +
      '<div class="title">' + esc(title) + (d._saved ? '<span class="draft">Draft saved</span>' : '') + '</div>' +
      '<div class="field"><label>From</label><select id="c-from">' + from + '</select></div>' +
      '<div class="field"><label>To</label><input id="c-to" value="' + esc(d.to) + '" placeholder="name@example.com, …" autocomplete="off" spellcheck="false">' +
      (showCc ? '' : '<button class="toggle-cc" type="button" id="c-show-cc">Cc Bcc</button>') + '</div>' +
      (showCc ? '<div class="field"><label>Cc</label><input id="c-cc" value="' + esc(d.cc) + '" autocomplete="off" spellcheck="false"></div>' +
        '<div class="field"><label>Bcc</label><input id="c-bcc" value="' + esc(d.bcc) + '" autocomplete="off" spellcheck="false"></div>' : '') +
      '<div class="field"><label>Subject</label><input id="c-subject" value="' + esc(d.subject) + '"></div>' +
      '<textarea id="c-body" placeholder="Write…  **bold**, *italic*, - lists, [links](https://…)">' + esc(d.body) + '</textarea>' +
      '<div class="files" id="c-files">' + (d.attachments || []).map(function(f, i) {
        return '<span class="file">' + esc(f.name) + ' <span class="size">' + fmtSize(Math.floor(f.data.length * 3 / 4)) + '</span><button type="button" data-rm="' + i + '" aria-label="Remove">✕</button></span>';
      }).join('') + '</div>' +
      (S.ai && d.thread ? '<div class="ai-row"><input id="c-intent" placeholder="Draft with AI — e.g. “say yes, propose Tuesday”"><button class="btn" type="button" id="c-ai">✦ Draft</button></div>' : '') +
      '<div class="error" id="c-error">' + esc(S.composerError) + '</div>' +
      '<div class="actions"><button class="btn primary" type="button" id="c-send">Send<span class="kbd">⌘↩</span></button>' +
      '<button class="btn" type="button" id="c-later">Send later</button>' +
      '<button class="btn' + (d._remind ? ' set' : '') + '" type="button" id="c-remind">' + (d._remind ? 'Remind: ' + esc(d._remindLabel) : 'Remind me') + '</button>' +
      '<button class="btn" type="button" id="c-attach">Attach</button><span class="spacer"></span>' +
      '<button class="btn" type="button" id="c-discard">Discard</button></div></div>';
    const fields = ['to', 'cc', 'bcc', 'subject', 'body'];
    fields.forEach(function(f) {
      const el = $('c-' + f);
      if (el) el.addEventListener('input', function() { d[f] = el.value; scheduleDraftSave(); });
    });
    $('c-from').addEventListener('change', function() { d.account = $('c-from').value; scheduleDraftSave(); });
    const cc = $('c-show-cc');
    if (cc) cc.addEventListener('click', function() { d._cc = true; renderComposer(); $('c-cc').focus(); });
    $('c-send').addEventListener('click', function() { send(null); });
    $('c-later').addEventListener('click', function(e) {
      openPopover(e.currentTarget, 'Send later', laterOptions(), function(o) { send(o.at); }, true);
    });
    $('c-remind').addEventListener('click', function(e) {
      openPopover(e.currentTarget, 'Remind me if no reply', remindOptions(), function(o) {
        d._remind = o.after; d._remindLabel = o.label.replace('In ', ''); renderComposer();
      });
    });
    $('c-attach').addEventListener('click', function() { ipc({ type: 'pick_files', key: d.key }); });
    $('c-discard').addEventListener('click', discard);
    const ai = $('c-ai');
    if (ai) ai.addEventListener('click', function() {
      const intent = $('c-intent').value.trim();
      if (!intent) { $('c-intent').focus(); return; }
      ipc({ type: 'ai', kind: 'draft', item: { account: d.account, thread: d.thread }, text: intent });
      showUndo('The assistant is drafting…', null, 4000);
    });
    $('c-files').addEventListener('click', function(e) {
      const rm = e.target.closest('[data-rm]');
      if (rm) { d.attachments.splice(Number(rm.dataset.rm), 1); scheduleDraftSave(); renderComposer(); }
    });
    const box = $('composer');
    box.addEventListener('dragover', function(e) { e.preventDefault(); box.classList.add('dragging'); });
    box.addEventListener('dragleave', function() { box.classList.remove('dragging'); });
    box.addEventListener('drop', function(e) {
      e.preventDefault(); box.classList.remove('dragging');
      Array.from(e.dataTransfer.files || []).forEach(function(file) {
        const reader = new FileReader();
        reader.onload = function() {
          const data = String(reader.result).split(',')[1] || '';
          d.attachments.push({ name: file.name, mime: file.type || 'application/octet-stream', data: data });
          scheduleDraftSave();
          renderComposer();
        };
        reader.readAsDataURL(file);
      });
    });
    const focusField = !d.to ? 'c-to' : 'c-body';
    setTimeout(function() { const el = $(focusField); if (el && document.activeElement.tagName !== 'INPUT') el.focus(); }, 0);
    if (d.thread) box.scrollIntoView({ block: 'nearest' });
  }

  let draftTimer = 0;
  function cleanDraft(d) {
    return { key: d.key, account: d.account, mode: d.mode, thread: d.thread, message: d.message, to: d.to, cc: d.cc, bcc: d.bcc, subject: d.subject, body: d.body, attachments: d.attachments || [] };
  }
  function scheduleDraftSave() {
    clearTimeout(draftTimer);
    const d = S.composer;
    draftTimer = setTimeout(function() {
      if (!d || S.composer !== d) return;
      ipc({ type: 'save_draft', draft: cleanDraft(d) });
      d._saved = true;
      const t = document.querySelector('#composer .title');
      if (t && !t.querySelector('.draft')) t.insertAdjacentHTML('beforeend', '<span class="draft">Draft saved</span>');
    }, SAVE_DRAFT_MS);
  }
  function send(sendAt) {
    const d = S.composer;
    if (!d) return;
    clearTimeout(draftTimer);
    if (!d.to.trim() && !(d.cc || '').trim() && !(d.bcc || '').trim()) { S.composerError = 'Add a recipient.'; renderComposer(); return; }
    ipc({ type: 'send', draft: cleanDraft(d), send_at: sendAt || null, remind_after: d._remind || null });
    S.sending.set(d.key, d);
    closeComposer();
  }
  function closeComposer() {
    const d = S.composer;
    S.composer = null;
    if (d && d.thread) { const slot = $('reply-slot'); if (slot) slot.innerHTML = ''; }
    else if (!S.thread) { panel.classList.remove('detail'); detail.innerHTML = ''; list.focus(); }
  }
  function discard() {
    const d = S.composer;
    if (!d) return;
    clearTimeout(draftTimer);
    ipc({ type: 'delete_draft', key: d.key });
    S.drafts = S.drafts.filter(function(x) { return x.key !== d.key; });
    if (S.thread) S.thread.draft = null;
    closeComposer();
    render();
  }

  // ── Keyboard (Triage table) ─────────────────────────────────────────────────
  function typing(e) {
    const t = e.target;
    return t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.tagName === 'SELECT' || t.isContentEditable);
  }
  function onKey(e) {
    if (e.isComposing) return;
    if (PV.m) {
      const step = { ArrowLeft: -1, ArrowRight: 1 }[e.key];
      if (e.key === 'Escape' || e.key === ' ') { e.preventDefault(); closePreview(); }
      else if (step) { e.preventDefault(); stepPreview(step); }
      return;
    }
    if (popover.classList.contains('open')) {
      if (e.key === 'Escape') { e.preventDefault(); closePopover(); return; }
      const n = Number(e.key);
      const opts = popover._options || [];
      if (n >= 1 && n <= opts.length && !typing(e)) { e.preventDefault(); const pick = popoverPick; closePopover(); pick(opts[n - 1]); }
      return;
    }
    if (help.classList.contains('open') && (e.key === 'Escape' || e.key === '?')) { e.preventDefault(); help.classList.remove('open'); return; }
    if (e.metaKey && e.key === 'Enter' && S.composer) { e.preventDefault(); send(null); return; }
    if (e.metaKey && (e.key === 'z' || e.key === 'Z') && !typing(e) && S.undo) { e.preventDefault(); undo(); return; }
    if (typing(e)) {
      if (e.key === 'Escape') {
        e.preventDefault();
        if (e.target === search) { if (search.value) { search.value = ''; backToInbox(); } list.focus(); }
        else if (S.composer) { closeComposer(); if (S.thread) detail.focus(); }
        else e.target.blur();
      }
      if (e.target === search && e.key === 'Enter') { e.preventDefault(); runSearch(); }
      if (e.target === search && (e.key === 'ArrowDown')) { e.preventDefault(); list.focus(); }
      return;
    }
    // ⌘A selects every conversation listed; in a message it selects text.
    if (e.metaKey && !e.shiftKey && !e.altKey && !e.ctrlKey && (e.key === 'a' || e.key === 'A') &&
        e.target.ownerDocument === document && !detail.contains(e.target) && (!panel.classList.contains('detail') || S.fullscreen)) {
      e.preventDefault(); selectAll(); return;
    }
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    const inThread = S.thread && panel.classList.contains('detail');
    const k = e.key;
    let handled = true;
    switch (k) {
      case 'j': case 'ArrowDown':
        if (inThread && !S.fullscreen) nextThread(1); else moveFocus(1); break;
      case 'k': case 'ArrowUp':
        if (inThread && !S.fullscreen) nextThread(-1); else moveFocus(-1); break;
      case 'Enter': case 'o': if (!inThread || S.fullscreen) openFocused(); else handled = false; break;
      case 'Escape':
        if (S.composer) closeComposer();
        else if (inThread) closeThread();
        else if (S.selected.size) { S.selected.clear(); render(); }
        else if (S.mode !== 'inbox') backToInbox();
        else handled = false;
        break;
      case 'e': act('done', targets()); break;
      case 'E': doneAll(); break;
      case 'h': snooze(null); break;
      case 'p': {
        const items = targets();
        const pinned = inThread ? S.thread.row && S.thread.row.pinned : S.rows[S.focus] && S.rows[S.focus].thread && S.rows[S.focus].thread.pinned;
        act(pinned ? 'unpin' : 'pin', items);
        break;
      }
      case 'm': act('mute', targets()); break;
      case 'b': act('block', targets()); break;
      case 't': act('to_notes', targets()); break;
      case 'u': act('mark_unread', targets()); break;
      case 'x': toggleSelect(); break;
      case 'r': case 'a': case 'f': {
        const mode = { r: 'reply', a: 'all', f: 'forward' }[k];
        if (inThread) reply(mode);
        else { const r = S.rows[S.focus]; if (r && r.thread) { S._pendingReply = mode; openThread(r.thread.account, r.thread.thread, false); } }
        break;
      }
      case 'c': compose(); break;
      case 's': if (S.ai && S.thread) threadAction('summarise'); else handled = false; break;
      case '/': search.focus(); search.select(); break;
      case '?': renderHelp(); help.classList.toggle('open'); break;
      default: handled = false;
    }
    if (handled) e.preventDefault();
  }
  document.addEventListener('keydown', onKey);

  function nextThread(step) {
    const i = S.rows.findIndex(function(r) { return r.thread && S.thread && key(r.thread) === key(S.thread); });
    let j = i + step;
    while (j >= 0 && j < S.rows.length && !S.rows[j].thread) j += step;
    if (j < 0 || j >= S.rows.length) return;
    S.focus = j;
    openThread(S.rows[j].thread.account, S.rows[j].thread.thread, false);
  }

  function renderHelp() {
    const keys = [
      ['j / k', 'Next / previous'], ['↩', 'Open'], ['Esc', 'Back'], ['e', 'Done'], ['h', 'Snooze'], ['p', 'Pin'],
      ['m', 'Mute'], ['b', 'Block sender'], ['t', 'To notes'], ['u', 'Mark unread'], ['r', 'Reply'], ['a', 'Reply all'],
      ['f', 'Forward'], ['c', 'Compose'], ['/', 'Search'], ['x', 'Select'], ['⌘Z', 'Undo'], ['⌘↩', 'Send'],
    ];
    if (S.ai) keys.push(['s', 'Summarise (AI)']);
    help.innerHTML = keys.map(function(k) { return '<div><kbd>' + esc(k[0]) + '</kbd>' + esc(k[1]) + '</div>'; }).join('');
  }

  // SRCH-4: a question in plain words goes to the assistant (AI-6).
  function askSearch() {
    const q = search.value.trim() || S.query;
    if (!q) return;
    ipc({ type: 'ai', kind: 'search', item: null, text: q });
    showUndo('Asked the assistant to find it', null, 3000);
  }
  // AI-4: one key applies a brief group's action \u2014 Done for every conversation shown.
  function doneAll() {
    if (S.mode !== 'show' || !S.showItems.length) return;
    act('done', S.showItems.map(function(t) { return { account: t.account, thread: t.thread }; }));
  }

  // ── Search (SRCH) ───────────────────────────────────────────────────────────
  function runSearch() {
    const q = search.value.trim();
    if (!q) { backToInbox(); return; }
    S.mode = 'search'; S.query = q; S.focus = 0;
    // Enter also asks the servers, for mail older than what is kept here.
    ipc({ type: 'search', query: q, server: true });
    list.focus();
  }
  let searchTimer = 0;
  search.addEventListener('input', function() {
    clearTimeout(searchTimer);
    searchTimer = setTimeout(function() { if (search.value.trim()) { S.mode = 'search'; S.query = search.value.trim(); ipc({ type: 'search', query: S.query }); } else backToInbox(); }, 220);
  });

  // ── Bar, pane and resize ────────────────────────────────────────────────────
  $('tab-notes').addEventListener('click', function() { ipc({ type: 'tab', tab: 'notes' }); });
  $('close-btn').addEventListener('click', function() { ipc({ type: 'hide' }); });
  $('fullscreen-btn').addEventListener('click', function() { ipc({ type: 'fullscreen' }); });
  $('compose-btn').addEventListener('click', function() { compose(); });
  $('help-btn').addEventListener('click', function() { renderHelp(); help.classList.toggle('open'); });
  $('brief-btn').addEventListener('click', function() { ipc({ type: 'ai', kind: 'brief', item: null, text: '' }); showUndo('Asked the assistant for your brief', null, 3000); });

  const handle = $('resize-handle');
  let dragPointer = null, dragStartX = 0, dragStartWidth = 0, dragWidth = 0, dragFrame = 0;
  function queueResize(width) {
    dragWidth = Math.max(0, Math.round(width));
    if (dragFrame) return;
    dragFrame = requestAnimationFrame(function() { dragFrame = 0; ipc({ type: 'resize', width: dragWidth }); });
  }
  handle.addEventListener('pointerdown', function(e) {
    if (!e.isPrimary || e.button !== 0) return;
    e.preventDefault();
    dragPointer = e.pointerId; dragStartX = e.screenX; dragStartWidth = Math.round(innerWidth); dragWidth = dragStartWidth;
    handle.setPointerCapture(e.pointerId); handle.classList.add('active'); document.body.classList.add('resizing');
  });
  handle.addEventListener('pointermove', function(e) {
    if (e.pointerId !== dragPointer) return;
    e.preventDefault();
    queueResize(dragStartWidth + dragStartX - e.screenX);
  });
  function finishResize(e) {
    if (e.pointerId !== dragPointer) return;
    e.preventDefault();
    if (e.type !== 'pointercancel') dragWidth = Math.max(0, Math.round(dragStartWidth + dragStartX - e.screenX));
    if (dragFrame) { cancelAnimationFrame(dragFrame); dragFrame = 0; }
    if (handle.hasPointerCapture(e.pointerId)) handle.releasePointerCapture(e.pointerId);
    dragPointer = null; handle.classList.remove('active'); document.body.classList.remove('resizing');
    ipc({ type: 'resize_end', width: dragWidth });
  }
  handle.addEventListener('pointerup', finishResize);
  handle.addEventListener('pointercancel', finishResize);
  handle.addEventListener('dblclick', function(e) { e.preventDefault(); ipc({ type: 'resize_reset' }); });

  window.addEventListener('focus', function() { ipc({ type: 'focus' }); });
  window.addEventListener('blur', function() { ipc({ type: 'blur' }); });

  function renderBanner() {
    const bad = S.accounts.filter(function(a) { return a.state === 'offline' || a.state === 'auth' || a.state === 'nopassword'; });
    $('banner').innerHTML = bad.map(function(a) {
      const what = a.state === 'auth' ? 'sign-in failed' : a.state === 'nopassword' ? 'needs its password in Settings → Mail' : 'offline, retrying';
      return '<div><b>' + esc(a.address) + '</b>: ' + esc(what) + (a.message ? ' — ' + esc(a.message) : '') + '</div>';
    }).join('');
  }

  let refreshTimer = 0;
  function refresh() {
    clearTimeout(refreshTimer);
    refreshTimer = setTimeout(function() {
      if (S.mode === 'search') ipc({ type: 'search', query: S.query });
      else ipc({ type: 'inbox' });
      if (S.thread && !S.thread.loading && !S.composer) ipc({ type: 'thread', account: S.thread.account, thread: S.thread.thread });
    }, 150);
  }

  // ── From Rust ───────────────────────────────────────────────────────────────
  window.__mail = {
    setup: function(data) {
      const switched = data.ws !== S.ws;
      S.ws = data.ws;
      S.accounts = data.accounts || [];
      S.ai = !!data.ai;
      const images = S.blockImages !== !!data.block_images;
      S.blockImages = !!data.block_images;
      $('brief-btn').hidden = !S.ai || !S.accounts.length;
      if (switched) {
        closePreview();
        S.view = null; S.thread = null; S.composer = null; S.mode = 'inbox'; S.selected.clear(); S.expanded.clear(); S.focus = 0;
        panel.classList.remove('detail'); detail.innerHTML = ''; search.value = '';
      } else if (images) renderThread();
      renderBanner();
      render();
      if (S.accounts.length) ipc({ type: 'inbox' });
    },
    status: function(acc, state, message) {
      const a = account(acc);
      if (!a) return;
      a.state = state; a.message = message;
      renderBanner();
    },
    inbox: function(data) {
      S.view = data.view; S.scheduled = data.scheduled || []; S.drafts = data.drafts || [];
      if (S.mode === 'inbox') render();
    },
    search: function(data) {
      if (S.mode !== 'search' || data.query !== S.query) return;
      S.hits = data.hits; render();
    },
    thread: function(data) {
      const t = S.thread;
      if (!t || t.account !== data.account || t.thread !== data.thread) return;
      const keep = { open: t.open, shown: t.shown };
      S.thread = Object.assign(data, keep);
      renderThread();
      if (S._pendingReply) { const m = S._pendingReply; S._pendingReply = null; reply(m); }
      else if (data.draft && !S.composer) { S.composer = Object.assign({}, data.draft); renderComposer(); }
    },
    acted: function() { refresh(); },
    images: function() {},
    preview: function(d) {
      if (d.data && PV.m) PV.cache.set(d.message + ':' + d.index, d.data);
      if (!PV.m || PV.m.id !== d.message || PV.j !== d.index) return;
      const ft = fileType(PV.m.attachments[PV.j]);
      if (d.error) pvBody.innerHTML = previewNote(ft, d.error);
      else showPreview(ft, d.data);
    },
    queued: function(data) {
      const d = S.sending.get(data.key);
      const label = data.later ? 'Scheduled for ' + fmtTime(data.at, true) : 'Sending…';
      showUndo(label, function() { ipc({ type: 'cancel_send', id: data.id }); }, data.later ? UNDO_BAR_MS : Math.max(1000, (data.at - now()) * 1000));
      S.sending.set('id:' + data.id, d);
      S.sending.delete(data.key);
      refresh();
    },
    cancelled: function(data) {
      const d = S.sending.get('id:' + data.id);
      S.sending.delete('id:' + data.id);
      if (!data.ok) { showUndo('Already sent', null, 2500); return; }
      if (d) { ipc({ type: 'save_draft', draft: cleanDraft(d) }); openComposer(d); }
      refresh();
    },
    sendError: function(data) {
      const d = S.sending.get(data.key);
      S.sending.delete(data.key);
      if (d) { S.composer = d; S.composerError = data.error; openComposer(d); S.composerError = data.error; renderComposer(); }
    },
    changed: refresh,
    open: function(acc, thread, fromNotes) { S.mode = 'inbox'; openThread(acc, thread, fromNotes); },
    openDraft: function(draft) { draft.attachments = draft.attachments || []; openComposer(draft); showUndo('Draft ready — review and send', null, 4000); },
    show: function(data) { S.mode = 'show'; S.showTitle = data.title; S.showItems = data.items; S.focus = 0; closeThread(true); render(); },
    files: function(k, files) {
      const d = S.composer;
      if (!d || d.key !== k) return;
      d.attachments = (d.attachments || []).concat(files);
      scheduleDraftSave();
      renderComposer();
    },
    unread: function(n) { $('unread').textContent = n ? String(n) : ''; },
    fullscreen: function(on) {
      S.fullscreen = !!on;
      panel.classList.toggle('fullscreen', S.fullscreen);
      const b = $('fullscreen-btn');
      b.classList.toggle('active', S.fullscreen);
      b.querySelector('.ic-enter').style.display = S.fullscreen ? 'none' : '';
      b.querySelector('.ic-exit').style.display = S.fullscreen ? '' : 'none';
      b.title = (S.fullscreen ? 'Exit fullscreen' : 'Toggle fullscreen') + (S.chords.fullscreen ? ' (' + S.chords.fullscreen + ')' : '');
    },
    focus: function() {
      if (S.composer) { const b = $('c-body'); if (b) { b.focus(); return; } }
      list.focus();
    },
  };

  window.__setShortcuts = function(data) {
    const actions = data && Array.isArray(data.actions) ? data.actions : [];
    const chordFor = function(id) {
      const a = actions.find(function(item) { return item.id === id; });
      return a && Array.isArray(a.keys) ? a.keys.join('') : '';
    };
    S.chords = { inbox: chordFor('inbox'), notes: chordFor('notes'), fullscreen: chordFor('sidebar_fullscreen') };
    $('close-btn').title = 'Hide' + (S.chords.inbox ? ' (' + S.chords.inbox + ')' : '');
    $('tab-notes').title = 'Notes' + (S.chords.notes ? ' (' + S.chords.notes + ')' : '');
    $('tab-mail').title = 'Mail' + (S.chords.inbox ? ' (' + S.chords.inbox + ')' : '');
    window.__mail.fullscreen(S.fullscreen);
  };
  window.__setShortcuts(/*@@KEYBINDINGS_JSON@@*/);
  render();
  ipc({ type: 'ready' });
})();
</script>
</body>
</html>"#
        .replace("/*@@THEME@@*/", crate::theme::CSS)
        .replace("/*@@KEYBINDINGS_JSON@@*/", keybindings_json)
}
