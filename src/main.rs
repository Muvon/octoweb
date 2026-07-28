mod a2ui_render_ui;
mod a2ui_watcher;
mod acp;
mod address_bar_html;
mod async_eval;
mod browser;
mod cold_open;
mod config;
mod content_rules;
mod crash_report;
mod dialog_patch;
mod dom_actions;
mod error_page_html;
mod find_bar_html;
mod hibernation;
mod icons;
mod inline_edit_html;
mod keybindings;
mod macos;
mod mcp;
mod nav_error_patch;
mod newtab_html;
mod notification_html;
mod overlay_html;
mod progress_bar_html;
mod prompt_history_js;
mod quickslots;
mod quickslots_html;
mod readiness_js;
mod sanitize;
mod settings_html;
mod shortcuts_html;
mod sidebar_html;
mod snapshot_js;
mod tab_stats;
mod url;
mod webview_utils;

use std::collections::{HashMap, VecDeque};
use std::sync::{
    atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
    Arc, Mutex,
};
use tao::{
    dpi::LogicalSize,
    event::{Event, WindowEvent},
    event_loop::{ControlFlow, EventLoop, EventLoopBuilder},
    keyboard::{KeyCode, ModifiersState},
    platform::macos::{WindowBuilderExtMacOS, WindowExtMacOS},
    window::WindowBuilder,
};
use wry::http::Response;
use wry::{BackgroundThrottlingPolicy, WebView, WebViewBuilder, WebViewExtMacOS};

use browser::TabManager;
use config::Config;
use mcp::{interpret_dom_result, HistoryInfo, McpCommand, PageInfo, TabInfo};

#[derive(Debug)]
enum AppEvent {
    ToggleOverlay,
    HideOverlay,
    TitleChanged(usize, String),      // (tab_id, title)
    BrowserUrlChanged(usize, String), // (tab_id, url)
    FaviconFetched(String, String),   // (domain, data_uri)
    NavigateTo(String),
    PushAddressHistory, // address bar requested history snapshot for URL autocomplete
    UrlEditExpand(bool), // address bar entered/left edit mode — grow the bar webview so the
    // suggestion dropdown isn't clipped by the 32 px titlebar height.
    UrlEditRequest, // Cmd+E — trigger URL edit on the address bar (no shift)
    SwitchTab(usize),
    CloseTab(usize),
    PrevTab,                 // Ctrl+P — switch to previous tab in MRU order
    NextTab,                 // Ctrl+N — switch to next tab in MRU order
    ToggleSidebar,           // Cmd+Shift+A — toggle AI assistant sidebar
    ToggleFullscreen,        // ⌘Return — toggle native macOS fullscreen on the chrome window
    ToggleSidebarFullscreen, // ⌘⇧Return / icon — sidebar expands to full window width inside chrome
    AcpPrompt(u64, String, Vec<(String, String)>), // (session_id, text, images) — user prompt + optional images
    AcpCancel(u64),                                // (session_id) — user clicked stop button
    AcpSetAgent(u64, String), // (session_id, tag) — change agent tag and restart that session
    AcpClearSession(u64),     // (session_id) — restart session with same tag (clear chat)
    AcpSessionCreate(String, String), // (title, tag) — create new session (capped at MAX_SESSIONS)
    AcpSessionCreatePanel,    // ⌘T in sidebar — open the new-session input panel
    AcpSessionClose(u64),     // (session_id) — close session (refused if last one)
    AcpSessionCloseActive,    // ⌘W in sidebar — close currently active session
    AcpSessionSwitch(u64),    // (session_id) — switch active session
    AcpSessionRename(u64, String), // (session_id, title) — rename session
    SidebarReady,             // sidebar JS finished initializing — push restore
    SidebarReFocus,           // app reactivated while sidebar owned key — restore textarea focus
    AskAI(String),            // overlay ⌘⇧Enter — open sidebar + send prompt
    ToggleDevTools,           // Cmd+Shift+I — open devtools for active tab
    OpenInNewTab(String),     // Cmd+click / target=_blank — open URL in new tab and switch to it
    PageLoadStarted(usize),   // (tab_id) — show progress bar
    PageLoadFinished(usize),  // (tab_id) — hide progress bar
    NavigationError(usize, String, String), // (tab_id, url, error) — show error page
    Reload,                   // Cmd+R — reload current page
    MediaPlaying(usize, bool), // (tab_id, is_playing) — audio/video state changed
    PageInfo(usize, u64, u64), // (tab_id, bytes, ms) — page load stats from PerformanceNavigationTiming
    RemoveHistory(String),     // URL to remove from history
    QuickSlotOpen(usize),      // ⌘1–⌘0 — open saved URL in slot 0–9
    QuickSlotSave(usize),      // ⌘⇧1–⌘⇧0 — save current page to slot 0–9
    QuickSlotRemove(usize),    // remove slot (from footer bar ✕ or newtab page)
    AcpWake,                   // lightweight wake — ACP thread pokes event loop
    AcpReconnect(u64, u64), // (session_id, gen) — scheduled reconnection attempt for given session
    AcpSignIn(u64),         // sidebar "Sign in" button — start `/login` for the session
    AcpRefreshAccount(u64), // re-probe `/usage` (login poller tick, or manual refresh)
    DownloadStarted(usize, String), // (tab_id, filename) — navigation became a download, close the tab
    DownloadCompleted(String, bool), // (filename, success) — show notification toast
    DismissNotification,            // user clicked X on notification toast
    ToggleSettings,                 // ⌘, — toggle settings modal
    HideSettings,                   // JS Esc / backdrop click in settings modal
    UpdateConfig(String, String),   // (key, value) — config field changed in settings UI
    ToggleShortcuts,                // ⌘/ — toggle keyboard shortcuts overlay
    HideShortcuts,                  // JS Esc / backdrop click in shortcuts overlay
    ToggleFindBar,                  // ⌘F — toggle find-in-page bar
    HideFindBar,                    // Esc / close button in find bar
    FindInPage(String),             // search query from find bar input
    FindNext,                       // next match (Enter in find bar)
    FindPrev,                       // previous match (⇧Enter in find bar)
    FindCount(usize, usize),        // (current, total) — match count from tab WebView
    WebContentTerminated(usize),    // (tab_id) — WebContent XPC process killed by OS
    ScrollDown,                     // ⌃D — scroll down one full screen
    ScrollUp,                       // ⌃U — scroll up one full screen
    ScrollTop,                      // ⌃T — scroll to top of page
    ScrollBottom,                   // ⌃B — scroll to bottom of page
    Screenshot,                     // ⌘S — screenshot visible viewport → NSSavePanel + clipboard
    ScreenshotFullPage,             // ⌘⇧S — full page screenshot → NSSavePanel + clipboard
    SnapshotCaptured(usize, String), // (tab_id, base64_data_uri) — frozen tab snapshot for instant restore
    FaviconCacheLoaded(HashMap<String, String>), // background-loaded favicon cache from disk
    InlineEditRequest,               // ⌘⇧E — capture selection, show modal
    InlineEditReady(String, Option<(f64, f64)>), // (text, optional (x, y) — None = no anchor, host picks default)
    InlineEditSubmit(String),                    // user submitted prompt in modal
    InlineEditClose,                             // user closed modal (Esc / close btn)
    InlineEditHide,                              // hide modal but keep processing
    InlineEditResize(f64),                       // modal content height changed
    ZoomIn,                                      // ⌘= — increase page zoom
    ZoomOut,                                     // ⌘- — decrease page zoom
    ZoomReset,                                   // ⌘0 — reset page zoom to 100%
    LearningWake,                                // background learning agent pokes event loop
    LearningReady(String), // active tab content extracted — build prompt and send
    JsDialog(dialog_patch::DialogInfo), // JS alert/confirm/prompt captured
    /// A2UI surface envelope (from the `render_ui` tool) changed on disk —
    /// watcher saw a new or status-flipped file. Forwarded to the sidebar's
    /// currently-active session as a `.msg.ui` bubble.
    A2uiUpdate(a2ui_watcher::A2uiSnapshot),
    KeybindRecord(String, String), // (action_id, chord) — remap a global shortcut
    KeybindReset(String),          // (action_id) — restore one action to its default
    KeybindResetAll,               // restore every keybinding to default
    KeybindCapture(bool),          // settings panel started/stopped recording a chord
    Quit,
}

/// Map a remappable [`keybindings::Action`] to the concrete [`AppEvent`] for the
/// current UI context. Returns `None` when the action is suppressed in this
/// context (e.g. scrolling while the command palette is open) — the caller then
/// passes the key through to the focused WebView, matching prior behavior.
fn keybind_to_event(
    action: keybindings::Action,
    overlay: bool,
    find: bool,
    inline: bool,
    sidebar: bool,
) -> Option<AppEvent> {
    use keybindings::Action as A;
    Some(match action {
        A::CommandPalette => AppEvent::ToggleOverlay,
        A::Sidebar => AppEvent::ToggleSidebar,
        A::DevTools => AppEvent::ToggleDevTools,
        A::Settings => AppEvent::ToggleSettings,
        A::Shortcuts => AppEvent::ToggleShortcuts,
        A::Quit => AppEvent::Quit,
        A::SidebarFullscreen if !overlay && !inline => AppEvent::ToggleSidebarFullscreen,
        A::Fullscreen if !overlay && !inline && !find => AppEvent::ToggleFullscreen,
        A::InlineEdit if !overlay => AppEvent::InlineEditRequest,
        A::UrlEdit if !overlay && !inline && !find && !sidebar => AppEvent::UrlEditRequest,
        A::NewSession if sidebar && !overlay => AppEvent::AcpSessionCreatePanel,
        A::CloseTab if !overlay => {
            if sidebar {
                AppEvent::AcpSessionCloseActive
            } else {
                AppEvent::CloseTab(0)
            }
        }
        A::PrevTab if !overlay && !inline && !sidebar => {
            if find {
                AppEvent::FindPrev
            } else {
                AppEvent::PrevTab
            }
        }
        A::NextTab if !overlay && !inline && !sidebar => {
            if find {
                AppEvent::FindNext
            } else {
                AppEvent::NextTab
            }
        }
        A::ScrollDown if !overlay && !inline && !sidebar => AppEvent::ScrollDown,
        A::ScrollUp if !overlay && !inline && !sidebar => AppEvent::ScrollUp,
        A::ScrollTop if !overlay && !inline && !sidebar => AppEvent::ScrollTop,
        A::ScrollBottom if !overlay && !inline && !sidebar => AppEvent::ScrollBottom,
        A::Reload if !overlay => AppEvent::Reload,
        A::Screenshot if !overlay => AppEvent::Screenshot,
        A::ScreenshotFull if !overlay => AppEvent::ScreenshotFullPage,
        A::ZoomIn if !overlay => AppEvent::ZoomIn,
        A::ZoomOut if !overlay => AppEvent::ZoomOut,
        A::ZoomReset if !overlay => AppEvent::ZoomReset,
        A::Find if !overlay => AppEvent::ToggleFindBar,
        _ => return None,
    })
}

/// Per-ACP-session live state.
///
/// Multiple sessions can run in parallel (capped at `MAX_SESSIONS`). Each owns a
/// dedicated subprocess via `AcpHandle`. The sidebar UI shows a tab strip; events
/// from each handle are tagged with `id` before being forwarded to the sidebar JS,
/// which keeps a separate DOM message log per session.
struct AcpSession {
    /// Stable monotonic local id (not the ACP protocol session id).
    id: u64,
    /// User-visible label shown in the header strip.
    title: String,
    /// Agent command tag (the `<tag>` in `octomind acp <tag>`).
    tag: String,
    /// Live handle. `None` while reconnecting after an error.
    handle: Option<acp::AcpHandle>,
    /// Reconnect retry counter (per-session exponential backoff).
    retry_count: u32,
    /// Generation counter — bumped on every intentional reconnect to invalidate
    /// stale `AcpReconnect` timers from prior error cycles for THIS session.
    reconnect_gen: u64,
    /// Timestamp of the last user-initiated cancel that hasn't yet completed.
    /// A second `AcpCancel` within ~12s escalates to a force-kill + respawn.
    /// Cleared on any terminal event (Done/Cancelled/Error).
    last_cancel_at: Option<std::time::Instant>,
    /// ACP protocol session id reported by the agent on `AgentEvent::Connected`.
    /// Passed back as `--resume <id>` when force-respawning the subprocess so
    /// the agent restores its in-memory conversation context. Cleared on
    /// AcpSetAgent / AcpClearSession (which want a fresh chat).
    acp_session_id: Option<String>,
    /// Persisted message log — user prompts, finalized agent responses with
    /// their tool runs, errors. Survives app restarts via `acp_history.json`.
    /// Inline images stay live-only. Capped at `max_acp_session_messages`.
    messages: Vec<config::AcpMessage>,
    /// Accumulator for the in-flight agent response. Filled on every
    /// `AgentEvent::Chunk` and flushed into `messages` on Done / Cancelled /
    /// Error so the persisted log only contains finalized turns.
    agent_buf: String,
    /// Tool runs of the in-flight turn. Mirrors what the live feed shows;
    /// flushed into the agent message alongside `agent_buf` so the steps
    /// group survives restarts.
    tool_buf: Vec<config::AcpToolRecord>,
    /// In-flight tool lookup: tool-call id → (tool_buf index, start time).
    tool_starts: HashMap<String, (usize, std::time::Instant)>,
    /// Wall-clock start of the in-flight turn — set on prompt dispatch,
    /// consumed by the flush into `turn_ms`.
    turn_started: Option<std::time::Instant>,
    /// Cached `AvailableCommands` payload (already JSON-serialized for direct
    /// re-injection into JS). Set when the agent emits commands; re-pushed on
    /// `sidebar_ready` so slash-command auto-complete survives sidebar opens
    /// that occur after the agent already announced its commands.
    available_commands_json: Option<String>,
    /// Cached account/quota status (JSON for `window.__setAccount`) from the
    /// last `/usage` probe. Re-pushed on `sidebar_ready` so the login chip
    /// survives sidebar opens that happen after the probe.
    account_json: Option<String>,
    /// OS pid of this session's octomind subprocess. Set on `ProcessPid`.
    /// Used by the A2UI watcher to route incoming envelopes to the session
    /// whose octomind actually invoked `render_ui` — the bash script
    /// stamps `parent_pid` into each envelope, and we match against this.
    /// Cleared on Error/respawn; the next spawn fills it again.
    octomind_pid: Option<u32>,
}

/// Hard cap on parallel ACP sessions — each is a forked subprocess with its own
/// memory footprint.
pub const MAX_SESSIONS: usize = 10;

