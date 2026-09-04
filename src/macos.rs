//! macOS-specific setup: environment init, dock icon, Edit menu, MRU list,
//! automatic termination disable.

/// Set the app's preferred language to English for WKWebView's Accept-Language header.
///
/// By default, WKWebView sends all system preferred languages in Accept-Language.
/// If the user has multiple languages (e.g., en, en-TH, ru-TH, th), sites may suggest
/// content in those languages. This forces English-only by setting NSUserDefaults
/// AppleLanguages before any WebView is created.
///
/// Must be called before any WKWebView instantiation.
pub fn set_english_locale() {
    // Set AppleLanguages to ["en"] so WKWebView sends "Accept-Language: en" only.
    // Without this, WKWebView uses the system's full language list (e.g., en, en-TH, ru-TH, th)
    // which causes sites to suggest content in those languages.
    use objc2::runtime::AnyObject;
    use objc2::{class, msg_send};
    use objc2_foundation::NSString;

    unsafe {
        let en: *mut AnyObject = msg_send![class!(NSString), stringWithUTF8String: c"en".as_ptr()];
        let arr: *mut AnyObject = msg_send![class!(NSArray), arrayWithObject: en];
        let defaults: *mut AnyObject = msg_send![class!(NSUserDefaults), standardUserDefaults];
        let key = NSString::from_str("AppleLanguages");
        let _: () = msg_send![defaults, setObject: arr, forKey: &*key];
    }
    tracing::debug!("set AppleLanguages to [\"en\"] for WKWebView Accept-Language header");
}

/// Disable macOS automatic termination.
///
/// macOS may automatically quit apps that have no visible windows or are
/// backgrounded for too long. For a browser with tabs, this is undesirable.
/// This function opts out of automatic termination both programmatically
/// and should be paired with NSSupportsAutomaticTermination=false in Info.plist.
pub fn disable_automatic_termination() {
    use objc2_foundation::{NSProcessInfo, NSString};
    NSProcessInfo::processInfo()
        .disableAutomaticTermination(&NSString::from_str("Browser with active tabs"));
    tracing::debug!("disabled automatic termination");
}

