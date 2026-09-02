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
        response: oneshot::Sender<Result<NavigateOutcome, String>>,
    },
    /// Get list of open tabs, most recently active first
    GetTabs {
        limit: usize,
        query: Option<String>,
        response: oneshot::Sender<Result<TabsPage, String>>,
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
        /// Watchdog ceiling; the script itself is not interrupted.
        timeout_ms: u64,
        response: oneshot::Sender<Result<String, String>>,
    },
    /// Click element by selector — Ok carries the effect summary
    Click {
        tab_id: Option<usize>,
        selector: String,
        expect: Option<String>,
        response: oneshot::Sender<Result<String, String>>,
    },
    /// Hover over element by selector — Ok carries the effect summary
    Hover {
        tab_id: Option<usize>,
        selector: String,
        expect: Option<String>,
        response: oneshot::Sender<Result<String, String>>,
    },
    /// Type text into input — Ok carries the effect summary
    Type {
        tab_id: Option<usize>,
        selector: String,
        text: String,
        expect: Option<String>,
        response: oneshot::Sender<Result<String, String>>,
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
    /// Press a keyboard key — Ok carries the effect summary
    PressKey {
        tab_id: Option<usize>,
        key: String,
        selector: Option<String>,
        modifiers: Vec<String>,
        expect: Option<String>,
        response: oneshot::Sender<Result<String, String>>,
    },
    /// Wait for page load or element to appear
    Wait {
        tab_id: Option<usize>,
        event: String,
        timeout_ms: u64,
        response: oneshot::Sender<Result<String, String>>,
    },
    /// Select an option in a <select> dropdown — Ok carries the effect summary
    SelectOption {
        tab_id: Option<usize>,
        selector: String,
        value: String,
        expect: Option<String>,
        response: oneshot::Sender<Result<String, String>>,
    },
    /// Find and dismiss a cookie/consent/newsletter overlay (trusted click on
    /// the reject/close control; never auto-accepts).
    DismissOverlay {
        tab_id: Option<usize>,
        response: oneshot::Sender<Result<String, String>>,
    },
    /// Take a snapshot of interactive elements on the page
    Snapshot {
        tab_id: Option<usize>,
        /// CSS selector or @ref to scope the scan to one container.
        within: Option<String>,
        /// Only report elements changed since the last snapshot of this tab.
        diff: bool,
        response: oneshot::Sender<Result<String, String>>,
        /// Internal: see `GetPageContent::is_retry`.
        is_retry: bool,
    },
}

// ─────────────────────────────────────────────────────────────────────
// DOM-result interpreter (shared by Click/Hover/Type/PressKey/SelectOption)
// ─────────────────────────────────────────────────────────────────────

/// Map a status string returned by our injected DOM scripts into a
/// `Result<String, String>`: Ok carries the effect summary (empty when the
/// script reported plain `true`), Err an actionable message for the AI.
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
pub fn interpret_dom_result(val: &str, selector: &str) -> Result<String, String> {
    // Scripts resolve to a JS string; async_eval hands it over JSON-encoded.
    let inner: String = serde_json::from_str(val).unwrap_or_else(|_| val.to_string());
    let trimmed = inner.trim();
    if let Some(effect) = trimmed.strip_prefix("true|") {
        return Ok(format_effect(effect));
    }
    match trimmed {
        "true" => Ok(String::new()),
        status => Err(dom_status_error(status, selector)),
    }
}

/// Actionable message for a harness failure status (see `interpret_dom_result`).
pub fn dom_status_error(status: &str, selector: &str) -> String {
    match status {
        "stale" => format!(
            "@ref '{selector}' is stale — call browser_snapshot first to refresh refs (they invalidate on navigation)"
        ),
        "missing" => format!(
            "No element matched selector: {selector} (retried for {}s)",
            crate::dom_actions::RETRY_MS / 1000
        ),
        "detached" => format!(
            "Element for '{selector}' is no longer in the DOM — re-snapshot or wait for the page to settle"
        ),
        "disabled" => format!(
            "Element '{selector}' stayed disabled/readonly — wait for the page to enable it or pick another element"
        ),
        "noteditable" => format!(
            "Element '{selector}' is not a text field — it is not an <input>, <textarea>, or contenteditable. \
             Pick the editable element itself (in browser_snapshot it shows as a 'textbox'/contenteditable @ref); \
             avoid placeholder/label nodes that overlay the real editor"
        ),
        "typefailed" => format!(
            "Could not insert text into '{selector}' — the editor accepted neither a synthetic paste nor an editing command. \
             It may require a real click to focus first (try browser_click on it, then browser_type), or it is a custom editor that needs key-by-key input"
        ),
        s if s.starts_with("occluded:") => format!(
            "Element '{selector}' is covered by {} — dismiss that overlay (cookie banner, modal, dropdown) first, or scroll it away, then retry",
            &s[9..]
        ),
        s if s.starts_with("invalid:") => format!(
            "Invalid CSS selector '{selector}': {}",
            &s[8..]
        ),
        other => format!("Unexpected DOM result: {other}"),
    }
}

/// Render an effect diff (`window.__octoweb_pre.diff()` JSON) as the
/// one-line suffix appended to action results. Only what changed is listed;
/// an empty diff says so explicitly — "nothing observable happened" is the
/// signal the AI needs to stop re-clicking and look at console/network.
pub fn format_effect(payload_json: &str) -> String {
    format_effect_with_download(payload_json, None)
}

