# Octoweb Developer Guide

Lightweight macOS browser built on wry/tao. Performance-first, minimal footprint. AI assistant via ACP (Agent Client Protocol).

## Architecture

```
main.rs                  ← App entry, event loop, WebView pool
   │
   ├── browser.rs        ← TabManager (tabs, history, active state)
   ├── config.rs         ← Config, session persistence, favicon cache
   ├── acp.rs            ← ACP integration (AI assistant connection)
   ├── mcp.rs            ← MCP server (AI tool control over HTTP JSON-RPC)
   │
   ├── url.rs            ← URL resolution (user input → navigable URL)
   ├── webview_utils.rs  ← Injected JS scripts, favicon cache lookup, overlay data
   ├── macos.rs          ← macOS-specific: dock icon, Edit menu, MRU list
   ├── nav_error_patch.rs← WKWebView navigation error callbacks (ObjC runtime patch)
   │
   ├── overlay_html.rs   ← Command palette (spotlight-style) HTML/JS/CSS
   ├── sidebar_html.rs   ← AI assistant sidebar HTML/JS/CSS
   ├── error_page_html.rs← Custom error page for load failures
   ├── progress_bar_html.rs ← Top-of-screen progress bar
   └── toggle_btn_html.rs← Floating AI toggle button
```

**Single source of truth:** `Config` in `config.rs`. User preferences, search engine, window dimensions. Session restored from `~/Library/Application Support/octoweb/`.

## Where to Look

| Area | Entry point |
|------|-------------|
| App entry & event loop | `src/main.rs` → `main()` |
| Tab management | `src/browser.rs` → `TabManager` |
| History & visit tracking | `src/browser.rs` → `history`, `visit_count()` |
| Config & persistence | `src/config.rs` → `Config::load()`, `save()` |
| Session save/restore | `src/config.rs` → `save_session()`, `load_session()` |
| Favicon cache (disk) | `src/config.rs` → `save_favicons()`, `load_favicons()` |
| Favicon cache (lookup) | `src/webview_utils.rs` → `cached_favicon()` |
| ACP (AI assistant) | `src/acp.rs` → `AcpHandle`, `AgentEvent` |
| MCP (AI tool server) | `src/mcp.rs` → `McpServer`, `McpCommand` |
| URL resolution | `src/url.rs` → `resolve_url()` |
| Injected JS scripts | `src/webview_utils.rs` → `FAVICON_FETCH_SCRIPT`, `MEDIA_TRACK_SCRIPT` |
| Overlay data builder | `src/webview_utils.rs` → `build_items_json()` |
| macOS dock icon | `src/macos.rs` → `set_app_icon()` |
| macOS Edit menu | `src/macos.rs` → `setup_edit_menu()` |
| MRU tab ordering | `src/macos.rs` → `mru_push()` |
| Nav error handling | `src/nav_error_patch.rs` → `register()`, `inject_from_webview()` |
| Keyboard shortcuts | `src/main.rs` → `K_KEYCODE`, `W_KEYCODE`, etc. |
| Command palette UI | `src/overlay_html.rs` |
| AI sidebar UI | `src/sidebar_html.rs` |
| Error page UI | `src/error_page_html.rs` → `html()` |
| Progress bar UI | `src/progress_bar_html.rs` → `html()` |
| AI toggle button UI | `src/toggle_btn_html.rs` → `html()` |

## Code Quality Rules

**Build & lint — run after every change:**
```bash
cargo check                              # fast syntax check
cargo clippy -- -D warnings              # must pass clean
cargo build --release                    # for actual testing
```

**Errors — fail fast, never hide:**
```rust
// ✅ expose problems immediately
let cfg = Config::load();

// ❌ never silently fall back
let cfg = Config::load().unwrap_or_default();  // hides config errors
```

**Logging — use `tracing`, never `println!`/`eprintln!`:**
```rust
// ✅ structured tracing — controlled by RUST_LOG env (default: warn)
tracing::debug!(url = %url, tab_id, "navigating to URL");
tracing::warn!(error = %e, "config parse failed, using defaults");
tracing::error!(path = %path.display(), "failed to write config");

// ❌ never use print macros
println!("DEBUG: ...");                    // not in release, not structured
eprintln!("error: ...");                   // bypasses log filtering
#[cfg(debug_assertions)] eprintln!(...);   // old pattern, replaced by tracing
```

**Tracing levels:**
- `error!` — something broke, needs attention
- `warn!` — degraded but recoverable (e.g. config parse fail → defaults)
- `info!` — significant lifecycle events (app start, MCP server bound)
- `debug!` — operational detail (navigation, tab switch, MCP commands)
- `trace!` — verbose internals (IPC messages, JS injection)

Run with `RUST_LOG=debug cargo run` to see debug output. Default is `warn`.

**WebView state — always check tab exists:**
```rust
// ✅ defensive
if let Some(tab) = tab_manager.active_tab() {
    // work with tab
}

// ❌ assumes tab exists
let tab = tab_manager.active_tab().unwrap();  // can panic
```