/// Import the user's full shell environment into this process.
///
/// macOS `.app` bundles launched from Finder/Dock get a sanitized environment:
/// minimal PATH (`/usr/bin:/bin:/usr/sbin:/sbin`), no user exports from
/// `~/.zshrc` / `~/.bashrc`. This means API keys, custom PATH entries
/// (Homebrew, Cargo, NVM, etc.), and other user config are all missing.
///
/// Fix: spawn the user's login shell (`$SHELL -l -c env`), capture its full
/// environment, and import every variable into our process. This gives child
/// processes (e.g. `octomind`) the same environment the user has in Terminal.
///
/// Skips a small set of shell-session-specific vars that don't apply to us.
pub fn init_env() {
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());

    let output = match std::process::Command::new(&shell)
        .args(["-l", "-i", "-c", "env"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
    {
        Ok(o) if o.status.success() => o,
        _ => {
            tracing::warn!(
                shell,
                "failed to capture user shell env — PATH may be incomplete"
            );
            return;
        }
    };

    // Vars that belong to the parent shell session, not to us.
    const SKIP: &[&str] = &[
        "SHLVL",
        "TERM",
        "TERM_PROGRAM",
        "TERM_PROGRAM_VERSION",
        "TERM_SESSION_ID",
        "TMPDIR",
        "PWD",
        "OLDPWD",
        "_",
        "GHOSTTY_RESOURCES_DIR",
        "GHOSTTY_BIN_DIR",
    ];

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut imported = 0usize;

    for line in stdout.lines() {
        // env output is KEY=VALUE (value may contain '=')
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if key.is_empty() || SKIP.contains(&key) {
            continue;
        }
        std::env::set_var(key, value);
        imported += 1;
    }

    tracing::debug!(shell, imported, "inherited user shell environment");
}

/// Suppress tao's NSTextInputClient path so it cannot generate a duplicate
/// `insertText:replacementRange:` call into the WKWebView during WebKit's
/// keyDown-resend flow.
///
/// # The bug this fixes
///
/// Editors that call `event.preventDefault()` only on `textInput` (and not on
/// `keydown`/`keypress`) — e.g. Lexical on x.com compose — leave WebKit's
/// `EventHandler::keyEvent` returning `false` (`eventWasHandled = false`,
/// see WebCore/page/EventHandler.cpp:4416). When that happens,
/// `WebViewImpl::doneWithKeyEvent` resends the keystroke via `[NSApp sendEvent:]`
/// (Source/WebKit/UIProcess/mac/WebViewImpl.mm:5489-5506).
///
/// The resent event walks the responder chain. WKWebView forwards via
/// `_web_superKeyDown:` → `[NSResponder keyDown:]` → `nextResponder`, landing
/// on tao's parent `NSView` (because we use `build_as_child` for every tab).
/// Tao's `keyDown:` (tao-0.35.0/src/platform_impl/macos/view.rs:695) calls
/// `interpretKeyEvents:` on itself. macOS's text-input system routes
/// `insertText:replacementRange:` through the active `NSTextInputContext`
/// whose client is the firstResponder — the WKWebView. WebKit then handles
/// that second `insertText:` outside the keyDown-collection scope:
/// `WebViewImpl::insertText` line 5715 takes the async branch →
/// `WebPage::insertTextAsync` (WebPageCocoa.mm:2065) →
/// `Editor::insertText(text, /*triggeringEvent=*/nullptr, ...)` →
/// fresh `textInput` + `beforeinput` + `input` dispatch + DOM mutation.
/// Result: the character inserts twice ("H" → "HH").
///
/// Safari's standalone WKWebView container has no NSTextInputClient-conforming
/// parent in the chain, so the resend has nowhere to land and never produces a
/// second `insertText:`. This same bug is tracked upstream as
/// <https://github.com/tauri-apps/tauri/issues/8705>.
///
/// # The fix
///
/// We install a no-op `interpretKeyEvents:` on the `TaoView` ObjC class. Tao's
/// `keyDown:` still runs end-to-end (so `WindowEvent::KeyboardInput` is still
/// queued — used for `Cmd+[` / `Cmd+]` history-nav), but the call to
/// `[self interpretKeyEvents:array]` becomes a no-op. The macOS text-input
/// system never gets invoked from tao, so the duplicate `insertText:` to the
/// WKWebView never happens.
///
/// Mirrors what `WryWebViewParent.keyDown:` does in wry's non-child path
/// (wry-0.55.0/src/wkwebview/class/wry_web_view_parent.rs:29-38) — that view's
/// `keyDown:` doesn't call `interpretKeyEvents:` either, which is why the bug
/// only reproduces in `build_as_child` mode.
///
/// Safe because:
/// - octoweb does not consume `WindowEvent::ReceivedImeText` anywhere — all
///   text input lives inside WKWebViews. Suppressing tao's IME path loses
///   nothing observable to the user.
/// - Tao's `WindowEvent::KeyboardInput` (used for `Cmd+[` / `Cmd+]` shortcuts
///   in the main event loop) is queued *after* the `interpretKeyEvents:` call
///   in tao's `keyDown:`, so it remains intact.
///
/// Must be called once at startup, after the first tao window is built (which
/// is when the `TaoView` ObjC class is registered). Idempotent.
pub fn install_taoview_keyboard_fix() {
    use objc2::ffi::{class_addMethod, objc_getClass};
    use objc2::runtime::{AnyClass, Sel};
    use objc2::sel;
    use std::ffi::c_void;

    // No-op replacement for `interpretKeyEvents:`. Signature must match the
    // ObjC selector exactly: void return; receiver, _cmd, then the NSArray*
    // argument. We don't read or release the argument — ObjC retains/release
    // semantics for method arguments are handled by the caller.
    extern "C-unwind" fn no_op(_self: *mut c_void, _sel: Sel, _events: *mut c_void) {}

    unsafe {
        let class_ptr = objc_getClass(c"TaoView".as_ptr());
        if class_ptr.is_null() {
            // Tao window hasn't been created yet (TaoView class isn't
            // registered). The caller is expected to invoke this after the
            // first window is built. Log so we'd notice if the order changes.
            tracing::warn!(
                "install_taoview_keyboard_fix: TaoView class not found; \
                 typing-bug fix not applied. Call after first window build."
            );
            return;
        }
        let class = class_ptr as *mut AnyClass;

        // ObjC type encoding: `v@:@` = void return, id (self), SEL (_cmd),
        // id (NSArray * — the events array).
        let types = c"v@:@";

        // `interpretKeyEvents:` is inherited from NSResponder; TaoView itself
        // doesn't define it. class_addMethod installs it as a direct override.
        // Same call pattern as dialog_patch.rs.
        let added = class_addMethod(
            class,
            sel!(interpretKeyEvents:),
            std::mem::transmute::<
                extern "C-unwind" fn(*mut c_void, Sel, *mut c_void),
                unsafe extern "C-unwind" fn(),
            >(no_op),
            types.as_ptr(),
        );

        // `class_addMethod` returns `objc2::runtime::Bool`, not a Rust bool.
        if added.into() {
            tracing::debug!(
                "TaoView interpretKeyEvents: override installed — suppresses \
                 duplicate insertText: that would otherwise double-insert \
                 characters in WKWebView editors (see fn docstring)"
            );
        } else {
            tracing::warn!(
                "install_taoview_keyboard_fix: class_addMethod returned false \
                 (already present?); typing-bug fix may not be active."
            );
        }
    }
}

/// Hand a URL with a custom scheme (e.g. `tg://`, `figma://`, `mailto:`) to
/// macOS so the registered app can handle it. Returns `true` if the OS accepted
/// the URL (an app was found and launched).
pub fn open_external_url(url: &str) -> bool {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSString, NSURL};

    let ns_str = NSString::from_str(url);
    let Some(ns_url) = NSURL::URLWithString(&ns_str) else {
        tracing::warn!(url, "open_external_url: invalid URL");
        return false;
    };
    let workspace = NSWorkspace::sharedWorkspace();
    let ok = workspace.openURL(&ns_url);
    tracing::debug!(url, ok, "open_external_url");
    ok
}

