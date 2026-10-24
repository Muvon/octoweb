//! macOS-specific setup: environment init, dock icon, Edit menu, MRU list,
//! automatic termination disable.

/// Files the user picks in an open panel, for mail attachments. Empty when
/// they cancel.
pub fn pick_files() -> Vec<std::path::PathBuf> {
	use objc2::MainThreadMarker;
	use objc2_app_kit::{NSModalResponseOK, NSOpenPanel};
	use objc2_foundation::NSString;

	let Some(mtm) = MainThreadMarker::new() else {
		return Vec::new();
	};
	let panel = NSOpenPanel::openPanel(mtm);
	panel.setCanChooseDirectories(false);
	panel.setCanChooseFiles(true);
	panel.setAllowsMultipleSelection(true);
	panel.setMessage(Some(&NSString::from_str("Choose files to attach")));
	if panel.runModal() != NSModalResponseOK {
		return Vec::new();
	}
	panel
		.URLs()
		.iter()
		.filter_map(|u| u.path())
		.map(|p| std::path::PathBuf::from(p.to_string()))
		.collect()
}

/// The few moments that make a sound (spec SND-1); everything else is silent.
#[derive(Clone, Copy, Debug)]
pub enum Sound {
	Sent,
	Done,
	DraftReady,
	NewMail,
}

impl Sound {
	fn wav(self) -> &'static [u8] {
		match self {
			Sound::Sent => include_bytes!("../assets/sounds/mail-sent.wav"),
			Sound::Done => include_bytes!("../assets/sounds/done.wav"),
			Sound::DraftReady => include_bytes!("../assets/sounds/draft-ready.wav"),
			Sound::NewMail => include_bytes!("../assets/sounds/new-mail.wav"),
		}
	}
}

type Loaded = std::cell::OnceCell<Option<objc2::rc::Retained<objc2::runtime::AnyObject>>>;

thread_local! {
	// Kept for the app's life: a sound released mid-play goes quiet.
	static SOUNDS: [Loaded; 4] = const { [const { std::cell::OnceCell::new() }; 4] };
}

/// Play `sound`, unless macOS has "Play user interface sound effects" off.
/// Main thread only.
pub fn play(sound: Sound) {
	use objc2::runtime::AnyObject;
	use objc2::{class, msg_send};
	use objc2_foundation::NSData;

	if !ui_sounds_enabled() {
		return;
	}
	SOUNDS.with(|sounds| {
		let loaded = sounds[sound as usize].get_or_init(|| unsafe {
			let data = NSData::with_bytes(sound.wav());
			let player: *mut AnyObject = msg_send![class!(NSSound), alloc];
			let player: *mut AnyObject = msg_send![player, initWithData: &*data];
			objc2::rc::Retained::from_raw(player)
		});
		let Some(player) = loaded else {
			tracing::warn!(?sound, "sound did not load");
			return;
		};
		unsafe {
			// A repeat restarts it rather than overlapping.
			let _: bool = msg_send![&**player, stop];
			let _: bool = msg_send![&**player, play];
		}
	});
}

/// System Settings → Sound → "Play user interface sound effects". macOS
/// writes the key only once it is turned off.
fn ui_sounds_enabled() -> bool {
	use objc2::runtime::AnyObject;
	use objc2::{class, msg_send};
	use objc2_foundation::NSString;

	unsafe {
		let defaults: *mut AnyObject = msg_send![class!(NSUserDefaults), standardUserDefaults];
		let key = NSString::from_str("com.apple.sound.uiaudio.enabled");
		let value: *mut AnyObject = msg_send![&*defaults, objectForKey: &*key];
		value.is_null() || msg_send![&*value, boolValue]
	}
}

#[link(name = "PDFKit", kind = "framework")]
extern "C" {}

/// The text of a PDF, by PDFKit, for the mail drafting assistant. `None` when
/// it does not open or has no text layer (a scan). Any thread.
pub fn pdf_text(bytes: &[u8]) -> Option<String> {
	use objc2::runtime::AnyObject;
	use objc2::{class, msg_send};
	use objc2_foundation::{NSData, NSString};

	objc2::rc::autoreleasepool(|_| unsafe {
		let data = NSData::with_bytes(bytes);
		let doc: *mut AnyObject = msg_send![class!(PDFDocument), alloc];
		let doc: *mut AnyObject = msg_send![doc, initWithData: &*data];
		let doc = objc2::rc::Retained::from_raw(doc)?;
		let text: *mut NSString = msg_send![&*doc, string];
		let text = text.as_ref()?.to_string();
		(!text.trim().is_empty()).then_some(text)
	})
}