**Memory — WebView cleanup on tab close:**
```rust
// WebViews are stored separately in HashMap<usize, WebView>
// Always remove from map when closing:
if let Some(wv) = webviews.remove(&tab_id) {
    // WebView dropped automatically
}
```

## Adding a New Feature

1. **State** → Add to `TabManager` or `Config` depending on persistence needs
2. **UI event** → Add to `AppEvent` enum in `main.rs`
3. **Handler** → Match in main event loop, update state, call WebView scripts
4. **Persistence** → If needed, add to `Config::save()` / `load()`

**Where to put new code:**
- URL/input parsing → `url.rs`
- Injected JS scripts or favicon/overlay helpers → `webview_utils.rs`
- macOS-specific (menus, dock, system APIs) → `macos.rs`
- Navigation error handling → `nav_error_patch.rs`
- MCP tool for AI control → `mcp.rs` (add variant to `McpCommand`, implement tool method)
- New HTML component → new `*_html.rs` file (pattern: `pub fn html() -> &'static str`)
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

**Favicons:**
- Fetched via injected JS (`webview_utils::FAVICON_FETCH_SCRIPT`)
- Cached by domain in `HashMap<String, String>` (base64 data-URI)
- Lookup via `webview_utils::cached_favicon()`
- Persisted to `favicons.json` on app exit (`config::save_favicons()`)

**ACP (AI assistant):**
- Async runtime in separate tokio context
- Events polled via `AcpHandle::poll()` in main loop
- `AgentEvent::Connected` / `Message` / `Done` / `Error`

**MCP (AI tool server):**
- HTTP JSON-RPC on `localhost:3434/mcp`
- Commands sent via `mpsc` channel → drained in main event loop (WebView is not thread-safe)
- Each tool: `McpCommand` variant + handler in main loop + `#[tool]` method in `mcp.rs`
- Add new tool: add `McpCommand` variant → handle in main.rs drain loop → add `#[tool]` method

**Navigation error patch (`nav_error_patch.rs`):**
- Injects ObjC methods into wry's `WKNavigationDelegate` at runtime
- `register(ptr, callback)` per WebView, `unregister(ptr)` on close
- `inject_from_webview(ptr)` once after first WebView created

**Keyboard shortcuts** — global shortcuts live in the CGEventTap block in `src/main.rs`; palette shortcuts in `handleEditingHotkeys` in `src/overlay_html.rs`. When adding a shortcut, update both the code and `README.md` (user-facing shortcut list).

## Debugging Starting Points

Run with `RUST_LOG=debug cargo run` to see all debug-level tracing output.

| Problem | Where to start |
|---------|----------------|
| Tab not switching | `main.rs` → `AppEvent::SwitchTab` handler |
| History not saving | `browser.rs` → `update_url()`, `config.rs` → `save_session()` |
| Favicon not showing | `webview_utils.rs` → `FAVICON_FETCH_SCRIPT`, `cached_favicon()` |
| ACP not connecting | `acp.rs` → `connect()`, `init_session()` |
| MCP tools not working | `mcp.rs` → tool methods, `main.rs` → `McpCommand` drain loop |
| URL not resolving | `url.rs` → `resolve_url()` |
| Keyboard not working | `main.rs` → `K_KEYCODE` constants, event tap setup |
| Nav error (blank page) | `nav_error_patch.rs` → `inject_from_webview()`, `fire_error_callback()` |
| WebView crash | Check `wry` version compatibility, macOS version |
| No log output | Set `RUST_LOG=debug` or `RUST_LOG=trace` env var |

## Performance Notes

- **Single WebView pool** — WebViews reused, not recreated
- **Favicon cache** — base64 data-URIs, no network on load
- **History bounded** — `max_history` prevents unbounded growth
- **MRU bounded** — most-recently-used list capped at 64 entries
- **Release profile** — LTO, codegen-units=1, stripped binaries

## Module Organization Rules

**`main.rs` is the event loop — not a dumping ground.**
- Only event matching, WebView wiring, and window management belong here
- Extract reusable logic into the appropriate module (see architecture diagram)

**HTML modules (`*_html.rs`) follow a pattern:**
- `pub fn html() -> &'static str` for static pages (progress bar, toggle button)
- `pub fn html(args...) -> String` for dynamic pages (error page)
- Self-contained: HTML + CSS + JS in one Rust string literal
- No external dependencies — everything inlined

**Keep modules focused:**
| Module | Owns | Does NOT own |
|--------|------|-------------|
| `url.rs` | URL parsing, scheme detection, search URL | Navigation logic |
| `webview_utils.rs` | Injected JS, favicon lookup, overlay data | WebView creation |
| `macos.rs` | Dock icon, Edit menu, MRU list | Window management |
| `config.rs` | Disk persistence (config, session, favicons) | In-memory caches |
| `browser.rs` | Tab state, history, visit counts | WebView instances |
| `mcp.rs` | MCP protocol, tool definitions | Command execution |
| `nav_error_patch.rs` | ObjC runtime patching | Error page rendering |