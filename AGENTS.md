# Octoweb Developer Guide

Lightweight macOS browser built on wry/tao. Performance-first, minimal footprint. AI assistant via one shared `octomind server` (WebSocket). MCP server for external AI tool control.

## Architecture

```
main.rs                      ← App entry, event loop, WebView pool, keyboard (CGEventTap)
   │
   ├── browser.rs            ← TabManager (tabs, history, active state, visit counts)
   ├── config.rs             ← Config, session persistence, favicon cache, prompt history
   ├── agent.rs              ← Assistant sessions over the octomind WebSocket (sidebar, learning, editor, mail)
   ├── octomind_server.rs    ← The single `octomind server` process: lazy start, token, restart, shutdown
   ├── mcp.rs                ← MCP server — 42 browser, mail and calendar tools over HTTP JSON-RPC
   ├── agent_tabs.rs         ← MCP-driven tabs: kept rendering off-screen, exempt from hibernation
   │
   ├── calendar.rs           ← Calendar engine (specs/002-calendar): store, sync thread, next meeting, availability
   ├── calendar_google.rs    ← Google Calendar API v3 (Gmail accounts)
   ├── calendar_graph.rs     ← Microsoft Graph calendars (Outlook / 365)
   ├── calendar_dav.rs       ← CalDAV + iCalendar + RRULE expansion (iCloud, Private Email, any server)
   ├── calendar_quick.rs     ← Quick add: plain English → event draft
   ├── calendar_time.rs      ← Civil dates, local time, IANA zones via NSTimeZone
   ├── calendar_html.rs      ← Calendar pane tab (⌘⇧D): next meeting, list/day/week, details, form
   ├── http.rs               ← HTTPS via /usr/bin/curl with the request on stdin (no secrets in argv)
   │
   ├── url.rs                ← URL resolution (user input → navigable URL)
   ├── webview_utils.rs      ← Injected JS scripts, favicon cache lookup, overlay data
   ├── macos.rs              ← macOS-specific: dock icon, app/Edit menu, MRU list, external URLs
   ├── updates.rs            ← Sparkle auto-updates: framework loaded at runtime, delegate, settings (docs/auto-updates.md)
   ├── nav_error_patch.rs    ← WKWebView navigation error callbacks (ObjC runtime patch)
   ├── site_permissions.rs   ← Camera/mic/location: per-origin decisions, held prompts, live capture pause/stop
   ├── geolocation.rs        ← CoreLocation → WebKit geolocation provider (WKWebView on macOS has none)
   ├── content_rules.rs      ← WKContentRuleList (ad/tracker blocking)
   ├── crash_report.rs       ← WebContent process termination tracking
   ├── cold_open.rs          ← First-run setup, URL scheme handler (kAEGetURL)
   ├── hibernation.rs        ← Session save/restore (tabs, URLs)
   ├── tab_stats.rs          ← Per-tab CPU/RSS stats from WebContent XPC process
   ├── quickslots.rs         ← Fast-access slot logic (⌘⇧1–0 save, ⌘1–0 open)
   ├── prompt_history_js.rs  ← Shared JS module: prompt history nav, Ctrl+R search, ghost text
   │
   ├── page_index.rs         ← Full-text index of visited page text (palette `/query`, MCP search tool)
   ├── later.rs              ← Save-for-later queue (⌘⇧L) — decays after later_days, per workspace
   ├── markdown_js.rs        ← Page → markdown walker for ⌥⌘C
   ├── overlay_html.rs       ← Command palette (⌘⇧P) — fuzzy search tabs/history, `/` searches page text
   ├── sidebar_html.rs       ← AI assistant sidebar (⌘⇧A) — chat UI
   ├── address_bar_html.rs   ← Address bar with URL display + AI button
   ├── inline_edit_html.rs   ← Inline AI edit modal (⌘⇧E) — text transformation
   ├── settings_html.rs      ← Settings modal (⌘,)
   ├── shortcuts_html.rs     ← Keyboard shortcuts overlay (⌘/)
   ├── newtab_html.rs        ← New tab page (quick slots, search)
   ├── find_bar_html.rs      ← Find-in-page bar (⌘F)
   ├── quickslots_html.rs    ← Quick slots footer bar UI
   ├── notification_html.rs  ← Toast notification UI
   ├── error_page_html.rs    ← Custom error page for load failures
   └── progress_bar_html.rs  ← Top-of-screen page load progress bar
```