/// Mark a saved mail attachment the way Mail does, so Gatekeeper checks it
/// before it first opens and the Finder says where it came from. Unmarked, an
/// app or script from a message would open with no check at all.
pub fn quarantine_attachment(path: &std::path::Path) -> bool {
	use objc2::runtime::AnyObject;
	use objc2::{class, msg_send};
	use objc2_foundation::{NSString, NSURL};

	objc2::rc::autoreleasepool(|_| unsafe {
		let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
		let props: *mut AnyObject = msg_send![class!(NSMutableDictionary), dictionary];
		for (key, value) in [
			("LSQuarantineType", "LSQuarantineTypeEmailAttachment"),
			("LSQuarantineAgentName", "Octoweb"),
		] {
			let (key, value) = (NSString::from_str(key), NSString::from_str(value));
			let _: () = msg_send![props, setObject: &*value, forKey: &*key];
		}
		let key = NSString::from_str("NSURLQuarantinePropertiesKey");
		msg_send![
			&*url,
			setResourceValue: props,
			forKey: &*key,
			error: std::ptr::null_mut::<*mut AnyObject>()
		]
	})
}
/// Set the app's preferred language to English for WKWebView's Accept-Language header.
///
/// By default, WKWebView sends all system preferred languages in Accept-Language.
/// If the user has multiple languages (e.g., en, en-TH, ru-TH, th), sites may suggest
/// content in those languages. This forces English-only by setting NSUserDefaults
/// AppleLanguages before any WebView is created.
///
/// Must be called before any WKWebView instantiation.
pub fn set_english_locale() {
	// Set AppleLanguages to ["en-US"] so WKWebView sends "Accept-Language: en-US"
	// only. Without this, WKWebView uses the system's full language list (e.g.
	// en, en-TH, ru-TH, th) which causes sites to suggest content in those
	// languages. Region-tagged on purpose: a bare "en" makes `navigator.language`
	// and Accept-Language disagree with the macOS Safari user agent we send, and
	// fingerprinting bot checks score that inconsistency.
	use objc2::runtime::AnyObject;
	use objc2::{class, msg_send};
	use objc2_foundation::NSString;

	unsafe {
		let en: *mut AnyObject = msg_send![class!(NSString), stringWithUTF8String: c"en-US".as_ptr()];
		let arr: *mut AnyObject = msg_send![class!(NSArray), arrayWithObject: en];
		let defaults: *mut AnyObject = msg_send![class!(NSUserDefaults), standardUserDefaults];
		let key = NSString::from_str("AppleLanguages");
		let _: () = msg_send![defaults, setObject: arr, forKey: &*key];
	}
	tracing::debug!("set AppleLanguages to [\"en-US\"] for WKWebView Accept-Language header");
}

