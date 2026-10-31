//! Site permissions — camera, microphone and location, remembered per origin.
//!
//! wry answers every `getUserMedia` with Grant, and WebKit's own geolocation
//! sheet forgets its answer, so both requests are routed here instead: a
//! stored decision answers at once; otherwise the request is held (its
//! decision block copied) until the user answers the address-bar prompt.
//! Live capture can be paused, resumed or stopped through
//! `set{Camera,Microphone}CaptureState:` (macOS 12+).
//!
//! WebKit calls UI delegates on the main thread and the event loop drains our
//! events there, so all state lives in thread-locals.

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap, VecDeque};
use std::ffi::{c_char, CStr};
use std::fs;
use std::path::PathBuf;
use std::sync::Once;

use block2::{Block, RcBlock};
use objc2::runtime::{AnyClass, AnyObject, Sel};
use objc2::{msg_send, sel};
use serde::{Deserialize, Serialize};

/// `WKPermissionDecision` — Prompt = 0, Grant = 1, Deny = 2.
const GRANT: isize = 1;
const DENY: isize = 2;

/// `WKMediaCaptureState` — None = 0, Active = 1, Muted = 2.
const CAPTURE_NONE: isize = 0;
const CAPTURE_ACTIVE: isize = 1;
const CAPTURE_MUTED: isize = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
	Camera,
	Microphone,
	Location,
}

const KINDS: [Kind; 3] = [Kind::Camera, Kind::Microphone, Kind::Location];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decision {
	Allow,
	Block,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CaptureState {
	Active,
	Muted,
	None,
}

/// What the address-bar permission popover asks for.
#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Action {
	/// Answer the oldest held request; `remember` stores it for the origin.
	Answer { allow: bool, remember: bool },
	/// Store a decision for the page's origin; `None` asks again next time.
	Set { kind: Kind, decision: Option<Decision> },
	/// Pause, resume or stop live camera / microphone capture.
	Capture { kind: Kind, state: CaptureState },
	/// Forget every decision for the page's origin.
	Reset,
}

/// Origin (`https://host[:port]`) → decision per kind.
type Store = BTreeMap<String, BTreeMap<Kind, Decision>>;

type Notify = Box<dyn Fn(usize)>;

struct Request {
	id: u64,
	origin: String,
	kinds: Vec<Kind>,
	handler: Option<RcBlock<dyn Fn(isize)>>,
}

impl Request {
	fn answer(&mut self, allow: bool) {
		if let Some(handler) = self.handler.take() {
			handler.call((if allow { GRANT } else { DENY },));
		}
	}
}

impl Drop for Request {
	// WebKit raises if a decision handler is released without being called.
	fn drop(&mut self) {
		self.answer(false);
	}
}

thread_local! {
	static STORE: RefCell<Option<Store>> = const { RefCell::new(None) };
	/// Held requests per WKWebView pointer, oldest first.
	static PENDING: RefCell<HashMap<usize, VecDeque<Request>>> = RefCell::new(HashMap::new());
	/// Kinds refused to the page now in each WebView: (origin, kinds).
	static REFUSED: RefCell<HashMap<usize, (String, Vec<Kind>)>> = RefCell::new(HashMap::new());
	static NOTIFY: RefCell<Option<Notify>> = const { RefCell::new(None) };
	static NEXT_ID: Cell<u64> = const { Cell::new(1) };
}

static MEDIA_HOOK: Once = Once::new();

/// `notify(wv_ptr)` runs whenever a WebView's permission state changes
/// outside the event loop (a page asked for something).
pub fn set_notify(notify: impl Fn(usize) + 'static) {
	NOTIFY.with_borrow_mut(|n| *n = Some(Box::new(notify)));
}

