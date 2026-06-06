# Octoweb Developer Guide

Lightweight macOS browser built on wry/tao. Performance-first, minimal footprint. AI assistant via ACP (Agent Client Protocol). MCP server for external AI tool control.

## Architecture

```
main.rs                      ← App entry, event loop, WebView pool, keyboard (CGEventTap)
   │
   ├── browser.rs            ← TabManager (tabs, history, active state, visit counts)
   ├── config.rs             ← Config, session persistence, favicon cache, prompt history
   ├── acp.rs                ← ACP integration (sidebar AI + background learning agent)
   ├── mcp.rs                ← MCP server — 20 browser control tools over HTTP JSON-RPC
   │
   ├── url.rs                ← URL resolution (user input → navigable URL)
   ├── webview_utils.rs      ← Injected JS scripts, favicon cache lookup, overlay data
   ├── macos.rs              ← macOS-specific: dock icon, Edit menu, MRU list, external URLs
   ├── nav_error_patch.rs    ← WKWebView navigation error callbacks (ObjC runtime patch)
   ├── content_rules.rs      ← WKContentRuleList (ad/tracker blocking)
   ├── crash_report.rs       ← WebContent process termination tracking
   ├── cold_open.rs          ← First-run setup, URL scheme handler (kAEGetURL)
   ├── hibernation.rs        ← Session save/restore (tabs, URLs)
   ├── tab_stats.rs          ← Per-tab CPU/RSS stats from WebContent XPC process
   ├── quickslots.rs         ← Fast-access slot logic (⌘⇧1–0 save, ⌘1–0 open)
   ├── prompt_history_js.rs  ← Shared JS module: prompt history nav, Ctrl+R search, ghost text
   │
   ├── overlay_html.rs       ← Command palette (⌘⇧P) — fuzzy search tabs/history
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

### ACP — AI Assistant & Background Learning (`acp.rs`)

Two ACP connections run independently:

1. **Sidebar assistant** (`octoweb:assistant`) — interactive chat, user-initiated
2. **Background learner** (`octoweb:learning`) — periodic, silent, memorizes browsing patterns

**Architecture:**
- Each `AcpHandle` spawns a dedicated OS thread with its own `tokio::runtime` + `LocalSet`
- Subprocess: `octomind acp <agent_tag>` with piped stdin/stdout
- Communication: `mpsc` channels (events → main thread, prompts → agent thread)
- Main thread polls via `AcpHandle::poll()` on every event loop tick
- Process killed on drop (`kill_on_drop(true)`)

**Idle timeout:** Resets on every `session_notification` — only fires after 5 min of silence (not 5 min total). Safe for long tool chains.

**Reconnection:** Exponential backoff (1s→2s→4s→8s→16s→30s), max 5 retries. Generation counter invalidates stale timers.

### MCP — Browser Control Server (`mcp.rs`)

HTTP JSON-RPC on `localhost:3434/mcp`. External AI agents control the browser via 20 tools:

| Category | Tools |
|----------|-------|
| Navigation | `browser_navigate`, `browser_go_back`, `browser_go_forward`, `browser_reload`, `browser_wait` |
| Tab management | `browser_get_tabs`, `browser_get_current_tab`, `browser_switch_tab`, `browser_close_tab` |
| Page content | `browser_get_page_info`, `browser_get_page_content`, `browser_execute_js`, `browser_screenshot` |
| Interaction | `browser_click`, `browser_type`, `browser_scroll`, `browser_press_key`, `browser_select_option` |
| History & media | `browser_get_history`, `browser_get_playing_tabs` |

**Architecture:**
- `McpServer` tool handlers → `McpCommand` enum → `mpsc` channel → main event loop
- Main loop drains commands synchronously (WebView is not thread-safe)
- Responses via `oneshot` channels with 30s timeout
- `send_command<T>()` helper eliminates boilerplate across all 20 handlers

**Adding a new MCP tool:**
1. Add `McpCommand` variant with `response: oneshot::Sender<Result<T, String>>`
2. Add request type with `#[derive(Deserialize, schemars::JsonSchema)]` + `#[schemars(description)]` on each field
3. Add `#[tool(description = "...")]` method in `#[tool_router] impl McpServer` — use `send_command()`
4. Handle the command in `main.rs` MCP drain loop

