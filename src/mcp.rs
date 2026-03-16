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
//! - browser_type: Type text into input
//! - browser_go_back: Navigate back in history
//! - browser_go_forward: Navigate forward in history
//! - browser_get_history: Get browsing history
//! - browser_get_playing_tabs: Get tabs playing audio
//! - browser_reload: Reload a tab

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
    /// Click element by selector
    Click {
        tab_id: Option<usize>,
        selector: String,
        response: oneshot::Sender<Result<(), String>>,
    },
    /// Type text into input
    Type {
        tab_id: Option<usize>,
        selector: String,
        text: String,
        response: oneshot::Sender<Result<(), String>>,
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
pub struct GetHistoryRequest {
    #[schemars(
        description = "Maximum number of entries to return (default: 50, most recent first)"
    )]
    pub limit: Option<usize>,
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

#[tool_router]
impl McpServer {
    pub fn new(command_tx: mpsc::UnboundedSender<McpCommand>) -> Self {
        Self {
            state: McpState { command_tx },
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "Navigate to a URL. By default navigates the user's visible tab in-place. Set new_tab=true to open in a new tab (switches to it). Set background=true with new_tab=true to open a hidden background tab without disturbing the user's view — ideal for research. Set tab_id to navigate a specific tab (including background ones) in-place. Returns the tab ID."
    )]
    async fn browser_navigate(
        &self,
        Parameters(req): Parameters<NavigateRequest>,
    ) -> Result<CallToolResult, McpError> {
        let new_tab = req.new_tab.unwrap_or(false);
        let background = req.background.unwrap_or(false);
        tracing::debug!(url = %req.url, ?req.tab_id, new_tab, background, "MCP browser_navigate");

        let (tx, rx) = oneshot::channel();

        self.state
            .command_tx
            .send(McpCommand::Navigate {
                url: req.url,
                tab_id: req.tab_id,
                new_tab,
                background,
                response: tx,
            })
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let result = rx
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        match result {
            Ok(tab_id) => {
                let msg = if background {
                    format!("Opened background tab {tab_id}")
                } else if new_tab {
                    format!("Opened new tab {tab_id}")
                } else {
                    format!("Navigated tab {tab_id}")
                };
                Ok(CallToolResult::success(vec![Content::text(msg)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    #[tool(
        description = "List all open tabs in the browser. Returns array of {id, title, url, is_active, is_playing_audio}. is_active=true marks the tab the user is currently viewing. Use tab IDs from this list to target specific tabs with other tools."
    )]
    async fn browser_get_tabs(&self) -> Result<CallToolResult, McpError> {
        tracing::debug!("MCP browser_get_tabs");

        let (tx, rx) = oneshot::channel();

        self.state
            .command_tx
            .send(McpCommand::GetTabs { response: tx })
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        tracing::debug!("MCP get_tabs command sent");

        let result = rx
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        tracing::debug!(?result, "MCP get_tabs response");

        match result {
            Ok(tabs) => {
                let json = serde_json::to_string(&tabs)
                    .map_err(|e| McpError::internal_error(e.to_string(), None))?;
                Ok(CallToolResult::success(vec![Content::text(json)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    #[tool(
        description = "Switch the user's visible tab to the one with the given ID. This changes what the user sees — only use when the task is done and the user needs to see the result, or they explicitly asked to go somewhere."
    )]
    async fn browser_switch_tab(
        &self,
        Parameters(req): Parameters<SwitchTabRequest>,
    ) -> Result<CallToolResult, McpError> {
        let (tx, rx) = oneshot::channel();

        self.state
            .command_tx
            .send(McpCommand::SwitchTab {
                tab_id: req.tab_id,
                response: tx,
            })
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let result = rx
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        match result {
            Ok(()) => Ok(CallToolResult::success(vec![Content::text(format!(
                "Switched to tab {}",
                req.tab_id
            ))])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    #[tool(
        description = "Close a tab by its ID. If it's the user's visible tab, the next tab becomes visible. Always close background tabs when you're done with them."
    )]
    async fn browser_close_tab(
        &self,
        Parameters(req): Parameters<CloseTabRequest>,
    ) -> Result<CallToolResult, McpError> {
        let (tx, rx) = oneshot::channel();

        self.state
            .command_tx
            .send(McpCommand::CloseTab {
                tab_id: req.tab_id,
                response: tx,
            })
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let result = rx
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        match result {
            Ok(()) => Ok(CallToolResult::success(vec![Content::text(format!(
                "Closed tab {}",
                req.tab_id
            ))])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    #[tool(
        description = "Get metadata about a page: title, URL, and meta description. Defaults to the user's visible tab if tab_id is omitted. Pass tab_id to read any tab including background ones."
    )]
    async fn browser_get_page_info(
        &self,
        Parameters(req): Parameters<TabIdRequest>,
    ) -> Result<CallToolResult, McpError> {
        let (tx, rx) = oneshot::channel();

        self.state
            .command_tx
            .send(McpCommand::GetPageInfo {
                tab_id: req.tab_id,
                response: tx,
            })
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let result = rx
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        match result {
            Ok(info) => {
                let json = serde_json::to_string(&info)
                    .map_err(|e| McpError::internal_error(e.to_string(), None))?;
                Ok(CallToolResult::success(vec![Content::text(json)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    #[tool(
        description = "Execute JavaScript code in a page and return the result as a string. Defaults to the user's visible tab if tab_id is omitted — pass tab_id to target background tabs. For reading page text prefer browser_get_page_content instead."
    )]
    async fn browser_execute_js(
        &self,
        Parameters(req): Parameters<ExecuteJsRequest>,
    ) -> Result<CallToolResult, McpError> {
        let (tx, rx) = oneshot::channel();

        self.state
            .command_tx
            .send(McpCommand::ExecuteJs {
                tab_id: req.tab_id,
                script: req.script,
                response: tx,
            })
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let result = rx
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        match result {
            Ok(value) => Ok(CallToolResult::success(vec![Content::text(value)])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    #[tool(
        description = "Click an element on the page by CSS selector. Defaults to the user's visible tab if tab_id is omitted — pass tab_id to click in background tabs."
    )]
    async fn browser_click(
        &self,
        Parameters(req): Parameters<ClickRequest>,
    ) -> Result<CallToolResult, McpError> {
        let (tx, rx) = oneshot::channel();

        self.state
            .command_tx
            .send(McpCommand::Click {
                tab_id: req.tab_id,
                selector: req.selector,
                response: tx,
            })
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let result = rx
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        match result {
            Ok(()) => Ok(CallToolResult::success(vec![Content::text(
                "Element clicked successfully".to_string(),
            )])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    #[tool(
        description = "Type text into an input element by CSS selector. Defaults to the user's visible tab if tab_id is omitted — pass tab_id to type in background tabs."
    )]
    async fn browser_type(
        &self,
        Parameters(req): Parameters<TypeRequest>,
    ) -> Result<CallToolResult, McpError> {
        let (tx, rx) = oneshot::channel();

        self.state
            .command_tx
            .send(McpCommand::Type {
                tab_id: req.tab_id,
                selector: req.selector,
                text: req.text,
                response: tx,
            })
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let result = rx
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        match result {
            Ok(()) => Ok(CallToolResult::success(vec![Content::text(
                "Text typed successfully".to_string(),
            )])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    #[tool(
        description = "Get the tab the user is currently viewing (the foreground tab). Returns {id, title, url, is_active, is_playing_audio}. Use this to understand what the user sees right now — NOT to get a tab you opened in the background (use browser_get_tabs for that)."
    )]
    async fn browser_get_current_tab(&self) -> Result<CallToolResult, McpError> {
        let (tx, rx) = oneshot::channel();

        self.state
            .command_tx
            .send(McpCommand::GetCurrentTab { response: tx })
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let result = rx
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        match result {
            Ok(tab) => {
                let json = serde_json::to_string(&tab)
                    .map_err(|e| McpError::internal_error(e.to_string(), None))?;
                Ok(CallToolResult::success(vec![Content::text(json)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    #[tool(
        description = "Navigate back in the browser history for a tab. Defaults to the user's visible tab if tab_id is omitted."
    )]
    async fn browser_go_back(
        &self,
        Parameters(req): Parameters<TabIdRequest>,
    ) -> Result<CallToolResult, McpError> {
        let (tx, rx) = oneshot::channel();

        self.state
            .command_tx
            .send(McpCommand::GoBack {
                tab_id: req.tab_id,
                response: tx,
            })
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let result = rx
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        match result {
            Ok(()) => Ok(CallToolResult::success(vec![Content::text(
                "Navigated back".to_string(),
            )])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    #[tool(
        description = "Navigate forward in the browser history for a tab. Defaults to the user's visible tab if tab_id is omitted."
    )]
    async fn browser_go_forward(
        &self,
        Parameters(req): Parameters<TabIdRequest>,
    ) -> Result<CallToolResult, McpError> {
        let (tx, rx) = oneshot::channel();

        self.state
            .command_tx
            .send(McpCommand::GoForward {
                tab_id: req.tab_id,
                response: tx,
            })
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let result = rx
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        match result {
            Ok(()) => Ok(CallToolResult::success(vec![Content::text(
                "Navigated forward".to_string(),
            )])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    #[tool(
        description = "Get browsing history entries, most recent first. Optionally limit the number of results."
    )]
    async fn browser_get_history(
        &self,
        Parameters(req): Parameters<GetHistoryRequest>,
    ) -> Result<CallToolResult, McpError> {
        let (tx, rx) = oneshot::channel();

        self.state
            .command_tx
            .send(McpCommand::GetHistory {
                limit: req.limit,
                response: tx,
            })
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let result = rx
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        match result {
            Ok(entries) => {
                let json = serde_json::to_string(&entries)
                    .map_err(|e| McpError::internal_error(e.to_string(), None))?;
                Ok(CallToolResult::success(vec![Content::text(json)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    #[tool(
        description = "Get list of tabs currently playing audio/video. Returns array of {id, title, url, is_active, is_playing_audio}."
    )]
    async fn browser_get_playing_tabs(&self) -> Result<CallToolResult, McpError> {
        let (tx, rx) = oneshot::channel();

        self.state
            .command_tx
            .send(McpCommand::GetPlayingTabs { response: tx })
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let result = rx
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        match result {
            Ok(tabs) => {
                let json = serde_json::to_string(&tabs)
                    .map_err(|e| McpError::internal_error(e.to_string(), None))?;
                Ok(CallToolResult::success(vec![Content::text(json)]))
            }
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    #[tool(description = "Reload a tab. Defaults to the user's visible tab if tab_id is omitted.")]
    async fn browser_reload(
        &self,
        Parameters(req): Parameters<TabIdRequest>,
    ) -> Result<CallToolResult, McpError> {
        let (tx, rx) = oneshot::channel();

        self.state
            .command_tx
            .send(McpCommand::Reload {
                tab_id: req.tab_id,
                response: tx,
            })
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let result = rx
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        match result {
            Ok(()) => Ok(CallToolResult::success(vec![Content::text(
                "Page reloaded".to_string(),
            )])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }

    #[tool(
        description = "Get the readable text content of a page (innerText). Use for reading articles, search results, or extracting data. Defaults to the user's visible tab if tab_id is omitted — pass tab_id to read background tabs."
    )]
    async fn browser_get_page_content(
        &self,
        Parameters(req): Parameters<TabIdRequest>,
    ) -> Result<CallToolResult, McpError> {
        let (tx, rx) = oneshot::channel();

        self.state
            .command_tx
            .send(McpCommand::GetPageContent {
                tab_id: req.tab_id,
                response: tx,
            })
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        let result = rx
            .await
            .map_err(|e| McpError::internal_error(e.to_string(), None))?;

        match result {
            Ok(content) => Ok(CallToolResult::success(vec![Content::text(content)])),
            Err(e) => Ok(CallToolResult::error(vec![Content::text(e)])),
        }
    }
}

#[tool_handler]
impl ServerHandler for McpServer {
    fn get_info(&self) -> ServerInfo {
        tracing::debug!("MCP get_info");

        ServerInfo::new(
            ServerCapabilities::builder()
                .enable_tools()
                .build(),
        )
        .with_server_info(Implementation::from_build_env())
        .with_protocol_version(ProtocolVersion::V_2024_11_05)
        .with_instructions("Octoweb browser control. Tools that accept tab_id default to the user's visible (foreground) tab when omitted. Always pass tab_id explicitly when working with background tabs. Use browser_navigate with new_tab+background to open tabs the user doesn't see.".to_string())
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
            let config = StreamableHttpServerConfig {
                json_response: true,
                stateful_mode: false,
                ..Default::default()
            };

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