/// [`format_effect`] plus a download the action triggered (tracked natively —
/// a save-to-disk leaves no trace in the DOM, so without this a working
/// "Download" click would read as "no observable change").
///
/// `payload_json` is either the settle shape `{diff,met,expect}` (actions with
/// effect capture) or a bare diff object (legacy callers). An expectation that
/// was checked is surfaced first — `✓ met` / `✗ NOT met` — since it's the
/// answer the agent asked for.
pub fn format_effect_with_download(payload_json: &str, download: Option<&str>) -> String {
    let parsed = serde_json::from_str::<serde_json::Value>(payload_json).ok();
    // Settle shape carries the diff under "diff"; otherwise treat the whole
    // object as the diff.
    let (diff, met, expect) = match parsed {
        Some(serde_json::Value::Object(o)) if o.contains_key("diff") => (
            o.get("diff").and_then(|d| d.as_object().cloned()),
            o.get("met").and_then(|m| m.as_bool()),
            o.get("expect").and_then(|e| e.as_str()).map(str::to_string),
        ),
        Some(serde_json::Value::Object(o)) => (Some(o), None, None),
        _ => (None, None, None),
    };
    let Some(obj) = diff else {
        return match download {
            Some(name) => format!(" → download started: {name} (saved to ~/Downloads)"),
            None => String::new(),
        };
    };
    let mut parts: Vec<String> = Vec::new();
    match (&expect, met) {
        (Some(e), Some(true)) => parts.push(format!("✓ expected {e} — met")),
        (Some(e), Some(false)) => parts.push(format!(
            "✗ expected {e} — NOT met within {}s",
            crate::dom_actions::SETTLE_MS / 1000
        )),
        _ => {}
    }
    if let Some(name) = download {
        parts.push(format!("download started: {name} (saved to ~/Downloads)"));
    }
    let str_of = |k: &str| obj.get(k).and_then(|x| x.as_str()).map(|s| s.to_string());
    if let Some(u) = str_of("url") {
        parts.push(format!("url → {u}"));
    }
    if let Some(t) = str_of("title") {
        parts.push(format!("title → \"{t}\""));
    }
    if let Some(d) = str_of("dialog") {
        parts.push(format!("dialog opened: \"{d}\""));
    }
    if let Some(t) = str_of("text") {
        parts.push(format!("new text: \"{t}\""));
    }
    if let Some(net) = obj.get("net").and_then(|n| n.as_array()) {
        // Entries are "METHOD url [status] [ERR]" — scrub only the URL token.
        let items: Vec<String> = net
            .iter()
            .filter_map(|n| n.as_str())
            .map(|n| {
                n.split(' ')
                    .map(|tok| {
                        if tok.contains("://") {
                            sanitize::sanitize_url(tok)
                        } else {
                            tok.to_string()
                        }
                    })
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect();
        if !items.is_empty() {
            parts.push(format!("requests: {}", items.join("; ")));
        }
    }
    if let Some(d) = str_of("dom") {
        parts.push(format!("dom {d} nodes"));
    }
    if let Some(f) = str_of("focus") {
        parts.push(format!("focus → {f}"));
    }
    if parts.is_empty() {
        " → no observable change within 450 ms (no navigation, request, DOM or focus change). \
         If you expected one: check browser_console_messages, or the control may need a different trigger."
            .to_string()
    } else {
        format!(" → {}", parts.join(" · "))
    }
}

/// Result of `browser_navigate`: the tab, and the filename when the response
/// turned out to be an attachment and was saved instead of rendered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavigateOutcome {
    pub tab_id: usize,
    pub download: Option<String>,
}

/// `browser_get_tabs` payload: `total` is the full count so a truncated list
/// is never mistaken for "all tabs".
#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TabsPage {
    pub total: usize,
    pub tabs: Vec<TabInfo>,
}

fn truncate_chars(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{cut}…")
}

// ─────────────────────────────────────────────────────────────────────
// Data types for MCP responses
// ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, schemars::JsonSchema)]
pub struct TabInfo {
    pub id: usize,
    pub title: String,
    pub url: String,
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_active: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub is_playing_audio: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
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

