mod acp;
mod browser;
mod config;
mod error_page_html;
mod macos;
mod mcp;
mod nav_error_patch;
mod newtab_html;
mod notification_html;
mod overlay_html;
mod progress_bar_html;
mod quickslots;
mod quickslots_html;
mod sidebar_html;
mod toggle_btn_html;
mod url;
mod webview_utils;

use std::collections::HashMap;
use std::sync::{
    atomic::{AtomicBool, Ordering},
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
    PrevTab,                                // Ctrl+P — switch to previous tab in MRU order
    NextTab,                                // Ctrl+N — switch to next tab in MRU order
    ToggleSidebar,                          // Cmd+Shift+A — toggle AI assistant sidebar
    AcpPrompt(String),                      // user typed a prompt in the sidebar
    AcpCancel,                              // user clicked stop button — cancel current prompt
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
    RemoveHistory(String), // URL to remove from history
    QuickSlotOpen(usize), // ⌘1–⌘0 — open saved URL in slot 0–9
    QuickSlotSave(usize), // ⌘⇧1–⌘⇧0 — save current page to slot 0–9
    QuickSlotRemove(usize), // remove slot (from footer bar ✕ or newtab page)
    AcpWake,            // lightweight wake — ACP thread pokes event loop
    DownloadStarted(usize), // (tab_id) — navigation became a download, close the tab
    DownloadCompleted(String, bool), // (filename, success) — show notification toast
    DismissNotification, // user clicked X on notification toast
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

    // Import user's full shell environment (PATH, API keys, etc.) for .app context.
    macos::init_env();

    let cfg = Config::load();
    let tabs = Arc::new(Mutex::new(TabManager::new(cfg.max_history)));

    let event_loop: EventLoop<AppEvent> = EventLoopBuilder::with_user_event().build();
    let proxy = event_loop.create_proxy();
    let overlay_hotkey_visible = Arc::new(AtomicBool::new(false));
    // Tracks whether the browser window is the frontmost app.
    // CGEventTap fires system-wide, so we gate all hotkeys on this flag.
    let app_focused = Arc::new(AtomicBool::new(false));

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

    let overlay_win = Arc::new(overlay_win);
    let _overlay_win_id = overlay_win.id();

    // ── Chrome window — borderless transparent layer for persistent UI ────
    // Floats above browser_win; holds sidebar, footer, toggle button, progress bar.
    // Transparent areas pass clicks through to browser_win underneath.
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

    let make_webview = {
        let browser_win = Arc::clone(&browser_win);
        let proxy = proxy.clone();
        move |tab_id: usize, url: &str| -> WebView {
            let p1 = proxy.clone();
            let p2 = proxy.clone();
            let p3 = proxy.clone();
            let p4 = proxy.clone();
            let p5 = proxy.clone();
            let p6 = proxy.clone();
            let sz = browser_win.inner_size();
            let bounds = wry::Rect {
                position: tao::dpi::PhysicalPosition::new(0u32, 0u32).into(),
                size: tao::dpi::PhysicalSize::new(sz.width, sz.height).into(),
            };
            WebViewBuilder::new()
                .with_url(url)
                .with_back_forward_navigation_gestures(true)
                .with_bounds(bounds)
                // Suspend JS timers, rAF, and network on hidden tabs (macOS 14+, no-op on older)
                .with_background_throttling(BackgroundThrottlingPolicy::Suspend)
                // Safari-compatible UA so sites serve optimised WebKit assets; octoweb tag for identification
                .with_user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.3 Safari/605.1.15 Octoweb/1.0")
                // Fetch and cache favicon as base64 data-URI on every page load.
                // Tries <link rel="icon"> first (highest quality), falls back to /favicon.ico.
                // Posts IPC only once per domain per session (deduplication in Rust).
                .with_initialization_script(webview_utils::FAVICON_FETCH_SCRIPT)
                // Track audio/video playback state and notify Rust via IPC.
                .with_initialization_script(webview_utils::MEDIA_TRACK_SCRIPT)
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
                            _ => {}
                        }
                    }
                })
                .with_on_page_load_handler(move |event, url| {
                    use wry::PageLoadEvent;
                    match event {
                        PageLoadEvent::Started => {
                            let _ = p1.send_event(AppEvent::PageLoadStarted(tab_id));
                        }
                        PageLoadEvent::Finished => {
                            let _ = p1.send_event(AppEvent::BrowserUrlChanged(tab_id, url.to_string()));
                            let _ = p1.send_event(AppEvent::PageLoadFinished(tab_id));
                        }
                    }
                })
                .with_document_title_changed_handler(move |title| {
                    let _ = p2.send_event(AppEvent::TitleChanged(tab_id, title));
                })
                .with_new_window_req_handler(move |url, _features| {
                    // Cmd+click or target=_blank — open in a new tab instead of a new window.
                    let _ = p4.send_event(AppEvent::OpenInNewTab(url));
                    wry::NewWindowResponse::Deny
                })
                .with_download_started_handler(move |_url, _path| {
                    // Navigation became a download — tell main loop to close the tab
                    let _ = p5.send_event(AppEvent::DownloadStarted(tab_id));
                    true // allow the download
                })
                .with_download_completed_handler(move |_url, path, success| {
                    let filename = path
                        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().into_owned()))
                        .unwrap_or_default();
                    let _ = p6.send_event(AppEvent::DownloadCompleted(filename, success));
                })
                .build_as_child(&*browser_win)
                .expect("Failed to create tab WebView")
        }
    };

    let mut tab_webviews: HashMap<usize, WebView> = HashMap::new();
    let mut active_wv_id;
    let mut mru: Vec<usize> = Vec::new();
    // Deferred tab swap: (old_visible_tab, new_loading_tab).
    // Old tab stays visible while new one loads behind it (Safari-style).
    let mut pending_swap: Option<(usize, usize)> = None;

    // Favicon cache: domain → base64 data-URI, persisted across sessions.
    let mut favicon_cache: HashMap<String, String> = config::load_favicons();
    let mut quick_slots = quickslots::load();

    // Restore previous session if available, otherwise open home page.
    let session = config::load_session();
    let urls: Vec<String> = match &session {
        Some(s) if !s.tabs.is_empty() => s.tabs.clone(),
        _ => vec![home.clone()],
    };
    let active_url = session
        .as_ref()
        .map(|s| s.active_url.as_str())
        .unwrap_or(&home)
        .to_string();

    let mut first_id: Option<usize> = None;
    let mut restored_active_id: Option<usize> = None;
    for url in &urls {
        let tab_id = tabs.lock().unwrap().open(url.clone());
        let wv = make_webview(tab_id, url);
        // For about:blank, load the styled new-tab page
        if url == "about:blank" {
            let html = newtab_html::html(&quickslots::to_json(&quick_slots));
            let _ = wv.load_html(&html);
        }
        // Register ObjC error callback for this WebView
        let wv_ptr = objc2::rc::Retained::as_ptr(&wv.webview()) as usize;
        // Inject error methods into WryNavigationDelegate (only runs once)
        nav_error_patch::inject_from_webview(wv_ptr);
        let p = proxy.clone();
        nav_error_patch::register(wv_ptr, move |url, code| {
            let _ = p.send_event(AppEvent::NavigationError(tab_id, url, code.to_string()));
        });
        // Hide all tabs initially; we'll show the active one below.
        let _ = wv.set_visible(false);
        tab_webviews.insert(tab_id, wv);
        mru.push(tab_id);
        if first_id.is_none() {
            first_id = Some(tab_id);
        }
        if url == &active_url {
            restored_active_id = Some(tab_id);
        }
    }

    // Show the active tab (prefer matched URL, fallback to first).
    active_wv_id = restored_active_id.or(first_id).unwrap();
    if let Some(wv) = tab_webviews.get(&active_wv_id) {
        let _ = wv.set_visible(true);
    }
    tabs.lock().unwrap().switch(active_wv_id);
    macos::mru_push(&mut mru, active_wv_id);

    // ── Overlay WebView ───────────────────────────────────────────────────
    let overlay_wv = WebViewBuilder::new()
        .with_html(overlay_html::html())
        .with_transparent(true)
        .with_ipc_handler({
            let p = proxy.clone();
            let ow = Arc::clone(&overlay_win);
            let overlay_state = Arc::clone(&overlay_hotkey_visible);
            move |msg| {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(msg.body()) {
                    match v["type"].as_str() {
                        Some("overlay_open") => {
                            overlay_state.store(true, Ordering::Relaxed);
                        }
                        Some("overlay_close") | Some("close") => {
                            ow.set_visible(false);
                            overlay_state.store(false, Ordering::Relaxed);
                            let _ = p.send_event(AppEvent::HideOverlay);
                        }
                        Some("navigate") => {
                            ow.set_visible(false);
                            overlay_state.store(false, Ordering::Relaxed);
                            if let Some(url) = v["url"].as_str() {
                                let _ = p.send_event(AppEvent::NavigateTo(url.to_string()));
                            }
                        }
                        Some("switch_tab") => {
                            ow.set_visible(false);
                            overlay_state.store(false, Ordering::Relaxed);
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
                            ow.set_visible(false);
                            overlay_state.store(false, Ordering::Relaxed);
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

    // ── Sidebar WebView (child of browser_win, right-edge panel) ──────────
    // Hidden by default; shown/hidden via ToggleSidebar.
    // SIDEBAR_W is in logical points; scale to physical pixels for bounds arithmetic.
    const SIDEBAR_W_LOGICAL: f64 = 440.0;
    let sidebar_w = (SIDEBAR_W_LOGICAL * browser_win.scale_factor()) as u32;
    let sz0 = browser_win.inner_size();

    // Toggle button: 44×44 logical pt pill in the top-right corner, 12pt margin.
    const BTN_SIZE_LOGICAL: f64 = 44.0;
    const BTN_MARGIN_LOGICAL: f64 = 12.0;
    let btn_size = (BTN_SIZE_LOGICAL * browser_win.scale_factor()) as u32;
    let btn_margin = (BTN_MARGIN_LOGICAL * browser_win.scale_factor()) as u32;

    let toggle_btn_wv = WebViewBuilder::new()
        .with_html(toggle_btn_html::html())
        .with_transparent(true)
        .with_bounds(wry::Rect {
            position: tao::dpi::PhysicalPosition::new(
                sz0.width.saturating_sub(btn_size + btn_margin),
                btn_margin,
            )
            .into(),
            size: tao::dpi::PhysicalSize::new(btn_size, btn_size).into(),
        })
        .with_ipc_handler({
            let p = proxy.clone();
            move |msg| {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(msg.body()) {
                    if v["type"].as_str() == Some("toggle_sidebar") {
                        let _ = p.send_event(AppEvent::ToggleSidebar);
                    }
                }
            }
        })
        .build_as_child(&*chrome_win)
        .expect("Failed to create toggle button WebView");
    let sidebar_wv = WebViewBuilder::new()
        .with_html(sidebar_html::html())
        .with_transparent(true)
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
                            if let Some(text) = v["text"].as_str() {
                                let _ = p.send_event(AppEvent::AcpPrompt(text.to_string()));
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

    // ── Progress bar WebView (thin bar at top during page load) ───────────
    let progress_wv = WebViewBuilder::new()
        .with_html(progress_bar_html::html())
        .with_transparent(true)
        .with_bounds(wry::Rect {
            position: tao::dpi::PhysicalPosition::new(0u32, 0u32).into(),
            size: tao::dpi::PhysicalSize::new(sz0.width, 3u32).into(),
        })
        .build_as_child(&*chrome_win)
        .expect("Failed to create progress WebView");
    let _ = progress_wv.set_visible(false);

    let mut progress_visible = false;
    // Instant when __finish was called — we hide progress_wv after the CSS fade (400ms)
    let mut progress_hide_at: Option<std::time::Instant> = None;

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
                sz0.width.saturating_sub(notif_w + btn_margin),
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

    // ── Quick-slots footer bar (thin strip at bottom of browser window) ───
    const FOOTER_H_LOGICAL: f64 = 36.0;
    let footer_h = (FOOTER_H_LOGICAL * browser_win.scale_factor()) as u32;
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
        .build_as_child(&*chrome_win)
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

    // ── MCP server — exposes browser control tools on localhost:3434 ───────
    let mut mcp_handle = Some(mcp::spawn_mcp_server());

    // ── Global hotkey via CGEventTap ─────────────────────────────────────
    // rdev crashes on macOS 15+ because TSMGetInputSourceProperty (called in
    // rdev's raw_callback) asserts it must run on the main thread. We use
    // CGEventTap directly — it runs on the main CFRunLoop, no extra thread.
    let _tap = {
        use core_foundation::runloop::{kCFRunLoopCommonModes, CFRunLoop};
        use core_graphics::event::{
            CGEventTap, CGEventTapLocation, CGEventTapOptions, CGEventTapPlacement, CGEventType,
            CallbackResult,
        };

        let p = proxy.clone();
        let overlay_state = Arc::clone(&overlay_hotkey_visible);
        let focused_state = Arc::clone(&app_focused);
        // keyCode 40 = k, flagsChanged catches modifier-only events separately.
        // We check the CGEventFlags for Command inside the callback.
        let tap = CGEventTap::new(
            CGEventTapLocation::HID,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::Default, // active tap — lets us consume specific keys
            vec![CGEventType::KeyDown],
            move |_proxy, _etype, event| {
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
                const R_KEYCODE: i64 = 15; // r (Cmd+R = reload)
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
                } else if cmd && shift && keycode == I_KEYCODE {
                    let _ = p.send_event(AppEvent::ToggleDevTools);
                    CallbackResult::Drop
                } else if cmd && keycode == W_KEYCODE && !overlay_state.load(Ordering::Relaxed) {
                    let _ = p.send_event(AppEvent::CloseTab(0));
                    CallbackResult::Drop
                } else if cmd && keycode == Q_KEYCODE {
                    let _ = p.send_event(AppEvent::Quit);
                    CallbackResult::Drop
                } else if ctrl && keycode == P_KEYCODE && !overlay_state.load(Ordering::Relaxed) {
                    let _ = p.send_event(AppEvent::PrevTab);
                    CallbackResult::Drop
                } else if ctrl && keycode == N_KEYCODE && !overlay_state.load(Ordering::Relaxed) {
                    let _ = p.send_event(AppEvent::NextTab);
                    CallbackResult::Drop
                } else if cmd && keycode == R_KEYCODE && !overlay_state.load(Ordering::Relaxed) {
                    let _ = p.send_event(AppEvent::Reload);
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
    let mut icon_set = false;

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
            tab_webviews.insert(id, wv);
            id
        }};
    }

    /// Switch visibility from the current active tab to `target`, handling pending_swap.
    macro_rules! switch_visible_tab {
        ($target:expr) => {{
            let target = $target;
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
            active_wv_id = target;
        }};
    }

    /// Refresh the overlay item list (if visible).
    macro_rules! refresh_overlay {
        () => {
            if overlay_visible {
                let json = {
                    let mut tm = tabs.lock().unwrap();
                    let vc = tm.visit_counts();
                    tm.ensure_contiguous();
                    webview_utils::build_items_json(tm.tabs(), tm.history(), &vc, &favicon_cache)
                };
                let _ = overlay_wv.evaluate_script(&format!(
                    "window.__refreshItems && window.__refreshItems({json})"
                ));
            }
        };
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
        // Poll mode when sidebar is open (ACP events) or progress bar is fading out.
        // Wait mode otherwise to avoid burning CPU while idle.
        *control_flow = if sidebar_visible || progress_hide_at.is_some() {
            ControlFlow::Poll
        } else {
            ControlFlow::Wait
        };

        // Hide progress bar after CSS fade completes
        if let Some(hide_at) = progress_hide_at {
            if std::time::Instant::now() >= hide_at {
                let _ = progress_wv.set_visible(false);
                progress_visible = false;
                progress_hide_at = None;
            }
        }

        // Set dock icon and app menu once — must happen after tao has initialized NSApplication.
        if !icon_set {
            macos::set_app_icon();
            macos::setup_edit_menu();
            icon_set = true;
        }

        // Drain ACP events and forward to the UI on every tick.
        if let Some(ref mut handle) = acp_handle {
            for ev in handle.poll() {
                match ev {
                    acp::AgentEvent::Connected => {
                        let _ = sidebar_wv.evaluate_script(
                            "window.__setConnected && window.__setConnected()"
                        );
                    }
                    acp::AgentEvent::Chunk(chunk) => {
                        let escaped = webview_utils::escape_js_template(&chunk);
                        let _ = sidebar_wv.evaluate_script(&format!(
                        "window.__appendChunk && window.__appendChunk(`{escaped}`)"
                    ));
                        // Show badge + notification toast when sidebar is hidden
                        if !sidebar_visible {
                            let _ = toggle_btn_wv.evaluate_script(
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
                    acp::AgentEvent::ToolStart { id, title, kind } => {
                        let eid = webview_utils::escape_js_template(&id);
                        let etitle = webview_utils::escape_js_template(&title);
                        let ekind = webview_utils::escape_js_template(&kind);
                        let _ = sidebar_wv.evaluate_script(&format!(
                            "window.__toolStart && window.__toolStart(`{eid}`,`{etitle}`,`{ekind}`)"
                        ));
                    }
                    acp::AgentEvent::ToolUpdate { id, title, status } => {
                        let eid = webview_utils::escape_js_template(&id);
                        let etitle = webview_utils::escape_js_template(title.as_deref().unwrap_or(""));
                        let estatus = webview_utils::escape_js_template(&status);
                        let _ = sidebar_wv.evaluate_script(&format!(
                            "window.__toolUpdate && window.__toolUpdate(`{eid}`,`{etitle}`,`{estatus}`)"
                        ));
                    }
                    acp::AgentEvent::Done => {
                        let _ = sidebar_wv.evaluate_script(
                            "window.__setThinking && window.__setThinking(false)"
                        );
                        if !sidebar_visible {
                            let _ = toggle_btn_wv.evaluate_script(
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
                        let escaped = webview_utils::escape_js_template(&err);
                        let _ = sidebar_wv.evaluate_script(&format!(
                        "window.__appendError && window.__appendError(`{escaped}`)"
                    ));
                        if !sidebar_visible {
                            let _ = toggle_btn_wv.evaluate_script(
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

        // Drain MCP commands and execute on main thread (WebView is not thread-safe).
        if let Some(ref mut handle) = mcp_handle {
            while let Some(cmd) = handle.poll() {
                tracing::debug!(cmd = ?std::mem::discriminant(&cmd), "MCP command received");

                match cmd {
                    McpCommand::Navigate { url, tab_id, new_tab, background, response } => {
                        tracing::debug!(url = %url, ?tab_id, new_tab, background, "MCP navigate");

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
                                tabs.lock().unwrap().update_url(target_id, resolved);
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
                    McpCommand::Screenshot { tab_id, response } => {
                        let target_id = tab_id.unwrap_or(active_wv_id);
                        if let Some(wv) = tab_webviews.get(&target_id) {
                            let wv_ptr = objc2::rc::Retained::as_ptr(&wv.webview()) as usize;
                            let response = std::sync::Arc::new(std::sync::Mutex::new(Some(response)));
                            let response_cb = response.clone();

                            // WKWebView.takeSnapshot(with:completionHandler:)
                            // completionHandler: ^(NSImage * _Nullable image, NSError * _Nullable error)
                            let handler = block2::RcBlock::new(move |image: *mut objc2::runtime::AnyObject, error: *mut objc2::runtime::AnyObject| {
                                let Some(tx) = response_cb.lock().unwrap().take() else { return };

                                if image.is_null() {
                                    let msg = if !error.is_null() {
                                        unsafe {
                                            let desc: *mut objc2::runtime::AnyObject = objc2::msg_send![&*error, localizedDescription];
                                            if desc.is_null() {
                                                "Screenshot failed".to_string()
                                            } else {
                                                let bytes: *const u8 = objc2::msg_send![&*desc, UTF8String];
                                                std::ffi::CStr::from_ptr(bytes.cast()).to_string_lossy().into_owned()
                                            }
                                        }
                                    } else {
                                        "Screenshot returned nil image".to_string()
                                    };
                                    let _ = tx.send(Err(msg));
                                    return;
                                }

                                unsafe {
                                    // NSImage → TIFF → NSBitmapImageRep → PNG data
                                    let tiff: *mut objc2::runtime::AnyObject = objc2::msg_send![&*image, TIFFRepresentation];
                                    if tiff.is_null() {
                                        let _ = tx.send(Err("Failed to get TIFF data".to_string()));
                                        return;
                                    }
                                    let rep: *mut objc2::runtime::AnyObject = objc2::msg_send![
                                        objc2::class!(NSBitmapImageRep),
                                        imageRepWithData: &*tiff
                                    ];
                                    if rep.is_null() {
                                        let _ = tx.send(Err("Failed to create bitmap rep".to_string()));
                                        return;
                                    }
                                    // NSBitmapImageFileTypePNG = 4
                                    let empty_dict: *mut objc2::runtime::AnyObject = objc2::msg_send![objc2::class!(NSDictionary), dictionary];
                                    let png_data: *mut objc2::runtime::AnyObject = objc2::msg_send![
                                        &*rep,
                                        representationUsingType: 4u64,
                                        properties: &*empty_dict
                                    ];
                                    if png_data.is_null() {
                                        let _ = tx.send(Err("Failed to encode PNG".to_string()));
                                        return;
                                    }

                                    // Get raw bytes from NSData
                                    let length: usize = objc2::msg_send![&*png_data, length];
                                    let bytes_ptr: *const u8 = objc2::msg_send![&*png_data, bytes];
                                    let png_bytes = std::slice::from_raw_parts(bytes_ptr, length);

                                    // Write to temp file
                                    let ts = std::time::SystemTime::now()
                                        .duration_since(std::time::UNIX_EPOCH)
                                        .unwrap_or_default()
                                        .as_millis();
                                    let path = std::env::temp_dir().join(format!("octoweb-screenshot-{ts}.png"));
                                    if let Err(e) = std::fs::write(&path, png_bytes) {
                                        let _ = tx.send(Err(format!("Failed to write file: {e}")));
                                        return;
                                    }

                                    // Copy to clipboard as PNG
                                    let pb: *mut objc2::runtime::AnyObject = objc2::msg_send![objc2::class!(NSPasteboard), generalPasteboard];
                                    let _: () = objc2::msg_send![&*pb, clearContents];
                                    let png_type: *mut objc2::runtime::AnyObject = objc2::msg_send![
                                        objc2::class!(NSString),
                                        stringWithUTF8String: c"public.png".as_ptr()
                                    ];
                                    let _: bool = objc2::msg_send![&*pb, setData: &*png_data, forType: &*png_type];

                                    let path_str = path.to_string_lossy().into_owned();
                                    tracing::debug!(path = %path_str, "Screenshot saved and copied to clipboard");
                                    let _ = tx.send(Ok(path_str));
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
                overlay_win.set_visible(false);
                overlay_visible = false;
                overlay_hotkey_visible.store(false, Ordering::Relaxed);
            }

            // ── Toggle overlay ────────────────────────────────────────────
            Event::UserEvent(AppEvent::ToggleOverlay) => {
                if overlay_visible {
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
                        let vc = tm.visit_counts();
                        tm.ensure_contiguous();
                        webview_utils::build_items_json(tm.tabs(), tm.history(), &vc, &favicon_cache)
                    };
                    let _ = overlay_wv.evaluate_script(&format!(
                    "window.__setItems && window.__setItems({json})"
                ));
                    overlay_win.set_visible(true);
                    overlay_win.set_focus();
                    overlay_visible = true;
                    overlay_hotkey_visible.store(true, Ordering::Relaxed);
                }
            }

            // ── Navigate: new tab with its own WebView ────────────────────
            Event::UserEvent(AppEvent::NavigateTo(raw)) => {
                overlay_visible = false;
                overlay_hotkey_visible.store(false, Ordering::Relaxed);
                let url = url::resolve_url(&raw, &search_engine);
                let tab_id = tabs.lock().unwrap().open(url.clone());
                // Keep the currently *visible* tab on screen while new one loads.
                // If a swap is already pending, the visible tab is the old one from that swap.
                let visible_id = pending_swap.map(|(old, _)| old).unwrap_or(active_wv_id);
                spawn_tab_webview!(tab_id, &url);
                active_wv_id = tab_id;
                pending_swap = Some((visible_id, tab_id));
                macos::mru_push(&mut mru, tab_id);
                browser_win.set_focus();
            }

            // ── Open in new tab: Cmd+click or target=_blank ───────────────
            Event::UserEvent(AppEvent::OpenInNewTab(url)) => {
                let tab_id = tabs.lock().unwrap().open(url.clone());
                // Keep the currently visible tab on screen while new one loads.
                let visible_id = pending_swap.map(|(old, _)| old).unwrap_or(active_wv_id);
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
                if tab_id == active_wv_id {
                    if app_focused.load(Ordering::Relaxed) { browser_win.set_focus(); }
                    return;
                }
                switch_visible_tab!(tab_id);
                macos::mru_push(&mut mru, tab_id);
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
                    let next_id = {
                        let mut tm = tabs.lock().unwrap();
                        tm.close(id);
                        tm.active_tab().map(|t| t.id)
                    };
                    if let Some(wv) = tab_webviews.get(&id) {
                        let wv_ptr = objc2::rc::Retained::as_ptr(&wv.webview()) as usize;
                        nav_error_patch::unregister(wv_ptr);
                    }
                    tab_webviews.remove(&id);
                    mru.retain(|&x| x != id);
                    match next_id {
                        Some(next) => {
                            if let Some(wv) = tab_webviews.get(&next) {
                                let _ = wv.set_visible(true);
                            }
                            active_wv_id = next;
                            macos::mru_push(&mut mru, next);
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
                tabs.lock().unwrap().remove_history(&url);
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
                    // Clear unread badge on toggle button
                    let _ = toggle_btn_wv.evaluate_script(
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
            Event::UserEvent(AppEvent::AcpPrompt(text)) => {
                if let Some(ref handle) = acp_handle {
                    handle.send_prompt(text);
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

            // ── Download started — close the tab that became a download ──────
            Event::UserEvent(AppEvent::DownloadStarted(tab_id)) => {
                tracing::debug!(tab_id, "Download started, closing download tab");
                let _ = proxy.send_event(AppEvent::CloseTab(tab_id));
            }

            // ── Download completed — show notification toast ─────────────────
            Event::UserEvent(AppEvent::DownloadCompleted(filename, success)) => {
                let msg = if success {
                    format!("Downloaded: {filename}")
                } else {
                    format!("Download failed: {filename}")
                };
                tracing::debug!(%msg, "Download completed");
                let escaped = webview_utils::escape_js_template(&msg);
                if !notification_visible {
                    let _ = notification_wv.set_visible(true);
                    notification_visible = true;
                }
                let _ = notification_wv.evaluate_script(&format!(
                    "window.__show && window.__show(`{escaped}`)"
                ));
            }

            // ── Quit ──────────────────────────────────────────────────────
            Event::UserEvent(AppEvent::Quit) => {
                save_and_exit(&tabs, &favicon_cache, control_flow);
            }

            // ── Title update ──────────────────────────────────────────────
            Event::UserEvent(AppEvent::TitleChanged(tab_id, title)) => {
                tabs.lock().unwrap().update_title(tab_id, title.clone());
                if tab_id == active_wv_id {
                    browser_win.set_title(&title);
                }
            }

            // ── URL update from page load ─────────────────────────────────
            Event::UserEvent(AppEvent::BrowserUrlChanged(tab_id, url)) => {
                tabs.lock().unwrap().update_url(tab_id, url);
            }

            // ── Favicon fetched from page — store in cache ────────────────
            // Only update + save when we get a new domain (avoids redundant disk writes).
            Event::UserEvent(AppEvent::FaviconFetched(domain, data_uri)) => {
                if favicon_cache.get(&domain).map(|s| s.as_str()) != Some(&data_uri) {
                    favicon_cache.insert(domain, data_uri);
                    config::save_favicons(&favicon_cache);
                }
            }

            // ── Page load progress ───────────────────────────────────────────
            // Only show progress for the active tab — background tabs load silently.
            Event::UserEvent(AppEvent::PageLoadStarted(tab_id)) => {
                if tab_id == active_wv_id {
                    // Cancel any pending hide — new load started
                    progress_hide_at = None;
                    let _ = progress_wv.evaluate_script("window.__start && window.__start()");
                    if !progress_visible {
                        let _ = progress_wv.set_visible(true);
                        progress_visible = true;
                    }
                }
            }

            Event::UserEvent(AppEvent::PageLoadFinished(tab_id)) => {
                if tab_id == active_wv_id && progress_visible {
                    let _ = progress_wv.evaluate_script("window.__finish && window.__finish()");
                    // Hide after CSS fade completes (width 0.2s + opacity 0.3s delay 0.1s = 600ms, +50ms buffer)
                    progress_hide_at = Some(std::time::Instant::now() + std::time::Duration::from_millis(650));
                }
                // Deferred swap: new page finished loading — show it, hide old tab.
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

            Event::UserEvent(AppEvent::NavigationError(tab_id, url, error)) => {
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

            // ── Reload current page ───────────────────────────────────────────
            Event::UserEvent(AppEvent::Reload) => {
                if let Some(wv) = tab_webviews.get(&active_wv_id) {
                    let _ = wv.reload();
                }
            }

            // ── Media playing state changed ───────────────────────────────────
            Event::UserEvent(AppEvent::MediaPlaying(tab_id, is_playing)) => {
                tabs.lock().unwrap().set_playing_audio(tab_id, is_playing);
            }

            // ── Window events ─────────────────────────────────────────────
            Event::WindowEvent {
                window_id,
                event: ref win_event,
                ..
            } => match win_event {
                WindowEvent::CloseRequested => {
                    if window_id == browser_win_id {
                        save_and_exit(&tabs, &favicon_cache, control_flow);
                    } else if window_id == chrome_win_id {
                        // Chrome window should not be closed independently — ignore.
                    } else {
                        overlay_win.set_visible(false);
                        overlay_visible = false;
                        overlay_hotkey_visible.store(false, Ordering::Relaxed);
                    }
                }

                WindowEvent::Resized(sz) if window_id == browser_win_id => {
                    // Keep chrome overlay window in sync with browser window size
                    chrome_win.set_inner_size(*sz);
                    if overlay_visible {
                        overlay_win.set_inner_size(*sz);
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
                    // Always reposition toggle button on resize
                    let _ = toggle_btn_wv.set_bounds(wry::Rect {
                        position: tao::dpi::PhysicalPosition::new(
                            sz.width.saturating_sub(btn_size + btn_margin),
                            btn_margin,
                        ).into(),
                        size: tao::dpi::PhysicalSize::new(btn_size, btn_size).into(),
                    });
                    // Resize progress bar width
                    let _ = progress_wv.set_bounds(wry::Rect {
                        position: tao::dpi::PhysicalPosition::new(0u32, 0u32).into(),
                        size: tao::dpi::PhysicalSize::new(sz.width, 3u32).into(),
                    });
                    // Reposition notification toast at top-right
                    let _ = notification_wv.set_bounds(wry::Rect {
                        position: tao::dpi::PhysicalPosition::new(
                            sz.width.saturating_sub(notif_w + btn_margin),
                            0u32,
                        ).into(),
                        size: tao::dpi::PhysicalSize::new(notif_w, notif_h).into(),
                    });
                    // Resize active tab to full width (sidebar overlays, doesn't shrink)
                    let bounds = wry::Rect {
                        position: tao::dpi::PhysicalPosition::new(0u32, 0u32).into(),
                        size: tao::dpi::PhysicalSize::new(sz.width, sz.height).into(),
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

                WindowEvent::Focused(focused) if window_id == browser_win_id || window_id == chrome_win_id => {
                    if *focused {
                        app_focused.store(true, Ordering::Relaxed);
                    } else {
                        // Only mark unfocused if NEITHER window has focus.
                        // When clicking between browser_win and chrome_win, one gains
                        // focus before the other loses it, so we defer the check.
                        let bf = browser_win.is_focused();
                        let cf = chrome_win.is_focused();
                        if !bf && !cf {
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

/// Save session state and exit the event loop.
fn save_and_exit(
    tabs: &Arc<Mutex<browser::TabManager>>,
    favicon_cache: &std::collections::HashMap<String, String>,
    control_flow: &mut ControlFlow,
) {
    let tm = tabs.lock().unwrap();
    let tab_urls: Vec<String> = tm.tabs().iter().map(|t| t.url.clone()).collect();
    let active_url = tm
        .active_tab()
        .map(|t| t.url.as_str())
        .unwrap_or("")
        .to_string();
    drop(tm);
    config::save_session(&tab_urls, &active_url);
    config::save_favicons(favicon_cache);
    *control_flow = ControlFlow::Exit;
}