/// Set the macOS dock/app icon from the embedded PNG.
pub fn set_app_icon() {
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::{MainThreadMarker, NSData};

    const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");

    // SAFETY: called at startup on the main thread before the event loop starts.
    let mtm = unsafe { MainThreadMarker::new_unchecked() };
    unsafe {
        let data = NSData::with_bytes(ICON_PNG);
        if let Some(image) = NSImage::initWithData(mtm.alloc(), &data) {
            let app = NSApplication::sharedApplication(mtm);
            app.setApplicationIconImage(Some(&image));
        }
    }
}

/// Install symmetric observers for `NSApplicationDidResignActiveNotification`
/// and `NSApplicationDidBecomeActiveNotification` so that `chrome_ns_window`
/// (our borderless transparent sidebar window) plays nicely with app
/// activation transitions.
///
/// **Resign on deactivate** — `chrome_win` is a borderless transparent child
/// window. We explicitly call `makeKeyWindow` on it to route input into the
/// sidebar WebView. macOS does NOT reliably auto-resign key from borderless
/// child windows when the app deactivates (Cmd+Tab, click on another app's
/// window), so the chrome window can stay key — meaning typing in the other
/// app (e.g. pressing Enter in Slack) routes back to our WebView and pulls
/// octoweb back to the front. On deactivation we explicitly call
/// `[chrome_ns_window resignKeyWindow]` so AppKit hands key status to the
/// actual frontmost app's window.
///
/// **Restore on activate** — when our app reactivates after a *transient*
/// deactivation (notification banner, Spotlight, brief Cmd-Tab) and the
/// sidebar still owns input focus from the user's perspective
/// (`sidebar_owns_key` is true), we re-promote `chrome_win` to key window
/// and notify the event loop via `AppEvent::SidebarReFocus` so it can
/// re-focus the sidebar WKWebView and the textarea inside it. Without this,
/// the previous fix overshoots: any deactivation drops key permanently and
/// the user's typing focus silently disappears.
///
/// Both observers run on the main thread (queue: nil = posting thread; these
/// notifications are posted on the main thread by NSApplication).
///
/// `chrome_ns_window` must be a non-null NSWindow pointer; observers retain
/// it for the app's lifetime (we never uninstall them).
///
/// # Safety
/// Caller must pass a valid NSWindow pointer that lives for the duration of
/// the app (chrome_win is never dropped).
pub unsafe fn install_app_active_observers(
    chrome_ns_window: *mut std::ffi::c_void,
    sidebar_owns_key: std::sync::Arc<std::sync::atomic::AtomicBool>,
    proxy: tao::event_loop::EventLoopProxy<crate::AppEvent>,
) {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use objc2_foundation::NSString;
    use std::sync::atomic::Ordering;

    if chrome_ns_window.is_null() {
        return;
    }

    let center: *mut AnyObject = msg_send![objc2::class!(NSNotificationCenter), defaultCenter];
    if center.is_null() {
        tracing::warn!("NSNotificationCenter defaultCenter is null");
        return;
    }

    // Capture the pointer as usize so the block is `Send + 'static` safe.
    let win_addr = chrome_ns_window as usize;

    // ── DidResignActive: drop key from chrome_win so it doesn't trap input ──
    let resign_block = block2::RcBlock::new(move |_notif: *mut AnyObject| {
        let win = win_addr as *mut AnyObject;
        if win.is_null() {
            return;
        }
        let is_key: bool = unsafe { msg_send![win, isKeyWindow] };
        if is_key {
            let _: () = unsafe { msg_send![win, resignKeyWindow] };
            tracing::debug!("chrome_win resigned key on app deactivate");
        }
    });
    let resign_name = NSString::from_str("NSApplicationDidResignActiveNotification");
    let _resign_token: *mut AnyObject = msg_send![
        center,
        addObserverForName: &*resign_name,
        object: std::ptr::null::<AnyObject>(),
        queue: std::ptr::null::<AnyObject>(),
        usingBlock: &*resign_block,
    ];
    std::mem::forget(resign_block);

    // ── DidBecomeActive: if sidebar owned key before, restore it ───────────
    let activate_block = block2::RcBlock::new(move |_notif: *mut AnyObject| {
        if !sidebar_owns_key.load(Ordering::Relaxed) {
            return;
        }
        let win = win_addr as *mut AnyObject;
        if win.is_null() {
            return;
        }
        let is_key: bool = unsafe { msg_send![win, isKeyWindow] };
        if !is_key {
            let _: () = unsafe { msg_send![win, makeKeyWindow] };
            tracing::debug!("chrome_win re-promoted to key on app reactivate");
        }
        // Hand off to the event loop: wry WebView is !Send and must be
        // touched on the main event-loop thread.
        let _ = proxy.send_event(crate::AppEvent::SidebarReFocus);
    });
    let activate_name = NSString::from_str("NSApplicationDidBecomeActiveNotification");
    let _activate_token: *mut AnyObject = msg_send![
        center,
        addObserverForName: &*activate_name,
        object: std::ptr::null::<AnyObject>(),
        queue: std::ptr::null::<AnyObject>(),
        usingBlock: &*activate_block,
    ];
    std::mem::forget(activate_block);

    tracing::debug!("installed NSApplication active/resign observers");
}