    /// Listing form: titles and URLs cut to what identifies a tab. A 200-tab
    /// session was 36 KB (~9k tokens) per `browser_get_tabs` call before this.
    pub fn compact(mut self) -> Self {
        self.title = truncate_chars(&self.title, 60);
        self.url = truncate_chars(&self.url, 100);
        self
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
    #[schemars(
        description = "How long to wait for the script (and any Promise it returns) to settle. Default 10000, max 60000. Raise it for scripts that await many network calls."
    )]
    pub timeout_ms: Option<u64>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct GetTabsRequest {
    #[schemars(
        description = "Max tabs to return, most recently active first (the visible tab is always included). Default 40, max 200."
    )]
    pub limit: Option<usize>,
    #[schemars(
        description = "Case-insensitive substring to match against title or URL (e.g. \"drive.google.com\")."
    )]
    pub query: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct ClickRequest {
    #[schemars(description = "Tab to target. Omit for the user's visible tab.")]
    pub tab_id: Option<usize>,
    #[schemars(description = "CSS selector or @ref from browser_snapshot.")]
    pub selector: String,
    #[schemars(
        description = "Optional expected outcome to wait for after the action (up to 6s), reported as ✓/✗ in the result — one call to act AND verify. Forms: \"text:Saved\" (text appears), \"gone:Loading\" (text disappears), \"url:/dashboard\" (URL contains), \"selector:.success\" / \"selector_gone:.spinner\". A bare string is treated as text."
    )]
    pub expect: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct HoverRequest {
    #[schemars(description = "Tab to target. Omit for the user's visible tab.")]
    pub tab_id: Option<usize>,
    #[schemars(description = "CSS selector or @ref from browser_snapshot.")]
    pub selector: String,
    #[schemars(
        description = "Optional expected outcome to wait for after the action (up to 6s), reported as ✓/✗ in the result — one call to act AND verify. Forms: \"text:Saved\" (text appears), \"gone:Loading\" (text disappears), \"url:/dashboard\" (URL contains), \"selector:.success\" / \"selector_gone:.spinner\". A bare string is treated as text."
    )]
    pub expect: Option<String>,
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
    #[schemars(
        description = "Optional expected outcome to wait for after the action (up to 6s), reported as ✓/✗ in the result — one call to act AND verify. Forms: \"text:Saved\" (text appears), \"gone:Loading\" (text disappears), \"url:/dashboard\" (URL contains), \"selector:.success\" / \"selector_gone:.spinner\". A bare string is treated as text."
    )]
    pub expect: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FormField {
    #[schemars(
        description = "CSS selector or @ref of an <input>, <textarea>, or contenteditable."
    )]
    pub selector: String,
    #[schemars(description = "Value to set (replaces the existing value).")]
    pub value: String,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct FillFormRequest {
    #[schemars(description = "Tab to target. Omit for the user's visible tab.")]
    pub tab_id: Option<usize>,
    #[schemars(description = "Fields to fill, in order.")]
    pub fields: Vec<FormField>,
    #[schemars(
        description = "Optional CSS selector or @ref of a submit button to click after filling."
    )]
    pub submit: Option<String>,
    #[schemars(
        description = "Optional expected outcome after submit (see browser_click's expect)."
    )]
    pub expect: Option<String>,
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
    #[schemars(
        description = "Optional expected outcome to wait for after the action (up to 6s), reported as ✓/✗ in the result — one call to act AND verify. Forms: \"text:Saved\" (text appears), \"gone:Loading\" (text disappears), \"url:/dashboard\" (URL contains), \"selector:.success\" / \"selector_gone:.spinner\". A bare string is treated as text."
    )]
    pub expect: Option<String>,
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
pub struct WaitRequest {
    #[schemars(description = "Tab to target. Omit for the user's visible tab.")]
    pub tab_id: Option<usize>,
    #[schemars(
        description = "What to wait for: \"load\" (default) | \"domcontentloaded\" | \"ready\" (full SPA readiness — same probe browser_navigate uses; resolves \"ready\"/\"live\"/\"partial\") | \"text:<phrase>\" (until that visible text appears) | \"text_gone:<phrase>\" (until it disappears) | a CSS selector (until it matches). Resolves \"ready\" or \"timeout\"."
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
    #[schemars(
        description = "Scope the scan to one container (a dialog, a form, a list): a CSS selector or an @ref from a prior snapshot. Omit to scan the whole page."
    )]
    pub within: Option<String>,
    #[schemars(
        description = "When true, return only elements that appeared, changed, or were removed since the last snapshot of this tab (refs are stable across snapshots). On a busy SPA this is far cheaper than a full re-snapshot. Default false."
    )]
    pub diff: Option<bool>,
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
    #[schemars(
        description = "Include captured response bodies (fetch/XHR, texty content-types, ≤20 KB each) — use to read the JSON API behind a UI. Off by default to save tokens; narrow with `filter` first."
    )]
    pub include_body: Option<bool>,
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
    #[schemars(
        description = "Optional expected outcome to wait for after the action (up to 6s), reported as ✓/✗ in the result — one call to act AND verify. Forms: \"text:Saved\" (text appears), \"gone:Loading\" (text disappears), \"url:/dashboard\" (URL contains), \"selector:.success\" / \"selector_gone:.spinner\". A bare string is treated as text."
    )]
    pub expect: Option<String>,
}

// ─────────────────────────────────────────────────────────────────────
// MCP Server Implementation
// ─────────────────────────────────────────────────────────────────────