/// Route `wv_ptr`'s camera, microphone and location requests through here.
pub fn install(wv_ptr: usize) {
	type MediaImp = extern "C-unwind" fn(
		*mut AnyObject,
		Sel,
		*mut AnyObject,
		*mut AnyObject,
		*mut AnyObject,
		isize,
		*mut AnyObject,
	);
	type GeoImp =
		extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, *mut AnyObject, *mut AnyObject, *mut AnyObject);

	unsafe {
		let wv = wv_ptr as *mut AnyObject;
		let delegate: *mut AnyObject = msg_send![wv, UIDelegate];
		if delegate.is_null() {
			return;
		}
		let class: *const AnyClass = msg_send![delegate, class];
		// wry's delegate implements this one (always Grant), so WebKit
		// already knows the delegate answers it — swapping the IMP suffices.
		MEDIA_HOOK.call_once(|| {
			let previous = objc2::ffi::class_replaceMethod(
				class as *mut _,
				sel!(webView:requestMediaCapturePermissionForOrigin:initiatedByFrame:type:decisionHandler:),
				std::mem::transmute::<MediaImp, unsafe extern "C-unwind" fn()>(media_request),
				c"v@:@@@q@?".as_ptr(),
			);
			if previous.is_none() {
				tracing::warn!(
					"wry's media capture delegate method missing — camera/microphone requests may be refused"
				);
			}
		});
		// WebKit on macOS denies `navigator.geolocation` outright unless the UI
		// delegate has this private method, which wry's lacks.
		let added = objc2::ffi::class_addMethod(
			class as *mut _,
			sel!(_webView:requestGeolocationPermissionForOrigin:initiatedByFrame:decisionHandler:),
			std::mem::transmute::<GeoImp, unsafe extern "C-unwind" fn()>(geolocation_request),
			c"v@:@@@@?".as_ptr(),
		);
		// WebKit snapshots the delegate's methods when it is set, and wry set
		// this WebView's before the class gained the method.
		if added.as_bool() {
			let _: () = msg_send![wv, setUIDelegate: delegate];
		}
	}
}

/// Drop `wv_ptr`'s held requests (refusing them) before its WebView goes.
pub fn forget(wv_ptr: usize) {
	let dropped = PENDING.with_borrow_mut(|p| p.remove(&wv_ptr));
	drop(dropped);
	REFUSED.with_borrow_mut(|r| r.remove(&wv_ptr));
}

/// The page in `wv_ptr` has the camera or microphone on (or paused).
pub fn is_capturing(wv_ptr: usize) -> bool {
	let wv = wv_ptr as *mut AnyObject;
	unsafe { capture_state(wv, Kind::Camera) != CAPTURE_NONE || capture_state(wv, Kind::Microphone) != CAPTURE_NONE }
}

/// Turn the camera and microphone off in `wv_ptr`.
pub fn stop_capture(wv_ptr: usize) {
	set_capture(wv_ptr, Kind::Camera, CaptureState::None);
	set_capture(wv_ptr, Kind::Microphone, CaptureState::None);
}

/// Address-bar state for the page in `wv_ptr`, as JSON (`null` off http(s)):
/// `{origin, host, items: [{kind, decision, live, refused}], prompt: {id, kinds} | null}`.
pub fn state_json(wv_ptr: usize) -> String {
	let wv = wv_ptr as *mut AnyObject;
	let Some((origin, host)) = (unsafe { page_origin(wv) }) else {
		forget(wv_ptr);
		return "null".to_string();
	};
	// A request from a page the tab has since left can no longer be answered.
	let stale: VecDeque<Request> = PENDING.with_borrow_mut(|p| {
		let Some(queue) = p.get_mut(&wv_ptr) else {
			return VecDeque::new();
		};
		let (keep, stale) = std::mem::take(queue).into_iter().partition(|r| r.origin == origin);
		*queue = keep;
		stale
	});
	drop(stale);

	let rules = with_store(|s| s.get(&origin).cloned()).unwrap_or_default();
	let refused = REFUSED.with_borrow(|r| match r.get(&wv_ptr) {
		Some((o, kinds)) if *o == origin => kinds.clone(),
		_ => Vec::new(),
	});
	let items: Vec<serde_json::Value> = KINDS
		.iter()
		.map(|&kind| {
			let live = match unsafe { capture_state(wv, kind) } {
				CAPTURE_ACTIVE => Some("active"),
				CAPTURE_MUTED => Some("muted"),
				_ => None,
			};
			// macOS can withhold location from the whole app; only asked once the
			// site has it, as the CoreLocation query is not free.
			let system_denied = kind == Kind::Location
				&& rules.get(&kind) == Some(&Decision::Allow)
				&& crate::geolocation::system_denied();
			serde_json::json!({
				"kind": kind,
				"decision": rules.get(&kind),
				"live": live,
				"refused": refused.contains(&kind),
				"system_denied": system_denied,
			})
		})
		.collect();
	let prompt = PENDING.with_borrow(|p| {
		p.get(&wv_ptr)
			.and_then(|q| q.front())
			.map(|r| serde_json::json!({ "id": r.id, "kinds": r.kinds }))
	});
	serde_json::json!({ "origin": origin, "host": host, "items": items, "prompt": prompt }).to_string()
}