### Keyboard Shortcuts (CGEventTap in `main.rs`)

Global shortcuts intercepted at the system level via `CGEventTap`:
- **⌘ combos**: K (overlay), ⇧A (sidebar), ⇧E (inline edit), ⇧I (devtools), R (reload), W (close tab), etc.
- **Ctrl combos**: P/N (prev/next tab), D/U (scroll), T/B (top/bottom)
- Guards: overlay, inline edit, and sidebar visibility atomics prevent conflicts
- When sidebar is visible, Ctrl+P/N/R/E/U pass through to sidebar JS (prompt history navigation)

### Proactive Learning (`main.rs` timer + `octoweb:learning` formula)

Background agent that periodically analyzes browsing activity and memorizes user patterns.

**Flow:**
1. Timer fires every `learning_interval_min` (default 30 min)
2. Extracts active tab's `innerText` via async JS callback → `LearningReady(text)` event
3. Builds token-budgeted prompt (~16K chars): open tabs + recent history + active page text
4. Spawns `octomind acp octoweb:learning` ACP handle
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
| ACP (AI assistant) | `acp.rs` → `AcpHandle`, `AgentEvent`, `BrowserClient` |
| MCP (AI tool server) | `mcp.rs` → `McpServer`, `McpCommand`, `send_command()` |
| URL resolution | `url.rs` → `resolve_url()`, `is_external_scheme()` |
| Injected JS scripts | `webview_utils.rs` → `FAVICON_FETCH_SCRIPT`, `MEDIA_TRACK_SCRIPT` |
| Overlay data builder | `webview_utils.rs` → `build_items_json()` |
| macOS dock icon | `macos.rs` → `set_app_icon()` |
| macOS Edit menu | `macos.rs` → `setup_edit_menu()` |
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

**ACP lifecycle:**
- `AcpHandle::connect(cmd, wake)` → spawns subprocess, returns handle
- `send_prompt(text, images)` → queues prompt, returns false if channel dead
- `poll()` → drains pending `AgentEvent`s (non-blocking)
- Handle dropped → subprocess killed

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
| ACP not connecting | `acp.rs` → `connect()`, `init_session()` |
| ACP timeout on long sessions | `acp.rs` → idle timeout loop (resets on `activity.notify_one()`) |
| MCP tools not working | `mcp.rs` → tool methods, `main.rs` → `McpCommand` drain loop |
| URL not resolving | `url.rs` → `resolve_url()` |
| Keyboard not working | `main.rs` → CGEventTap block, check hotkey guard atomics |
| Ctrl+P/N in sidebar | `main.rs` → `sidebar_hotkey_visible` atomic must be true |
| Learning not running | Check `proactive_learning` config, `RUST_LOG=info` for "starting proactive learning" |
| Nav error (blank page) | `nav_error_patch.rs` → `inject_from_webview()` |
| WebView crash | `crash_report.rs` → `WebContentTerminated` event |
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
| `macos.rs` | Dock icon, Edit menu, MRU list | Window management |
| `config.rs` | Disk persistence (config, session, favicons, prompt history) | In-memory caches |
| `browser.rs` | Tab state, history, visit counts | WebView instances |
| `mcp.rs` | MCP protocol, tool definitions, `send_command` helper | Command execution (in main.rs) |
| `acp.rs` | ACP protocol, `AcpHandle`, `BrowserClient`, idle timeout | UI updates (in main.rs) |
| `prompt_history_js.rs` | Shared JS factory for prompt history | Per-component wiring |
| `nav_error_patch.rs` | ObjC runtime patching | Error page rendering |
| `quickslots.rs` | Slot storage, persistence | UI rendering |
| `content_rules.rs` | WKContentRuleList JSON | Rule application |
