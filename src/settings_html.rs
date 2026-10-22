/// Returns the HTML for the settings modal (⌘,).
/// Frosted-glass panel with form controls for all Config fields.
/// IPC messages: settings_close, settings_update, check_updates.
pub fn html() -> String {
	r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<style>
/*@@THEME@@*/
  *, *::before, *::after { box-sizing: border-box; margin: 0; padding: 0; }

  html, body {
    width: 100%; height: 100%;
    overflow: hidden;
    background: transparent;
    -webkit-font-smoothing: antialiased;
    font-family: var(--font-text);
    color: var(--label);
  }

  #backdrop {
    position: fixed; inset: 0;
    background: rgba(0, 0, 0, 0.10);
    display: flex;
    align-items: center;
    justify-content: center;
    animation: fadeIn var(--t-fast) var(--ease);
  }

  @keyframes fadeIn { from { opacity: 0; } to { opacity: 1; } }
  @keyframes scaleIn { from { opacity: 0; transform: scale(0.96); } to { opacity: 1; transform: none; } }

  #panel {
    background: var(--glass-thick);
    -webkit-backdrop-filter: var(--glass-blur);
    backdrop-filter: var(--glass-blur);
    border-radius: var(--r-panel);
    box-shadow: var(--shadow-float), var(--glass-shine);
    padding: 16px 20px;
    width: min(680px, calc(100vw - 32px));
    max-height: 80vh;
    overflow-y: auto;
    animation: scaleIn var(--t-pop) var(--spring);
  }

  #header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 16px;
  }

  #title {
    font-family: var(--font-display);
    font-size: 17px;
    font-weight: 600;
    color: var(--label);
    letter-spacing: -0.01em;
  }

  #close-btn {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 22px; height: 22px;
    border-radius: var(--r-capsule);
    border: none;
    background: transparent;
    cursor: pointer;
    color: var(--label-2);
    transition: background var(--t-fast) var(--ease), color var(--t-fast) var(--ease),
                transform var(--t-fast) var(--ease);
  }
  #close-btn:hover { background: var(--fill-hover); color: var(--label); }
  #close-btn:active { background: var(--fill-press); transform: scale(0.9); }

  #dismiss-hint {
    display: flex;
    align-items: center;
    gap: 5px;
    margin-left: auto;
    margin-right: 8px;
    color: var(--label-2);
    font-size: 11px;
  }

  .section {
    background: var(--fill);
    box-shadow: 0 0 0 0.5px var(--hairline);
    border-radius: var(--r-card);
    overflow: hidden;
    margin-bottom: 10px;
  }

  .section-title {
    font-family: var(--font-display);
    font-size: 13px;
    font-weight: 600;
    color: var(--label-2);
    padding: 10px 12px 6px;
  }

  code {
    font-family: var(--font-mono);
    font-size: 11px;
    background: var(--fill-hover);
    border: 0.5px solid var(--hairline);
    border-radius: var(--r-ctl);
    padding: 0 4px;
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    padding: 7px 12px;
    gap: 12px;
    min-height: 32px;
    flex-wrap: wrap;
  }

  .row + .row {
    box-shadow: 0 0.5px 0 var(--hairline) inset;
  }

  .row-label {
    font-size: 13px;
    color: var(--label);
    white-space: nowrap;
    flex-shrink: 0;
  }

  /* Stacked label + dim hint, used when a setting needs explanation */
  .row.with-hint { align-items: flex-start; padding-top: 9px; padding-bottom: 9px; }
  .row.with-hint .row-label-stack { display: flex; flex-direction: column; gap: 2px; min-width: 0; flex: 1; white-space: normal; }
  .row.with-hint .row-label { white-space: normal; }
  .row-hint {
    font-size: 11px;
    color: var(--label-2);
    line-height: 1.35;
    max-width: 240px;
  }

  .row-error {
    flex: 0 0 100%;
    padding-left: 0;
    color: var(--err);
    font-size: 11px;
    line-height: 1.35;
  }

  .row input[type="text"],
  .row input[type="password"],
  .row input[type="number"] {
    flex: 1;
    min-width: 0;
    height: 24px;
    padding: 0 8px;
    font-size: 13px;
    font-family: inherit;
    color: var(--label);
    background: var(--fill);
    border: none;
    border-radius: var(--r-ctl);
    box-shadow: 0 0 0 0.5px var(--hairline);
    outline: none;
    text-align: right;
    transition: background var(--t-fast) var(--ease), box-shadow var(--t-fast) var(--ease);
  }
  .row input[type="text"]:hover,
  .row input[type="password"]:hover,
  .row input[type="number"]:hover { background: var(--fill-hover); }
  .row input[type="text"]:focus,
  .row input[type="password"]:focus,
  .row input[type="number"]:focus {
    background: var(--fill-hover);
    box-shadow: 0 0 0 1px var(--accent), 0 0 0 3px color-mix(in srgb, var(--accent) 22%, transparent);
  }
  .row input[type="number"] { width: 70px; flex: none; }

  .row select {
    flex: none;
    max-width: 170px;
    min-height: 24px;
    padding: 2px 24px 2px 8px;
    font-size: 13px;
    font-family: inherit;
    color: var(--label);
    background-color: var(--fill);
    border: none;
    border-radius: var(--r-ctl);
    box-shadow: 0 0 0 0.5px var(--hairline);
    outline: none;
    accent-color: var(--accent);
    transition: background var(--t-fast) var(--ease), box-shadow var(--t-fast) var(--ease);
  }
  .row select:hover { background-color: var(--fill-hover); }
  .row select:active { background-color: var(--fill-press); }
  .row select:focus { box-shadow: 0 0 0 1px var(--accent), 0 0 0 3px color-mix(in srgb, var(--accent) 22%, transparent); }

  /* Toggle switch */
  .toggle {
    position: relative;
    width: 36px; height: 22px;
    background: var(--fill-press);
    border-radius: var(--r-capsule);
    cursor: pointer;
    transition: background var(--t-fast) var(--ease), transform var(--t-fast) var(--spring);
    flex-shrink: 0;
    border: none;
    padding: 0;
  }
  .toggle:hover { background: var(--fill-hover); }
  .toggle:active { background: var(--fill-press); transform: scale(0.94); }
  .toggle.on { background: var(--accent); }
  .toggle.on:hover { background: color-mix(in srgb, var(--accent) 88%, var(--label)); }
  .toggle::after {
    content: '';
    position: absolute;
    top: 2px; left: 2px;
    width: 18px; height: 18px;
    background: var(--on-accent);
    border-radius: 50%;
    box-shadow: 0 1px 3px color-mix(in srgb, var(--label) 18%, transparent);
    transition: transform var(--t-fast) var(--spring);
  }
  .toggle.on::after { transform: translateX(14px); }

  /* Tab bar */
  .tabs {
    display: flex;
    gap: 3px;
    margin-bottom: 14px;
    background: var(--fill);
    box-shadow: inset 0 0 0 0.5px var(--hairline);
    border-radius: var(--r-capsule);
    padding: 3px;
  }
  .tab {
    flex: 1;
    text-align: center;
    font-size: 13px;
    font-weight: 500;
    color: var(--label-2);
    padding: 5px 8px;
    border: none;
    min-height: 26px;
    border-radius: var(--r-capsule);
    background: transparent;
    cursor: pointer;
    transition: background var(--t-fast) var(--ease), color var(--t-fast) var(--ease),
                transform var(--t-fast) var(--ease);
  }
  .tab:hover { background: var(--fill-hover); color: var(--label); }
  .tab:active { background: var(--fill-press); transform: scale(0.98); }
  .tab.active {
    background: var(--glass-thin);
    color: var(--label);
    box-shadow: 0 0 0 0.5px var(--hairline), var(--glass-shine);
  }
  .tab-pane { display: none; }
  .tab-pane.active { display: block; }

  /* Keybindings */
  .keys { display: inline-flex; align-items: center; gap: 2px; }
  kbd.kbd {
    min-width: 17px;
    min-height: 17px;
    white-space: nowrap;
    user-select: none;
    font-size: 11px;
  }
  .kb-right { display: flex; align-items: center; gap: 6px; flex-shrink: 0; }
  .kb-bind {
    display: inline-flex;
    align-items: center;
    min-height: 22px;
    padding: 2px 6px;
    background: var(--fill);
    border: none;
    box-shadow: 0 0 0 0.5px var(--hairline);
    border-radius: var(--r-ctl);
    cursor: pointer;
    transition: background var(--t-fast) var(--ease), box-shadow var(--t-fast) var(--ease),
                transform var(--t-fast) var(--ease);
  }
  .kb-bind:hover { background: var(--fill-hover); }
  .kb-bind:active { background: var(--fill-press); transform: scale(0.98); }
  .kb-bind.recording {
    box-shadow: 0 0 0 1px var(--accent), 0 0 0 3px color-mix(in srgb, var(--accent) 22%, transparent);
  }
  .rec-hint { font-size: 11px; color: var(--accent); }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    padding: 0;
    margin: -1px;
    overflow: hidden;
    clip: rect(0, 0, 0, 0);
    white-space: nowrap;
    border: 0;
  }
  .kb-reset {
    display: flex;
    align-items: center;
    justify-content: center;
    width: 22px; height: 22px;
    border: none;
    background: transparent;
    color: var(--label-2);
    cursor: pointer;
    border-radius: var(--r-capsule);
    font-size: 13px;
    line-height: 1;
  }
  .kb-reset:hover { background: var(--fill-hover); color: var(--label); }
  .kb-reset:active { background: var(--fill-press); transform: scale(0.9); }
  .kb-error {
    display: none;
    font-size: 11px;
    color: var(--err);
    padding: 8px 2px 0;
  }
  .kb-error.show { display: block; }
  .kb-footer { display: flex; justify-content: space-between; align-items: center; padding-top: 10px; }
  .kb-note { font-size: 11px; color: var(--label-2); }
  .kb-reset-all {
    min-height: 24px;
    font-size: 13px;
    font-weight: 600;
    color: var(--on-accent);
    background: var(--accent);
    border: none;
    border-radius: var(--r-capsule);
    padding: 4px 10px;
    cursor: pointer;
    transition: filter var(--t-fast) var(--ease), transform var(--t-fast) var(--ease);
  }
  .kb-reset-all:hover { filter: brightness(1.08); }
  .kb-reset-all:active { filter: brightness(0.92); transform: scale(0.96); }

  /* Proxies */
  .proxy-head { display: flex; align-items: center; gap: 10px; padding: 8px 12px; }
  .proxy-open {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 0;
    border: none;
    background: transparent;
    font: inherit;
    color: inherit;
    text-align: left;
    cursor: pointer;
  }
  .proxy-text { flex: 1; min-width: 0; display: flex; flex-direction: column; gap: 1px; }
  .proxy-name,
  .proxy-sub { white-space: nowrap; overflow: hidden; text-overflow: ellipsis; }
  .proxy-name { font-size: 13px; color: var(--label); }
  .proxy-sub { font-size: 11px; color: var(--label-2); }
  .proxy-chev { font-size: 15px; color: var(--label-2); transition: transform var(--t-fast) var(--ease); }
  .proxy.open .proxy-chev { transform: rotate(90deg); }
  .proxy-dot { width: 8px; height: 8px; border-radius: 50%; flex-shrink: 0; background: var(--fill-press); }
  .proxy-dot.on { background: var(--accent); }
  .proxy-dot.connecting { background: var(--warn); }
  .proxy-dot.connected { background: var(--ok); }
  .proxy-dot.failed { background: var(--err); }
  .proxy-body { box-shadow: 0 0.5px 0 var(--hairline) inset; }
  .proxy-inline { flex: 1; min-width: 0; display: flex; align-items: center; gap: 6px; }
  .seg {
    display: inline-flex;
    gap: 2px;
    padding: 2px;
    background: var(--fill);
    border-radius: var(--r-capsule);
    box-shadow: inset 0 0 0 0.5px var(--hairline);
  }
  .seg button {
    padding: 3px 10px;
    border: none;
    border-radius: var(--r-capsule);
    background: transparent;
    font: inherit;
    font-size: 12px;
    color: var(--label-2);
    cursor: pointer;
    transition: background var(--t-fast) var(--ease), color var(--t-fast) var(--ease);
  }
  .seg button:hover { color: var(--label); }
  .seg button[aria-pressed="true"] {
    background: var(--glass-thin);
    color: var(--label);
    box-shadow: 0 0 0 0.5px var(--hairline), var(--glass-shine);
  }
  .proxy-status { min-width: 0; display: flex; align-items: center; gap: 6px; font-size: 11px; color: var(--label-2); }
  .proxy-status-text { overflow-wrap: anywhere; }
  .proxy-link,
  .proxy-remove {
    flex-shrink: 0;
    padding: 3px 8px;
    border: none;
    border-radius: var(--r-ctl);
    background: transparent;
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }
  .proxy-link { color: var(--accent); }
  .proxy-link:hover { background: var(--fill-hover); }
  .proxy-remove { color: var(--err); }
  .proxy-remove:hover { background: color-mix(in srgb, var(--err) 12%, transparent); }
  .row textarea {
    flex: 1 1 100%;
    min-height: 56px;
    padding: 6px 8px;
    font-size: 12px;
    font-family: var(--font-mono);
    color: var(--label);
    background: var(--fill);
    border: none;
    border-radius: var(--r-ctl);
    box-shadow: 0 0 0 0.5px var(--hairline);
    outline: none;
    resize: vertical;
  }
  .row textarea:focus {
    background: var(--fill-hover);
    box-shadow: 0 0 0 1px var(--accent), 0 0 0 3px color-mix(in srgb, var(--accent) 22%, transparent);
  }
  .proxy-empty { padding: 20px 12px; text-align: center; }
  .proxy-empty .kb-note { display: block; margin-top: 4px; }
  #proxy-add { flex-shrink: 0; margin-left: 12px; }
  .ext-icon { width: 24px; height: 24px; flex-shrink: 0; border-radius: 5px; object-fit: contain; }
  .ext-icon.blank { background: var(--fill-press); }
  .ext-version { margin-left: 5px; font-size: 11px; color: var(--label-2); }
  .ext-error { color: var(--err); white-space: normal; }
  #ext-pick { flex-shrink: 0; margin-left: 12px; }
  .ext-scope .seg { flex-wrap: wrap; }
  .mail-link { color: var(--accent); cursor: pointer; text-decoration: underline; }
  .ext-scope .ws-dot { display: inline-block; width: 7px; height: 7px; border-radius: 50%; margin-right: 5px; vertical-align: 1px; }
