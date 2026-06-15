//! MCP (Model Context Protocol) server for AI assistant control.
//!
//! Exposes browser tools over HTTP JSON-RPC on http://127.0.0.1:3434/mcp.
//!
//! # Tool surface
//!
//! Discovery & reading:
//! - `browser_snapshot`     — compact map of interactive elements with @N refs
//! - `browser_get_page_info`/`browser_get_page_content` — metadata and readable text
//! - `browser_execute_js`                                — escape hatch for custom DOM queries
//! - `browser_console_messages` / `browser_network_requests` — page diagnostics
//!
//! Tabs:
//! - `browser_navigate` (always background: new tab, or in-place via tab_id; never moves focus)
//! - `browser_get_tabs` / `browser_get_current_tab` / `browser_switch_tab` / `browser_close_tab`
//! - `browser_get_history` / `browser_get_playing_tabs`
//! - `browser_go_back` / `browser_go_forward` / `browser_reload`
//!
//! Interaction (selector accepts a CSS selector or a `@N` ref from snapshot;
//! actions auto-retry until actionable — see dom_actions.rs):
//! - `browser_click` / `browser_hover` / `browser_type`
//! - `browser_press_key` / `browser_select_option`
//! - `browser_scroll` / `browser_wait` / `browser_screenshot`
//! - `browser_handle_dialog` / `browser_upload_file` — arm answers for native
//!   dialogs and file choosers before the click that triggers them
//!
//! # Selector resolution
//!
//! `selector` is interpreted in JS by checking the first character: a leading
//! `@` resolves against `window.__octoweb_refs` (populated by `browser_snapshot`)
//! and anything else is passed to `document.querySelector`. The interaction
//! tools all return distinct error reasons (`stale`, `missing`, `detached`,
//! `invalid:<msg>`) so the AI knows whether to re-snapshot, fix the selector,
//! or wait for the page to settle.

use crate::sanitize;

use rmcp::{
    handler::server::wrapper::Parameters,
    model::{
        CallToolResult, Content, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
    },
    schemars, tool, tool_handler, tool_router,
    transport::streamable_http_server::{
        session::local::LocalSessionManager, StreamableHttpService,
    },
    ErrorData as McpError, ServerHandler,
};
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};

// ─────────────────────────────────────────────────────────────────────
// MCP Commands (sent from MCP server to main event loop)
// ─────────────────────────────────────────────────────────────────────

/// Commands that MCP tools can request from the main event loop.
pub enum McpCommand {
    /// Navigate to URL — in a new background tab (no tab_id) or an existing tab.
    /// Never moves focus; browser_switch_tab is the only way to foreground a tab.
    Navigate {
        url: String,
        tab_id: Option<usize>,
        response: oneshot::Sender<Result<usize, String>>,
    },
    /// Get list of all tabs
    GetTabs {
        response: oneshot::Sender<Result<Vec<TabInfo>, String>>,
    },
    /// Get the currently active tab
    GetCurrentTab {
        response: oneshot::Sender<Result<TabInfo, String>>,
    },
    /// Switch to tab by ID
    SwitchTab {
        tab_id: usize,
        response: oneshot::Sender<Result<(), String>>,
    },
    /// Close tab by ID
    CloseTab {
        tab_id: usize,
        response: oneshot::Sender<Result<(), String>>,
    },
    /// Get current page info (title, URL)
    GetPageInfo {
        tab_id: Option<usize>,
        response: oneshot::Sender<Result<PageInfo, String>>,
        /// Internal: see `GetPageContent::is_retry`.
        is_retry: bool,
    },
    /// Execute JavaScript in page — returns the JS result as a JSON string
    ExecuteJs {
        tab_id: Option<usize>,
        script: String,
        response: oneshot::Sender<Result<String, String>>,
    },
    /// Click element by selector — returns whether element was found
    Click {
        tab_id: Option<usize>,
        selector: String,
        response: oneshot::Sender<Result<bool, String>>,
    },
    /// Hover over element by selector — returns whether element was found
    Hover {
        tab_id: Option<usize>,
        selector: String,
        response: oneshot::Sender<Result<bool, String>>,
    },
    /// Type text into input — returns whether element was found
    Type {
        tab_id: Option<usize>,
        selector: String,
        text: String,
        response: oneshot::Sender<Result<bool, String>>,
    },
    /// Navigate back in browser history
    GoBack {
        tab_id: Option<usize>,
        response: oneshot::Sender<Result<(), String>>,
    },
    /// Navigate forward in browser history
    GoForward {
        tab_id: Option<usize>,
        response: oneshot::Sender<Result<(), String>>,
    },
    /// Get browsing history entries
    GetHistory {
        limit: Option<usize>,
        response: oneshot::Sender<Result<Vec<HistoryInfo>, String>>,
    },
    /// Get tabs currently playing audio
    GetPlayingTabs {
        response: oneshot::Sender<Result<Vec<TabInfo>, String>>,
    },
    /// Reload a tab
    Reload {
        tab_id: Option<usize>,
        response: oneshot::Sender<Result<(), String>>,
    },
    /// Get readable text content of a page
    GetPageContent {
        tab_id: Option<usize>,
        response: oneshot::Sender<Result<String, String>>,
        /// Internal: true when re-issued by a watchdog after the first eval's
        /// callback was discarded. Suppresses a second retry so we don't loop.
        is_retry: bool,
    },
    /// Take a screenshot of a tab (viewport or full page).
    /// Ok variant carries base64-encoded PNG data.
    Screenshot {
        tab_id: Option<usize>,
        full_page: bool,
        response: oneshot::Sender<Result<String, String>>,
    },
    /// Scroll the page, or the scrollable container of `selector`
    Scroll {
        tab_id: Option<usize>,
        direction: String,
        pixels: Option<i32>,
        selector: Option<String>,
        response: oneshot::Sender<Result<(), String>>,
    },
    /// Press a keyboard key
    PressKey {
        tab_id: Option<usize>,
        key: String,
        selector: Option<String>,
        modifiers: Vec<String>,
        response: oneshot::Sender<Result<bool, String>>,
    },
    /// Wait for page load or element to appear
    Wait {
        tab_id: Option<usize>,
        event: String,
        timeout_ms: u64,
        response: oneshot::Sender<Result<String, String>>,
    },
    /// Select an option in a <select> dropdown
    SelectOption {
        tab_id: Option<usize>,
        selector: String,
        value: String,
        response: oneshot::Sender<Result<bool, String>>,
    },
    /// Take a snapshot of interactive elements on the page
    Snapshot {
        tab_id: Option<usize>,
        response: oneshot::Sender<Result<String, String>>,
        /// Internal: see `GetPageContent::is_retry`.
        is_retry: bool,
    },
}

