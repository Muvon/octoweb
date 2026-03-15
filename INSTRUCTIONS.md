# Octoweb Developer Guide

Lightweight macOS browser built on wry/tao. Performance-first, minimal footprint. AI assistant via ACP (Agent Client Protocol).

## Architecture

```
main.rs
   │
   ├── browser.rs      ← TabManager (tabs, history, active state)
   ├── config.rs       ← Config, session persistence, favicon cache
   ├── acp.rs          ← ACP integration (AI assistant connection)
   │
   └── WebView pool    ← HashMap<usize, WebView> owned in main loop
```

**Single source of truth:** `Config` in `config.rs`. User preferences, search engine, window dimensions. Session restored from `~/Library/Application Support/octoweb/`.

## Where to Look

| Area | Entry point |
|------|-------------|
| App entry & event loop | `src/main.rs` → `main()` |
| Tab management | `src/browser.rs` → `TabManager` |
| History & visit tracking | `src/browser.rs` → `history`, `visit_count()` |
| Config & persistence | `src/config.rs` → `Config::load()`, `save()` |
| Favicon cache | `src/config.rs` → `save_favicons()`, `load_favicons()` |
| ACP (AI assistant) | `src/acp.rs` → `AcpHandle`, `AgentEvent` |
| URL resolution | `src/main.rs` → `resolve_url()`, `looks_like_*()` |
| Keyboard shortcuts | `src/main.rs` → `K_KEYCODE`, `W_KEYCODE`, etc. |
| Overlay UI | `src/overlay_html.rs` |
| Sidebar UI | `src/sidebar_html.rs` |

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

**Logging — no println in release:**
```rust
#[cfg(debug_assertions)]
println!("DEBUG: ...");   // ✅ debug-only

println!("DEBUG: ...");    // ❌ — visible in release, wrong output path
```

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
- Fetched via injected JS (`FAVICON_FETCH_SCRIPT`)
- Cached by domain in `HashMap<String, String>` (base64 data-URI)
- Persisted to `favicons.json` on app exit

**ACP (AI assistant):**
- Async runtime in separate tokio context
- Events polled via `AcpHandle::poll()` in main loop
- `AgentEvent::Connected` / `Message` / `Done` / `Error`

## Keyboard Shortcut Map

**This is the canonical list. Keep it in sync with `src/main.rs` (CGEventTap block) and `src/overlay_html.rs` (handleEditingHotkeys). Any new shortcut must be added here.**

### Global (CGEventTap — `src/main.rs`)

| Shortcut | Action | AppEvent |
|---|---|---|
| `⌘K` | Open command palette (spotlight-style) | `ToggleOverlay` |
| `⌘W` | Close current tab | `CloseTab(0)` |
| `⌘R` | Reload current page | `Reload` |
| `⌘Q` | Quit | `Quit` |
| `⌘⇧A` | Toggle AI assistant sidebar | `ToggleSidebar` |
| `⌘⇧I` | Toggle DevTools | `ToggleDevTools` |
| `⌃N` | Next tab | `NextTab` |
| `⌃P` | Previous tab | `PrevTab` |

> `⌃N` / `⌃P` and `⌘W` are suppressed while the overlay is open (guarded by `overlay_state`).

### Command Palette (`src/overlay_html.rs` — `handleEditingHotkeys`)

| Shortcut | Action |
|---|---|
| `↑` / `↓` | Move selection |
| `⌃N` / `⌃P` | Move selection (Emacs-style) |
| `Return` | Open / navigate / switch to tab |
| `Esc` | Close palette |
| `⌘W` | Close selected tab |
| `⌃A` | Cursor to start of input |
| `⌃E` | Cursor to end of input |
| `⌃K` | Delete from cursor to end of line |
| `⌃U` | Delete from cursor to start of line |
| `⌘V` | Paste from clipboard |
| `Home` / `End` | Cursor to start / end |

### AI Sidebar (`src/sidebar_html.rs`)

| Shortcut | Action |
|---|---|
| `Return` | Send prompt |
| `⇧Return` | Insert newline |

### Rule for developers

When adding or changing a shortcut:
1. Update the CGEventTap block in `src/main.rs` (global) **or** `handleEditingHotkeys` in `src/overlay_html.rs` (palette)
2. Update the table above in this file
3. Update the **Keyboard shortcuts** section in `README.md`

All three must stay in sync. The code is the source of truth; the tables are the human-readable mirror.

## Debugging Starting Points

| Problem | Where to start |
|---------|----------------|
| Tab not switching | `main.rs` → `AppEvent::SwitchTab` handler |
| History not saving | `browser.rs` → `update_url()`, `config.rs` → `save_session()` |
| Favicon not showing | `main.rs` → `FAVICON_FETCH_SCRIPT`, `config.rs` → `cached_favicon()` |
| ACP not connecting | `acp.rs` → `connect()`, `init_session()` |
| URL not resolving | `main.rs` → `resolve_url()`, `looks_like_*()` helpers |
| Keyboard not working | `main.rs` → `K_KEYCODE` constants, event tap setup |
| WebView crash | Check `wry` version compatibility, macOS version |

## Performance Notes

- **Single WebView pool** — WebViews reused, not recreated
- **Favicon cache** — base64 data-URIs, no network on load
- **History bounded** — `max_history` prevents unbounded growth
- **MRU bounded** — most-recently-used list capped at 64 entries
- **Release profile** — LTO, codegen-units=1, stripped binaries