</style>
</head>
<body>
<div id="backdrop">
  <div id="panel" class="glass-panel" role="dialog" aria-modal="true" aria-labelledby="title">
    <div id="header">
      <span id="title">Settings</span>
      <span id="dismiss-hint"><span class="kbd">esc</span> close</span>
      <button id="close-btn" title="Close" aria-label="Close settings">
        <svg width="10" height="10" viewBox="0 0 10 10" fill="none">
          <line x1="2" y1="2" x2="8" y2="8" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>
          <line x1="8" y1="2" x2="2" y2="8" stroke="currentColor" stroke-width="1.3" stroke-linecap="round"/>
        </svg>
      </button>
    </div>

    <div class="tabs">
      <button class="tab active" data-pane="tab-general">General</button>
      <button class="tab" data-pane="tab-keybindings">Keyboard shortcuts</button>
      <button class="tab" data-pane="tab-proxies">Proxies</button>
      <button class="tab" data-pane="tab-extensions">Extensions</button>
      <button class="tab" data-pane="tab-mail">Mail</button>
    </div>

    <div id="tab-general" class="tab-pane active">
    <div class="section">
      <div class="section-title">General</div>
      <div class="row with-hint">
        <div class="row-label-stack">
          <span class="row-label" id="appearance-label">Appearance</span>
          <span class="row-hint">Auto follows macOS. Websites follow your choice too.</span>
        </div>
        <span class="seg" id="appearance-seg" role="group" aria-labelledby="appearance-label">
          <button type="button" data-appearance="auto" aria-pressed="true">Auto</button>
          <button type="button" data-appearance="light" aria-pressed="false">Light</button>
          <button type="button" data-appearance="dark" aria-pressed="false">Dark</button>
        </span>
      </div>
      <div class="row with-hint">
        <div class="row-label-stack">
          <label class="row-label" id="sounds_label" for="sounds">Sounds</label>
          <span class="row-hint">Only for what matters: mail sent, marked done, an AI draft ready, new Important mail. Off is silent; macOS's interface sound setting is respected too.</span>
        </div>
        <button class="toggle" id="sounds" data-key="sounds" role="switch" aria-checked="true" aria-labelledby="sounds_label"></button>
      </div>
      <div class="row with-hint">
        <div class="row-label-stack">
          <label class="row-label" for="home_page">Home page</label>
          <span class="row-hint">Loads on launch when there's no previous session to restore.</span>
        </div>
        <input type="text" id="home_page" data-key="home_page">
      </div>
      <div class="row with-hint">
        <div class="row-label-stack">
          <label class="row-label" for="search_engine_select">Search engine</label>
          <span class="row-hint">Used when you type a search instead of a URL.</span>
        </div>
        <select id="search_engine_select">
          <option value="https://www.google.com/search?q={}">Google</option>
          <option value="https://duckduckgo.com/?q={}">DuckDuckGo</option>
          <option value="https://www.bing.com/search?q={}">Bing</option>
          <option value="https://search.brave.com/search?q={}">Brave</option>
          <option value="https://www.ecosia.org/search?q={}">Ecosia</option>
          <option value="custom">Custom…</option>
        </select>
      </div>
      <div class="row with-hint" id="search_engine_custom_row" style="display:none">
        <div class="row-label-stack">
          <label class="row-label" for="search_engine">Custom search URL</label>
          <span class="row-hint"><code>{}</code> is replaced with your search terms.</span>
        </div>
        <input type="text" id="search_engine" data-key="search_engine" placeholder="https://example.com/search?q={}">
      </div>
      <div class="row with-hint">
        <div class="row-label-stack">
          <label class="row-label" for="max_history">Max history</label>
          <span class="row-hint">Keep 0 or more total entries across sessions.</span>
        </div>
        <input type="number" id="max_history" data-key="max_history" min="0" step="1">
      </div>
      <div class="row with-hint">
        <div class="row-label-stack">
          <label class="row-label" id="link_hints_sticky_label" for="link_hints_sticky">Keep link hints on</label>
          <span class="row-hint"><span id="follow-link-key">⌘⇧F</span> hints stay through clicks, scrolling and page loads until you press it again.</span>
        </div>
        <button class="toggle" id="link_hints_sticky" data-key="link_hints_sticky" role="switch" aria-checked="false" aria-labelledby="link_hints_sticky_label"></button>
      </div>
    </div>

    <div class="section">
      <div class="section-title">Terminal</div>
      <div class="row with-hint">
        <div class="row-label-stack">
          <span class="row-label" id="terminal-appearance-label">Appearance</span>
          <span class="row-hint">General follows Appearance. Light or Dark keeps the terminal in that mode.</span>
        </div>
        <span class="seg" id="terminal-appearance-seg" role="group" aria-labelledby="terminal-appearance-label">
          <button type="button" data-appearance="general" aria-pressed="true">General</button>
          <button type="button" data-appearance="light" aria-pressed="false">Light</button>
          <button type="button" data-appearance="dark" aria-pressed="false">Dark</button>
        </span>
      </div>
      <div class="row with-hint">
        <div class="row-label-stack">
          <label class="row-label" id="terminal_option_as_meta_label" for="terminal_option_as_meta">Option as Meta</label>
          <span class="row-hint">⌥F, ⌥B, ⌥D and ⌥⌫ reach the shell as word keys. Off, ⌥ types characters such as ƒ.</span>
        </div>
        <button class="toggle" id="terminal_option_as_meta" data-key="terminal_option_as_meta" role="switch" aria-checked="false" aria-labelledby="terminal_option_as_meta_label"></button>
      </div>
    </div>

    <div class="section">
      <div class="section-title">Window</div>
      <div class="row with-hint">
        <div class="row-label-stack">
          <label class="row-label" for="window_width">Default width</label>
          <span class="row-hint">Use an initial window width of at least 400 pixels.</span>
        </div>
        <input type="number" id="window_width" data-key="window_width" min="400" step="10">
      </div>
      <div class="row with-hint">
        <div class="row-label-stack">
          <label class="row-label" for="window_height">Default height</label>
          <span class="row-hint">Use an initial window height of at least 300 pixels.</span>
        </div>
        <input type="number" id="window_height" data-key="window_height" min="300" step="10">
      </div>
    </div>

    <div class="section">
      <div class="section-title">Memory</div>
      <div class="row with-hint">
        <div class="row-label-stack">
          <label class="row-label" id="aggressive_hibernation_label" for="aggressive_hibernation">Save memory aggressively</label>
          <span class="row-hint">Reclaim tab memory sooner; leave this off on modern Macs so tabs survive longer.</span>
        </div>
        <button class="toggle" id="aggressive_hibernation" data-key="aggressive_hibernation" role="switch" aria-checked="false" aria-labelledby="aggressive_hibernation_label"></button>
      </div>
      <div class="row with-hint">
        <div class="row-label-stack">
          <label class="row-label" for="max_tabs">Max open tabs</label>
          <span class="row-hint">Close least-recently-used tabs above this non-negative limit while keeping pages in history; 0 disables the limit.</span>
        </div>
        <input type="number" id="max_tabs" data-key="max_tabs" min="0" step="1">
      </div>
    </div>

    <div class="section">
      <div class="section-title">AI</div>
      <div class="row with-hint">
        <div class="row-label-stack">
          <label class="row-label" id="ai_edit_auto_hide_label" for="ai_edit_auto_hide">Hide the editor after applying</label>
          <span class="row-hint">Hide the <span id="inline-edit-key">⌘⇧E</span> modal after submit; show a loading cursor instead.</span>
        </div>
        <button class="toggle" id="ai_edit_auto_hide" data-key="ai_edit_auto_hide" role="switch" aria-checked="false" aria-labelledby="ai_edit_auto_hide_label"></button>
      </div>
      <div class="row with-hint">
        <div class="row-label-stack">
          <label class="row-label" for="max_prompt_history">Editor prompt history size</label>
          <span class="row-hint">Keep at least 10 prompts for recall via ⌃P / ⌃N inside the editor.</span>
        </div>
        <input type="number" id="max_prompt_history" data-key="max_prompt_history" min="10" step="10">
      </div>
      <div class="row with-hint">
        <div class="row-label-stack">
          <label class="row-label" for="max_ai_prompt_history">Assistant prompt history size</label>
          <span class="row-hint">Keep at least 10 prompts available for recall in the assistant sidebar.</span>
        </div>
        <input type="number" id="max_ai_prompt_history" data-key="max_ai_prompt_history" min="10" step="10">
      </div>
      <div class="row with-hint">
        <div class="row-label-stack">
          <label class="row-label" id="proactive_learning_label" for="proactive_learning">Proactive learning</label>
          <span class="row-hint">Background agent reads recent browsing and memorizes patterns.</span>
        </div>
        <button class="toggle" id="proactive_learning" data-key="proactive_learning" role="switch" aria-checked="false" aria-labelledby="proactive_learning_label"></button>
      </div>
      <div class="row with-hint">
        <div class="row-label-stack">
          <label class="row-label" for="learning_interval_min">Learning interval (min)</label>
          <span class="row-hint">Run the learning agent at intervals of at least 5 minutes.</span>
        </div>
        <input type="number" id="learning_interval_min" data-key="learning_interval_min" min="5" step="5">
      </div>
    </div>

    <div class="section">
      <div class="section-title">Updates</div>
      <div class="row with-hint updates-on">
        <div class="row-label-stack">
          <label class="row-label" id="update_checks_label" for="update_checks">Check for updates daily</label>
          <span class="row-hint">Asks octoweb.xyz once a day whether a new version is out. The request carries the app's version and nothing else.</span>
        </div>
        <button class="toggle" id="update_checks" data-key="update_checks" role="switch" aria-checked="false" aria-labelledby="update_checks_label"></button>
      </div>
      <div class="row with-hint updates-on">
        <div class="row-label-stack">
          <label class="row-label" id="update_downloads_label" for="update_downloads">Download updates automatically</label>
          <span class="row-hint">A downloaded update installs the next time you quit Octoweb.</span>
        </div>
        <button class="toggle" id="update_downloads" data-key="update_downloads" role="switch" aria-checked="false" aria-labelledby="update_downloads_label"></button>
      </div>
      <div class="row with-hint">
        <div class="row-label-stack">
          <span class="row-label" id="update-version">Octoweb</span>
          <span class="row-hint" id="update-hint"></span>
        </div>
        <button class="kb-reset-all updates-on" id="update-check">Check for Updates…</button>
      </div>
    </div>
    </div><!-- /tab-general -->

    <div id="tab-keybindings" class="tab-pane">
      <div id="kb-groups"></div>
      <div id="kb-record-status" class="sr-only" aria-live="polite" aria-atomic="true"></div>
      <div id="kb-error" class="kb-error"></div>
      <div class="kb-footer">
        <span class="kb-note">Click a shortcut, then press the new combination.</span>
        <button class="kb-reset-all" id="kb-reset-all">Reset all</button>
      </div>
    </div>

    <div id="tab-proxies" class="tab-pane">
      <div id="proxy-list"></div>
      <div class="kb-footer">
        <span class="kb-note">Tabs opened on a proxy's sites send everything through it and keep their own cookies.</span>
        <button class="kb-reset-all" id="proxy-add">Add proxy</button>
      </div>
    </div>

    <div id="tab-extensions" class="tab-pane">
      <div id="ext-list"></div>
      <div id="ext-controls">
        <div class="section">
          <div class="row with-hint">
            <div class="row-label-stack">
              <label class="row-label" for="ext-source">Add from Chrome Web Store</label>
              <span class="row-hint">Paste the extension's store page address, or its id.</span>
            </div>
            <span class="proxy-inline">
              <input type="text" id="ext-source" placeholder="chromewebstore.google.com/detail/…" spellcheck="false" autocomplete="off">
              <button class="proxy-link" id="ext-add">Add</button>
            </span>
          </div>
        </div>
        <div class="kb-footer">
          <span class="kb-note">Extensions run in normal tabs, not in private ones.</span>
          <button class="kb-reset-all" id="ext-pick">Add from file…</button>
        </div>
      </div>
    </div>

    <div id="tab-mail" class="tab-pane">
      <div id="mail-list"></div>
      <div class="kb-footer">
        <span class="kb-note">Passwords are kept in the macOS Keychain. Mail stays on this Mac and your mail server.</span>
        <button class="kb-reset-all" id="mail-add">Add account</button>
      </div>
      <div class="section">
        <div class="section-title">Privacy</div>
        <div class="row with-hint">
          <div class="row-label-stack">
            <label class="row-label" id="mail_block_images_label" for="mail_block_images">Block remote images</label>
            <span class="row-hint">Senders can't see when you open their mail. Show images per message or always for a sender.</span>
          </div>
          <button class="toggle" id="mail_block_images" data-key="mail_block_images" role="switch" aria-checked="false" aria-labelledby="mail_block_images_label"></button>
        </div>
      </div>
      <div class="section">
        <div class="section-title">AI</div>
        <div class="row with-hint">
          <div class="row-label-stack">
            <label class="row-label" id="mail_ai_label" for="mail_ai">Mail AI</label>
            <span class="row-hint">Summaries, drafts, the daily brief and task extraction. Mail text you use them on goes to your assistant agent.</span>
          </div>
          <button class="toggle" id="mail_ai" data-key="mail_ai" role="switch" aria-checked="false" aria-labelledby="mail_ai_label"></button>
        </div>
        <div class="row with-hint">
          <div class="row-label-stack">
            <label class="row-label" id="mail_ai_summaries_label" for="mail_ai_summaries">Summaries as mail arrives</label>
            <span class="row-hint">A one-line summary on each Important conversation.</span>
          </div>
          <button class="toggle" id="mail_ai_summaries" data-key="mail_ai_summaries" role="switch" aria-checked="false" aria-labelledby="mail_ai_summaries_label"></button>
        </div>
        <div class="row with-hint">
          <div class="row-label-stack">
            <label class="row-label" id="mail_brief_morning_label" for="mail_brief_morning">Morning brief</label>
            <span class="row-hint">The assistant sorts your inbox into Needs reply, Waiting, FYI and Can archive the first time you open octoweb each day.</span>
          </div>
          <button class="toggle" id="mail_brief_morning" data-key="mail_brief_morning" role="switch" aria-checked="false" aria-labelledby="mail_brief_morning_label"></button>
        </div>
      </div>
      <div class="section">
        <div class="section-title">Calendar</div>
        <div class="row with-hint">
          <div class="row-label-stack">
            <label class="row-label" for="cal-alert">Meeting alerts</label>
            <span class="row-hint">A reminder with a Join button before each meeting, and a macOS notification when Octoweb is in the background.</span>
          </div>
          <select id="cal-alert">
            <option value="-1">Off</option>
            <option value="0">When it starts</option>
            <option value="1">1 minute before</option>
            <option value="2">2 minutes before</option>
            <option value="5">5 minutes before</option>
            <option value="10">10 minutes before</option>
          </select>
        </div>
        <div class="row with-hint">
          <div class="row-label-stack">
            <span class="row-label" id="cal-hours-label">Working hours</span>
            <span class="row-hint">The free times "Copy my free times" offers, on weekdays.</span>
          </div>
          <span class="proxy-inline" role="group" aria-labelledby="cal-hours-label"><input type="time" id="cal-day-start" aria-label="From"><input type="time" id="cal-day-end" aria-label="To"></span>
        </div>
      </div>
    </div>
  </div>
