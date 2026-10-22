//! Calendar tab of the right-hand pane (spec `specs/002-calendar`): the next
//! meeting with Join, quick add in plain English, the agenda, a day or week
//! grid, event details with RSVP, and the event form. Data comes from the
//! calendar engine through main.rs; this page keeps no state of its own.
//!
//! Messages to Rust (`window.ipc`): `calendar::Request`.
//!
//! Called from Rust:
//!   window.__cal.setup(data)          — workspace, accounts, calendars, prefs
//!   window.__cal.events(data)         — occurrences of the range last asked for
//!   window.__cal.changed()            — the store changed: ask again
//!   window.__cal.next(event|null)     — the meeting to show at the top
//!   window.__cal.parsed(id, result)   — quick-add text read into a draft
//!   window.__cal.done(op, ok, text)   — a save, delete or answer finished
//!   window.__cal.open(event)          — show one event
//!   window.__cal.draft(draft)         — open the form with a draft (agents)
//!   window.__cal.status(acct, state, message)
//!   window.__cal.unread(n)            — unread Important mail, on the Mail tab
//!   window.__cal.fullscreen(on)
//!   window.__cal.focus()
//!   window.__setShortcuts(data)

pub fn html(keybindings_json: &str) -> String {
	r##"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<style>
/*@@THEME@@*/
  html, body { width: 100%; height: 100%; margin: 0; overflow: hidden; background: var(--canvas); -webkit-font-smoothing: antialiased; font-family: var(--font-text); font-size: 13px; color: var(--label); }
  button, input, select, textarea { font: inherit; color: inherit; }
  #panel { position: fixed; inset: 0; display: flex; flex-direction: column; box-shadow: inset 0.5px 0 0 var(--hairline); }
  #resize-handle { position: fixed; top: 0; bottom: 0; left: 0; width: 6px; z-index: 30; cursor: col-resize; touch-action: none; }
  #resize-handle::after { content: ""; position: absolute; top: 0; bottom: 0; left: 0; width: 2px; background: var(--accent); opacity: 0; }
  #resize-handle:hover::after, #resize-handle.active::after { opacity: 1; }
  #panel.fullscreen #resize-handle { display: none; }
  body.resizing, body.resizing * { cursor: col-resize !important; }

  #bar { flex: 0 0 auto; display: flex; align-items: center; gap: 4px; height: 32px; padding: 0 10px; box-shadow: 0 0.5px 0 var(--hairline); font-size: var(--fs-caption); color: var(--label-2); -webkit-user-select: none; user-select: none; }
  .seg { display: flex; gap: 2px; padding: 2px; border-radius: var(--r-capsule); background: var(--fill); }
  .tab { border: none; background: transparent; height: 18px; padding: 0 10px; border-radius: var(--r-capsule); color: var(--label-2); font-size: 13px; font-weight: 500; cursor: pointer; display: flex; align-items: center; gap: 5px; transition: background var(--t-fast), color var(--t-fast); }
  .tab:hover { color: var(--label); }
  .tab.active { background: var(--card); color: var(--label); box-shadow: 0 0 0 0.5px var(--hairline), 0 1px 3px rgba(0,0,0,0.08); }
  .badge { min-width: 16px; padding: 0 5px; box-sizing: border-box; border-radius: var(--r-capsule); background: var(--accent); color: var(--on-accent); font-size: 10px; font-weight: 600; line-height: 16px; text-align: center; }
  .badge:empty { display: none; }
  .spacer { flex: 1 1 auto; }
  .icon-btn { flex-shrink: 0; display: flex; align-items: center; justify-content: center; width: 24px; height: 24px; padding: 0; border: none; border-radius: var(--r-ctl); background: transparent; color: var(--label-2); cursor: pointer; transition: background var(--t-fast), color var(--t-fast); }
  .icon-btn svg { width: 14px; height: 14px; }
  .icon-btn:hover { background: var(--fill-hover); color: var(--label); }
  .icon-btn:active { background: var(--fill-press); }
  .icon-btn.active { color: var(--accent); background: color-mix(in srgb, var(--accent) 15%, transparent); }
  .icon-btn.spin svg { animation: spin 0.9s linear infinite; }
  @keyframes spin { to { transform: rotate(360deg); } }

  #banner:empty { display: none; }
  .banner { display: flex; align-items: center; gap: 8px; margin: 8px 10px 0; padding: 7px 10px; border-radius: var(--r-ctl); background: color-mix(in srgb, var(--accent) 10%, var(--card)); font-size: 12px; line-height: 1.35; }
  .banner.err { background: color-mix(in srgb, var(--err) 10%, var(--card)); }
  .banner .text { flex: 1 1 auto; min-width: 0; overflow-wrap: anywhere; }
  .banner .who { color: var(--label-2); }

  .btn { border: none; border-radius: var(--r-ctl); padding: 0 10px; height: 24px; background: var(--fill); cursor: pointer; font-size: 12px; font-weight: 500; white-space: nowrap; transition: background var(--t-fast); }
  .btn:hover { background: var(--fill-hover); }
  .btn.primary { background: var(--accent); color: var(--on-accent); }
  .btn.primary:hover { background: color-mix(in srgb, var(--accent) 88%, black); }
  .btn.danger { color: var(--err); }
  .btn.on { background: var(--accent); color: var(--on-accent); }
  .btn:disabled { opacity: 0.5; cursor: default; }
  a, .link { color: var(--accent); text-decoration: none; cursor: pointer; }
  a:hover, .link:hover { text-decoration: underline; }

  #main { flex: 1 1 auto; min-height: 0; display: flex; flex-direction: column; }
  #next:empty { display: none; }
  .next { margin: 10px 10px 0; padding: 10px 12px; border-radius: var(--r-card); background: var(--card); box-shadow: 0 0 0 0.5px var(--hairline), 0 1px 3px rgba(0,0,0,0.06); display: flex; align-items: center; gap: 10px; cursor: pointer; }
  .next .bar { width: 4px; align-self: stretch; border-radius: 2px; flex-shrink: 0; }
  .next .info { flex: 1 1 auto; min-width: 0; }
  .next .when { font-size: 11px; font-weight: 600; text-transform: uppercase; letter-spacing: 0.3px; color: var(--label-2); }
  .next .when.soon { color: var(--accent); }
  .next .when.now { color: var(--ok); }
  .next .title { font-size: 14px; font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; margin-top: 1px; }
  .next .sub { font-size: 12px; color: var(--label-2); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .join { height: 28px; padding: 0 12px; font-size: 13px; font-weight: 600; display: flex; align-items: center; gap: 6px; }
  .join .k { font-weight: 500; opacity: 0.75; font-size: 11px; }

  #quick { margin: 10px 10px 0; position: relative; }
  #quick-input { width: 100%; box-sizing: border-box; height: 30px; padding: 0 10px 0 28px; border: none; border-radius: var(--r-ctl); background: var(--fill); outline: none; }
  #quick-input:focus { background: var(--card); box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 45%, transparent); }
  #quick-input::placeholder { color: var(--label-3); }
  #quick .plus { position: absolute; left: 9px; top: 7px; width: 14px; height: 14px; color: var(--label-3); pointer-events: none; }
  #quick-preview { font-size: 11.5px; color: var(--label-2); padding: 5px 2px 0; min-height: 15px; display: none; line-height: 1.4; }
  #quick-preview.show { display: block; }
  #quick-preview b { color: var(--label); font-weight: 600; }
  #quick-preview .hint { color: var(--label-3); }

  #nav { display: flex; align-items: center; gap: 4px; padding: 10px 10px 4px; -webkit-user-select: none; user-select: none; }
  #nav .label { font-size: 15px; font-weight: 600; margin-left: 4px; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  #nav .label span { color: var(--label-2); font-weight: 500; }
  /* Docked at its narrowest, "Today" says enough; the strip shows the date. */
  @media (max-width: 400px) { #panel:not(.fullscreen) #nav .label span { display: none; } }
  .views .tab { font-size: 12px; padding: 0 8px; }

  #strip { display: grid; grid-template-columns: repeat(7, 1fr); gap: 2px; padding: 2px 10px 6px; -webkit-user-select: none; user-select: none; }
  #panel.fullscreen #strip { display: none; }
  .sd { border: none; background: transparent; border-radius: 8px; padding: 3px 0 4px; cursor: pointer; display: flex; flex-direction: column; align-items: center; gap: 1px; }
  .sd:hover { background: var(--fill); }
  .sd .w { font-size: 10px; color: var(--label-3); font-weight: 600; text-transform: uppercase; }
  .sd .n { font-size: 13px; width: 24px; height: 24px; line-height: 24px; text-align: center; border-radius: 50%; font-weight: 500; }
  .sd.today .n { color: var(--accent); font-weight: 700; }
  .sd.sel .n { background: var(--label); color: var(--canvas); }
  .sd.today.sel .n { background: var(--accent); color: var(--on-accent); }
  .sd .dots { height: 4px; display: flex; gap: 2px; }
  .sd .dots i { width: 4px; height: 4px; border-radius: 50%; background: var(--label-3); }

  #view { flex: 1 1 auto; min-height: 0; overflow-y: auto; outline: none; position: relative; }
  .day-h { position: sticky; top: 0; z-index: 2; background: var(--canvas); padding: 10px 14px 4px; font-size: 12px; font-weight: 600; color: var(--label-2); display: flex; gap: 6px; align-items: baseline; }
  .day-h.today { color: var(--accent); }
  .day-h .d { color: var(--label-3); font-weight: 500; }
  .empty-day { padding: 2px 14px 8px; color: var(--label-3); font-size: 12px; }
  .row { display: flex; gap: 8px; padding: 6px 10px 6px 14px; margin: 0 4px; border-radius: 8px; cursor: pointer; align-items: flex-start; }
  .row:hover { background: var(--fill); }
  .row.sel { background: color-mix(in srgb, var(--accent) 14%, transparent); }
  .row.past { opacity: 0.5; }
  .row.declined .t1 { text-decoration: line-through; color: var(--label-2); }
  .row .time { width: 46px; flex-shrink: 0; font-size: 12px; font-variant-numeric: tabular-nums; color: var(--label); padding-top: 1px; }
  .row .time .e { display: block; color: var(--label-3); font-size: 11px; }
  .row .stripe { width: 3px; border-radius: 2px; align-self: stretch; flex-shrink: 0; }
  .row .body { flex: 1 1 auto; min-width: 0; }
  .row .t1 { font-weight: 500; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .row .t2 { font-size: 12px; color: var(--label-2); white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .row .mini-join { flex-shrink: 0; height: 22px; font-size: 11.5px; padding: 0 8px; }
  .tag { display: inline-block; font-size: 10.5px; font-weight: 600; padding: 0 5px; border-radius: 4px; line-height: 15px; margin-left: 4px; vertical-align: 1px; }
  .tag.invite { background: color-mix(in srgb, var(--accent) 16%, transparent); color: var(--accent); }
  .tag.maybe { background: var(--fill); color: var(--label-2); }
  .allday { display: flex; flex-wrap: wrap; gap: 4px; padding: 2px 14px 4px; }
  .chip { font-size: 12px; padding: 2px 8px; border-radius: 6px; cursor: pointer; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; max-width: 100%; box-sizing: border-box; }
  .chip.sel { outline: 2px solid var(--accent); }
  .nowline { display: flex; align-items: center; gap: 6px; margin: 1px 14px; color: var(--err); font-size: 10.5px; font-weight: 600; }
  .nowline::after { content: ""; flex: 1; height: 1.5px; background: var(--err); border-radius: 1px; }
  .end-note { padding: 14px; text-align: center; color: var(--label-3); font-size: 12px; }

  /* Day and week grid */
  .grid { position: relative; display: grid; }
  .g-top { position: sticky; top: 0; z-index: 4; background: var(--canvas); box-shadow: 0 0.5px 0 var(--hairline); }
  .g-head { display: grid; }
  .g-allday { display: grid; max-height: 84px; overflow-y: auto; }
  .g-allday .lbl { font-size: 10px; color: var(--label-3); text-align: right; padding: 4px 6px 0 0; }
  .g-head .c { padding: 4px 4px 4px 6px; font-size: 11.5px; font-weight: 600; color: var(--label-2); cursor: pointer; white-space: nowrap; overflow: hidden; }
  .g-head .c.today { color: var(--accent); }
  .g-all { display: flex; flex-direction: column; gap: 2px; padding: 2px; min-height: 0; }
  .g-all .chip { font-size: 11px; padding: 1px 5px; }
  .g-body { position: relative; display: grid; }
  .g-hours { position: relative; }
  .g-hours .h { position: absolute; right: 6px; font-size: 10.5px; color: var(--label-3); transform: translateY(-6px); font-variant-numeric: tabular-nums; }
  .g-col { position: relative; border-left: 0.5px solid var(--hairline); }
  .g-col .line { position: absolute; left: 0; right: 0; height: 0; border-top: 0.5px solid var(--hairline); }
  .g-col .half { border-top-style: dotted; opacity: 0.6; }
  .g-ev { position: absolute; box-sizing: border-box; border-radius: 5px; padding: 2px 5px; font-size: 11px; line-height: 1.25; overflow: hidden; cursor: pointer; border-left: 3px solid; }
  .g-ev .gt { font-weight: 600; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .g-ev .gs { opacity: 0.8; white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .g-ev.sel { box-shadow: 0 0 0 2px var(--accent); z-index: 2; }
  .g-ev.past { opacity: 0.55; }
  .g-ev.declined { opacity: 0.45; text-decoration: line-through; }
  .g-now { position: absolute; left: 0; right: 0; height: 0; border-top: 1.5px solid var(--err); z-index: 3; pointer-events: none; }
  .g-now::before { content: ""; position: absolute; left: -4px; top: -4.5px; width: 7px; height: 7px; border-radius: 50%; background: var(--err); }

  /* Sheets: details and the form, over the list */
  #sheet { position: absolute; left: 0; right: 0; bottom: 0; top: 32px; background: var(--canvas); z-index: 10; display: none; flex-direction: column; }
  #sheet.open { display: flex; animation: rise 0.18s var(--ease) both; }
  @keyframes rise { from { opacity: 0; transform: translateY(6px); } to { opacity: 1; transform: none; } }
  .sh-bar { flex: 0 0 auto; display: flex; align-items: center; gap: 6px; padding: 8px 10px; box-shadow: 0 0.5px 0 var(--hairline); }
  .sh-body { flex: 1 1 auto; overflow-y: auto; padding: 14px 16px 24px; }
  .sh-body h2 { font-size: 17px; font-weight: 650; margin: 0 0 6px; line-height: 1.3; overflow-wrap: anywhere; }
  .meta { display: flex; gap: 8px; align-items: flex-start; margin: 8px 0; font-size: 13px; line-height: 1.45; }
  .meta .ic { width: 16px; flex-shrink: 0; color: var(--label-3); text-align: center; padding-top: 1px; }
  .meta .v { flex: 1 1 auto; min-width: 0; overflow-wrap: anywhere; }
  .meta .sub { color: var(--label-2); font-size: 12px; }
  .dot { display: inline-block; width: 9px; height: 9px; border-radius: 50%; vertical-align: 0; margin-right: 5px; }
  .rsvp { display: flex; align-items: center; gap: 6px; margin: 12px 0; padding: 8px 10px; border-radius: var(--r-ctl); background: var(--fill); }
  .rsvp .q { flex: 1; font-weight: 500; }
  .guests { margin: 4px 0 0; padding: 0; list-style: none; }
  .guests li { display: flex; gap: 8px; align-items: center; padding: 3px 0; font-size: 12.5px; }
  .guests .st { width: 16px; text-align: center; font-size: 11px; flex-shrink: 0; }
  .guests .st.accepted { color: var(--ok); } .guests .st.declined { color: var(--err); } .guests .st.tentative { color: var(--warn); } .guests .st.needs { color: var(--label-3); }
  .guests .nm { flex: 1; min-width: 0; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  .guests .role { color: var(--label-3); font-size: 11px; }
  .desc { white-space: pre-wrap; font-size: 12.5px; line-height: 1.5; color: var(--label); margin-top: 14px; padding-top: 12px; border-top: 0.5px solid var(--hairline); overflow-wrap: anywhere; }
  .actions { display: flex; flex-wrap: wrap; gap: 6px; margin-top: 16px; }
  .confirm { display: flex; gap: 6px; align-items: center; flex-wrap: wrap; margin-top: 10px; font-size: 12px; color: var(--label-2); }

  .form .f { margin: 0 0 10px; }
  .form label.l { display: block; font-size: 11px; font-weight: 600; color: var(--label-2); margin-bottom: 3px; text-transform: uppercase; letter-spacing: 0.3px; }
  .form input[type=text], .form input[type=date], .form input[type=time], .form select, .form textarea { box-sizing: border-box; width: 100%; border: none; border-radius: var(--r-ctl); background: var(--fill); padding: 6px 8px; outline: none; font-size: 13px; }
  .form input:focus, .form select:focus, .form textarea:focus { background: var(--card); box-shadow: 0 0 0 2px color-mix(in srgb, var(--accent) 45%, transparent); }
  .form #f-title { font-size: 16px; font-weight: 600; padding: 8px; }
  .form .two { display: grid; grid-template-columns: 1fr 1fr; gap: 6px; }
  .form .two.one { grid-template-columns: 1fr; }
  .form .check { display: flex; align-items: center; gap: 6px; font-size: 13px; cursor: pointer; }
  .form textarea { resize: vertical; min-height: 64px; line-height: 1.45; }
  .form .err { color: var(--err); font-size: 12px; min-height: 16px; margin-top: 4px; }
  .form .note { color: var(--label-3); font-size: 11.5px; margin-top: 3px; }

  #pop { position: absolute; top: 34px; right: 8px; width: 290px; max-height: 70%; overflow-y: auto; z-index: 20; display: none; padding: 8px; background: var(--glass); border-radius: var(--r-card); box-shadow: var(--shadow-float); }
  #pop.open { display: block; }
  #pop .acct { font-size: 11px; font-weight: 600; color: var(--label-2); padding: 8px 6px 3px; display: flex; gap: 6px; align-items: center; }
  #pop .acct .state { font-weight: 500; color: var(--label-3); flex: 1; text-align: right; overflow: hidden; text-overflow: ellipsis; white-space: nowrap; }
  #pop label { display: flex; align-items: center; gap: 8px; padding: 4px 6px; border-radius: 6px; cursor: pointer; font-size: 13px; }
  #pop label:hover { background: var(--fill); }
  #pop input[type=checkbox] { accent-color: var(--accent); margin: 0; }
  #pop .sw { width: 10px; height: 10px; border-radius: 3px; flex-shrink: 0; }
  #pop .foot { border-top: 0.5px solid var(--hairline); margin-top: 6px; padding: 8px 6px 2px; display: flex; gap: 6px; flex-wrap: wrap; }

  .empty { padding: 40px 24px; text-align: center; color: var(--label-2); line-height: 1.5; }
  .empty h3 { color: var(--label); font-size: 15px; margin: 0 0 6px; }
</style>
</head>
<body>
<div id="panel">
  <div id="resize-handle" role="separator" aria-label="Resize pane" aria-orientation="vertical" tabindex="0"></div>
  <div id="bar">
    <div class="seg" role="tablist">
      <button class="tab" id="tab-notes" type="button" role="tab">Notes</button>
      <button class="tab" id="tab-mail" type="button" role="tab">Mail <span class="badge" id="unread"></span></button>
      <button class="tab active" id="tab-cal" type="button" role="tab" aria-selected="true">Calendar</button>
    </div>
    <span class="spacer"></span>
    <button class="icon-btn" id="share-btn" type="button" title="Copy my free times (s)" aria-label="Copy availability">
      <svg viewBox="0 0 14 14" fill="none"><rect x="2" y="3" width="10" height="9" rx="1.5" stroke="currentColor" stroke-width="1.2"/><path d="M2 6h10M5 1.5v3M9 1.5v3M5 8.8l1.4 1.3L9.2 7.4" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/></svg>
    </button>
    <button class="icon-btn" id="sync-btn" type="button" title="Sync now (r)" aria-label="Sync now">
      <svg viewBox="0 0 14 14" fill="none"><path d="M12 7a5 5 0 01-8.6 3.5M2 7a5 5 0 018.6-3.5M10.6 1.2v2.3H8.3M3.4 12.8v-2.3h2.3" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/></svg>
    </button>
    <button class="icon-btn" id="cals-btn" type="button" title="Calendars" aria-label="Calendars" aria-haspopup="true">
      <svg viewBox="0 0 14 14" fill="none"><path d="M2 3.5h10M2 7h10M2 10.5h10" stroke="currentColor" stroke-width="1.2" stroke-linecap="round"/><circle cx="4.5" cy="3.5" r="1.3" fill="var(--canvas)" stroke="currentColor" stroke-width="1.1"/><circle cx="9.5" cy="7" r="1.3" fill="var(--canvas)" stroke="currentColor" stroke-width="1.1"/><circle cx="6" cy="10.5" r="1.3" fill="var(--canvas)" stroke="currentColor" stroke-width="1.1"/></svg>
    </button>
    <button class="icon-btn" id="fullscreen-btn" type="button" title="Toggle fullscreen" aria-label="Toggle calendar fullscreen">
      <svg class="ic-enter" viewBox="0 0 14 14" fill="none"><path d="M2 5.5V2h3.5M12 5.5V2H8.5M2 8.5V12h3.5M12 8.5V12H8.5" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/></svg>
      <svg class="ic-exit" viewBox="0 0 14 14" fill="none" style="display:none"><path d="M5.5 2v3.5H2M8.5 2v3.5H12M5.5 12V8.5H2M8.5 12V8.5H12" stroke="currentColor" stroke-width="1.2" stroke-linecap="round" stroke-linejoin="round"/></svg>
    </button>
    <button class="icon-btn" id="close-btn" type="button" title="Hide" aria-label="Hide calendar">
      <svg viewBox="0 0 8 8" fill="none" style="width:8px;height:8px"><path d="M1 1L7 7M7 1L1 7" stroke="currentColor" stroke-width="1.6" stroke-linecap="round"/></svg>
    </button>
  </div>
  <div id="main">
    <div id="banner"></div>
    <div id="next"></div>
    <div id="quick">
      <svg class="plus" viewBox="0 0 14 14" fill="none"><path d="M7 2.5v9M2.5 7h9" stroke="currentColor" stroke-width="1.4" stroke-linecap="round"/></svg>
      <input id="quick-input" type="text" spellcheck="false" autocomplete="off" placeholder="Add: Lunch with anna@x.com tomorrow 1pm" aria-label="Quick add">
      <div id="quick-preview" aria-live="polite"></div>
    </div>
    <div id="nav">
      <button class="icon-btn" id="prev-btn" type="button" title="Previous (←)" aria-label="Previous"><svg viewBox="0 0 14 14" fill="none"><path d="M8.5 3L4.5 7l4 4" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/></svg></button>
      <button class="btn" id="today-btn" type="button" title="Today (t)">Today</button>
      <button class="icon-btn" id="next-btn" type="button" title="Next (→)" aria-label="Next"><svg viewBox="0 0 14 14" fill="none"><path d="M5.5 3l4 4-4 4" stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round"/></svg></button>
      <span class="label" id="nav-label"></span>
      <span class="spacer"></span>
      <div class="seg views" role="tablist" aria-label="View">
        <button class="tab" data-view="agenda" type="button">List</button>
        <button class="tab" data-view="day" type="button">Day</button>
        <button class="tab" data-view="week" type="button">Week</button>
      </div>
    </div>
    <div id="strip"></div>
    <div id="view" tabindex="0" aria-label="Events"></div>
  </div>
  <div id="sheet" role="dialog"></div>
  <div id="pop" role="menu"></div>
</div>
<script>
(function() {
  'use strict';
  const $ = function(id) { return document.getElementById(id); };
  function ipc(msg) { window.ipc.postMessage(JSON.stringify(msg)); }
  function esc(s) { return String(s == null ? '' : s).replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;'); }

  const DAY = 86400000;
  const S = {
    ws: null, accounts: [], calendars: [], hidden: [], prefs: {}, status: {},
    view: 'agenda', day: midnight(new Date()), events: [], range: null, loaded: false,
    sel: null, sheet: null, next: null, fullscreen: false, chords: {}, op: 0, ops: {},
    quickId: 0, quick: null, syncing: false, pickedView: false
  };

  // ── Dates ──────────────────────────────────────────────────────────────────
  function midnight(d) { const x = new Date(d); x.setHours(0, 0, 0, 0); return x; }
  function addDays(d, n) { const x = new Date(d); x.setDate(x.getDate() + n); return x; }
  function weekStart(d) { const x = midnight(d); const wd = (x.getDay() + 6) % 7; return addDays(x, -wd); }
  function sameDay(a, b) { return a.getFullYear() === b.getFullYear() && a.getMonth() === b.getMonth() && a.getDate() === b.getDate(); }
  function iso(d) { return d.getFullYear() + '-' + String(d.getMonth() + 1).padStart(2, '0') + '-' + String(d.getDate()).padStart(2, '0'); }
  function fromIso(s) { const p = String(s).split('-').map(Number); return new Date(p[0], p[1] - 1, p[2]); }
  function hhmm(d) { return String(d.getHours()).padStart(2, '0') + ':' + String(d.getMinutes()).padStart(2, '0'); }
  const TIME = new Intl.DateTimeFormat(undefined, { hour: 'numeric', minute: '2-digit' });
  function fmtTime(ts) { return TIME.format(new Date(ts * 1000)).replace(/\s?([AP])M$/i, function(_, a) { return a.toLowerCase() + 'm'; }); }
  const WD = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
  const MON = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
  function dayName(d) {
    const today = midnight(new Date());
    const diff = Math.round((midnight(d) - today) / DAY);
    const base = WD[d.getDay()] + ' ' + d.getDate() + ' ' + MON[d.getMonth()];
    if (diff === 0) return 'Today';
    if (diff === 1) return 'Tomorrow';
    if (diff === -1) return 'Yesterday';
    return base;
  }
  function longDate(d) { return WD[d.getDay()] + ', ' + d.getDate() + ' ' + MON[d.getMonth()] + (d.getFullYear() !== new Date().getFullYear() ? ' ' + d.getFullYear() : ''); }
  function duration(sec) {
    const m = Math.round(sec / 60);
    if (m < 60) return m + ' min';
    const h = Math.floor(m / 60), r = m % 60;
    return h + ' h' + (r ? ' ' + r + ' min' : '');
  }
  function relative(sec) {
    const m = Math.round(sec / 60);
    if (m <= 0) return 'now';
    if (m < 60) return 'in ' + m + ' min';
    const h = Math.floor(m / 60), r = m % 60;
    return 'in ' + h + ' h' + (r ? ' ' + r + ' min' : '');
  }
  function nowSec() { return Date.now() / 1000; }

  // ── Calendars and events ───────────────────────────────────────────────────
  function cal(key) { return S.calendars.find(function(c) { return c.key === key; }); }
  function color(e) { const c = cal(e.calendar); return (c && c.color) || '#888'; }
  function account(key) { return S.accounts.find(function(a) { return a.key === key; }); }
  function visible() { return S.events.filter(function(e) { return S.hidden.indexOf(e.calendar) < 0; }); }
  function eventDays(e) {
    // Local days an occurrence touches, as midnights.
    const out = [];
    let d = e.all_day ? fromIso(e.start_date) : midnight(new Date(e.start * 1000));
    const end = e.all_day ? fromIso(e.end_date) : new Date(Math.max(e.end, e.start + 1) * 1000 - 1);
    while (d <= end && out.length < 62) { if (!e.all_day || d < end) out.push(d); d = addDays(d, 1); }
    return out.length ? out : [midnight(new Date(e.start * 1000))];
  }
  function onDay(d) {
    const from = d.getTime() / 1000, to = addDays(d, 1).getTime() / 1000;
    return visible().filter(function(e) {
      if (e.all_day) return fromIso(e.start_date) <= d && fromIso(e.end_date) > d;
      return e.end > from && e.start < to || (e.start === e.end && e.start >= from && e.start < to);
    }).sort(function(a, b) { return (b.all_day - a.all_day) || (a.start - b.start) || (a.end - b.end); });
  }
  function service(url) {
    if (!url) return '';
    if (/meet\.google\.com/.test(url)) return 'Google Meet';
    if (/zoom/.test(url)) return 'Zoom';
    if (/teams\.(microsoft|live)/.test(url)) return 'Teams';
    if (/webex/.test(url)) return 'Webex';
    if (/slack\.com\/huddle/.test(url)) return 'Slack huddle';
    if (/whereby/.test(url)) return 'Whereby';
    if (/jit\.si/.test(url)) return 'Jitsi';
    if (/facetime/.test(url)) return 'FaceTime';
    return 'video call';
  }
  function backendOf(e) { const a = account(e.account || (cal(e.calendar) || {}).account); return a ? a.kind : ''; }

  // ── Requests ───────────────────────────────────────────────────────────────
  function wantRange() {
    // One request covers the strip, the day, the week and two weeks of list.
    const from = addDays(weekStart(S.day), 0);
    const to = addDays(from, S.view === 'agenda' ? 28 : 14);
    return { from: from.getTime() / 1000, to: to.getTime() / 1000 };
  }
  function load() {
    const r = wantRange();
    S.range = r;
    ipc({ type: 'range', from: r.from, to: r.to });
  }

  // ── Rendering ──────────────────────────────────────────────────────────────
  function render() {
    renderBanner();
    renderNext();
    renderNav();
    renderStrip();
    if (!S.accounts.length) {
      $('view').innerHTML = '<div class="empty"><h3>No calendars yet</h3>The calendars of your mail accounts show here: Google Calendar for Gmail, Outlook for Microsoft 365, iCloud and CalDAV for the rest.<br><br><button class="btn primary" id="go-settings">Add an account in Settings → Mail</button></div>';
      $('go-settings').onclick = function() { ipc({ type: 'settings' }); };
      return;
    }
    if (S.view === 'agenda') renderAgenda(); else renderGrid(S.view === 'week' ? 7 : 1);
  }

  function renderBanner() {
    const parts = [];
    S.accounts.forEach(function(a) {
      const st = S.status[a.key] || { state: a.state, message: a.message };
      if (st.state === 'connect' || st.state === 'auth') {
        const what = a.kind === 'graph' ? 'Outlook calendar' : 'Google Calendar';
        parts.push('<div class="banner"><div class="text">' + (st.state === 'auth' ? 'Sign in to ' + what + ' again' : 'Connect ' + what) +
          ' <span class="who">· ' + esc(a.address) + '</span></div><button class="btn primary" data-connect="' + esc(a.key) + '">Connect</button></div>');
      } else if (st.state === 'error' && st.message) {
        parts.push('<div class="banner err"><div class="text"><b>' + esc(a.address) + '</b>: ' + linkify(st.message) + '</div></div>');
      }
    });
    $('banner').innerHTML = parts.join('');
  }

  function joinButton(e, small) {
    if (!e.join) return '';
    const label = small ? 'Join' : 'Join ' + esc(service(e.join));
    const chord = !small && S.chords.join ? '<span class="k">' + esc(S.chords.join) + '</span>' : '';
    return '<button class="btn primary join' + (small ? ' mini-join' : '') + '" data-join="' + esc(e.join) + '" title="' + esc(e.join) + '">' + label + chord + '</button>';
  }

  function renderNext() {
    const e = S.next;
    if (!e) { $('next').innerHTML = ''; return; }
    const now = nowSec();
    let when, cls = '';
    if (e.start <= now) { when = 'Now · ' + duration(e.end - now) + ' left'; cls = 'now'; }
    else { when = relative(e.start - now) + ' · ' + fmtTime(e.start) + '–' + fmtTime(e.end); if (e.start - now <= 600) cls = 'soon'; }
    const sub = e.location && !/^https?:/.test(e.location) ? e.location : (e.join ? service(e.join) : (cal(e.calendar) || {}).name || '');
    $('next').innerHTML = '<div class="next" data-open="' + esc(e.key) + '"><div class="bar" style="background:' + esc(color(e)) + '"></div>' +
      '<div class="info"><div class="when ' + cls + '">' + esc(when) + '</div><div class="title">' + esc(e.title) + '</div>' +
      (sub ? '<div class="sub">' + esc(sub) + '</div>' : '') + '</div>' + joinButton(e, false) + '</div>';
  }

  function renderNav() {
    let label;
    if (S.view === 'week') {
      const ws = weekStart(S.day), we = addDays(ws, 6);
      label = MON[ws.getMonth()] + ' ' + ws.getDate() + ' – ' + (we.getMonth() !== ws.getMonth() ? MON[we.getMonth()] + ' ' : '') + we.getDate() + ' <span>' + we.getFullYear() + '</span>';
    } else {
      label = esc(dayName(S.day)) + (dayName(S.day).length < 10 ? ' <span>' + S.day.getDate() + ' ' + MON[S.day.getMonth()] + '</span>' : '');
    }
    $('nav-label').innerHTML = label;
    document.querySelectorAll('.views .tab').forEach(function(b) { b.classList.toggle('active', b.dataset.view === S.view); });
  }

  function renderStrip() {
    const ws = weekStart(S.day), today = midnight(new Date());
    let html = '';
    for (let i = 0; i < 7; i++) {
      const d = addDays(ws, i);
      const n = S.loaded ? Math.min(3, onDay(d).length) : 0;
      html += '<button class="sd' + (sameDay(d, today) ? ' today' : '') + (sameDay(d, S.day) ? ' sel' : '') + '" data-day="' + iso(d) + '" type="button">' +
        '<span class="w">' + WD[d.getDay()].slice(0, 2) + '</span><span class="n">' + d.getDate() + '</span><span class="dots">' + '<i></i>'.repeat(n) + '</span></button>';
    }
    $('strip').innerHTML = html;
  }

  function tags(e) {
    if (e.me === 'needs') return '<span class="tag invite">Invited</span>';
    if (e.me === 'tentative') return '<span class="tag maybe">Maybe</span>';
    return '';
  }

  function subline(e) {
    const bits = [];
    if (e.location && !/^https?:\/\//.test(e.location)) bits.push(e.location);
    if (e.join) bits.push(service(e.join));
    const guests = e.attendees.filter(function(a) { return !a.me; }).length;
    if (guests) bits.push(guests + (guests === 1 ? ' guest' : ' guests'));
    if (e.recurring) bits.push('↻');
    return bits.join(' · ');
  }

  function renderAgenda() {
    if (!S.loaded) { $('view').innerHTML = '<div class="end-note">Loading…</div>'; return; }
    const today = midnight(new Date()), now = nowSec();
    let html = '';
    const days = 14;
    let shown = 0;
    for (let i = 0; i < days; i++) {
      const d = addDays(S.day, i);
      const all = onDay(d);
      const isToday = sameDay(d, today);
      if (!all.length && !isToday && i !== 0) continue;
      shown++;
      html += '<div class="day-h' + (isToday ? ' today' : '') + '">' + esc(dayName(d)) + (dayName(d).length < 10 ? ' <span class="d">' + WD[d.getDay()] + ' ' + d.getDate() + ' ' + MON[d.getMonth()] + '</span>' : '') + '</div>';
      const allDay = all.filter(function(e) { return e.all_day; });
      const timed = all.filter(function(e) { return !e.all_day; });
      if (allDay.length) {
        html += '<div class="allday">' + allDay.map(function(e) {
          const c = color(e);
          return '<div class="chip' + (S.sel === e.key ? ' sel' : '') + '" data-open="' + esc(e.key) + '" style="background:color-mix(in srgb,' + esc(c) + ' 18%,transparent);color:var(--label)" title="' + esc(e.title) + '">' + esc(e.title) + '</div>';
        }).join('') + '</div>';
      }
      if (!timed.length && !allDay.length) html += '<div class="empty-day">Nothing scheduled</div>';
      let nowDrawn = !isToday;
      timed.forEach(function(e) {
        if (!nowDrawn && e.start > now) { html += '<div class="nowline">' + esc(fmtTime(now)) + '</div>'; nowDrawn = true; }
        const past = e.end <= now;
        const startsHere = sameDay(new Date(e.start * 1000), d);
        html += '<div class="row' + (past ? ' past' : '') + (e.declined || e.me === 'declined' ? ' declined' : '') + (S.sel === e.key ? ' sel' : '') + '" data-open="' + esc(e.key) + '">' +
          '<div class="time">' + (startsHere ? esc(fmtTime(e.start)) : '…') + '<span class="e">' + esc(fmtTime(e.end)) + '</span></div>' +
          '<div class="stripe" style="background:' + esc(color(e)) + '"></div>' +
          '<div class="body"><div class="t1">' + esc(e.title) + tags(e) + '</div><div class="t2">' + esc(subline(e)) + '</div></div>' +
          (!past && e.join && e.start - now < 3600 * 12 && isToday ? joinButton(e, true) : '') + '</div>';
      });
      if (!nowDrawn && timed.length) html += '<div class="nowline">' + esc(fmtTime(now)) + '</div>';
    }
    if (!shown) html += '<div class="empty-day">Nothing in the next two weeks</div>';
    html += '<div class="end-note">Showing two weeks from ' + esc(dayName(S.day).toLowerCase() === 'today' ? 'today' : longDate(S.day)) + '</div>';
    const keep = $('view').scrollTop;
    $('view').innerHTML = html;
    $('view').scrollTop = S.keepScroll ? keep : 0;
    S.keepScroll = false;
  }

  // Overlapping events side by side: greedy columns per cluster.
  function layout(events) {
    const sorted = events.slice().sort(function(a, b) { return a.start - b.start || b.end - a.end; });
    const out = [];
    let cluster = [], clusterEnd = -Infinity;
    function flush() {
      const cols = [];
      cluster.forEach(function(e) {
        let c = cols.findIndex(function(end) { return end <= e.start; });
        if (c < 0) { c = cols.length; cols.push(0); }
        cols[c] = Math.max(e.end, e.start + 900);
        out.push({ e: e, col: c });
      });
      out.slice(out.length - cluster.length).forEach(function(x) { x.cols = cols.length; });
      cluster = []; clusterEnd = -Infinity;
    }
    sorted.forEach(function(e) {
      if (e.start >= clusterEnd && cluster.length) flush();
      cluster.push(e);
      clusterEnd = Math.max(clusterEnd, Math.max(e.end, e.start + 900));
    });
    if (cluster.length) flush();
    return out;
  }

  function renderGrid(n) {
    if (!S.loaded) { $('view').innerHTML = '<div class="end-note">Loading…</div>'; return; }
    const H = n === 1 ? 46 : 42;
    const first = n === 1 ? S.day : weekStart(S.day);
    const today = midnight(new Date()), now = nowSec();
    const cols = 'grid-template-columns: 44px repeat(' + n + ', minmax(0, 1fr));';
    let head = '<div class="g-head" style="' + cols + '"><div></div>';
    let allRow = '<div class="g-allday" style="' + cols + '"><div class="lbl">all-day</div>';
    let hasAll = false;
    let body = '<div class="g-body" style="' + cols + 'height:' + (24 * H) + 'px"><div class="g-hours">';
    for (let h = 1; h < 24; h++) body += '<div class="h" style="top:' + (h * H) + 'px">' + (h < 10 ? '0' : '') + h + ':00</div>';
    body += '</div>';
    for (let i = 0; i < n; i++) {
      const d = addDays(first, i);
      const all = onDay(d);
      if (n > 1) head += '<div class="c' + (sameDay(d, today) ? ' today' : '') + '" data-day="' + iso(d) + '">' + WD[d.getDay()] + ' ' + d.getDate() + '</div>';
      const ad = all.filter(function(e) { return e.all_day; });
      if (ad.length) hasAll = true;
      allRow += '<div class="g-all">' + ad.map(function(e) {
        return '<div class="chip' + (S.sel === e.key ? ' sel' : '') + '" data-open="' + esc(e.key) + '" style="background:color-mix(in srgb,' + esc(color(e)) + ' 20%,transparent)">' + esc(e.title) + '</div>';
      }).join('') + '</div>';
      body += '<div class="g-col" data-col="' + iso(d) + '">';
      for (let h = 0; h < 24; h++) body += '<div class="line" style="top:' + (h * H) + 'px"></div><div class="line half" style="top:' + (h * H + H / 2) + 'px"></div>';
      const dayStart = d.getTime() / 1000, dayEnd = addDays(d, 1).getTime() / 1000;
      layout(all.filter(function(e) { return !e.all_day; })).forEach(function(x) {
        const e = x.e;
        const s = Math.max(e.start, dayStart), en = Math.min(Math.max(e.end, e.start + 900), dayEnd);
        const top = (s - dayStart) / 3600 * H, height = Math.max(16, (en - s) / 3600 * H - 2);
        const w = 100 / x.cols;
        const c = color(e);
        body += '<div class="g-ev' + (S.sel === e.key ? ' sel' : '') + (e.end <= now ? ' past' : '') + (e.me === 'declined' ? ' declined' : '') + '" data-open="' + esc(e.key) + '" style="top:' + top + 'px;height:' + height + 'px;left:calc(' + (x.col * w) + '% + 1px);width:calc(' + w + '% - 3px);background:color-mix(in srgb,' + esc(c) + ' 22%,var(--canvas));border-left-color:' + esc(c) + '" title="' + esc(e.title + ' · ' + fmtTime(e.start) + '–' + fmtTime(e.end)) + '">' +
          '<div class="gt">' + esc(e.title) + '</div>' + (height > 30 ? '<div class="gs">' + esc(fmtTime(e.start) + (e.location && !/^https?:/.test(e.location) ? ' · ' + e.location : '')) + '</div>' : '') + '</div>';
      });
      if (sameDay(d, today)) body += '<div class="g-now" style="top:' + ((now - dayStart) / 3600 * H) + 'px"></div>';
      body += '</div>';
    }
    body += '</div>';
    head += '</div>';
    allRow += '</div>';
    const keep = $('view').scrollTop;
    const top = (n > 1 ? head : '') + (hasAll ? allRow : '');
    $('view').innerHTML = '<div class="grid">' + (top ? '<div class="g-top">' + top + '</div>' : '') + body + '</div>';
    if (S.keepScroll) { $('view').scrollTop = keep; }
    else {
      const showToday = n === 1 ? sameDay(S.day, today) : weekStart(today).getTime() === first.getTime();
      const anchorHour = showToday ? Math.max(0, new Date().getHours() - 1) : 8;
      $('view').scrollTop = anchorHour * H;
    }
    S.keepScroll = false;
  }

  function linkify(text) {
    return esc(text).replace(/https?:\/\/[^\s<>"']+/g, function(u) {
      const clean = u.replace(/[.,;:)\]]+$/, '');
      return '<a data-url="' + clean + '">' + clean + '</a>' + u.slice(clean.length);
    });
  }

  // ── Details ────────────────────────────────────────────────────────────────
  function find(key) { return S.events.find(function(e) { return e.key === key; }); }

  function whenText(e) {
    if (e.all_day) {
      const a = fromIso(e.start_date), b = addDays(fromIso(e.end_date), -1);
      return sameDay(a, b) ? longDate(a) + ' · all day' : longDate(a) + ' – ' + longDate(b);
    }
    const s = new Date(e.start * 1000), en = new Date(e.end * 1000);
    if (sameDay(s, new Date((e.end - 1) * 1000))) return longDate(s) + ' · ' + fmtTime(e.start) + ' – ' + fmtTime(e.end);
    return longDate(s) + ' ' + fmtTime(e.start) + ' – ' + longDate(en) + ' ' + fmtTime(e.end);
  }

  function openDetail(e) {
    if (!e) return;
    S.sel = e.key;
    S.sheet = { kind: 'detail', key: e.key, event: e };
    renderSheet();
  }

  const STATUS_ICON = { accepted: '✓', declined: '✕', tentative: '?', needs: '•' };
  function renderDetail(e) {
    const c = cal(e.calendar) || {};
    const a = account(e.account) || {};
    const guests = e.attendees || [];
    const counts = { accepted: 0, declined: 0, tentative: 0, needs: 0 };
    guests.forEach(function(g) { counts[g.status] = (counts[g.status] || 0) + 1; });
    const pending = S.sheet.pending;
    let html = '<div class="sh-bar"><button class="btn" data-act="back">‹ Back</button><span class="spacer"></span>' +
      (e.editable ? '<button class="btn" data-act="edit" title="Edit (e)">Edit</button>' : '') +
      (e.editable || (e.recurring && backendOf(e) === 'dav') ? '<button class="btn danger" data-act="delete" title="Delete">Delete</button>' : '') + '</div>';
    html += '<div class="sh-body"><h2>' + esc(e.title) + '</h2>';
    html += '<div class="meta"><span class="ic">◷</span><div class="v">' + esc(whenText(e)) +
      (!e.all_day ? ' <span class="sub">(' + esc(duration(e.end - e.start)) + ')</span>' : '') + (e.recurring ? '<div class="sub">Repeats</div>' : '') + '</div></div>';
    html += '<div class="meta"><span class="ic"><span class="dot" style="background:' + esc(c.color || '#888') + ';margin:0"></span></span><div class="v">' + esc(c.name || '') + ' <span class="sub">· ' + esc(a.address || '') + '</span></div></div>';
    if (e.join) html += '<div class="meta"><span class="ic">▶</span><div class="v">' + joinButton(e, false) + '<div class="sub" style="margin-top:4px">' + linkify(e.join) + '</div></div></div>';
    if (e.location && e.location !== e.join) html += '<div class="meta"><span class="ic">⌖</span><div class="v">' + linkify(e.location) + '</div></div>';
    if (e.me) {
      const can = backendOf(e) !== 'dav';
      html += '<div class="rsvp"><span class="q">Going?</span>' + (can ? [['accepted', 'Yes'], ['tentative', 'Maybe'], ['declined', 'No']].map(function(r) {
        return '<button class="btn' + (e.me === r[0] ? ' on' : '') + '" data-rsvp="' + r[0] + '"' + (pending ? ' disabled' : '') + '>' + r[1] + '</button>';
      }).join('') : '<span class="sub">' + esc({ accepted: 'Yes', tentative: 'Maybe', declined: 'No', needs: 'Not answered' }[e.me] || e.me) + '</span>') + '</div>';
    }
    if (guests.length) {
      const summary = [counts.accepted + ' yes', counts.tentative ? counts.tentative + ' maybe' : '', counts.declined ? counts.declined + ' no' : '', counts.needs ? counts.needs + ' awaiting' : ''].filter(Boolean).join(', ');
      html += '<div class="meta"><span class="ic">☺</span><div class="v">' + guests.length + ' guests <span class="sub">· ' + esc(summary) + '</span><ul class="guests">' +
        guests.map(function(g) {
          return '<li><span class="st ' + esc(g.status) + '">' + (STATUS_ICON[g.status] || '•') + '</span><span class="nm" title="' + esc(g.email) + '">' + esc(g.name || g.email) + (g.me ? ' (you)' : '') + '</span>' +
            (g.organizer ? '<span class="role">organizer</span>' : g.optional ? '<span class="role">optional</span>' : '') + '</li>';
        }).join('') + '</ul></div></div>';
    }
    html += '<div class="actions"><button class="btn" data-act="notes" title="Add a meeting note to this workspace\'s notes">Meeting notes</button>' +
      (e.link ? '<button class="btn" data-url="' + esc(e.link) + '">Open in ' + (backendOf(e) === 'graph' ? 'Outlook' : backendOf(e) === 'google' ? 'Google Calendar' : 'browser') + '</button>' : '') + '</div>';
    if (S.sheet.confirm) {
      html += '<div class="confirm">' + (e.recurring ? 'Delete this repeating event:' : 'Delete this event' + (guests.length > 1 ? ' and notify guests?' : '?')) +
        (e.recurring ? '<button class="btn danger" data-del="one">Only this one</button><button class="btn danger" data-del="all">All events</button>' : '<button class="btn danger" data-del="one">Delete</button>') +
        '<button class="btn" data-act="nodelete">Cancel</button></div>';
    }
    if (e.description) html += '<div class="desc">' + linkify(e.description) + '</div>';
    html += '</div>';
    return html;
  }

  // ── Form ───────────────────────────────────────────────────────────────────
  function writable() { return S.calendars.filter(function(c) { return c.writable && S.hidden.indexOf(c.key) < 0; }); }
  function defaultCalendar() {
    const w = writable();
    const pref = w.find(function(c) { return c.key === S.prefs.default_calendar; });
    return pref || w.find(function(c) { return c.primary; }) || w[0] || null;
  }

  function openForm(draft, existing) {
    const c = defaultCalendar();
    if (!existing && !c) { ipc({ type: 'notice', text: 'No calendar here can take new events.' }); return; }
    const d = Object.assign({ key: '', calendar: c ? c.key : '', title: '', all_day: false, location: '', description: '', attendees: [], video: false, recur: '' }, draft || {});
    if (!d.start) {
      const s = new Date(Math.max(S.day.getTime(), Date.now()));
      if (!sameDay(s, S.day)) s.setTime(S.day.getTime() + 9 * 3600000);
      s.setMinutes(Math.ceil(s.getMinutes() / 30) * 30, 0, 0);
      d.start = s.getTime() / 1000; d.end = d.start + 3600;
    }
    if (!d.start_date) { d.start_date = iso(new Date(d.start * 1000)); d.end_date = iso(addDays(new Date(d.start * 1000), 1)); }
    if (!d.calendar && c) d.calendar = c.key;
    S.sheet = { kind: 'form', draft: d, existing: existing || null };
    renderSheet();
    setTimeout(function() { const t = $('f-title'); if (t) { t.focus(); t.setSelectionRange(t.value.length, t.value.length); } }, 30);
  }

  function renderForm() {
    const d = S.sheet.draft, existing = S.sheet.existing;
    const cals = existing ? [cal(existing.calendar)].filter(Boolean) : writable();
    const calKind = function(key) { const c = cal(key); const a = c && account(c.account); return a ? a.kind : ''; };
    const kind = calKind(d.calendar);
    const s = new Date(d.start * 1000), en = new Date(d.end * 1000);
    const lastDay = addDays(fromIso(d.end_date), -1);
    let html = '<div class="sh-bar"><button class="btn" data-act="cancel">Cancel</button><span class="spacer"></span>' +
      '<button class="btn primary" data-act="save" title="Save (⌘↩)"' + (S.sheet.saving ? ' disabled' : '') + '>' + (S.sheet.saving ? 'Saving…' : existing ? 'Save' : 'Add event') + '</button></div>';
    html += '<div class="sh-body form">';
    html += '<div class="f"><input type="text" id="f-title" placeholder="Title" value="' + esc(d.title) + '"></div>';
    html += '<div class="f"><label class="check"><input type="checkbox" id="f-allday"' + (d.all_day ? ' checked' : '') + '> All day</label></div>';
    if (d.all_day) {
      html += '<div class="f two"><div><label class="l">From</label><input type="date" id="f-sd" value="' + d.start_date + '"></div><div><label class="l">To</label><input type="date" id="f-ed" value="' + iso(lastDay) + '"></div></div>';
    } else {
      html += '<div class="f two"><div><label class="l">Starts</label><input type="date" id="f-sd" value="' + iso(s) + '"></div><div><label class="l">&nbsp;</label><input type="time" id="f-st" value="' + hhmm(s) + '"></div></div>';
      html += '<div class="f two"><div><label class="l">Ends</label><input type="date" id="f-ed" value="' + iso(en) + '"></div><div><label class="l">&nbsp;</label><input type="time" id="f-et" value="' + hhmm(en) + '"></div></div>';
    }
    if (!existing) {
      html += '<div class="f"><label class="l">Repeat</label><select id="f-recur">' + [['', 'Never'], ['daily', 'Every day'], ['weekdays', 'Every weekday'], ['weekly', 'Every week'], ['monthly', 'Every month'], ['yearly', 'Every year']].map(function(r) {
        return '<option value="' + r[0] + '"' + (d.recur === r[0] ? ' selected' : '') + '>' + r[1] + '</option>';
      }).join('') + '</select></div>';
    }
    html += '<div class="f"><label class="l">Calendar</label><select id="f-cal"' + (existing ? ' disabled' : '') + '>' + cals.map(function(c) {
      const a = account(c.account) || {};
      return '<option value="' + esc(c.key) + '"' + (c.key === d.calendar ? ' selected' : '') + '>' + esc(c.name + (c.name !== a.address ? ' — ' + a.address : '')) + '</option>';
    }).join('') + '</select></div>';
    html += '<div class="f"><label class="l">Guests</label><input type="text" id="f-guests" placeholder="Email addresses" value="' + esc(d.attendees.join(', ')) + '" spellcheck="false">' +
      '<div class="note">' + (existing ? 'Guests are notified of changes.' : 'Invitations go out when you add the event.') + '</div></div>';
    if ((kind === 'google' || kind === 'graph') && !(existing && existing.join)) {
      html += '<div class="f"><label class="check"><input type="checkbox" id="f-video"' + (d.video ? ' checked' : '') + '> Add ' + (kind === 'google' ? 'Google Meet' : 'Teams meeting') + ' link</label></div>';
    }
    html += '<div class="f"><label class="l">Location</label><input type="text" id="f-loc" value="' + esc(d.location) + '" placeholder="Place or link"></div>';
    html += '<div class="f"><label class="l">Notes</label><textarea id="f-desc">' + esc(d.description) + '</textarea></div>';
    html += '<div class="err" id="f-err">' + esc(S.sheet.error || '') + '</div></div>';
    return html;
  }

  function readForm() {
    const d = Object.assign({}, S.sheet.draft);
    const v = function(id) { const el = $(id); return el ? el.value : ''; };
    d.title = v('f-title');
    d.all_day = $('f-allday').checked;
    if (d.all_day) {
      const a = v('f-sd'), b = v('f-ed') || a;
      d.start_date = a; d.end_date = b ? iso(addDays(fromIso(b), 1)) : '';
      d.start = fromIso(a).getTime() / 1000; d.end = fromIso(d.end_date).getTime() / 1000;
    } else {
      const sd = v('f-sd'), st = v('f-st'), ed = v('f-ed') || sd, et = v('f-et');
      const start = new Date(sd + 'T' + (st || '09:00')), end = new Date(ed + 'T' + (et || st || '10:00'));
      d.start = Math.round(start.getTime() / 1000); d.end = Math.round(end.getTime() / 1000);
      d.start_date = iso(start); d.end_date = iso(addDays(start, 1));
    }
    if ($('f-recur')) d.recur = v('f-recur');
    if (!S.sheet.existing) d.calendar = v('f-cal') || d.calendar;
    d.attendees = v('f-guests').split(/[\s,;]+/).map(function(s) { return s.trim().replace(/^<|>$/g, ''); }).filter(Boolean);
    d.video = !!($('f-video') && $('f-video').checked);
    d.location = v('f-loc');
    d.description = v('f-desc');
    return d;
  }

  function saveForm() {
    if (!S.sheet || S.sheet.kind !== 'form' || S.sheet.saving) return;
    const d = readForm();
    if (!d.title.trim()) { S.sheet.draft = d; S.sheet.error = 'Give the event a title.'; renderSheet(); $('f-title').focus(); return; }
    if (d.end <= d.start) { S.sheet.draft = d; S.sheet.error = 'The event ends before it starts.'; renderSheet(); return; }
    S.sheet.draft = d; S.sheet.saving = true; S.sheet.error = '';
    const op = ++S.op;
    S.ops[op] = 'form';
    if (!S.sheet.existing && d.calendar) ipc({ type: 'default_calendar', key: d.calendar });
    ipc({ type: 'save', op: op, draft: d });
    renderSheet();
  }

  function renderSheet() {
    const sheet = $('sheet');
    if (!S.sheet) { sheet.classList.remove('open'); sheet.innerHTML = ''; return; }
    if (S.sheet.kind === 'detail') {
      const e = find(S.sheet.key) || S.sheet.event;
      S.sheet.event = e;
      sheet.innerHTML = renderDetail(e);
    } else {
      sheet.innerHTML = renderForm();
      const allday = $('f-allday');
      allday.addEventListener('change', function() { S.sheet.draft = readForm(); S.sheet.draft.all_day = allday.checked; if (allday.checked) { S.sheet.draft.end_date = iso(addDays(fromIso(S.sheet.draft.start_date), 1)); } renderSheet(); });
      const cal = $('f-cal');
      if (cal) cal.addEventListener('change', function() { S.sheet.draft = readForm(); renderSheet(); });
      const st = $('f-st'), et = $('f-et');
      if (st && et) {
        // Moving the start keeps the length.
        let len = S.sheet.draft.end - S.sheet.draft.start;
        st.addEventListener('change', function() {
          const d = readForm();
          const end = new Date((d.start + len) * 1000);
          et.value = hhmm(end); $('f-ed').value = iso(end);
        });
        et.addEventListener('change', function() { const d = readForm(); if (d.end > d.start) len = d.end - d.start; });
      }
    }
    sheet.classList.add('open');
  }

  function closeSheet() {
    S.sheet = null;
    renderSheet();
    $('view').focus();
  }

  // ── Quick add ──────────────────────────────────────────────────────────────
  const quick = $('quick-input'), preview = $('quick-preview');
  let quickTimer = 0;
  function quickSummary(d) {
    const c = cal(d.calendar);
    const bits = [];
    if (d.all_day) {
      const a = fromIso(d.start_date), b = addDays(fromIso(d.end_date), -1);
      bits.push(sameDay(a, b) ? longDate(a) + ', all day' : longDate(a) + ' – ' + longDate(b));
    } else {
      bits.push(longDate(new Date(d.start * 1000)) + ', ' + fmtTime(d.start) + '–' + fmtTime(d.end));
    }
    if (d.recur) bits.push({ daily: 'every day', weekdays: 'every weekday', weekly: 'every week', monthly: 'every month', yearly: 'every year' }[d.recur]);
    if (d.attendees.length) bits.push('invites ' + d.attendees.join(', '));
    if (d.location) bits.push('at ' + d.location);
    if (d.video) bits.push('with video');
    if (c) bits.push(c.name);
    return '<b>' + esc(d.title || 'Untitled') + '</b> · ' + esc(bits.join(' · ')) + ' <span class="hint">↩ add · ⌘↩ more</span>';
  }
  quick.addEventListener('input', function() {
    clearTimeout(quickTimer);
    const text = quick.value.trim();
    if (!text) { S.quick = null; preview.classList.remove('show'); return; }
    quickTimer = setTimeout(function() {
      const c = defaultCalendar();
      ipc({ type: 'quick', id: ++S.quickId, text: text, calendar: c ? c.key : '' });
    }, 90);
  });
  quick.addEventListener('keydown', function(e) {
    if (e.key === 'Escape') { e.preventDefault(); e.stopPropagation(); quick.value = ''; S.quick = null; preview.classList.remove('show'); $('view').focus(); return; }
    if (e.key !== 'Enter' || e.isComposing) return;
    e.preventDefault();
    e.stopPropagation();
    if (!S.quick || !quick.value.trim()) { if (!quick.value.trim()) openForm(null); return; }
    if (e.metaKey || !S.quick.calendar) { openForm(S.quick); quick.value = ''; S.quick = null; preview.classList.remove('show'); return; }
    if (!S.quick.title.trim()) { preview.innerHTML = 'Add a title.'; return; }
    const op = ++S.op;
    S.ops[op] = 'quick';
    ipc({ type: 'default_calendar', key: S.quick.calendar });
    ipc({ type: 'save', op: op, draft: S.quick });
    preview.innerHTML = 'Adding <b>' + esc(S.quick.title) + '</b>…';
    quick.value = '';
    S.quick = null;
  });

  // ── Calendars popover ──────────────────────────────────────────────────────
  function renderPop() {
    const pop = $('pop');
    let html = '';
    S.accounts.forEach(function(a) {
      const st = S.status[a.key] || { state: a.state, message: a.message };
      const label = { online: 'synced', connect: 'not connected', auth: 'sign in again', error: 'sync failed', off: st.message || 'off', connecting: 'connecting…' }[st.state] || st.state || '';
      html += '<div class="acct"><span>' + esc(a.address) + '</span><span class="state" title="' + esc(st.message || '') + '">' + esc(label) + '</span></div>';
      if (st.state === 'connect' || st.state === 'auth') html += '<div style="padding:2px 6px 4px"><button class="btn primary" data-connect="' + esc(a.key) + '">Connect calendar</button></div>';
      S.calendars.filter(function(c) { return c.account === a.key; }).forEach(function(c) {
        html += '<label><input type="checkbox" data-cal="' + esc(c.key) + '"' + (S.hidden.indexOf(c.key) < 0 ? ' checked' : '') + '><span class="sw" style="background:' + esc(c.color) + '"></span>' + esc(c.name) + (c.writable ? '' : ' <span style="color:var(--label-3);font-size:11px">read-only</span>') + '</label>';
      });
    });
    if (!S.accounts.length) html += '<div class="acct">No accounts in this workspace</div>';
    html += '<div class="foot"><button class="btn" data-act="settings">Accounts & alerts…</button></div>';
    pop.innerHTML = html;
  }
  function togglePop(open) {
    const pop = $('pop');
    const on = open == null ? !pop.classList.contains('open') : open;
    if (on) renderPop();
    pop.classList.toggle('open', on);
    $('cals-btn').classList.toggle('active', on);
  }

  // ── Events ─────────────────────────────────────────────────────────────────
  function setDay(d) {
    const before = wantRange();
    S.day = midnight(d);
    const after = wantRange();
    if (before.from !== after.from || before.to !== after.to) { S.loaded = S.events.length > 0 && S.loaded; load(); }
    render();
  }
  function step(n) {
    const by = S.view === 'week' ? 7 : 1;
    setDay(addDays(S.day, n * by));
  }
  function setView(v) {
    if (S.view === v) return;
    S.view = v;
    S.pickedView = true;
    load();
    render();
  }
  function ordered() {
    // Occurrences in the order they show, for j/k.
    const out = [];
    const days = S.view === 'agenda' ? 14 : S.view === 'week' ? 7 : 1;
    const first = S.view === 'week' ? weekStart(S.day) : S.day;
    const seen = {};
    for (let i = 0; i < days; i++) {
      onDay(addDays(first, i)).forEach(function(e) { if (!seen[e.key]) { seen[e.key] = true; out.push(e); } });
    }
    return out;
  }
  function move(delta) {
    const list = ordered();
    if (!list.length) return;
    let i = list.findIndex(function(e) { return e.key === S.sel; });
    if (i < 0) {
      const now = nowSec();
      i = list.findIndex(function(e) { return e.end > now; });
      if (i < 0) i = 0;
    } else i = Math.max(0, Math.min(list.length - 1, i + delta));
    S.sel = list[i].key;
    S.keepScroll = true;
    render();
    const el = document.querySelector('[data-open="' + CSS.escape(S.sel) + '"].row, [data-open="' + CSS.escape(S.sel) + '"].g-ev, [data-open="' + CSS.escape(S.sel) + '"].chip');
    if (el) el.scrollIntoView({ block: 'nearest' });
  }

  document.addEventListener('click', function(e) {
    const t = e.target;
    const url = t.closest('[data-url]');
    if (url) { e.preventDefault(); ipc({ type: 'open_url', url: url.dataset.url }); return; }
    const join = t.closest('[data-join]');
    if (join) { e.preventDefault(); e.stopPropagation(); ipc({ type: 'join', url: join.dataset.join }); return; }
    const connect = t.closest('[data-connect]');
    if (connect) { ipc({ type: 'connect', account: connect.dataset.connect }); togglePop(false); return; }
    if (!t.closest('#pop') && !t.closest('#cals-btn') && $('pop').classList.contains('open')) togglePop(false);
    const act = t.closest('[data-act]');
    if (act) {
      const a = act.dataset.act, ev = S.sheet && S.sheet.event;
      if (a === 'back' || a === 'cancel') closeSheet();
      else if (a === 'save') saveForm();
      else if (a === 'edit' && ev) openForm(Object.assign({}, ev, { attendees: ev.attendees.filter(function(g) { return !g.me && !g.organizer; }).map(function(g) { return g.email; }), video: false }), ev);
      else if (a === 'delete') { S.sheet.confirm = true; renderSheet(); }
      else if (a === 'nodelete') { S.sheet.confirm = false; renderSheet(); }
      else if (a === 'notes' && ev) ipc({ type: 'notes', key: ev.key });
      else if (a === 'settings') { togglePop(false); ipc({ type: 'settings' }); }
      return;
    }
    const del = t.closest('[data-del]');
    if (del && S.sheet && S.sheet.event) {
      const op = ++S.op;
      S.ops[op] = 'delete';
      ipc({ type: 'delete', op: op, key: S.sheet.event.key, all: del.dataset.del === 'all' });
      closeSheet();
      return;
    }
    const rsvp = t.closest('[data-rsvp]');
    if (rsvp && S.sheet && S.sheet.event) {
      const op = ++S.op;
      S.ops[op] = 'rsvp';
      S.sheet.pending = true;
      S.sheet.event.me = rsvp.dataset.rsvp;
      ipc({ type: 'rsvp', op: op, key: S.sheet.event.key, response: rsvp.dataset.rsvp });
      renderSheet();
      return;
    }
    const open = t.closest('[data-open]');
    if (open) { const ev = find(open.dataset.open) || (S.next && S.next.key === open.dataset.open ? S.next : null); openDetail(ev); return; }
    const day = t.closest('[data-day]');
    if (day) { setDay(fromIso(day.dataset.day)); if (S.view === 'week' && day.classList.contains('c')) setView('day'); return; }
    const view = t.closest('[data-view]');
    if (view) { setView(view.dataset.view); return; }
    const col = t.closest('.g-col');
    if (col && !t.closest('.g-ev')) {
      const rect = col.getBoundingClientRect();
      const H = S.view === 'day' ? 46 : 42;
      const minutes = Math.floor(((e.clientY - rect.top) / H * 60) / 30) * 30;
      const d = fromIso(col.dataset.col);
      d.setMinutes(minutes);
      openForm({ start: d.getTime() / 1000, end: d.getTime() / 1000 + 3600 });
    }
  });
  document.addEventListener('change', function(e) {
    const box = e.target.closest('[data-cal]');
    if (!box) return;
    const key = box.dataset.cal;
    const at = S.hidden.indexOf(key);
    if (box.checked && at >= 0) S.hidden.splice(at, 1);
    if (!box.checked && at < 0) S.hidden.push(key);
    ipc({ type: 'hidden', calendars: S.hidden });
    render();
  });

  $('tab-notes').addEventListener('click', function() { ipc({ type: 'tab', tab: 'notes' }); });
  $('tab-mail').addEventListener('click', function() { ipc({ type: 'tab', tab: 'mail' }); });
  $('close-btn').addEventListener('click', function() { ipc({ type: 'hide' }); });
  $('fullscreen-btn').addEventListener('click', function() { ipc({ type: 'fullscreen' }); });
  $('prev-btn').addEventListener('click', function() { step(-1); });
  $('next-btn').addEventListener('click', function() { step(1); });
  $('today-btn').addEventListener('click', function() { setDay(new Date()); });
  $('cals-btn').addEventListener('click', function() { togglePop(); });
  $('share-btn').addEventListener('click', function() { ipc({ type: 'share', days: 5 }); });
  $('sync-btn').addEventListener('click', function() { sync(); });
  function sync() { ipc({ type: 'refresh' }); S.syncing = true; $('sync-btn').classList.add('spin'); setTimeout(function() { $('sync-btn').classList.remove('spin'); }, 1200); }

  function typing(e) { const t = e.target; return t && (t.tagName === 'INPUT' || t.tagName === 'TEXTAREA' || t.tagName === 'SELECT' || t.isContentEditable); }
  document.addEventListener('keydown', function(e) {
    if (e.isComposing) return;
    if (S.sheet && S.sheet.kind === 'form') {
      if (e.key === 'Escape') { e.preventDefault(); closeSheet(); return; }
      if (e.key === 'Enter' && (e.metaKey || (!e.shiftKey && e.target && e.target.tagName === 'INPUT' && e.target.type !== 'checkbox'))) { e.preventDefault(); saveForm(); return; }
      return;
    }
    if (e.key === 'Tab' && !e.metaKey && !e.ctrlKey && !e.altKey) {
      if (typing(e)) return;
      e.preventDefault();
      ipc({ type: 'tab', tab: e.shiftKey ? 'mail' : 'notes' });
      return;
    }
    if (typing(e)) return;
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    const k = e.key;
    if (k === 'Escape') {
      e.preventDefault();
      if ($('pop').classList.contains('open')) togglePop(false);
      else if (S.sheet) closeSheet();
      else ipc({ type: 'hide' });
      return;
    }
    if (S.sheet && S.sheet.kind === 'detail') {
      const ev = S.sheet.event;
      if (k === 'Enter' && ev && ev.join) { e.preventDefault(); ipc({ type: 'join', url: ev.join }); }
      else if (k === 'e' && ev && ev.editable) { e.preventDefault(); document.querySelector('[data-act="edit"]').click(); }
      else if (k === 'Backspace' || k === 'Delete') { if (ev && ev.editable) { e.preventDefault(); S.sheet.confirm = true; renderSheet(); } }
      else if (k === 'j' || k === 'ArrowDown' || k === 'k' || k === 'ArrowUp') { e.preventDefault(); move(k === 'j' || k === 'ArrowDown' ? 1 : -1); openDetail(find(S.sel)); }
      return;
    }
    if (k === 'j' || k === 'ArrowDown') { e.preventDefault(); move(1); }
    else if (k === 'k' || k === 'ArrowUp') { e.preventDefault(); move(-1); }
    else if (k === 'ArrowLeft' || k === 'h') { e.preventDefault(); step(-1); }
    else if (k === 'ArrowRight' || k === 'l') { e.preventDefault(); step(1); }
    else if (k === 't') { e.preventDefault(); setDay(new Date()); }
    else if (k === 'Enter' || k === 'o') { const ev = find(S.sel); if (ev) { e.preventDefault(); openDetail(ev); } }
    else if (k === 'n' || k === 'c') { e.preventDefault(); quick.focus(); }
    else if (k === 'N' || k === 'C') { e.preventDefault(); openForm(null); }
    else if (k === 'v') { e.preventDefault(); setView({ agenda: 'day', day: 'week', week: 'agenda' }[S.view]); }
    else if (k === '1') setView('agenda'); else if (k === '2') setView('day'); else if (k === '3') setView('week');
    else if (k === 's') { e.preventDefault(); ipc({ type: 'share', days: 5 }); }
    else if (k === 'r') { e.preventDefault(); sync(); }
  });

  // Resize, as the other pane tabs do.
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
    dragPointer = e.pointerId; dragStartX = e.screenX; dragStartWidth = Math.round(window.innerWidth); dragWidth = dragStartWidth;
    handle.setPointerCapture(e.pointerId); handle.classList.add('active'); document.body.classList.add('resizing');
  });
  handle.addEventListener('pointermove', function(e) { if (e.pointerId !== dragPointer) return; e.preventDefault(); queueResize(dragStartWidth + dragStartX - e.screenX); });
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

  // The minute ticks: countdowns, the now line, and past events fading.
  setInterval(function() {
    renderNext();
    if (!S.sheet || S.sheet.kind !== 'form') { S.keepScroll = true; if (S.view === 'agenda') renderAgenda(); else renderGrid(S.view === 'week' ? 7 : 1); }
  }, 30000);

  // ── From Rust ──────────────────────────────────────────────────────────────
  window.__cal = {
    setup: function(d) {
      const wsChanged = d.ws !== S.ws;
      S.ws = d.ws;
      S.accounts = d.accounts || [];
      S.calendars = d.calendars || [];
      S.hidden = (d.hidden || []).slice();
      S.prefs = d.prefs || {};
      (d.accounts || []).forEach(function(a) { S.status[a.key] = { state: a.state, message: a.message }; });
      if (wsChanged) { S.sel = null; S.sheet = null; renderSheet(); S.loaded = false; S.events = []; }
      if (!S.pickedView) S.view = S.fullscreen ? 'week' : 'agenda';
      load();
      render();
      if ($('pop').classList.contains('open')) renderPop();
    },
    events: function(d) {
      if (!S.range || d.from !== S.range.from || d.to !== S.range.to) return;
      S.events = d.events || [];
      S.loaded = true;
      S.keepScroll = true;
      render();
      if (S.sheet && S.sheet.kind === 'detail') {
        const e = find(S.sheet.key);
        if (e) { S.sheet.event = e; S.sheet.pending = false; renderSheet(); }
      }
    },
    changed: function() { load(); },
    next: function(e) { S.next = e; renderNext(); },
    parsed: function(id, d) {
      if (id !== S.quickId || !quick.value.trim()) return;
      S.quick = d;
      preview.innerHTML = quickSummary(d);
      preview.classList.add('show');
    },
    done: function(op, ok, text) {
      const kind = S.ops[op];
      delete S.ops[op];
      if (kind === 'form' && S.sheet && S.sheet.kind === 'form') {
        if (ok) closeSheet(); else { S.sheet.saving = false; S.sheet.error = text; renderSheet(); }
      }
      if (kind === 'quick') { preview.innerHTML = ok ? esc(text) : '<span style="color:var(--err)">' + esc(text) + '</span>'; preview.classList.add('show'); setTimeout(function() { if (!quick.value) preview.classList.remove('show'); }, ok ? 2500 : 6000); }
      if (kind === 'rsvp' && S.sheet && S.sheet.kind === 'detail') { S.sheet.pending = false; if (!ok) { S.sheet.event.me = (find(S.sheet.key) || {}).me || S.sheet.event.me; } renderSheet(); }
    },
    open: function(e) { if (!e) return; S.day = midnight(new Date(e.start * 1000)); load(); render(); openDetail(e); },
    draft: function(d) { openForm(d); },
    status: function(acct, state, message) {
      S.status[acct] = { state: state, message: message };
      renderBanner();
      if ($('pop').classList.contains('open')) renderPop();
    },
    unread: function(n) { $('unread').textContent = n ? String(n) : ''; },
    fullscreen: function(on) {
      S.fullscreen = on;
      $('panel').classList.toggle('fullscreen', on);
      const b = $('fullscreen-btn');
      b.classList.toggle('active', on);
      b.querySelector('.ic-enter').style.display = on ? 'none' : '';
      b.querySelector('.ic-exit').style.display = on ? '' : 'none';
      if (!S.pickedView) { S.view = on ? 'week' : 'agenda'; load(); }
      render();
    },
    focus: function() {
      if (S.sheet && S.sheet.kind === 'form') { const t = $('f-title'); if (t) t.focus(); return; }
      $('view').focus();
    }
  };

  window.__setShortcuts = function(data) {
    const actions = data && Array.isArray(data.actions) ? data.actions : [];
    const chordFor = function(id) { const a = actions.find(function(x) { return x.id === id; }); return a && Array.isArray(a.keys) ? a.keys.join('') : ''; };
    S.chords = { join: chordFor('join_meeting'), calendar: chordFor('calendar'), notes: chordFor('notes'), inbox: chordFor('inbox'), fullscreen: chordFor('sidebar_fullscreen') };
    $('tab-notes').title = 'Notes' + (S.chords.notes ? ' (' + S.chords.notes + ')' : '');
    $('tab-mail').title = 'Mail' + (S.chords.inbox ? ' (' + S.chords.inbox + ')' : '');
    $('tab-cal').title = 'Calendar' + (S.chords.calendar ? ' (' + S.chords.calendar + ')' : '');
    $('fullscreen-btn').title = 'Toggle fullscreen' + (S.chords.fullscreen ? ' (' + S.chords.fullscreen + ')' : '');
    renderNext();
  };
  window.__setShortcuts(/*@@KEYBINDINGS_JSON@@*/);
  render();
  ipc({ type: 'ready' });
})();
</script>
</body>
</html>"##
		.replace("/*@@THEME@@*/", crate::theme::CSS)
		.replace("/*@@KEYBINDINGS_JSON@@*/", keybindings_json)
}

#[cfg(test)]
mod tests {
	/// `CAL_PREVIEW=/path/page.html cargo test --bin octoweb calendar_preview -- --ignored`
	/// writes the page with `window.ipc` recording messages, to look at in a
	/// browser with made-up data.
	#[test]
	#[ignore]
	fn calendar_preview() {
		let path = std::env::var("CAL_PREVIEW").expect("CAL_PREVIEW names the file");
		let stub = "<script>window.__sent=[];window.ipc={postMessage:function(m){window.__sent.push(JSON.parse(m));}};</script><script>";
		std::fs::write(path, super::html("null").replacen("<script>", stub, 1)).unwrap();
	}
}
