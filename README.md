# octoweb

A macOS browser for people who think with their keyboard.

Built on [wry](https://github.com/tauri-apps/wry) + [tao](https://github.com/tauri-apps/tao). No Electron. No Chrome. No memory bloat. Just WebKit, Rust, and a few sharp shortcuts.

---

## Why

Most browsers are built for everyone. octoweb is built for you — the person who reaches for the keyboard before the mouse, who has 20 tabs open but knows exactly which one they want, and who thinks the browser UI should get out of the way.

Three things it does differently:

1. **Keyboard-first navigation** — every action has a shortcut, nothing requires a click
2. **AI assistant built in** — not an extension, not a tab, a sidebar powered by a local [octomind](https://github.com/muvon/octomind) agent connected via [ACP](https://github.com/muvon/agent-client-protocol)
3. **MCP server inside the browser** — your AI tools can actually *drive* the browser (`localhost:3434`)

---

## AI assistant

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

No data leaves your machine unless your agent sends it somewhere. The AI provider call is made by octomind, not by the browser.

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
|---|---|
| `browser_navigate` | Navigate to a URL (`new_tab: true` to open in background) |
| `browser_get_tabs` | List all open tabs with IDs, titles, URLs |
| `browser_switch_tab` | Switch to a tab by ID |
| `browser_close_tab` | Close a tab by ID |
| `browser_get_page_info` | Get title, URL, meta description of current page |
| `browser_execute_js` | Run arbitrary JavaScript in the page |
| `browser_click` | Click an element by CSS selector |
| `browser_type` | Type text into an input by CSS selector |

Point Claude Desktop at `http://localhost:3434/mcp` and it can browse, read, fill forms, and navigate — all while you watch.

### Using octomind as the MCP client

Because octomind has a built-in `mcp` tool that can register external servers at runtime, you can give any running octomind agent live browser access in one step:

```
# inside an octomind session
/mcp add octoweb http://localhost:3434/mcp
```

The agent can now navigate pages, extract content, and interact with the browser as part of its normal tool use — no restart, no config change.

---

## Keyboard shortcuts

### Global

| Shortcut | Action |
|---|---|
| `⌘K` | Open command palette |
| `⌘W` | Close current tab |
| `⌘R` | Reload current page |
| `⌘Q` | Quit |
| `⌘⇧A` | Toggle AI sidebar |
| `⌘⇧I` | Toggle DevTools |
| `⌃N` | Next tab (MRU order) |
| `⌃P` | Previous tab (MRU order) |

### Command palette (`⌘K`)

The main interface. Type a URL, a search query, or any fragment of a page title or URL you've visited — it fuzzy-matches across open tabs and history instantly.

| Shortcut | Action |
|---|---|
| `↑` / `↓` | Move selection |
| `⌃N` / `⌃P` | Move selection (Emacs-style) |
| `Return` | Open / navigate / switch to tab |
| `Esc` | Close palette |
| `⌘W` | Close the selected tab (while palette is open) |
| `⌃A` | Move cursor to start of input |
| `⌃E` | Move cursor to end of input |
| `⌃K` | Delete from cursor to end of line |
| `⌃U` | Delete from cursor to start of line |
| `⌘V` | Paste from clipboard |
| `Home` / `End` | Move cursor to start / end |

### AI sidebar (`⌘⇧A`)

| Shortcut | Action |
|---|---|
| `Return` | Send prompt |
| `⇧Return` | Insert newline |

---

## Install & build

**Requirements:** macOS, Rust toolchain, Xcode Command Line Tools.

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

```toml
home_page     = "https://www.google.com"
search_engine = "https://www.google.com/search?q={}"
max_history   = 1000
window_width  = 1280
window_height = 800
```

Session (open tabs + active tab) is restored automatically on next launch.
Favicons are cached as base64 data-URIs — no network requests on startup.

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

MIT