/// Shared state for MCP tools to communicate with main event loop.
#[derive(Clone)]
pub struct McpState {
    /// Which workspace this server instance speaks for. Fixed at construction
    /// — each workspace has its own listener, so it can never be ambiguous.
    pub workspace_id: String,
    /// Channel to send commands to main event loop
    pub command_tx: mpsc::UnboundedSender<(String, McpCommand)>,
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
            .send((self.state.workspace_id.clone(), build(tx)))
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
    pub fn new(
        workspace_id: String,
        command_tx: mpsc::UnboundedSender<(String, McpCommand)>,
    ) -> Self {
        Self {
            state: McpState {
                workspace_id,
                command_tx,
            },
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

        let outcome = browser_try!(
            self.send_command(|tx| McpCommand::Navigate {
                url: req.url,
                tab_id: req.tab_id,
                response: tx,
            })
            .await?
        );

        let note = match &outcome.download {
            Some(name) => format!(
                "The response was an attachment: '{name}' is being saved to ~/Downloads. \
                 The tab kept its previous page — nothing to read or click here."
            ),
            None if in_place => "Tab navigated; focus unchanged.".to_string(),
            None => "Background tab opened — reuse this tab_id for follow-up calls; browser_switch_tab shows it to the user.".to_string(),
        };
        let result = serde_json::json!({
            "tab_id": outcome.tab_id,
            "mode": if outcome.download.is_some() { "download" } else if in_place { "in_place" } else { "new_background_tab" },
            "note": note,
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
        description = "Wait for a condition. Rarely needed: browser_navigate already waits for SPA readiness and every action reports its effect. Use for lazily-loaded content or after an action that reported a route change. event: \"load\" (default) | \"domcontentloaded\" | \"ready\" (full SPA readiness — main content rendered + DOM/JS quiet, with a steady-state fallback for live feeds; returns \"ready\" | \"live\" | \"partial\") | a CSS selector to wait for. Other events return \"ready\" or \"timeout\"."
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
        description = "List open tabs, most recently active first (the visible tab leads). Returns {total, tabs:[{id, title, url, is_active?, is_playing_audio?}]} — flags appear only when true, titles/URLs are trimmed. `total` counts every match so a truncated list is obvious; narrow with `query` (title/URL substring) instead of raising `limit`. Use the IDs to target other tools."
    )]
    async fn browser_get_tabs(
        &self,
        Parameters(req): Parameters<GetTabsRequest>,
    ) -> Result<CallToolResult, McpError> {
        let tabs = browser_try!(
            self.send_command(|tx| McpCommand::GetTabs {
                limit: req.limit.unwrap_or(40).clamp(1, 200),
                query: req.query.clone(),
                response: tx,
            })
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
        description = "Compact map of the page: header with URL, title, headings and any alert/status/dialog text (where 'Request sent' / 'Access denied' / validation errors live), then every interactive element with a @N ref you pass directly to browser_click / browser_type / browser_hover / browser_press_key / browser_select_option — much cheaper than guessing CSS. Refs are STABLE: the same element keeps its @N across snapshots of this tab (until navigation). Form controls carry their <label> text; radios/checkboxes show checked state. Includes same-origin iframes and open shadow DOM. A '+N present-but-hidden controls' note means an auto-hiding toolbar: browser_hover the page and re-snapshot. Use `within` (a CSS selector or @ref) to scan just one dialog/form/list, and `diff:true` to get only what changed since the last snapshot — on a busy SPA that is far cheaper than re-scanning the whole page. Start here on an unfamiliar page and re-snapshot to verify state instead of reading the DOM with browser_execute_js."
    )]
    async fn browser_snapshot(
        &self,
        Parameters(req): Parameters<SnapshotRequest>,
    ) -> Result<CallToolResult, McpError> {
        let snapshot = browser_try!(
            self.send_command(|tx| McpCommand::Snapshot {
                tab_id: req.tab_id,
                within: req.within.clone(),
                diff: req.diff.unwrap_or(false),
                response: tx,
                is_retry: false,
            })
            .await?
        );
        Ok(CallToolResult::success(vec![Content::text(snapshot)]))
    }

    #[tool(
        description = "Run JavaScript in the page and return its JSON-encoded result. Escape hatch — prefer browser_snapshot (page state + @refs), browser_get_page_content (text) and the action tools (each reports what changed). Last expression is the return value; a returned Promise is awaited; wrap multi-statement code in an IIFE. Exceptions come back with their real message and line. Default watchdog 10 s (timeout_ms up to 60000); a timeout means the script is still running, NOT that the page navigated — a real navigation is reported as such. Defaults to the visible tab."
    )]
    async fn browser_execute_js(
        &self,
        Parameters(req): Parameters<ExecuteJsRequest>,
    ) -> Result<CallToolResult, McpError> {
        let value = browser_try!(
            self.send_command(|tx| McpCommand::ExecuteJs {
                tab_id: req.tab_id,
                script: req.script,
                timeout_ms: req.timeout_ms.unwrap_or(10_000).clamp(1_000, 60_000),
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
        description = "Click an element by CSS selector or @ref from browser_snapshot. Scrolls into view, waits until it is stable and unobstructed, then delivers a TRUSTED native click (isTrusted=true, real user gesture — works on sites that ignore synthetic events, and grants popup/clipboard/fullscreen permissions). The result tells you what happened: navigation, SPA URL change, new text that appeared (toasts, confirmations, errors), dialogs opened, network requests fired, DOM/focus changes — or explicitly 'no observable change'. Trust that line instead of re-reading the page. On error you get a specific reason: stale @ref → re-snapshot; missing → no element; detached → element gone; occluded → dismiss the named overlay. Defaults to the visible tab."
    )]
    async fn browser_click(
        &self,
        Parameters(req): Parameters<ClickRequest>,
    ) -> Result<CallToolResult, McpError> {
        let summary = browser_try!(
            self.send_command(|tx| McpCommand::Click {
                tab_id: req.tab_id,
                selector: req.selector.clone(),
                expect: req.expect.clone(),
                response: tx,
            })
            .await?
        );
        Ok(CallToolResult::success(vec![Content::text(summary)]))
    }

    #[tool(
        description = "Hover over an element with a real (trusted) pointer move — activates CSS :hover, JS hover menus, and auto-hiding toolbars (video players, Drive viewers: hover then re-snapshot to reveal their controls). Reports what changed, like browser_click. Defaults to the visible tab."
    )]
    async fn browser_hover(
        &self,
        Parameters(req): Parameters<HoverRequest>,
    ) -> Result<CallToolResult, McpError> {
        let summary = browser_try!(
            self.send_command(|tx| McpCommand::Hover {
                tab_id: req.tab_id,
                selector: req.selector.clone(),
                expect: req.expect.clone(),
                response: tx,
            })
            .await?
        );
        Ok(CallToolResult::success(vec![Content::text(summary)]))
    }

