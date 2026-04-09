//! MCP (Model Context Protocol) server for AI assistant control.
//!
//! Exposes browser tools over HTTP JSON-RPC on localhost:3434/mcp:
//! - browser_navigate: Open URL in current tab, new tab, or background tab
//! - browser_get_tabs: List all open tabs
//! - browser_get_current_tab: Get the active tab
//! - browser_switch_tab: Switch to tab by ID
//! - browser_close_tab: Close tab by ID
//! - browser_get_page_info: Get title, URL, description
//! - browser_get_page_content: Get readable text content of a page
//! - browser_execute_js: Run JavaScript in page (returns result)
//! - browser_click: Click element by selector
//! - browser_hover: Hover over element by selector
//! - browser_type: Type text into input or contenteditable element
//! - browser_scroll: Scroll page up/down/top/bottom
//! - browser_press_key: Press a keyboard key
//! - browser_select_option: Select an option in a dropdown
//! - browser_wait: Wait for page load or element to appear
//! - browser_go_back: Navigate back in history
//! - browser_go_forward: Navigate forward in history
//! - browser_get_history: Get browsing history
//! - browser_get_playing_tabs: Get tabs playing audio
//! - browser_reload: Reload a tab
//! - browser_screenshot: Take a screenshot
//! - browser_get_html: Get HTML content of page or element
//! - browser_snapshot: Get compact element map with numeric refs for efficient interaction
//! - browser_handle_dialog: Accept/dismiss JS alert/confirm/prompt dialogs

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
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
    /// Navigate to URL in a specific tab, new tab, or background tab
    Navigate {
        url: String,
        tab_id: Option<usize>,
        new_tab: bool,
        background: bool,
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
    },
    /// Take a screenshot of a tab (viewport or full page).
    /// Ok variant carries base64-encoded PNG data.
    Screenshot {
        tab_id: Option<usize>,
        full_page: bool,
        response: oneshot::Sender<Result<String, String>>,
    },
    /// Scroll the page
    Scroll {
        tab_id: Option<usize>,
        direction: String,
        pixels: Option<i32>,
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
    /// Get HTML content of page or a specific element
    GetHtml {
        tab_id: Option<usize>,
        selector: Option<String>,
        response: oneshot::Sender<Result<String, String>>,
    },
    /// Take a snapshot of interactive elements on the page
    Snapshot {
        tab_id: Option<usize>,
        response: oneshot::Sender<Result<String, String>>,
    },
    /// Handle (accept/dismiss) a pending JS dialog
    HandleDialog {
        accept: bool,
        text: Option<String>,
        response: oneshot::Sender<Result<String, String>>,
    },
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

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct PageInfo {
    pub title: String,
    pub url: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct HistoryInfo {
    pub title: String,
    pub url: String,
    /// Unix timestamp (seconds) of when the page was visited
    pub visited_at: u64,
}

// ─────────────────────────────────────────────────────────────────────
// MCP Tool Request Types
// ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct NavigateRequest {
    #[schemars(description = "URL to navigate to")]
    pub url: String,
    #[schemars(
        description = "Tab ID to navigate in-place. Omit to use active tab (when new_tab is false) or create a new tab (when new_tab is true)"
    )]
    pub tab_id: Option<usize>,
    #[schemars(
        description = "Open in a new tab instead of navigating the current one (default: false)"
    )]
    pub new_tab: Option<bool>,
    #[schemars(
        description = "Keep the new tab hidden in the background — user stays on their current page. Only applies when new_tab is true. (default: false)"
    )]
    pub background: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TabIdRequest {
    #[schemars(
        description = "Tab ID to target. Omit to default to the user's visible (foreground) tab."
    )]
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
    #[schemars(
        description = "Tab ID to target. Omit to default to the user's visible (foreground) tab."
    )]
    pub tab_id: Option<usize>,
    #[schemars(description = "JavaScript code to execute")]
    pub script: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ClickRequest {
    #[schemars(
        description = "Tab ID to target. Omit to default to the user's visible (foreground) tab."
    )]
    pub tab_id: Option<usize>,
    #[schemars(description = "CSS selector for element to click")]
    pub selector: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct HoverRequest {
    #[schemars(
        description = "Tab ID to target. Omit to default to the user's visible (foreground) tab."
    )]
    pub tab_id: Option<usize>,
    #[schemars(description = "CSS selector for element to hover over")]
    pub selector: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct TypeRequest {
    #[schemars(
        description = "Tab ID to target. Omit to default to the user's visible (foreground) tab."
    )]
    pub tab_id: Option<usize>,
    #[schemars(description = "CSS selector for input element")]
    pub selector: String,
    #[schemars(description = "Text to type")]
    pub text: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ScreenshotRequest {
    #[schemars(
        description = "Tab ID to target. Omit to default to the user's visible (foreground) tab."
    )]
    pub tab_id: Option<usize>,
    #[schemars(
        description = "Capture the entire scrollable page instead of just the visible viewport. (default: false)"
    )]
    pub full_page: Option<bool>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetHistoryRequest {
    #[schemars(
        description = "Maximum number of entries to return (default: 50, most recent first)"
    )]
    pub limit: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ScrollRequest {
    #[schemars(
        description = "Tab ID to target. Omit to default to the user's visible (foreground) tab."
    )]
    pub tab_id: Option<usize>,
    #[schemars(
        description = "Scroll direction: \"up\", \"down\", \"top\" (page start), or \"bottom\" (page end)"
    )]
    pub direction: String,
    #[schemars(
        description = "Custom scroll distance in pixels. Overrides direction's default (~one viewport). Only used with \"up\" and \"down\"."
    )]
    pub pixels: Option<i32>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct PressKeyRequest {
    #[schemars(
        description = "Tab ID to target. Omit to default to the user's visible (foreground) tab."
    )]
    pub tab_id: Option<usize>,
    #[schemars(
        description = "Key to press — standard KeyboardEvent.key values: \"Enter\", \"Escape\", \"Tab\", \"ArrowDown\", \"Backspace\", \"a\", \"1\", etc."
    )]
    pub key: String,
    #[schemars(
        description = "CSS selector for element to target. Omit to use the currently focused element."
    )]
    pub selector: Option<String>,
    #[schemars(
        description = "Modifier keys to hold: \"shift\", \"ctrl\", \"alt\", \"meta\". Example: [\"ctrl\", \"shift\"]"
    )]
    pub modifiers: Option<Vec<String>>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct WaitRequest {
    #[schemars(
        description = "Tab ID to target. Omit to default to the user's visible (foreground) tab."
    )]
    pub tab_id: Option<usize>,
    #[schemars(
        description = "What to wait for: \"load\" (page fully loaded, default), \"domcontentloaded\" (DOM ready), or a CSS selector string (waits for that element to appear in DOM)"
    )]
    pub event: Option<String>,
    #[schemars(description = "Maximum time to wait in milliseconds (default: 10000, max: 30000)")]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetHtmlRequest {
    #[schemars(
        description = "Tab ID to target. Omit to default to the user's visible (foreground) tab."
    )]
    pub tab_id: Option<usize>,
    #[schemars(
        description = "CSS selector for a specific element. Returns that element's outerHTML. Omit to get the full page HTML with scripts and styles removed."
    )]
    pub selector: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SnapshotRequest {
    #[schemars(
        description = "Tab ID to target. Omit to default to the user's visible (foreground) tab."
    )]
    pub tab_id: Option<usize>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct HandleDialogRequest {
    #[schemars(description = "true to accept (OK/Yes), false to dismiss (Cancel/No)")]
    pub accept: bool,
    #[schemars(description = "Text to enter for prompt() dialogs. Ignored for alert/confirm.")]
    pub text: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct SelectOptionRequest {
    #[schemars(
        description = "Tab ID to target. Omit to default to the user's visible (foreground) tab."
    )]
    pub tab_id: Option<usize>,
    #[schemars(description = "CSS selector for the <select> element")]
    pub selector: String,
    #[schemars(description = "Value of the <option> to select")]
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
    tool_router: ToolRouter<Self>,
}

