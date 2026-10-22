//! Mail tab of the right-hand pane (spec `specs/001-mail`): the workspace's
//! inbox, a thread, and the composer. Docked it shows one of them at a time;
//! fullscreen puts the list beside the thread.
//!
//! HTML mail renders in a sandboxed iframe without scripts, under a CSP that
//! blocks every remote load until the user allows images (READ-2). Its links,
//! keys and swipes are handled natively in `mail_patch.rs` (READ-3, TRI-11).
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
//!   key({key, shift}), swipe(phase, …), mailto(href) — native input (mail_patch.rs)
//!   draftStream({key, text, done}), draftFailed({key, message}) — the reply writer (AI-3)
//!   summaryStream({item, text, done}), summaryFailed({item, message}) — Summarise (AI-2)
//!   searchReady({ws, id, query}), searchFailed({ws, id, message}) — Ask AI (AI-6)
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

  /* Sized like the assistant sidebar's header beside it: a 32px row, 22px
     pills in 13px text, 24px controls. */
  #bar {
    flex: 0 0 auto; display: flex; align-items: center; gap: 4px;
    height: 32px; padding: 0 10px;
    box-shadow: 0 0.5px 0 var(--hairline);
    font-size: var(--fs-caption); color: var(--label-2);
    -webkit-user-select: none; user-select: none;
  }
  .seg { display: flex; gap: 2px; padding: 2px; border-radius: var(--r-capsule); background: var(--fill); }
  .tab { border: none; background: transparent; height: 18px; padding: 0 10px; border-radius: var(--r-capsule); color: var(--label-2); font-size: 13px; font-weight: 500; cursor: pointer; display: flex; align-items: center; gap: 5px; transition: background var(--t-fast), color var(--t-fast); }
  .tab:hover { color: var(--label); }
  .tab.active { background: var(--card); color: var(--label); box-shadow: 0 0 0 0.5px var(--hairline), 0 1px 3px rgba(0, 0, 0, 0.08); }
  .badge { min-width: 16px; padding: 0 5px; box-sizing: border-box; border-radius: var(--r-capsule); background: var(--accent); color: var(--on-accent); font-size: 10px; font-weight: 600; line-height: 16px; text-align: center; }
  .badge:empty { display: none; }
  #bar .spacer { flex: 1; }
  #bar .icon-btn { width: 24px; height: 24px; border-radius: var(--r-ctl); }
  #bar .icon-btn svg { width: 14px; height: 14px; }
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

  #searchbox { flex: 0 0 auto; position: relative; padding: 10px 10px 6px; display: flex; gap: 6px; align-items: center; }
  #search-key { position: absolute; right: 18px; top: 50%; margin-top: 2px; transform: translateY(-50%); pointer-events: none; font-size: 10.5px; }
  #search:focus ~ #search-key, #search:not(:placeholder-shown) ~ #search-key { display: none; }
  #search {
    flex: 1; min-width: 0; height: 30px; padding: 0 12px 0 30px; border-radius: var(--r-capsule);
    border: none; outline: none; color: var(--label); font: inherit;
    background: var(--fill) url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='14' height='14' fill='none' stroke='%238e8e93' stroke-width='1.5' stroke-linecap='round'%3E%3Ccircle cx='6' cy='6' r='4.25'/%3E%3Cpath d='M9.2 9.2L12.5 12.5'/%3E%3C/svg%3E") no-repeat 11px center;
    transition: box-shadow var(--t-fast);
  }
  #search::placeholder { color: var(--label-3); }
  #search:focus { box-shadow: 0 0 0 3px color-mix(in srgb, var(--accent) 40%, transparent); }
  #list { position: relative; flex: 1 1 auto; overflow-y: auto; overflow-x: hidden; padding-bottom: 24px; outline: none; }

  .section { padding: 14px 18px 5px; font-size: 12px; font-weight: 600; color: var(--label-2); display: flex; gap: 6px; align-items: center; }
  .section .more { margin-left: auto; font-weight: 400; }
  .row {
    position: relative; display: flex; gap: 8px; padding: 8px 12px 9px 9px; cursor: default; color: var(--label);
    border-radius: 10px; margin: 0 8px; transition: background var(--t-fast);
  }
  .row + .row::before { content: ""; position: absolute; top: 0; left: 64px; right: 10px; height: 0.5px; background: var(--hairline); }
  .row:hover::before, .row:hover + .row::before, .row.focused::before, .row.focused + .row::before,
  .row.selected::before, .row.selected + .row::before, .row.open::before, .row.open + .row::before { opacity: 0; }
  .row:hover { background: var(--fill); }
  .row.focused { background: var(--fill-hover); }
  .row.selected { background: color-mix(in srgb, var(--accent) 16%, transparent); }
  #panel.fullscreen .row.open { background: color-mix(in srgb, var(--accent) 12%, transparent); }
  /* The focused list highlights like an AppKit table: accent fill, white text. */
  #list:focus .row.focused, #list:focus .row.selected {
    background: var(--accent); color: var(--on-accent);
    --label: var(--on-accent); --label-2: rgba(255, 255, 255, 0.82); --label-3: rgba(255, 255, 255, 0.66);
    --fill: rgba(255, 255, 255, 0.2); --fill-hover: rgba(255, 255, 255, 0.28);
  }
  #list:focus .row.selected.focused { box-shadow: inset 0 0 0 1.5px rgba(255, 255, 255, 0.55); }
  #list:focus .row.focused .dot, #list:focus .row.selected .dot { background: var(--on-accent) !important; }
  #list:focus .row.focused .avatar, #list:focus .row.selected .avatar { box-shadow: 0 0 0 1.5px rgba(255, 255, 255, 0.75); }
  #list:focus .row.focused .ai, #list:focus .row.selected .ai, #list:focus .row.focused .mark.accent, #list:focus .row.selected .mark.accent { color: var(--on-accent); }
  #list:focus .row.focused .pill.primary { background: var(--on-accent); color: var(--accent); }
  /* TRI-11: a swipe slides the row or thread right, its own background and
     all, uncovering a Done track behind it that turns solid, check popping
     in, once releasing would mark it Done. --swipe is the travel, registered
     so a release animates the element and its track as one. */
  @property --swipe { syntax: '<length>'; inherits: true; initial-value: 0px; }
  @property --swipe-fill { syntax: '<color>'; inherits: false; initial-value: transparent; }
  .swiping { transform: translate3d(var(--swipe), 0, 0); }
  .row.swiping { z-index: 1; }
  .swipe-track {
    position: fixed; z-index: 0; pointer-events: none;
    width: calc(var(--swipe) + var(--swipe-under, 0px)); border-radius: var(--swipe-radius, 0);
    --swipe-fill: color-mix(in srgb, var(--ok) 40%, transparent);
    background:
      url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 14 14'%3E%3Cpath d='M3 7.5l2.5 2.5L11 4.5' fill='none' stroke='white' stroke-width='1.8' stroke-linecap='round' stroke-linejoin='round'/%3E%3C/svg%3E")
      no-repeat max(14px, calc(100% - 36px - var(--swipe-under, 0px))) center / var(--swipe-check, 0px) var(--swipe-fill);
    transition: --swipe-fill var(--t-fast) var(--ease), background-size var(--t-pop) var(--spring);
  }
  /* A row's rounded corners: the track runs on under it. */
  .swipe-track.under { --swipe-under: 12px; --swipe-radius: 10px; }
  .swipe-track.swipe-armed { --swipe-fill: var(--ok); --swipe-check: 18px; }
  /* On the open thread the same swipe means back: a neutral track, a chevron. */
  .swipe-track.back {
    --swipe-fill: var(--fill);
    background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 14 14'%3E%3Cpath d='M8.5 3L4.5 7l4 4' fill='none' stroke='%238e8e93' stroke-width='1.8' stroke-linecap='round' stroke-linejoin='round'/%3E%3C/svg%3E");
  }
  .swipe-track.back.swipe-armed { --swipe-fill: var(--fill-hover); }
  .swiping.swipe-back { transition: --swipe 0.32s var(--spring); }
  .swipe-track.swipe-back { transition: --swipe 0.32s var(--spring), --swipe-fill var(--t-fast) var(--ease), background-size var(--t-pop) var(--spring); }
  .swiping.swipe-out { transition: --swipe 0.2s var(--ease); }
  .swipe-track.swipe-out { transition: --swipe 0.2s var(--ease), --swipe-fill var(--t-fast) var(--ease), background-size var(--t-pop) var(--spring); }
  .row.swipe-out { box-sizing: border-box; }
  .row.swipe-fold, .swipe-track.swipe-fold {
    height: 0 !important; padding-top: 0; padding-bottom: 0; opacity: 0; overflow: hidden;
    transition: height 0.18s var(--ease), padding 0.18s var(--ease), opacity 0.18s var(--ease);
  }
  .dot { flex: 0 0 auto; width: 9px; height: 9px; border-radius: 50%; margin-top: 4px; }
  .row:not(.unread) .dot { visibility: hidden; }
  /* With several accounts a read row keeps a faint dot in its account's color. */
  #panel.multi .row:not(.unread) .dot { visibility: visible; transform: scale(0.55); opacity: 0.6; }
  .row .avatar { width: 30px; height: 30px; font-size: 12px; margin-top: 1px; }
  .row .main { flex: 1; min-width: 0; }
  .line1 { display: flex; gap: 5px; align-items: center; }
  .from { flex: 1; min-width: 0; overflow: hidden; white-space: nowrap; text-overflow: ellipsis; font-weight: 500; }
  .row.unread .from { font-weight: 650; }
  .row.unread .subject { font-weight: 600; }
  .count { flex-shrink: 0; color: var(--label-2); font-size: 10.5px; font-weight: 600; padding: 0 5px; border-radius: var(--r-capsule); background: var(--fill); line-height: 15px; font-variant-numeric: tabular-nums; }
  .clip { flex-shrink: 0; display: flex; color: var(--label-2); }
  .clip svg { width: 11px; height: 11px; }
  .time { flex-shrink: 0; color: var(--label-2); font-size: 11.5px; font-variant-numeric: tabular-nums; }
  .subject { overflow: hidden; white-space: nowrap; text-overflow: ellipsis; margin-top: 1px; }
  .preview { color: var(--label-2); overflow: hidden; display: -webkit-box; -webkit-line-clamp: 2; -webkit-box-orient: vertical; font-size: 12px; line-height: 1.38; margin-top: 2px; }
  .preview .ai { color: var(--accent); }
  .marks { display: flex; gap: 4px; margin-top: 5px; flex-wrap: wrap; }
  .mark { font-size: 10.5px; font-weight: 500; padding: 0 7px; border-radius: var(--r-capsule); background: var(--fill); color: var(--label-2); line-height: 17px; }
  .mark.accent { background: color-mix(in srgb, var(--accent) 16%, transparent); color: var(--accent); }
  .row .gate { display: flex; gap: 4px; align-self: center; }
  .pill { border: none; border-radius: var(--r-capsule); padding: 3px 11px; background: var(--fill); cursor: pointer; font-size: 11.5px; transition: background var(--t-fast); }
  .pill:hover { background: var(--fill-hover); }
  .pill.primary { background: var(--accent); color: var(--on-accent); font-weight: 500; }
  .group .count { margin-left: 2px; }
  .chev { flex: 0 0 auto; align-self: flex-start; width: 9px; height: 17px; line-height: 17px; text-align: center; color: var(--label-3); transition: transform var(--t-fast); }
  .group.open .chev { transform: rotate(90deg); }
  .empty { padding: 48px 24px; text-align: center; color: var(--label-3); line-height: 1.6; }
  .empty .big { font-size: var(--fs-title); font-weight: 600; color: var(--label-2); margin-bottom: 4px; }

  #selbar {
    flex: 0 0 auto; display: none; align-items: center; gap: 2px; margin: 4px 10px 10px; padding: 4px 4px 4px 14px;
    border-radius: var(--r-capsule); background: var(--glass); box-shadow: var(--shadow-float), var(--glass-shine);
  }
  #selbar.open { display: flex; --in-x: 0px; --in-y: 12px; animation: rise-in 0.3s var(--spring) both; }
  #sel-count { flex: 1; font-size: 12px; font-weight: 600; white-space: nowrap; }

  /* Thread */
  .thread-head {
    position: sticky; top: 0; z-index: 2; padding: 10px 12px 8px; box-shadow: 0 0.5px 0 var(--hairline);
    background: var(--canvas);
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
  /* Contacts-style monogram in the sender's own color, hashed from the
     address; machine mail from a verified domain wears that site's icon. */
  .avatar {
    position: relative; flex: 0 0 auto; width: 28px; height: 28px; border-radius: 50%;
    display: flex; align-items: center; justify-content: center;
    color: #fff; font-size: 11px; font-weight: 600; letter-spacing: .02em; text-shadow: 0 0.5px 1px rgba(0, 0, 0, 0.16);
    background: linear-gradient(180deg, var(--av-1, #a6abb6), var(--av-2, #858a94));
    box-shadow: inset 0 0 0 0.5px rgba(0, 0, 0, 0.08), inset 0 0.5px 0 rgba(255, 255, 255, 0.25);
  }
  @media (prefers-color-scheme: dark) {
    .avatar { background: linear-gradient(rgba(0, 0, 0, 0.12), rgba(0, 0, 0, 0.12)), linear-gradient(180deg, var(--av-1, #7a7e87), var(--av-2, #5a5d64)); }
  }
  .avatar svg { width: 14px; height: 14px; }
  .avatar .logo {
    position: absolute; inset: 0; width: 100%; height: 100%; box-sizing: border-box; padding: 21%;
    border-radius: 50%; object-fit: contain; background: #fff; box-shadow: inset 0 0 0 0.5px rgba(0, 0, 0, 0.12);
    opacity: 0; transition: opacity var(--t-fast);
  }
  .avatar .logo.ok { opacity: 1; }
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
  /* Shown once it has its height: no white box flashing, then jumping. */
  iframe.mail { width: 100%; border: none; display: block; background: #fff; border-radius: 10px; min-height: 40px; opacity: 0; transition: opacity 0.16s var(--ease); }
  iframe.mail.plain, iframe.mail.adaptive { background: transparent; }
  iframe.mail.plain { border-radius: 0; }
  iframe.mail.fitted { opacity: 1; }
  .images-bar { margin: 2px 14px 8px; padding: 5px 6px 5px 12px; border-radius: var(--r-capsule); background: var(--fill); font-size: 12px; color: var(--label-2); display: flex; gap: 6px; align-items: center; flex-wrap: wrap; }
  .images-bar span { flex: 1; }
  .images-bar .pill { background: var(--card); box-shadow: 0 0 0 0.5px var(--hairline); }
  .attachments { display: flex; flex-wrap: wrap; gap: 8px; padding: 0 14px 14px; }
  .invite { display: flex; gap: 12px; margin: 0 14px 14px; padding: 12px; border-radius: var(--r-card); background: var(--card); box-shadow: 0 0 0 0.5px var(--hairline), 0 1px 3px rgba(0, 0, 0, 0.06); }
  .invite.pending { color: var(--label-3); }
  .invite.cancelled .inv-title { text-decoration: line-through; color: var(--label-2); }
  .inv-date { flex: 0 0 44px; height: 48px; border-radius: 10px; overflow: hidden; display: flex; flex-direction: column; align-items: center; background: var(--fill); }
  .inv-date .mon { width: 100%; text-align: center; font-size: 10px; font-weight: 700; text-transform: uppercase; color: var(--on-accent); background: var(--err); line-height: 15px; }
  .inv-date .day { font-size: 20px; font-weight: 600; line-height: 32px; }
  .inv-body { flex: 1 1 auto; min-width: 0; }
  .inv-title { font-weight: 600; font-size: 14px; overflow-wrap: anywhere; }
  .inv-when { font-size: 12px; color: var(--label-2); margin-top: 2px; overflow-wrap: anywhere; }
  .inv-actions { display: flex; flex-wrap: wrap; align-items: center; gap: 6px; margin-top: 10px; }
  .inv-note { font-size: 12px; color: var(--label-2); margin-right: 2px; }
  .invite .pill.on, .invite .pill.primary { background: var(--accent); color: var(--on-accent); }
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
  .quote-toggle { margin: 0 14px 12px; font-size: 11.5px; color: var(--label-2); }
  .summary { margin: 10px 12px 0; padding: 9px 12px; border-radius: var(--r-card); background: color-mix(in srgb, var(--accent) 10%, transparent); font-size: 12.5px; line-height: 1.45; }
  .thread-summary { white-space: pre-wrap; overflow-wrap: anywhere; }
  .thread-summary .spark { color: var(--accent); }
  .thread-summary.writing { color: var(--label-2); }

  /* Composer */
  .composer { margin: 10px 12px 16px; border-radius: var(--r-card); box-shadow: 0 0 0 0.5px var(--hairline), var(--shadow-float); background: var(--card); overflow: hidden; }
  .composer.standalone { margin-top: 12px; }
  .composer .title { padding: 10px 14px 2px; font-size: 12px; font-weight: 600; color: var(--label-2); display: flex; gap: 6px; }
  .composer .title .draft { color: var(--accent); font-weight: 400; }
  .field { display: flex; align-items: center; gap: 8px; padding: 0 14px; box-shadow: 0 0.5px 0 var(--hairline); min-height: 34px; }
  .field label { width: 52px; color: var(--label-3); font-size: 12px; flex-shrink: 0; }
  .field input, .field select { flex: 1; min-width: 0; border: none; outline: none; background: transparent; color: var(--label); font: inherit; height: 32px; padding: 0; margin: 0; }
  .field select {
    -webkit-appearance: none; appearance: none; padding-right: 18px;
    background: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='8' height='12' fill='none' stroke='%238e8e93' stroke-width='1.3' stroke-linecap='round' stroke-linejoin='round'%3E%3Cpath d='M1.5 4.5L4 2l2.5 2.5M1.5 7.5L4 10l2.5-2.5'/%3E%3C/svg%3E") no-repeat right center;
  }
  .field .toggle-cc { color: var(--label-3); font-size: 12px; cursor: pointer; background: none; border: none; }
  .composer textarea {
    display: block; width: 100%; box-sizing: border-box; min-height: 160px; resize: none; overflow: hidden;
    border: none; outline: none; background: transparent; color: var(--label);
    font: 13px/1.55 var(--font-text); padding: 12px 14px;
  }
  .composer.dragging { box-shadow: 0 0 0 2px var(--accent); }
  .files { display: flex; flex-wrap: wrap; gap: 6px; padding: 0 14px 8px; }
  .files:empty { display: none; }
  .file { background: var(--fill); border-radius: var(--r-capsule); padding: 3px 10px; font-size: 12px; display: flex; gap: 6px; align-items: center; }
  .file button { border: none; background: none; cursor: pointer; color: var(--label-3); padding: 0; }
  /* AI-3: the writer lives in the composer, right under the text it writes. */
  /* Labelled, beside Send: an unlabelled sparkle is not read as a button. */
  .btn.write { position: relative; overflow: hidden; color: var(--accent); font-weight: 600; background: color-mix(in srgb, var(--accent) 12%, var(--fill)); }
  .btn.write:hover { background: color-mix(in srgb, var(--accent) 20%, var(--fill)); }
  .btn.write .kbd { color: var(--accent); opacity: .6; }
  /* One sweep of light when a reply opens, then still. */
  .btn.write.intro::after {
    content: ""; position: absolute; inset: 0; pointer-events: none;
    background: linear-gradient(100deg, transparent 30%, rgba(255, 255, 255, 0.65) 50%, transparent 70%);
    transform: translateX(-100%); animation: write-sweep 1.1s var(--ease) 0.35s 1;
  }
  .writer-draft {
    min-height: 160px; padding: 12px 14px; white-space: pre-wrap; overflow-wrap: anywhere;
    font: 13px/1.55 var(--font-text); color: var(--label);
    background: color-mix(in srgb, var(--accent) 6%, transparent);
  }
  /* Lighter while it streams, like any suggestion not yet kept. */
  .writer-draft.writing { color: var(--label-2); }
  .writer-draft .caret, .thread-summary .caret {
    display: inline-block; width: 7px; height: 1.1em; margin-left: 1px; vertical-align: text-bottom;
    border-radius: 2px; background: var(--accent); animation: writer-blink 1s steps(1) infinite;
  }
  .writer { display: flex; flex-wrap: wrap; gap: 6px; align-items: center; padding: 8px 10px 8px 14px; box-shadow: 0 -0.5px 0 var(--hairline); }
  .writer .ask { flex: 1 1 180px; min-width: 0; display: flex; align-items: center; gap: 8px; height: 28px; padding: 0 11px; border-radius: var(--r-capsule); background: var(--fill); }
  .writer .ask input { flex: 1; min-width: 0; border: none; outline: none; background: none; font: inherit; color: var(--label); }
  .writer .spark { color: var(--accent); }
  .writer .status { flex: 1; color: var(--label-2); }
  .writer .err { flex: 1; min-width: 0; color: var(--err); font-size: 12px; }
  .writer .kbd { margin-left: 5px; color: var(--label-3); }
  .writer .btn.primary .kbd { color: var(--on-accent); opacity: .7; }
  .actions { display: flex; gap: 6px; padding: 10px 10px 10px 14px; align-items: center; box-shadow: 0 -0.5px 0 var(--hairline); flex-wrap: wrap; }
  .actions .spacer { flex: 1; }
  .btn { border: none; border-radius: var(--r-capsule); padding: 5px 13px; background: var(--fill); cursor: pointer; font-size: 12.5px; transition: background var(--t-fast); }
  .btn:hover { background: var(--fill-hover); }
  .btn.primary { background: var(--accent); color: var(--on-accent); font-weight: 600; }
  .btn.primary:hover { filter: brightness(1.06); }
  .btn.set { color: var(--accent); }
  .error { color: var(--err); padding: 6px 14px; font-size: 12px; }
  .error:empty { display: none; }
  .btn .kbd { min-width: 0; min-height: 0; padding: 0; margin-left: 5px; background: none; box-shadow: none; color: var(--label-3); }
  .btn.primary .kbd { color: var(--on-accent); opacity: .7; }

  /* Popover, undo bar, help */
  #popover { position: fixed; z-index: 50; min-width: 220px; background: var(--glass-thick); -webkit-backdrop-filter: var(--glass-blur); backdrop-filter: var(--glass-blur); border-radius: var(--r-card); box-shadow: var(--shadow-float), var(--glass-shine); padding: 5px; display: none; }
  #popover.open { display: block; transform-origin: top left; animation: pop-in 0.16s var(--ease); }
  #popover .opt { display: flex; gap: 10px; align-items: center; width: 100%; border: none; background: none; text-align: left; padding: 6px 9px; border-radius: 9px; cursor: pointer; }
  #popover .opt:hover, #popover .opt.focused { background: var(--fill-hover); }
  #popover .opt .when { margin-left: auto; color: var(--label-3); font-size: 11.5px; }
  #popover .opt .n { color: var(--label-3); font-size: 11px; width: 10px; }
  #popover .custom { display: flex; gap: 6px; padding: 6px 6px 3px; }
  #popover .custom input { flex: 1; border: none; border-radius: var(--r-capsule); background: var(--fill); color: var(--label); font: inherit; padding: 3px 10px; }
  #popover .title { padding: 4px 9px 6px; font-size: 12px; color: var(--label-2); font-weight: 600; }
  #undo {
    position: fixed; left: 50%; bottom: 14px; z-index: 40; background: var(--label); color: var(--canvas); border-radius: var(--r-capsule); padding: 6px 6px 6px 16px; font-size: 12.5px; display: flex; align-items: center; gap: 10px; box-shadow: var(--shadow-float); white-space: nowrap;
    opacity: 0; visibility: hidden; pointer-events: none; transform: translate(-50%, 12px) scale(0.96);
    transition: opacity 0.16s var(--ease), transform 0.16s var(--ease), visibility 0s 0.16s, bottom 0.24s var(--ease);
  }
  body.selecting #undo { bottom: 62px; }
  #undo.open {
    opacity: 1; visibility: visible; pointer-events: auto; transform: translate(-50%, 0);
    transition: opacity 0.18s var(--ease), transform 0.32s var(--spring), visibility 0s, bottom 0.24s var(--ease);
  }
  /* A new action while it shows: the bar answers with a small pop. */
  #undo.bump { animation: undo-bump 0.26s var(--spring); }
  #undo button { border: none; border-radius: var(--r-capsule); padding: 4px 12px; background: color-mix(in srgb, var(--canvas) 22%, transparent); color: var(--canvas); cursor: pointer; font-weight: 600; }
  #help { position: fixed; inset: 46px 14px auto 14px; z-index: 45; background: var(--glass-thick); -webkit-backdrop-filter: var(--glass-blur); backdrop-filter: var(--glass-blur); border-radius: var(--r-card); box-shadow: var(--shadow-float), var(--glass-shine); padding: 14px 18px; display: none; columns: 2; column-gap: 24px; font-size: 12px; }
  #help.open { display: block; transform-origin: top right; animation: pop-in var(--t-pop) var(--ease); }
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
  @keyframes writer-blink { 50% { opacity: 0; } }
  @keyframes write-sweep { to { transform: translateX(100%); } }

  /* Motion: things move the way they came. The thread slides in over the
     list and the list back over it; the next conversation rises into place,
     the previous one drops in; rows glide when others leave (render). The
     theme's reduced-motion rule stops all of it. */
  @keyframes fade-in { from { opacity: 0; } }
  @keyframes rise-in { from { opacity: 0; transform: translate3d(var(--in-x, 0px), var(--in-y, 0px), 0); } }
  @keyframes pop-in { from { opacity: 0; transform: scale(0.96); } }
  @keyframes ghost-out { to { opacity: 0; transform: scale(0.97); } }
  @keyframes undo-bump { 40% { transform: translate(-50%, 0) scale(1.04); } }
  #panel.detail:not(.fullscreen) #detail-pane { --in-x: 24px; --in-y: 0px; animation: rise-in 0.26s var(--ease); }
  #panel:not(.detail):not(.fullscreen) #list-pane { --in-x: -24px; --in-y: 0px; animation: rise-in 0.26s var(--ease); }
  #detail-pane[data-enter="next"] { --in-y: 14px; }
  #detail-pane[data-enter="prev"] { --in-y: -14px; }
  #detail-pane[data-enter="open"] { --in-y: 0px; }
  #detail-pane[data-enter="next"] > :not(.thread-head), #detail-pane[data-enter="next"] .thread-title .subject,
  #detail-pane[data-enter="prev"] > :not(.thread-head), #detail-pane[data-enter="prev"] .thread-title .subject,
  #detail-pane[data-enter="open"] > * { --in-x: 0px; animation: rise-in 0.22s var(--ease) both; }
  #detail-pane[data-enter="fill"] > :not(.thread-head) { animation: fade-in 0.18s var(--ease) both; }
  /* Not loaded yet: say so only if it takes a moment. */
  #panel #detail-pane > .pending { animation: fade-in 0.2s var(--ease) 0.35s both; }
  .msg.expand { animation: fade-in 0.2s var(--ease) both; }
  .composer.enter { --in-x: 0px; --in-y: 10px; animation: rise-in 0.24s var(--ease) both; }
  .row.row-in { animation: fade-in 0.22s var(--ease) both; }
  .row.row-ghost { position: absolute; z-index: 2; margin: 0; pointer-events: none; animation: ghost-out 0.2s var(--ease) forwards; }
  .row.row-ghost::before { display: none; }
</style>
</head>
<body>
<div id="panel">
  <div id="resize-handle" role="separator" aria-label="Resize pane" aria-orientation="vertical" tabindex="0"></div>
  <div id="bar">
    <div class="seg" role="tablist">
      <button class="tab" id="tab-notes" type="button" role="tab">Notes</button>
      <button class="tab active" id="tab-mail" type="button" role="tab" aria-selected="true">Mail <span class="badge" id="unread"></span></button>
      <button class="tab" id="tab-cal" type="button" role="tab">Calendar</button>
    </div>
    <span class="spacer"></span>
    <button class="icon-btn" id="brief-btn" type="button" title="Daily brief (AI)" aria-label="Daily brief" hidden>
      <svg viewBox="0 0 14 14" fill="none"><path d="M7 1.5l1.3 3.2 3.2 1.3-3.2 1.3L7 10.5 5.7 7.3 2.5 6l3.2-1.3z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round"/></svg>
    </button>
    <button class="icon-btn" id="compose-btn" type="button" title="Compose (c)" aria-label="Compose">
      <svg viewBox="0 0 14 14" fill="none"><path d="M11.5 7.5V11a1.5 1.5 0 01-1.5 1.5H3A1.5 1.5 0 011.5 11V4A1.5 1.5 0 013 2.5h3.5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/><path d="M10.6 1.6l1.8 1.8L7.2 8.6 5 9.1l.5-2.2z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round"/></svg>
    </button>
    <button class="icon-btn" id="help-btn" type="button" title="Keyboard (?)" aria-label="Keyboard shortcuts">
      <svg viewBox="0 0 14 14" fill="none"><rect x="1.5" y="3.5" width="11" height="7" rx="1.5" stroke="currentColor" stroke-width="1.1"/><path d="M4 6h.01M6 6h.01M8 6h.01M10 6h.01M4.5 8.2h5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/></svg>
    </button>
    <button class="icon-btn" id="fullscreen-btn" type="button" title="Toggle fullscreen" aria-label="Toggle mail fullscreen">
      <svg class="ic-enter" viewBox="0 0 14 14" fill="none"><path d="M2 5.5V2h3.5M12 5.5V2H8.5M2 8.5V12h3.5M12 8.5V12H8.5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/></svg>
      <svg class="ic-exit" viewBox="0 0 14 14" fill="none" style="display:none"><path d="M5.5 2v3.5H2M8.5 2v3.5H12M5.5 12V8.5H2M8.5 12V8.5H12" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/></svg>
    </button>
    <button class="icon-btn" id="close-btn" type="button" title="Hide" aria-label="Hide mail">
      <svg viewBox="0 0 8 8" fill="none" style="width:8px;height:8px"><path d="M1 1L7 7M7 1L1 7" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/></svg>
    </button>
  </div>
  <div id="banner"></div>
  <div id="body">
    <div id="list-pane">
      <div id="searchbox"><input id="search" type="search" placeholder="Search all mail" spellcheck="false" autocomplete="off"><span class="kbd" id="search-key">/</span></div>
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
  // A conversation acted away stays off the list this long, whatever older
  // inbox reply arrives meanwhile.
  const GONE_MS = 5000;
  const GLIDE_MS = 260, GHOST_MS = 200;
  const REDUCED = matchMedia('(prefers-reduced-motion: reduce)');

  const S = {
    ws: null, accounts: [], ai: false, blockImages: false,
    view: null, scheduled: [], drafts: [],
    mode: 'inbox', query: '', hits: [], searching: false, searchError: null, aiSearch: null,
    showTitle: '', showGroups: [], showEmpty: '', briefing: null,
    rows: [], focus: 0, selected: new Set(), expanded: new Set(),
    thread: null, fromNotes: false, composer: null, composerError: '', writers: new Map(),
    undo: null, sending: new Map(), fullscreen: false, chords: {},
    gone: new Map(), enter: '', focusKey: null,
  };

  function ipc(msg) { window.ipc.postMessage(JSON.stringify(msg)); }
  function esc(s) { return String(s == null ? '' : s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;'); }
  function key(item) { return item.account + '\u0000' + item.thread; }
  // A list row's identity across redraws, for focus and motion.
  function rowKey(r) {
    return !r ? '' : r.thread ? 't' + key(r.thread) : r.group ? 'g' + r.group.addr : r.draft ? 'd' + r.draft.key : r.scheduled ? 's' + r.scheduled.id : '';
  }
  function rowAttr(k) { return esc(encodeURIComponent(k)); }
  function account(k) { return S.accounts.find(function(a) { return a.key === k; }); }
  function color(k) {
    const i = S.accounts.findIndex(function(a) { return a.key === k; });
    const a = S.accounts[i];
    return (a && a.color) || DOT_COLORS[Math.max(0, i) % DOT_COLORS.length];
  }
  function now() { return Math.floor(Date.now() / 1000); }

  function fmtTime(ts, long) {
    const d = new Date(ts * 1000), n = new Date();
    const time = d.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' });
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
    if (undoBar.classList.contains('open')) { undoBar.classList.remove('bump'); void undoBar.offsetWidth; undoBar.classList.add('bump'); }
    undoBar.classList.add('open');
    clearTimeout(undoTimer);
    undoTimer = setTimeout(hideUndo, ms || UNDO_BAR_MS);
  }
  function hideUndo() { undoBar.classList.remove('open'); undoAction = null; }
  $('undo-btn').addEventListener('click', function() { const a = undoAction; hideUndo(); if (a) a(); });
  // ⌘Z takes back what the undo bar offers — a send or a scheduled send
  // too, until it leaves — else the last triage action.
  function undoKey() {
    if (undoAction) { const a = undoAction; hideUndo(); a(); return true; }
    if (S.undo) { undo(); return true; }
    return false;
  }

  // ── Triage ──────────────────────────────────────────────────────────────────
  const INVERSE = { done: 'restore', snooze: 'unsnooze', pin: 'unpin', mute: 'unmute', block: 'unblock', spam: 'unspam', to_notes: 'undo_notes', mark_unread: 'mark_read', mark_read: 'mark_unread', unpin: 'pin', accept: 'unaccept', important: 'unclass', other: 'unclass' };
  const LABEL = { done: 'Done', snooze: 'Snoozed', pin: 'Pinned', unpin: 'Unpinned', mute: 'Muted', block: 'Sender blocked', spam: 'Reported as spam', to_notes: 'Added to notes', mark_unread: 'Marked unread', mark_read: 'Marked read', accept: 'Sender accepted', important: 'Moved to Important', other: 'Moved to Other' };
  const REMOVES = { done: 1, snooze: 1, mute: 1, block: 1, spam: 1, to_notes: 1 };

  function act(verb, items, until) {
    if (!items.length) return;
    ipc({ type: 'act', verb: verb, items: items, until: until || null });
    const inverse = INVERSE[verb];
    S.undo = inverse ? { verb: inverse, items: items } : null;
    const n = items.length > 1 ? items.length + ' conversations' : '';
    const label = (LABEL[verb] || verb) + (n ? ' · ' + n : '') + (verb === 'snooze' && until ? ' until ' + fmtTime(until, true) : '');
    showUndo(label, inverse ? undo : null);
    const keys = new Set(items.map(key));
    if (!REMOVES[verb]) { local(verb, keys); return; }
    // TRI-3: the next conversation takes over — in the open thread it opens
    // in its place, in the list it takes focus.
    keys.forEach(function(k) { S.gone.set(k, Date.now() + GONE_MS); });
    const thread = S.thread && keys.has(key(S.thread));
    const next = thread && !S.fromNotes ? nextAfter(keys) : null;
    dropRows(keys);
    S.selected.clear();
    if (thread) closeThread(true);
    if (next) {
      S.focusKey = 't' + key(next.t);
      openThread(next.t.account, next.t.thread, false, next.dir);
    }
    render();
  }
  // What the server will say anyway, shown now.
  function local(verb, keys) {
    if (verb === 'pin' || verb === 'unpin') touch(keys, function(t) { t.pinned = verb === 'pin'; });
    else if (verb === 'mark_read' || verb === 'mark_unread') touch(keys, function(t) { t.unread = verb === 'mark_unread'; });
    else return;
    render();
    if (S.thread && S.thread.messages && keys.has(key(S.thread)) && headSig(S.thread) !== S.thread.headSig) renderHead();
  }
  // Every listed copy of the conversations in `keys`, changed in place.
  function touch(keys, fn) {
    const each = function(t) { if (keys.has(key(t))) fn(t); };
    const v = S.view;
    if (v) { v.important.forEach(each); v.new_senders.forEach(each); v.other.forEach(function(g) { g.threads.forEach(each); }); }
    S.hits.forEach(each);
    S.showGroups.forEach(function(g) { g.items.forEach(each); });
    if (S.thread && S.thread.row && keys.has(key(S.thread))) fn(S.thread.row);
  }
  // The conversation below the open one, else above, that `keys` leaves,
  // and which way that is.
  function nextAfter(keys) {
    const here = S.rows.findIndex(function(r) { return r.thread && key(r.thread) === key(S.thread); });
    if (here < 0) return null;
    const left = function(j) { const r = S.rows[j]; return r && r.thread && !keys.has(key(r.thread)); };
    for (let j = here + 1; j < S.rows.length; j++) if (left(j)) return { t: S.rows[j].thread, dir: 'next' };
    for (let j = here - 1; j >= 0; j--) if (left(j)) return { t: S.rows[j].thread, dir: 'prev' };
    return null;
  }
  function undo() {
    const u = S.undo;
    S.undo = null;
    hideUndo();
    if (!u) return;
    ipc({ type: 'act', verb: u.verb, items: u.items, until: null });
    const keys = new Set(u.items.map(key));
    keys.forEach(function(k) { S.gone.delete(k); });
    local(u.verb, keys);
  }
  function dropRows(keys) {
    const v = S.view;
    const keep = function(t) { return !keys.has(key(t)); };
    if (v) {
      v.important = v.important.filter(keep);
      v.new_senders = v.new_senders.filter(keep);
      v.other.forEach(function(g) { g.threads = g.threads.filter(keep); });
      v.other = v.other.filter(function(g) { return g.threads.length; });
    }
    S.hits = S.hits.filter(keep);
    S.showGroups.forEach(function(g) { g.items = g.items.filter(keep); });
    S.showGroups = S.showGroups.filter(function(g) { return g.items.length; });
  }
  // Drops what was acted away from a list sent before the server knew.
  function pruneGone() {
    const keys = new Set(), t = Date.now();
    S.gone.forEach(function(until, k) { if (until > t) keys.add(k); else S.gone.delete(k); });
    if (keys.size) dropRows(keys);
  }
  // Selected rows, else the focused one (a sender group: all its
  // conversations, as its swipe), else the open thread.
  function targets() {
    if (S.selected.size) return Array.from(S.selected).map(function(k) { const p = k.split('\u0000'); return { account: p[0], thread: p[1] }; });
    if (S.thread && panel.classList.contains('detail') && !S.fullscreen) return [{ account: S.thread.account, thread: S.thread.thread }];
    const r = S.rows[S.focus];
    if (r && r.thread) return [{ account: r.thread.account, thread: r.thread.thread }];
    if (r && r.group) return r.group.threads.map(function(t) { return { account: t.account, thread: t.thread }; });
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
    const preview = t.summary ? '<span class="ai">✦</span> ' + esc(t.summary) : esc(t.preview);
    const open = S.thread && S.thread.account === t.account && S.thread.thread === t.thread;
    const cls = 'row' + (t.unread ? ' unread' : '') + (S.selected.has(key(t)) ? ' selected' : '') + (i === S.focus ? ' focused' : '') + (open ? ' open' : '');
    return '<div class="' + cls + '" data-i="' + i + '" data-k="' + rowAttr(rowKey(S.rows[i])) + '" role="option">' +
      '<span class="dot" style="background:' + esc(color(t.account)) + '"></span>' + avatar({ name: t.from_name, addr: t.from_addr }, t.brand) +
      '<div class="main"><div class="line1"><span class="from">' + esc(t.from_name || t.from_addr) + '</span>' +
      (t.count > 1 ? '<span class="count">' + t.count + '</span>' : '') +
      (t.attachment ? '<span class="clip" title="Attachment">' + I.clip + '</span>' : '') +
      '<span class="time">' + esc(fmtTime(t.key || t.date)) + '</span></div>' +
      '<div class="subject">' + esc(t.subject || '(no subject)') + '</div>' +
      (preview ? '<div class="preview">' + preview + '</div>' : '') +
      (marks.length ? '<div class="marks">' + marks.join('') + '</div>' : '') +
      '</div>' + (extra || '') + '</div>';
  }

  let listShape = '';
  function render() {
    // The same list redrawn keeps focus on its row while others come and go.
    const shape = [S.ws, S.mode, S.query, S.showTitle].join('\u0001');
    const same = shape === listShape;
    listShape = shape;
    const want = S.focusKey || (same ? rowKey(S.rows[S.focus]) : '');
    S.focusKey = null;
    const before = same ? measureRows() : null;
    S.rows = [];
    let html = '';
    const push = function(r) { S.rows.push(r); return S.rows.length - 1; };
    const attrs = function(i) { return '" data-i="' + i + '" data-k="' + rowAttr(rowKey(S.rows[i])) + '">'; };
    if (!S.accounts.length) {
      listShape = '';
      list.innerHTML = '<div class="empty"><div class="big">No mail accounts in this workspace</div>' +
        'Add Gmail, iCloud, Private Email or any IMAP account.<br><br>' +
        '<button class="pill primary" type="button" id="add-account">Add account</button></div>';
      $('add-account').addEventListener('click', function() { ipc({ type: 'settings' }); });
      return;
    }
    const doneBtn = function(gi) { return '<button class="pill" type="button" data-done-group="' + gi + '" title="Done for every conversation here (E)">Done all</button>'; };
    if (S.mode === 'search') {
      const finding = S.aiSearch && S.aiSearch.phase === 'writing';
      html += '<div class="section">Results · All mail' +
        '<span class="more" style="display:flex;gap:4px">' +
        (S.ai ? '<button class="pill" type="button" id="ask-ai" title="Find mail using your own words"' + (finding ? ' disabled' : '') + '>✦ ' + (finding ? 'Searching…' : 'Ask AI') + '</button>' : '') +
        '<button class="pill" type="button" id="back-inbox">Inbox</button></span></div>';
      if (S.aiSearch && S.aiSearch.phase === 'error') html += '<div class="summary" role="alert">' + esc(S.aiSearch.error) + '</div>';
      if (S.searchError) html += '<div class="summary" role="alert">' + esc(S.searchError) + '</div>';
      if (S.searching) html += '<div class="summary" role="status">Searching all mail…</div>';
      S.hits.forEach(function(t) { html += rowHtml(t, push({ thread: t })); });
      if (!S.hits.length && !S.searching) html += '<div class="empty" role="status">' + (finding ? 'Finding mail…' : 'Nothing found.') + '</div>';
    } else if (S.mode === 'show') {
      // One list under the title, or the brief's groups each under its own (AI-4).
      const groups = S.showGroups;
      const titled = groups.length > 1 || (groups.length === 1 && groups[0].title !== S.showTitle);
      html += '<div class="section">' + esc(S.showTitle) +
        '<span class="more" style="display:flex;gap:4px">' + (groups.length && !titled ? doneBtn(0) : '') +
        '<button class="pill" type="button" id="back-inbox">Inbox</button></span></div>';
      if (S.briefing && S.briefing.phase === 'writing') html += '<div class="summary" role="status">Sorting your inbox…</div>';
      if (S.briefing && S.briefing.phase === 'error') html += '<div class="summary" role="alert">' + esc(S.briefing.error) + '</div>';
      groups.forEach(function(g, gi) {
        if (titled) html += '<div class="section">' + esc(g.title) + ' · ' + g.items.length + '<span class="more">' + doneBtn(gi) + '</span></div>';
        g.items.forEach(function(t) { html += rowHtml(t, push({ thread: t, show: gi })); });
      });
      if (!groups.length && !S.briefing) html += '<div class="empty" role="status">' + esc(S.showEmpty || 'Nothing found.') + '</div>';
    } else if (S.view) {
      const v = S.view;
      if (S.drafts.length) {
        html += '<div class="section">Drafts</div>';
        S.drafts.forEach(function(d) {
          const i = push({ draft: d });
          html += '<div class="row' + (i === S.focus ? ' focused' : '') + attrs(i) + '<span class="dot" style="background:' + esc(color(d.account)) + '"></span>' + avatar(recipient(d.to)) +
            '<div class="main"><div class="line1"><span class="from">To: ' + esc(d.to || '—') + '</span></div><div class="subject">' + esc(d.subject || '(no subject)') + '</div></div></div>';
        });
      }
      if (S.scheduled.length) {
        html += '<div class="section">Sending later</div>';
        S.scheduled.forEach(function(s) {
          const i = push({ scheduled: s });
          html += '<div class="row' + (i === S.focus ? ' focused' : '') + attrs(i) + '<span class="dot" style="background:' + esc(color(s.account)) + '"></span>' + '<span class="avatar" aria-hidden="true">' + I.snooze + '</span>' +
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
          html += '<div class="row group' + (open ? ' open' : '') + (g.unread ? ' unread' : '') + (i === S.focus ? ' focused' : '') + attrs(i) +
            '<span class="chev">›</span>' + avatar(g, g.threads[0].brand) + '<div class="main"><div class="line1"><span class="from">' + esc(g.name || g.addr) + '</span>' +
            '<span class="count">' + g.threads.length + '</span><span class="time">' + esc(fmtTime(g.threads[0].key)) + '</span></div>' +
            '<div class="preview">' + esc(g.threads[0].subject) + '</div></div></div>';
          if (open) g.threads.forEach(function(t) { html += rowHtml(t, push({ thread: t })); });
        });
      }
    } else {
      html = '<div class="empty">Loading…</div>';
    }
    const built = S.focus;
    const found = want ? S.rows.findIndex(function(r) { return rowKey(r) === want; }) : -1;
    S.focus = Math.max(0, Math.min(found >= 0 ? found : S.focus, S.rows.length - 1));
    list.innerHTML = html;
    if (S.focus !== built) {
      const old = list.querySelector('.row.focused');
      if (old) old.classList.remove('focused');
      const el = list.querySelector('.row[data-i="' + S.focus + '"]');
      if (el) el.classList.add('focused');
    }
    const back = $('back-inbox');
    if (back) back.addEventListener('click', backToInbox);
    const ask = $('ask-ai');
    if (ask) ask.addEventListener('click', askSearch);
    list.querySelectorAll('[data-done-group]').forEach(function(b) {
      b.addEventListener('click', function() { doneAll(Number(b.dataset.doneGroup)); });
    });
    const focused = list.querySelector('.row.focused');
    if (focused) focused.scrollIntoView({ block: 'nearest' });
    glide(before);
    renderSelbar();
  }

  // Where each row sat before a redraw, by identity.
  function measureRows() {
    const m = new Map();
    if (REDUCED.matches) return m;
    list.querySelectorAll('.row[data-k]').forEach(function(el) {
      m.set(el.dataset.k, { el: el, top: el.offsetTop, left: el.offsetLeft, width: el.offsetWidth });
    });
    return m;
  }
  // Rows that stayed glide from where they were, new ones fade in, and those
  // that left fade out where they were, so nothing jumps.
  function glide(before) {
    if (!before || !before.size) return;
    const moved = [], added = [];
    list.querySelectorAll('.row[data-k]').forEach(function(el) {
      const old = before.get(el.dataset.k);
      if (!old) { added.push(el); return; }
      before.delete(el.dataset.k);
      const dy = old.top - el.offsetTop;
      if (Math.abs(dy) >= 1) moved.push([el, dy]);
    });
    added.forEach(function(el) { el.classList.add('row-in'); });
    before.forEach(function(old) {
      const g = old.el;
      g.removeAttribute('data-i'); g.removeAttribute('data-k');
      g.classList.add('row-ghost');
      g.style.top = old.top + 'px'; g.style.left = old.left + 'px'; g.style.width = old.width + 'px';
      list.appendChild(g);
      setTimeout(function() { g.remove(); }, GHOST_MS);
    });
    if (!moved.length) return;
    moved.forEach(function(p) { p[0].style.transition = 'none'; p[0].style.transform = 'translateY(' + p[1] + 'px)'; });
    void list.offsetHeight;
    moved.forEach(function(p) { p[0].style.transition = 'transform ' + GLIDE_MS + 'ms var(--ease)'; p[0].style.transform = ''; });
    setTimeout(function() { moved.forEach(function(p) { p[0].style.transition = ''; }); }, GLIDE_MS);
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
    if (!r || !r.thread) return;
    if (S.fullscreen) openThread(r.thread.account, r.thread.thread, false, step > 0 ? 'next' : 'prev');
    else prefetchSoon(r.thread);
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
    S.mode = 'inbox'; S.query = ''; S.aiSearch = null; S.searching = false; S.searchError = null; search.value = ''; S.focus = 0;
    ipc({ type: 'inbox' });
    render();
  }

  // ── Thread (READ) ───────────────────────────────────────────────────────────
  // Conversations as last sent: one opens at once, and its fresh copy only
  // redraws what changed. Those beside the open one load ahead (peek: they
  // stay unread), so moving on never waits.
  const THREADS_MAX = 40, ASK_AGAIN_MS = 10000;
  const threads = new Map(), asked = new Map();
  function remember(data) {
    const k = key(data);
    threads.delete(k); threads.set(k, data); asked.delete(k);
    if (threads.size > THREADS_MAX) threads.delete(threads.keys().next().value);
  }
  function prefetch(t) {
    const k = key(t);
    if (threads.has(k) || Date.now() - (asked.get(k) || 0) < ASK_AGAIN_MS) return;
    asked.set(k, Date.now());
    ipc({ type: 'thread', account: t.account, thread: t.thread, peek: true });
  }
  let prefetchTimer = 0;
  function prefetchSoon(t) {
    clearTimeout(prefetchTimer);
    prefetchTimer = setTimeout(function() { prefetch(t); }, 150);
  }
  function prefetchAround(k) {
    const i = S.rows.findIndex(function(r) { return r.thread && key(r.thread) === k; });
    if (i < 0) return;
    [1, -1].forEach(function(step) {
      let j = i + step;
      while (S.rows[j] && !S.rows[j].thread) j += step;
      if (S.rows[j]) prefetch(S.rows[j].thread);
    });
  }

  // `enter` is how it arrives: 'next' or 'prev' along the list; else it
  // slides in over the list (docked) or fades in (fullscreen).
  function openThread(acc, thread, fromNotes, enter) {
    const k = acc + '\u0000' + thread;
    S.fromNotes = !!fromNotes;
    if (S.thread && S.thread.messages && key(S.thread) === k) { panel.classList.add('detail'); return; }
    leaveComposer();
    const hit = threads.get(k);
    S.thread = hit ? Object.assign({}, hit) : { account: acc, thread: thread, loading: true };
    S.enter = enter || (S.fullscreen ? 'open' : '');
    panel.classList.add('detail');
    markOpen(k);
    if (hit) { resumeWriter(); renderThread(); } else pendingThread(k);
    ipc({ type: 'thread', account: acc, thread: thread });
  }
  // Its row shows it open and, as opening marks it read (READ-5), read.
  function markOpen(k) {
    list.querySelectorAll('.row.open:not(.group)').forEach(function(el) { el.classList.remove('open'); });
    if (!k) return;
    touch(new Set([k]), function(t) { t.unread = false; });
    const el = list.querySelector('.row[data-k="' + rowAttr('t' + k) + '"]');
    if (el) el.classList.remove('unread');
    if (el) el.classList.add('open');
  }
  // Not loaded yet: its row's subject and the toolbar, so nothing moves when
  // the messages come.
  function pendingThread(k) {
    const r = S.rows.find(function(x) { return x.thread && key(x.thread) === k; });
    detail.dataset.enter = S.enter;
    S.enter = '';
    detail.innerHTML = threadHead(r ? r.thread.subject || '(no subject)' : '', r ? r.thread : {}) +
      '<div class="empty pending">Loading…</div>';
    detail.scrollTop = 0;
    detail._k = k;
    detail._pending = true;
  }
  function closeThread(keepFocus) {
    leaveComposer(); S.thread = null;
    panel.classList.remove('detail');
    detail.innerHTML = '';
    detail._k = null;
    detail.dataset.enter = '';
    markOpen(null);
    if (S.fromNotes && !keepFocus) { S.fromNotes = false; ipc({ type: 'back_to_notes' }); return; }
    S.fromNotes = false;
    list.focus();
  }

  const REMOTE = /(?:src|background|srcset)\s*=\s*["']?\s*https?:|url\(\s*["']?\s*https?:/i;

  // How a mail meets the card, as Apple Mail does it. One that ships its own
  // dark design gets it ('adaptive'). One that paints no background is just
  // text: it reads on the card in the app's colours, as a plain-text one does
  // ('plain'). Any other keeps the white sheet it was designed on ('sheet').
  const DARK_READY = /prefers-color-scheme\s*:\s*dark|color-schemes?["']?\s+content\s*=\s*["'][^"']*dark|color-scheme\s*:[^;}"'<]*dark/i;
  const PAINTED = /\bbgcolor\s*=|\bbackground\s*=|background(?:-color|-image)?\s*:(?!\s*(?:transparent|none|inherit|initial|unset)\b)/i;
  function frameKind(html) { return DARK_READY.test(html) ? 'adaptive' : PAINTED.test(html) ? 'sheet' : 'plain'; }

  function frameDoc(html, remote, kind) {
    const csp = "default-src 'none'; style-src 'unsafe-inline'; font-src data:; img-src data:" + (remote ? ' https: http:' : '');
    // The frame's scheme matches the pane's, or WebKit paints it opaque; the
    // mail's own meta can't set it, as it lands in the body.
    const sheet = kind === 'sheet'
      ? 'html,body{background:#fff}body{padding:10px 12px;color:#1d1d1f}'
      : ':root{color-scheme:light dark}html,body{background:transparent}body{padding:0;color:rgba(0,0,0,.85)}a{color:#0a84ff}' +
        '@media (prefers-color-scheme:dark){body{color:rgba(255,255,255,.85)}a{color:#409cff}}';
    return '<!DOCTYPE html><html><head><meta charset="utf-8">' +
      '<meta http-equiv="Content-Security-Policy" content="' + csp + '">' +
      '<style>html{margin:0;padding:0}body{margin:0;display:flow-root;font:13px/1.5 -apple-system,Helvetica,sans-serif;overflow-wrap:anywhere}' + sheet +
      'img{max-width:100%;height:auto}table{max-width:100%}' +
      '.gmail_quote,blockquote[type=cite],#divRplyFwdMsg,#appendonsend~*{display:none}' +
      'body.quotes .gmail_quote,body.quotes blockquote[type=cite],body.quotes #divRplyFwdMsg,body.quotes #appendonsend~*{display:block}</style></head><body>' +
      html + '</body></html>';
  }

  // On the dark card, text the sender set dark (Gmail's #222, Outlook's
  // black) turns light: same hue, lightness mirrored.
  function readableInDark(doc) {
    if (!window.matchMedia('(prefers-color-scheme: dark)').matches) return;
    const view = doc.defaultView;
    doc.querySelectorAll('[style*="color" i], font[color]').forEach(function(el) {
      const c = (view.getComputedStyle(el).color.match(/[\d.]+/g) || []).map(Number);
      if (c.length < 3) return;
      const r = c[0] / 255, g = c[1] / 255, b = c[2] / 255;
      const max = Math.max(r, g, b), min = Math.min(r, g, b), l = (max + min) / 2;
      if (l >= 0.5) return;
      const d = max - min, s = d ? d / (1 - Math.abs(2 * l - 1)) : 0;
      const h = !d ? 0 : max === r ? ((g - b) / d + 6) % 6 : max === g ? (b - r) / d + 2 : (r - g) / d + 4;
      el.style.setProperty('color', 'hsl(' + Math.round(h * 60) + ',' + Math.round(s * 100) + '%,' + Math.round(Math.min(1 - l, 0.85) * 100) + '%)', 'important');
    });
  }

  // READ-3: bare addresses in a message become links — web, www. and mail.
  const LINK = /\b(?:https?:\/\/|www\.)[^\s<>"`]+|[\w.+-]+@[\w-]+(?:\.[\w-]+)*\.[a-z]{2,}/gi;
  // Sentence punctuation and an unopened closing bracket end the link.
  function trimLink(s) {
    for (;;) {
      const c = s.slice(-1), open = { ')': '(', ']': '[', '}': '{' }[c];
      if (/[.,;:!?*'"]/.test(c) || (open && s.split(open).length < s.split(c).length)) s = s.slice(0, -1);
      else return s;
    }
  }
  function linkHref(s) { return /^www\./i.test(s) ? 'https://' + s : s.indexOf('://') < 0 ? 'mailto:' + s : s; }
  // [text, link, text, link, …, text]: links at the odd indexes.
  function linkParts(text) {
    const parts = [];
    let last = 0;
    text.replace(LINK, function(m, at) {
      const link = trimLink(m);
      if (link.replace(/^(?:https?:\/\/|www\.)/i, '')) { parts.push(text.slice(last, at), link); last = at + link.length; }
      return m;
    });
    parts.push(text.slice(last));
    return parts;
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
      const html = linkParts(line).map(function(p, k) {
        return k % 2 ? '<a href="' + esc(linkHref(p)) + '" title="' + esc(linkHref(p)) + '">' + esc(p) + '</a>' : esc(p);
      }).join('');
      out += (sig ? '<span style="color:var(--label-3)">' + html + '</span>' : html) + '\n';
    });
    if (inQuote) out += '</span>';
    return out;
  }

  // HTML mail often carries its addresses as bare text, too.
  function linkifyFrame(doc) {
    const walk = doc.createTreeWalker(doc.body, NodeFilter.SHOW_TEXT, { acceptNode: function(n) {
      return n.parentElement && n.parentElement.closest('a, script, style, textarea, title') ? NodeFilter.FILTER_REJECT : NodeFilter.FILTER_ACCEPT;
    } });
    const nodes = [];
    for (let n = walk.nextNode(); n; n = walk.nextNode()) { LINK.lastIndex = 0; if (LINK.test(n.data)) nodes.push(n); }
    nodes.forEach(function(n) {
      const parts = linkParts(n.data);
      if (parts.length < 3) return;
      const frag = doc.createDocumentFragment();
      parts.forEach(function(p, k) {
        if (!(k % 2)) { if (p) frag.appendChild(doc.createTextNode(p)); return; }
        const a = doc.createElement('a');
        a.href = linkHref(p);
        a.textContent = p;
        frag.appendChild(a);
      });
      n.parentNode.replaceChild(frag, n);
    });
  }

  function hasQuote(m) {
    return m.html ? /gmail_quote|blockquote[^>]*type=["']?cite|divRplyFwdMsg|appendonsend/i.test(m.html) : /^\s*>/m.test(m.text || '');
  }

  // Remote images load unless the user blocks them (READ-2), then per
  // message or per sender.
  function imagesOn(t, m, i) {
    return !S.blockImages || (t.images || []).indexOf(m.from.addr) >= 0 || (t.shown && t.shown.has(i));
  }
  // A bare address goes by its local part (sienna@x.ai is S, not SX), or by
  // its domain when the local part is a machine's (noreply@crates.io is C).
  function initials(a) {
    const parts = (a.addr || '?').split('@'), domain = (parts[1] || '').split('.');
    const machine = domain.length > 1 && /^(no[-_]?reply|do[-_]?not[-_]?reply|notifications?|mailer-daemon)$/i.test(parts[0]);
    const name = a.name !== a.addr ? a.name : '';
    const words = (name || (machine ? domain[domain.length - 2] : parts[0])).replace(/[^\p{L}\p{N}\s]/gu, ' ').trim().split(/\s+/);
    const first = words[0] || '?', last = words.length > 1 ? words[words.length - 1] : '';
    return (Array.from(first)[0] + (last ? Array.from(last)[0] : '')).toUpperCase();
  }
  // System hues, each a top-to-bottom gradient deep enough for white initials.
  const AVATAR_COLORS = [
    ['#ff7a7c', '#e8383c'], ['#ffa85a', '#f07418'], ['#f5be4a', '#d99400'], ['#62d482', '#2bab4c'],
    ['#45cfc4', '#0fa39c'], ['#52c4ee', '#1597d0'], ['#55a6ff', '#1670ec'], ['#8c83f7', '#5a4de0'],
    ['#d474ee', '#a53ac6'], ['#ff7697', '#e6315f'], ['#c9a383', '#9c7656'],
  ];
  function hue(s) {
    let h = 2166136261;
    for (const c of s.toLowerCase()) h = Math.imul(h ^ c.codePointAt(0), 16777619);
    return AVATAR_COLORS[(h >>> 0) % AVATAR_COLORS.length];
  }
  // Logos come from the favicon cache (octoweb-fav://, see main.rs): true
  // once one loaded, false where none did, so redraws neither flash nor ask again.
  const logos = new Map();
  document.addEventListener('load', function(e) {
    if (!e.target.classList || !e.target.classList.contains('logo')) return;
    e.target.classList.add('ok');
    logos.set(e.target.dataset.brand, true);
  }, true);
  document.addEventListener('error', function(e) {
    if (!e.target.classList || !e.target.classList.contains('logo')) return;
    logos.set(e.target.dataset.brand, false);
    e.target.remove();
  }, true);
  function avatar(a, brand) {
    const c = hue(a.addr || a.name || '');
    const known = brand && logos.get(brand);
    const logo = brand && known !== false ?
      '<img class="logo' + (known ? ' ok' : '') + '" src="octoweb-fav://' + esc(brand) + '" data-brand="' + esc(brand) + '" alt="">' : '';
    return '<span class="avatar" style="--av-1:' + c[0] + ';--av-2:' + c[1] + '" aria-hidden="true">' + esc(initials(a)) + logo + '</span>';
  }
  // A draft's `To:` line as an address, for its avatar.
  function recipient(to) {
    const first = String(to || '').split(',')[0].trim();
    const m = /<([^>]+)>/.exec(first);
    return { name: first.replace(/<[^>]*>/, '').replace(/"/g, '').trim(), addr: m ? m[1] : first };
  }

  const I = {
    back: '<svg viewBox="0 0 14 14" fill="none"><path d="M8.5 3L4.5 7l4 4" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/></svg>',
    done: '<svg viewBox="0 0 14 14" fill="none"><path d="M3 7.5l2.5 2.5L11 4.5" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/></svg>',
    snooze: '<svg viewBox="0 0 14 14" fill="none"><circle cx="7" cy="7.5" r="4.5" stroke="currentColor" stroke-width="1.2"/><path d="M7 5v2.7l1.8 1" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/></svg>',
    pin: '<svg viewBox="0 0 14 14" fill="none"><path d="M5 2h4l-.6 3.5L10.5 8h-7L5.6 5.5z" stroke="currentColor" stroke-width="1.1" stroke-linejoin="round"/><path d="M7 8v4" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/></svg>',
    mute: '<svg viewBox="0 0 14 14" fill="none"><path d="M3 5.5h2l3-2.5v8L5 8.5H3z" stroke="currentColor" stroke-width="1.1" stroke-linejoin="round"/><path d="M10 5.5l2.5 3M12.5 5.5L10 8.5" stroke="currentColor" stroke-width="1.1" stroke-linecap="round"/></svg>',
    block: '<svg viewBox="0 0 14 14" fill="none"><circle cx="7" cy="7" r="4.6" stroke="currentColor" stroke-width="1.2"/><path d="M3.8 10.2l6.4-6.4" stroke="currentColor" stroke-width="1.2"/></svg>',
    spam: '<svg viewBox="0 0 14 14" fill="none"><path d="M5 1.8h4L12.2 5v4L9 12.2H5L1.8 9V5z" stroke="currentColor" stroke-width="1.1" stroke-linejoin="round"/><path d="M7 4.3v3.2" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/><circle cx="7" cy="9.6" r=".75" fill="currentColor"/></svg>',
    notes: '<svg viewBox="0 0 14 14" fill="none"><rect x="2.5" y="2" width="9" height="10" rx="1.5" stroke="currentColor" stroke-width="1.1"/><path d="M4.5 5.5l1 1 2-2M8.5 6h1M4.5 9h5" stroke="currentColor" stroke-width="1.1" stroke-linecap="round"/></svg>',
    unread: '<svg viewBox="0 0 14 14" fill="none"><rect x="2" y="3.5" width="10" height="7" rx="1.2" stroke="currentColor" stroke-width="1.1"/><path d="M2.5 4l4.5 3.3L11.5 4" stroke="currentColor" stroke-width="1.1"/></svg>',
    reply: '<svg viewBox="0 0 14 14" fill="none"><path d="M5.5 4L2.5 7l3 3" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/><path d="M3 7h5a3.5 3.5 0 013.5 3.5V11" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/></svg>',
    all: '<svg viewBox="0 0 14 14" fill="none"><path d="M6.5 4L3.5 7l3 3M4 4L1 7l3 3" stroke="currentColor" stroke-width="1.1" stroke-linecap="round" stroke-linejoin="round"/><path d="M4 7h4.5A3.5 3.5 0 0112 10.5V11" stroke="currentColor" stroke-width="1.1" stroke-linecap="round"/></svg>',
    forward: '<svg viewBox="0 0 14 14" fill="none"><path d="M8.5 4l3 3-3 3" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/><path d="M11 7H6a3.5 3.5 0 00-3.5 3.5V11" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/></svg>',
    spark: '<svg viewBox="0 0 14 14" fill="none"><path d="M7 1.5l1.3 3.2 3.2 1.3-3.2 1.3L7 10.5 5.7 7.3 2.5 6l3.2-1.3z" stroke="currentColor" stroke-width="1.2" stroke-linejoin="round"/></svg>',
    tasks: '<svg viewBox="0 0 14 14" fill="none"><path d="M2.5 4l1 1 2-2M2.5 9l1 1 2-2M7.5 4.5h4M7.5 9.5h4" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/></svg>',
    clip: '<svg viewBox="0 0 14 14" fill="none"><path d="M11.2 6.6L7 10.8a2.6 2.6 0 01-3.7-3.7l4.6-4.6a1.7 1.7 0 012.4 2.4L5.8 9.4a.8.8 0 01-1.2-1.2l4-4" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/></svg>',
    trash: '<svg viewBox="0 0 14 14" fill="none"><path d="M2.5 4h9M5.5 4V2.8a.8.8 0 01.8-.8h1.4a.8.8 0 01.8.8V4M3.7 4l.6 7.3a1 1 0 001 .9h3.4a1 1 0 001-.9L10.3 4" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/></svg>',
    read: '<svg viewBox="0 0 14 14" fill="none"><path d="M2 6.2L7 3l5 3.2V11a.8.8 0 01-.8.8H2.8A.8.8 0 012 11z" stroke="currentColor" stroke-width="1.1" stroke-linejoin="round"/><path d="M2.4 6.5L7 9.3l4.6-2.8" stroke="currentColor" stroke-width="1.1"/></svg>',
    close: '<svg viewBox="0 0 14 14" fill="none"><path d="M4 4l6 6M10 4l-6 6" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/></svg>',
  };
  function iconBtn(attr, id, label, keyHint, svg) {
    return '<button class="icon-btn" type="button" ' + attr + '="' + id + '" title="' + esc(label + (keyHint ? ' (' + keyHint + ')' : '')) + '" aria-label="' + esc(label) + '">' + svg + '</button>';
  }

  function threadHead(subject, row) {
    const btn = function(id, label, keyHint, svg) { return iconBtn('data-act', id, label, keyHint, svg); };
    const group = function(inner) { return '<div class="tgroup">' + inner + '</div>'; };
    return '<div class="thread-head"><div class="thread-title">' + btn('back', 'Back', 'Esc', I.back) +
      '<h1 class="subject">' + esc(subject) + '</h1></div>' +
      '<div class="toolbar">' +
      group(btn('done', 'Done', 'e', I.done) + btn('snooze', 'Snooze', 'h', I.snooze) +
        btn(row.pinned ? 'unpin' : 'pin', row.pinned ? 'Unpin' : 'Pin', 'p', I.pin) + btn('to_notes', 'To notes', 't', I.notes)) +
      group(btn('mute', 'Mute', 'm', I.mute) + btn('block', 'Block sender', 'b', I.block) + btn('spam', 'Report spam', '!', I.spam) + btn('mark_unread', 'Mark unread', 'u', I.unread)) +
      group(btn('reply', 'Reply', 'r', I.reply) + btn('all', 'Reply all', 'a', I.all) + btn('forward', 'Forward', 'f', I.forward)) +
      (S.ai ? group(btn('summarise', 'Summarise with AI', 's', I.spark) + btn('extract', 'Extract tasks with AI', '', I.tasks)) : '') +
      (row.class === 'other' ? '<button class="pill" type="button" data-act="important">Move sender to Important</button>' : '') +
      (row.class === 'important' ? '<button class="pill" type="button" data-act="other" title="Later mail from this sender goes to Other">Move sender to Other</button>' : '') +
      '</div></div>';
  }
  // ── Calendar invitations (spec 002-calendar INV-*) ──
  // An invitation's `text/calendar` part becomes a card: the event, and the
  // user's answer when their calendar already has it, else Add to calendar.
  const INVITES = {};
  function isInvite(a) {
    const mime = (a.mime || '').toLowerCase();
    return mime === 'text/calendar' || mime === 'application/ics' || /\.ics$/i.test(a.name || '');
  }
  const INV_MON = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
  const INV_DAY = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
  function invClock(ts) {
    return new Date(ts * 1000).toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' });
  }
  function invWhen(c) {
    if (c.all_day) {
      const a = new Date(c.start_date + 'T00:00'), b = new Date(c.end_date + 'T00:00');
      b.setDate(b.getDate() - 1);
      const day = function(d) { return INV_DAY[d.getDay()] + ' ' + d.getDate() + ' ' + INV_MON[d.getMonth()]; };
      return a.getTime() === b.getTime() ? day(a) + ' · all day' : day(a) + ' – ' + day(b);
    }
    const d = new Date(c.start * 1000);
    return INV_DAY[d.getDay()] + ' ' + d.getDate() + ' ' + INV_MON[d.getMonth()] + ' · ' + invClock(c.start) + ' – ' + invClock(c.end);
  }
  function inviteCard(id, c) {
    if (!c) return '<div class="invite pending" data-inv="' + id + '"><div class="inv-body"><div class="inv-when">Calendar invitation…</div></div></div>';
    if (c.error) return '<div class="invite" data-inv="' + id + '"><div class="inv-body"><div class="inv-when">' + esc(c.error) + '</div></div></div>';
    const d = new Date((c.all_day ? new Date(c.start_date + 'T00:00').getTime() / 1000 : c.start) * 1000);
    const e = c.event;
    let actions = '';
    if (c.cancelled) {
      actions = '<span class="inv-note">This event was cancelled</span>';
    } else if (e) {
      if (e.me) {
        actions = '<span class="inv-note">Going?</span>' + [['accepted', 'Yes'], ['tentative', 'Maybe'], ['declined', 'No']].map(function(r) {
          return '<button class="pill' + (e.me === r[0] ? ' on' : '') + '" type="button" data-inv-rsvp="' + r[0] + '" data-key="' + esc(e.key) + '" data-series="' + (c.recurring ? 1 : 0) + '">' + r[1] + '</button>';
        }).join('');
      } else {
        actions = '<span class="inv-note">In your calendar</span>';
      }
      actions += '<button class="pill" type="button" data-inv-open="' + esc(e.key) + '">Open in Calendar</button>';
    } else {
      actions = '<button class="pill primary" type="button" data-inv-add="' + id + '">Add to calendar</button>';
    }
    if (c.join && !c.cancelled && c.end * 1000 > Date.now()) actions += '<button class="pill" type="button" data-inv-join="' + esc(c.join) + '">Join</button>';
    return '<div class="invite' + (c.cancelled ? ' cancelled' : '') + '" data-inv="' + id + '">' +
      '<div class="inv-date"><span class="mon">' + INV_MON[d.getMonth()] + '</span><span class="day">' + d.getDate() + '</span></div>' +
      '<div class="inv-body"><div class="inv-title">' + esc(c.title) + '</div>' +
      '<div class="inv-when">' + esc(invWhen(c)) + (c.recurring ? ' · repeats' : '') + '</div>' +
      (c.location ? '<div class="inv-when">' + esc(c.location) + '</div>' : '') +
      (c.organizer ? '<div class="inv-when">From ' + esc(c.organizer) + '</div>' : '') +
      '<div class="inv-actions">' + actions + '</div></div></div>';
  }
  function inviteHtml(m, j) {
    if (!(m.id in INVITES)) {
      INVITES[m.id] = null;
      ipc({ type: 'invite', message: m.id, index: j });
    }
    return inviteCard(m.id, INVITES[m.id]);
  }
  document.addEventListener('click', function(e) {
    const t = e.target.closest && e.target.closest('[data-inv-rsvp],[data-inv-add],[data-inv-open],[data-inv-join]');
    if (!t) return;
    e.preventDefault();
    e.stopPropagation();
    if (t.dataset.invRsvp) {
      ipc({ type: 'invite_rsvp', key: t.dataset.key, response: t.dataset.invRsvp, series: t.dataset.series === '1' });
      t.parentNode.querySelectorAll('[data-inv-rsvp]').forEach(function(b) { b.classList.toggle('on', b === t); });
    } else if (t.dataset.invAdd) {
      ipc({ type: 'invite_add', message: Number(t.dataset.invAdd) });
      t.disabled = true;
      t.textContent = 'Adding…';
    } else if (t.dataset.invOpen) {
      ipc({ type: 'invite_open', key: t.dataset.invOpen });
    } else if (t.dataset.invJoin) {
      ipc({ type: 'invite_join', url: t.dataset.invJoin });
    }
  }, true);

  function messageHtml(t, m, i) {
    const open = i === t.messages.length - 1 || t.open.has(i);
    if (!open) {
      const snip = (m.text || '').replace(/\s+/g, ' ').slice(0, 120);
      return '<div class="msg collapsed" data-i="' + i + '"><div class="msg-head">' + avatar(m.from, m.brand) + '<span class="who"><b>' + esc(m.from.name || m.from.addr) + '</b>' +
        ' <span class="snip">— ' + esc(snip) + '</span></span><span class="time">' + esc(fmtTime(m.date)) + '</span></div></div>';
    }
    const who = m.from.name ? '<b>' + esc(m.from.name) + '</b> <span class="addr">' + esc(m.from.addr) + '</span>' : '<b>' + esc(m.from.addr) + '</b>';
    const recipients = m.to.concat(m.cc).map(function(a) { return a.addr === t.me ? 'me' : (a.name || a.addr); }).join(', ');
    let html = '<div class="msg" data-i="' + i + '"><div class="msg-head">' + avatar(m.from, m.brand) + '<span class="who">' + who + '</span><span class="time">' + esc(fmtTime(m.date, true)) + '</span></div>' +
      (recipients ? '<div class="msg-to">to ' + esc(recipients) + '</div>' : '');
    if (m.html && REMOTE.test(m.html) && !imagesOn(t, m, i)) {
      html += '<div class="images-bar"><span>Remote images are hidden.</span><button class="pill" type="button" data-images="once" data-i="' + i + '">Show</button>' +
        '<button class="pill" type="button" data-images="always" data-i="' + i + '">Always from ' + esc(m.from.name || m.from.addr) + '</button></div>';
    }
    // No scripts in the frame; mail_patch.rs handles its links, keys and
    // swipes. allow-popups lets target=_blank links reach it.
    html += '<div class="msg-body">' + (m.html ? '<iframe class="mail ' + frameKind(m.html) + '" sandbox="allow-same-origin allow-popups" data-i="' + i + '" title="Message"></iframe>' :
      '<div class="msg-text">' + textHtml(m.text || (m.body_loaded ? '' : 'Loading…')) + '</div>') + '</div>';
    if (hasQuote(m)) html += '<button class="pill quote-toggle" type="button" data-quotes="' + i + '">Show quoted text</button>';
    const invite = (m.attachments || []).findIndex(isInvite);
    if (invite >= 0) html += inviteHtml(m, invite);
    const files = (m.attachments || []).map(function(a, j) { return [a, j]; }).filter(function(p) { return !isInvite(p[0]); });
    if (files.length) {
      html += '<div class="attachments">' + files.map(function(p) {
        const a = p[0], j = p[1], ft = fileType(a);
        return '<button class="att" type="button" data-att="' + i + ':' + j + '" title="Preview ' + esc(a.name) + '"><span class="ft ' + ft.cls + '">' + esc(ft.label) + '</span>' +
          '<span><span class="nm">' + esc(a.name) + '</span><span class="size">' + fmtSize(a.size) + '</span></span></button>';
      }).join('') + '</div>';
    }
    return html + '</div>';
  }
  // What each part of the thread is drawn from: a fresh copy that draws the
  // same is not redrawn, so its message frames do not reload.
  function headSig(t) {
    const r = t.row || {};
    return [t.messages[0].subject, r.pinned, r.class, r.summary, S.ai].join('\u0001');
  }
  function bodySig(t) {
    return (t.images || []).slice().sort().join(',') + '\u0001' + t.messages.map(function(m) {
      return [m.id, m.body_loaded, (m.html || '').length, (m.text || '').length, (m.attachments || []).length].join(':');
    }).join('|');
  }

  function renderThread() {
    const t = S.thread;
    if (!t || !t.messages) return;
    const k = key(t), same = detail._k === k;
    // Unread messages open, and stay open once read.
    if (!t.open) t.open = new Set(t.messages.map(function(m, i) { return m.seen ? -1 : i; }).filter(function(i) { return i >= 0; }));
    // Redrawn in place it keeps its scroll and its frames' heights; another
    // conversation starts at the top.
    const top = same ? detail.scrollTop : 0;
    const heights = {};
    if (same) detail.querySelectorAll('iframe.mail').forEach(function(f) { heights[f.dataset.i] = f.style.height; });
    detail.dataset.enter = same ? (detail._pending ? 'fill' : '') : S.enter;
    S.enter = '';
    const row = t.row || {};
    let html = threadHead(t.messages[0].subject || '(no subject)', row) +
      '<div class="summary" id="t-line"' + (row.summary ? '' : ' hidden') + '><span style="color:var(--accent)">✦</span> <span class="tl-text">' + esc(row.summary || '') + '</span></div>' +
      '<div id="t-summary" hidden></div>';
    t.messages.forEach(function(m, i) { html += messageHtml(t, m, i); });
    html += '<div id="reply-slot"></div>';
    detail.innerHTML = html;
    detail._k = k;
    detail._pending = false;
    t.headSig = headSig(t);
    t.bodySig = bodySig(t);
    renderSummary();
    syncFrameFocus();
    wireMessages(detail, heights);
    detail.scrollTop = top;
    if (S.composer) renderComposer();
    prefetchAround(k);
  }
  // The toolbar and the inbox line alone, when only they changed.
  function renderHead() {
    const t = S.thread, head = detail.querySelector('.thread-head');
    if (!t || !t.messages || !head) return;
    const row = t.row || {};
    detail.dataset.enter = '';
    head.outerHTML = threadHead(t.messages[0].subject || '(no subject)', row);
    const line = $('t-line');
    if (line) line.querySelector('.tl-text').textContent = row.summary || '';
    t.headSig = headSig(t);
    renderSummary();
  }
  // One message redrawn, only its frame reloading: a folded one opening, or
  // its images shown.
  function renderMessage(i) {
    const t = S.thread, el = detail.querySelector('.msg[data-i="' + i + '"]');
    if (!t || !t.messages || !el) return;
    detail.dataset.enter = '';
    const f = el.querySelector('iframe.mail'), heights = {};
    if (f) heights[i] = f.style.height;
    el.insertAdjacentHTML('afterend', messageHtml(t, t.messages[i], i));
    const fresh = el.nextElementSibling;
    el.remove();
    if (el.classList.contains('collapsed') && !fresh.classList.contains('collapsed')) fresh.classList.add('expand');
    wireMessages(fresh, heights);
  }
  function wireMessages(root, heights) {
    const t = S.thread;
    root.querySelectorAll('iframe.mail').forEach(function(f) {
      const i = Number(f.dataset.i), m = t.messages[i];
      if (heights[i]) f.style.height = heights[i];
      f.addEventListener('load', function() { wireFrame(f); f.classList.add('fitted'); });
      f.srcdoc = frameDoc(m.html, imagesOn(t, m, i), frameKind(m.html));
      whenParsed(f);
    });
  }
  // A fresh copy of the open thread: only what changed is redrawn.
  function showThread(data) {
    const t = S.thread;
    S.thread = Object.assign({}, data, { open: t.open, shown: t.shown, headSig: t.headSig, bodySig: t.bodySig });
    resumeWriter();
    const fresh = S.thread;
    if (!t.messages || detail._k !== key(fresh) || detail._pending || bodySig(fresh) !== t.bodySig) renderThread();
    else if (headSig(fresh) !== t.headSig) renderHead();
    if (S.composer && S.composer.thread === fresh.thread && !$('composer')) renderComposer();
  }

  // AI-2: the open conversation's summary, streamed in under its toolbar.
  // It stays with its conversation when the user looks elsewhere.
  function renderSummary() {
    const slot = $('t-summary'), line = $('t-line'), s = S.tsum, t = S.thread;
    if (!slot) return;
    const mine = !!(s && t && s.account === t.account && s.thread === t.thread);
    // It says more than the inbox line, which steps aside.
    if (line) line.hidden = mine || !(t && t.row && t.row.summary);
    slot.hidden = !mine;
    if (!mine) return;
    slot.className = 'summary thread-summary' + (s.phase === 'writing' ? ' writing' : '');
    slot.innerHTML = '<span class="spark">✦</span> ' + esc(s.phase === 'error' ? s.error : s.text || 'Summarising…') +
      (s.phase === 'writing' ? '<span class="caret"></span>' : '');
  }

  // A frame shows once its mail is parsed, not at `load`: that waits for
  // every remote image, and one that stalls (a tracking pixel) left the
  // message blank. `load` still wires it should parsing be missed.
  function whenParsed(f) {
    let tries = 0;
    (function check() {
      if (f._wired || !f.isConnected) return;
      const doc = f.contentDocument;
      if (doc && doc.readyState !== 'loading') wireFrame(f);
      if (!f._wired && ++tries < 600) setTimeout(check, 16);
    })();
  }
  // Once, on the mail's own document — not the blank one the frame starts with.
  function wireFrame(f) {
    const doc = f.contentDocument;
    if (f._wired || !doc || doc.URL !== 'about:srcdoc' || !doc.body) return;
    f._wired = true;
    f.classList.add('fitted');
    if (f.classList.contains('plain')) readableInDark(doc);
    // The body's own height: the document's is at least the frame's, so it
    // would never shrink below the iframe default.
    const fit = function() { f.style.height = Math.ceil(doc.body.getBoundingClientRect().height) + 'px'; };
    fit();
    // Again whenever its content grows, as images arrive.
    if (window.ResizeObserver) new ResizeObserver(fit).observe(doc.body);
    else setTimeout(fit, 300);
    linkifyFrame(doc);
    // READ-3: the full address shows on hover before clicking.
    doc.querySelectorAll('a[href]').forEach(function(a) { a.title = a.getAttribute('href'); });
    f._fit = fit;
  }

  // The message frame runs no scripts, so mail_patch.rs forwards the keys
  // typed in it while it holds focus.
  let frameFocused = false;
  function syncFrameFocus() {
    const on = !!document.activeElement && document.activeElement.matches('iframe.mail');
    if (on === frameFocused) return;
    frameFocused = on;
    ipc({ type: 'frame_focus', focused: on });
  }
  // Focus moving into a frame blurs this window before it lands there.
  window.addEventListener('blur', function() { setTimeout(syncFrameFocus, 0); });
  window.addEventListener('focus', syncFrameFocus);
  document.addEventListener('focusin', syncFrameFocus);

  // ── Swipe right (TRI-11): Done on a row or group, back on the open thread ──
  // mail_patch.rs owns the gesture (travel, when it arms, whether the release
  // commits); this draws it on what it began on. Timers, not transitionend:
  // with reduced motion there are no transitions to end.
  const SWIPE_BACK_MS = 320, SWIPE_OUT_MS = 200, SWIPE_FOLD_MS = 180;
  let swipe = null;
  function swipeAt(x, y) {
    const el = document.elementFromPoint(x, y);
    if (!el || S.composer || PV.m || el.closest('.swipe-out')) return null;
    const row = el.closest('.row');
    const r = row && list.contains(row) && S.rows[Number(row.dataset.i)];
    if (r && r.thread) return { el: row, also: [], items: [{ account: r.thread.account, thread: r.thread.thread }] };
    if (r && r.group) {
      // A sender group: all its conversations, and when it is open their rows
      // fold away with it.
      const keys = new Set(r.group.threads.map(key));
      const also = Array.from(list.querySelectorAll('.row')).filter(function(x) {
        const o = S.rows[Number(x.dataset.i)];
        return o && o.thread && keys.has(key(o.thread));
      });
      return { el: row, also: also, items: r.group.threads.map(function(t) { return { account: t.account, thread: t.thread }; }) };
    }
    // The open thread swipes back to the list, like its back button.
    if (S.thread && detail.contains(el)) return { el: detail, also: [], back: true };
    return null;
  }
  function onSwipe(phase, a, b) {
    if (phase === 'begin') {
      swipe = swipeAt(a, b);
      if (swipe) startSwipe(swipe); else ipc({ type: 'swipe_miss' });
      return;
    }
    const s = swipe;
    if (!s) return;
    if (phase === 'move') {
      s.dx = a; s.armed = b;
      // One paint per frame however fast the trackpad reports.
      if (!s.frame) s.frame = requestAnimationFrame(function() { s.frame = 0; paintSwipe(s); });
      return;
    }
    swipe = null;
    cancelAnimationFrame(s.frame);
    if (a) swipeOut(s); else swipeBack(s);
  }
  // The track sits where the element started, behind it, clipped to the part
  // on screen; fixed, since nothing scrolls while the fingers swipe.
  function startSwipe(s) {
    const el = s.el;
    // Still easing back from a release: take it over where it is.
    const travel = parseFloat(getComputedStyle(el).getPropertyValue('--swipe')) || 0;
    if (el._track) el._track.remove();
    el.classList.remove('swipe-back');
    el.classList.add('swiping');
    const r = el.getBoundingClientRect(), clip = (el === detail ? detail : list).getBoundingClientRect();
    const top = Math.max(r.top, clip.top), bottom = Math.min(r.bottom, clip.bottom);
    const track = document.createElement('div');
    track.className = 'swipe-track' + (s.back ? ' back' : ' under');
    track.style.left = (r.left - travel) + 'px';
    track.style.top = top + 'px';
    track.style.height = Math.max(0, bottom - top) + 'px';
    document.body.appendChild(track);
    el._track = s.track = track;
    s.dx = travel; s.armed = false; s.frame = 0;
    setSwipe(s, travel);
  }
  function setSwipe(s, px) {
    s.el.style.setProperty('--swipe', px + 'px');
    s.track.style.setProperty('--swipe', px + 'px');
  }
  function swipeClass(s, op, names) {
    s.el.classList[op].apply(s.el.classList, names);
    s.track.classList[op].apply(s.track.classList, names);
  }
  function endSwipe(s) {
    s.el.classList.remove('swiping', 'swipe-back', 'swipe-out');
    s.el.style.removeProperty('--swipe');
    s.track.remove();
    if (s.el._track === s.track) s.el._track = null;
  }
  function paintSwipe(s) {
    setSwipe(s, Math.max(0, Math.min(s.dx, s.el.clientWidth)));
    s.track.classList.toggle('swipe-armed', s.armed);
  }
  function swipeBack(s) {
    swipeClass(s, 'add', ['swipe-back']);
    s.track.classList.remove('swipe-armed');
    setSwipe(s, 0);
    setTimeout(function() {
      // A new swipe on this element took over.
      if (s.el._track === s.track) endSwipe(s);
    }, SWIPE_BACK_MS);
  }
  // The element leaves to the right. The thread is then closed; a row folds
  // away, its track with it, so the rows below glide up before the list
  // redraws without it.
  function swipeOut(s) {
    swipeClass(s, 'add', ['swipe-out']);
    s.also.forEach(function(el) { el.classList.add('swipe-out'); });
    s.track.classList.add('swipe-armed');
    setSwipe(s, s.el.clientWidth);
    setTimeout(function() {
      if (s.back) {
        endSwipe(s);
        closeThread();
        return;
      }
      // Gone already: an inbox redraw meanwhile must not bring it back.
      s.items.forEach(function(i) { S.gone.set(key(i), Date.now() + GONE_MS); });
      const fold = [s.el].concat(s.also);
      fold.forEach(function(el) { el.style.height = el.offsetHeight + 'px'; });
      void s.el.offsetHeight;
      requestAnimationFrame(function() {
        s.track.classList.add('swipe-fold');
        fold.forEach(function(el) { el.classList.add('swipe-fold'); });
        setTimeout(function() { s.track.remove(); act('done', s.items); }, SWIPE_FOLD_MS);
      });
    }, SWIPE_OUT_MS);
  }

  detail.addEventListener('click', function(e) {
    const a = e.target.closest('a[href]');
    // The link navigates, and mail_patch.rs opens it in a tab.
    if (a) return;
    const t = S.thread;
    const actBtn = e.target.closest('[data-act]');
    if (actBtn) { threadAction(actBtn.dataset.act, actBtn); return; }
    const collapsed = e.target.closest('.msg.collapsed');
    if (collapsed) { const i = Number(collapsed.dataset.i); t.open.add(i); renderMessage(i); return; }
    const images = e.target.closest('[data-images]');
    if (images) {
      const i = Number(images.dataset.i), m = t.messages[i];
      t.shown = t.shown || new Set(); t.shown.add(i);
      if (images.dataset.images === 'always') {
        t.images = (t.images || []).concat([m.from.addr]);
        t.bodySig = bodySig(t);
        ipc({ type: 'images', account: t.account, thread: t.thread, sender: m.from.addr, always: true });
        t.messages.forEach(function(x, j) { if (x.from.addr === m.from.addr) renderMessage(j); });
      } else renderMessage(i);
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
    iconBtn('data-sel', 'to_notes', 'To notes', 't', I.notes) + iconBtn('data-sel', 'spam', 'Report spam', '!', I.spam) + iconBtn('data-sel', 'clear', 'Clear selection', 'Esc', I.close);
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
      case 'summarise':
        S.tsum = { account: t.account, thread: t.thread, text: '', phase: 'writing' };
        ipc({ type: 'ai', kind: 'summarise', item: item[0], text: '' });
        renderSummary();
        break;
      case 'extract': ipc({ type: 'ai', kind: 'extract', item: item[0], text: '' }); showUndo('Finding tasks…', null, 2500); break;
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
      // Opened from what was kept: the thread is there to reply under.
      if (S.thread.messages) renderComposer();
      return;
    }
    if (!d.thread) { S.thread = null; panel.classList.add('detail'); detail.innerHTML = ''; detail._k = null; detail.dataset.enter = ''; markOpen(null); }
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
    const w = S.writers.get(d.key) || null;
    const drafted = w && (w.phase === 'writing' || w.phase === 'review');
    // The button's light sweeps once per composer, not on every redraw.
    const intro = canWrite(d) && !d._writeSeen;
    if (intro) d._writeSeen = true;
    // It rises in when it opens, not on every redraw.
    const enter = !slot.querySelector('#composer');
    slot.innerHTML = '<div class="composer' + (d.thread ? '' : ' standalone') + (enter ? ' enter' : '') + '" id="composer">' +
      '<div class="title">' + esc(title) + (d._saved ? '<span class="draft">Draft saved</span>' : '') + '</div>' +
      '<div class="field"><label>From</label><select id="c-from">' + from + '</select></div>' +
      '<div class="field"><label>To</label><input id="c-to" value="' + esc(d.to) + '" placeholder="name@example.com, …" autocomplete="off" spellcheck="false">' +
      (showCc ? '' : '<button class="toggle-cc" type="button" id="c-show-cc">Cc Bcc</button>') + '</div>' +
      (showCc ? '<div class="field"><label>Cc</label><input id="c-cc" value="' + esc(d.cc) + '" autocomplete="off" spellcheck="false"></div>' +
        '<div class="field"><label>Bcc</label><input id="c-bcc" value="' + esc(d.bcc) + '" autocomplete="off" spellcheck="false"></div>' : '') +
      '<div class="field"><label>Subject</label><input id="c-subject" value="' + esc(d.subject) + '"></div>' +
      (drafted ? '<div class="writer-draft' + (w.phase === 'writing' ? ' writing' : '') + '" id="w-draft">' + esc(w.text) +
          (w.phase === 'writing' ? '<span class="caret"></span>' : '') + '</div>' :
        '<textarea id="c-body" placeholder="' +
          (canWrite(d) ? 'Write — or ⌘J to draft it with AI' : 'Write…  **bold**, *italic*, - lists, [links](https://…)') + '">' + esc(d.body) + '</textarea>') +
      (w ? writerBar(d, w) : '') +
      '<div class="files" id="c-files">' + (d.attachments || []).map(function(f, i) {
        return '<span class="file">' + esc(f.name) + ' <span class="size">' + fmtSize(Math.floor(f.data.length * 3 / 4)) + '</span><button type="button" data-rm="' + i + '" aria-label="Remove">✕</button></span>';
      }).join('') + '</div>' +
      '<div class="error" id="c-error">' + esc(S.composerError) + '</div>' +
      '<div class="actions"><button class="btn primary" type="button" id="c-send">Send<span class="kbd">⌘↩</span></button>' +
      '<button class="btn" type="button" id="c-later">Send later</button>' +
      '<button class="btn' + (d._remind ? ' set' : '') + '" type="button" id="c-remind">' + (d._remind ? 'Remind: ' + esc(d._remindLabel) : 'Remind me') + '</button>' +
      (canWrite(d) && !w ? '<button class="btn write' + (intro ? ' intro' : '') + '" type="button" id="c-write" title="Draft the reply, or refine what you wrote, from this conversation">✦ <span class="label">' + writeLabel(d) + '</span><span class="kbd">⌘J</span></button>' : '') +
      '<span class="spacer"></span>' +
      '<button class="icon-btn" type="button" id="c-attach" title="Attach files" aria-label="Attach files">' + I.clip + '</button>' +
      '<button class="icon-btn" type="button" id="c-discard" title="Discard draft" aria-label="Discard draft">' + I.trash + '</button></div></div>';
    const fields = ['to', 'cc', 'bcc', 'subject', 'body'];
    fields.forEach(function(f) {
      const el = $('c-' + f);
      if (el) el.addEventListener('input', function() {
        d[f] = el.value;
        scheduleDraftSave();
        if (f !== 'body') return;
        grow(el);
        const label = document.querySelector('#c-write .label');
        if (label) label.textContent = writeLabel(d);
      });
    });
    const body = $('c-body');
    if (body) grow(body);
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
    wireWriter(d, w, box);
    if (!w) {
      const focusField = !d.to ? 'c-to' : 'c-body';
      setTimeout(function() { const el = $(focusField); if (el && document.activeElement.tagName !== 'INPUT') el.focus(); }, 0);
    }
    if (d.thread) box.scrollIntoView({ block: 'nearest', behavior: enter && !REDUCED.matches ? 'smooth' : 'auto' });
  }

  // The body grows with its text; the thread scrolls, not the textarea.
  function grow(el) { el.style.height = 'auto'; el.style.height = el.scrollHeight + 'px'; }

  let draftTimer = 0, draftPending = null;
  function cleanDraft(d) {
    return { key: d.key, account: d.account, mode: d.mode, thread: d.thread, message: d.message, to: d.to, cc: d.cc, bcc: d.bcc, subject: d.subject, body: d.body, attachments: d.attachments || [] };
  }
  function scheduleDraftSave() {
    clearTimeout(draftTimer);
    const d = draftPending = S.composer;
    draftTimer = setTimeout(function() {
      draftPending = null;
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
    // Other threads' writers keep going in the background.
    if (d) stopWriter(d.key);
    if (d && d.thread) { const slot = $('reply-slot'); if (slot) slot.innerHTML = ''; }
    else if (!S.thread) { panel.classList.remove('detail'); detail.innerHTML = ''; list.focus(); }
  }
  // Leaving a thread keeps the reply its writer is on (AI-3): the text
  // streams on, and the composer comes back with it when the thread opens.
  // Each reply has its own writer, so several can draft at once.
  function leaveComposer() {
    const d = S.composer;
    S.composer = null;
    if (!d) return;
    // Words typed just before leaving are kept, not lost to the debounce.
    if (draftPending === d) {
      clearTimeout(draftTimer);
      draftPending = null;
      ipc({ type: 'save_draft', draft: cleanDraft(d) });
    }
    const w = S.writers.get(d.key);
    if (w && (w.phase === 'writing' || w.phase === 'review')) w.draft = d;
    else stopWriter(d.key);
  }
  // The composer a backgrounded writer keeps for the thread now open.
  function resumeWriter() {
    const t = S.thread;
    if (S.composer || !t) return;
    for (const w of S.writers.values()) {
      if (!w.draft || w.item.account !== t.account || w.item.thread !== t.thread) continue;
      S.composer = w.draft;
      w.draft = null;
      return;
    }
  }

  // ── Writer (AI-3) ─────────────────────────────────────────────────────────
  // ⌘J or ✦ opens it under the text; octoweb:mail-writer turns the notes
  // (or nothing) into the reply, streamed in place. Keep puts it in the body
  // as one native edit, so ⌘Z brings the notes back.
  const WRITER_TUNES = [['Shorter', 'Make it shorter'], ['Friendlier', 'Make it friendlier'], ['More formal', 'Make it more formal']];
  function canWrite(d) { return S.ai && d.thread && d.mode !== 'forward'; }
  // With words in the body the writer refines them; with none it drafts.
  function writeLabel(d) { return d.body.trim() ? 'Refine with AI' : 'Write with AI'; }
  function writerBar(d, w) {
    const kbd = function(k) { return '<span class="kbd">' + k + '</span>'; };
    const close = function(label) { return '<button class="pill" type="button" data-w="close">' + label + kbd('esc') + '</button>'; };
    const open = '<div class="writer" id="writer">';
    if (w.phase === 'ask' && d.body.trim()) {
      return open + '<button class="btn primary" type="button" data-w="improve">Improve' + kbd('↩') + '</button>' +
        WRITER_TUNES.map(function(t) { return '<button class="pill" type="button" data-tune="' + esc(t[1]) + '">' + esc(t[0]) + '</button>'; }).join('') +
        '<label class="ask"><input id="w-input" autocomplete="off" placeholder="Or say how to change it…"></label>' + close('Cancel') + '</div>';
    }
    if (w.phase === 'ask') {
      return open + '<label class="ask"><span class="spark">✦</span><input id="w-input" autocomplete="off" placeholder="What should it say? Or just ↩"></label>' +
        close('Cancel') + '</div>';
    }
    if (w.phase === 'writing') {
      return open + '<span class="spark">✦</span><span class="status">Writing…</span>' + close('Stop') + '</div>';
    }
    if (w.phase === 'error') {
      return open + '<span class="spark">✦</span><span class="err">' + esc(w.error) + '</span>' +
        '<button class="pill" type="button" data-w="retry">Try again</button>' + close('Close') + '</div>';
    }
    return open + '<button class="btn primary" type="button" data-w="keep">Keep' + kbd('↩') + '</button>' +
      WRITER_TUNES.map(function(t) { return '<button class="pill" type="button" data-tune="' + esc(t[1]) + '">' + esc(t[0]) + '</button>'; }).join('') +
      '<button class="pill" type="button" data-w="retry" title="Write it again" aria-label="Write it again">↻</button>' +
      '<label class="ask"><input id="w-input" autocomplete="off" placeholder="Change something…"></label>' + close('Discard') + '</div>';
  }
  function wireWriter(d, w, box) {
    // Ahead of the page's keys: Esc here closes the writer, not the composer.
    box.addEventListener('keydown', function(e) {
      const mine = S.writers.has(d.key);
      if (e.metaKey && !e.shiftKey && !e.altKey && !e.ctrlKey && e.key.toLowerCase() === 'j' && canWrite(d)) {
        e.preventDefault(); e.stopPropagation();
        if (mine) closeWriter(d); else openWriter(d);
      } else if (e.key === 'Escape' && mine) {
        e.preventDefault(); e.stopPropagation();
        closeWriter(d);
      }
    });
    const button = $('c-write');
    if (button) button.addEventListener('click', function() { openWriter(d); });
    if (!w) return;
    const bar = $('writer');
    bar.addEventListener('click', function(e) {
      const b = e.target.closest('[data-w], [data-tune]');
      if (!b) return;
      // Before any reply exists the tunes work on what the body holds.
      if (b.dataset.tune) write(d, b.dataset.tune, w.phase === 'ask' ? d.body : w.text);
      else if (b.dataset.w === 'improve') write(d, '', d.body);
      else if (b.dataset.w === 'keep') keepWriting(d);
      else if (b.dataset.w === 'retry') write(d, w.request.instruction, w.request.text);
      else closeWriter(d);
    });
    const input = $('w-input');
    if (input) {
      // In review an empty ↩ keeps the reply; words ask for a change.
      input.addEventListener('keydown', function(e) {
        if (e.key !== 'Enter' || e.isComposing) return;
        e.preventDefault();
        const said = input.value.trim();
        if (w.phase === 'ask') write(d, said, d.body);
        else if (said) write(d, said, w.text);
        else keepWriting(d);
      });
      input.focus();
    } else {
      // Focus stays in the composer, so Esc stops the writer rather than
      // reaching the page and closing the whole composer.
      bar.querySelector('button').focus();
    }
    bar.scrollIntoView({ block: 'nearest' });
  }
  function openWriter(d) {
    stopWriter(d.key);
    S.writers.set(d.key, { key: d.key, phase: 'ask' });
    renderComposer();
  }
  function write(d, instruction, text) {
    stopWriter(d.key);
    // The conversation's account, not From: the reply belongs to the thread.
    const item = { account: S.thread.account, thread: d.thread };
    S.writers.set(d.key, { key: d.key, item: item, phase: 'writing', text: '', request: { instruction: instruction, text: text } });
    ipc({ type: 'ai', kind: 'draft', item: item, text: instruction, body: text });
    renderComposer();
  }
  function closeWriter(d) {
    stopWriter(d.key);
    renderComposer();
    const body = $('c-body');
    if (body) body.focus();
  }
  function keepWriting(d) {
    const text = S.writers.get(d.key).text;
    S.writers.delete(d.key);
    renderComposer();
    // One native edit: the field's own ⌘Z brings back what it held.
    const body = $('c-body');
    body.focus();
    body.select();
    document.execCommand('insertText', false, text);
  }
  function stopWriter(k) {
    const w = S.writers.get(k);
    if (w && w.phase === 'writing') ipc({ type: 'draft_stop', key: k });
    S.writers.delete(k);
  }
  function discard() {
    const d = S.composer;
    if (!d) return;
    clearTimeout(draftTimer);
    ipc({ type: 'delete_draft', key: d.key });
    S.drafts = S.drafts.filter(function(x) { return x.key !== d.key; });
    if (S.thread) { S.thread.draft = null; const kept = threads.get(key(S.thread)); if (kept) kept.draft = null; }
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
    // Tab and ⇧Tab move through the pane's tabs, as Tab cycles the
    // assistant's sessions. Only the composer, a form, keeps Tab for its fields.
    if (e.key === 'Tab' && !e.metaKey && !e.ctrlKey && !e.altKey) {
      if (e.target && e.target.closest && e.target.closest('#composer')) return;
      e.preventDefault();
      ipc({ type: 'tab', tab: e.shiftKey ? 'notes' : 'calendar' });
      return;
    }
    // ⌘↩ sends, and ⇧⌘D as in Apple Mail.
    if (e.metaKey && S.composer && (e.key === 'Enter' || (e.shiftKey && e.key.toLowerCase() === 'd'))) { e.preventDefault(); send(null); return; }
    if (e.metaKey && !e.shiftKey && !e.altKey && !e.ctrlKey && (e.key === 'z' || e.key === 'Z') && !typing(e) && undoKey()) { e.preventDefault(); return; }
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
    // Apple Mail's chords work beside the single letters.
    if (e.metaKey && !e.altKey) {
      const chord = (e.ctrlKey ? '⌃' : '') + (e.shiftKey ? '⇧' : '') + e.key.toLowerCase();
      const run = {
        'r': function() { replyTo('reply'); }, '⇧r': function() { replyTo('all'); }, '⇧f': function() { replyTo('forward'); },
        'n': function() { compose(); }, '⌃a': function() { act('done', targets()); }, '⇧u': function() { act('mark_unread', targets()); },
      }[chord];
      if (run) { e.preventDefault(); run(); return; }
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
      case '!': act('spam', targets()); break;
      case 't': act('to_notes', targets()); break;
      case 'u': act('mark_unread', targets()); break;
      case 'x': toggleSelect(); break;
      case 'r': replyTo('reply'); break;
      case 'a': replyTo('all'); break;
      case 'f': replyTo('forward'); break;
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
    openThread(S.rows[j].thread.account, S.rows[j].thread.thread, false, step > 0 ? 'next' : 'prev');
  }
  // Reply in the open thread, or open the focused one and reply there.
  function replyTo(mode) {
    if (S.thread && panel.classList.contains('detail')) { reply(mode); return; }
    const r = S.rows[S.focus];
    if (r && r.thread) {
      openThread(r.thread.account, r.thread.thread, false);
      // Kept from before, it can be replied to at once.
      if (S.thread.messages) reply(mode); else S._pendingReply = mode;
    }
  }

  function renderHelp() {
    const keys = [
      ['j / k', 'Next / previous'], ['↩', 'Open'], ['Esc', 'Back'], ['e', 'Done'], ['h', 'Snooze'], ['p', 'Pin'],
      ['m', 'Mute'], ['b', 'Block sender'], ['!', 'Report spam'], ['t', 'To notes'], ['u', 'Mark unread'], ['r', 'Reply'], ['a', 'Reply all'],
      ['f', 'Forward'], ['c', 'Compose'], ['/', 'Search'], ['x', 'Select'], ['⌘Z', 'Undo'], ['⌘↩', 'Send'],
      ['⌘R ⇧⌘R', 'Reply / all'], ['⇧⌘F', 'Forward'], ['⌘N', 'Compose'], ['⌃⌘A', 'Done'], ['⇧⌘U', 'Mark unread'], ['⇧⌘D', 'Send'],
    ];
    if (S.ai) keys.push(['s', 'Summarise (AI)'], ['⌘J', 'Write a reply with AI']);
    help.innerHTML = keys.map(function(k) { return '<div><kbd>' + esc(k[0]) + '</kbd>' + esc(k[1]) + '</div>'; }).join('');
  }

  // SRCH-4: the reader translates the request; the app runs the search (AI-6).
  let searchRequest = 0;
  function askSearch() {
    const q = search.value.trim();
    if (!q) return;
    clearTimeout(searchTimer);
    S.mode = 'search'; S.query = q; S.focus = 0;
    S.searching = false; S.searchError = null;
    S.aiSearch = { id: ++searchRequest, phase: 'writing', error: '' };
    ipc({ type: 'search_ai', id: S.aiSearch.id, query: q });
    render();
  }
  // AI-4: one key applies a brief group's action — Done for every conversation
  // in the group of the focused row, or in group `gi`.
  function doneAll(gi) {
    if (S.mode !== 'show') return;
    if (gi === undefined) { const r = S.rows[S.focus]; gi = r && r.show !== undefined ? r.show : 0; }
    const g = S.showGroups[gi];
    if (!g || !g.items.length) return;
    act('done', g.items.map(function(t) { return { account: t.account, thread: t.thread }; }));
  }
  // The brief runs on the mail reader, not the assistant: the list says it is
  // coming, and `brief` or `briefFailed` follows.
  function startBrief() {
    S.mode = 'show'; S.showTitle = 'Daily brief'; S.showGroups = []; S.showEmpty = ''; S.briefing = { phase: 'writing' }; S.focus = 0;
    closeThread(true);
    render();
    ipc({ type: 'ai', kind: 'brief', item: null, text: '' });
  }

  // ── Search (SRCH) ───────────────────────────────────────────────────────────
  function runSearch() {
    clearTimeout(searchTimer);
    S.aiSearch = null;
    const q = search.value.trim();
    if (!q) { backToInbox(); return; }
    S.mode = 'search'; S.query = q; S.focus = 0;
    ipc({ type: 'search', query: q, server: true });
    list.focus();
  }
  let searchTimer = 0;
  search.addEventListener('input', function() {
    clearTimeout(searchTimer);
    if (S.aiSearch) { S.aiSearch = null; render(); }
    searchTimer = setTimeout(function() { if (search.value.trim()) { S.mode = 'search'; S.query = search.value.trim(); ipc({ type: 'search', query: S.query, server: true }); } else backToInbox(); }, 220);
  });

  // ── Bar, pane and resize ────────────────────────────────────────────────────
  $('tab-notes').addEventListener('click', function() { ipc({ type: 'tab', tab: 'notes' }); });
  $('tab-cal').addEventListener('click', function() { ipc({ type: 'tab', tab: 'calendar' }); });
  $('close-btn').addEventListener('click', function() { ipc({ type: 'hide' }); });
  $('fullscreen-btn').addEventListener('click', function() { ipc({ type: 'fullscreen' }); });
  $('compose-btn').addEventListener('click', function() { compose(); });
  $('help-btn').addEventListener('click', function() { renderHelp(); help.classList.toggle('open'); });
  $('brief-btn').addEventListener('click', startBrief);

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
      if (S.mode === 'search') ipc({ type: 'search', query: S.query, server: true });
      else ipc({ type: 'inbox' });
      // A redraw, not an open: a thread marked unread stays unread.
      if (S.thread && !S.thread.loading && !S.composer) ipc({ type: 'thread', account: S.thread.account, thread: S.thread.thread, peek: true });
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
      panel.classList.toggle('multi', S.accounts.length > 1);
      if (switched) {
        closePreview();
        clearTimeout(searchTimer); S.aiSearch = null; S.searching = false; S.searchError = null; S.briefing = null;
        S.view = null; S.thread = null; S.composer = null; Array.from(S.writers.keys()).forEach(stopWriter); S.mode = 'inbox'; S.selected.clear(); S.expanded.clear(); S.focus = 0;
        S.gone.clear(); threads.clear(); asked.clear();
        panel.classList.remove('detail'); detail.innerHTML = ''; detail._k = null; search.value = '';
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
      pruneGone();
      if (S.mode !== 'inbox') return;
      render();
      // The row in focus loads ahead, to open at once.
      const r = S.rows[S.focus];
      if (!S.thread && r && r.thread) prefetchSoon(r.thread);
    },
    search: function(data) {
      if (S.mode !== 'search' || data.query !== S.query || data.query !== search.value.trim()) return;
      S.hits = data.hits;
      S.searching = data.searching;
      S.searchError = data.error;
      pruneGone();
      render();
    },
    searchReady: function(data) {
      const s = S.aiSearch;
      if (data.ws !== S.ws || S.mode !== 'search' || !s || s.id !== data.id || s.phase !== 'writing') return;
      search.value = data.query;
      runSearch();
    },
    searchFailed: function(data) {
      const s = S.aiSearch;
      if (data.ws !== S.ws || S.mode !== 'search' || !s || s.id !== data.id || s.phase !== 'writing') return;
      s.phase = 'error'; s.error = data.message;
      render();
    },
    // A conversation, opened or loaded ahead (`peek`).
    thread: function(data) {
      remember(data);
      const t = S.thread;
      if (!t || t.account !== data.account || t.thread !== data.thread) return;
      showThread(data);
      if (data.peek) return;
      if (S._pendingReply) { const m = S._pendingReply; S._pendingReply = null; reply(m); }
      else if (data.draft && !S.composer) { S.composer = Object.assign({}, data.draft); renderComposer(); }
    },
    acted: function() { refresh(); },
    images: function() {},
    key: function(k) {
      onKey({ key: k.key, shiftKey: k.shift, metaKey: false, ctrlKey: false, altKey: false, isComposing: false,
        target: document.activeElement, preventDefault: function() {} });
    },
    // ⌘Z typed while the message frame has focus (see `mail_patch`).
    undo: function() { undoKey(); },
    swipe: onSwipe,
    mailto: function(href) {
      compose({ to: decodeURIComponent(href.slice(7).split('?')[0]) });
    },
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
    invite: function(d) {
      INVITES[d.message] = d.card || { error: d.error || 'This invitation could not be read.' };
      document.querySelectorAll('[data-inv="' + d.message + '"]').forEach(function(el) {
        el.outerHTML = inviteCard(d.message, INVITES[d.message]);
      });
    },
    openDraft: function(draft) { draft.attachments = draft.attachments || []; openComposer(draft); showUndo('Draft ready — review and send', null, 4000); },
    // The writer's reply so far for the composer with draft key `key`.
    draftStream: function(data) {
      const w = S.writers.get(data.key);
      if (!w || w.phase !== 'writing') return;
      w.text = data.text;
      // Off screen it only keeps the text, for when its thread opens again.
      const shown = S.composer && S.composer.key === w.key;
      if (data.done) { w.phase = 'review'; if (shown) renderComposer(); return; }
      if (!shown) return;
      const el = $('w-draft');
      if (el) el.innerHTML = esc(data.text) + '<span class="caret"></span>';
    },
    // The summary of conversation `item` so far (AI-2).
    summaryStream: function(data) {
      const s = S.tsum;
      if (!s || s.account !== data.item.account || s.thread !== data.item.thread || s.phase !== 'writing') return;
      s.text = data.text;
      if (data.done) s.phase = 'done';
      renderSummary();
    },
    summaryFailed: function(data) {
      const s = S.tsum;
      if (!s || s.account !== data.item.account || s.thread !== data.item.thread || s.phase !== 'writing') return;
      s.phase = 'error';
      s.error = data.message;
      renderSummary();
    },
    draftFailed: function(data) {
      const w = S.writers.get(data.key);
      if (!w || w.phase !== 'writing') return;
      w.phase = 'error';
      w.error = data.message;
      if (S.composer && S.composer.key === w.key) renderComposer();
    },
    show: function(data) { S.mode = 'show'; S.showTitle = data.title; S.showGroups = [{ title: data.title, items: data.items }]; S.showEmpty = ''; S.briefing = null; S.focus = 0; closeThread(true); render(); },
    brief: function(data) {
      if (data.ws !== S.ws) return;
      S.mode = 'show'; S.showTitle = 'Daily brief'; S.showGroups = data.groups; S.showEmpty = 'Your inbox is empty.'; S.briefing = null; S.focus = 0;
      closeThread(true);
      pruneGone();
      render();
    },
    briefFailed: function(data) {
      if (data.ws !== S.ws || !S.briefing || S.briefing.phase !== 'writing') return;
      S.briefing = { phase: 'error', error: data.message };
      if (S.mode === 'show') render();
      else showUndo(data.message, null, 5000);
    },
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

#[cfg(test)]
mod tests {
	#[test]
	fn native_search_rejects_stale_answers_and_shows_errors() {
		let source = format!(
			"{}\ncheckSearch({});",
			include_str!("mail_search_test.js"),
			serde_json::to_string(&super::html("null")).unwrap(),
		);
		let Some(out) = crate::a2ui_js::run_node("mail_search", &source, &[]) else {
			return;
		};
		assert!(
			out.status.success(),
			"mail search failed:\n{}",
			String::from_utf8_lossy(&out.stderr),
		);
	}
}