fn main() {
    // Initialize tracing: RUST_LOG env controls verbosity (e.g. RUST_LOG=debug).
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    // Set English as the preferred language for WKWebView's Accept-Language header.
    // Must be called before any WebView is created.
    macos::set_english_locale();

    // Start compiling the content blocker (WKContentRuleList) asynchronously.
    // The completion block fires on the main thread after the event loop starts
    // and applies the rule list to all tab WebViews. On subsequent launches
    // WebKit loads from its disk cache — effectively instant.
    // Must be called on the main thread, before the event loop.
    {
        // SAFETY: we are on the main thread here (before the event loop starts).
        let mtm = unsafe { objc2_foundation::MainThreadMarker::new_unchecked() };
        content_rules::init(mtm);
    }

    // Import user's full shell environment (PATH, API keys, etc.) for .app context.
    macos::init_env();

    // ── Crash diagnostics (must be early — before anything that can panic) ────
    crash_report::rotate_log();
    crash_report::install_panic_hook();
    crash_report::install_signal_handlers();
    crash_report::log_startup();

    let mut cfg = Config::load();
    let tabs = Arc::new(Mutex::new(TabManager::new(cfg.max_history)));

    // Restore browsing history in a background thread so startup isn't blocked
    // by JSON deserialization of up to 1000 history entries. The thread finishes
    // in <50ms on typical hardware; history is available well before the user
    // can open the overlay (Cmd+Shift+P) for the first time.
    {
        let tabs_for_history = Arc::clone(&tabs);
        std::thread::spawn(move || {
            let persisted = config::load_history();
            if !persisted.is_empty() {
                tabs_for_history.lock().unwrap().seed_history(persisted);
            }
        });
    }

    cold_open::install_early(); // capture kAEGetURL before tao drops it
    let event_loop: EventLoop<AppEvent> = EventLoopBuilder::with_user_event().build();
    cold_open::install(); // hook application:openURLs: for warm launch
    let proxy = event_loop.create_proxy();
    let overlay_hotkey_visible = Arc::new(AtomicBool::new(false));
    let find_bar_hotkey_visible = Arc::new(AtomicBool::new(false));
    let inline_edit_hotkey_visible = Arc::new(AtomicBool::new(false));
    let sidebar_hotkey_visible = Arc::new(AtomicBool::new(false));
    // True while the settings panel is recording a new keybinding: the monitor
    // passes every key through untouched so the settings WebView can capture the
    // chord (otherwise a bound combo like ⌘⇧P would be swallowed before the UI
    // sees it). Cleared on capture end or when the panel closes.
    let keybind_capturing = Arc::new(AtomicBool::new(false));
    // Tracks whether chrome_win (sidebar host) currently owns logical input
    // focus. Read by the NSApplicationDidBecomeActive observer to decide
    // whether to restore key status + textarea focus after a transient
    // deactivation (notification banner, Spotlight, brief Cmd-Tab).
    let sidebar_owns_key = Arc::new(AtomicBool::new(false));
    // Note: app-frontmost state is queried on demand via `is_app_active()`
    // (NSApplication.isActive). We deliberately do NOT cache via
    // WindowEvent::Focused — modal panels from other apps (1Password ⌘\,
    // takeSnapshot, screenshot picker, system dialogs) steal key-window
    // status from our NSWindow without macOS deactivating the app. tao
    // delivers Focused(false) but never Focused(true) when they dismiss,
    // so any cached flag stays stuck at false and breaks all hotkeys.

    // ── System stats for custom protocol (avoids evaluate_script leak) ─────
    // JS polls via fetch('octoweb-sys://stats') instead of evaluate_script.
    // wry#1489: WKWebView JS evaluation contexts leak on every evaluate_script call.
    let sys_stats_cpu = Arc::new(AtomicU32::new(0));
    let sys_stats_mem = Arc::new(AtomicU64::new(0));

    // ── Browser window ────────────────────────────────────────────────────
    let browser_win = WindowBuilder::new()
        .with_title("octoweb")
        .with_inner_size(LogicalSize::new(cfg.window_width, cfg.window_height))
        .with_titlebar_transparent(true)
        .with_fullsize_content_view(true)
        .with_title_hidden(true)
        .with_titlebar_buttons_hidden(false)
        .build(&event_loop)
        .expect("Failed to create browser window");

    let browser_win = Arc::new(browser_win);
    let browser_win_id = browser_win.id();

    // Allow native macOS fullscreen on the browser window (⌘Return).
    // NSWindowCollectionBehaviorFullScreenPrimary = 1 << 7 = 128.
    // chrome_win (created below) gets FullScreenAuxiliary so it follows into
    // the fullscreen space — without these bits, child windows stay on the
    // original desktop and the sidebar drifts out of sync.
    unsafe {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;
        let ns_win: *mut AnyObject = browser_win.ns_window() as *mut AnyObject;
        let _: () = msg_send![ns_win, setCollectionBehavior: 128u64];
    }

    // First tao window has been built — its `TaoView` ObjC class is now
    // registered with the runtime, so we can install our keyDown fix that
    // prevents duplicate insertText: into WKWebViews under Lexical (x.com
    // compose). See `macos::install_taoview_keyboard_fix` docstring for
    // the full chain of why this is necessary and why it's safe.
    macos::install_taoview_keyboard_fix();

    // ── Overlay window — transparent, always-on-top, hidden by default ────
    let overlay_win = WindowBuilder::new()
        .with_title("")
        .with_inner_size(LogicalSize::new(cfg.window_width, cfg.window_height))
        .with_decorations(false)
        .with_transparent(true)
        .with_always_on_top(true)
        .with_visible(false)
        .build(&event_loop)
        .expect("Failed to create overlay window");
    // Set hidesOnDeactivate — window auto-hides when app resigns active (Cmd+Tab, etc.)
    // This is how Spotlight/Raycast overlays behave.
    unsafe {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;
        let ns_win: *mut AnyObject = overlay_win.ns_window() as *mut AnyObject;
        let _: () = msg_send![ns_win, setHidesOnDeactivate: true];
    }

    let overlay_win = Arc::new(overlay_win);
    // ── Chrome window — borderless transparent layer for persistent UI ────
    // Floats above browser_win; holds sidebar, footer, notification toast.
    // Transparent areas pass clicks through to browser_win underneath.
    // Address bar + progress bar are children of browser_win (titlebar zone).
    let chrome_win = {
        let bsz = browser_win.inner_size();
        let w = WindowBuilder::new()
            .with_title("")
            .with_inner_size(tao::dpi::PhysicalSize::new(bsz.width, bsz.height))
            .with_decorations(false)
            .with_transparent(true)
            .with_visible(true)
            .build(&event_loop)
            .expect("Failed to create chrome window");
        // Position chrome_win exactly over browser_win's content area.
        if let Ok(pos) = browser_win.outer_position() {
            w.set_outer_position(pos);
        }
        unsafe {
            use objc2::msg_send;
            use objc2::runtime::AnyObject;
            let ns_win: *mut AnyObject = w.ns_window() as *mut AnyObject;
            // setHasShadow:NO — chrome layer shouldn't cast its own shadow
            let _: () = msg_send![ns_win, setHasShadow: false];
            // Transient (8 = 1<<3) → not in Mission Control.
            // FullScreenAuxiliary (256 = 1<<8) → follows parent into native
            // fullscreen instead of staying on the original desktop, which
            // would leave the sidebar/footer out of sync after ⌘Return.
            let _: () = msg_send![ns_win, setCollectionBehavior: 264u64];
            // Make chrome_win a child of browser_win — it moves/minimizes/closes with parent.
            let parent_ns: *mut AnyObject = browser_win.ns_window() as *mut AnyObject;
            // NSWindowAbove = 1
            let _: () = msg_send![parent_ns, addChildWindow: ns_win, ordered: 1i64];
        }
        w
    };
    let chrome_win = Arc::new(chrome_win);
    let chrome_win_id = chrome_win.id();

    // Resign chrome_win's key status on deactivate (so Slack-Enter doesn't
    // route back to us) and restore it on reactivate when the sidebar still
    // owned focus (so transient deactivations — banners, Spotlight — don't
    // silently steal the user's typing focus).
    unsafe {
        macos::install_app_active_observers(
            chrome_win.ns_window(),
            Arc::clone(&sidebar_owns_key),
            proxy.clone(),
        );
    }

    // ── Browser WebView factory ───────────────────────────────────────────
    // Each tab gets its own WebView (build_as_child). Hide/show to switch —
    // no reload, full state (scroll, video, JS) preserved.
    let home = cfg.home_page.clone();
    // mut: kept in sync when the user changes the engine in Settings.
    let mut search_engine = cfg.search_engine.clone();

    // Address bar lives in the macOS titlebar zone (32pt actual height). Tab WebViews start at y=0;
    // fullsize_content_view handles the titlebar inset natively.
    const ADDRESS_BAR_H_LOGICAL: f64 = 32.0;
    let address_bar_h = (ADDRESS_BAR_H_LOGICAL * browser_win.scale_factor()) as u32;

    const FOOTER_H_LOGICAL: f64 = 36.0;
    let footer_h = (FOOTER_H_LOGICAL * browser_win.scale_factor()) as u32;

    let make_webview = {
        let browser_win = Arc::clone(&browser_win);
        let proxy = proxy.clone();
        let bar_h = address_bar_h;
        let ft_h = footer_h;
        move |tab_id: usize, url: &str| -> WebView {
            let p1 = proxy.clone();
            let p2 = proxy.clone();
            let p3 = proxy.clone();
            let p4 = proxy.clone();
            let p5 = proxy.clone();
            let p6 = proxy.clone();
            let sz = browser_win.inner_size();
            let bounds = wry::Rect {
                position: tao::dpi::PhysicalPosition::new(0u32, bar_h).into(),
                size: tao::dpi::PhysicalSize::new(sz.width, sz.height.saturating_sub(bar_h + ft_h))
                    .into(),
            };
            let wv = WebViewBuilder::new()
                .with_url(url)
                .with_devtools(true)
                .with_back_forward_navigation_gestures(true)
                .with_bounds(bounds)
                // Suspend JS timers, rAF, and network on hidden tabs (macOS 14+, no-op on older)
                .with_background_throttling(BackgroundThrottlingPolicy::Suspend)
                // Safari-compatible UA so sites serve optimised WebKit assets; octoweb tag for identification
                .with_user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.3 Safari/605.1.15 Octoweb/1.0")
                // Single merged script: page stats + favicon + URL tracking +
                // media tracking + find-in-page. One JSC compile pass instead of five.
                .with_initialization_script(webview_utils::COMBINED_SCRIPT)
                // Intercept navigation to external URL schemes (tg://, figma://, mailto:, etc.)
                // and hand them off to macOS instead of loading them in the WebView.
                .with_navigation_handler(|nav_url: String| {
                    if url::is_external_scheme(&nav_url) {
                        macos::open_external_url(&nav_url);
                        return false; // prevent WebView from navigating
                    }
                    true
                })
                .with_ipc_handler(move |msg| {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(msg.body()) {
                        match v["type"].as_str() {
                            Some("favicon") => {
                                if let (Some(domain), Some(data)) =
                                (v["domain"].as_str(), v["data"].as_str())
                                {
                                    let _ = p3.send_event(AppEvent::FaviconFetched(
                                        domain.to_string(),
                                        data.to_string(),
                                    ));
                                }
                            }
                            Some("navigation_error") => {
                                if let Some(url) = v["url"].as_str() {
                                    let error = v["error"].as_str().unwrap_or("").to_string();
                                    let _ = p3.send_event(AppEvent::NavigationError(
                                        tab_id,
                                        url.to_string(),
                                        error,
                                    ));
                                }
                            }
                            Some("error_retry") => {
                                if let Some(url) = v["url"].as_str() {
                                    let _ = p3.send_event(AppEvent::NavigateTo(url.to_string()));
                                }
                            }
                            Some("media:playing") => {
                                let _ = p3.send_event(AppEvent::MediaPlaying(tab_id, true));
                            }
                            Some("media:paused") => {
                                let _ = p3.send_event(AppEvent::MediaPlaying(tab_id, false));
                            }
                            Some("quickslot_open") => {
                                if let Some(slot) = v["slot"].as_u64() {
                                    let _ = p3.send_event(AppEvent::QuickSlotOpen(slot as usize));
                                }
                            }
                            Some("quickslot_save") => {
                                if let Some(slot) = v["slot"].as_u64() {
                                    let _ = p3.send_event(AppEvent::QuickSlotSave(slot as usize));
                                }
                            }
                            Some("page_info") => {
                                let size = v["size"].as_u64().unwrap_or(0);
                                let time = v["time"].as_u64().unwrap_or(0);
                                let _ = p3.send_event(AppEvent::PageInfo(tab_id, size, time));
                            }
                            Some("find_count") => {
                                let current = v["current"].as_u64().unwrap_or(0) as usize;
                                let total = v["total"].as_u64().unwrap_or(0) as usize;
                                let _ = p3.send_event(AppEvent::FindCount(current, total));
                            }
                            Some("inline_edit_ready") => {
                                let text = v["text"].as_str().unwrap_or("").to_string();
                                // x/y may be null when the page had no real
                                // selection / no focused editable — host then
                                // picks a sensible default anchor.
                                let pos = match (v["x"].as_f64(), v["y"].as_f64()) {
                                    (Some(x), Some(y)) => Some((x, y)),
                                    _ => None,
                                };
                                let _ = p3.send_event(AppEvent::InlineEditReady(text, pos));
                            }
                            // Same-document navigation (SPA pushState/replaceState/popstate).
                            // Reuses BrowserUrlChanged so the address bar + tab history update
                            // exactly as they do for full page loads.
                            Some("url_changed") => {
                                if let Some(url) = v["url"].as_str() {
                                    let _ = p3.send_event(AppEvent::BrowserUrlChanged(
                                        tab_id,
                                        url.to_string(),
                                    ));
                                }
                            }
                            // SPA title change — MutationObserver + setter intercept in COMBINED_SCRIPT
                            Some("title_changed") => {
                                if let Some(title) = v["title"].as_str() {
                                    let _ = p3.send_event(AppEvent::TitleChanged(
                                        tab_id,
                                        title.to_string(),
                                    ));
                                }
                            }
                            Some("open_new_tab") => {
                                if let Some(url) = v["url"].as_str() {
                                    let _ = p3.send_event(AppEvent::OpenInNewTab(url.to_string()));
                                }
                            }
                            _ => {}
                        }
                    }
                })
                .with_on_page_load_handler(move |event, url| {
                    use wry::PageLoadEvent;
                    let url = url.to_string();
                    match event {
                        PageLoadEvent::Started => {
                            let _ = p1.send_event(AppEvent::BrowserUrlChanged(tab_id, url));
                            let _ = p1.send_event(AppEvent::PageLoadStarted(tab_id));
                        }
                        PageLoadEvent::Finished => {
                            let _ = p1.send_event(AppEvent::PageLoadFinished(tab_id));
                        }
                    }
                })
                .with_document_title_changed_handler(move |title| {
                    let _ = p2.send_event(AppEvent::TitleChanged(tab_id, title));
                })
                .with_new_window_req_handler(move |url, features| {
                    if features.size.is_some() {
                        // Popup with explicit dimensions (OAuth, login flows) —
                        // allow wry to create a real window so window.opener is preserved.
                        wry::NewWindowResponse::Allow
                    } else {
                        // Regular window.open() without dimensions — open in a new tab.
                        // <a target="_blank"> clicks are already intercepted by the JS
                        // listener in COMBINED_SCRIPT, but window.open() calls bypass
                        // that listener and arrive here.
                        let _ = p4.send_event(AppEvent::OpenInNewTab(url));
                        wry::NewWindowResponse::Deny
                    }
                })
                .with_download_started_handler(move |url, _path| {
                    // Extract filename from URL for the toast notification
                    let filename = url
                        .rsplit('/')
                        .next()
                        .and_then(|s| s.split('?').next())
                        .filter(|s| !s.is_empty())
                        .unwrap_or("file")
                        .to_string();
                    let _ = p5.send_event(AppEvent::DownloadStarted(tab_id, filename));
                    true // allow the download
                })
                .with_download_completed_handler(move |_url, path, success| {
                    let filename = path
                        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                        .unwrap_or_default();
                    let _ = p6.send_event(AppEvent::DownloadCompleted(filename, success));
                })
                .build_as_child(&*browser_win)
                .expect("Failed to create tab WebView");
            // Apply compiled content rules to this WebView. If the async
            // compilation hasn't finished yet, the pointer is queued and
            // applied when the completion block fires.
            let wv_ptr = objc2::rc::Retained::as_ptr(&wv.webview()) as usize;
            content_rules::apply_to_webview(wv_ptr);

            // Tune WKPreferences via the live shared reference inside configuration.
            // - Disable Google Safe Browsing: saves ~50-100ms per navigation.
            // - Enable page cache (BFCache): instant back/forward like Safari.
            //   `_usesPageCache` (WKPreferencesPrivate.h, macOS 10.13.4+) freezes
            //   pages in memory on navigation so swipe-back restores instantly
            //   instead of reloading. BFCache restores fire JS `pageshow` with
            //   `persisted=true` but do NOT fire didCommitNavigation/didFinish,
            //   so our PageLoadStarted/Finished handlers stay clean.
            unsafe {
                use objc2::msg_send;
                use objc2::runtime::AnyObject;
                let wv_raw = wv_ptr as *mut AnyObject;
                let config: *mut AnyObject = msg_send![wv_raw, configuration];
                if !config.is_null() {
                    let prefs: *mut AnyObject = msg_send![config, preferences];
                    if !prefs.is_null() {
                        let _: () = msg_send![prefs, setFraudulentWebsiteWarningEnabled: false];
                        let _: () = msg_send![prefs, _setUsesPageCache: true];
                        // Enable Web Speech API recognition. WKWebView keeps this
                        // off by default (unlike Safari) — without it the page
                        // gets `service-not-allowed`.
                        let _: () = msg_send![prefs, _setSpeechRecognitionEnabled: true];
                        // getUserMedia (mic/camera) is also gated behind a
                        // private flag in WKWebView; flip it on so dictation
                        // sites that combine getUserMedia + SpeechRecognition
                        // (Vext, etc.) can capture audio.
                        let _: () = msg_send![prefs, _setMediaDevicesEnabled: true];
                    }
                }
            }
            // Send the tab WKWebView to the back of browser_win.contentView's
            // subviews. All chrome overlays (address bar, progress, inline
            // edit, notification, footer) are siblings of tab webviews; the
            // default NSView z-order is creation order, so a tab opened AFTER
            // those overlays would paint on top of them. That hides the
            // address bar's autocomplete dropdown (which lives inside the bar
            // webview's grown 340 px region) behind whichever tab the user
            // opened most recently. Pinning each tab below its siblings keeps
            // the overlays permanently on top.
            unsafe {
                use objc2::msg_send;
                use objc2::runtime::AnyObject;
                let view: *mut AnyObject =
                    objc2::rc::Retained::as_ptr(&wv.webview()) as *mut AnyObject;
                let superview: *mut AnyObject = msg_send![view, superview];
                if !superview.is_null() {
                    // NSWindowBelow = -1: place `view` behind all existing siblings.
                    let nil_view: *mut AnyObject = std::ptr::null_mut();
                    let _: () = msg_send![
                        superview,
                        addSubview: view,
                        positioned: -1i64,
                        relativeTo: nil_view
                    ];
                }
            }
            wv
        }
    };

    let mut tab_webviews: HashMap<usize, WebView> = HashMap::new();
    // Tabs restored from session but not yet loaded (lazy loading).
    // Key = tab_id, Value = URL to load when user switches to it.
    let mut pending_tabs: HashMap<usize, String> = HashMap::new();
    let mut active_wv_id;
    let mut mru: Vec<usize> = Vec::new();
    // Deferred tab swap: (old_visible_tab, new_loading_tab).
    // Old tab stays visible while new one loads behind it (Safari-style).
    let mut pending_swap: Option<(usize, usize)> = None;

    // Favicon cache: domain → base64 data-URI, persisted across sessions.
    // favicon_order tracks insertion order for FIFO eviction at 500 entries —
    // prevents unbounded memory growth during long browsing sessions.
    // Loaded in a background thread (same pattern as history) to keep startup fast.
    const FAVICON_CAP: usize = 500;
    let mut favicon_cache: HashMap<String, String> = HashMap::new();
    let mut favicon_order: VecDeque<String> = VecDeque::new();
    {
        let p = proxy.clone();
        std::thread::spawn(move || {
            let cache = config::load_favicons();
            let _ = p.send_event(AppEvent::FaviconCacheLoaded(cache));
        });
    }

    // Frozen tab snapshots: tab_id → base64 PNG data-URI captured on tab hide.
    // Used for instant visual restore when switching to a hibernated tab.
    let mut tab_snapshots: HashMap<usize, String> = HashMap::new();
    // Deferred navigation: tab_id → real URL.  Set when a hibernated tab is
    // restored with a snapshot — the snapshot HTML loads first, then on
    // PageLoadFinished we navigate to the real URL.
    let mut deferred_nav: HashMap<usize, String> = HashMap::new();
    // Tabs currently mid-snapshot-restore.  While a tab is in this set,
    // BrowserUrlChanged events carrying "about:blank" are suppressed so the
    // real URL in TabManager is not clobbered by the transient snapshot load.
    let mut restoring_tabs: std::collections::HashSet<usize> = std::collections::HashSet::new();
    // MCP navigate: tab_id → (inserted_at, pending oneshot response).
    // Stored when browser_navigate is called; resolved when PageLoadFinished fires
    // and the DOM stabilises (SPA hydration complete).
    let mut mcp_nav_pending: HashMap<
        usize,
        (
            std::time::Instant,
            tokio::sync::oneshot::Sender<Result<usize, String>>,
        ),
    > = HashMap::new();
    let mut quick_slots = quickslots::load();

    // Restore previous session if available, otherwise open home page.
    let session = config::load_session();
    let session_tabs: Vec<config::SessionTab> = match &session {
        Some(s) if !s.tabs.is_empty() => s.tabs.clone(),
        _ => vec![config::SessionTab {
            url: home.clone(),
            title: String::new(),
        }],
    };
    let active_url = session
        .as_ref()
        .map(|s| s.active_url.as_str())
        .unwrap_or(&home)
        .to_string();

    let mut first_id: Option<usize> = None;
    let mut restored_active_id: Option<usize> = None;
    for st in &session_tabs {
        let tab_id = if st.title.is_empty() {
            tabs.lock().unwrap().open(st.url.clone())
        } else {
            tabs.lock()
                .unwrap()
                .open_with_title(st.url.clone(), st.title.clone())
        };
        // Lazy loading: only create WebView for the active tab.
        // Other tabs are stored in pending_tabs and loaded on demand.
        if st.url == active_url {
            // Active tab — create WebView immediately
            let wv = make_webview(tab_id, &st.url);
            if st.url == "about:blank" {
                let html = newtab_html::html(&quickslots::to_json(&quick_slots));
                let _ = wv.load_html(&html);
            }
            let wv_ptr = objc2::rc::Retained::as_ptr(&wv.webview()) as usize;
            nav_error_patch::inject_from_webview(wv_ptr);
            dialog_patch::inject_from_webview(wv_ptr);
            let p = proxy.clone();
            nav_error_patch::register(wv_ptr, move |url, code| {
                let _ = p.send_event(AppEvent::NavigationError(tab_id, url, code.to_string()));
            });
            let pt = proxy.clone();
            nav_error_patch::register_termination(wv_ptr, move || {
                let _ = pt.send_event(AppEvent::WebContentTerminated(tab_id));
            });
            let pd = proxy.clone();
            dialog_patch::register(wv_ptr, tab_id, move |info| {
                let _ = pd.send_event(AppEvent::JsDialog(info));
            });
            let _ = wv.set_visible(true);
            tab_webviews.insert(tab_id, wv);
            restored_active_id = Some(tab_id);
        } else {
            // Background tab — defer WebView creation until user switches to it
            pending_tabs.insert(tab_id, st.url.clone());
        }
        mru.push(tab_id);
        if first_id.is_none() {
            first_id = Some(tab_id);
        }
    }

    // Active tab is the one we just created (or first if no match)
    active_wv_id = restored_active_id.or(first_id).unwrap();
    tabs.lock().unwrap().switch(active_wv_id);
    macos::mru_push(&mut mru, active_wv_id);

    // ── Overlay WebView ───────────────────────────────────────────────────
    let overlay_wv = WebViewBuilder::new()
        .with_html(overlay_html::html())
        .with_transparent(true)
        .with_custom_protocol("octoweb-lib".into(), |_wv_id, request| {
            let path = request.uri().path().trim_start_matches('/');
            let (data, mime): (&[u8], &str) = match path {
                "fuzzysort.min.js" => (
                    include_bytes!("../assets/lib/fuzzysort.min.js"),
                    "application/javascript",
                ),
                _ => {
                    return Response::builder()
                        .status(404)
                        .body(Vec::new().into())
                        .unwrap()
                }
            };
            Response::builder()
                .header("Content-Type", mime)
                .header("Access-Control-Allow-Origin", "*")
                .body(data.to_vec().into())
                .unwrap()
        })
        .with_ipc_handler({
            let p = proxy.clone();
            let ow = Arc::clone(&overlay_win);
            let overlay_state = Arc::clone(&overlay_hotkey_visible);
            move |msg| {
                let dismiss = || {
                    ow.set_visible(false);
                    overlay_state.store(false, Ordering::Relaxed);
                };

                if let Ok(v) = serde_json::from_str::<serde_json::Value>(msg.body()) {
                    match v["type"].as_str() {
                        Some("overlay_open") => {
                            overlay_state.store(true, Ordering::Relaxed);
                        }
                        Some("overlay_close") | Some("close") => {
                            dismiss();
                            let _ = p.send_event(AppEvent::HideOverlay);
                        }
                        Some("navigate") => {
                            dismiss();
                            if let Some(url) = v["url"].as_str() {
                                let _ = p.send_event(AppEvent::NavigateTo(url.to_string()));
                            }
                        }
                        Some("switch_tab") => {
                            dismiss();
                            if let Some(tab_id) = v["tab_id"].as_u64() {
                                let _ = p.send_event(AppEvent::SwitchTab(tab_id as usize));
                            }
                        }
                        Some("close_tab") => {
                            if let Some(id) = v["tab_id"].as_u64() {
                                let _ = p.send_event(AppEvent::CloseTab(id as usize));
                            }
                        }
                        Some("remove_history") => {
                            if let Some(url) = v["url"].as_str() {
                                let _ = p.send_event(AppEvent::RemoveHistory(url.to_string()));
                            }
                        }
                        Some("ask_ai") => {
                            dismiss();
                            let _ = p.send_event(AppEvent::HideOverlay);
                            if let Some(text) = v["text"].as_str() {
                                let _ = p.send_event(AppEvent::AskAI(text.to_string()));
                            }
                        }
                        _ => {}
                    }
                }
            }
        })
        .build(&*overlay_win)
        .expect("Failed to create overlay WebView");

    // ── Shortcuts overlay window — transparent, always-on-top, hidden ─────
    let shortcuts_win = WindowBuilder::new()
        .with_title("")
        .with_inner_size(LogicalSize::new(cfg.window_width, cfg.window_height))
        .with_decorations(false)
        .with_transparent(true)
        .with_always_on_top(true)
        .with_visible(false)
        .build(&event_loop)
        .expect("Failed to create shortcuts window");
    // Set hidesOnDeactivate — window auto-hides when app resigns active (Cmd+Tab, etc.)
    unsafe {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;
        let ns_win: *mut AnyObject = shortcuts_win.ns_window() as *mut AnyObject;
        let _: () = msg_send![ns_win, setHidesOnDeactivate: true];
    }

    let shortcuts_win = Arc::new(shortcuts_win);
    let shortcuts_wv = WebViewBuilder::new()
        .with_html(shortcuts_html::html())
        .with_transparent(true)
        .with_ipc_handler({
            let p = proxy.clone();
            let sw = Arc::clone(&shortcuts_win);
            move |msg| {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(msg.body()) {
                    if v["type"].as_str() == Some("shortcuts_close") {
                        sw.set_visible(false);
                        let _ = p.send_event(AppEvent::HideShortcuts);
                    }
                }
            }
        })
        .build(&*shortcuts_win)
        .expect("Failed to create shortcuts WebView");

    // ── Settings modal (⌘,) ────────────────────────────────────────────────
    let settings_win = WindowBuilder::new()
        .with_title("")
        .with_inner_size(LogicalSize::new(cfg.window_width, cfg.window_height))
        .with_decorations(false)
        .with_transparent(true)
        .with_always_on_top(true)
        .with_visible(false)
        .build(&event_loop)
        .expect("Failed to create settings window");
    unsafe {
        use objc2::msg_send;
        use objc2::runtime::AnyObject;
        let ns_win: *mut AnyObject = settings_win.ns_window() as *mut AnyObject;
        let _: () = msg_send![ns_win, setHidesOnDeactivate: true];
    }
    let settings_win = Arc::new(settings_win);
    let settings_wv = WebViewBuilder::new()
        .with_html(settings_html::html())
        .with_transparent(true)
        .with_ipc_handler({
            let p = proxy.clone();
            let sw = Arc::clone(&settings_win);
            move |msg| {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(msg.body()) {
                    match v["type"].as_str() {
                        Some("settings_close") => {
                            sw.set_visible(false);
                            let _ = p.send_event(AppEvent::HideSettings);
                        }
                        Some("settings_update") => {
                            if let (Some(key), Some(val)) = (v["key"].as_str(), v["value"].as_str())
                            {
                                let _ = p.send_event(AppEvent::UpdateConfig(
                                    key.to_string(),
                                    val.to_string(),
                                ));
                            }
                        }
                        Some("keybind_record") => {
                            if let (Some(a), Some(c)) = (v["action"].as_str(), v["chord"].as_str())
                            {
                                let _ = p.send_event(AppEvent::KeybindRecord(
                                    a.to_string(),
                                    c.to_string(),
                                ));
                            }
                        }
                        Some("keybind_reset") => {
                            if let Some(a) = v["action"].as_str() {
                                let _ = p.send_event(AppEvent::KeybindReset(a.to_string()));
                            }
                        }
                        Some("keybind_reset_all") => {
                            let _ = p.send_event(AppEvent::KeybindResetAll);
                        }
                        Some("keybind_capture") => {
                            if let Some(on) = v["on"].as_bool() {
                                let _ = p.send_event(AppEvent::KeybindCapture(on));
                            }
                        }
                        _ => {}
                    }
                }
            }
        })
        .build(&*settings_win)
        .expect("Failed to create settings WebView");
    let mut settings_visible = false;

    // ── Sidebar WebView (child of browser_win, right-edge panel) ──────────
    // Hidden by default; shown/hidden via ToggleSidebar.
    // SIDEBAR_W is in logical points; scale to physical pixels for bounds arithmetic.
    const SIDEBAR_W_LOGICAL: f64 = 440.0;
    let sidebar_w = (SIDEBAR_W_LOGICAL * browser_win.scale_factor()) as u32;
    let sz0 = browser_win.inner_size();

    // Notification margin from right edge (logical 12pt)
    const NOTIF_MARGIN_LOGICAL: f64 = 12.0;
    let notif_margin = (NOTIF_MARGIN_LOGICAL * browser_win.scale_factor()) as u32;
    let sidebar_wv = WebViewBuilder::new()
        .with_html(sidebar_html::html(cfg.max_ai_prompt_history))
        .with_transparent(true)
        .with_custom_protocol("octoweb-lib".into(), |_wv_id, request| {
            let path = request.uri().path().trim_start_matches('/');
            let (data, mime): (&[u8], &str) = match path {
                "pdf.min.js" => (
                    include_bytes!("../assets/lib/pdf.min.js"),
                    "application/javascript",
                ),
                "pdf.worker.min.js" => (
                    include_bytes!("../assets/lib/pdf.worker.min.js"),
                    "application/javascript",
                ),
                "mammoth.browser.min.js" => (
                    include_bytes!("../assets/lib/mammoth.browser.min.js"),
                    "application/javascript",
                ),
                "marked.min.js" => (
                    include_bytes!("../assets/lib/marked.min.js"),
                    "application/javascript",
                ),
                "fuzzysort.min.js" => (
                    include_bytes!("../assets/lib/fuzzysort.min.js"),
                    "application/javascript",
                ),
                _ => {
                    return Response::builder()
                        .status(404)
                        .body(Vec::new().into())
                        .unwrap()
                }
            };
            Response::builder()
                .header("Content-Type", mime)
                .header("Access-Control-Allow-Origin", "*")
                .body(data.to_vec().into())
                .unwrap()
        })
        .with_bounds(wry::Rect {
            position: tao::dpi::PhysicalPosition::new(sz0.width.saturating_sub(sidebar_w), 0u32)
                .into(),
            size: tao::dpi::PhysicalSize::new(sidebar_w, sz0.height).into(),
        })
        .with_navigation_handler({
            let p = proxy.clone();
            move |url: String| {
                // Allow initial HTML load and internal anchors
                if url.starts_with("about:") || url.starts_with("data:") {
                    return true;
                }
                // External links → open in a browser tab instead
                if url.starts_with("http://") || url.starts_with("https://") {
                    let _ = p.send_event(AppEvent::OpenInNewTab(url));
                    return false;
                }
                true
            }
        })
        .with_ipc_handler({
            let p = proxy.clone();
            move |msg| {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(msg.body()) {
                    match v["type"].as_str() {
                        Some("acp_prompt") => {
                            let sid = v["session_id"].as_u64().unwrap_or(0);
                            let text = v["text"].as_str().unwrap_or("").to_string();
                            let mut images = Vec::new();
                            if let Some(arr) = v["images"].as_array() {
                                for img in arr {
                                    if let (Some(data), Some(mime)) =
                                        (img["data"].as_str(), img["mimeType"].as_str())
                                    {
                                        images.push((data.to_string(), mime.to_string()));
                                    }
                                }
                            }
                            if !text.is_empty() || !images.is_empty() {
                                let _ = p.send_event(AppEvent::AcpPrompt(sid, text, images));
                            }
                        }
                        Some("acp_cancel") => {
                            let sid = v["session_id"].as_u64().unwrap_or(0);
                            let _ = p.send_event(AppEvent::AcpCancel(sid));
                        }
                        Some("acp_signin") => {
                            let sid = v["session_id"].as_u64().unwrap_or(0);
                            let _ = p.send_event(AppEvent::AcpSignIn(sid));
                        }
                        Some("sidebar_close") => {
                            let _ = p.send_event(AppEvent::ToggleSidebar);
                        }
                        Some("sidebar_fullscreen_toggle") => {
                            let _ = p.send_event(AppEvent::ToggleSidebarFullscreen);
                        }
                        Some("acp_set_agent") => {
                            let sid = v["session_id"].as_u64().unwrap_or(0);
                            if let Some(tag) = v["tag"].as_str() {
                                let _ = p.send_event(AppEvent::AcpSetAgent(sid, tag.to_string()));
                            }
                        }
                        Some("acp_clear_session") => {
                            let sid = v["session_id"].as_u64().unwrap_or(0);
                            let _ = p.send_event(AppEvent::AcpClearSession(sid));
                        }
                        Some("acp_session_create") => {
                            let title = v["title"].as_str().unwrap_or("").to_string();
                            let tag = v["tag"].as_str().unwrap_or("").to_string();
                            if !tag.is_empty() {
                                let _ = p.send_event(AppEvent::AcpSessionCreate(title, tag));
                            }
                        }
                        Some("acp_session_close") => {
                            let sid = v["session_id"].as_u64().unwrap_or(0);
                            let _ = p.send_event(AppEvent::AcpSessionClose(sid));
                        }
                        Some("acp_session_switch") => {
                            let sid = v["session_id"].as_u64().unwrap_or(0);
                            let _ = p.send_event(AppEvent::AcpSessionSwitch(sid));
                        }
                        Some("acp_session_rename") => {
                            let sid = v["session_id"].as_u64().unwrap_or(0);
                            let title = v["title"].as_str().unwrap_or("").to_string();
                            let _ = p.send_event(AppEvent::AcpSessionRename(sid, title));
                        }
                        Some("sidebar_ready") => {
                            let _ = p.send_event(AppEvent::SidebarReady);
                        }
                        Some("a2ui_resolve") => {
                            let file_id = v["file_id"].as_str().unwrap_or("").to_string();
                            let action = v["action"].clone();
                            if !file_id.is_empty() {
                                if let Err(e) = a2ui_render_ui::resolve(&file_id, action) {
                                    tracing::warn!(file_id = %file_id, error = %e, "failed to resolve A2UI envelope");
                                }
                            }
                        }
                        Some("a2ui_replay_event") => {
                            // Click on a ghost surface (bash poll loop dead).
                            // Deliver the event to the agent as a synthetic
                            // prompt in this session — the agent has the
                            // surface in chat history and can resume in a
                            // fresh turn.
                            let sid = v["sid"].as_u64().unwrap_or(0);
                            let action = &v["action"];
                            let name = action["name"].as_str().unwrap_or("");
                            let surface_id = action["surfaceId"].as_str().unwrap_or("");
                            let source = action["sourceComponentId"].as_str().unwrap_or("");
                            let context_json = serde_json::to_string(&action["context"])
                                .unwrap_or_else(|_| "{}".into());
                            let data_model_json = serde_json::to_string(&action["dataModel"])
                                .unwrap_or_else(|_| "{}".into());
                            let prompt = format!(
                                "[A2UI event — out-of-band click on prior surface]\nThe user clicked a button on a surface whose `render_ui` poll has already exited (fire-and-forget update, replay, or prior session). The click is below — treat it as the user's choice and **do the work the event implies right now** (post the tweet, save the draft, advance the wizard, etc.). Then call `render_ui` again with the SAME `surfaceId` AND `await_events` listing the remaining choices so the next click resolves normally instead of routing through here again.\n\nsurfaceId: {surface_id}\nevent: {name}\nsourceComponentId: {source}\ncontext: {context_json}\ndataModel: {data_model_json}"
                            );
                            if sid != 0 {
                                let _ = p.send_event(AppEvent::AcpPrompt(sid, prompt, vec![]));
                            }
                        }
                        Some("a2ui_open_url") => {
                            // A2UI `Button.action.openUrl` — open in a new browser tab.
                            if let Some(url) = v["url"].as_str() {
                                let _ = p.send_event(AppEvent::OpenInNewTab(url.to_string()));
                            }
                        }
                        Some("copy_text") => {
                            if let Some(text) = v["text"].as_str() {
                                unsafe {
                                    use objc2::runtime::AnyObject;
                                    use objc2::{class, msg_send};
                                    let pb: *mut AnyObject =
                                        msg_send![class!(NSPasteboard), generalPasteboard];
                                    let _: () = msg_send![pb, clearContents];
                                    let ns_str: *mut AnyObject = msg_send![class!(NSString), alloc];
                                    let ns_str: *mut AnyObject = msg_send![
                                        ns_str,
                                        initWithBytes: text.as_ptr(),
                                        length: text.len(),
                                        encoding: 4u64 // NSUTF8StringEncoding
                                    ];
                                    let ns_str = objc2::rc::Retained::from_raw(ns_str)
                                        .expect("NSString initWithBytes failed");
                                    let arr: *mut AnyObject = msg_send![
                                        class!(NSArray),
                                        arrayWithObject: &*ns_str
                                    ];
                                    let _: bool = msg_send![pb, writeObjects: arr];
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
        })
        .build_as_child(&*chrome_win)
        .expect("Failed to create sidebar WebView");
    let _ = sidebar_wv.set_visible(false);

    // ── Address bar WebView (titlebar zone — URL + stats + 🐙 toggle) ─────
    // Child of browser_win so macOS traffic lights render ON TOP natively.
    // Window corner rounding and titlebar glass effect handled by macOS.
    let address_bar_wv = WebViewBuilder::new()
        .with_html(address_bar_html::html())
        .with_transparent(true)
        .with_bounds(wry::Rect {
            position: tao::dpi::PhysicalPosition::new(0u32, 0u32).into(),
            size: tao::dpi::PhysicalSize::new(sz0.width, address_bar_h).into(),
        })
        .with_ipc_handler({
            let p = proxy.clone();
            let bw = Arc::clone(&browser_win);
            move |msg| {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(msg.body()) {
                    match v["type"].as_str() {
                        Some("begin_window_drag") => {
                            // Delegate to native macOS performWindowDragWithEvent:.
                            // It uses the OS drag threshold (~3px), so a click without
                            // intent to drag won't move the window — fixes accidental
                            // window movement when users click into the bar trying to
                            // reach content below.
                            unsafe {
                                use objc2::runtime::AnyObject;
                                use objc2::{class, msg_send};
                                let app: *mut AnyObject =
                                    msg_send![class!(NSApplication), sharedApplication];
                                let event: *mut AnyObject = msg_send![app, currentEvent];
                                if !event.is_null() {
                                    let ns_win: *mut AnyObject = bw.ns_window() as *mut AnyObject;
                                    let _: () =
                                        msg_send![ns_win, performWindowDragWithEvent: event];
                                }
                            }
                        }
                        Some("toggle_sidebar") => {
                            let _ = p.send_event(AppEvent::ToggleSidebar);
                        }
                        Some("toggle_overlay") => {
                            let _ = p.send_event(AppEvent::ToggleOverlay);
                        }
                        Some("close_tab") => {
                            let _ = p.send_event(AppEvent::CloseTab(0));
                        }
                        Some("copy_text") => {
                            if let Some(text) = v["text"].as_str() {
                                unsafe {
                                    use objc2::runtime::AnyObject;
                                    use objc2::{class, msg_send};
                                    let pb: *mut AnyObject =
                                        msg_send![class!(NSPasteboard), generalPasteboard];
                                    let _: () = msg_send![pb, clearContents];
                                    let ns_str: *mut AnyObject = msg_send![class!(NSString), alloc];
                                    let ns_str: *mut AnyObject = msg_send![
                                        ns_str,
                                        initWithBytes: text.as_ptr(),
                                        length: text.len(),
                                        encoding: 4u64 // NSUTF8StringEncoding
                                    ];
                                    let ns_str = objc2::rc::Retained::from_raw(ns_str)
                                        .expect("NSString initWithBytes failed");
                                    let arr: *mut AnyObject = msg_send![
                                        class!(NSArray),
                                        arrayWithObject: &*ns_str
                                    ];
                                    let _: bool = msg_send![pb, writeObjects: arr];
                                }
                            }
                        }
                        Some("toggle_settings") => {
                            let _ = p.send_event(AppEvent::ToggleSettings);
                        }
                        Some("toggle_shortcuts") => {
                            let _ = p.send_event(AppEvent::ToggleShortcuts);
                        }
                        Some("url_edit_open") => {
                            // Address bar opened edit mode — push history snapshot for autocomplete.
                            let _ = p.send_event(AppEvent::PushAddressHistory);
                        }
                        Some("url_edit_expand") => {
                            // Grow / shrink the address bar webview so the suggestion dropdown
                            // can paint below the 32 px titlebar without being clipped.
                            let expanded = v["expanded"].as_bool().unwrap_or(false);
                            let _ = p.send_event(AppEvent::UrlEditExpand(expanded));
                        }
                        Some("navigate") => {
                            // Submitted URL/query from the address bar edit field.
                            if let Some(url) = v["url"].as_str() {
                                let _ = p.send_event(AppEvent::NavigateTo(url.to_string()));
                            }
                        }
                        _ => {}
                    }
                }
            }
        })
        .with_custom_protocol("octoweb-sys".into(), {
            let cpu = Arc::clone(&sys_stats_cpu);
            let mem = Arc::clone(&sys_stats_mem);
            move |_webview_id, _request| {
                let cpu_pct = cpu.load(Ordering::Relaxed);
                let mem_mb = mem.load(Ordering::Relaxed);
                let json = format!("{{\"cpu_pct\":{},\"mem_mb\":{}}}", cpu_pct, mem_mb);
                Response::builder()
                    .header("Content-Type", "application/json")
                    .header("Access-Control-Allow-Origin", "*")
                    .body(json.into_bytes().into())
                    .unwrap()
            }
        })
        .build_as_child(&*browser_win)
        .expect("Failed to create address bar WebView");
    // Initialize address bar with the active tab's URL, title, and favicon from session.
    {
        let tm = tabs.lock().unwrap();
        let tab = tm.tabs().iter().find(|t| t.id == active_wv_id);
        let url = tab.map(|t| t.url.clone()).unwrap_or_default();
        let title = tab.map(|t| t.title.clone()).unwrap_or_default();
        drop(tm);
        let secure = url.starts_with("https://");
        let escaped_url = webview_utils::escape_js_template(&url);
        let escaped_title = webview_utils::escape_js_template(&title);
        let _ = address_bar_wv.evaluate_script(&format!(
            "window.__update && window.__update(`{escaped_url}`, {secure}, `{escaped_title}`, 0, 0)"
        ));
        if let Some(fav) = webview_utils::cached_favicon(&url, &favicon_cache) {
            let escaped_fav = webview_utils::escape_js_template(fav);
            let _ = address_bar_wv.evaluate_script(&format!(
                "window.__setFavicon && window.__setFavicon(`{escaped_fav}`)"
            ));
        }
        // Set window title from restored session
        if !title.is_empty() {
            browser_win.set_title(&title);
        }
    }

    // ── Progress bar WebView (thin bar at bottom edge of address bar) ─────
    // Child of browser_win (same as address bar) so it layers correctly.
    let progress_wv = WebViewBuilder::new()
        .with_html(progress_bar_html::html())
        .with_transparent(true)
        .with_bounds(wry::Rect {
            position: tao::dpi::PhysicalPosition::new(0u32, address_bar_h).into(),
            size: tao::dpi::PhysicalSize::new(sz0.width, 3u32).into(),
        })
        .build_as_child(&*browser_win)
        .expect("Failed to create progress WebView");
    let _ = progress_wv.set_visible(false);

    let mut progress_visible = false;
    // Instant when __finish was called — we hide progress_wv after the CSS fade (400ms)
    let mut progress_hide_at: Option<std::time::Instant> = None;

    // ── Find bar WebView (⌘F — pill-shaped bar at top-right) ──────────────
    const FIND_BAR_W_LOGICAL: f64 = 320.0;
    const FIND_BAR_H_LOGICAL: f64 = 36.0;
    let find_bar_w = (FIND_BAR_W_LOGICAL * browser_win.scale_factor()) as u32;
    let find_bar_h = (FIND_BAR_H_LOGICAL * browser_win.scale_factor()) as u32;
    let find_bar_wv = WebViewBuilder::new()
        .with_html(find_bar_html::html())
        .with_transparent(true)
        .with_bounds(wry::Rect {
            position: tao::dpi::PhysicalPosition::new(
                sz0.width.saturating_sub(find_bar_w + 8),
                address_bar_h,
            )
            .into(),
            size: tao::dpi::PhysicalSize::new(find_bar_w, find_bar_h).into(),
        })
        .with_ipc_handler({
            let p = proxy.clone();
            move |msg| {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(msg.body()) {
                    match v["type"].as_str() {
                        Some("find_query") => {
                            if let Some(q) = v["query"].as_str() {
                                let _ = p.send_event(AppEvent::FindInPage(q.to_string()));
                            }
                        }
                        Some("find_next") => {
                            let _ = p.send_event(AppEvent::FindNext);
                        }
                        Some("find_prev") => {
                            let _ = p.send_event(AppEvent::FindPrev);
                        }
                        Some("find_close") => {
                            let _ = p.send_event(AppEvent::HideFindBar);
                        }
                        _ => {}
                    }
                }
            }
        })
        .build_as_child(&*chrome_win)
        .expect("Failed to create find bar WebView");
    let _ = find_bar_wv.set_visible(false);
    let mut find_bar_visible = false;

    // ── Inline AI edit modal (⌘⇧E) ──────────────────────────────────────
    const INLINE_EDIT_W_LOGICAL: f64 = 350.0;
    const INLINE_EDIT_H_LOGICAL: f64 = 36.0;
    let inline_edit_w = (INLINE_EDIT_W_LOGICAL * browser_win.scale_factor()) as u32;
    let inline_edit_h = (INLINE_EDIT_H_LOGICAL * browser_win.scale_factor()) as u32;
    let inline_edit_wv = WebViewBuilder::new()
        .with_html(inline_edit_html::html())
        .with_transparent(true)
        .with_bounds(wry::Rect {
            position: tao::dpi::PhysicalPosition::new(
                (sz0.width.saturating_sub(inline_edit_w)) / 2,
                address_bar_h,
            )
            .into(),
            size: tao::dpi::PhysicalSize::new(inline_edit_w, inline_edit_h).into(),
        })
        .with_ipc_handler({
            let p = proxy.clone();
            move |msg| {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(msg.body()) {
                    match v["type"].as_str() {
                        Some("inline_edit_submit") => {
                            if let Some(prompt) = v["prompt"].as_str() {
                                let _ =
                                    p.send_event(AppEvent::InlineEditSubmit(prompt.to_string()));
                            }
                        }
                        Some("inline_edit_close") => {
                            let _ = p.send_event(AppEvent::InlineEditClose);
                        }
                        Some("inline_edit_hide") => {
                            let _ = p.send_event(AppEvent::InlineEditHide);
                        }
                        Some("inline_edit_resize") => {
                            if let Some(h) = v["height"].as_f64() {
                                let _ = p.send_event(AppEvent::InlineEditResize(h));
                            }
                        }
                        _ => {}
                    }
                }
            }
        })
        .build_as_child(&*chrome_win)
        .expect("Failed to create inline edit WebView");
    let _ = inline_edit_wv.set_visible(false);
    let mut inline_edit_visible = false;
    // True from the moment a ⌘⇧E request is issued until the modal is shown
    // (or the request is abandoned). Guards against a double-press toggle race
    // where the second press arrives before `InlineEditReady` flips
    // `inline_edit_visible = true`, which previously caused the modal to
    // either capture twice or — worse — immediately close itself when the
    // user only meant to open it once.
    let mut inline_edit_pending = false;
    let mut inline_edit_selected_text = String::new();
    let mut inline_edit_tab_id: usize = 0;
    let mut inline_edit_acp: Option<acp::AcpHandle> = None;
    let mut inline_edit_response = String::new();
    let mut prompt_history: Vec<String> = config::load_prompt_history();
    prompt_history.truncate(cfg.max_prompt_history);
    let mut ai_prompt_history: Vec<String> = config::load_ai_prompt_history();
    ai_prompt_history.truncate(cfg.max_ai_prompt_history);

    // ── Per-tab WebContent process stats (CPU%, RSS) ───────────────────────
    // Sampled every 2 s from the active tab's WebContent XPC process.
    let mut sys_stats_last: Option<tab_stats::TabStatsSample> = None;
    let mut sys_stats_next_at = std::time::Instant::now();
    // System memory-pressure poll cadence. `host_statistics64` is a Mach
    // syscall — sampling it on every event-loop tick (~60Hz) is wasteful.
    // 2s is plenty for hibernation decisions.
    let mut mem_pressure_next_at = std::time::Instant::now();
    let mut cached_pressure = hibernation::MemoryPressure::Normal;
    // Tabs with active media playback — used to throttle sys_stats polling
    let mut media_playing_tabs: std::collections::HashSet<usize> = std::collections::HashSet::new();

    // ── Crash diagnostics: periodic health log ────────────────────────────
    let app_start_instant = std::time::Instant::now();
    let mut health_log_next_at = std::time::Instant::now() + std::time::Duration::from_secs(30);

    // ── Debounced history persistence: save 60 s after first mutation ─────
    let mut history_save_at: Option<std::time::Instant> = None;

    // ── Proactive hibernation: runs every 60 s independent of memory pressure ─
    // Complementary to the reactive hibernation (which only fires under pressure).
    // Default is the modern-laptop friendly curve (8 / 16 / 32 / 64 GB anchors
    // with linear interpolation); cfg.aggressive_hibernation flips to the legacy
    // sqrt scaling for tight-RAM setups or eager reclamation.
    let mut proactive_config = hibernation::ProactiveConfig::from_total_memory(
        hibernation::total_system_memory(),
        cfg.aggressive_hibernation,
    );
    tracing::info!(
        ?proactive_config,
        aggressive = cfg.aggressive_hibernation,
        "Proactive hibernation config (RAM-scaled)"
    );
    let mut proactive_hiber_next_at =
        std::time::Instant::now() + std::time::Duration::from_secs(60);

    // First-run welcome toast: fires once 1.5 s after launch (gives the notif
    // webview time to load), then flips first_run_completed so it never shows
    // again on this profile.
    let mut welcome_toast_at: Option<std::time::Instant> = if !cfg.first_run_completed {
        Some(std::time::Instant::now() + std::time::Duration::from_millis(1500))
    } else {
        None
    };

    // ── Notification toast WebView (Tahoe-style banner at top-center) ─────
    const NOTIF_W_LOGICAL: f64 = 360.0;
    const NOTIF_H_LOGICAL: f64 = 64.0;
    let scale = browser_win.scale_factor();
    let notif_w = (NOTIF_W_LOGICAL * scale) as u32;
    let notif_h = (NOTIF_H_LOGICAL * scale) as u32;
    let notification_wv = WebViewBuilder::new()
        .with_html(notification_html::html())
        .with_transparent(true)
        .with_bounds(wry::Rect {
            position: tao::dpi::PhysicalPosition::new(
                sz0.width.saturating_sub(notif_w + notif_margin),
                0u32,
            )
            .into(),
            size: tao::dpi::PhysicalSize::new(notif_w, notif_h).into(),
        })
        .with_ipc_handler({
            let p = proxy.clone();
            move |msg| {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(msg.body()) {
                    match v["type"].as_str() {
                        Some("open_sidebar") => {
                            let _ = p.send_event(AppEvent::ToggleSidebar);
                        }
                        Some("dismiss_notification") => {
                            let _ = p.send_event(AppEvent::DismissNotification);
                        }
                        _ => {}
                    }
                }
            }
        })
        .build_as_child(&*chrome_win)
        .expect("Failed to create notification WebView");
    let _ = notification_wv.set_visible(false);
    let mut notification_visible = false;

    // ── Quick-slots footer bar (static strip at bottom — page content ends above it) ──
    let footer_wv = WebViewBuilder::new()
        .with_html(quickslots_html::html())
        .with_transparent(true)
        .with_bounds(wry::Rect {
            position: tao::dpi::PhysicalPosition::new(0u32, sz0.height.saturating_sub(footer_h))
                .into(),
            size: tao::dpi::PhysicalSize::new(sz0.width, footer_h).into(),
        })
        .with_ipc_handler({
            let p = proxy.clone();
            move |msg| {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(msg.body()) {
                    match v["type"].as_str() {
                        Some("quickslot_open") => {
                            if let Some(slot) = v["slot"].as_u64() {
                                let _ = p.send_event(AppEvent::QuickSlotOpen(slot as usize));
                            }
                        }
                        Some("quickslot_remove") => {
                            if let Some(slot) = v["slot"].as_u64() {
                                let _ = p.send_event(AppEvent::QuickSlotRemove(slot as usize));
                            }
                        }
                        Some("quickslot_save") => {
                            if let Some(slot) = v["slot"].as_u64() {
                                let _ = p.send_event(AppEvent::QuickSlotSave(slot as usize));
                            }
                        }
                        _ => {}
                    }
                }
            }
        })
        .build_as_child(&*browser_win)
        .expect("Failed to create quickslots footer WebView");
    // Initialize footer with saved slots
    {
        let json = quickslots::to_json(&quick_slots);
        let _ = footer_wv.evaluate_script(&format!(
            "window.__updateSlots && window.__updateSlots({json})"
        ));
    }

    // ── ACP sessions — multiple parallel agent connections ─────────────────
    // Each session owns its own `octomind acp <tag>` subprocess. The sidebar shows
    // them as tabs in its header strip; switching toggles which DOM message log is
    // visible (the others remain mounted as detached fragments).
    //
    // At least one session always exists (the default `octoweb:assistant` —
    // matches pre-multi-session behavior on startup).
    let acp_proxy = proxy.clone();
    let make_wake = |proxy: tao::event_loop::EventLoopProxy<AppEvent>| {
        move || {
            let _ = proxy.send_event(AppEvent::AcpWake);
        }
    };

    // ── A2UI ────────────────────────────────────────────────────────────────
    // Ship the `render_ui` tool into `<workspace>/.agents/tools/` so any
    // octomind subprocess we spawn (CWD = workspace) auto-discovers it. Then start a
    // background watcher on the envelope queue dir; every status transition
    // becomes an `A2uiUpdate` event the active session renders inline.
    match a2ui_render_ui::install() {
        Ok(p) => tracing::info!(path = %p.display(), "installed render_ui tool"),
        Err(e) => tracing::warn!(error = %e, "render_ui install failed — A2UI surfaces disabled"),
    }
    {
        let proxy = acp_proxy.clone();
        a2ui_watcher::start(a2ui_render_ui::queue_dir(), move |snap| {
            let _ = proxy.send_event(AppEvent::A2uiUpdate(snap));
        });
        std::thread::spawn(|| {
            a2ui_render_ui::prune_old(&a2ui_render_ui::queue_dir(), 86_400);
        });
    }
    // Restore persisted ACP history (sessions + per-session message log) if
    // present, else start with a fresh default Assistant session. `--resume`
    // is passed when an `acp_session_id` was captured before — the agent then
    // restores its in-memory conversation context, matching what the user
    // sees in the replayed message log.
    let persisted_acp = config::load_acp_history();
    let (mut sessions, mut next_session_id, mut active_session_id) = match persisted_acp {
        Some(h) if !h.sessions.is_empty() => {
            let mut sessions: Vec<AcpSession> = Vec::with_capacity(h.sessions.len());
            let mut max_id: u64 = 0;
            let cap = cfg.max_acp_session_messages;
            // Only spawn the active session's subprocess at startup. Others
            // connect lazily on first switch — keeps cold-start fast and
            // baseline CPU/RAM low when many sessions are persisted.
            let active_target = h.active_id;
            for mut snap in h.sessions.into_iter() {
                if snap.id > max_id {
                    max_id = snap.id;
                }
                // Defensive cap on load — if the file was hand-edited or the
                // cap was lowered between runs, we don't want to bring oversized
                // history back into memory. FIFO drop matches push_acp_msg.
                if snap.messages.len() > cap {
                    let drop = snap.messages.len() - cap;
                    snap.messages.drain(..drop);
                }
                let handle = if snap.id == active_target {
                    let cmd = match &snap.acp_session_id {
                        Some(id) => format!("octomind acp {} --resume {}", snap.tag, id),
                        None => format!("octomind acp {}", snap.tag),
                    };
                    acp::AcpHandle::connect(&cmd, make_wake(acp_proxy.clone())).ok()
                } else {
                    None // lazy: connect on AcpSessionSwitch
                };
                sessions.push(AcpSession {
                    id: snap.id,
                    title: snap.title,
                    tag: snap.tag,
                    handle,
                    retry_count: 0,
                    reconnect_gen: 0,
                    last_cancel_at: None,
                    acp_session_id: snap.acp_session_id,
                    messages: snap.messages,
                    agent_buf: String::new(),
                    tool_buf: Vec::new(),
                    tool_starts: HashMap::new(),
                    turn_started: None,
                    available_commands_json: None,
                    account_json: None,
                    octomind_pid: None,
                });
            }
            let active = if sessions.iter().any(|s| s.id == h.active_id) {
                h.active_id
            } else {
                sessions[0].id
            };
            (sessions, max_id + 1, active)
        }
        _ => {
            let default_session = AcpSession {
                id: 1,
                title: "Assistant".to_string(),
                tag: "octoweb:assistant".to_string(),
                handle: acp::AcpHandle::connect(
                    "octomind acp octoweb:assistant",
                    make_wake(acp_proxy.clone()),
                )
                .ok(),
                retry_count: 0,
                reconnect_gen: 0,
                last_cancel_at: None,
                acp_session_id: None,
                messages: Vec::new(),
                agent_buf: String::new(),
                tool_buf: Vec::new(),
                tool_starts: HashMap::new(),
                turn_started: None,
                available_commands_json: None,
                account_json: None,
                octomind_pid: None,
            };
            (vec![default_session], 2u64, 1u64)
        }
    };

    // ACP reconnection backoff bounds (shared across all sessions — backoff state
    // itself lives per-session in `AcpSession::retry_count` / `reconnect_gen`).
    const ACP_MAX_RETRIES: u32 = 5;
    const ACP_BASE_DELAY_SECS: u64 = 1;
    const ACP_MAX_DELAY_SECS: u64 = 30;

    // ── Proactive learning — background agent that memorizes browsing patterns ─
    let mut learning_handle: Option<acp::AcpHandle> = None;
    let mut learning_next_at: Option<std::time::Instant> = if cfg.proactive_learning {
        // First run after 2 minutes so there's some history to analyze
        Some(std::time::Instant::now() + std::time::Duration::from_secs(120))
    } else {
        None
    };

    // ── MCP server — exposes browser control tools on localhost:3434 ───────
    let mut mcp_handle = Some(mcp::spawn_mcp_server());
    // Cloned sender used by JS-callback watchdogs to re-issue read commands
    // (Snapshot/GetPageContent/GetPageInfo) when wry's
    // `evaluateJavaScript:completionHandler:` block is discarded by WKWebView
    // mid-eval (SPA navigation, JS-context teardown, parallel evals racing).
    let mcp_cmd_tx = mcp_handle
        .as_ref()
        .expect("mcp_handle just initialized")
        .command_tx
        .clone();

    // ── In-app shortcuts via NSEvent local monitor ───────────────────────
    // Local monitors observe key events delivered to *our* app's run loop
    // before they reach the responder chain (WKWebView, text fields, etc.).
    // Returning `nil` consumes the event, returning the event passes it on.
    //
    // Why not CGEventTap: it requires Accessibility permission, which macOS
    // ties to the binary's code-signature hash. Every rebuild invalidates
    // the grant silently and the tap goes dead with no error. Local monitors
    // are AppKit-level, need zero permissions, survive rebuilds, and macOS
    // only routes key events here when octoweb is the frontmost app — so we
    // no longer need an `is_app_active()` guard inside the handler either.
    //
    // Configurable bindings: the monitor reads this shared keymap on every
    // keystroke; remapping from the settings panel takes a write lock and
    // rebuilds it, so changes apply on the next keypress without a restart.
    let keymap = Arc::new(std::sync::RwLock::new(keybindings::Keymap::load()));
    let _key_monitor: *mut objc2::runtime::AnyObject = {
        use block2::RcBlock;
        use objc2::runtime::AnyObject;
        use objc2::{class, msg_send};

        // NSEventMaskKeyDown = 1 << NSEventTypeKeyDown(10)
        const MASK_KEYDOWN: u64 = 1 << 10;
        // NSEventModifierFlags bits (see NSEvent.h)
        const FLAG_SHIFT: u64 = 1 << 17;
        const FLAG_CONTROL: u64 = 1 << 18;
        const FLAG_OPTION: u64 = 1 << 19;
        const FLAG_COMMAND: u64 = 1 << 20;

        // Esc and the digit row are handled as a fixed fallback below; every
        // other global shortcut is data-driven via the configurable Keymap.
        const ESC_KEYCODE: u16 = 53;
        // ⌘1–⌘9 then ⌘0 → quickslots (⌘⇧+digit saves). Positional, not remappable.
        const DIGIT_KEYCODES: [u16; 10] = [18, 19, 20, 21, 23, 22, 26, 28, 25, 29];

        let p = proxy.clone();
        let km = Arc::clone(&keymap);
        let capturing = Arc::clone(&keybind_capturing);
        let overlay_state = Arc::clone(&overlay_hotkey_visible);
        let find_bar_state = Arc::clone(&find_bar_hotkey_visible);
        let inline_edit_state = Arc::clone(&inline_edit_hotkey_visible);
        let sidebar_state = Arc::clone(&sidebar_hotkey_visible);

        let handler = RcBlock::new(move |event: *mut AnyObject| -> *mut AnyObject {
            if event.is_null() {
                return event;
            }
            // SAFETY: KeyDown mask guarantees `event` is an NSEvent of that type.
            let keycode: u16 = unsafe { msg_send![event, keyCode] };
            let flags: u64 = unsafe { msg_send![event, modifierFlags] };
            let cmd = flags & FLAG_COMMAND != 0;
            let ctrl = flags & FLAG_CONTROL != 0;
            let shift = flags & FLAG_SHIFT != 0;
            let opt = flags & FLAG_OPTION != 0;

            let consume = std::ptr::null_mut::<AnyObject>();
            let pass = event;

            // While the settings panel is recording a chord, let every key reach
            // the focused WebView so it can be captured (don't fire shortcuts).
            if capturing.load(Ordering::Relaxed) {
                return pass;
            }

            let mut mods = 0u8;
            if cmd {
                mods |= keybindings::MOD_CMD;
            }
            if ctrl {
                mods |= keybindings::MOD_CTRL;
            }
            if opt {
                mods |= keybindings::MOD_OPT;
            }
            if shift {
                mods |= keybindings::MOD_SHIFT;
            }

            // Configurable bindings take priority. A chord matched to an action
            // that's context-guarded in this UI state (e.g. scrolling while the
            // command palette is open) falls through so the key still reaches
            // the focused WebView — mirroring the previous hardcoded behavior.
            let action = km.read().ok().and_then(|k| k.lookup(mods, keycode));
            if let Some(action) = action {
                if let Some(ev) = keybind_to_event(
                    action,
                    overlay_state.load(Ordering::Relaxed),
                    find_bar_state.load(Ordering::Relaxed),
                    inline_edit_state.load(Ordering::Relaxed),
                    sidebar_state.load(Ordering::Relaxed),
                ) {
                    let _ = p.send_event(ev);
                    return consume;
                }
            }

            // Fixed fallback: ⌘ + digit opens a quickslot, ⌘⇧ + digit saves one.
            if cmd && !overlay_state.load(Ordering::Relaxed) {
                if let Some(slot) = DIGIT_KEYCODES.iter().position(|&k| k == keycode) {
                    let _ = p.send_event(if shift {
                        AppEvent::QuickSlotSave(slot)
                    } else {
                        AppEvent::QuickSlotOpen(slot)
                    });
                    return consume;
                }
            }

            // Esc closes the find bar when it owns focus (contextual, not remappable).
            if keycode == ESC_KEYCODE && find_bar_state.load(Ordering::Relaxed) {
                let _ = p.send_event(AppEvent::HideFindBar);
                return consume;
            }

            pass
        });

        // AppKit copies the block when registering the monitor, so forgetting
        // the RcBlock keeps the underlying allocation alive for the app's
        // lifetime without leaking a strong ref the wrong way.
        let monitor: *mut AnyObject = unsafe {
            msg_send![
                class!(NSEvent),
                addLocalMonitorForEventsMatchingMask: MASK_KEYDOWN,
                handler: &*handler,
            ]
        };
        std::mem::forget(handler);
        monitor
    };

    let mut modifiers = ModifiersState::default();
    let mut overlay_visible = false;
    let mut sidebar_visible = false;
    let mut sidebar_fullscreen = false; // sidebar takes the full chrome window width when true
    let mut shortcuts_visible = false;
    // Sessions with a login flow in flight → the poller thread's stop flag. Set
    // when `/login` starts, cleared (and the session respawned to load the new
    // hub key) once `/usage` reports signed-in. Transient, not per-session state.
    let mut login_pollers: std::collections::HashMap<
        u64,
        std::sync::Arc<std::sync::atomic::AtomicBool>,
    > = std::collections::HashMap::new();
    // Address bar URL edit mode — when true, the address bar webview is grown
    // to full window height so the autocomplete dropdown and backdrop can
    // paint outside the 32 px titlebar strip without being clipped.
    let mut address_bar_editing = false;
    let mut icon_set = false;
    let mut zoom_level: f64 = 1.0;

    // ── Helper macros (expand in-place, access event-loop locals) ─────────

    /// Push the current keybindings into both the settings panel (so the
    /// Keybindings tab re-renders) and the shortcuts overlay (so the help
    /// reflects live mappings). `$err` is an `Option<String>` surfaced as an
    /// inline error in the settings UI (e.g. a rejected conflicting chord).
    macro_rules! sync_keybindings {
        ($err:expr) => {{
            if let Ok(k) = keymap.read() {
                let mut v = k.ui_json();
                if let Some(msg) = $err {
                    v["error"] = serde_json::Value::String(msg);
                }
                let json = v.to_string();
                let _ = settings_wv.evaluate_script(&format!(
                    "window.__setKeybindings && window.__setKeybindings({})",
                    json
                ));
                let _ = shortcuts_wv.evaluate_script(&format!(
                    "window.__setShortcuts && window.__setShortcuts({})",
                    json
                ));
            }
        }};
    }

    /// Create a WebView for a new tab, register nav-error callback, insert into map.
    /// Returns the tab_id. The WebView starts hidden (shown on PageLoadFinished).
    macro_rules! spawn_tab_webview {
        ($tab_id:expr, $url:expr) => {{
            let id = $tab_id;
            let wv = make_webview(id, $url);
            let _ = wv.set_visible(false);
            let wv_ptr = objc2::rc::Retained::as_ptr(&wv.webview()) as usize;
            let p = proxy.clone();
            nav_error_patch::register(wv_ptr, move |url, code| {
                let _ = p.send_event(AppEvent::NavigationError(id, url, code.to_string()));
            });
            let pt = proxy.clone();
            nav_error_patch::register_termination(wv_ptr, move || {
                let _ = pt.send_event(AppEvent::WebContentTerminated(id));
            });
            let pd = proxy.clone();
            dialog_patch::register(wv_ptr, id, move |info| {
                let _ = pd.send_event(AppEvent::JsDialog(info));
            });
            tab_webviews.insert(id, wv);
            if zoom_level != 1.0 {
                if let Some(wv) = tab_webviews.get(&id) {
                    let _ = wv.zoom(zoom_level);
                }
            }
            id
        }};
    }

    /// Lazy-load: ensure a WebView exists for `target`, creating it from `pending_tabs`
    /// if hibernated/session-restored. Registers nav_error / dialog / termination handlers
    /// and restores any frozen snapshot. Does NOT change visibility, active tab, or
    /// address bar — caller is responsible for those if needed.
    /// No-op if the WebView already exists or the tab id is unknown.
    macro_rules! ensure_webview {
        ($target:expr) => {{
            let target = $target;
            if !tab_webviews.contains_key(&target) {
                if let Some(url) = pending_tabs.remove(&target) {
                    let has_snapshot = tab_snapshots.contains_key(&target);
                    let load_url = if has_snapshot { "about:blank" } else { url.as_str() };
                    let wv = make_webview(target, load_url);
                    if !has_snapshot && url == "about:blank" {
                        let html = newtab_html::html(&quickslots::to_json(&quick_slots));
                        let _ = wv.load_html(&html);
                    }
                    let wv_ptr = objc2::rc::Retained::as_ptr(&wv.webview()) as usize;
                    nav_error_patch::inject_from_webview(wv_ptr);
                    dialog_patch::inject_from_webview(wv_ptr);
                    let p = proxy.clone();
                    nav_error_patch::register(wv_ptr, move |url, code| {
                        let _ =
                            p.send_event(AppEvent::NavigationError(target, url, code.to_string()));
                    });
                    let pt = proxy.clone();
                    nav_error_patch::register_termination(wv_ptr, move || {
                        let _ = pt.send_event(AppEvent::WebContentTerminated(target));
                    });
                    let pd = proxy.clone();
                    dialog_patch::register(wv_ptr, target, move |info| {
                        let _ = pd.send_event(AppEvent::JsDialog(info));
                    });
                    // If a snapshot exists, show it immediately — deferred navigation to
                    // the real URL happens on PageLoadFinished once the snapshot renders.
                    if let Some(snap) = tab_snapshots.remove(&target) {
                        let html = format!(
                            "<html><head><style>*{{margin:0;padding:0}}body{{overflow:hidden}}img{{display:block;width:100vw;height:100vh;object-fit:cover;object-position:top left}}</style></head><body><img src=\"{}\"></body></html>",
                            snap
                        );
                        let _ = wv.load_html(&html);
                        deferred_nav.insert(target, url);
                        restoring_tabs.insert(target);
                    }
                    let _ = wv.set_visible(false);
                    tab_webviews.insert(target, wv);
                }
            }
        }};
    }

    /// Watchdog for JS-callback handlers that can't safely retry (writes:
    /// Click/Hover/Type/PressKey/SelectOption/ExecuteJs/Wait).
    ///
    /// wry's `evaluate_script_with_callback` registers a
    /// `WKWebView.evaluateJavaScript:completionHandler:` block. WKWebView
    /// discards the block (without firing it) if the JS context tears down
    /// mid-eval — SPA navigation, parallel evals racing against an SPA
    /// re-render, or WebContent process recycling. The closure capturing
    /// the response Sender is then dropped, the Sender drops without
    /// sending, and the MCP receiver gets `RecvError` ("channel closed").
    ///
    /// This macro spawns a short-lived thread that, after `delay_ms`,
    /// claims the Sender (if still un-fired) and emits a structured error
    /// the AI can act on. If the callback already fired, `take()` returns
    /// None and the thread is a no-op.
    macro_rules! mcp_eval_err_watchdog {
        ($arc:expr, $delay_ms:expr, $err_msg:expr) => {{
            let arc_wd = $arc.clone();
            let msg: String = $err_msg;
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis($delay_ms));
                if let Some(tx) = arc_wd.lock().unwrap().take() {
                    let _ = tx.send(Err(msg));
                }
            });
        }};
    }

    /// Auto-restore for MCP handlers: if `target` is a known tab in TabManager but
    /// has been hibernated (WebView dropped, URL parked in `pending_tabs`), recreate
    /// the WebView in-place. Returns true if the tab is known, false if the id is bogus.
    /// MCP handlers should fall through to "Tab not found" only when this returns false.
    macro_rules! mcp_ensure_tab {
        ($target:expr) => {{
            let t = $target;
            let known = tabs.lock().unwrap().tabs().iter().any(|tab| tab.id == t);
            if known {
                ensure_webview!(t);
            }
            known
        }};
    }

    /// Switch visibility from the current active tab to `target`, handling pending_swap.
    /// Also handles lazy loading: creates WebView on first access if tab was pending.
    macro_rules! switch_visible_tab {
        ($target:expr) => {{
            let target = $target;
            // Capture a frozen snapshot of the outgoing tab (async — fires callback later).
            // Only for live WebViews that aren't about:blank / newtab / error pages.
            if active_wv_id != target {
                if let Some(wv) = tab_webviews.get(&active_wv_id) {
                    let wv_ptr = objc2::rc::Retained::as_ptr(&wv.webview()) as usize;
                    capture_tab_snapshot(wv_ptr, active_wv_id, proxy.clone());
                }
            }
            // Lazy load: create WebView if this tab was pending (hibernated or session-restored).
            ensure_webview!(target);
            tabs.lock().unwrap().switch(target);
            if let Some((old_id, new_id)) = pending_swap.take() {
                if let Some(wv) = tab_webviews.get(&old_id) {
                    let _ = wv.set_visible(false);
                }
                if new_id != target {
                    if let Some(wv) = tab_webviews.get(&new_id) {
                        let _ = wv.set_visible(false);
                    }
                }
            } else if let Some(wv) = tab_webviews.get(&active_wv_id) {
                let _ = wv.set_visible(false);
            }
            if let Some(wv) = tab_webviews.get(&target) {
                // Re-apply bounds on every show: a tab created/sized for a
                // previous window size keeps stale bounds, which would leave a
                // gap above — or let the footer paint over — the page. Pin it
                // to the content area [address bar .. footer] so the quick-slots
                // bar never covers the viewport.
                let sz = browser_win.inner_size();
                let _ = wv.set_bounds(wry::Rect {
                    position: tao::dpi::PhysicalPosition::new(0u32, address_bar_h).into(),
                    size: tao::dpi::PhysicalSize::new(
                        sz.width,
                        sz.height.saturating_sub(address_bar_h + footer_h),
                    )
                    .into(),
                });
                let _ = wv.set_visible(true);
            }
            // Hide progress bar when switching away from a loading tab
            if progress_visible {
                let _ = progress_wv.set_visible(false);
                progress_visible = false;
                progress_hide_at = None;
            }
            active_wv_id = target;
            // Update address bar with the new tab's URL
            let url = tabs
                .lock()
                .unwrap()
                .tabs()
                .iter()
                .find(|t| t.id == target)
                .map(|t| t.url.clone())
                .unwrap_or_default();
            update_address_bar_url!(url);
        }};
    }

    /// Re-assert keyboard first-responder on the visible page WebView.
    ///
    /// `browser_win.set_focus()` only restores *window-level* key status. The
    /// command palette (`overlay_win`) is a separate key window the user drives
    /// by keyboard; once it closes, the page WebView is visible but is NOT the
    /// first responder, so keystrokes land nowhere until the user clicks the
    /// page. wry's `focus()` calls `makeFirstResponder:` on the WebView's
    /// NSView, fixing keyboard-only navigation (palette / address bar). Skipped
    /// when another surface legitimately owns input (sidebar, overlay,
    /// settings/shortcuts panels) or the app isn't frontmost.
    macro_rules! focus_active_webview {
        () => {{
            if is_app_active()
                && !overlay_visible
                && !settings_visible
                && !shortcuts_visible
                && !sidebar_owns_key.load(Ordering::Relaxed)
            {
                browser_win.set_focus();
                if let Some(wv) = tab_webviews.get(&active_wv_id) {
                    let _ = wv.focus();
                }
            }
        }};
    }

    /// Refresh the overlay item list (if visible).
    macro_rules! refresh_overlay {
        () => {
            if overlay_visible {
                let json = {
                    let mut tm = tabs.lock().unwrap();
                    tm.ensure_contiguous();
                    let hib: std::collections::HashSet<usize> =
                        pending_tabs.keys().copied().collect();
                    webview_utils::build_items_json(tm.tabs(), tm.history(), &favicon_cache, &hib)
                };
                let _ = overlay_wv.evaluate_script(&format!(
                    "window.__refreshItems && window.__refreshItems({json})"
                ));
            }
        };
    }

    /// Update address bar with the given URL, title, and cached page stats.
    macro_rules! update_address_bar_url {
        ($url:expr) => {{
            let u = &$url;
            let secure = u.starts_with("https://");
            let escaped_url = webview_utils::escape_js_template(u);
            let suggest_json = {
                let mut tm = tabs.lock().unwrap();
                tm.ensure_contiguous();
                let hib: std::collections::HashSet<usize> = pending_tabs.keys().copied().collect();
                webview_utils::build_items_json(tm.tabs(), tm.history(), &favicon_cache, &hib)
            };
            let tm = tabs.lock().unwrap();
            let tab = tm.tabs().iter().find(|t| t.url == *u || t.id == active_wv_id);
            let raw_title = tab.map(|t| t.title.clone()).unwrap_or_default();
            let (pb, pt) = tab.map(|t| (t.page_bytes, t.page_time_ms)).unwrap_or((0, 0));
            drop(tm);
            let escaped_title = webview_utils::escape_js_template(&raw_title);
            let _ = address_bar_wv.evaluate_script(&format!(
                "window.__update && window.__update(`{escaped_url}`, {secure}, `{escaped_title}`, {pb}, {pt})"
            ));
            // Push the full history+tabs snapshot now too so the URL-edit
            // autocomplete is always primed — no IPC round-trip needed when the
            // user clicks the pencil / hits ⌘E.
            let _ = address_bar_wv.evaluate_script(&format!(
                "window.__setSuggestions && window.__setSuggestions({suggest_json})"
            ));
            // Push cached favicon for this URL's domain
            if let Some(fav) = webview_utils::cached_favicon(u, &favicon_cache) {
                let escaped_fav = webview_utils::escape_js_template(fav);
                let _ = address_bar_wv.evaluate_script(&format!(
                    "window.__setFavicon && window.__setFavicon(`{escaped_fav}`)"
                ));
            } else {
                let _ = address_bar_wv.evaluate_script(
                    "window.__setFavicon && window.__setFavicon(``)"
                );
            }
        }};
    }

    /// Sync quick-slot UI: footer bar + any about:blank newtab pages.
    macro_rules! sync_quickslots_ui {
        () => {{
            let json = quickslots::to_json(&quick_slots);
            let _ = footer_wv.evaluate_script(&format!(
                "window.__updateSlots && window.__updateSlots({json})"
            ));
            for (&tid, wv) in &tab_webviews {
                let tab_url = tabs
                    .lock()
                    .unwrap()
                    .tabs()
                    .iter()
                    .find(|t| t.id == tid)
                    .map(|t| t.url.clone());
                if tab_url.as_deref() == Some("about:blank") {
                    let html = newtab_html::html(&quickslots::to_json(&quick_slots));
                    let _ = wv.load_html(&html);
                }
            }
        }};
    }

    // ── Event loop ────────────────────────────────────────────────────────
    event_loop.run(move |event, _, control_flow| {
        // Never spin (Poll) — use a bounded wake time so ACP/progress events are
        // still responsive (≤16 ms latency) without burning CPU during WebRTC calls.

        // Drain URLs captured before the event loop callback was registered (cold launch).
        // Checked every tick — macOS may deliver the URL after the run loop starts.
        for url in cold_open::take() {
            tracing::debug!(url = %url, "cold_open: draining URL");
            let _ = proxy.send_event(AppEvent::NavigateTo(url));
        }
        let mut next_wake = if let Some(hide_at) = progress_hide_at {
            sys_stats_next_at.min(hide_at)
        } else {
            sys_stats_next_at
        };
        if let Some(save_at) = history_save_at {
            next_wake = next_wake.min(save_at);
        }
        if let Some(earliest) = mcp_nav_pending.values().map(|(t, _)| *t).min() {
            next_wake = next_wake.min(earliest + std::time::Duration::from_secs(15));
        }
        *control_flow = ControlFlow::WaitUntil(
            next_wake.min(std::time::Instant::now() + std::time::Duration::from_millis(16)),
        );

        // Hide progress bar after CSS fade completes
        if let Some(hide_at) = progress_hide_at {
            if std::time::Instant::now() >= hide_at {
                let _ = progress_wv.set_visible(false);
                progress_visible = false;
                progress_hide_at = None;
            }
        }
        // ── Sample active tab's WebContent process stats ─────────────────
        // Store in atomics for custom protocol polling (avoids evaluate_script leak).
        // JS polls via fetch('octoweb-sys://stats') — no WKWebView JS context leak.
        let now = std::time::Instant::now();
        if now >= sys_stats_next_at {
            let media_active = media_playing_tabs.contains(&active_wv_id);
            let interval = if media_active { 5 } else { 2 };
            sys_stats_next_at = now + std::time::Duration::from_secs(interval);
            if let Some(wv) = tab_webviews.get(&active_wv_id) {
                let wv_ptr = objc2::rc::Retained::as_ptr(&wv.webview()) as usize;
                if let Some(pid) = tab_stats::webview_pid(wv_ptr) {
                    if let Some((rss, cpu_ns)) = tab_stats::sample_pid(pid) {
                        let cpu_pct = if let Some(ref prev) = sys_stats_last {
                            tab_stats::compute_cpu_pct(prev, cpu_ns, now.duration_since(prev.ts))
                        } else {
                            0.0
                        };
                        sys_stats_last = Some(tab_stats::TabStatsSample { cpu_ns, ts: now });
                        let mem_mb = rss / (1024 * 1024);
                        let cpu_i = cpu_pct.round() as u32;
                        // Store in atomics for custom protocol polling
                        sys_stats_cpu.store(cpu_i, Ordering::Relaxed);
                        sys_stats_mem.store(mem_mb, Ordering::Relaxed);
                    }
                }
            }
        }

        // ── Safety net: resolve MCP navigations stuck longer than 15s ────
        {
            let stale_cutoff = now - std::time::Duration::from_secs(15);
            let stale_ids: Vec<usize> = mcp_nav_pending.iter()
                .filter(|(_, (t, _))| *t < stale_cutoff)
                .map(|(id, _)| *id)
                .collect();
            for id in stale_ids {
                if let Some((_, tx)) = mcp_nav_pending.remove(&id) {
                    tracing::warn!(tab_id = id, "MCP navigate: resolving stale pending (>15s)");
                    let _ = tx.send(Ok(id));
                }
            }
        }

        // ── Crash diagnostics: periodic health snapshot ──────────────────
        if now >= health_log_next_at {
            health_log_next_at = now + std::time::Duration::from_secs(30);
            let active_rss_mb = sys_stats_mem.load(Ordering::Relaxed);
            let main_rss_mb = crash_report::main_process_rss() / (1024 * 1024);
            let pressure = cached_pressure;
            let pressure_str = match pressure {
                hibernation::MemoryPressure::Normal => "normal",
                hibernation::MemoryPressure::Warning => "warning",
                hibernation::MemoryPressure::Critical => "critical",
            };
            let (tab_count, active_url) = {
                let tm = tabs.lock().unwrap();
                let count = tm.tabs().len();
                let url = tm.active_tab().map(|t| t.url.clone()).unwrap_or_default();
                (count, url)
            };
            crash_report::log_health(&crash_report::HealthSnapshot {
                uptime_secs: app_start_instant.elapsed().as_secs(),
                tab_count,
                active_rss_mb,
                main_rss_mb,
                pressure: pressure_str,
                active_url: &active_url,
            });
        }

        // ── Debounced history save ──────────────────────────────────────
        if let Some(save_at) = history_save_at {
            if now >= save_at {
                history_save_at = None;
                let snapshot: Vec<browser::HistoryEntry> = {
                    let mut tm = tabs.lock().unwrap();
                    tm.ensure_contiguous();
                    tm.history().to_vec()
                };
                std::thread::spawn(move || config::save_history(&snapshot));
            }
        }

        // ── Tab hibernation: offload idle tabs under memory pressure ─────
        // Re-samples system memory pressure on a 2s timer (the Mach
        // `host_statistics64` syscall is too expensive to call every tick).
        {
            if now >= mem_pressure_next_at {
                mem_pressure_next_at = now + std::time::Duration::from_secs(2);
                cached_pressure = hibernation::system_memory_pressure();
            }
            let pressure = cached_pressure;
            if pressure != hibernation::MemoryPressure::Normal {
                let victims = {
                    let tm = tabs.lock().unwrap();
                    hibernation::pick_victims(
                        tm.tabs(),
                        &tab_webviews,
                        &pending_tabs,
                        pending_swap,
                        &mru,
                        active_wv_id,
                        &media_playing_tabs,
                        pressure,
                    )
                };
                for victim_id in victims {
                    // Save URL so switch_visible_tab! can restore later
                    let url = tabs.lock().unwrap()
                        .tabs()
                        .iter()
                        .find(|t| t.id == victim_id)
                        .map(|t| t.url.clone())
                        .unwrap_or_default();
                    if url.is_empty() {
                        continue;
                    }
                    // Unregister nav error callbacks before destroying WebView
                    if let Some(wv) = tab_webviews.get(&victim_id) {
                        let wv_ptr = objc2::rc::Retained::as_ptr(&wv.webview()) as usize;
                        nav_error_patch::unregister(wv_ptr);
                        nav_error_patch::unregister_termination(wv_ptr);
                        dialog_patch::unregister(wv_ptr);
                    }
                    tab_webviews.remove(&victim_id); // WebView dropped → XPC process freed
                    media_playing_tabs.remove(&victim_id);  // Clean up stale media state
                    pending_tabs.insert(victim_id, url.clone());
                    tracing::debug!(
                        tab_id = victim_id,
                        url = %url,
                        pressure = ?pressure,
                        "hibernated tab under memory pressure",
                    );
                }
            }
        }

        // ── First-run welcome toast: one-shot ─────────────────────────────
        if let Some(at) = welcome_toast_at {
            if now >= at {
                welcome_toast_at = None;
                let _ = notification_wv.set_visible(true);
                notification_visible = true;
                // text, icon, title, autoDismissMs — reuse the existing toast API
                let _ = notification_wv.evaluate_script(
                    r#"window.__show && window.__show(
                        'Press ⌘⇧P to search, ⌘/ for shortcuts.',
                        '🐙',
                        'Welcome to Octoweb',
                        6000
                    )"#,
                );
                cfg.first_run_completed = true;
                cfg.save();
            }
        }

        // ── Proactive hibernation: freeze idle / cold tabs every 60 s ────────
        // Runs regardless of memory pressure — reclaims resources before the OS
        // has to ask us to. Same victim-destruction logic as the reactive path.
        if now >= proactive_hiber_next_at {
            proactive_hiber_next_at = now + std::time::Duration::from_secs(60);
            let victims = {
                let tm = tabs.lock().unwrap();
                hibernation::pick_proactive_victims(
                    tm.tabs(),
                    &tab_webviews,
                    &pending_tabs,
                    pending_swap,
                    active_wv_id,
                    &media_playing_tabs,
                    &proactive_config,
                )
            };
            for victim_id in victims {
                let url = tabs.lock().unwrap()
                    .tabs()
                    .iter()
                    .find(|t| t.id == victim_id)
                    .map(|t| t.url.clone())
                    .unwrap_or_default();
                if url.is_empty() {
                    continue;
                }
                if let Some(wv) = tab_webviews.get(&victim_id) {
                    let wv_ptr = objc2::rc::Retained::as_ptr(&wv.webview()) as usize;
                    nav_error_patch::unregister(wv_ptr);
                    nav_error_patch::unregister_termination(wv_ptr);
                }
                tab_webviews.remove(&victim_id);
                media_playing_tabs.remove(&victim_id);
                pending_tabs.insert(victim_id, url.clone());
                tracing::debug!(tab_id = victim_id, url = %url, "proactively hibernated idle tab");
            }
        }

        // Set dock icon and app menu once — must happen after tao has initialized NSApplication.
        if !icon_set {
            macos::set_app_icon();
            macos::setup_edit_menu();
            macos::disable_automatic_termination();
            icon_set = true;
        }

        // Drain ACP events from every session and forward to the sidebar UI.
        // Events are tagged with `session_id` so JS can route them to the correct
        // per-session DOM message log (only the active session is visible in #messages).
        //
        // We collect (session_id, events) into an owned Vec first so the immutable
        // borrow of `sessions` is released before we mutate it (e.g. on Error we
        // drop a session's handle).
        let drained: Vec<(u64, Vec<acp::AgentEvent>)> = sessions
            .iter_mut()
            .map(|s| (s.id, s.handle.as_mut().map(|h| h.poll()).unwrap_or_default()))
            .collect();
        for (sid, events) in drained {
            for ev in events {
                match ev {
                    acp::AgentEvent::Connected(acp_sid) => {
                        let mut should_persist = false;
                        let mut resume_failed = false;
                        if let Some(s) = sessions.iter_mut().find(|s| s.id == sid) {
                            s.retry_count = 0;
                            if let Some(old) = s.acp_session_id.as_deref() {
                                if old != acp_sid.as_str() {
                                    // Resume failed: the agent minted a fresh session
                                    // instead of restoring the saved one. Keep the saved
                                    // id — the old session still exists on disk and the
                                    // next (re)connect retries it. Adopting the fresh id
                                    // would permanently orphan the real context.
                                    tracing::warn!(session_id = sid, saved = %old, fresh = %acp_sid, "ACP resume failed — agent started a fresh session; keeping saved id");
                                    resume_failed = true;
                                }
                            } else {
                                // No saved id (new/cleared session) — adopt so a future
                                // cold start can pass `--resume <id>` and recover the
                                // agent's own conversation context.
                                s.acp_session_id = Some(acp_sid.clone());
                                should_persist = true;
                            }
                        }
                        let _ = sidebar_wv.evaluate_script(&format!(
                            "window.__setSessionStatus && window.__setSessionStatus({sid},'ready')"
                        ));
                        if resume_failed {
                            let _ = sidebar_wv.evaluate_script(&format!(
                                "window.__appendError && window.__appendError({sid},`previous agent context wasn't restored (fresh session) — restart octoweb to retry it`)"
                            ));
                        }
                        if should_persist {
                            persist_acp_history(&sessions, active_session_id, cfg.max_acp_session_messages);
                        }
                    }
                    acp::AgentEvent::ProcessPid(pid) => {
                        if let Some(s) = sessions.iter_mut().find(|s| s.id == sid) {
                            s.octomind_pid = Some(pid);
                        }
                    }
                    acp::AgentEvent::Image { data, mime_type } => {
                        let _ = sidebar_wv.evaluate_script(&format!(
                            "window.__appendImage && window.__appendImage({sid},`{mime_type}`,`{data}`)"
                        ));
                    }
                    acp::AgentEvent::Chunk(chunk) => {
                        // Accumulate the in-flight agent response so we can
                        // commit one finalized "agent" message to the persisted
                        // log on Done/Cancelled — no save during streaming.
                        if let Some(s) = sessions.iter_mut().find(|s| s.id == sid) {
                            s.agent_buf.push_str(&chunk);
                        }
                        let escaped = webview_utils::escape_js_template(&chunk);
                        let _ = sidebar_wv.evaluate_script(&format!(
                            "window.__appendChunk && window.__appendChunk({sid},`{escaped}`)"
                        ));
                        // Show badge + notification toast when sidebar is hidden.
                        // The toast surfaces ANY session's chunk — only one toast
                        // visible at a time is fine (it auto-dismisses).
                        if !sidebar_visible {
                            let _ = address_bar_wv.evaluate_script(
                                "window.__setBadge && window.__setBadge(true)"
                            );
                            if !notification_visible {
                                let _ = notification_wv.set_visible(true);
                                notification_visible = true;
                            }
                            let _ = notification_wv.evaluate_script(&format!(
                                "window.__show && window.__show(`{escaped}`)"
                            ));
                        }
                    }
                    acp::AgentEvent::ToolStart { id, title, kind, raw_input, locations } => {
                        // Record for persistence so the steps group survives a
                        // restart. `render_ui` is skipped: it renders as an A2UI
                        // bubble (persisted separately) and the sidebar suppresses
                        // its tool row — a record here would replay as a phantom step.
                        if !title.starts_with("render_ui") {
                            if let Some(s) = sessions.iter_mut().find(|s| s.id == sid) {
                                s.tool_starts.insert(id.clone(), (s.tool_buf.len(), std::time::Instant::now()));
                                s.tool_buf.push(config::AcpToolRecord {
                                    kind: kind.clone(),
                                    title: title.clone(),
                                    status: "running".to_string(),
                                    duration_ms: 0,
                                    raw_input: raw_input.as_ref().map(cap_tool_json),
                                    locations: locations.clone(),
                                    raw_output: None,
                                });
                            }
                        }
                        let eid = webview_utils::escape_js_template(&id);
                        let etitle = webview_utils::escape_js_template(&title);
                        let ekind = webview_utils::escape_js_template(&kind);
                        let raw_input_json = raw_input
                            .as_ref()
                            .and_then(|v| serde_json::to_string(v).ok())
                            .unwrap_or_else(|| "null".to_string());
                        let locations_json = serde_json::to_string(&locations).unwrap_or_else(|_| "[]".to_string());
                        let _ = sidebar_wv.evaluate_script(&format!(
                            "window.__toolStart && window.__toolStart({sid},`{eid}`,`{etitle}`,`{ekind}`,{raw_input_json},{locations_json})"
                        ));
                    }
                    acp::AgentEvent::ToolUpdate { id, title, status, raw_output } => {
                        if let Some(s) = sessions.iter_mut().find(|s| s.id == sid) {
                            if let Some(&(idx, started)) = s.tool_starts.get(&id) {
                                if let Some(rec) = s.tool_buf.get_mut(idx) {
                                    if let Some(t) = title.as_deref() {
                                        if !t.is_empty() {
                                            rec.title = t.to_string();
                                        }
                                    }
                                    if let Some(o) = raw_output.as_ref() {
                                        rec.raw_output = Some(cap_tool_json(o));
                                    }
                                    if (status == "completed" || status == "failed") && rec.status == "running" {
                                        rec.status = status.clone();
                                        rec.duration_ms = started.elapsed().as_millis() as u64;
                                    }
                                }
                            }
                        }
                        let eid = webview_utils::escape_js_template(&id);
                        let etitle = webview_utils::escape_js_template(title.as_deref().unwrap_or(""));
                        let estatus = webview_utils::escape_js_template(&status);
                        let raw_output_json = raw_output
                            .as_ref()
                            .and_then(|v| serde_json::to_string(v).ok())
                            .unwrap_or_else(|| "null".to_string());
                        let _ = sidebar_wv.evaluate_script(&format!(
                            "window.__toolUpdate && window.__toolUpdate({sid},`{eid}`,`{etitle}`,`{estatus}`,{raw_output_json})"
                        ));
                    }
                    acp::AgentEvent::AvailableCommands(commands) => {
                        let json: Vec<serde_json::Value> = commands.iter().map(|c| {
                            serde_json::json!({"name": c.name, "description": c.description, "hint": c.hint})
                        }).collect();
                        let json_str = serde_json::to_string(&json).unwrap_or_else(|_| "[]".into());
                        // Cache so we can re-push to JS on `sidebar_ready` —
                        // the agent only emits commands once on connect, but
                        // the sidebar may not be open yet at that moment.
                        if let Some(s) = sessions.iter_mut().find(|s| s.id == sid) {
                            s.available_commands_json = Some(json_str.clone());
                        }
                        let escaped = webview_utils::escape_js_template(&json_str);
                        let _ = sidebar_wv.evaluate_script(&format!(
                            "window.__setAvailableCommands && window.__setAvailableCommands({sid},`{escaped}`)"
                        ));
                    }
                    acp::AgentEvent::Done => {
                        let mut should_persist = false;
                        if let Some(s) = sessions.iter_mut().find(|s| s.id == sid) {
                            s.last_cancel_at = None;
                            // Flush the streamed response + tool runs into the
                            // persisted log as a single "agent" message so the
                            // steps group survives restarts.
                            if flush_agent_turn(s, cfg.max_acp_session_messages) {
                                should_persist = true;
                            }
                        }
                        let _ = sidebar_wv.evaluate_script(&format!(
                            "window.__setThinking && window.__setThinking({sid},false)"
                        ));
                        if !sidebar_visible {
                            let _ = address_bar_wv.evaluate_script(
                                "window.__setBadge && window.__setBadge(true)"
                            );
                        }
                        if should_persist {
                            persist_acp_history(&sessions, active_session_id, cfg.max_acp_session_messages);
                        }
                    }
                    acp::AgentEvent::Cancelled => {
                        let mut should_persist = false;
                        if let Some(s) = sessions.iter_mut().find(|s| s.id == sid) {
                            s.last_cancel_at = None;
                            // Save the partial turn (cancelled mid-stream) so
                            // the user sees the same thing they had on screen.
                            if flush_agent_turn(s, cfg.max_acp_session_messages) {
                                should_persist = true;
                            }
                        }
                        let _ = sidebar_wv.evaluate_script(&format!(
                            "window.__setThinking && window.__setThinking({sid},false)"
                        ));
                        if should_persist {
                            persist_acp_history(&sessions, active_session_id, cfg.max_acp_session_messages);
                        }
                    }
                    acp::AgentEvent::Error(err) => {
                        tracing::warn!(session_id = sid, error = %err, "ACP connection error");
                        // Mutate session state in an inner scope so the &mut borrow
                        // is released before we call evaluate_script + persist (both
                        // read &sessions). Decision drives the post-borrow JS calls.
                        enum Decision { Reconnect(u64, u64), MaxExceeded }
                        let decision: Option<Decision>;
                        let mut should_persist = false;
                        let mut user_facing_err: Option<String> = None;
                        {
                            let Some(s) = sessions.iter_mut().find(|s| s.id == sid) else { continue };
                            // Drop dead handle immediately so prompts aren't silently lost.
                            s.handle = None;
                            s.octomind_pid = None; // pid is now stale — next spawn fills it
                            s.last_cancel_at = None;
                            // Save the partial in-flight turn so the persisted log
                            // mirrors what the user saw before the disconnect.
                            if flush_agent_turn(s, cfg.max_acp_session_messages) {
                                should_persist = true;
                            }
                            if s.retry_count < ACP_MAX_RETRIES {
                                // First disconnect of this cycle: surface to chat so the user
                                // knows their in-flight prompt is gone (it isn't replayed) and
                                // so __appendError → __setThinking(false) flushes pending tool
                                // rows into the message history. Subsequent retries don't spam.
                                if s.retry_count == 0 {
                                    let m = format!("agent disconnected: {err} — reconnecting");
                                    push_acp_msg(s, "error", m.clone(), cfg.max_acp_session_messages);
                                    should_persist = true;
                                    user_facing_err = Some(m);
                                }
                                s.retry_count += 1;
                                let delay = std::cmp::min(
                                    ACP_BASE_DELAY_SECS * 2u64.pow(s.retry_count - 1),
                                    ACP_MAX_DELAY_SECS,
                                );
                                tracing::info!(session_id = sid, retry = s.retry_count, delay_s = delay, "scheduling ACP reconnection");
                                // Bump generation so any previously scheduled timer for this session is ignored.
                                s.reconnect_gen += 1;
                                decision = Some(Decision::Reconnect(s.reconnect_gen, delay));
                            } else {
                                tracing::error!(session_id = sid, "ACP max reconnection retries exceeded");
                                push_acp_msg(s, "error", err.clone(), cfg.max_acp_session_messages);
                                should_persist = true;
                                user_facing_err = Some(err.clone());
                                decision = Some(Decision::MaxExceeded);
                            }
                        }
                        match decision {
                            Some(Decision::Reconnect(gen, delay)) => {
                                if let Some(m) = &user_facing_err {
                                    let escaped = webview_utils::escape_js_template(m);
                                    let _ = sidebar_wv.evaluate_script(&format!(
                                        "window.__appendError && window.__appendError({sid},`{escaped}`)"
                                    ));
                                }
                                let _ = sidebar_wv.evaluate_script(&format!(
                                    "window.__setSessionStatus && window.__setSessionStatus({sid},'connecting')"
                                ));
                                let proxy_clone = acp_proxy.clone();
                                std::thread::spawn(move || {
                                    std::thread::sleep(std::time::Duration::from_secs(delay));
                                    let _ = proxy_clone.send_event(AppEvent::AcpReconnect(sid, gen));
                                });
                            }
                            Some(Decision::MaxExceeded) => {
                                let _ = sidebar_wv.evaluate_script(&format!(
                                    "window.__setSessionStatus && window.__setSessionStatus({sid},'error')"
                                ));
                                if let Some(m) = &user_facing_err {
                                    let escaped = webview_utils::escape_js_template(m);
                                    let _ = sidebar_wv.evaluate_script(&format!(
                                        "window.__appendError && window.__appendError({sid},`{escaped}`)"
                                    ));
                                    if !sidebar_visible {
                                        let _ = address_bar_wv.evaluate_script(
                                            "window.__setBadge && window.__setBadge(true)"
                                        );
                                        if !notification_visible {
                                            let _ = notification_wv.set_visible(true);
                                            notification_visible = true;
                                        }
                                        let _ = notification_wv.evaluate_script(&format!(
                                            "window.__show && window.__show(`{escaped}`)"
                                        ));
                                    }
                                }
                            }
                            None => {}
                        }
                        if should_persist {
                            persist_acp_history(&sessions, active_session_id, cfg.max_acp_session_messages);
                        }
                    }
                    acp::AgentEvent::Account { signed_in, account, over_quota, summary } => {
                        let json = serde_json::json!({
                            "signed_in": signed_in,
                            "account": account,
                            "over_quota": over_quota,
                            "summary": summary,
                        });
                        let json_str = serde_json::to_string(&json).unwrap_or_else(|_| "{}".into());
                        if let Some(s) = sessions.iter_mut().find(|s| s.id == sid) {
                            s.account_json = Some(json_str.clone());
                        }
                        let escaped = webview_utils::escape_js_template(&json_str);
                        let _ = sidebar_wv.evaluate_script(&format!(
                            "window.__setAccount && window.__setAccount({sid},`{escaped}`)"
                        ));
                        // A login flow was in flight and just succeeded: stop the
                        // poller and respawn the session so the fresh OCTOHUB_API_KEY
                        // in `.env` is loaded (it's read at process startup). Resume
                        // by id to keep the conversation context.
                        if signed_in {
                            if let Some(stop) = login_pollers.remove(&sid) {
                                stop.store(true, std::sync::atomic::Ordering::Relaxed);
                                if let Some(s) = sessions.iter_mut().find(|s| s.id == sid) {
                                    s.retry_count = 0;
                                    s.reconnect_gen += 1;
                                    let cmd = match &s.acp_session_id {
                                        Some(id) => format!("octomind acp {} --resume {}", s.tag, id),
                                        None => format!("octomind acp {}", s.tag),
                                    };
                                    s.handle = acp::AcpHandle::connect(&cmd, make_wake(acp_proxy.clone())).ok();
                                    let _ = sidebar_wv.evaluate_script(&format!(
                                        "window.__setSessionStatus && window.__setSessionStatus({sid},'connecting')"
                                    ));
                                }
                            }
                        }
                    }
                    acp::AgentEvent::LoginStarted { url, code, already_signed_in } => {
                        if already_signed_in {
                            // Nothing to do — the chip already reflects it. Nudge a
                            // refresh so any stale "signed out" state corrects.
                            let _ = acp_proxy.send_event(AppEvent::AcpRefreshAccount(sid));
                        } else if !url.is_empty() {
                            // Open the verification URL in a browser tab (octoweb IS
                            // a browser — the whole point) and show the code.
                            let _ = acp_proxy.send_event(AppEvent::OpenInNewTab(url.clone()));
                            let ecode = webview_utils::escape_js_template(&code);
                            let _ = sidebar_wv.evaluate_script(&format!(
                                "window.__loginPending && window.__loginPending({sid},`{ecode}`)"
                            ));
                            // Poll `/usage` until the browser approval lands. Bounded
                            // to the server's ~15-min device-code TTL; the stop flag
                            // ends it early once signed-in (handled in Account above).
                            let stop = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
                            login_pollers.insert(sid, std::sync::Arc::clone(&stop));
                            let proxy_clone = acp_proxy.clone();
                            std::thread::spawn(move || {
                                for _ in 0..100 {
                                    std::thread::sleep(std::time::Duration::from_secs(3));
                                    if stop.load(std::sync::atomic::Ordering::Relaxed) {
                                        return;
                                    }
                                    if proxy_clone.send_event(AppEvent::AcpRefreshAccount(sid)).is_err() {
                                        return;
                                    }
                                }
                            });
                        }
                    }
                }
            }
        }

        // Drain inline-edit ACP events.
        let inline_events = inline_edit_acp.as_mut().map(|h| h.poll()).unwrap_or_default();
        for ev in inline_events {
            match ev {
                acp::AgentEvent::Chunk(chunk) => {
                    inline_edit_response.push_str(&chunk);
                }
                acp::AgentEvent::Done => {
                    // Parse <text>...</text> from response; fallback to full response
                    let result = if let (Some(start), Some(end)) = (
                        inline_edit_response.find("<text>"),
                        inline_edit_response.rfind("</text>"),
                    ) {
                        inline_edit_response[start + 6..end].trim().to_string()
                    } else {
                        inline_edit_response.clone()
                    };

                    // Replace text in the original tab + reset cursor
                    if let Some(wv) = tab_webviews.get(&inline_edit_tab_id) {
                        let escaped = webview_utils::escape_js_template(&result);
                        let _ = wv.evaluate_script(&format!(
                            "window.__inlineEditReplace && window.__inlineEditReplace(`{escaped}`)"
                        ));
                        let _ = wv.evaluate_script(
                            "document.documentElement.style.cursor=''"
                        );
                    }

                    // Hide modal, clean up
                    let _ = inline_edit_wv.set_visible(false);
                    let _ = inline_edit_wv.evaluate_script("window.__clear && window.__clear()");
                    inline_edit_visible = false;
                    inline_edit_pending = false;
                    inline_edit_hotkey_visible.store(false, Ordering::Relaxed);
                    inline_edit_acp = None;
                    inline_edit_response.clear();
                }
                acp::AgentEvent::Error(err) => {
                    tracing::warn!(error = %err, "inline edit ACP error");
                    // Reset cursor on target tab
                    if let Some(wv) = tab_webviews.get(&inline_edit_tab_id) {
                        let _ = wv.evaluate_script(
                            "document.documentElement.style.cursor=''"
                        );
                    }
                    let escaped = webview_utils::escape_js_template(&err);
                    let _ = inline_edit_wv.evaluate_script(&format!(
                        "window.__setError && window.__setError(`{escaped}`)"
                    ));
                    inline_edit_acp = None;
                    inline_edit_response.clear();
                }
                _ => {} // Ignore Connected, ToolStart, etc.
            }
        }

        // ── Proactive learning: timer check + event drain ─────────────────────
        if cfg.proactive_learning {
            if let Some(next) = learning_next_at {
                if std::time::Instant::now() >= next && learning_handle.is_none() {
                    // Schedule next run immediately so we don't re-trigger
                    learning_next_at = Some(std::time::Instant::now() + std::time::Duration::from_secs(cfg.learning_interval_min * 60));
                    // Extract active tab's readable text, then fire LearningReady
                    if let Some(wv) = tab_webviews.get(&active_wv_id) {
                        let lp = proxy.clone();
                        let _ = wv.evaluate_script_with_callback(
                            &webview_utils::well_formed_js("document.body ? document.body.innerText : ''"),
                            move |val| {
                                let text = serde_json::from_str::<String>(&val).unwrap_or(val);
                                let _ = lp.send_event(AppEvent::LearningReady(text));
                            },
                        );
                    } else {
                        let _ = proxy.send_event(AppEvent::LearningReady(String::new()));
                    }
                }
            }
        }
        // Drain learning agent events (background — no UI)
        if let Some(ref mut h) = learning_handle {
            for ev in h.poll() {
                match ev {
                    acp::AgentEvent::Done => {
                        tracing::info!("proactive learning run completed");
                        learning_handle = None;
                    }
                    acp::AgentEvent::Error(err) => {
                        tracing::warn!(error = %err, "proactive learning error");
                        learning_handle = None;
                    }
                    _ => {} // Ignore Chunk, ToolStart, etc.
                }
            }
        }

        // Drain MCP commands and execute on main thread (WebView is not thread-safe).
        if let Some(ref mut handle) = mcp_handle {
            while let Some(cmd) = handle.poll() {
                tracing::debug!(cmd = ?std::mem::discriminant(&cmd), "MCP command received");

                // Touch the target tab so hibernation knows it's in use.
                // Prevents background tabs used by AI from being offloaded.
                let touch_id = match &cmd {
                    McpCommand::Navigate { tab_id, .. } => *tab_id,
                    McpCommand::GetPageInfo { tab_id, .. }
                    | McpCommand::ExecuteJs { tab_id, .. }
                    | McpCommand::GetPageContent { tab_id, .. }
                    | McpCommand::Snapshot { tab_id, .. }
                    | McpCommand::Screenshot { tab_id, .. }
                    | McpCommand::Reload { tab_id, .. }
                    | McpCommand::GoBack { tab_id, .. }
                    | McpCommand::GoForward { tab_id, .. }
                    | McpCommand::Scroll { tab_id, .. }
                    | McpCommand::Wait { tab_id, .. } => *tab_id,
                    McpCommand::Click { tab_id, .. }
                    | McpCommand::Hover { tab_id, .. }
                    | McpCommand::Type { tab_id, .. }
                    | McpCommand::PressKey { tab_id, .. }
                    | McpCommand::SelectOption { tab_id, .. } => *tab_id,
                    _ => None,
                };
                if let Some(id) = touch_id.or(Some(active_wv_id)) {
                    tabs.lock().unwrap().touch(id);
                }

                match cmd {
                    McpCommand::Navigate { url, tab_id, response } => {
                        tracing::debug!(url = %url, ?tab_id, "MCP navigate");

                        if url.trim().is_empty() {
                            let _ = response.send(Err("url is empty".into()));
                            continue;
                        }

                        // External scheme → hand off to macOS, not loadable in WebView.
                        // Resolves immediately (no page-load wait) since macOS handles it.
                        if url::is_external_scheme(&url) {
                            let ok = macos::open_external_url(&url);
                            let _ = response.send(if ok {
                                Ok(active_wv_id)
                            } else {
                                Err(format!("No app registered for URL: {url}"))
                            });
                            continue;
                        }

                        let resolved = url::resolve_url(&url, &search_engine);

                        // ── Decide the target tab ─────────────────────────────────
                        //
                        //   tab_id given → navigate that tab in-place; error if it
                        //                  no longer exists (the AI just retries
                        //                  without tab_id to get a fresh tab).
                        //   no tab_id    → open a NEW background tab.
                        //
                        // MCP navigation NEVER moves focus — the AI must call
                        // browser_switch_tab to show a tab to the user.
                        let in_place = match tab_id {
                            Some(id) => {
                                let exists = tabs.lock().unwrap().tabs().iter().any(|t| t.id == id);
                                if !exists {
                                    let _ = response.send(Err(format!(
                                        "Tab {id} not found — omit tab_id to open a new background tab"
                                    )));
                                    continue;
                                }
                                Some(id)
                            }
                            None => None,
                        };

                        // ── Execute ───────────────────────────────────────────────
                        match in_place {
                            Some(target_id) => {
                                // Auto-restore if the tab was hibernated.
                                let _ = mcp_ensure_tab!(target_id);
                                if let Some(wv) = tab_webviews.get(&target_id) {
                                    let _ = wv.load_url(&resolved);
                                    if tabs.lock().unwrap().update_url(target_id, resolved) {
                                        history_save_at.get_or_insert(
                                            std::time::Instant::now() + std::time::Duration::from_secs(60),
                                        );
                                    }
                                    // Replace any prior pending nav for this tab so
                                    // only the latest navigate gets to resolve.
                                    if let Some((_, old)) = mcp_nav_pending.remove(&target_id) {
                                        let _ = old.send(Err("Superseded by new navigation".into()));
                                    }
                                    mcp_nav_pending.insert(target_id, (std::time::Instant::now(), response));
                                } else {
                                    // mcp_ensure_tab! returned false (tab id is in
                                    // TabManager but WebView creation failed) — rare,
                                    // but surface it cleanly.
                                    let _ = response.send(Err(format!(
                                        "Tab {target_id} could not be restored"
                                    )));
                                }
                            }
                            None => {
                                let new_id = tabs.lock().unwrap().open_background(resolved.clone());
                                spawn_tab_webview!(new_id, &resolved);

                                if let Some((_, old)) = mcp_nav_pending.remove(&new_id) {
                                    let _ = old.send(Err("Superseded by new navigation".into()));
                                }
                                mcp_nav_pending.insert(new_id, (std::time::Instant::now(), response));
                            }
                        }
                    }
                    McpCommand::GetTabs { response } => {
                        tracing::debug!("MCP get_tabs");

                        let tm = tabs.lock().unwrap();
                        let active = tm.active_id();
                        let tabs_list: Vec<TabInfo> = tm.tabs().iter().map(|t| {
                            TabInfo::from_tab(t, active == Some(t.id))
                        }).collect();

                        tracing::debug!(count = tabs_list.len(), "MCP get_tabs response");

                        let _ = response.send(Ok(tabs_list));
                    }
                    McpCommand::SwitchTab { tab_id, response } => {
                        tracing::debug!(tab_id, "MCP switch_tab");

                        // Validate before firing — the event handler silently no-ops
                        // on an unknown id, which would otherwise report a bogus success.
                        let exists = tabs.lock().unwrap().tabs().iter().any(|t| t.id == tab_id);
                        if exists {
                            // Update tab state synchronously so an immediate
                            // browser_get_tabs / get_current_tab sees the new
                            // active tab; the event does the visual swap and
                            // re-runs switch() idempotently.
                            tabs.lock().unwrap().switch(tab_id);
                            let _ = proxy.send_event(AppEvent::SwitchTab(tab_id));
                            let _ = response.send(Ok(()));
                        } else {
                            let _ = response.send(Err(format!("Tab {tab_id} not found")));
                        }
                    }
                    McpCommand::CloseTab { tab_id, response } => {
                        tracing::debug!(tab_id, "MCP close_tab");

                        // Validate before firing. Also guards against tab_id=0, which the
                        // internal event treats as "close the active tab" — never an MCP intent.
                        let exists = tabs.lock().unwrap().tabs().iter().any(|t| t.id == tab_id);
                        if exists {
                            // Remove from TabManager synchronously so an immediate
                            // browser_get_tabs no longer lists it; the event tears
                            // down the WebView (its close() re-run is a no-op).
                            tabs.lock().unwrap().close(tab_id);
                            let _ = proxy.send_event(AppEvent::CloseTab(tab_id));
                            let _ = response.send(Ok(()));
                        } else {
                            let _ = response.send(Err(format!("Tab {tab_id} not found")));
                        }
                    }
                    McpCommand::GetPageInfo { tab_id, response, is_retry } => {
                        let tm = tabs.lock().unwrap();
                        let target_id = tab_id.or(tm.active_id());
                        let base_info = target_id.and_then(|id| {
                            tm.tabs().iter().find(|t| t.id == id).map(|t| (id, t.title.clone(), t.url.clone()))
                        });
                        drop(tm);
                        match base_info {
                            Some((id, title, url)) => {
                                // Auto-restore if the tab was hibernated
                                let _ = mcp_ensure_tab!(id);
                                if let Some(wv) = tab_webviews.get(&id) {
                                    let response = std::sync::Arc::new(std::sync::Mutex::new(Some(response)));
                                    let response_cb = response.clone();
                                    let cb_title = title.clone();
                                    let cb_url = url.clone();
                                    match wv.evaluate_script_with_callback(
                                        &webview_utils::well_formed_js("document.querySelector('meta[name=\"description\"]')?.content || ''"),
                                        move |val| {
                                            if let Some(tx) = response_cb.lock().unwrap().take() {
                                                let desc = serde_json::from_str::<String>(&val).unwrap_or(val);
                                                let description = if desc.is_empty() { None } else { Some(desc) };
                                                let _ = tx.send(Ok(PageInfo::new(cb_title.clone(), cb_url.clone(), description)));
                                            }
                                        },
                                    ) {
                                        Ok(()) => {
                                            if !is_retry {
                                                // Idempotent read — auto-retry once on watchdog.
                                                let arc_wd = response.clone();
                                                let tx_for_retry = mcp_cmd_tx.clone();
                                                std::thread::spawn(move || {
                                                    std::thread::sleep(std::time::Duration::from_millis(3000));
                                                    if let Some(tx) = arc_wd.lock().unwrap().take() {
                                                        let _ = tx_for_retry.send(mcp::McpCommand::GetPageInfo {
                                                            tab_id: Some(id),
                                                            response: tx,
                                                            is_retry: true,
                                                        });
                                                    }
                                                });
                                            } else {
                                                // Retry already attempted — degrade to title+url
                                                // without description. The AI still gets something
                                                // useful instead of an error.
                                                let arc_wd = response.clone();
                                                let fallback_title = title.clone();
                                                let fallback_url = url.clone();
                                                std::thread::spawn(move || {
                                                    std::thread::sleep(std::time::Duration::from_millis(5000));
                                                    if let Some(tx) = arc_wd.lock().unwrap().take() {
                                                        let _ = tx.send(Ok(PageInfo::new(fallback_title, fallback_url, None)));
                                                    }
                                                });
                                            }
                                        }
                                        Err(_) => {
                                            // JS failed — return info without description
                                            if let Some(tx) = response.lock().unwrap().take() {
                                                let _ = tx.send(Ok(PageInfo::new(title, url, None)));
                                            }
                                        }
                                    }
                                } else {
                                    let _ = response.send(Ok(PageInfo::new(title, url, None)));
                                }
                            }
                            None => {
                                let _ = response.send(Err("Tab not found".to_string()));
                            }
                        }
                    }
                    McpCommand::ExecuteJs { tab_id, script, response } => {
                        let target_id = tab_id.or(Some(active_wv_id));
                        if let Some(id) = target_id {
                            if script.trim().is_empty() {
                                let _ = response.send(Err("script is empty".into()));
                                continue;
                            }
                            let _ = mcp_ensure_tab!(id);
                            if let Some(wv) = tab_webviews.get(&id) {
                                // Wrap sender so we can reclaim it if the callback never fires.
                                // eval_async_expr awaits Promises, so scripts returning one
                                // (fetch chains, waits) resolve to their settled value.
                                let response = std::sync::Arc::new(std::sync::Mutex::new(Some(response)));
                                let response_cb = response.clone();
                                async_eval::eval_async_expr(wv, &script, move |res| {
                                    if let Some(tx) = response_cb.lock().unwrap().take() {
                                        let _ = tx.send(res.map_err(|e| format!("JS error: {e}")));
                                    }
                                });
                                mcp_eval_err_watchdog!(response, 10_000, format!(
                                    "browser_execute_js callback was discarded — the page navigated or \
                                     the JS context tore down during the call. The script may or may not \
                                     have run. Verify with browser_get_tabs / browser_snapshot and retry. \
                                     Not auto-retried because the script's intent is unknown."
                                ));
                            } else {
                                let _ = response.send(Err("Tab not found".to_string()));
                            }
                        } else {
                            let _ = response.send(Err("No active tab".to_string()));
                        }
                    }
                    McpCommand::Click { tab_id, selector, response } => {
                        let target_id = tab_id.or(Some(active_wv_id));
                        if let Some(id) = target_id {
                            if selector.trim().is_empty() {
                                let _ = response.send(Err("selector is empty — pass a CSS selector or @ref from browser_snapshot".into()));
                                continue;
                            }
                            let _ = mcp_ensure_tab!(id);
                            if let Some(wv) = tab_webviews.get(&id) {
                                // Returns: "true" | "stale" | "missing" | "detached"
                                //   stale    → @ref but no snapshot taken yet (or refs cleared by navigation)
                                //   missing  → selector matched no element
                                //   detached → element existed but is no longer in the DOM
                                let script = dom_actions::click_script(&selector);
                                let response = std::sync::Arc::new(std::sync::Mutex::new(Some(response)));
                                let response_cb = response.clone();
                                let sel_for_err = selector.clone();
                                async_eval::eval_async_expr(wv, &script, move |res| {
                                    if let Some(tx) = response_cb.lock().unwrap().take() {
                                        let _ = tx.send(res
                                            .map_err(|e| format!("Click failed: {e}"))
                                            .and_then(|val| interpret_dom_result(&val, &sel_for_err)));
                                    }
                                });
                                mcp_eval_err_watchdog!(response, dom_actions::WATCHDOG_MS, format!(
                                    "Click on '{selector}' was dropped — the JS callback never fired, almost \
                                     always because the page navigated or re-rendered during the click \
                                     (common on heavy SPAs and with parallel clicks). The tab is still \
                                     alive; whether the click happened is unknown. Call browser_get_tabs \
                                     to confirm state, browser_snapshot to refresh @refs, then retry \
                                     sequentially."
                                ));
                            } else {
                                let _ = response.send(Err("Tab not found".to_string()));
                            }
                        } else {
                            let _ = response.send(Err("No active tab".to_string()));
                        }
                    }
                    McpCommand::Hover { tab_id, selector, response } => {
                        let target_id = tab_id.or(Some(active_wv_id));
                        if let Some(id) = target_id {
                            if selector.trim().is_empty() {
                                let _ = response.send(Err("selector is empty — pass a CSS selector or @ref from browser_snapshot".into()));
                                continue;
                            }
                            let _ = mcp_ensure_tab!(id);
                            if let Some(wv) = tab_webviews.get(&id) {
                                let script = dom_actions::hover_script(&selector);
                                let response = std::sync::Arc::new(std::sync::Mutex::new(Some(response)));
                                let response_cb = response.clone();
                                let sel_for_err = selector.clone();
                                async_eval::eval_async_expr(wv, &script, move |res| {
                                    if let Some(tx) = response_cb.lock().unwrap().take() {
                                        let _ = tx.send(res
                                            .map_err(|e| format!("Hover failed: {e}"))
                                            .and_then(|val| interpret_dom_result(&val, &sel_for_err)));
                                    }
                                });
                                mcp_eval_err_watchdog!(response, dom_actions::WATCHDOG_MS, format!(
                                    "Hover on '{selector}' was dropped (JS callback discarded by page \
                                     navigation/re-render). Re-snapshot and retry."
                                ));
                            } else {
                                let _ = response.send(Err("Tab not found".to_string()));
                            }
                        } else {
                            let _ = response.send(Err("No active tab".to_string()));
                        }
                    }
                    McpCommand::Type { tab_id, selector, text, response } => {
                        let target_id = tab_id.or(Some(active_wv_id));
                        if let Some(id) = target_id {
                            if selector.trim().is_empty() {
                                let _ = response.send(Err("selector is empty — pass a CSS selector or @ref from browser_snapshot".into()));
                                continue;
                            }
                            let _ = mcp_ensure_tab!(id);
                            if let Some(wv) = tab_webviews.get(&id) {
                                let script = dom_actions::type_script(&selector, &text);
                                let response = std::sync::Arc::new(std::sync::Mutex::new(Some(response)));
                                let response_cb = response.clone();
                                let sel_for_err = selector.clone();
                                async_eval::eval_async_expr(wv, &script, move |res| {
                                    if let Some(tx) = response_cb.lock().unwrap().take() {
                                        let _ = tx.send(res
                                            .map_err(|e| format!("Type failed: {e}"))
                                            .and_then(|val| interpret_dom_result(&val, &sel_for_err)));
                                    }
                                });
                                mcp_eval_err_watchdog!(response, dom_actions::WATCHDOG_MS, format!(
                                    "Type into '{selector}' was dropped (JS callback discarded by page \
                                     navigation/re-render). Whether the text was set is unknown — \
                                     re-snapshot, verify field state, and retry."
                                ));
                            } else {
                                let _ = response.send(Err("Tab not found".to_string()));
                            }
                        } else {
                            let _ = response.send(Err("No active tab".to_string()));
                        }
                    }
                    McpCommand::GetCurrentTab { response } => {
                        let tm = tabs.lock().unwrap();
                        let result = tm.active_tab().map(|t| {
                            TabInfo::from_tab(t, true)
                        }).ok_or("No active tab".to_string());
                        let _ = response.send(result);
                    }
                    McpCommand::GoBack { tab_id, response } => {
                        let target_id = tab_id.or(Some(active_wv_id));
                        if let Some(id) = target_id {
                            let _ = mcp_ensure_tab!(id);
                            if let Some(wv) = tab_webviews.get(&id) {
                                match wv.evaluate_script("history.back()") {
                                    Ok(()) => { let _ = response.send(Ok(())); }
                                    Err(e) => { let _ = response.send(Err(format!("GoBack failed: {e}"))); }
                                }
                            } else {
                                let _ = response.send(Err("Tab not found".to_string()));
                            }
                        } else {
                            let _ = response.send(Err("No active tab".to_string()));
                        }
                    }
                    McpCommand::GoForward { tab_id, response } => {
                        let target_id = tab_id.or(Some(active_wv_id));
                        if let Some(id) = target_id {
                            let _ = mcp_ensure_tab!(id);
                            if let Some(wv) = tab_webviews.get(&id) {
                                match wv.evaluate_script("history.forward()") {
                                    Ok(()) => { let _ = response.send(Ok(())); }
                                    Err(e) => { let _ = response.send(Err(format!("GoForward failed: {e}"))); }
                                }
                            } else {
                                let _ = response.send(Err("Tab not found".to_string()));
                            }
                        } else {
                            let _ = response.send(Err("No active tab".to_string()));
                        }
                    }
                    McpCommand::GetHistory { limit, response } => {
                        let mut tm = tabs.lock().unwrap();
                        tm.ensure_contiguous();
                        let history = tm.history();
                        let limit = limit.unwrap_or(50);
                        // history is oldest-first; return most recent first
                        let entries: Vec<HistoryInfo> = history.iter().rev().take(limit).map(|e| {
                            HistoryInfo::from_entry(e)
                        }).collect();
                        let _ = response.send(Ok(entries));
                    }
                    McpCommand::GetPlayingTabs { response } => {
                        let tm = tabs.lock().unwrap();
                        let active = tm.active_id();
                        let playing: Vec<TabInfo> = tm.tabs().iter()
                            .filter(|t| t.is_playing_audio)
                            .map(|t| TabInfo::from_tab(t, active == Some(t.id)))
                            .collect();
                        let _ = response.send(Ok(playing));
                    }
                    McpCommand::Reload { tab_id, response } => {
                        let target_id = tab_id.unwrap_or(active_wv_id);
                        let _ = mcp_ensure_tab!(target_id);
                        if let Some(wv) = tab_webviews.get(&target_id) {
                            let _ = wv.reload();
                            let _ = response.send(Ok(()));
                        } else {
                            let _ = response.send(Err("Tab not found".to_string()));
                        }
                    }
                    McpCommand::Screenshot { tab_id, full_page, response } => {
                        let target_id = tab_id.unwrap_or(active_wv_id);
                        let _ = mcp_ensure_tab!(target_id);
                        if let Some(wv) = tab_webviews.get(&target_id) {
                            let wv_ptr = objc2::rc::Retained::as_ptr(&wv.webview()) as usize;
                            let response = std::sync::Arc::new(std::sync::Mutex::new(Some(response)));

                            if full_page {
                                // Full page: createPDF → PDFDocument → stitch → PNG
                                let response_cb = response.clone();
                                let handler = block2::RcBlock::new(move |pdf_data: *mut objc2::runtime::AnyObject, error: *mut objc2::runtime::AnyObject| {
                                    let Some(tx) = response_cb.lock().unwrap().take() else { return };
                                    if pdf_data.is_null() {
                                        let msg = if !error.is_null() {
                                            unsafe {
                                                let desc: *mut objc2::runtime::AnyObject = objc2::msg_send![&*error, localizedDescription];
                                                if desc.is_null() { "createPDF failed".to_string() }
                                                else {
                                                    let bytes: *const u8 = objc2::msg_send![&*desc, UTF8String];
                                                    std::ffi::CStr::from_ptr(bytes.cast()).to_string_lossy().into_owned()
                                                }
                                            }
                                        } else { "createPDF returned nil".to_string() };
                                        let _ = tx.send(Err(msg));
                                        return;
                                    }
                                    unsafe {
                                        let Some(cg_image) = pdf_data_to_cgimage(pdf_data) else {
                                            let _ = tx.send(Err("Failed to render PDF to CGImage".to_string()));
                                            return;
                                        };
                                        copy_cgimage_to_clipboard(&cg_image);
                                        // Encode PNG only for MCP base64 transport
                                        let png_data = cgimage_to_png_data(&cg_image);
                                        if png_data.is_null() {
                                            let _ = tx.send(Err("Failed to encode PNG".to_string()));
                                            return;
                                        }
                                        let length: usize = objc2::msg_send![&*png_data, length];
                                        let bytes: *const u8 = objc2::msg_send![&*png_data, bytes];
                                        let slice = std::slice::from_raw_parts(bytes, length);
                                        use base64::Engine;
                                        let b64 = base64::engine::general_purpose::STANDARD.encode(slice);
                                        tracing::debug!(len = b64.len(), "Full page screenshot captured");
                                        let _ = tx.send(Ok(b64));
                                    }
                                });
                                unsafe {
                                    let wv_obj: *mut objc2::runtime::AnyObject = wv_ptr as *mut objc2::runtime::AnyObject;
                                    let nil: *const objc2::runtime::AnyObject = std::ptr::null();
                                    let _: () = objc2::msg_send![
                                        &*wv_obj,
                                        createPDFWithConfiguration: nil,
                                        completionHandler: &*handler
                                    ];
                                }
                            } else {
                                // Viewport: takeSnapshot → PNG
                                let response_cb = response.clone();
                                let handler = block2::RcBlock::new(move |image: *mut objc2::runtime::AnyObject, error: *mut objc2::runtime::AnyObject| {
                                    let Some(tx) = response_cb.lock().unwrap().take() else { return };
                                    if image.is_null() {
                                        let msg = if !error.is_null() {
                                            unsafe {
                                                let desc: *mut objc2::runtime::AnyObject = objc2::msg_send![&*error, localizedDescription];
                                                if desc.is_null() { "Screenshot failed".to_string() }
                                                else {
                                                    let bytes: *const u8 = objc2::msg_send![&*desc, UTF8String];
                                                    std::ffi::CStr::from_ptr(bytes.cast()).to_string_lossy().into_owned()
                                                }
                                            }
                                        } else { "Screenshot returned nil image".to_string() };
                                        let _ = tx.send(Err(msg));
                                        return;
                                    }
                                    unsafe {
                                        let png_data = nsimage_to_png_data(image);
                                        if png_data.is_null() {
                                            let _ = tx.send(Err("Failed to encode PNG".to_string()));
                                            return;
                                        }
                                        copy_png_to_clipboard(png_data);
                                        // Return base64-encoded PNG so MCP can pass it as an image
                                        let length: usize = objc2::msg_send![&*png_data, length];
                                        let bytes: *const u8 = objc2::msg_send![&*png_data, bytes];
                                        let slice = std::slice::from_raw_parts(bytes, length);
                                        use base64::Engine;
                                        let b64 = base64::engine::general_purpose::STANDARD.encode(slice);
                                        tracing::debug!(len = b64.len(), "Viewport screenshot captured");
                                        let _ = tx.send(Ok(b64));
                                    }
                                });
                                unsafe {
                                    let wv_obj: *mut objc2::runtime::AnyObject = wv_ptr as *mut objc2::runtime::AnyObject;
                                    let nil: *const objc2::runtime::AnyObject = std::ptr::null();
                                    let _: () = objc2::msg_send![
                                        &*wv_obj,
                                        takeSnapshotWithConfiguration: nil,
                                        completionHandler: &*handler
                                    ];
                                }
                            }
                        } else {
                            let _ = response.send(Err("Tab not found".to_string()));
                        }
                    }
                    McpCommand::GetPageContent { tab_id, response, is_retry } => {
                        let target_id = tab_id.unwrap_or(active_wv_id);
                        let _ = mcp_ensure_tab!(target_id);
                        if let Some(wv) = tab_webviews.get(&target_id) {
                            let response = std::sync::Arc::new(std::sync::Mutex::new(Some(response)));
                            let response_cb = response.clone();
                            match wv.evaluate_script_with_callback(
                                &webview_utils::well_formed_js("document.body ? document.body.innerText : ''"),
                                move |val| {
                                    if let Some(tx) = response_cb.lock().unwrap().take() {
                                        // val comes as a JSON string — strip outer quotes
                                        let text = serde_json::from_str::<String>(&val).unwrap_or(val);
                                        let _ = tx.send(Ok(sanitize::sanitize_text(&text)));
                                    }
                                },
                            ) {
                                Ok(()) => {
                                    if !is_retry {
                                        let arc_wd = response.clone();
                                        let tx_for_retry = mcp_cmd_tx.clone();
                                        std::thread::spawn(move || {
                                            std::thread::sleep(std::time::Duration::from_millis(3000));
                                            if let Some(tx) = arc_wd.lock().unwrap().take() {
                                                let _ = tx_for_retry.send(mcp::McpCommand::GetPageContent {
                                                    tab_id: Some(target_id),
                                                    response: tx,
                                                    is_retry: true,
                                                });
                                            }
                                        });
                                    } else {
                                        mcp_eval_err_watchdog!(response, 5000, format!(
                                            "browser_get_page_content callback was discarded twice — the page \
                                             keeps tearing down its JS context. Verify with browser_get_tabs \
                                             once the page is settled, then retry."
                                        ));
                                    }
                                }
                                Err(e) => {
                                    if let Some(tx) = response.lock().unwrap().take() {
                                        let _ = tx.send(Err(format!("JS error: {e}")));
                                    }
                                }
                            }
                        } else {
                            let _ = response.send(Err("Tab not found".to_string()));
                        }
                    }
                    McpCommand::Scroll { tab_id, direction, pixels, selector, response } => {
                        let target_id = tab_id.unwrap_or(active_wv_id);
                        let _ = mcp_ensure_tab!(target_id);
                        if let Some(wv) = tab_webviews.get(&target_id) {
                            if !matches!(direction.as_str(), "up" | "down" | "top" | "bottom") {
                                let _ = response.send(Err(format!("Invalid direction: {direction}. Use up, down, top, or bottom.")));
                                continue;
                            }
                            // Element-scoped scroll: scroll the element's nearest
                            // scrollable container via the actionability harness.
                            if let Some(sel) = selector.filter(|s| !s.trim().is_empty()) {
                                let script = dom_actions::scroll_script(&sel, &direction, pixels);
                                let response = std::sync::Arc::new(std::sync::Mutex::new(Some(response)));
                                let response_cb = response.clone();
                                let sel_for_err = sel.clone();
                                async_eval::eval_async_expr(wv, &script, move |res| {
                                    if let Some(tx) = response_cb.lock().unwrap().take() {
                                        let _ = tx.send(res
                                            .map_err(|e| format!("Scroll failed: {e}"))
                                            .and_then(|val| interpret_dom_result(&val, &sel_for_err))
                                            .map(|_| ()));
                                    }
                                });
                                mcp_eval_err_watchdog!(response, dom_actions::WATCHDOG_MS, format!(
                                    "Scroll at '{sel}' was dropped (JS callback discarded by page \
                                     navigation/re-render). Re-snapshot and retry."
                                ));
                                continue;
                            }
                            let script = match direction.as_str() {
                                "top" => "window.scrollTo(0, 0)".to_string(),
                                "bottom" => "window.scrollTo(0, Math.max(document.body.scrollHeight, document.documentElement.scrollHeight))".to_string(),
                                "up" => match pixels {
                                    Some(px) => format!("window.scrollBy(0, -{px})"),
                                    None => "window.scrollBy(0, -(window.innerHeight - 100))".to_string(),
                                },
                                "down" => match pixels {
                                    Some(px) => format!("window.scrollBy(0, {px})"),
                                    None => "window.scrollBy(0, window.innerHeight - 100)".to_string(),
                                },
                                _ => {
                                    let _ = response.send(Err(format!("Invalid direction: {direction}. Use up, down, top, or bottom.")));
                                    continue;
                                }
                            };
                            match wv.evaluate_script(&script) {
                                Ok(()) => { let _ = response.send(Ok(())); }
                                Err(e) => { let _ = response.send(Err(format!("Scroll failed: {e}"))); }
                            }
                        } else {
                            let _ = response.send(Err("Tab not found".to_string()));
                        }
                    }
                    McpCommand::PressKey { tab_id, key, selector, modifiers, response } => {
                        let target_id = tab_id.unwrap_or(active_wv_id);
                        if key.is_empty() {
                            let _ = response.send(Err("key is empty — pass a KeyboardEvent.key value like 'Enter', 'Escape', 'a'".into()));
                            continue;
                        }
                        if let Some(ref s) = selector {
                            if s.trim().is_empty() {
                                let _ = response.send(Err("selector is empty — omit it to target the focused element, or pass a CSS selector / @ref".into()));
                                continue;
                            }
                        }
                        let _ = mcp_ensure_tab!(target_id);
                        if let Some(wv) = tab_webviews.get(&target_id) {
                            let script =
                                dom_actions::press_key_script(selector.as_deref(), &key, &modifiers);
                            let response = std::sync::Arc::new(std::sync::Mutex::new(Some(response)));
                            let response_cb = response.clone();
                            let sel_for_err = selector.clone().unwrap_or_else(|| "(focused element)".into());
                            let sel_for_msg = sel_for_err.clone();
                            async_eval::eval_async_expr(wv, &script, move |res| {
                                if let Some(tx) = response_cb.lock().unwrap().take() {
                                    let _ = tx.send(res
                                        .map_err(|e| format!("PressKey failed: {e}"))
                                        .and_then(|val| interpret_dom_result(&val, &sel_for_err)));
                                }
                            });
                            mcp_eval_err_watchdog!(response, dom_actions::WATCHDOG_MS, format!(
                                "PressKey on '{sel_for_msg}' was dropped (JS callback discarded by page \
                                 navigation/re-render). Re-snapshot and retry."
                            ));
                        } else {
                            let _ = response.send(Err("Tab not found".to_string()));
                        }
                    }
                    McpCommand::Wait { tab_id, event, timeout_ms, response } => {
                        let target_id = tab_id.unwrap_or(active_wv_id);
                        let _ = mcp_ensure_tab!(target_id);
                        if let Some(wv) = tab_webviews.get(&target_id) {
                            let script = match event.as_str() {
                                "load" => format!(
                                    "new Promise(r => {{ if (document.readyState === 'complete') r('ready'); else {{ const t = setTimeout(() => r('timeout'), {timeout_ms}); window.addEventListener('load', () => {{ clearTimeout(t); r('ready'); }}, {{once: true}}); }} }})"
                                ),
                                "domcontentloaded" => format!(
                                    "new Promise(r => {{ if (document.readyState !== 'loading') r('ready'); else {{ const t = setTimeout(() => r('timeout'), {timeout_ms}); document.addEventListener('DOMContentLoaded', () => {{ clearTimeout(t); r('ready'); }}, {{once: true}}); }} }})"
                                ),
                                // Full SPA readiness — same probe browser_navigate uses.
                                // Has its own 8 s internal cap; the `timeout_ms` argument
                                // is intentionally ignored here.
                                "ready" => readiness_js::READINESS_JS.to_string(),
                                // Treat anything else as a CSS selector
                                selector => {
                                    let sel_json = serde_json::to_string(selector).unwrap_or_default();
                                    format!(
                                        "new Promise(r => {{ if (document.querySelector({sel_json})) return r('ready'); const t = setTimeout(() => {{ if (o) o.disconnect(); r('timeout'); }}, {timeout_ms}); const o = new MutationObserver(() => {{ if (document.querySelector({sel_json})) {{ o.disconnect(); clearTimeout(t); r('ready'); }} }}); o.observe(document.documentElement, {{childList: true, subtree: true}}); }})"
                                    )
                                }
                            };
                            let response = std::sync::Arc::new(std::sync::Mutex::new(Some(response)));
                            let response_cb = response.clone();
                            // The wait script itself can legitimately run up to `timeout_ms`
                            // before resolving with 'timeout'. The watchdog only fires if
                            // the callback was discarded — give the script timeout_ms + 5s.
                            let wd_ms = timeout_ms.saturating_add(5000);
                            async_eval::eval_async_expr(wv, &script, move |res| {
                                if let Some(tx) = response_cb.lock().unwrap().take() {
                                    // A JS exception mid-wait almost always means the
                                    // page navigated and tore down the wait's context —
                                    // for the AI that's a signal, not a failure.
                                    let _ = tx.send(Ok(match res {
                                        Ok(val) => serde_json::from_str::<String>(&val).unwrap_or(val),
                                        Err(_) => "interrupted: the page navigated mid-wait — \
                                                   call browser_wait again to wait on the new page"
                                            .to_string(),
                                    }));
                                }
                            });
                            mcp_eval_err_watchdog!(response, wd_ms, format!(
                                "browser_wait callback was discarded — the page navigated mid-wait. \
                                 If the navigation is what you wanted, call browser_get_tabs to verify \
                                 and continue; otherwise retry."
                            ));
                        } else {
                            let _ = response.send(Err("Tab not found".to_string()));
                        }
                    }
                    McpCommand::SelectOption { tab_id, selector, value, response } => {
                        let target_id = tab_id.unwrap_or(active_wv_id);
                        if selector.trim().is_empty() {
                            let _ = response.send(Err("selector is empty — pass a CSS selector or @ref of a <select>".into()));
                            continue;
                        }
                        let _ = mcp_ensure_tab!(target_id);
                        if let Some(wv) = tab_webviews.get(&target_id) {
                            // Returns: "true" | "stale" | "missing" | "detached" | "not-select" | "no-such-option"
                            let script = dom_actions::select_option_script(&selector, &value);
                            let response = std::sync::Arc::new(std::sync::Mutex::new(Some(response)));
                            let response_cb = response.clone();
                            let sel_for_err = selector.clone();
                            let sel_for_msg = sel_for_err.clone();
                            let val_for_err = value.clone();
                            async_eval::eval_async_expr(wv, &script, move |res| {
                                if let Some(tx) = response_cb.lock().unwrap().take() {
                                    let result = res
                                        .map_err(|e| format!("SelectOption failed: {e}"))
                                        .and_then(|val| {
                                            match val.trim().trim_matches('"') {
                                                "not-select" => Err(format!("Element matched by '{sel_for_err}' is not a <select>")),
                                                "no-such-option" => Err(format!("<select> '{sel_for_err}' has no option with value='{val_for_err}'")),
                                                _ => interpret_dom_result(&val, &sel_for_err),
                                            }
                                        });
                                    let _ = tx.send(result);
                                }
                            });
                            mcp_eval_err_watchdog!(response, dom_actions::WATCHDOG_MS, format!(
                                "SelectOption on '{sel_for_msg}' was dropped (JS callback discarded by \
                                 page navigation/re-render). Re-snapshot and retry."
                            ));
                        } else {
                            let _ = response.send(Err("Tab not found".to_string()));
                        }
                    }
                    McpCommand::Snapshot { tab_id, response, is_retry } => {
                        let target_id = tab_id.unwrap_or(active_wv_id);
                        let _ = mcp_ensure_tab!(target_id);
                        if let Some(wv) = tab_webviews.get(&target_id) {
                            let response = std::sync::Arc::new(std::sync::Mutex::new(Some(response)));
                            let response_cb = response.clone();
                            match wv.evaluate_script_with_callback(
                                &webview_utils::well_formed_js(snapshot_js::SNAPSHOT_JS),
                                move |val| {
                                    if let Some(tx) = response_cb.lock().unwrap().take() {
                                        let text = serde_json::from_str::<String>(&val).unwrap_or(val);
                                        let _ = tx.send(Ok(text));
                                    }
                                },
                            ) {
                                Ok(()) => {
                                    if !is_retry {
                                        // Idempotent read — auto-retry once via the MCP channel
                                        // if the callback is discarded. The retry is dispatched
                                        // by the watchdog thread; the retry itself runs with
                                        // is_retry=true so a second failure produces a clean
                                        // error instead of looping.
                                        let arc_wd = response.clone();
                                        let tx_for_retry = mcp_cmd_tx.clone();
                                        std::thread::spawn(move || {
                                            std::thread::sleep(std::time::Duration::from_millis(3000));
                                            if let Some(tx) = arc_wd.lock().unwrap().take() {
                                                let _ = tx_for_retry.send(mcp::McpCommand::Snapshot {
                                                    tab_id: Some(target_id),
                                                    response: tx,
                                                    is_retry: true,
                                                });
                                            }
                                        });
                                    } else {
                                        mcp_eval_err_watchdog!(response, 5000, format!(
                                            "browser_snapshot callback was discarded twice — the page keeps \
                                             tearing down its JS context (heavy SPA re-renders, navigation \
                                             loop). Call browser_get_tabs to confirm the tab is settled, then \
                                             retry once the page is quiet."
                                        ));
                                    }
                                }
                                Err(e) => {
                                    if let Some(tx) = response.lock().unwrap().take() {
                                        let _ = tx.send(Err(format!("Snapshot failed: {e}")));
                                    }
                                }
                            }
                        } else {
                            let _ = response.send(Err("Tab not found".to_string()));
                        }
                    }
                }
            }
        }

        match event {
            // ── Hide overlay (from JS Esc / backdrop click) ───────────────
            Event::UserEvent(AppEvent::HideOverlay) => {
                objc2_app_kit::NSCursor::setHiddenUntilMouseMoves(false);
                overlay_win.set_visible(false);
                overlay_visible = false;
                overlay_hotkey_visible.store(false, Ordering::Relaxed);
            }

            // ── Hide shortcuts overlay (from JS Esc / backdrop click) ─────
            Event::UserEvent(AppEvent::HideShortcuts) => {
                shortcuts_win.set_visible(false);
                shortcuts_visible = false;
            }

            // ── Address bar URL edit: grow just enough for the dropdown ───────
            // While expanded, the address bar webview adds room for ~8 suggestion
            // rows below the 32 px titlebar strip so the dropdown is visible.
            Event::UserEvent(AppEvent::UrlEditExpand(expanded)) => {
                address_bar_editing = expanded;
                let sz = browser_win.inner_size();
                let scale = browser_win.scale_factor();
                let expanded_extra: u32 = if expanded {
                    (340.0 * scale) as u32
                } else {
                    0
                };
                let _ = address_bar_wv.set_bounds(wry::Rect {
                    position: tao::dpi::PhysicalPosition::new(0u32, 0u32).into(),
                    size: tao::dpi::PhysicalSize::new(sz.width, address_bar_h + expanded_extra).into(),
                });
            }

            // ── Cmd+E: trigger URL edit (same as clicking the pencil) ─────────
            // Move keyboard first-responder to the address bar webview FIRST so
            // ⌃A / ⌃E / typing reach the URL input. Without this, the input is
            // focused at the DOM level but the underlying NSView isn't first
            // responder (it was on the content webview or wherever the user
            // pressed ⌘E from), and keystrokes are routed to that other view.
            Event::UserEvent(AppEvent::UrlEditRequest) => {
                let _ = address_bar_wv.focus();
                let _ = address_bar_wv.evaluate_script(
                    "window.__urlEditOpen && window.__urlEditOpen()"
                );
            }

            // ── Address bar URL edit: push history+tabs snapshot for autocomplete ─
            Event::UserEvent(AppEvent::PushAddressHistory) => {
                let json = {
                    let mut tm = tabs.lock().unwrap();
                    tm.ensure_contiguous();
                    let hib: std::collections::HashSet<usize> = pending_tabs.keys().copied().collect();
                    webview_utils::build_items_json(tm.tabs(), tm.history(), &favicon_cache, &hib)
                };
                let _ = address_bar_wv.evaluate_script(&format!(
                    "window.__setSuggestions && window.__setSuggestions({json})"
                ));
            }

            // ── Toggle shortcuts overlay ──────────────────────────────────
            // ── Settings modal (⌘,) ──────────────────────────────────────
            Event::UserEvent(AppEvent::ToggleSettings) => {
                if settings_visible {
                    settings_win.set_visible(false);
                    settings_visible = false;
                    keybind_capturing.store(false, Ordering::Relaxed);
                } else {
                    let sz = browser_win.inner_size();
                    settings_win.set_inner_size(tao::dpi::PhysicalSize::new(sz.width, sz.height));
                    if let Ok(pos) = browser_win.outer_position() {
                        settings_win.set_outer_position(pos);
                    }
                    // Inject current config values into the UI
                    let config_json = serde_json::to_string(&cfg).unwrap_or_default();
                    let _ = settings_wv.evaluate_script(&format!(
                        "window.__setConfig && window.__setConfig({})", config_json
                    ));
                    // Inject current keybindings into the Keybindings tab
                    if let Ok(k) = keymap.read() {
                        let _ = settings_wv.evaluate_script(&format!(
                            "window.__setKeybindings && window.__setKeybindings({})",
                            k.ui_json()
                        ));
                    }
                    settings_win.set_visible(true);
                    settings_win.set_focus();
                    settings_visible = true;
                }
            }
            Event::UserEvent(AppEvent::HideSettings) => {
                settings_win.set_visible(false);
                settings_visible = false;
                keybind_capturing.store(false, Ordering::Relaxed);
            }
            Event::UserEvent(AppEvent::UpdateConfig(key, val)) => {
                match key.as_str() {
                    "home_page" => cfg.home_page = val,
                    "search_engine" => {
                        search_engine = val.clone();
                        cfg.search_engine = val;
                    }
                    "max_history" => {
                        if let Ok(n) = val.parse::<usize>() { cfg.max_history = n; }
                    }
                    "window_width" => {
                        if let Ok(n) = val.parse::<u32>() { cfg.window_width = n; }
                    }
                    "window_height" => {
                        if let Ok(n) = val.parse::<u32>() { cfg.window_height = n; }
                    }
                    "ai_edit_auto_hide" => {
                        cfg.ai_edit_auto_hide = val == "true";
                    }
                    "max_prompt_history" => {
                        if let Ok(n) = val.parse::<usize>() {
                            cfg.max_prompt_history = n;
                            prompt_history.truncate(n);
                        }
                    }
                    "max_ai_prompt_history" => {
                        if let Ok(n) = val.parse::<usize>() {
                            cfg.max_ai_prompt_history = n;
                            ai_prompt_history.truncate(n);
                        }
                    }
                    "proactive_learning" => {
                        cfg.proactive_learning = val == "true";
                        if cfg.proactive_learning {
                            if learning_next_at.is_none() {
                                learning_next_at = Some(std::time::Instant::now() + std::time::Duration::from_secs(cfg.learning_interval_min * 60));
                            }
                        } else {
                            learning_next_at = None;
                            learning_handle = None;
                        }
                    }
                    "learning_interval_min" => {
                        if let Ok(n) = val.parse::<u64>() {
                            cfg.learning_interval_min = n.max(5);
                            if cfg.proactive_learning {
                                learning_next_at = Some(std::time::Instant::now() + std::time::Duration::from_secs(cfg.learning_interval_min * 60));
                            }
                        }
                    }
                    "aggressive_hibernation" => {
                        cfg.aggressive_hibernation = val == "true";
                        proactive_config = hibernation::ProactiveConfig::from_total_memory(
                            hibernation::total_system_memory(),
                            cfg.aggressive_hibernation,
                        );
                        tracing::info!(
                            ?proactive_config,
                            aggressive = cfg.aggressive_hibernation,
                            "Proactive hibernation config updated"
                        );
                    }
                    _ => {}
                }
                cfg.save();
            }

            Event::UserEvent(AppEvent::KeybindRecord(action, chord)) => {
                let err = match keymap.write() {
                    Ok(mut k) => k.rebind(&action, &chord).err(),
                    Err(_) => Some("Internal error".to_string()),
                };
                sync_keybindings!(err);
            }
            Event::UserEvent(AppEvent::KeybindReset(action)) => {
                let err = match keymap.write() {
                    Ok(mut k) => k.reset(&action).err(),
                    Err(_) => Some("Internal error".to_string()),
                };
                sync_keybindings!(err);
            }
            Event::UserEvent(AppEvent::KeybindResetAll) => {
                if let Ok(mut k) = keymap.write() {
                    k.reset_all();
                }
                sync_keybindings!(None::<String>);
            }
            Event::UserEvent(AppEvent::KeybindCapture(on)) => {
                keybind_capturing.store(on, Ordering::Relaxed);
            }

            Event::UserEvent(AppEvent::ToggleShortcuts) => {
                if shortcuts_visible {
                    shortcuts_win.set_visible(false);
                    shortcuts_visible = false;
                } else {
                    let sz = browser_win.inner_size();
                    shortcuts_win.set_inner_size(tao::dpi::PhysicalSize::new(sz.width, sz.height));
                    if let Ok(pos) = browser_win.outer_position() {
                        shortcuts_win.set_outer_position(pos);
                    }
                    // Reflect live keybindings in the Global column.
                    if let Ok(k) = keymap.read() {
                        let _ = shortcuts_wv.evaluate_script(&format!(
                            "window.__setShortcuts && window.__setShortcuts({})",
                            k.ui_json()
                        ));
                    }
                    shortcuts_win.set_visible(true);
                    shortcuts_win.set_focus();
                    shortcuts_visible = true;
                }
            }

            // ── Toggle overlay ────────────────────────────────────────────
            Event::UserEvent(AppEvent::ToggleOverlay) => {
                if overlay_visible {
                    objc2_app_kit::NSCursor::setHiddenUntilMouseMoves(false);
                    overlay_win.set_visible(false);
                    overlay_visible = false;
                    overlay_hotkey_visible.store(false, Ordering::Relaxed);
                } else {
                    let sz = browser_win.inner_size();
                    overlay_win.set_inner_size(tao::dpi::PhysicalSize::new(sz.width, sz.height));
                    if let Ok(pos) = browser_win.outer_position() {
                        overlay_win.set_outer_position(pos);
                    }
                    let json = {
                        let mut tm = tabs.lock().unwrap();
                        tm.ensure_contiguous();
                        let hib: std::collections::HashSet<usize> = pending_tabs.keys().copied().collect();
                        webview_utils::build_items_json(tm.tabs(), tm.history(), &favicon_cache, &hib)
                    };
                    let se = webview_utils::escape_js_template(&search_engine);
                    let _ = overlay_wv.evaluate_script(&format!(
                        "window.__setSearchEngine && window.__setSearchEngine(`{se}`)"
                    ));
                    let _ = overlay_wv.evaluate_script(&format!(
                    "window.__setItems && window.__setItems({json})"
                ));
                    overlay_win.set_visible(true);
                    overlay_win.set_focus();
                    objc2_app_kit::NSCursor::setHiddenUntilMouseMoves(true);
                    overlay_visible = true;
                    overlay_hotkey_visible.store(true, Ordering::Relaxed);

                    // Prefetch DNS for top visited domains — macOS DNS cache is
                    // system-wide, so resolving here benefits subsequent tab navigations.
                    if let Some(wv) = tab_webviews.get(&active_wv_id) {
                        let domains: Vec<String> = {
                            let tm = tabs.lock().unwrap();
                            let mut seen = std::collections::HashSet::new();
                            tm.history()
                                .iter()
                                .filter_map(|h| {
                                    webview_utils::extract_domain(&h.url)
                                        .map(|d| d.to_string())
                                })
                                .filter(|d| seen.insert(d.clone()))
                                .take(10)
                                .collect()
                        };
                        if !domains.is_empty() {
                            let js: String = domains
                                .iter()
                                .map(|d| {
                                    format!(
                                        "{{var l=document.createElement('link');l.rel='dns-prefetch';l.href='https://{}';document.head.appendChild(l);}}",
                                        d
                                    )
                                })
                                .collect();
                            let _ = wv.evaluate_script(&js);
                        }
                    }
                }
            }

            // ── Navigate: new tab with its own WebView ────────────────────
            Event::UserEvent(AppEvent::NavigateTo(raw)) => {
                overlay_visible = false;
                overlay_hotkey_visible.store(false, Ordering::Relaxed);
                // External scheme (tg://, figma://, mailto:, etc.) → hand off to macOS
                if url::is_external_scheme(&raw) {
                    macos::open_external_url(&raw);
                    browser_win.set_focus();
                    return;
                }
                let url = url::resolve_url(&raw, &search_engine);
                let tab_id = tabs.lock().unwrap().open(url.clone());
                // Keep the currently *visible* tab on screen while new one loads.
                // If a swap is already pending, the visible tab is the old one from that swap;
                // the old new_id tab is now orphaned — clean it up.
                let visible_id = if let Some((old, orphan)) = pending_swap.take() {
                    // Orphaned tab: was loading but superseded by this new navigation.
                    if orphan != old {
                        tabs.lock().unwrap().close(orphan);
                        if let Some(wv) = tab_webviews.get(&orphan) {
                            let wv_ptr = objc2::rc::Retained::as_ptr(&wv.webview()) as usize;
                            nav_error_patch::unregister(wv_ptr);
                            nav_error_patch::unregister_termination(wv_ptr);
                        }
                        tab_webviews.remove(&orphan);
                        pending_tabs.remove(&orphan);
                        mru.retain(|&x| x != orphan);
                        tracing::debug!(orphan, "NavigateTo: cleaned up orphaned tab");
                    }
                    old
                } else {
                    active_wv_id
                };
                spawn_tab_webview!(tab_id, &url);
                active_wv_id = tab_id;
                pending_swap = Some((visible_id, tab_id));
                tracing::debug!(tab_id, visible_id, url, "NavigateTo: pending_swap set");
                macos::mru_push(&mut mru, tab_id);
                browser_win.set_focus();
                // Show progress bar immediately — PageLoadStarted fires on didCommitNavigation
                // (after server responds), so we'd miss the DNS/TCP/request phase entirely.
                if url != "about:blank" {
                    progress_hide_at = None;
                    let _ = progress_wv.evaluate_script("window.__start && window.__start()");
                    if !progress_visible {
                        let _ = progress_wv.set_visible(true);
                        progress_visible = true;
                    }
                }
            }

            // ── Open in new tab: Cmd+click or target=_blank ───────────────
            Event::UserEvent(AppEvent::OpenInNewTab(url)) => {
                // External scheme (tg://, figma://, mailto:, etc.) → hand off to macOS
                if url::is_external_scheme(&url) {
                    macos::open_external_url(&url);
                    browser_win.set_focus();
                    return;
                }
                let tab_id = tabs.lock().unwrap().open(url.clone());
                // Keep the currently visible tab on screen while new one loads.
                // Clean up orphaned tab if a swap was already pending.
                let visible_id = if let Some((old, orphan)) = pending_swap.take() {
                    if orphan != old {
                        tabs.lock().unwrap().close(orphan);
                        if let Some(wv) = tab_webviews.get(&orphan) {
                            let wv_ptr = objc2::rc::Retained::as_ptr(&wv.webview()) as usize;
                            nav_error_patch::unregister(wv_ptr);
                            nav_error_patch::unregister_termination(wv_ptr);
                        }
                        tab_webviews.remove(&orphan);
                        pending_tabs.remove(&orphan);
                        mru.retain(|&x| x != orphan);
                        tracing::debug!(orphan, "OpenInNewTab: cleaned up orphaned tab");
                    }
                    old
                } else {
                    active_wv_id
                };
                spawn_tab_webview!(tab_id, &url);
                active_wv_id = tab_id;
                pending_swap = Some((visible_id, tab_id));
                macos::mru_push(&mut mru, tab_id);
                browser_win.set_focus();
            }

            // ── Switch tab: hide current, show target — no reload ─────────
            Event::UserEvent(AppEvent::SwitchTab(tab_id)) => {
                overlay_visible = false;
                overlay_hotkey_visible.store(false, Ordering::Relaxed);
                // Hide find bar on tab switch — highlights are per-tab
                if find_bar_visible {
                    let _ = find_bar_wv.set_visible(false);
                    let _ = find_bar_wv.evaluate_script("window.__clear && window.__clear()");
                    find_bar_visible = false;
                    find_bar_hotkey_visible.store(false, Ordering::Relaxed);
                    if let Some(wv) = tab_webviews.get(&active_wv_id) {
                        let _ = wv.evaluate_script("window.__findClear && window.__findClear()");
                    }
                }
                // Hide inline edit modal on tab switch — selection is per-tab
                if inline_edit_visible || inline_edit_pending {
                    let _ = inline_edit_wv.set_visible(false);
                    let _ = inline_edit_wv.evaluate_script("window.__clear && window.__clear()");
                    inline_edit_visible = false;
                    inline_edit_pending = false;
                    inline_edit_hotkey_visible.store(false, Ordering::Relaxed);
                    if let Some(ref h) = inline_edit_acp { h.cancel(); }
                    inline_edit_acp = None;
                    inline_edit_response.clear();
                }
                if tab_id == active_wv_id {
                    focus_active_webview!();
                    return;
                }
                switch_visible_tab!(tab_id);
                macos::mru_push(&mut mru, tab_id);
                // Reset stats so first sample after switch starts fresh (no stale CPU delta)
                sys_stats_last = None;
                sys_stats_cpu.store(0, Ordering::Relaxed);
                sys_stats_mem.store(0, Ordering::Relaxed);
                sys_stats_next_at = std::time::Instant::now();
                let _ = address_bar_wv.evaluate_script("window.__sysStats && window.__sysStats(null, null)");
                focus_active_webview!();
            }

            // ── Close tab ─────────────────────────────────────────────────
            Event::UserEvent(AppEvent::CloseTab(tab_id)) => {
                let id = {
                    let tm = tabs.lock().unwrap();
                    if tab_id == 0 {
                        tm.active_id()
                    } else {
                        Some(tab_id)
                    }
                };
                if let Some(id) = id {
                    // Cancel any pending swap involving this tab
                    if let Some((old_id, new_id)) = pending_swap.take() {
                        if id == new_id {
                            // Closing the loading tab — old tab is still visible, keep it
                        } else if id == old_id {
                            // Closing the old visible tab — show the new one
                            if let Some(wv) = tab_webviews.get(&new_id) {
                                let _ = wv.set_visible(true);
                            }
                        } else {
                            // Closing an unrelated tab — restore pending_swap
                            pending_swap = Some((old_id, new_id));
                        }
                    }
                    tabs.lock().unwrap().close(id);
                    if let Some(wv) = tab_webviews.get(&id) {
                        let wv_ptr = objc2::rc::Retained::as_ptr(&wv.webview()) as usize;
                        nav_error_patch::unregister(wv_ptr);
                        nav_error_patch::unregister_termination(wv_ptr);
                        dialog_patch::unregister(wv_ptr);
                    }
                    tab_webviews.remove(&id);
                    media_playing_tabs.remove(&id);  // Clean up stale media state
                    pending_tabs.remove(&id);  // Also remove if it was pending lazy load
                    tab_snapshots.remove(&id);
                    deferred_nav.remove(&id);
                    restoring_tabs.remove(&id);
                    if let Some((_, response)) = mcp_nav_pending.remove(&id) {
                        let _ = response.send(Err("Tab closed".into()));
                    }
                    mru.retain(|&x| x != id);
                    // Pick a successor only when the closed tab was the visible
                    // one. Closing a background tab (the normal MCP cleanup path)
                    // must not re-switch tabs or steal the user's focus.
                    if id == active_wv_id {
                        // Switch to the most-recently-used tab (MRU[0] after removal)
                        match mru.first().copied() {
                            Some(next) => {
                                tabs.lock().unwrap().switch(next);
                                // switch_visible_tab! handles lazy loading (pending_tabs) and
                                // sets active_wv_id + updates address bar — covers both loaded
                                // and not-yet-loaded tabs.
                                switch_visible_tab!(next);
                                if is_app_active() {
                                    if overlay_visible {
                                        overlay_win.set_focus();
                                    } else {
                                        browser_win.set_focus();
                                    }
                                }
                            }
                            None => *control_flow = ControlFlow::Exit,
                        }
                    } else if tabs.lock().unwrap().tabs().is_empty() {
                        *control_flow = ControlFlow::Exit;
                    }
                    // Refresh overlay if it's open so the closed tab disappears
                    refresh_overlay!();
                }
            }

            // ── Remove history entry (from overlay × button) ────────────────
            Event::UserEvent(AppEvent::RemoveHistory(url)) => {
                if tabs.lock().unwrap().remove_history(&url) {
                    history_save_at.get_or_insert(std::time::Instant::now() + std::time::Duration::from_secs(60));
                }
                // Refresh overlay so the removed entry disappears
                refresh_overlay!();
            }

            // ── Ctrl+P: switch to previous tab in MRU order ───────────────
            Event::UserEvent(AppEvent::PrevTab) => {
                if overlay_visible {
                    return;
                }
                // Find current position in MRU, go to next older tab
                if mru.len() < 2 {
                    return;
                }
                let pos = mru.iter().position(|&id| id == active_wv_id).unwrap_or(0);
                let target = mru[(pos + 1) % mru.len()];
                switch_visible_tab!(target);
                browser_win.set_focus();
            }

            // ── Ctrl+N: switch to next tab in MRU order ───────────────────
            Event::UserEvent(AppEvent::NextTab) => {
                if overlay_visible {
                    return;
                }
                // Find current position in MRU, go to next newer tab (wrap to end)
                if mru.len() < 2 {
                    return;
                }
                let pos = mru.iter().position(|&id| id == active_wv_id).unwrap_or(0);
                let target = if pos == 0 { *mru.last().unwrap() } else { mru[pos - 1] };
                switch_visible_tab!(target);
                browser_win.set_focus();
            }

            // ── Toggle sidebar (Cmd+Shift+A or JS sidebar_close) ──────────
            // Sidebar overlays on top of the page — no tab/footer resizing.
            Event::UserEvent(AppEvent::ToggleSidebar) => {
                let sz = browser_win.inner_size();
                if sidebar_visible {
                    let _ = sidebar_wv.set_visible(false);
                    sidebar_visible = false;
                    sidebar_fullscreen = false; // reset on hide so next open is normal-width
                    sidebar_hotkey_visible.store(false, Ordering::Relaxed);
                    sidebar_owns_key.store(false, Ordering::Relaxed);
                    // Return key window status to browser_win so the page
                    // WebView receives keyboard input again.
                    browser_win.set_focus();
                } else {
                    let w = if sidebar_fullscreen { sz.width } else { sidebar_w };
                    let x = if sidebar_fullscreen { 0u32 } else { sz.width.saturating_sub(sidebar_w) };
                    let _ = sidebar_wv.set_bounds(wry::Rect {
                        position: tao::dpi::PhysicalPosition::new(x, 0u32).into(),
                        size: tao::dpi::PhysicalSize::new(w, sz.height).into(),
                    });
                    let _ = sidebar_wv.set_visible(true);
                    sidebar_visible = true;
                    sidebar_hotkey_visible.store(true, Ordering::Relaxed);
                    sidebar_owns_key.store(true, Ordering::Relaxed);
                    // Seed sidebar with persisted AI prompt history so older prompts
                    // are reachable via Ctrl+P / Ctrl+N inside any session.
                    let hist_json = serde_json::to_string(&ai_prompt_history)
                        .unwrap_or_else(|_| "[]".into());
                    let _ = sidebar_wv.evaluate_script(&format!(
                        "window.__setHistory && window.__setHistory({hist_json})"
                    ));
                    // Clear unread badge on address bar 🐙 button
                    let _ = address_bar_wv.evaluate_script(
                        "window.__setBadge && window.__setBadge(false)"
                    );
                    // Hide notification toast if visible
                    if notification_visible {
                        let _ = notification_wv.evaluate_script(
                            "window.__hide && window.__hide()"
                        );
                        let _ = notification_wv.set_visible(false);
                        notification_visible = false;
                    }
                    // Make chrome_win the key window so its child WebView can
                    // accept first-responder, then focus the sidebar WKWebView
                    // and the textarea inside it.
                    unsafe {
                        use objc2::msg_send;
                        use objc2::runtime::AnyObject;
                        let ns_win: *mut AnyObject = chrome_win.ns_window() as *mut AnyObject;
                        let _: () = msg_send![ns_win, makeKeyWindow];
                    }
                    let _ = sidebar_wv.focus();
                    let _ = sidebar_wv.evaluate_script(
                        "(function(){var el=document.getElementById('prompt-input');if(!el)return;\
                         var n=0;(function f(){if(n++>20)return;el.focus();\
                         if(document.activeElement===el)return;setTimeout(f,30)})()})()"
                    );
                    // Reconnect any session whose handle has died (e.g. after
                    // max retries exceeded — manual sidebar open re-arms it).
                    for s in sessions.iter_mut() {
                        if s.handle.is_none() {
                            s.retry_count = 0;
                            s.reconnect_gen += 1; // invalidate any pending backoff timers
                            // Must carry --resume: this loop also hits lazily
                            // unconnected persisted sessions — a bare spawn would
                            // mint a fresh octomind session and the Connected
                            // handler would overwrite (lose) the saved resume id.
                            let cmd = match &s.acp_session_id {
                                Some(id) => format!("octomind acp {} --resume {}", s.tag, id),
                                None => format!("octomind acp {}", s.tag),
                            };
                            s.handle = acp::AcpHandle::connect(
                                &cmd,
                                make_wake(acp_proxy.clone()),
                            )
                            .ok();
                            let sid = s.id;
                            let _ = sidebar_wv.evaluate_script(&format!(
                                "window.__setSessionStatus && window.__setSessionStatus({sid},'connecting')"
                            ));
                        }
                    }
                }
            }

            // ── Toggle native macOS fullscreen on the browser window (⌘Return) ─
            // chrome_win is a borderless child — fullscreen-primary lives on
            // browser_win, fullscreen-auxiliary on chrome_win (set at creation).
            Event::UserEvent(AppEvent::ToggleFullscreen) => {
                unsafe {
                    use objc2::msg_send;
                    use objc2::runtime::AnyObject;
                    let ns_win: *mut AnyObject = browser_win.ns_window() as *mut AnyObject;
                    let _: () = msg_send![ns_win, toggleFullScreen: std::ptr::null_mut::<AnyObject>()];
                }
            }

            // ── Toggle assistant fullscreen — sidebar fills the chrome window
            //     (⌘⇧Return or click the icon in the sidebar header). Bounded
            //     by chrome window, NOT a native macOS fullscreen — orthogonal
            //     to ToggleFullscreen so the two can be combined.
            Event::UserEvent(AppEvent::ToggleSidebarFullscreen) => {
                if !sidebar_visible {
                    // No sense expanding a hidden sidebar; surface it first then expand.
                    let _ = proxy.send_event(AppEvent::ToggleSidebar);
                    sidebar_fullscreen = true;
                } else {
                    sidebar_fullscreen = !sidebar_fullscreen;
                    let sz = browser_win.inner_size();
                    let w = if sidebar_fullscreen { sz.width } else { sidebar_w };
                    let x = if sidebar_fullscreen { 0u32 } else { sz.width.saturating_sub(sidebar_w) };
                    let _ = sidebar_wv.set_bounds(wry::Rect {
                        position: tao::dpi::PhysicalPosition::new(x, 0u32).into(),
                        size: tao::dpi::PhysicalSize::new(w, sz.height).into(),
                    });
                    // Inform the sidebar JS so the toggle icon can flip its state.
                    let _ = sidebar_wv.evaluate_script(&format!(
                        "window.__setSidebarFullscreen && window.__setSidebarFullscreen({})",
                        sidebar_fullscreen
                    ));
                }
            }

            // ── Toggle DevTools (Cmd+Shift+I) ────────────────────────────────────
            Event::UserEvent(AppEvent::ToggleDevTools) => {
                if let Some(wv) = tab_webviews.get(&active_wv_id) {
                    if wv.is_devtools_open() {
                        wv.close_devtools();
                    } else {
                        wv.open_devtools();
                    }
                }
            }

            // ── ACP events ─────────────────────────────────────────────────────
            Event::UserEvent(AppEvent::AcpPrompt(sid, text, images)) => {
                // Record prompt in shared AI history (MRU, dedup) so it persists
                // across restarts and seeds future sessions.
                if !text.is_empty() {
                    if let Some(pos) = ai_prompt_history.iter().position(|p| p == &text) {
                        ai_prompt_history.remove(pos);
                    }
                    ai_prompt_history.insert(0, text.clone());
                    ai_prompt_history.truncate(cfg.max_ai_prompt_history);
                    let snapshot = ai_prompt_history.clone();
                    std::thread::spawn(move || config::save_ai_prompt_history(&snapshot));
                }
                let mut should_persist = false;
                if let Some(s) = sessions.iter_mut().find(|s| s.id == sid) {
                    // New turn starts now — anchors `turn_ms` on the flush.
                    s.turn_started = Some(std::time::Instant::now());
                    if !text.is_empty() {
                        push_acp_msg(s, "user", text.clone(), cfg.max_acp_session_messages);
                        should_persist = true;
                    }
                    // Lazy-spawn safety net: prompts can race ahead of an
                    // explicit switch on first activation of a persisted session.
                    if s.handle.is_none() {
                        let cmd = match &s.acp_session_id {
                            Some(id) => format!("octomind acp {} --resume {}", s.tag, id),
                            None => format!("octomind acp {}", s.tag),
                        };
                        s.handle = acp::AcpHandle::connect(
                            &cmd,
                            make_wake(acp_proxy.clone()),
                        )
                        .ok();
                        s.retry_count = 0;
                        s.reconnect_gen += 1;
                    }
                    if let Some(ref handle) = s.handle {
                        if !handle.send_prompt(text, images) {
                            // Channel dead — drop handle so reconnection can re-arm.
                            s.handle = None;
                            let _ = sidebar_wv.evaluate_script(&format!(
                                "window.__setSessionStatus && window.__setSessionStatus({sid},'error')"
                            ));
                        }
                    }
                }
                if should_persist {
                    persist_acp_history(&sessions, active_session_id, cfg.max_acp_session_messages);
                }
            }

            // ── ACP cancel (stop button) ───────────────────────────────────────────
            // First click: polite ACP cancel notification.
            // Second click within 12s: force-kill the subprocess and respawn the
            // handle with the same agent tag. This is the user's escape hatch when
            // the agent ignores `CancelNotification` (e.g. wedged on a syscall).
            Event::UserEvent(AppEvent::AcpSignIn(sid)) => {
                // Kick off `/login`; the LoginStarted event opens the browser tab.
                if let Some(h) = sessions.iter().find(|s| s.id == sid).and_then(|s| s.handle.as_ref()) {
                    let _ = h.send_command("/login".to_string());
                }
            }
            Event::UserEvent(AppEvent::AcpRefreshAccount(sid)) => {
                if let Some(h) = sessions.iter().find(|s| s.id == sid).and_then(|s| s.handle.as_ref()) {
                    let _ = h.send_command("/usage".to_string());
                }
            }
            Event::UserEvent(AppEvent::AcpCancel(sid)) => {
                let mut should_persist = false;
                if let Some(s) = sessions.iter_mut().find(|s| s.id == sid) {
                    let now = std::time::Instant::now();
                    let recent = s
                        .last_cancel_at
                        .map(|t| now.duration_since(t) < std::time::Duration::from_secs(12))
                        .unwrap_or(false);

                    if recent {
                        let tag = s.tag.clone();
                        let resume = s.acp_session_id.clone();
                        s.retry_count = 0;
                        s.reconnect_gen += 1;
                        s.handle = None; // drop -> kill_on_drop fires on the child
                        let cmd = match &resume {
                            Some(id) => format!("octomind acp {} --resume {}", tag, id),
                            None => format!("octomind acp {}", tag),
                        };
                        s.handle = acp::AcpHandle::connect(
                            &cmd,
                            make_wake(acp_proxy.clone()),
                        )
                        .ok();
                        s.last_cancel_at = None;
                        // Save the partial turn (the user just force-killed
                        // mid-stream — keep what they already saw) and the
                        // synthetic error so a restart shows the same trail.
                        flush_agent_turn(s, cfg.max_acp_session_messages);
                        push_acp_msg(s, "error", "agent force-stopped and reconnected".to_string(), cfg.max_acp_session_messages);
                        should_persist = true;
                        let _ = sidebar_wv.evaluate_script(&format!(
                            "window.__setThinking && window.__setThinking({sid},false)"
                        ));
                        let _ = sidebar_wv.evaluate_script(&format!(
                            "window.__setSessionStatus && window.__setSessionStatus({sid},'connecting')"
                        ));
                        let _ = sidebar_wv.evaluate_script(&format!(
                            "window.__appendError && window.__appendError({sid},`agent force-stopped and reconnected`)"
                        ));
                    } else if let Some(ref handle) = s.handle {
                        handle.cancel();
                        s.last_cancel_at = Some(now);
                    }
                }
                if should_persist {
                    persist_acp_history(&sessions, active_session_id, cfg.max_acp_session_messages);
                }
            }

            // ── ACP set agent for an existing session (kills + respawns its handle) ──
            Event::UserEvent(AppEvent::AcpSetAgent(sid, tag)) => {
                let mut should_persist = false;
                if let Some(s) = sessions.iter_mut().find(|s| s.id == sid) {
                    s.tag = tag.clone();
                    s.retry_count = 0;
                    s.reconnect_gen += 1;
                    s.acp_session_id = None; // user switched agent — start fresh
                    s.handle = None; // drop old (kills subprocess)
                    s.handle = acp::AcpHandle::connect(
                        &format!("octomind acp {}", tag),
                        make_wake(acp_proxy.clone()),
                    )
                    .ok();
                    // Switching agents discards the prior conversation context.
                    s.messages.clear();
                    s.agent_buf.clear();
                    s.tool_buf.clear();
                    s.tool_starts.clear();
                    s.turn_started = None;
                    s.available_commands_json = None;
                    should_persist = true;
                    let _ = sidebar_wv.evaluate_script(&format!(
                        "window.__setSessionStatus && window.__setSessionStatus({sid},'connecting')"
                    ));
                    let escaped = webview_utils::escape_js_template(&tag);
                    let _ = sidebar_wv.evaluate_script(&format!(
                        "window.__updateSessionTag && window.__updateSessionTag({sid},`{escaped}`)"
                    ));
                }
                if should_persist {
                    persist_acp_history(&sessions, active_session_id, cfg.max_acp_session_messages);
                }
            }

            // ── ACP clear session (same agent, fresh chat) ────────────────────────
            Event::UserEvent(AppEvent::AcpClearSession(sid)) => {
                let mut should_persist = false;
                if let Some(s) = sessions.iter_mut().find(|s| s.id == sid) {
                    s.retry_count = 0;
                    s.reconnect_gen += 1;
                    s.acp_session_id = None; // explicit fresh chat
                    s.handle = None; // drop old (kills subprocess)
                    s.handle = acp::AcpHandle::connect(
                        &format!("octomind acp {}", s.tag),
                        make_wake(acp_proxy.clone()),
                    )
                    .ok();
                    // User asked for a fresh chat — drop the persisted log too.
                    s.messages.clear();
                    s.agent_buf.clear();
                    s.tool_buf.clear();
                    s.tool_starts.clear();
                    s.turn_started = None;
                    should_persist = true;
                    let _ = sidebar_wv.evaluate_script(&format!(
                        "window.__setSessionStatus && window.__setSessionStatus({sid},'connecting')"
                    ));
                }
                if should_persist {
                    persist_acp_history(&sessions, active_session_id, cfg.max_acp_session_messages);
                }
            }

            // ── ⌘T in sidebar — open the new-session panel and focus title ─────────
            Event::UserEvent(AppEvent::AcpSessionCreatePanel) => {
                let _ = sidebar_wv.evaluate_script(
                    "window.__openCreatePanel && window.__openCreatePanel()",
                );
            }

            // ── ACP create new session (capped at MAX_SESSIONS) ────────────────────
            Event::UserEvent(AppEvent::AcpSessionCreate(title, tag)) => {
                if sessions.len() >= MAX_SESSIONS {
                    tracing::warn!(max = MAX_SESSIONS, "session create rejected — cap reached");
                } else {
                    let sid = next_session_id;
                    next_session_id += 1;
                    let handle = acp::AcpHandle::connect(
                        &format!("octomind acp {}", tag),
                        make_wake(acp_proxy.clone()),
                    )
                    .ok();
                    sessions.push(AcpSession {
                        id: sid,
                        title: title.clone(),
                        tag: tag.clone(),
                        handle,
                        retry_count: 0,
                        reconnect_gen: 0,
                        last_cancel_at: None,
                        acp_session_id: None,
                        messages: Vec::new(),
                        agent_buf: String::new(),
                        tool_buf: Vec::new(),
                        tool_starts: HashMap::new(),
                        turn_started: None,
                        available_commands_json: None,
                    account_json: None,
                        octomind_pid: None,
                    });
                    active_session_id = sid; // auto-switch to new session
                    let etitle = webview_utils::escape_js_template(&title);
                    let etag = webview_utils::escape_js_template(&tag);
                    let _ = sidebar_wv.evaluate_script(&format!(
                        "window.__addSession && window.__addSession({sid},`{etitle}`,`{etag}`,'connecting')"
                    ));
                    let _ = sidebar_wv.evaluate_script(&format!(
                        "window.__switchSession && window.__switchSession({sid})"
                    ));
                    persist_acp_history(&sessions, active_session_id, cfg.max_acp_session_messages);
                }
            }

            // ── ⌘W in sidebar — close currently active session ─────────────────────
            Event::UserEvent(AppEvent::AcpSessionCloseActive) => {
                let sid = active_session_id;
                let _ = proxy.send_event(AppEvent::AcpSessionClose(sid));
            }

            // ── ACP close session (refused if it's the only one) ───────────────────
            Event::UserEvent(AppEvent::AcpSessionClose(sid)) => {
                if sessions.len() <= 1 {
                    tracing::debug!(session_id = sid, "session close ignored — last session");
                } else if let Some(pos) = sessions.iter().position(|s| s.id == sid) {
                    let _removed = sessions.remove(pos); // drop kills subprocess
                    // If we closed the active session, fall back to the first one.
                    if active_session_id == sid {
                        active_session_id = sessions[0].id;
                        let new_active = active_session_id;
                        let _ = sidebar_wv.evaluate_script(&format!(
                            "window.__switchSession && window.__switchSession({new_active})"
                        ));
                    }
                    let _ = sidebar_wv.evaluate_script(&format!(
                        "window.__removeSession && window.__removeSession({sid})"
                    ));
                    persist_acp_history(&sessions, active_session_id, cfg.max_acp_session_messages);
                }
            }

            // ── ACP switch active session ──────────────────────────────────────────
            Event::UserEvent(AppEvent::AcpSessionSwitch(sid))
                if sessions.iter().any(|s| s.id == sid) =>
            {
                active_session_id = sid;
                // Lazy-spawn: if this session has no live handle (persisted
                // but not yet connected this run), bring it up now.
                if let Some(s) = sessions.iter_mut().find(|s| s.id == sid) {
                    if s.handle.is_none() {
                        let cmd = match &s.acp_session_id {
                            Some(id) => format!("octomind acp {} --resume {}", s.tag, id),
                            None => format!("octomind acp {}", s.tag),
                        };
                        s.handle = acp::AcpHandle::connect(
                            &cmd,
                            make_wake(acp_proxy.clone()),
                        )
                        .ok();
                        s.retry_count = 0;
                        s.reconnect_gen += 1;
                        let _ = sidebar_wv.evaluate_script(&format!(
                            "window.__setSessionStatus && window.__setSessionStatus({sid},'connecting')"
                        ));
                    }
                }
                let _ = sidebar_wv.evaluate_script(&format!(
                    "window.__switchSession && window.__switchSession({sid})"
                ));
                persist_acp_history(&sessions, active_session_id, cfg.max_acp_session_messages);
            }

            // ── ACP rename session (title only) ────────────────────────────────────
            Event::UserEvent(AppEvent::AcpSessionRename(sid, title)) => {
                let mut should_persist = false;
                if let Some(s) = sessions.iter_mut().find(|s| s.id == sid) {
                    s.title = title.clone();
                    should_persist = true;
                    let etitle = webview_utils::escape_js_template(&title);
                    let _ = sidebar_wv.evaluate_script(&format!(
                        "window.__renameSession && window.__renameSession({sid},`{etitle}`)"
                    ));
                }
                if should_persist {
                    persist_acp_history(&sessions, active_session_id, cfg.max_acp_session_messages);
                }
            }

            // ── Sidebar JS finished init — push persisted state to JS ──────────────
            // The JS initDefault always creates sid=1 as a placeholder. Here we
            // reconcile JS state with Rust state: rename/retag sid=1 if needed,
            // add any extra persisted sessions, replay messages, restore the
            // last-active session, and re-broadcast cached available_commands.
            Event::UserEvent(AppEvent::SidebarReady) => {
                let rust_has_sid1 = sessions.iter().any(|s| s.id == 1);
                for s in sessions.iter() {
                    let sid = s.id;
                    let etitle = webview_utils::escape_js_template(&s.title);
                    let etag = webview_utils::escape_js_template(&s.tag);
                    // Status mirrors the live handle state. Lazy sessions
                    // (handle=None, no error) appear 'ready' — they connect
                    // on first switch. Truly errored sessions don't survive
                    // restart because we drop the handle on connection error.
                    let status = if s.handle.is_some() && s.acp_session_id.is_some() {
                        "ready"
                    } else if s.handle.is_some() {
                        "connecting"
                    } else {
                        "ready" // lazy — not yet spawned, will connect on switch
                    };
                    // No-op if the session is already in the JS map (e.g. sid=1
                    // from initDefault). Title/tag/status are pushed below.
                    let _ = sidebar_wv.evaluate_script(&format!(
                        "window.__addSession && window.__addSession({sid},`{etitle}`,`{etag}`,'{status}')"
                    ));
                    // Override the JS placeholder values for sid=1 (it was added
                    // before we knew the persisted title/tag).
                    if sid == 1 {
                        let _ = sidebar_wv.evaluate_script(&format!(
                            "window.__renameSession && window.__renameSession({sid},`{etitle}`)"
                        ));
                        let _ = sidebar_wv.evaluate_script(&format!(
                            "window.__updateSessionTag && window.__updateSessionTag({sid},`{etag}`)"
                        ));
                        let _ = sidebar_wv.evaluate_script(&format!(
                            "window.__setSessionStatus && window.__setSessionStatus({sid},'{status}')"
                        ));
                    }
                    if let Some(ref json) = s.available_commands_json {
                        let escaped = webview_utils::escape_js_template(json);
                        let _ = sidebar_wv.evaluate_script(&format!(
                            "window.__setAvailableCommands && window.__setAvailableCommands({sid},`{escaped}`)"
                        ));
                    }
                    if let Some(ref json) = s.account_json {
                        let escaped = webview_utils::escape_js_template(json);
                        let _ = sidebar_wv.evaluate_script(&format!(
                            "window.__setAccount && window.__setAccount({sid},`{escaped}`)"
                        ));
                    }
                    if !s.messages.is_empty() {
                        if let Ok(msgs_json) = serde_json::to_string(&s.messages) {
                            let _ = sidebar_wv.evaluate_script(&format!(
                                "window.__replayMessages && window.__replayMessages({sid},{msgs_json})"
                            ));
                        }
                    }
                }
                // Strip the JS placeholder sid=1 if persisted history doesn't
                // include it (user closed it before quitting last session).
                if !rust_has_sid1 {
                    let _ = sidebar_wv.evaluate_script(
                        "window.__removeSession && window.__removeSession(1)"
                    );
                }
                let active = active_session_id;
                let _ = sidebar_wv.evaluate_script(&format!(
                    "window.__switchSession && window.__switchSession({active})"
                ));
            }

            // ── Sidebar re-focus after transient app reactivation ─────────
            // Posted by the NSApplicationDidBecomeActive observer (macos.rs)
            // when our app reactivates and the sidebar still owns logical
            // focus. Re-promotes the WKWebView and the textarea so the user's
            // typing focus survives notification banners, Spotlight, etc.
            Event::UserEvent(AppEvent::SidebarReFocus) if sidebar_visible => {
                let _ = sidebar_wv.focus();
                let _ = sidebar_wv.evaluate_script(
                    "(function(){var el=document.getElementById('prompt-input');\
                     if(!el)return;if(document.activeElement===el)return;\
                     var n=0;(function f(){if(n++>20)return;el.focus();\
                     if(document.activeElement===el)return;setTimeout(f,30)})()})()"
                );
            }

            // ── Ask AI: open sidebar + inject prompt ──────────────────────
            Event::UserEvent(AppEvent::AskAI(text)) => {
                // Open sidebar if not already visible
                if !sidebar_visible {
                    let _ = proxy.send_event(AppEvent::ToggleSidebar);
                }
                // Inject the prompt into the sidebar and submit it
                let escaped = webview_utils::escape_js_template(&text);
                let _ = sidebar_wv.evaluate_script(&format!(
                    "window.__injectPrompt && window.__injectPrompt(`{escaped}`)"
                ));
            }

            // ── Quick-slot: open saved URL ────────────────────────────────
            // If a tab with this URL is already open → switch to it.
            // Otherwise → open in a new tab (preserves current tab, e.g. music).
            Event::UserEvent(AppEvent::QuickSlotOpen(slot)) => {
                if let Some(ref qs) = quick_slots[slot] {
                    let url = qs.url.clone();
                    let existing_tab = {
                        let tm = tabs.lock().unwrap();
                        let normalized = url.trim_end_matches('/');
                        tm.tabs().iter()
                            .find(|t| t.url.trim_end_matches('/') == normalized)
                            .map(|t| t.id)
                    };
                    if let Some(tab_id) = existing_tab {
                        let _ = proxy.send_event(AppEvent::SwitchTab(tab_id));
                    } else {
                        let _ = proxy.send_event(AppEvent::NavigateTo(url));
                    }
                }
            }

            // ── Quick-slot: save current page to slot ─────────────────────
            Event::UserEvent(AppEvent::QuickSlotSave(slot)) => {
                let info = {
                    let tm = tabs.lock().unwrap();
                    tm.active_tab().map(|t| (t.url.clone(), t.title.clone()))
                };
                if let Some((url, title)) = info {
                    if url == "about:blank" || url.is_empty() {
                        // On blank page: remove the slot
                        quick_slots[slot] = None;
                    } else {
                        // Save current page into the slot
                        let favicon = webview_utils::cached_favicon(&url, &favicon_cache)
                            .map(String::from);
                        quick_slots[slot] = Some(quickslots::QuickSlot {
                            url,
                            title,
                            favicon,
                        });
                    }
                    quickslots::save(&quick_slots);
                    sync_quickslots_ui!();
                }
            }

            // ── Quick-slot: remove slot ───────────────────────────────────
            Event::UserEvent(AppEvent::QuickSlotRemove(slot)) => {
                quick_slots[slot] = None;
                quickslots::save(&quick_slots);
                sync_quickslots_ui!();
            }

            // ── ACP wake — no-op, just wakes the loop so ACP poll runs ──
            Event::UserEvent(AppEvent::AcpWake) => {}

            // ── A2UI envelope changed on disk → render in the active session ─
            // We don't try to bind surfaces to a specific octomind subprocess.
            // The agent only renders during its own turn, so the foreground
            // session is the right place for the surface to appear.
            Event::UserEvent(AppEvent::A2uiUpdate(snap)) => {
                // Route by `parent_pid` — match the session whose octomind
                // subprocess invoked render_ui. The bash script stamps its
                // $PPID into the envelope on creation. Falls back to the
                // active session only if no match (e.g. before any session
                // has registered its pid, or for envelopes from outside).
                let target_sid = sessions
                    .iter()
                    .find(|s| s.octomind_pid == Some(snap.parent_pid) && snap.parent_pid != 0)
                    .map(|s| s.id)
                    .unwrap_or(active_session_id);
                let sid = target_sid;
                // A surface is "live" only if its render_ui bash subprocess is
                // still polling — i.e. its parent octomind subprocess matches
                // a session we're currently running. Anything else is a ghost:
                // file on disk, but no one to receive the resolution. Clicks
                // on ghost surfaces are suppressed with a toast in the sidebar.
                let is_live = snap.parent_pid != 0
                    && sessions
                        .iter()
                        .any(|s| s.octomind_pid == Some(snap.parent_pid));
                let live_js = if is_live { "true" } else { "false" };
                let body_json = serde_json::to_string(&snap.body).unwrap_or_else(|_| "{}".into());
                let escaped_id = webview_utils::escape_js_template(&snap.id);
                let escaped_body = webview_utils::escape_js_template(&body_json);
                if snap.status == "pending" {
                    let _ = sidebar_wv.evaluate_script(&format!(
                        "window.__a2uiUpdate && window.__a2uiUpdate({sid},`{escaped_id}`,JSON.parse(`{escaped_body}`),{live_js})"
                    ));
                } else {
                    let _ = sidebar_wv.evaluate_script(&format!(
                        "window.__a2uiResolved && window.__a2uiResolved({sid},`{escaped_id}`,JSON.parse(`{escaped_body}`))"
                    ));
                }
                // Persist the surface alongside the chat log so a cold restart
                // can rebuild the bubble. Same fileId upserts in place — keeps
                // pending→resolved transitions to a single bubble.
                if let Some(s) = sessions.iter_mut().find(|s| s.id == sid) {
                    upsert_a2ui_msg(s, &snap.id, snap.body, cfg.max_acp_session_messages);
                    persist_acp_history(&sessions, active_session_id, cfg.max_acp_session_messages);
                }
            }
            // ── Learning wake — no-op, just wakes the loop so learning poll runs ──
            Event::UserEvent(AppEvent::LearningWake) => {}

            // ── Learning ready — active tab text extracted, build prompt and send ──
            Event::UserEvent(AppEvent::LearningReady(page_text))
                if learning_handle.is_none() && cfg.proactive_learning =>
            {
                let mut tm = tabs.lock().unwrap();
                tm.ensure_contiguous();
                if let Some(prompt) = build_learning_prompt(&tm, active_wv_id, &page_text) {
                    drop(tm);
                    tracing::info!(prompt_len = prompt.len(), "starting proactive learning run");
                    let lp = proxy.clone();
                    learning_handle = acp::AcpHandle::connect(
                        "octomind acp octoweb:learning",
                        move || { let _ = lp.send_event(AppEvent::LearningWake); },
                    ).ok();
                    if let Some(ref h) = learning_handle {
                        h.send_prompt(prompt, vec![]);
                    }
                }
            }

            // ── ACP reconnection attempt (scheduled after error) ──
            Event::UserEvent(AppEvent::AcpReconnect(sid, gen)) => {
                if let Some(s) = sessions.iter_mut().find(|s| s.id == sid) {
                    // Ignore stale timers from a previous error cycle — the user may
                    // have manually reconnected (AcpSetAgent / AcpClearSession) in the
                    // meantime, which already bumped this session's reconnect_gen.
                    if gen != s.reconnect_gen {
                        tracing::debug!(session_id = sid, gen, current = s.reconnect_gen, "ignoring stale ACP reconnect timer");
                    } else {
                        tracing::info!(session_id = sid, retry = s.retry_count, "attempting ACP reconnection");
                        s.handle = None; // drop old handle if any
                        let cmd = match &s.acp_session_id {
                            Some(id) => format!("octomind acp {} --resume {}", s.tag, id),
                            None => format!("octomind acp {}", s.tag),
                        };
                        s.handle = acp::AcpHandle::connect(
                            &cmd,
                            make_wake(acp_proxy.clone()),
                        )
                        .ok();
                    }
                } else {
                    tracing::debug!(session_id = sid, "reconnect for unknown session — ignoring");
                }
            }

            // ── Dismiss notification toast ─────────────────────────────────────
            Event::UserEvent(AppEvent::DismissNotification)
                if notification_visible =>
            {
                let _ = notification_wv.evaluate_script(
                    "window.__hide && window.__hide()"
                );
                let _ = notification_wv.set_visible(false);
                notification_visible = false;
            }

            // ── Find bar: toggle ⌘F ──────────────────────────────────────────
            Event::UserEvent(AppEvent::ToggleFindBar) => {
                if find_bar_visible {
                    // Hide find bar and clear highlights
                    let _ = find_bar_wv.set_visible(false);
                    let _ = find_bar_wv.evaluate_script("window.__clear && window.__clear()");
                    find_bar_visible = false;
                    find_bar_hotkey_visible.store(false, Ordering::Relaxed);
                    if let Some(wv) = tab_webviews.get(&active_wv_id) {
                        let _ = wv.evaluate_script("window.__findClear && window.__findClear()");
                    }
                    browser_win.set_focus();
                } else {
                    // Position find bar at top-right, just below address bar
                    let sz = browser_win.inner_size();
                    let _ = find_bar_wv.set_bounds(wry::Rect {
                        position: tao::dpi::PhysicalPosition::new(
                            sz.width.saturating_sub(find_bar_w + 8),
                            address_bar_h,
                        ).into(),
                        size: tao::dpi::PhysicalSize::new(find_bar_w, find_bar_h).into(),
                    });
                    let _ = find_bar_wv.set_visible(true);
                    find_bar_visible = true;
                    find_bar_hotkey_visible.store(true, Ordering::Relaxed);
                    // Focus the find bar input via chrome_win (child WebView needs key window)
                    unsafe {
                        use objc2::msg_send;
                        use objc2::runtime::AnyObject;
                        let ns_win: *mut AnyObject = chrome_win.ns_window() as *mut AnyObject;
                        let _: () = msg_send![ns_win, makeKeyWindow];
                    }
                    let _ = find_bar_wv.focus();
                    let _ = find_bar_wv.evaluate_script("window.__focus && window.__focus()");
                }
            }

            // ── Find bar: hide (Esc / close button) ─────────────────────────
            Event::UserEvent(AppEvent::HideFindBar) if find_bar_visible => {
                let _ = find_bar_wv.set_visible(false);
                let _ = find_bar_wv.evaluate_script("window.__clear && window.__clear()");
                find_bar_visible = false;
                find_bar_hotkey_visible.store(false, Ordering::Relaxed);
                if let Some(wv) = tab_webviews.get(&active_wv_id) {
                    let _ = wv.evaluate_script("window.__findClear && window.__findClear()");
                }
                browser_win.set_focus();
            }

            // ── Find bar: search query from input ───────────────────────────
            Event::UserEvent(AppEvent::FindInPage(query)) => {
                if let Some(wv) = tab_webviews.get(&active_wv_id) {
                    let escaped = webview_utils::escape_js_template(&query);
                    let _ = wv.evaluate_script(&format!(
                        "window.__findInPage && window.__findInPage(`{escaped}`)"
                    ));
                }
            }

            // ── Find bar: next match ────────────────────────────────────────
            Event::UserEvent(AppEvent::FindNext) => {
                if let Some(wv) = tab_webviews.get(&active_wv_id) {
                    let _ = wv.evaluate_script("window.__findNext && window.__findNext()");
                }
            }

            // ── Find bar: previous match ────────────────────────────────────
            Event::UserEvent(AppEvent::FindPrev) => {
                if let Some(wv) = tab_webviews.get(&active_wv_id) {
                    let _ = wv.evaluate_script("window.__findPrev && window.__findPrev()");
                }
            }

            // ── Find bar: match count from tab WebView ──────────────────────
            Event::UserEvent(AppEvent::FindCount(current, total))
                if find_bar_visible =>
            {
                let _ = find_bar_wv.evaluate_script(&format!(
                    "window.__setCount && window.__setCount({current}, {total})"
                ));
            }

            // ── Download started — show toast, keep the tab open ────────────
            Event::UserEvent(AppEvent::DownloadStarted(_tab_id, filename)) => {
                tracing::debug!(%filename, "Download started");
                let msg = format!("Downloading: {filename}…");
                let escaped = webview_utils::escape_js_template(&msg);
                if !notification_visible {
                    let _ = notification_wv.set_visible(true);
                    notification_visible = true;
                }
                let _ = notification_wv.evaluate_script(&format!(
                    "window.__show && window.__show(`{escaped}`, `\u{2B07}\u{FE0F}`, `Download`)"
                ));
            }

            // ── Download completed — show notification toast ─────────────────
            Event::UserEvent(AppEvent::DownloadCompleted(filename, success)) => {
                let (msg, icon) = if success {
                    (format!("Downloaded: {filename}"), "\u{2705}")
                } else {
                    (format!("Download failed: {filename}"), "\u{274C}")
                };
                tracing::debug!(%msg, "Download completed");
                let escaped = webview_utils::escape_js_template(&msg);
                if !notification_visible {
                    let _ = notification_wv.set_visible(true);
                    notification_visible = true;
                }
                let _ = notification_wv.evaluate_script(&format!(
                    "window.__show && window.__show(`{escaped}`, `{icon}`, `Download`, 4000)"
                ));
            }

            // ── JS Dialog captured by dialog_patch (auto-dismissed; logged for visibility) ─
            Event::UserEvent(AppEvent::JsDialog(info)) => {
                tracing::debug!(tab_id = info.tab_id, ?info.dialog_type, %info.message, "JS dialog auto-dismissed");
            }

            // ── Quit ──────────────────────────────────────────────────────
            Event::UserEvent(AppEvent::Quit) => {
                crash_report::log_exit_trigger("Quit");
                save_and_exit(
                    &tabs,
                    &favicon_cache,
                    &prompt_history,
                    &ai_prompt_history,
                    &sessions,
                    active_session_id,
                    cfg.max_acp_session_messages,
                    control_flow,
                );
            }

            // ── Title update ──────────────────────────────────────────────
            Event::UserEvent(AppEvent::TitleChanged(tab_id, title)) => {
                if tabs.lock().unwrap().update_title(tab_id, title.clone()) {
                    history_save_at.get_or_insert(std::time::Instant::now() + std::time::Duration::from_secs(60));
                }
                if tab_id == active_wv_id {
                    browser_win.set_title(&title);
                    let escaped = webview_utils::escape_js_template(&title);
                    let _ = address_bar_wv.evaluate_script(&format!(
                        "window.__setTitle && window.__setTitle(`{escaped}`)"
                    ));
                }
            }

            // ── URL update from page load ─────────────────────────────────
            Event::UserEvent(AppEvent::BrowserUrlChanged(tab_id, url)) => {
                tracing::debug!(tab_id, %url, "BrowserUrlChanged");
                // During snapshot restore, suppress about:blank URL changes so the
                // real URL in TabManager is not clobbered by the transient snapshot load.
                if restoring_tabs.contains(&tab_id) {
                    if deferred_nav.contains_key(&tab_id) {
                        // Still in snapshot phase — suppress entirely
                        return;
                    }
                    // deferred_nav consumed → real URL is now loading, stop suppressing
                    restoring_tabs.remove(&tab_id);
                }
                if tabs.lock().unwrap().update_url(tab_id, url.clone()) {
                    history_save_at.get_or_insert(std::time::Instant::now() + std::time::Duration::from_secs(60));
                }
                if tab_id == active_wv_id {
                    update_address_bar_url!(url);
                    // Reset window title — TitleChanged will set the real one once the page loads.
                    browser_win.set_title(&url);
                }
            }

            // ── Favicon fetched from page — store in cache ────────────────
            // Only update + save when we get a new/changed entry (avoids redundant writes).
            // FIFO eviction at FAVICON_CAP keeps memory bounded.
            Event::UserEvent(AppEvent::FaviconFetched(domain, data_uri)) => {
                if favicon_cache.get(&domain).map(|s| s.as_str()) != Some(&data_uri) {
                    let is_new = !favicon_cache.contains_key(&domain);
                    if is_new && favicon_order.len() >= FAVICON_CAP {
                        // Evict oldest entry to stay within cap.
                        if let Some(oldest) = favicon_order.pop_front() {
                            favicon_cache.remove(&oldest);
                        }
                    }
                    favicon_cache.insert(domain.clone(), data_uri.clone());
                    if is_new {
                        favicon_order.push_back(domain.clone());
                    }
                    config::save_favicons(&favicon_cache);
                }
                // Push to address bar ONLY if this domain matches the active tab's domain
                let active_url = tabs.lock().unwrap().tabs().iter()
                    .find(|t| t.id == active_wv_id)
                    .map(|t| t.url.clone())
                    .unwrap_or_default();
                // Extract domain from active URL and compare
                let active_domain = webview_utils::extract_domain(&active_url);
                if active_domain == Some(domain.as_str()) {
                    let escaped = webview_utils::escape_js_template(&data_uri);
                    let _ = address_bar_wv.evaluate_script(&format!(
                        "window.__setFavicon && window.__setFavicon(`{escaped}`)"
                    ));
                }
            }

            // ── Page load progress ───────────────────────────────────────────
            // Only show progress for the active tab — background tabs load silently.
            // Skip progress bar for about:blank (instant, no network).
            Event::UserEvent(AppEvent::PageLoadStarted(tab_id)) => {
                tracing::debug!(tab_id, active_wv_id, ?pending_swap, "PageLoadStarted");
                // Deferred swap: first bytes received (didCommitNavigation) — page is rendering,
                // safe to show it now and hide the old tab.
                if let Some((old_id, new_id)) = pending_swap {
                    if tab_id == new_id {
                        if let Some(wv) = tab_webviews.get(&new_id) {
                            let _ = wv.set_visible(true);
                        }
                        if old_id != new_id {
                            if let Some(wv) = tab_webviews.get(&old_id) {
                                let _ = wv.set_visible(false);
                            }
                        }
                        pending_swap = None;
                        // The page just became visible after a keyboard-driven
                        // navigation (palette / address bar). The palette was the
                        // key window; now that it's gone, hand keyboard
                        // first-responder to the page so typing works without a
                        // click. `browser_win.set_focus()` (in NavigateTo) only
                        // set window-level key, not the view-level responder.
                        focus_active_webview!();
                    }
                }
                if tab_id == active_wv_id {
                    // Dismiss find bar — highlights are per-page and become stale on navigation
                    if find_bar_visible {
                        let _ = find_bar_wv.set_visible(false);
                        let _ = find_bar_wv.evaluate_script("window.__clear && window.__clear()");
                        find_bar_visible = false;
                        find_bar_hotkey_visible.store(false, Ordering::Relaxed);
                    }
                    // NOTE: we intentionally do NOT dismiss the inline edit modal on
                    // PageLoadStarted. Many sites trigger main-frame navigations as a
                    // side-effect of in-page activity (SPA route changes, ad redirects,
                    // tracker handoffs, meta-refresh, late `location.replace`). Closing
                    // the modal here surfaced as a flaky "⌘⇧E auto-cancels until I
                    // click the page first" bug. The captured selection range may go
                    // stale on actual navigation — the replace will gracefully no-op
                    // when `window.__octoweb_edit` is gone — but the user's prompt
                    // input is preserved. Real dismissal still happens on user-driven
                    // close (Esc/✕), tab switch, and `NavigateTo` from the address bar.
                    // Check if this is about:blank — skip progress bar for instant pages
                    let url = tabs.lock().unwrap().tabs().iter()
                        .find(|t| t.id == tab_id)
                        .map(|t| t.url.clone())
                        .unwrap_or_default();
                    if url != "about:blank" {
                        // Cancel any pending hide — new load started
                        progress_hide_at = None;
                        if !progress_visible {
                            // Link click or in-page navigation — bar not yet started, start it now.
                            // (NavigateTo already starts the bar immediately, so we skip __start()
                            // here to avoid resetting the animation mid-flight at didCommitNavigation.)
                            let _ = progress_wv.evaluate_script("window.__start && window.__start()");
                            let _ = progress_wv.set_visible(true);
                            progress_visible = true;
                        }
                    }
                    // Clear address bar stats for new navigation
                    let _ = address_bar_wv.evaluate_script("window.__clear && window.__clear()");
                    let _ = address_bar_wv.evaluate_script("window.__sysStats && window.__sysStats(null, null)");
                    sys_stats_last = None;
                    sys_stats_cpu.store(0, Ordering::Relaxed);
                    sys_stats_mem.store(0, Ordering::Relaxed);
                }
            }

            Event::UserEvent(AppEvent::PageLoadFinished(tab_id)) => {
                tracing::debug!(tab_id, active_wv_id, ?pending_swap, "PageLoadFinished");
                // Deferred navigation: snapshot HTML just rendered — now load the real URL.
                // The snapshot stays visible (WebKit paint-holding) until the real page paints.
                if let Some(url) = deferred_nav.remove(&tab_id) {
                    if let Some(wv) = tab_webviews.get(&tab_id) {
                        let _ = wv.load_url(&url);
                    }
                    // Don't hide progress bar yet — the real page load will fire its own events.
                    return;
                }
                if tab_id == active_wv_id && progress_visible {
                    let _ = progress_wv.evaluate_script("window.__finish && window.__finish()");
                    // Hide after CSS fade completes (width 0.2s + opacity 0.3s delay 0.1s = 600ms, +50ms buffer)
                    progress_hide_at = Some(std::time::Instant::now() + std::time::Duration::from_millis(650));
                }
                // MCP navigate: page finished loading — wait for DOM stability
                // (SPA frameworks render in microtasks/rAF after load).
                if let Some((_, response)) = mcp_nav_pending.remove(&tab_id) {
                    if let Some(wv) = tab_webviews.get(&tab_id) {
                        let response = std::sync::Arc::new(std::sync::Mutex::new(Some(response)));
                        let response_cb = response.clone();
                        let result_id = tab_id;
                        // Wait for the page to *actually* render — see readiness_js.rs.
                        // Resolves to "ready" | "live" | "partial"; we surface the reason
                        // via tracing so debugging flaky sites is easier without changing
                        // the Navigate response shape.
                        async_eval::eval_async_expr(wv, readiness_js::READINESS_JS, move |res| {
                            if let Some(tx) = response_cb.lock().unwrap().take() {
                                let reason = match res {
                                    Ok(val) => serde_json::from_str::<String>(&val)
                                        .unwrap_or_else(|_| val.trim().trim_matches('"').to_string()),
                                    // Probe failed (page tore down its JS context) —
                                    // resolve anyway rather than hang the navigate.
                                    Err(e) => format!("probe-error: {e}"),
                                };
                                tracing::debug!(tab_id = result_id, %reason, "MCP nav readiness");
                                let _ = tx.send(Ok(result_id));
                            }
                        });
                    } else {
                        let _ = response.send(Ok(tab_id));
                    }
                }
            }

            Event::UserEvent(AppEvent::NavigationError(tab_id, url, error)) => {
                // External scheme that slipped through → try opening with macOS
                if url::is_external_scheme(&url) {
                    macos::open_external_url(&url);
                    browser_win.set_focus();
                    return;
                }
                // Fail pending MCP navigate
                if let Some((_, response)) = mcp_nav_pending.remove(&tab_id) {
                    let _ = response.send(Err(format!("Navigation error: {error}")));
                }
                // Hide progress bar immediately on error (only if active tab)
                if tab_id == active_wv_id && progress_visible {
                    let _ = progress_wv.set_visible(false);
                    progress_visible = false;
                    progress_hide_at = None;
                }
                // Load error page directly into the failing browser WebView
                if let Some(wv) = tab_webviews.get(&tab_id) {
                    let error_html = error_page_html::html(&url, &error);
                    let _ = wv.load_html(&error_html);
                }
                // Abandon any deferred snapshot restore — error page replaces it
                deferred_nav.remove(&tab_id);
                restoring_tabs.remove(&tab_id);
                // Complete deferred swap so the error page is visible
                if let Some((old_id, new_id)) = pending_swap {
                    if tab_id == new_id {
                        if let Some(wv) = tab_webviews.get(&new_id) {
                            let _ = wv.set_visible(true);
                        }
                        if old_id != new_id {
                            if let Some(wv) = tab_webviews.get(&old_id) {
                                let _ = wv.set_visible(false);
                            }
                        }
                        pending_swap = None;
                    }
                }
            }

            // ── WebContent process terminated (OS killed XPC process) ────────
            Event::UserEvent(AppEvent::WebContentTerminated(tab_id)) => {
                // Fail pending MCP navigate
                if let Some((_, response)) = mcp_nav_pending.remove(&tab_id) {
                    let _ = response.send(Err("WebContent process crashed".into()));
                }
                // Prefer the deferred URL (mid-snapshot-restore) over TabManager
                // since the latter may transiently hold "about:blank".
                let url = deferred_nav.remove(&tab_id)
                    .or_else(|| {
                        tabs.lock().unwrap().tabs().iter()
                            .find(|t| t.id == tab_id)
                            .map(|t| t.url.clone())
                    })
                    .unwrap_or_default();
                restoring_tabs.remove(&tab_id);
                tracing::warn!(tab_id, %url, "WebContent process terminated — reloading");
                // Log to crash.log for post-mortem analysis.
                let (pid, rss_mb) = tab_webviews.get(&tab_id)
                    .map(|wv| {
                        let wv_ptr = objc2::rc::Retained::as_ptr(&wv.webview()) as usize;
                        let pid = tab_stats::webview_pid(wv_ptr);
                        let rss_mb = pid
                            .and_then(tab_stats::sample_pid)
                            .map(|(rss, _)| rss / (1024 * 1024))
                            .unwrap_or(0);
                        (pid, rss_mb)
                    })
                    .unwrap_or((None, 0));
                crash_report::log_webcontent_terminated(tab_id, &url, pid, rss_mb);
                if let Some(wv) = tab_webviews.get(&tab_id) {
                    if url.is_empty() || url == "about:blank" {
                        let _ = wv.load_url("about:blank");
                    } else {
                        let _ = wv.load_url(&url);
                    }
                }
            }

            // ── Page scroll (⌃D / ⌃U / ⌃T / ⌃B) ─────────────────────────────
            Event::UserEvent(AppEvent::ScrollDown) => {
                if let Some(wv) = tab_webviews.get(&active_wv_id) {
                    let _ = wv.evaluate_script("window.scrollBy({top:window.innerHeight,behavior:'smooth'})");
                }
            }
            Event::UserEvent(AppEvent::ScrollUp) => {
                if let Some(wv) = tab_webviews.get(&active_wv_id) {
                    let _ = wv.evaluate_script("window.scrollBy({top:-window.innerHeight,behavior:'smooth'})");
                }
            }
            Event::UserEvent(AppEvent::ScrollTop) => {
                if let Some(wv) = tab_webviews.get(&active_wv_id) {
                    let _ = wv.evaluate_script("window.scrollTo({top:0,behavior:'smooth'})");
                }
            }
            Event::UserEvent(AppEvent::ScrollBottom) => {
                if let Some(wv) = tab_webviews.get(&active_wv_id) {
                    let _ = wv.evaluate_script("window.scrollTo({top:document.body.scrollHeight,behavior:'smooth'})");
                }
            }

            // ── Zoom in / out (⌘= / ⌘-) ────────────────────────────────────
            Event::UserEvent(AppEvent::ZoomIn) => {
                zoom_level = (zoom_level + 0.1).min(3.0);
                for wv in tab_webviews.values() {
                    let _ = wv.zoom(zoom_level);
                }
            }
            Event::UserEvent(AppEvent::ZoomOut) => {
                zoom_level = (zoom_level - 0.1).max(0.5);
                for wv in tab_webviews.values() {
                    let _ = wv.zoom(zoom_level);
                }
            }
            Event::UserEvent(AppEvent::ZoomReset) => {
                zoom_level = 1.0;
                for wv in tab_webviews.values() {
                    let _ = wv.zoom(zoom_level);
                }
            }

            // ── Inline AI edit (⌘⇧E) ──────────────────────────────────────
            Event::UserEvent(AppEvent::InlineEditRequest) => {
                if inline_edit_visible {
                    // Already open — close it (toggle behavior)
                    let _ = inline_edit_wv.set_visible(false);
                    let _ = inline_edit_wv.evaluate_script("window.__clear && window.__clear()");
                    inline_edit_visible = false;
                    inline_edit_pending = false;
                    inline_edit_hotkey_visible.store(false, Ordering::Relaxed);
                    if let Some(ref h) = inline_edit_acp { h.cancel(); }
                    inline_edit_acp = None;
                    inline_edit_response.clear();
                    browser_win.set_focus();
                } else if inline_edit_pending {
                    // A previous ⌘⇧E is still in-flight (capture round-trip to the
                    // tab's WebView hasn't completed). Ignore the duplicate so the
                    // soon-to-arrive `InlineEditReady` doesn't get toggled-closed
                    // by what the user perceives as a single keypress.
                    tracing::debug!("InlineEditRequest ignored: capture already pending");
                } else if let Some(wv) = tab_webviews.get(&active_wv_id) {
                    inline_edit_pending = true;
                    // Lock the keyboard guard now, not after the async capture
                    // returns — this prevents any racing Ctrl+P/N/etc. shortcuts
                    // from being interpreted as tab navigation between the
                    // request and the modal becoming visible.
                    inline_edit_hotkey_visible.store(true, Ordering::Relaxed);
                    let _ = wv.evaluate_script("window.__inlineEditCapture && window.__inlineEditCapture()");
                }
            }
            Event::UserEvent(AppEvent::InlineEditReady(text, pos)) => {
                // Drop late/duplicate ready events (e.g. second capture that
                // sneaked through before the pending flag was set, or a stale
                // ready from before the modal was closed).
                if inline_edit_visible || !inline_edit_pending {
                    tracing::debug!(
                        visible = inline_edit_visible,
                        pending = inline_edit_pending,
                        "InlineEditReady dropped (already visible or no pending request)"
                    );
                    return;
                }
                inline_edit_pending = false;
                inline_edit_selected_text = text;
                inline_edit_tab_id = active_wv_id;
                let scale = browser_win.scale_factor();
                let sz = browser_win.inner_size();
                // Position from JS anchor when available, otherwise default to
                // top-center under the address bar (e.g. user pressed ⌘⇧E on a
                // freshly loaded tab with no focused input and no selection —
                // common right after ⌘⇧P navigation, which is exactly when the
                // modal previously appeared way down at <body>'s bottom edge).
                let (mut px, mut py) = match pos {
                    Some((x, y)) => (
                        (x * scale) as u32,
                        (y * scale) as u32 + address_bar_h,
                    ),
                    None => (
                        sz.width.saturating_sub(inline_edit_w) / 2,
                        address_bar_h + 8,
                    ),
                };
                // Clamp so modal stays within window bounds
                if px + inline_edit_w > sz.width {
                    px = sz.width.saturating_sub(inline_edit_w + 8);
                }
                if py + inline_edit_h > sz.height {
                    py = sz.height.saturating_sub(inline_edit_h + 8);
                }
                let _ = inline_edit_wv.set_bounds(wry::Rect {
                    position: tao::dpi::PhysicalPosition::new(px, py).into(),
                    size: tao::dpi::PhysicalSize::new(inline_edit_w, inline_edit_h).into(),
                });
                let _ = inline_edit_wv.set_visible(true);
                inline_edit_visible = true;
                inline_edit_hotkey_visible.store(true, Ordering::Relaxed);
                // Push history FIRST so ghost autocomplete is armed by the time
                // the user starts typing — `__focus()` resets internal state
                // (which `setHistory` then re-seeds), and `evaluate_script` is
                // FIFO within a single WKWebView, so the order matters.
                let hist_json = serde_json::to_string(&prompt_history).unwrap_or_else(|_| "[]".into());
                let _ = inline_edit_wv.evaluate_script(&format!(
                    "window.__setHistory && window.__setHistory({hist_json})"
                ));
                unsafe {
                    use objc2::msg_send;
                    use objc2::runtime::AnyObject;
                    let ns_win: *mut AnyObject = chrome_win.ns_window() as *mut AnyObject;
                    let _: () = msg_send![ns_win, makeKeyWindow];
                }
                let _ = inline_edit_wv.focus();
                // Focus-retry loop: WKWebView focus can be stolen by a tab that
                // is still loading/auto-focusing (very common right after a
                // ⌘⇧P-driven NavigateTo — the new page hasn't fully committed
                // yet, then steals first-responder back from us). Mirrors the
                // pattern used by the sidebar's prompt-input focus handshake.
                let _ = inline_edit_wv.evaluate_script(
                    "(function(){var el=document.getElementById('input');if(!el)return;\
                     window.__focus && window.__focus();\
                     var n=0;(function f(){if(n++>20)return;el.focus();\
                     if(document.activeElement===el)return;setTimeout(f,30)})()})()"
                );
            }
            Event::UserEvent(AppEvent::InlineEditSubmit(prompt)) => {
                // Record in prompt history (MRU dedup)
                if let Some(pos) = prompt_history.iter().position(|p| p == &prompt) {
                    prompt_history.remove(pos);
                }
                prompt_history.insert(0, prompt.clone());
                prompt_history.truncate(cfg.max_prompt_history);
                {
                    let snapshot = prompt_history.clone();
                    std::thread::spawn(move || config::save_prompt_history(&snapshot));
                }
                let _ = inline_edit_wv.evaluate_script("window.__setProcessing && window.__setProcessing(true)");
                let formatted = if inline_edit_selected_text.is_empty() {
                    prompt
                } else {
                    format!("{}\n<text>{}</text>", prompt, inline_edit_selected_text)
                };
                inline_edit_response.clear();
                inline_edit_acp = acp::AcpHandle::connect(
                    "octomind acp octoweb:editor",
                    { let p = acp_proxy.clone(); move || { let _ = p.send_event(AppEvent::AcpWake); } }
                ).ok();
                if let Some(ref h) = inline_edit_acp {
                    h.send_prompt(formatted, vec![]);
                }
                // Auto-hide modal if configured
                if cfg.ai_edit_auto_hide {
                    let _ = inline_edit_wv.set_visible(false);
                    inline_edit_visible = false;
                    inline_edit_pending = false;
                    inline_edit_hotkey_visible.store(false, Ordering::Relaxed);
                    if let Some(wv) = tab_webviews.get(&inline_edit_tab_id) {
                        let _ = wv.evaluate_script(
                            "document.documentElement.style.cursor='wait'"
                        );
                    }
                    browser_win.set_focus();
                }
            }
            Event::UserEvent(AppEvent::InlineEditHide) if inline_edit_visible => {
                let _ = inline_edit_wv.set_visible(false);
                inline_edit_visible = false;
                inline_edit_pending = false;
                inline_edit_hotkey_visible.store(false, Ordering::Relaxed);
                // Set loading cursor on the target tab while processing continues
                if let Some(wv) = tab_webviews.get(&inline_edit_tab_id) {
                    let _ = wv.evaluate_script(
                        "document.documentElement.style.cursor='wait'"
                    );
                }
                browser_win.set_focus();
            }
            Event::UserEvent(AppEvent::InlineEditResize(h))
                if inline_edit_visible =>
            {
                let scale = browser_win.scale_factor();
                let new_h = (h * scale) as u32;
                let bounds = inline_edit_wv.bounds().unwrap_or(wry::Rect {
                    position: tao::dpi::PhysicalPosition::new(0u32, 0u32).into(),
                    size: tao::dpi::PhysicalSize::new(inline_edit_w, inline_edit_h).into(),
                });
                let _ = inline_edit_wv.set_bounds(wry::Rect {
                    position: bounds.position,
                    size: tao::dpi::PhysicalSize::new(inline_edit_w, new_h).into(),
                });
            }
            Event::UserEvent(AppEvent::InlineEditClose) if inline_edit_visible || inline_edit_pending => {
                let _ = inline_edit_wv.set_visible(false);
                let _ = inline_edit_wv.evaluate_script("window.__clear && window.__clear()");
                inline_edit_visible = false;
                inline_edit_pending = false;
                inline_edit_hotkey_visible.store(false, Ordering::Relaxed);
                if let Some(ref h) = inline_edit_acp { h.cancel(); }
                inline_edit_acp = None;
                inline_edit_response.clear();
                browser_win.set_focus();
            }

            // ── Reload current page ───────────────────────────────────────────
            Event::UserEvent(AppEvent::Reload) => {
                if let Some(wv) = tab_webviews.get(&active_wv_id) {
                    // If mid-snapshot-restore, skip the snapshot and load the real URL directly.
                    if let Some(url) = deferred_nav.remove(&active_wv_id) {
                        restoring_tabs.remove(&active_wv_id);
                        let _ = wv.load_url(&url);
                    } else {
                        let _ = wv.reload();
                    }
                }
            }

            // ── Screenshot (⌘S) — viewport → clipboard ─────────────────────
            Event::UserEvent(AppEvent::Screenshot) => {
                if let Some(wv) = tab_webviews.get(&active_wv_id) {
                    let wv_ptr = objc2::rc::Retained::as_ptr(&wv.webview()) as usize;
                    let ns_win_ptr = browser_win.ns_window() as usize;
                    screenshot_to_clipboard(wv_ptr, ns_win_ptr);
                }
            }

            // ── Full page screenshot (⌘⇧S) — full page → clipboard ──────────
            Event::UserEvent(AppEvent::ScreenshotFullPage) => {
                if let Some(wv) = tab_webviews.get(&active_wv_id) {
                    let wv_ptr = objc2::rc::Retained::as_ptr(&wv.webview()) as usize;
                    let ns_win_ptr = browser_win.ns_window() as usize;
                    screenshot_full_page_to_clipboard(wv_ptr, ns_win_ptr);
                }
            }

            // ── Frozen tab snapshot captured (async callback) ──────────────────
            Event::UserEvent(AppEvent::SnapshotCaptured(tab_id, data_uri))
                // Discard suspiciously small snapshots (likely blank/hidden WebView).
                // A real page snapshot is typically >5 KB as base64 PNG.
                if data_uri.len() > 5_000 =>
            {
                tab_snapshots.insert(tab_id, data_uri);
            }

            // ── Favicon cache loaded from disk (background thread) ──────────────
            Event::UserEvent(AppEvent::FaviconCacheLoaded(cache)) => {
                for (domain, data) in cache {
                    favicon_cache.entry(domain.clone()).or_insert_with(|| {
                        favicon_order.push_back(domain);
                        data
                    });
                }
                while favicon_order.len() > FAVICON_CAP {
                    if let Some(oldest) = favicon_order.pop_front() {
                        favicon_cache.remove(&oldest);
                    }
                }
                tracing::debug!(count = favicon_cache.len(), "favicon cache loaded from disk");
            }

            // ── Media playing state changed ───────────────────────────────────
            Event::UserEvent(AppEvent::MediaPlaying(tab_id, is_playing)) => {
                tabs.lock().unwrap().set_playing_audio(tab_id, is_playing);
                if is_playing {
                    media_playing_tabs.insert(tab_id);
                } else {
                    media_playing_tabs.remove(&tab_id);
                }
            }

            // ── Page info (size + load time) from injected script ─────────────
            Event::UserEvent(AppEvent::PageInfo(tab_id, bytes, ms)) => {
                tabs.lock().unwrap().set_page_info(tab_id, bytes, ms);
                if tab_id == active_wv_id {
                    let _ = address_bar_wv.evaluate_script(&format!(
                        "window.__stats && window.__stats({bytes}, {ms})"
                    ));
                }
            }

            // ── OS opened URLs (default browser / open command) ─────────
            Event::Opened { urls } => {
                for url in urls {
                    let raw = url.to_string();
                    tracing::debug!(url = %raw, "OS opened URL");
                    let _ = proxy.send_event(AppEvent::NavigateTo(raw));
                }
                // Bring window to front when opened externally
                browser_win.set_focus();
            }

            // ── Window events ─────────────────────────────────────────────
            Event::WindowEvent {
                window_id,
                event: ref win_event,
                ..
            } => match win_event {
                WindowEvent::CloseRequested => {
                    if window_id == browser_win_id {
                        crash_report::log_exit_trigger("CloseRequested");
                        save_and_exit(
                            &tabs,
                            &favicon_cache,
                            &prompt_history,
                            &ai_prompt_history,
                            &sessions,
                            active_session_id,
                            cfg.max_acp_session_messages,
                            control_flow,
                        );
                    } else if window_id == chrome_win_id {
                        // Chrome window should not be closed independently — ignore.
                    } else {
                        // Close any overlay window (command palette or shortcuts)
                        overlay_win.set_visible(false);
                        overlay_visible = false;
                        overlay_hotkey_visible.store(false, Ordering::Relaxed);
                        shortcuts_win.set_visible(false);
                        shortcuts_visible = false;
                        settings_win.set_visible(false);
                        settings_visible = false;
                    }
                }

                WindowEvent::Resized(sz) if window_id == browser_win_id => {
                    // Keep chrome overlay window in sync — both size AND position.
                    // During native-fullscreen transitions browser_win moves to a
                    // new screen origin, and the addChildWindow: relationship
                    // doesn't always follow cleanly for borderless transparent
                    // children, so we re-anchor explicitly.
                    chrome_win.set_inner_size(*sz);
                    if let Ok(pos) = browser_win.outer_position() {
                        chrome_win.set_outer_position(pos);
                    }
                    if overlay_visible {
                        overlay_win.set_inner_size(*sz);
                    }
                    if shortcuts_visible {
                        shortcuts_win.set_inner_size(*sz);
                    }
                    if settings_visible {
                        settings_win.set_inner_size(*sz);
                    }
                    // Resize sidebar to track window height and stay at right edge
                    // (or full-width when sidebar_fullscreen is set).
                    if sidebar_visible {
                        let w = if sidebar_fullscreen { sz.width } else { sidebar_w };
                        let x = if sidebar_fullscreen { 0u32 } else { sz.width.saturating_sub(sidebar_w) };
                        let _ = sidebar_wv.set_bounds(wry::Rect {
                            position: tao::dpi::PhysicalPosition::new(x, 0u32).into(),
                            size: tao::dpi::PhysicalSize::new(w, sz.height).into(),
                        });
                    }
                    // Resize progress bar width (sits at bottom edge of address bar)
                    let _ = progress_wv.set_bounds(wry::Rect {
                        position: tao::dpi::PhysicalPosition::new(0u32, address_bar_h).into(),
                        size: tao::dpi::PhysicalSize::new(sz.width, 3u32).into(),
                    });
                    // Reposition find bar at top-right
                    if find_bar_visible {
                        let _ = find_bar_wv.set_bounds(wry::Rect {
                            position: tao::dpi::PhysicalPosition::new(
                                sz.width.saturating_sub(find_bar_w + 8),
                                address_bar_h,
                            ).into(),
                            size: tao::dpi::PhysicalSize::new(find_bar_w, find_bar_h).into(),
                        });
                    }
                    // Resize address bar to full width. While the user is in URL
                    // edit mode the bar is grown by ~340 px so the autocomplete
                    // dropdown stays visible below the 32 px titlebar strip.
                    let bar_h = if address_bar_editing {
                        let scale = browser_win.scale_factor();
                        address_bar_h + (340.0 * scale) as u32
                    } else {
                        address_bar_h
                    };
                    let _ = address_bar_wv.set_bounds(wry::Rect {
                        position: tao::dpi::PhysicalPosition::new(0u32, 0u32).into(),
                        size: tao::dpi::PhysicalSize::new(sz.width, bar_h).into(),
                    });
                    // Reposition notification toast at top-right
                    let _ = notification_wv.set_bounds(wry::Rect {
                        position: tao::dpi::PhysicalPosition::new(
                            sz.width.saturating_sub(notif_w + notif_margin),
                            0u32,
                        ).into(),
                        size: tao::dpi::PhysicalSize::new(notif_w, notif_h).into(),
                    });
                    // Resize active tab to full width (offset below address bar)
                    let bounds = wry::Rect {
                        position: tao::dpi::PhysicalPosition::new(0u32, address_bar_h).into(),
                        size: tao::dpi::PhysicalSize::new(sz.width, sz.height.saturating_sub(address_bar_h + footer_h)).into(),
                    };
                    if let Some(wv) = tab_webviews.get(&active_wv_id) {
                        let _ = wv.set_bounds(bounds);
                    }
                    // Resize footer bar to full width (pinned to bottom)
                    let _ = footer_wv.set_bounds(wry::Rect {
                        position: tao::dpi::PhysicalPosition::new(
                            0u32,
                            sz.height.saturating_sub(footer_h),
                        ).into(),
                        size: tao::dpi::PhysicalSize::new(sz.width, footer_h).into(),
                    });
                }

                // Browser window moved (drag, fullscreen enter/exit, screen
                // change). Re-anchor the chrome overlay so its sidebar/footer
                // stay glued to the parent window's content area.
                WindowEvent::Moved(pos) if window_id == browser_win_id => {
                    chrome_win.set_outer_position(*pos);
                }

                WindowEvent::ModifiersChanged(mods) => {
                    modifiers = *mods;
                }


                WindowEvent::KeyboardInput {
                    event: key_event, ..
                } => {
                    use tao::event::ElementState;
                    if key_event.state != ElementState::Pressed {
                        return;
                    }
                    let cmd = modifiers.super_key();
                    if let Some(wv) = tab_webviews.get(&active_wv_id) {
                        match key_event.physical_key {
                            KeyCode::BracketLeft if cmd => {
                                let _ = wv.evaluate_script("history.back()");
                            }
                            KeyCode::BracketRight if cmd => {
                                let _ = wv.evaluate_script("history.forward()");
                            }
                            _ => {}
                        }
                    }
                }

                _ => {}
            },

            _ => {}
        }
    });
}

// ── Frozen tab snapshot ───────────────────────────────────────────────────
/// Capture a viewport snapshot of the given WKWebView and send it back as a
/// base64 PNG data-URI via `AppEvent::SnapshotCaptured`. Used to show instant
/// frozen content when switching to a hibernated tab.
fn capture_tab_snapshot(
    wv_ptr: usize,
    tab_id: usize,
    proxy: tao::event_loop::EventLoopProxy<AppEvent>,
) {
    let handler = block2::RcBlock::new(
        move |image: *mut objc2::runtime::AnyObject, _error: *mut objc2::runtime::AnyObject| {
            if image.is_null() {
                return;
            }
            unsafe {
                let png_data = nsimage_to_png_data(image);
                if png_data.is_null() {
                    return;
                }
                // Use NSData's built-in base64 encoder (no extra crate needed).
                let b64_nsstr: *mut objc2::runtime::AnyObject =
                    objc2::msg_send![&*png_data, base64EncodedStringWithOptions: 0u64];
                if b64_nsstr.is_null() {
                    return;
                }
                let utf8: *const u8 = objc2::msg_send![&*b64_nsstr, UTF8String];
                let cstr = std::ffi::CStr::from_ptr(utf8 as *const std::ffi::c_char);
                let b64 = cstr.to_string_lossy();
                let data_uri = format!("data:image/png;base64,{b64}");
                let _ = proxy.send_event(AppEvent::SnapshotCaptured(tab_id, data_uri));
            }
        },
    );

    unsafe {
        let wv_obj: *mut objc2::runtime::AnyObject = wv_ptr as *mut objc2::runtime::AnyObject;
        let nil: *const objc2::runtime::AnyObject = std::ptr::null();
        let _: () = objc2::msg_send![
            &*wv_obj,
            takeSnapshotWithConfiguration: nil,
            completionHandler: &*handler
        ];
    }
}

// ── Screenshot helpers ────────────────────────────────────────────────────
/// Convert an NSImage to PNG NSData. Returns null on failure.
unsafe fn nsimage_to_png_data(
    image: *mut objc2::runtime::AnyObject,
) -> *mut objc2::runtime::AnyObject {
    // Get CGImage directly — avoids TIFF serialization round-trip
    use objc2_core_foundation::CGRect;
    let size: objc2_core_foundation::CGSize = objc2::msg_send![&*image, size];
    let proposed = CGRect {
        origin: objc2_core_foundation::CGPoint { x: 0.0, y: 0.0 },
        size,
    };
    let mut rect = proposed;
    let cg_image: *mut objc2::runtime::AnyObject = objc2::msg_send![&*image, CGImageForProposedRect: &mut rect, context: std::ptr::null::<objc2::runtime::AnyObject>(), hints: std::ptr::null::<objc2::runtime::AnyObject>()];
    if cg_image.is_null() {
        // Fallback: TIFF path for images without CGImage backing
        let tiff: *mut objc2::runtime::AnyObject = objc2::msg_send![&*image, TIFFRepresentation];
        if tiff.is_null() {
            return std::ptr::null_mut();
        }
        let rep: *mut objc2::runtime::AnyObject = objc2::msg_send![
            objc2::class!(NSBitmapImageRep),
            imageRepWithData: &*tiff
        ];
        if rep.is_null() {
            return std::ptr::null_mut();
        }
        let empty_dict: *mut objc2::runtime::AnyObject =
            objc2::msg_send![objc2::class!(NSDictionary), dictionary];
        return objc2::msg_send![
            &*rep,
            representationUsingType: 4u64,
            properties: &*empty_dict
        ];
    }
    let rep: *mut objc2::runtime::AnyObject =
        objc2::msg_send![objc2::class!(NSBitmapImageRep), alloc];
    let rep: *mut objc2::runtime::AnyObject = objc2::msg_send![
        rep, initWithCGImage: cg_image
    ];
    // Own the rep: it retains the window-sized CGImage (IOSurface-backed), so
    // leaking it pins tens of MB per capture.
    let Some(rep) = objc2::rc::Retained::from_raw(rep) else {
        return std::ptr::null_mut();
    };
    let empty_dict: *mut objc2::runtime::AnyObject =
        objc2::msg_send![objc2::class!(NSDictionary), dictionary];
    // NSBitmapImageFileTypePNG = 4
    let png_data: *mut objc2::runtime::AnyObject = objc2::msg_send![
        &*rep,
        representationUsingType: 4u64,
        properties: &*empty_dict
    ];
    png_data
}

/// Copy PNG NSData to the system clipboard.
unsafe fn copy_png_to_clipboard(png_data: *mut objc2::runtime::AnyObject) {
    if png_data.is_null() {
        return;
    }
    let pb: *mut objc2::runtime::AnyObject =
        objc2::msg_send![objc2::class!(NSPasteboard), generalPasteboard];
    let _: () = objc2::msg_send![&*pb, clearContents];
    let png_type: *mut objc2::runtime::AnyObject = objc2::msg_send![
        objc2::class!(NSString),
        stringWithUTF8String: c"public.png".as_ptr()
    ];
    let _: bool = objc2::msg_send![&*pb, setData: &*png_data, forType: &*png_type];
}

/// Viewport screenshot: takeSnapshot → PNG → clipboard.
/// `ns_win_ptr` is the NSWindow pointer to restore key-window focus after the
/// async snapshot completes — takeSnapshot can temporarily steal key-window
/// status — without this, takeSnapshot's ephemeral panel can leave our window
fn screenshot_to_clipboard(wv_ptr: usize, ns_win_ptr: usize) {
    let handler = block2::RcBlock::new(
        move |image: *mut objc2::runtime::AnyObject, _error: *mut objc2::runtime::AnyObject| {
            if image.is_null() {
                tracing::error!("Viewport screenshot failed: nil image");
            } else {
                unsafe {
                    let png_data = nsimage_to_png_data(image);
                    if png_data.is_null() {
                        tracing::error!("Viewport screenshot: failed to encode PNG");
                    } else {
                        copy_png_to_clipboard(png_data);
                        tracing::debug!("Screenshot copied to clipboard");
                    }
                }
            }
            // Restore key-window focus unconditionally — takeSnapshot can cause the
            // window to lose key status, which permanently breaks keybindings.
            unsafe {
                let ns_win: *mut objc2::runtime::AnyObject =
                    ns_win_ptr as *mut objc2::runtime::AnyObject;
                let _: () = objc2::msg_send![ns_win, makeKeyWindow];
            }
        },
    );

    unsafe {
        let wv_obj: *mut objc2::runtime::AnyObject = wv_ptr as *mut objc2::runtime::AnyObject;
        let nil: *const objc2::runtime::AnyObject = std::ptr::null();
        let _: () = objc2::msg_send![
            &*wv_obj,
            takeSnapshotWithConfiguration: nil,
            completionHandler: &*handler
        ];
    }
}

/// Render PDF NSData to a CGImage via CGBitmapContext + drawWithBox.
/// Uses 1x scale and caps height at 16384px for speed.
/// Returns None on failure. Caller decides encoding (clipboard vs PNG).
unsafe fn pdf_data_to_cgimage(
    pdf_data: *mut objc2::runtime::AnyObject,
) -> Option<core_graphics::image::CGImage> {
    use core_graphics::color_space::CGColorSpace;
    use core_graphics::context::CGContext;
    use objc2_core_foundation::CGRect;

    let t0 = std::time::Instant::now();

    let pdf_doc: *mut objc2::runtime::AnyObject =
        objc2::msg_send![objc2::class!(PDFDocument), alloc];
    let pdf_doc: *mut objc2::runtime::AnyObject = objc2::msg_send![
        pdf_doc,
        initWithData: &*pdf_data
    ];
    // Own the document — it retains the whole PDF NSData.
    let pdf_doc = objc2::rc::Retained::from_raw(pdf_doc)?;

    let page_count: usize = objc2::msg_send![&*pdf_doc, pageCount];
    if page_count == 0 {
        return None;
    }

    // 1x scale — Retina 2x quadruples pixel count for no clipboard benefit
    let scale: f64 = 1.0;
    const MAX_HEIGHT_PX: f64 = 16384.0;

    // Measure pages, cap at MAX_HEIGHT_PX
    let mut total_height: f64 = 0.0;
    let mut max_width: f64 = 0.0;
    let mut render_count = page_count;
    for i in 0..page_count {
        let page: *mut objc2::runtime::AnyObject = objc2::msg_send![&*pdf_doc, pageAtIndex: i];
        let bounds: CGRect = objc2::msg_send![&*page, boundsForBox: 0isize];
        let page_h = bounds.size.height * scale;
        if total_height + page_h > MAX_HEIGHT_PX && i > 0 {
            render_count = i;
            tracing::debug!(
                pages = page_count,
                rendered = render_count,
                "Full page screenshot truncated at height cap"
            );
            break;
        }
        total_height += page_h;
        if bounds.size.width > max_width {
            max_width = bounds.size.width;
        }
    }

    let px_w = (max_width * scale).ceil() as usize;
    let px_h = total_height.ceil() as usize;
    if px_w == 0 || px_h == 0 {
        return None;
    }

    // Create a single CGBitmapContext — one allocation for the entire image
    let color_space = CGColorSpace::create_device_rgb();
    let ctx = CGContext::create_bitmap_context(
        None,
        px_w,
        px_h,
        8,
        px_w * 4,
        &color_space,
        // kCGImageAlphaPremultipliedLast (RGBA)
        core_graphics::base::kCGImageAlphaPremultipliedLast,
    );

    // Fill white background
    ctx.set_rgb_fill_color(1.0, 1.0, 1.0, 1.0);
    ctx.fill_rect(core_graphics::geometry::CGRect::new(
        &core_graphics::geometry::CGPoint::new(0.0, 0.0),
        &core_graphics::geometry::CGSize::new(px_w as f64, px_h as f64),
    ));

    // Render each page directly into the bitmap context (bottom-up in CG coords)
    let mut y_offset: f64 = 0.0;
    for i in (0..render_count).rev() {
        let page: *mut objc2::runtime::AnyObject = objc2::msg_send![&*pdf_doc, pageAtIndex: i];
        let bounds: CGRect = objc2::msg_send![&*page, boundsForBox: 0isize];
        let page_px_h = bounds.size.height * scale;

        ctx.save();
        ctx.translate(0.0, y_offset);
        ctx.scale(scale, scale);

        // drawWithBox:toContext: rasterizes the PDF page (including embedded images)
        use foreign_types::ForeignType;
        let ctx_ptr = ctx.as_ptr() as *const std::ffi::c_void;
        let _: () = objc2::msg_send![
            &*page,
            drawWithBox: 0isize,
            toContext: ctx_ptr
        ];

        ctx.restore();
        y_offset += page_px_h;
    }

    let render_ms = t0.elapsed().as_millis();
    tracing::debug!(
        px_w,
        px_h,
        pages = render_count,
        render_ms,
        "PDF rendered to CGImage"
    );

    ctx.create_image()
}

/// Encode a CGImage to PNG NSData. Used by MCP path (needs base64).
unsafe fn cgimage_to_png_data(
    cg_image: &core_graphics::image::CGImage,
) -> *mut objc2::runtime::AnyObject {
    use foreign_types::ForeignType;
    let cg_image_ptr = cg_image.as_ptr() as *const objc2::runtime::AnyObject;
    let rep: *mut objc2::runtime::AnyObject =
        objc2::msg_send![objc2::class!(NSBitmapImageRep), alloc];
    let rep: *mut objc2::runtime::AnyObject = objc2::msg_send![
        rep, initWithCGImage: cg_image_ptr
    ];
    // Own the rep — it retains the full-page bitmap.
    let Some(rep) = objc2::rc::Retained::from_raw(rep) else {
        return std::ptr::null_mut();
    };
    let empty_dict: *mut objc2::runtime::AnyObject =
        objc2::msg_send![objc2::class!(NSDictionary), dictionary];
    // NSBitmapImageFileTypePNG = 4
    objc2::msg_send![
        &*rep,
        representationUsingType: 4u64,
        properties: &*empty_dict
    ]
}

/// Copy a CGImage directly to clipboard as NSImage — skips PNG encoding entirely.
unsafe fn copy_cgimage_to_clipboard(cg_image: &core_graphics::image::CGImage) {
    use foreign_types::ForeignType;
    let cg_ptr = cg_image.as_ptr() as *const std::ffi::c_void;
    let w = cg_image.width();
    let h = cg_image.height();
    let size = objc2_core_foundation::CGSize {
        width: w as f64,
        height: h as f64,
    };

    let ns_image: *mut objc2::runtime::AnyObject = objc2::msg_send![objc2::class!(NSImage), alloc];
    let ns_image: *mut objc2::runtime::AnyObject =
        objc2::msg_send![ns_image, initWithCGImage: cg_ptr, size: size];
    // Own the image — it retains the full-page bitmap.
    let Some(ns_image) = objc2::rc::Retained::from_raw(ns_image) else {
        tracing::error!("Failed to create NSImage from CGImage");
        return;
    };

    let pb: *mut objc2::runtime::AnyObject =
        objc2::msg_send![objc2::class!(NSPasteboard), generalPasteboard];
    let _: () = objc2::msg_send![&*pb, clearContents];
    // NSImage conforms to NSPasteboardWriting — writeObjects handles all UTIs
    let array: *mut objc2::runtime::AnyObject = objc2::msg_send![
        objc2::class!(NSArray),
        arrayWithObject: &*ns_image
    ];
    let _: bool = objc2::msg_send![&*pb, writeObjects: &*array];
}

/// Full page screenshot: createPDF → stitch → PNG → clipboard.
/// `ns_win_ptr` is the NSWindow pointer to restore key-window focus after the
/// async createPDF completes — same issue as viewport screenshot.
fn screenshot_full_page_to_clipboard(wv_ptr: usize, ns_win_ptr: usize) {
    let handler = block2::RcBlock::new(
        move |pdf_data: *mut objc2::runtime::AnyObject, _error: *mut objc2::runtime::AnyObject| {
            if pdf_data.is_null() {
                tracing::error!("Full page screenshot failed: createPDF returned nil");
            } else {
                unsafe {
                    // Skip PNG encoding — write CGImage as NSImage directly to clipboard
                    if let Some(cg_image) = pdf_data_to_cgimage(pdf_data) {
                        copy_cgimage_to_clipboard(&cg_image);
                        tracing::debug!("Full page screenshot copied to clipboard");
                    } else {
                        tracing::error!("Full page screenshot: failed to render PDF");
                    }
                }
            }
            // Restore key-window focus unconditionally — createPDF can cause the
            // window to lose key status, which permanently breaks keybindings.
            unsafe {
                let ns_win: *mut objc2::runtime::AnyObject =
                    ns_win_ptr as *mut objc2::runtime::AnyObject;
                let _: () = objc2::msg_send![ns_win, makeKeyWindow];
            }
        },
    );

    unsafe {
        let wv_obj: *mut objc2::runtime::AnyObject = wv_ptr as *mut objc2::runtime::AnyObject;
        let nil: *const objc2::runtime::AnyObject = std::ptr::null();
        let _: () = objc2::msg_send![
            &*wv_obj,
            createPDFWithConfiguration: nil,
            completionHandler: &*handler
        ];
    }
}

/// Build a token-budgeted prompt for the proactive learning agent.
/// Includes open tabs, recent history, and the active tab's readable text.
fn build_learning_prompt(
    tabs: &browser::TabManager,
    active_wv_id: usize,
    active_tab_text: &str,
) -> Option<String> {
    const BUDGET: usize = 16_000; // ~4K tokens
                                  // Reserve space: ~4K for page content, rest for tabs + history
    const CONTENT_BUDGET: usize = 4_000;

    let history = tabs.history();
    let tab_list = tabs.tabs();

    // Skip if no meaningful data
    if history.is_empty() && tab_list.is_empty() {
        return None;
    }

    // Check if active tab is on a sensitive page (login, payment, etc.)
    // — skip content extraction entirely to prevent leaking credentials.
    let active_url = tab_list
        .iter()
        .find(|t| t.id == active_wv_id)
        .map(|t| t.url.as_str())
        .unwrap_or("");
    let on_sensitive_page = sanitize::is_sensitive_page(active_url);

    let mut out = String::with_capacity(BUDGET);

    // Open tabs (compact: title + sanitized URL per line)
    if !tab_list.is_empty() {
        out.push_str("Open tabs:\n");
        for t in tab_list {
            let marker = if t.id == active_wv_id { " *" } else { "" };
            let safe_url = sanitize::sanitize_url(&t.url);
            let line = format!("- {}{} | {}\n", t.title, marker, safe_url);
            if out.len() + line.len() > BUDGET - CONTENT_BUDGET - 2000 {
                break;
            }
            out.push_str(&line);
        }
        out.push('\n');
    }

    // Recent history (most recent first, deduplicated against open tabs)
    if !history.is_empty() {
        out.push_str("Recent history:\n");
        let tab_urls: std::collections::HashSet<&str> =
            tab_list.iter().map(|t| t.url.as_str()).collect();
        for entry in history.iter().rev() {
            // Skip entries already visible in open tabs
            if tab_urls.contains(entry.url.as_str()) {
                continue;
            }
            let safe_url = sanitize::sanitize_url(&entry.url);
            let line = format!("- {} | {}\n", entry.title, safe_url);
            if out.len() + line.len() > BUDGET - CONTENT_BUDGET - 200 {
                out.push_str("- ...\n");
                break;
            }
            out.push_str(&line);
        }
        out.push('\n');
    }

    // Active tab page text — skip entirely on sensitive pages
    if !active_tab_text.is_empty() && !on_sensitive_page {
        out.push_str("Active page text:\n");
        // Clean: collapse whitespace, strip control chars
        let cleaned: String = active_tab_text
            .chars()
            .filter(|c| !c.is_control() || *c == '\n')
            .collect();
        // Collapse runs of 3+ newlines into 2
        let mut prev_nl = 0u8;
        let mut trimmed = String::with_capacity(CONTENT_BUDGET);
        for ch in cleaned.chars() {
            if ch == '\n' {
                prev_nl += 1;
                if prev_nl <= 2 {
                    trimmed.push(ch);
                }
            } else {
                prev_nl = 0;
                trimmed.push(ch);
            }
            if trimmed.len() >= CONTENT_BUDGET {
                break;
            }
        }
        let sanitized = sanitize::sanitize_text(trimmed.trim());
        out.push_str(&sanitized);
        out.push('\n');
    } else if on_sensitive_page {
        out.push_str("Active page: [sensitive page — content omitted]\n");
    }

    Some(out)
}

/// Append a message to a session's persisted log, stamping it with the
/// current wall-clock time, and FIFO-drop the oldest once over the cap.
/// Capping in memory (not just on disk) is what bounds long-running
/// session growth — without it the in-memory `Vec<AcpMessage>` grows
/// forever even though the on-disk snapshot stays bounded.
///
/// We do NOT rely on the agent subprocess to police context size:
/// `octomind`'s own conversation `Vec<Message>` is unbounded, and it only
/// compresses on explicit `/done` / `/truncate` slash commands — so the
/// truncation responsibility lives here on the client side.
///
/// Caller is expected to dispatch `persist_acp_history` afterwards
/// (fire-and-forget on a worker thread).
fn push_acp_msg(s: &mut AcpSession, role: &str, text: String, max_msgs: usize) {
    let msg = config::AcpMessage {
        role: role.to_string(),
        text,
        ts: epoch_ms(),
        a2ui: None,
        tools: Vec::new(),
        turn_ms: 0,
    };
    push_acp_message(s, msg, max_msgs);
}

fn push_acp_message(s: &mut AcpSession, msg: config::AcpMessage, max_msgs: usize) {
    s.messages.push(msg);
    if s.messages.len() > max_msgs {
        let drop = s.messages.len() - max_msgs;
        s.messages.drain(..drop);
    }
}

fn epoch_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Flush the in-flight agent turn (streamed text + tool runs) into the
/// persisted log as one "agent" message. Returns true if anything was
/// committed. Tool-only turns (no text) are kept — the sidebar renders
/// their steps group as the record of the work.
fn flush_agent_turn(s: &mut AcpSession, max_msgs: usize) -> bool {
    let text = std::mem::take(&mut s.agent_buf);
    let tools = std::mem::take(&mut s.tool_buf);
    s.tool_starts.clear();
    let turn_ms = s
        .turn_started
        .take()
        .map(|t| t.elapsed().as_millis() as u64)
        .unwrap_or(0);
    if text.is_empty() && tools.is_empty() {
        return false;
    }
    let msg = config::AcpMessage {
        role: "agent".to_string(),
        text,
        ts: epoch_ms(),
        a2ui: None,
        tools,
        turn_ms,
    };
    push_acp_message(s, msg, max_msgs);
    true
}

/// Cap a tool's raw input/output JSON for persistence — page dumps and MCP
/// fetches can be megabytes and would bloat every acp_history.json save.
const ACP_TOOL_JSON_CAP: usize = 8 * 1024;

fn cap_tool_json(v: &serde_json::Value) -> serde_json::Value {
    match serde_json::to_string(v) {
        Ok(s) if s.len() > ACP_TOOL_JSON_CAP => {
            let mut t: String = s.chars().take(ACP_TOOL_JSON_CAP).collect();
            t.push_str(" … (truncated)");
            serde_json::Value::String(t)
        }
        _ => v.clone(),
    }
}

/// Upsert an A2UI surface snapshot in the session's message log. Same envelope
/// (matched by `file_id`) updates in place — keeps the chat showing one bubble
/// per surface across pending→resolved transitions instead of stacking dupes.
fn upsert_a2ui_msg(s: &mut AcpSession, file_id: &str, body: serde_json::Value, max_msgs: usize) {
    // Skip vacuous envelopes — no messages means no surface target, and
    // persisting them would replay as orphan "Loading…" bubbles on restart.
    let has_messages = body
        .get("messages")
        .and_then(|m| m.as_array())
        .map(|a| !a.is_empty())
        .unwrap_or(false);
    if !has_messages {
        return;
    }
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    if let Some(m) = s
        .messages
        .iter_mut()
        .rev()
        .find(|m| m.role == "ui" && m.text == file_id)
    {
        m.a2ui = Some(body);
        // Keep original `ts` — the bubble's chronological spot AND its time
        // label both anchor on first creation. Mutations land in place; they
        // don't shuffle the bubble or reset its clock.
        return;
    }
    s.messages.push(config::AcpMessage {
        role: "ui".into(),
        text: file_id.into(),
        ts,
        a2ui: Some(body),
        tools: Vec::new(),
        turn_ms: 0,
    });
    if s.messages.len() > max_msgs {
        let drop = s.messages.len() - max_msgs;
        s.messages.drain(..drop);
    }
}

/// Snapshot every ACP session into `AcpHistory` and dispatch the JSON write
/// on a background thread. Mirrors `save_ai_prompt_history` — fire-and-forget,
/// errors are logged but never block the event loop. Caps each session's
/// message log at `max_msgs` (oldest dropped first).
fn persist_acp_history(sessions: &[AcpSession], active_id: u64, max_msgs: usize) {
    let snap_sessions: Vec<config::AcpSessionSnapshot> = sessions
        .iter()
        .map(|s| {
            let start = s.messages.len().saturating_sub(max_msgs);
            config::AcpSessionSnapshot {
                id: s.id,
                title: s.title.clone(),
                tag: s.tag.clone(),
                acp_session_id: s.acp_session_id.clone(),
                messages: s.messages[start..].to_vec(),
            }
        })
        .collect();
    let history = config::AcpHistory {
        sessions: snap_sessions,
        active_id,
    };
    std::thread::spawn(move || config::save_acp_history(&history));
}

/// Save session state and exit the event loop.
#[allow(clippy::too_many_arguments)]
fn save_and_exit(
    tabs: &Arc<Mutex<browser::TabManager>>,
    favicon_cache: &std::collections::HashMap<String, String>,
    prompt_history: &[String],
    ai_prompt_history: &[String],
    acp_sessions: &[AcpSession],
    acp_active_id: u64,
    max_acp_session_messages: usize,
    control_flow: &mut ControlFlow,
) {
    let mut tm = tabs.lock().unwrap();
    let session_tabs: Vec<config::SessionTab> = tm
        .tabs()
        .iter()
        .map(|t| config::SessionTab {
            url: t.url.clone(),
            title: t.title.clone(),
        })
        .collect();
    let active_url = tm
        .active_tab()
        .map(|t| t.url.as_str())
        .unwrap_or("")
        .to_string();
    tm.ensure_contiguous();
    config::save_session(&session_tabs, &active_url);
    config::save_favicons(favicon_cache);
    config::save_history(tm.history());
    config::save_prompt_history(prompt_history);
    config::save_ai_prompt_history(ai_prompt_history);
    // Synchronous final save so the ACP log is durable across cold restart
    // even if the OS reaps background threads on app exit.
    let acp_history = config::AcpHistory {
        sessions: acp_sessions
            .iter()
            .map(|s| {
                let start = s.messages.len().saturating_sub(max_acp_session_messages);
                config::AcpSessionSnapshot {
                    id: s.id,
                    title: s.title.clone(),
                    tag: s.tag.clone(),
                    acp_session_id: s.acp_session_id.clone(),
                    messages: s.messages[start..].to_vec(),
                }
            })
            .collect(),
        active_id: acp_active_id,
    };
    config::save_acp_history(&acp_history);
    drop(tm);
    // Terminate agent subprocesses before exiting. ControlFlow::Exit ends the
    // process via std::process::exit — destructors (and tokio's kill_on_drop)
    // never run, so octomind children would outlive us as orphans. An orphan
    // still flushing its session file makes the next launch's `--resume` race
    // it and silently mint a fresh session — that's how agent context got
    // lost across restarts. SIGTERM lets octomind save; bounded wait, then
    // SIGKILL stragglers.
    let pids: Vec<u32> = acp_sessions.iter().filter_map(|s| s.octomind_pid).collect();
    for &pid in &pids {
        unsafe { libc::kill(pid as libc::c_int, libc::SIGTERM) };
    }
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    for &pid in &pids {
        while unsafe { libc::kill(pid as libc::c_int, 0) } == 0
            && std::time::Instant::now() < deadline
        {
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        if unsafe { libc::kill(pid as libc::c_int, 0) } == 0 {
            tracing::warn!(pid, "octomind did not exit on SIGTERM — killing");
            unsafe { libc::kill(pid as libc::c_int, libc::SIGKILL) };
        }
    }
    crash_report::log_clean_shutdown();
    *control_flow = ControlFlow::Exit;
}

/// Returns true when octoweb is the frontmost macOS application.
///
/// Compares `[[NSWorkspace sharedWorkspace] frontmostApplication].processIdentifier`
/// to our own PID. We deliberately avoid both caching via `WindowEvent::Focused`
/// AND `[NSApp isActive]`:
///
/// - Cached `Focused`: modal panels from other apps (1Password AutoFill ⌘\,
///   screenshot picker, takeSnapshot, system dialogs) steal key-window status
///   from our NSWindow without macOS deactivating the app — tao delivers
///   `Focused(false)` but never `Focused(true)` when they dismiss, so a
///   cached flag stays stuck at false and breaks every hotkey.
///
/// - `[NSApp isActive]`: AppKit's own active state can fail to clear when
///   the user switches away (notification missed / not pumped fast enough),
///   so it can read `true` even when another app is frontmost. CGEventTap
///   would then keep consuming our hotkey keycodes globally and the user
///   sees "octoweb steals my keyboard in other apps".
///
/// `NSWorkspace.frontmostApplication` queries live system state every call
/// (no caching) and is the API Apple documents for "which app receives key
/// events right now". Cost: one Objective-C msg_send chain per call.
fn is_app_active() -> bool {
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    unsafe {
        let workspace: *mut AnyObject = msg_send![class!(NSWorkspace), sharedWorkspace];
        if workspace.is_null() {
            return false;
        }
        let frontmost: *mut AnyObject = msg_send![workspace, frontmostApplication];
        if frontmost.is_null() {
            return false;
        }
        let front_pid: i32 = msg_send![frontmost, processIdentifier];
        front_pid == std::process::id() as i32
    }
}