## Key Subsystems

### Assistant sessions — one `octomind server` (`agent.rs`, `octomind_server.rs`)

Every assistant runs as a session on ONE `octomind server` process, reached over its WebSocket protocol:

1. **Sidebar chats** (`octoweb:assistant` or any tag) — resumable, one per sidebar tab, per workspace
2. **Background learner** (`octoweb:learning`), **inline edit** (`octoweb:editor`), **mail jobs**
   (`octoweb:mail-reader` / `octoweb:mail-writer`) — one-shot sessions, unloaded when done

**Server (`octomind_server.rs`):**
- Started lazily by the first session: `octomind server octoweb:assistant --port 0 --sandbox`, cwd
  `~/.local/share/octoweb` (loads `.env` there); the bound address is read from its stdout
- `OCTOMIND_SERVER_TOKEN` (per-run secret) authenticates every connection; `OCTOWEB_MCP_TOKEN` is the
  MCP agent-server token (see MCP below); `OCTOMIND_MEDIA_ROOT` holds pasted images
- Dead server → sessions error → main loop reconnect → `endpoint()` starts a new one; sessions resume from disk
- `shutdown()` on quit; a pid file reaps a server orphaned by a crash
- Debug: launch with `OCTOWEB_OCTOMIND_TOKEN=<secret>` to attach your own WS client to the app's server
  (`Authorization: Bearer <secret>`; address in the `octomind server ready url=…` log line)

**Session (`agent.rs`):**
- `AgentHandle::connect(AgentSpec, wake)` — one WS connection per session, on a shared 2-thread runtime
- Bind: `{type: session, session_id, cwd, role}` — `cwd` = `agent_workspace::prepare(workspace, role)`, so each
  workspace's assistant gets its own directory; `role` applies to a newly created session only
- New sessions get an octoweb-chosen id (`octoweb-<hex>`), reported via `AgentEvent::Connected` and persisted
- Frames → `AgentEvent`s; a turn ends on its `cost` frame (`Done`/`Cancelled`), a failed turn is `TurnError`
  (session stays usable), a dead connection is `Error` (reconnect)
- `/…` prompts run as `command` frames; `cancel()` sends a `cancel` frame; `close()` unloads the session
- A prompt refused because the session's own background turn holds it ("busy") is resent after that turn's `cost`

**Idle timeout:** 20 min without any frame while a turn is outstanding.

**Reconnection:** Exponential backoff (1s→2s→4s→8s→16s→30s), max 5 retries. Generation counter invalidates stale timers.

### MCP — Browser Control Server (`mcp.rs`)

HTTP JSON-RPC on `localhost:3434/mcp` (`OCTOWEB_MCP_PORT` overrides; `OCTOWEB_CONFIG_DIR` isolates the profile for side-by-side instances). External AI agents control the browser via 42 tools:

| Category | Tools |
|----------|-------|
| Navigation | `browser_navigate` (always background), `browser_go_back`, `browser_go_forward`, `browser_reload`, `browser_wait` |
| Tab management | `browser_get_tabs`, `browser_get_current_tab`, `browser_switch_tab`, `browser_close_tab` |
| Page content | `browser_snapshot`, `browser_get_page_info`, `browser_get_page_content`, `browser_execute_js`, `browser_screenshot` |
| Interaction | `browser_click`, `browser_hover`, `browser_type`, `browser_fill_form`, `browser_scroll`, `browser_press_key`, `browser_select_option`, `browser_dismiss_overlay` |
| Dialogs & uploads | `browser_handle_dialog`, `browser_upload_file` (arm before the triggering click) |
| Diagnostics | `browser_console_messages`, `browser_network_requests` |
| History & media | `browser_get_history`, `browser_search_history_content`, `browser_get_later`, `browser_get_playing_tabs` |
| Mail (workspace's accounts; never sends, archives or deletes) | `mail_list_inbox`, `mail_search`, `mail_read_thread`, `mail_create_draft`, `mail_show`, `mail_set_summaries`, `mail_add_tasks` |
| Calendar (workspace's accounts; never creates, edits or answers) | `calendar_list_events`, `calendar_get_event`, `calendar_find_free_time`, `calendar_draft_event` (opens a filled form for the user) |
| Sidebar UI | `render_ui` (A2UI surface; blocks until the user answers) |

Interactions run through an actionability harness (`dom_actions.rs`): resolve with retry, bounding-box stability, occlusion detection, pointer+mouse event sequences. Async scripts evaluate via `callAsyncJavaScript` (`async_eval.rs`) so Promises are actually awaited. End-to-end suite: `test_mcp.py` + `tests/fixtures/` (28 tests against a live instance).

**Architecture:**
- `McpServer` tool handlers → `McpCommand` enum → `mpsc` channel → main event loop
- Main loop drains commands synchronously (WebView is not thread-safe)
- Responses via `oneshot` channels with 30s timeout
- `send_command<T>()` helper eliminates boilerplate across all 20 handlers
- Scoping: the `X-Octoweb-Workspace` header token names the caller's workspace (and chat). The shared octomind
  server sends one agent-server token for all its sessions; `call_tool` then reads the calling octomind session from
  `_meta["io.modelcontextprotocol/clientCapabilities"].experimental.session.session_id` and resolves it through
  `route_agent_session` (set by `agent.rs` before binding) to that session's own token

**Adding a new MCP tool:**
1. Add `McpCommand` variant with `response: oneshot::Sender<Result<T, String>>`
2. Add request type with `#[derive(Deserialize, schemars::JsonSchema)]` + `#[schemars(description)]` on each field
3. Add `#[tool(description = "...")]` method in `#[tool_router] impl McpServer` — use `send_command()`
4. Handle the command in `main.rs` MCP drain loop

### Keyboard Shortcuts (CGEventTap in `main.rs`)

Global shortcuts intercepted at the system level via `CGEventTap`:
- **⌘ combos**: K (overlay), ⇧A (sidebar), ⇧E (inline edit), ⇧I (mail), ⌥I (devtools), R (reload), W (close tab), etc.
- **Ctrl combos**: P/N (prev/next tab), D/U (scroll), T/B (top/bottom)
- Guards: overlay, inline edit, and sidebar visibility atomics prevent conflicts
- When sidebar is visible, Ctrl+P/N/R/E/U pass through to sidebar JS (prompt history navigation)

### Proactive Learning (`main.rs` timer + `octoweb:learning` formula)

Background agent that periodically analyzes browsing activity and memorizes user patterns.

**Flow:**
1. Timer fires every `learning_interval_min` (default 30 min)
2. Extracts active tab's `innerText` via async JS callback → `LearningReady(text)` event
3. Builds token-budgeted prompt (~16K chars): open tabs + recent history + active page text
4. Starts a one-shot `octoweb:learning` session on the shared server
5. Agent calls `remember` (check existing), then `memorize` (new insights) or skips
6. Handle dropped on Done/Error, waits for next interval

**Config:** `proactive_learning: bool` (default true), `learning_interval_min: u64` (default 30)

### Prompt History (`prompt_history_js.rs`)

Shared JS module used by both the inline edit modal and the AI sidebar:
- `createPromptHistory(inputEl, ghostEl, placeholder, onResize)` — factory function
- **Ctrl+P/N**: Navigate older/newer prompts (MRU order)
- **Ctrl+R**: Reverse incremental search through history
- **Ctrl+E**: Accept ghost text autocomplete
- **Ctrl+U**: Clear input to cursor start

History stored per-context: `prompt_history.json` (editor), `ai_prompt_history.json` (sidebar).

## Where to Look

| Area | Entry point |
|------|-------------|
| App entry & event loop | `main.rs` → `main()` |
| Tab management | `browser.rs` → `TabManager` |
| History & visit tracking | `browser.rs` → `history()`, `update_url()`, `visit_count()` |
| Config & persistence | `config.rs` → `Config::load()`, `save()` |
| Session save/restore | `config.rs` → `save_session()`, `load_session()` |
| Prompt history (editor) | `config.rs` → `save_prompt_history()`, `load_prompt_history()` |
| Prompt history (sidebar) | `config.rs` → `save_ai_prompt_history()`, `load_ai_prompt_history()` |
| Favicon cache | `config.rs` → `save_favicons()`, `load_favicons()` |
| Assistant sessions | `agent.rs` → `AgentHandle`, `AgentSpec`, `AgentEvent` |
| octomind server process | `octomind_server.rs` → `endpoint()`, `shutdown()` |
| MCP (AI tool server) | `mcp.rs` → `McpServer`, `McpCommand`, `send_command()` |
| Calendar sync & store | `calendar.rs` → `Engine`, `source()`, `next_meeting()`, `free_slots()`; providers in `calendar_{google,graph,dav}.rs` |
| Calendar pane | `calendar_html.rs` (`window.__cal`), `main.rs` → `mount_calendar!`, `AppEvent::Calendar`, meeting clock (`meeting_check_at`) |
| Quick add parsing | `calendar_quick.rs` → `parse()` |
| URL resolution | `url.rs` → `resolve_url()`, `is_external_scheme()` |
| Injected JS scripts | `webview_utils.rs` → `FAVICON_FETCH_SCRIPT`, `MEDIA_TRACK_SCRIPT` |
| Overlay data builder | `webview_utils.rs` → `build_items_json()` |
| macOS dock icon | `macos.rs` → `set_app_icon()` |
| macOS menu bar (app + Edit) | `macos.rs` → `setup_edit_menu()` |
| Auto-updates (Sparkle) | `updates.rs` → `start()`, `menu_item()`, `settings_script()`; release: `release.sh`, `docs/auto-updates.md` |
| MRU tab ordering | `macos.rs` → `mru_push()` |
| Nav error handling | `nav_error_patch.rs` → `register()`, `inject_from_webview()` |
| Keyboard shortcuts | `main.rs` → CGEventTap block (`K_KEYCODE`, etc.) |
| Quick slots | `quickslots.rs` → `load()`, `save()`, `QuickSlot` |
| Content blocking | `content_rules.rs` → rule list injection |
| Crash reports | `crash_report.rs` → `log_exit_trigger()`, `log_clean_shutdown()` |
| Cold open / URL scheme | `cold_open.rs` → `install_early()`, `install()` |
| Tab CPU/RSS stats | `tab_stats.rs` → `TabStatsSample` |

## Config Fields (`config.rs` → `Config`)

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `home_page` | `String` | `https://www.google.com` | URL loaded on startup |
| `search_engine` | `String` | `https://www.google.com/search?q={}` | Search URL (`{}` = query) |
| `max_history` | `usize` | `1000` | Max browsing history entries |
| `window_width` | `u32` | `1280` | Initial window width |
| `window_height` | `u32` | `800` | Initial window height |
| `ai_edit_auto_hide` | `bool` | `false` | Auto-hide inline edit modal after submit |
| `max_prompt_history` | `usize` | `50` | Editor prompt history size |
| `max_ai_prompt_history` | `usize` | `50` | Sidebar prompt history size |
| `proactive_learning` | `bool` | `true` | Enable background browsing pattern learning |
| `learning_interval_min` | `u64` | `30` | Minutes between learning runs |
| `calendar_alert_min` | `i32` | `1` | Minutes before a meeting its alert shows; `-1` off |
| `calendar_hidden` | `Vec<String>` | `[]` | Calendar keys left out of the pane and alerts |
| `calendar_default` | `String` | `""` | Calendar key new events go to |
| `calendar_day_start` / `calendar_day_end` | `u32` | `540` / `1080` | Working hours (minutes) for shared availability |

Per mail account: `calendar` (bool, default true) and `caldav_url` (CalDAV server override).

All fields use `#[serde(default)]` for backward compatibility — new fields are automatically filled with defaults when loading old config files.

## Code Quality Rules

**Build & lint — run after every change:**
```bash
cargo check                              # fast syntax check
cargo clippy -- -D warnings              # must pass clean
cargo build --release                    # for actual testing
```

**Logging — use `tracing`, never `println!`/`eprintln!`:**
```rust
tracing::debug!(url = %url, tab_id, "navigating to URL");
tracing::warn!(error = %e, "config parse failed, using defaults");
```

**Tracing levels:**
- `error!` — something broke, needs attention
- `warn!` — degraded but recoverable
- `info!` — significant lifecycle events (app start, MCP server bound, learning run)
- `debug!` — operational detail (navigation, tab switch, MCP commands)
- `trace!` — verbose internals (IPC messages, JS injection)

Run with `RUST_LOG=debug cargo run` to see debug output. Default is `warn`.

## Adding a New Feature

1. **State** → Add to `TabManager` or `Config` depending on persistence needs
2. **UI event** → Add to `AppEvent` enum in `main.rs`
3. **Handler** → Match in main event loop, update state, call WebView scripts
4. **Persistence** → If needed, add to `Config::save()` / `load()`
5. **Settings UI** → Add row to `settings_html.rs` AI/General section
6. **Settings handler** → Add match arm in `UpdateConfig` handler in `main.rs`

**Where to put new code:**
- URL/input parsing → `url.rs`
- Injected JS scripts or favicon/overlay helpers → `webview_utils.rs`
- macOS-specific (menus, dock, system APIs) → `macos.rs`
- MCP tool for AI control → `mcp.rs` (add `McpCommand` + `#[tool]` method)
- New HTML component → new `*_html.rs` file (pattern: `pub fn html() -> &'static str` or `-> String`)
- Shared JS between HTML modules → `prompt_history_js.rs` pattern (const fn returning `&'static str`)
- Keep `main.rs` for event loop wiring only — extract logic into modules

## Key Patterns

**Tab lifecycle:**
- `TabManager::open()` → returns new tab ID
- `TabManager::close()` → removes tab, caller removes WebView from HashMap
- `TabManager::switch()` → changes active tab
- WebView created/destroyed in `main.rs`, NOT in `TabManager`

**History:**
- Pushed on every URL change via `update_url()`
- Title backfilled via `update_title()` after page load
- Bounded by `max_history` (set at TabManager creation)

**Agent session lifecycle:**
- `AgentHandle::connect(AgentSpec::chat(..) | AgentSpec::one_shot(..), wake)` → binds the session, returns handle
- `send_prompt(text, images)` → queues prompt, returns false if the session task exited
- `poll()` → drains pending `AgentEvent`s (non-blocking)
- Handle dropped → disconnects (chat stays loaded for a resume; one-shot is unloaded); `close()` → unloads

**MCP tool pattern:**
```rust
// In mcp.rs — tool handler (5 lines with send_command helper):
#[tool(description = "...")]
async fn browser_X(&self, Parameters(req): Parameters<XRequest>) -> Result<CallToolResult, McpError> {
    let result = self.send_command(|tx| McpCommand::X { ..., response: tx }).await?;
    Ok(CallToolResult::success(vec![Content::text(...)]))
}

// In main.rs — command handler:
McpCommand::X { ..., response } => {
    // Execute on WebView (main thread only)
    let _ = response.send(Ok(...));
}
```

**HTML modules return types:**
- `-> &'static str` for pure static HTML (progress bar, error page)
- `-> String` when embedding shared JS via `.replace("/* PLACEHOLDER */", js_code)` (sidebar, inline edit)

**CGEventTap guard atomics:**
- `overlay_hotkey_visible`, `inline_edit_hotkey_visible`, `sidebar_hotkey_visible`
- All Ctrl+key shortcuts check these to avoid intercepting keys meant for focused UI components

## Debugging Starting Points

Run with `RUST_LOG=debug cargo run` to see all debug-level tracing output.

| Problem | Where to start |
|---------|----------------|
| Tab not switching | `main.rs` → `AppEvent::SwitchTab` handler |
| History not saving | `browser.rs` → `update_url()`, `config.rs` → `save_session()` |
| Favicon not showing | `webview_utils.rs` → `FAVICON_FETCH_SCRIPT`, `cached_favicon()` |
| Assistant not connecting | `octomind_server.rs` → `start()` (startup stderr is in the error), `agent.rs` → `run()` / `bind()` |
| Calendar not syncing | Pane → Calendars menu shows each account's state; `RUST_LOG=octoweb::calendar=debug`; CalDAV e2e: `OCTOWEB_DAV_TEST=<url> cargo test --bin octoweb dav_round_trip -- --ignored` |
| Meeting alert / ⌘J not firing | `main.rs` → meeting clock (`meeting_check_at`), `calendar::next_meeting()`, `meeting_joinable` atomic |
| Assistant turn never ends | `agent.rs` → `on_cost()` / `on_error()` routing; `RUST_LOG=octoweb=debug` |
| Tool call acts on wrong workspace | `mcp.rs` → `call_tool()` agent-server token + `_meta` session routing (`route_agent_session`) |
| MCP tools not working | `mcp.rs` → tool methods, `main.rs` → `McpCommand` drain loop |
| URL not resolving | `url.rs` → `resolve_url()` |
| Keyboard not working | `main.rs` → CGEventTap block, check hotkey guard atomics |
| Ctrl+P/N in sidebar | `main.rs` → `sidebar_hotkey_visible` atomic must be true |
| Learning not running | Check `proactive_learning` config, `RUST_LOG=info` for "starting proactive learning" |
| Nav error (blank page) | `nav_error_patch.rs` → `inject_from_webview()` |
| WebView crash | `crash_report.rs` → `WebContentTerminated` event |
| Updates off / not offered | `RUST_LOG=octoweb::updates=debug`; `docs/auto-updates.md` → Troubleshooting |
| No log output | Set `RUST_LOG=debug` or `RUST_LOG=trace` env var |

## Module Organization Rules

**`main.rs` is the event loop — not a dumping ground.**
- Only event matching, WebView wiring, and window management belong here
- Extract reusable logic into the appropriate module

**Keep modules focused:**
| Module | Owns | Does NOT own |
|--------|------|-------------|
| `url.rs` | URL parsing, scheme detection, search URL | Navigation logic |
| `webview_utils.rs` | Injected JS, favicon lookup, overlay data | WebView creation |
| `macos.rs` | Dock icon, menu bar, MRU list | Window management |
| `updates.rs` | Sparkle loading, updater delegate, update settings | Release publishing (`release.sh`) |
| `config.rs` | Disk persistence (config, session, favicons, prompt history) | In-memory caches |
| `browser.rs` | Tab state, history, visit counts | WebView instances |
| `mcp.rs` | MCP protocol, tool definitions, `send_command` helper | Command execution (in main.rs) |
| `agent.rs` | Session protocol, `AgentHandle`, turn routing, idle timeout | UI updates (in main.rs) |
| `octomind_server.rs` | The server process: start, token, restart, shutdown | Session state |
| `prompt_history_js.rs` | Shared JS factory for prompt history | Per-component wiring |
| `nav_error_patch.rs` | ObjC runtime patching | Error page rendering |
| `quickslots.rs` | Slot storage, persistence | UI rendering |
| `content_rules.rs` | WKContentRuleList JSON | Rule application |
| `calendar.rs` | Calendar store, sync thread, provider dispatch, availability | Pane UI, alerts (in main.rs) |
| `http.rs` | curl-backed HTTPS with secrets on stdin | Provider semantics |
