//! macOS-specific setup: dock icon, Edit menu, MRU list, PATH expansion.

/// Expand `PATH` so child processes (e.g. `octomind`) can be found when
/// launched as a macOS `.app` bundle. Finder gives apps a minimal PATH
/// (`/usr/bin:/bin:/usr/sbin:/sbin`), missing user-installed locations.
///
/// Reads `/etc/paths`, `/etc/paths.d/*`, and appends well-known user dirs.
/// Deduplicates and preserves existing entries.
pub fn expand_path() {
    let current = std::env::var("PATH").unwrap_or_default();
    let mut dirs: Vec<String> = current.split(':').map(str::to_string).collect();

    // Append only if not already present.
    let mut push = |dir: String| {
        if !dir.is_empty() && !dirs.contains(&dir) {
            dirs.push(dir);
        }
    };

    // /etc/paths — system-wide base paths
    if let Ok(contents) = std::fs::read_to_string("/etc/paths") {
        for line in contents.lines() {
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                push(trimmed.to_string());
            }
        }
    }

    // /etc/paths.d/* — per-package additions (Homebrew, Go, etc.)
    if let Ok(entries) = std::fs::read_dir("/etc/paths.d") {
        for entry in entries.flatten() {
            if let Ok(contents) = std::fs::read_to_string(entry.path()) {
                for line in contents.lines() {
                    let trimmed = line.trim();
                    if !trimmed.is_empty() {
                        push(trimmed.to_string());
                    }
                }
            }
        }
    }

    // Well-known user binary locations
    if let Some(home) = dirs::home_dir() {
        for subdir in [".cargo/bin", ".local/bin", "bin", "go/bin"] {
            push(home.join(subdir).to_string_lossy().into_owned());
        }
    }

    // Common system locations that Finder PATH may lack
    for dir in ["/opt/homebrew/bin", "/opt/homebrew/sbin", "/usr/local/bin"] {
        push(dir.to_string());
    }

    // Rebuild PATH, dropping empty entries
    let new_path: String = dirs
        .into_iter()
        .filter(|d| !d.is_empty())
        .collect::<Vec<_>>()
        .join(":");
    std::env::set_var("PATH", &new_path);

    tracing::debug!(path = %new_path, "expanded PATH for .app context");
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