// ─────────────────────────────────────────────────────────────────────
// DOM-result interpreter (shared by Click/Hover/Type/PressKey/SelectOption)
// ─────────────────────────────────────────────────────────────────────

/// Map a status string returned by our injected DOM scripts into a
/// `Result<bool, String>` with an actionable message for the AI.
///
/// Statuses:
/// - `"true"`         — action succeeded
/// - `"stale"`        — `@ref` selector but `window.__octoweb_refs` is missing
///   or the ref was never registered. Most often: navigation cleared refs.
///   Fix: call `browser_snapshot` again.
/// - `"missing"`      — CSS selector matched no element (after the retry window).
/// - `"detached"`     — `@ref` resolved but the element is no longer in the DOM.
/// - `"invalid:<msg>"`— `document.querySelector` threw on a malformed selector.
/// - `"disabled"`     — element stayed disabled/readonly through the retry window.
/// - `"occluded:<el>"`— another element kept covering the click point; `<el>`
///   describes the cover (tag#id.class) so the AI knows what to dismiss.
///
/// `val` is the JSON-encoded return value from `evaluate_script_with_callback`,
/// so it usually arrives as a quoted string (`"\"true\""`); the trim handles
/// both quoted and unquoted forms.
pub fn interpret_dom_result(val: &str, selector: &str) -> Result<bool, String> {
    let trimmed = val.trim().trim_matches('"');
    match trimmed {
        "true" => Ok(true),
        "stale" => Err(format!(
            "@ref '{selector}' is stale — call browser_snapshot first to refresh refs (they invalidate on navigation)"
        )),
        "missing" => Err(format!(
            "No element matched selector: {selector} (retried for {}s)",
            crate::dom_actions::RETRY_MS / 1000
        )),
        "detached" => Err(format!(
            "Element for '{selector}' is no longer in the DOM — re-snapshot or wait for the page to settle"
        )),
        "disabled" => Err(format!(
            "Element '{selector}' stayed disabled/readonly — wait for the page to enable it or pick another element"
        )),
        s if s.starts_with("occluded:") => Err(format!(
            "Element '{selector}' is covered by {} — dismiss that overlay (cookie banner, modal, dropdown) first, or scroll it away, then retry",
            &s[9..]
        )),
        s if s.starts_with("invalid:") => Err(format!(
            "Invalid CSS selector '{selector}': {}",
            &s[8..]
        )),
        _ => Err(format!("Unexpected DOM result: {val}")),
    }
}

// ─────────────────────────────────────────────────────────────────────
// Data types for MCP responses
// ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TabInfo {
    pub id: usize,
    pub title: String,
    pub url: String,
    pub is_active: bool,
    pub is_playing_audio: bool,
}