/// Disable macOS automatic termination.
///
/// macOS may automatically quit apps that have no visible windows or are
/// backgrounded for too long. For a browser with tabs, this is undesirable.
/// This function opts out of automatic termination both programmatically
/// and should be paired with NSSupportsAutomaticTermination=false in Info.plist.
pub fn disable_automatic_termination() {
	use objc2_foundation::{NSProcessInfo, NSString};
	NSProcessInfo::processInfo().disableAutomaticTermination(&NSString::from_str("Browser with active tabs"));
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
			tracing::warn!(shell, "failed to capture user shell env — PATH may be incomplete");
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
/// `keyDown:` still runs end-to-end, but the call to
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
/// - Nothing reads tao's keyboard events: every shortcut is dispatched from the
///   NSEvent local monitor in `main.rs` against the configurable keymap, so no
///   binding depends on tao's keyboard path.
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
			std::mem::transmute::<extern "C-unwind" fn(*mut c_void, Sel, *mut c_void), unsafe extern "C-unwind" fn()>(
				no_op,
			),
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

/// Bounce the Dock icon until the app is activated (a meeting is starting).
pub fn request_attention() {
	use objc2::runtime::AnyObject;
	use objc2::{class, msg_send};
	// SAFETY: NSApplication is set up before the event loop runs.
	unsafe {
		let app: *mut AnyObject = msg_send![class!(NSApplication), sharedApplication];
		// NSCriticalRequest
		let _: isize = msg_send![app, requestUserAttention: 0usize];
	}
}

#[link(name = "UserNotifications", kind = "framework")]
extern "C" {}

/// Whether this process runs from an app bundle: the notification center
/// throws for a bare binary (`cargo run`).
fn bundled() -> bool {
	use objc2::runtime::AnyObject;
	use objc2::{class, msg_send};
	// SAFETY: NSBundle mainBundle and its accessors have no preconditions.
	unsafe {
		let bundle: *mut AnyObject = msg_send![class!(NSBundle), mainBundle];
		if bundle.is_null() {
			return false;
		}
		let id: *mut objc2_foundation::NSString = msg_send![bundle, bundleIdentifier];
		let path: *mut objc2_foundation::NSString = msg_send![bundle, bundlePath];
		!id.is_null() && !path.is_null() && (*path).to_string().ends_with(".app")
	}
}

/// A macOS notification, for when the app is not in front. Asks for
/// permission the first time. Clicking it brings octoweb forward, where the
/// in-app alert with its Join button is waiting.
pub fn notify(id: &str, title: &str, body: &str) {
	use block2::RcBlock;
	use objc2::runtime::{AnyObject, Bool};
	use objc2::{class, msg_send};
	use objc2_foundation::NSString;

	static ASKED: std::sync::Once = std::sync::Once::new();
	if !bundled() {
		return;
	}
	// SAFETY: plain UserNotifications calls with retained arguments; the
	// content created with `new` is released after the request holds it.
	unsafe {
		let center: *mut AnyObject = msg_send![class!(UNUserNotificationCenter), currentNotificationCenter];
		if center.is_null() {
			return;
		}
		ASKED.call_once(|| {
			let done = RcBlock::new(|_granted: Bool, _error: *mut AnyObject| {});
			// UNAuthorizationOptionSound | UNAuthorizationOptionAlert
			let _: () = msg_send![center, requestAuthorizationWithOptions: 6usize, completionHandler: &*done];
		});
		let content: *mut AnyObject = msg_send![class!(UNMutableNotificationContent), new];
		let title = NSString::from_str(title);
		let body = NSString::from_str(body);
		let _: () = msg_send![content, setTitle: &*title];
		let _: () = msg_send![content, setBody: &*body];
		let sound: *mut AnyObject = msg_send![class!(UNNotificationSound), defaultSound];
		let _: () = msg_send![content, setSound: sound];
		let ident = NSString::from_str(id);
		let request: *mut AnyObject = msg_send![
			class!(UNNotificationRequest),
			requestWithIdentifier: &*ident,
			content: content,
			trigger: std::ptr::null_mut::<AnyObject>()
		];
		let added = RcBlock::new(|error: *mut AnyObject| {
			if !error.is_null() {
				tracing::debug!("meeting notification not shown");
			}
		});
		let _: () = msg_send![center, addNotificationRequest: request, withCompletionHandler: &*added];
		let _: () = msg_send![content, release];
	}
}

/// Ghostty's command-line binary, located through Launch Services so an install
/// outside /Applications is found too. `None` when Ghostty isn't installed.
pub fn ghostty_cli() -> Option<std::path::PathBuf> {
	use objc2_app_kit::NSWorkspace;
	use objc2_foundation::NSString;

	let bundle_id = NSString::from_str("com.mitchellh.ghostty");
	let app = NSWorkspace::sharedWorkspace().URLForApplicationWithBundleIdentifier(&bundle_id)?;
	let app = app.path()?;
	Some(std::path::PathBuf::from(app.to_string()).join("Contents/MacOS/ghostty"))
}

/// Lock one view light (`Some(false)`) or dark (`Some(true)`) whatever the app
/// appearance is, or `None` to inherit it again. WebKit derives
/// `prefers-color-scheme` from the view's effective appearance.
pub fn set_view_appearance(view: *mut objc2::runtime::AnyObject, dark: Option<bool>) {
	use objc2::runtime::AnyObject;
	use objc2::{class, msg_send};

	// SAFETY: `view` is a live NSView, messaged on the main thread.
	unsafe {
		let appearance: *mut AnyObject = match dark {
			Some(dark) => {
				let name = objc2_foundation::NSString::from_str(if dark {
					"NSAppearanceNameDarkAqua"
				} else {
					"NSAppearanceNameAqua"
				});
				msg_send![class!(NSAppearance), appearanceNamed: &*name]
			}
			None => std::ptr::null_mut(),
		};
		let _: () = msg_send![view, setAppearance: appearance];
	}
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

/// Keep the borderless sidebar window from retaining key status on deactivate,
/// and restore its text responder when macOS makes that window key again.
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
/// **Restore on becoming key** — app activation does not mean the sidebar owns
/// input. The user may have selected the page, or an external nonactivating
/// panel may own keyboard focus. Never promote chrome_win from an app-active
/// notification. Wait for its own DidBecomeKey notification, then let the event
/// loop recheck ownership before restoring the sidebar's text responder.
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

	// ── DidBecomeKey: restore text focus without taking key from a panel ──
	let activate_block = block2::RcBlock::new(move |_notif: *mut AnyObject| {
		if !sidebar_owns_key.load(Ordering::Relaxed) {
			return;
		}
		// Hand off to the event loop: wry WebView is !Send and must be
		// touched on the main event-loop thread.
		let _ = proxy.send_event(crate::AppEvent::SidebarReFocus);
	});
	let activate_name = NSString::from_str("NSWindowDidBecomeKeyNotification");
	let _activate_token: *mut AnyObject = msg_send![
		center,
		addObserverForName: &*activate_name,
		object: chrome_ns_window as *mut AnyObject,
		queue: std::ptr::null::<AnyObject>(),
		usingBlock: &*activate_block,
	];
	std::mem::forget(activate_block);

	tracing::debug!("installed app-resign and chrome-key observers");
}

/// Query actual keyboard ownership, including nonactivating external panels.
///
/// # Safety
/// `window` must be a live NSWindow pointer, accessed on the main thread.
pub unsafe fn window_is_key(window: *mut std::ffi::c_void) -> bool {
	use objc2::{msg_send, runtime::AnyObject};
	!window.is_null() && msg_send![window as *mut AnyObject, isKeyWindow]
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
///
/// When the updater runs (`updates::start`, called first), an app menu with
/// "Check for Updates…" comes before Edit. macOS shows the first menu under
/// the app's name, so without it the Edit items sit there.
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

		if let Some(update_item) = crate::updates::menu_item(mtm) {
			let app_menu = NSMenu::new(mtm);
			app_menu.addItem(&update_item);
			let app_item = NSMenuItem::new(mtm);
			app_item.setSubmenu(Some(&app_menu));
			menubar.addItem(&app_item);
		}

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
		for tap in taps.get("taps").and_then(|t| t.as_array()).into_iter().flatten() {
			if let Some(local) = tap.get("local_path").and_then(|p| p.as_str()) {
				tap_dirs.push(local.into());
			} else if let Some((user, repo)) = tap.get("name").and_then(|n| n.as_str()).and_then(|n| n.split_once('/'))
			{
				tap_dirs.push(root.join("taps").join(user).join(format!("octomind-{repo}")));
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

#[cfg(test)]
mod tests {
	use super::*;

	/// A one-page PDF showing `text` in Helvetica.
	fn pdf(text: &str) -> Vec<u8> {
		let stream = format!("BT /F1 12 Tf 72 720 Td ({text}) Tj ET");
		let objects = [
			"<< /Type /Catalog /Pages 2 0 R >>".to_string(),
			"<< /Type /Pages /Kids [3 0 R] /Count 1 >>".into(),
			"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] /Contents 4 0 R \
             /Resources << /Font << /F1 5 0 R >> >> >>"
				.into(),
			format!("<< /Length {} >>\nstream\n{stream}\nendstream", stream.len()),
			"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica >>".into(),
		];
		let mut out = b"%PDF-1.4\n".to_vec();
		let mut offsets = Vec::new();
		for (i, o) in objects.iter().enumerate() {
			offsets.push(out.len());
			out.extend(format!("{} 0 obj\n{o}\nendobj\n", i + 1).bytes());
		}
		let xref = out.len();
		out.extend(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).bytes());
		for off in offsets {
			out.extend(format!("{off:010} 00000 n \n").bytes());
		}
		out.extend(
			format!(
				"trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n",
				objects.len() + 1
			)
			.bytes(),
		);
		out
	}

	#[test]
	fn every_sound_decodes() {
		use objc2::runtime::AnyObject;
		use objc2::{class, msg_send};
		use objc2_foundation::NSData;

		for sound in [Sound::Sent, Sound::Done, Sound::DraftReady, Sound::NewMail] {
			let data = NSData::with_bytes(sound.wav());
			let seconds: f64 = unsafe {
				let player: *mut AnyObject = msg_send![class!(NSSound), alloc];
				let player: *mut AnyObject = msg_send![player, initWithData: &*data];
				let player = objc2::rc::Retained::from_raw(player).expect("NSSound decodes it");
				msg_send![&*player, duration]
			};
			assert!(seconds > 0.1 && seconds < 1.0, "{sound:?}: {seconds}s");
		}
	}

	#[test]
	fn pdf_text_reads_the_text_layer() {
		let text = pdf_text(&pdf("Total due 4200 HKD by 31 October")).unwrap();
		assert!(text.contains("Total due 4200 HKD by 31 October"), "{text:?}");
		assert_eq!(pdf_text(b"not a pdf"), None);
	}

	#[test]
	fn saved_attachments_are_quarantined() {
		let dir = std::env::temp_dir().join(format!("octoweb-quarantine-{}", std::process::id()));
		std::fs::create_dir_all(&dir).unwrap();
		let path = dir.join("invoice.command");
		std::fs::write(&path, b"#!/bin/sh\n").unwrap();
		assert!(quarantine_attachment(&path));
		let mark = std::process::Command::new("/usr/bin/xattr")
			.args(["-p", "com.apple.quarantine"])
			.arg(&path)
			.output()
			.unwrap();
		std::fs::remove_dir_all(&dir).unwrap();
		let mark = String::from_utf8_lossy(&mark.stdout);
		assert!(mark.contains(";Octoweb;"), "{mark:?}");
	}
}
