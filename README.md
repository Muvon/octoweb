# Octoweb — the keyboard-first AI browser for macOS

[![Version](https://img.shields.io/badge/version-0.12.0-blue.svg)](https://github.com/muvon/octoweb/releases)
[![Platform](https://img.shields.io/badge/platform-macOS-black.svg)](https://github.com/muvon/octoweb/releases)
[![License](https://img.shields.io/badge/license-Apache%202.0-green.svg)](LICENSE)

**The browser you reach for when you want to think. macOS only, by design.**

Built on WebKit and Rust. No Electron. No Chrome. No mouse required.

🏠 [Product page](https://octomind.run/product/octoweb/) · [Changelog](CHANGELOG.md)

---

## Contents

- [Why Octoweb?](#why-octoweb)
- [Features](#features)
- [AI Assistant](#ai-assistant)
- [MCP server (AI browser control)](#mcp-server-ai-browser-control)
- [Keyboard shortcuts](#keyboard-shortcuts)
- [Install](#install)
- [Configuration](#configuration)
- [What it is (and isn't)](#what-it-is-and-isnt)
- [Tech stack](#tech-stack)

---

## Why Octoweb?

Most browsers are built around the mouse. Octoweb is built around the keyboard — and around the idea that your browser should amplify your thinking, not interrupt it. Every action has a shortcut. The AI assistant lives in a sidebar, not a tab. And your AI tools can drive the browser directly via MCP. No extensions. No config. Just open it and go.

**Three things it does differently:**

1. **Keyboard-first navigation** — Every action has a shortcut. Nothing requires a click. The command palette (`⌘⇧P`) fuzzy-searches tabs and history. Pin any page to a fast-access slot with `⌘⇧1`–`⌘⇧0` and jump back with `⌘1`–`⌘9` from anywhere.

2. **AI assistant built in** — Not an extension, not a tab. A sidebar overlay powered by a local [octomind](https://github.com/muvon/octomind) agent via [ACP](https://github.com/muvon/agent-client-protocol). Ask questions about the current page, get code explanations, summarize content — all without leaving the browser.

3. **MCP server inside the browser** — Your AI tools can actually *drive* the browser. Octoweb runs an MCP server on `localhost:3434/mcp` that exposes 29 tools for navigation, tab management, page interaction, and content extraction. Point Claude Desktop or any MCP client at it and watch it browse.

---

## Features

- **Command palette** (`⌘⇧P`) — Fuzzy search across tabs and history. Type a URL, search query, or page fragment.
- **Fast-access slots** (`⌘1`–`⌘0`) — Pin up to 10 pages for instant access. Footer bar shows all slots.
- **Tab pinning** (`⌘⇧N`) — Pin the current tab to the fast-access bar with one keystroke.
- **Workspaces** (`⌘⇧O`) — Isolated browser profiles: each workspace has its own tabs, history, AI sessions, and a dedicated WebKit data store, so cookies, localStorage, and cache never leak between them. Switch with `⌘1`–`⌘0` while the popover is open.
- **Configurable keybindings** — Remap any shortcut in Settings (`⌘,`). Changes apply on the next keystroke, no restart.
- **AI sidebar** (`⌘⇧A`) — Chat with a local AI agent about the current page. Streaming responses, code blocks with copy.
- **Inline AI edit** (`⌘⇧E`) — Select text on any page, transform it with AI. Rewrite, summarize, translate.
- **Proactive learning** — Background agent periodically analyzes your browsing and memorizes patterns. Enabled by default, configurable interval — disable in Settings (`⌘,`).
- **MCP server** — 29 tools for AI clients to control the browser. Navigate, click, type, screenshot, extract content.
- **Find-in-page** (`⌘F`) — Full-text search with highlighting.
- **Page zoom** — `+`/`-` to zoom, `⌘0` to reset.
- **Screenshots** — `⌘S` for viewport, `⌘⇧S` for full page. Copied to clipboard.
- **PDF & DOCX viewer** — Open documents directly in the browser.
- **Session restore** — Tabs and history persist across restarts.
- **Favicon caching** — No network requests on startup. Icons stored as base64.
- **Content blocking** — Built-in tracker and ad blocker via WKContentRuleList.
- **Smart tab hibernation** — Background tabs under memory pressure are frozen.

---

## AI Assistant

Press `⌘⇧A` to open the sidebar. It connects to a local AI agent running under [octomind](https://github.com/muvon/octomind) using the [Agent Client Protocol (ACP)](https://github.com/muvon/agent-client-protocol). Responses stream in as they arrive.

### How it works

```
octoweb sidebar  ──ACP/JSON-RPC──▶  octomind acp octoweb:assistant
                                         │
                                    specialist agent config
                                    from the tap registry
                                         │
                                    your chosen AI provider
                                    (OpenAI, Anthropic, etc.)
```

octomind is a plug-and-play AI agent runtime. You install it once, point it at an API key, and get a fully configured specialist agent — model, system prompt, tools, and all — with zero manual setup. The `octoweb:assistant` tag fetches that agent's configuration from the [community tap registry](https://github.com/muvon/octomind-tap) automatically.

### Setup

**1. Install octomind:**

```bash
curl -fsSL https://raw.githubusercontent.com/muvon/octomind/master/install.sh | bash
```

**2. Set an API key** (any supported provider):

```bash
export OPENROUTER_API_KEY="your_key"   # easiest — covers all providers
# or: OPENAI_API_KEY, ANTHROPIC_API_KEY, etc.
```

**3. Start octomind in ACP mode** (octoweb connects to this):

```bash
octomind acp octoweb:assistant
```

octomind fetches the `octoweb:assistant` agent manifest from the tap, installs any required tools, and starts listening for ACP connections. octoweb's sidebar connects automatically.

**4. Open the sidebar in octoweb:** `⌘⇧A`

### Changing the agent

The agent tag in the sidebar header defaults to `octoweb:assistant`. You can type any tag your octomind instance knows about — `developer:rust`, `assistant`, your own custom agents — and the sidebar reconnects to that agent immediately.

Shipped tags include `octoweb:assistant` (sidebar chat), `octoweb:editor` (inline text transformation, used by `⌘⇧E`), `octoweb:learning` (background learning), and `octoweb:trend` (live social trend research) — see the [tap registry](https://github.com/muvon/octomind-tap).

No data leaves your machine unless your agent sends it somewhere. The AI provider call is made by octomind, not by the browser.

### Agent-rendered UI (A2UI)

Agents can do more than reply with text: via the `render_ui` tool they can draw interactive surfaces — forms, buttons, live views — inline in the sidebar chat. Buttons can open URLs in a new tab or resolve back to the agent.

### Proactive Learning

Octoweb runs a background agent that periodically analyzes your browsing patterns and memorizes insights. This is enabled by default — turn it off in Settings (`⌘,`).

**How it works:**

1. Every `learning_interval_min` (default: 30), the background agent wakes up
2. It collects: open tabs, recent history, and the active page's text content
3. The agent calls `remember` to check existing memories, then `memorize` for new insights
4. Insights are stored locally in octomind's memory system

**Configuration:**

```toml
proactive_learning    = true    # enable background learning
learning_interval_min = 30     # minutes between runs (floored to 5 on load)
```

The learning agent runs as a separate `octomind acp octoweb:learning` process. It's completely independent from the sidebar assistant — you can use one without the other.

---

## MCP server (AI browser control)

octoweb runs an MCP server on `localhost:3434/mcp`. Any MCP-compatible AI client — Claude Desktop, octomind itself, your own scripts — can use it to control the browser directly.

```
Claude Desktop / octomind / any MCP client
        │
        │  HTTP JSON-RPC
        ▼
  localhost:3434/mcp  (inside octoweb)
        │
        ▼
  WebKit WebView — navigate, click, type, read, run JS
```

**Available tools:**

| Tool | What it does |
|------|--------------|
| `browser_navigate` | Navigate to a URL — always in the background (new tab, or in-place via `tab_id`); never steals focus |
| `browser_go_back` | Go back in history |
| `browser_go_forward` | Go forward in history |
| `browser_reload` | Reload current page |
| `browser_wait` | Wait for load / SPA readiness / a CSS selector to appear |
| `browser_get_tabs` | List all open tabs with IDs, titles, URLs |
| `browser_get_current_tab` | Get the tab the user is viewing |
| `browser_switch_tab` | Show a tab to the user (the only tool that changes focus) |
| `browser_close_tab` | Close a tab by ID |
| `browser_snapshot` | Map of interactive elements with `@N` refs — pierces iframes, shadow DOM, and listener-only clickables |
| `browser_get_page_info` | Get title, URL, meta description |
| `browser_get_page_content` | Get page text content (innerText) |
| `browser_execute_js` | Run JavaScript in the page (Promises are awaited) |
| `browser_click` | Click an element — auto-retries until present, stable, and unobstructed; reports what covers it |
| `browser_type` | Set an input/textarea/contenteditable value (React-safe) |
| `browser_fill_form` | Fill several fields — and optionally submit — in one call |
| `browser_dismiss_overlay` | Dismiss cookie/consent overlays — prefers Reject/Decline, never auto-accepts |
| `browser_hover` | Hover with full pointer + mouse event sequence |
| `browser_scroll` | Scroll the window, or an element's scrollable container via `selector` |
| `browser_press_key` | Press a key; Enter on a form input submits like a real keypress |
| `browser_select_option` | Select an option in a `<select>` by value or label |
| `browser_screenshot` | Take a screenshot (`full_page: true` for entire page) |
| `browser_console_messages` | Recent console output + JS errors of the page |
| `browser_network_requests` | Recent fetch/XHR activity with statuses and timings |
| `browser_handle_dialog` | Arm auto-answers for upcoming `alert`/`confirm`/`prompt` dialogs |
| `browser_upload_file` | Arm the next file chooser with local file paths |
| `browser_get_history` | Get browsing history entries |
| `browser_get_playing_tabs` | List tabs currently playing audio/video |
| `render_ui` | Draw an interactive A2UI surface (forms, approval cards, live views) in the AI sidebar and block until the user clicks |

**Workspace routing:** one server serves every workspace. A request's `X-Octoweb-Workspace` token says which workspace it acts on; callers that send no token fall through to the first workspace. The sidebar's agent sessions carry their token automatically, so an agent driving tabs in a background workspace never disturbs the one you're looking at.

Point Claude Desktop at `http://localhost:3434/mcp` and it can browse, read, fill forms, and navigate — all while you watch. Set `OCTOWEB_MCP_PORT` / `OCTOWEB_CONFIG_DIR` to run an isolated second instance (e.g. for the e2e suite: `python3 test_mcp.py --mcp-url http://127.0.0.1:3435/mcp`).

### Using octomind as the MCP client

Because octomind has a built-in `mcp` tool that can register external servers at runtime, you can give any running octomind agent live browser access in one step:

```
# inside an octomind session
/mcp add octoweb http://localhost:3434/mcp
```

The agent can now navigate pages, extract content, and interact with the browser as part of its normal tool use — no restart, no config change.

---

## Keyboard shortcuts

Every shortcut is configurable in Settings (`⌘,`) — remaps persist to `~/.config/octoweb/keybindings.json` and apply on the very next keystroke, no restart. The `⌘`+digit slot keys are positional and not remappable.

### Global

| Shortcut | Action |
|---|---|
| `⌘⇧P` | Open command palette |
| `⌘W` | Close tab / session |
| `⌘T` | New AI session |
| `⌘R` | Reload current page |
| `⌘E` | Edit address |
| `⌘S` | Screenshot (viewport) — copy to clipboard |
| `⌘⇧S` | Screenshot (full page) — copy to clipboard |
| `⌘F` | Toggle find-in-page bar |
| `⌘/` | Show keyboard shortcuts |
| `⌘,` | Settings |
| `⌘Q` | Quit |
| `⌘⇧A` | Toggle AI sidebar |
| `⌘⇧I` | Toggle DevTools |
| `⌘↵` | Fullscreen window |
| `⌘⇧↵` | Fullscreen AI sidebar |
| `⌘⇧N` | Pin/unpin tab |
| `⌘⇧O` | Toggle workspace switcher |
| `⌘1` – `⌘9` | Open fast-access slot 1–9 |
| `⌘⇧1` – `⌘⇧9`, `⌘⇧0` | Save current page to slot 1–10 |
| `⌃N` | Next tab (MRU order) |
| `⌃P` | Previous tab (MRU order) |
| `⌃D` | Scroll half page down |
| `⌃U` | Scroll half page up |
| `⌃T` | Scroll to top of page |
| `⌃B` | Scroll to bottom of page |

### Fast-access slots

Pin any page to a numbered slot and jump back to it instantly — one keystroke from anywhere. Slots are shown in a footer bar at the bottom of the window and on the new-tab page.

- **`⌘⇧1`–`⌘⇧9` / `⌘⇧0`** — save the current page to slot 1–9 / 10
- **`⌘1`–`⌘9`** — navigate to the saved URL in that slot

Slots persist across restarts. An empty slot does nothing.

`⌘0` defaults to zoom reset and wins over slot 10 — rebind zoom reset in Settings if you want `⌘0` to open slot 10. `⌘⇧0` still saves to slot 10.

### Command palette (`⌘⇧P`)

The main interface. Type a URL, a search query, or any fragment of a page title or URL you've visited — it fuzzy-matches across open tabs and history instantly, ranked by match quality and visit frequency.

When a query is entered, three action rows appear at the bottom: **Search Google**, **Open URL**, and **Ask AI**. Select one and press Enter, or use the dedicated shortcuts below.

| Shortcut | Action |
|---|---|
| `↑` / `↓` | Move selection |
| `⌃N` / `⌃P` | Move selection (Emacs-style) |
| `↵` | Confirm selection (open / switch / search) |
| `⌘↵` | Force navigate: open as URL if it looks like one, otherwise search |
| `⌘⇧↵` | Send query to AI sidebar |
| `⌘W` | Close selected tab / remove selected history entry |
| `⌘1` – `⌘9`, `⌘0` | Jump directly to result 1–10 (tabs and history only) |
| `Esc` | Close palette |
| `⌃A` / `Home` | Move cursor to start of input |
| `⌃E` / `End` | Move cursor to end of input |
| `⌃K` | Delete from cursor to end of line |
| `⌃U` | Delete from cursor to start of line |
| `⌘V` | Paste from clipboard |

### AI sidebar (`⌘⇧A`)

The sidebar overlays the page on the right — the page content underneath is not resized.

| Shortcut | Action |
|----------|--------|
| `↵` | Send prompt |
| `⇧↵` | Insert newline |
| `↵` *(agent input)* | Apply agent tag |
| `Esc` *(agent input)* | Show agent chip |

**Prompt history:**

- `Ctrl+P` / `Ctrl+N` — Navigate older/newer prompts (MRU order)
- `Ctrl+R` — Reverse incremental search through history
- `Ctrl+E` — Accept ghost text autocomplete
- `Ctrl+U` — Clear input to cursor start

History persists across sessions (`ai_prompt_history.json`).

### Inline AI edit (`⌘⇧E`)

Select text on any page and press `⌘⇧E` to open the inline edit modal. The AI can rewrite, summarize, translate, or transform the selected text.

| Shortcut | Action |
|----------|--------|
| `↵` | Submit transformation |
| `⇧↵` | Insert newline |
| `Esc` | Close modal |

Prompt history works the same as the sidebar (`Ctrl+P/N/R/E/U`).

### Find-in-page (`⌘F`)

Full-text search with highlighting. Uses CSS Custom Highlight API for fast, native rendering.

| Shortcut | Action |
|----------|--------|
| `↵` | Next match |
| `⇧↵` | Previous match |
| `Esc` | Close find bar |

### Page zoom

| Shortcut | Action |
|----------|--------|
| `⌘+` | Zoom in |
| `⌘-` | Zoom out |
| `⌘0` | Reset zoom |

---

## Install

**Homebrew** (recommended):

```bash
brew install --cask muvon/tap/octoweb
```

**Direct download:** grab the archive for your Mac — Apple Silicon or Intel — from [GitHub Releases](https://github.com/muvon/octoweb/releases), unpack it, and drop `Octoweb.app` into `/Applications`.

**Build from source** (requires Rust toolchain and Xcode Command Line Tools):

```bash
git clone https://github.com/muvon/octoweb
cd octoweb

# Dev build (ad-hoc signed, no cert needed)
./build.sh --dev

# Release build (requires Developer ID cert)
./build.sh

# Run
open dist/Octoweb.app
```

Or install to Applications:
```bash
cp -r dist/Octoweb.app /Applications/
```

---

## Configuration

Config lives at `~/Library/Application Support/octoweb/config.toml`. Created on first launch with defaults.

Open Settings with `⌘,` to configure visually, or edit the file directly:

```toml
home_page                = "https://www.google.com"
search_engine            = "https://www.google.com/search?q={}"
max_history              = 1000
window_width             = 1280
window_height            = 800
ai_edit_auto_hide        = false      # auto-hide inline edit after submit
max_prompt_history       = 50         # editor prompt history size
max_ai_prompt_history    = 50         # sidebar prompt history size
max_acp_session_messages = 500        # chat messages retained per AI session (FIFO)
proactive_learning       = true       # enable background learning agent
learning_interval_min    = 30         # minutes between learning runs (floored to 5 on load)
aggressive_hibernation   = false      # aggressive tab-hibernation curve for tight-RAM machines
max_tabs                 = 500        # max open tabs (0 = no limit)
```

**Persistence:**

- **Session** — Open tabs and active tab restored on next launch
- **History** — Browsing history up to `max_history` entries
- **Favicons** — Cached as base64 data-URIs, no network on startup
- **Prompt history** — Separate histories for inline edit and sidebar

---

## What it is (and isn't)

octoweb is an experiment. It's a real, usable browser — WebKit rendering, proper tab management, session restore, back/forward gestures, progress bar, error pages — but it's also a playground for the idea that a browser can be a first-class AI client, not just a container for AI extensions.

It's macOS only. It will stay that way for now — the whole thing leans on macOS-native APIs (CGEventTap for global hotkeys, WKWebView via wry, AppKit for the dock icon and menus).

It won't replace your main browser. It might become the browser you reach for when you want to think.

---

## Tech stack

- **[wry](https://github.com/tauri-apps/wry)** — WebView (WKWebView on macOS)
- **[tao](https://github.com/tauri-apps/tao)** — windowing + event loop
- **[octomind](https://github.com/muvon/octomind)** — plug-and-play AI agent runtime powering the sidebar
- **[agent-client-protocol](https://github.com/muvon/agent-client-protocol)** — ACP for browser ↔ agent communication
- **[rmcp](https://github.com/modelcontextprotocol/rust-sdk)** — MCP server (AI browser control)
- **CGEventTap** — system-wide keyboard shortcuts without rdev
- Rust, release profile: LTO + `codegen-units=1` + stripped binary

---

## License

[Apache 2.0](LICENSE)