/// Apply a popover action to the page in `wv_ptr`.
pub fn apply(wv_ptr: usize, action: Action) {
	let Some((origin, _)) = (unsafe { page_origin(wv_ptr as *mut AnyObject) }) else {
		return;
	};
	match action {
		Action::Answer { allow, remember } => {
			let front = PENDING.with_borrow_mut(|p| p.get_mut(&wv_ptr).and_then(|q| q.pop_front()));
			if let Some(mut req) = front {
				tracing::debug!(origin = %req.origin, kinds = ?req.kinds, allow, remember, "site permission answered");
				if remember {
					let decision = if allow { Decision::Allow } else { Decision::Block };
					set_decisions(&req.origin, &req.kinds, Some(decision));
				}
				if !allow {
					note_refused(wv_ptr, &req.origin, &req.kinds);
				}
				req.answer(allow);
			}
		}
		Action::Set { kind, decision } => {
			set_decisions(&origin, &[kind], decision);
			match decision {
				Some(Decision::Block) => set_capture(wv_ptr, kind, CaptureState::None),
				_ => unnote_refused(wv_ptr, kind),
			}
		}
		Action::Capture { kind, state } => set_capture(wv_ptr, kind, state),
		Action::Reset => {
			with_store(|s| {
				if s.remove(&origin).is_some() {
					save(s);
				}
			});
			REFUSED.with_borrow_mut(|r| r.remove(&wv_ptr));
		}
	}
	settle(wv_ptr);
}

extern "C-unwind" fn media_request(
	_this: *mut AnyObject,
	_sel: Sel,
	webview: *mut AnyObject,
	_origin: *mut AnyObject,
	_frame: *mut AnyObject,
	capture_type: isize,
	handler: *mut AnyObject,
) {
	// `WKMediaCaptureType` — Camera = 0, Microphone = 1, CameraAndMicrophone = 2.
	let kinds = match capture_type {
		0 => vec![Kind::Camera],
		1 => vec![Kind::Microphone],
		_ => vec![Kind::Camera, Kind::Microphone],
	};
	request(webview, kinds, handler);
}

extern "C-unwind" fn geolocation_request(
	_this: *mut AnyObject,
	_sel: Sel,
	webview: *mut AnyObject,
	_origin: *mut AnyObject,
	_frame: *mut AnyObject,
	handler: *mut AnyObject,
) {
	request(webview, vec![Kind::Location], handler);
}

/// Decisions are keyed by the tab's top-level origin — the site the user sees
/// in the address bar. Embedded frames only get here when the page delegated
/// the feature to them (Permissions-Policy, enforced by WebKit).
fn request(webview: *mut AnyObject, kinds: Vec<Kind>, handler: *mut AnyObject) {
	let handler = unsafe { (*(handler as *const Block<dyn Fn(isize)>)).copy() };
	let wv_ptr = webview as usize;
	let Some((origin, _)) = (unsafe { page_origin(webview) }) else {
		handler.call((DENY,));
		return;
	};
	let id = NEXT_ID.get();
	NEXT_ID.set(id + 1);
	let mut req = Request {
		id,
		origin,
		kinds,
		handler: Some(handler),
	};
	let rules = with_store(|s| s.get(&req.origin).cloned());
	match decide(rules.as_ref(), &req.kinds) {
		Some(allow) => {
			tracing::debug!(origin = %req.origin, kinds = ?req.kinds, allow, "site permission answered from store");
			if !allow {
				note_refused(wv_ptr, &req.origin, &req.kinds);
			}
			req.answer(allow);
		}
		None => {
			tracing::debug!(origin = %req.origin, kinds = ?req.kinds, "site permission requested");
			PENDING.with_borrow_mut(|p| p.entry(wv_ptr).or_default().push_back(req));
		}
	}
	NOTIFY.with_borrow(|n| {
		if let Some(notify) = n {
			notify(wv_ptr);
		}
	});
}