</div>
<script>
(function() {
  function ipc(msg) {
    window.ipc.postMessage(JSON.stringify(msg));
  }

  function close() {
    cancelRecord();
    ipc({ type: 'settings_close' });
  }

  document.getElementById('backdrop').addEventListener('mousedown', function(e) {
    if (e.target === this) close();
  });
  document.getElementById('close-btn').addEventListener('click', close);
  document.addEventListener('keydown', function(e) {
    if (e.key === 'Escape') { e.preventDefault(); close(); return; }
    if (e.key === 'Tab') {
      var focusable = Array.prototype.slice.call(document.querySelectorAll('#panel button:not([disabled]), #panel input:not([disabled]), #panel select:not([disabled]), #panel textarea:not([disabled])'))
        .filter(function(el) { return el.offsetParent !== null; });
      if (!focusable.length) return;
      var first = focusable[0], last = focusable[focusable.length - 1];
      if (e.shiftKey && document.activeElement === first) { e.preventDefault(); last.focus(); }
      else if (!e.shiftKey && document.activeElement === last) { e.preventDefault(); first.focus(); }
    }
  });

  var lastValidValues = {};

  function validationError(el) {
    var value = el.value.trim();
    if (el.id === 'search_engine' && value.indexOf('{}') < 0) {
      return 'Include {} where the search terms should appear.';
    }
    if (el.type !== 'number') return '';
    if (value === '' || !Number.isFinite(Number(value)) || !Number.isInteger(Number(value))) {
      return 'Enter a whole number.';
    }
    var number = Number(value);
    if ((el.id === 'max_history' || el.id === 'max_tabs') && number < 0) {
      return 'Enter a non-negative integer.';
    }
    var min = el.hasAttribute('min') ? Number(el.min) : null;
    var max = el.hasAttribute('max') ? Number(el.max) : null;
    if (min !== null && number < min) return 'Enter ' + min + ' or more.';
    if (max !== null && number > max) return 'Enter ' + max + ' or less.';
    return '';
  }

  function showRowError(el, message) {
    var row = el.closest('.row');
    if (!row) return;
    var error = row.querySelector('.row-error');
    if (message) {
      if (!error) {
        error = document.createElement('div');
        error.className = 'row-error';
        error.setAttribute('aria-live', 'polite');
        row.appendChild(error);
      }
      error.textContent = message;
      el.setAttribute('aria-invalid', 'true');
      el.setAttribute('aria-describedby', error.id || (error.id = el.id + '-error'));
    } else {
      if (error) error.remove();
      el.removeAttribute('aria-invalid');
      el.removeAttribute('aria-describedby');
    }
  }

  function commitInput(el) {
    var error = validationError(el);
    showRowError(el, error);
    if (error) return;
    lastValidValues[el.dataset.key] = el.value;
    ipc({ type: 'settings_update', key: el.dataset.key, value: el.value });
  }

  // Validate locally before any text or numeric value reaches Rust.
  document.querySelectorAll('input[data-key]').forEach(function(el) {
    el.addEventListener('change', function() { commitInput(el); });
  });

  // Toggle switches
  document.querySelectorAll('.toggle[data-key]').forEach(function(el) {
    el.addEventListener('click', function() {
      var on = !el.classList.contains('on');
      el.classList.toggle('on', on);
      el.setAttribute('aria-checked', on ? 'true' : 'false');
      ipc({ type: 'settings_update', key: el.dataset.key, value: on ? 'true' : 'false' });
    });
  });

  // Populate fields from Rust
  window.__setConfig = function(cfg) {
    for (var key in cfg) {
      var el = document.getElementById(key);
      if (!el) continue;
      if (el.classList.contains('toggle')) {
        var on = cfg[key] === true || cfg[key] === 'true';
        el.classList.toggle('on', on);
        el.setAttribute('aria-checked', on ? 'true' : 'false');
      } else {
        el.value = cfg[key];
        if (el.dataset.key) lastValidValues[el.dataset.key] = String(cfg[key]);
        showRowError(el, '');
      }
    }
    proxies = cfg.proxies;
    renderProxies();
    mailAccounts = cfg.mail_accounts || [];
    renderMail();
    var hm = function(m) { return String(Math.floor(m / 60)).padStart(2, '0') + ':' + String(m % 60).padStart(2, '0'); };
    document.getElementById('cal-alert').value = String(cfg.calendar_alert_min);
    document.getElementById('cal-day-start').value = hm(cfg.calendar_day_start);
    document.getElementById('cal-day-end').value = hm(cfg.calendar_day_end);
    syncSearchEngine();
    showAppearance('appearance-seg', cfg.appearance);
    showAppearance('terminal-appearance-seg', cfg.terminal_appearance);
    requestAnimationFrame(function() { document.getElementById('close-btn').focus(); });
  };

  document.getElementById('cal-alert').addEventListener('change', function(e) {
    ipc({ type: 'settings_update', key: 'calendar_alert_min', value: e.target.value });
  });
  ['cal-day-start', 'cal-day-end'].forEach(function(id) {
    document.getElementById(id).addEventListener('change', function(e) {
      if (e.target.value) ipc({ type: 'settings_update', key: id === 'cal-day-start' ? 'calendar_day_start' : 'calendar_day_end', value: e.target.value });
    });
  });

  // ── Updates ───────────────────────────────────────────────────────────
  // Sparkle keeps these in its own user defaults, so Rust sends them apart
  // from the config. A --dev build, or one without the signing key, has none.
  window.__setUpdates = function(u) {
    document.querySelectorAll('.updates-on').forEach(function(el) { el.style.display = u.available ? '' : 'none'; });
    document.getElementById('update-version').textContent = 'Octoweb ' + u.version;
    document.getElementById('update-hint').textContent = u.available
      ? 'Each update is checked against Octoweb\'s signing key before it installs.'
      : 'Updates are off in this build.';
    if (!u.available) return;
    [['update_checks', u.checks], ['update_downloads', u.downloads]].forEach(function(p) {
      var el = document.getElementById(p[0]);
      el.classList.toggle('on', p[1]);
      el.setAttribute('aria-checked', p[1] ? 'true' : 'false');
    });
  };
  document.getElementById('update-check').addEventListener('click', function() {
    ipc({ type: 'check_updates' });
  });

  // ── Appearance ────────────────────────────────────────────────────────
  // Rust applies it app-wide: every window and page switches at once, this
  // panel included. The terminal row follows it or locks the terminal panel.
  var APPEARANCE_KEYS = {
    'appearance-seg': 'appearance',
    'terminal-appearance-seg': 'terminal_appearance'
  };

  function showAppearance(seg, value) {
    document.querySelectorAll('#' + seg + ' button').forEach(function(b) {
      b.setAttribute('aria-pressed', String(b.dataset.appearance === value));
    });
  }

  Object.keys(APPEARANCE_KEYS).forEach(function(seg) {
    document.querySelectorAll('#' + seg + ' button').forEach(function(b) {
      b.addEventListener('click', function() {
        showAppearance(seg, b.dataset.appearance);
        ipc({ type: 'settings_update', key: APPEARANCE_KEYS[seg], value: b.dataset.appearance });
      });
    });
  });

  // ── Search engine presets ─────────────────────────────────────────────
  // The select holds full URL templates; "custom" reveals the raw URL input.
  var seSelect    = document.getElementById('search_engine_select');
  var seCustomRow = document.getElementById('search_engine_custom_row');
  var seInput     = document.getElementById('search_engine');

  function syncSearchEngine() {
    var tpl = seInput.value;
    var preset = Array.prototype.some.call(seSelect.options, function(o) {
      return o.value === tpl;
    });
    seSelect.value = preset ? tpl : 'custom';
    seCustomRow.style.display = preset ? 'none' : '';
  }

  seSelect.addEventListener('change', function() {
    if (seSelect.value === 'custom') {
      seCustomRow.style.display = '';
      seInput.focus();
      return;
    }
    seCustomRow.style.display = 'none';
    seInput.value = seSelect.value;
    lastValidValues.search_engine = seSelect.value;
    showRowError(seInput, '');
    ipc({ type: 'settings_update', key: 'search_engine', value: seSelect.value });
  });

  // ── Proxies ───────────────────────────────────────────────────────────
  // The list round-trips as one JSON value. Ids are made here once and never
  // change: Rust derives each proxy's cookie store and Keychain entry from
  // them. A typed password rides along once as `password`; Rust moves it to
  // the Keychain and only `has_password` stays.
  var proxies = [];
  var proxyStatus = {};
  var openProxy = -1;
  var removeArmed = null;
  var proxyList = document.getElementById('proxy-list');
  var KIND_LABELS = { ssh: 'SSH tunnel', socks5: 'SOCKS5', http: 'HTTP' };

  // Both mirror site_proxy::valid_endpoint in Rust.
  function hostError(host) {
    return /^[A-Za-z0-9.\-_:]+$/.test(host) ? '' : 'Enter a host name or IP address.';
  }
  function serverError(server) {
    return server && server[0] !== '-' && !/[\s\x00-\x1f\x7f-\x9f]/.test(server)
      ? '' : 'Enter user@server or a host from ~/.ssh/config.';
  }
  function endpointError(p) {
    return p.kind === 'ssh' ? serverError(p.ssh) : hostError(p.host);
  }

  function saveProxies() {
    ipc({ type: 'settings_update', key: 'proxies', value: JSON.stringify(proxies) });
    proxies.forEach(function(p) { delete p.password; });
  }

  function proxyState(p) {
    if (!p.enabled) return { state: 'off', text: 'Off' };
    if (p.kind !== 'ssh') return { state: 'on', text: 'On' };
    var hex = p.id.map(function(b) { return (b < 16 ? '0' : '') + b.toString(16); }).join('');
    var s = proxyStatus[hex];
    if (s && s.state === 'connected') return { state: 'connected', text: 'Connected · SOCKS5 on 127.0.0.1:' + p.port };
    if (s && s.state === 'failed') return { state: 'failed', text: s.message };
    return { state: 'connecting', text: 'Connecting…' };
  }

  function proxyTitle(p, i) {
    return p.name || (p.kind === 'ssh' ? p.ssh : p.host) || 'Proxy ' + (i + 1);
  }

  function proxySummary(p) {
    var target = p.kind === 'ssh'
      ? (p.ssh || 'No server')
      : (p.host ? p.host + ':' + p.port : 'No server');
    var sites = p.sites.length === 1 ? '1 site' : p.sites.length + ' sites';
    return KIND_LABELS[p.kind] + ' · ' + target + ' · ' + sites;
  }

  function proxyRow(label, id, control, hint) {
    var labelHtml = '<label class="row-label" for="' + id + '">' + label + '</label>';
    return hint
      ? '<div class="row with-hint"><div class="row-label-stack">' + labelHtml +
          '<span class="row-hint">' + hint + '</span></div>' + control + '</div>'
      : '<div class="row">' + labelHtml + control + '</div>';
  }

  function proxyBody(p, i) {
    var id = 'proxy-' + i;
    var st = proxyState(p);
    var kinds = ['ssh', 'socks5', 'http'].map(function(k) {
      return '<button data-kind="' + k + '" aria-pressed="' + (p.kind === k) + '">' + KIND_LABELS[k] + '</button>';
    }).join('');
    var html = proxyRow('Name', id + '-name',
        '<input type="text" id="' + id + '-name" data-field="name" placeholder="Optional" value="' + esc(p.name) + '">') +
      '<div class="row"><span class="row-label" id="' + id + '-kind">Type</span>' +
        '<span class="seg" role="group" aria-labelledby="' + id + '-kind">' + kinds + '</span></div>';
    if (p.kind === 'ssh') {
      html += proxyRow('Server', id + '-ssh',
          '<input type="text" id="' + id + '-ssh" data-field="ssh" placeholder="user@server" value="' + esc(p.ssh) + '">',
          'What you would pass to <code>ssh</code>. Hosts from ~/.ssh/config work too.') +
        proxyRow('Password', id + '-password',
          '<span class="proxy-inline">' +
            '<input type="password" id="' + id + '-password" data-field="password" autocomplete="off" placeholder="' +
              (p.has_password ? 'Saved in Keychain' : 'Optional') + '">' +
            (p.has_password ? '<button class="proxy-link" data-forget>Forget</button>' : '') +
          '</span>',
          'Keys and ssh-agent are tried first.') +
        proxyRow('Local port', id + '-port',
          '<input type="number" id="' + id + '-port" data-field="port" min="1024" max="65535" step="1" value="' + p.port + '">',
          'Tabs reach the tunnel at 127.0.0.1 on this port. macOS reserves ports below 1024.');
    } else {
      html += proxyRow('Server', id + '-host',
        '<span class="proxy-inline">' +
          '<input type="text" id="' + id + '-host" data-field="host" placeholder="127.0.0.1" value="' + esc(p.host) + '">' +
          '<input type="number" id="' + id + '-port" data-field="port" min="1" max="65535" step="1" value="' + p.port + '" aria-label="Port">' +
        '</span>');
    }
    html += proxyRow('Sites', id + '-sites',
        '<textarea id="' + id + '-sites" data-field="sites" rows="3" placeholder="example.com">' + esc(p.sites.join('\n')) + '</textarea>',
        'One per line, a domain or a full URL. Subdomains match too.') +
      '<div class="row">' +
        '<span class="proxy-status"><span class="proxy-dot ' + st.state + '"></span>' +
          '<span class="proxy-status-text">' + esc(st.text) + '</span></span>' +
        '<button class="proxy-remove" data-remove>Remove</button>' +
      '</div>';
    return '<div class="proxy-body" id="' + id + '-body">' + html + '</div>';
  }

  function proxyCard(p, i) {
    var st = proxyState(p);
    var open = i === openProxy;
    var title = proxyTitle(p, i);
    return '<div class="section proxy' + (open ? ' open' : '') + '" data-proxy="' + i + '">' +
      '<div class="proxy-head">' +
        '<button class="proxy-open" aria-expanded="' + open + '" aria-controls="proxy-' + i + '-body">' +
          '<span class="proxy-dot ' + st.state + '" title="' + esc(st.text) + '"></span>' +
          '<span class="proxy-text">' +
            '<span class="proxy-name">' + esc(title) + '</span>' +
            '<span class="proxy-sub">' + esc(proxySummary(p)) + '</span>' +
          '</span>' +
          '<span class="proxy-chev" aria-hidden="true">›</span>' +
        '</button>' +
        '<button class="toggle' + (p.enabled ? ' on' : '') + '" role="switch" aria-checked="' + p.enabled + '" aria-label="Use ' + esc(title) + '"></button>' +
      '</div>' +
      (open ? proxyBody(p, i) : '') +
    '</div>';
  }

  function renderProxies(focusSelector) {
    removeArmed = null;
    proxyList.innerHTML = proxies.length
      ? proxies.map(proxyCard).join('')
      : '<div class="section proxy-empty"><div class="row-label">No proxies yet</div>' +
        '<span class="kb-note">Add an SSH tunnel or a SOCKS5/HTTP proxy, then list the sites that should use it.</span></div>';
    var el = focusSelector && proxyList.querySelector(focusSelector);
    if (el) el.focus();
  }

  // Updates a card's header and status line in place, so an open editor
  // keeps focus while you type.
  function refreshProxy(i) {
    var card = proxyList.querySelector('[data-proxy="' + i + '"]');
    if (!card) return;
    var p = proxies[i];
    var st = proxyState(p);
    var title = proxyTitle(p, i);
    card.querySelectorAll('.proxy-dot').forEach(function(dot) { dot.className = 'proxy-dot ' + st.state; });
    card.querySelector('.proxy-dot').title = st.text;
    card.querySelector('.proxy-name').textContent = title;
    card.querySelector('.proxy-sub').textContent = proxySummary(p);
    var text = card.querySelector('.proxy-status-text');
    if (text) text.textContent = st.text;
    var toggle = card.querySelector('.toggle');
    toggle.classList.toggle('on', p.enabled);
    toggle.setAttribute('aria-checked', p.enabled ? 'true' : 'false');
    toggle.setAttribute('aria-label', 'Use ' + title);
  }

  // Rust pushes {hex id: {state, message}} for SSH tunnels on open and on change.
  window.__setProxyStatus = function(status) {
    proxyStatus = status;
    proxies.forEach(function(_, i) { refreshProxy(i); });
  };

  proxyList.addEventListener('click', function(e) {
    var card = e.target.closest('[data-proxy]');
    if (!card) return;
    var i = Number(card.dataset.proxy);
    var p = proxies[i];
    var remove = e.target.closest('[data-remove]');
    if (removeArmed && remove !== removeArmed) {
      removeArmed.textContent = 'Remove';
      removeArmed = null;
    }
    if (remove) {
      // Two clicks, so a stray one can't drop a proxy.
      if (!removeArmed) {
        removeArmed = remove;
        remove.textContent = 'Click again to remove';
        return;
      }
      proxies.splice(i, 1);
      openProxy = -1;
      renderProxies();
      saveProxies();
      return;
    }
    if (e.target.closest('.proxy-open')) {
      openProxy = openProxy === i ? -1 : i;
      renderProxies('[data-proxy="' + i + '"] .proxy-open');
      return;
    }
    var kind = e.target.closest('[data-kind]');
    if (kind) {
      if (p.kind === kind.dataset.kind) return;
      p.kind = kind.dataset.kind;
      // Don't leave a proxy on that can't connect with its new type.
      if (endpointError(p)) p.enabled = false;
      renderProxies('[data-proxy="' + i + '"] [data-kind="' + p.kind + '"]');
      saveProxies();
      return;
    }
    if (e.target.closest('[data-forget]')) {
      p.password = '';
      p.has_password = false;
      saveProxies();
      renderProxies('[data-proxy="' + i + '"] [data-field="password"]');
      return;
    }
    if (e.target.closest('.toggle')) {
      var error = p.enabled ? '' : endpointError(p);
      if (error) {
        openProxy = i;
        renderProxies();
        var input = proxyList.querySelector('[data-proxy="' + i + '"] [data-field="' + (p.kind === 'ssh' ? 'ssh' : 'host') + '"]');
        showRowError(input, error);
        input.focus();
        return;
      }
      p.enabled = !p.enabled;
      refreshProxy(i);
      saveProxies();
    }
  });

  proxyList.addEventListener('change', function(e) {
    var el = e.target;
    var card = el.closest('[data-proxy]');
    if (!card || !el.dataset.field) return;
    var i = Number(card.dataset.proxy);
    var p = proxies[i];
    var value = el.value.trim();
    var error = '';
    switch (el.dataset.field) {
      case 'name':
        p.name = value;
        break;
      case 'host':
        error = hostError(value);
        if (!error) p.host = value;
        break;
      case 'ssh':
        error = serverError(value);
        if (!error) p.ssh = value;
        break;
      case 'port':
        error = validationError(el);
        if (!error) p.port = Number(value);
        break;
      case 'password':
        if (!el.value) return;
        p.password = el.value;
        p.has_password = true;
        break;
      case 'sites':
        p.sites = el.value.split('\n').map(function(s) { return s.trim(); }).filter(Boolean);
        break;
    }
    showRowError(el, error);
    if (error) return;
    saveProxies();
    if (el.dataset.field === 'password') {
      el.value = '';
      el.placeholder = 'Saved in Keychain';
      if (!card.querySelector('[data-forget]')) {
        el.insertAdjacentHTML('afterend', '<button class="proxy-link" data-forget>Forget</button>');
      }
    }
    refreshProxy(i);
  });

  document.getElementById('proxy-add').addEventListener('click', function() {
    var port = 1080;
    while (proxies.some(function(p) { return p.port === port; })) port++;
    proxies.push({
      id: Array.from(crypto.getRandomValues(new Uint8Array(16))),
      name: '',
      enabled: false,
      kind: 'ssh',
      host: '',
      port: port,
      ssh: '',
      has_password: false,
      sites: []
    });
    openProxy = proxies.length - 1;
    renderProxies('[data-proxy="' + openProxy + '"] [data-field="ssh"]');
    saveProxies();
  });

  // ── Extensions ─────────────────────────────────────────────────────────────
  var extList = document.getElementById('ext-list');
  var extSource = document.getElementById('ext-source');
  var extRemoveArmed = null;
  var extItems = {};
  var extWorkspaces = [];
  var extActive = null;

  // "all" also covers workspaces created later; ticking every workspace by
  // hand does not.
  function extScope(x) {
    var all = x.scope === 'all';
    var chips = '<button data-scope-all aria-pressed="' + all + '">All workspaces</button>' +
      extWorkspaces.map(function(w) {
        var on = all || x.scope.indexOf(w.id) >= 0;
        return '<button data-ws="' + esc(w.id) + '" aria-pressed="' + on + '">' +
          '<span class="ws-dot" style="background:' + esc(w.color) + '"></span>' + esc(w.name) + '</button>';
      }).join('');
    return '<div class="row ext-scope"><span class="row-label" id="ext-scope-' + esc(x.id) + '">Runs in</span>' +
      '<span class="seg" role="group" aria-labelledby="ext-scope-' + esc(x.id) + '">' + chips + '</span></div>';
  }

  function extCard(x) {
    var icon = x.icon
      ? '<img class="ext-icon" src="' + esc(x.icon) + '" alt="">'
      : '<span class="ext-icon blank"></span>';
    var sub = x.error
      ? '<span class="proxy-sub ext-error">' + esc(x.error) + '</span>'
      : '<span class="proxy-sub" title="' + esc(x.path) + '">' + esc(x.description || x.path) + '</span>';
    var version = x.version ? '<span class="ext-version">' + esc(x.version) + '</span>' : '';
    return '<div class="section" data-ext="' + esc(x.id) + '"><div class="proxy-head">' + icon +
        '<span class="proxy-text"><span class="proxy-name">' + esc(x.name) + version + '</span>' + sub + '</span>' +
        (x.options ? '<button class="proxy-link" data-options>Options</button>' : '') +
        '<button class="proxy-remove" data-remove>Remove</button>' +
        '<button class="toggle' + (x.enabled ? ' on' : '') + '" role="switch" aria-checked="' + x.enabled +
          '" aria-label="Turn on ' + esc(x.name) + '"></button>' +
      '</div>' + extScope(x) + '</div>';
  }

  window.__setExtensions = function(data) {
    extRemoveArmed = null;
    extWorkspaces = data.workspaces;
    extActive = data.active;
    renderMail();
    extItems = {};
    data.items.forEach(function(x) { extItems[x.id] = x; });
    document.getElementById('ext-controls').style.display = data.supported ? '' : 'none';
    if (!data.supported) {
      extList.innerHTML = '<div class="section proxy-empty"><div class="row-label">Extensions need macOS 15.4 or later</div></div>';
      return;
    }
    extList.innerHTML = data.items.length
      ? data.items.map(extCard).join('')
      : '<div class="section proxy-empty"><div class="row-label">No extensions yet</div>' +
        '<span class="kb-note">Add one from the Chrome Web Store, or a folder, .zip, .xpi or .crx file.</span></div>';
  };

  extList.addEventListener('click', function(e) {
    var card = e.target.closest('[data-ext]');
    if (!card) return;
    var id = card.dataset.ext;
    var remove = e.target.closest('[data-remove]');
    if (extRemoveArmed && remove !== extRemoveArmed) {
      extRemoveArmed.textContent = 'Remove';
      extRemoveArmed = null;
    }
    if (remove) {
      // Two clicks, so a stray one can't uninstall an extension and its data.
      if (!extRemoveArmed) {
        extRemoveArmed = remove;
        remove.textContent = 'Click again to remove';
        return;
      }
      ipc({ type: 'ext_remove', id: id });
      return;
    }
    if (e.target.closest('[data-options]')) {
      ipc({ type: 'ext_options', id: id });
      return;
    }
    if (e.target.closest('[data-scope-all]')) {
      if (extItems[id].scope !== 'all') ipc({ type: 'ext_scope', id: id, scope: 'all' });
      return;
    }
    var chip = e.target.closest('[data-ws]');
    if (chip) {
      var current = extItems[id].scope === 'all'
        ? extWorkspaces.map(function(w) { return w.id; })
        : extItems[id].scope.slice();
      var at = current.indexOf(chip.dataset.ws);
      if (at >= 0) current.splice(at, 1); else current.push(chip.dataset.ws);
      // Off everywhere is what the switch is for.
      if (!current.length) return;
      ipc({ type: 'ext_scope', id: id, scope: current });
      return;
    }
    var toggle = e.target.closest('.toggle');
    if (toggle) {
      var on = !toggle.classList.contains('on');
      toggle.classList.toggle('on', on);
      toggle.setAttribute('aria-checked', on ? 'true' : 'false');
      ipc({ type: 'ext_enable', id: id, enabled: on });
    }
  });

  function installFromSource() {
    var value = extSource.value.trim();
    if (!value) return;
    ipc({ type: 'ext_install', value: value });
    extSource.value = '';
  }
  document.getElementById('ext-add').addEventListener('click', installFromSource);
  extSource.addEventListener('keydown', function(e) {
    if (e.key === 'Enter') { e.preventDefault(); installFromSource(); }
  });
  document.getElementById('ext-pick').addEventListener('click', function() {
    ipc({ type: 'ext_pick' });
  });

  // ── Mail ───────────────────────────────────────────────────────────────────
  // Like proxies, the list round-trips as one JSON value with ids made here
  // once; a typed password rides along once and Rust moves it to the Keychain.
  var mailAccounts = [];
  var mailStatus = {};
  var mailCheck = {};
  var openMail = -1;
  var mailRemoveArmed = null;
  var mailList = document.getElementById('mail-list');
  var PROVIDERS = { gmail: 'Gmail', icloud: 'iCloud', privateemail: 'Private Email', outlook: 'Outlook / 365', other: 'Other IMAP' };
  var PROVIDER_HINTS = {
    gmail: 'Use an <a class="mail-link" data-url="https://myaccount.google.com/apppasswords">app password</a>, which needs 2-Step Verification.',
    icloud: 'Use an <a class="mail-link" data-url="https://account.apple.com/account/manage">app-specific password</a>, which needs two-factor authentication.',
    privateemail: 'Your mailbox password. Servers: mail.privateemail.com.',
    outlook: 'Microsoft accounts sign in with OAuth through an app you register.',
    other: 'IMAP over TLS, usually port 993; SMTP with TLS (465) or STARTTLS (587).'
  };
  var MAIL_COLORS = ['#0a84ff', '#ff9f0a', '#30d158', '#bf5af2', '#ff375f', '#64d2ff'];
  // Own-client OAuth (spec: Gmail OAuth). How to make the client, per provider.
  var OAUTH_HELP = {
    gmail: 'In <a class="mail-link" data-url="https://console.cloud.google.com/apis/credentials">Google Cloud Console</a>, create an OAuth client of type <b>Desktop app</b>, and set the OAuth consent screen to <b>In production</b> \u2014 in Testing, Google ends the sign-in after 7 days. Paste its client ID and secret, then sign in. For the calendar, also turn on the <a class="mail-link" data-url="https://console.cloud.google.com/apis/library/calendar-json.googleapis.com">Google Calendar API</a> in the same project.',
    outlook: 'In <a class="mail-link" data-url="https://entra.microsoft.com/#view/Microsoft_AAD_RegisteredApps/ApplicationsListBlade">Microsoft Entra \u2192 App registrations</a>, register an app, add the <b>Mobile and desktop</b> platform with redirect URI <code>http://127.0.0.1</code>, and the delegated permissions IMAP.AccessAsUser.All, SMTP.Send and offline_access, and Calendars.ReadWrite on Microsoft Graph for the calendar. Paste its client ID; no secret is needed.'
  };
  function oauthCapable(a) { return a.provider === 'gmail' || a.provider === 'outlook'; }
  // This build carries a Google client (build.sh), so Gmail needs no setup.
  var GOOGLE_BUILT_IN = /*@@GOOGLE_BUILT_IN@@*/false;
  function ownClient(a) { return a.provider !== 'gmail' || !GOOGLE_BUILT_IN || !!a.oauth_client_id; }
  function preferGoogle(a) { if (a.provider === 'gmail' && GOOGLE_BUILT_IN && !a.has_password) a.auth = 'oauth'; }

  function mailHex(a) {
    return a.id.map(function(b) { return (b < 16 ? '0' : '') + b.toString(16); }).join('');
  }
  function guessProvider(address) {
    var domain = (address.split('@')[1] || '').toLowerCase();
    if (domain === 'gmail.com' || domain === 'googlemail.com') return 'gmail';
    if (domain === 'icloud.com' || domain === 'me.com' || domain === 'mac.com') return 'icloud';
    if (domain === 'outlook.com' || domain === 'hotmail.com' || domain === 'live.com' || domain === 'msn.com') return 'outlook';
    return null;
  }
  function saveMail() {
    ipc({ type: 'settings_update', key: 'mail_accounts', value: JSON.stringify(mailAccounts) });
    mailAccounts.forEach(function(a) { delete a.password; delete a.client_secret; });
  }
  function mailState(a) {
    var k = mailHex(a), c = mailCheck[k], s = mailStatus[k];
    if (c === 'checking') return { state: 'connecting', text: 'Checking sign-in…' };
    if (c && c !== 'ok') return { state: 'failed', text: c };
    if (!a.address || a.address.indexOf('@') < 0) return { state: 'off', text: 'Needs an address' };
    if (!a.has_password) return { state: 'off', text: a.auth === 'oauth' ? 'Needs sign-in' : 'Needs its password' };
    // ACC-4c: an account left in no workspace stays quiet.
    if (a.scope !== 'all' && !(a.scope.only || []).length) return { state: 'off', text: 'Not in any workspace' };
    if (s && s.state === 'online') return { state: 'connected', text: c === 'ok' ? 'Connected · IMAP and SMTP sign-in work' : 'Connected' };
    if (s && s.state === 'auth') return { state: 'failed', text: 'Sign-in failed' + (s.message ? ' — ' + s.message : '') };
    if (s && s.state === 'offline') return { state: 'failed', text: 'Offline, retrying' + (s.message ? ' — ' + s.message : '') };
    if (s && s.state === 'nopassword') return { state: 'off', text: 'Needs its password' };
    return { state: 'connecting', text: 'Connecting…' };
  }
  function mailScope(a, i) {
    var all = a.scope === 'all';
    var chips = '<button data-mail-scope-all aria-pressed="' + all + '">All workspaces</button>' +
      extWorkspaces.map(function(w) {
        var on = all || (a.scope.only || []).indexOf(w.id) >= 0;
        return '<button data-mail-ws="' + esc(w.id) + '" aria-pressed="' + on + '">' +
          '<span class="ws-dot" style="background:' + esc(w.color) + '"></span>' + esc(w.name) + '</button>';
      }).join('');
    return '<div class="row ext-scope with-hint"><div class="row-label-stack"><span class="row-label" id="mail-' + i + '-scope">Workspaces</span>' +
      '<span class="row-hint">Its inbox, search and notifications appear only there.</span></div>' +
      '<span class="seg" role="group" aria-labelledby="mail-' + i + '-scope">' + chips + '</span></div>';
  }
  function mailBody(a, i) {
    var id = 'mail-' + i;
    var st = mailState(a);
    var providers = Object.keys(PROVIDERS).map(function(p) {
      return '<button data-provider="' + p + '" aria-pressed="' + (a.provider === p) + '">' + PROVIDERS[p] + '</button>';
    }).join('');
    var html = '<div class="row with-hint"><div class="row-label-stack"><span class="row-label" id="' + id + '-provider">Provider</span>' +
        '<span class="row-hint">' + (a.auth === 'oauth' && a.provider !== 'outlook' ? (ownClient(a) ? 'Signs in with your own OAuth client, below.' : 'Signs in with your Google account.') : PROVIDER_HINTS[a.provider]) + '</span></div>' +
        '<span class="seg" role="group" aria-labelledby="' + id + '-provider">' + providers + '</span></div>' +
      proxyRow('Address', id + '-address', '<input type="email" id="' + id + '-address" data-mail="address" placeholder="you@example.com" value="' + esc(a.address) + '" spellcheck="false">') +
      (oauthCapable(a) && a.provider !== 'outlook'
        ? '<div class="row"><span class="row-label" id="' + id + '-auth">Sign-in</span><span class="seg" role="group" aria-labelledby="' + id + '-auth">' +
            '<button data-auth="password" aria-pressed="' + (a.auth !== 'oauth') + '">App password</button>' +
            '<button data-auth="oauth" aria-pressed="' + (a.auth === 'oauth') + '">' + (ownClient(a) ? 'OAuth (your own client)' : 'Sign in with Google') + '</button></span></div>'
        : '') +
      (a.auth === 'oauth'
        ? (ownClient(a)
          ? '<div class="row with-hint"><div class="row-label-stack"><span class="row-label">OAuth client</span><span class="row-hint">' + OAUTH_HELP[a.provider] + '</span></div></div>' +
            proxyRow('Client ID', id + '-cid', '<input type="text" id="' + id + '-cid" data-mail="oauth_client_id" placeholder="…apps.googleusercontent.com" value="' + esc(a.oauth_client_id) + '" spellcheck="false">')
          : '') +
          (a.provider === 'gmail' && ownClient(a)
            ? proxyRow('Client secret', id + '-csec', '<span class="proxy-inline"><input type="password" id="' + id + '-csec" data-mail="client_secret" autocomplete="off" placeholder="' + (a.has_client_secret ? 'Saved in Keychain' : 'From the same client') + '"></span>')
            : '') +
          '<div class="row"><span class="row-label">Account</span><span class="proxy-inline"><span class="kb-note">' + (a.has_password ? 'Signed in' : 'Not signed in') + '</span>' +
            '<button class="proxy-link" data-mail-signin>' + (a.has_password ? 'Sign in again' : (a.provider === 'gmail' ? 'Sign in with Google' : 'Sign in with Microsoft')) + '</button></span></div>'
        : proxyRow('Password', id + '-password',
        '<span class="proxy-inline"><input type="password" id="' + id + '-password" data-mail="password" autocomplete="off" placeholder="' +
          (a.has_password ? 'Saved in Keychain' : (a.provider === 'gmail' || a.provider === 'icloud' ? 'App password' : 'Password')) + '">' +
          (a.has_password ? '<button class="proxy-link" data-mail-forget>Forget</button>' : '') + '</span>')) +
      proxyRow('Name', id + '-name', '<input type="text" id="' + id + '-name" data-mail="name" placeholder="Shown on mail you send" value="' + esc(a.name) + '">');
    if (a.provider === 'other') {
      html += proxyRow('IMAP server', id + '-imap',
          '<span class="proxy-inline"><input type="text" id="' + id + '-imap" data-mail="imap_host" placeholder="imap.example.com" value="' + esc(a.imap_host) + '" spellcheck="false">' +
          '<input type="number" data-mail="imap_port" min="1" max="65535" value="' + (a.imap_port || 993) + '" aria-label="IMAP port"></span>') +
        proxyRow('SMTP server', id + '-smtp',
          '<span class="proxy-inline"><input type="text" id="' + id + '-smtp" data-mail="smtp_host" placeholder="smtp.example.com" value="' + esc(a.smtp_host) + '" spellcheck="false">' +
          '<input type="number" data-mail="smtp_port" min="1" max="65535" value="' + (a.smtp_port || 465) + '" aria-label="SMTP port"></span>') +
        '<div class="row"><span class="row-label" id="' + id + '-sec">SMTP security</span><span class="seg" role="group" aria-labelledby="' + id + '-sec">' +
          [['tls', 'TLS'], ['starttls', 'STARTTLS'], ['plain', 'None (this Mac only)']].map(function(s) {
            return '<button data-security="' + s[0] + '" aria-pressed="' + (a.smtp_security === s[0]) + '">' + s[1] + '</button>';
          }).join('') + '</span></div>' +
        proxyRow('Login', id + '-login', '<input type="text" id="' + id + '-login" data-mail="login" placeholder="Same as the address" value="' + esc(a.login) + '" spellcheck="false">');
    }
    html += proxyRow('Signature', id + '-signature', '<textarea id="' + id + '-signature" data-mail="signature" rows="3" placeholder="Optional">' + esc(a.signature) + '</textarea>') +
      '<div class="row with-hint"><div class="row-label-stack"><span class="row-label" id="' + id + '-gk">Gatekeeper</span>' +
        '<span class="row-hint">Mail from senders you never wrote to or heard from waits in New senders until you accept or block them.</span></div>' +
        '<button class="toggle' + (a.gatekeeper ? ' on' : '') + '" data-mail-gk role="switch" aria-checked="' + a.gatekeeper + '" aria-labelledby="' + id + '-gk"></button></div>' +
      proxyRow('Color', id + '-color', '<input type="color" id="' + id + '-color" data-mail="color" value="' + esc(a.color || MAIL_COLORS[i % MAIL_COLORS.length]) + '">') +
      proxyRow('Archive folder', id + '-archive', '<input type="text" id="' + id + '-archive" data-mail="archive_folder" placeholder="Found on the server" value="' + esc(a.archive_folder) + '" spellcheck="false">',
        'Where Done moves mail. Leave empty unless your server needs a specific folder.') +
      calendarRows(a, i) +
      mailScope(a, i) +
      '<div class="row"><span class="proxy-status"><span class="proxy-dot ' + st.state + '"></span><span class="proxy-status-text">' + esc(st.text) + '</span></span>' +
        '<button class="proxy-link" data-mail-check>Check connection</button>' +
        '<button class="proxy-remove" data-mail-remove>Remove</button></div>';
    return '<div class="proxy-body" id="' + id + '-body">' + html + '</div>';
  }
  // ── Calendar of a mail account (spec 002-calendar ACC-*) ──
  var calendarStatus = {};
  window.__setCalendarStatus = function(status) { calendarStatus = status; renderMailKeepFocus(); };
  var CAL_HINTS = {
    gmail: 'Google Calendar, connected separately from mail.',
    outlook: 'Outlook calendar, through Microsoft Graph.',
    icloud: 'iCloud Calendar over CalDAV, with the same app-specific password.',
    privateemail: 'CalDAV, with the mailbox password.',
    other: 'CalDAV, with the account’s password.'
  };
  function calendarRows(a, i) {
    var id = 'mail-' + i;
    var on = a.calendar !== false;
    var st = calendarStatus[mailHex(a)] || {};
    var html = '<div class="row with-hint"><div class="row-label-stack"><span class="row-label" id="' + id + '-cal">Calendar</span>' +
      '<span class="row-hint">' + CAL_HINTS[a.provider] + ' It shows in the pane’s Calendar tab.</span></div>' +
      '<button class="toggle' + (on ? ' on' : '') + '" data-mail-cal role="switch" aria-checked="' + on + '" aria-labelledby="' + id + '-cal"></button></div>';
    if (!on) return html;
    var text = { online: 'Synced', connecting: 'Connecting…', connect: 'Not connected', auth: 'Sign-in expired', error: st.message || 'Sync failed', off: st.message || 'Off' }[st.state] || 'Waiting';
    if (a.provider === 'gmail' || a.provider === 'outlook') {
      var connected = st.state && st.state !== 'connect';
      html += '<div class="row"><span class="row-label">Calendar access</span><span class="proxy-inline"><span class="kb-note">' + esc(text) + '</span>' +
        '<button class="proxy-link" data-cal-connect>' + (connected ? 'Connect again' : 'Connect calendar') + '</button></span></div>';
    } else {
      html += proxyRow('CalDAV server', id + '-caldav', '<input type="text" id="' + id + '-caldav" data-mail="caldav_url" placeholder="Found automatically" value="' + esc(a.caldav_url || '') + '" spellcheck="false">',
        'Only if it is not found: the server’s CalDAV address. ' + esc(text));
    }
    return html;
  }

  function mailCard(a, i) {
    var st = mailState(a);
    var open = i === openMail;
    return '<div class="section proxy' + (open ? ' open' : '') + '" data-mail-i="' + i + '"><div class="proxy-head">' +
      '<button class="proxy-open" aria-expanded="' + open + '" aria-controls="mail-' + i + '-body">' +
        '<span class="proxy-dot" style="background:' + esc(a.color || MAIL_COLORS[i % MAIL_COLORS.length]) + '" title="Account color"></span>' +
        '<span class="proxy-text"><span class="proxy-name">' + esc(a.address || 'New account') + '</span>' +
        '<span class="proxy-sub">' + esc(PROVIDERS[a.provider] + ' · ' + st.text) + '</span></span>' +
        '<span class="proxy-chev" aria-hidden="true">›</span></button></div>' +
      (open ? mailBody(a, i) : '') + '</div>';
  }
  function renderMail(focusSelector) {
    mailRemoveArmed = null;
    mailList.innerHTML = mailAccounts.length
      ? mailAccounts.map(mailCard).join('')
      : '<div class="section proxy-empty"><div class="row-label">No mail accounts yet</div>' +
        '<span class="kb-note">Add Gmail, iCloud, Namecheap Private Email or any IMAP account. Open the inbox with ⌘⇧I.</span></div>';
    var el = focusSelector && mailList.querySelector(focusSelector);
    if (el) el.focus();
  }
  window.__setMailStatus = function(status) { mailStatus = status; renderMailKeepFocus(); };
  window.__mailVerified = function(key, error) { mailCheck[key] = error || 'ok'; renderMailKeepFocus(); };
  function renderMailKeepFocus() {
    var active = document.activeElement;
    if (active && mailList.contains(active) && active.tagName !== 'BUTTON') return;
    renderMail();
  }
  function checkMail(a) {
    var k = mailHex(a);
    mailCheck[k] = 'checking';
    ipc({ type: 'mail_verify', id: k });
  }

  mailList.addEventListener('click', function(e) {
    var link = e.target.closest('[data-url]');
    if (link) { e.preventDefault(); ipc({ type: 'open_url', url: link.dataset.url }); return; }
    var card = e.target.closest('[data-mail-i]');
    if (!card) return;
    var i = Number(card.dataset.mailI);
    var a = mailAccounts[i];
    var remove = e.target.closest('[data-mail-remove]');
    if (mailRemoveArmed && remove !== mailRemoveArmed) { mailRemoveArmed.textContent = 'Remove'; mailRemoveArmed = null; }
    if (remove) {
      // Two clicks: removing deletes the account's local mail (not the server's).
      if (!mailRemoveArmed) { mailRemoveArmed = remove; remove.textContent = 'Click again — removes its mail from this Mac'; return; }
      mailAccounts.splice(i, 1);
      openMail = -1;
      saveMail();
      renderMail();
      return;
    }
    if (e.target.closest('.proxy-open')) {
      openMail = openMail === i ? -1 : i;
      renderMail('[data-mail-i="' + i + '"] .proxy-open');
      return;
    }
    var prov = e.target.closest('[data-provider]');
    if (prov) {
      a.provider = prov.dataset.provider;
      if (a.provider === 'outlook') a.auth = 'oauth';
      if (!oauthCapable(a)) a.auth = 'password';
      preferGoogle(a);
      saveMail();
      renderMail('[data-mail-i="' + i + '"] [data-provider="' + a.provider + '"]');
      return;
    }
    var authBtn = e.target.closest('[data-auth]');
    if (authBtn) {
      if (a.auth !== authBtn.dataset.auth) {
        a.auth = authBtn.dataset.auth;
        // The stored secret belongs to the other method.
        a.password = ''; a.has_password = false;
        saveMail();
        renderMail();
      }
      return;
    }
    if (e.target.closest('[data-mail-signin]')) {
      mailCheck[mailHex(a)] = 'checking';
      ipc({ type: 'mail_sign_in', id: mailHex(a) });
      renderMail();
      return;
    }
    var sec = e.target.closest('[data-security]');
    if (sec) { a.smtp_security = sec.dataset.security; saveMail(); renderMail(); return; }
    if (e.target.closest('[data-mail-cal]')) { a.calendar = a.calendar === false; saveMail(); renderMail('[data-mail-i="' + i + '"] [data-mail-cal]'); return; }
    if (e.target.closest('[data-cal-connect]')) { ipc({ type: 'calendar_connect', id: mailHex(a) }); return; }
    if (e.target.closest('[data-mail-gk]')) { a.gatekeeper = !a.gatekeeper; saveMail(); renderMail('[data-mail-i="' + i + '"] [data-mail-gk]'); return; }
    if (e.target.closest('[data-mail-forget]')) { a.password = ''; a.has_password = false; saveMail(); renderMail(); return; }
    if (e.target.closest('[data-mail-check]')) { checkMail(a); renderMail(); return; }
    if (e.target.closest('[data-mail-scope-all]')) {
      if (a.scope !== 'all') { a.scope = 'all'; saveMail(); renderMail(); }
      return;
    }
    var chip = e.target.closest('[data-mail-ws]');
    if (chip) {
      var current = a.scope === 'all' ? extWorkspaces.map(function(w) { return w.id; }) : (a.scope.only || []).slice();
      var at = current.indexOf(chip.dataset.mailWs);
      if (at >= 0) current.splice(at, 1); else current.push(chip.dataset.mailWs);
      a.scope = { only: current };
      saveMail();
      renderMail();
    }
  });

  mailList.addEventListener('change', function(e) {
    var el = e.target;
    var card = el.closest('[data-mail-i]');
    if (!card || !el.dataset.mail) return;
    var i = Number(card.dataset.mailI);
    var a = mailAccounts[i];
    var field = el.dataset.mail;
    var value = field === 'signature' ? el.value : el.value.trim();
    if (field === 'password') {
      if (!el.value) return;
      a.password = el.value;
      a.has_password = true;
    } else if (field === 'client_secret') {
      if (!el.value) return;
      a.client_secret = el.value;
      a.has_client_secret = true;
    } else if (field === 'imap_port' || field === 'smtp_port') {
      a[field] = Number(value) || 0;
    } else {
      a[field] = value;
    }
    if (field === 'address') {
      var guess = guessProvider(value);
      if (guess && a.provider !== guess && a.provider !== 'other') {
        a.provider = guess;
        if (guess === 'outlook') a.auth = 'oauth';
        preferGoogle(a);
      }
    }
    saveMail();
    // A new password or address is worth checking at once (ACC-2).
    if ((field === 'password' || field === 'address') && a.auth !== 'oauth' && a.has_password && a.address.indexOf('@') > 0) checkMail(a);
    renderMail();
  });

  document.getElementById('mail-add').addEventListener('click', function() {
    mailAccounts.push({
      id: Array.from(crypto.getRandomValues(new Uint8Array(16))),
      provider: 'gmail',
      address: '',
      name: '',
      color: MAIL_COLORS[mailAccounts.length % MAIL_COLORS.length],
      signature: '',
      gatekeeper: false,
      // ACC-4: a new account starts in the workspace it was added in.
      scope: extActive ? { only: [extActive] } : 'all',
      imap_host: '', imap_port: 993, smtp_host: '', smtp_port: 465, smtp_security: 'tls',
      login: '', archive_folder: '', has_password: false,
      auth: 'password', oauth_client_id: '', has_client_secret: false,
      calendar: true, caldav_url: ''
    });
    openMail = mailAccounts.length - 1;
    renderMail('[data-mail-i="' + openMail + '"] [data-mail="address"]');
    saveMail();
  });

  // ── Tabs ──────────────────────────────────────────────────────────────────
  function showTab(pane) {
    document.querySelectorAll('.tab').forEach(function(x) { x.classList.toggle('active', x.dataset.pane === pane); });
    document.querySelectorAll('.tab-pane').forEach(function(x) { x.classList.toggle('active', x.id === pane); });
    cancelRecord();
  }
  document.querySelectorAll('.tab').forEach(function(t) {
    t.addEventListener('click', function() { showTab(t.dataset.pane); });
  });
  window.__showTab = showTab;

  // ── Keybindings ───────────────────────────────────────────────────────
  function esc(s) {
    return String(s).replace(/[&<>"]/g, function(c) {
      return { '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;' }[c];
    });
  }

  // JS KeyboardEvent.code → the physical key token the Rust keymap understands.
  function codeToToken(code) {
    if (/^Key[A-Z]$/.test(code)) return code.slice(3).toLowerCase();
    if (/^Digit[0-9]$/.test(code)) return code.slice(5);
    var m = {
      Minus: 'minus', Equal: 'equal', BracketLeft: 'bracketleft', BracketRight: 'bracketright',
      Backslash: 'backslash', Semicolon: 'semicolon', Quote: 'quote', Comma: 'comma',
      Period: 'period', Slash: 'slash', Backquote: 'backquote', Enter: 'return',
      Space: 'space', Tab: 'tab', Escape: 'escape',
      ArrowLeft: 'left', ArrowRight: 'right', ArrowUp: 'up', ArrowDown: 'down'
    };
    return m[code] || null;
  }

  var lastData = null;     // most recent bindings snapshot, for cancel re-render
  var recordingId = null;  // action id currently capturing a chord
  var recordingBtn = null;
  var pendingFocusId = null;

  function actionFor(data, id) {
    return data && data.actions && data.actions.find(function(a) { return a.id === id; });
  }

  function actionRow(id) {
    return Array.prototype.find.call(document.querySelectorAll('[data-action]'), function(row) {
      return row.dataset.action === id;
    });
  }

  function focusBinding(id) {
    var row = actionRow(id);
    var button = row && row.querySelector('[data-bind]');
    if (button) button.focus();
  }

  function setRecordingStatus(message) {
    document.getElementById('kb-record-status').textContent = message;
  }

  function cancelRecord() {
    if (recordingId) {
      var id = recordingId;
      ipc({ type: 'keybind_capture', on: false });
      recordingId = null;
      recordingBtn = null;
      setRecordingStatus('');
      if (lastData) replaceActionRow(id, lastData, true);
    }
  }

  function startRecord(id, btn) {
    if (recordingBtn && recordingBtn !== btn) recordingBtn.classList.remove('recording');
    recordingId = id;
    recordingBtn = btn;
    btn.classList.add('recording');
    btn.querySelector('.keys').innerHTML = '<span class="rec-hint">Press keys…</span>';
    var action = actionFor(lastData, id);
    if (action) setRecordingStatus('Recording shortcut for ' + action.label + '. Press keys, or Escape to cancel.');
    // Tell the host to stop firing global shortcuts so we can capture the chord.
    ipc({ type: 'keybind_capture', on: true });
  }

  function kbRow(a) {
    var keys = a.keys.map(function(k) { return '<kbd class="kbd">' + esc(k) + '</kbd>'; }).join('');
    var currentChord = a.keys.join('');
    var controlId = 'kb-bind-' + esc(a.id);
    var reset = a.is_default ? '' :
      '<button class="kb-reset" data-reset="' + esc(a.id) + '" title="Reset to default" aria-label="Reset ' + esc(a.label) + ' to default">↺</button>';
    return '<div class="row" data-action="' + esc(a.id) + '">' +
      '<label class="row-label" for="' + controlId + '">' + esc(a.label) + '</label>' +
      '<span class="kb-right">' + reset +
        '<button class="kb-bind" id="' + controlId + '" data-bind="' + esc(a.id) + '" aria-label="Change shortcut for ' + esc(a.label) + ', currently ' + esc(currentChord) + '"><span class="keys">' + keys + '</span></button>' +
      '</span></div>';
  }

  function updateKeybindingError(data) {
    var err = document.getElementById('kb-error');
    if (data.error) { err.textContent = data.error; err.classList.add('show'); }
    else { err.textContent = ''; err.classList.remove('show'); }
  }

  function replaceActionRow(id, data, restoreFocus) {
    var action = actionFor(data, id);
    var existing = actionRow(id);
    if (!action || !existing) return false;
    var holder = document.createElement('div');
    holder.innerHTML = kbRow(action);
    existing.replaceWith(holder.firstElementChild);
    if (restoreFocus) focusBinding(id);
    return true;
  }

  function render(data, restoreFocusId) {
    lastData = data;
    recordingId = null;
    recordingBtn = null;
    var groups = {}, order = [];
    data.actions.forEach(function(a) {
      if (!groups[a.group]) { groups[a.group] = []; order.push(a.group); }
      groups[a.group].push(a);
    });
    document.getElementById('kb-groups').innerHTML = order.map(function(g) {
      return '<div class="section"><div class="section-title">' + esc(g) + '</div>' +
        groups[g].map(kbRow).join('') + '</div>';
    }).join('');
    updateKeybindingError(data);
    if (restoreFocusId) focusBinding(restoreFocusId);
  }

  // Rust pushes the full binding set on open and after every edit.
  window.__setKeybindings = function(data) {
    if (!data || !data.actions) return;
    var focused = document.activeElement.closest && document.activeElement.closest('[data-action]');
    var restoreFocusId = pendingFocusId || (focused && focused.dataset.action);
    var previous = lastData;
    lastData = data;
    recordingId = null;
    recordingBtn = null;

    var changed = previous ? data.actions.filter(function(action) {
      var old = actionFor(previous, action.id);
      return !old || JSON.stringify(old) !== JSON.stringify(action);
    }).map(function(action) { return action.id; }) : [];

    if (!previous) {
      render(data, restoreFocusId);
    } else if (pendingFocusId) {
      if (!replaceActionRow(pendingFocusId, data, true)) render(data, pendingFocusId);
      updateKeybindingError(data);
    } else if (changed.length === 1) {
      if (!replaceActionRow(changed[0], data, restoreFocusId === changed[0])) render(data, restoreFocusId);
      updateKeybindingError(data);
    } else if (changed.length > 1) {
      render(data, restoreFocusId);
    } else {
      updateKeybindingError(data);
      if (restoreFocusId) focusBinding(restoreFocusId);
    }
    pendingFocusId = null;

    var inlineEdit = actionFor(data, 'inline_edit');
    if (inlineEdit) document.getElementById('inline-edit-key').textContent = inlineEdit.keys.join('');
    var followLink = actionFor(data, 'follow_link');
    if (followLink) document.getElementById('follow-link-key').textContent = followLink.keys.join('');
  };

  // Delegate clicks: start recording on a binding, reset on the ↺ button.
  document.getElementById('kb-groups').addEventListener('click', function(e) {
    var reset = e.target.closest('[data-reset]');
    if (reset) {
      cancelRecord();
      pendingFocusId = reset.dataset.reset;
      ipc({ type: 'keybind_reset', action: pendingFocusId });
      return;
    }
    var bind = e.target.closest('[data-bind]');
    if (bind) {
      var id = bind.dataset.bind;
      if (recordingId) cancelRecord(); // restores the previously recording row
      var fresh = document.querySelector('.kb-bind[data-bind="' + id + '"]');
      startRecord(id, fresh || bind);
    }
  });

  document.getElementById('kb-reset-all').addEventListener('click', function() {
    cancelRecord();
    ipc({ type: 'keybind_reset_all' });
  });

  // Capture phase so a recording keystroke is intercepted before the modal's
  // Esc-to-close handler (which lives on the bubble phase) ever sees it.
  document.addEventListener('keydown', function(e) {
    if (!recordingId) return;
    e.preventDefault();
    e.stopPropagation();
    // Wait through bare modifier presses for the real key.
    if (['Meta', 'Shift', 'Control', 'Alt'].indexOf(e.key) >= 0) return;
    // Esc with no modifiers cancels the capture.
    if (e.code === 'Escape' && !e.metaKey && !e.ctrlKey && !e.altKey && !e.shiftKey) {
      cancelRecord();
      return;
    }
    var token = codeToToken(e.code);
    if (!token) return; // unsupported physical key — keep waiting
    var mods = [];
    if (e.metaKey) mods.push('cmd');
    if (e.ctrlKey) mods.push('ctrl');
    if (e.altKey) mods.push('opt');
    if (e.shiftKey) mods.push('shift');
    var id = recordingId;
    recordingId = null;
    recordingBtn = null;
    setRecordingStatus('');
    pendingFocusId = id;
    ipc({ type: 'keybind_record', action: id, chord: mods.concat([token]).join('+') });
    ipc({ type: 'keybind_capture', on: false });
  }, true);
})();
</script>
</body>
</html>"#
    .replace("/*@@THEME@@*/", crate::theme::CSS)
    .replace(
        "/*@@GOOGLE_BUILT_IN@@*/false",
        if crate::mail_oauth::bundled_google().is_some() {
            "true"
        } else {
            "false"
        },
    )
}
