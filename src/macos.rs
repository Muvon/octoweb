//! macOS-specific setup: environment init, dock icon, Edit menu, MRU list.

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
        .args(["-l", "-c", "env"])
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