/// Push tab_id to front of MRU list, removing any prior occurrence.
/// Keeps the list bounded to 64 entries (more than enough for any session).
pub fn mru_push(mru: &mut Vec<usize>, id: usize) {
    mru.retain(|&x| x != id);
    mru.insert(0, id);
    mru.truncate(64);
}

/// Install a minimal macOS menu bar so that standard Edit shortcuts
/// (Cmd+A, Cmd+C, Cmd+V, Cmd+X, Cmd+Z, Cmd+Shift+Z) are routed through
/// the NSResponder chain to WKWebView.
///
/// Without a menu bar, macOS has no `selectAll:` / `copy:` / `paste:` /
/// `cut:` / `undo:` / `redo:` actions registered, so those key equivalents
/// are silently swallowed and never reach the web view's text inputs.
pub fn setup_edit_menu() {
    use objc2::sel;
    use objc2_app_kit::{NSApplication, NSMenu, NSMenuItem};
    use objc2_foundation::{MainThreadMarker, NSString};

    // SAFETY: called on the main thread from the tao event loop.
    let mtm = unsafe { MainThreadMarker::new_unchecked() };

    unsafe {
        let app = NSApplication::sharedApplication(mtm);

        // Top-level menu bar.
        let menubar = NSMenu::new(mtm);

        // ── Edit menu ────────────────────────────────────────────────────
        let edit_menu = NSMenu::initWithTitle(mtm.alloc(), &NSString::from_str("Edit"));

        // Helper: add an item with title, action selector, and key equivalent.
        macro_rules! add_item {
            ($menu:expr, $title:expr, $sel:expr, $key:expr) => {{
                let item = NSMenuItem::initWithTitle_action_keyEquivalent(
                    mtm.alloc(),
                    &NSString::from_str($title),
                    Some($sel),
                    &NSString::from_str($key),
                );
                $menu.addItem(&item);
            }};
        }

        add_item!(edit_menu, "Select All", sel!(selectAll:), "a");
        edit_menu.addItem(&NSMenuItem::separatorItem(mtm));
        add_item!(edit_menu, "Cut", sel!(cut:), "x");
        add_item!(edit_menu, "Copy", sel!(copy:), "c");
        add_item!(edit_menu, "Paste", sel!(paste:), "v");
        edit_menu.addItem(&NSMenuItem::separatorItem(mtm));
        add_item!(edit_menu, "Undo", sel!(undo:), "z");
        // Redo: Cmd+Shift+Z — set modifier mask after creation.
        let redo = NSMenuItem::initWithTitle_action_keyEquivalent(
            mtm.alloc(),
            &NSString::from_str("Redo"),
            Some(sel!(redo:)),
            &NSString::from_str("Z"), // uppercase = Shift in key equivalent
        );
        edit_menu.addItem(&redo);

        // Wrap the Edit menu in a top-level container item.
        let edit_item = NSMenuItem::new(mtm);
        edit_item.setSubmenu(Some(&edit_menu));
        menubar.addItem(&edit_item);

        app.setMainMenu(Some(&menubar));
    }
}

