mod acp;
mod address_bar_html;
mod browser;
mod cold_open;
mod config;
mod content_rules;
mod crash_report;
mod error_page_html;
mod find_bar_html;
mod hibernation;
mod inline_edit_html;
mod macos;
mod mcp;
mod nav_error_patch;
mod newtab_html;
mod notification_html;
mod overlay_html;
mod progress_bar_html;
mod quickslots;
mod quickslots_html;
mod settings_html;
mod shortcuts_html;
mod sidebar_html;
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
use mcp::{HistoryInfo, McpCommand, PageInfo, TabInfo};

#[derive(Debug)]
enum AppEvent {
    ToggleOverlay,
    HideOverlay,
    TitleChanged(usize, String),      // (tab_id, title)
    BrowserUrlChanged(usize, String), // (tab_id, url)
    FaviconFetched(String, String),   // (domain, data_uri)
    NavigateTo(String),
    SwitchTab(usize),
    CloseTab(usize),
    PrevTab,                                  // Ctrl+P — switch to previous tab in MRU order
    NextTab,                                  // Ctrl+N — switch to next tab in MRU order
    ToggleSidebar,                            // Cmd+Shift+A — toggle AI assistant sidebar
    AcpPrompt(String, Vec<(String, String)>), // user typed a prompt + optional images (base64, mime)
    AcpCancel,                                // user clicked stop button — cancel current prompt
    AcpRestart(String), // change agent tag (e.g. "octoweb:assistant") and reconnect
    AcpNewSession,      // restart session with same agent tag (clear chat)
    AskAI(String),      // overlay ⌘⇧Enter — open sidebar + send prompt
    ToggleDevTools,     // Cmd+Shift+I — open devtools for active tab
    OpenInNewTab(String), // Cmd+click / target=_blank — open URL in new tab and switch to it
    PageLoadStarted(usize), // (tab_id) — show progress bar
    PageLoadFinished(usize), // (tab_id) — hide progress bar
    NavigationError(usize, String, String), // (tab_id, url, error) — show error page
    Reload,             // Cmd+R — reload current page
    MediaPlaying(usize, bool), // (tab_id, is_playing) — audio/video state changed
    PageInfo(usize, u64, u64), // (tab_id, bytes, ms) — page load stats from PerformanceNavigationTiming
    RemoveHistory(String),     // URL to remove from history
    QuickSlotOpen(usize),      // ⌘1–⌘0 — open saved URL in slot 0–9
    QuickSlotSave(usize),      // ⌘⇧1–⌘⇧0 — save current page to slot 0–9
    QuickSlotRemove(usize),    // remove slot (from footer bar ✕ or newtab page)
    AcpWake,                   // lightweight wake — ACP thread pokes event loop
    AcpReconnect(u64), // scheduled reconnection attempt after error (carries generation to ignore stale timers)
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
    InlineEditReady(String, f64, f64), // (text, x, y) — selected text + cursor position
    InlineEditSubmit(String),        // user submitted prompt in modal
    InlineEditClose,                 // user closed modal (Esc / close btn)
    InlineEditHide,                  // hide modal but keep processing
    InlineEditResize(f64),           // modal content height changed
    ZoomIn,                          // ⌘= — increase page zoom
    ZoomOut,                         // ⌘- — decrease page zoom
    ZoomReset,                       // ⌘0 — reset page zoom to 100%
    Quit,
}
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
    // can open the overlay (Cmd+K) for the first time.
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
    // Tracks whether the browser window is the frontmost app.
    // CGEventTap fires system-wide, so we gate all hotkeys on this flag.
    let app_focused = Arc::new(AtomicBool::new(false));

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
    let overlay_win_id = overlay_win.id();
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
            // NSWindowCollectionBehaviorTransient — don't show in Mission Control / Exposé
            let _: () = msg_send![ns_win, setCollectionBehavior: 8u64]; // 1 << 3
                                                                        // Make chrome_win a child of browser_win — it moves/minimizes/closes with parent.
            let parent_ns: *mut AnyObject = browser_win.ns_window() as *mut AnyObject;
            // NSWindowAbove = 1
            let _: () = msg_send![parent_ns, addChildWindow: ns_win, ordered: 1i64];
        }
        w
    };
    let chrome_win = Arc::new(chrome_win);
    let chrome_win_id = chrome_win.id();

    // ── Browser WebView factory ───────────────────────────────────────────
    // Each tab gets its own WebView (build_as_child). Hide/show to switch —
    // no reload, full state (scroll, video, JS) preserved.
    let home = cfg.home_page.clone();
    let search_engine = cfg.search_engine.clone();

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
                                let x = v["x"].as_f64().unwrap_or(0.0);
                                let y = v["y"].as_f64().unwrap_or(0.0);
                                let _ = p3.send_event(AppEvent::InlineEditReady(text, x, y));
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
                        // Regular target=_blank — open in a new tab.
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
                    }
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
            let p = proxy.clone();
            nav_error_patch::register(wv_ptr, move |url, code| {
                let _ = p.send_event(AppEvent::NavigationError(tab_id, url, code.to_string()));
            });
            let pt = proxy.clone();
            nav_error_patch::register_termination(wv_ptr, move || {
                let _ = pt.send_event(AppEvent::WebContentTerminated(tab_id));
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
    let _shortcuts_wv = WebViewBuilder::new()
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
        .with_html(sidebar_html::html())
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
                                let _ = p.send_event(AppEvent::AcpPrompt(text, images));
                            }
                        }
                        Some("acp_cancel") => {
                            let _ = p.send_event(AppEvent::AcpCancel);
                        }
                        Some("sidebar_close") => {
                            let _ = p.send_event(AppEvent::ToggleSidebar);
                        }
                        Some("acp_set_agent") => {
                            if let Some(tag) = v["tag"].as_str() {
                                let _ = p.send_event(AppEvent::AcpRestart(tag.to_string()));
                            }
                        }
                        Some("acp_new_session") => {
                            let _ = p.send_event(AppEvent::AcpNewSession);
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
                                    let arr: *mut AnyObject = msg_send![
                                        class!(NSArray),
                                        arrayWithObject: ns_str
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
                        Some("drag_move") => {
                            let dx = v["dx"].as_f64().unwrap_or(0.0);
                            let dy = v["dy"].as_f64().unwrap_or(0.0);
                            if let Ok(pos) = bw.outer_position() {
                                let scale = bw.scale_factor();
                                let lp: tao::dpi::LogicalPosition<f64> = pos.to_logical(scale);
                                bw.set_outer_position(tao::dpi::LogicalPosition::new(
                                    lp.x + dx,
                                    lp.y + dy,
                                ));
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
                                    let arr: *mut AnyObject = msg_send![
                                        class!(NSArray),
                                        arrayWithObject: ns_str
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
    let mut inline_edit_selected_text = String::new();
    let mut inline_edit_tab_id: usize = 0;
    let mut inline_edit_acp: Option<acp::AcpHandle> = None;
    let mut inline_edit_response = String::new();
    let mut prompt_history: Vec<String> = config::load_prompt_history();
    prompt_history.truncate(cfg.max_prompt_history);

    // ── Per-tab WebContent process stats (CPU%, RSS) ───────────────────────
    // Sampled every 2 s from the active tab's WebContent XPC process.
    let mut sys_stats_last: Option<tab_stats::TabStatsSample> = None;
    let mut sys_stats_next_at = std::time::Instant::now();
    // Tabs with active media playback — used to throttle sys_stats polling
    let mut media_playing_tabs: std::collections::HashSet<usize> = std::collections::HashSet::new();

    // ── Crash diagnostics: periodic health log ────────────────────────────
    let app_start_instant = std::time::Instant::now();
    let mut health_log_next_at = std::time::Instant::now() + std::time::Duration::from_secs(30);

    // ── Debounced history persistence: save 60 s after first mutation ─────
    let mut history_save_at: Option<std::time::Instant> = None;

    // ── Proactive hibernation: runs every 60 s independent of memory pressure ─
    // Complementary to the reactive hibernation (which only fires under pressure).
    // Hibernates frozen (idle > 10 min) tabs always, and cold (idle > 3 min)
    // tabs when too many background WebViews are alive or a tab is memory-heavy.
    let mut proactive_hiber_next_at =
        std::time::Instant::now() + std::time::Duration::from_secs(60);

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

    // ── ACP handle — spawns octomind acp subprocess in background ─────────
    let mut acp_tag = "octoweb:assistant".to_string();
    let acp_proxy = proxy.clone();
    let mut acp_handle = acp::AcpHandle::connect(&format!("octomind acp {}", acp_tag), {
        let p = acp_proxy.clone();
        move || {
            let _ = p.send_event(AppEvent::AcpWake);
        }
    })
    .ok();

    // ACP reconnection state — exponential backoff on connection failures
    let mut acp_retry_count: u32 = 0;
    // Generation counter: incremented on every intentional reconnect so stale
    // sleep-thread timers (from prior error backoffs) are ignored when they fire.
    let mut acp_reconnect_gen: u64 = 0;
    const ACP_MAX_RETRIES: u32 = 5;
    const ACP_BASE_DELAY_SECS: u64 = 1;
    const ACP_MAX_DELAY_SECS: u64 = 30;

    // ── MCP server — exposes browser control tools on localhost:3434 ───────
    let mut mcp_handle = Some(mcp::spawn_mcp_server());

    // ── Global hotkey via CGEventTap ─────────────────────────────────────
    // rdev crashes on macOS 15+ because TSMGetInputSourceProperty (called in
    // rdev's raw_callback) asserts it must run on the main thread. We use
    // CGEventTap directly — it runs on the main CFRunLoop, no extra thread.
    let _tap = {
        use core_foundation::base::TCFType;
        use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
        use core_graphics::event::{
            CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
            CallbackResult,
        };

        let p = proxy.clone();
        let overlay_state = Arc::clone(&overlay_hotkey_visible);
        let find_bar_state = Arc::clone(&find_bar_hotkey_visible);
        let inline_edit_state = Arc::clone(&inline_edit_hotkey_visible);
        let focused_state = Arc::clone(&app_focused);
        // Stores the raw CFMachPortRef so the callback can re-enable the tap
        // if macOS disables it due to timeout (see TapDisabledByTimeout below).
        let tap_port = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let tap_port_cb = Arc::clone(&tap_port);
        // keyCode 40 = k, flagsChanged catches modifier-only events separately.
        // We check the CGEventFlags for Command inside the callback.
        let tap = CGEventTap::new(
            CGEventTapLocation::HID,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::Default, // active tap — lets us consume specific keys
            vec![CGEventType::KeyDown],
            move |_proxy, _etype, event| {
                // macOS disables active event taps that take too long to respond.
                // When that happens, it sends TapDisabledByTimeout — re-enable immediately.
                // CGEventType doesn't impl PartialEq, so compare raw discriminants.
                let etype_raw = _etype as u32;
                if etype_raw == CGEventType::TapDisabledByTimeout as u32
                    || etype_raw == CGEventType::TapDisabledByUserInput as u32
                {
                    let port = tap_port_cb.load(Ordering::Relaxed);
                    if port != 0 {
                        extern "C" {
                            fn CGEventTapEnable(tap: *mut std::ffi::c_void, enable: bool);
                        }
                        unsafe { CGEventTapEnable(port as *mut std::ffi::c_void, true) };
                        tracing::warn!("CGEventTap was disabled by macOS — re-enabled");
                    }
                    return CallbackResult::Keep;
                }
                // Only act when octoweb is the frontmost application.
                if !focused_state.load(Ordering::Relaxed) {
                    return CallbackResult::Keep;
                }
                use core_graphics::event::{CGEventFlags, EventField};
                const K_KEYCODE: i64 = 40; // k
                const W_KEYCODE: i64 = 13; // w
                const Q_KEYCODE: i64 = 12; // q
                const P_KEYCODE: i64 = 35; // p
                const N_KEYCODE: i64 = 45; // n
                const A_KEYCODE: i64 = 0;  // a (Cmd+Shift+A = toggle sidebar)
                const I_KEYCODE: i64 = 34; // i (Cmd+Shift+I = toggle devtools)
                const COMMA_KEYCODE: i64 = 43; // , (Cmd+, = settings)
                const SLASH_KEYCODE: i64 = 44; // / (Cmd+/ = toggle shortcuts)
                const R_KEYCODE: i64 = 15; // r (Cmd+R = reload)
                const F_KEYCODE: i64 = 3;  // f (Cmd+F = find in page)
                const S_KEYCODE: i64 = 1;  // s (Cmd+S = screenshot, Cmd+Shift+S = full page screenshot)
                const D_KEYCODE: i64 = 2;  // d (Ctrl+D = scroll half down)
                const U_KEYCODE: i64 = 32; // u (Ctrl+U = scroll half up)
                const T_KEYCODE: i64 = 17; // t (Ctrl+T = scroll to top)
                const B_KEYCODE: i64 = 11; // b (Ctrl+B = scroll to bottom)
                const E_KEYCODE: i64 = 14; // e (⌘⇧E = inline AI edit)
                const EQUAL_KEYCODE: i64 = 24; // =/+ (⌘= = zoom in)
                const MINUS_KEYCODE: i64 = 27; // -/_ (⌘- = zoom out)
                const ESC_KEYCODE: i64 = 53; // Escape
                // Digit keycodes: 1–9 = keycodes 18,19,20,21,23,22,26,28,25; 0 = 29
                const DIGIT_KEYCODES: [i64; 10] = [18, 19, 20, 21, 23, 22, 26, 28, 25, 29];
                let keycode = event.get_integer_value_field(EventField::KEYBOARD_EVENT_KEYCODE);
                let flags = event.get_flags();
                let cmd   = flags.contains(CGEventFlags::CGEventFlagCommand);
                let ctrl  = flags.contains(CGEventFlags::CGEventFlagControl);
                let shift = flags.contains(CGEventFlags::CGEventFlagShift);
                // Return Drop to consume (suppress) the event, Keep to pass it through.
                if cmd && keycode == K_KEYCODE {
                    let _ = p.send_event(AppEvent::ToggleOverlay);
                    CallbackResult::Drop
                } else if cmd && shift && keycode == A_KEYCODE {
                    let _ = p.send_event(AppEvent::ToggleSidebar);
                    CallbackResult::Drop
                } else if cmd && shift && keycode == E_KEYCODE && !overlay_state.load(Ordering::Relaxed) {
                    let _ = p.send_event(AppEvent::InlineEditRequest);
                    CallbackResult::Drop
                } else if cmd && shift && keycode == I_KEYCODE {
                    let _ = p.send_event(AppEvent::ToggleDevTools);
                    CallbackResult::Drop
                } else if cmd && keycode == COMMA_KEYCODE {
                    let _ = p.send_event(AppEvent::ToggleSettings);
                    CallbackResult::Drop
                } else if cmd && keycode == SLASH_KEYCODE {
                    let _ = p.send_event(AppEvent::ToggleShortcuts);
                    CallbackResult::Drop
                } else if cmd && keycode == W_KEYCODE && !overlay_state.load(Ordering::Relaxed) {
                    let _ = p.send_event(AppEvent::CloseTab(0));
                    CallbackResult::Drop
                } else if cmd && !shift && keycode == Q_KEYCODE {
                    let _ = p.send_event(AppEvent::Quit);
                    CallbackResult::Drop
                } else if ctrl && keycode == P_KEYCODE && !overlay_state.load(Ordering::Relaxed) && !inline_edit_state.load(Ordering::Relaxed) {
                    if find_bar_state.load(Ordering::Relaxed) {
                        let _ = p.send_event(AppEvent::FindPrev);
                    } else {
                        let _ = p.send_event(AppEvent::PrevTab);
                    }
                    CallbackResult::Drop
                } else if ctrl && keycode == N_KEYCODE && !overlay_state.load(Ordering::Relaxed) && !inline_edit_state.load(Ordering::Relaxed) {
                    if find_bar_state.load(Ordering::Relaxed) {
                        let _ = p.send_event(AppEvent::FindNext);
                    } else {
                        let _ = p.send_event(AppEvent::NextTab);
                    }
                    CallbackResult::Drop
                } else if ctrl && keycode == D_KEYCODE && !overlay_state.load(Ordering::Relaxed) && !inline_edit_state.load(Ordering::Relaxed) {
                    let _ = p.send_event(AppEvent::ScrollDown);
                    CallbackResult::Drop
                } else if ctrl && keycode == U_KEYCODE && !overlay_state.load(Ordering::Relaxed) && !inline_edit_state.load(Ordering::Relaxed) {
                    let _ = p.send_event(AppEvent::ScrollUp);
                    CallbackResult::Drop
                } else if ctrl && keycode == T_KEYCODE && !overlay_state.load(Ordering::Relaxed) && !inline_edit_state.load(Ordering::Relaxed) {
                    let _ = p.send_event(AppEvent::ScrollTop);
                    CallbackResult::Drop
                } else if ctrl && keycode == B_KEYCODE && !overlay_state.load(Ordering::Relaxed) && !inline_edit_state.load(Ordering::Relaxed) {
                    let _ = p.send_event(AppEvent::ScrollBottom);
                    CallbackResult::Drop
                } else if cmd && keycode == R_KEYCODE && !overlay_state.load(Ordering::Relaxed) {
                    let _ = p.send_event(AppEvent::Reload);
                    CallbackResult::Drop
                } else if cmd && shift && keycode == S_KEYCODE && !overlay_state.load(Ordering::Relaxed) {
                    let _ = p.send_event(AppEvent::ScreenshotFullPage);
                    CallbackResult::Drop
                } else if cmd && keycode == S_KEYCODE && !overlay_state.load(Ordering::Relaxed) {
                    let _ = p.send_event(AppEvent::Screenshot);
                    CallbackResult::Drop
                } else if keycode == ESC_KEYCODE && find_bar_state.load(Ordering::Relaxed) {
                    // Esc closes find bar regardless of which WebView has focus
                    let _ = p.send_event(AppEvent::HideFindBar);
                    CallbackResult::Drop
                } else if cmd && keycode == EQUAL_KEYCODE && !overlay_state.load(Ordering::Relaxed) {
                    let _ = p.send_event(AppEvent::ZoomIn);
                    CallbackResult::Drop
                } else if cmd && keycode == MINUS_KEYCODE && !overlay_state.load(Ordering::Relaxed) {
                    let _ = p.send_event(AppEvent::ZoomOut);
                    CallbackResult::Drop
                } else if cmd && !shift && keycode == 29 && !overlay_state.load(Ordering::Relaxed) {
                    // ⌘0 = reset zoom (overrides QuickSlotOpen for digit 0)
                    let _ = p.send_event(AppEvent::ZoomReset);
                    CallbackResult::Drop
                } else if cmd && keycode == F_KEYCODE && !overlay_state.load(Ordering::Relaxed) {
                    let _ = p.send_event(AppEvent::ToggleFindBar);
                    CallbackResult::Drop
                } else if cmd && !overlay_state.load(Ordering::Relaxed) {
                    // ⌘+digit → QuickSlotOpen, ⌘⇧+digit → QuickSlotSave
                    if let Some(slot) = DIGIT_KEYCODES.iter().position(|&k| k == keycode) {
                        if shift {
                            let _ = p.send_event(AppEvent::QuickSlotSave(slot));
                        } else {
                            let _ = p.send_event(AppEvent::QuickSlotOpen(slot));
                        }
                        return CallbackResult::Drop;
                    }
                    CallbackResult::Keep
                } else {
                    CallbackResult::Keep
                }
            },
        )
            .expect("CGEventTap::new failed — go to System Settings → Privacy & Security → Accessibility and enable your terminal app");

        // Store the raw mach port ref so the callback can re-enable the tap.
        tap_port.store(
            tap.mach_port().as_concrete_TypeRef() as usize,
            Ordering::Relaxed,
        );
        tap.enable();
        // Schedule the tap's mach port on the main run loop so it fires
        // on the same thread as AppKit (no TSM thread-assertion crash).
        let loop_src = tap
            .mach_port()
            .create_runloop_source(0)
            .expect("CFRunLoopSource creation failed");
        CFRunLoop::get_current().add_source(&loop_src, unsafe { kCFRunLoopCommonModes });

        tap // keep alive for the duration of the program
    };

    let mut modifiers = ModifiersState::default();
    let mut overlay_visible = false;
    let mut sidebar_visible = false;
    let mut shortcuts_visible = false;
    let mut icon_set = false;
    let mut zoom_level: f64 = 1.0;

    // ── Helper macros (expand in-place, access event-loop locals) ─────────

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
            tab_webviews.insert(id, wv);
            if zoom_level != 1.0 {
                if let Some(wv) = tab_webviews.get(&id) {
                    let _ = wv.zoom(zoom_level);
                }
            }
            id
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
                    let p = proxy.clone();
                    nav_error_patch::register(wv_ptr, move |url, code| {
                        let _ =
                            p.send_event(AppEvent::NavigationError(target, url, code.to_string()));
                    });
                    let pt = proxy.clone();
                    nav_error_patch::register_termination(wv_ptr, move || {
                        let _ = pt.send_event(AppEvent::WebContentTerminated(target));
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
                    }
                    let _ = wv.set_visible(false);
                    tab_webviews.insert(target, wv);
                }
            }
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

    /// Refresh the overlay item list (if visible).
    macro_rules! refresh_overlay {
        () => {
            if overlay_visible {
                let json = {
                    let mut tm = tabs.lock().unwrap();
                    tm.ensure_contiguous();
                    webview_utils::build_items_json(tm.tabs(), tm.history(), &favicon_cache)
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
            let tm = tabs.lock().unwrap();
            let tab = tm.tabs().iter().find(|t| t.url == *u || t.id == active_wv_id);
            let raw_title = tab.map(|t| t.title.clone()).unwrap_or_default();
            let (pb, pt) = tab.map(|t| (t.page_bytes, t.page_time_ms)).unwrap_or((0, 0));
            drop(tm);
            let escaped_title = webview_utils::escape_js_template(&raw_title);
            let _ = address_bar_wv.evaluate_script(&format!(
                "window.__update && window.__update(`{escaped_url}`, {secure}, `{escaped_title}`, {pb}, {pt})"
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

        // ── Crash diagnostics: periodic health snapshot ──────────────────
        if now >= health_log_next_at {
            health_log_next_at = now + std::time::Duration::from_secs(30);
            let active_rss_mb = sys_stats_mem.load(Ordering::Relaxed);
            let main_rss_mb = crash_report::main_process_rss() / (1024 * 1024);
            let pressure = hibernation::system_memory_pressure();
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
        // Piggybacks on the sys_stats timer — checked every 2 s.
        // Under memory pressure, destroys WebViews of idle background tabs
        // (moving them to pending_tabs for lazy reload on next switch).
        {
            let pressure = hibernation::system_memory_pressure();
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

        // Drain ACP events and forward to the UI on every tick.
        // Collect into owned Vec first so we can drop the handle on error.
        let acp_events = acp_handle.as_mut().map(|h| h.poll()).unwrap_or_default();
        for ev in acp_events {
            match ev {
                    acp::AgentEvent::Connected => {
                        acp_retry_count = 0; // reset retry count on successful connection
                        let _ = sidebar_wv.evaluate_script(
                            "window.__setConnected && window.__setConnected()"
                        );
                    }
                    acp::AgentEvent::Image { data, mime_type } => {
                        // Pass base64 image to sidebar — use template literal to avoid quote issues
                        let _ = sidebar_wv.evaluate_script(&format!(
                            "window.__appendImage && window.__appendImage(`{mime_type}`,`{data}`)"
                        ));
                    }
                    acp::AgentEvent::Chunk(chunk) => {
                        let escaped = webview_utils::escape_js_template(&chunk);
                        let _ = sidebar_wv.evaluate_script(&format!(
                        "window.__appendChunk && window.__appendChunk(`{escaped}`)"
                    ));
                        // Show badge + notification toast when sidebar is hidden
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
                        let eid = webview_utils::escape_js_template(&id);
                        let etitle = webview_utils::escape_js_template(&title);
                        let ekind = webview_utils::escape_js_template(&kind);
                        let raw_input_json = raw_input
                            .as_ref()
                            .and_then(|v| serde_json::to_string(v).ok())
                            .unwrap_or_else(|| "null".to_string());
                        let locations_json = serde_json::to_string(&locations).unwrap_or_else(|_| "[]".to_string());
                        let _ = sidebar_wv.evaluate_script(&format!(
                            "window.__toolStart && window.__toolStart(`{eid}`,`{etitle}`,`{ekind}`,{raw_input_json},{locations_json})"
                        ));
                    }
                    acp::AgentEvent::ToolUpdate { id, title, status, raw_output } => {
                        let eid = webview_utils::escape_js_template(&id);
                        let etitle = webview_utils::escape_js_template(title.as_deref().unwrap_or(""));
                        let estatus = webview_utils::escape_js_template(&status);
                        let raw_output_json = raw_output
                            .as_ref()
                            .and_then(|v| serde_json::to_string(v).ok())
                            .unwrap_or_else(|| "null".to_string());
                        let _ = sidebar_wv.evaluate_script(&format!(
                            "window.__toolUpdate && window.__toolUpdate(`{eid}`,`{etitle}`,`{estatus}`,{raw_output_json})"
                        ));
                    }
                    acp::AgentEvent::Done => {
                        let _ = sidebar_wv.evaluate_script(
                            "window.__setThinking && window.__setThinking(false)"
                        );
                        if !sidebar_visible {
                            let _ = address_bar_wv.evaluate_script(
                                "window.__setBadge && window.__setBadge(true)"
                            );
                        }
                    }
                    acp::AgentEvent::Cancelled => {
                        let _ = sidebar_wv.evaluate_script(
                            "window.__setThinking && window.__setThinking(false)"
                        );
                    }
                    acp::AgentEvent::Error(err) => {
                        tracing::warn!(error = %err, "ACP connection error");
                        // Drop dead handle immediately so prompts aren't silently lost
                        acp_handle = None;
                        // Schedule reconnection with exponential backoff
                        if acp_retry_count < ACP_MAX_RETRIES {
                            acp_retry_count += 1;
                            let delay = std::cmp::min(
                                ACP_BASE_DELAY_SECS * 2u64.pow(acp_retry_count - 1),
                                ACP_MAX_DELAY_SECS,
                            );
                            tracing::info!(retry = acp_retry_count, delay_s = delay, "scheduling ACP reconnection");
                            // Show connecting status in UI
                            let _ = sidebar_wv.evaluate_script(
                                "window.__setConnecting && window.__setConnecting()"
                            );
                            // Bump generation so any previously scheduled timer is ignored.
                            acp_reconnect_gen += 1;
                            let gen = acp_reconnect_gen;
                            // Schedule reconnection after delay
                            let proxy_clone = acp_proxy.clone();
                            std::thread::spawn(move || {
                                std::thread::sleep(std::time::Duration::from_secs(delay));
                                let _ = proxy_clone.send_event(AppEvent::AcpReconnect(gen));
                            });
                        } else {
                            // Max retries exceeded — show error state
                            tracing::error!("ACP max reconnection retries exceeded");
                            let _ = sidebar_wv.evaluate_script("window.__setError && window.__setError()");
                            let escaped = webview_utils::escape_js_template(&err);
                            let _ = sidebar_wv.evaluate_script(&format!(
                                "window.__appendError && window.__appendError(`{escaped}`)"
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

        // Drain MCP commands and execute on main thread (WebView is not thread-safe).
        if let Some(ref mut handle) = mcp_handle {
            while let Some(cmd) = handle.poll() {
                tracing::debug!(cmd = ?std::mem::discriminant(&cmd), "MCP command received");

                match cmd {
                    McpCommand::Navigate { url, tab_id, new_tab, background, response } => {
                        tracing::debug!(url = %url, ?tab_id, new_tab, background, "MCP navigate");

                        // External scheme → hand off to macOS, not loadable in WebView
                        if url::is_external_scheme(&url) {
                            let ok = macos::open_external_url(&url);
                            let _ = response.send(if ok {
                                Ok(active_wv_id)
                            } else {
                                Err(format!("No app registered for URL: {url}"))
                            });
                            continue;
                        }

                        if new_tab {
                            // Open a new tab with its own WebView
                            let resolved = url::resolve_url(&url, &search_engine);
                            let new_id = tabs.lock().unwrap().open(resolved.clone());
                            spawn_tab_webview!(new_id, &resolved);

                            if !background {
                                // Switch to the new tab (deferred swap)
                                let visible_id = pending_swap.map(|(old, _)| old).unwrap_or(active_wv_id);
                                active_wv_id = new_id;
                                pending_swap = Some((visible_id, new_id));
                                macos::mru_push(&mut mru, new_id);
                                if app_focused.load(Ordering::Relaxed) { browser_win.set_focus(); }
                            }
                            let _ = response.send(Ok(new_id));
                        } else {
                            // Navigate an existing tab in-place
                            let target_id = tab_id.unwrap_or(active_wv_id);
                            if let Some(wv) = tab_webviews.get(&target_id) {
                                let resolved = url::resolve_url(&url, &search_engine);
                                let escaped = resolved.replace('\\', "\\\\").replace('\'', "\\'");
                                let _ = wv.evaluate_script(&format!("window.location.href = '{escaped}'"));
                                if tabs.lock().unwrap().update_url(target_id, resolved) {
                                    history_save_at.get_or_insert(std::time::Instant::now() + std::time::Duration::from_secs(60));
                                }
                                let _ = response.send(Ok(target_id));
                            } else {
                                let _ = response.send(Err("Tab not found".to_string()));
                            }
                        }
                    }
                    McpCommand::GetTabs { response } => {
                        tracing::debug!("MCP get_tabs");

                        let tm = tabs.lock().unwrap();
                        let tabs_list: Vec<TabInfo> = tm.tabs().iter().map(|t| TabInfo {
                            id: t.id,
                            title: t.title.clone(),
                            url: t.url.clone(),
                            is_active: tm.active_id() == Some(t.id),
                            is_playing_audio: t.is_playing_audio,
                        }).collect();

                        tracing::debug!(count = tabs_list.len(), "MCP get_tabs response");

                        let _ = response.send(Ok(tabs_list));
                    }
                    McpCommand::SwitchTab { tab_id, response } => {
                        tracing::debug!(tab_id, "MCP switch_tab");

                        let _ = proxy.send_event(AppEvent::SwitchTab(tab_id));
                        let _ = response.send(Ok(()));
                    }
                    McpCommand::CloseTab { tab_id, response } => {
                        tracing::debug!(tab_id, "MCP close_tab");

                        let _ = proxy.send_event(AppEvent::CloseTab(tab_id));
                        let _ = response.send(Ok(()));
                    }
                    McpCommand::GetPageInfo { tab_id, response } => {
                        let tm = tabs.lock().unwrap();
                        let target_id = tab_id.or(tm.active_id());
                        let base_info = target_id.and_then(|id| {
                            tm.tabs().iter().find(|t| t.id == id).map(|t| (id, t.title.clone(), t.url.clone()))
                        });
                        drop(tm);
                        match base_info {
                            Some((id, title, url)) => {
                                if let Some(wv) = tab_webviews.get(&id) {
                                    let response = std::sync::Arc::new(std::sync::Mutex::new(Some(response)));
                                    let response_cb = response.clone();
                                    let cb_title = title.clone();
                                    let cb_url = url.clone();
                                    match wv.evaluate_script_with_callback(
                                        "document.querySelector('meta[name=\"description\"]')?.content || ''",
                                        move |val| {
                                            if let Some(tx) = response_cb.lock().unwrap().take() {
                                                let desc = serde_json::from_str::<String>(&val).unwrap_or(val);
                                                let description = if desc.is_empty() { None } else { Some(desc) };
                                                let _ = tx.send(Ok(PageInfo { title: cb_title.clone(), url: cb_url.clone(), description }));
                                            }
                                        },
                                    ) {
                                        Ok(()) => {}
                                        Err(_) => {
                                            // JS failed — return info without description
                                            if let Some(tx) = response.lock().unwrap().take() {
                                                let _ = tx.send(Ok(PageInfo { title, url, description: None }));
                                            }
                                        }
                                    }
                                } else {
                                    let _ = response.send(Ok(PageInfo { title, url, description: None }));
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
                            if let Some(wv) = tab_webviews.get(&id) {
                                // Wrap sender so we can reclaim it if the callback never fires
                                let response = std::sync::Arc::new(std::sync::Mutex::new(Some(response)));
                                let response_cb = response.clone();
                                match wv.evaluate_script_with_callback(&script, move |val| {
                                    if let Some(tx) = response_cb.lock().unwrap().take() {
                                        let _ = tx.send(Ok(val));
                                    }
                                }) {
                                    Ok(()) => {}
                                    Err(e) => {
                                        // callback won't fire — send error via the Arc
                                        if let Some(tx) = response.lock().unwrap().take() {
                                            let _ = tx.send(Err(format!("JS error: {e}")));
                                        }
                                    }
                                }
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
                            if let Some(wv) = tab_webviews.get(&id) {
                                let script = format!(
                                    "(function() {{ const el = document.querySelector({}); if (el) {{ el.click(); return true; }} return false; }})()",
                                    serde_json::to_string(&selector).unwrap_or_default()
                                );
                                match wv.evaluate_script(&script) {
                                    Ok(_) => { let _ = response.send(Ok(())); }
                                    Err(e) => { let _ = response.send(Err(format!("Click failed: {}", e))); }
                                }
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
                            if let Some(wv) = tab_webviews.get(&id) {
                                let script = format!(
                                    "(function() {{ const el = document.querySelector({}); if (el) {{ el.value = {}; el.dispatchEvent(new Event('input', {{bubbles: true}})); return true; }} return false; }})()",
                                    serde_json::to_string(&selector).unwrap_or_default(),
                                    serde_json::to_string(&text).unwrap_or_default()
                                );
                                match wv.evaluate_script(&script) {
                                    Ok(_) => { let _ = response.send(Ok(())); }
                                    Err(e) => { let _ = response.send(Err(format!("Type failed: {}", e))); }
                                }
                            } else {
                                let _ = response.send(Err("Tab not found".to_string()));
                            }
                        } else {
                            let _ = response.send(Err("No active tab".to_string()));
                        }
                    }
                    McpCommand::GetCurrentTab { response } => {
                        let tm = tabs.lock().unwrap();
                        let result = tm.active_tab().map(|t| TabInfo {
                            id: t.id,
                            title: t.title.clone(),
                            url: t.url.clone(),
                            is_active: true,
                            is_playing_audio: t.is_playing_audio,
                        }).ok_or("No active tab".to_string());
                        let _ = response.send(result);
                    }
                    McpCommand::GoBack { tab_id, response } => {
                        let target_id = tab_id.or(Some(active_wv_id));
                        if let Some(id) = target_id {
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
                        let entries: Vec<HistoryInfo> = history.iter().rev().take(limit).map(|e| HistoryInfo {
                            title: e.title.clone(),
                            url: e.url.clone(),
                            visited_at: e.visited_at,
                        }).collect();
                        let _ = response.send(Ok(entries));
                    }
                    McpCommand::GetPlayingTabs { response } => {
                        let tm = tabs.lock().unwrap();
                        let playing: Vec<TabInfo> = tm.tabs().iter()
                            .filter(|t| t.is_playing_audio)
                            .map(|t| TabInfo {
                                id: t.id,
                                title: t.title.clone(),
                                url: t.url.clone(),
                                is_active: tm.active_id() == Some(t.id),
                                is_playing_audio: true,
                            }).collect();
                        let _ = response.send(Ok(playing));
                    }
                    McpCommand::Reload { tab_id, response } => {
                        let target_id = tab_id.unwrap_or(active_wv_id);
                        if let Some(wv) = tab_webviews.get(&target_id) {
                            let _ = wv.reload();
                            let _ = response.send(Ok(()));
                        } else {
                            let _ = response.send(Err("Tab not found".to_string()));
                        }
                    }
                    McpCommand::Screenshot { tab_id, full_page, response } => {
                        let target_id = tab_id.unwrap_or(active_wv_id);
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
                                        let final_image = pdf_data_to_nsimage(pdf_data);
                                        if final_image.is_null() {
                                            let _ = tx.send(Err("Failed to render PDF to image".to_string()));
                                            return;
                                        }
                                        let png_data = nsimage_to_png_data(final_image);
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
                    McpCommand::GetPageContent { tab_id, response } => {
                        let target_id = tab_id.unwrap_or(active_wv_id);
                        if let Some(wv) = tab_webviews.get(&target_id) {
                            let response = std::sync::Arc::new(std::sync::Mutex::new(Some(response)));
                            let response_cb = response.clone();
                            match wv.evaluate_script_with_callback(
                                "document.body ? document.body.innerText : ''",
                                move |val| {
                                    if let Some(tx) = response_cb.lock().unwrap().take() {
                                        // val comes as a JSON string — strip outer quotes
                                        let text = serde_json::from_str::<String>(&val).unwrap_or(val);
                                        let _ = tx.send(Ok(text));
                                    }
                                },
                            ) {
                                Ok(()) => {}
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

            // ── Toggle shortcuts overlay ──────────────────────────────────
            // ── Settings modal (⌘,) ──────────────────────────────────────
            Event::UserEvent(AppEvent::ToggleSettings) => {
                if settings_visible {
                    settings_win.set_visible(false);
                    settings_visible = false;
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
                    settings_win.set_visible(true);
                    settings_win.set_focus();
                    settings_visible = true;
                }
            }
            Event::UserEvent(AppEvent::HideSettings) => {
                settings_win.set_visible(false);
                settings_visible = false;
            }
            Event::UserEvent(AppEvent::UpdateConfig(key, val)) => {
                match key.as_str() {
                    "home_page" => cfg.home_page = val,
                    "search_engine" => cfg.search_engine = val,
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
                    _ => {}
                }
                cfg.save();
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
                        webview_utils::build_items_json(tm.tabs(), tm.history(), &favicon_cache)
                    };
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
                if inline_edit_visible {
                    let _ = inline_edit_wv.set_visible(false);
                    let _ = inline_edit_wv.evaluate_script("window.__clear && window.__clear()");
                    inline_edit_visible = false;
                    if let Some(ref h) = inline_edit_acp { h.cancel(); }
                    inline_edit_acp = None;
                    inline_edit_response.clear();
                }
                if tab_id == active_wv_id {
                    if app_focused.load(Ordering::Relaxed) { browser_win.set_focus(); }
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
                if app_focused.load(Ordering::Relaxed) { browser_win.set_focus(); }
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
                    }
                    tab_webviews.remove(&id);
                    media_playing_tabs.remove(&id);  // Clean up stale media state
                    pending_tabs.remove(&id);  // Also remove if it was pending lazy load
                    tab_snapshots.remove(&id);
                    deferred_nav.remove(&id);
                    mru.retain(|&x| x != id);
                    // Switch to the most-recently-used tab (MRU[0] after removal)
                    match mru.first().copied() {
                        Some(next) => {
                            tabs.lock().unwrap().switch(next);
                            // switch_visible_tab! handles lazy loading (pending_tabs) and
                            // sets active_wv_id + updates address bar — covers both loaded
                            // and not-yet-loaded tabs.
                            switch_visible_tab!(next);
                            if app_focused.load(Ordering::Relaxed) { browser_win.set_focus(); }
                        }
                        None => *control_flow = ControlFlow::Exit,
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
                    // Return key window status to browser_win so the page
                    // WebView receives keyboard input again.
                    browser_win.set_focus();
                } else {
                    // Reposition sidebar to right edge (window may have been resized)
                    let _ = sidebar_wv.set_bounds(wry::Rect {
                        position: tao::dpi::PhysicalPosition::new(
                            sz.width.saturating_sub(sidebar_w),
                            0u32,
                        ).into(),
                        size: tao::dpi::PhysicalSize::new(sidebar_w, sz.height).into(),
                    });
                    let _ = sidebar_wv.set_visible(true);
                    sidebar_visible = true;
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
                    // Connect ACP if not yet connected
                    if acp_handle.is_none() {
                        acp_retry_count = 0; // reset retry count on manual open
                        acp_reconnect_gen += 1; // invalidate any pending backoff timers
                        acp_handle = acp::AcpHandle::connect(&format!("octomind acp {}", acp_tag), {
                            let p = acp_proxy.clone();
                            move || { let _ = p.send_event(AppEvent::AcpWake); }
                        }).ok();
                    }
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
            Event::UserEvent(AppEvent::AcpPrompt(text, images)) => {
                if let Some(ref handle) = acp_handle {
                    if !handle.send_prompt(text, images) {
                        // Channel dead — ACP thread exited without error event
                        acp_handle = None;
                        let _ = sidebar_wv.evaluate_script(
                            "window.__setError && window.__setError()"
                        );
                    }
                }
            }

            // ── ACP cancel (stop button) ───────────────────────────────────────────
            Event::UserEvent(AppEvent::AcpCancel) => {
                if let Some(ref handle) = acp_handle {
                    handle.cancel();
                }
            }

            // ── ACP restart with new agent command ─────────────────────────────────
            Event::UserEvent(AppEvent::AcpRestart(tag)) => {
                acp_tag = tag.clone();
                acp_retry_count = 0; // reset retry count on manual restart
                acp_reconnect_gen += 1; // invalidate any pending backoff timers
                acp_handle = None; // drop old handle (kills subprocess)
                acp_handle = acp::AcpHandle::connect(&format!("octomind acp {}", acp_tag), {
                    let p = acp_proxy.clone();
                    move || { let _ = p.send_event(AppEvent::AcpWake); }
                }).ok();
                // Reset sidebar status to "connecting"
                let _ = sidebar_wv.evaluate_script(
                    "window.__setConnecting && window.__setConnecting()"
                );
                // Update the chip label in the sidebar
                let escaped = webview_utils::escape_js_template(&tag);
                let _ = sidebar_wv.evaluate_script(&format!(
                "window.__setAgentTag && window.__setAgentTag(`{escaped}`)"
            ));
            }

            // ── ACP new session (same agent, fresh chat) ────────────────────────
            Event::UserEvent(AppEvent::AcpNewSession) => {
                acp_retry_count = 0; // reset retry count on new session
                acp_reconnect_gen += 1; // invalidate any pending backoff timers
                acp_handle = None; // drop old handle (kills subprocess)
                acp_handle = acp::AcpHandle::connect(&format!("octomind acp {}", acp_tag), {
                    let p = acp_proxy.clone();
                    move || { let _ = p.send_event(AppEvent::AcpWake); }
                }).ok();
                let _ = sidebar_wv.evaluate_script(
                    "window.__setConnecting && window.__setConnecting()"
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

            // ── ACP reconnection attempt (scheduled after error) ──
            Event::UserEvent(AppEvent::AcpReconnect(gen)) => {
                // Ignore stale timers from a previous error cycle — the user may
                // have manually reconnected (AcpRestart / AcpNewSession) in the
                // meantime, which already bumped acp_reconnect_gen.
                if gen != acp_reconnect_gen {
                    tracing::debug!(gen, current = acp_reconnect_gen, "ignoring stale ACP reconnect timer");
                } else {
                    tracing::info!(retry = acp_retry_count, "attempting ACP reconnection");
                    acp_handle = None; // drop old handle if any
                    acp_handle = acp::AcpHandle::connect(&format!("octomind acp {}", acp_tag), {
                        let p = acp_proxy.clone();
                        move || { let _ = p.send_event(AppEvent::AcpWake); }
                    }).ok();
                }
            }

            // ── Dismiss notification toast ─────────────────────────────────────
            Event::UserEvent(AppEvent::DismissNotification) => {
                if notification_visible {
                    let _ = notification_wv.evaluate_script(
                        "window.__hide && window.__hide()"
                    );
                    let _ = notification_wv.set_visible(false);
                    notification_visible = false;
                }
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
            Event::UserEvent(AppEvent::HideFindBar) => {
                if find_bar_visible {
                    let _ = find_bar_wv.set_visible(false);
                    let _ = find_bar_wv.evaluate_script("window.__clear && window.__clear()");
                    find_bar_visible = false;
                    find_bar_hotkey_visible.store(false, Ordering::Relaxed);
                    if let Some(wv) = tab_webviews.get(&active_wv_id) {
                        let _ = wv.evaluate_script("window.__findClear && window.__findClear()");
                    }
                    browser_win.set_focus();
                }
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
            Event::UserEvent(AppEvent::FindCount(current, total)) => {
                if find_bar_visible {
                    let _ = find_bar_wv.evaluate_script(&format!(
                        "window.__setCount && window.__setCount({current}, {total})"
                    ));
                }
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

            // ── Quit ──────────────────────────────────────────────────────
            Event::UserEvent(AppEvent::Quit) => {
                crash_report::log_exit_trigger("Quit");
                save_and_exit(&tabs, &favicon_cache, &prompt_history, control_flow);
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
                    // Dismiss inline edit — selection becomes stale on navigation
                    if inline_edit_visible {
                        let _ = inline_edit_wv.set_visible(false);
                        let _ = inline_edit_wv.evaluate_script("window.__clear && window.__clear()");
                        inline_edit_visible = false;
                        if let Some(ref h) = inline_edit_acp { h.cancel(); }
                        inline_edit_acp = None;
                        inline_edit_response.clear();
                    }
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
            }

            Event::UserEvent(AppEvent::NavigationError(tab_id, url, error)) => {
                // External scheme that slipped through → try opening with macOS
                if url::is_external_scheme(&url) {
                    macos::open_external_url(&url);
                    browser_win.set_focus();
                    return;
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
                let url = tabs.lock().unwrap().tabs().iter()
                    .find(|t| t.id == tab_id)
                    .map(|t| t.url.clone())
                    .unwrap_or_default();
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
                    inline_edit_hotkey_visible.store(false, Ordering::Relaxed);
                    if let Some(ref h) = inline_edit_acp { h.cancel(); }
                    inline_edit_acp = None;
                    inline_edit_response.clear();
                    browser_win.set_focus();
                } else if let Some(wv) = tab_webviews.get(&active_wv_id) {
                    let _ = wv.evaluate_script("window.__inlineEditCapture && window.__inlineEditCapture()");
                }
            }
            Event::UserEvent(AppEvent::InlineEditReady(text, x, y)) => {
                inline_edit_selected_text = text;
                inline_edit_tab_id = active_wv_id;
                // Position at cursor — coordinates are CSS pixels from tab viewport
                let scale = browser_win.scale_factor();
                let sz = browser_win.inner_size();
                let mut px = (x * scale) as u32;
                let mut py = (y * scale) as u32 + address_bar_h;
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
                unsafe {
                    use objc2::msg_send;
                    use objc2::runtime::AnyObject;
                    let ns_win: *mut AnyObject = chrome_win.ns_window() as *mut AnyObject;
                    let _: () = msg_send![ns_win, makeKeyWindow];
                }
                let _ = inline_edit_wv.focus();
                let _ = inline_edit_wv.evaluate_script("window.__focus && window.__focus()");
                // Inject prompt history into the modal JS
                let hist_json = serde_json::to_string(&prompt_history).unwrap_or_else(|_| "[]".into());
                let _ = inline_edit_wv.evaluate_script(&format!(
                    "window.__setHistory && window.__setHistory({hist_json})"
                ));
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
                    inline_edit_hotkey_visible.store(false, Ordering::Relaxed);
                    if let Some(wv) = tab_webviews.get(&inline_edit_tab_id) {
                        let _ = wv.evaluate_script(
                            "document.documentElement.style.cursor='wait'"
                        );
                    }
                    browser_win.set_focus();
                }
            }
            Event::UserEvent(AppEvent::InlineEditHide) => {
                if inline_edit_visible {
                    let _ = inline_edit_wv.set_visible(false);
                    inline_edit_visible = false;
                    inline_edit_hotkey_visible.store(false, Ordering::Relaxed);
                    // Set loading cursor on the target tab while processing continues
                    if let Some(wv) = tab_webviews.get(&inline_edit_tab_id) {
                        let _ = wv.evaluate_script(
                            "document.documentElement.style.cursor='wait'"
                        );
                    }
                    browser_win.set_focus();
                }
            }
            Event::UserEvent(AppEvent::InlineEditResize(h)) => {
                if inline_edit_visible {
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
            }
            Event::UserEvent(AppEvent::InlineEditClose) => {
                if inline_edit_visible {
                    let _ = inline_edit_wv.set_visible(false);
                    let _ = inline_edit_wv.evaluate_script("window.__clear && window.__clear()");
                    inline_edit_visible = false;
                    inline_edit_hotkey_visible.store(false, Ordering::Relaxed);
                    if let Some(ref h) = inline_edit_acp { h.cancel(); }
                    inline_edit_acp = None;
                    inline_edit_response.clear();
                    browser_win.set_focus();
                }
            }

            // ── Reload current page ───────────────────────────────────────────
            Event::UserEvent(AppEvent::Reload) => {
                if let Some(wv) = tab_webviews.get(&active_wv_id) {
                    let _ = wv.reload();
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
            Event::UserEvent(AppEvent::SnapshotCaptured(tab_id, data_uri)) => {
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
                        save_and_exit(&tabs, &favicon_cache, &prompt_history, control_flow);
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
                    // Keep chrome overlay window in sync with browser window size
                    chrome_win.set_inner_size(*sz);
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
                    if sidebar_visible {
                        let _ = sidebar_wv.set_bounds(wry::Rect {
                            position: tao::dpi::PhysicalPosition::new(
                                sz.width.saturating_sub(sidebar_w),
                                0u32,
                            ).into(),
                            size: tao::dpi::PhysicalSize::new(sidebar_w, sz.height).into(),
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
                    // Resize address bar to full width
                    let _ = address_bar_wv.set_bounds(wry::Rect {
                        position: tao::dpi::PhysicalPosition::new(0u32, 0u32).into(),
                        size: tao::dpi::PhysicalSize::new(sz.width, address_bar_h).into(),
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

                WindowEvent::ModifiersChanged(mods) => {
                    modifiers = *mods;
                }

                WindowEvent::Focused(focused) if window_id == browser_win_id || window_id == chrome_win_id || window_id == overlay_win_id => {
                    if *focused {
                        app_focused.store(true, Ordering::Relaxed);
                    } else {
                        // Only mark unfocused if NO app window has focus.
                        // When clicking between windows, one gains focus before
                        // the other loses it, so we defer the check.
                        let bf = browser_win.is_focused();
                        let cf = chrome_win.is_focused();
                        let of = overlay_win.is_focused();
                        if !bf && !cf && !of {
                            app_focused.store(false, Ordering::Relaxed);
                        }
                    }
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
/// status, which sets app_focused=false and breaks all keybindings.
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

/// Convert raw PDF NSData into a stitched NSImage (all pages vertically).
/// Returns null on failure. Caller must use the returned pointer before it's released.
unsafe fn pdf_data_to_nsimage(
    pdf_data: *mut objc2::runtime::AnyObject,
) -> *mut objc2::runtime::AnyObject {
    use objc2_core_foundation::{CGPoint, CGRect, CGSize};

    let pdf_doc: *mut objc2::runtime::AnyObject =
        objc2::msg_send![objc2::class!(PDFDocument), alloc];
    let pdf_doc: *mut objc2::runtime::AnyObject = objc2::msg_send![
        pdf_doc,
        initWithData: &*pdf_data
    ];
    if pdf_doc.is_null() {
        return std::ptr::null_mut();
    }

    let page_count: usize = objc2::msg_send![&*pdf_doc, pageCount];
    if page_count == 0 {
        return std::ptr::null_mut();
    }

    let screen: *mut objc2::runtime::AnyObject =
        objc2::msg_send![objc2::class!(NSScreen), mainScreen];
    let scale: f64 = if screen.is_null() {
        2.0
    } else {
        objc2::msg_send![&*screen, backingScaleFactor]
    };

    let mut total_height: f64 = 0.0;
    let mut max_width: f64 = 0.0;
    for i in 0..page_count {
        let page: *mut objc2::runtime::AnyObject = objc2::msg_send![&*pdf_doc, pageAtIndex: i];
        let bounds: CGRect = objc2::msg_send![&*page, boundsForBox: 0isize];
        total_height += bounds.size.height;
        if bounds.size.width > max_width {
            max_width = bounds.size.width;
        }
    }

    let px_w = (max_width * scale).ceil();
    let px_h = (total_height * scale).ceil();
    let size = CGSize {
        width: px_w,
        height: px_h,
    };

    let final_image: *mut objc2::runtime::AnyObject =
        objc2::msg_send![objc2::class!(NSImage), alloc];
    let final_image: *mut objc2::runtime::AnyObject = objc2::msg_send![
        final_image, initWithSize: size
    ];

    let _: () = objc2::msg_send![&*final_image, lockFocus];

    let white: *mut objc2::runtime::AnyObject =
        objc2::msg_send![objc2::class!(NSColor), whiteColor];
    let _: () = objc2::msg_send![&*white, setFill];
    let full_rect = CGRect {
        origin: CGPoint { x: 0.0, y: 0.0 },
        size,
    };
    let _: () = objc2_app_kit::NSRectFill(full_rect);

    let mut y_offset = px_h;
    for i in 0..page_count {
        let page: *mut objc2::runtime::AnyObject = objc2::msg_send![&*pdf_doc, pageAtIndex: i];
        let bounds: CGRect = objc2::msg_send![&*page, boundsForBox: 0isize];
        let page_px_w = bounds.size.width * scale;
        let page_px_h = bounds.size.height * scale;
        y_offset -= page_px_h;

        let thumb_size = CGSize {
            width: page_px_w,
            height: page_px_h,
        };
        let thumb: *mut objc2::runtime::AnyObject = objc2::msg_send![
            &*page,
            thumbnailOfSize: thumb_size,
            forBox: 0isize
        ];
        if thumb.is_null() {
            continue;
        }

        let dest_rect = CGRect {
            origin: CGPoint {
                x: 0.0,
                y: y_offset,
            },
            size: CGSize {
                width: page_px_w,
                height: page_px_h,
            },
        };
        let _: () = objc2::msg_send![
            &*thumb,
            drawInRect: dest_rect,
            fromRect: CGRect {
                origin: CGPoint { x: 0.0, y: 0.0 },
                size: CGSize { width: 0.0, height: 0.0 },
            },
            operation: 2u64,
            fraction: 1.0f64
        ];
    }

    let _: () = objc2::msg_send![&*final_image, unlockFocus];
    final_image
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
                    let final_image = pdf_data_to_nsimage(pdf_data);
                    if final_image.is_null() {
                        tracing::error!("Full page screenshot: failed to render PDF to image");
                    } else {
                        let png_data = nsimage_to_png_data(final_image);
                        if png_data.is_null() {
                            tracing::error!("Full page screenshot: failed to encode PNG");
                        } else {
                            copy_png_to_clipboard(png_data);
                            tracing::debug!("Full page screenshot copied to clipboard");
                        }
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

/// Save session state and exit the event loop.
fn save_and_exit(
    tabs: &Arc<Mutex<browser::TabManager>>,
    favicon_cache: &std::collections::HashMap<String, String>,
    prompt_history: &[String],
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
    drop(tm);
    crash_report::log_clean_shutdown();
    *control_flow = ControlFlow::Exit;
}