    #[tool(
        description = "Set the value of a text field. Works with <input>, <textarea>, and contenteditable rich editors including model-backed ones (Lexical, DraftJS, ProseMirror) — their internal model updates so submit buttons gated on it enable. REPLACES the existing value — does not append. For inputs it bypasses React's controlled-input cache via the prototype value setter; for rich editors it drives a synthetic paste (falling back to an editing command). Errors clearly if the target is not actually editable (e.g. a placeholder/label node) instead of silently doing nothing. To press Enter / submit afterwards, use browser_press_key, or click the submit button with browser_click. Reports what changed after typing (e.g. a submit button enabling, suggestions appearing). Defaults to the visible tab."
    )]
    async fn browser_type(
        &self,
        Parameters(req): Parameters<TypeRequest>,
    ) -> Result<CallToolResult, McpError> {
        let summary = browser_try!(
            self.send_command(|tx| McpCommand::Type {
                tab_id: req.tab_id,
                selector: req.selector.clone(),
                text: req.text,
                expect: req.expect.clone(),
                response: tx,
            })
            .await?
        );
        Ok(CallToolResult::success(vec![Content::text(summary)]))
    }

    #[tool(
        description = "Fill several fields and (optionally) submit in ONE call — the whole form instead of N round-trips. Each field is {selector, value} (CSS or @ref); values replace, not append. Pass `submit` (a button selector/@ref) to click after filling, and `expect` to verify the result (see browser_click). Reports each field ✓/✗ plus the submit's effect. Defaults to the visible tab."
    )]
    async fn browser_fill_form(
        &self,
        Parameters(req): Parameters<FillFormRequest>,
    ) -> Result<CallToolResult, McpError> {
        if req.fields.is_empty() {
            return Ok(err_result(
                "fields is empty — pass at least one {selector, value}".into(),
            ));
        }
        let mut lines = Vec::with_capacity(req.fields.len());
        let mut filled = 0usize;
        for f in &req.fields {
            let r = self
                .send_command(|tx| McpCommand::Type {
                    tab_id: req.tab_id,
                    selector: f.selector.clone(),
                    text: f.value.clone(),
                    expect: None,
                    response: tx,
                })
                .await?;
            match r {
                Ok(_) => {
                    filled += 1;
                    lines.push(format!("✓ {}", f.selector));
                }
                Err(e) => lines.push(format!("✗ {}: {e}", f.selector)),
            }
        }
        let submit_line = if let Some(sub) = &req.submit {
            let r = self
                .send_command(|tx| McpCommand::Click {
                    tab_id: req.tab_id,
                    selector: sub.clone(),
                    expect: req.expect.clone(),
                    response: tx,
                })
                .await?;
            match r {
                Ok(effect) => format!("\nsubmit: {effect}"),
                Err(e) => format!("\nsubmit ✗ {sub}: {e}"),
            }
        } else {
            String::new()
        };
        let body = format!(
            "Filled {filled}/{} field(s):\n{}{submit_line}",
            req.fields.len(),
            lines.join("\n")
        );
        Ok(CallToolResult::success(vec![Content::text(body)]))
    }

    #[tool(
        description = "Get past a cookie/consent/newsletter overlay blocking the page. Scans the document, shadow DOM and same-origin iframes for the dismiss control and clicks it (trusted) — preferring Reject/Decline, then Close/×. It NEVER auto-clicks Accept/Agree (granting consent is the user's choice); if only an accept-type control exists it reports that so you can decide. Call this when a click returns `occluded` or a banner covers the page. Defaults to the visible tab."
    )]
    async fn browser_dismiss_overlay(
        &self,
        Parameters(req): Parameters<TabIdRequest>,
    ) -> Result<CallToolResult, McpError> {
        let summary = browser_try!(
            self.send_command(|tx| McpCommand::DismissOverlay {
                tab_id: req.tab_id,
                response: tx,
            })
            .await?
        );
        Ok(CallToolResult::success(vec![Content::text(summary)]))
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
        description = "Press a key with a TRUSTED native key event (isTrusted=true, real default actions: Enter submits forms and activates buttons, Space toggles, Tab moves focus, Escape closes, characters insert). Optional modifiers. If selector is given, focus that element first; otherwise the currently focused element receives the key. For setting a whole field value prefer browser_type; use this for single keys. Reports what changed, like browser_click. Unknown key names are rejected — use KeyboardEvent.key names (Enter, Escape, Tab, ArrowDown, Backspace, F5) or a single character. Defaults to the visible tab."
    )]
    async fn browser_press_key(
        &self,
        Parameters(req): Parameters<PressKeyRequest>,
    ) -> Result<CallToolResult, McpError> {
        let summary = browser_try!(
            self.send_command(|tx| McpCommand::PressKey {
                tab_id: req.tab_id,
                key: req.key.clone(),
                selector: req.selector.clone(),
                modifiers: req.modifiers.unwrap_or_default(),
                expect: req.expect.clone(),
                response: tx,
            })
            .await?
        );
        Ok(CallToolResult::success(vec![Content::text(summary)]))
    }

    #[tool(
        description = "Pick an option in a <select> by its value attribute. Fires a change event and reports what changed (dependent fields, requests). The visible options are listed in browser_snapshot's `options=[...]` attribute. Defaults to the visible tab."
    )]
    async fn browser_select_option(
        &self,
        Parameters(req): Parameters<SelectOptionRequest>,
    ) -> Result<CallToolResult, McpError> {
        let summary = browser_try!(
            self.send_command(|tx| McpCommand::SelectOption {
                tab_id: req.tab_id,
                selector: req.selector.clone(),
                value: req.value.clone(),
                expect: req.expect.clone(),
                response: tx,
            })
            .await?
        );
        Ok(CallToolResult::success(vec![Content::text(summary)]))
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
                timeout_ms: 10_000,
                response: tx,
            })
            .await?
        );
        Ok(CallToolResult::success(vec![Content::text(
            sanitize_json_entries(&value, "text", sanitize::sanitize_text),
        )]))
    }

    #[tool(
        description = "Recent network requests made by the page. Returns a JSON array [{method,url,status,type,ms,ts,error?}], newest last. type=fetch|xhr carry a real status (0 + error = failed); other types (beacon, img, script, iframe, link, other — from resource timing) prove the request fired but may report status 0. Use to debug missing data, failed submissions, or to discover the API behind a UI. Captured from page load (last 200 of each kind kept). Defaults to the visible tab."
    )]
    async fn browser_network_requests(
        &self,
        Parameters(req): Parameters<NetworkRequest>,
    ) -> Result<CallToolResult, McpError> {
        let limit = req.limit.unwrap_or(50).min(200);
        let include_body = req.include_body.unwrap_or(false);
        let filter = match &req.filter {
            Some(f) => format!(
                ".filter(function(e){{return e.url.indexOf({})!==-1}})",
                serde_json::to_string(f).unwrap_or_default()
            ),
            None => String::new(),
        };
        // Strip bodies unless asked — a page under load can buffer many 20 KB
        // JSON payloads and blow the response budget.
        let strip = if include_body {
            String::new()
        } else {
            ".map(function(e){var c={};for(var k in e){if(k!=='body'&&k!=='seq')c[k]=e[k];}return c;})".to_string()
        };
        let script = format!(
            "JSON.stringify((window.__octoweb_net||[]).concat(window.__octoweb_res||[])\
             .sort(function(a,b){{return (a.seq||0)-(b.seq||0)}}){filter}.slice(-{limit}){strip})"
        );
        let value = browser_try!(
            self.send_command(|tx| McpCommand::ExecuteJs {
                tab_id: req.tab_id,
                script,
                timeout_ms: 10_000,
                response: tx,
            })
            .await?
        );
        let scrubbed = sanitize_json_entries(&value, "url", sanitize::sanitize_url);
        // Response bodies can carry PANs/tokens the URL scrub misses — run the
        // PAN/text redactor over them too.
        let scrubbed = if include_body {
            sanitize_json_entries(&scrubbed, "body", sanitize::sanitize_text)
        } else {
            scrubbed
        };
        Ok(CallToolResult::success(vec![Content::text(scrubbed)]))
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
                Reading pages: browser_snapshot shows page state (title, headings, alerts, dialogs) plus \
                actionable elements; browser_get_page_content gives the full text. Use browser_execute_js \
                only for data the other tools cannot express (return value is JSON-encoded; wrap \
                multi-statement code in an IIFE) — never just to check whether an action worked.\n\
                \n\
                Actions are real input: browser_click, browser_hover and browser_press_key deliver trusted \
                native events (isTrusted=true, user gesture), so sites that ignore synthetic events work. \
                Every action returns a one-line effect summary — navigation, URL change, new text (toasts, \
                confirmations, errors), dialogs, requests fired, DOM/focus changes, or 'no observable \
                change'. Read that line; do not re-read the page to find out what happened. 'No observable \
                change' after a click is real information: try the control's other trigger (Enter/Space via \
                browser_press_key, a hover first), or check browser_console_messages.\n\
                \n\
                Typing & submitting: browser_type sets a value (replaces, doesn't append) and fires \
                input+change. To submit, follow with browser_press_key(key=\"Enter\") — a native Enter \
                that submits the form. For single keys use browser_press_key, not browser_type.\n\
                \n\
                Waiting: browser_navigate already blocks until the page is idle — no follow-up wait needed. \
                Interactions auto-retry for ~2.5s until the element is present, stable, and unobstructed, \
                so you rarely need explicit waits; after an action reports a route change, re-snapshot or \
                browser_wait with a CSS selector for the new content. A tool reporting a navigation means \
                one really happened (tracked natively); a timeout means the script is still running.\n\
                \n\
                Debugging: when a page misbehaves or an interaction has no effect, check \
                browser_console_messages (JS errors, with real messages) and browser_network_requests \
                (fetch/XHR with status; pass include_body=true to read the JSON API behind a UI, plus \
                beacons/images/scripts that fired) before retrying blindly.\n\
                \n\
                One-call efficiency — prefer these over act-then-poll-then-verify chains: give any \
                action an `expect` (\"text:Saved\", \"gone:Loading\", \"url:/done\", \"selector:.ok\") and it \
                waits up to 6s for that outcome and reports ✓/✗ — no separate browser_wait. Fill a whole \
                form with browser_fill_form (fields + submit + expect in one call). @refs are STABLE \
                across snapshots, so after a change use browser_snapshot(diff:true) to see only what moved, \
                or browser_snapshot(within:\"@ref\") to read just one dialog/form. browser_wait also takes \
                \"text:<phrase>\" / \"text_gone:<phrase>\".\n\
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

/// Where one workspace's MCP server listens, and the secret that reaches it.
#[derive(Debug, Clone)]
pub struct McpEndpoint {
    pub port: u16,
    pub token: String,
}

impl McpEndpoint {
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/mcp/{}", self.port, self.token)
    }
}

