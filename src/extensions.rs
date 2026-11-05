//! Browser extensions through WebKit's `WKWebExtension` API (macOS 15.4+).
//!
//! Workspaces are isolated, and so are their extensions: every workspace gets
//! a [`Space`] — its own `WKWebExtensionController`, so its own extension
//! storage, holding only the extensions whose [`Scope`] includes it. To an
//! extension, its workspace is the one browser window there is. Incognito
//! tabs get no controller, as in Safari.
//!
//! WebKit asks about tabs and windows through the objects in
//! `extension_objc`, answered from each space's model, which [`sync`]
//! reconciles with the live `WorkspaceManager` once per event-loop pass — so
//! no open, close or switch path in main.rs has to remember to tell
//! extensions. Requests that change app state go back to main.rs as
//! [`Action`]s through the event loop, never re-entrantly from inside a
//! WebKit callback.
//!
//! Every WebKit object here is main-thread-only, and so is `STATE`. WebKit
//! calls straight back into `extension_objc`, which reads `STATE`, so nothing
//! may call into WebKit while `STATE` is borrowed: copy what you need out,
//! release, then call.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::ptr::{null_mut, NonNull};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, ProtocolObject, Sel};
use objc2::{msg_send, sel, MainThreadMarker, MainThreadOnly, Message};
use objc2_app_kit::{
	NSAlert, NSAlertFirstButtonReturn, NSBitmapImageFileType, NSBitmapImageRep, NSEvent, NSImage, NSMenu, NSMenuItem,
	NSModalResponse, NSModalResponseOK, NSOpenPanel, NSView,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{
	NSArray, NSCopying, NSDate, NSDictionary, NSError, NSLocalizedDescriptionKey, NSOperatingSystemVersion,
	NSProcessInfo, NSRectEdge, NSSet, NSString, NSURL, NSUUID,
};
use objc2_web_kit::{
	WKWebExtension, WKWebExtensionContext, WKWebExtensionContextPermissionStatus, WKWebExtensionController,
	WKWebExtensionControllerConfiguration, WKWebExtensionDataRecord, WKWebExtensionMatchPattern,
	WKWebExtensionPermission, WKWebExtensionTab, WKWebExtensionTabChangedProperties, WKWebExtensionTabConfiguration,
	WKWebExtensionWindow, WKWebExtensionWindowConfiguration, WKWebView, WKWebViewConfiguration, WKWebsiteDataStore,
};

use crate::extension_objc::{ExtDelegate, ExtTab, ExtWindow};
use crate::extension_store::{self, Record, Scope};
use crate::workspace::{Workspace, WorkspaceManager};
use wry::WebViewExtMacOS;

/// Toolbar buttons draw icons at 16 px; render twice that for Retina.
const TOOLBAR_ICON_PT: f64 = 32.0;
/// Settings rows draw icons at 24 px.
const SETTINGS_ICON_PT: f64 = 48.0;
const ALERT_ICON_PT: f64 = 64.0;

/// Work only main.rs can do — it owns the tabs, WebViews and chrome surfaces.
#[derive(Debug)]
pub enum Action {
	Activate(usize),
	Close(usize),
	Load(usize, String),
	/// Open a tab in `workspace`. When `reply` is set, report the new tab id
	/// with [`tab_opened`].
	OpenTab {
		url: Option<String>,
		workspace: String,
		active: bool,
		reply: Option<u64>,
	},
	FocusWindow(String),
	/// Actions on the toolbar changed: re-push [`toolbar_script`].
	Toolbar,
	/// The installed list changed: re-push the toolbar and [`settings_script`].
	Installed,
	Notify(String),
}

#[derive(Debug)]
pub enum Event {
	/// Chrome Web Store URL or id, or a path, typed into settings.
	Install(String),
	/// Choose a folder or archive with an open panel, then install it.
	PickAndInstall,
	SetEnabled(String, bool),
	SetScope(String, Scope),
	Remove(String),
	OpenOptions(String),
	/// Toolbar button clicked; `anchor` is its x, y, width, height in the
	/// address bar's CSS pixels — where the popup points.
	Click {
		id: String,
		anchor: [f64; 4],
	},
	Downloaded {
		id: String,
		result: Result<PathBuf, String>,
	},
	/// Raised from a WebKit callback; handed to main.rs on the next turn.
	App(Action),
}

static SENDER: OnceLock<Box<dyn Fn(Event) + Send + Sync>> = OnceLock::new();

fn send(event: Event) {
	if let Some(sender) = SENDER.get() {
		sender(event);
	}
}

fn send_app(action: Action) {
	send(Event::App(action));
}

thread_local! {
	static STATE: RefCell<Option<State>> = const { RefCell::new(None) };
}

struct State {
	/// Serves every controller; they hold it weakly.
	delegate: Retained<ExtDelegate>,
	installed: Vec<Installed>,
	/// Workspace id → its controller's storage identifier, created on first use.
	controller_ids: BTreeMap<String, [u8; 16]>,
	spaces: Vec<Space>,
	/// Id and name of the workspace on screen, as of the last [`sync`].
	active: Option<(String, String)>,
	replies: Vec<Reply>,
	next_reply: u64,
	/// Address bar WebView — popups point at its buttons.
	toolbar: Option<Retained<NSView>>,
	anchors: HashMap<String, CGRect>,
}

struct Installed {
	record: Record,
	/// Parsed manifest and resources; `None` until parsed, or if parsing failed.
	extension: Option<Retained<WKWebExtension>>,
	error: Option<String>,
}

impl State {
	fn find(&self, id: &str) -> Option<&Installed> {
		self.installed.iter().find(|i| i.record.id == id)
	}

	fn find_mut(&mut self, id: &str) -> Option<&mut Installed> {
		self.installed.iter_mut().find(|i| i.record.id == id)
	}

	fn space(&self, workspace: &str) -> Option<&Space> {
		self.spaces.iter().find(|s| s.workspace == workspace)
	}

	fn space_mut(&mut self, workspace: &str) -> Option<&mut Space> {
		self.spaces.iter_mut().find(|s| s.workspace == workspace)
	}

	fn space_of(&self, controller: &WKWebExtensionController) -> Option<&Space> {
		self.spaces.iter().find(|s| std::ptr::eq(&*s.controller, controller))
	}

	fn active_space(&self) -> Option<&Space> {
		self.space(&self.active.as_ref()?.0)
	}

	fn save(&self) {
		extension_store::save(&extension_store::Store::new(
			self.controller_ids.clone(),
			self.installed.iter().map(|i| i.record.clone()).collect(),
		));
	}
}

fn with<R>(f: impl FnOnce(&State) -> R) -> Option<R> {
	STATE.with(|s| s.borrow().as_ref().map(f))
}

fn with_mut<R>(f: impl FnOnce(&mut State) -> R) -> Option<R> {
	STATE.with(|s| s.borrow_mut().as_mut().map(f))
}

type TabReplyBlock = RcBlock<dyn Fn(*mut ProtocolObject<dyn WKWebExtensionTab>, *mut NSError)>;
type WindowReplyBlock = RcBlock<dyn Fn(*mut ProtocolObject<dyn WKWebExtensionWindow>, *mut NSError)>;
type GrantReply<T> = RcBlock<dyn Fn(NonNull<NSSet<T>>, *mut NSDate)>;

/// An extension waiting for the tab (or window) it asked for.
struct Reply {
	id: u64,
	tab: Option<usize>,
	to: ReplyTo,
}

enum ReplyTo {
	Tab(TabReplyBlock),
	Window(WindowReplyBlock),
}

// ── Spaces: one workspace's extensions, and its tabs as they last heard ─────

struct TabEntry {
	obj: Retained<ExtTab>,
	title: String,
	url: String,
	loading: bool,
	audio: bool,
	webview: Option<Retained<WKWebView>>,
}

struct Space {
	workspace: String,
	controller: Retained<WKWebExtensionController>,
	/// Extension id → its context, loaded into `controller`.
	contexts: HashMap<String, Retained<WKWebExtensionContext>>,
	/// Suffix of this space's `webkit-extension://` hosts. Copies of one
	/// extension in two controllers must not share a base URL — WebKit routes
	/// their messages by it. Derived from the controller id, so stable.
	host_tag: String,
	window: Retained<ExtWindow>,
	/// `didOpenWindow` sent.
	announced: bool,
	tabs: HashMap<usize, TabEntry>,
	/// Tab ids in tab-strip order.
	order: Vec<usize>,
	active: Option<usize>,
	focused: bool,
}

enum Change {
	OpenWindow(Retained<ExtWindow>),
	CloseTab(Retained<ExtTab>),
	OpenTab(Retained<ExtTab>),
	Props(Retained<ExtTab>, WKWebExtensionTabChangedProperties),
	Activate(Retained<ExtTab>, Option<Retained<ExtTab>>),
	Focus(Option<Retained<ExtWindow>>),
}

impl Space {
	fn active_tab(&self) -> Option<Retained<ExtTab>> {
		self.tabs.get(&self.active?).map(|t| t.obj.clone())
	}

	fn window_object(&self) -> Retained<ProtocolObject<dyn WKWebExtensionWindow>> {
		ProtocolObject::from_retained(self.window.clone())
	}

	/// Bring the model in line with the workspace's live tabs and list what
	/// changed, in the order WebKit must hear it.
	fn reconcile(&mut self, ws: &Workspace, focused: bool, mtm: MainThreadMarker) -> Vec<Change> {
		let tm = ws.tabs.lock().unwrap();
		let mut changes = Vec::new();
		if !self.announced {
			self.announced = true;
			changes.push(Change::OpenWindow(self.window.clone()));
		}
		let live: HashSet<usize> = tm.tabs().iter().filter(|t| !t.incognito).map(|t| t.id).collect();
		let gone: Vec<usize> = self.tabs.keys().filter(|id| !live.contains(id)).copied().collect();
		for id in gone {
			let entry = self.tabs.remove(&id).expect("id came from the map");
			changes.push(Change::CloseTab(entry.obj));
		}
		self.order.clear();
		for tab in tm.tabs().iter().filter(|t| !t.incognito) {
			self.order.push(tab.id);
			let webview = ws.webviews.get(&tab.id).map(|wv| Retained::into_super(wv.webview()));
			let loading = webview.as_ref().is_some_and(|wv| unsafe { wv.isLoading() });
			let Some(entry) = self.tabs.get_mut(&tab.id) else {
				let obj = ExtTab::new(mtm, tab.id);
				changes.push(Change::OpenTab(obj.clone()));
				self.tabs.insert(
					tab.id,
					TabEntry {
						obj,
						title: tab.title.clone(),
						url: tab.url.clone(),
						loading,
						audio: tab.is_playing_audio,
						webview,
					},
				);
				continue;
			};
			let mut changed = WKWebExtensionTabChangedProperties::None;
			if entry.title != tab.title {
				entry.title.clone_from(&tab.title);
				changed |= WKWebExtensionTabChangedProperties::Title;
			}
			if entry.url != tab.url {
				entry.url.clone_from(&tab.url);
				changed |= WKWebExtensionTabChangedProperties::URL;
			}
			if entry.loading != loading {
				entry.loading = loading;
				changed |= WKWebExtensionTabChangedProperties::Loading;
			}
			if entry.audio != tab.is_playing_audio {
				entry.audio = tab.is_playing_audio;
				changed |= WKWebExtensionTabChangedProperties::PlayingAudio;
			}
			entry.webview = webview;
			if !changed.is_empty() {
				changes.push(Change::Props(entry.obj.clone(), changed));
			}
		}
		let active = tm.active_id().filter(|id| self.tabs.contains_key(id));
		if active != self.active {
			if let Some(tab) = active.and_then(|id| self.tabs.get(&id)) {
				let previous = self.active.and_then(|id| self.tabs.get(&id)).map(|t| t.obj.clone());
				changes.push(Change::Activate(tab.obj.clone(), previous));
			}
			self.active = active;
		}
		if focused != self.focused {
			self.focused = focused;
			changes.push(Change::Focus(focused.then(|| self.window.clone())));
		}
		changes
	}
}

// ── Lifecycle ────────────────────────────────────────────────────────────────

pub fn supported() -> bool {
	NSProcessInfo::processInfo().isOperatingSystemAtLeastVersion(NSOperatingSystemVersion {
		majorVersion: 15,
		minorVersion: 4,
		patchVersion: 0,
	})
}

/// Load the installed list and start parsing it. `sender` delivers [`Event`]s
/// back to the event loop (from any thread). Must run on the main thread
/// before the first tab WebView is built.
pub fn init(sender: impl Fn(Event) + Send + Sync + 'static) {
	let _ = SENDER.set(Box::new(sender));
	if !supported() {
		tracing::info!("browser extensions need macOS 15.4 or later");
		return;
	}
	let store = match extension_store::load() {
		Ok(store) => store,
		Err(e) => {
			tracing::error!(error = %e, "extensions disabled: installed list unreadable");
			return;
		}
	};
	let mtm = MainThreadMarker::new().expect("extensions::init runs on the main thread");
	let records = store.extensions.clone();
	STATE.with(|s| {
		*s.borrow_mut() = Some(State {
			delegate: ExtDelegate::new(mtm),
			installed: store
				.extensions
				.into_iter()
				.map(|record| Installed {
					record,
					extension: None,
					error: None,
				})
				.collect(),
			controller_ids: store.controllers,
			spaces: Vec::new(),
			active: None,
			replies: Vec::new(),
			next_reply: 1,
			toolbar: None,
			anchors: HashMap::new(),
		})
	});
	for record in records {
		parse(&record.path.clone(), move |result| {
			with_mut(|s| {
				if let Some(installed) = s.find_mut(&record.id) {
					match result {
						Ok(extension) => installed.extension = Some(extension),
						Err(e) => {
							tracing::warn!(id = %record.id, path = %record.path.display(), error = %e, "extension unreadable");
							installed.error = Some(e);
						}
					}
				}
			});
			apply();
			send_app(Action::Installed);
		});
	}
}

/// The address bar WebView, which popups point at.
pub fn set_toolbar_view(webview_ptr: usize) {
	let view = unsafe { Retained::retain(webview_ptr as *mut NSView) };
	with_mut(|s| s.toolbar = view);
}

/// WebKit parses an extension asynchronously; `done` runs on the main thread.
fn parse(path: &Path, done: impl Fn(Result<Retained<WKWebExtension>, String>) + 'static) {
	let Some(mtm) = MainThreadMarker::new() else {
		return;
	};
	let url = NSURL::fileURLWithPath_isDirectory(&NSString::from_str(&path.to_string_lossy()), path.is_dir());
	let block = RcBlock::new(move |extension: *mut WKWebExtension, error: *mut NSError| {
		match unsafe { Retained::retain(extension) } {
			Some(extension) => done(Ok(extension)),
			None => done(Err(describe(error))),
		}
	});
	unsafe { WKWebExtension::extensionWithResourceBaseURL_completionHandler(&url, &block, mtm) };
}

fn new_controller(id: [u8; 16], mtm: MainThreadMarker) -> Retained<WKWebExtensionController> {
	unsafe {
		let config = WKWebExtensionControllerConfiguration::configurationWithIdentifier(&NSUUID::from_bytes(id), mtm);
		WKWebExtensionController::initWithConfiguration(WKWebExtensionController::alloc(mtm), &config)
	}
}

/// The controller of `workspace`, creating its space — and loading the
/// extensions that run there — on first use.
fn ensure_space(workspace: &str) -> Option<Retained<WKWebExtensionController>> {
	enum Found {
		Running(Retained<WKWebExtensionController>),
		New([u8; 16], Retained<ExtDelegate>),
	}
	let mtm = MainThreadMarker::new()?;
	let found = with_mut(|s| {
		if let Some(space) = s.space(workspace) {
			return Found::Running(space.controller.clone());
		}
		let id = match s.controller_ids.get(workspace) {
			Some(id) => *id,
			None => {
				let id = crate::workspace::random_bytes16();
				s.controller_ids.insert(workspace.to_string(), id);
				s.save();
				id
			}
		};
		Found::New(id, s.delegate.clone())
	})?;
	let (id, delegate) = match found {
		Found::Running(controller) => return Some(controller),
		Found::New(id, delegate) => (id, delegate),
	};
	let controller = new_controller(id, mtm);
	unsafe { controller.setDelegate(Some(ProtocolObject::from_ref(&*delegate))) };
	let window = ExtWindow::new(mtm, workspace.to_string());
	with_mut(|s| {
		s.spaces.push(Space {
			workspace: workspace.to_string(),
			controller: controller.clone(),
			host_tag: id[..8].iter().map(|b| format!("{b:02x}")).collect(),
			contexts: HashMap::new(),
			window,
			announced: false,
			tabs: HashMap::new(),
			order: Vec::new(),
			active: None,
			focused: false,
		})
	});
	apply();
	Some(controller)
}

/// Load and unload contexts so every space runs exactly the enabled, parsed
/// extensions whose scope includes it.
fn apply() {
	type Start = (
		String,
		Retained<WKWebExtensionController>,
		String,
		Retained<WKWebExtension>,
		Record,
	);
	type Stop = (
		String,
		Retained<WKWebExtensionController>,
		String,
		Retained<WKWebExtensionContext>,
	);
	let (stops, starts): (Vec<Stop>, Vec<Start>) = with(|s| {
		let mut stops = Vec::new();
		let mut starts = Vec::new();
		for space in &s.spaces {
			for installed in &s.installed {
				let record = &installed.record;
				let wanted = record.enabled && record.scope.includes(&space.workspace);
				match (wanted, space.contexts.get(&record.id), &installed.extension) {
					(true, None, Some(extension)) => starts.push((
						space.workspace.clone(),
						space.controller.clone(),
						space.host_tag.clone(),
						extension.clone(),
						record.clone(),
					)),
					(false, Some(context), _) => stops.push((
						space.workspace.clone(),
						space.controller.clone(),
						record.id.clone(),
						context.clone(),
					)),
					_ => {}
				}
			}
			for (id, context) in &space.contexts {
				if s.find(id).is_none() {
					stops.push((
						space.workspace.clone(),
						space.controller.clone(),
						id.clone(),
						context.clone(),
					));
				}
			}
		}
		(stops, starts)
	})
	.unwrap_or_default();
	for (workspace, controller, id, context) in stops {
		if let Err(e) = unsafe { controller.unloadExtensionContext_error(&context) } {
			tracing::warn!(%id, %workspace, error = %e.localizedDescription(), "failed to unload extension");
		}
		with_mut(|s| s.space_mut(&workspace).map(|sp| sp.contexts.remove(&id)));
	}
	for (workspace, controller, host_tag, extension, record) in starts {
		let result = load(&controller, &host_tag, &extension, &record);
		with_mut(|s| match result {
			Ok(context) => {
				tracing::info!(id = %record.id, %workspace, "extension loaded");
				if let Some(space) = s.space_mut(&workspace) {
					space.contexts.insert(record.id.clone(), context);
				}
				if let Some(installed) = s.find_mut(&record.id) {
					installed.error = None;
				}
			}
			Err(error) => {
				tracing::warn!(id = %record.id, %workspace, %error, "extension failed to load");
				if let Some(installed) = s.find_mut(&record.id) {
					installed.error = Some(error);
				}
			}
		});
	}
}

fn load(
	controller: &WKWebExtensionController,
	host_tag: &str,
	extension: &WKWebExtension,
	record: &Record,
) -> Result<Retained<WKWebExtensionContext>, String> {
	let context = unsafe { WKWebExtensionContext::contextForExtension(extension) };
	unsafe {
		context.setUniqueIdentifier(&NSString::from_str(&record.id));
		let base = format!("webkit-extension://{}-{host_tag}/", record.id);
		if let Some(base) = NSURL::URLWithString(&NSString::from_str(&base)) {
			context.setBaseURL(&base);
		}
		context.setInspectable(true);
		grant(&context, &record.permissions, &record.patterns);
		controller
			.loadExtensionContext_error(&context)
			.map_err(|e| e.localizedDescription().to_string())?;
	}
	Ok(context)
}

fn grant(context: &WKWebExtensionContext, permissions: &[String], patterns: &[String]) {
	unsafe {
		for permission in permissions {
			context.setPermissionStatus_forPermission(
				WKWebExtensionContextPermissionStatus::GrantedExplicitly,
				&NSString::from_str(permission),
			);
		}
		let sites: BTreeSet<String> = patterns.iter().map(|p| site_pattern(p)).collect();
		for pattern in &sites {
			match WKWebExtensionMatchPattern::initWithString_error(
				WKWebExtensionMatchPattern::alloc(context.mtm()),
				&NSString::from_str(pattern),
			) {
				Ok(pattern) => context.setPermissionStatus_forMatchPattern(
					WKWebExtensionContextPermissionStatus::GrantedExplicitly,
					&pattern,
				),
				Err(e) => {
					tracing::warn!(%pattern, error = %e.localizedDescription(), "skipping invalid match pattern")
				}
			}
		}
	}
}

/// A granted match pattern widened to its whole site, `scheme://host/*`.
/// Chrome ignores the path of host permissions, but WebKit registers an
/// extension's content scripts once per granted pattern whose host matches:
/// granting `github.com/*` and `github.com/login/*` side by side runs every
/// GitHub content script twice. One pattern per site keeps it to one.
fn site_pattern(pattern: &str) -> String {
	match pattern.split_once("://") {
		Some((scheme, rest)) => {
			let host = rest.split('/').next().unwrap_or(rest);
			format!("{scheme}://{host}/*")
		}
		// `<all_urls>` names no site.
		None => pattern.to_string(),
	}
}

/// Delete what extensions stored in a controller: everything, or one
/// extension's data when `context` names it.
fn erase(controller: Retained<WKWebExtensionController>, context: Option<Retained<WKWebExtensionContext>>) {
	let Some(mtm) = MainThreadMarker::new() else {
		return;
	};
	let types = unsafe { WKWebExtensionController::allExtensionDataTypes(mtm) };
	let remove = {
		let (controller, types) = (controller.clone(), types.clone());
		move |records: &NSArray<WKWebExtensionDataRecord>| unsafe {
			controller.removeDataOfTypes_fromDataRecords_completionHandler(&types, records, &RcBlock::new(|| {}));
		}
	};
	unsafe {
		match context {
			Some(context) => {
				let block = RcBlock::new(move |record: *mut WKWebExtensionDataRecord| {
					if let Some(record) = Retained::retain(record) {
						remove(&NSArray::from_retained_slice(&[record]));
					}
				});
				controller.fetchDataRecordOfTypes_forExtensionContext_completionHandler(&types, &context, &block);
			}
			None => {
				let block = RcBlock::new(move |records: NonNull<NSArray<WKWebExtensionDataRecord>>| {
					remove(records.as_ref());
				});
				controller.fetchDataRecordsOfTypes_completionHandler(&types, &block);
			}
		}
	}
}

/// Reconcile what extensions know with the live workspaces and tabs. Called
/// once per event-loop pass; cheap when nothing changed, nearly free with
/// nothing installed.
pub fn sync(wm: &WorkspaceManager) {
	let Some(mtm) = MainThreadMarker::new() else {
		return;
	};
	let Some((dropped, orphaned, changes)) = with_mut(|s| {
		let active = wm.active();
		if s.active
			.as_ref()
			.is_none_or(|(id, name)| *id != active.id || *name != active.name)
		{
			s.active = Some((active.id.clone(), active.name.clone()));
		}
		// Deleted workspaces take their extension data with them.
		let mut dropped = Vec::new();
		let mut orphaned = Vec::new();
		if s.controller_ids.keys().any(|id| wm.index_of(id).is_none()) {
			let (gone, kept): (Vec<Space>, Vec<Space>) =
				s.spaces.drain(..).partition(|sp| wm.index_of(&sp.workspace).is_none());
			s.spaces = kept;
			let gone_ids: Vec<String> = s
				.controller_ids
				.keys()
				.filter(|id| wm.index_of(id).is_none())
				.cloned()
				.collect();
			for id in &gone_ids {
				let controller_id = s.controller_ids.remove(id).expect("id came from the map");
				if !gone.iter().any(|sp| &sp.workspace == id) {
					orphaned.push(controller_id);
				}
			}
			for installed in &mut s.installed {
				if let Scope::Only(ids) = &mut installed.record.scope {
					ids.retain(|id| !gone_ids.contains(id));
				}
			}
			s.save();
			dropped = gone;
		}
		let mut changes = Vec::new();
		if !s.installed.is_empty() {
			for space in &mut s.spaces {
				let Some(i) = wm.index_of(&space.workspace) else {
					continue;
				};
				let space_changes = space.reconcile(wm.at(i), i == wm.active_index(), mtm);
				if !space_changes.is_empty() {
					changes.push((space.controller.clone(), space_changes));
				}
			}
		}
		Some((dropped, orphaned, changes))
	})
	.flatten() else {
		return;
	};
	for space in dropped {
		for (id, context) in &space.contexts {
			if let Err(e) = unsafe { space.controller.unloadExtensionContext_error(context) } {
				tracing::warn!(%id, error = %e.localizedDescription(), "failed to unload extension");
			}
		}
		tracing::info!(workspace = %space.workspace, "deleting the extension data of a removed workspace");
		erase(space.controller, None);
	}
	for id in orphaned {
		erase(new_controller(id, mtm), None);
	}
	if changes.is_empty() {
		return;
	}
	let mut toolbar = false;
	for (controller, space_changes) in changes {
		for change in space_changes {
			unsafe {
				match change {
					Change::OpenWindow(window) => controller.didOpenWindow(ProtocolObject::from_ref(&*window)),
					Change::CloseTab(tab) => {
						controller.didCloseTab_windowIsClosing(ProtocolObject::from_ref(&*tab), false)
					}
					Change::OpenTab(tab) => controller.didOpenTab(ProtocolObject::from_ref(&*tab)),
					Change::Props(tab, changed) => {
						controller.didChangeTabProperties_forTab(changed, ProtocolObject::from_ref(&*tab))
					}
					Change::Activate(tab, previous) => {
						toolbar = true;
						controller.didActivateTab_previousActiveTab(
							ProtocolObject::from_ref(&*tab),
							previous.as_deref().map(ProtocolObject::from_ref),
						)
					}
					Change::Focus(window) => {
						toolbar = true;
						controller.didFocusWindow(window.as_deref().map(ProtocolObject::from_ref))
					}
				}
			}
		}
	}
	resolve_replies();
	if toolbar {
		send_app(Action::Toolbar);
	}
}

/// main.rs opened the tab an extension asked for.
pub fn tab_opened(reply: u64, tab: usize) {
	with_mut(|s| {
		if let Some(r) = s.replies.iter_mut().find(|r| r.id == reply) {
			r.tab = Some(tab);
		}
	});
}

/// Answer extensions whose tabs have reached a space's model.
fn resolve_replies() {
	let ready: Vec<(ReplyTo, Retained<ExtTab>, Retained<ExtWindow>)> = with_mut(|s| {
		let mut ready = Vec::new();
		let mut i = 0;
		while i < s.replies.len() {
			let found = s.replies[i].tab.and_then(|id| {
				s.spaces
					.iter()
					.find_map(|sp| sp.tabs.get(&id).map(|t| (t.obj.clone(), sp.window.clone())))
			});
			match found {
				Some((tab, window)) => ready.push((s.replies.remove(i).to, tab, window)),
				None => i += 1,
			}
		}
		ready
	})
	.unwrap_or_default();
	for (to, tab, window) in ready {
		match to {
			ReplyTo::Tab(block) => {
				let tab = ProtocolObject::<dyn WKWebExtensionTab>::from_ref(&*tab);
				block.call((tab as *const _ as *mut _, null_mut()));
			}
			ReplyTo::Window(block) => {
				let window = ProtocolObject::<dyn WKWebExtensionWindow>::from_ref(&*window);
				block.call((window as *const _ as *mut _, null_mut()));
			}
		}
	}
}

fn add_reply(to: ReplyTo) -> Option<u64> {
	with_mut(|s| {
		let id = s.next_reply;
		s.next_reply += 1;
		s.replies.push(Reply { id, tab: None, to });
		id
	})
}

// ── Answers for extension_objc ───────────────────────────────────────────────

fn tab_in<R>(id: usize, f: impl FnOnce(&Space, &TabEntry) -> R) -> Option<R> {
	with(|s| {
		s.spaces
			.iter()
			.find_map(|sp| sp.tabs.get(&id).map(|t| (sp, t)))
			.map(|(sp, t)| f(sp, t))
	})
	.flatten()
}

pub fn tab_window(id: usize) -> Option<Retained<ProtocolObject<dyn WKWebExtensionWindow>>> {
	tab_in(id, |sp, _| sp.window_object())
}

pub fn tab_index(id: usize) -> Option<usize> {
	tab_in(id, |sp, _| sp.order.iter().position(|&t| t == id)).flatten()
}

pub fn tab_webview(id: usize) -> Option<Retained<WKWebView>> {
	tab_in(id, |_, t| t.webview.clone()).flatten()
}

pub fn tab_title(id: usize) -> Option<String> {
	tab_in(id, |_, t| t.title.clone())
}

pub fn tab_url(id: usize) -> Option<String> {
	tab_in(id, |_, t| t.url.clone()).filter(|u| !u.is_empty())
}

pub fn tab_loading(id: usize) -> bool {
	tab_in(id, |_, t| t.loading).unwrap_or(false)
}

pub fn tab_playing_audio(id: usize) -> bool {
	tab_in(id, |_, t| t.audio).unwrap_or(false)
}

pub fn tab_selected(id: usize) -> bool {
	tab_in(id, |sp, _| sp.active == Some(id)).unwrap_or(false)
}

pub fn activate_tab(id: usize) {
	send_app(Action::Activate(id));
}

pub fn close_tab(id: usize) {
	send_app(Action::Close(id));
}

pub fn load_tab(id: usize, url: String) {
	send_app(Action::Load(id, url));
}

pub fn duplicate_tab(id: usize, active: bool, reply: TabReplyBlock) {
	let Some(workspace) = tab_in(id, |sp, _| sp.workspace.clone()) else {
		let err = error("The tab is gone");
		reply.call((null_mut(), Retained::as_ptr(&err).cast_mut()));
		return;
	};
	send_app(Action::OpenTab {
		url: tab_url(id),
		workspace,
		active,
		reply: add_reply(ReplyTo::Tab(reply)),
	});
}

pub fn window_tabs(workspace: &str) -> Retained<NSArray<ProtocolObject<dyn WKWebExtensionTab>>> {
	let tabs: Vec<Retained<ProtocolObject<dyn WKWebExtensionTab>>> = with(|s| {
		s.space(workspace).map_or_else(Vec::new, |sp| {
			sp.order
				.iter()
				.filter_map(|t| sp.tabs.get(t))
				.map(|t| ProtocolObject::from_retained(t.obj.clone()))
				.collect()
		})
	})
	.unwrap_or_default();
	NSArray::from_retained_slice(&tabs)
}

pub fn window_active_tab(workspace: &str) -> Option<Retained<ProtocolObject<dyn WKWebExtensionTab>>> {
	with(|s| s.space(workspace)?.active_tab().map(ProtocolObject::from_retained)).flatten()
}

pub fn focus_window(workspace: &str) {
	send_app(Action::FocusWindow(workspace.to_string()));
}

/// An extension's only window is its own workspace.
pub fn open_windows(
	controller: &WKWebExtensionController,
) -> Retained<NSArray<ProtocolObject<dyn WKWebExtensionWindow>>> {
	let window = with(|s| s.space_of(controller).map(Space::window_object)).flatten();
	NSArray::from_retained_slice(window.as_slice())
}

/// Focused while its workspace is the one on screen.
pub fn focused_window(
	controller: &WKWebExtensionController,
) -> Option<Retained<ProtocolObject<dyn WKWebExtensionWindow>>> {
	with(|s| s.space_of(controller).filter(|sp| sp.focused).map(Space::window_object)).flatten()
}

fn workspace_of(controller: &WKWebExtensionController) -> Option<String> {
	with(|s| s.space_of(controller).map(|sp| sp.workspace.clone())).flatten()
}

fn url_string(url: Option<Retained<NSURL>>) -> Option<String> {
	url.and_then(|u| u.absoluteString()).map(|s| s.to_string())
}

pub fn open_tab(
	controller: &WKWebExtensionController,
	configuration: &WKWebExtensionTabConfiguration,
	reply: TabReplyBlock,
) {
	let Some(workspace) = workspace_of(controller) else {
		let err = error("The extension's workspace is gone");
		reply.call((null_mut(), Retained::as_ptr(&err).cast_mut()));
		return;
	};
	let (url, active) = unsafe { (url_string(configuration.url()), configuration.shouldBeActive()) };
	send_app(Action::OpenTab {
		url,
		workspace,
		active,
		reply: add_reply(ReplyTo::Tab(reply)),
	});
}

/// A workspace is the only window an extension has, so a "new window" opens
/// its tabs there and answers with that window.
pub fn open_window(
	controller: &WKWebExtensionController,
	configuration: &WKWebExtensionWindowConfiguration,
	reply: WindowReplyBlock,
) {
	let Some(workspace) = workspace_of(controller) else {
		let err = error("The extension's workspace is gone");
		reply.call((null_mut(), Retained::as_ptr(&err).cast_mut()));
		return;
	};
	let (urls, focus) = unsafe { (configuration.tabURLs(), configuration.shouldBeFocused()) };
	let urls: Vec<String> = urls.iter().filter_map(|u| url_string(Some(u))).collect();
	if urls.is_empty() {
		let window = with(|s| s.space(&workspace).map(Space::window_object)).flatten();
		reply.call((
			window.as_deref().map_or(null_mut(), |w| w as *const _ as *mut _),
			null_mut(),
		));
		return;
	}
	let mut reply = Some(ReplyTo::Window(reply));
	for (i, url) in urls.into_iter().enumerate() {
		send_app(Action::OpenTab {
			url: Some(url),
			workspace: workspace.clone(),
			active: focus && i == 0,
			reply: reply.take().and_then(add_reply),
		});
	}
}

fn display_name(extension: &WKWebExtension) -> String {
	unsafe { extension.displayName() }.map_or_else(|| "This extension".to_string(), |n| n.to_string())
}

fn alert(title: &str, detail: &str, accept: &str, icon: Option<&NSImage>) -> Option<Retained<NSAlert>> {
	let alert = NSAlert::new(MainThreadMarker::new()?);
	alert.setMessageText(&NSString::from_str(title));
	alert.setInformativeText(&NSString::from_str(detail));
	alert.addButtonWithTitle(&NSString::from_str(accept));
	alert.addButtonWithTitle(&NSString::from_str("Cancel"));
	if let Some(icon) = icon {
		unsafe { alert.setIcon(Some(icon)) };
	}
	Some(alert)
}

/// App-modal yes/no, for things the user just asked for (adding an extension).
fn confirm(title: &str, detail: &str, accept: &str, icon: Option<&NSImage>) -> bool {
	alert(title, detail, accept, icon).is_some_and(|a| a.runModal() == NSAlertFirstButtonReturn)
}

/// Ask whether `context` may have `items` (permission names or match
/// patterns) in a sheet on the browser window, then call `done` with the
/// answer. Never blocks: the request comes from an extension mid-flight, and
/// WebKit denies it on its own if unanswered for two minutes.
fn ask(context: &WKWebExtensionContext, verb: &str, items: &[String], done: impl FnOnce(bool) + 'static) {
	let window = with(|s| s.toolbar.as_ref().and_then(|v| v.window())).flatten();
	let extension = unsafe { context.webExtension() };
	let mut items = items.to_vec();
	items.sort();
	let alert = alert(
		&format!("Allow “{}” to {verb}?", display_name(&extension)),
		&items.join("\n"),
		"Allow",
		unsafe { extension.iconForSize(CGSize::new(ALERT_ICON_PT, ALERT_ICON_PT)) }.as_deref(),
	);
	let (Some(window), Some(alert)) = (window, alert) else {
		tracing::warn!("no browser window to ask for extension permissions in — denied");
		done(false);
		return;
	};
	let done = Cell::new(Some(done));
	let shown = alert.clone();
	let handler = RcBlock::new(move |response: NSModalResponse| {
		// Holds the alert until AppKit is done with it.
		let _ = &shown;
		if let Some(done) = done.take() {
			done(response == NSAlertFirstButtonReturn);
		}
	});
	alert.beginSheetModalForWindow_completionHandler(&window, Some(&handler));
}

/// Grant to every loaded copy of the extension — the grant is the user's,
/// not one workspace's — and persist it for the next launch.
fn grant_everywhere(context: &WKWebExtensionContext, permissions: &[String], patterns: &[String]) {
	let id = unsafe { context.uniqueIdentifier() }.to_string();
	let contexts: Vec<Retained<WKWebExtensionContext>> =
		with(|s| s.spaces.iter().filter_map(|sp| sp.contexts.get(&id).cloned()).collect()).unwrap_or_default();
	for context in &contexts {
		grant(context, permissions, patterns);
	}
	with_mut(|s| {
		if let Some(installed) = s.find_mut(&id) {
			let record = &mut installed.record;
			for p in permissions {
				if !record.permissions.contains(p) {
					record.permissions.push(p.clone());
				}
			}
			for p in patterns {
				if !record.patterns.contains(p) {
					record.patterns.push(p.clone());
				}
			}
		}
		s.save();
	});
}

/// `permissions.request()` for API permissions.
pub fn prompt_permissions(
	context: &WKWebExtensionContext,
	permissions: &NSSet<WKWebExtensionPermission>,
	reply: GrantReply<WKWebExtensionPermission>,
) {
	let names: Vec<String> = permissions.iter().map(|p| p.to_string()).collect();
	let (requester, permissions) = (context.retain(), permissions.copy());
	ask(context, "use more of the browser", &names.clone(), move |allowed| {
		let granted = if allowed {
			grant_everywhere(&requester, &names, &[]);
			permissions
		} else {
			NSSet::new()
		};
		reply.call((NonNull::from(&*granted), null_mut()));
	});
}

/// `permissions.request()` for sites.
pub fn prompt_patterns(
	context: &WKWebExtensionContext,
	patterns: &NSSet<WKWebExtensionMatchPattern>,
	reply: GrantReply<WKWebExtensionMatchPattern>,
) {
	let names: Vec<String> = patterns.iter().map(|p| unsafe { p.string() }.to_string()).collect();
	let (requester, patterns) = (context.retain(), patterns.copy());
	ask(context, "read and change these sites", &names.clone(), move |allowed| {
		let granted = if allowed {
			grant_everywhere(&requester, &[], &names);
			patterns
		} else {
			NSSet::new()
		};
		reply.call((NonNull::from(&*granted), null_mut()));
	});
}

pub fn toolbar_changed() {
	send_app(Action::Toolbar);
}

pub fn present_popup(
	action: &objc2_web_kit::WKWebExtensionAction,
	context: &WKWebExtensionContext,
) -> Result<(), String> {
	let popover = unsafe { action.popupPopover() }.ok_or("The extension has no popup")?;
	let id = unsafe { context.uniqueIdentifier() }.to_string();
	let (view, anchor) =
		with(|s| (s.toolbar.clone(), s.anchors.get(&id).copied())).ok_or("Extensions are not running")?;
	let view = view.ok_or("No toolbar to show the popup from")?;
	let bounds = view.bounds();
	// No click to point at (the extension opened its popup itself): point at
	// the right end of the bar, where the buttons are.
	let anchor = anchor.unwrap_or(CGRect::new(
		CGPoint::new(bounds.size.width - 40.0, 0.0),
		CGSize::new(1.0, bounds.size.height),
	));
	let (rect, edge) = if view.isFlipped() {
		(anchor, NSRectEdge::MaxY)
	} else {
		let mut flipped = anchor;
		flipped.origin.y = bounds.size.height - anchor.origin.y - anchor.size.height;
		(flipped, NSRectEdge::MinY)
	};
	tracing::debug!(extension = %id, "showing extension popup");
	popover.showRelativeToRect_ofView_preferredEdge(rect, &view, edge);
	Ok(())
}

pub fn error(message: &str) -> Retained<NSError> {
	let description = NSString::from_str(message);
	let info = NSDictionary::from_slices(
		&[unsafe { NSLocalizedDescriptionKey }],
		&[description.as_ref() as &AnyObject],
	);
	unsafe { NSError::errorWithDomain_code_userInfo(&NSString::from_str("octoweb.extensions"), 1, Some(&info)) }
}

fn describe(error: *mut NSError) -> String {
	unsafe { error.as_ref() }.map_or_else(|| "Unknown error".to_string(), |e| e.localizedDescription().to_string())
}

// ── Tab WebViews ─────────────────────────────────────────────────────────────

/// macOS that has identified data stores (`dataStoreForIdentifier`).
const IDENTIFIED_STORE_MIN_MACOS: isize = 14;

thread_local! {
	/// One process pool for every tab. WebKit handles each screen change (every
	/// auto-brightness or HDR headroom step) once per pool on the main thread,
	/// so a pool per tab froze the app for a second or more on each change.
	static TAB_PROCESS_POOL: Retained<AnyObject> = unsafe { msg_send![objc2::class!(WKProcessPool), new] };
}

/// Configuration for a tab's WKWebView in `workspace`, on the shared tab
/// process pool. Extension pages need their own context's configuration;
/// other pages get the workspace's extension controller (when extensions are
/// available) on the workspace's data store, or an in-memory store for
/// incognito tabs. wry leaves the store alone when handed a configuration.
pub fn tab_configuration(
	url: &str,
	workspace: &str,
	data_store_id: Option<[u8; 16]>,
	incognito: bool,
) -> Retained<WKWebViewConfiguration> {
	let mtm = MainThreadMarker::new().expect("tab WebViews are built on the main thread");
	let controller = if incognito { None } else { ensure_space(workspace) };
	unsafe {
		if let Some(controller) = controller.as_ref().filter(|_| url.starts_with("webkit-extension:")) {
			let context =
				NSURL::URLWithString(&NSString::from_str(url)).and_then(|u| controller.extensionContextForURL(&u));
			if let Some(config) = context.and_then(|c| c.webViewConfiguration()) {
				return config;
			}
		}
		let config = WKWebViewConfiguration::new(mtm);
		TAB_PROCESS_POOL.with(|pool| {
			let _: () = msg_send![&*config, setProcessPool: &**pool];
		});
		if let Some(controller) = &controller {
			config.setWebExtensionController(Some(controller));
		}
		let identified_stores =
			NSProcessInfo::processInfo().operatingSystemVersion().majorVersion >= IDENTIFIED_STORE_MIN_MACOS;
		let store = match data_store_id {
			_ if incognito => WKWebsiteDataStore::nonPersistentDataStore(mtm),
			Some(id) if identified_stores => WKWebsiteDataStore::dataStoreForIdentifier(&NSUUID::from_bytes(id), mtm),
			_ => WKWebsiteDataStore::defaultDataStore(mtm),
		};
		config.setWebsiteDataStore(&store);
		config
	}
}

fn active_contexts() -> Vec<Retained<WKWebExtensionContext>> {
	with(|s| s.active_space().map(|sp| sp.contexts.values().cloned().collect()))
		.flatten()
		.unwrap_or_default()
}

/// Run a key-down `NSEvent` past the keyboard commands of the extensions in
/// the workspace on screen; `true` when one handled it.
pub fn perform_command(event: *mut AnyObject) -> bool {
	let Some(event) = (unsafe { (event as *const NSEvent).as_ref() }) else {
		return false;
	};
	active_contexts()
		.iter()
		.any(|c| unsafe { c.performCommandForEvent(event) })
}

type WillOpenMenuImp = extern "C-unwind" fn(*mut AnyObject, Sel, *mut NSMenu, *mut NSEvent);

/// `-[NSView willOpenMenu:withEvent:]` as wry's WebView class had it.
static ORIGINAL_WILL_OPEN_MENU: AtomicUsize = AtomicUsize::new(0);

/// Add extension items to tab context menus. The hook goes on the WebView
/// class once; non-tab WebViews share the class and get nothing added.
pub fn hook_context_menu(webview_ptr: usize) {
	if ORIGINAL_WILL_OPEN_MENU.load(Ordering::SeqCst) != 0 || with(|_| ()).is_none() {
		return;
	}
	let sel = sel!(willOpenMenu:withEvent:);
	unsafe {
		let wv = webview_ptr as *mut AnyObject;
		let class: *const AnyClass = msg_send![&*wv, class];
		let previous = objc2::ffi::class_replaceMethod(
			class as *mut _,
			sel,
			std::mem::transmute::<WillOpenMenuImp, unsafe extern "C-unwind" fn()>(will_open_menu),
			c"v@:@@".as_ptr(),
		);
		// Not overridden on the class itself: chain to the inherited version.
		let original = previous.or_else(|| {
			let superclass = (*class).superclass()?;
			superclass.instance_method(sel).map(|m| m.implementation())
		});
		match original {
			Some(original) => ORIGINAL_WILL_OPEN_MENU.store(original as usize, Ordering::SeqCst),
			None => tracing::warn!("WebView class has no willOpenMenu:withEvent: — no extension menu items"),
		}
	}
}

extern "C-unwind" fn will_open_menu(this: *mut AnyObject, sel: Sel, menu: *mut NSMenu, event: *mut NSEvent) {
	let original = ORIGINAL_WILL_OPEN_MENU.load(Ordering::SeqCst);
	if original != 0 {
		let original: WillOpenMenuImp = unsafe { std::mem::transmute(original) };
		original(this, sel, menu, event);
	}
	let Some(menu) = (unsafe { menu.as_ref() }) else {
		return;
	};
	let webview = this as usize;
	let Some((tab, contexts)) = with(|s| {
		s.spaces.iter().find_map(|sp| {
			let tab = sp.tabs.values().find(|t| {
				t.webview
					.as_ref()
					.is_some_and(|w| Retained::as_ptr(w) as usize == webview)
			})?;
			Some((tab.obj.clone(), sp.contexts.values().cloned().collect::<Vec<_>>()))
		})
	})
	.flatten() else {
		return;
	};
	let Some(mtm) = MainThreadMarker::new() else {
		return;
	};
	let mut separated = false;
	for context in contexts {
		let items = unsafe { context.menuItemsForTab(ProtocolObject::from_ref(&*tab)) };
		for item in items.iter() {
			if !separated {
				menu.addItem(&NSMenuItem::separatorItem(mtm));
				separated = true;
			}
			menu.addItem(&item);
		}
	}
}

// ── Settings and toolbar ─────────────────────────────────────────────────────

pub fn handle(event: Event) -> Vec<Action> {
	match event {
		Event::App(action) => vec![action],
		Event::Install(input) => install(input.trim()),
		Event::PickAndInstall => pick().map_or_else(Vec::new, |path| install_path(&path)),
		Event::Downloaded { id, result } => match result {
			Ok(path) => {
				confirm_install(id, path);
				Vec::new()
			}
			Err(e) => vec![Action::Notify(e)],
		},
		Event::SetEnabled(id, enabled) => {
			with_mut(|s| {
				if let Some(installed) = s.find_mut(&id) {
					installed.record.enabled = enabled;
				}
				s.save();
			});
			apply();
			vec![Action::Installed]
		}
		Event::SetScope(id, scope) => {
			with_mut(|s| {
				if let Some(installed) = s.find_mut(&id) {
					installed.record.scope = scope;
				}
				s.save();
			});
			apply();
			vec![Action::Installed]
		}
		Event::Remove(id) => {
			remove(&id);
			vec![Action::Installed]
		}
		Event::OpenOptions(id) => options(&id).into_iter().collect(),
		Event::Click { id, anchor } => {
			click(&id, anchor);
			Vec::new()
		}
	}
}

fn install(input: &str) -> Vec<Action> {
	if input.is_empty() {
		return Vec::new();
	}
	if with(|_| ()).is_none() {
		return vec![Action::Notify("Extensions need macOS 15.4 or later".into())];
	}
	if let Some(id) = extension_store::store_id(input) {
		std::thread::spawn(move || {
			let result = extension_store::download_store_item(&id);
			send(Event::Downloaded { id, result });
		});
		return vec![Action::Notify("Downloading the extension…".into())];
	}
	let path = match input.strip_prefix("~/") {
		Some(rest) => dirs::home_dir().unwrap_or_default().join(rest),
		None => PathBuf::from(input),
	};
	install_path(&path)
}

fn pick() -> Option<PathBuf> {
	let mtm = MainThreadMarker::new()?;
	let panel = NSOpenPanel::openPanel(mtm);
	panel.setCanChooseDirectories(true);
	panel.setCanChooseFiles(true);
	panel.setAllowsMultipleSelection(false);
	panel.setMessage(Some(&NSString::from_str(
		"Choose an extension folder, or a .zip, .xpi or .crx file",
	)));
	if panel.runModal() != NSModalResponseOK {
		return None;
	}
	panel.URL().and_then(|u| u.path()).map(|p| PathBuf::from(p.to_string()))
}

fn install_path(path: &Path) -> Vec<Action> {
	if with(|_| ()).is_none() {
		return vec![Action::Notify("Extensions need macOS 15.4 or later".into())];
	}
	// Reinstalling a folder that is already installed updates it in place.
	let id = with(|s| {
		s.installed
			.iter()
			.find(|i| i.record.path == path)
			.map(|i| i.record.id.clone())
	})
	.flatten()
	.unwrap_or_else(extension_store::new_id);
	match extension_store::import(path, &id) {
		Ok(stored) => {
			confirm_install(id, stored);
			Vec::new()
		}
		Err(e) => vec![Action::Notify(e)],
	}
}

/// Parse what was imported, show what it asks for, and install on yes — in
/// the workspace on screen. An installed extension with the same id is
/// replaced, keeping its data and its workspaces.
fn confirm_install(id: String, path: PathBuf) {
	parse(&path.clone(), move |result| {
		let extension = match result {
			Ok(extension) => extension,
			Err(e) => {
				extension_store::discard(&path);
				send_app(Action::Notify(format!("Couldn't add the extension: {e}")));
				return;
			}
		};
		let name = display_name(&extension);
		let (permissions, patterns) = unsafe {
			(
				extension
					.requestedPermissions()
					.iter()
					.map(|p| p.to_string())
					.collect::<Vec<_>>(),
				extension
					.allRequestedMatchPatterns()
					.iter()
					.map(|p| p.string().to_string())
					.collect::<Vec<_>>(),
			)
		};
		let (previous, (active_id, active_name)) = with(|s| {
			(
				s.find(&id).map(|i| i.record.clone()),
				s.active
					.clone()
					.expect("sync records the workspace on screen before anything can be installed"),
			)
		})
		.expect("installs only start while extensions are running");
		let mut detail = Vec::new();
		if !permissions.is_empty() {
			let mut sorted = permissions.clone();
			sorted.sort();
			detail.push(format!("It can use: {}", sorted.join(", ")));
		}
		if !patterns.is_empty() {
			let mut sorted = patterns.clone();
			sorted.sort();
			detail.push(format!("It can read and change:\n{}", sorted.join("\n")));
		}
		if previous.is_none() {
			detail.push(format!(
				"It will run in the “{active_name}” workspace. Add it to more in Settings → Extensions."
			));
		}
		let icon = unsafe { extension.iconForSize(CGSize::new(ALERT_ICON_PT, ALERT_ICON_PT)) };
		if !confirm(
			&format!("Add “{name}”?"),
			&detail.join("\n\n"),
			"Add Extension",
			icon.as_deref(),
		) {
			extension_store::discard(&path);
			return;
		}
		let record = Record {
			id: id.clone(),
			path: path.clone(),
			enabled: true,
			permissions,
			patterns,
			scope: previous
				.as_ref()
				.map_or_else(|| Scope::Only(vec![active_id]), |p| p.scope.clone()),
		};
		// A reinstall replaces the old version: unload it everywhere first.
		with_mut(|s| s.installed.retain(|i| i.record.id != id));
		apply();
		if let Some(previous) = previous.filter(|p| p.path != path) {
			extension_store::discard(&previous.path);
		}
		with_mut(|s| {
			s.installed.push(Installed {
				record,
				extension: Some(extension.clone()),
				error: None,
			});
			s.save();
		});
		apply();
		send_app(Action::Installed);
		send_app(Action::Notify(format!("Added {name}")));
	});
}

/// Uninstall: unload it everywhere, delete its data from every workspace,
/// delete its archive, forget it.
fn remove(id: &str) {
	let Some(removed) = with_mut(|s| {
		let i = s.installed.iter().position(|i| i.record.id == id)?;
		let removed = s.installed.remove(i);
		s.save();
		Some(removed)
	})
	.flatten() else {
		return;
	};
	apply();
	if let (Some(extension), Some(mtm)) = (&removed.extension, MainThreadMarker::new()) {
		let (live, dormant): (Vec<_>, Vec<_>) = with(|s| {
			let live: Vec<_> = s.spaces.iter().map(|sp| sp.controller.clone()).collect();
			let dormant: Vec<_> = s
				.controller_ids
				.iter()
				.filter(|(ws, _)| s.space(ws).is_none())
				.map(|(_, id)| *id)
				.collect();
			(live, dormant)
		})
		.unwrap_or_default();
		let context = unsafe { WKWebExtensionContext::contextForExtension(extension) };
		unsafe { context.setUniqueIdentifier(&NSString::from_str(id)) };
		for controller in live
			.into_iter()
			.chain(dormant.into_iter().map(|cid| new_controller(cid, mtm)))
		{
			erase(controller, Some(context.clone()));
		}
	}
	extension_store::discard(&removed.record.path);
}

/// Open the options page in the workspace on screen — the extension must
/// run there for its page to load.
fn options(id: &str) -> Option<Action> {
	let (context, workspace) = with(|s| {
		let space = s.active_space()?;
		Some((space.contexts.get(id)?.clone(), space.workspace.clone()))
	})
	.flatten()?;
	Some(Action::OpenTab {
		url: Some(url_string(unsafe { context.optionsPageURL() })?),
		workspace,
		active: true,
		reply: None,
	})
}

fn click(id: &str, anchor: [f64; 4]) {
	let rect = CGRect::new(CGPoint::new(anchor[0], anchor[1]), CGSize::new(anchor[2], anchor[3]));
	let Some((context, tab)) = with_mut(|s| {
		s.anchors.insert(id.to_string(), rect);
		let space = s.active_space()?;
		Some((space.contexts.get(id)?.clone(), space.active_tab()))
	})
	.flatten() else {
		return;
	};
	unsafe { context.performActionForTab(tab.as_deref().map(ProtocolObject::from_ref)) };
}

fn png_data_uri(image: &NSImage) -> Option<String> {
	use base64::Engine;
	let tiff = image.TIFFRepresentation()?;
	let rep = NSBitmapImageRep::imageRepWithData(&tiff)?;
	let png = unsafe { rep.representationUsingType_properties(NSBitmapImageFileType::PNG, &NSDictionary::new()) }?;
	Some(format!(
		"data:image/png;base64,{}",
		base64::engine::general_purpose::STANDARD.encode(png.to_vec())
	))
}

/// Script pushing the toolbar buttons — one per extension running in the
/// workspace on screen, as its action stands for the focused tab — into the
/// address bar.
pub fn toolbar_script() -> String {
	let (contexts, tab) = with(|s| {
		let Some(space) = s.active_space() else {
			return (Vec::new(), None);
		};
		let contexts: Vec<(String, Retained<WKWebExtensionContext>)> = s
			.installed
			.iter()
			.filter_map(|i| Some((i.record.id.clone(), space.contexts.get(&i.record.id)?.clone())))
			.collect();
		(contexts, space.active_tab())
	})
	.unwrap_or_default();
	let buttons: Vec<serde_json::Value> = contexts
		.iter()
		.filter_map(|(id, context)| {
			let action = unsafe { context.actionForTab(tab.as_deref().map(ProtocolObject::from_ref)) }?;
			let icon = unsafe { action.iconForSize(CGSize::new(TOOLBAR_ICON_PT, TOOLBAR_ICON_PT)) };
			Some(serde_json::json!({
				"id": id,
				"label": unsafe { action.label() }.to_string(),
				"badge": unsafe { action.badgeText() }.to_string(),
				"enabled": unsafe { action.isEnabled() },
				"icon": icon.as_deref().and_then(png_data_uri),
			}))
		})
		.collect();
	tracing::debug!(buttons = ?buttons.iter().map(|b| (&b["label"], &b["badge"])).collect::<Vec<_>>(), "extension toolbar");
	format!(
		"window.__setExtensionButtons && window.__setExtensionButtons({})",
		serde_json::Value::Array(buttons)
	)
}

/// Script pushing the installed list, and the workspaces to scope it to,
/// into the settings panel.
pub fn settings_script(wm: &WorkspaceManager) -> String {
	struct Row {
		record: Record,
		extension: Option<Retained<WKWebExtension>>,
		runs_here: bool,
		error: Option<String>,
	}
	let active = &wm.active().id;
	let rows: Vec<Row> = with(|s| {
		s.installed
			.iter()
			.map(|i| Row {
				record: i.record.clone(),
				extension: i.extension.clone(),
				runs_here: s.space(active).is_some_and(|sp| sp.contexts.contains_key(&i.record.id)),
				error: i.error.clone(),
			})
			.collect()
	})
	.unwrap_or_default();
	let items: Vec<serde_json::Value> = rows
		.into_iter()
		.map(|row| {
			let ext = row.extension.as_deref();
			let text = |f: unsafe fn(&WKWebExtension) -> Option<Retained<NSString>>| {
				ext.and_then(|e| unsafe { f(e) })
					.map(|s| s.to_string())
					.unwrap_or_default()
			};
			serde_json::json!({
				"id": row.record.id,
				"name": ext.map_or_else(|| row.record.path.display().to_string(), display_name),
				"version": text(WKWebExtension::displayVersion),
				"description": text(WKWebExtension::displayDescription),
				"enabled": row.record.enabled,
				"scope": match &row.record.scope {
					Scope::All => serde_json::json!("all"),
					Scope::Only(ids) => serde_json::json!(ids),
				},
				"options": row.runs_here && ext.is_some_and(|e| unsafe { e.hasOptionsPage() }),
				"error": row.error,
				"path": row.record.path.display().to_string(),
				"icon": ext
					.and_then(|e| unsafe { e.iconForSize(CGSize::new(SETTINGS_ICON_PT, SETTINGS_ICON_PT)) })
					.as_deref()
					.and_then(png_data_uri),
			})
		})
		.collect();
	let workspaces: Vec<serde_json::Value> = wm
		.list()
		.iter()
		.map(|w| serde_json::json!({ "id": w.id, "name": w.name, "color": w.color }))
		.collect();
	format!(
		"window.__setExtensions && window.__setExtensions({})",
		serde_json::json!({
			"supported": with(|_| ()).is_some(),
			"items": items,
			"workspaces": workspaces,
			"active": active,
		})
	)
}

#[cfg(test)]
mod tests {
	use super::site_pattern;

	#[test]
	fn granted_patterns_widen_to_their_site() {
		assert_eq!(site_pattern("https://github.com/login/*"), "https://github.com/*");
		assert_eq!(site_pattern("https://github.com/*"), "https://github.com/*");
		assert_eq!(
			site_pattern("https://app.clockify.me/reports/detailed*"),
			"https://app.clockify.me/*"
		);
		assert_eq!(site_pattern("*://*.example.com/*"), "*://*.example.com/*");
		assert_eq!(site_pattern("<all_urls>"), "<all_urls>");
	}
}