/// Grant when every kind is allowed, refuse when any is blocked, else ask.
fn decide(rules: Option<&BTreeMap<Kind, Decision>>, kinds: &[Kind]) -> Option<bool> {
	let decisions: Vec<Option<Decision>> = kinds.iter().map(|k| rules.and_then(|r| r.get(k).copied())).collect();
	if decisions.contains(&Some(Decision::Block)) {
		Some(false)
	} else if decisions.iter().all(|d| *d == Some(Decision::Allow)) {
		Some(true)
	} else {
		None
	}
}

/// Answer the held requests a decision change now settles.
fn settle(wv_ptr: usize) {
	let settled: Vec<(Request, bool)> = PENDING.with_borrow_mut(|p| {
		let Some(queue) = p.get_mut(&wv_ptr) else {
			return Vec::new();
		};
		let mut settled = Vec::new();
		let mut keep = VecDeque::new();
		for req in queue.drain(..) {
			let rules = with_store(|s| s.get(&req.origin).cloned());
			match decide(rules.as_ref(), &req.kinds) {
				Some(allow) => settled.push((req, allow)),
				None => keep.push_back(req),
			}
		}
		*queue = keep;
		settled
	});
	for (mut req, allow) in settled {
		if !allow {
			note_refused(wv_ptr, &req.origin, &req.kinds);
		}
		req.answer(allow);
	}
}

fn note_refused(wv_ptr: usize, origin: &str, kinds: &[Kind]) {
	REFUSED.with_borrow_mut(|r| {
		let entry = r.entry(wv_ptr).or_insert_with(|| (origin.to_string(), Vec::new()));
		if entry.0 != origin {
			*entry = (origin.to_string(), Vec::new());
		}
		for kind in kinds {
			if !entry.1.contains(kind) {
				entry.1.push(*kind);
			}
		}
	});
}

fn unnote_refused(wv_ptr: usize, kind: Kind) {
	REFUSED.with_borrow_mut(|r| {
		if let Some((_, kinds)) = r.get_mut(&wv_ptr) {
			kinds.retain(|k| *k != kind);
		}
	});
}

fn set_decisions(origin: &str, kinds: &[Kind], decision: Option<Decision>) {
	with_store(|s| {
		let rules = s.entry(origin.to_string()).or_default();
		for &kind in kinds {
			match decision {
				Some(d) => {
					rules.insert(kind, d);
				}
				None => {
					rules.remove(&kind);
				}
			}
		}
		if rules.is_empty() {
			s.remove(origin);
		}
		save(s);
	});
}

fn set_capture(wv_ptr: usize, kind: Kind, state: CaptureState) {
	let wv = wv_ptr as *mut AnyObject;
	let value = match state {
		CaptureState::Active => CAPTURE_ACTIVE,
		CaptureState::Muted => CAPTURE_MUTED,
		CaptureState::None => CAPTURE_NONE,
	};
	let none: Option<&Block<dyn Fn()>> = None;
	unsafe {
		match kind {
			Kind::Camera if responds(wv, sel!(setCameraCaptureState:completionHandler:)) => {
				let _: () = msg_send![wv, setCameraCaptureState: value, completionHandler: none];
			}
			Kind::Microphone if responds(wv, sel!(setMicrophoneCaptureState:completionHandler:)) => {
				let _: () = msg_send![wv, setMicrophoneCaptureState: value, completionHandler: none];
			}
			_ => {}
		}
	}
}

unsafe fn capture_state(wv: *mut AnyObject, kind: Kind) -> isize {
	match kind {
		Kind::Camera if responds(wv, sel!(cameraCaptureState)) => msg_send![wv, cameraCaptureState],
		Kind::Microphone if responds(wv, sel!(microphoneCaptureState)) => msg_send![wv, microphoneCaptureState],
		_ => CAPTURE_NONE,
	}
}

unsafe fn responds(obj: *mut AnyObject, sel: Sel) -> bool {
	msg_send![obj, respondsToSelector: sel]
}

unsafe fn ns_string(obj: *mut AnyObject) -> Option<String> {
	if obj.is_null() {
		return None;
	}
	let bytes: *const c_char = msg_send![obj, UTF8String];
	if bytes.is_null() {
		return None;
	}
	Some(CStr::from_ptr(bytes).to_string_lossy().into_owned())
}