/// Control messages to the tokio thread that owns the listeners.
enum ServerCtl {
    Start {
        workspace_id: String,
        token: String,
        listener: std::net::TcpListener,
    },
    Stop(String),
}

/// Handle returned from spawn_mcp_server for polling commands.
///
/// Every workspace gets its own listener, so the port+token a request arrives
/// on *is* the caller's workspace identity — commands come back tagged with
/// it and the main loop applies them to that workspace rather than to whatever
/// happens to be on screen.
pub struct McpHandle {
    /// Receiver for (workspace_id, command) from MCP tools
    pub command_rx: mpsc::UnboundedReceiver<(String, McpCommand)>,
    /// Sender used by the main loop to re-issue read commands from a
    /// watchdog when the first eval's callback was discarded.
    pub command_tx: mpsc::UnboundedSender<(String, McpCommand)>,
    ctl_tx: mpsc::UnboundedSender<ServerCtl>,
}

pub fn spawn_mcp_server() -> McpHandle {
    let (command_tx, command_rx) = mpsc::unbounded_channel::<(String, McpCommand)>();
    let (ctl_tx, mut ctl_rx) = mpsc::unbounded_channel::<ServerCtl>();

    let command_tx_for_servers = command_tx.clone();
    let command_tx_for_handle = command_tx.clone();
    // Spawn tokio runtime in a separate thread
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("Failed to create tokio runtime");
        rt.block_on(async move {
            let mut shutdowns: std::collections::HashMap<String, oneshot::Sender<()>> =
                std::collections::HashMap::new();
            while let Some(ctl) = ctl_rx.recv().await {
                match ctl {
                    ServerCtl::Start {
                        workspace_id,
                        token,
                        listener,
                    } => {
                        let listener = match tokio::net::TcpListener::from_std(listener) {
                            Ok(l) => l,
                            Err(e) => {
                                tracing::warn!(error = %e, %workspace_id, "MCP listener adopt failed");
                                continue;
                            }
                        };

                        let ws_id = workspace_id.clone();
                        let ctx = command_tx_for_servers.clone();
                        let server_factory = move || {
                            Ok(McpServer::new(ws_id.clone(), ctx.clone()))
                                as Result<McpServer, std::io::Error>
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

                        // The token is baked into the route, so a request with the
                        // wrong one 404s in the router — no auth code of our own.
                        let app = axum::Router::new().nest_service(&format!("/mcp/{token}"), service);

                        let (sd_tx, sd_rx) = oneshot::channel::<()>();
                        shutdowns.insert(workspace_id.clone(), sd_tx);
                        tokio::spawn(async move {
                            let served = axum::serve(listener, app)
                                .with_graceful_shutdown(async move {
                                    let _ = sd_rx.await;
                                })
                                .await;
                            if let Err(e) = served {
                                tracing::error!(error = %e, %workspace_id, "MCP HTTP server error");
                            }
                        });
                    }
                    ServerCtl::Stop(workspace_id) => {
                        if let Some(sd) = shutdowns.remove(&workspace_id) {
                            let _ = sd.send(());
                        }
                    }
                }
            }
        });
    });

    McpHandle {
        command_rx,
        command_tx: command_tx_for_handle,
        ctl_tx,
    }
}