/// Helpers (separate impl block so the #[tool_router] macro doesn't interfere).
impl McpServer {
    /// Send a command to the main event loop and wait for the response (30 s timeout).
    async fn send_command<T>(
        &self,
        build: impl FnOnce(oneshot::Sender<Result<T, String>>) -> McpCommand,
    ) -> Result<T, McpError> {
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
            .map_err(|e| McpError::internal_error(e.to_string(), None))?
            .map_err(|e| McpError::internal_error(e, None))
    }
}

#[tool_router]
impl McpServer {
    pub fn new(command_tx: mpsc::UnboundedSender<McpCommand>) -> Self {
        Self {
            state: McpState { command_tx },
            tool_router: Self::tool_router(),
        }
    }

    // ── Navigation ──────────────────────────────────────────────────

    #[tool(
        description = "Navigate to a URL and wait for the page to fully load (including SPA rendering). Blocks until DOM and network are idle — no need to call browser_wait afterwards. By default navigates the user's visible tab in-place. Set new_tab=true to open in a new tab (switches to it). Set background=true with new_tab=true to open a hidden background tab without disturbing the user's view — ideal for research. Set tab_id to navigate a specific tab (including background ones) in-place. Returns the tab ID."
    )]
    async fn browser_navigate(
        &self,
        Parameters(req): Parameters<NavigateRequest>,
    ) -> Result<CallToolResult, McpError> {
        let new_tab = req.new_tab.unwrap_or(false);
        let background = req.background.unwrap_or(false);
        tracing::debug!(url = %req.url, ?req.tab_id, new_tab, background, "MCP browser_navigate");

        let tab_id = self
            .send_command(|tx| McpCommand::Navigate {
                url: req.url,
                tab_id: req.tab_id,
                new_tab,
                background,
                response: tx,
            })
            .await?;

        let msg = if background {
            format!("Opened background tab {tab_id}")
        } else if new_tab {
            format!("Opened new tab {tab_id}")
        } else {
            format!("Navigated tab {tab_id}")
        };
        Ok(CallToolResult::success(vec![Content::text(msg)]))
    }

    #[tool(
        description = "Navigate back in the browser history for a tab. Defaults to the user's visible tab if tab_id is omitted."
    )]
    async fn browser_go_back(
        &self,
        Parameters(req): Parameters<TabIdRequest>,
    ) -> Result<CallToolResult, McpError> {
        self.send_command(|tx| McpCommand::GoBack {
            tab_id: req.tab_id,
            response: tx,
        })
        .await?;
        Ok(CallToolResult::success(vec![Content::text(
            "Navigated back",
        )]))
    }

    #[tool(
        description = "Navigate forward in the browser history for a tab. Defaults to the user's visible tab if tab_id is omitted."
    )]
    async fn browser_go_forward(
        &self,
        Parameters(req): Parameters<TabIdRequest>,
    ) -> Result<CallToolResult, McpError> {
        self.send_command(|tx| McpCommand::GoForward {
            tab_id: req.tab_id,
            response: tx,
        })
        .await?;
        Ok(CallToolResult::success(vec![Content::text(
            "Navigated forward",
        )]))
    }

    #[tool(description = "Reload a tab. Defaults to the user's visible tab if tab_id is omitted.")]
    async fn browser_reload(
        &self,
        Parameters(req): Parameters<TabIdRequest>,
    ) -> Result<CallToolResult, McpError> {
        self.send_command(|tx| McpCommand::Reload {
            tab_id: req.tab_id,
            response: tx,
        })
        .await?;
        Ok(CallToolResult::success(vec![Content::text(
            "Page reloaded",
        )]))
    }

    #[tool(
        description = "Wait for a condition before proceeding. Not needed after browser_navigate (which already waits). Useful after browser_click that triggers SPA route changes, or to wait for lazily-loaded content. event=\"load\" (default) waits for full page load, \"domcontentloaded\" waits for DOM ready, or pass a CSS selector to wait for a specific element to appear. Returns \"ready\" on success or \"timeout\" if the condition wasn't met. Defaults to the user's visible tab if tab_id is omitted."
    )]
    async fn browser_wait(
        &self,
        Parameters(req): Parameters<WaitRequest>,
    ) -> Result<CallToolResult, McpError> {
        let event = req.event.unwrap_or_else(|| "load".to_string());
        let timeout_ms = req.timeout_ms.unwrap_or(10_000).min(30_000);
        let result = self
            .send_command(|tx| McpCommand::Wait {
                tab_id: req.tab_id,
                event,
                timeout_ms,
                response: tx,
            })
            .await?;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }

    // ── Tab management ──────────────────────────────────────────────

    #[tool(
        description = "List all open tabs in the browser. Returns array of {id, title, url, is_active, is_playing_audio}. is_active=true marks the tab the user is currently viewing. Use tab IDs from this list to target specific tabs with other tools."
    )]
    async fn browser_get_tabs(&self) -> Result<CallToolResult, McpError> {
        let tabs = self
            .send_command(|tx| McpCommand::GetTabs { response: tx })
            .await?;
        let json = serde_json::to_string(&tabs)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Get the tab the user is currently viewing (the foreground tab). Returns {id, title, url, is_active, is_playing_audio}. Use this to understand what the user sees right now — NOT to get a tab you opened in the background (use browser_get_tabs for that)."
    )]
    async fn browser_get_current_tab(&self) -> Result<CallToolResult, McpError> {
        let tab = self
            .send_command(|tx| McpCommand::GetCurrentTab { response: tx })
            .await?;
        let json = serde_json::to_string(&tab)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Switch the user's visible tab to the one with the given ID. This changes what the user sees — only use when the task is done and the user needs to see the result, or they explicitly asked to go somewhere."
    )]
    async fn browser_switch_tab(
        &self,
        Parameters(req): Parameters<SwitchTabRequest>,
    ) -> Result<CallToolResult, McpError> {
        self.send_command(|tx| McpCommand::SwitchTab {
            tab_id: req.tab_id,
            response: tx,
        })
        .await?;
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Switched to tab {}",
            req.tab_id
        ))]))
    }

    #[tool(
        description = "Close a tab by its ID. If it's the user's visible tab, the next tab becomes visible. Always close background tabs when you're done with them."
    )]
    async fn browser_close_tab(
        &self,
        Parameters(req): Parameters<CloseTabRequest>,
    ) -> Result<CallToolResult, McpError> {
        self.send_command(|tx| McpCommand::CloseTab {
            tab_id: req.tab_id,
            response: tx,
        })
        .await?;
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Closed tab {}",
            req.tab_id
        ))]))
    }

    // ── Page content ────────────────────────────────────────────────

    #[tool(
        description = "Get metadata about a page: title, URL, and meta description. Defaults to the user's visible tab if tab_id is omitted. Pass tab_id to read any tab including background ones."
    )]
    async fn browser_get_page_info(
        &self,
        Parameters(req): Parameters<TabIdRequest>,
    ) -> Result<CallToolResult, McpError> {
        let info = self
            .send_command(|tx| McpCommand::GetPageInfo {
                tab_id: req.tab_id,
                response: tx,
            })
            .await?;
        let json = serde_json::to_string(&info)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Get the readable text content of a page (innerText). Use for reading articles, search results, or extracting data. Defaults to the user's visible tab if tab_id is omitted — pass tab_id to read background tabs."
    )]
    async fn browser_get_page_content(
        &self,
        Parameters(req): Parameters<TabIdRequest>,
    ) -> Result<CallToolResult, McpError> {
        let content = self
            .send_command(|tx| McpCommand::GetPageContent {
                tab_id: req.tab_id,
                response: tx,
            })
            .await?;
        Ok(CallToolResult::success(vec![Content::text(content)]))
    }

    #[tool(
        description = "Get HTML content of a page or a specific element. Without selector: returns the page's HTML with scripts and styles stripped — useful for understanding page structure, forms, tables, and element attributes. With selector: returns the matching element's outerHTML. Use browser_get_page_content for plain text, this tool for structure. Defaults to the user's visible tab if tab_id is omitted."
    )]
    async fn browser_get_html(
        &self,
        Parameters(req): Parameters<GetHtmlRequest>,
    ) -> Result<CallToolResult, McpError> {
        let html = self
            .send_command(|tx| McpCommand::GetHtml {
                tab_id: req.tab_id,
                selector: req.selector,
                response: tx,
            })
            .await?;
        Ok(CallToolResult::success(vec![Content::text(html)]))
    }

    #[tool(
        description = "Get a compact map of all interactive elements on the page — links, buttons, inputs, selects, etc. Each element is assigned a @ref number (e.g. @1, @2) that you can pass directly as the selector to browser_click, browser_type, browser_hover, and other interaction tools. This is the most efficient way to discover what's on a page and interact with it. Includes elements inside same-origin iframes. Call this before interacting with a page you haven't seen yet. Defaults to the user's visible tab if tab_id is omitted."
    )]
    async fn browser_snapshot(
        &self,
        Parameters(req): Parameters<SnapshotRequest>,
    ) -> Result<CallToolResult, McpError> {
        let snapshot = self
            .send_command(|tx| McpCommand::Snapshot {
                tab_id: req.tab_id,
                response: tx,
            })
            .await?;
        Ok(CallToolResult::success(vec![Content::text(snapshot)]))
    }

    #[tool(
        description = "Execute JavaScript code in a page and return the result as a string. Defaults to the user's visible tab if tab_id is omitted — pass tab_id to target background tabs. For reading page text prefer browser_get_page_content instead."
    )]
    async fn browser_execute_js(
        &self,
        Parameters(req): Parameters<ExecuteJsRequest>,
    ) -> Result<CallToolResult, McpError> {
        let value = self
            .send_command(|tx| McpCommand::ExecuteJs {
                tab_id: req.tab_id,
                script: req.script,
                response: tx,
            })
            .await?;
        Ok(CallToolResult::success(vec![Content::text(value)]))
    }

    #[tool(
        description = "Take a screenshot of a tab's web page and copy it to the clipboard so the user can paste it anywhere. Set full_page to true to capture the entire scrollable page (not just the visible viewport). Defaults to the user's visible tab if tab_id is omitted."
    )]
    async fn browser_screenshot(
        &self,
        Parameters(req): Parameters<ScreenshotRequest>,
    ) -> Result<CallToolResult, McpError> {
        let b64_png = self
            .send_command(|tx| McpCommand::Screenshot {
                tab_id: req.tab_id,
                full_page: req.full_page.unwrap_or(false),
                response: tx,
            })
            .await?;
        Ok(CallToolResult::success(vec![Content::image(
            b64_png,
            "image/png",
        )]))
    }

    // ── Interaction ─────────────────────────────────────────────────

    #[tool(
        description = "Click an element on the page by CSS selector or @ref from browser_snapshot. Returns whether the element was found and clicked. Defaults to the user's visible tab if tab_id is omitted — pass tab_id to click in background tabs."
    )]
    async fn browser_click(
        &self,
        Parameters(req): Parameters<ClickRequest>,
    ) -> Result<CallToolResult, McpError> {
        let found = self
            .send_command(|tx| McpCommand::Click {
                tab_id: req.tab_id,
                selector: req.selector.clone(),
                response: tx,
            })
            .await?;
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
        description = "Hover over an element on the page by CSS selector (or @ref from browser_snapshot). Triggers mouseenter, mouseover, and mousemove JS events — works for JS-based menus (Bootstrap, React, Material UI) but cannot activate pure CSS :hover pseudo-class (WebKit limitation). Returns whether the element was found. Defaults to the user's visible tab if tab_id is omitted."
    )]
    async fn browser_hover(
        &self,
        Parameters(req): Parameters<HoverRequest>,
    ) -> Result<CallToolResult, McpError> {
        let found = self
            .send_command(|tx| McpCommand::Hover {
                tab_id: req.tab_id,
                selector: req.selector.clone(),
                response: tx,
            })
            .await?;
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
        description = "Type text into an element by CSS selector or @ref from browser_snapshot. Replaces the current value — does not append. Works with <input>, <textarea>, and contenteditable elements (rich text editors, Gmail compose, etc.). Focuses the element, sets its value, and fires input+change events. Returns whether the element was found. Defaults to the user's visible tab if tab_id is omitted — pass tab_id to type in background tabs."
    )]
    async fn browser_type(
        &self,
        Parameters(req): Parameters<TypeRequest>,
    ) -> Result<CallToolResult, McpError> {
        let found = self
            .send_command(|tx| McpCommand::Type {
                tab_id: req.tab_id,
                selector: req.selector.clone(),
                text: req.text,
                response: tx,
            })
            .await?;
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
        description = "Scroll the page in a tab. Use \"up\"/\"down\" to scroll by roughly one viewport, \"top\"/\"bottom\" to jump to page start/end. Optionally set pixels for a custom scroll distance with \"up\"/\"down\". Defaults to the user's visible tab if tab_id is omitted."
    )]
    async fn browser_scroll(
        &self,
        Parameters(req): Parameters<ScrollRequest>,
    ) -> Result<CallToolResult, McpError> {
        self.send_command(|tx| McpCommand::Scroll {
            tab_id: req.tab_id,
            direction: req.direction.clone(),
            pixels: req.pixels,
            response: tx,
        })
        .await?;
        Ok(CallToolResult::success(vec![Content::text(format!(
            "Scrolled {}",
            req.direction
        ))]))
    }

    #[tool(
        description = "Press a keyboard key, optionally with modifiers. Use for form submission (Enter), closing dialogs (Escape), moving focus (Tab), or any keyboard interaction. If selector is provided (CSS selector or @ref from browser_snapshot), focuses that element first. Returns whether the target element was found (always true when no selector is given). Defaults to the user's visible tab if tab_id is omitted."
    )]
    async fn browser_press_key(
        &self,
        Parameters(req): Parameters<PressKeyRequest>,
    ) -> Result<CallToolResult, McpError> {
        let found = self
            .send_command(|tx| McpCommand::PressKey {
                tab_id: req.tab_id,
                key: req.key.clone(),
                selector: req.selector.clone(),
                modifiers: req.modifiers.unwrap_or_default(),
                response: tx,
            })
            .await?;
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
        description = "Select an option in a <select> dropdown by its value. Accepts CSS selector or @ref from browser_snapshot. Dispatches a change event after selection. Returns whether the <select> element and the matching option were found. Defaults to the user's visible tab if tab_id is omitted."
    )]
    async fn browser_select_option(
        &self,
        Parameters(req): Parameters<SelectOptionRequest>,
    ) -> Result<CallToolResult, McpError> {
        let found = self
            .send_command(|tx| McpCommand::SelectOption {
                tab_id: req.tab_id,
                selector: req.selector.clone(),
                value: req.value.clone(),
                response: tx,
            })
            .await?;
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

    #[tool(
        description = "Get browsing history entries, most recent first. Optionally limit the number of results."
    )]
    async fn browser_get_history(
        &self,
        Parameters(req): Parameters<GetHistoryRequest>,
    ) -> Result<CallToolResult, McpError> {
        let entries = self
            .send_command(|tx| McpCommand::GetHistory {
                limit: req.limit,
                response: tx,
            })
            .await?;
        let json = serde_json::to_string(&entries)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    #[tool(
        description = "Get list of tabs currently playing audio/video. Returns array of {id, title, url, is_active, is_playing_audio}."
    )]
    async fn browser_get_playing_tabs(&self) -> Result<CallToolResult, McpError> {
        let tabs = self
            .send_command(|tx| McpCommand::GetPlayingTabs { response: tx })
            .await?;
        let json = serde_json::to_string(&tabs)
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;
        Ok(CallToolResult::success(vec![Content::text(json)]))
    }

    // ── Dialogs ────────────────────────────────────────────────────

    #[tool(
        description = "Handle a pending JavaScript dialog (alert, confirm, or prompt). JS dialogs block the page until resolved. Set accept=true to click OK/Yes, false to click Cancel/No. For prompt() dialogs, provide the text to enter. Returns the dialog type and message. Returns an error if no dialog is currently pending."
    )]
    async fn browser_handle_dialog(
        &self,
        Parameters(req): Parameters<HandleDialogRequest>,
    ) -> Result<CallToolResult, McpError> {
        let result = self
            .send_command(|tx| McpCommand::HandleDialog {
                accept: req.accept,
                text: req.text,
                response: tx,
            })
            .await?;
        Ok(CallToolResult::success(vec![Content::text(result)]))
    }
}

