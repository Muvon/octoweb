mod acp;
mod browser;
mod config;
mod error_page_html;
mod macos;
mod mcp;
mod nav_error_patch;
mod overlay_html;
mod progress_bar_html;
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
    platform::macos::WindowBuilderExtMacOS,
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
    AcpRestart(String), // change agent tag (e.g. "octoweb:assistant") and reconnect
    ToggleDevTools,     // Cmd+Shift+I — open devtools for active tab
    OpenInNewTab(String), // Cmd+click / target=_blank — open URL in new tab and switch to it
    PageLoadStarted(usize), // (tab_id) — show progress bar
    PageLoadFinished(usize), // (tab_id) — hide progress bar
    NavigationError(usize, String, String), // (tab_id, url, error) — show error page
    Reload,             // Cmd+R — reload current page
    MediaPlaying(usize, bool), // (tab_id, is_playing) — audio/video state changed
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

    // ── Browser WebView factory ───────────────────────────────────────────
    // Each tab gets its own WebView (build_as_child). Hide/show to switch —
    // no reload, full state (scroll, video, JS) preserved.
    let home = cfg.home_page.clone();

    let make_webview = {
        let browser_win = Arc::clone(&browser_win);
        let proxy = proxy.clone();
        move |tab_id: usize, url: &str| -> WebView {
            let p1 = proxy.clone();
            let p2 = proxy.clone();
            let p3 = proxy.clone();
            let p4 = proxy.clone();
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
                .build_as_child(&*browser_win)
                .expect("Failed to create tab WebView")
        }
    };

    let mut tab_webviews: HashMap<usize, WebView> = HashMap::new();
    let mut active_wv_id;
    let mut mru: Vec<usize> = Vec::new();

    // Favicon cache: domain → base64 data-URI, persisted across sessions.
    let mut favicon_cache: HashMap<String, String> = config::load_favicons();

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
        .build_as_child(&*browser_win)
        .expect("Failed to create toggle button WebView");
    let sidebar_wv = WebViewBuilder::new()
        .with_html(sidebar_html::html())
        .with_transparent(true)
        .with_bounds(wry::Rect {
            position: tao::dpi::PhysicalPosition::new(sz0.width.saturating_sub(sidebar_w), 0u32)
                .into(),
            size: tao::dpi::PhysicalSize::new(sidebar_w, sz0.height).into(),
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
                        Some("sidebar_close") => {
                            let _ = p.send_event(AppEvent::ToggleSidebar);
                        }
                        Some("acp_set_agent") => {
                            if let Some(tag) = v["tag"].as_str() {
                                let _ = p.send_event(AppEvent::AcpRestart(tag.to_string()));
                            }
                        }
                        _ => {}
                    }
                }
            }
        })
        .build_as_child(&*browser_win)
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
        .build_as_child(&*browser_win)
        .expect("Failed to create progress WebView");
    let _ = progress_wv.set_visible(false);

    let mut progress_visible = false;
    // Instant when __finish was called — we hide progress_wv after the CSS fade (400ms)
    let mut progress_hide_at: Option<std::time::Instant> = None;

    // ── ACP handle — spawns octomind acp subprocess in background ─────────
    let mut acp_tag = "octoweb:assistant".to_string();
    let mut acp_handle = acp::AcpHandle::connect(&format!("octomind acp {}", acp_tag)).ok();

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
                        let escaped = chunk.replace('\\', "\\\\").replace('`', "\\`");
                        let _ = sidebar_wv.evaluate_script(&format!(
                        "window.__appendChunk && window.__appendChunk(`{escaped}`)"
                    ));
                    }
                    acp::AgentEvent::Done => {
                        let _ = sidebar_wv.evaluate_script(
                            "window.__setThinking && window.__setThinking(false)"
                        );
                    }
                    acp::AgentEvent::Error(err) => {
                        let escaped = err.replace('\\', "\\\\").replace('`', "\\`");
                        let _ = sidebar_wv.evaluate_script(&format!(
                        "window.__appendError && window.__appendError(`{escaped}`)"
                    ));
                    }
                }
            }
        }

        // Drain MCP commands and execute on main thread (WebView is not thread-safe).
        if let Some(ref mut handle) = mcp_handle {
            while let Some(cmd) = handle.poll() {
                tracing::debug!(cmd = ?std::mem::discriminant(&cmd), "MCP command received");

                match cmd {
                    McpCommand::Navigate { url, new_tab, response } => {
                        tracing::debug!(url = %url, new_tab, "MCP navigate");

                        if new_tab {
                            let _ = proxy.send_event(AppEvent::OpenInNewTab(url));
                        } else {
                            let _ = proxy.send_event(AppEvent::NavigateTo(url));
                        }
                        let _ = response.send(Ok(()));
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
                        let info = target_id.and_then(|id| {
                            tm.tabs().iter().find(|t| t.id == id).map(|t| PageInfo {
                                title: t.title.clone(),
                                url: t.url.clone(),
                                description: None, // TODO: extract from meta tag
                            })
                        });
                        let _ = response.send(info.ok_or("Tab not found".to_string()));
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
                        let tm = tabs.lock().unwrap();
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
                        let tm = tabs.lock().unwrap();
                        webview_utils::build_items_json(tm.tabs(), tm.history(), &tm, &favicon_cache)
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
                let url = url::resolve_url(&raw);
                let tab_id = tabs.lock().unwrap().open(url.clone());
                if let Some(wv) = tab_webviews.get(&active_wv_id) {
                    let _ = wv.set_visible(false);
                }
                let new_wv = make_webview(tab_id, &url);
                let wv_ptr = objc2::rc::Retained::as_ptr(&new_wv.webview()) as usize;
                let p = proxy.clone();
                nav_error_patch::register(wv_ptr, move |url, code| {
                    let _ = p.send_event(AppEvent::NavigationError(tab_id, url, code.to_string()));
                });
                tab_webviews.insert(tab_id, new_wv);
                active_wv_id = tab_id;
                macos::mru_push(&mut mru, tab_id);
                browser_win.set_focus();
            }

            // ── Open in new tab: Cmd+click or target=_blank ───────────────
            Event::UserEvent(AppEvent::OpenInNewTab(url)) => {
                let tab_id = tabs.lock().unwrap().open(url.clone());
                if let Some(wv) = tab_webviews.get(&active_wv_id) {
                    let _ = wv.set_visible(false);
                }
                let new_wv = make_webview(tab_id, &url);
                let wv_ptr = objc2::rc::Retained::as_ptr(&new_wv.webview()) as usize;
                let p = proxy.clone();
                nav_error_patch::register(wv_ptr, move |url, code| {
                    let _ = p.send_event(AppEvent::NavigationError(tab_id, url, code.to_string()));
                });
                tab_webviews.insert(tab_id, new_wv);
                active_wv_id = tab_id;
                macos::mru_push(&mut mru, tab_id);
                browser_win.set_focus();
            }

            // ── Switch tab: hide current, show target — no reload ─────────
            Event::UserEvent(AppEvent::SwitchTab(tab_id)) => {
                overlay_visible = false;
                overlay_hotkey_visible.store(false, Ordering::Relaxed);
                if tab_id == active_wv_id {
                    browser_win.set_focus();
                    return;
                }
                tabs.lock().unwrap().switch(tab_id);
                if let Some(wv) = tab_webviews.get(&active_wv_id) {
                    let _ = wv.set_visible(false);
                }
                if let Some(wv) = tab_webviews.get(&tab_id) {
                    let _ = wv.set_visible(true);
                }
                active_wv_id = tab_id;
                    macos::mru_push(&mut mru, tab_id);
                browser_win.set_focus();
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
                            browser_win.set_focus();
                        }
                        None => *control_flow = ControlFlow::Exit,
                    }
                }
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
                tabs.lock().unwrap().switch(target);
                if let Some(wv) = tab_webviews.get(&active_wv_id) {
                    let _ = wv.set_visible(false);
                }
                if let Some(wv) = tab_webviews.get(&target) {
                    let _ = wv.set_visible(true);
                }
                active_wv_id = target;
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
                tabs.lock().unwrap().switch(target);
                if let Some(wv) = tab_webviews.get(&active_wv_id) {
                    let _ = wv.set_visible(false);
                }
                if let Some(wv) = tab_webviews.get(&target) {
                    let _ = wv.set_visible(true);
                }
                active_wv_id = target;
                browser_win.set_focus();
            }

            // ── Toggle sidebar (Cmd+Shift+A or JS sidebar_close) ──────────
            Event::UserEvent(AppEvent::ToggleSidebar) => {
                let sz = browser_win.inner_size();
                if sidebar_visible {
                    let _ = sidebar_wv.set_visible(false);
                    sidebar_visible = false;
                    // Restore active tab to full width
                    let bounds = wry::Rect {
                        position: tao::dpi::PhysicalPosition::new(0u32, 0u32).into(),
                        size: tao::dpi::PhysicalSize::new(sz.width, sz.height).into(),
                    };
                    if let Some(wv) = tab_webviews.get(&active_wv_id) {
                        let _ = wv.set_bounds(bounds);
                    }
                    // Move toggle button back to right edge (no sidebar offset)
                    let _ = toggle_btn_wv.set_bounds(wry::Rect {
                        position: tao::dpi::PhysicalPosition::new(
                            sz.width.saturating_sub(btn_size + btn_margin),
                            btn_margin,
                        ).into(),
                        size: tao::dpi::PhysicalSize::new(btn_size, btn_size).into(),
                    });
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
                    // Focus the prompt input so the user can type immediately
                    let _ = sidebar_wv.evaluate_script(
                        "document.getElementById('prompt-input').focus()"
                    );
                    // Shrink active tab to leave room for sidebar
                    let tab_w = sz.width.saturating_sub(sidebar_w);
                    let bounds = wry::Rect {
                        position: tao::dpi::PhysicalPosition::new(0u32, 0u32).into(),
                        size: tao::dpi::PhysicalSize::new(tab_w, sz.height).into(),
                    };
                    if let Some(wv) = tab_webviews.get(&active_wv_id) {
                        let _ = wv.set_bounds(bounds);
                    }
                    // Move toggle button left of sidebar
                    let _ = toggle_btn_wv.set_bounds(wry::Rect {
                        position: tao::dpi::PhysicalPosition::new(
                            sz.width.saturating_sub(sidebar_w + btn_size + btn_margin),
                            btn_margin,
                        ).into(),
                        size: tao::dpi::PhysicalSize::new(btn_size, btn_size).into(),
                    });
                    // Connect ACP if not yet connected
                    if acp_handle.is_none() {
                        acp_handle = acp::AcpHandle::connect(&format!("octomind acp {}", acp_tag)).ok();
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

            // ── ACP restart with new agent command ─────────────────────────────
            Event::UserEvent(AppEvent::AcpRestart(tag)) => {
                acp_tag = tag.clone();
                acp_handle = None; // drop old handle (kills subprocess)
                acp_handle = acp::AcpHandle::connect(&format!("octomind acp {}", acp_tag)).ok();
                // Reset sidebar status to "connecting"
                let _ = sidebar_wv.evaluate_script(
                    "window.__setConnecting && window.__setConnecting()"
                );
                // Update the chip label in the sidebar
                let escaped = tag.replace('\\', "\\\\").replace('`', "\\`");
                let _ = sidebar_wv.evaluate_script(&format!(
                "window.__setAgentTag && window.__setAgentTag(`{escaped}`)"
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
                    } else {
                        overlay_win.set_visible(false);
                        overlay_visible = false;
                        overlay_hotkey_visible.store(false, Ordering::Relaxed);
                    }
                }

                WindowEvent::Resized(sz) if window_id == browser_win_id => {
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
                    let btn_x = if sidebar_visible {
                        sz.width.saturating_sub(sidebar_w + btn_size + btn_margin)
                    } else {
                        sz.width.saturating_sub(btn_size + btn_margin)
                    };
                    let _ = toggle_btn_wv.set_bounds(wry::Rect {
                        position: tao::dpi::PhysicalPosition::new(btn_x, btn_margin).into(),
                        size: tao::dpi::PhysicalSize::new(btn_size, btn_size).into(),
                    });
                    // Resize progress bar width
                    let _ = progress_wv.set_bounds(wry::Rect {
                        position: tao::dpi::PhysicalPosition::new(0u32, 0u32).into(),
                        size: tao::dpi::PhysicalSize::new(sz.width, 3u32).into(),
                    });
                    // Resize active tab (leave room for sidebar if open)
                    let tab_w = if sidebar_visible { sz.width.saturating_sub(sidebar_w) } else { sz.width };
                    let bounds = wry::Rect {
                        position: tao::dpi::PhysicalPosition::new(0u32, 0u32).into(),
                        size: tao::dpi::PhysicalSize::new(tab_w, sz.height).into(),
                    };
                    if let Some(wv) = tab_webviews.get(&active_wv_id) {
                        let _ = wv.set_bounds(bounds);
                    }
                }

                WindowEvent::ModifiersChanged(mods) => {
                    modifiers = *mods;
                }

                WindowEvent::Focused(focused) if window_id == browser_win_id => {
                    app_focused.store(*focused, Ordering::Relaxed);
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
                            KeyCode::KeyR if cmd => {
                                let _ = wv.evaluate_script("location.reload()");
                            }
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