impl TabInfo {
    /// Build from a browser::Tab with URL sanitization (strips sensitive query params).
    pub fn from_tab(tab: &crate::browser::Tab, is_active: bool) -> Self {
        Self {
            id: tab.id,
            title: tab.title.clone(),
            url: sanitize::sanitize_url(&tab.url),
            is_active,
            is_playing_audio: tab.is_playing_audio,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PageInfo {
    pub title: String,
    pub url: String,
    pub description: Option<String>,
}

impl PageInfo {
    /// Build with URL sanitization and PAN scrubbing on description.
    pub fn new(title: String, url: String, description: Option<String>) -> Self {
        Self {
            title,
            url: sanitize::sanitize_url(&url),
            description: description.map(|d| sanitize::sanitize_text(&d)),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct HistoryInfo {
    pub title: String,
    pub url: String,
    /// Unix timestamp (seconds) of when the page was visited
    pub visited_at: u64,
}

impl HistoryInfo {
    /// Build from a browser::HistoryEntry with URL sanitization.
    pub fn from_entry(entry: &crate::browser::HistoryEntry) -> Self {
        Self {
            title: entry.title.clone(),
            url: sanitize::sanitize_url(&entry.url),
            visited_at: entry.visited_at,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────
// MCP Tool Request Types
// ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct NavigateRequest {
    #[schemars(
        description = "URL or search query. Bare hosts and queries are resolved automatically."
    )]
    pub url: String,
    #[schemars(
        description = "Existing tab to navigate in-place (e.g. a tab you opened earlier). Omit to open a NEW background tab. To drive the tab the user is looking at, pass the id from browser_get_current_tab."
    )]
    pub tab_id: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TabIdRequest {
    #[schemars(description = "Tab to target. Omit for the user's visible tab.")]
    pub tab_id: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SwitchTabRequest {
    #[schemars(description = "Tab ID to switch to")]
    pub tab_id: usize,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct CloseTabRequest {
    #[schemars(description = "Tab ID to close")]
    pub tab_id: usize,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ExecuteJsRequest {
    #[schemars(description = "Tab to target. Omit for the user's visible tab.")]
    pub tab_id: Option<usize>,
    #[schemars(description = "JavaScript code to execute")]
    pub script: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ClickRequest {
    #[schemars(description = "Tab to target. Omit for the user's visible tab.")]
    pub tab_id: Option<usize>,
    #[schemars(description = "CSS selector or @ref from browser_snapshot.")]
    pub selector: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct HoverRequest {
    #[schemars(description = "Tab to target. Omit for the user's visible tab.")]
    pub tab_id: Option<usize>,
    #[schemars(description = "CSS selector or @ref from browser_snapshot.")]
    pub selector: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TypeRequest {
    #[schemars(description = "Tab to target. Omit for the user's visible tab.")]
    pub tab_id: Option<usize>,
    #[schemars(
        description = "CSS selector or @ref of an <input>, <textarea>, or contenteditable element."
    )]
    pub selector: String,
    #[schemars(description = "Text to set. Replaces the existing value — does not append.")]
    pub text: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ScreenshotRequest {
    #[schemars(description = "Tab to target. Omit for the user's visible tab.")]
    pub tab_id: Option<usize>,
    #[schemars(
        description = "Capture the entire scrollable page instead of just the viewport. Default false."
    )]
    pub full_page: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetHistoryRequest {
    #[schemars(description = "Max entries, most recent first. Default 50.")]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ScrollRequest {
    #[schemars(description = "Tab to target. Omit for the user's visible tab.")]
    pub tab_id: Option<usize>,
    #[schemars(description = "\"up\", \"down\", \"top\", or \"bottom\".")]
    pub direction: String,
    #[schemars(description = "Distance in px (only with up/down). Default: ~one viewport.")]
    pub pixels: Option<i32>,
    #[schemars(
        description = "CSS selector or @ref: scroll the nearest scrollable container of that element instead of the window. Needed for app-style pages (chat lists, virtualized feeds) where the window itself never scrolls."
    )]
    pub selector: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PressKeyRequest {
    #[schemars(description = "Tab to target. Omit for the user's visible tab.")]
    pub tab_id: Option<usize>,
    #[schemars(
        description = "KeyboardEvent.key value: \"Enter\", \"Escape\", \"Tab\", \"ArrowDown\", \"Backspace\", \"a\", etc."
    )]
    pub key: String,
    #[schemars(
        description = "Target element (CSS selector or @ref). Omit to use the focused element."
    )]
    pub selector: Option<String>,
    #[schemars(description = "Modifiers to hold: any of \"shift\", \"ctrl\", \"alt\", \"meta\".")]
    pub modifiers: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct WaitRequest {
    #[schemars(description = "Tab to target. Omit for the user's visible tab.")]
    pub tab_id: Option<usize>,
    #[schemars(
        description = "\"load\" (default) | \"domcontentloaded\" | \"ready\" (full SPA readiness — same probe browser_navigate uses; resolves to \"ready\"/\"live\"/\"partial\") | a CSS selector to wait for."
    )]
    pub event: Option<String>,
    #[schemars(
        description = "Timeout in ms. Default 10000, max 25000. Ignored for event=\"ready\" (the probe has its own 8 s ceiling)."
    )]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SnapshotRequest {
    #[schemars(description = "Tab to target. Omit for the user's visible tab.")]
    pub tab_id: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ConsoleRequest {
    #[schemars(description = "Tab to target. Omit for the user's visible tab.")]
    pub tab_id: Option<usize>,
    #[schemars(
        description = "Only entries of this level: \"log\", \"info\", \"warn\", \"error\", \"debug\". Omit for all."
    )]
    pub level: Option<String>,
    #[schemars(description = "Max entries, newest last. Default 50, max 200.")]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct NetworkRequest {
    #[schemars(description = "Tab to target. Omit for the user's visible tab.")]
    pub tab_id: Option<usize>,
    #[schemars(description = "Substring filter on the request URL.")]
    pub filter: Option<String>,
    #[schemars(description = "Max entries, newest last. Default 50, max 200.")]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct HandleDialogRequest {
    #[schemars(description = "\"accept\" (OK) or \"dismiss\" (Cancel).")]
    pub action: String,
    #[schemars(
        description = "Text to enter into a prompt() when accepting. Defaults to the page's default text."
    )]
    pub prompt_text: Option<String>,
    #[schemars(description = "How many upcoming dialogs to auto-answer. Default 1, max 20.")]
    pub count: Option<u32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct UploadFileRequest {
    #[schemars(description = "Absolute paths of existing files to attach.")]
    pub paths: Vec<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SelectOptionRequest {
    #[schemars(description = "Tab to target. Omit for the user's visible tab.")]
    pub tab_id: Option<usize>,
    #[schemars(description = "CSS selector or @ref of the <select> element.")]
    pub selector: String,
    #[schemars(description = "Value attribute of the <option> to select.")]
    pub value: String,
}

// ─────────────────────────────────────────────────────────────────────
// MCP Server Implementation
// ─────────────────────────────────────────────────────────────────────

/// Shared state for MCP tools to communicate with main event loop.
#[derive(Clone)]
pub struct McpState {
    /// Channel to send commands to main event loop
    pub command_tx: mpsc::UnboundedSender<McpCommand>,
}

/// MCP Server that exposes browser control tools.
#[derive(Clone)]
pub struct McpServer {
    state: McpState,
}

/// Tool-level error: model-visible (`isError` result), unlike protocol
/// errors which many clients surface only as opaque transport failures.
/// Domain failures (tab gone, element missing, occluded, …) are part of the
/// browsing loop and the AI must be able to read and react to them.
fn err_result(msg: String) -> CallToolResult {
    CallToolResult::error(vec![Content::text(msg)])
}

/// Unwrap a browser-domain `Result` inside a tool body: `Ok(v)` yields `v`,
/// `Err(msg)` returns a model-visible tool error from the enclosing tool fn.
macro_rules! browser_try {
    ($e:expr) => {
        match $e {
            Ok(v) => v,
            Err(msg) => return Ok(err_result(msg)),
        }
    };
}

/// Helpers (separate impl block so the #[tool_router] macro doesn't interfere).
impl McpServer {
    /// Send a command to the main event loop and wait for the response (30 s timeout).
    ///
    /// Outer error = infrastructure failure (channel/timeout) → protocol error.
    /// Inner error = browser-domain failure → tools surface it via
    /// `browser_try!` as a model-visible `isError` result.
    async fn send_command<T>(
        &self,
        build: impl FnOnce(oneshot::Sender<Result<T, String>>) -> McpCommand,
    ) -> Result<Result<T, String>, McpError> {
        let (tx, rx) = oneshot::channel();
        self.state
            .command_tx
            .send(build(tx))
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        tokio::time::timeout(std::time::Duration::from_secs(30), rx)
            .await
            .map_err(|_| {
                McpError::internal_error("browser did not respond within 30 s".to_string(), None)
            })?
            // Inner `RecvError` means the main-loop sender was dropped
            // without sending. With the per-handler watchdogs in main.rs,
            // every JS-callback path now claims its Sender on timeout and
            // sends a structured error or retries, so this branch should
            // only fire on catastrophic main-loop drops (panic, shutdown).
            // Keep the message defensive — actionable rather than the bare
            // tokio "channel closed" that misleads the AI into thinking
            // the tab is dead.
            .map_err(|_| {
                McpError::internal_error(
                    "browser dropped this request without replying (main loop \
                 unavailable). Tab state is unchanged; retry the call."
                        .to_string(),
                    None,
                )
            })
    }
}

#[tool_router]
impl McpServer {
    pub fn new(command_tx: mpsc::UnboundedSender<McpCommand>) -> Self {
        Self {
            state: McpState { command_tx },
        }
    }

    // ── Navigation ──────────────────────────────────────────────────

    #[tool(
        description = "Navigate to a URL. Blocks until the page (including SPA bootstrap) is fully loaded — no follow-up browser_wait needed. Returns the tab ID.\n\nDefault (url only): opens a NEW tab in the background. Pass tab_id to navigate an existing tab in-place instead (errors if it no longer exists — omit tab_id to get a fresh one).\n\nNavigation NEVER changes what the user sees. To show a tab to the user, call browser_switch_tab with the returned tab ID. To navigate the tab the user is viewing, pass the id from browser_get_current_tab."
    )]
    async fn browser_navigate(
        &self,
        Parameters(req): Parameters<NavigateRequest>,
    ) -> Result<CallToolResult, McpError> {
        let in_place = req.tab_id.is_some();
        tracing::debug!(url = %req.url, ?req.tab_id, "MCP browser_navigate");

        let tab_id = browser_try!(
            self.send_command(|tx| McpCommand::Navigate {
                url: req.url,
                tab_id: req.tab_id,
                response: tx,
            })
            .await?
        );

        let result = serde_json::json!({
            "tab_id": tab_id,
            "mode": if in_place { "in_place" } else { "new_background_tab" },
            "note": if in_place {
                "Tab navigated; focus unchanged."
            } else {
                "Background tab opened — reuse this tab_id for follow-up calls; browser_switch_tab shows it to the user."
            },
        });
        Ok(CallToolResult::success(vec![Content::text(
            result.to_string(),
        )]))
    }

    #[tool(description = "Go back in this tab's history. Defaults to the visible tab.")]
    async fn browser_go_back(
        &self,
        Parameters(req): Parameters<TabIdRequest>,
    ) -> Result<CallToolResult, McpError> {
        browser_try!(
            self.send_command(|tx| McpCommand::GoBack {
                tab_id: req.tab_id,
                response: tx,
            })
            .await?
        );
        Ok(CallToolResult::success(vec![Content::text(
            "Navigated back",
        )]))
    }

    #[tool(description = "Go forward in this tab's history. Defaults to the visible tab.")]
    async fn browser_go_forward(
        &self,
        Parameters(req): Parameters<TabIdRequest>,
    ) -> Result<CallToolResult, McpError> {
        browser_try!(
            self.send_command(|tx| McpCommand::GoForward {
                tab_id: req.tab_id,
                response: tx,
            })
            .await?
        );
        Ok(CallToolResult::success(vec![Content::text(
            "Navigated forward",
        )]))
    }

    #[tool(description = "Reload the tab. Defaults to the visible tab.")]
    async fn browser_reload(
        &self,
        Parameters(req): Parameters<TabIdRequest>,
    ) -> Result<CallToolResult, McpError> {
        browser_try!(
            self.send_command(|tx| McpCommand::Reload {
                tab_id: req.tab_id,
                response: tx,
            })
            .await?
        );
        Ok(CallToolResult::success(vec![Content::text(
            "Page reloaded",
        )]))
    }

    #[tool(
        description = "Wait for a condition. NOT needed after browser_navigate (already waits). Use after a click that triggers an SPA route change, or for lazily-loaded content. event: \"load\" (default) | \"domcontentloaded\" | \"ready\" (full SPA readiness — main content rendered + DOM/JS quiet, with a steady-state fallback for live feeds; returns \"ready\" | \"live\" | \"partial\") | a CSS selector to wait for. Other events return \"ready\" or \"timeout\"."
    )]
    async fn browser_wait(
        &self,
        Parameters(req): Parameters<WaitRequest>,
    ) -> Result<CallToolResult, McpError> {
        let event = req.event.unwrap_or_else(|| "load".to_string());
        // Cap below send_command's 30 s ceiling so a maxed-out wait returns a
        // clean "timeout" instead of racing the generic MCP-timeout error.
        let timeout_ms = req.timeout_ms.unwrap_or(10_000).min(25_000);
        let result = browser_try!(
            self.send_command(|tx| McpCommand::Wait {
                tab_id: req.tab_id,
                event,
                timeout_ms,
                response: tx,
            })
            .await?
        );
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    // ── Tab management ──────────────────────────────────────────────

    #[tool(
        description = "List all open tabs. Returns [{id, title, url, is_active, is_playing_audio}]. is_active marks the visible tab. Use the IDs to target other tools."
    )]
    async fn browser_get_tabs(&self) -> Result<CallToolResult, McpError> {
        let tabs = browser_try!(
            self.send_command(|tx| McpCommand::GetTabs { response: tx })
                .await?
        );
        let json = serde_json::to_string(&tabs)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Get the tab the user is currently viewing. Use this to know what's on screen right now. For background tabs, use browser_get_tabs instead."
    )]
    async fn browser_get_current_tab(&self) -> Result<CallToolResult, McpError> {
        let tab = browser_try!(
            self.send_command(|tx| McpCommand::GetCurrentTab { response: tx })
                .await?
        );
        let json = serde_json::to_string(&tab)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Make a tab visible to the user. This changes what they see — only use when the task is done and they need to see the result, or they explicitly asked to switch."
    )]
    async fn browser_switch_tab(
        &self,
        Parameters(req): Parameters<SwitchTabRequest>,
    ) -> Result<CallToolResult, McpError> {
        browser_try!(
            self.send_command(|tx| McpCommand::SwitchTab {
                tab_id: req.tab_id,
                response: tx,
            })
            .await?
        );
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Switched to tab {}",
            req.tab_id
        ))]))
    }

    #[tool(
        description = "Close a tab by ID. If it's the visible tab, the next tab becomes visible. Always close background tabs when you're done."
    )]
    async fn browser_close_tab(
        &self,
        Parameters(req): Parameters<CloseTabRequest>,
    ) -> Result<CallToolResult, McpError> {
        browser_try!(
            self.send_command(|tx| McpCommand::CloseTab {
                tab_id: req.tab_id,
                response: tx,
            })
            .await?
        );
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Closed tab {}",
            req.tab_id
        ))]))
    }

    // ── Page content ────────────────────────────────────────────────

    #[tool(description = "Page metadata: {title, url, description}. Defaults to the visible tab.")]
    async fn browser_get_page_info(
        &self,
        Parameters(req): Parameters<TabIdRequest>,
    ) -> Result<CallToolResult, McpError> {
        let info = browser_try!(
            self.send_command(|tx| McpCommand::GetPageInfo {
                tab_id: req.tab_id,
                response: tx,
                is_retry: false,
            })
            .await?
        );
        let json = serde_json::to_string(&info)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Readable text of the page (document.body.innerText). Use for reading articles, search results, or extracting data. Defaults to the visible tab."
    )]
    async fn browser_get_page_content(
        &self,
        Parameters(req): Parameters<TabIdRequest>,
    ) -> Result<CallToolResult, McpError> {
        let content = browser_try!(
            self.send_command(|tx| McpCommand::GetPageContent {
                tab_id: req.tab_id,
                response: tx,
                is_retry: false,
            })
            .await?
        );
        Ok(CallToolResult::success(vec![Content::text(content)]))
    }

    #[tool(
        description = "Compact map of interactive elements with @ref selectors. Each link/button/input gets a @N ref you can pass directly to browser_click / browser_type / browser_hover / browser_press_key / browser_select_option as the selector — much cheaper than guessing CSS. Includes same-origin iframes and open shadow DOM (web components) — @refs reach elements there that CSS selectors cannot. Refs invalidate on navigation; call again after browser_navigate or any SPA route change. Always start here on an unfamiliar page."
    )]
    async fn browser_snapshot(
        &self,
        Parameters(req): Parameters<SnapshotRequest>,
    ) -> Result<CallToolResult, McpError> {
        let snapshot = browser_try!(
            self.send_command(|tx| McpCommand::Snapshot {
                tab_id: req.tab_id,
                response: tx,
                is_retry: false,
            })
            .await?
        );
        Ok(CallToolResult::success(vec![Content::text(snapshot)]))
    }

    #[tool(
        description = "Run JavaScript in the page and return its JSON-encoded result. Use only when other tools don't fit — prefer browser_get_page_content for text and browser_snapshot+click/type for interaction. Last expression is the return value (wrap multi-statement code in an IIFE). Defaults to the visible tab."
    )]
    async fn browser_execute_js(
        &self,
        Parameters(req): Parameters<ExecuteJsRequest>,
    ) -> Result<CallToolResult, McpError> {
        let value = browser_try!(
            self.send_command(|tx| McpCommand::ExecuteJs {
                tab_id: req.tab_id,
                script: req.script,
                response: tx,
            })
            .await?
        );
        Ok(CallToolResult::success(vec![Content::text(value)]))
    }

    #[tool(
        description = "Screenshot the tab's content and copy the PNG to the clipboard. Returns the image to the caller too. full_page=true captures the entire scrollable page. Defaults to the visible tab."
    )]
    async fn browser_screenshot(
        &self,
        Parameters(req): Parameters<ScreenshotRequest>,
    ) -> Result<CallToolResult, McpError> {
        let b64_png = browser_try!(
            self.send_command(|tx| McpCommand::Screenshot {
                tab_id: req.tab_id,
                full_page: req.full_page.unwrap_or(false),
                response: tx,
            })
            .await?
        );
        Ok(CallToolResult::success(vec![Content::image(
            b64_png,
            "image/png",
        )]))
    }

    // ── Interaction ─────────────────────────────────────────────────

    #[tool(
        description = "Click an element by CSS selector or @ref from browser_snapshot. Scrolls into view, then dispatches the full pointerdown+mousedown+focus+pointerup+mouseup+click sequence (works with pointer-event UIs like React/MUI/Radix). On error you get a specific reason: stale @ref → re-snapshot; missing → no element; detached → element gone from DOM. Defaults to the visible tab."
    )]
    async fn browser_click(
        &self,
        Parameters(req): Parameters<ClickRequest>,
    ) -> Result<CallToolResult, McpError> {
        let found = browser_try!(
            self.send_command(|tx| McpCommand::Click {
                tab_id: req.tab_id,
                selector: req.selector.clone(),
                response: tx,
            })
            .await?
        );
        if found {
            Ok(CallToolResult::success(vec![Content::text(
                "Element clicked successfully",
            )]))
        } else {
            Ok(CallToolResult::error(vec![Content::text(format!(
                "Element not found: {}",
                req.selector
            ))]))
        }
    }

    #[tool(
        description = "Hover over an element. Dispatches pointer + mouse enter/over/move events — works for JS-driven menus (Bootstrap, React, MUI, Radix). Pure CSS :hover cannot be activated synthetically (WebKit limitation). Defaults to the visible tab."
    )]
    async fn browser_hover(
        &self,
        Parameters(req): Parameters<HoverRequest>,
    ) -> Result<CallToolResult, McpError> {
        let found = browser_try!(
            self.send_command(|tx| McpCommand::Hover {
                tab_id: req.tab_id,
                selector: req.selector.clone(),
                response: tx,
            })
            .await?
        );
        if found {
            Ok(CallToolResult::success(vec![Content::text(
                "Element hovered successfully",
            )]))
        } else {
            Ok(CallToolResult::error(vec![Content::text(format!(
                "Element not found: {}",
                req.selector
            ))]))
        }
    }

    #[tool(
        description = "Set the value of an input. Works with <input>, <textarea>, and contenteditable (Gmail compose, rich editors). REPLACES the existing value — does not append. Bypasses React's controlled-input cache by using the prototype's value setter, then fires input+change. To press Enter / submit afterwards, use browser_press_key. Defaults to the visible tab."
    )]
    async fn browser_type(
        &self,
        Parameters(req): Parameters<TypeRequest>,
    ) -> Result<CallToolResult, McpError> {
        let found = browser_try!(
            self.send_command(|tx| McpCommand::Type {
                tab_id: req.tab_id,
                selector: req.selector.clone(),
                text: req.text,
                response: tx,
            })
            .await?
        );
        if found {
            Ok(CallToolResult::success(vec![Content::text(
                "Text typed successfully",
            )]))
        } else {
            Ok(CallToolResult::error(vec![Content::text(format!(
                "Element not found: {}",
                req.selector
            ))]))
        }
    }

    #[tool(
        description = "Scroll the page. up/down = ~one viewport (or custom pixels), top/bottom = jump to start/end. Pass selector (CSS or @ref) to scroll that element's scrollable container instead of the window — required for chat lists, sidebars, and virtualized feeds. Defaults to the visible tab."
    )]
    async fn browser_scroll(
        &self,
        Parameters(req): Parameters<ScrollRequest>,
    ) -> Result<CallToolResult, McpError> {
        browser_try!(
            self.send_command(|tx| McpCommand::Scroll {
                tab_id: req.tab_id,
                direction: req.direction.clone(),
                pixels: req.pixels,
                selector: req.selector,
                response: tx,
            })
            .await?
        );
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Scrolled {}",
            req.direction
        ))]))
    }

    #[tool(
        description = "Press a key, optionally with modifiers. Use for Enter (submit), Escape (close), Tab (focus), arrows, etc. Enter on an <input> inside a form also submits the form (like a real keypress) unless the page handled the keydown itself. If selector is given, focus that element first; otherwise the currently focused element receives the key. Note: this dispatches synthetic KeyboardEvents — for *typing characters into an input* prefer browser_type. Defaults to the visible tab."
    )]
    async fn browser_press_key(
        &self,
        Parameters(req): Parameters<PressKeyRequest>,
    ) -> Result<CallToolResult, McpError> {
        let found = browser_try!(
            self.send_command(|tx| McpCommand::PressKey {
                tab_id: req.tab_id,
                key: req.key.clone(),
                selector: req.selector.clone(),
                modifiers: req.modifiers.unwrap_or_default(),
                response: tx,
            })
            .await?
        );
        if found {
            Ok(CallToolResult::success(vec![Content::text(format!(
                "Pressed key: {}",
                req.key
            ))]))
        } else {
            Ok(CallToolResult::error(vec![Content::text(format!(
                "Element not found: {}",
                req.selector.unwrap_or_default()
            ))]))
        }
    }

    #[tool(
        description = "Pick an option in a <select> by its value attribute. Fires a change event. The visible options are listed in browser_snapshot's `options=[...]` attribute. Defaults to the visible tab."
    )]
    async fn browser_select_option(
        &self,
        Parameters(req): Parameters<SelectOptionRequest>,
    ) -> Result<CallToolResult, McpError> {
        let found = browser_try!(
            self.send_command(|tx| McpCommand::SelectOption {
                tab_id: req.tab_id,
                selector: req.selector.clone(),
                value: req.value.clone(),
                response: tx,
            })
            .await?
        );
        if found {
            Ok(CallToolResult::success(vec![Content::text(format!(
                "Selected option: {}",
                req.value
            ))]))
        } else {
            Ok(CallToolResult::error(vec![Content::text(format!(
                "Select element or option not found: {}",
                req.selector
            ))]))
        }
    }

    // ── History & media ─────────────────────────────────────────────

    #[tool(description = "Browsing history entries, most recent first.")]
    async fn browser_get_history(
        &self,
        Parameters(req): Parameters<GetHistoryRequest>,
    ) -> Result<CallToolResult, McpError> {
        let entries = browser_try!(
            self.send_command(|tx| McpCommand::GetHistory {
                limit: req.limit,
                response: tx,
            })
            .await?
        );
        let json = serde_json::to_string(&entries)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(description = "Tabs currently playing audio or video.")]
    async fn browser_get_playing_tabs(&self) -> Result<CallToolResult, McpError> {
        let tabs = browser_try!(
            self.send_command(|tx| McpCommand::GetPlayingTabs { response: tx })
                .await?
        );
        let json = serde_json::to_string(&tabs)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    // ── Observability ───────────────────────────────────────────────

    #[tool(
        description = "Recent console output of the page: console.log/info/warn/error/debug, uncaught errors, unhandled promise rejections. Returns a JSON array [{level,text,ts}], newest last. Use this to debug why a page or an interaction isn't working — failed JS often explains a dead button. Captured from page load (last 200 kept). Defaults to the visible tab."
    )]
    async fn browser_console_messages(
        &self,
        Parameters(req): Parameters<ConsoleRequest>,
    ) -> Result<CallToolResult, McpError> {
        let limit = req.limit.unwrap_or(50).min(200);
        let filter = match &req.level {
            Some(level) => format!(
                ".filter(function(e){{return e.level==={}}})",
                serde_json::to_string(level).unwrap_or_default()
            ),
            None => String::new(),
        };
        let script =
            format!("JSON.stringify((window.__octoweb_console||[]){filter}.slice(-{limit}))");
        let value = browser_try!(
            self.send_command(|tx| McpCommand::ExecuteJs {
                tab_id: req.tab_id,
                script,
                response: tx,
            })
            .await?
        );
        Ok(CallToolResult::success(vec![Content::text(
            sanitize_json_entries(&value, "text", sanitize::sanitize_text),
        )]))
    }

    #[tool(
        description = "Recent network requests made by the page (fetch + XHR). Returns a JSON array [{method,url,status,type,ms,ts,error?}], newest last. status 0 + error = request failed. Use to debug missing data, failed submissions, or to discover the API behind a UI. Captured from page load (last 200 kept). Defaults to the visible tab."
    )]
    async fn browser_network_requests(
        &self,
        Parameters(req): Parameters<NetworkRequest>,
    ) -> Result<CallToolResult, McpError> {
        let limit = req.limit.unwrap_or(50).min(200);
        let filter = match &req.filter {
            Some(f) => format!(
                ".filter(function(e){{return e.url.indexOf({})!==-1}})",
                serde_json::to_string(f).unwrap_or_default()
            ),
            None => String::new(),
        };
        let script = format!("JSON.stringify((window.__octoweb_net||[]){filter}.slice(-{limit}))");
        let value = browser_try!(
            self.send_command(|tx| McpCommand::ExecuteJs {
                tab_id: req.tab_id,
                script,
                response: tx,
            })
            .await?
        );
        Ok(CallToolResult::success(vec![Content::text(
            sanitize_json_entries(&value, "url", sanitize::sanitize_url),
        )]))
    }

    // ── Dialogs & uploads ───────────────────────────────────────────
    // WKWebView's CompletionHandlerCallChecker requires the completion
    // handler to be invoked before the delegate method returns, so dialogs
    // can't be deferred to an MCP round-trip. Instead the AI ARMS an answer
    // up front; the native handler consumes it synchronously when the dialog
    // fires (dialog_patch.rs).

    #[tool(
        description = "Arm auto-answering for upcoming JS dialogs (alert/confirm/prompt). Call this BEFORE the click that triggers the dialog. The next `count` (default 1) dialogs are answered instantly with no popup; un-armed dialogs show a native popup to the user that you cannot answer. accept=OK, dismiss=Cancel; prompt_text fills a prompt() when accepting. Applies to all tabs."
    )]
    async fn browser_handle_dialog(
        &self,
        Parameters(req): Parameters<HandleDialogRequest>,
    ) -> Result<CallToolResult, McpError> {
        let accept = match req.action.as_str() {
            "accept" => true,
            "dismiss" => false,
            other => {
                return Err(McpError::invalid_params(
                    format!("action must be \"accept\" or \"dismiss\", got \"{other}\""),
                    None,
                ))
            }
        };
        let count = req.count.unwrap_or(1).clamp(1, 20);
        crate::dialog_patch::arm_dialogs(accept, req.prompt_text, count);
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Armed: next {count} dialog(s) will be {}",
            if accept { "accepted" } else { "dismissed" }
        ))]))
    }

    #[tool(
        description = "Arm the NEXT file chooser with these absolute file paths — no panel is shown. Flow: call this first, then browser_click the file input or upload button; the chooser opened by that click receives the files automatically. One-shot (arms exactly one chooser)."
    )]
    async fn browser_upload_file(
        &self,
        Parameters(req): Parameters<UploadFileRequest>,
    ) -> Result<CallToolResult, McpError> {
        if req.paths.is_empty() {
            return Err(McpError::invalid_params(
                "paths must contain at least one file",
                None,
            ));
        }
        for path in &req.paths {
            if !std::path::Path::new(path).is_file() {
                return Err(McpError::invalid_params(
                    format!("File not found: {path}"),
                    None,
                ));
            }
        }
        let count = req.paths.len();
        crate::dialog_patch::arm_upload(req.paths);
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Armed: the next file chooser receives {count} file(s). Now click the file input (browser_click)."
        ))]))
    }
}