#[tool_handler]
impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerInfo {
        tracing::debug!("MCP get_info");

        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::from_build_env())
            .with_protocol_version(ProtocolVersion::V_2024_11_05)
            .with_instructions("Octoweb browser control. Start with browser_snapshot to discover interactive elements — it returns @ref numbers you can pass as selectors to click/type/hover. Tools that accept tab_id default to the user's visible (foreground) tab when omitted. Always pass tab_id explicitly when working with background tabs. Use browser_navigate with new_tab+background to open tabs the user doesn't see. Use browser_handle_dialog to respond to JS alert/confirm/prompt dialogs that block the page.".to_string())
    }
}

// ─────────────────────────────────────────────────────────────────────
// Server spawning (HTTP JSON-RPC)
// ─────────────────────────────────────────────────────────────────────

/// Handle returned from spawn_mcp_server for polling commands.
pub struct McpHandle {
    /// Receiver for commands from MCP tools
    pub command_rx: mpsc::UnboundedReceiver<McpCommand>,
}

pub fn spawn_mcp_server() -> McpHandle {
    let (command_tx, command_rx) = mpsc::unbounded_channel::<McpCommand>();

    let command_tx_clone = command_tx.clone();
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

            let listener = match tokio::net::TcpListener::bind("127.0.0.1:3434").await {
                Ok(l) => l,
                Err(e) => {
                    tracing::warn!(error = %e, "MCP HTTP server failed to bind");
                    return;
                }
            };

            tracing::info!("MCP HTTP server listening on http://127.0.0.1:3434/mcp");

            if let Err(e) = axum::serve(listener, app).await {
                tracing::error!(error = %e, "MCP HTTP server error");
            }
        });
    });

    McpHandle { command_rx }
}

impl McpHandle {
    /// Poll for pending MCP commands (non-blocking).
    /// Returns None if no commands are pending.
    pub fn poll(&mut self) -> Option<McpCommand> {
        self.command_rx.try_recv().ok()
    }
}