impl McpHandle {
    /// Poll for pending MCP commands (non-blocking).
    /// Returns None if no commands are pending.
    pub fn poll(&mut self) -> Option<(String, McpCommand)> {
        self.command_rx.try_recv().ok()
    }

    /// Bind a listener for `workspace_id` and start serving on it.
    ///
    /// The bind happens here, on the caller's thread, so the port is known
    /// synchronously — the caller needs it immediately to build the agent's
    /// MCP URL. `preferred_port` is honoured when free (the Default workspace
    /// keeps 3434 so pre-workspaces agent configs still resolve); everything
    /// else takes an ephemeral port. Returns `None` if the bind fails.
    pub fn start_workspace(&self, workspace_id: &str, preferred_port: u16) -> Option<McpEndpoint> {
        let listener = std::net::TcpListener::bind(("127.0.0.1", preferred_port))
            .or_else(|e| {
                if preferred_port != 0 {
                    tracing::warn!(error = %e, preferred_port, "MCP port busy — falling back to ephemeral");
                    std::net::TcpListener::bind(("127.0.0.1", 0))
                } else {
                    Err(e)
                }
            })
            .map_err(|e| tracing::warn!(error = %e, %workspace_id, "MCP bind failed"))
            .ok()?;
        let port = listener.local_addr().ok()?.port();
        listener.set_nonblocking(true).ok()?;

        let token = random_token();
        let endpoint = McpEndpoint {
            port,
            token: token.clone(),
        };
        self.ctl_tx
            .send(ServerCtl::Start {
                workspace_id: workspace_id.to_string(),
                token,
                listener,
            })
            .ok()?;
        tracing::info!(%workspace_id, port, "MCP HTTP server listening");
        Some(endpoint)
    }