/// Decode a wry-JSON-encoded `JSON.stringify` result and scrub one string
/// field of each entry (PAN/token hygiene, parity with TabInfo/PageInfo).
fn sanitize_json_entries(raw: &str, field: &str, scrub: fn(&str) -> String) -> String {
    let inner: String = serde_json::from_str(raw).unwrap_or_else(|_| raw.to_string());
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&inner) else {
        return "[]".into();
    };
    if let Some(entries) = value.as_array_mut() {
        for entry in entries {
            if let Some(s) = entry.get(field).and_then(|x| x.as_str()) {
                entry[field] = serde_json::Value::String(scrub(s));
            }
        }
    }
    value.to_string()
}

#[tool_handler]
impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerInfo {
        tracing::debug!("MCP get_info");

        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::from_build_env())
            .with_protocol_version(ProtocolVersion::V_2024_11_05)
            .with_instructions(
                "Octoweb browser control over MCP. Quick reference:\n\
                \n\
                Discovery: browser_snapshot returns a compact list of interactive elements with @N refs. \
                Pass those refs directly as the `selector` to browser_click / browser_type / browser_hover / \
                browser_press_key / browser_select_option — much cheaper than guessing CSS. Refs invalidate \
                on navigation or full re-render; re-snapshot when interactions start failing with `stale`.\n\
                \n\
                Tabs: browser_navigate ALWAYS works in the background — it opens a new hidden tab \
                (or navigates an existing one via tab_id) and never changes what the user sees. \
                Always pass the returned tab_id to subsequent reads/clicks/navigations so you keep \
                working in your own tab. The ONLY way to show a tab to the user is browser_switch_tab \
                — call it when the task is done or the user asked to see the page. All other tools \
                default to the user's visible tab when tab_id is omitted; to drive that tab, pass the \
                id from browser_get_current_tab. Close background tabs when done.\n\
                \n\
                Reading pages: prefer browser_get_page_content for text and browser_snapshot for \
                actionable elements. Use browser_execute_js only when those don't fit (return value is \
                JSON-encoded; wrap multi-statement code in an IIFE).\n\
                \n\
                Typing & submitting: browser_type sets a value (replaces, doesn't append) and fires \
                input+change. To submit, follow with browser_press_key(key=\"Enter\"). For per-character \
                key events use browser_press_key, not browser_type.\n\
                \n\
                Waiting: browser_navigate already blocks until the page is idle — no follow-up wait needed. \
                Interactions auto-retry for ~2.5s until the element is present, stable, and unobstructed, \
                so you rarely need explicit waits; after clicks that trigger SPA route changes, use \
                browser_wait with a CSS selector for the new content.\n\
                \n\
                Debugging: when a page misbehaves or an interaction has no effect, check \
                browser_console_messages (JS errors) and browser_network_requests (failed calls) before \
                retrying blindly.\n\
                \n\
                Dialogs & uploads: arm BEFORE the triggering click — browser_handle_dialog answers the \
                next alert/confirm/prompt, browser_upload_file feeds the next file chooser. Un-armed \
                dialogs pop up natively for the user and you cannot answer them."
                    .to_string(),
            )
    }
}