/// Path to the bundled octomind tap.
///
/// Inside a `.app` it sits in `Contents/Resources/tap`; in a `cargo run` build
/// the binary is in `target/<profile>/`, so fall back to the repo's `tap/`.
fn bundled_tap_dir() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    // …/Octoweb.app/Contents/MacOS/octoweb → …/Contents/Resources/tap
    let bundled = exe.parent()?.parent()?.join("Resources").join("tap");
    if bundled.is_dir() {
        return Some(bundled);
    }
    // …/target/debug/octoweb → …/tap
    let dev = exe.parent()?.parent()?.parent()?.join("tap");
    dev.is_dir().then_some(dev)
}

/// Register the bundled tap with octomind so `octoweb:*` agents resolve.
///
/// Re-points every launch instead of registering once: the tap is a symlink
/// into the app bundle, so it goes stale the moment someone moves the app.
/// `octomind tap` refuses to re-add an existing name, hence the untap first.
/// Both calls are best-effort — a missing octomind just means no assistant,
/// which the sidebar already reports on its own.
pub fn register_octomind_tap(tap_name: &str) {
    let Some(dir) = bundled_tap_dir() else {
        tracing::warn!("bundled octomind tap not found — octoweb: agents will not resolve");
        return;
    };
    let run = |args: &[&str]| {
        std::process::Command::new("octomind")
            .args(args)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
    };
    let _ = run(&["untap", tap_name]);
    match run(&["tap", tap_name, &dir.to_string_lossy()]) {
        Ok(s) if s.success() => {
            tracing::info!(tap = tap_name, path = %dir.display(), "octomind tap registered")
        }
        Ok(s) => tracing::warn!(tap = tap_name, code = ?s.code(), "octomind tap failed"),
        Err(e) => tracing::warn!(error = %e, "octomind not on PATH — skipping tap registration"),
    }
}

/// Every agent tag octomind can resolve (`category:variant`) with its title,
/// sorted alphabetically. Feeds the sidebar's new-session tag autocomplete.
///
/// Scanned off disk because octomind has no "list agents" command. Resolution
/// is first-tap-wins, so `taps.toml` order is honoured and later duplicates are
/// dropped; the cloned-tap sweep at the end is what surfaces the always-active
/// built-in tap, which never appears in `taps.toml`.
pub fn octomind_agents() -> Vec<(String, String)> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let root = home.join(".local/share/octomind");
    let read = |dir: std::path::PathBuf| std::fs::read_dir(dir).into_iter().flatten().flatten();

    let mut tap_dirs: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(taps) = std::fs::read_to_string(root.join("taps.toml"))
        .unwrap_or_default()
        .parse::<toml::Value>()
    {
        for tap in taps
            .get("taps")
            .and_then(|t| t.as_array())
            .into_iter()
            .flatten()
        {
            if let Some(local) = tap.get("local_path").and_then(|p| p.as_str()) {
                tap_dirs.push(local.into());
            } else if let Some((user, repo)) = tap
                .get("name")
                .and_then(|n| n.as_str())
                .and_then(|n| n.split_once('/'))
            {
                tap_dirs.push(
                    root.join("taps")
                        .join(user)
                        .join(format!("octomind-{repo}")),
                );
            }
        }
    }
    for user in read(root.join("taps")) {
        tap_dirs.extend(read(user.path()).map(|repo| repo.path()));
    }

    let mut seen = std::collections::HashSet::new();
    let mut agents = Vec::new();
    for tap in tap_dirs {
        for category in read(tap.join("agents")) {
            let name = category.file_name().to_string_lossy().into_owned();
            for entry in read(category.path()) {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) != Some("toml") {
                    continue;
                }
                let Some(variant) = path.file_stem().and_then(|s| s.to_str()) else {
                    continue;
                };
                let tag = format!("{name}:{variant}");
                if !seen.insert(tag.clone()) {
                    continue;
                }
                // Agent TOMLs carry their human title in a header comment.
                let title = std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|s| {
                        s.lines()
                            .take(8)
                            .find_map(|l| l.strip_prefix("# Title:").map(|t| t.trim().to_string()))
                    })
                    .unwrap_or_default();
                agents.push((tag, title));
            }
        }
    }
    agents.sort();
    agents
}