/// (origin, display host) of the page in `wv`; `None` off http(s).
unsafe fn page_origin(wv: *mut AnyObject) -> Option<(String, String)> {
	if wv.is_null() {
		return None;
	}
	let url: *mut AnyObject = msg_send![wv, URL];
	if url.is_null() {
		return None;
	}
	let scheme = ns_string(msg_send![url, scheme])?;
	let host = ns_string(msg_send![url, host])?;
	let port: *mut AnyObject = msg_send![url, port];
	let port = if port.is_null() {
		None
	} else {
		let p: i64 = msg_send![port, longLongValue];
		Some(p)
	};
	origin_of(&scheme, &host, port)
}

/// Same shape as JS `location.origin` — the default port is left out.
fn origin_of(scheme: &str, host: &str, port: Option<i64>) -> Option<(String, String)> {
	let scheme = scheme.to_ascii_lowercase();
	let default_port = match scheme.as_str() {
		"https" => 443,
		"http" => 80,
		_ => return None,
	};
	if host.is_empty() {
		return None;
	}
	let host = host.to_ascii_lowercase();
	let host = match port {
		Some(p) if p != default_port => format!("{host}:{p}"),
		_ => host,
	};
	Some((format!("{scheme}://{host}"), host))
}

fn with_store<R>(f: impl FnOnce(&mut Store) -> R) -> R {
	STORE.with_borrow_mut(|s| f(s.get_or_insert_with(load)))
}

fn path() -> PathBuf {
	crate::config::base_dir().join("octoweb").join("site_permissions.json")
}

fn load() -> Store {
	fs::read_to_string(path())
		.ok()
		.and_then(|s| serde_json::from_str(&s).ok())
		.unwrap_or_default()
}

fn save(store: &Store) {
	let path = path();
	if let Some(parent) = path.parent() {
		let _ = fs::create_dir_all(parent);
	}
	let tmp = path.with_extension("json.tmp");
	let result = serde_json::to_string_pretty(store)
		.map_err(std::io::Error::other)
		.and_then(|s| fs::write(&tmp, s))
		.and_then(|()| fs::rename(&tmp, &path));
	if let Err(e) = result {
		tracing::warn!(error = %e, "failed to save site permissions");
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	#[test]
	fn origin_matches_location_origin() {
		assert_eq!(
			origin_of("HTTPS", "Meet.Google.com", None),
			Some(("https://meet.google.com".into(), "meet.google.com".into()))
		);
		assert_eq!(origin_of("https", "a.test", Some(443)).unwrap().0, "https://a.test");
		assert_eq!(
			origin_of("http", "localhost", Some(3000)).unwrap().0,
			"http://localhost:3000"
		);
		assert_eq!(origin_of("file", "", None), None);
		assert_eq!(origin_of("about", "", None), None);
	}

	#[test]
	fn decide_needs_every_kind_allowed() {
		let mut rules = BTreeMap::new();
		assert_eq!(decide(None, &[Kind::Camera]), None);
		rules.insert(Kind::Camera, Decision::Allow);
		assert_eq!(decide(Some(&rules), &[Kind::Camera]), Some(true));
		assert_eq!(decide(Some(&rules), &[Kind::Camera, Kind::Microphone]), None);
		rules.insert(Kind::Microphone, Decision::Block);
		assert_eq!(decide(Some(&rules), &[Kind::Camera, Kind::Microphone]), Some(false));
	}

	#[test]
	fn store_round_trips_as_json() {
		let mut store = Store::new();
		store
			.entry("https://a.test".into())
			.or_default()
			.insert(Kind::Location, Decision::Block);
		let json = serde_json::to_string(&store).unwrap();
		assert_eq!(json, r#"{"https://a.test":{"location":"block"}}"#);
		assert_eq!(serde_json::from_str::<Store>(&json).unwrap(), store);
	}

	#[test]
	fn actions_parse_from_popover_json() {
		let a: Action = serde_json::from_str(r#"{"op":"set","kind":"camera","decision":null}"#).unwrap();
		assert!(matches!(
			a,
			Action::Set {
				kind: Kind::Camera,
				decision: None
			}
		));
		let a: Action = serde_json::from_str(r#"{"op":"capture","kind":"microphone","state":"muted"}"#).unwrap();
		assert!(matches!(
			a,
			Action::Capture {
				state: CaptureState::Muted,
				..
			}
		));
	}
}