// ─────────────────────────────────────────────────────────────────────
// Server spawning (HTTP JSON-RPC)
// ─────────────────────────────────────────────────────────────────────

/// Handle returned from spawn_mcp_server for polling commands.
pub struct McpHandle {
    /// Receiver for commands from MCP tools
    pub command_rx: mpsc::UnboundedReceiver<McpCommand>,
    /// Sender used by the main loop to re-issue read commands from a
    /// watchdog when the first eval's callback was discarded.
    pub command_tx: mpsc::UnboundedSender<McpCommand>,
}

pub fn spawn_mcp_server() -> McpHandle {
    let (command_tx, command_rx) = mpsc::unbounded_channel::<McpCommand>();

    let command_tx_clone = command_tx.clone();
    let command_tx_for_handle = command_tx.clone();
    // Spawn tokio runtime in a separate thread
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
        rt.block_on(async move {
            let server_factory = move || {
                let server = McpServer::new(command_tx_clone.clone());
                Ok(server) as Result<McpServer, std::io::Error>
            };

            // Stateless JSON-RPC mode: no sessions, plain JSON responses.
            // Per MCP Streamable HTTP spec — clients POST JSON-RPC, get JSON back.
            use rmcp::transport::streamable_http_server::tower::StreamableHttpServerConfig;
            let mut config = StreamableHttpServerConfig::default();
            config.json_response = true;
            config.stateful_mode = false;

            let service = StreamableHttpService::new(
                server_factory,
                LocalSessionManager::default().into(),
                config,
            );

            let app = axum::Router::new().nest_service("/mcp", service);

            // OCTOWEB_MCP_PORT override lets a second instance (e2e tests,
            // isolated profiles) run beside the daily browser on 3434.
            let port: u16 = std::env::var("OCTOWEB_MCP_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(3434);
            let listener = match tokio::net::TcpListener::bind(("127.0.0.1", port)).await {
                Ok(l) => l,
                Err(e) => {
                    tracing::warn!(error = %e, port, "MCP HTTP server failed to bind");
                    return;
                }
            };

            tracing::info!("MCP HTTP server listening on http://127.0.0.1:{port}/mcp");

            if let Err(e) = axum::serve(listener, app).await {
                tracing::error!(error = %e, "MCP HTTP server error");
            }
        });
    });

    McpHandle {
        command_rx,
        command_tx: command_tx_for_handle,
    }
}

impl McpHandle {
    /// Poll for pending MCP commands (non-blocking).
    /// Returns None if no commands are pending.
    pub fn poll(&mut self) -> Option<McpCommand> {
        self.command_rx.try_recv().ok()
    }
}