    /// Shut the workspace's listener down — called when a workspace is deleted.
    pub fn stop_workspace(&self, workspace_id: &str) {
        let _ = self.ctl_tx.send(ServerCtl::Stop(workspace_id.to_string()));
    }
}

/// 32 hex chars from `/dev/urandom`. Ephemeral: minted per run, never
/// persisted, so a stale token can't outlive the process that issued it.
fn random_token() -> String {
    use std::io::Read;
    let mut buf = [0u8; 16];
    std::fs::File::open("/dev/urandom")
        .and_then(|mut f| f.read_exact(&mut buf))
        .expect("failed to read /dev/urandom for MCP token");
    buf.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tab(id: usize, title: &str, url: &str) -> crate::browser::Tab {
        crate::browser::Tab {
            id,
            title: title.into(),
            url: url.into(),
            is_playing_audio: false,
            page_bytes: 0,
            page_time_ms: 0,
            last_active_at: std::time::Instant::now(),
        }
    }

    #[test]
    fn dom_result_plain_true_has_no_effect_text() {
        assert_eq!(interpret_dom_result("\"true\"", "@1"), Ok(String::new()));
    }

    #[test]
    fn dom_result_with_effect_payload() {
        let val =
            serde_json::to_string("true|{\"text\":\"Request sent\",\"dom\":\"+2/-1\"}").unwrap();
        let out = interpret_dom_result(&val, "@7").unwrap();
        assert!(out.contains("new text: \"Request sent\""), "{out}");
        assert!(out.contains("dom +2/-1 nodes"), "{out}");
    }

    #[test]
    fn dom_result_statuses_are_actionable() {
        let err = interpret_dom_result("\"stale\"", "@3").unwrap_err();
        assert!(err.contains("browser_snapshot"), "{err}");
        let err = interpret_dom_result("\"occluded:div#cookie\"", "#go").unwrap_err();
        assert!(err.contains("div#cookie"), "{err}");
        assert_eq!(
            dom_status_error("weird", "x"),
            "Unexpected DOM result: weird"
        );
    }

    #[test]
    fn effect_lists_only_changes_and_names_silence() {
        let quiet = format_effect("{}");
        assert!(quiet.contains("no observable change"), "{quiet}");
        let full = format_effect(
            "{\"url\":\"https://x.test/next\",\"dialog\":\"Are you sure?\",\"net\":[\"POST https://x.test/api 200\"],\"focus\":\"input \\\"q\\\"\"}",
        );
        assert!(full.starts_with(" → url → https://x.test/next"), "{full}");
        assert!(full.contains("dialog opened: \"Are you sure?\""), "{full}");
        assert!(
            full.contains("requests: POST https://x.test/api 200"),
            "{full}"
        );
        assert!(full.contains("focus → input \"q\""), "{full}");
        assert_eq!(format_effect("not json"), "");
    }

    #[test]
    fn effect_reports_downloads_even_when_dom_is_silent() {
        let out = format_effect_with_download("{}", Some("MVI_5662.mp4"));
        assert_eq!(
            out,
            " → download started: MVI_5662.mp4 (saved to ~/Downloads)"
        );
        let out = format_effect_with_download("{\"dom\":\"+1/-0\"}", Some("a.jpg"));
        assert!(out.starts_with(" → download started: a.jpg"), "{out}");
        assert!(out.contains("dom +1/-0 nodes"), "{out}");
    }

    #[test]
    fn effect_scrubs_sensitive_query_params() {
        let out = format_effect("{\"net\":[\"GET https://x.test/cb?token=abc123 200\"]}");
        assert!(!out.contains("abc123"), "{out}");
    }

    #[test]
    fn tab_info_compacts_and_skips_false_flags() {
        let long = "t".repeat(200);
        let info = TabInfo::from_tab(&tab(1, &long, "https://example.com/a"), false).compact();
        assert_eq!(info.title.chars().count(), 60);
        assert!(info.title.ends_with('…'));
        let json = serde_json::to_string(&info).unwrap();
        assert!(!json.contains("is_active"), "{json}");
        assert!(!json.contains("is_playing_audio"), "{json}");
        let active =
            serde_json::to_string(&TabInfo::from_tab(&tab(2, "x", "https://a.b"), true)).unwrap();
        assert!(active.contains("\"is_active\":true"), "{active}");
    }
}
